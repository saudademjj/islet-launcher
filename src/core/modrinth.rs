use crate::core::{
    content::{self, ContentKind},
    download::{self, DownloadProgress, DownloadTask},
    error::{AppError, AppResult},
    instances::Instance,
    paths::AppPaths,
    util::{atomic_write, now_millis},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio_util::sync::CancellationToken;
const API: &str = "https://api.modrinth.com/v2";
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub project_id: String,
    /// Search hits carry the slug; the front end keys its Chinese-name table on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    pub title: String,
    pub description: String,
    pub downloads: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    /// Loaders and tags, e.g. `iris` / `optifine` on a shader pack.
    #[serde(default)]
    pub categories: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub hits: Vec<Project>,
    pub total_hits: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModVersion {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub files: Vec<ModFile>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    /// "release", "beta" or "alpha".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_published: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModFile {
    pub url: String,
    pub filename: String,
    pub primary: bool,
    pub size: u64,
    pub hashes: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub version_id: Option<String>,
    pub project_id: Option<String>,
    pub dependency_type: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Installed {
    version_id: String,
    filename: String,
}
type Manifest = BTreeMap<String, Installed>;

/// What one install or update changed, kept next to the files it replaced
/// so the latest one can be undone.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UndoRecord {
    created_ms: i64,
    /// Files it put in `mods` (a disabled copy ends in `.disabled`).
    added: Vec<String>,
    /// Files it moved out of `mods` into this backup folder.
    replaced: Vec<String>,
    /// The Modrinth manifest from before, or none if there was none.
    manifest: Option<String>,
}

const UNDO_FILE: &str = "undo.json";

/// The latest install or update that can still be undone.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastModChange {
    pub created_ms: i64,
    pub added: usize,
    pub replaced: usize,
}

async fn get<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    path: &str,
    query: &[(&str, String)],
) -> AppResult<T> {
    Ok(client
        .get(format!("{API}{path}"))
        .query(query)
        .timeout(Duration::from_secs(30))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
async fn post<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    path: &str,
    body: &serde_json::Value,
) -> AppResult<T> {
    Ok(client
        .post(format!("{API}{path}"))
        .json(body)
        .timeout(Duration::from_secs(30))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
fn identifier(id: &str) -> AppResult<()> {
    if !id.is_empty()
        && id.len() <= 100
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Ok(())
    } else {
        Err(AppError::new("modrinth", crate::core::i18n::pick("模组标识无效。", "Invalid mod id.")))
    }
}
/// What a project page shows beyond the search hit.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectDetails {
    pub body: String,
    /// "markdown" (Modrinth) or "html" (CurseForge); the page sanitizes both.
    pub format: &'static str,
    pub gallery: Vec<GalleryImage>,
    /// `page`, `source`, `wiki`, `issues` or `discord`, all https.
    pub links: Vec<(&'static str, String)>,
    pub license: Option<String>,
    pub updated: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GalleryImage {
    pub url: String,
    pub title: Option<String>,
}

/// Descriptions are shown, not stored; a runaway one is cut short.
pub(crate) const MAX_BODY: usize = 200_000;

pub(crate) fn https(url: Option<String>) -> Option<String> {
    url.filter(|url| url.starts_with("https://") && url.len() <= 2_000)
}

pub(crate) fn clip(mut body: String) -> String {
    if body.len() > MAX_BODY {
        let mut end = MAX_BODY;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        body.truncate(end);
    }
    body
}

pub async fn details(client: &reqwest::Client, id: &str) -> AppResult<ProjectDetails> {
    identifier(id)?;
    #[derive(Deserialize)]
    struct Raw {
        slug: String,
        project_type: String,
        #[serde(default)]
        body: String,
        #[serde(default)]
        gallery: Vec<RawImage>,
        source_url: Option<String>,
        wiki_url: Option<String>,
        issues_url: Option<String>,
        discord_url: Option<String>,
        license: Option<License>,
        updated: Option<String>,
    }
    #[derive(Deserialize)]
    struct RawImage {
        url: String,
        title: Option<String>,
        #[serde(default)]
        featured: bool,
        #[serde(default)]
        ordering: i64,
    }
    #[derive(Deserialize)]
    struct License {
        id: String,
        name: Option<String>,
    }
    let raw: Raw = get(client, &format!("/project/{id}"), &[]).await?;
    let mut gallery = raw.gallery;
    gallery.sort_by_key(|image| (!image.featured, image.ordering));
    let page = (identifier(&raw.slug).is_ok() && identifier(&raw.project_type).is_ok())
        .then(|| format!("https://modrinth.com/{}/{}", raw.project_type, raw.slug));
    let links = [("page", page), ("source", raw.source_url), ("wiki", raw.wiki_url), ("issues", raw.issues_url), ("discord", raw.discord_url)]
        .into_iter()
        .filter_map(|(kind, url)| https(url).map(|url| (kind, url)))
        .collect();
    Ok(ProjectDetails {
        body: clip(raw.body),
        format: "markdown",
        gallery: gallery
            .into_iter()
            .filter_map(|image| https(Some(image.url)).map(|url| GalleryImage { url, title: image.title }))
            .take(24)
            .collect(),
        links,
        license: raw.license.map(|l| l.name.filter(|n| !n.is_empty()).unwrap_or(l.id)),
        updated: raw.updated,
    })
}

/// Modpack categories both sources understand, by Modrinth's tag names.
pub const MODPACK_CATEGORIES: &[&str] = &["adventure", "technology", "magic", "quests", "combat", "multiplayer", "lightweight", "kitchen-sink", "challenging", "optimization"];
/// Loaders a modpack can be filtered by.
pub const MODPACK_LOADERS: &[&str] = &["forge", "neoforge", "fabric", "quilt"];

/// Modpacks, optionally for one Minecraft version, loader and category.
pub async fn search_modpacks(client: &reqwest::Client, query: &str, offset: usize, game_version: Option<&str>, loader: Option<&str>, category: Option<&str>, sort: &str) -> AppResult<SearchResults> {
    if query.chars().count() > 100 {
        return Err(AppError::new("modrinth", crate::core::i18n::pick("搜索词太长。", "The search is too long.")));
    }
    let mut facets = vec![vec!["project_type:modpack".to_string()]];
    if let Some(version) = game_version.filter(|v| crate::core::fabric::valid_version(v)) {
        facets.push(vec![format!("versions:{version}")]);
    }
    for tag in [loader.filter(|l| MODPACK_LOADERS.contains(l)), category.filter(|c| MODPACK_CATEGORIES.contains(c))].into_iter().flatten() {
        facets.push(vec![format!("categories:{tag}")]);
    }
    let index = match sort {
        "downloads" | "follows" | "newest" | "updated" => sort,
        _ => "relevance",
    };
    get(
        client,
        "/search",
        &[
            ("query", query.to_string()),
            ("facets", serde_json::json!(facets).to_string()),
            ("index", index.to_string()),
            ("limit", "20".into()),
            ("offset", offset.to_string()),
        ],
    )
    .await
}

/// Every version of a modpack, newest first.
pub async fn modpack_versions(client: &reqwest::Client, project: &str) -> AppResult<Vec<ModVersion>> {
    identifier(project)?;
    get(client, &format!("/project/{project}/version"), &[]).await
}

/// The `.mrpack` of a modpack version: address, file name, SHA-1 and the
/// project it belongs to.
pub async fn modpack_file(client: &reqwest::Client, version: &str) -> AppResult<(String, String, Option<String>, String)> {
    identifier(version)?;
    let version: ModVersion = get(client, &format!("/version/{version}"), &[]).await?;
    let file = version
        .files
        .iter()
        .filter(|file| file.filename.to_ascii_lowercase().ends_with(".mrpack"))
        .max_by_key(|file| file.primary)
        .ok_or_else(|| AppError::new("modrinth", crate::core::i18n::pick("这个版本没有 .mrpack 文件。", "This version has no .mrpack file.")))?;
    if !file.url.starts_with("https://") {
        return Err(AppError::new("modrinth", crate::core::i18n::pick("整合包下载地址无效。", "Invalid modpack download address.")));
    }
    Ok((file.url.clone(), file.filename.clone(), file.hashes.get("sha1").cloned(), version.project_id.clone()))
}

/// `categories` narrow the results (each one must match: a category, a
/// shader loader, a resolution); `sort` is a Modrinth search index.
pub async fn search(
    client: &reqwest::Client,
    instance: &Instance,
    query: &str,
    offset: usize,
    kind: ContentKind,
    categories: &[String],
    sort: &str,
) -> AppResult<SearchResults> {
    if kind == ContentKind::World {
        return Err(no_maps());
    }
    let facets = search_facets(instance, kind, categories)?;
    let index = match sort {
        "downloads" | "follows" | "newest" | "updated" => sort,
        _ => "relevance",
    };
    get(
        client,
        "/search",
        &[
            ("query", query.to_string()),
            ("facets", facets.to_string()),
            ("index", index.to_string()),
            ("limit", "10".into()),
            ("offset", offset.to_string()),
        ],
    )
    .await
}
/// Mods must match the instance's loader; resource packs and shader packs
/// only the game version (which shader loader is needed shows as a tag).
/// Each chosen category is one more condition.
fn search_facets(instance: &Instance, kind: ContentKind, categories: &[String]) -> AppResult<serde_json::Value> {
    let mut facets = vec![
        vec![format!("project_type:{}", kind.project_type())],
        vec![format!("versions:{}", instance.version_id)],
    ];
    if kind == ContentKind::Mod {
        check_loader(instance)?;
        // One OR group: any loader this world can run.
        facets.push(mod_loaders(&instance.loader).iter().map(|loader| format!("categories:{loader}")).collect());
    }
    for category in categories {
        // Modrinth tags: "optimization", "iris", "16x", "512x+", "8x-".
        let valid = !category.is_empty()
            && category.len() <= 32
            && category.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'+' | b'_'));
        if !valid {
            return Err(AppError::new("modrinth", crate::core::i18n::pick("筛选条件无效。", "Invalid filter.")));
        }
        facets.push(vec![format!("categories:{category}")]);
    }
    Ok(serde_json::json!(facets))
}
pub async fn versions(
    client: &reqwest::Client,
    instance: &Instance,
    project: &str,
    kind: ContentKind,
) -> AppResult<Vec<ModVersion>> {
    if kind == ContentKind::Mod {
        check_loader(instance)?;
    }
    identifier(project)?;
    let mut query = vec![(
        "game_versions",
        serde_json::json!([instance.version_id]).to_string(),
    )];
    match kind {
        ContentKind::Mod => query.push(("loaders", serde_json::json!(mod_loaders(&instance.loader)).to_string())),
        ContentKind::ResourcePack => query.push(("loaders", r#"["minecraft"]"#.to_string())),
        ContentKind::Datapack => query.push(("loaders", r#"["datapack"]"#.to_string())),
        ContentKind::Shader => {}
        ContentKind::World => return Err(no_maps()),
    }
    let list: Vec<ModVersion> = get(client, &format!("/project/{project}/version"), &query).await?;
    Ok(list
        .into_iter()
        .filter(|v| compatible(v, instance, kind))
        .collect())
}
/// The mod loaders whose mods a world can load: Quilt also runs Fabric mods.
pub fn mod_loaders(loader: &str) -> Vec<&str> {
    match loader {
        "quilt" => vec!["quilt", "fabric"],
        "legacyfabric" => vec!["legacy-fabric"],
        "cleanroom" => vec!["forge"],
        other => vec![other],
    }
}
fn compatible(version: &ModVersion, instance: &Instance, kind: ContentKind) -> bool {
    version.game_versions.contains(&instance.version_id)
        && (kind != ContentKind::Mod || mod_loaders(&instance.loader).iter().any(|loader| version.loaders.iter().any(|l| l == loader)))
}
/// Modrinth hosts no maps; they come from CurseForge.
pub fn no_maps() -> AppError {
    AppError::new("modrinth", crate::core::i18n::pick("Modrinth 上没有地图，请切换到 CurseForge 搜索。", "Modrinth has no maps; search CurseForge instead."))
}

fn check_loader(instance: &Instance) -> AppResult<()> {
    if ["fabric", "quilt", "forge", "neoforge", "legacyfabric", "cleanroom", "liteloader"].contains(&instance.loader.as_str()) {
        Ok(())
    } else {
        Err(AppError::new(
            "modrinth",
            crate::core::i18n::pick("请先在常规设置选择并保存 Fabric、Quilt、Forge 或 NeoForge。", "Choose and save Fabric, Quilt, Forge or NeoForge under General first."),
        ))
    }
}
fn download_file(
    version: &ModVersion,
    destination: &Path,
    kind: ContentKind,
) -> AppResult<(DownloadTask, String)> {
    let file = version
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| version.files.first())
        .ok_or_else(|| AppError::new("modrinth", crate::tr!("这个{}版本没有可下载文件。", "This {} version has no file to download.", kind.label())))?;
    let safe = crate::core::java::runtime::safe_archive_path(&file.filename)?;
    if safe.components().count() != 1
        || !(file.filename.to_lowercase().ends_with(kind.extension()) || (kind == ContentKind::Mod && file.filename.to_lowercase().ends_with(".litemod")))
        || file.filename.contains(':')
    {
        return Err(AppError::new("modrinth", crate::tr!("{}文件名无效。", "Invalid {} file name.", kind.label())));
    }
    let url =
        url::Url::parse(&file.url).map_err(|_| AppError::new("modrinth", crate::core::i18n::pick("模组下载地址无效。", "Invalid mod download address.")))?;
    if url.scheme() != "https" || url.host_str() != Some("cdn.modrinth.com") {
        return Err(AppError::new("modrinth", crate::core::i18n::pick("下载地址不属于 Modrinth CDN。", "The download address isn't on Modrinth's CDN.")));
    }
    let hash = file
        .hashes
        .get("sha1")
        .filter(|v| v.len() == 40 && v.bytes().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| AppError::new("modrinth", crate::tr!("{}缺少 SHA-1 校验值。", "The {} has no SHA-1 hash.", kind.label())))?;
    Ok((
        DownloadTask::new(
            &version.name,
            &file.url,
            vec![],
            destination.join(safe),
            Some(hash.clone()),
            Some(file.size),
        ),
        file.filename.clone(),
    ))
}

/// Installs (or updates) the given mod versions with their required
/// dependencies. Several ids at once is how "update all" works.
pub async fn install(
    client: &reqwest::Client,
    paths: &AppPaths,
    instance: &Instance,
    version_ids: &[String],
    cancel: &CancellationToken,
) -> AppResult<String> {
    check_loader(instance)?;
    if version_ids.is_empty() {
        return Err(AppError::new("modrinth", crate::core::i18n::pick("没有选择要安装的模组版本。", "No mod version chosen to install.")));
    }
    for id in version_ids {
        identifier(id)?;
    }
    let mods = paths.instance_game_dir(&instance.id).join("mods");
    std::fs::create_dir_all(&mods)?;
    let manifest_path = paths
        .instance_dir(&instance.id)
        .join("modrinth-installed.json");
    let previous_manifest = std::fs::read_to_string(&manifest_path).ok();
    let mut manifest: Manifest = if manifest_path.is_file() {
        serde_json::from_slice(&std::fs::read(&manifest_path)?)?
    } else {
        BTreeMap::new()
    };
    let present = |name: &str| mods.join(name).is_file() || mods.join(format!("{name}.disabled")).is_file();
    // Mods added by hand are recognised by hash, so an update or a
    // dependency replaces them instead of adding a second copy.
    match local_hashes(paths, &instance.id).await {
        Ok(hashes) if !hashes.is_empty() => match identify(client, &hashes).await {
            Ok(found) => adopt(&mut manifest, &hashes, &found, present),
            Err(error) => tracing::warn!(%error, "could not identify local mods"),
        },
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "could not hash local mods"),
    }
    let mut pending: Vec<(Option<String>, Option<String>)> =
        version_ids.iter().map(|id| (None, Some(id.clone()))).collect();
    let mut chosen: BTreeMap<String, ModVersion> = BTreeMap::new();
    let mut seen = HashSet::new();
    while let Some((project, pinned)) = pending.pop() {
        if cancel.is_cancelled() {
            return Err(AppError::new("cancelled", crate::core::i18n::pick("模组下载已取消。", "Mod download cancelled.")));
        }
        if chosen.len() > 512 {
            return Err(AppError::new("modrinth", crate::core::i18n::pick("模组依赖过多。", "Too many mod dependencies.")));
        }
        if pinned.is_none() && project.as_ref().is_some_and(|p| chosen.contains_key(p)) {
            continue;
        }
        let version = if let Some(id) = pinned {
            identifier(&id)?;
            if !seen.insert(id.clone()) {
                continue;
            }
            get::<ModVersion>(client, &format!("/version/{id}"), &[]).await?
        } else {
            versions(
                client,
                instance,
                project
                    .as_deref()
                    .ok_or_else(|| AppError::new("modrinth", crate::core::i18n::pick("必需依赖无法自动定位。", "A required dependency couldn't be found automatically.")))?,
                ContentKind::Mod,
            )
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| AppError::new("modrinth", crate::core::i18n::pick("某个必需依赖没有兼容版本。", "A required dependency has no compatible version.")))?
        };
        if !compatible(&version, instance, ContentKind::Mod) {
            return Err(AppError::new(
                "modrinth",
                crate::tr!("{} 不兼容当前游戏版本或加载器。", "{} doesn't fit this game version or loader.", version.name),
            ));
        }
        if chosen
            .get(&version.project_id)
            .is_some_and(|v| v.id != version.id)
        {
            return Err(AppError::new(
                "modrinth",
                crate::core::i18n::pick("依赖要求了同一模组的不同版本，请调整选择。", "Dependencies ask for different versions of the same mod; adjust your choice."),
            ));
        }
        for dependency in &version.dependencies {
            if dependency.dependency_type == "required" {
                pending.push((dependency.project_id.clone(), dependency.version_id.clone()));
            }
        }
        chosen.insert(version.project_id.clone(), version);
    }
    for version in chosen.values() {
        for dependency in &version.dependencies {
            if dependency.dependency_type == "incompatible" {
                let conflict = dependency
                    .project_id
                    .as_ref()
                    .is_some_and(|p| chosen.contains_key(p) || manifest.contains_key(p))
                    || dependency.version_id.as_ref().is_some_and(|v| {
                        chosen.values().any(|m| &m.id == v)
                            || manifest.values().any(|m| &m.version_id == v)
                    });
                if conflict {
                    return Err(AppError::new(
                        "modrinth",
                        crate::tr!("{} 与已安装或选中的模组存在冲突。", "{} conflicts with an installed or chosen mod.", version.name),
                    ));
                }
            }
        }
    }
    let stage = paths
        .instance_dir(&instance.id)
        .join(format!(".mod-stage-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&stage)?;
    let result = async {
        let mut tasks = Vec::new();
        let mut names = HashSet::new();
        for version in chosen.values() {
            let (task, filename) = download_file(version, &stage, ContentKind::Mod)?;
            if !names.insert(filename.to_lowercase()) {
                return Err(AppError::new("modrinth", crate::core::i18n::pick("两个模组使用了相同文件名。", "Two mods use the same file name.")));
            }
            // Never overwrite an unmanaged local mod, including a disabled copy.
            if [
                mods.join(&filename),
                mods.join(format!("{filename}.disabled")),
            ]
            .iter()
            .any(|p| p.exists())
                && !manifest
                    .get(&version.project_id)
                    .is_some_and(|m| m.filename == filename)
            {
                return Err(AppError::new(
                    "modrinth",
                    crate::tr!("本地已有 {filename}；请先移走或备份该文件。", "{filename} is already there; move it or back it up first."),
                ));
            }
            tasks.push(task);
        }
        let progress = Arc::new(DownloadProgress::new(&tasks));
        download::download_all(tasks, client.clone(), 4, cancel.clone(), progress).await?;
        if cancel.is_cancelled() {
            return Err(AppError::new("cancelled", crate::core::i18n::pick("模组下载已取消。", "Mod download cancelled.")));
        }
        let backup = paths
            .instance_dir(&instance.id)
            .join("mod-backups")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&backup)?;
        let mut moved = Vec::new();
        let mut published = Vec::new();
        let (mut replaced, mut added) = (Vec::new(), Vec::new());
        let commit = (|| -> AppResult<()> {
            for version in chosen.values() {
                let (_, filename) = download_file(version, &stage, ContentKind::Mod)?;
                // An update keeps the mod as it was: one the player turned
                // off stays off (only a disabled copy was there).
                let mut keep_disabled = false;
                if let Some(old) = manifest.get(&version.project_id) {
                    crate::core::java::runtime::safe_archive_path(&old.filename)?;
                    let disabled = format!("{}.disabled", old.filename);
                    keep_disabled = stays_disabled(&mods, &old.filename);
                    for name in [&old.filename, &disabled] {
                        let source = mods.join(name);
                        if source.is_file() {
                            let saved = backup.join(name);
                            std::fs::rename(&source, &saved)?;
                            moved.push((source, saved));
                        }
                    }
                    if old.filename != filename {
                        replaced.push(format!("mods/{}", old.filename));
                    }
                }
                let target = mods.join(if keep_disabled { format!("{filename}.disabled") } else { filename.clone() });
                std::fs::rename(stage.join(&filename), &target)?;
                added.push(format!("mods/{filename}"));
                published.push(target);
                manifest.insert(
                    version.project_id.clone(),
                    Installed {
                        version_id: version.id.clone(),
                        filename,
                    },
                );
            }
            // A modpack's repair must not bring back the files this replaced.
            crate::core::modpacks::record_replaced(paths, &instance.id, &replaced, &added)?;
            atomic_write(&manifest_path, &serde_json::to_vec_pretty(&manifest)?)?;
            let record = UndoRecord {
                created_ms: now_millis(),
                added: published.iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect(),
                replaced: moved.iter().filter_map(|(source, _)| source.file_name()).map(|n| n.to_string_lossy().into_owned()).collect(),
                manifest: previous_manifest.clone(),
            };
            atomic_write(&backup.join(UNDO_FILE), &serde_json::to_vec_pretty(&record)?)?;
            Ok(())
        })();
        if let Err(e) = commit {
            for p in published {
                let _ = std::fs::remove_file(p);
            }
            for (source, saved) in moved.into_iter().rev() {
                let _ = std::fs::rename(saved, source);
            }
            return Err(e);
        }
        Ok(crate::tr!("已安装或更新 {} 个模组（包含必需依赖）；在「模组」页可以撤销这一次。", "Installed or updated {} mods (with required dependencies); you can undo this on the Mods tab.",
            chosen.len()
        ))
    }
    .await;
    let _ = std::fs::remove_dir_all(stage);
    result
}

/// The newest backup folder holding an undo record, with the record.
fn latest_undo(paths: &AppPaths, id: &str) -> AppResult<Option<(std::path::PathBuf, UndoRecord)>> {
    AppPaths::validate_instance_id(id)?;
    let Ok(entries) = std::fs::read_dir(paths.instance_dir(id).join("mod-backups")) else {
        return Ok(None);
    };
    let mut latest: Option<(std::path::PathBuf, UndoRecord)> = None;
    for entry in entries.flatten() {
        let Ok(bytes) = std::fs::read(entry.path().join(UNDO_FILE)) else { continue };
        let Ok(record) = serde_json::from_slice::<UndoRecord>(&bytes) else { continue };
        if latest.as_ref().is_none_or(|(_, newest)| record.created_ms > newest.created_ms) {
            latest = Some((entry.path(), record));
        }
    }
    Ok(latest)
}

/// The install or update that "undo" would take back, if there is one.
pub fn last_change(paths: &AppPaths, id: &str) -> AppResult<Option<LastModChange>> {
    Ok(latest_undo(paths, id)?.map(|(_, record)| LastModChange {
        created_ms: record.created_ms,
        added: record.added.len(),
        replaced: record.replaced.len(),
    }))
}

/// Takes back the latest install or update: its files leave `mods`, the
/// files it replaced come back, and the manifest returns to what it was.
pub fn undo_last_change(paths: &AppPaths, id: &str) -> AppResult<String> {
    let Some((folder, record)) = latest_undo(paths, id)? else {
        return Err(AppError::new("mod_undo", crate::core::i18n::pick("没有可以撤销的模组更新。", "There's no mod update to undo.")));
    };
    let mods = paths.instance_game_dir(id).join("mods");
    for name in record.added.iter().chain(&record.replaced) {
        crate::core::java::runtime::safe_archive_path(name)?;
    }
    // The player may have turned a mod on or off since; both copies go.
    for name in &record.added {
        let base = name.strip_suffix(".disabled").unwrap_or(name);
        for candidate in [base.to_string(), format!("{base}.disabled")] {
            let path = mods.join(&candidate);
            if path.is_file() {
                std::fs::remove_file(path)?;
            }
        }
    }
    let mut restored = 0;
    for name in &record.replaced {
        let saved = folder.join(name);
        let target = mods.join(name);
        if saved.is_file() && !target.exists() {
            std::fs::rename(saved, target)?;
            restored += 1;
        }
    }
    // A modpack's repair follows: the files this brought back are the pack's
    // own again, and the ones it took away stay away.
    let as_mods = |names: &[String]| -> Vec<String> {
        names.iter().map(|n| format!("mods/{}", n.strip_suffix(".disabled").unwrap_or(n))).collect()
    };
    crate::core::modpacks::record_replaced(paths, id, &as_mods(&record.added), &as_mods(&record.replaced))?;
    let manifest_path = paths.instance_dir(id).join("modrinth-installed.json");
    match &record.manifest {
        Some(text) => atomic_write(&manifest_path, text.as_bytes())?,
        None => {
            let _ = std::fs::remove_file(&manifest_path);
        }
    }
    std::fs::remove_dir_all(&folder)?;
    Ok(crate::tr!(
        "已撤销上次模组更新：移除 {} 个文件，恢复 {} 个旧文件。",
        "Undid the last mod update: removed {} files and brought back {} old ones.",
        record.added.len(),
        restored
    ))
}

/// Whether the player had turned this mod off: only its disabled copy is there.
fn stays_disabled(mods: &Path, filename: &str) -> bool {
    mods.join(format!("{filename}.disabled")).is_file() && !mods.join(filename).is_file()
}

/// Downloads one resource pack or shader pack into its folder. Packs have no
/// dependencies, and an existing file of the same name is never replaced.
pub async fn install_pack(
    client: &reqwest::Client,
    paths: &AppPaths,
    instance: &Instance,
    version_id: &str,
    kind: ContentKind,
    save: Option<&str>,
    cancel: &CancellationToken,
) -> AppResult<String> {
    if kind == ContentKind::Mod {
        return Err(AppError::new("modrinth", crate::core::i18n::pick("模组请使用模组安装。", "Install mods as mods.")));
    }
    if kind == ContentKind::World {
        return Err(no_maps());
    }
    identifier(version_id)?;
    let version: ModVersion = get(client, &format!("/version/{version_id}"), &[]).await?;
    if !compatible(&version, instance, kind) {
        return Err(AppError::new(
            "modrinth",
            crate::tr!("这个{}版本不支持 Minecraft {}。", "This {} version doesn't support Minecraft {}.", kind.label(), instance.version_id),
        ));
    }
    let folder = content::target_folder(paths, &instance.id, kind, save)?;
    std::fs::create_dir_all(&folder)?;
    let stage = paths
        .instance_dir(&instance.id)
        .join(format!(".pack-stage-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&stage)?;
    let result = async {
        let (task, filename) = download_file(&version, &stage, kind)?;
        let target = folder.join(&filename);
        if target.exists() {
            return Err(AppError::new(
                "modrinth",
                crate::tr!("{} 已经在 {} 目录里了。", "{} is already in the {} folder.", filename, kind.folder()),
            ));
        }
        let tasks = vec![task];
        let progress = Arc::new(DownloadProgress::new(&tasks));
        download::download_all(tasks, client.clone(), 1, cancel.clone(), progress).await?;
        if cancel.is_cancelled() {
            return Err(AppError::new("cancelled", crate::core::i18n::pick("下载已取消。", "Download cancelled.")));
        }
        std::fs::rename(stage.join(&filename), &target)?;
        Ok(crate::tr!("已安装{}：{}", "Installed {}: {}", kind.label(), filename))
    }
    .await;
    let _ = std::fs::remove_dir_all(stage);
    result
}

async fn local_hashes(paths: &AppPaths, id: &str) -> AppResult<Vec<(String, String)>> {
    let paths = paths.clone();
    let id = id.to_string();
    tokio::task::spawn_blocking(move || content::mod_hashes(&paths, &id))
        .await
        .map_err(|error| AppError::detail("modrinth", crate::core::i18n::pick("计算模组校验值失败。", "Couldn't hash the mods."), error.to_string()))?
}

/// Versions on Modrinth whose files have these hashes, keyed by hash.
async fn identify(
    client: &reqwest::Client,
    hashes: &[(String, String)],
) -> AppResult<HashMap<String, ModVersion>> {
    let list: Vec<&str> = hashes.iter().map(|(_, hash)| hash.as_str()).collect();
    post(
        client,
        "/version_files",
        &serde_json::json!({ "hashes": list, "algorithm": "sha1" }),
    )
    .await
}

/// Modrinth's own copy of files, by their SHA-1: the file entry with its
/// download address and SHA-512, as a modpack index needs.
pub async fn files_by_hash(client: &reqwest::Client, hashes: &[String]) -> AppResult<HashMap<String, ModFile>> {
    if hashes.is_empty() {
        return Ok(HashMap::new());
    }
    let found: HashMap<String, ModVersion> = post(client, "/version_files", &serde_json::json!({ "hashes": hashes, "algorithm": "sha1" })).await?;
    Ok(found
        .into_iter()
        .filter_map(|(hash, version)| {
            let file = version.files.into_iter().find(|f| f.hashes.get("sha1").is_some_and(|h| h.eq_ignore_ascii_case(&hash)))?;
            Some((hash, file))
        })
        .collect())
}

/// Whether each file's project runs on a server ("required", "optional",
/// "unsupported" or "unknown"), by the file's SHA-1. Files Modrinth doesn't
/// know are left out.
pub async fn server_sides(client: &reqwest::Client, hashes: &[String]) -> AppResult<HashMap<String, String>> {
    if hashes.is_empty() {
        return Ok(HashMap::new());
    }
    let found: HashMap<String, ModVersion> = post(client, "/version_files", &serde_json::json!({ "hashes": hashes, "algorithm": "sha1" })).await?;
    let mut ids: Vec<&str> = found.values().map(|v| v.project_id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    #[derive(Deserialize)]
    struct Sides {
        id: String,
        #[serde(default)]
        server_side: String,
    }
    let projects: Vec<Sides> = get(client, "/projects", &[("ids", serde_json::json!(ids).to_string())]).await?;
    Ok(found
        .into_iter()
        .filter_map(|(hash, version)| projects.iter().find(|p| p.id == version.project_id).map(|p| (hash, p.server_side.clone())))
        .collect())
}

type Hashes<'a> = &'a [(String, String)];

/// Records identified local files as the installed copy of their project.
fn adopt(
    manifest: &mut Manifest,
    hashes: Hashes<'_>,
    found: &HashMap<String, ModVersion>,
    present: impl Fn(&str) -> bool,
) {
    for (file, hash) in hashes {
        let Some(version) = found.get(hash) else {
            continue;
        };
        if manifest
            .get(&version.project_id)
            .is_some_and(|m| present(&m.filename))
        {
            continue;
        }
        manifest.insert(
            version.project_id.clone(),
            Installed {
                version_id: version.id.clone(),
                filename: file.strip_suffix(".disabled").unwrap_or(file).to_string(),
            },
        );
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ProjectInfo {
    id: String,
    #[serde(default)]
    slug: Option<String>,
    title: String,
    #[serde(default)]
    icon_url: Option<String>,
}

/// What Modrinth knows about one local mod file.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModInsight {
    pub file: String,
    pub project_id: String,
    pub slug: Option<String>,
    pub title: String,
    pub icon_url: Option<String>,
    pub version_id: String,
    pub version_number: String,
    /// A newer version for this game version and loader, if there is one.
    pub update: Option<ModUpdate>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModUpdate {
    pub version_id: String,
    pub version_number: String,
}

/// Names, icons and available updates for the instance's local mods. Files
/// Modrinth does not know are left out; the caller still lists them.
pub async fn inspect(
    client: &reqwest::Client,
    paths: &AppPaths,
    instance: &Instance,
) -> AppResult<Vec<ModInsight>> {
    check_loader(instance)?;
    let hashes = local_hashes(paths, &instance.id).await?;
    if hashes.is_empty() {
        return Ok(Vec::new());
    }
    let current = identify(client, &hashes).await?;
    if current.is_empty() {
        return Ok(Vec::new());
    }
    let known: Vec<&str> = hashes
        .iter()
        .map(|(_, hash)| hash.as_str())
        .filter(|hash| current.contains_key(*hash))
        .collect();
    // Names and updates are extras: without them the list still shows.
    let latest: HashMap<String, ModVersion> = post(
        client,
        "/version_files/update",
        &serde_json::json!({
            "hashes": known,
            "algorithm": "sha1",
            "loaders": mod_loaders(&instance.loader),
            "game_versions": [instance.version_id],
        }),
    )
    .await
    .unwrap_or_else(|error| {
        tracing::warn!(%error, "mod update check failed");
        HashMap::new()
    });
    let mut ids: Vec<&str> = current.values().map(|v| v.project_id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    let projects: Vec<ProjectInfo> = get(client, "/projects", &[("ids", serde_json::json!(ids).to_string())])
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "mod project lookup failed");
            Vec::new()
        });
    Ok(assemble(&hashes, &current, &latest, &projects))
}

/// What Modrinth knows about one local resource pack or shader pack.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackInsight {
    pub kind: ContentKind,
    pub file: String,
    pub project_id: String,
    pub title: String,
    pub icon_url: Option<String>,
}

/// Names and icons for the instance's zipped resource packs and shader
/// packs, found by their hashes. Shader packs carry no picture of their own,
/// so this is where theirs comes from. Unknown files are left out.
pub async fn inspect_packs(client: &reqwest::Client, paths: &AppPaths, instance: &Instance) -> AppResult<Vec<PackInsight>> {
    let (paths, id) = (paths.clone(), instance.id.clone());
    let hashes = tokio::task::spawn_blocking(move || content::pack_hashes(&paths, &id))
        .await
        .map_err(|error| AppError::detail("modrinth", crate::core::i18n::pick("计算资源包校验值失败。", "Couldn't hash the resource packs."), error.to_string()))??;
    if hashes.is_empty() {
        return Ok(Vec::new());
    }
    let pairs: Vec<(String, String)> = hashes.iter().map(|(_, file, hash)| (file.clone(), hash.clone())).collect();
    let current = identify(client, &pairs).await?;
    if current.is_empty() {
        return Ok(Vec::new());
    }
    let mut ids: Vec<&str> = current.values().map(|v| v.project_id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    let projects: Vec<ProjectInfo> = get(client, "/projects", &[("ids", serde_json::json!(ids).to_string())])
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "pack project lookup failed");
            Vec::new()
        });
    Ok(hashes
        .iter()
        .filter_map(|(kind, file, hash)| {
            let version = current.get(hash)?;
            let project = projects.iter().find(|p| p.id == version.project_id);
            Some(PackInsight {
                kind: *kind,
                file: file.clone(),
                project_id: version.project_id.clone(),
                title: project.map_or_else(|| version.name.clone(), |p| p.title.clone()),
                icon_url: project
                    .and_then(|p| p.icon_url.clone())
                    .filter(|url| url.starts_with("https://cdn.modrinth.com/")),
            })
        })
        .collect())
}

fn assemble(
    hashes: Hashes<'_>,
    current: &HashMap<String, ModVersion>,
    latest: &HashMap<String, ModVersion>,
    projects: &[ProjectInfo],
) -> Vec<ModInsight> {
    hashes
        .iter()
        .filter_map(|(file, hash)| {
            let version = current.get(hash)?;
            let project = projects.iter().find(|p| p.id == version.project_id);
            let update = latest
                .get(hash)
                .filter(|newer| {
                    newer.id != version.id
                        && newer.project_id == version.project_id
                        && match (&newer.date_published, &version.date_published) {
                            (Some(a), Some(b)) => a > b,
                            _ => true,
                        }
                })
                .map(|newer| ModUpdate {
                    version_id: newer.id.clone(),
                    version_number: newer.version_number.clone(),
                });
            Some(ModInsight {
                file: file.clone(),
                project_id: version.project_id.clone(),
                slug: project.and_then(|p| p.slug.clone()),
                title: project.map_or_else(|| version.name.clone(), |p| p.title.clone()),
                icon_url: project
                    .and_then(|p| p.icon_url.clone())
                    .filter(|url| url.starts_with("https://cdn.modrinth.com/")),
                version_id: version.id.clone(),
                version_number: version.version_number.clone(),
                update,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_hits_keep_the_slug() {
        let hit: Project = serde_json::from_str(
            r#"{"project_id":"LNytGWDc","slug":"create","title":"Create","description":"","downloads":1}"#,
        )
        .unwrap();
        assert_eq!(hit.slug.as_deref(), Some("create"));
        let json = serde_json::to_value(&hit).unwrap();
        assert_eq!(json["slug"], "create");
        let old: Project =
            serde_json::from_str(r#"{"project_id":"x","title":"X","description":"","downloads":0}"#).unwrap();
        assert!(old.slug.is_none());
    }
    #[test]
    fn reject_wrong_loader_and_unsafe_mod_filename() {
        let instance = Instance {
            version_id: "1.20.1".into(),
            loader: "fabric".into(),
            ..Default::default()
        };
        let mut v = ModVersion {
            id: "v".into(),
            project_id: "p".into(),
            name: "test".into(),
            version_number: "1".into(),
            game_versions: vec!["1.20.1".into()],
            loaders: vec!["forge".into()],
            files: vec![],
            dependencies: vec![],
            version_type: None,
            date_published: None,
        };
        assert!(!compatible(&v, &instance, ContentKind::Mod));
        // Packs only care about the game version.
        assert!(compatible(&v, &instance, ContentKind::ResourcePack));
        v.loaders = vec!["fabric".into()];
        assert!(compatible(&v, &instance, ContentKind::Mod));
        v.files.push(ModFile {
            url: "https://cdn.modrinth.com/test.jar".into(),
            filename: "../test.jar".into(),
            primary: true,
            size: 10,
            hashes: BTreeMap::from([("sha1".into(), "a".repeat(40))]),
        });
        assert!(download_file(&v, Path::new("/tmp"), ContentKind::Mod).is_err());
        v.files[0].filename = "test.jar".into();
        assert!(download_file(&v, Path::new("/tmp"), ContentKind::Mod).is_ok());
        // A pack must be a .zip, and a mod must be a .jar.
        assert!(download_file(&v, Path::new("/tmp"), ContentKind::Shader).is_err());
        v.files[0].filename = "BSL.zip".into();
        assert!(download_file(&v, Path::new("/tmp"), ContentKind::Shader).is_ok());
        assert!(download_file(&v, Path::new("/tmp"), ContentKind::Mod).is_err());
        v.files[0].url = "https://example.com/test.jar".into();
        assert!(download_file(&v, Path::new("/tmp"), ContentKind::Shader).is_err());
    }

    fn version(id: &str, project: &str, number: &str, date: &str) -> ModVersion {
        ModVersion {
            id: id.into(),
            project_id: project.into(),
            name: format!("{project} {number}"),
            version_number: number.into(),
            game_versions: vec!["1.21.1".into()],
            loaders: vec!["fabric".into()],
            files: vec![],
            dependencies: vec![],
            version_type: Some("release".into()),
            date_published: Some(date.into()),
        }
    }

    #[test]
    fn an_update_keeps_a_disabled_mod_disabled() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("off.jar.disabled"), b"").unwrap();
        std::fs::write(dir.path().join("on.jar"), b"").unwrap();
        assert!(stays_disabled(dir.path(), "off.jar"));
        assert!(!stays_disabled(dir.path(), "on.jar"));
        assert!(!stays_disabled(dir.path(), "missing.jar"));
    }

    #[test]
    fn the_latest_mod_update_can_be_undone() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let store = crate::core::instances::InstanceStore::load(paths.clone()).unwrap();
        let id = store.create("撤销", "1.21", "release").unwrap().id;
        let mods = paths.instance_game_dir(&id).join("mods");
        let backups = paths.instance_dir(&id).join("mod-backups");
        std::fs::create_dir_all(&mods).unwrap();
        assert!(last_change(&paths, &id).unwrap().is_none());

        // An older install of sodium-1, then an update to sodium-2 that also
        // added a dependency; the player turned sodium-2 off since.
        let older = backups.join("older");
        std::fs::create_dir_all(&older).unwrap();
        let record = |added: &[&str], replaced: &[&str], manifest: Option<&str>, created_ms| UndoRecord {
            created_ms,
            added: added.iter().map(|s| s.to_string()).collect(),
            replaced: replaced.iter().map(|s| s.to_string()).collect(),
            manifest: manifest.map(String::from),
        };
        std::fs::write(older.join(UNDO_FILE), serde_json::to_vec(&record(&["sodium-1.jar"], &[], None, 1)).unwrap()).unwrap();
        let newer = backups.join("newer");
        std::fs::create_dir_all(&newer).unwrap();
        std::fs::write(newer.join("sodium-1.jar"), b"old").unwrap();
        std::fs::write(
            newer.join(UNDO_FILE),
            serde_json::to_vec(&record(&["sodium-2.jar", "fabric-api.jar"], &["sodium-1.jar"], Some("{\"a\":1}"), 2)).unwrap(),
        )
        .unwrap();
        std::fs::write(mods.join("sodium-2.jar.disabled"), b"new").unwrap();
        std::fs::write(mods.join("fabric-api.jar"), b"api").unwrap();
        std::fs::write(paths.instance_dir(&id).join("modrinth-installed.json"), "{}").unwrap();

        let last = last_change(&paths, &id).unwrap().unwrap();
        assert_eq!((last.created_ms, last.added, last.replaced), (2, 2, 1));
        undo_last_change(&paths, &id).unwrap();
        let mut left: Vec<_> = std::fs::read_dir(&mods).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        assert_eq!(left, ["sodium-1.jar"]);
        assert_eq!(std::fs::read_to_string(mods.join("sodium-1.jar")).unwrap(), "old");
        assert_eq!(std::fs::read_to_string(paths.instance_dir(&id).join("modrinth-installed.json")).unwrap(), "{\"a\":1}");
        assert!(!newer.exists());

        // The one before is next; it had no manifest, so none is left.
        assert_eq!(last_change(&paths, &id).unwrap().unwrap().created_ms, 1);
        undo_last_change(&paths, &id).unwrap();
        assert_eq!(std::fs::read_dir(&mods).unwrap().count(), 0);
        assert!(!paths.instance_dir(&id).join("modrinth-installed.json").exists());
        assert!(last_change(&paths, &id).unwrap().is_none());
        assert!(undo_last_change(&paths, &id).is_err());
    }

    #[test]
    fn facets_follow_the_content_kind() {
        let fabric = Instance {
            version_id: "1.21.1".into(),
            loader: "fabric".into(),
            ..Default::default()
        };
        assert_eq!(
            search_facets(&fabric, ContentKind::Mod, &[]).unwrap(),
            serde_json::json!([["project_type:mod"], ["versions:1.21.1"], ["categories:fabric"]])
        );
        let vanilla = Instance {
            version_id: "1.21.1".into(),
            ..Default::default()
        };
        // Vanilla cannot load mods, but resource packs need no loader.
        assert!(search_facets(&vanilla, ContentKind::Mod, &[]).is_err());
        assert_eq!(
            search_facets(&vanilla, ContentKind::ResourcePack, &[]).unwrap(),
            serde_json::json!([["project_type:resourcepack"], ["versions:1.21.1"]])
        );
        assert_eq!(
            search_facets(&vanilla, ContentKind::Shader, &[]).unwrap(),
            serde_json::json!([["project_type:shader"], ["versions:1.21.1"]])
        );
        // Chosen filters each add a condition; anything else is refused.
        assert_eq!(
            search_facets(&vanilla, ContentKind::Shader, &["iris".into(), "realistic".into()]).unwrap(),
            serde_json::json!([["project_type:shader"], ["versions:1.21.1"], ["categories:iris"], ["categories:realistic"]])
        );
        assert_eq!(
            search_facets(&vanilla, ContentKind::ResourcePack, &["512x+".into()]).unwrap(),
            serde_json::json!([["project_type:resourcepack"], ["versions:1.21.1"], ["categories:512x+"]])
        );
        assert!(search_facets(&vanilla, ContentKind::Shader, &["a\"b".into()]).is_err());
        assert!(search_facets(&vanilla, ContentKind::Shader, &["".into()]).is_err());
    }

    #[test]
    fn local_files_are_adopted_once_per_project() {
        let hashes = vec![
            ("sodium.jar".to_string(), "h1".to_string()),
            ("lithium.jar.disabled".to_string(), "h2".to_string()),
            ("unknown.jar".to_string(), "h3".to_string()),
            ("managed.jar".to_string(), "h4".to_string()),
        ];
        let found = HashMap::from([
            ("h1".to_string(), version("v1", "sodium", "0.5", "2024-01-01T00:00:00Z")),
            ("h2".to_string(), version("v2", "lithium", "0.11", "2024-01-01T00:00:00Z")),
            ("h4".to_string(), version("v4", "managed", "2.0", "2024-01-01T00:00:00Z")),
        ]);
        let mut manifest = Manifest::from([(
            "managed".to_string(),
            Installed {
                version_id: "old".into(),
                filename: "managed-1.0.jar".into(),
            },
        )]);
        // The manifest's own file for "managed" is still there, so it stays.
        adopt(&mut manifest, &hashes, &found, |name| name == "managed-1.0.jar");
        assert_eq!(manifest["sodium"].filename, "sodium.jar");
        assert_eq!(manifest["lithium"].filename, "lithium.jar");
        assert_eq!(manifest["managed"].version_id, "old");
        assert_eq!(manifest.len(), 3);
    }

    #[test]
    fn insights_offer_only_newer_versions() {
        let hashes = vec![
            ("sodium.jar".to_string(), "h1".to_string()),
            ("lithium.jar".to_string(), "h2".to_string()),
            ("beta.jar".to_string(), "h3".to_string()),
            ("local.jar".to_string(), "h4".to_string()),
        ];
        let current = HashMap::from([
            ("h1".to_string(), version("v1", "sodium", "0.5", "2024-01-01T00:00:00Z")),
            ("h2".to_string(), version("v2", "lithium", "0.11", "2024-03-01T00:00:00Z")),
            ("h3".to_string(), version("v3", "beta", "2.0-beta", "2024-06-01T00:00:00Z")),
        ]);
        let latest = HashMap::from([
            ("h1".to_string(), version("v9", "sodium", "0.6", "2024-05-01T00:00:00Z")),
            ("h2".to_string(), version("v2", "lithium", "0.11", "2024-03-01T00:00:00Z")),
            // An older release than the beta the player runs is no update.
            ("h3".to_string(), version("v0", "beta", "1.9", "2024-02-01T00:00:00Z")),
        ]);
        let projects = vec![
            ProjectInfo {
                id: "sodium".into(),
                slug: Some("sodium".into()),
                title: "Sodium".into(),
                icon_url: Some("https://cdn.modrinth.com/data/AANobbMI/icon.png".into()),
            },
            ProjectInfo {
                id: "lithium".into(),
                slug: None,
                title: "Lithium".into(),
                icon_url: Some("https://example.com/icon.png".into()),
            },
        ];
        let found = assemble(&hashes, &current, &latest, &projects);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].title, "Sodium");
        assert_eq!(found[0].update.as_ref().map(|u| u.version_number.as_str()), Some("0.6"));
        assert!(found[1].update.is_none());
        // Icons only come from Modrinth's CDN.
        assert!(found[1].icon_url.is_none());
        // Without project details the version name stands in for the title.
        assert_eq!(found[2].title, "beta 2.0-beta");
        assert!(found[2].update.is_none());
    }

    #[test]
    fn search_hits_read_icons_and_categories() {
        let hit: Project = serde_json::from_str(
            r#"{"project_id":"HVnmMxH1","slug":"complementary-reimagined","title":"Complementary Shaders",
                "description":"","downloads":1,"icon_url":"https://cdn.modrinth.com/data/HVnmMxH1/icon.png",
                "categories":["iris","optifine"]}"#,
        )
        .unwrap();
        assert_eq!(hit.categories, ["iris", "optifine"]);
        assert!(hit.icon_url.is_some());
    }
}

pub async fn changelog(client: &reqwest::Client, version: &str) -> AppResult<String> {
    identifier(version)?;
    let value: serde_json::Value = get(client, &format!("/version/{version}"), &[]).await?;
    Ok(value["changelog"].as_str().unwrap_or("").to_string())
}
