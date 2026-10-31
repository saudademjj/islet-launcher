use crate::core::download::{DownloadProgress, DownloadTask, download_all};
use crate::core::error::{AppError, AppResult};
use crate::core::java::detect::{self, JavaInstallation};
use crate::core::paths::AppPaths;
use crate::core::platform;
use crate::core::settings::DownloadSource;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub type JavaProgress = Arc<dyn Fn(&str, u64, u64) + Send + Sync>;
#[path = "sources.rs"]
mod sources;

/// A Java for `major` running as `arch` (the game's architecture, see
/// `game_arch`): the chosen path, a runtime the launcher keeps, one found on
/// the machine, or a fresh download.
pub async fn ensure_java(
    client: &reqwest::Client,
    paths: &AppPaths,
    major: u32,
    override_path: Option<&str>,
    arch: &str,
    cancel: &CancellationToken,
    report: Option<JavaProgress>,
) -> AppResult<JavaInstallation> {
    ensure_java_with_source(client, paths, major, override_path, arch, cancel, report, DownloadSource::Bmclapi).await
}

pub async fn ensure_java_with_source(
    client: &reqwest::Client,
    paths: &AppPaths,
    major: u32,
    override_path: Option<&str>,
    arch: &str,
    cancel: &CancellationToken,
    report: Option<JavaProgress>,
    source: DownloadSource,
) -> AppResult<JavaInstallation> {
    check_cancel(cancel)?;
    if !platform::can_run_arch(arch) {
        return Err(AppError::new(
            "rosetta",
            crate::core::i18n::pick("这个版本的游戏只有 Intel 版的本地库，需要用 Rosetta 2 运行。请在「终端」里运行 softwareupdate --install-rosetta 安装后再试。", "This game version has Intel-only native libraries and needs Rosetta 2. Run softwareupdate --install-rosetta in Terminal, then try again."),
        ));
    }
    let arch = platform::normalize_arch(arch).to_string();
    if let Some(path) = override_path.filter(|p| *p != "auto") {
        let path = PathBuf::from(path);
        let java = tokio::task::spawn_blocking(move || detect::inspect(&path))
            .await
            .map_err(|err| AppError::detail("java_path", crate::core::i18n::pick("检测 Java 失败。", "Couldn't check Java."), err.to_string()))?
            .ok_or_else(|| {
                AppError::new(
                    "java_path",
                    crate::core::i18n::pick("Java 路径无效；请选择 bin/java.exe，或清空路径使用自动管理。", "Invalid Java path; choose bin/java.exe, or clear the path to let the launcher manage Java."),
                )
            })?;
        // A path chosen by the user (or set by a crash fix) is explicit: allow
        // a newer major that still satisfies the game's minimum. Automatic
        // runtime selection below keeps the exact-match rule.
        validate_explicit_for(&java, major, &arch)?;
        return Ok(java);
    }
    let root = paths.java_dir("adoptium", major, platform::adoptium_arch_for(&arch));
    if root.join(".complete").is_file() {
        if let Some(binary) = find_java_binary(&root) {
            let java = tokio::task::spawn_blocking(move || detect::inspect(&binary))
                .await
                .ok()
                .flatten();
            if let Some(java) = java.filter(|j| validate_for(j, major, &arch).is_ok()) {
                return Ok(java);
            }
        }
    }
    let wanted = arch.clone();
    let detected = tokio::task::spawn_blocking(move || detect::find_for_major(major, &wanted))
        .await
        .map_err(|err| AppError::detail("java_detect", crate::core::i18n::pick("检测 Java 失败。", "Couldn't check Java."), err.to_string()))?;
    if let Some(java) = detected {
        return Ok(java);
    }
    if let Some(report) = &report {
        report(crate::core::i18n::pick("查询运行时", "Looking up the runtime"), 0, 0);
    }
    let package = sources::resolve(client, major, &arch, source, cancel).await?;
    let archive = match download_package(client, paths, &package, major, &arch, source, cancel, &report).await {
        Ok(archive) => archive,
        Err(error) if source == DownloadSource::Bmclapi && error.code() != "cancelled" => {
            // The API can be ahead of the mirrors. Use the mirror's own
            // release metadata rather than removing checks or mixing files.
            tracing::warn!(major, "Java package unavailable; trying the domestic release catalogue");
            let package = sources::mirror_package(client, major, &arch, cancel).await?;
            download_package(client, paths, &package, major, &arch, source, cancel, &report).await?
        }
        Err(error) => return Err(error),
    };
    check_cancel(cancel)?;
    if let Some(report) = &report {
        report(crate::core::i18n::pick("解压并校验运行时", "Unpacking and verifying the runtime"), 0, 0);
    }
    let stage = root.with_file_name(format!(".install-{}", uuid::Uuid::new_v4()));
    let source = archive.clone();
    let destination = stage.clone();
    let token = cancel.clone();
    let extracted =
        tokio::task::spawn_blocking(move || extract_archive(&source, &destination, &token))
            .await
            .map_err(|err| AppError::detail("java_extract", crate::core::i18n::pick("解压运行时失败。", "Couldn't unpack the runtime."), err.to_string()))?;
    if let Err(error) = extracted {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(error);
    }
    let prepared = (|| -> AppResult<PathBuf> {
        check_cancel(cancel)?;
        let java_binary = find_java_binary(&stage)
            .ok_or_else(|| AppError::new("java_extract", crate::core::i18n::pick("运行时中缺少 Java 可执行文件。", "The runtime has no Java executable.")))?;
        let java = detect::inspect(&java_binary)
            .ok_or_else(|| AppError::new("java_extract", crate::core::i18n::pick("下载的 Java 无法在本机运行。", "The downloaded Java can't run on this computer.")))?;
        validate_for(&java, major, &arch)?;
        let relative = java_binary
            .strip_prefix(&stage)
            .map_err(|_| AppError::new("java_path", crate::core::i18n::pick("Java 路径异常。", "Unexpected Java path.")))?
            .to_path_buf();
        std::fs::write(stage.join(".complete"), b"1")?;
        Ok(relative)
    })();
    let relative = match prepared {
        Ok(p) => p,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&stage);
            return Err(e);
        }
    };
    let previous = root.with_file_name(format!(".previous-{}", uuid::Uuid::new_v4()));
    let had_previous = root.exists();
    if had_previous {
        if let Err(error) = std::fs::rename(&root, &previous) {
            let _ = std::fs::remove_dir_all(&stage);
            return Err(error.into());
        }
    }
    if let Err(error) = std::fs::rename(&stage, &root) {
        if had_previous {
            let _ = std::fs::rename(&previous, &root);
        }
        let _ = std::fs::remove_dir_all(&stage);
        return Err(error.into());
    }
    if had_previous {
        let _ = std::fs::remove_dir_all(previous);
    }
    let _ = std::fs::remove_file(archive);
    detect::inspect(&root.join(relative))
        .ok_or_else(|| AppError::new("java_path", crate::core::i18n::pick("无法读取安装后的 Java。", "Couldn't read the installed Java.")))
}

/// A Linux Java runtime for another computer, such as a server host:
/// its archive name, SHA-256 and where to get it, best source first.
#[derive(Debug, Clone)]
pub struct LinuxRuntime {
    pub name: String,
    pub sha256: String,
    pub urls: Vec<String>,
    pub size: Option<u64>,
}

pub async fn linux_runtime(client: &reqwest::Client, major: u32, arch: &str, source: DownloadSource, cancel: &CancellationToken) -> AppResult<LinuxRuntime> {
    let package = sources::resolve_for(client, major, arch, "linux", source, cancel).await?;
    let urls = sources::download_urls_for(&package, major, arch, "linux", source);
    Ok(LinuxRuntime { name: package.name, sha256: package.checksum, urls, size: package.size })
}

/// Downloads that runtime here, checked against its SHA-256, for hosts that
/// can't fetch it themselves.
pub async fn download_linux_runtime(client: &reqwest::Client, paths: &AppPaths, runtime: &LinuxRuntime, cancel: &CancellationToken) -> AppResult<PathBuf> {
    let archive = paths.java.join("downloads").join(safe_archive_path(&runtime.name)?);
    let mut urls = runtime.urls.clone();
    let task = DownloadTask::new(crate::core::i18n::pick("Java 运行时", "Java runtime"), urls.remove(0), urls, archive.clone(), None, runtime.size)
        .with_sha256(runtime.sha256.clone())
        .with_min_speed(64 * 1024);
    let progress = Arc::new(DownloadProgress::new(std::slice::from_ref(&task)));
    download_all(vec![task], client.clone(), 1, cancel.clone(), progress).await?;
    Ok(archive)
}

async fn download_package(
    client: &reqwest::Client,
    paths: &AppPaths,
    package: &sources::Package,
    major: u32,
    arch: &str,
    source: DownloadSource,
    cancel: &CancellationToken,
    report: &Option<JavaProgress>,
) -> AppResult<PathBuf> {
    let archive = paths.java.join("downloads").join(safe_archive_path(&package.name)?);
    let mut urls = sources::download_urls(package, major, arch, source);
    let task = DownloadTask::new(
        crate::core::i18n::pick("Java 运行时", "Java runtime"),
        urls.remove(0), urls, archive.clone(), None, package.size,
    ).with_sha256(package.checksum.clone()).with_min_speed(64 * 1024);
    let progress = Arc::new(DownloadProgress::new(std::slice::from_ref(&task)));
    let download = download_all(
        vec![task],
        client.clone(),
        1,
        cancel.clone(),
        progress.clone(),
    );
    tokio::pin!(download);
    let mut interval = tokio::time::interval(Duration::from_millis(200));
    loop {
        tokio::select! {
            result = &mut download => { result?; break; },
            _ = interval.tick() => { if let Some(report) = report { let p = progress.snapshot(); report(crate::core::i18n::pick("下载运行时", "Downloading the runtime"), p.bytes_done, p.bytes_total); } }
        }
    }
    Ok(archive)
}

/// Exact match used by the managed/detected automatic runtime cache. Keep this
/// strict so an accidental cache hit cannot substitute a different major.
pub fn validate(java: &JavaInstallation, major: u32) -> AppResult<()> {
    validate_for(java, major, std::env::consts::ARCH)
}

pub fn validate_for(java: &JavaInstallation, major: u32, arch: &str) -> AppResult<()> {
    if java.major != major {
        return Err(AppError::new(
            "java_version",
            crate::tr!("该游戏需要 Java {major}，当前选择为 Java {}。请把这个世界的 Java 改回自动匹配。", "This game needs Java {major}, but Java {} is chosen. Clear the world's Java path to choose automatically.",
                java.major
            ),
        ));
    }
    validate_arch(java, arch)
}

/// Rule for a Java path the user explicitly selected (or a crash fix saved):
/// the selected major must meet the game's minimum, but a newer major is
/// allowed. Crash fixes use this path because a downloaded runtime can be
/// newer than the version recorded in old game metadata.
pub fn validate_explicit(java: &JavaInstallation, required: u32) -> AppResult<()> {
    validate_explicit_for(java, required, std::env::consts::ARCH)
}

pub fn validate_explicit_for(java: &JavaInstallation, required: u32, arch: &str) -> AppResult<()> {
    if java.major < required {
        return Err(AppError::new(
            "java_version",
            crate::tr!("该游戏至少需要 Java {required}，当前选择为 Java {}。请选择 Java {required} 或更高版本。", "This game needs Java {required} or later, but Java {} is chosen. Choose Java {required} or newer.",
                java.major
            ),
        ));
    }
    validate_arch(java, arch)
}

fn validate_arch(java: &JavaInstallation, arch: &str) -> AppResult<()> {
    if !detect::same_arch(&java.arch, arch) {
        let hint = if platform::normalize_arch(arch) == "x86_64" && platform::normalize_arch(std::env::consts::ARCH) == "aarch64" {
            crate::core::i18n::pick("这个版本要用 Intel（x86_64）版 Java 经 Rosetta 2 运行，请清空 Java 路径让启动器自动下载。", "This version runs with an Intel (x86_64) Java under Rosetta 2; clear the Java path so the launcher downloads one.")
        } else {
            crate::core::i18n::pick("请选择相同架构的 Java，或清空路径让启动器自动选择。", "Choose a Java of the same architecture, or clear the path to choose automatically.")
        };
        return Err(AppError::new(
            "java_arch",
            crate::tr!("Java 架构 {} 与游戏需要的 {} 不匹配。{hint}", "Java architecture {} doesn't match the {} the game needs. {hint}", java.arch, platform::normalize_arch(arch)),
        ));
    }
    Ok(())
}

/// Validate using portable rules, including Windows drive prefixes on Unix.
pub fn safe_archive_path(name: &str) -> AppResult<PathBuf> {
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/')
        || normalized.contains(':')
        || normalized.contains('\0')
        || normalized.split('/').any(|part| part == "..")
    {
        return Err(AppError::new("archive_path", crate::core::i18n::pick("压缩包包含不安全的路径。", "The archive has an unsafe path.")));
    }
    let path = PathBuf::from(normalized);
    if path.as_os_str().is_empty() {
        return Err(AppError::new("archive_path", crate::core::i18n::pick("压缩包路径为空。", "An archive path is empty.")));
    }
    Ok(path)
}

fn extract_archive(archive: &Path, dest: &Path, cancel: &CancellationToken) -> AppResult<()> {
    std::fs::create_dir_all(dest)?;
    let file = std::fs::File::open(archive)?;
    if archive.extension().is_some_and(|ext| ext == "zip") {
        let mut zip = zip::ZipArchive::new(file).map_err(|e| {
            AppError::detail("java_extract", crate::core::i18n::pick("无法读取 ZIP 运行时。", "Couldn't read the ZIP runtime."), e.to_string())
        })?;
        for i in 0..zip.len() {
            check_cancel(cancel)?;
            let mut entry = zip
                .by_index(i)
                .map_err(|e| AppError::detail("java_extract", crate::core::i18n::pick("ZIP 文件损坏。", "The ZIP file is damaged."), e.to_string()))?;
            let relative = safe_archive_path(&entry.name().map_err(|e| {
                AppError::detail("java_extract", crate::core::i18n::pick("ZIP 文件名无效。", "Invalid ZIP file name."), e.to_string())
            })?)?;
            if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
                return Err(AppError::new("archive_path", crate::core::i18n::pick("ZIP 中不允许符号链接。", "Symbolic links aren't allowed in the ZIP.")));
            }
            let target = dest.join(relative);
            if entry.is_dir() {
                std::fs::create_dir_all(target)?;
                continue;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut output = std::fs::File::create(&target)?;
            std::io::copy(&mut entry, &mut output)?;
        }
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
        for entry in tar.entries()? {
            check_cancel(cancel)?;
            let mut entry = entry?;
            safe_archive_path(&entry.path()?.to_string_lossy())?;
            if !entry.unpack_in(dest)? {
                return Err(AppError::new("archive_path", crate::core::i18n::pick("运行时路径超出安装目录。", "A runtime path points outside the install folder.")));
            }
        }
    }
    Ok(())
}

fn find_java_binary(root: &Path) -> Option<PathBuf> {
    fn walk(root: &Path, depth: usize) -> Option<PathBuf> {
        if depth > 8 {
            return None;
        }
        for entry in std::fs::read_dir(root).ok()?.flatten() {
            let path = entry.path();
            if entry.file_type().ok()?.is_dir() {
                if let Some(java) = walk(&path, depth + 1) {
                    return Some(java);
                }
            } else if path
                .file_name()
                .is_some_and(|n| n == platform::java_executable())
                && path.parent()?.file_name().is_some_and(|n| n == "bin")
            {
                return Some(path);
            }
        }
        None
    }
    walk(root, 0)
}
fn cancelled() -> AppError {
    AppError::new("cancelled", crate::core::i18n::pick("操作已取消。", "Cancelled."))
}
fn check_cancel(token: &CancellationToken) -> AppResult<()> {
    if token.is_cancelled() {
        Err(cancelled())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opt-in Windows acceptance: the local CONNECT proxy rejects Adoptium
    /// and GitHub, so both metadata and archive must come from domestic hosts.
    #[tokio::test]
    #[ignore = "downloads and executes real Java runtimes; run explicitly on Windows"]
    async fn domestic_java_downloads_without_adoptium_or_github() {
        let proxy = std::env::var("MC_LAUNCHER_TEST_PROXY").expect("acceptance proxy");
        let client = reqwest::Client::builder().no_proxy().proxy(reqwest::Proxy::all(proxy).unwrap())
            .user_agent("Islet/acceptance").connect_timeout(Duration::from_secs(8))
            .read_timeout(Duration::from_secs(20)).build().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let cancel = CancellationToken::new();
        for major in [8, 17, 21, 25] {
            let started = std::time::Instant::now();
            let package = sources::resolve(&client, major, std::env::consts::ARCH, DownloadSource::Bmclapi, &cancel).await.unwrap();
            assert!(package.link.starts_with("https://mirrors.ustc.edu.cn/"));
            let archive = download_package(&client, &paths, &package, major, std::env::consts::ARCH, DownloadSource::Bmclapi, &cancel, &None).await.unwrap();
            let root = dir.path().join(format!("java-{major}"));
            extract_archive(&archive, &root, &cancel).unwrap();
            let java = detect::inspect(&find_java_binary(&root).unwrap()).unwrap();
            validate_for(&java, major, std::env::consts::ARCH).unwrap();
            println!("JAVA {major} PASS version={} arch={} elapsed={:?}", java.version, java.arch, started.elapsed());
        }
    }

    fn java(major: u32) -> JavaInstallation {
        JavaInstallation {
            path: format!("/java/{major}/bin/java"),
            version: format!("{major}.0.1"),
            major,
            arch: std::env::consts::ARCH.to_string(),
            vendor: "test".to_string(),
        }
    }

    #[test]
    fn explicit_java_may_be_newer_than_the_required_major() {
        assert!(validate_explicit(&java(21), 17).is_ok());
        assert!(validate_explicit(&java(21), 21).is_ok());
        assert!(validate_explicit(&java(17), 21).is_err());
    }

    #[test]
    fn automatic_runtime_validation_stays_exact() {
        assert!(validate(&java(21), 21).is_ok());
        assert!(validate(&java(21), 17).is_err());
        assert!(validate(&java(17), 21).is_err());
    }

    #[test]
    fn explicit_java_still_checks_architecture() {
        let mut wrong_arch = java(21);
        wrong_arch.arch = if std::env::consts::ARCH == "aarch64" {
            "x86_64".to_string()
        } else {
            "aarch64".to_string()
        };
        assert!(validate_explicit(&wrong_arch, 17).is_err());
    }

    #[test]
    fn rejects_traversal_for_both_platform_path_syntaxes() {
        for value in ["../evil", "..\\evil", "C:\\evil", "/evil", "a/../../evil"] {
            assert!(safe_archive_path(value).is_err(), "{value}");
        }
        assert_eq!(
            safe_archive_path("jdk/bin/java.exe").unwrap(),
            PathBuf::from("jdk/bin/java.exe")
        );
    }
    #[test]
    fn extracts_windows_jre_zip_in_isolated_directory() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("jre.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        zip.start_file("jre/bin/java.exe", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"test executable").unwrap();
        zip.finish().unwrap();
        let dest = root.path().join("extract");
        extract_archive(&archive, &dest, &CancellationToken::new()).unwrap();
        assert_eq!(
            std::fs::read(dest.join("jre/bin/java.exe")).unwrap(),
            b"test executable"
        );
    }
}
