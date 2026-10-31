//! A world's mod config files (`config/`, and Forge's `defaultconfigs/`),
//! listed, read and written as text. Before a file is overwritten its
//! previous text is kept under the launcher's backups, so one bad edit can
//! be undone.
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use crate::core::{
    error::{AppError, AppResult},
    paths::AppPaths,
    util::now_millis,
};

const ROOTS: &[&str] = &["config", "defaultconfigs"];
const EXTENSIONS: &[&str] = &["toml", "json", "json5", "jsonc", "cfg", "conf", "properties", "txt", "yml", "yaml", "snbt", "ini"];
/// Bigger files are data, not settings anyone edits by hand.
const MAX_BYTES: u64 = 1024 * 1024;
const MAX_FILES: usize = 5000;
const MAX_DEPTH: usize = 6;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFile {
    /// Relative to the game folder, with forward slashes, e.g. `config/sodium-options.json`.
    pub path: String,
    pub bytes: u64,
    pub modified_ms: i64,
}

fn editable(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Every editable config file, by path.
pub fn list(paths: &AppPaths, id: &str) -> AppResult<Vec<ConfigFile>> {
    AppPaths::validate_instance_id(id)?;
    let game = paths.instance_game_dir(id);
    let mut found = Vec::new();
    for root in ROOTS {
        walk(&game.join(root), root, 0, &mut found);
    }
    found.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    Ok(found)
}

fn walk(dir: &Path, relative: &str, depth: usize, found: &mut Vec<ConfigFile>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if found.len() >= MAX_FILES {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        // Links could lead out of the game folder.
        let Ok(meta) = fs::symlink_metadata(entry.path()) else { continue };
        let path = format!("{relative}/{name}");
        if meta.is_dir() {
            if depth < MAX_DEPTH {
                walk(&entry.path(), &path, depth + 1, found);
            }
        } else if meta.is_file() && meta.len() <= MAX_BYTES && editable(&name) {
            let modified_ms = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as i64);
            found.push(ConfigFile { path, bytes: meta.len(), modified_ms });
        }
    }
}

/// The file a relative path names, refusing anything outside the config
/// folders, any link on the way, and file types that aren't settings.
fn resolve(paths: &AppPaths, id: &str, relative: &str) -> AppResult<PathBuf> {
    AppPaths::validate_instance_id(id)?;
    let invalid = || AppError::new("config_path", crate::core::i18n::pick("配置文件路径无效。", "Invalid config file path."));
    let parts: Vec<&str> = relative.split('/').collect();
    if parts.len() < 2 || parts.len() > MAX_DEPTH + 2 || !ROOTS.contains(&parts[0]) || !editable(parts[parts.len() - 1]) {
        return Err(invalid());
    }
    let mut path = paths.instance_game_dir(id);
    for part in parts {
        if part.is_empty() || part.contains('\\') || !matches!(Path::new(part).components().next(), Some(Component::Normal(_))) || Path::new(part).components().count() != 1 {
            return Err(invalid());
        }
        path.push(part);
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(invalid());
        }
    }
    Ok(path)
}

pub fn read(paths: &AppPaths, id: &str, relative: &str) -> AppResult<String> {
    let path = resolve(paths, id, relative)?;
    let meta = fs::metadata(&path).map_err(|_| AppError::new("config_missing", crate::core::i18n::pick("找不到这个配置文件。", "Config file not found.")))?;
    if meta.len() > MAX_BYTES {
        return Err(AppError::new("config_large", crate::core::i18n::pick("这个文件超过 1 MB，不适合在这里编辑。", "This file is over 1 MB, too big to edit here.")));
    }
    String::from_utf8(fs::read(&path)?).map_err(|_| AppError::new("config_binary", crate::core::i18n::pick("这个文件不是 UTF-8 文本，不能在这里编辑。", "This file isn't UTF-8 text, so it can't be edited here.")))
}

/// Replaces a config file's text. Only existing files are edited; the old
/// text goes to `backups/<id>/config/<path>.<ms>` first.
pub fn write(paths: &AppPaths, id: &str, relative: &str, text: &str) -> AppResult<()> {
    if text.len() as u64 > MAX_BYTES {
        return Err(AppError::new("config_large", crate::core::i18n::pick("内容超过 1 MB，没有保存。", "Over 1 MB; not saved.")));
    }
    let path = resolve(paths, id, relative)?;
    let old = fs::read(&path).map_err(|_| AppError::new("config_missing", crate::core::i18n::pick("找不到这个配置文件。", "Config file not found.")))?;
    if old == text.as_bytes() {
        return Ok(());
    }
    let backup = paths.root.join("backups").join(id).join(format!("{relative}.{}", now_millis()));
    if let Some(parent) = backup.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&backup, &old)?;
    let temporary = path.with_extension(format!("{}.islet-tmp", path.extension().and_then(|e| e.to_str()).unwrap_or("")));
    fs::write(&temporary, text)?;
    fs::rename(&temporary, &path).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::instances::InstanceStore;

    #[test]
    fn config_files_are_listed_read_and_written_with_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let instance = InstanceStore::load(paths.clone()).unwrap().create("测试", "1.20.1", "release").unwrap();
        let game = paths.instance_game_dir(&instance.id);
        fs::create_dir_all(game.join("config/create")).unwrap();
        fs::write(game.join("config/create/client.toml"), "a = 1\n").unwrap();
        fs::write(game.join("config/sodium-options.json"), "{}").unwrap();
        fs::write(game.join("config/cache.bin"), [0u8, 1, 2]).unwrap();

        let files: Vec<String> = list(&paths, &instance.id).unwrap().into_iter().map(|f| f.path).collect();
        assert_eq!(files, vec!["config/create/client.toml", "config/sodium-options.json"]);
        assert_eq!(read(&paths, &instance.id, "config/create/client.toml").unwrap(), "a = 1\n");

        write(&paths, &instance.id, "config/create/client.toml", "a = 2\n").unwrap();
        assert_eq!(fs::read_to_string(game.join("config/create/client.toml")).unwrap(), "a = 2\n");
        let kept = fs::read_dir(paths.root.join("backups").join(&instance.id).join("config/create")).unwrap().count();
        assert_eq!(kept, 1);

        for bad in ["config/../options.txt", "saves/x.json", "config/cache.bin", "../config/a.toml", "config", "config/a/../../b.toml", "/etc/passwd.txt"] {
            assert!(read(&paths, &instance.id, bad).is_err(), "{bad}");
        }
        // Only existing files are edited.
        assert!(write(&paths, &instance.id, "config/new.toml", "x").is_err());
    }
}
