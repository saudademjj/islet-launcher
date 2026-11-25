//! Local mod, pack, and world management. No operation deletes the only copy of saves.
mod jar_meta;
mod packs;
#[cfg(test)]
mod tests;

pub use jar_meta::{ModMeta, mod_icons, mod_meta};
pub use packs::{PackFile, pack_hashes, packs, set_resource_packs};

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
    /// A map: a zipped save, unpacked into `saves`. Only CurseForge has them.
    World,
}

impl ContentKind {
    pub fn folder(self) -> &'static str {
        match self {
            Self::Mod => "mods",
            Self::ResourcePack => "resourcepacks",
            Self::Shader => "shaderpacks",
            Self::Datapack => "datapacks",
            Self::World => "saves",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Mod => ".jar",
            Self::ResourcePack | Self::Shader | Self::Datapack | Self::World => ".zip",
        }
    }

    /// Modrinth's `project_type` facet value.
    pub fn project_type(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::ResourcePack => "resourcepack",
            Self::Shader => "shader",
            Self::Datapack => "datapack",
            Self::World => "world",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mod => crate::core::i18n::pick("模组", "mod"),
            Self::ResourcePack => crate::core::i18n::pick("资源包", "resource pack"),
            Self::Shader => crate::core::i18n::pick("光影包", "shader pack"),
            Self::Datapack => crate::core::i18n::pick("数据包", "datapack"),
            Self::World => crate::core::i18n::pick("地图", "map"),
        }
    }
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
    if !unchanged
        && let Ok(encoded) = serde_json::to_vec(&fresh) {
            let _ = crate::core::util::atomic_write(&cache_path, &encoded);
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
pub(super) type FileCache<T> = std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<PathBuf, ((u64, i64), T)>>>;

pub(super) fn cached<T: Clone>(cache: &FileCache<T>, path: &Path, read: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
    let Some(stamp) = stamp(path) else {
        return read();
    };
    if let Some((seen, value)) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(path)
        && *seen == stamp {
            return Ok(value.clone());
        }
    let value = read()?;
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_path_buf(), (stamp, value.clone()));
    Ok(value)
}

fn file_sha1(data: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    hex::encode(Sha1::digest(data))
}

/// Packs can be hundreds of megabytes, so they are hashed in pieces.
pub(super) fn stream_sha1(path: &Path) -> AppResult<String> {
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
    result.sort_by_key(|a| a.name.to_lowercase());
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

pub(super) const WORLD_ICON_LIMIT: u64 = 64 * 1024;

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
