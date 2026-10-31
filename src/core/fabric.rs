use crate::core::{
    download::DownloadTask,
    error::{AppError, AppResult},
    mojang::version::{Arguments, Library, VersionJson, maven_path},
    paths::AppPaths,
    settings::DownloadSource,
};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
struct LoaderEntry {
    loader: Loader,
}
#[derive(Deserialize)]
struct Loader {
    version: String,
    /// Fabric says; Quilt marks pre-releases with a "-beta" suffix instead.
    stable: Option<bool>,
}

/// Fabric and Quilt install the same way: a launch profile from their meta
/// server that adds a main class and libraries to the vanilla version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Fabric,
    Quilt,
    Legacy,
}

impl Flavor {
    pub fn of(loader: &str) -> Option<Self> {
        match loader {
            "fabric" => Some(Flavor::Fabric),
            "quilt" => Some(Flavor::Quilt),
            "legacyfabric" => Some(Flavor::Legacy),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Flavor::Fabric => "Fabric",
            Flavor::Quilt => "Quilt",
            Flavor::Legacy => "Legacy Fabric",
        }
    }
    pub fn meta(self) -> &'static str {
        match self {
            Flavor::Fabric => "https://meta.fabricmc.net/v2",
            Flavor::Quilt => "https://meta.quiltmc.org/v3",
            Flavor::Legacy => "https://meta.legacyfabric.net/v2",
        }
    }
    fn mirror_meta(self) -> Option<&'static str> {
        match self {
            Flavor::Fabric => Some("https://bmclapi2.bangbang93.com/fabric-meta/v2"),
            Flavor::Quilt => Some("https://bmclapi2.bangbang93.com/quilt-meta/v3"),
            Flavor::Legacy => None,
        }
    }

    fn meta_urls(self, path: &str, source: DownloadSource) -> Vec<String> {
        let mut urls = Vec::new();
        if source == DownloadSource::Bmclapi {
            if let Some(base) = self.mirror_meta() { urls.push(format!("{base}/{path}")); }
        }
        urls.push(format!("{}/{path}", self.meta()));
        urls
    }
    /// Library hosts a profile may point at: the project's own Maven (Quilt
    /// also uses Fabric's intermediary mappings).
    fn hosts(self) -> &'static [&'static str] {
        match self {
            Flavor::Fabric => &["maven.fabricmc.net"],
            Flavor::Quilt => &["maven.quiltmc.org", "maven.fabricmc.net"],
            Flavor::Legacy => &["maven.legacyfabric.net", "repo.legacyfabric.net", "maven.fabricmc.net"],
        }
    }
}

impl Loader {
    fn is_stable(&self) -> bool {
        self.stable.unwrap_or_else(|| !self.version.contains('-'))
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    inherits_from: String,
    main_class: String,
    #[serde(default)]
    arguments: Arguments,
    libraries: Vec<FabricLibrary>,
}
#[derive(Deserialize)]
struct FabricLibrary {
    name: String,
    url: String,
    sha1: Option<String>,
    size: Option<u64>,
    natives: Option<std::collections::HashMap<String, String>>,
    extract: Option<crate::core::mojang::version::Extract>,
}

pub fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-+".contains(&c))
        && value != "."
        && value != ".."
}

pub async fn prepare(
    client: &reqwest::Client,
    paths: &AppPaths,
    base: &VersionJson,
    pinned: Option<&str>,
    cancel: &CancellationToken,
) -> AppResult<(VersionJson, Vec<DownloadTask>, String)> {
    prepare_flavor(Flavor::Fabric, client, paths, base, pinned, cancel).await
}

pub async fn prepare_flavor(
    flavor: Flavor,
    client: &reqwest::Client,
    paths: &AppPaths,
    base: &VersionJson,
    pinned: Option<&str>,
    cancel: &CancellationToken,
) -> AppResult<(VersionJson, Vec<DownloadTask>, String)> {
    prepare_flavor_with_source(flavor, client, paths, base, pinned, cancel, DownloadSource::Bmclapi).await
}

pub async fn prepare_flavor_with_source(
    flavor: Flavor,
    client: &reqwest::Client,
    paths: &AppPaths,
    base: &VersionJson,
    pinned: Option<&str>,
    cancel: &CancellationToken,
    source: DownloadSource,
) -> AppResult<(VersionJson, Vec<DownloadTask>, String)> {
    let name = flavor.name();
    if !valid_version(&base.id) {
        return Err(AppError::new("fabric", crate::core::i18n::pick("Minecraft 版本无效。", "Invalid Minecraft version.")));
    }
    let loader = if let Some(version) = pinned {
        version.to_string()
    } else {
        let list: Vec<LoaderEntry> = get_json(
            client,
            &flavor.meta_urls(&format!("versions/loader/{}", base.id), source),
            cancel,
        )
        .await?;
        list.into_iter()
            .find(|item| item.loader.is_stable())
            .ok_or_else(|| {
                AppError::new("fabric", crate::tr!("这个 Minecraft 版本没有稳定的 {name} Loader。", "This Minecraft version has no stable {name} Loader."))
            })?
            .loader
            .version
    };
    if !valid_version(&loader) {
        return Err(AppError::new("fabric", crate::tr!("{name} Loader 版本无效。", "Invalid {name} Loader version.")));
    }
    let profile: Profile = get_json(
        client,
        &flavor.meta_urls(&format!("versions/loader/{}/{loader}/profile/json", base.id), source),
        cancel,
    )
    .await?;
    if profile.inherits_from != base.id {
        return Err(AppError::new("fabric", crate::tr!("{name} 配置与游戏版本不匹配。", "The {name} profile doesn't match the game version.")));
    }
    let mut version = base.clone();
    version.main_class = profile.main_class;
    let arguments = version.arguments.get_or_insert_with(Arguments::default);
    arguments.jvm.extend(profile.arguments.jvm);
    arguments.game.extend(profile.arguments.game);
    let mut tasks = Vec::new();
    for library in profile.libraries {
        if let Some(natives) = &library.natives {
            let os = crate::core::mojang::rules::current_os_name();
            if let Some(classifier) = natives.get(os) {
                let classifier = classifier.replace("${arch}", "64");
                let relative = maven_path(&format!("{}:{classifier}", library.name)).ok_or_else(|| AppError::new("fabric", "Invalid native coordinates"))?;
                crate::core::java::runtime::safe_archive_path(&relative)?;
                let root = url::Url::parse(&library.url).map_err(|e| AppError::new("fabric", e.to_string()))?;
                if root.scheme() != "https" || !root.host_str().is_some_and(|h| flavor.hosts().contains(&h)) { return Err(AppError::new("fabric", "Invalid native Maven")); }
                let url = format!("{}/{relative}", library.url.trim_end_matches('/'));
                let hash = library_hash(client, &url, None, source, cancel).await?;
                let (primary, fallback) = library_urls(&url, source);
                tasks.push(DownloadTask::new(&library.name, primary, fallback, paths.library_path(&relative), Some(hash.clone()), None));
                let info = crate::core::mojang::version::DownloadInfo { path: Some(relative), url: Some(url), sha1: Some(hash), size: None, ..Default::default() };
                let key = library.name.split(':').take(2).collect::<Vec<_>>().join(":");
                version.libraries.retain(|l| l.name.split(':').take(2).collect::<Vec<_>>().join(":") != key);
                version.libraries.push(Library { name: library.name, natives: library.natives, extract: library.extract, rules: vec![], downloads: Some(crate::core::mojang::version::LibraryDownloads { artifact: None, classifiers: Some([(classifier, info)].into()) }) });
            }
            continue;
        }
        let relative = maven_path(&library.name)
            .ok_or_else(|| AppError::new("fabric", crate::tr!("{name} 库坐标无效。", "Invalid {name} library coordinates.")))?;
        crate::core::java::runtime::safe_archive_path(&relative)?;
        let base_url = url::Url::parse(&library.url)
            .map_err(|e| AppError::detail("fabric", crate::tr!("{name} 下载地址无效。", "Invalid {name} download address."), e.to_string()))?;
        if base_url.scheme() != "https" || !base_url.host_str().is_some_and(|host| flavor.hosts().contains(&host)) {
            return Err(AppError::new("fabric", crate::tr!("{name} 库地址不属于官方 Maven。", "A {name} library isn't from the official Maven.")));
        }
        let url = format!(
            "{}{relative}",
            library.url.trim_end_matches('/').to_owned() + "/"
        );
        let sha1 = library_hash(client, &url, library.sha1, source, cancel).await?;
        let (primary, fallback) = library_urls(&url, source);
        tasks.push(DownloadTask::new(
            format!("{name} {}", library.name),
            primary,
            fallback,
            paths.library_path(&relative),
            Some(sha1),
            library.size,
        ));
        let group_artifact = library
            .name
            .split(':')
            .take(2)
            .collect::<Vec<_>>()
            .join(":");
        version.libraries.retain(|existing| {
            existing
                .name
                .split(':')
                .take(2)
                .collect::<Vec<_>>()
                .join(":")
                != group_artifact
        });
        version.libraries.push(Library {
            name: library.name,
            downloads: None,
            natives: None,
            rules: vec![],
            extract: None,
        });
    }
    Ok((version, tasks, loader))
}
pub(crate) async fn loader_entries<T: serde::de::DeserializeOwned>(
    flavor: Flavor, client: &reqwest::Client, minecraft: &str, source: DownloadSource,
) -> AppResult<T> {
    get_json(client, &flavor.meta_urls(&format!("versions/loader/{minecraft}"), source), &CancellationToken::new()).await
}

fn library_urls(url: &str, source: DownloadSource) -> (String, Vec<String>) {
    if source == DownloadSource::Bmclapi {
        if let Ok(parsed) = url::Url::parse(url) {
            let path = match parsed.host_str() {
                Some("maven.fabricmc.net") => Some(parsed.path()),
                Some("maven.quiltmc.org") => parsed.path().strip_prefix("/repository/release"),
                _ => None,
            };
            if let Some(path) = path {
                return (format!("https://bmclapi2.bangbang93.com/maven{path}"), vec![url.to_string()]);
            }
        }
    }
    (url.into(), vec![])
}

fn valid_hash(hash: &str) -> bool { hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()) }

async fn library_hash(client: &reqwest::Client, url: &str, provided: Option<String>, source: DownloadSource,
    cancel: &CancellationToken) -> AppResult<String> {
    if let Some(hash) = provided {
        return if valid_hash(&hash) { Ok(hash) } else { Err(hash_error()) };
    }
    let (primary, fallback) = library_urls(url, source);
    // Some mirrors cache the JAR but not .sha1. A 404 or malformed response
    // must try the next checksum source, never disable integrity checking.
    for candidate in std::iter::once(primary).chain(fallback) {
        let result = tokio::select! {
            _ = cancel.cancelled() => return Err(cancelled()),
            result = async { client.get(format!("{candidate}.sha1")).timeout(std::time::Duration::from_secs(8))
                .send().await?.error_for_status()?.text().await } => result,
        };
        if let Ok(text) = result {
            let hash = text.split_whitespace().next().unwrap_or("");
            if valid_hash(hash) { return Ok(hash.into()); }
        }
    }
    Err(hash_error())
}

fn hash_error() -> AppError {
    AppError::new("fabric_hash", crate::core::i18n::pick(
        "无法取得加载器依赖的有效校验值，请重试或切换下载源。",
        "Couldn't obtain a valid loader checksum; retry or switch download sources.",
    ))
}

fn cancelled() -> AppError { AppError::new("cancelled", crate::core::i18n::pick("操作已取消。", "Cancelled.")) }

async fn get_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    urls: &[String],
    cancel: &CancellationToken,
) -> AppResult<T> {
    let mut last = None;
    for url in urls {
        let result = tokio::select! {
            _ = cancel.cancelled() => return Err(cancelled()),
            result = async { client.get(url).timeout(std::time::Duration::from_secs(10)).send().await?.error_for_status()?.json().await } => result,
        };
        match result {
            Ok(value) => return Ok(value),
            Err(error) => { tracing::warn!(url, "loader metadata source failed; trying the next source"); last = Some(AppError::from(error)); }
        }
    }
    Err(last.unwrap_or_else(|| AppError::new("fabric", "No loader metadata source")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "downloads real loader files; run explicitly on Windows"]
    async fn domestic_fabric_loader_downloads() {
        let proxy = std::env::var("MC_LAUNCHER_TEST_PROXY").expect("acceptance proxy");
        let client = reqwest::Client::builder().no_proxy().proxy(reqwest::Proxy::all(proxy).unwrap())
            .user_agent("Islet/acceptance").connect_timeout(std::time::Duration::from_secs(8))
            .read_timeout(std::time::Duration::from_secs(20)).build().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let cancel = CancellationToken::new();
        for minecraft in ["1.20.1", "1.21.1", "26.3"] {
            let entries = crate::core::loaders::list_with_source(&client, "fabric", minecraft, DownloadSource::Bmclapi).await.unwrap();
            assert!(!entries.is_empty());
            let base: VersionJson = serde_json::from_value(serde_json::json!({"id":minecraft,"type":"release","mainClass":"net.minecraft.client.main.Main"})).unwrap();
            let (merged, tasks, loader) = prepare_flavor_with_source(Flavor::Fabric, &client, &paths, &base, None, &cancel, DownloadSource::Bmclapi).await.unwrap();
            assert!(merged.main_class.contains("KnotClient"));
            assert!(tasks.iter().all(|t| t.url.starts_with("https://bmclapi2.bangbang93.com/") && t.sha1.is_some() && !t.fallback_urls.is_empty()));
            let count = tasks.len();
            let progress = std::sync::Arc::new(crate::core::download::DownloadProgress::new(&tasks));
            crate::core::download::download_all(tasks, client.clone(), 4, cancel.clone(), progress).await.unwrap();
            println!("FABRIC {minecraft} PASS loader={loader} verifiedLibraries={count}");
        }
    }

    #[tokio::test]
    async fn malformed_mirror_metadata_falls_back_and_cancellation_stops_requests() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let base = format!("http://{}", server.server_addr());
        let worker = std::thread::spawn(move || {
            for body in ["<html>upstream unavailable</html>", r#"{"version":"0.20.1"}"#] {
                let request = server.recv_timeout(std::time::Duration::from_secs(5)).unwrap().unwrap();
                request.respond(tiny_http::Response::from_string(body)).unwrap();
            }
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let value: serde_json::Value = get_json(&client, &[format!("{base}/mirror"), format!("{base}/official")], &CancellationToken::new()).await.unwrap();
        assert_eq!(value["version"], "0.20.1");
        worker.join().unwrap();
        let cancel = CancellationToken::new(); cancel.cancel();
        let error = get_json::<serde_json::Value>(&client, &[base], &cancel).await.unwrap_err();
        assert_eq!(error.code(), "cancelled");
    }

    #[test]
    fn loader_mirrors_keep_paths_and_official_mode_stays_official() {
        let url = "https://maven.fabricmc.net/net/fabricmc/sponge-mixin/0.17.5+mixin.0.8.7/sponge-mixin-0.17.5+mixin.0.8.7.jar";
        let (mirror, fallbacks) = library_urls(url, DownloadSource::Bmclapi);
        assert_eq!(mirror, url.replace("https://maven.fabricmc.net", "https://bmclapi2.bangbang93.com/maven"));
        assert_eq!(fallbacks, [url]);
        assert_eq!(library_urls(url, DownloadSource::Official), (url.into(), vec![]));
        assert!(library_urls("https://maven.fabricmc.net.evil.test/library.jar", DownloadSource::Bmclapi).1.is_empty());
        assert_eq!(Flavor::Fabric.meta_urls("versions/loader/1.21.1", DownloadSource::Bmclapi).len(), 2);
        assert_eq!(Flavor::Quilt.meta_urls("versions/loader/1.20.1", DownloadSource::Official).len(), 1);
        assert!(!valid_hash("<html>checksum</html>"));
    }
}
