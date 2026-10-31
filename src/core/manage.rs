//! Removing and moving things inside a world. Mods, packs, saves and
//! datapacks go to the launcher's recycle folder rather than being deleted;
//! only emptying the recycle bin deletes for good. Single saves can be backed
//! up (by hand, or automatically after the game exits), restored, imported,
//! exported and renamed; the launcher's shared caches can be measured and
//! cleaned.
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

use crate::core::{
    content::{archive_tree, tree_size, zip_error},
    error::{AppError, AppResult},
    instances::Instance,
    java::runtime::safe_archive_path,
    paths::AppPaths,
    servers::nbt::{self, Tag},
    util::now_millis,
};

/// Something inside a world's game folder that can be removed on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    Mod,
    ResourcePack,
    Shader,
    Save,
    Datapack,
}

impl ItemKind {
    fn folder(self) -> &'static str {
        match self {
            ItemKind::Mod => "mods",
            ItemKind::ResourcePack => "resourcepacks",
            ItemKind::Shader => "shaderpacks",
            ItemKind::Save => "saves",
            ItemKind::Datapack => "datapacks",
        }
    }
}

/// A file or folder name that stays inside its parent: one normal component.
pub fn single_name(name: &str) -> AppResult<&str> {
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(_)), None) if !name.contains(['/', '\\', '\0']) => Ok(name),
        _ => Err(AppError::new("item_name", crate::core::i18n::pick("名称无效。", "Invalid name."))),
    }
}

/// Where an item lives. Datapacks belong to a save.
fn item_path(paths: &AppPaths, id: &str, kind: ItemKind, name: &str, save: Option<&str>) -> AppResult<PathBuf> {
    AppPaths::validate_instance_id(id)?;
    let name = single_name(name)?;
    let game = paths.instance_game_dir(id);
    let path = match kind {
        ItemKind::Datapack => {
            let save = single_name(save.ok_or_else(|| AppError::new("item_name", crate::core::i18n::pick("没有指定存档。", "No save given.")))?)?;
            game.join("saves").join(save).join("datapacks").join(name)
        }
        _ => game.join(kind.folder()).join(name),
    };
    let meta = fs::symlink_metadata(&path).map_err(|_| AppError::new("item_missing", crate::core::i18n::pick("找不到这个文件，可能已被移走。", "File not found; it may have been moved.")))?;
    if meta.file_type().is_symlink() {
        return Err(AppError::new("item_link", crate::core::i18n::pick("这是一个符号链接，请在文件夹里手动处理。", "This is a symbolic link; handle it in the folder yourself.")));
    }
    if kind == ItemKind::Save && !path.join("level.dat").is_file() {
        return Err(AppError::new("world_missing", crate::core::i18n::pick("找不到这个存档。", "Save not found.")));
    }
    Ok(path)
}

fn recycle_root(paths: &AppPaths) -> PathBuf {
    paths.root.join("recycle")
}

/// Moves one item to `recycle/content/<world>/<folder>/<time>-<name>`.
pub fn recycle_item(paths: &AppPaths, id: &str, kind: ItemKind, name: &str, save: Option<&str>) -> AppResult<PathBuf> {
    let source = item_path(paths, id, kind, name, save)?;
    let dir = recycle_root(paths).join("content").join(id).join(kind.folder());
    fs::create_dir_all(&dir)?;
    let target = dir.join(format!("{}-{name}", now_millis()));
    fs::rename(&source, &target).map_err(|error| {
        AppError::detail("item_recycle", crate::core::i18n::pick("移到回收站失败，文件可能正被游戏或其他程序占用。", "Couldn't move it to the recycle bin; the game or another program may be using it."), error.to_string())
    })?;
    // A file a modpack brought in stays gone: repairing the pack must not
    // download it again.
    if matches!(kind, ItemKind::Mod | ItemKind::ResourcePack | ItemKind::Shader) {
        let pack_path = format!("{}/{}", kind.folder(), name.trim_end_matches(".disabled"));
        crate::core::modpacks::record_replaced(paths, id, &[pack_path], &[])?;
    }
    Ok(target)
}

/// Copies local resource or shader packs (zip files or folders) in.
pub fn add_packs(paths: &AppPaths, id: &str, kind: ItemKind, sources: &[PathBuf]) -> AppResult<usize> {
    AppPaths::validate_instance_id(id)?;
    let folder = match kind {
        ItemKind::ResourcePack | ItemKind::Shader => kind.folder(),
        _ => return Err(AppError::new("pack_kind", crate::core::i18n::pick("只能添加资源包或光影。", "Only resource packs or shader packs can be added."))),
    };
    copy_into(&paths.instance_game_dir(id).join(folder), sources, &["zip"])
}

/// Copies datapacks (zip files or folders) into one save.
pub fn add_datapacks(paths: &AppPaths, id: &str, save: &str, sources: &[PathBuf]) -> AppResult<usize> {
    let world = item_path(paths, id, ItemKind::Save, save, None)?;
    copy_into(&world.join("datapacks"), sources, &["zip"])
}

/// Copies files with one of `extensions`, or folders, into `dir` without
/// replacing anything already there.
fn copy_into(dir: &Path, sources: &[PathBuf], extensions: &[&str]) -> AppResult<usize> {
    fs::create_dir_all(dir)?;
    let mut copied = 0;
    for source in sources {
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| AppError::new("item_name", crate::core::i18n::pick("文件名无效。", "Invalid file name.")))?;
        let meta = fs::symlink_metadata(source)?;
        let wanted = meta.is_dir()
            || source
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| extensions.iter().any(|want| ext.eq_ignore_ascii_case(want)));
        if !wanted || meta.file_type().is_symlink() {
            return Err(AppError::new("item_type", crate::tr!("「{name}」不是可以添加的文件。", "\"{name}\" isn't a file that can be added.")));
        }
        let target = dir.join(name);
        if target.exists() {
            return Err(AppError::new("item_exists", crate::tr!("已经有一个「{name}」，未做更改。", "There's already a \"{name}\"; nothing changed.")));
        }
        if meta.is_dir() {
            copy_tree(source, &target)?;
        } else {
            fs::copy(source, &target)?;
        }
        copied += 1;
    }
    Ok(copied)
}

fn copy_tree(source: &Path, target: &Path) -> AppResult<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let to = target.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &to)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

// ---------- datapacks ----------

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Datapack {
    pub name: String,
    pub bytes: u64,
    pub folder: bool,
    /// From `pack.mcmeta`, colour codes removed.
    pub description: Option<String>,
}

pub fn datapacks(paths: &AppPaths, id: &str, save: &str) -> AppResult<Vec<Datapack>> {
    let world = item_path(paths, id, ItemKind::Save, save, None)?;
    let Ok(entries) = fs::read_dir(world.join("datapacks")) else {
        return Ok(Vec::new());
    };
    let mut list: Vec<Datapack> = entries
        .flatten()
        .filter_map(|entry| {
            let kind = entry.file_type().ok()?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let folder = kind.is_dir();
            let zip = kind.is_file() && name.to_ascii_lowercase().ends_with(".zip");
            if !folder && !zip {
                return None;
            }
            let meta = if folder {
                fs::read(path.join("pack.mcmeta")).ok()
            } else {
                zip_entry(&path, "pack.mcmeta", 256 * 1024)
            };
            let description = meta
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|json| json.pointer("/pack/description").cloned())
                .map(|value| match value {
                    serde_json::Value::String(text) => text,
                    other => crate::core::servers::motd(&other).into_iter().map(|span| span.text).collect(),
                })
                .map(|text| crate::core::servers::strip_codes(&text).trim().to_string())
                .filter(|text| !text.is_empty());
            Some(Datapack {
                bytes: if folder { tree_size(&path) } else { entry.metadata().map(|m| m.len()).unwrap_or(0) },
                name,
                folder,
                description,
            })
        })
        .collect();
    list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(list)
}

fn zip_entry(path: &Path, name: &str, limit: u64) -> Option<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(fs::File::open(path).ok()?).ok()?;
    let entry = zip.by_name(name).ok()?;
    if entry.size() > limit {
        return None;
    }
    let mut bytes = Vec::new();
    entry.take(limit).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

// ---------- level.dat ----------

/// The world's own name from `level.dat` (`Data.LevelName`).
pub fn level_name(world: &Path) -> Option<String> {
    let (_, root) = read_level(world).ok()?;
    root.get("Data")?.get("LevelName")?.as_str().map(str::to_string)
}

fn read_level(world: &Path) -> AppResult<(String, Tag)> {
    let compressed = fs::read(world.join("level.dat"))?;
    if compressed.len() > 16 * 1024 * 1024 {
        return Err(AppError::new("level_dat", crate::core::i18n::pick("level.dat 过大。", "level.dat is too large.")));
    }
    let mut data = Vec::new();
    flate2::read::GzDecoder::new(&compressed[..])
        .take(64 * 1024 * 1024)
        .read_to_end(&mut data)
        .map_err(|error| AppError::detail("level_dat", crate::core::i18n::pick("无法读取 level.dat。", "Couldn't read level.dat."), error.to_string()))?;
    nbt::read(&data)
}

fn write_level(world: &Path, name: &str, root: &Tag) -> AppResult<()> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&nbt::write(name, root))?;
    let bytes = encoder.finish()?;
    let path = world.join("level.dat");
    let temporary = world.join("level.dat.launcher-tmp");
    fs::write(&temporary, bytes)?;
    // The game keeps the previous level.dat as level.dat_old; so do we.
    let _ = fs::copy(&path, world.join("level.dat_old"));
    fs::rename(&temporary, &path)?;
    Ok(())
}

/// Renames a save: the name shown in the game and, where it is free, the
/// folder too.
pub fn rename_save(paths: &AppPaths, id: &str, folder: &str, name: &str) -> AppResult<String> {
    let world = item_path(paths, id, ItemKind::Save, folder, None)?;
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(AppError::new("save_name", crate::core::i18n::pick("存档名需要 1–64 个字符。", "Save names need 1–64 characters.")));
    }
    let (root_name, mut root) = read_level(&world)?;
    let Tag::Compound(_) = root else {
        return Err(AppError::new("level_dat", crate::core::i18n::pick("level.dat 格式无法识别。", "level.dat's format isn't recognized.")));
    };
    let mut data = root.get("Data").cloned().ok_or_else(|| AppError::new("level_dat", crate::core::i18n::pick("level.dat 缺少 Data。", "level.dat has no Data.")))?;
    data.set("LevelName", Tag::String(name.to_string()));
    root.set("Data", data);
    write_level(&world, &root_name, &root)?;
    // The folder follows when the new name is a usable, unused folder name.
    let wanted = folder_name(name);
    let saves = world.parent().map(Path::to_path_buf).unwrap_or_default();
    if wanted != folder && !wanted.is_empty() && !saves.join(&wanted).exists() && fs::rename(&world, saves.join(&wanted)).is_ok() {
        return Ok(wanted);
    }
    Ok(folder.to_string())
}

/// A folder name the way the game makes one: characters that are not
/// allowed in file names become underscores.
fn folder_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '.') || c.is_control() { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().to_string();
    // Windows reserves a few device names.
    let upper = cleaned.to_ascii_uppercase();
    if ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "LPT1", "LPT2", "LPT3"].contains(&upper.as_str()) {
        return format!("_{cleaned}_");
    }
    cleaned
}

/// A save folder name not used yet: `name`, then `name (2)` …
fn free_folder(saves: &Path, name: &str) -> String {
    let base = folder_name(name);
    let base = if base.is_empty() { "World".to_string() } else { base };
    if !saves.join(&base).exists() {
        return base;
    }
    (2..)
        .map(|n| format!("{base} ({n})"))
        .find(|candidate| !saves.join(candidate).exists())
        .unwrap()
}

// ---------- single-save backups, import and export ----------

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveBackup {
    pub path: String,
    pub bytes: u64,
    pub created_ms: i64,
    /// Made after a game session rather than by the player; only these are
    /// rotated out.
    pub auto: bool,
}

fn save_backup_dir(paths: &AppPaths, id: &str, folder: &str) -> PathBuf {
    paths.root.join("backups").join(id).join("saves").join(folder)
}

fn zip_world(world: &Path, folder: &str, output: &Path) -> AppResult<()> {
    let temporary = output.with_extension("zip.part");
    let result = (|| -> AppResult<()> {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut zip = zip::ZipWriter::new(fs::File::create(&temporary)?);
        archive_tree(&mut zip, world, Path::new(folder))?;
        zip.finish().map_err(zip_error)?.sync_all()?;
        fs::rename(&temporary, output)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Zips one save into the launcher's backups.
pub fn backup_save(paths: &AppPaths, id: &str, folder: &str) -> AppResult<PathBuf> {
    let world = item_path(paths, id, ItemKind::Save, folder, None)?;
    let output = save_backup_dir(paths, id, folder).join(format!("{}.zip", now_millis()));
    zip_world(&world, folder, &output)?;
    Ok(output)
}

pub fn save_backups(paths: &AppPaths, id: &str, folder: &str) -> AppResult<Vec<SaveBackup>> {
    AppPaths::validate_instance_id(id)?;
    let folder = single_name(folder)?;
    let Ok(entries) = fs::read_dir(save_backup_dir(paths, id, folder)) else {
        return Ok(Vec::new());
    };
    let mut list: Vec<SaveBackup> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let stem = name.strip_suffix(".zip")?;
            let (stem, auto) = match stem.strip_suffix(".auto") {
                Some(stem) => (stem, true),
                None => (stem, false),
            };
            Some(SaveBackup {
                path: entry.path().to_string_lossy().into_owned(),
                bytes: entry.metadata().ok()?.len(),
                created_ms: stem.parse().ok()?,
                auto,
            })
        })
        .collect();
    list.sort_by(|a, b| b.created_ms.cmp(&a.created_ms));
    Ok(list)
}

/// Backs up every save the game wrote to since `since_ms` (the session's
/// start), then keeps only the newest `keep` automatic backups of each.
/// Backing up after the session costs the player no waiting, and the
/// previous session's backup is the state this one started from.
pub fn auto_backup_saves(paths: &AppPaths, id: &str, since_ms: i64, keep: usize) -> AppResult<Vec<String>> {
    AppPaths::validate_instance_id(id)?;
    let saves = paths.instance_game_dir(id).join("saves");
    let Ok(entries) = fs::read_dir(&saves) else {
        return Ok(Vec::new());
    };
    let mut done = Vec::new();
    for entry in entries.flatten() {
        let folder = entry.file_name().to_string_lossy().into_owned();
        if folder.starts_with('.') || !entry.path().join("level.dat").is_file() {
            continue;
        }
        if !written_since(&entry.path().join("level.dat"), since_ms) {
            continue;
        }
        let output = save_backup_dir(paths, id, &folder).join(format!("{}.auto.zip", now_millis()));
        zip_world(&entry.path(), &folder, &output)?;
        prune_auto_backups(paths, id, &folder, keep)?;
        done.push(folder);
    }
    Ok(done)
}

fn written_since(file: &Path, since_ms: i64) -> bool {
    let Ok(modified) = fs::metadata(file).and_then(|m| m.modified()) else {
        return false;
    };
    modified
        .duration_since(std::time::UNIX_EPOCH)
        .is_ok_and(|age| age.as_millis() as i64 >= since_ms)
}

/// Deletes all but the newest `keep` automatic backups of one save. Backups
/// the player made by hand are never touched.
fn prune_auto_backups(paths: &AppPaths, id: &str, folder: &str, keep: usize) -> AppResult<()> {
    let autos: Vec<SaveBackup> = save_backups(paths, id, folder)?.into_iter().filter(|b| b.auto).collect();
    for old in autos.iter().skip(keep.max(1)) {
        fs::remove_file(&old.path)?;
    }
    Ok(())
}

/// Exports one save as a zip the game and other launchers can import.
pub fn export_save(paths: &AppPaths, id: &str, folder: &str, destination: &Path) -> AppResult<()> {
    let world = item_path(paths, id, ItemKind::Save, folder, None)?;
    if destination.exists() {
        return Err(AppError::new("export_exists", crate::core::i18n::pick("目标位置已有同名文件。", "A file with that name is already there.")));
    }
    zip_world(&world, folder, destination)
}

/// Brings a save in from a zip (the world at its root or in one folder) or
/// from a world folder. Returns the folder it lands in.
pub fn import_save(paths: &AppPaths, id: &str, source: &Path) -> AppResult<String> {
    AppPaths::validate_instance_id(id)?;
    let saves = paths.instance_game_dir(id).join("saves");
    fs::create_dir_all(&saves)?;
    if source.is_dir() {
        if !source.join("level.dat").is_file() {
            return Err(AppError::new("save_import", crate::core::i18n::pick("这个文件夹不是存档：里面没有 level.dat。", "This folder isn't a save: it has no level.dat.")));
        }
        let name = level_name(source)
            .or_else(|| source.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let folder = free_folder(&saves, &name);
        copy_tree(source, &saves.join(&folder))?;
        return Ok(folder);
    }
    let stage = saves.join(format!(".import-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| -> AppResult<String> {
        let root = extract_world(source, &stage)?;
        let name = level_name(&root)
            .or_else(|| source.file_stem().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let folder = free_folder(&saves, &name);
        fs::rename(&root, saves.join(&folder))?;
        Ok(folder)
    })();
    let _ = fs::remove_dir_all(&stage);
    result
}

/// Unzips into `stage` and returns the folder that holds `level.dat`.
fn extract_world(source: &Path, stage: &Path) -> AppResult<PathBuf> {
    let mut zip = zip::ZipArchive::new(fs::File::open(source)?).map_err(zip_error)?;
    // Find the world root: the shallowest level.dat.
    let mut root: Option<PathBuf> = None;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(zip_error)?;
        let path = safe_archive_path(&entry.name().map_err(zip_error)?)?;
        if path.file_name().is_some_and(|n| n == "level.dat") {
            let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
            if root.as_ref().is_none_or(|r| parent.components().count() < r.components().count()) {
                root = Some(parent);
            }
        }
    }
    let root = root.ok_or_else(|| AppError::new("save_import", crate::core::i18n::pick("压缩包里没有找到存档（level.dat）。", "No save (level.dat) in the archive.")))?;
    fs::create_dir_all(stage)?;
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(zip_error)?;
        let path = safe_archive_path(&entry.name().map_err(zip_error)?)?;
        let Ok(relative) = path.strip_prefix(&root) else {
            continue;
        };
        total = total.saturating_add(entry.size());
        if total > 32 * 1024 * 1024 * 1024 || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err(AppError::new("save_import", crate::core::i18n::pick("存档超出 32 GB 或包含符号链接。", "The save exceeds 32 GB or contains symbolic links.")));
        }
        let dest = stage.join("world").join(relative);
        if entry.is_dir() {
            fs::create_dir_all(dest)?;
        } else {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            std::io::copy(&mut entry, &mut fs::File::create(dest)?)?;
        }
    }
    Ok(stage.join("world"))
}

/// Puts a backup back. The save as it is now goes to the recycle bin first.
pub fn restore_save(paths: &AppPaths, id: &str, folder: &str, backup: &Path) -> AppResult<()> {
    let folder = single_name(folder)?;
    let dir = save_backup_dir(paths, id, folder);
    if backup.parent() != Some(dir.as_path()) {
        return Err(AppError::new("save_restore", crate::core::i18n::pick("只能恢复启动器为这个存档做的备份。", "Only backups the launcher made of this save can be restored.")));
    }
    let saves = paths.instance_game_dir(id).join("saves");
    let stage = saves.join(format!(".restore-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| -> AppResult<()> {
        let root = extract_world(backup, &stage)?;
        if saves.join(folder).exists() {
            recycle_item(paths, id, ItemKind::Save, folder, None)?;
        }
        fs::rename(root, saves.join(folder))?;
        Ok(())
    })();
    let _ = fs::remove_dir_all(&stage);
    result
}

// ---------- recycle bin ----------

/// Empties the recycle bin: one recycled world, or everything (worlds,
/// removed mods, packs, saves and screenshots). This and rotating automatic
/// save backups are the only places the launcher deletes for good.
pub fn purge_recycle(paths: &AppPaths, only: Option<&str>) -> AppResult<()> {
    let root = recycle_root(paths);
    match only {
        Some(id) => {
            AppPaths::validate_instance_id(id)?;
            let dir = root.join(id);
            if dir.is_dir() {
                fs::remove_dir_all(&dir)?;
            }
            let content = root.join("content").join(id);
            if content.is_dir() {
                fs::remove_dir_all(content)?;
            }
        }
        None => {
            if root.is_dir() {
                fs::remove_dir_all(&root)?;
            }
        }
    }
    Ok(())
}

// ---------- storage ----------

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaUsage {
    pub path: String,
    pub major: u32,
    pub bytes: u64,
    /// No world needs this Java major any more.
    pub unused: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    pub root: String,
    pub worlds: u64,
    pub libraries: u64,
    pub assets: u64,
    pub versions: u64,
    /// Version caches no world (including recycled ones) uses.
    pub unused_versions: Vec<String>,
    pub unused_versions_bytes: u64,
    pub java: Vec<JavaUsage>,
    pub backups: u64,
    pub recycle: u64,
    pub thumbnails: u64,
    pub logs: u64,
}

/// What everything takes on disk. `needs` maps each world's Minecraft
/// version to the Java major it needs, when known.
pub fn usage(paths: &AppPaths, worlds: &[Instance], recycled: &[Instance], java_needs: &HashSet<u32>) -> StorageUsage {
    let used_versions: HashSet<&str> = worlds.iter().chain(recycled).map(|i| i.version_id.as_str()).collect();
    let mut unused_versions = Vec::new();
    let mut unused_versions_bytes = 0;
    if let Ok(entries) = fs::read_dir(&paths.versions) {
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !used_versions.contains(name.as_str()) {
                unused_versions_bytes += tree_size(&entry.path());
                unused_versions.push(name);
            }
        }
    }
    unused_versions.sort();
    StorageUsage {
        root: paths.root.to_string_lossy().into_owned(),
        worlds: tree_size(&paths.instances),
        libraries: tree_size(&paths.libraries),
        assets: tree_size(&paths.assets),
        versions: tree_size(&paths.versions),
        unused_versions,
        unused_versions_bytes,
        java: java_runtimes(paths, java_needs),
        backups: tree_size(&paths.root.join("backups")),
        recycle: tree_size(&recycle_root(paths)),
        thumbnails: tree_size(&paths.cache.join("thumbnails")),
        logs: tree_size(&paths.logs),
    }
}

/// Java runtimes the launcher downloaded: `java/<vendor>/<major>/<arch>`.
fn java_runtimes(paths: &AppPaths, needs: &HashSet<u32>) -> Vec<JavaUsage> {
    let mut list = Vec::new();
    for vendor in fs::read_dir(&paths.java).into_iter().flatten().flatten() {
        for major in fs::read_dir(vendor.path()).into_iter().flatten().flatten() {
            let Ok(number) = major.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            for arch in fs::read_dir(major.path()).into_iter().flatten().flatten() {
                if !arch.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                list.push(JavaUsage {
                    path: arch.path().to_string_lossy().into_owned(),
                    major: number,
                    bytes: tree_size(&arch.path()),
                    unused: !needs.contains(&number),
                });
            }
        }
    }
    list.sort_by(|a, b| b.major.cmp(&a.major));
    list
}

/// What a cleanup removes. Everything here can be downloaded or rebuilt.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Cleanup {
    Thumbnails,
    UnusedVersions,
    Java { path: String },
    OldLogs,
}

pub fn clean(paths: &AppPaths, worlds: &[Instance], recycled: &[Instance], what: &Cleanup) -> AppResult<u64> {
    let removed = match what {
        Cleanup::Thumbnails => remove_measured(&paths.cache.join("thumbnails"))?,
        Cleanup::UnusedVersions => {
            let usage_now = usage(paths, worlds, recycled, &HashSet::new());
            let mut total = 0;
            for name in usage_now.unused_versions {
                total += remove_measured(&paths.version_cache_dir(&name))?;
            }
            total
        }
        Cleanup::Java { path } => {
            let path = PathBuf::from(path);
            // Only a runtime folder the launcher manages: java/<vendor>/<major>/<arch>.
            let inside = path
                .strip_prefix(&paths.java)
                .ok()
                .filter(|relative| relative.components().count() == 3 && relative.components().all(|c| matches!(c, Component::Normal(_))));
            if inside.is_none() {
                return Err(AppError::new("clean_java", crate::core::i18n::pick("只能删除启动器下载的 Java。", "Only Java the launcher downloaded can be deleted.")));
            }
            remove_measured(&path)?
        }
        Cleanup::OldLogs => {
            // Launcher logs older than a week; the current one stays.
            let cutoff = now_millis() - 7 * 24 * 3600 * 1000;
            let mut total = 0;
            for entry in fs::read_dir(&paths.logs).into_iter().flatten().flatten() {
                let meta = entry.metadata()?;
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(i64::MAX);
                if meta.is_file() && modified < cutoff {
                    total += meta.len();
                    fs::remove_file(entry.path())?;
                }
            }
            total
        }
    };
    Ok(removed)
}

fn remove_measured(path: &Path) -> AppResult<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let bytes = tree_size(path);
    fs::remove_dir_all(path)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::instances::InstanceStore;

    fn level_dat(name: &str) -> Vec<u8> {
        let root = Tag::Compound(vec![(
            "Data".into(),
            Tag::Compound(vec![("LevelName".into(), Tag::String(name.into())), ("version".into(), Tag::Int(19133))]),
        )]);
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&nbt::write("", &root)).unwrap();
        encoder.finish().unwrap()
    }

    fn world() -> (tempfile::TempDir, AppPaths, Instance) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let store = InstanceStore::load(paths.clone()).unwrap();
        let instance = store.create("测试", "1.21", "release").unwrap();
        (dir, paths, instance)
    }

    #[test]
    fn items_go_to_the_recycle_bin_and_names_cannot_escape() {
        let (_dir, paths, instance) = world();
        let mods = paths.instance_game_dir(&instance.id).join("mods");
        fs::create_dir_all(&mods).unwrap();
        fs::write(mods.join("a.jar"), b"jar").unwrap();
        let moved = recycle_item(&paths, &instance.id, ItemKind::Mod, "a.jar", None).unwrap();
        assert!(!mods.join("a.jar").exists());
        assert_eq!(fs::read(moved).unwrap(), b"jar");
        for bad in ["../a.jar", "a/b.jar", "", ".."] {
            assert!(recycle_item(&paths, &instance.id, ItemKind::Mod, bad, None).is_err(), "{bad}");
        }
        purge_recycle(&paths, None).unwrap();
        assert!(!paths.root.join("recycle").exists());
    }

    #[test]
    fn a_save_is_renamed_in_level_dat_and_on_disk() {
        let (_dir, paths, instance) = world();
        let world = paths.instance_game_dir(&instance.id).join("saves/old");
        fs::create_dir_all(&world).unwrap();
        fs::write(world.join("level.dat"), level_dat("Old")).unwrap();
        let folder = rename_save(&paths, &instance.id, "old", "新世界: 2").unwrap();
        assert_eq!(folder, "新世界_ 2");
        let renamed = paths.instance_game_dir(&instance.id).join("saves").join(&folder);
        assert_eq!(level_name(&renamed).as_deref(), Some("新世界: 2"));
        assert!(renamed.join("level.dat_old").is_file());
    }

    #[test]
    fn saves_round_trip_through_backup_export_and_import() {
        let (dir, paths, instance) = world();
        let world = paths.instance_game_dir(&instance.id).join("saves/home");
        fs::create_dir_all(world.join("region")).unwrap();
        // Named apart from its folder: macOS folders ignore case.
        fs::write(world.join("level.dat"), level_dat("Base")).unwrap();
        fs::write(world.join("region/r.0.0.mca"), b"chunks").unwrap();

        let backup = backup_save(&paths, &instance.id, "home").unwrap();
        assert_eq!(save_backups(&paths, &instance.id, "home").unwrap().len(), 1);
        fs::write(world.join("region/r.0.0.mca"), b"changed").unwrap();
        restore_save(&paths, &instance.id, "home", &backup).unwrap();
        assert_eq!(fs::read(world.join("region/r.0.0.mca")).unwrap(), b"chunks");
        // A backup from elsewhere is refused.
        assert!(restore_save(&paths, &instance.id, "home", &dir.path().join("x.zip")).is_err());

        let exported = dir.path().join("home.zip");
        export_save(&paths, &instance.id, "home", &exported).unwrap();
        let folder = import_save(&paths, &instance.id, &exported).unwrap();
        assert_eq!(folder, "Base");
        let again = import_save(&paths, &instance.id, &exported).unwrap();
        assert_eq!(again, "Base (2)");
        assert!(paths.instance_game_dir(&instance.id).join("saves/Base (2)/region/r.0.0.mca").is_file());
    }

    #[test]
    fn played_saves_are_backed_up_and_only_automatic_backups_rotate() {
        let (_dir, paths, instance) = world();
        let saves = paths.instance_game_dir(&instance.id).join("saves");
        for name in ["played", "untouched"] {
            fs::create_dir_all(saves.join(name)).unwrap();
            fs::write(saves.join(name).join("level.dat"), level_dat(name)).unwrap();
        }
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        fs::File::options().write(true).open(saves.join("untouched/level.dat")).unwrap().set_modified(old).unwrap();
        let session = now_millis() - 60_000;

        backup_save(&paths, &instance.id, "played").unwrap();
        for round in 0..4 {
            let done = auto_backup_saves(&paths, &instance.id, session, 2).unwrap();
            assert_eq!(done, vec!["played".to_string()], "round {round}");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let backups = save_backups(&paths, &instance.id, "played").unwrap();
        assert_eq!(backups.iter().filter(|b| b.auto).count(), 2);
        assert_eq!(backups.iter().filter(|b| !b.auto).count(), 1);
        assert!(save_backups(&paths, &instance.id, "untouched").unwrap().is_empty());
    }

    #[test]
    fn datapacks_list_their_description_and_can_be_added() {
        let (dir, paths, instance) = world();
        let world = paths.instance_game_dir(&instance.id).join("saves/w");
        fs::create_dir_all(&world).unwrap();
        fs::write(world.join("level.dat"), level_dat("W")).unwrap();
        let pack = dir.path().join("tools");
        fs::create_dir_all(&pack).unwrap();
        fs::write(pack.join("pack.mcmeta"), r#"{"pack":{"pack_format":48,"description":"§a好用的工具"}}"#).unwrap();
        assert_eq!(add_datapacks(&paths, &instance.id, "w", &[pack.clone()]).unwrap(), 1);
        let list = datapacks(&paths, &instance.id, "w").unwrap();
        assert_eq!(list[0].name, "tools");
        assert_eq!(list[0].description.as_deref(), Some("好用的工具"));
        // Adding the same one again changes nothing.
        assert!(add_datapacks(&paths, &instance.id, "w", &[pack]).is_err());
    }

    #[test]
    fn cleanup_keeps_versions_in_use_and_only_managed_java() {
        let (_dir, paths, instance) = world();
        fs::create_dir_all(paths.version_cache_dir("1.21")).unwrap();
        fs::create_dir_all(paths.version_cache_dir("1.8.9")).unwrap();
        fs::write(paths.version_cache_dir("1.8.9").join("client.jar"), b"jar").unwrap();
        let used = usage(&paths, std::slice::from_ref(&instance), &[], &HashSet::new());
        assert_eq!(used.unused_versions, vec!["1.8.9".to_string()]);
        assert_eq!(clean(&paths, &[instance.clone()], &[], &Cleanup::UnusedVersions).unwrap(), 3);
        assert!(paths.version_cache_dir("1.21").is_dir());
        let outside = Cleanup::Java { path: paths.root.to_string_lossy().into_owned() };
        assert!(clean(&paths, &[instance], &[], &outside).is_err());
    }
}
