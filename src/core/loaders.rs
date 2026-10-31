use crate::core::{
    download::{self, DownloadProgress, DownloadTask},
    error::{AppError, AppResult},
    fabric::valid_version,
    java::detect::JavaInstallation,
    mojang::version::{Arguments, VersionJson},
    paths::AppPaths,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoaderVersion {
    pub version: String,
    pub stable: bool,
}

pub async fn list(
    client: &reqwest::Client,
    loader: &str,
    minecraft: &str,
) -> AppResult<Vec<LoaderVersion>> {
    list_with_source(client, loader, minecraft, crate::core::settings::DownloadSource::Bmclapi).await
}

pub async fn list_with_source(
    client: &reqwest::Client,
    loader: &str,
    minecraft: &str,
    source: crate::core::settings::DownloadSource,
) -> AppResult<Vec<LoaderVersion>> {
    if !valid_version(minecraft) {
        return Err(AppError::new("loader", crate::core::i18n::pick("Minecraft 版本号无效。", "Invalid Minecraft version.")));
    }
    if ["liteloader", "cleanroom"].contains(&loader) { return crate::core::extra_loaders::list(client, loader, minecraft).await; }
    if loader == "vanilla" {
        return Ok(vec![]);
    }
    let mut versions = if loader == "neoforge" {
        // BMCLAPI publishes an exact Minecraft-to-NeoForge mapping, including
        // the newer year-based Minecraft versions. Fall back to upstream.
        #[derive(Deserialize)]
        struct Entry {
            version: String,
            mcversion: String,
        }
        let mirror = async {
            client
                .get(format!(
                    "https://bmclapi2.bangbang93.com/neoforge/list/{minecraft}"
                ))
                .timeout(Duration::from_secs(15))
                .send()
                .await?
                .error_for_status()?
                .json::<Vec<Entry>>()
                .await
        }
        .await;
        match mirror {
            Ok(entries) => entries
                .into_iter()
                .filter(|e| e.mcversion == minecraft)
                .map(|e| LoaderVersion {
                    stable: !e.version.contains("beta") && !e.version.contains("alpha"),
                    version: e.version,
                })
                .collect(),
            Err(_) => {
                let prefix = neo_prefix(minecraft)
                    .ok_or_else(|| AppError::new("loader", crate::core::i18n::pick("没有匹配的 NeoForge 版本。", "No matching NeoForge version.")))?;
                let xml = client.get("https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml")
                    .timeout(Duration::from_secs(15)).send().await?.error_for_status()?.text().await?;
                parse_versions(&xml)
                    .into_iter()
                    .filter(|v| v.starts_with(&prefix))
                    .map(|version| LoaderVersion {
                        stable: !version.contains("beta"),
                        version,
                    })
                    .collect()
            }
        }
    } else if let Some(flavor) = crate::core::fabric::Flavor::of(loader) {
        #[derive(Deserialize)]
        struct Entry {
            loader: Listed,
        }
        #[derive(Deserialize)]
        struct Listed {
            version: String,
            stable: Option<bool>,
        }
        let entries: Vec<Entry> = crate::core::fabric::loader_entries(flavor, client, minecraft, source).await?;
        entries
            .into_iter()
            .map(|e| LoaderVersion {
                stable: e.loader.stable.unwrap_or_else(|| !e.loader.version.contains('-')),
                version: e.loader.version,
            })
            .collect::<Vec<_>>()
    } else {
        let (url, prefix) = match loader {
            "forge" => (
                "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml",
                format!("{minecraft}-"),
            ),
            "neoforge" => (
                "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml",
                neo_prefix(minecraft).ok_or_else(|| {
                    AppError::new("loader", crate::core::i18n::pick("NeoForge 支持 Minecraft 1.20.2 及更新版本。", "NeoForge supports Minecraft 1.20.2 and later."))
                })?,
            ),
            _ => return Err(AppError::new("loader", crate::core::i18n::pick("未知的加载器。", "Unknown loader."))),
        };
        let xml = client
            .get(url)
            .timeout(Duration::from_secs(30))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        parse_versions(&xml)
            .into_iter()
            .filter(|v| v.starts_with(&prefix))
            .map(|v| {
                let version = if loader == "forge" {
                    v.strip_prefix(&prefix).unwrap().to_string()
                } else {
                    v
                };
                LoaderVersion {
                    stable: !version.contains("beta")
                        && !version.contains("alpha")
                        && !version.contains("rc"),
                    version,
                }
            })
            .collect()
    };
    versions.retain(|v| valid_version(&v.version));
    versions.sort_by(|a, b| {
        b.stable
            .cmp(&a.stable)
            .then_with(|| numeric_key(&b.version).cmp(&numeric_key(&a.version)))
            .then_with(|| b.version.cmp(&a.version))
    });
    versions.dedup_by(|a, b| a.version == b.version);
    Ok(versions)
}
fn numeric_key(v: &str) -> Vec<u32> {
    v.split(|c: char| !c.is_ascii_digit())
        .filter_map(|s| s.parse().ok())
        .collect()
}
fn neo_prefix(mc: &str) -> Option<String> {
    let parts: Vec<u32> = mc
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if parts.first() == Some(&1)
        && parts.len() >= 2
        && (parts[1] > 20 || (parts[1] == 20 && parts.get(2).copied().unwrap_or(0) >= 2))
    {
        Some(format!(
            "{}.{}.",
            parts[1],
            parts.get(2).copied().unwrap_or(0)
        ))
    } else if parts.first().is_some_and(|year| *year >= 26) && parts.len() >= 2 {
        Some(format!("{mc}."))
    } else {
        None
    }
}
fn parse_versions(xml: &str) -> Vec<String> {
    xml.split("<version>")
        .skip(1)
        .filter_map(|s| {
            s.split_once("</version>")
                .map(|(v, _)| v.trim().to_string())
        })
        .collect()
}

pub fn installer_url(loader: &str, mc: &str, version: &str) -> AppResult<String> {
    if !valid_version(mc) || !valid_version(version) {
        return Err(AppError::new("loader", crate::core::i18n::pick("加载器版本号无效。", "Invalid loader version.")));
    }
    match loader {
        "forge" => Ok(format!(
            "https://maven.minecraftforge.net/net/minecraftforge/forge/{mc}-{version}/forge-{mc}-{version}-installer.jar"
        )),
        "neoforge" => Ok(format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar"
        )),
        _ => Err(AppError::new("loader", crate::core::i18n::pick("加载器不使用此安装器。", "This loader doesn't use an installer."))),
    }
}

/// The official Forge or NeoForge installer in the shared cache, checked
/// against the SHA-1 its maven (or the mirror) publishes.
pub async fn fetch_installer(client: &reqwest::Client, paths: &AppPaths, loader: &str, mc: &str, version: &str, cancel: &CancellationToken) -> AppResult<std::path::PathBuf> {
    let url = installer_url(loader, mc, version)?;
    let official_hash = async {
        client
            .get(format!("{url}.sha1"))
            .timeout(Duration::from_secs(8))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await
    }
    .await;
    let mirror = mirror_url(&url);
    let hash = match official_hash {
        Ok(text) => text.split_whitespace().next().unwrap_or("").to_string(),
        Err(_) => {
            // Obtain the content-addressed checksum over authenticated HTTPS
            // from the configured mirror, without following its CDN redirect.
            let metadata = crate::core::net::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(20))
                .build()?
                .get(format!("{mirror}?metadata={}", uuid::Uuid::new_v4()))
                .send()
                .await?;
            metadata
                .headers()
                .get("x-bmclapi-hash")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string()
        }
    };
    if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(AppError::new("loader", crate::core::i18n::pick("安装器缺少可信的 SHA-1 校验值。", "The installer has no trusted SHA-1 hash.")));
    }
    let installer = paths
        .cache
        .join("installers")
        .join(format!("{loader}-{mc}-{version}.jar"));
    let task = DownloadTask::new(
        crate::core::i18n::pick("加载器安装器", "Loader installer"),
        mirror_attempt(&mirror),
        vec![mirror_attempt(&mirror), mirror_attempt(&mirror), url],
        installer.clone(),
        Some(hash),
        None,
    );
    download::download_all(
        vec![task.clone()],
        client.clone(),
        1,
        cancel.clone(),
        Arc::new(DownloadProgress::new(&[task])),
    )
    .await?;
    Ok(installer)
}

/// Run the official installer in the shared cache, where no user saves live.
pub async fn install(
    client: &reqwest::Client,
    paths: &AppPaths,
    base: &VersionJson,
    loader: &str,
    pinned: Option<&str>,
    java: &JavaInstallation,
    cancel: &CancellationToken,
    report: crate::core::java::runtime::JavaProgress,
) -> AppResult<(VersionJson, String)> {
    let versions = list(client, loader, &base.id).await?;
    let selected = match pinned {
        Some(v) if versions.iter().any(|e| e.version == v) => v.to_string(),
        Some(_) => {
            return Err(AppError::new(
                "loader",
                crate::core::i18n::pick("所选加载器与 Minecraft 版本不匹配，请刷新版本列表。", "The chosen loader doesn't match the Minecraft version; refresh the version list."),
            ));
        }
        None => versions
            .first()
            .ok_or_else(|| AppError::new("loader", crate::core::i18n::pick("这个 Minecraft 版本没有可用的加载器。", "No loader is available for this Minecraft version.")))?
            .version
            .clone(),
    };
    report(crate::core::i18n::pick("正在下载并校验加载器安装器", "Downloading and verifying the loader installer"), 0, 0);
    let installer = fetch_installer(client, paths, loader, &base.id, &selected, cancel).await?;
    let (profile_id, dependency_tasks) = {
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&installer)?)
            .map_err(|e| AppError::detail("loader", crate::core::i18n::pick("安装器损坏。", "The installer is damaged."), e.to_string()))?;
        let value: serde_json::Value = serde_json::from_reader(
            archive
                .by_name("install_profile.json")
                .map_err(|e| AppError::detail("loader", crate::core::i18n::pick("缺少安装器配置。", "The installer profile is missing."), e.to_string()))?,
        )?;
        let profile_id = value
            .get("version")
            .and_then(|v| v.as_str())
            .or_else(|| value.pointer("/versionInfo/id").and_then(|v| v.as_str()))
            .ok_or_else(|| AppError::new("loader", crate::core::i18n::pick("安装器缺少版本标识。", "The installer has no version id.")))?
            .to_string();
        if value.get("versionInfo").is_some() {
            let overlay = legacy_forge(paths, base, &value, &mut archive)?;
            drop(archive);
            let merged = merge(base, overlay)?;
            complete_libraries(client, paths, &merged, cancel).await?;
            return Ok((merged, selected));
        }
        let mut dependencies = Vec::new();
        if loader == "neoforge" {
            let json = value
                .get("json")
                .and_then(|v| v.as_str())
                .unwrap_or("/version.json")
                .trim_start_matches('/');
            let overlay: serde_json::Value = serde_json::from_reader(
                archive
                    .by_name(json)
                    .map_err(|e| AppError::detail("loader", crate::core::i18n::pick("缺少加载器配置。", "The loader profile is missing."), e.to_string()))?,
            )?;
            let mut seen = std::collections::HashSet::new();
            for library in value
                .get("libraries")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .chain(
                    overlay
                        .get("libraries")
                        .and_then(|v| v.as_array())
                        .into_iter()
                        .flatten(),
                )
            {
                let Some(artifact) = library.pointer("/downloads/artifact") else {
                    continue;
                };
                let Some(url) = artifact
                    .get("url")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                else {
                    continue;
                };
                let path = artifact
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AppError::new("loader", crate::core::i18n::pick("加载器库路径缺失。", "A loader library path is missing.")))?;
                crate::core::java::runtime::safe_archive_path(path)?;
                if !seen.insert(path.to_string()) {
                    continue;
                }
                let hash = artifact
                    .get("sha1")
                    .and_then(|v| v.as_str())
                    .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                    .ok_or_else(|| AppError::new("loader", crate::core::i18n::pick("加载器依赖缺少校验值。", "A loader dependency has no hash.")))?;
                let mirror = mirror_url(url);
                dependencies.push(DownloadTask::new(
                    path,
                    format!("https://repo.maven.apache.org/maven2/{path}"),
                    vec![
                        mirror_attempt(&mirror),
                        mirror_attempt(&mirror),
                        mirror_attempt(&mirror),
                        mirror_attempt(&mirror),
                        url.to_string(),
                    ],
                    paths.library_path(path),
                    Some(hash.to_string()),
                    artifact.get("size").and_then(|v| v.as_u64()),
                ));
            }
        }
        (profile_id, dependencies)
    };
    if !dependency_tasks.is_empty() {
        report(crate::core::i18n::pick("正在下载并校验 NeoForge 依赖库", "Downloading and verifying NeoForge libraries"), 0, 0);
        let progress = Arc::new(DownloadProgress::new(&dependency_tasks));
        let download = download::download_all(
            dependency_tasks,
            crate::core::net::builder()
                .connect_timeout(Duration::from_secs(10))
                .read_timeout(Duration::from_secs(20))
                .timeout(Duration::from_secs(180))
                .build()?,
            4,
            cancel.clone(),
            progress.clone(),
        );
        tokio::pin!(download);
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        loop {
            tokio::select! {
                result = &mut download => { result?; break; },
                _ = tick.tick() => { let p = progress.snapshot(); report(crate::core::i18n::pick("正在下载并校验 NeoForge 依赖库", "Downloading and verifying NeoForge libraries"), p.bytes_done, p.bytes_total); }
            }
        }
    }
    if !valid_version(&profile_id) {
        return Err(AppError::new("loader", crate::core::i18n::pick("安装器版本路径无效。", "Invalid installer version path.")));
    }
    let base_dir = paths.version_cache_dir(&base.id);
    crate::core::util::atomic_write(
        &base_dir.join(format!("{}.json", base.id)),
        &serde_json::to_vec_pretty(base)?,
    )?;
    let vanilla_jar = base_dir.join(format!("{}.jar", base.id));
    if !vanilla_jar.exists() {
        if std::fs::hard_link(paths.client_jar(&base.id), &vanilla_jar).is_err() {
            std::fs::copy(paths.client_jar(&base.id), &vanilla_jar)?;
        }
    }
    let profiles = paths.cache.join("launcher_profiles.json");
    if !profiles.exists() {
        crate::core::util::atomic_write(&profiles, br#"{"profiles":{}}"#)?;
    }
    let log_path = paths
        .cache
        .join("installers")
        .join(format!("{loader}-{}-{selected}.log", base.id));
    let log = std::fs::File::create(&log_path)?;
    let mut command = crate::core::platform::quiet_command(&java.path);
    command
        .args(["-Djava.awt.headless=true", "-Dfile.encoding=UTF-8", "-jar"])
        .arg(&installer)
        .arg("--installClient")
        .arg(&paths.cache)
        .current_dir(installer.parent().unwrap())
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    if loader == "neoforge" {
        command.arg("--offline");
    }
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    let mut child = command.spawn()?;
    report(
        crate::core::i18n::pick("正在运行官方加载器安装器（日志保存在 cache/installers）", "Running the official loader installer (logs in cache/installers)"),
        0,
        0,
    );
    let status = tokio::select! {
        _ = cancel.cancelled() => { let _ = child.kill().await; return Err(AppError::new("cancelled", crate::core::i18n::pick("加载器安装已取消。", "Loader install cancelled."))); },
        result = tokio::time::timeout(Duration::from_secs(1800), child.wait()) => result.map_err(|_| AppError::new("loader", crate::core::i18n::pick("加载器安装超时，请检查网络和安装日志。", "The loader install timed out; check the network and the install log.")))??,
    };
    if !status.success() {
        return Err(AppError::new(
            "loader",
            crate::tr!("{loader} 安装失败，请查看 {}", "{loader} failed to install; see {}", log_path.display()),
        ));
    }
    let installed = paths
        .versions
        .join(&profile_id)
        .join(format!("{profile_id}.json"));
    if !installed.is_file() {
        return Err(AppError::new(
            "loader",
            crate::tr!("安装器没有生成客户端配置；请查看 {}", "The installer made no client profile; see {}", log_path.display()),
        ));
    }
    let overlay: VersionJson = serde_json::from_slice(&std::fs::read(installed)?)?;
    Ok((merge(base, overlay)?, selected))
}

/// Profiles extracted from the old GUI-only installer still need their
/// LaunchWrapper, Scala and ASM libraries before they can be launched.
async fn complete_libraries(client: &reqwest::Client, paths: &AppPaths, version: &VersionJson, cancel: &CancellationToken) -> AppResult<()> {
    let mut tasks = Vec::new();
    for library in &version.libraries {
        let Some(artifact) = library.downloads.as_ref().and_then(|d| d.artifact.as_ref()) else { continue };
        let Some(relative) = crate::core::mojang::version::artifact_path(library) else { continue };
        crate::core::java::runtime::safe_archive_path(&relative)?;
        let target = paths.library_path(&relative);
        if target.is_file() { continue; }
        let Some(url) = artifact.url.as_ref().filter(|u| u.starts_with("https://")) else { continue };
        tasks.push(DownloadTask::new(&library.name, url, vec![format!("https://libraries.minecraft.net/{relative}")], target, artifact.sha1.clone(), artifact.size));
    }
    let progress = Arc::new(DownloadProgress::new(&tasks));
    download::download_all(tasks, client.clone(), 4, cancel.clone(), progress).await
}

/// Forge installers older than 1.12.2-14.23.5.2847 (1.7.10, 1.8.9, 1.10.2 …)
/// cannot install a client without their window. Their profile already holds
/// the finished version JSON (`versionInfo`) and the Forge jar
/// (`install.filePath`), so those are laid out directly instead.
fn legacy_forge(
    paths: &AppPaths,
    base: &VersionJson,
    profile: &serde_json::Value,
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> AppResult<VersionJson> {
    let incomplete = || AppError::new("loader", crate::core::i18n::pick("旧版 Forge 安装器的内容不完整。", "The old Forge installer is incomplete."));
    let text = |pointer: &str| profile.pointer(pointer).and_then(|v| v.as_str()).ok_or_else(incomplete);
    let coordinate = text("/install/path")?;
    let relative = crate::core::mojang::version::maven_path(coordinate).ok_or_else(incomplete)?;
    crate::core::java::runtime::safe_archive_path(&relative)?;
    let target = paths.library_path(&relative);
    let mut entry = archive.by_name(text("/install/filePath")?).map_err(|_| incomplete())?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut bytes)?;
    crate::core::util::atomic_write(&target, &bytes)?;

    let mut info = profile.get("versionInfo").cloned().ok_or_else(incomplete)?;
    // The vanilla libraries come from the base version, which has their
    // downloads and checksums; only Forge's own are added.
    let known: HashSet<String> = base.libraries.iter().map(|library| library_key(&library.name)).collect();
    let mut libraries = Vec::new();
    for library in info.get("libraries").and_then(|v| v.as_array()).into_iter().flatten() {
        let Some(name) = library.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        if library.get("clientreq").and_then(|v| v.as_bool()) == Some(false) || known.contains(&library_key(name)) {
            continue;
        }
        let Some(path) = crate::core::mojang::version::maven_path(name) else {
            continue;
        };
        crate::core::java::runtime::safe_archive_path(&path)?;
        let url = if name == coordinate {
            // Forge's maven only has this jar under its "-universal" name.
            format!("https://maven.minecraftforge.net/{}-universal.jar", path.trim_end_matches(".jar"))
        } else {
            match library.get("url").and_then(|v| v.as_str()) {
                Some(root) => format!("{}{path}", legacy_maven(root)),
                None => format!("https://libraries.minecraft.net/{path}"),
            }
        };
        libraries.push(serde_json::json!({ "name": name, "downloads": { "artifact": { "path": path, "url": url } } }));
    }
    info["libraries"] = serde_json::Value::Array(libraries);
    if info.get("inheritsFrom").is_none() {
        info["inheritsFrom"] = serde_json::json!(base.id);
    }
    Ok(serde_json::from_value(info)?)
}

/// Old profiles name Forge's retired plain-HTTP maven.
fn legacy_maven(root: &str) -> String {
    let root = root.trim_end_matches('/');
    if root.contains("files.minecraftforge.net") || root.contains("maven.minecraftforge.net") {
        "https://maven.minecraftforge.net/".into()
    } else {
        format!("{}/", root.replacen("http://", "https://", 1))
    }
}

pub fn merge(base: &VersionJson, overlay: VersionJson) -> AppResult<VersionJson> {
    if overlay.inherits_from.as_deref() != Some(&base.id) {
        return Err(AppError::new("loader", crate::core::i18n::pick("加载器配置与游戏版本不匹配。", "The loader profile doesn't match the game version.")));
    }
    let mut merged = base.clone();
    merged.main_class = overlay.main_class;
    if let Some(legacy) = overlay.minecraft_arguments {
        merged.minecraft_arguments = Some(legacy);
        merged.arguments = None;
    }
    if let Some(args) = overlay.arguments {
        let target = merged.arguments.get_or_insert_with(Arguments::default);
        target.game.extend(args.game);
        target.jvm.extend(args.jvm);
    }
    if overlay.java_version.is_some() {
        merged.java_version = overlay.java_version;
    }
    for library in overlay.libraries {
        if let Some(path) = crate::core::mojang::version::artifact_path(&library) {
            crate::core::java::runtime::safe_archive_path(&path)?;
        }
        let key = library_key(&library.name);
        merged.libraries.retain(|old| library_key(&old.name) != key);
        merged.libraries.push(library);
    }
    Ok(merged)
}

fn library_key(name: &str) -> String {
    let (coordinate, extension) = name.split_once('@').unwrap_or((name, "jar"));
    let parts: Vec<_> = coordinate.split(':').collect();
    format!(
        "{}:{}:{}@{}",
        parts.first().unwrap_or(&""),
        parts.get(1).unwrap_or(&""),
        parts.get(3).unwrap_or(&""),
        extension
    )
}
fn mirror_url(url: &str) -> String {
    for root in [
        "https://maven.neoforged.net/releases/",
        "https://maven.minecraftforge.net/",
        "https://libraries.minecraft.net/",
        "https://repo.maven.apache.org/maven2/",
        "https://repo1.maven.org/maven2/",
    ] {
        if let Some(path) = url.strip_prefix(root) {
            return format!("https://bmclapi2.bangbang93.com/maven/{path}");
        }
    }
    url.to_string()
}
fn mirror_attempt(url: &str) -> String {
    if url.starts_with("https://bmclapi2.bangbang93.com/") {
        format!("{url}?islet={}", uuid::Uuid::new_v4())
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_forge_profiles_install_without_the_installer_window() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let coordinate = "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10";
        let profile = serde_json::json!({
            "install": { "path": coordinate, "filePath": "forge-1.7.10-10.13.4.1614-1.7.10-universal.jar" },
            "versionInfo": {
                "id": "1.7.10-Forge10.13.4.1614-1.7.10", "type": "release",
                "mainClass": "net.minecraft.launchwrapper.Launch",
                "minecraftArguments": "--username ${auth_player_name} --tweakClass cpw.mods.fml.common.launcher.FMLTweaker",
                "libraries": [
                    { "name": coordinate, "url": "http://files.minecraftforge.net/maven/" },
                    { "name": "net.minecraft:launchwrapper:1.12" },
                    { "name": "org.scala-lang:scala-library:2.11.1", "url": "http://files.minecraftforge.net/maven/", "clientreq": true },
                    { "name": "com.typesafe:config:1.2.1", "clientreq": false },
                    { "name": "com.google.guava:guava:17.0" }
                ]
            }
        });
        let installer = dir.path().join("installer.jar");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&installer).unwrap());
        zip.start_file("forge-1.7.10-10.13.4.1614-1.7.10-universal.jar", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"forge").unwrap();
        zip.finish().unwrap();
        let base: VersionJson = serde_json::from_value(serde_json::json!({
            "id": "1.7.10", "type": "release", "mainClass": "net.minecraft.client.main.Main",
            "minecraftArguments": "--username ${auth_player_name}",
            "libraries": [{ "name": "com.google.guava:guava:17.0" }]
        }))
        .unwrap();

        let mut archive = zip::ZipArchive::new(std::fs::File::open(&installer).unwrap()).unwrap();
        let merged = merge(&base, legacy_forge(&paths, &base, &profile, &mut archive).unwrap()).unwrap();
        let jar = paths.library_path("net/minecraftforge/forge/1.7.10-10.13.4.1614-1.7.10/forge-1.7.10-10.13.4.1614-1.7.10.jar");
        assert_eq!(std::fs::read(jar).unwrap(), b"forge");
        assert_eq!(merged.main_class, "net.minecraft.launchwrapper.Launch");
        assert!(merged.minecraft_arguments.unwrap().contains("FMLTweaker"));
        let urls: Vec<String> = merged
            .libraries
            .iter()
            .filter_map(|l| l.downloads.as_ref()?.artifact.as_ref()?.url.clone())
            .collect();
        assert!(urls.iter().any(|u| u.ends_with("forge-1.7.10-10.13.4.1614-1.7.10-universal.jar")));
        assert!(urls.contains(&"https://libraries.minecraft.net/net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar".to_string()));
        assert!(urls.contains(&"https://maven.minecraftforge.net/org/scala-lang/scala-library/2.11.1/scala-library-2.11.1.jar".to_string()));
        assert!(!merged.libraries.iter().any(|l| l.name.starts_with("com.typesafe:config")));
        assert_eq!(merged.libraries.iter().filter(|l| l.name.starts_with("com.google.guava")).count(), 1);
    }
    #[test]
    fn metadata_matches_exact_minecraft_branch() {
        assert_eq!(neo_prefix("1.21.1"), Some("21.1.".into()));
        assert_eq!(neo_prefix("1.21"), Some("21.0.".into()));
        assert!(neo_prefix("1.20.1").is_none());
        assert!(numeric_key("47.10.0") > numeric_key("47.9.9"));
        assert_eq!(
            parse_versions("<versions><version>47.1.0</version></versions>"),
            vec!["47.1.0"]
        );
        assert!(installer_url("forge", "../x", "47.1.0").is_err());
    }
}
