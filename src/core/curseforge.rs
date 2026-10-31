//! CurseForge: search, versions and installs for mods, resource packs and
//! shaders, and the file list of CurseForge modpacks. Its API needs a key
//! (free from console.curseforge.com), set in the launcher's settings.
//! Results come back in the same shapes as Modrinth's, with ids prefixed
//! `cf:` so the rest of the launcher can tell the two apart.
use std::{collections::{BTreeMap, HashMap}, path::Path, sync::Arc, time::Duration};

use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::core::{
    content::ContentKind,
    download::{self, DownloadProgress, DownloadTask},
    error::{AppError, AppResult},
    instances::Instance,
    modrinth::{Dependency, ModFile, ModVersion, Project, SearchResults},
    paths::AppPaths,
};

const API: &str = "https://api.curseforge.com/v1";
const MINECRAFT: u32 = 432;
pub const PREFIX: &str = "cf:";

fn class_id(kind: ContentKind) -> u32 {
    match kind {
        ContentKind::Mod => 6,
        ContentKind::ResourcePack => 12,
        ContentKind::Shader => 6552,
        ContentKind::Datapack => 6945,
        ContentKind::World => 17,
    }
}

fn kind_of_class(class: u32) -> Option<ContentKind> {
    match class {
        6 => Some(ContentKind::Mod),
        12 => Some(ContentKind::ResourcePack),
        6552 => Some(ContentKind::Shader),
        6945 => Some(ContentKind::Datapack),
        17 => Some(ContentKind::World),
        _ => None,
    }
}

/// CurseForge's numbers for loaders.
fn loader_type(loader: &str) -> Option<u32> {
    match loader {
        "forge" | "cleanroom" => Some(1),
        "fabric" => Some(4),
        "liteloader" => Some(3),
        "quilt" => Some(5),
        "neoforge" => Some(6),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
struct Page<T> {
    data: T,
    #[serde(default)]
    pagination: Option<Pagination>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    total_count: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfMod {
    id: u64,
    name: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    download_count: f64,
    #[serde(default)]
    logo: Option<Logo>,
    #[serde(default)]
    categories: Vec<Category>,
    #[serde(default)]
    class_id: Option<u32>,
    /// The newest file per game version and loader.
    #[serde(default)]
    latest_files_indexes: Vec<FileIndex>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileIndex {
    game_version: String,
    file_id: u64,
    filename: String,
    #[serde(default)]
    release_type: u32,
    #[serde(default)]
    mod_loader: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Logo {
    #[serde(default)]
    thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Category {
    #[serde(default)]
    slug: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfFile {
    pub id: u64,
    pub mod_id: u64,
    #[serde(default)]
    pub display_name: String,
    pub file_name: String,
    #[serde(default)]
    pub release_type: u32,
    #[serde(default)]
    pub file_date: String,
    #[serde(default)]
    pub file_length: u64,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub hashes: Vec<Hash>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<CfDependency>,
    #[serde(default)]
    pub file_fingerprint: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Hash {
    pub value: String,
    pub algo: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfDependency {
    pub mod_id: u64,
    pub relation_type: u32,
}

impl CfFile {
    pub fn sha1(&self) -> Option<String> {
        self.hashes
            .iter()
            .find(|hash| hash.algo == 1 && hash.value.len() == 40 && hash.value.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(|hash| hash.value.to_ascii_lowercase())
    }

    /// The download address. Projects that turned off third-party downloads
    /// have none in the API; their files still live at the CDN path.
    pub fn urls(&self) -> Vec<String> {
        let encoded: String = url::form_urlencoded::byte_serialize(self.file_name.as_bytes()).collect::<String>().replace('+', "%20");
        let path = format!("files/{}/{}/{encoded}", self.id / 1000, self.id % 1000);
        let mut urls = Vec::new();
        if let Some(url) = self.download_url.as_ref().filter(|url| url.starts_with("https://")) {
            urls.push(url.clone());
        }
        urls.push(format!("https://edge.forgecdn.net/{path}"));
        urls.push(format!("https://mediafilez.forgecdn.net/{path}"));
        urls.dedup();
        urls
    }

    fn fits(&self, instance: &Instance, kind: ContentKind) -> bool {
        let has = |name: &str| self.game_versions.iter().any(|v| v.eq_ignore_ascii_case(name));
        has(&instance.version_id)
            && (kind != ContentKind::Mod
                || crate::core::modrinth::mod_loaders(&instance.loader).iter().any(|loader| {
                    has(match *loader {
                        "neoforge" => "NeoForge",
                        "forge" => "Forge",
                        "fabric" => "Fabric",
                        "quilt" => "Quilt",
                        other => other,
                    })
                }))
    }

    fn version(&self) -> ModVersion {
        let loaders = ["Forge", "NeoForge", "Fabric", "Quilt", "LiteLoader"]
            .iter()
            .filter(|name| self.game_versions.iter().any(|v| v.eq_ignore_ascii_case(name)))
            .map(|name| name.to_lowercase())
            .collect();
        ModVersion {
            id: format!("{PREFIX}{}:{}", self.mod_id, self.id),
            project_id: format!("{PREFIX}{}", self.mod_id),
            name: self.display_name.clone(),
            version_number: if self.display_name.is_empty() { self.file_name.clone() } else { self.display_name.clone() },
            game_versions: self.game_versions.iter().filter(|v| v.chars().next().is_some_and(|c| c.is_ascii_digit())).cloned().collect(),
            loaders,
            files: vec![ModFile {
                url: self.urls().into_iter().next().unwrap_or_default(),
                filename: self.file_name.clone(),
                primary: true,
                size: self.file_length,
                hashes: self.sha1().map(|h| BTreeMap::from([("sha1".to_string(), h)])).unwrap_or_default(),
            }],
            dependencies: self
                .dependencies
                .iter()
                .filter(|d| d.relation_type == 3)
                .map(|d| Dependency { version_id: None, project_id: Some(format!("{PREFIX}{}", d.mod_id)), dependency_type: "required".into() })
                .collect(),
            version_type: Some(match self.release_type {
                2 => "beta",
                3 => "alpha",
                _ => "release",
            }
            .into()),
            date_published: Some(self.file_date.clone()),
        }
    }
}

fn require_key(key: &str) -> AppResult<&str> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::new("curseforge_key", crate::core::i18n::pick("使用 CurseForge 需要先在「设置 → 下载」里填写 CurseForge API Key。", "To use CurseForge, enter a CurseForge API key under Settings → Downloads first.")));
    }
    if key.len() > 200 || !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(AppError::new("curseforge_key", crate::core::i18n::pick("CurseForge API Key 格式不对。", "The CurseForge API key looks wrong.")));
    }
    Ok(key)
}

async fn get<T: serde::de::DeserializeOwned>(client: &reqwest::Client, key: &str, path: &str, query: &[(&str, String)]) -> AppResult<T> {
    let response = client
        .get(format!("{API}{path}"))
        .header("x-api-key", require_key(key)?)
        .query(query)
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    check(response).await
}

async fn post<T: serde::de::DeserializeOwned>(client: &reqwest::Client, key: &str, path: &str, body: &serde_json::Value) -> AppResult<T> {
    let response = client
        .post(format!("{API}{path}"))
        .header("x-api-key", require_key(key)?)
        .json(body)
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    check(response).await
}

async fn check<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> AppResult<T> {
    if response.status() == reqwest::StatusCode::FORBIDDEN || response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(AppError::new("curseforge_key", crate::core::i18n::pick("CurseForge 拒绝了这个 API Key，请在设置里检查。", "CurseForge rejected the API key; check it in Settings.")));
    }
    Ok(response.error_for_status()?.json().await?)
}

/// Parses `cf:<mod>` or `cf:<mod>:<file>`.
pub fn parse_id(id: &str) -> Option<(u64, Option<u64>)> {
    let rest = id.strip_prefix(PREFIX)?;
    let mut parts = rest.split(':');
    let project = parts.next()?.parse().ok()?;
    let file = match parts.next() {
        Some(file) => Some(file.parse().ok()?),
        None => None,
    };
    parts.next().is_none().then_some((project, file))
}

pub async fn search(
    client: &reqwest::Client,
    key: &str,
    instance: &Instance,
    query: &str,
    offset: usize,
    kind: ContentKind,
    sort: &str,
) -> AppResult<SearchResults> {
    if query.chars().count() > 100 {
        return Err(AppError::new("curseforge", crate::core::i18n::pick("搜索词太长。", "The search is too long.")));
    }
    let mut params = vec![
        ("gameId", MINECRAFT.to_string()),
        ("classId", class_id(kind).to_string()),
        ("gameVersion", instance.version_id.clone()),
        ("searchFilter", query.to_string()),
        ("index", offset.min(9_990).to_string()),
        ("pageSize", "10".into()),
        ("sortOrder", "desc".into()),
        (
            "sortField",
            match sort {
                "downloads" => "6",
                "updated" => "3",
                "newest" => "11",
                _ => "2",
            }
            .into(),
        ),
    ];
    if kind == ContentKind::Mod {
        let loader = loader_type(&instance.loader)
            .ok_or_else(|| AppError::new("curseforge", crate::core::i18n::pick("请先在常规设置选择并保存 Fabric、Quilt、Forge 或 NeoForge。", "Choose and save Fabric, Quilt, Forge or NeoForge under General first.")))?;
        params.push(("modLoaderType", loader.to_string()));
    }
    let page: Page<Vec<CfMod>> = get(client, key, "/mods/search", &params).await?;
    Ok(SearchResults {
        total_hits: page.pagination.map(|p| p.total_count.min(10_000)).unwrap_or(0),
        hits: page.data.into_iter().map(project).collect(),
    })
}

fn project(item: CfMod) -> Project {
    Project {
        project_id: format!("{PREFIX}{}", item.id),
        slug: Some(item.slug),
        title: item.name,
        description: item.summary,
        downloads: item.download_count.max(0.0) as u64,
        icon_url: item.logo.and_then(|logo| logo.thumbnail_url).filter(|url| url.starts_with("https://media.forgecdn.net/")),
        categories: item.categories.into_iter().map(|c| c.slug).collect(),
    }
}

pub async fn details(client: &reqwest::Client, key: &str, project: &str) -> AppResult<crate::core::modrinth::ProjectDetails> {
    use crate::core::modrinth::{GalleryImage, ProjectDetails, clip, https};
    let (mod_id, _) = parse_id(project).ok_or_else(|| AppError::new("curseforge", crate::core::i18n::pick("项目编号无效。", "Invalid project id.")))?;
    #[derive(Deserialize, Default)]
    #[serde(rename_all = "camelCase")]
    struct Links {
        website_url: Option<String>,
        wiki_url: Option<String>,
        issues_url: Option<String>,
        source_url: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Raw {
        #[serde(default)]
        links: Links,
        #[serde(default)]
        screenshots: Vec<Shot>,
        date_modified: Option<String>,
    }
    #[derive(Deserialize)]
    struct Shot {
        url: String,
        title: Option<String>,
    }
    let info: Page<Raw> = get(client, key, &format!("/mods/{mod_id}"), &[]).await?;
    let body: Page<String> = get(client, key, &format!("/mods/{mod_id}/description"), &[]).await?;
    let raw = info.data;
    let links = [("page", raw.links.website_url), ("source", raw.links.source_url), ("wiki", raw.links.wiki_url), ("issues", raw.links.issues_url)]
        .into_iter()
        .filter_map(|(kind, url)| https(url).map(|url| (kind, url)))
        .collect();
    Ok(ProjectDetails {
        body: clip(body.data),
        format: "html",
        gallery: raw
            .screenshots
            .into_iter()
            .filter_map(|shot| https(Some(shot.url)).map(|url| GalleryImage { url, title: shot.title }))
            .take(24)
            .collect(),
        links,
        license: None,
        updated: raw.date_modified,
    })
}

/// CurseForge's file fingerprint: 32-bit MurmurHash2 (seed 1) of the file
/// with tabs, newlines, carriage returns and spaces left out.
pub fn fingerprint(data: &[u8]) -> u32 {
    const M: u32 = 0x5bd1_e995;
    let bytes: Vec<u8> = data.iter().copied().filter(|b| !matches!(b, 9 | 10 | 13 | 32)).collect();
    let mut hash = 1 ^ bytes.len() as u32;
    let mut chunks = bytes.chunks_exact(4);
    for chunk in &mut chunks {
        let mut k = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]).wrapping_mul(M);
        k ^= k >> 24;
        hash = hash.wrapping_mul(M) ^ k.wrapping_mul(M);
    }
    let rest = chunks.remainder();
    if !rest.is_empty() {
        for (i, byte) in rest.iter().enumerate() {
            hash ^= (*byte as u32) << (8 * i);
        }
        hash = hash.wrapping_mul(M);
    }
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(M);
    hash ^ (hash >> 15)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Matches {
    #[serde(default)]
    exact_matches: Vec<Match>,
}

#[derive(Debug, Deserialize)]
struct Match {
    id: u64,
    file: CfFile,
}

/// Local mod files CurseForge recognises: file name → (mod, file).
async fn match_files(client: &reqwest::Client, key: &str, paths: &AppPaths, instance: &Instance, only: Option<&std::collections::HashSet<String>>) -> AppResult<Vec<(String, u64, CfFile)>> {
    let local = {
        let paths = paths.clone();
        let id = instance.id.clone();
        tokio::task::spawn_blocking(move || crate::core::content::mod_fingerprints(&paths, &id))
            .await
            .map_err(|error| AppError::detail("curseforge", crate::core::i18n::pick("计算模组校验值失败。", "Couldn't hash the mods."), error.to_string()))??
    };
    let local: Vec<(String, u32)> = local.into_iter().filter(|(name, _)| only.is_none_or(|only| only.contains(name))).collect();
    if local.is_empty() {
        return Ok(Vec::new());
    }
    let prints: Vec<u32> = local.iter().map(|(_, print)| *print).collect();
    let page: Page<Matches> = post(client, key, &format!("/fingerprints/{MINECRAFT}"), &serde_json::json!({ "fingerprints": prints })).await?;
    let mut found = Vec::new();
    for found_match in page.data.exact_matches {
        let print = found_match.file.file_fingerprint;
        for (name, _) in local.iter().filter(|(_, local)| Some(*local) == print) {
            found.push((name.clone(), found_match.id, found_match.file.clone()));
        }
    }
    Ok(found)
}

/// Names, icons and updates for local mods Modrinth does not know, from
/// CurseForge. `only` lists the file names still unidentified.
pub async fn inspect(client: &reqwest::Client, key: &str, paths: &AppPaths, instance: &Instance, only: &std::collections::HashSet<String>) -> AppResult<Vec<crate::core::modrinth::ModInsight>> {
    use crate::core::modrinth::{ModInsight, ModUpdate};
    let found = match_files(client, key, paths, instance, Some(only)).await?;
    if found.is_empty() {
        return Ok(Vec::new());
    }
    let mut ids: Vec<u64> = found.iter().map(|(_, id, _)| *id).collect();
    ids.sort_unstable();
    ids.dedup();
    let mods: HashMap<u64, CfMod> = mods_by_id(client, key, &ids).await?.into_iter().map(|m| (m.id, m)).collect();
    let loader = loader_type(&instance.loader);
    Ok(found
        .into_iter()
        .map(|(name, mod_id, file)| {
            let info = mods.get(&mod_id);
            let newest = info.and_then(|info| {
                let fitting: Vec<&FileIndex> = info
                    .latest_files_indexes
                    .iter()
                    .filter(|index| index.game_version == instance.version_id && (index.mod_loader.is_none() || index.mod_loader == loader))
                    .collect();
                // Stay on releases unless only betas exist.
                let releases: Vec<&&FileIndex> = fitting.iter().filter(|index| index.release_type == 1).collect();
                let pool: Vec<&FileIndex> = if releases.is_empty() { fitting.clone() } else { releases.into_iter().copied().collect() };
                pool.into_iter().max_by_key(|index| index.file_id)
            });
            ModInsight {
                file: name,
                project_id: format!("{PREFIX}{mod_id}"),
                slug: info.map(|info| info.slug.clone()).filter(|slug| !slug.is_empty()),
                title: info.map_or_else(|| file.display_name.clone(), |info| info.name.clone()),
                icon_url: info.and_then(|info| info.logo.as_ref()?.thumbnail_url.clone()).filter(|url| url.starts_with("https://media.forgecdn.net/")),
                version_id: format!("{PREFIX}{mod_id}:{}", file.id),
                version_number: if file.display_name.is_empty() { file.file_name.clone() } else { file.display_name.clone() },
                update: newest.filter(|index| index.file_id > file.id).map(|index| ModUpdate {
                    version_id: format!("{PREFIX}{mod_id}:{}", index.file_id),
                    version_number: index.filename.trim_end_matches(".jar").to_string(),
                }),
            }
        })
        .collect())
}

/// CurseForge's modpack category for one of Modrinth's tag names; Modrinth's
/// "optimization" has no match and is left out.
fn modpack_category(tag: &str) -> Option<u32> {
    Some(match tag {
        "adventure" => 4475,
        "technology" => 4472,
        "magic" => 4473,
        "quests" => 4478,
        "combat" => 4483,
        "multiplayer" => 4484,
        "lightweight" => 4481,
        "kitchen-sink" => 4482,
        "challenging" => 4479,
        "skyblock" => 4736,
        _ => return None,
    })
}

/// Modpacks (class 4471), optionally for one Minecraft version, loader and
/// category.
#[allow(clippy::too_many_arguments)]
pub async fn search_modpacks(client: &reqwest::Client, key: &str, query: &str, offset: usize, game_version: Option<&str>, loader: Option<&str>, category: Option<&str>, sort: &str) -> AppResult<SearchResults> {
    if query.chars().count() > 100 {
        return Err(AppError::new("curseforge", crate::core::i18n::pick("搜索词太长。", "The search is too long.")));
    }
    let mut params = vec![
        ("gameId", MINECRAFT.to_string()),
        ("classId", "4471".to_string()),
        ("searchFilter", query.to_string()),
        ("index", offset.min(9_980).to_string()),
        ("pageSize", "20".into()),
        ("sortOrder", "desc".into()),
        (
            "sortField",
            match sort {
                "downloads" => "6",
                "updated" => "3",
                "newest" => "11",
                _ => "2",
            }
            .into(),
        ),
    ];
    if let Some(version) = game_version.filter(|v| crate::core::fabric::valid_version(v)) {
        params.push(("gameVersion", version.to_string()));
    }
    if let Some(loader) = loader.and_then(loader_type) {
        params.push(("modLoaderType", loader.to_string()));
    }
    if let Some(category) = category.and_then(modpack_category) {
        params.push(("categoryId", category.to_string()));
    }
    let page: Page<Vec<CfMod>> = get(client, key, "/mods/search", &params).await?;
    Ok(SearchResults {
        total_hits: page.pagination.map(|p| p.total_count.min(10_000)).unwrap_or(0),
        hits: page.data.into_iter().map(project).collect(),
    })
}

/// The files of a modpack, newest first.
pub async fn modpack_versions(client: &reqwest::Client, key: &str, project: &str) -> AppResult<Vec<ModVersion>> {
    let (mod_id, _) = parse_id(project).ok_or_else(|| AppError::new("curseforge", crate::core::i18n::pick("项目编号无效。", "Invalid project id.")))?;
    let page: Page<Vec<CfFile>> = get(client, key, &format!("/mods/{mod_id}/files"), &[("pageSize", "50".to_string())]).await?;
    let mut files = page.data;
    files.sort_by(|a, b| b.file_date.cmp(&a.file_date));
    Ok(files.iter().map(CfFile::version).collect())
}

/// A modpack file's address, name and SHA-1.
pub async fn modpack_file(client: &reqwest::Client, key: &str, version: &str) -> AppResult<(Vec<String>, String, Option<String>)> {
    let Some((_, Some(file))) = parse_id(version) else {
        return Err(AppError::new("curseforge", crate::core::i18n::pick("版本编号无效。", "Invalid version id.")));
    };
    let file = files_by_id(client, key, &[file])
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::new("curseforge", crate::core::i18n::pick("找不到这个整合包文件。", "Modpack file not found.")))?;
    if !file.file_name.to_ascii_lowercase().ends_with(".zip") {
        return Err(AppError::new("curseforge", crate::core::i18n::pick("这个文件不是整合包。", "This file isn't a modpack.")));
    }
    Ok((file.urls(), file.file_name.clone(), file.sha1()))
}

/// Files of a project that fit the world, newest first.
pub async fn versions(client: &reqwest::Client, key: &str, instance: &Instance, project: &str, kind: ContentKind) -> AppResult<Vec<ModVersion>> {
    let (mod_id, _) = parse_id(project).ok_or_else(|| AppError::new("curseforge", crate::core::i18n::pick("项目编号无效。", "Invalid project id.")))?;
    Ok(files_for(client, key, instance, mod_id, kind).await?.iter().map(CfFile::version).collect())
}

async fn files_for(client: &reqwest::Client, key: &str, instance: &Instance, mod_id: u64, kind: ContentKind) -> AppResult<Vec<CfFile>> {
    let mut params = vec![("gameVersion", instance.version_id.clone()), ("pageSize", "50".to_string())];
    if kind == ContentKind::Mod {
        if let Some(loader) = loader_type(&instance.loader) {
            params.push(("modLoaderType", loader.to_string()));
        }
    }
    let page: Page<Vec<CfFile>> = get(client, key, &format!("/mods/{mod_id}/files"), &params).await?;
    let mut files: Vec<CfFile> = page.data.into_iter().filter(|file| file.fits(instance, kind)).collect();
    files.sort_by(|a, b| b.file_date.cmp(&a.file_date));
    Ok(files)
}

async fn files_by_id(client: &reqwest::Client, key: &str, ids: &[u64]) -> AppResult<Vec<CfFile>> {
    let mut result = Vec::new();
    for chunk in ids.chunks(500) {
        let page: Page<Vec<CfFile>> = post(client, key, "/mods/files", &serde_json::json!({ "fileIds": chunk })).await?;
        result.extend(page.data);
    }
    Ok(result)
}

async fn mods_by_id(client: &reqwest::Client, key: &str, ids: &[u64]) -> AppResult<Vec<CfMod>> {
    let mut result = Vec::new();
    for chunk in ids.chunks(500) {
        let page: Page<Vec<CfMod>> = post(client, key, "/mods", &serde_json::json!({ "modIds": chunk })).await?;
        result.extend(page.data);
    }
    Ok(result)
}

fn safe_file_name(name: &str, kind: ContentKind) -> AppResult<()> {
    let ok = !name.is_empty()
        && name.len() <= 200
        && !name.contains(['/', '\\', ':', '\0'])
        && name != "."
        && name != ".."
        && (name.to_lowercase().ends_with(kind.extension()) || (kind == ContentKind::Mod && name.to_lowercase().ends_with(".litemod")));
    if ok { Ok(()) } else { Err(AppError::new("curseforge", crate::tr!("{}文件名无效：{name}", "Invalid {} file name: {name}", kind.label()))) }
}

/// Installs chosen files (`cf:<mod>:<file>`) and, for mods, their required
/// dependencies (newest file that fits). Files already present are left.
pub async fn install(
    client: &reqwest::Client,
    key: &str,
    paths: &AppPaths,
    instance: &Instance,
    ids: &[String],
    kind: ContentKind,
    save: Option<&str>,
    cancel: &CancellationToken,
) -> AppResult<String> {
    let mut file_ids = Vec::new();
    for id in ids {
        match parse_id(id) {
            Some((_, Some(file))) => file_ids.push(file),
            _ => return Err(AppError::new("curseforge", crate::core::i18n::pick("版本编号无效。", "Invalid version id."))),
        }
    }
    let mut chosen = files_by_id(client, key, &file_ids).await?;
    if chosen.iter().any(|file| !file.fits(instance, kind)) {
        return Err(AppError::new("curseforge", crate::tr!("这个{}版本不适配 {}。", "This {} version doesn't fit {}.", kind.label(), instance.version_id)));
    }
    // Mods already in the folder, by project: a dependency already there is
    // not added again, and an updated mod replaces its older file.
    let present: HashMap<u64, Vec<String>> = if kind == ContentKind::Mod {
        match match_files(client, key, paths, instance, None).await {
            Ok(found) => found.into_iter().fold(HashMap::new(), |mut map, (name, id, _)| {
                map.entry(id).or_insert_with(Vec::new).push(name);
                map
            }),
            Err(error) => {
                tracing::warn!(%error, "could not match local mods on CurseForge");
                HashMap::new()
            }
        }
    } else {
        HashMap::new()
    };
    if kind == ContentKind::Mod {
        // Required dependencies, a few levels deep.
        let mut seen: std::collections::HashSet<u64> = chosen.iter().map(|f| f.mod_id).chain(present.keys().copied()).collect();
        let mut queue: Vec<u64> = chosen.iter().flat_map(|f| f.dependencies.iter().filter(|d| d.relation_type == 3).map(|d| d.mod_id)).collect();
        let mut rounds = 0;
        while let Some(dep) = queue.pop() {
            if !seen.insert(dep) || rounds > 64 {
                continue;
            }
            rounds += 1;
            let file = files_for(client, key, instance, dep, kind)
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| AppError::new("curseforge", crate::tr!("缺少前置模组（CurseForge 项目 {dep}）适配 {} 的版本。", "A required dependency (CurseForge project {dep}) has no version for {}.", instance.version_id)))?;
            queue.extend(file.dependencies.iter().filter(|d| d.relation_type == 3).map(|d| d.mod_id));
            chosen.push(file);
        }
    }
    let folder = crate::core::content::target_folder(paths, &instance.id, kind, save)?;
    std::fs::create_dir_all(&folder)?;
    let stage = paths.instance_dir(&instance.id).join(format!(".curseforge-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&stage)?;
    let result = async {
        let mut tasks = Vec::new();
        let mut names = Vec::new();
        for file in &chosen {
            safe_file_name(&file.file_name, kind)?;
            if folder.join(&file.file_name).exists() || folder.join(format!("{}.disabled", file.file_name)).exists() {
                continue;
            }
            let mut urls = file.urls();
            let first = urls.remove(0);
            tasks.push(DownloadTask::new(
                format!("CurseForge {}", file.file_name),
                first,
                urls,
                stage.join(&file.file_name),
                file.sha1(),
                (file.file_length > 0).then_some(file.file_length),
            ));
            names.push(file.file_name.clone());
        }
        if tasks.is_empty() {
            return Ok(crate::core::i18n::pick("都已经装好了。", "Already installed.").to_string());
        }
        let progress = Arc::new(DownloadProgress::new(&tasks));
        download::download_all(tasks, client.clone(), 4, cancel.clone(), progress).await?;
        if cancel.is_cancelled() {
            return Err(AppError::new("cancelled", crate::core::i18n::pick("下载已取消。", "Download cancelled.")));
        }
        for name in &names {
            if kind == ContentKind::World {
                // A map is a zipped save: unpack it next to the others under
                // a free folder name, as importing a save does.
                let save = crate::core::manage::import_save(paths, &instance.id, &stage.join(name))?;
                return Ok(crate::tr!("已添加地图「{save}」，在存档里就能找到。", "Added the map \"{save}\"; it's with your saves."));
            }
            std::fs::rename(stage.join(name), folder.join(name))?;
        }
        for file in chosen.iter().filter(|file| names.contains(&file.file_name)) {
            for old in present.get(&file.mod_id).into_iter().flatten().filter(|old| old.trim_end_matches(".disabled") != file.file_name) {
                crate::core::manage::recycle_item(paths, &instance.id, crate::core::manage::ItemKind::Mod, old, None)?;
            }
        }
        Ok(if names.len() == 1 { crate::tr!("已安装 {}。", "Installed {}.", names[0]) } else { crate::tr!("已安装 {} 个文件（含前置）。", "Installed {} files (with dependencies).", names.len()) })
    }
    .await;
    let _ = std::fs::remove_dir_all(&stage);
    result
}

/// Which local files CurseForge hosts, by fingerprint: (project, file).
pub async fn files_by_fingerprint(client: &reqwest::Client, key: &str, fingerprints: &[u32]) -> AppResult<HashMap<u32, (u64, u64)>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Matches {
        #[serde(default)]
        exact_matches: Vec<Match>,
    }
    #[derive(Deserialize)]
    struct Match {
        file: MatchedFile,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct MatchedFile {
        id: u64,
        mod_id: u64,
        file_fingerprint: u32,
    }
    let mut found = HashMap::new();
    for chunk in fingerprints.chunks(500) {
        let page: Page<Matches> = post(client, key, &format!("/fingerprints/{MINECRAFT}"), &serde_json::json!({ "fingerprints": chunk })).await?;
        for hit in page.data.exact_matches {
            found.insert(hit.file.file_fingerprint, (hit.file.mod_id, hit.file.id));
        }
    }
    Ok(found)
}

/// A CurseForge modpack's `manifest.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackManifest {
    pub minecraft: PackMinecraft,
    #[serde(default)]
    pub name: String,
    /// The pack's own version label ("1.4.2").
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub files: Vec<PackEntry>,
    #[serde(default = "default_overrides")]
    pub overrides: String,
}

fn default_overrides() -> String {
    "overrides".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackMinecraft {
    pub version: String,
    #[serde(default)]
    pub mod_loaders: Vec<PackLoader>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackLoader {
    pub id: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackEntry {
    #[serde(rename = "projectID")]
    pub project_id: u64,
    #[serde(rename = "fileID")]
    pub file_id: u64,
    #[serde(default = "yes")]
    pub required: bool,
}

fn yes() -> bool {
    true
}

impl PackManifest {
    /// `forge-47.2.0` → ("forge", "47.2.0").
    pub fn loader(&self) -> Option<(String, String)> {
        let loader = self.minecraft.mod_loaders.iter().find(|l| l.primary).or_else(|| self.minecraft.mod_loaders.first())?;
        let (name, version) = loader.id.split_once('-')?;
        let name = match name {
            "forge" | "fabric" | "quilt" | "neoforge" => name,
            _ => return None,
        };
        Some((name.to_string(), version.to_string()))
    }
}

/// One file of a modpack, resolved: where it goes and how to get it.
#[derive(Debug, Clone)]
pub struct ResolvedFile {
    pub path: String,
    pub sha1: String,
    pub urls: Vec<String>,
    pub size: u64,
}

/// Turns a modpack's project/file numbers into downloads. Files without a
/// published SHA-1 cannot be verified and are refused.
pub async fn resolve_pack(client: &reqwest::Client, key: &str, manifest: &PackManifest) -> AppResult<Vec<ResolvedFile>> {
    let wanted: Vec<&PackEntry> = manifest.files.iter().filter(|f| f.required).collect();
    let files = files_by_id(client, key, &wanted.iter().map(|f| f.file_id).collect::<Vec<_>>()).await?;
    let mut mod_ids: Vec<u64> = files.iter().map(|f| f.mod_id).collect();
    mod_ids.sort_unstable();
    mod_ids.dedup();
    let classes: std::collections::HashMap<u64, u32> = mods_by_id(client, key, &mod_ids)
        .await?
        .into_iter()
        .map(|m| (m.id, m.class_id.unwrap_or(6)))
        .collect();
    let mut resolved = Vec::new();
    for entry in wanted {
        let file = files
            .iter()
            .find(|f| f.id == entry.file_id)
            .ok_or_else(|| AppError::new("curseforge", crate::tr!("整合包里的文件 {} 在 CurseForge 上找不到了。", "The modpack file {} is gone from CurseForge.", entry.file_id)))?;
        let kind = kind_of_class(classes.get(&file.mod_id).copied().unwrap_or(6)).unwrap_or(ContentKind::Mod);
        safe_file_name(&file.file_name, kind).or_else(|_| safe_file_name(&file.file_name, ContentKind::ResourcePack))?;
        let sha1 = file
            .sha1()
            .ok_or_else(|| AppError::new("curseforge", crate::tr!("{} 没有校验值，无法安全下载。", "{} has no hash, so it can't be downloaded safely.", file.file_name)))?;
        resolved.push(ResolvedFile {
            path: format!("{}/{}", kind.folder(), file.file_name),
            sha1,
            urls: file.urls(),
            size: file.file_length,
        });
    }
    Ok(resolved)
}

/// Reads `manifest.json` from a zip, when it is a CurseForge modpack.
pub fn read_manifest(source: &Path) -> AppResult<Option<PackManifest>> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(source)?).map_err(crate::core::content::zip_error)?;
    let Ok(entry) = zip.by_name("manifest.json") else {
        return Ok(None);
    };
    if entry.size() > 8 * 1024 * 1024 {
        return Err(AppError::new("modpack", crate::core::i18n::pick("整合包清单过大。", "The modpack manifest is too large.")));
    }
    let mut text = String::new();
    entry.take(8 * 1024 * 1024).read_to_string(&mut text)?;
    let manifest: PackManifest = serde_json::from_str(&text).map_err(|e| AppError::detail("modpack", crate::core::i18n::pick("CurseForge 整合包清单无法读取。", "Couldn't read the CurseForge modpack manifest."), e.to_string()))?;
    if !crate::core::fabric::valid_version(&manifest.minecraft.version) {
        return Err(AppError::new("modpack", crate::core::i18n::pick("整合包缺少有效的 Minecraft 版本。", "The modpack has no valid Minecraft version.")));
    }
    Ok(Some(manifest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_ignore_whitespace() {
        // Values from a line-by-line port of the C MurmurHash2, seed 1.
        assert_eq!(fingerprint(b""), 0x5bd1_5e36);
        assert_eq!(fingerprint(b" \n\t\r"), 0x5bd1_5e36);
        assert_eq!(fingerprint(b"hello world"), 0xa85c_bded);
        assert_eq!(fingerprint(b"helloworld"), 0xa85c_bded);
        assert_eq!(fingerprint(b"abc"), 0x60a4_fcc1);
    }

    #[test]
    fn ids_manifests_and_cdn_urls() {
        assert_eq!(parse_id("cf:238222"), Some((238222, None)));
        assert_eq!(parse_id("cf:238222:4712866"), Some((238222, Some(4712866))));
        assert_eq!(parse_id("cf:x"), None);
        assert_eq!(parse_id("AANobbMI"), None);

        let manifest: PackManifest = serde_json::from_str(
            r#"{"minecraft":{"version":"1.20.1","modLoaders":[{"id":"forge-47.2.0","primary":true}]},
                "manifestType":"minecraftModpack","name":"测试包","files":[{"projectID":1,"fileID":4712866,"required":true}]}"#,
        )
        .unwrap();
        assert_eq!(manifest.loader(), Some(("forge".into(), "47.2.0".into())));
        assert_eq!(manifest.overrides, "overrides");

        let file: CfFile = serde_json::from_str(
            r#"{"id":4712866,"modId":238222,"fileName":"jei 1.20.1.jar","downloadUrl":null,
                "hashes":[{"value":"ABCDEF0123456789ABCDEF0123456789ABCDEF01","algo":1}],
                "gameVersions":["1.20.1","Forge","Client"],"dependencies":[{"modId":1,"relationType":3}]}"#,
        )
        .unwrap();
        assert_eq!(file.urls()[0], "https://edge.forgecdn.net/files/4712/866/jei%201.20.1.jar");
        assert_eq!(file.sha1().as_deref(), Some("abcdef0123456789abcdef0123456789abcdef01"));
        let version = file.version();
        assert_eq!(version.id, "cf:238222:4712866");
        assert_eq!(version.loaders, vec!["forge".to_string()]);
        assert_eq!(version.dependencies[0].project_id.as_deref(), Some("cf:1"));
        let forge = Instance { version_id: "1.20.1".into(), loader: "forge".into(), ..Instance::default() };
        let fabric = Instance { version_id: "1.20.1".into(), loader: "fabric".into(), ..Instance::default() };
        assert!(file.fits(&forge, ContentKind::Mod));
        assert!(!file.fits(&fabric, ContentKind::Mod));
    }
}

pub async fn changelog(client: &reqwest::Client, key: &str, version: &str) -> AppResult<String> {
    let Some((project, Some(file))) = parse_id(version) else { return Err(AppError::new("curseforge", "Invalid file id")); };
    let page: Page<String> = get(client, key, &format!("/mods/{project}/files/{file}/changelog"), &[]).await?;
    Ok(page.data)
}
