use crate::core::{
    download::DownloadTask,
    error::{AppError, AppResult},
    instances::{Instance, InstanceStore},
    java::runtime::safe_archive_path,
    paths::AppPaths,
    util::atomic_write,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::Path,
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Index {
    format_version: u32,
    game: String,
    /// The pack's own version; required by the format, absent in old exports.
    #[serde(default)]
    version_id: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    dependencies: HashMap<String, String>,
    files: Vec<PackFile>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackFile {
    path: String,
    hashes: HashMap<String, String>,
    downloads: Vec<String>,
    file_size: u64,
    #[serde(default)]
    env: HashMap<String, String>,
}
pub(crate) fn trusted_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port_or_known_default() == Some(443)
            && matches!(
                url.host_str(),
                Some(
                    "cdn.modrinth.com"
                        | "github.com"
                        | "raw.githubusercontent.com"
                        | "objects.githubusercontent.com"
                        | "mediafilez.forgecdn.net"
                        | "edge.forgecdn.net"
                )
            )
    })
}
fn validate(index: &Index) -> AppResult<()> {
    if index.format_version != 1 || index.game != "minecraft" {
        return Err(AppError::new(
            "modpack",
            crate::core::i18n::pick("只支持 Minecraft 的 mrpack v1 格式。", "Only Minecraft mrpack v1 is supported."),
        ));
    }
    if index
        .dependencies
        .keys()
        .any(|key| !["minecraft", "fabric-loader", "quilt-loader", "forge", "neoforge", "legacy-fabric-loader", "liteloader", "cleanroom"].contains(&key.as_str()))
    {
        return Err(AppError::new(
            "modpack_loader",
            crate::core::i18n::pick("整合包需要的加载器还不支持（支持 Fabric、Quilt、Forge、NeoForge）。", "The modpack needs a loader that isn't supported (Fabric, Quilt, Forge and NeoForge are)."),
        ));
    }
    if index.dependencies.len() > 2 {
        return Err(AppError::new("modpack_loader", crate::core::i18n::pick("整合包同时要求了多个加载器。", "The modpack asks for more than one loader.")));
    }
    if !index
        .dependencies
        .get("minecraft")
        .is_some_and(|v| crate::core::fabric::valid_version(v))
    {
        return Err(AppError::new(
            "modpack",
            crate::core::i18n::pick("整合包缺少有效的 Minecraft 版本。", "The modpack has no valid Minecraft version."),
        ));
    }
    if index
        .dependencies
        .iter()
        .any(|(key, v)| key != "minecraft" && !crate::core::fabric::valid_version(v))
    {
        return Err(AppError::new("modpack", crate::core::i18n::pick("加载器版本无效。", "Invalid loader version.")));
    }
    let mut paths = HashSet::new();
    for file in &index.files {
        let path = safe_archive_path(&file.path)?;
        if !paths.insert(path.to_string_lossy().to_lowercase()) {
            return Err(AppError::new("modpack_path", crate::core::i18n::pick("整合包包含重复路径。", "The modpack has duplicate paths.")));
        }
        if !file
            .hashes
            .get("sha1")
            .is_some_and(|h| h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(AppError::new("modpack_hash", crate::core::i18n::pick("整合包文件缺少 SHA-1 校验。", "A modpack file has no SHA-1 hash.")));
        }
        if file.downloads.is_empty() || file.downloads.iter().any(|url| !trusted_url(url)) {
            return Err(AppError::new(
                "modpack_url",
                crate::core::i18n::pick("整合包包含未支持的下载来源；仅支持 Modrinth、GitHub 和 CurseForge CDN。", "The modpack uses an unsupported download source; only Modrinth, GitHub and CurseForge's CDN are allowed."),
            ));
        }
    }
    Ok(())
}

/// The loader a Modrinth pack asks for, by its `dependencies` keys.
fn pack_loader(dependencies: &std::collections::HashMap<String, String>) -> (&'static str, Option<String>) {
    for (key, loader) in [("fabric-loader", "fabric"), ("quilt-loader", "quilt"), ("forge", "forge"), ("neoforge", "neoforge"), ("legacy-fabric-loader", "legacyfabric"), ("liteloader", "liteloader"), ("cleanroom", "cleanroom")] {
        if let Some(version) = dependencies.get(key) {
            return (loader, Some(version.clone()));
        }
    }
    ("vanilla", None)
}

fn read_index(zip: &mut zip::ZipArchive<fs::File>) -> AppResult<Index> {
    let entry = zip.by_name("modrinth.index.json").map_err(zip_error)?;
    if entry.size() > 8 * 1024 * 1024 {
        return Err(AppError::new("modpack", crate::core::i18n::pick("整合包索引过大。", "The modpack index is too large.")));
    }
    let index: Index = serde_json::from_reader(entry.take(8 * 1024 * 1024))?;
    validate(&index)?;
    Ok(index)
}

/// Copies a pack's override folders into `dest`; later prefixes win, so
/// client overrides beat common ones regardless of ZIP entry order.
fn extract_overrides(zip: &mut zip::ZipArchive<fs::File>, dest_root: &Path, prefixes: &[String]) -> AppResult<()> {
    fs::create_dir_all(dest_root)?;
    let mut total = 0u64;
    for prefix in prefixes {
        let prefix = format!("{}/", prefix.trim_end_matches('/'));
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(zip_error)?;
            let entry_name = entry.name().map_err(zip_error)?;
            let Some(relative) = entry_name.strip_prefix(prefix.as_str()) else {
                continue;
            };
            if relative.is_empty() {
                continue;
            }
            let relative = safe_archive_path(relative)?;
            total = total.saturating_add(entry.size());
            if total > 8 * 1024 * 1024 * 1024 || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
                return Err(AppError::new("modpack", crate::core::i18n::pick("整合包覆盖文件超出 8 GB 或包含符号链接。", "The modpack overrides exceed 8 GB or contain symbolic links.")));
            }
            let dest = dest_root.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(dest)?;
            } else {
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                std::io::copy(&mut entry, &mut fs::File::create(dest)?)?;
            }
        }
    }
    Ok(())
}

const MRPACK_OVERRIDES: [&str; 2] = ["overrides", "client-overrides"];

/// Where an online pack came from, so its world can look for newer
/// versions: a Modrinth project and version id, or `cf:<mod>` and
/// `cf:<mod>:<file>`. Kept beside modpack.json.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSource {
    pub project: String,
    pub version: String,
}
const SOURCE_FILE: &str = "modpack-source.json";

pub fn source(paths: &AppPaths, id: &str) -> Option<PackSource> {
    AppPaths::validate_instance_id(id).ok()?;
    serde_json::from_slice(&fs::read(paths.instance_dir(id).join(SOURCE_FILE)).ok()?).ok()
}

pub fn record_source(paths: &AppPaths, id: &str, source: &PackSource) -> AppResult<()> {
    AppPaths::validate_instance_id(id)?;
    atomic_write(&paths.instance_dir(id).join(SOURCE_FILE), &serde_json::to_vec_pretty(source)?)
}

/// The newest version worth moving to from the current one (known by id,
/// or after a manual update only by the pack's own version number), if
/// any. `versions` is newest first; a world on a release is only offered
/// releases.
pub fn newer_version<'a>(versions: &'a [crate::core::modrinth::ModVersion], current: &str, number: &str) -> Option<&'a crate::core::modrinth::ModVersion> {
    let stable = |v: &crate::core::modrinth::ModVersion| v.version_type.as_deref().is_none_or(|t| t == "release");
    let found = versions
        .iter()
        .position(|v| !current.is_empty() && v.id == current)
        .or_else(|| versions.iter().position(|v| !number.is_empty() && v.version_number == number));
    let (newer, on_release) = match found {
        Some(index) => (&versions[..index], stable(&versions[index])),
        // The version was taken down: anything listed is a way forward.
        None => (versions, true),
    };
    newer.iter().find(|v| !on_release || stable(v)).or_else(|| newer.first().filter(|_| !on_release))
}

/// Downloads one version of an online modpack (a Modrinth version id, or
/// `cf:<project>:<file>`) into the cache, checked against its SHA-1.
/// Returns the file, ready to import, and where it came from; the caller
/// removes the file afterwards.
pub async fn download_online(client: &reqwest::Client, curseforge_key: &str, paths: &AppPaths, version: &str) -> AppResult<(std::path::PathBuf, PackSource)> {
    let (urls, name, sha1, project) = if version.starts_with(crate::core::curseforge::PREFIX) {
        let (urls, name, sha1) = crate::core::curseforge::modpack_file(client, curseforge_key, version).await?;
        let project = crate::core::curseforge::parse_id(version).map(|(id, _)| format!("{}{id}", crate::core::curseforge::PREFIX)).unwrap_or_default();
        (urls, name, sha1, project)
    } else {
        let (url, name, sha1, project) = crate::core::modrinth::modpack_file(client, version).await?;
        (vec![url], name, sha1, project)
    };
    let safe = !name.is_empty() && name.len() <= 200 && !name.contains(['/', '\\', '\0']) && !name.starts_with('.');
    let mut urls = urls.into_iter().filter(|url| url.starts_with("https://"));
    let (Some(first), true) = (urls.next(), safe) else {
        return Err(AppError::new("modpack", crate::core::i18n::pick("整合包下载地址无效。", "Invalid modpack download address.")));
    };
    let folder = paths.cache.join("modpacks");
    fs::create_dir_all(&folder)?;
    let target = folder.join(format!("{}-{name}", uuid::Uuid::new_v4().simple()));
    let task = DownloadTask::new(name.clone(), first, urls.collect(), target.clone(), sha1, None);
    let tasks = vec![task];
    let progress = std::sync::Arc::new(crate::core::download::DownloadProgress::new(&tasks));
    let result = crate::core::download::download_all(tasks, client.clone(), 1, tokio_util::sync::CancellationToken::new(), progress).await;
    if let Err(error) = result {
        let _ = fs::remove_file(&target);
        return Err(error);
    }
    Ok((target, PackSource { project, version: version.to_string() }))
}

/// Imports a Modrinth pack (.mrpack).
pub fn import(paths: &AppPaths, store: &InstanceStore, source: &Path) -> AppResult<Instance> {
    let mut zip = zip::ZipArchive::new(fs::File::open(source)?).map_err(zip_error)?;
    let index = read_index(&mut zip)?;
    import_index(paths, store, &mut zip, index, &MRPACK_OVERRIDES.map(String::from))
}

/// Imports a CurseForge pack whose files were already looked up.
pub fn import_curseforge(
    paths: &AppPaths,
    store: &InstanceStore,
    source: &Path,
    manifest: &crate::core::curseforge::PackManifest,
    files: Vec<crate::core::curseforge::ResolvedFile>,
) -> AppResult<Instance> {
    let index = curseforge_index(manifest, files)?;
    let mut zip = zip::ZipArchive::new(fs::File::open(source)?).map_err(zip_error)?;
    import_index(paths, store, &mut zip, index, &[manifest.overrides.clone()])
}

/// A CurseForge pack in the same shape as a Modrinth one, so installing,
/// repairing and updating treat both alike.
fn curseforge_index(manifest: &crate::core::curseforge::PackManifest, files: Vec<crate::core::curseforge::ResolvedFile>) -> AppResult<Index> {
    let mut dependencies = HashMap::from([("minecraft".to_string(), manifest.minecraft.version.clone())]);
    if let Some((loader, version)) = manifest.loader() {
        let key = match loader.as_str() {
            "fabric" => "fabric-loader",
            "quilt" => "quilt-loader",
            "legacyfabric" => "legacy-fabric-loader",
            other => other,
        };
        dependencies.insert(key.to_string(), version);
    }
    let index = Index {
        format_version: 1,
        game: "minecraft".into(),
        version_id: manifest.version.trim().to_string(),
        summary: None,
        name: if manifest.name.trim().is_empty() { crate::core::i18n::pick("CurseForge 整合包", "CurseForge modpack").into() } else { manifest.name.clone() },
        dependencies,
        files: files
            .into_iter()
            .map(|file| PackFile {
                path: file.path,
                hashes: HashMap::from([("sha1".to_string(), file.sha1)]),
                downloads: file.urls,
                file_size: file.size,
                env: HashMap::new(),
            })
            .collect(),
    };
    validate(&index)?;
    Ok(index)
}

fn import_index(paths: &AppPaths, store: &InstanceStore, zip: &mut zip::ZipArchive<fs::File>, index: Index, prefixes: &[String]) -> AppResult<Instance> {
    let id = uuid::Uuid::new_v4().to_string();
    let stage = paths.instances.join(format!(".import-{id}"));
    let prepared = (|| -> AppResult<Instance> {
        fs::create_dir_all(stage.join("minecraft/mods"))?;
        extract_overrides(zip, &stage.join("modpack-overrides"), prefixes)?;
        let (loader, loader_version) = pack_loader(&index.dependencies);
        let instance = Instance {
            id: id.clone(),
            name: index.name.chars().take(40).collect(),
            version_id: index.dependencies["minecraft"].clone(),
            loader: loader.to_string(),
            loader_version,
            created_at: crate::core::util::now_millis(),
            ..Instance::default()
        };
        atomic_write(
            &stage.join("instance.json"),
            &serde_json::to_vec_pretty(&instance)?,
        )?;
        atomic_write(
            &stage.join("modpack.json"),
            &serde_json::to_vec_pretty(&index)?,
        )?;
        fs::rename(&stage, paths.instance_dir(&id))?;
        store.update(&instance)?;
        Ok(instance)
    })();
    if prepared.is_err() {
        let _ = fs::remove_dir_all(stage);
    }
    prepared
}

/// The pack a world came from: its name and version, when it is one.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackInfo {
    pub name: String,
    pub version: String,
    pub files: usize,
    /// Set for packs installed from Modrinth or CurseForge, which can then
    /// be updated online.
    pub source: Option<PackSource>,
}

pub fn info(paths: &AppPaths, id: &str) -> Option<PackInfo> {
    AppPaths::validate_instance_id(id).ok()?;
    let index: Index = serde_json::from_slice(&fs::read(paths.instance_dir(id).join("modpack.json")).ok()?).ok()?;
    Some(PackInfo { name: index.name, version: index.version_id, files: index.files.len(), source: source(paths, id) })
}

// ---------- export ----------

/// What goes into an exported pack besides mods, packs and configs.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportOptions {
    pub name: String,
    pub version: String,
    pub summary: String,
    /// `"curseforge"` for a CurseForge .zip; anything else is a .mrpack.
    pub format: String,
    /// The game's own settings (options.txt).
    pub options: bool,
    pub servers: bool,
    pub saves: bool,
    /// More files for the overrides, by path in the game folder.
    #[serde(skip)]
    pub extra: Vec<(String, Vec<u8>)>,
}

impl ExportOptions {
    pub fn curseforge(&self) -> bool {
        self.format == "curseforge"
    }
}

/// Folders of settings that make a pack what it is.
const CONFIG_FOLDERS: &[&str] = &["config", "defaultconfigs", "kubejs", "scripts", "global_packs", "paxi", "openloader"];

/// An enabled mod or pack of a world, with the hashes Modrinth and
/// CurseForge look files up by.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// Path in the game folder ("mods/sodium.jar").
    pub path: String,
    pub sha1: String,
    pub fingerprint: u32,
}

pub fn export_candidates(paths: &AppPaths, id: &str) -> AppResult<Vec<Candidate>> {
    use sha1::Digest;
    let game = paths.instance_game_dir(id);
    let mut list = Vec::new();
    for (folder, extension) in [("mods", ".jar"), ("resourcepacks", ".zip"), ("shaderpacks", ".zip")] {
        for entry in fs::read_dir(game.join(folder)).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !entry.file_type().is_ok_and(|t| t.is_file()) || !name.to_lowercase().ends_with(extension) {
                continue;
            }
            let bytes = fs::read(entry.path())?;
            list.push(Candidate {
                path: format!("{folder}/{name}"),
                sha1: hex::encode(sha1::Sha1::digest(&bytes)),
                fingerprint: crate::core::curseforge::fingerprint(&bytes),
            });
        }
    }
    Ok(list)
}

/// The files a host already serves, which the pack links instead of
/// carrying: Modrinth files by SHA-1, CurseForge (project, file) by
/// fingerprint.
pub enum Hosted<'a> {
    Modrinth(&'a HashMap<String, crate::core::modrinth::ModFile>),
    CurseForge(&'a HashMap<u32, (u64, u64)>),
}

/// The `minecraft.modLoaders[].id` CurseForge uses ("forge-47.2.0").
fn curseforge_loader(instance: &Instance, version: &str) -> String {
    let name = match instance.loader.as_str() {
        "legacyfabric" => "fabric",
        other => other,
    };
    format!("{name}-{version}")
}

/// Writes the world as a Modrinth pack or a CurseForge pack. Files the host
/// serves are linked; everything else rides along as overrides. Returns
/// (linked, bundled) counts.
pub fn export(
    paths: &AppPaths,
    instance: &Instance,
    candidates: &[Candidate],
    hosted: Hosted,
    options: &ExportOptions,
    destination: &Path,
) -> AppResult<(usize, usize)> {
    use std::io::Write;
    let loader_version = if instance.loader == "vanilla" {
        None
    } else {
        Some(
            instance
                .loader_version
                .clone()
                .ok_or_else(|| AppError::new("modpack_export", crate::core::i18n::pick("先安装一次这个世界，确定加载器版本后再导出。", "Install this world once so its loader version is fixed, then export.")))?,
        )
    };
    let name = if options.name.trim().is_empty() { instance.name.clone() } else { options.name.trim().to_string() };
    let version = if options.version.trim().is_empty() { "1.0.0".to_string() } else { options.version.trim().to_string() };
    let mut bundled: Vec<String> = Vec::new();
    let (manifest_name, manifest, linked) = match hosted {
        Hosted::Modrinth(hosted) => {
            let mut dependencies = HashMap::from([("minecraft".to_string(), instance.version_id.clone())]);
            if let Some(loader_version) = &loader_version {
                let key = match instance.loader.as_str() {
                    "fabric" => "fabric-loader",
                    "quilt" => "quilt-loader",
                    "legacyfabric" => "legacy-fabric-loader",
                    other => other,
                };
                dependencies.insert(key.to_string(), loader_version.clone());
            }
            let mut files = Vec::new();
            for candidate in candidates {
                match hosted.get(&candidate.sha1) {
                    Some(file) if file.hashes.contains_key("sha512") && trusted_url(&file.url) => files.push(PackFile {
                        path: candidate.path.clone(),
                        hashes: HashMap::from([("sha1".to_string(), candidate.sha1.clone()), ("sha512".to_string(), file.hashes["sha512"].clone())]),
                        downloads: vec![file.url.clone()],
                        file_size: file.size,
                        env: HashMap::new(),
                    }),
                    _ => bundled.push(candidate.path.clone()),
                }
            }
            let index = Index {
                format_version: 1,
                game: "minecraft".into(),
                version_id: version,
                name,
                summary: Some(options.summary.trim().to_string()).filter(|s| !s.is_empty()),
                dependencies,
                files,
            };
            ("modrinth.index.json", serde_json::to_vec_pretty(&index)?, index.files.len())
        }
        Hosted::CurseForge(hosted) => {
            let mut files = Vec::new();
            let mut seen = HashSet::new();
            for candidate in candidates {
                match hosted.get(&candidate.fingerprint) {
                    Some(&(project, file)) if seen.insert(file) => files.push(serde_json::json!({ "projectID": project, "fileID": file, "required": true })),
                    _ => bundled.push(candidate.path.clone()),
                }
            }
            let loaders: Vec<serde_json::Value> = loader_version
                .iter()
                .map(|v| serde_json::json!({ "id": curseforge_loader(instance, v), "primary": true }))
                .collect();
            let linked = files.len();
            let manifest = serde_json::json!({
                "minecraft": { "version": instance.version_id, "modLoaders": loaders },
                "manifestType": "minecraftModpack",
                "manifestVersion": 1,
                "name": name,
                "version": version,
                "author": "",
                "files": files,
                "overrides": "overrides",
            });
            ("manifest.json", serde_json::to_vec_pretty(&manifest)?, linked)
        }
    };
    let game = paths.instance_game_dir(&instance.id);
    let temporary = destination.with_extension("part");
    let result = (|| -> AppResult<()> {
        let mut zip = zip::ZipWriter::new(fs::File::create(&temporary)?);
        let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file(manifest_name, deflate).map_err(zip_error)?;
        zip.write_all(&manifest)?;
        let mut add = |relative: &str| -> AppResult<()> {
            let path = game.join(relative);
            if path.is_dir() {
                crate::core::content::archive_tree(&mut zip, &path, &Path::new("overrides").join(relative))?;
            } else if path.is_file() {
                zip.start_file(format!("overrides/{relative}"), deflate).map_err(zip_error)?;
                std::io::copy(&mut fs::File::open(&path)?, &mut zip)?;
            }
            Ok(())
        };
        for folder in CONFIG_FOLDERS {
            add(folder)?;
        }
        for path in &bundled {
            add(path)?;
        }
        if options.options {
            for file in ["options.txt", "optionsof.txt", "optionsshaders.txt"] {
                add(file)?;
            }
        }
        if options.servers {
            add("servers.dat")?;
        }
        if options.saves {
            add("saves")?;
        }
        for (path, bytes) in &options.extra {
            zip.start_file(format!("overrides/{path}"), deflate).map_err(zip_error)?;
            zip.write_all(bytes)?;
        }
        zip.finish().map_err(zip_error)?.sync_all()?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            fs::rename(&temporary, destination)?;
            Ok((linked, bundled.len()))
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error)
        }
    }
}

/// What a pack update did, for the message after it.
#[derive(Debug, Default)]
pub struct PackUpdate {
    pub removed: usize,
    pub version_changed: bool,
}

/// Moves a modpack world to a newer version of its pack (an .mrpack, or a
/// CurseForge pack already resolved). Files the old version had and the new
/// one dropped go to the recycle bin, unless the player changed them; the
/// new overrides apply at the next install, which the world now needs.
pub fn update(
    paths: &AppPaths,
    store: &InstanceStore,
    id: &str,
    source: &Path,
    curseforge: Option<(&crate::core::curseforge::PackManifest, Vec<crate::core::curseforge::ResolvedFile>)>,
) -> AppResult<PackUpdate> {
    let mut instance = store.get(id)?;
    let root = paths.instance_dir(id);
    let old: Index = fs::read(root.join("modpack.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or_else(|| AppError::new("modpack", crate::core::i18n::pick("这个世界不是从整合包导入的，无法按整合包更新。", "This world wasn't imported from a modpack, so it can't be updated as one.")))?;
    let mut zip = zip::ZipArchive::new(fs::File::open(source)?).map_err(zip_error)?;
    let (index, prefixes) = match curseforge {
        Some((manifest, files)) => (curseforge_index(manifest, files)?, vec![manifest.overrides.clone()]),
        None => (read_index(&mut zip)?, MRPACK_OVERRIDES.map(String::from).to_vec()),
    };
    // New overrides first: a broken pack must leave the world as it was.
    let staged = root.join(format!(".overrides-{}", uuid::Uuid::new_v4().simple()));
    if let Err(error) = extract_overrides(&mut zip, &staged, &prefixes) {
        let _ = fs::remove_dir_all(&staged);
        return Err(error);
    }
    let keep: HashSet<&str> = index.files.iter().map(|f| f.path.as_str()).collect();
    let game = paths.instance_game_dir(id);
    let mut result = PackUpdate::default();
    for file in old.files.iter().filter(|f| !keep.contains(f.path.as_str())) {
        let Ok(relative) = safe_archive_path(&file.path) else {
            continue;
        };
        let path = game.join(&relative);
        let disabled = path.with_file_name(format!("{}.disabled", path.file_name().unwrap_or_default().to_string_lossy()));
        let Some(found) = [path, disabled].into_iter().find(|p| p.is_file()) else {
            continue;
        };
        // Only the pack's own unchanged file goes.
        let same = fs::read(&found).ok().is_some_and(|bytes| {
            use sha1::Digest;
            Some(hex::encode(sha1::Sha1::digest(&bytes))) == file.hashes.get("sha1").map(|h| h.to_lowercase())
        });
        let Some(folder) = relative.parent().and_then(|p| p.to_str()) else {
            continue;
        };
        let kind = match folder {
            "mods" => crate::core::manage::ItemKind::Mod,
            "resourcepacks" => crate::core::manage::ItemKind::ResourcePack,
            "shaderpacks" => crate::core::manage::ItemKind::Shader,
            _ => continue,
        };
        if same {
            let name = found.file_name().unwrap_or_default().to_string_lossy().into_owned();
            crate::core::manage::recycle_item(paths, id, kind, &name, None)?;
            result.removed += 1;
        }
    }
    let overrides = root.join("modpack-overrides");
    if overrides.exists() {
        fs::remove_dir_all(&overrides)?;
    }
    fs::rename(&staged, &overrides)?;
    let _ = fs::remove_file(root.join(".overrides-applied"));
    let _ = fs::remove_file(root.join(REPLACED_FILE));
    let (loader, loader_version) = pack_loader(&index.dependencies);
    let version = index.dependencies["minecraft"].clone();
    result.version_changed = version != instance.version_id || loader != instance.loader || loader_version != instance.loader_version;
    if result.version_changed {
        let _ = fs::remove_file(root.join("launch-version.json"));
    }
    atomic_write(&root.join("modpack.json"), &serde_json::to_vec_pretty(&index)?)?;
    // A chosen file's version id is unknown; the caller records it when the
    // update came from online.
    if let Some(mut known) = self::source(paths, id) {
        known.version.clear();
        record_source(paths, id, &known)?;
    }
    instance.version_id = version;
    instance.loader = loader.to_string();
    instance.loader_version = loader_version;
    instance.installed = false;
    store.update(&instance)?;
    Ok(result)
}

/// Modpack files that a later mod update replaced, by their path in the
/// pack ("mods/sodium-0.5.jar"). Kept beside modpack.json.
const REPLACED_FILE: &str = "modpack-replaced.json";

fn replaced_files(paths: &AppPaths, id: &str) -> HashSet<String> {
    fs::read(paths.instance_dir(id).join(REPLACED_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Notes what a mod update took out (`removed`) and put in (`added`), so
/// repairing a modpack instance does not download the old file again next
/// to its newer version. Instances that are not modpacks keep no record.
pub fn record_replaced(paths: &AppPaths, id: &str, removed: &[String], added: &[String]) -> AppResult<()> {
    if !paths.instance_dir(id).join("modpack.json").is_file() {
        return Ok(());
    }
    let mut replaced = replaced_files(paths, id);
    let before = replaced.clone();
    replaced.extend(removed.iter().cloned());
    // Going back to a pack's own file makes it the pack's again.
    for path in added {
        replaced.remove(path);
    }
    if replaced == before {
        return Ok(());
    }
    let mut sorted: Vec<&String> = replaced.iter().collect();
    sorted.sort();
    atomic_write(&paths.instance_dir(id).join(REPLACED_FILE), &serde_json::to_vec_pretty(&sorted)?)
}

pub fn downloads(paths: &AppPaths, instance: &Instance) -> AppResult<Vec<DownloadTask>> {
    let path = paths.instance_dir(&instance.id).join("modpack.json");
    if !path.is_file() {
        return Ok(vec![]);
    }
    let index: Index = serde_json::from_slice(&fs::read(path)?)?;
    validate(&index)?;
    let replaced = replaced_files(paths, &instance.id);
    index
        .files
        .into_iter()
        .filter(|f| f.env.get("client").is_none_or(|v| v != "unsupported"))
        .filter(|f| !replaced.contains(&f.path))
        .map(|file| {
            let mut urls = file.downloads.into_iter();
            Ok(DownloadTask::new(
                file.path.clone(),
                urls.next().unwrap(),
                urls.collect(),
                {
                    let target = paths
                        .instance_game_dir(&instance.id)
                        .join(safe_archive_path(&file.path)?);
                    let disabled = target.with_file_name(format!(
                        "{}.disabled",
                        target.file_name().unwrap_or_default().to_string_lossy()
                    ));
                    if file.path.ends_with(".jar") && disabled.is_file() && !target.exists() {
                        disabled
                    } else {
                        target
                    }
                },
                file.hashes.get("sha1").cloned(),
                Some(file.file_size),
            ))
        })
        .collect()
}
fn zip_error(error: zip::result::ZipError) -> AppError {
    AppError::detail("modpack", crate::core::i18n::pick("无法读取 mrpack 文件。", "Couldn't read the mrpack file."), error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offers_the_newest_release_after_the_current_version() {
        let version = |id: &str, number: &str, kind: &str| crate::core::modrinth::ModVersion {
            id: id.into(),
            project_id: "p".into(),
            name: number.into(),
            version_number: number.into(),
            game_versions: vec![],
            loaders: vec![],
            files: vec![],
            dependencies: vec![],
            version_type: Some(kind.into()),
            date_published: None,
        };
        let list = [version("d", "4.0-beta", "beta"), version("c", "3.0", "release"), version("b", "2.0", "release"), version("a", "1.0", "release")];
        assert_eq!(newer_version(&list, "a", "").map(|v| v.id.as_str()), Some("c"));
        assert_eq!(newer_version(&list, "", "2.0").map(|v| v.id.as_str()), Some("c"));
        assert!(newer_version(&list, "c", "3.0").is_none());
        // A world already on a beta follows betas.
        let betas = [version("e", "5.0-beta", "beta"), version("d", "4.0-beta", "beta")];
        assert_eq!(newer_version(&betas, "d", "").map(|v| v.id.as_str()), Some("e"));
        assert!(newer_version(&betas, "e", "").is_none());
    }

    #[test]
    fn refuses_unsupported_loader_and_untrusted_sources() {
        assert!(trusted_url("https://cdn.modrinth.com/data/a/file.jar"));
        assert!(!trusted_url("http://cdn.modrinth.com/file.jar"));
        assert!(!trusted_url("https://127.0.0.1/secret"));
        assert!(!trusted_url("https://cdn.modrinth.com.evil.test/file"));
        let index = Index {
            format_version: 1,
            game: "minecraft".into(),
            version_id: String::new(),
            summary: None,
            name: "pack".into(),
            files: vec![],
            dependencies: HashMap::from([
                ("minecraft".into(), "1.21".into()),
                ("unknown-loader".into(), "1".into()),
            ]),
        };
        assert!(validate(&index).is_err());
    }

    #[test]
    fn a_pack_update_drops_only_unchanged_old_files() {
        use sha1::Digest;
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let store = InstanceStore::load(paths.clone()).unwrap();
        let sha = |bytes: &[u8]| hex::encode(sha1::Sha1::digest(bytes));
        let file = |path: &str, hash: String| PackFile {
            path: path.into(),
            hashes: HashMap::from([("sha1".into(), hash)]),
            downloads: vec![format!("https://cdn.modrinth.com/data/x/{path}")],
            file_size: 1,
            env: HashMap::new(),
        };
        let pack = |files: Vec<PackFile>, mc: &str| Index {
            format_version: 1,
            game: "minecraft".into(),
            version_id: "1".into(),
            summary: None,
            name: "包".into(),
            files,
            dependencies: HashMap::from([("minecraft".into(), mc.into()), ("forge".into(), "47.2.0".into())]),
        };
        let write_pack = |path: &Path, index: &Index| {
            let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("modrinth.index.json", options).unwrap();
            zip.write_all(&serde_json::to_vec(index).unwrap()).unwrap();
            zip.start_file("overrides/config/a.toml", options).unwrap();
            zip.write_all(b"new=1").unwrap();
            zip.finish().unwrap();
        };
        let v1 = dir.path().join("v1.mrpack");
        write_pack(&v1, &pack(vec![file("mods/old.jar", sha(b"old")), file("mods/edited.jar", sha(b"pack")), file("mods/kept.jar", sha(b"kept"))], "1.20.1"));
        let instance = import(&paths, &store, &v1).unwrap();
        assert_eq!((instance.loader.as_str(), instance.loader_version.as_deref()), ("forge", Some("47.2.0")));
        let mods = paths.instance_game_dir(&instance.id).join("mods");
        fs::write(mods.join("old.jar"), b"old").unwrap();
        fs::write(mods.join("edited.jar"), b"player changed this").unwrap();
        fs::write(mods.join("kept.jar"), b"kept").unwrap();
        fs::write(paths.instance_dir(&instance.id).join(".overrides-applied"), b"").unwrap();

        let v2 = dir.path().join("v2.mrpack");
        write_pack(&v2, &pack(vec![file("mods/kept.jar", sha(b"kept")), file("mods/new.jar", sha(b"new"))], "1.20.2"));
        let result = update(&paths, &store, &instance.id, &v2, None).unwrap();
        assert_eq!(result.removed, 1);
        assert!(result.version_changed);
        assert!(!mods.join("old.jar").exists());
        assert!(mods.join("edited.jar").exists(), "a file the player changed stays");
        assert!(mods.join("kept.jar").exists());
        let updated = store.get(&instance.id).unwrap();
        assert_eq!(updated.version_id, "1.20.2");
        assert!(!updated.installed);
        // New overrides wait for the next install.
        assert!(!paths.instance_dir(&instance.id).join(".overrides-applied").exists());
        assert!(paths.instance_dir(&instance.id).join("modpack-overrides/config/a.toml").is_file());
    }

    #[test]
    fn export_links_hosted_files_and_bundles_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let store = InstanceStore::load(paths.clone()).unwrap();
        let mut instance = store.create("导出", "1.21.1", "release").unwrap();
        instance.loader = "fabric".into();
        instance.loader_version = Some("0.16.0".into());
        let game = paths.instance_game_dir(&instance.id);
        fs::create_dir_all(game.join("mods")).unwrap();
        fs::create_dir_all(game.join("config")).unwrap();
        fs::write(game.join("mods/sodium.jar"), b"sodium").unwrap();
        fs::write(game.join("mods/own.jar"), b"own").unwrap();
        fs::write(game.join("mods/off.jar.disabled"), b"off").unwrap();
        fs::write(game.join("config/sodium.json"), b"{}").unwrap();
        fs::write(game.join("options.txt"), b"fov:0").unwrap();
        let candidates = export_candidates(&paths, &instance.id).unwrap();
        assert_eq!(candidates.len(), 2, "disabled mods stay out");
        let sodium = candidates.iter().find(|c| c.path == "mods/sodium.jar").unwrap().sha1.clone();
        let hosted = HashMap::from([(
            sodium.clone(),
            crate::core::modrinth::ModFile {
                url: "https://cdn.modrinth.com/data/AANobbMI/versions/x/sodium.jar".into(),
                filename: "sodium.jar".into(),
                primary: true,
                size: 6,
                hashes: std::collections::BTreeMap::from([("sha1".into(), sodium), ("sha512".into(), "b".repeat(128))]),
            },
        )]);
        let out = dir.path().join("pack.mrpack");
        let options = ExportOptions { version: "2.0".into(), ..ExportOptions::default() };
        assert_eq!(export(&paths, &instance, &candidates, Hosted::Modrinth(&hosted), &options, &out).unwrap(), (1, 1));
        let mut zip = zip::ZipArchive::new(fs::File::open(&out).unwrap()).unwrap();
        let index: Index = serde_json::from_reader(zip.by_name("modrinth.index.json").unwrap()).unwrap();
        assert_eq!(index.version_id, "2.0");
        assert_eq!(index.dependencies.get("fabric-loader").map(String::as_str), Some("0.16.0"));
        assert_eq!(index.files[0].path, "mods/sodium.jar");
        assert!(zip.by_name("overrides/mods/own.jar").is_ok());
        assert!(zip.by_name("overrides/config/sodium.json").is_ok());
        assert!(zip.by_name("overrides/options.txt").is_err(), "settings only when asked");

        // The same world as a CurseForge pack, which this launcher reads back.
        let fingerprint = candidates.iter().find(|c| c.path == "mods/sodium.jar").unwrap().fingerprint;
        let hosted = HashMap::from([(fingerprint, (394468u64, 5_000_000u64))]);
        let out = dir.path().join("pack.zip");
        let options = ExportOptions { format: "curseforge".into(), version: "2.0".into(), ..ExportOptions::default() };
        assert_eq!(export(&paths, &instance, &candidates, Hosted::CurseForge(&hosted), &options, &out).unwrap(), (1, 1));
        let manifest = crate::core::curseforge::read_manifest(&out).unwrap().expect("a CurseForge manifest");
        assert_eq!(manifest.version, "2.0");
        assert_eq!(manifest.loader(), Some(("fabric".into(), "0.16.0".into())));
        assert_eq!((manifest.files[0].project_id, manifest.files[0].file_id), (394468, 5_000_000));
        let mut zip = zip::ZipArchive::new(fs::File::open(&out).unwrap()).unwrap();
        assert!(zip.by_name("overrides/mods/own.jar").is_ok());
        assert!(zip.by_name("overrides/mods/sodium.jar").is_err());
    }

    #[test]
    fn repair_skips_files_a_mod_update_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let instance = Instance { id: "pack-1".into(), ..Instance::default() };
        let root = paths.instance_dir(&instance.id);
        fs::create_dir_all(&root).unwrap();
        let file = |path: &str| PackFile {
            path: path.into(),
            hashes: HashMap::from([("sha1".into(), "a".repeat(40))]),
            downloads: vec![format!("https://cdn.modrinth.com/data/x/{path}")],
            file_size: 1,
            env: HashMap::new(),
        };
        let index = Index {
            format_version: 1,
            game: "minecraft".into(),
            version_id: String::new(),
            summary: None,
            name: "pack".into(),
            files: vec![file("mods/sodium-v1.jar"), file("mods/lithium.jar")],
            dependencies: HashMap::from([("minecraft".into(), "1.21".into()), ("fabric-loader".into(), "0.16".into())]),
        };
        fs::write(root.join("modpack.json"), serde_json::to_vec(&index).unwrap()).unwrap();
        let names = |paths: &AppPaths| -> Vec<String> {
            downloads(paths, &instance).unwrap().iter().map(|t| t.label.clone()).collect()
        };
        assert_eq!(names(&paths), ["mods/sodium-v1.jar", "mods/lithium.jar"]);

        // Updating sodium to v2 drops v1 from repairs.
        record_replaced(&paths, &instance.id, &["mods/sodium-v1.jar".into()], &["mods/sodium-v2.jar".into()]).unwrap();
        assert_eq!(names(&paths), ["mods/lithium.jar"]);
        // Going back to the pack's own v1 brings it back.
        record_replaced(&paths, &instance.id, &["mods/sodium-v2.jar".into()], &["mods/sodium-v1.jar".into()]).unwrap();
        assert_eq!(names(&paths), ["mods/sodium-v1.jar", "mods/lithium.jar"]);

        // Instances that are not modpacks keep no record.
        record_replaced(&paths, "plain", &["mods/a.jar".into()], &[]).unwrap();
        assert!(!paths.instance_dir("plain").join(REPLACED_FILE).exists());
    }
}

/// Apply overrides after downloads, once. Repair must preserve user-edited config.
pub fn apply_overrides(paths: &AppPaths, instance: &Instance) -> AppResult<()> {
    let root = paths.instance_dir(&instance.id);
    let source = root.join("modpack-overrides");
    let marker = root.join(".overrides-applied");
    if !source.is_dir() || marker.exists() {
        return Ok(());
    }
    fn copy(source: &Path, dest: &Path) -> AppResult<()> {
        fs::create_dir_all(dest)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                copy(&entry.path(), &dest.join(entry.file_name()))?;
            } else if entry.file_type()?.is_file() {
                fs::copy(entry.path(), dest.join(entry.file_name()))?;
            } else {
                return Err(AppError::new("modpack_path", "覆盖目录包含非普通文件。"));
            }
        }
        Ok(())
    }
    copy(&source, &paths.instance_game_dir(&instance.id))?;
    atomic_write(&marker, b"1")
}
