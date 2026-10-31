//! Bringing games in from elsewhere: the official launcher's `.minecraft`,
//! HMCL and PCL (versions with or without isolation) and Prism/MultiMC
//! instances. Each becomes a world here with the same Minecraft version and
//! loader; its saves, mods, packs and settings are copied, never moved.
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::core::{
    error::{AppError, AppResult},
    instances::{Instance, InstanceStore},
    paths::AppPaths,
};

/// One game found in the chosen folder.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCandidate {
    /// What to pass back to import it: the version folder or the instance folder.
    pub source: String,
    pub name: String,
    pub version_id: String,
    pub version_type: String,
    pub loader: String,
    pub loader_version: Option<String>,
    /// Where its saves and mods are.
    pub game_dir: String,
    /// "official", "isolated" (HMCL/PCL version isolation) or "prism".
    pub kind: String,
    pub saves: usize,
    pub mods: usize,
}

/// Where other launchers keep games by default, when those folders exist.
pub fn known_dirs() -> Vec<(String, String)> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    let mut list: Vec<(&str, Option<PathBuf>)> = Vec::new();
    if cfg!(target_os = "macos") {
        let support = home.as_ref().map(|h| h.join("Library/Application Support"));
        list.push((crate::core::i18n::pick("官方启动器", "Official launcher"), support.as_ref().map(|s| s.join("minecraft"))));
        list.push(("Prism Launcher", support.as_ref().map(|s| s.join("PrismLauncher/instances"))));
        list.push(("HMCL", home.as_ref().map(|h| h.join("Library/Application Support/hmcl/.minecraft"))));
    } else if cfg!(windows) {
        list.push((crate::core::i18n::pick("官方启动器", "Official launcher"), appdata.as_ref().map(|a| a.join(".minecraft"))));
        list.push(("Prism Launcher", appdata.as_ref().map(|a| a.join("PrismLauncher/instances"))));
        list.push(("HMCL", appdata.as_ref().map(|a| a.join(".hmcl/.minecraft"))));
    } else {
        list.push((crate::core::i18n::pick("官方启动器", "Official launcher"), home.as_ref().map(|h| h.join(".minecraft"))));
        list.push(("Prism Launcher", home.as_ref().map(|h| h.join(".local/share/PrismLauncher/instances"))));
    }
    list.into_iter()
        .filter_map(|(label, path)| path.filter(|p| p.is_dir()).map(|p| (label.to_string(), p.to_string_lossy().into_owned())))
        .collect()
}

/// Everything importable in or right under `root`.
pub fn scan(root: &Path) -> Vec<ImportCandidate> {
    let mut found = Vec::new();
    scan_into(root, &mut found, 2);
    found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    found.dedup_by(|a, b| a.source == b.source);
    found
}

fn scan_into(dir: &Path, found: &mut Vec<ImportCandidate>, depth: u32) {
    if let Some(candidate) = prism(dir) {
        found.push(candidate);
        return;
    }
    if let Some(candidate) = version_folder(dir) {
        found.push(candidate);
        return;
    }
    let versions = dir.join("versions");
    if versions.is_dir() {
        for entry in read_dirs(&versions) {
            if let Some(candidate) = version_folder(&entry) {
                found.push(candidate);
            }
        }
        return;
    }
    if depth == 0 {
        return;
    }
    // A launcher folder holding `.minecraft`, or Prism's `instances` folder.
    for sub in [dir.join(".minecraft"), dir.join("minecraft")] {
        if sub.is_dir() {
            scan_into(&sub, found, depth - 1);
        }
    }
    for entry in read_dirs(dir) {
        if entry.join("instance.cfg").is_file() {
            scan_into(&entry, found, 0);
        }
    }
}

fn read_dirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .map(|entry| entry.path())
        .collect()
}

/// A Prism Launcher / MultiMC instance: `instance.cfg`, `mmc-pack.json`, and
/// the game in `.minecraft` (or `minecraft`).
fn prism(dir: &Path) -> Option<ImportCandidate> {
    let cfg = fs::read_to_string(dir.join("instance.cfg")).ok()?;
    let pack: serde_json::Value = serde_json::from_slice(&fs::read(dir.join("mmc-pack.json")).ok()?).ok()?;
    let name = cfg
        .lines()
        .find_map(|line| line.strip_prefix("name="))
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| file_name(dir));
    let mut version_id = None;
    let mut loader = ("vanilla".to_string(), None);
    for component in pack.get("components")?.as_array()? {
        let uid = component.get("uid").and_then(|v| v.as_str()).unwrap_or_default();
        let version = component.get("version").and_then(|v| v.as_str()).map(str::to_string);
        match uid {
            "net.minecraft" => version_id = version,
            "net.fabricmc.fabric-loader" => loader = ("fabric".into(), version),
            "org.quiltmc.quilt-loader" => loader = ("quilt".into(), version),
            "net.minecraftforge" => loader = ("forge".into(), version),
            "net.neoforged" => loader = ("neoforge".into(), version),
            _ => {}
        }
    }
    let game = [dir.join(".minecraft"), dir.join("minecraft")].into_iter().find(|p| p.is_dir()).unwrap_or_else(|| dir.join(".minecraft"));
    Some(candidate(dir, name, version_id?, "release".into(), loader, &game, "prism"))
}

/// `versions/<id>` with `<id>.json`. With version isolation (HMCL, PCL) the
/// game lives in the version folder; otherwise in the `.minecraft` above.
fn version_folder(dir: &Path) -> Option<ImportCandidate> {
    let id = file_name(dir);
    let json: serde_json::Value = serde_json::from_slice(&fs::read(dir.join(format!("{id}.json"))).ok()?).ok()?;
    let version_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("release").to_string();
    let loader = detect_loader(&json);
    let version_id = minecraft_version(&json, &id)?;
    let isolated = ["saves", "mods", "options.txt", "config", "resourcepacks"].iter().any(|name| dir.join(name).exists());
    let root = dir.parent().and_then(Path::parent).map(Path::to_path_buf);
    let (game, kind) = match (isolated, root) {
        (false, Some(root)) if root.join("versions").is_dir() => (root, "official"),
        _ => (dir.to_path_buf(), "isolated"),
    };
    Some(candidate(dir, id, version_id, version_type, loader, &game, kind))
}

fn candidate(source: &Path, name: String, version_id: String, version_type: String, loader: (String, Option<String>), game: &Path, kind: &str) -> ImportCandidate {
    let count = |sub: &str| fs::read_dir(game.join(sub)).map(|d| d.flatten().count()).unwrap_or(0);
    ImportCandidate {
        source: source.to_string_lossy().into_owned(),
        name,
        version_id,
        version_type,
        loader: loader.0,
        loader_version: loader.1,
        game_dir: game.to_string_lossy().into_owned(),
        kind: kind.to_string(),
        saves: count("saves"),
        mods: fs::read_dir(game.join("mods"))
            .map(|d| d.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".jar")).count())
            .unwrap_or(0),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The Minecraft version under a (possibly renamed, possibly merged)
/// version JSON: `inheritsFrom`, PCL's `clientVersion`, HMCL's `patches`,
/// the game jar it names, or the id itself when it looks like a version.
fn minecraft_version(json: &serde_json::Value, id: &str) -> Option<String> {
    let text = |key: &str| json.get(key).and_then(|v| v.as_str()).map(str::to_string);
    text("inheritsFrom")
        .or_else(|| text("clientVersion"))
        .or_else(|| {
            json.get("patches")?
                .as_array()?
                .iter()
                .find(|patch| patch.get("id").and_then(|v| v.as_str()) == Some("game"))?
                .get("version")?
                .as_str()
                .map(str::to_string)
        })
        .or_else(|| text("jar"))
        .or_else(|| {
            let looks = id.chars().next().is_some_and(|c| c.is_ascii_digit()) && id.chars().all(|c| c.is_ascii_alphanumeric() || ".-_ ".contains(c));
            looks.then(|| id.to_string())
        })
        .filter(|v| !v.is_empty() && v.len() <= 64)
}

/// The loader in a version JSON, from its libraries.
fn detect_loader(json: &serde_json::Value) -> (String, Option<String>) {
    let mc = minecraft_version(json, "").unwrap_or_default();
    let libraries = json.get("libraries").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    for library in &libraries {
        let Some(name) = library.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let mut parts = name.split(':');
        let (group, artifact, version) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or("").to_string());
        match (group, artifact) {
            ("net.fabricmc", "fabric-loader") => return ("fabric".into(), Some(version)),
            ("org.quiltmc", "quilt-loader") => return ("quilt".into(), Some(version)),
            ("net.neoforged", "neoforge") => return ("neoforge".into(), Some(version)),
            ("net.minecraftforge", "forge" | "fmlloader") => {
                // "1.20.1-47.2.0" → "47.2.0"; older ones keep their suffix.
                let own = version.strip_prefix(&format!("{mc}-")).map(str::to_string);
                return ("forge".into(), own.or(Some(version)).filter(|v| !v.is_empty()));
            }
            _ => {}
        }
    }
    // HMCL lists loaders as patches too.
    for patch in json.get("patches").and_then(|v| v.as_array()).into_iter().flatten() {
        let id = patch.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        let version = patch.get("version").and_then(|v| v.as_str()).map(str::to_string);
        match id {
            "fabric" => return ("fabric".into(), version),
            "quilt" => return ("quilt".into(), version),
            "forge" => return ("forge".into(), version),
            "neoforge" => return ("neoforge".into(), version),
            _ => {}
        }
    }
    ("vanilla".into(), None)
}

/// Launcher files and shared caches: never copied into the world.
const SKIP: &[&str] = &[
    "versions", "libraries", "assets", "runtime", "logs", "crash-reports", "natives", "bin", "debug",
    ".fabric", ".quilt", ".mixin.out", "webcache", "webcache2", "PCL", ".hmcl", "hmclversion.cfg",
    "launcher_profiles.json", "launcher_accounts.json", "launcher_accounts_microsoft_store.json",
    "launcher_settings.json", "launcher_log.txt", "launcher_ui_state.json", "launcher_product_state.json",
    "launcher_cef_log.txt", "launcher_msa_credentials.bin", "treatment_tags.json", "usercache.json",
    "usernamecache.json", "instance.cfg", "mmc-pack.json",
];

/// Creates the world and copies the game in. Returns the new world.
pub fn import(paths: &AppPaths, store: &InstanceStore, source: &Path) -> AppResult<Instance> {
    let found = prism(source)
        .or_else(|| version_folder(source))
        .ok_or_else(|| AppError::new("import_game", crate::core::i18n::pick("这里没有可导入的游戏版本。", "No importable game version here.")))?;
    let name: String = found.name.chars().take(40).collect();
    let instance = store.create(&name, &found.version_id, &found.version_type)?;
    let result = (|| -> AppResult<Instance> {
        store.configure(&instance.id, &name, None, None, &found.loader)?;
        // A loader version the launcher cannot read is chosen again at install.
        let pinned = found.loader_version.clone().filter(|v| crate::core::fabric::valid_version(v));
        store.configure_loader_version(&instance.id, pinned)?;
        let game = PathBuf::from(&found.game_dir);
        let own_id = file_name(source);
        copy_game(&game, &paths.instance_game_dir(&instance.id), &own_id)?;
        store.get(&instance.id)
    })();
    if result.is_err() {
        // Leave nothing half made behind.
        let _ = store.remove(&instance.id);
        let _ = fs::remove_dir_all(paths.root.join("recycle").join(&instance.id));
    }
    result
}

/// Copies a game folder, skipping launcher files and the version's own
/// json, jar and natives. Symlinks are skipped too.
fn copy_game(from: &Path, to: &Path, version_id: &str) -> AppResult<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let own = name == format!("{version_id}.json") || name == format!("{version_id}.jar") || name.ends_with("-natives") || name.starts_with("natives-");
        if SKIP.contains(&name.as_str()) || own {
            continue;
        }
        let kind = entry.file_type()?;
        let target = to.join(&name);
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    fs::create_dir_all(to.join("mods"))?;
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> AppResult<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn finds_official_isolated_and_prism_games() {
        let dir = tempfile::tempdir().unwrap();
        let mc = dir.path().join(".minecraft");
        // Official: vanilla 1.20.1, saves in .minecraft.
        write(&mc.join("versions/1.20.1/1.20.1.json"), r#"{"id":"1.20.1","type":"release","libraries":[]}"#);
        write(&mc.join("saves/World/level.dat"), "x");
        // HMCL/PCL: an isolated Fabric version renamed by the player.
        write(
            &mc.join("versions/我的生存/我的生存.json"),
            r#"{"id":"我的生存","inheritsFrom":"1.20.1","libraries":[{"name":"net.fabricmc:fabric-loader:0.15.11"}]}"#,
        );
        write(&mc.join("versions/我的生存/mods/sodium.jar"), "jar");
        write(&mc.join("versions/我的生存/options.txt"), "lang:zh_cn");
        // A merged Forge version (PCL writes clientVersion).
        write(
            &mc.join("versions/1.20.1-forge/1.20.1-forge.json"),
            r#"{"id":"1.20.1-forge","clientVersion":"1.20.1","libraries":[{"name":"net.minecraftforge:fmlloader:1.20.1-47.2.0"}]}"#,
        );
        let found = scan(dir.path());
        assert_eq!(found.len(), 3, "{found:#?}");
        let fabric = found.iter().find(|c| c.name == "我的生存").unwrap();
        assert_eq!((fabric.loader.as_str(), fabric.loader_version.as_deref(), fabric.kind.as_str()), ("fabric", Some("0.15.11"), "isolated"));
        assert_eq!(fabric.mods, 1);
        let forge = found.iter().find(|c| c.name == "1.20.1-forge").unwrap();
        assert_eq!((forge.version_id.as_str(), forge.loader_version.as_deref()), ("1.20.1", Some("47.2.0")));
        let vanilla = found.iter().find(|c| c.name == "1.20.1").unwrap();
        assert_eq!((vanilla.kind.as_str(), vanilla.saves), ("official", 1));

        let prism_dir = dir.path().join("instances/Create");
        write(&prism_dir.join("instance.cfg"), "InstanceType=OneSix\nname=机械动力\n");
        write(
            &prism_dir.join("mmc-pack.json"),
            r#"{"components":[{"uid":"net.minecraft","version":"1.20.1"},{"uid":"net.minecraftforge","version":"47.2.0"}]}"#,
        );
        write(&prism_dir.join(".minecraft/mods/create.jar"), "jar");
        let prism = scan(&dir.path().join("instances"));
        assert_eq!(prism.len(), 1);
        assert_eq!((prism[0].name.as_str(), prism[0].loader.as_str(), prism[0].mods), ("机械动力", "forge", 1));
    }

    #[test]
    fn importing_copies_the_game_but_not_launcher_files() {
        let dir = tempfile::tempdir().unwrap();
        let mc = dir.path().join(".minecraft");
        let version = mc.join("versions/生存");
        write(&version.join("生存.json"), r#"{"id":"生存","inheritsFrom":"1.21.1","libraries":[{"name":"net.fabricmc:fabric-loader:0.16.0"}]}"#);
        write(&version.join("生存.jar"), "jar");
        write(&version.join("saves/Home/level.dat"), "x");
        write(&version.join("mods/a.jar"), "jar");
        write(&version.join("PCL/Setup.ini"), "x");
        write(&version.join("生存-natives/lwjgl.dll"), "x");

        let root = dir.path().join("launcher");
        let paths = AppPaths::from_root(&root).unwrap();
        let store = InstanceStore::load(paths.clone()).unwrap();
        let instance = import(&paths, &store, &version).unwrap();
        assert_eq!((instance.version_id.as_str(), instance.loader.as_str(), instance.loader_version.as_deref()), ("1.21.1", "fabric", Some("0.16.0")));
        let game = paths.instance_game_dir(&instance.id);
        assert!(game.join("saves/Home/level.dat").is_file());
        assert!(game.join("mods/a.jar").is_file());
        for skipped in ["PCL", "生存.json", "生存.jar", "生存-natives"] {
            assert!(!game.join(skipped).exists(), "{skipped}");
        }
        // The source is left as it was.
        assert!(version.join("saves/Home/level.dat").is_file());
    }
}
