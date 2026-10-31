use std::collections::HashSet;
use std::path::PathBuf;

use tokio_util::sync::CancellationToken;

use crate::core::download::DownloadTask;
use crate::core::error::{AppError, AppResult};
use crate::core::mojang::manifest::{self, VersionSummary};
use crate::core::mojang::rules::{RuleEnv, rules_allow_in};
use crate::core::mojang::version::{
    self, AssetIndexJson, VersionJson, java_major, version_asset_index_id,
};
use crate::core::paths::AppPaths;
use crate::core::settings::DownloadSource;
use crate::core::util::{atomic_write, sha1_hex};

const OFFICIAL_LIBRARIES: &str = "https://libraries.minecraft.net/";
const OFFICIAL_ASSETS: &str = "https://resources.download.minecraft.net/";
const BMCLAPI_BASE: &str = "https://bmclapi2.bangbang93.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Client,
    Library,
    Native,
    Asset,
    Logging,
}

#[derive(Debug, Clone)]
pub struct InstallFile {
    pub kind: FileKind,
    pub task: DownloadTask,
}

#[derive(Debug, Clone)]
pub struct InstallPlan {
    pub version_id: String,
    pub version: VersionJson,
    pub files: Vec<InstallFile>,
    pub total_bytes: u64,
    pub java_major: u32,
    /// What the game runs as; see `version::game_arch`.
    pub arch: &'static str,
}

/// Fetch official metadata and turn it into a list of file downloads.
/// Game metadata is always fetched from Mojang; BMCLAPI is only used as a
/// faster file host and is verified against Mojang's SHA-1 hashes.
pub async fn build_plan(
    client: &reqwest::Client,
    paths: &AppPaths,
    version_id: &str,
    source: DownloadSource,
    cancel: &CancellationToken,
) -> AppResult<InstallPlan> {
    check_cancel(cancel)?;
    let manifest = manifest::load_manifest(client, paths, false).await?;
    let summary = manifest
        .versions
        .iter()
        .find(|version| version.id == version_id)
        .cloned()
        .ok_or_else(|| AppError::new("version_not_found", crate::core::i18n::pick("找不到该 Minecraft 版本。", "Minecraft version not found.")))?;

    let version = load_version_json(client, paths, &summary, cancel).await?;
    let arch = version::game_arch(&version);
    let env = RuleEnv { arch, custom_resolution: false };
    if version.inherits_from.is_some() {
        return Err(AppError::new(
            "inherited_version",
            crate::core::i18n::pick("原版下载入口不能直接读取继承配置，请通过加载器安装流程安装。", "Inherited versions (Forge, NeoForge and other loaders) aren't supported here; use vanilla first."),
        ));
    }

    let asset_index_id = version_asset_index_id(&version)
        .ok_or_else(|| AppError::new("asset_index", crate::core::i18n::pick("版本缺少资源索引信息。", "The version has no asset index.")))?
        .to_string();
    let asset_index = load_asset_index(client, paths, &version, &asset_index_id, cancel).await?;

    check_cancel(cancel)?;
    let mut files = Vec::new();
    let mut seen_dest: HashSet<PathBuf> = HashSet::new();
    let mut total_bytes = 0u64;

    if let Some(client_jar) = &version.downloads.client {
        let official = client_jar.url.clone().unwrap_or_default();
        if official.is_empty() {
            return Err(AppError::new("client_jar", crate::core::i18n::pick("版本缺少客户端下载地址。", "The version has no client download address.")));
        }
        let mirror = format!("{BMCLAPI_BASE}/version/{version_id}/client");
        let (primary, fallback) = url_for_source(source, &mirror, &official);
        let task = DownloadTask::new(
            crate::core::i18n::pick("Minecraft 客户端", "Minecraft client"),
            primary,
            fallback,
            paths.client_jar(version_id),
            client_jar.sha1.clone(),
            client_jar.size,
        );
        total_bytes += task.size.unwrap_or(0);
        files.push(InstallFile {
            kind: FileKind::Client,
            task,
        });
    } else {
        return Err(AppError::new("client_jar", crate::core::i18n::pick("版本缺少客户端下载信息。", "The version has no client download info.")));
    }

    for library in &version.libraries {
        if !rules_allow_in(&library.rules, env) {
            continue;
        }
        if let Some(artifact) = library
            .downloads
            .as_ref()
            .and_then(|downloads| downloads.artifact.as_ref())
        {
            if let Some(relative) = version::artifact_path(library) {
                let dest = paths.library_path(&relative);
                if seen_dest.insert(dest.clone()) {
                    let official_url = artifact
                        .url
                        .clone()
                        .unwrap_or_else(|| format!("{OFFICIAL_LIBRARIES}{relative}"));
                    let mirror_url = format!("{BMCLAPI_BASE}/maven/{relative}");
                    let (primary, fallback) = url_for_source(source, &mirror_url, &official_url);
                    let task = DownloadTask::new(
                        crate::tr!("依赖库 {}", "Library {}", library.name),
                        primary,
                        fallback,
                        dest,
                        artifact.sha1.clone(),
                        artifact.size,
                    );
                    total_bytes += task.size.unwrap_or(0);
                    files.push(InstallFile {
                        kind: FileKind::Library,
                        task,
                    });
                }
            }
        }

        if let Some(native) = version::native_download_for(library, arch) {
            let relative = native
                .path
                .clone()
                .or_else(|| version::artifact_path(library))
                .or_else(|| version::maven_path(&library.name));
            if let Some(relative) = relative {
                let dest = paths.library_path(&relative);
                if seen_dest.insert(dest.clone()) {
                    let official_url = native
                        .url
                        .clone()
                        .unwrap_or_else(|| format!("{OFFICIAL_LIBRARIES}{relative}"));
                    let mirror_url = format!("{BMCLAPI_BASE}/maven/{relative}");
                    let (primary, fallback) = url_for_source(source, &mirror_url, &official_url);
                    let task = DownloadTask::new(
                        crate::tr!("本地库 {}", "Native library {}", library.name),
                        primary,
                        fallback,
                        dest,
                        native.sha1.clone(),
                        native.size,
                    );
                    total_bytes += task.size.unwrap_or(0);
                    files.push(InstallFile {
                        kind: FileKind::Native,
                        task,
                    });
                }
            }
        }
    }

    let mut seen_assets = HashSet::new();
    for object in asset_index.objects.values() {
        if object.hash.len() < 2 || !seen_assets.insert(object.hash.clone()) {
            continue;
        }
        let prefix = &object.hash[0..2];
        let official_url = format!("{OFFICIAL_ASSETS}{prefix}/{}", object.hash);
        let mirror_url = format!("{BMCLAPI_BASE}/assets/{prefix}/{}", object.hash);
        let (primary, fallback) = url_for_source(source, &mirror_url, &official_url);
        let task = DownloadTask::new(
            crate::tr!("资源文件 {}", "Asset {}", object.hash),
            primary,
            fallback,
            paths.asset_object(&object.hash),
            Some(object.hash.clone()),
            Some(object.size),
        );
        total_bytes += object.size;
        files.push(InstallFile {
            kind: FileKind::Asset,
            task,
        });
    }

    if let Some(logging) = version
        .logging
        .as_ref()
        .and_then(|logging| logging.client.as_ref())
    {
        if let Some(url) = logging.file.url.clone() {
            let file_name = version::log_config_name(logging);
            let dest = paths.assets.join("log_configs").join(file_name);
            let task = DownloadTask::new(
                crate::core::i18n::pick("日志配置", "Logging config"),
                url.clone(),
                Vec::new(),
                dest,
                logging.file.sha1.clone(),
                logging.file.size,
            );
            total_bytes += task.size.unwrap_or(0);
            files.push(InstallFile {
                kind: FileKind::Logging,
                task,
            });
        }
    }

    check_cancel(cancel)?;
    Ok(InstallPlan {
        version_id: version_id.to_string(),
        java_major: java_major(&version),
        arch,
        total_bytes,
        files,
        version,
    })
}

async fn load_version_json(
    client: &reqwest::Client,
    paths: &AppPaths,
    summary: &VersionSummary,
    cancel: &CancellationToken,
) -> AppResult<VersionJson> {
    let dest = paths.version_json(&summary.id);
    if let Ok(bytes) = std::fs::read(&dest) {
        if summary.sha1.is_empty() || sha1_hex(&bytes).eq_ignore_ascii_case(&summary.sha1) {
            if let Ok(version) = serde_json::from_slice::<VersionJson>(&bytes) {
                return Ok(version);
            }
        }
    }

    check_cancel(cancel)?;
    let bytes = fetch_bytes(client, &summary.url, Some(&summary.sha1), cancel).await?;
    atomic_write(&dest, &bytes)?;
    serde_json::from_slice(&bytes)
        .map_err(|err| AppError::detail("version_json", crate::core::i18n::pick("解析版本配置失败。", "Couldn't parse the version profile."), err.to_string()))
}

async fn load_asset_index(
    client: &reqwest::Client,
    paths: &AppPaths,
    version: &VersionJson,
    asset_index_id: &str,
    cancel: &CancellationToken,
) -> AppResult<AssetIndexJson> {
    let index = version
        .asset_index
        .as_ref()
        .ok_or_else(|| AppError::new("asset_index", crate::core::i18n::pick("版本缺少资源索引信息。", "The version has no asset index.")))?;
    let dest = paths.asset_index(asset_index_id);

    if let Ok(bytes) = std::fs::read(&dest) {
        if index
            .sha1
            .as_ref()
            .is_none_or(|expected| sha1_hex(&bytes).eq_ignore_ascii_case(expected))
        {
            if let Ok(parsed) = serde_json::from_slice::<AssetIndexJson>(&bytes) {
                return Ok(parsed);
            }
        }
    }

    let url = index
        .url
        .clone()
        .ok_or_else(|| AppError::new("asset_index", crate::core::i18n::pick("资源索引缺少下载地址。", "The asset index has no download address.")))?;
    check_cancel(cancel)?;
    let bytes = fetch_bytes(client, &url, index.sha1.as_deref(), cancel).await?;
    atomic_write(&dest, &bytes)?;
    serde_json::from_slice(&bytes)
        .map_err(|err| AppError::detail("asset_index", crate::core::i18n::pick("解析资源索引失败。", "Couldn't parse the asset index."), err.to_string()))
}

async fn fetch_bytes(
    client: &reqwest::Client,
    url: &str,
    expected_sha1: Option<&str>,
    cancel: &CancellationToken,
) -> AppResult<Vec<u8>> {
    let response = tokio::select! {
        _ = cancel.cancelled() => return Err(AppError::new("cancelled", crate::core::i18n::pick("操作已取消。", "Cancelled."))),
        response = client.get(url).send() => response?,
    };
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::new(
            "metadata",
            crate::tr!("获取 Minecraft 元数据失败（HTTP {status}）。", "Couldn't get Minecraft metadata (HTTP {status})."),
        ));
    }
    let bytes = tokio::select! {
        _ = cancel.cancelled() => return Err(AppError::new("cancelled", crate::core::i18n::pick("操作已取消。", "Cancelled."))),
        bytes = response.bytes() => bytes?,
    };
    if let Some(expected) = expected_sha1 {
        if !expected.is_empty() && !sha1_hex(&bytes).eq_ignore_ascii_case(expected) {
            return Err(AppError::new(
                "metadata_hash",
                crate::core::i18n::pick("Minecraft 元数据校验失败，请重试。", "Minecraft metadata failed verification; try again."),
            ));
        }
    }
    Ok(bytes.to_vec())
}

fn url_for_source(
    source: DownloadSource,
    mirror_url: &str,
    official_url: &str,
) -> (String, Vec<String>) {
    match source {
        DownloadSource::Bmclapi => (mirror_url.to_string(), vec![official_url.to_string()]),
        DownloadSource::Official => (official_url.to_string(), Vec::new()),
    }
}

fn check_cancel(cancel: &CancellationToken) -> AppResult<()> {
    if cancel.is_cancelled() {
        Err(AppError::new("cancelled", crate::core::i18n::pick("操作已取消。", "Cancelled.")))
    } else {
        Ok(())
    }
}
