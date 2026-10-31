//! Temurin metadata and byte-identical domestic mirrors. No download is
//! accepted without a SHA-256, including when Adoptium's API is unreachable.
use super::{cancelled, safe_archive_path};
use crate::core::{error::{AppError, AppResult}, platform, settings::DownloadSource};
use serde::Deserialize;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const USTC: &str = "https://mirrors.ustc.edu.cn/github-release/adoptium";
const TUNA: &str = "https://mirrors.tuna.tsinghua.edu.cn/Adoptium";

#[derive(Debug, Deserialize)]
pub(super) struct Package {
    pub link: String,
    pub checksum: String,
    pub name: String,
    pub size: Option<u64>,
}
#[derive(Deserialize)]
struct Asset { binary: Binary }
#[derive(Deserialize)]
struct Binary { package: Package }

fn invalid() -> AppError {
    AppError::new("java_download", crate::core::i18n::pick(
        "没有找到带有效校验值的 Java 运行时，请重试或切换下载源。",
        "No Java runtime with a valid checksum was found; retry or switch sources.",
    ))
}

fn package_prefix(major: u32, arch: &str, os: &str) -> String {
    let arch = match platform::normalize_arch(arch) {
        "aarch64" => "aarch64", "x86" => "x86-32", _ => "x64",
    };
    format!("OpenJDK{major}U-jre_{arch}_{os}_hotspot_")
}

fn valid_name(name: &str, major: u32, arch: &str, os: &str) -> bool {
    let prefix = package_prefix(major, arch, os);
    let suffix = if os == "windows" { ".zip" } else { ".tar.gz" };
    name.starts_with(&prefix) && name.ends_with(suffix)
        && name.len() > prefix.len() + suffix.len()
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-+".contains(&c))
        && safe_archive_path(name).is_ok_and(|p| p.components().count() == 1)
}

fn check(package: Package, major: u32, arch: &str) -> AppResult<Package> {
    if !valid_name(&package.name, major, arch, platform::adoptium_os())
        || package.checksum.len() != 64
        || !package.checksum.bytes().all(|b| b.is_ascii_hexdigit())
        || !url::Url::parse(&package.link).is_ok_and(|u| u.scheme() == "https"
            && u.host_str().is_some_and(|h| ["github.com", "mirrors.ustc.edu.cn"].contains(&h))) {
        return Err(invalid());
    }
    Ok(package)
}

async fn text(client: &reqwest::Client, url: &str, cancel: &CancellationToken) -> AppResult<String> {
    tokio::select! {
        _ = cancel.cancelled() => Err(cancelled()),
        result = async { client.get(url).timeout(Duration::from_secs(10)).send().await?
            .error_for_status()?.text().await } => result.map_err(Into::into),
    }
}

pub(super) async fn resolve(client: &reqwest::Client, major: u32, arch: &str, source: DownloadSource,
    cancel: &CancellationToken) -> AppResult<Package> {
    let url = format!("https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture={}&image_type=jre&os={}&vendor=eclipse&project=jdk",
        platform::adoptium_arch_for(arch), platform::adoptium_os());
    let result = async {
        let assets: Vec<Asset> = serde_json::from_str(&text(client, &url, cancel).await?)?;
        check(assets.into_iter().next().ok_or_else(invalid)?.binary.package, major, arch)
    }.await;
    match result {
        Ok(package) => Ok(package),
        Err(error) if error.code() == "cancelled" || source == DownloadSource::Official => Err(error),
        Err(_) => {
            tracing::warn!(major, "Adoptium metadata unavailable; querying the domestic release catalogue");
            mirror_package(client, major, arch, cancel).await
        }
    }
}

// The mirror exposes filenames, not Adoptium's assets API. Only accept a
// basename matching the requested runtime, never a URL supplied by HTML.
fn catalogue_names(html: &str, major: u32, arch: &str, os: &str) -> Vec<String> {
    let mut names: Vec<String> = html.split("href=\"").skip(1)
        .filter_map(|s| s.split('"').next())
        .filter(|s| !s.contains(['?', '#', '%', '\\']) && !s.split('/').any(|p| p == ".."))
        .filter_map(|s| s.rsplit('/').next())
        .filter(|s| valid_name(s, major, arch, os)).map(str::to_owned).collect();
    let key = |s: &str| s.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse::<u32>().ok()).collect::<Vec<_>>();
    names.sort_by_key(|b| std::cmp::Reverse(key(b)));
    names.dedup();
    names
}

pub(super) async fn mirror_package(client: &reqwest::Client, major: u32, arch: &str,
    cancel: &CancellationToken) -> AppResult<Package> {
    let base = format!("{USTC}/temurin{major}-binaries/LatestRelease");
    let html = text(client, &format!("{base}/"), cancel).await?;
    let name = catalogue_names(&html, major, arch, platform::adoptium_os())
        .into_iter().next().ok_or_else(invalid)?;
    // These are the upstream release's checksum sidecars, mirrored together
    // with the archive. TUNA itself does not mirror checksum sidecars.
    let checksum = text(client, &format!("{base}/{name}.sha256.txt"), cancel).await?;
    let fields: Vec<_> = checksum.split_whitespace().collect();
    if fields.len() != 2 || fields[1].trim_start_matches('*') != name { return Err(invalid()); }
    check(Package { link: format!("{base}/{name}"), checksum: fields[0].into(), name, size: None }, major, arch)
}

pub(super) fn download_urls(package: &Package, major: u32, arch: &str, source: DownloadSource) -> Vec<String> {
    if source == DownloadSource::Official { return vec![package.link.clone()]; }
    let tuna_arch = if platform::normalize_arch(arch) == "x86" { "x32" } else { platform::adoptium_arch_for(arch) };
    let mut urls = vec![
        format!("{TUNA}/{major}/jre/{tuna_arch}/{}/{}", platform::adoptium_os(), package.name),
        format!("{USTC}/temurin{major}-binaries/LatestRelease/{}", package.name),
        package.link.clone(),
    ];
    let mut seen = std::collections::HashSet::new();
    urls.retain(|u| seen.insert(u.clone()));
    urls
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalogue_rejects_other_architectures_types_and_paths() {
        let html = r#"<a href="/github-release/adoptium/temurin21-binaries/LatestRelease/OpenJDK21U-jre_x64_windows_hotspot_21.0.9_10.zip">ok</a>
          <a href="OpenJDK21U-jdk_x64_windows_hotspot_21.0.9_10.zip">jdk</a>
          <a href="OpenJDK21U-jre_aarch64_windows_hotspot_21.0.9_10.zip">arm</a>
          <a href="../OpenJDK21U-jre_x64_windows_hotspot_21.0.9_10.zip">escape</a>
          <a href="OpenJDK21U-jre_x64_windows_hotspot_21.0.9_10.zip?x">query</a>"#;
        assert_eq!(catalogue_names(html, 21, "x86_64", "windows"), ["OpenJDK21U-jre_x64_windows_hotspot_21.0.9_10.zip"]);
        assert!(valid_name("OpenJDK8U-jre_x86-32_windows_hotspot_8u472b08.zip", 8, "x86", "windows"));
        assert!(valid_name("OpenJDK17U-jre_aarch64_mac_hotspot_17.0.17_10.tar.gz", 17, "aarch64", "mac"));
    }
    #[test]
    fn runtime_metadata_must_have_a_checksum_and_correct_major() {
        let name = format!("{}25.0.5_8{}", package_prefix(25, std::env::consts::ARCH, platform::adoptium_os()), if cfg!(windows) { ".zip" } else { ".tar.gz" });
        let p = || Package { name: name.clone(), link: "https://github.com/adoptium/temurin25-binaries/releases/download/jdk-25/test".into(), checksum: "a".repeat(64), size: None };
        assert!(check(p(), 25, std::env::consts::ARCH).is_ok());
        assert!(check(p(), 21, std::env::consts::ARCH).is_err());
        let mut bad = p(); bad.checksum = "<html>".into(); assert!(check(bad, 25, std::env::consts::ARCH).is_err());
        let urls = download_urls(&p(), 25, std::env::consts::ARCH, DownloadSource::Bmclapi);
        assert!(urls[0].starts_with(TUNA)); assert!(urls[1].starts_with(USTC));
        assert_eq!(download_urls(&p(), 25, std::env::consts::ARCH, DownloadSource::Official), [p().link]);
    }
}
