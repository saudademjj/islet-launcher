//! The mods a server made with Islet runs, carried in the pack friends
//! import. Before they join, the launcher compares their mods with it and
//! can fetch what is missing, so nobody is turned away at the door.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::core::download::{self, DownloadProgress, DownloadTask};
use crate::core::error::{AppError, AppResult};
use crate::core::manage::{self, ItemKind};
use crate::core::paths::AppPaths;
use crate::core::servers::nbt::Tag;

/// In the game folder, beside `servers.dat`.
pub const FILE: &str = "islet-server.json";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Manifest {
    pub name: String,
    /// Where the server is; empty when the host didn't say.
    pub address: String,
    pub mods: Vec<ManifestMod>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ManifestMod {
    pub file: String,
    pub sha1: String,
    /// The mod's own id, to find an older copy of it.
    pub id: Option<String>,
    /// Where to get exactly this file, when Modrinth has it.
    pub url: Option<String>,
}

/// What stands between a world and the server it is about to join.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub server: String,
    pub missing: Vec<ManifestMod>,
    /// Local files that are another version of a missing mod.
    pub outdated: Vec<String>,
    /// Every missing mod can be downloaded.
    pub fixable: bool,
}

/// A `servers.dat` holding just this server.
pub fn servers_dat(name: &str, address: &str) -> Vec<u8> {
    let entry = Tag::Compound(vec![("name".into(), Tag::String(name.into())), ("ip".into(), Tag::String(address.into()))]);
    crate::core::servers::nbt::write("", &Tag::Compound(vec![("servers".into(), Tag::List(10, vec![entry]))]))
}

pub fn read(paths: &AppPaths, id: &str) -> Option<Manifest> {
    AppPaths::validate_instance_id(id).ok()?;
    serde_json::from_slice(&fs::read(paths.instance_game_dir(id).join(FILE)).ok()?).ok()
}

/// "Host:25565" and "host" are the same server.
fn same_server(a: &str, b: &str) -> bool {
    let key = |address: &str| {
        crate::core::servers::parse_address(address).ok().map(|(host, port)| (host.to_lowercase(), port.unwrap_or(25565)))
    };
    key(a).is_some() && key(a) == key(b)
}

/// The mods the world lacks for `address`, or `None` when it has them all
/// or wasn't made for that server.
pub fn check(paths: &AppPaths, id: &str, address: &str) -> AppResult<Option<Check>> {
    let Some(manifest) = read(paths, id) else {
        return Ok(None);
    };
    if !manifest.address.trim().is_empty() && !same_server(&manifest.address, address) {
        return Ok(None);
    }
    let local = crate::core::content::mod_hashes(paths, id)?;
    let have: HashSet<&str> = local.iter().map(|(_, sha1)| sha1.as_str()).collect();
    let missing: Vec<ManifestMod> = manifest.mods.iter().filter(|m| !have.contains(m.sha1.as_str())).cloned().collect();
    if missing.is_empty() {
        return Ok(None);
    }
    let wanted: HashSet<&str> = manifest.mods.iter().map(|m| m.sha1.as_str()).collect();
    let missing_ids: HashSet<&str> = missing.iter().filter_map(|m| m.id.as_deref()).collect();
    let meta = crate::core::content::mod_meta(paths, id)?;
    let outdated = local
        .iter()
        .filter(|(name, sha1)| !wanted.contains(sha1.as_str()) && meta.get(name).and_then(|m| m.id.as_deref()).is_some_and(|mod_id| missing_ids.contains(mod_id)))
        .map(|(name, _)| name.clone())
        .collect();
    let fixable = missing.iter().all(|m| m.url.as_deref().is_some_and(crate::core::modpacks::trusted_url));
    Ok(Some(Check { server: manifest.name, missing, outdated, fixable }))
}

/// A plain `.jar` file name, nothing that points elsewhere.
fn jar_name(name: &str) -> Option<String> {
    let mut parts = std::path::Path::new(name).components();
    let single = matches!((parts.next(), parts.next()), (Some(std::path::Component::Normal(_)), None));
    (single && name.to_lowercase().ends_with(".jar") && !name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))).then(|| name.to_string())
}

/// Downloads the missing mods (each checked against its SHA-1) and moves
/// the versions they replace to the recycle bin. Returns (added, removed).
pub async fn fix(client: &reqwest::Client, paths: &AppPaths, id: &str, address: &str, cancel: &CancellationToken) -> AppResult<(usize, usize)> {
    let Some(check) = check(paths, id, address)? else {
        return Ok((0, 0));
    };
    if !check.fixable {
        return Err(AppError::new(
            "server_mods",
            crate::core::i18n::pick("有的模组没法自动下载，请向服主要最新的整合包，用「从整合包更新」装上。", "Some mods can't be downloaded; ask the host for the latest pack and use \"Update from pack\"."),
        ));
    }
    let mods = paths.instance_game_dir(id).join("mods");
    let mut names = HashMap::new();
    let mut tasks = Vec::new();
    for item in &check.missing {
        let file = jar_name(&item.file).ok_or_else(|| AppError::detail("server_mods", crate::core::i18n::pick("整合包里的模组文件名不安全。", "A mod file name in the pack isn't safe."), item.file.clone()))?;
        let target = mods.join(&file);
        // A disabled or differently named copy may sit under the same name.
        if target.exists() {
            return Err(AppError::detail("server_mods", crate::core::i18n::pick("模组文件夹里已有同名但内容不同的文件。", "The mods folder already has a different file with that name."), file));
        }
        names.insert(file.clone(), ());
        tasks.push(DownloadTask::new(file, item.url.clone().unwrap_or_default(), Vec::new(), target, Some(item.sha1.clone()), None));
    }
    let progress = Arc::new(DownloadProgress::new(&tasks));
    download::download_all(tasks, client.clone(), 4, cancel.clone(), progress).await?;
    for name in &check.outdated {
        manage::recycle_item(paths, id, ItemKind::Mod, name, None)?;
    }
    Ok((names.len(), check.outdated.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_match_with_the_default_port() {
        assert!(same_server("Play.Example.com", "play.example.com:25565"));
        assert!(!same_server("play.example.com:25566", "play.example.com"));
        assert!(!same_server("", ""));
    }

    #[test]
    fn only_plain_jar_names_are_written() {
        assert_eq!(jar_name("sodium-0.6.jar").as_deref(), Some("sodium-0.6.jar"));
        for bad in ["../evil.jar", "a/b.jar", "C:evil.jar", "notes.txt", ""] {
            assert!(jar_name(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn the_server_list_holds_the_server() {
        let (_, root) = crate::core::servers::nbt::read(&servers_dat("生存", "1.2.3.4:25570")).unwrap();
        let Some(Tag::List(10, items)) = root.get("servers") else { panic!("no list") };
        assert_eq!(items[0].get("ip").and_then(Tag::as_str), Some("1.2.3.4:25570"));
        assert_eq!(items[0].get("name").and_then(Tag::as_str), Some("生存"));
    }

    #[test]
    fn missing_and_outdated_mods_are_found() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let w = "6f1c2a1e-6c1f-4a52-9d3b-0b6a7d2c9e11";
        let mods = paths.instance_game_dir(w).join("mods");
        fs::create_dir_all(&mods).unwrap();
        let jar = |name: &str, id: &str, body: &str| {
            use std::io::Write;
            let mut zip = zip::ZipWriter::new(fs::File::create(mods.join(name)).unwrap());
            zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(format!(r#"{{"id":"{id}","version":"{body}"}}"#).as_bytes()).unwrap();
            zip.finish().unwrap();
            use sha1::Digest;
            hex::encode(sha1::Sha1::digest(fs::read(mods.join(name)).unwrap()))
        };
        let kept = jar("lib.jar", "lib", "1");
        jar("tech-1.0.jar", "tech", "1.0");
        let manifest = Manifest {
            name: "生存".into(),
            address: "example.com".into(),
            mods: vec![
                ManifestMod { file: "lib.jar".into(), sha1: kept, id: Some("lib".into()), url: None },
                ManifestMod { file: "tech-1.1.jar".into(), sha1: "a".repeat(40), id: Some("tech".into()), url: Some("https://cdn.modrinth.com/data/x/tech-1.1.jar".into()) },
            ],
        };
        fs::write(paths.instance_game_dir(w).join(FILE), serde_json::to_vec(&manifest).unwrap()).unwrap();

        assert!(check(&paths, w, "other.example.com").unwrap().is_none());
        let found = check(&paths, w, "example.com:25565").unwrap().unwrap();
        assert_eq!(found.missing.iter().map(|m| m.file.as_str()).collect::<Vec<_>>(), ["tech-1.1.jar"]);
        assert_eq!(found.outdated, ["tech-1.0.jar"]);
        assert!(found.fixable);
    }
}
