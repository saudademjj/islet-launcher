use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::error::{AppError, AppResult};
use crate::core::paths::AppPaths;
use crate::core::util::{atomic_write, now_millis};

const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const CACHE_TTL_MS: i64 = 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSummary {
    pub id: String,
    #[serde(rename = "versionType", alias = "type")]
    pub version_type: String,
    pub url: String,
    pub time: String,
    pub release_time: String,
    pub sha1: String,
    pub compliance_level: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<VersionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionList {
    pub latest: LatestVersions,
    pub versions: Vec<VersionSummary>,
    pub from_cache: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ManifestCache {
    fetched_at: i64,
    manifest: VersionManifest,
}

pub fn find_version<'a>(
    manifest: &'a VersionManifest,
    version_id: &str,
) -> Option<&'a VersionSummary> {
    manifest
        .versions
        .iter()
        .find(|version| version.id == version_id)
}

/// Metadata always comes from Mojang, never from a mirror. If the network is
/// down we fall back to the last cached manifest.
pub async fn load_manifest(
    client: &reqwest::Client,
    paths: &AppPaths,
    force_refresh: bool,
) -> AppResult<VersionList> {
    let cache_path = paths.version_manifest_file();
    let cached: Option<ManifestCache> = if cache_path.exists() {
        std::fs::read(&cache_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    } else {
        None
    };

    if !force_refresh {
        if let Some(cache) = &cached {
            if now_millis() - cache.fetched_at < CACHE_TTL_MS {
                return Ok(VersionList {
                    latest: cache.manifest.latest.clone(),
                    versions: cache.manifest.versions.clone(),
                    from_cache: true,
                });
            }
        }
    }

    match fetch_manifest(client).await {
        Ok(manifest) => {
            let cache = ManifestCache {
                fetched_at: now_millis(),
                manifest: manifest.clone(),
            };
            if let Ok(encoded) = serde_json::to_vec_pretty(&cache) {
                let _ = atomic_write(&cache_path, &encoded);
            }
            Ok(VersionList {
                latest: manifest.latest,
                versions: manifest.versions,
                from_cache: false,
            })
        }
        Err(err) => {
            if let Some(cache) = cached {
                tracing::warn!(error = %err, "version manifest fetch failed, using cache");
                Ok(VersionList {
                    latest: cache.manifest.latest,
                    versions: cache.manifest.versions,
                    from_cache: true,
                })
            } else {
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::VersionSummary;

    #[test]
    fn version_type_is_serialized_for_the_frontend() {
        let json = r#"{
            "id": "1.20.1",
            "type": "release",
            "url": "https://example.com/version.json",
            "time": "2023-06-12T00:00:00+00:00",
            "releaseTime": "2023-06-12T00:00:00+00:00",
            "sha1": "abc"
        }"#;
        let summary: VersionSummary = serde_json::from_str(json).unwrap();
        assert_eq!(summary.version_type, "release");
        let value = serde_json::to_value(&summary).unwrap();
        assert_eq!(value["versionType"], "release");
        assert!(value.get("type").is_none());
    }
}

async fn fetch_manifest(client: &reqwest::Client) -> AppResult<VersionManifest> {
    let response = client
        .get(MANIFEST_URL)
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::new(
            "manifest",
            format!("获取 Minecraft 版本列表失败（HTTP {status}）。"),
        ));
    }
    response.json::<VersionManifest>().await.map_err(|err| {
        AppError::detail("manifest", "解析 Minecraft 版本列表失败。", err.to_string())
    })
}
