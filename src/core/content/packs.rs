//! Resource pack and shader pack discovery, icon reading, ordering, and hashing.
use std::{fs, path::Path};

use crate::core::{
    error::{AppError, AppResult},
    paths::AppPaths,
};

use super::{ContentKind, FileCache, WORLD_ICON_LIMIT, cached, stream_sha1, tree_size};

static PACK_HASHES: FileCache<String> = std::sync::LazyLock::new(Default::default);
static PACK_ICONS: FileCache<Option<String>> = std::sync::LazyLock::new(Default::default);

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
pub(super) fn enabled_packs(game_dir: &Path, kind: ContentKind) -> Vec<String> {
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
        ContentKind::Mod | ContentKind::Datapack | ContentKind::World => Vec::new(),
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
pub(super) fn unescape_property(value: &str) -> String {
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

/// `pack.png` from a pack folder or zip, as a data URL, cached by file stamp.
fn pack_icon(path: &Path, folder: bool) -> Option<String> {
    let target = if folder { path.join("pack.png") } else { path.to_path_buf() };
    cached(&PACK_ICONS, &target, || Ok(read_pack_icon(path, folder)))
        .ok()
        .flatten()
}

fn read_pack_icon(path: &Path, folder: bool) -> Option<String> {
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
