//! Local mod and world management. No operation deletes the only copy of saves.
use crate::core::{
    error::{AppError, AppResult},
    instances::Instance,
    java::runtime::safe_archive_path,
    paths::AppPaths,
    util::now_millis,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModFile {
    pub name: String,
    pub enabled: bool,
    pub bytes: u64,
}

/// What a Modrinth project installs as, and where it lives in the game folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    #[default]
    Mod,
    ResourcePack,
    Shader,
    /// Goes into one save's `datapacks`, not the game folder.
    Datapack,
}

impl ContentKind {
    pub fn folder(self) -> &'static str {
        match self {
            Self::Mod => "mods",
            Self::ResourcePack => "resourcepacks",
            Self::Shader => "shaderpacks",
            Self::Datapack => "datapacks",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Mod => ".jar",
            Self::ResourcePack | Self::Shader | Self::Datapack => ".zip",
        }
    }

    /// Modrinth's `project_type` facet value.
    pub fn project_type(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::ResourcePack => "resourcepack",
            Self::Shader => "shader",
            Self::Datapack => "datapack",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mod => crate::core::i18n::pick("模组", "mod"),
            Self::ResourcePack => crate::core::i18n::pick("资源包", "resource pack"),
            Self::Shader => crate::core::i18n::pick("光影包", "shader pack"),
            Self::Datapack => crate::core::i18n::pick("数据包", "datapack"),
        }
    }
}

/// A resource pack or shader pack: a `.zip`, or an unpacked folder.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackFile {
    pub name: String,
    pub bytes: u64,
    pub folder: bool,
    /// The pack's own `pack.png` as a data URL, when it has a small one.
    pub icon: Option<String>,
    /// Where the game has it enabled: 1 is the top of its list (the pack
    /// that wins); `None` when it is not enabled.
    pub active: Option<u32>,
}

/// The packs the game has enabled, by file name, highest priority first.
/// Resource packs come from options.txt (stored lowest first); a shader pack
/// from Iris's or OptiFine's own settings file.
fn enabled_packs(game_dir: &Path, kind: ContentKind) -> Vec<String> {
    let read = |name: &str| fs::read(game_dir.join(name)).ok().map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let setting = |text: &str, key: &str| text.lines().find_map(|line| line.strip_prefix(key).map(|v| v.trim().to_string()));
    match kind {
        ContentKind::ResourcePack => {
            let Some(text) = read("options.txt") else { return Vec::new() };
            let Some(list) = setting(&text, "resourcePacks:") else { return Vec::new() };
            let names: Vec<String> = serde_json::from_str(&list).unwrap_or_default();
            names.iter().rev().filter_map(|n| n.strip_prefix("file/").map(str::to_string)).collect()
        }
        // Both are written as Java properties, which escape non-ASCII names.
        ContentKind::Shader => {
            if let Some(text) = read("config/iris.properties") {
                let enabled = setting(&text, "enableShaders=").is_none_or(|v| v != "false");
                return setting(&text, "shaderPack=")
                    .map(|name| unescape_property(&name))
                    .filter(|name| enabled && !name.is_empty())
                    .into_iter()
                    .collect();
            }
            if let Some(text) = read("optionsshaders.txt") {
                return setting(&text, "shaderPack=")
                    .map(|name| unescape_property(&name))
                    .filter(|name| !name.is_empty() && name != "OFF")
                    .into_iter()
                    .collect();
            }
            Vec::new()
        }
        ContentKind::Mod | ContentKind::Datapack => Vec::new(),
    }
}

/// Turns resource packs on and sets their order, as the game's own pack
/// screen does: `top_first` lists the enabled packs, the one that wins
/// first. Built-in entries (vanilla, mod resources) keep their place at the
/// bottom.
pub fn set_resource_packs(paths: &AppPaths, id: &str, top_first: &[String]) -> AppResult<()> {
    AppPaths::validate_instance_id(id)?;
    let game = paths.instance_game_dir(id);
    let file = game.join("options.txt");
    let Ok(bytes) = fs::read(&file) else {
        return Err(AppError::new("options_missing", crate::core::i18n::pick("这个世界还没启动过。先进一次游戏，再在这里调整资源包。", "This world hasn't been started yet. Play it once, then adjust resource packs here.")));
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    for name in top_first {
        crate::core::manage::single_name(name)?;
        if !game.join("resourcepacks").join(name).exists() {
            return Err(AppError::new("pack_missing", crate::tr!("找不到资源包「{name}」。", "Resource pack \"{name}\" not found.")));
        }
    }
    let current: Vec<String> = text
        .lines()
        .find_map(|line| line.strip_prefix("resourcePacks:"))
        .and_then(|list| serde_json::from_str(list.trim()).ok())
        .unwrap_or_else(|| vec!["vanilla".to_string()]);
    let mut list: Vec<String> = current.into_iter().filter(|entry| !entry.starts_with("file/")).collect();
    list.extend(top_first.iter().rev().map(|name| format!("file/{name}")));
    let line = format!("resourcePacks:{}", serde_json::to_string(&list)?);
    let mut found = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|existing| {
            if existing.starts_with("resourcePacks:") {
                found = true;
                line.clone()
            } else {
                existing.to_string()
            }
        })
        .collect();
    if !found {
        lines.push(line);
    }
    crate::core::util::atomic_write(&file, format!("{}\n", lines.join("\n")).as_bytes())?;
    Ok(())
}

/// A Java properties value as written: `\uXXXX` (UTF-16, so pairs make one
/// character), `\t`-style escapes, and a backslash before anything else.
fn unescape_property(value: &str) -> String {
    let mut units: Vec<u16> = Vec::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        let ch = if ch != '\\' {
            ch
        } else {
            match chars.next() {
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    match u16::from_str_radix(&hex, 16) {
                        Ok(unit) if hex.len() == 4 => {
                            units.push(unit);
                            continue;
                        }
                        _ => {
                            units.extend("\\u".encode_utf16().chain(hex.encode_utf16()));
                            continue;
                        }
                    }
                }
                Some('t') => '\t',
                Some('n') => '\n',
                Some('r') => '\r',
                Some('f') => '\u{c}',
                Some(other) => other,
                None => break,
            }
        };
        let mut buffer = [0u16; 2];
        units.extend_from_slice(ch.encode_utf16(&mut buffer));
    }
    String::from_utf16_lossy(&units)
}

/// `pack.png` from a pack folder or zip, as a data URL, when it is small.
fn pack_icon(path: &Path, folder: bool) -> Option<String> {
    use std::io::Read;
    let bytes = if folder {
        let icon = path.join("pack.png");
        let meta = fs::metadata(&icon).ok()?;
        if !meta.is_file() || meta.len() > WORLD_ICON_LIMIT {
            return None;
        }
        fs::read(icon).ok()?
    } else {
        let mut archive = zip::ZipArchive::new(fs::File::open(path).ok()?).ok()?;
        let entry = archive.by_name("pack.png").ok()?;
        if entry.size() > WORLD_ICON_LIMIT {
            return None;
        }
        let mut bytes = Vec::new();
        entry.take(WORLD_ICON_LIMIT).read_to_end(&mut bytes).ok()?;
        bytes
    };
    use base64::Engine;
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

pub fn packs(paths: &AppPaths, id: &str, kind: ContentKind) -> AppResult<Vec<PackFile>> {
    AppPaths::validate_instance_id(id)?;
    let game_dir = paths.instance_game_dir(id);
    let Ok(entries) = fs::read_dir(game_dir.join(kind.folder())) else {
        return Ok(Vec::new());
    };
    let enabled = enabled_packs(&game_dir, kind);
    let active = |name: &str| enabled.iter().position(|n| n == name).map(|i| i as u32 + 1);
    let mut result = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if file_type.is_dir() {
            result.push(PackFile {
                bytes: tree_size(&entry.path()),
                icon: pack_icon(&entry.path(), true),
                active: active(&name),
                name,
                folder: true,
            });
        } else if file_type.is_file() && name.to_ascii_lowercase().ends_with(".zip") {
            result.push(PackFile {
                bytes: entry.metadata().map(|meta| meta.len()).unwrap_or(0),
                icon: pack_icon(&entry.path(), false),
                active: active(&name),
                name,
                folder: false,
            });
        }
    }
    // Enabled packs first, in the game's order; the rest by name.
    result.sort_by(|a, b| {
        a.active
            .unwrap_or(u32::MAX)
            .cmp(&b.active.unwrap_or(u32::MAX))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(result)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CachedHash {
    bytes: u64,
    modified_ms: i64,
    sha1: String,
    /// CurseForge's fingerprint; missing in caches written before it existed.
    #[serde(default)]
    fingerprint: Option<u32>,
}

/// SHA-1 of every local mod file (disabled ones included), as
/// `(file name, hash)`. Hashes are cached by size and modification time in
/// the instance folder, so only new or changed files are read again.
pub fn mod_hashes(paths: &AppPaths, id: &str) -> AppResult<Vec<(String, String)>> {
    Ok(hashed_mods(paths, id)?.into_iter().map(|(name, sha1, _)| (name, sha1)).collect())
}

/// CurseForge fingerprints of every local mod file, from the same cache.
pub fn mod_fingerprints(paths: &AppPaths, id: &str) -> AppResult<Vec<(String, u32)>> {
    Ok(hashed_mods(paths, id)?.into_iter().map(|(name, _, fingerprint)| (name, fingerprint)).collect())
}

fn hashed_mods(paths: &AppPaths, id: &str) -> AppResult<Vec<(String, String, u32)>> {
    AppPaths::validate_instance_id(id)?;
    let dir = paths.instance_game_dir(id).join("mods");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let cache_path = paths.instance_dir(id).join("mod-hashes.json");
    let cached: std::collections::BTreeMap<String, CachedHash> = fs::read(&cache_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let mut fresh = std::collections::BTreeMap::new();
    let mut result = Vec::new();
    for file in mods(paths, id)? {
        let path = dir.join(&file.name);
        let modified_ms = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_millis() as i64)
            .unwrap_or(0);
        let (sha1, fingerprint) = match cached.get(&file.name) {
            Some(CachedHash { bytes, modified_ms: when, sha1, fingerprint: Some(fingerprint) }) if *bytes == file.bytes && *when == modified_ms => (sha1.clone(), *fingerprint),
            _ => {
                let data = fs::read(&path)?;
                (file_sha1(&data), crate::core::curseforge::fingerprint(&data))
            }
        };
        result.push((file.name.clone(), sha1.clone(), fingerprint));
        fresh.insert(
            file.name,
            CachedHash {
                bytes: file.bytes,
                modified_ms,
                sha1,
                fingerprint: Some(fingerprint),
            },
        );
    }
    // The cache only saves work; failing to write it is not an error.
    let unchanged = fresh.len() == cached.len()
        && fresh
            .iter()
            .all(|(name, entry)| cached.get(name).is_some_and(|old| old.sha1 == entry.sha1 && old.modified_ms == entry.modified_ms && old.fingerprint == entry.fingerprint));
    if !unchanged {
        if let Ok(encoded) = serde_json::to_vec(&fresh) {
            let _ = crate::core::util::atomic_write(&cache_path, &encoded);
        }
    }
    Ok(result)
}

/// A file's size and modification time: when both are unchanged, so is
/// anything read from it.
fn stamp(path: &Path) -> Option<(u64, i64)> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_millis() as i64);
    Some((meta.len(), modified))
}

/// Things read from files, kept for this run while the file is unchanged.
type FileCache<T> = std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<PathBuf, ((u64, i64), T)>>>;

fn cached<T: Clone>(cache: &FileCache<T>, path: &Path, read: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
    let Some(stamp) = stamp(path) else {
        return read();
    };
    if let Some((seen, value)) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(path) {
        if *seen == stamp {
            return Ok(value.clone());
        }
    }
    let value = read()?;
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_path_buf(), (stamp, value.clone()));
    Ok(value)
}

static PACK_HASHES: FileCache<String> = std::sync::LazyLock::new(Default::default);
static MOD_ICONS: FileCache<Option<String>> = std::sync::LazyLock::new(Default::default);
static MOD_META: FileCache<ModMeta> = std::sync::LazyLock::new(Default::default);

/// What a mod's jar says about itself: who it is, and which other mods it
/// cannot load without.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModMeta {
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    /// Required mod ids; the game, Java and the loader itself left out.
    pub depends: Vec<String>,
    /// Other ids this jar answers to, the jars bundled inside it included.
    pub provides: Vec<String>,
}

/// Ids a mod may require that are never files in the mods folder.
const PLATFORM_IDS: &[&str] = &["minecraft", "java", "fabricloader", "fabric-loader", "quilt_loader", "forge", "neoforge", "mixinextras"];

/// Each mod's metadata by file name, for names, sorting and dependency warnings.
pub fn mod_meta(paths: &AppPaths, id: &str) -> AppResult<std::collections::BTreeMap<String, ModMeta>> {
    AppPaths::validate_instance_id(id)?;
    let dir = paths.instance_game_dir(id).join("mods");
    let mut metas = std::collections::BTreeMap::new();
    if !dir.is_dir() {
        return Ok(metas);
    }
    for file in mods(paths, id)? {
        let path = dir.join(&file.name);
        let meta = cached(&MOD_META, &path, || Ok(read_mod_meta(&path)))?;
        if meta != ModMeta::default() {
            metas.insert(file.name, meta);
        }
    }
    Ok(metas)
}

fn read_mod_meta(path: &Path) -> ModMeta {
    let Some(mut archive) = fs::File::open(path).ok().and_then(|file| zip::ZipArchive::new(file).ok()) else {
        return ModMeta::default();
    };
    let (mut meta, nested) = jar_meta(&mut archive);
    // Loaders ship API modules and libraries as jars inside the jar: a mod
    // that needs one of those is satisfied by this file.
    for name in nested.iter().take(256) {
        let Some(bytes) = zip_bytes(&mut archive, name, 16 * 1024 * 1024) else { continue };
        let Ok(mut inner) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else { continue };
        let (inner, _) = jar_meta(&mut inner);
        meta.provides.extend(inner.id.into_iter().chain(inner.provides));
    }
    meta.provides.sort();
    meta.provides.dedup();
    let own = |id: &String| meta.id.as_ref() == Some(id) || meta.provides.contains(id);
    let depends = meta.depends.iter().filter(|id| !own(id)).cloned().collect();
    meta.depends = depends;
    meta
}

/// The metadata in one jar, and the jars it bundles.
fn jar_meta<R: std::io::Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>) -> (ModMeta, Vec<String>) {
    let mut meta = ModMeta::default();
    let mut nested = Vec::new();
    let text = |archive: &mut zip::ZipArchive<R>, name: &str| zip_bytes(archive, name, 256 * 1024).and_then(|bytes| String::from_utf8(bytes).ok());
    let json = |text: Option<String>| text.and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    let string = |value: Option<&serde_json::Value>| value.and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty()).map(String::from);
    // A dependency is an id, or an object with one; Quilt ids may carry a maven group.
    let entry_id = |value: &serde_json::Value| {
        let id = value.as_str().or_else(|| value.get("id")?.as_str())?;
        let optional = value.get("optional").and_then(|v| v.as_bool()).unwrap_or(false);
        (!optional).then(|| id.rsplit(':').next().unwrap_or(id).to_string())
    };
    if let Some(v) = json(text(archive, "fabric.mod.json")) {
        meta.id = string(v.get("id"));
        meta.name = string(v.get("name"));
        meta.description = string(v.get("description"));
        meta.version = string(v.get("version"));
        if let Some(depends) = v.get("depends").and_then(|d| d.as_object()) {
            meta.depends.extend(depends.keys().cloned());
        }
        if let Some(provides) = v.get("provides").and_then(|p| p.as_array()) {
            meta.provides.extend(provides.iter().filter_map(entry_id));
        }
        if let Some(jars) = v.get("jars").and_then(|j| j.as_array()) {
            nested.extend(jars.iter().filter_map(|jar| jar.get("file")?.as_str().map(String::from)));
        }
    } else if let Some(v) = json(text(archive, "quilt.mod.json")) {
        meta.id = string(v.pointer("/quilt_loader/id"));
        meta.name = string(v.pointer("/quilt_loader/metadata/name"));
        meta.description = string(v.pointer("/quilt_loader/metadata/description"));
        meta.version = string(v.pointer("/quilt_loader/version"));
        if let Some(depends) = v.pointer("/quilt_loader/depends").and_then(|d| d.as_array()) {
            meta.depends.extend(depends.iter().filter_map(entry_id));
        }
        if let Some(provides) = v.pointer("/quilt_loader/provides").and_then(|p| p.as_array()) {
            meta.provides.extend(provides.iter().filter_map(entry_id));
        }
        if let Some(jars) = v.pointer("/quilt_loader/jars").and_then(|j| j.as_array()) {
            nested.extend(jars.iter().filter_map(|jar| jar.as_str().map(String::from)));
        }
    } else if let Some(toml) = text(archive, "META-INF/neoforge.mods.toml").or_else(|| text(archive, "META-INF/mods.toml")) {
        toml_meta(&toml, &mut meta);
        if let Some(v) = json(text(archive, "META-INF/jarjar/metadata.json")) {
            if let Some(jars) = v.get("jars").and_then(|j| j.as_array()) {
                nested.extend(jars.iter().filter_map(|jar| jar.get("path")?.as_str().map(String::from)));
            }
        }
    }
    meta.depends.retain(|id| !PLATFORM_IDS.contains(&id.as_str()));
    meta.depends.sort();
    meta.depends.dedup();
    (meta, nested)
}

/// The first mod's id, name, version and description, its required
/// dependencies, and the ids of any further mods in a mods.toml, read
/// line by line without a TOML parser.
fn toml_meta(text: &str, meta: &mut ModMeta) {
    #[derive(PartialEq)]
    enum Section {
        Other,
        Mod(usize),
        Dependency,
    }
    let mut section = Section::Other;
    let mut mods = 0;
    // The dependency being read: its id, and whether it is required.
    let mut dependency: Option<(String, bool)> = None;
    let mut in_multiline = false;
    let flush = |dependency: &mut Option<(String, bool)>, meta: &mut ModMeta| {
        if let Some((id, true)) = dependency.take().filter(|(id, _)| !id.is_empty()) {
            meta.depends.push(id);
        }
    };
    for line in text.lines() {
        let line = line.trim();
        if in_multiline {
            in_multiline = !(line.contains("'''") || line.contains("\"\"\""));
            continue;
        }
        if line.starts_with('[') {
            flush(&mut dependency, meta);
            section = if line.starts_with("[[mods]]") {
                mods += 1;
                Section::Mod(mods)
            } else if line.starts_with("[[dependencies.") {
                // Forge names it with `mandatory`, NeoForge with `type`; both default to required.
                dependency = Some((String::new(), true));
                Section::Dependency
            } else {
                Section::Other
            };
            continue;
        }
        let Some((key, raw)) = line.split_once('=') else { continue };
        let (key, raw) = (key.trim(), raw.trim());
        if raw.starts_with("'''") || raw.starts_with("\"\"\"") {
            in_multiline = !raw[3..].contains(&raw[..3]);
            continue;
        }
        let value = toml_value(raw);
        match &section {
            Section::Mod(1) => match key {
                "modId" => meta.id = value,
                "displayName" => meta.name = value,
                "description" => meta.description = value,
                "version" => meta.version = value.filter(|v| !v.starts_with("${")),
                _ => {}
            },
            Section::Mod(_) if key == "modId" => meta.provides.extend(value),
            Section::Dependency => {
                if let Some((id, required)) = dependency.as_mut() {
                    match key {
                        "modId" => *id = value.unwrap_or_default(),
                        "mandatory" => *required = value.as_deref() == Some("true"),
                        "type" => *required = value.is_some_and(|v| v.eq_ignore_ascii_case("required")),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    flush(&mut dependency, meta);
}

/// A one-line TOML value: the inside of a quoted string, or a bare word,
/// without a trailing comment.
fn toml_value(raw: &str) -> Option<String> {
    let value = match raw.chars().next()? {
        quote @ ('"' | '\'') => {
            let inner = &raw[1..];
            &inner[..inner.find(quote)?]
        }
        _ => raw.split('#').next().unwrap_or("").trim(),
    };
    Some(value.trim().to_string()).filter(|v| !v.is_empty())
}

/// An entry of a zip, when it is no larger than `limit`.
fn zip_bytes<R: std::io::Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>, name: &str, limit: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let entry = archive.by_name(name.trim_start_matches("./").trim_start_matches('/')).ok()?;
    if entry.size() > limit {
        return None;
    }
    let mut bytes = Vec::new();
    entry.take(limit).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Zipped resource packs and shader packs with their SHA-1, for Modrinth to
/// identify. Folders cannot be identified and are left out.
pub fn pack_hashes(paths: &AppPaths, id: &str) -> AppResult<Vec<(ContentKind, String, String)>> {
    AppPaths::validate_instance_id(id)?;
    let mut result = Vec::new();
    for kind in [ContentKind::ResourcePack, ContentKind::Shader] {
        let Ok(entries) = fs::read_dir(paths.instance_game_dir(id).join(kind.folder())) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.to_ascii_lowercase().ends_with(".zip") || !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            let path = entry.path();
            let sha1 = cached(&PACK_HASHES, &path, || stream_sha1(&path))?;
            result.push((kind, name, sha1));
        }
    }
    Ok(result)
}

/// Each mod's own icon from inside its jar, by file name, as data URLs.
/// Mods without a usable one are left out.
pub fn mod_icons(paths: &AppPaths, id: &str) -> AppResult<std::collections::BTreeMap<String, String>> {
    AppPaths::validate_instance_id(id)?;
    let dir = paths.instance_game_dir(id).join("mods");
    let mut icons = std::collections::BTreeMap::new();
    if !dir.is_dir() {
        return Ok(icons);
    }
    for file in mods(paths, id)? {
        let path = dir.join(&file.name);
        if let Some(icon) = cached(&MOD_ICONS, &path, || Ok(mod_icon(&path)))? {
            icons.insert(file.name, icon);
        }
    }
    Ok(icons)
}

/// The icon a mod names in its metadata (Fabric, Quilt, Forge, NeoForge),
/// else a `pack.png` at the root of the jar.
fn mod_icon(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(fs::File::open(path).ok()?).ok()?;
    let mut text = |name: &str| -> Option<String> {
        let entry = archive.by_name(name).ok()?;
        if entry.size() > 256 * 1024 {
            return None;
        }
        let mut text = String::new();
        entry.take(256 * 1024).read_to_string(&mut text).ok()?;
        Some(text)
    };
    let json = |text: Option<String>| text.and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    let mut candidates: Vec<String> = Vec::new();
    if let Some(meta) = json(text("fabric.mod.json")) {
        candidates.extend(icon_choice(meta.get("icon")));
    }
    if let Some(meta) = json(text("quilt.mod.json")) {
        candidates.extend(icon_choice(meta.pointer("/quilt_loader/metadata/icon")));
    }
    for name in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
        if let Some(toml) = text(name) {
            candidates.extend(toml_logo(&toml));
        }
    }
    candidates.push("pack.png".into());
    candidates.iter().find_map(|name| zip_image(&mut archive, name.trim_start_matches("./").trim_start_matches('/')))
}

/// Fabric's `icon` is a path, or paths by pixel size; take the one nearest
/// 64, the size the launcher shows.
fn icon_choice(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(path) => Some(path.clone()),
        serde_json::Value::Object(sizes) => sizes
            .iter()
            .filter_map(|(size, path)| Some((size.parse::<i64>().ok()?, path.as_str()?)))
            .min_by_key(|(size, _)| ((size - 64).abs(), -size))
            .map(|(_, path)| path.to_string()),
        _ => None,
    }
}

/// `logoFile="logo.png"` from a mods.toml, without a TOML parser.
fn toml_logo(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let value = line.trim().strip_prefix("logoFile")?.trim_start().strip_prefix('=')?.trim();
        let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let inner = &value[1..];
        Some(inner[..inner.find(quote)?].to_string()).filter(|path| !path.is_empty())
    })
}

/// A small PNG or JPEG inside a zip, as a data URL.
fn zip_image(archive: &mut zip::ZipArchive<fs::File>, name: &str) -> Option<String> {
    use base64::Engine;
    use std::io::Read;
    let lower = name.to_ascii_lowercase();
    let mime = if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else {
        return None;
    };
    let entry = archive.by_name(name).ok()?;
    if entry.size() > WORLD_ICON_LIMIT {
        return None;
    }
    let mut bytes = Vec::new();
    entry.take(WORLD_ICON_LIMIT).read_to_end(&mut bytes).ok()?;
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

fn file_sha1(data: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    hex::encode(Sha1::digest(data))
}

/// Packs can be hundreds of megabytes, so they are hashed in pieces.
fn stream_sha1(path: &Path) -> AppResult<String> {
    use sha1::{Digest, Sha1};
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn mods(paths: &AppPaths, id: &str) -> AppResult<Vec<ModFile>> {
    AppPaths::validate_instance_id(id)?;
    let mut result = Vec::new();
    for entry in fs::read_dir(paths.instance_game_dir(id).join("mods"))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_file()
            && (name.ends_with(".jar") || name.ends_with(".jar.disabled") || name.ends_with(".litemod") || name.ends_with(".litemod.disabled"))
        {
            result.push(ModFile {
                enabled: name.ends_with(".jar") || name.ends_with(".litemod"),
                name,
                bytes: entry.metadata()?.len(),
            });
        }
    }
    result.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(result)
}

/// A singleplayer world in the instance's `saves` folder.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldInfo {
    /// Folder name under `saves`; also the world's id.
    pub folder: String,
    /// The name shown in the game (`LevelName`), when it can be read.
    pub name: Option<String>,
    pub bytes: u64,
    /// When `level.dat` was last written, i.e. last played.
    pub modified_ms: Option<i64>,
    /// The world's `icon.png` as a data URL, when small enough to inline.
    pub icon: Option<String>,
}

const WORLD_ICON_LIMIT: u64 = 64 * 1024;

pub fn worlds(paths: &AppPaths, id: &str) -> AppResult<Vec<WorldInfo>> {
    AppPaths::validate_instance_id(id)?;
    let saves = paths.instance_game_dir(id).join("saves");
    let Ok(entries) = fs::read_dir(&saves) else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        // Only real folders that hold a world; symlinks are not followed.
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) || !dir.join("level.dat").is_file() {
            continue;
        }
        let modified_ms = fs::metadata(dir.join("level.dat"))
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_millis() as i64);
        let icon_path = dir.join("icon.png");
        let icon = fs::metadata(&icon_path)
            .ok()
            .filter(|meta| meta.is_file() && meta.len() <= WORLD_ICON_LIMIT)
            .and_then(|_| fs::read(&icon_path).ok())
            .map(|bytes| {
                use base64::Engine;
                format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                )
            });
        result.push(WorldInfo {
            folder: entry.file_name().to_string_lossy().into_owned(),
            name: crate::core::manage::level_name(&dir),
            bytes: tree_size(&dir),
            modified_ms,
            icon,
        });
    }
    result.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms).then_with(|| a.folder.cmp(&b.folder)));
    Ok(result)
}

/// The folder of one world, checked to be a direct child of `saves`.
/// Where downloaded content of this kind goes: the game folder's own, or
/// for a datapack the chosen save's `datapacks`.
pub fn target_folder(paths: &AppPaths, id: &str, kind: ContentKind, save: Option<&str>) -> AppResult<PathBuf> {
    if kind != ContentKind::Datapack {
        return Ok(paths.instance_game_dir(id).join(kind.folder()));
    }
    let save = save.ok_or_else(|| AppError::new("world_missing", crate::core::i18n::pick("请先选择要装进哪个存档。", "Choose which save to put it in first.")))?;
    Ok(world_dir(paths, id, save)?.join("datapacks"))
}

pub fn world_dir(paths: &AppPaths, id: &str, folder: &str) -> AppResult<PathBuf> {
    AppPaths::validate_instance_id(id)?;
    let mut parts = Path::new(folder).components();
    let single = matches!(
        (parts.next(), parts.next()),
        (Some(std::path::Component::Normal(_)), None)
    );
    let dir = paths.instance_game_dir(id).join("saves").join(folder);
    if !single || !dir.join("level.dat").is_file() {
        return Err(AppError::new("world_missing", crate::core::i18n::pick("找不到这个存档。", "Save not found.")));
    }
    Ok(dir)
}

/// Bytes under a folder, not following symlinks.
pub(crate) fn tree_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => tree_size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map(|meta| meta.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

/// Enables or disables several mods at once; those already that way are
/// left alone. Returns how many changed.
pub fn set_mods_enabled(paths: &AppPaths, id: &str, names: &[String], enabled: bool) -> AppResult<usize> {
    let current = mods(paths, id)?;
    let mut changed = 0;
    let mut seen = std::collections::HashSet::new();
    for name in names {
        if !seen.insert(name) { continue; }
        if current.iter().any(|item| &item.name == name && item.enabled != enabled) {
            toggle_mod(paths, id, name)?;
            changed += 1;
        }
    }
    Ok(changed)
}

pub fn toggle_mod(paths: &AppPaths, id: &str, name: &str) -> AppResult<()> {
    let item = mods(paths, id)?
        .into_iter()
        .find(|item| item.name == name)
        .ok_or_else(|| AppError::new("mod_missing", crate::core::i18n::pick("找不到该模组。", "Mod not found.")))?;
    let new_name = if item.enabled {
        format!("{name}.disabled")
    } else {
        name.trim_end_matches(".disabled").to_string()
    };
    let root = paths.instance_game_dir(id).join("mods");
    if root.join(&new_name).exists() {
        return Err(AppError::new(
            "mod_exists",
            crate::core::i18n::pick("目标文件已存在，请先检查模组目录。", "The target file already exists; check the mods folder first."),
        ));
    }
    fs::rename(root.join(name), root.join(new_name))?;
    Ok(())
}

pub fn add_mods(paths: &AppPaths, id: &str, sources: &[PathBuf]) -> AppResult<()> {
    AppPaths::validate_instance_id(id)?;
    let root = paths.instance_game_dir(id).join("mods");
    fs::create_dir_all(&root)?;
    let mut names = std::collections::HashSet::new();
    for path in sources {
        let name = path
            .file_name()
            .ok_or_else(|| AppError::new("mod_file", crate::core::i18n::pick("模组文件名无效。", "Invalid mod file name.")))?;
        if path.extension().is_none_or(|ext| !ext.eq_ignore_ascii_case("jar") && !ext.eq_ignore_ascii_case("litemod")) || !path.is_file() {
            return Err(AppError::new("mod_file", crate::core::i18n::pick("请选择 .jar 模组文件。", "Choose .jar mod files.")));
        }
        if root.join(name).exists() || !names.insert(name.to_os_string()) {
            return Err(AppError::new(
                "mod_exists",
                crate::tr!("{} 已存在，未覆盖原文件。", "{} already exists; the original was kept.", name.to_string_lossy()),
            ));
        }
        zip::ZipArchive::new(fs::File::open(path)?).map_err(|e| {
            AppError::detail("mod_file", crate::core::i18n::pick("模组不是有效的 JAR 文件。", "The mod isn't a valid JAR file."), e.to_string())
        })?;
    }
    for path in sources {
        fs::copy(path, root.join(path.file_name().unwrap()))?;
    }
    Ok(())
}

pub fn backup(paths: &AppPaths, instance: &Instance) -> AppResult<PathBuf> {
    let dir = paths.root.join("backups").join(&instance.id);
    fs::create_dir_all(&dir)?;
    let output = dir.join(format!(
        "{}-{}.zip",
        now_millis(),
        uuid::Uuid::new_v4().simple()
    ));
    let temporary = output.with_extension("zip.part");
    let result = (|| -> AppResult<()> {
        let mut zip = zip::ZipWriter::new(fs::File::create(&temporary)?);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("instance.json", options)
            .map_err(zip_error)?;
        zip.write_all(&serde_json::to_vec_pretty(instance)?)?;
        archive_tree(
            &mut zip,
            &paths.instance_game_dir(&instance.id),
            Path::new("minecraft"),
        )?;
        zip.finish().map_err(zip_error)?.sync_all()?;
        fs::rename(&temporary, &output)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map(|_| output)
}

pub(crate) fn archive_tree(zip: &mut zip::ZipWriter<fs::File>, dir: &Path, prefix: &Path) -> AppResult<()> {
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            return Err(AppError::new(
                "backup_link",
                crate::core::i18n::pick("游戏目录含符号链接，备份已停止，未删除任何文件。", "The game folder has symbolic links; the backup stopped and nothing was deleted."),
            ));
        }
        let relative = prefix.join(entry.file_name());
        let name = relative.to_string_lossy().replace('\\', "/");
        if ty.is_dir() {
            zip.add_directory(format!("{name}/"), options)
                .map_err(zip_error)?;
            archive_tree(zip, &entry.path(), &relative)?;
        } else if ty.is_file() {
            zip.start_file(name, options).map_err(zip_error)?;
            std::io::copy(&mut fs::File::open(entry.path())?, zip)?;
        }
    }
    Ok(())
}

pub fn restore_backup(paths: &AppPaths, instance: &Instance, source: &Path) -> AppResult<PathBuf> {
    let mut zip = zip::ZipArchive::new(fs::File::open(source)?).map_err(zip_error)?;
    let metadata: Instance =
        serde_json::from_reader(zip.by_name("instance.json").map_err(zip_error)?)?;
    if metadata.version_id != instance.version_id || metadata.loader != instance.loader {
        return Err(AppError::new(
            "backup_version",
            crate::core::i18n::pick("备份的 Minecraft 版本或加载器与当前世界不匹配。请恢复到对应版本的世界。", "The backup's Minecraft version or loader doesn't match this world. Restore it into a matching world."),
        ));
    }
    let stage = paths
        .instance_dir(&instance.id)
        .join(format!(".restore-{}", uuid::Uuid::new_v4()));
    let prepared = (|| -> AppResult<()> {
        fs::create_dir_all(&stage)?;
        let mut total = 0u64;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(zip_error)?;
            let path = safe_archive_path(&entry.name().map_err(zip_error)?)?;
            let Ok(relative) = path.strip_prefix("minecraft") else {
                continue;
            };
            total = total.saturating_add(entry.size());
            if total > 32 * 1024 * 1024 * 1024
                || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
            {
                return Err(AppError::new(
                    "backup_size",
                    crate::core::i18n::pick("备份超出 32 GB 或包含符号链接。", "The backup exceeds 32 GB or contains symbolic links."),
                ));
            }
            let dest = stage.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(dest)?;
            } else {
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                std::io::copy(&mut entry, &mut fs::File::create(dest)?)?;
            }
        }
        Ok(())
    })();
    if let Err(error) = prepared {
        let _ = fs::remove_dir_all(stage);
        return Err(error);
    }
    // A fresh backup is mandatory before replacing any existing world.
    let saved = match backup(paths, instance) {
        Ok(p) => p,
        Err(e) => {
            let _ = fs::remove_dir_all(stage);
            return Err(e);
        }
    };
    let live = paths.instance_game_dir(&instance.id);
    let previous = paths
        .instance_dir(&instance.id)
        .join(format!(".before-restore-{}", uuid::Uuid::new_v4()));
    // On Windows a rename fails while any file inside is open (Explorer,
    // antivirus, a game server); the staged copy must not be left behind.
    if let Err(error) = fs::rename(&live, &previous) {
        let _ = fs::remove_dir_all(&stage);
        return Err(AppError::detail(
            "backup_restore",
            crate::core::i18n::pick("游戏目录正被其他程序占用，未做任何更改。请关闭资源管理器窗口或相关程序后重试。", "Another program is using the game folder; nothing changed. Close file manager windows or that program and try again."),
            error.to_string(),
        ));
    }
    if let Err(error) = fs::rename(&stage, &live) {
        let _ = fs::remove_dir_all(&stage);
        if fs::rename(&previous, &live).is_err() {
            return Err(AppError::detail(
                "backup_restore",
                crate::tr!("恢复中断，原数据完整保存在 {}，请将其改名回 minecraft。", "The restore stopped; the original data is safe in {}. Rename it back to minecraft.",
                    previous.display()
                ),
                error.to_string(),
            ));
        }
        return Err(error.into());
    }
    // Previous data remains recoverable in the ZIP backup; cleanup is best effort.
    let _ = fs::remove_dir_all(previous);
    fs::create_dir_all(live.join("mods"))?;
    Ok(saved)
}
pub(crate) fn zip_error(error: zip::result::ZipError) -> AppError {
    AppError::detail("archive", crate::core::i18n::pick("压缩包无法读写。", "The archive can't be read or written."), error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resource_pack_order_is_written_like_the_game_does() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
        let instance = store.create("包", "1.21", "release").unwrap();
        let game = paths.instance_game_dir(&instance.id);
        fs::create_dir_all(game.join("resourcepacks/Faithful")).unwrap();
        fs::write(game.join("resourcepacks/a.zip"), b"zip").unwrap();
        assert!(set_resource_packs(&paths, &instance.id, &["a.zip".into()]).is_err(), "needs options.txt");
        fs::write(game.join("options.txt"), "fov:0.0\nresourcePacks:[\"vanilla\",\"fabric\",\"file/old.zip\"]\nlang:zh_cn\n").unwrap();
        set_resource_packs(&paths, &instance.id, &["Faithful".into(), "a.zip".into()]).unwrap();
        let text = fs::read_to_string(game.join("options.txt")).unwrap();
        assert!(text.contains(r#"resourcePacks:["vanilla","fabric","file/a.zip","file/Faithful"]"#), "{text}");
        assert!(text.contains("lang:zh_cn"));
        assert_eq!(enabled_packs(&game, ContentKind::ResourcePack), vec!["Faithful", "a.zip"]);
        assert!(set_resource_packs(&paths, &instance.id, &["../x".into()]).is_err());
    }

    #[test]
    fn mod_hashes_are_cached_and_follow_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
        let instance = store.create("模组", "1.21", "release").unwrap();
        let mods_dir = paths.instance_game_dir(&instance.id).join("mods");
        fs::create_dir_all(&mods_dir).unwrap();
        fs::write(mods_dir.join("a.jar"), b"abc").unwrap();
        fs::write(mods_dir.join("b.jar.disabled"), b"").unwrap();
        fs::write(mods_dir.join("notes.txt"), b"x").unwrap();

        let first = mod_hashes(&paths, &instance.id).unwrap();
        assert_eq!(
            first,
            vec![
                ("a.jar".to_string(), "a9993e364706816aba3e25717850c26c9cd0d89d".to_string()),
                ("b.jar.disabled".to_string(), "da39a3ee5e6b4b0d3255bfef95601890afd80709".to_string()),
            ]
        );
        assert!(paths.instance_dir(&instance.id).join("mod-hashes.json").is_file());

        // A changed file is hashed again even though the cache has an entry.
        fs::write(mods_dir.join("a.jar"), b"abcd").unwrap();
        let file = fs::File::options().write(true).open(mods_dir.join("a.jar")).unwrap();
        file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(5)).unwrap();
        let second = mod_hashes(&paths, &instance.id).unwrap();
        assert_eq!(second[0].1, "81fe8bfe87576c3ecb22426f8e57847382917acf");
    }

    #[test]
    fn packs_list_zips_and_folders_only() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
        let instance = store.create("资源", "1.21", "release").unwrap();
        assert!(packs(&paths, &instance.id, ContentKind::Shader).unwrap().is_empty());
        let shaders = paths.instance_game_dir(&instance.id).join("shaderpacks");
        fs::create_dir_all(shaders.join("Unpacked/shaders")).unwrap();
        fs::write(shaders.join("Unpacked/shaders/a.fsh"), b"12345").unwrap();
        fs::write(shaders.join("BSL.zip"), b"zip").unwrap();
        fs::write(shaders.join("BSL.zip.txt"), b"settings").unwrap();
        let found = packs(&paths, &instance.id, ContentKind::Shader).unwrap();
        assert_eq!(found.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["BSL.zip", "Unpacked"]);
        assert!(found[1].folder);
        assert_eq!(found[1].bytes, 5);
    }

    #[test]
    fn packs_carry_their_icon_and_the_games_order() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
        let instance = store.create("资源", "1.21", "release").unwrap();
        let game = paths.instance_game_dir(&instance.id);
        let packs_dir = game.join("resourcepacks");
        fs::create_dir_all(packs_dir.join("Folder")).unwrap();
        fs::write(packs_dir.join("Folder/pack.png"), b"png").unwrap();
        {
            let mut zip = zip::ZipWriter::new(fs::File::create(packs_dir.join("Zipped.zip")).unwrap());
            zip.start_file("pack.png", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"zpng").unwrap();
            zip.finish().unwrap();
        }
        fs::write(packs_dir.join("Off.zip"), b"not a zip").unwrap();
        // Stored lowest priority first: Zipped wins over Folder.
        fs::write(game.join("options.txt"), "lang:zh_cn\nresourcePacks:[\"vanilla\",\"file/Folder\",\"file/Zipped.zip\"]\n").unwrap();

        let found = packs(&paths, &instance.id, ContentKind::ResourcePack).unwrap();
        let summary: Vec<_> = found.iter().map(|p| (p.name.as_str(), p.active)).collect();
        assert_eq!(summary, [("Zipped.zip", Some(1)), ("Folder", Some(2)), ("Off.zip", None)]);
        assert_eq!(found[0].icon.as_deref(), Some("data:image/png;base64,enBuZw=="));
        assert_eq!(found[1].icon.as_deref(), Some("data:image/png;base64,cG5n"));
        assert_eq!(found[2].icon, None);

        let shaders = game.join("shaderpacks");
        fs::create_dir_all(&shaders).unwrap();
        fs::write(shaders.join("BSL.zip"), b"zip").unwrap();
        fs::write(shaders.join("Other.zip"), b"zip").unwrap();
        fs::create_dir_all(game.join("config")).unwrap();
        fs::write(game.join("config/iris.properties"), "enableShaders=true\nshaderPack=BSL.zip\n").unwrap();
        let found = packs(&paths, &instance.id, ContentKind::Shader).unwrap();
        assert_eq!(found[0].name, "BSL.zip");
        assert_eq!(found[0].active, Some(1));
        assert_eq!(found[1].active, None);

        // Iris writes a Chinese name escaped.
        fs::write(shaders.join("光影 v2.zip"), b"zip").unwrap();
        fs::write(game.join("config/iris.properties"), "#Iris\nshaderPack=\\u5149\\u5F71 v2.zip\n").unwrap();
        let found = packs(&paths, &instance.id, ContentKind::Shader).unwrap();
        assert_eq!((found[0].name.as_str(), found[0].active), ("光影 v2.zip", Some(1)));
    }

    fn jar(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, bytes) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn mods_show_the_icon_from_their_jar() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
        let instance = store.create("图标", "1.21", "release").unwrap();
        let mods_dir = paths.instance_game_dir(&instance.id).join("mods");
        fs::create_dir_all(&mods_dir).unwrap();
        // Fabric by size: the one nearest 64 wins.
        jar(
            &mods_dir.join("fabric.jar"),
            &[
                ("fabric.mod.json", br#"{"id":"a","icon":{"16":"assets/a/16.png","128":"assets/a/128.png","64":"assets/a/64.png"}}"#),
                ("assets/a/64.png", b"p64"),
                ("assets/a/16.png", b"p16"),
            ],
        );
        jar(
            &mods_dir.join("forge.jar.disabled"),
            &[("META-INF/mods.toml", b"modLoader=\"javafml\"\n[[mods]]\nlogoFile = \"/logo.png\" # banner\n"), ("logo.png", b"logo")],
        );
        jar(&mods_dir.join("plain.jar"), &[("pack.png", b"pk")]);
        jar(&mods_dir.join("none.jar"), &[("fabric.mod.json", br#"{"icon":"missing.png"}"#)]);
        fs::write(mods_dir.join("broken.jar"), b"not a zip").unwrap();

        let icons = mod_icons(&paths, &instance.id).unwrap();
        let summary: Vec<_> = icons.iter().map(|(name, url)| (name.as_str(), url.as_str())).collect();
        assert_eq!(
            summary,
            [
                ("fabric.jar", "data:image/png;base64,cDY0"),
                ("forge.jar.disabled", "data:image/png;base64,bG9nbw=="),
                ("plain.jar", "data:image/png;base64,cGs="),
            ]
        );
        assert_eq!(toml_logo("logoFile='a.png'"), Some("a.png".into()));
        assert_eq!(toml_logo("logoFile=\"\""), None);
    }

    #[test]
    fn mods_say_what_they_need() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
        let instance = store.create("依赖", "1.21", "release").unwrap();
        let mods_dir = paths.instance_game_dir(&instance.id).join("mods");
        fs::create_dir_all(&mods_dir).unwrap();
        // Fabric API answers for the modules bundled inside it.
        let module = dir.path().join("module.jar");
        jar(&module, &[("fabric.mod.json", br#"{"id":"fabric-resource-loader-v0"}"#)]);
        jar(
            &mods_dir.join("fabric-api.jar"),
            &[
                ("fabric.mod.json", br#"{"id":"fabric-api","name":"Fabric API","provides":["fabric"],"jars":[{"file":"META-INF/jars/module.jar"}]}"#),
                ("META-INF/jars/module.jar", &fs::read(&module).unwrap()),
            ],
        );
        jar(
            &mods_dir.join("sodium.jar"),
            &[("fabric.mod.json", br#"{"id":"sodium","name":"Sodium","version":"0.6","description":"Fast","depends":{"minecraft":"1.21","fabricloader":"*","fabric-resource-loader-v0":"*"}}"#)],
        );
        jar(
            &mods_dir.join("create.jar.disabled"),
            &[(
                "META-INF/neoforge.mods.toml",
                b"modLoader=\"javafml\"\n[[mods]]\nmodId=\"create\"\nversion=\"${file.jarVersion}\"\ndisplayName='Create' # name\ndescription='''\nmodId=\"not-this\"\n'''\n[[mods]]\nmodId=\"flywheel\"\n[[dependencies.create]]\nmodId=\"neoforge\"\ntype=\"required\"\n[[dependencies.create]]\nmodId=\"ponder\"\n[[dependencies.create]]\nmodId=\"jei\"\ntype=\"optional\"\n[[dependencies.create]]\nmodId=\"registrate\"\nmandatory=true\n",
            )],
        );
        // A NeoForge mod answers for the mods it bundles with jarjar.
        let ponder = dir.path().join("ponder.jar");
        jar(&ponder, &[("META-INF/neoforge.mods.toml", b"[[mods]]\nmodId=\"ponder\"\n")]);
        let create_jar = mods_dir.join("create.jar.disabled");
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        {
            let mut archive = zip::ZipArchive::new(fs::File::open(&create_jar).unwrap()).unwrap();
            let mut toml = String::new();
            std::io::Read::read_to_string(&mut archive.by_name("META-INF/neoforge.mods.toml").unwrap(), &mut toml).unwrap();
            files.push(("META-INF/neoforge.mods.toml".into(), toml.into_bytes()));
        }
        files.push(("META-INF/jarjar/metadata.json".into(), br#"{"jars":[{"path":"META-INF/jarjar/ponder.jar"}]}"#.to_vec()));
        files.push(("META-INF/jarjar/ponder.jar".into(), fs::read(&ponder).unwrap()));
        jar(&create_jar, &files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect::<Vec<_>>());
        jar(&mods_dir.join("plain.jar"), &[("pack.png", b"pk")]);

        let metas = mod_meta(&paths, &instance.id).unwrap();
        assert_eq!(metas.keys().collect::<Vec<_>>(), ["create.jar.disabled", "fabric-api.jar", "sodium.jar"]);
        let api = &metas["fabric-api.jar"];
        assert_eq!(api.provides, ["fabric", "fabric-resource-loader-v0"]);
        let sodium = &metas["sodium.jar"];
        assert_eq!((sodium.name.as_deref(), sodium.version.as_deref(), sodium.description.as_deref()), (Some("Sodium"), Some("0.6"), Some("Fast")));
        assert_eq!(sodium.depends, ["fabric-resource-loader-v0"]);
        let create = &metas["create.jar.disabled"];
        assert_eq!((create.id.as_deref(), create.name.as_deref(), create.version.as_deref()), (Some("create"), Some("Create"), None));
        assert_eq!(create.provides, ["flywheel", "ponder"]);
        assert_eq!(create.depends, ["registrate"]);
    }

    #[test]
    fn zipped_packs_are_hashed_for_identification() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
        let instance = store.create("哈希", "1.21", "release").unwrap();
        let game = paths.instance_game_dir(&instance.id);
        fs::create_dir_all(game.join("resourcepacks/Folder")).unwrap();
        fs::create_dir_all(game.join("shaderpacks")).unwrap();
        fs::write(game.join("resourcepacks/A.zip"), b"abc").unwrap();
        fs::write(game.join("shaderpacks/S.ZIP"), b"abc").unwrap();
        let mut found = pack_hashes(&paths, &instance.id).unwrap();
        found.sort_by(|a, b| a.1.cmp(&b.1));
        let sha = "a9993e364706816aba3e25717850c26c9cd0d89d";
        assert_eq!(found, vec![(ContentKind::ResourcePack, "A.zip".into(), sha.into()), (ContentKind::Shader, "S.ZIP".into(), sha.into())]);
    }

    #[test]
    fn properties_values_are_unescaped() {
        assert_eq!(unescape_property(r"\u5149\u5F71\:x\\y"), "光影:x\\y");
        assert_eq!(unescape_property(r"\uD83D\uDE00"), "😀");
        assert_eq!(unescape_property(r"bad\u12"), r"bad\u12");
    }

    #[test]
    fn worlds_lists_real_saves_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
        let instance = store.create("世界", "1.21", "release").unwrap();
        assert!(worlds(&paths, &instance.id).unwrap().is_empty());

        let saves = paths.instance_game_dir(&instance.id).join("saves");
        for name in ["旧世界", "新世界"] {
            fs::create_dir_all(saves.join(name).join("region")).unwrap();
            fs::write(saves.join(name).join("level.dat"), b"0123456789").unwrap();
            fs::write(saves.join(name).join("region/r.0.0.mca"), vec![0u8; 100]).unwrap();
        }
        fs::write(saves.join("新世界/icon.png"), b"png").unwrap();
        // A folder without level.dat is not a world.
        fs::create_dir_all(saves.join("杂物")).unwrap();
        let old = fs::File::options().write(true).open(saves.join("旧世界/level.dat")).unwrap();
        old.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000)).unwrap();

        let found = worlds(&paths, &instance.id).unwrap();
        assert_eq!(found.iter().map(|w| w.folder.as_str()).collect::<Vec<_>>(), ["新世界", "旧世界"]);
        assert_eq!(found[0].bytes, 113);
        assert_eq!(found[0].icon.as_deref(), Some("data:image/png;base64,cG5n"));
        assert!(found[1].icon.is_none());
        assert_eq!(found[1].modified_ms, Some(1_000_000));

        assert!(world_dir(&paths, &instance.id, "新世界").is_ok());
        assert!(world_dir(&paths, &instance.id, "杂物").is_err());
        assert!(world_dir(&paths, &instance.id, "../新世界").is_err());
        assert!(world_dir(&paths, &instance.id, "新世界/region").is_err());
    }

    #[test]
    fn restore_keeps_pre_restore_data_as_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
        let instance = store.create("世界", "1.21", "release").unwrap();
        let world = paths.instance_game_dir(&instance.id).join("saves/世界");
        fs::create_dir_all(&world).unwrap();
        fs::write(world.join("level.dat"), b"old world").unwrap();
        let saved = backup(&paths, &instance).unwrap();
        fs::write(world.join("level.dat"), b"new world").unwrap();
        let preserved = restore_backup(&paths, &instance, &saved).unwrap();
        assert_eq!(fs::read(world.join("level.dat")).unwrap(), b"old world");
        let mut zip = zip::ZipArchive::new(fs::File::open(preserved).unwrap()).unwrap();
        let mut data = Vec::new();
        std::io::Read::read_to_end(
            &mut zip.by_name("minecraft/saves/世界/level.dat").unwrap(),
            &mut data,
        )
        .unwrap();
        assert_eq!(data, b"new world");
    }
}
