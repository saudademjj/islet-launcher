//! Mod JAR metadata (Fabric, Quilt, Forge, NeoForge, LiteLoader) and embedded icon reading.
use std::{fs, path::Path};

use crate::core::{error::AppResult, paths::AppPaths};

use super::{FileCache, WORLD_ICON_LIMIT, cached, mods};

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
    /// Loaders this jar declares metadata for ("fabric", "quilt", "forge", "neoforge", "liteloader").
    #[serde(default)]
    pub loaders: Vec<String>,
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
    let fabric_json = json(text(archive, "fabric.mod.json"));
    let quilt_json = json(text(archive, "quilt.mod.json"));
    let neoforge_toml = text(archive, "META-INF/neoforge.mods.toml");
    let forge_toml = text(archive, "META-INF/mods.toml");
    let mcmod_json = json(text(archive, "mcmod.info"));
    let litemod_json = json(text(archive, "litemod.json"));

    if fabric_json.is_some() {
        meta.loaders.push("fabric".into());
    }
    if quilt_json.is_some() {
        meta.loaders.push("quilt".into());
    }
    if neoforge_toml.is_some() {
        meta.loaders.push("neoforge".into());
    }
    if let Some(toml) = &forge_toml {
        for loader in toml_declared_loaders(toml) {
            meta.loaders.push(loader.into());
        }
    }
    if mcmod_json.is_some() {
        meta.loaders.push("forge".into());
    }
    if litemod_json.is_some() {
        meta.loaders.push("liteloader".into());
    }
    meta.loaders.sort();
    meta.loaders.dedup();

    if let Some(v) = &fabric_json {
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
    } else if let Some(v) = &quilt_json {
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
    } else if let Some(toml) = neoforge_toml.as_deref().or(forge_toml.as_deref()) {
        toml_meta(toml, &mut meta);
        if let Some(v) = json(text(archive, "META-INF/jarjar/metadata.json"))
            && let Some(jars) = v.get("jars").and_then(|j| j.as_array())
        {
            nested.extend(jars.iter().filter_map(|jar| jar.get("path")?.as_str().map(String::from)));
        }
    } else if let Some(v) = &mcmod_json {
        let entries = v
            .as_array()
            .or_else(|| v.get("modList").and_then(|m| m.as_array()));
        if let Some(first) = entries.and_then(|list| list.first()) {
            meta.id = string(first.get("modid"));
            meta.name = string(first.get("name"));
            meta.description = string(first.get("description"));
            meta.version = string(first.get("version")).filter(|ver| !ver.starts_with("${"));
        }
        if let Some(list) = entries {
            meta.provides
                .extend(list.iter().skip(1).filter_map(|item| string(item.get("modid"))));
        }
    } else if let Some(v) = &litemod_json {
        let name = string(v.get("name"));
        meta.id = name.as_ref().map(|n| n.to_ascii_lowercase().replace(' ', "_"));
        meta.name = name;
        meta.description = string(v.get("description"));
        meta.version = string(v.get("version")).or_else(|| string(v.get("revision")));
    }
    meta.depends.retain(|id| !PLATFORM_IDS.contains(&id.as_str()));
    meta.depends.sort();
    meta.depends.dedup();
    (meta, nested)
}

/// Distinguishes NeoForge 1.20.2–1.20.4 jars (which still use `META-INF/mods.toml`
/// with a `modId = "neoforge"` dependency) from Forge jars.
fn toml_declared_loaders(text: &str) -> Vec<&'static str> {
    let mut in_dep = false;
    let mut in_multiline = false;
    let mut mentions_forge = false;
    let mut mentions_neoforge = false;
    for line in text.lines() {
        let line = line.trim();
        if in_multiline {
            in_multiline = !(line.contains("'''") || line.contains("\"\"\""));
            continue;
        }
        if line.starts_with('[') {
            in_dep = line.starts_with("[[dependencies.");
            continue;
        }
        let Some((key, raw)) = line.split_once('=') else { continue };
        let (key, raw) = (key.trim(), raw.trim());
        if raw.starts_with("'''") || raw.starts_with("\"\"\"") {
            in_multiline = !raw[3..].contains(&raw[..3]);
            continue;
        }
        if in_dep && key == "modId"
            && let Some(value) = toml_value(raw)
        {
            if value.eq_ignore_ascii_case("neoforge") {
                mentions_neoforge = true;
            } else if value.eq_ignore_ascii_case("forge") {
                mentions_forge = true;
            }
        }
    }
    match (mentions_forge, mentions_neoforge) {
        (true, false) => vec!["forge"],
        (false, true) => vec!["neoforge"],
        _ => vec!["forge", "neoforge"],
    }
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
pub(super) fn toml_logo(text: &str) -> Option<String> {
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
