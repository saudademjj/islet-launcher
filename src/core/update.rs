//! Telling the player a newer launcher is out, and fetching it. The release
//! manifest lives at a URL fixed when building (`MC_LAUNCHER_UPDATE_URL`);
//! without one the check is off. A package is only downloaded when the
//! manifest gives its SHA-256, and only opened (the installer or disk image
//! shows itself) after the hash matches.
use std::{path::PathBuf, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::core::download::{self, DownloadProgress, DownloadTask};
use crate::core::error::{AppError, AppResult};
use crate::core::paths::AppPaths;

/// The manifest: `{"version": "0.5.0", "notes": "…", "url": "https://…",
/// "downloads": {"macos-aarch64": {"url": "https://…", "sha256": "…"}}}`.
/// A download may also be a bare URL; then only the page is offered.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    #[serde(default)]
    pub notes: String,
    /// The release page.
    pub url: String,
    /// Downloads by platform ("macos-aarch64", "windows-x86_64" …).
    #[serde(default, skip_serializing)]
    pub downloads: std::collections::HashMap<String, Download>,
    /// This computer's package, when it can be checked before opening.
    #[serde(default)]
    pub package: Option<Package>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Download {
    Url(String),
    Checked { url: String, sha256: Option<String> },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Package {
    pub url: String,
    pub sha256: String,
}

pub fn source() -> Option<&'static str> {
    option_env!("MC_LAUNCHER_UPDATE_URL").filter(|url| url.starts_with("https://"))
}

/// The manifest keys for this computer, most specific first.
fn platform_keys() -> Vec<String> {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        other => other,
    };
    let mut keys = vec![format!("{os}-{}", std::env::consts::ARCH)];
    if os == "macos" {
        keys.push("macos-universal".into());
    }
    keys
}

fn package_for(downloads: &std::collections::HashMap<String, Download>) -> Option<Package> {
    platform_keys().iter().find_map(|key| match downloads.get(key)? {
        Download::Checked { url, sha256: Some(sha256) }
            if url.starts_with("https://") && sha256.len() == 64 && sha256.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            Some(Package { url: url.clone(), sha256: sha256.to_ascii_lowercase() })
        }
        _ => None,
    })
}

/// The newer release, if there is one.
pub async fn check(client: &reqwest::Client, current: &str) -> AppResult<Option<Release>> {
    let Some(url) = source() else {
        return Err(AppError::new("update_source", crate::core::i18n::pick("这个版本的启动器没有配置更新地址。", "This launcher build has no update source.")));
    };
    let mut release: Release = client
        .get(url)
        .timeout(Duration::from_secs(15))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if !release.url.starts_with("https://") {
        return Err(AppError::new("update_source", crate::core::i18n::pick("更新信息里的下载地址无效。", "The download address in the update info is invalid.")));
    }
    release.package = package_for(&release.downloads);
    Ok(newer(&release.version, current).then_some(release))
}

/// The package's own file name, if it is a plain installer or image name.
fn file_name(url: &str) -> Option<String> {
    let name = url.split(['?', '#']).next()?.rsplit('/').next()?;
    let known = [".dmg", ".exe", ".msi", ".appimage", ".deb", ".rpm", ".zip"].iter().any(|ext| name.to_ascii_lowercase().ends_with(ext));
    let safe = !name.is_empty() && name.len() <= 120 && !name.starts_with('.') && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b));
    (known && safe).then(|| name.to_string())
}

/// Downloads the package into the cache and checks its SHA-256. Returns
/// the file, ready to open.
pub async fn download(client: &reqwest::Client, paths: &AppPaths, package: &Package) -> AppResult<PathBuf> {
    let cancel = CancellationToken::new();
    let name = file_name(&package.url).ok_or_else(|| AppError::new("update", crate::core::i18n::pick("更新包的文件名无效。", "The update package has an invalid file name.")))?;
    let folder = paths.cache.join("updates");
    std::fs::create_dir_all(&folder)?;
    let target = folder.join(&name);
    let task = DownloadTask::new(crate::core::i18n::pick("启动器更新", "Launcher update"), package.url.clone(), vec![], target.clone(), None, None)
        .with_sha256(package.sha256.clone());
    let tasks = vec![task];
    let progress = Arc::new(DownloadProgress::new(&tasks));
    download::download_all(tasks, client.clone(), 1, cancel, progress).await?;
    if !target.is_file() {
        return Err(AppError::new("update", crate::core::i18n::pick("更新包没有下载完整，请重试。", "The update package didn't download completely; try again.")));
    }
    Ok(target)
}

/// "0.10.0" is newer than "0.9.3"; a pre-release suffix sorts before the release.
pub fn newer(candidate: &str, current: &str) -> bool {
    fn parts(version: &str) -> (Vec<u64>, bool) {
        let version = version.trim().trim_start_matches('v');
        let (core, pre) = match version.split_once('-') {
            Some((core, _)) => (core, true),
            None => (version, false),
        };
        (core.split('.').map(|p| p.parse().unwrap_or(0)).collect(), pre)
    }
    let (a, a_pre) = parts(candidate);
    let (b, b_pre) = parts(current);
    let len = a.len().max(b.len());
    for i in 0..len {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    !a_pre && b_pre
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packages_need_a_hash_and_a_plain_name() {
        let hash = "a".repeat(64);
        let mut downloads = std::collections::HashMap::new();
        for key in platform_keys() {
            downloads.insert(key, Download::Url("https://example.com/MC.dmg".into()));
        }
        assert!(package_for(&downloads).is_none());
        let key = platform_keys().remove(0);
        downloads.insert(key, Download::Checked { url: "https://example.com/MC-0.5.0.dmg".into(), sha256: Some(hash.clone()) });
        assert_eq!(package_for(&downloads).unwrap().sha256, hash);
        let parsed: Download = serde_json::from_str(r#"{"url":"https://example.com/a.exe","sha256":null}"#).unwrap();
        assert!(matches!(parsed, Download::Checked { sha256: None, .. }));
        assert_eq!(file_name("https://example.com/v/MC%20Launcher.dmg"), None);
        assert_eq!(file_name("https://example.com/v/MC-Launcher_0.5.0_x64-setup.exe?x=1").as_deref(), Some("MC-Launcher_0.5.0_x64-setup.exe"));
        assert_eq!(file_name("https://example.com/v/run.sh"), None);
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("0.10.0", "0.9.3"));
        assert!(newer("v0.5.0", "0.4.9"));
        assert!(!newer("0.4.0", "0.4.0"));
        assert!(!newer("0.4.0-beta.1", "0.4.0"));
        assert!(newer("0.4.0", "0.4.0-beta.1"));
        assert!(!newer("0.3.9", "0.4.0"));
    }
}
