//! A world as a dedicated server: the official server for its game and
//! loader (or Paper or Purpur for plugins), the mods a server runs
//! (Modrinth says which are client-only), its config folders and start
//! scripts, in a folder ready to run here or to upload to a host.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::core::download::{self, DownloadProgress, DownloadTask};
use crate::core::error::{AppError, AppResult};
use crate::core::i18n::pick;
use crate::core::instances::{Instance, runs_plugins};
use crate::core::mojang::version::VersionJson;
use crate::core::paths::AppPaths;
use crate::core::settings::DownloadSource;

/// Folders of settings a server reads as the client does.
const CONFIG_DIRS: &[&str] = &["config", "defaultconfigs", "kubejs", "scripts"];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerOptions {
    /// The player agreed to the Minecraft EULA in the launcher.
    pub accept_eula: bool,
    /// MB for the server; 0 works it out on the machine it starts on.
    pub memory_mb: u32,
    /// A save (its folder under `saves`) that becomes the server's world.
    pub world: Option<String>,
    pub port: u16,
    /// Only players signed in with Microsoft get in.
    pub online_mode: bool,
    /// Also write the pack friends import to play on the server.
    pub client_pack: bool,
    /// Where friends join, written into their pack.
    pub address: String,
    /// The UUID the host plays as on the server, so their inventory and
    /// position in the save come along. Filled in by the launcher.
    #[serde(skip)]
    pub host_uuid: Option<String>,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self { accept_eula: false, memory_mb: 4096, world: None, port: 25565, online_mode: true, client_pack: true, address: String::new(), host_uuid: None }
    }
}

#[derive(Debug, Default)]
pub struct Summary {
    pub mods: usize,
    /// Mods left out because they only run on the client.
    pub client_only: Vec<String>,
    /// The mods the server runs, as (file name, SHA-1).
    pub included: Vec<(String, String)>,
    /// A save became the server's world.
    pub world: bool,
}

/// What a save says about how it is played, for `server.properties`.
#[derive(Debug, Default, PartialEq)]
struct WorldRules {
    difficulty: Option<i8>,
    game_mode: Option<i32>,
    hardcore: bool,
}

/// How the start scripts run the server.
enum Start {
    /// `java -jar <jar> nogui`.
    Jar(String),
    /// Modern Forge and NeoForge: an argument file per platform.
    ArgFiles { unix: String, windows: String },
}

fn fail(zh: &'static str, en: &'static str, detail: impl Into<String>) -> AppError {
    AppError::detail("server_pack", pick(zh, en), detail.into())
}

fn sha1_of(path: &Path) -> AppResult<String> {
    use sha1::Digest;
    Ok(hex::encode(sha1::Sha1::digest(fs::read(path)?)))
}

async fn fetch(client: &reqwest::Client, task: DownloadTask, cancel: &CancellationToken) -> AppResult<()> {
    let progress = Arc::new(DownloadProgress::new(std::slice::from_ref(&task)));
    download::download_all(vec![task], client.clone(), 1, cancel.clone(), progress).await
}

/// Builds the server in `destination`, which must be empty or not exist.
/// `base` is the vanilla version, `java` the Java the world runs with.
#[allow(clippy::too_many_arguments)]
pub async fn create(
    client: &reqwest::Client,
    paths: &AppPaths,
    instance: &Instance,
    base: &VersionJson,
    java: &Path,
    source: DownloadSource,
    options: &ServerOptions,
    destination: &Path,
    cancel: &CancellationToken,
) -> AppResult<Summary> {
    if fs::read_dir(destination).is_ok_and(|mut entries| entries.next().is_some()) {
        return Err(AppError::new("server_pack", pick("请选一个空文件夹放服务端。", "Choose an empty folder for the server.")));
    }
    let loader_version = match instance.loader.as_str() {
        "vanilla" | "paper" | "purpur" => String::new(),
        "fabric" | "quilt" | "forge" | "neoforge" => instance.loader_version.clone().ok_or_else(|| {
            AppError::new("server_pack", pick("先安装一次这个世界，确定加载器版本后再导出服务端。", "Install this world once so its loader version is fixed, then export the server."))
        })?,
        _ => {
            return Err(AppError::new(
                "server_pack",
                pick("这个加载器没有可以一键生成的服务端；原版、Fabric、Quilt、Forge、NeoForge、Paper 和 Purpur 可以。", "This loader has no server Islet can set up; vanilla, Fabric, Quilt, Forge, NeoForge, Paper and Purpur do."),
            ));
        }
    };
    let save = options.world.as_deref().map(|folder| crate::core::content::world_dir(paths, &instance.id, folder)).transpose()?;
    fs::create_dir_all(destination)?;
    let mut summary = copy_content(client, paths, instance, destination).await?;
    let start = install(client, paths, instance, base, &loader_version, java, source, destination, cancel).await?;
    write_scripts(destination, java, options.memory_mb, &start)?;
    let rules = match &save {
        Some(save) => {
            summary.world = true;
            copy_world(save, &destination.join("world"), options.host_uuid.as_deref())?
        }
        None => WorldRules::default(),
    };
    fs::write(destination.join("server.properties"), properties(instance, options, &rules))?;
    if options.accept_eula {
        fs::write(destination.join("eula.txt"), "# Agreed to in Islet: https://aka.ms/MinecraftEULA\neula=true\n")?;
    }
    Ok(summary)
}

/// Mods (or plugins) the server runs, and the config folders.
async fn copy_content(client: &reqwest::Client, paths: &AppPaths, instance: &Instance, destination: &Path) -> AppResult<Summary> {
    let game = paths.instance_game_dir(&instance.id);
    // A plugin server's plugins wait in the project's mods folder too.
    let folder = if runs_plugins(&instance.loader) { "plugins" } else { "mods" };
    let mut mods = Vec::new();
    for entry in fs::read_dir(game.join("mods")).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().is_ok_and(|kind| kind.is_file()) && name.to_lowercase().ends_with(".jar") {
            mods.push((name, entry.path()));
        }
    }
    let hashes = mods.iter().map(|(_, path)| sha1_of(path)).collect::<AppResult<Vec<_>>>()?;
    // Without the lookup every mod goes along; a client-only one may then
    // stop the server, which the summary can't warn about.
    let sides = crate::core::modrinth::server_sides(client, &hashes).await.unwrap_or_else(|error| {
        tracing::warn!(%error, "server side lookup failed");
        Default::default()
    });
    let mut summary = Summary::default();
    for ((name, path), hash) in mods.iter().zip(&hashes) {
        if sides.get(hash).is_some_and(|side| side == "unsupported") {
            summary.client_only.push(name.clone());
            continue;
        }
        fs::create_dir_all(destination.join(folder))?;
        fs::copy(path, destination.join(folder).join(name))?;
        summary.mods += 1;
        summary.included.push((name.clone(), hash.clone()));
    }
    for folder in CONFIG_DIRS {
        let from = game.join(folder);
        if from.is_dir() {
            crate::core::manage::copy_tree(&from, &destination.join(folder))?;
        }
    }
    Ok(summary)
}

#[allow(clippy::too_many_arguments)]
async fn install(
    client: &reqwest::Client,
    paths: &AppPaths,
    instance: &Instance,
    base: &VersionJson,
    loader_version: &str,
    java: &Path,
    source: DownloadSource,
    destination: &Path,
    cancel: &CancellationToken,
) -> AppResult<Start> {
    let mc = instance.version_id.as_str();
    match instance.loader.as_str() {
        "vanilla" => {
            fetch(client, crate::core::mojang::install::server_jar_task(base, source, destination.join("server.jar"))?, cancel).await?;
            Ok(Start::Jar("server.jar".into()))
        }
        "paper" | "purpur" => {
            plugin_server(client, instance, base, source, destination, cancel).await?;
            Ok(Start::Jar("server.jar".into()))
        }
        "fabric" => {
            // Fabric's server launcher fetches its libraries on first start,
            // and runs the vanilla server.jar beside it.
            fetch(client, crate::core::mojang::install::server_jar_task(base, source, destination.join("server.jar"))?, cancel).await?;
            #[derive(Deserialize)]
            struct Installer {
                version: String,
                stable: bool,
            }
            let meta = "https://meta.fabricmc.net/v2";
            let installers: Vec<Installer> = client.get(format!("{meta}/versions/installer")).timeout(Duration::from_secs(20)).send().await?.error_for_status()?.json().await?;
            let installer = installers.iter().find(|i| i.stable).or(installers.first()).ok_or_else(|| fail("找不到 Fabric 安装器。", "No Fabric installer was found.", ""))?;
            let path = format!("versions/loader/{mc}/{loader_version}/{}/server/jar", installer.version);
            let mut urls = vec![format!("{meta}/{path}")];
            if source == DownloadSource::Bmclapi {
                urls.insert(0, format!("https://bmclapi2.bangbang93.com/fabric-meta/v2/{path}"));
            }
            let first = urls.remove(0);
            fetch(client, DownloadTask::new("Fabric", first, urls, destination.join("fabric-server-launch.jar"), None, None), cancel).await?;
            Ok(Start::Jar("fabric-server-launch.jar".into()))
        }
        "quilt" => {
            #[derive(Deserialize)]
            struct Installer {
                url: String,
            }
            let installers: Vec<Installer> = client.get("https://meta.quiltmc.org/v3/versions/installer").timeout(Duration::from_secs(20)).send().await?.error_for_status()?.json().await?;
            let url = installers.first().map(|i| i.url.clone()).filter(|url| url.starts_with("https://maven.quiltmc.org/")).ok_or_else(|| fail("找不到 Quilt 安装器。", "No Quilt installer was found.", ""))?;
            let jar = paths.cache.join("installers").join("quilt-installer.jar");
            let _ = fs::remove_file(&jar);
            fetch(client, DownloadTask::new("Quilt", url, Vec::new(), jar.clone(), None, None), cancel).await?;
            let install_dir = format!("--install-dir={}", destination.display());
            run_installer(java, &jar, &["install", "server", mc, loader_version, "--download-server", &install_dir], destination, cancel).await?;
            Ok(Start::Jar("quilt-server-launch.jar".into()))
        }
        loader => {
            let installer = crate::core::loaders::fetch_installer(client, paths, loader, mc, loader_version, cancel).await?;
            run_installer(java, &installer, &["--installServer", &destination.display().to_string()], destination, cancel).await?;
            for log in ["installer.log", &format!("{}.log", installer.file_name().unwrap_or_default().to_string_lossy())] {
                let _ = fs::remove_file(destination.join(log));
            }
            forge_start(destination)
        }
    }
}

/// Paper's or Purpur's newest build for the version, checked against the
/// hash its API gives. Both patch the vanilla server on first start; it is
/// put where they look, from the chosen source, so they needn't fetch it.
async fn plugin_server(client: &reqwest::Client, instance: &Instance, base: &VersionJson, source: DownloadSource, destination: &Path, cancel: &CancellationToken) -> AppResult<()> {
    let mc = instance.version_id.as_str();
    if !crate::core::fabric::valid_version(mc) {
        return Err(fail("版本号无效。", "Invalid version.", mc));
    }
    let unsupported = || {
        AppError::new(
            "server_pack",
            crate::tr!("{} 还不支持 Minecraft {mc}，换一个版本试试。", "{} doesn't support Minecraft {mc} yet; try another version.", loader_name(&instance.loader)),
        )
    };
    let jar = destination.join("server.jar");
    if instance.loader == "paper" {
        #[derive(Deserialize)]
        struct Checksums {
            sha256: String,
        }
        #[derive(Deserialize)]
        struct Download {
            url: String,
            checksums: Checksums,
            size: Option<u64>,
        }
        #[derive(Deserialize)]
        struct Build {
            downloads: std::collections::HashMap<String, Download>,
        }
        let response = client.get(format!("https://fill.papermc.io/v3/projects/paper/versions/{mc}/builds/latest")).timeout(Duration::from_secs(20)).send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(unsupported());
        }
        let build: Build = response.error_for_status()?.json().await?;
        let download = build.downloads.get("server:default").filter(|d| d.url.starts_with("https://")).ok_or_else(unsupported)?;
        fetch(client, DownloadTask::new("Paper", download.url.clone(), Vec::new(), jar, None, download.size).with_sha256(download.checksums.sha256.clone()), cancel).await?;
    } else {
        #[derive(Deserialize)]
        struct Build {
            build: String,
            md5: String,
        }
        let response = client.get(format!("https://api.purpurmc.org/v2/purpur/{mc}/latest")).timeout(Duration::from_secs(20)).send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(unsupported());
        }
        let build: Build = response.error_for_status()?.json().await?;
        if !build.build.bytes().all(|b| b.is_ascii_digit()) || build.build.is_empty() {
            return Err(fail("Purpur 返回的构建号无效。", "Purpur gave an invalid build number.", build.build));
        }
        fetch(client, DownloadTask::new("Purpur", format!("https://api.purpurmc.org/v2/purpur/{mc}/{}/download", build.build), Vec::new(), jar.clone(), None, None), cancel).await?;
        let sum = format!("{:x}", md5::compute(fs::read(&jar)?));
        if !sum.eq_ignore_ascii_case(&build.md5) {
            let _ = fs::remove_file(&jar);
            return Err(fail("Purpur 下载的文件校验不对，请重试。", "The Purpur download didn't match its checksum; try again.", format!("{sum} != {}", build.md5)));
        }
    }
    let vanilla = destination.join("cache").join(format!("mojang_{mc}.jar"));
    if let Err(error) = fetch(client, crate::core::mojang::install::server_jar_task(base, source, vanilla.clone())?, cancel).await {
        // The server fetches it itself then.
        tracing::warn!(%error, "couldn't fetch the vanilla server for patching");
        let _ = fs::remove_file(&vanilla);
    }
    Ok(())
}

fn loader_name(loader: &str) -> &str {
    match loader {
        "paper" => "Paper",
        "purpur" => "Purpur",
        "fabric" => "Fabric",
        "quilt" => "Quilt",
        "forge" => "Forge",
        "neoforge" => "NeoForge",
        _ => loader,
    }
}

/// Runs an official installer with the world's Java, in `cwd`.
async fn run_installer(java: &Path, jar: &Path, args: &[&str], cwd: &Path, cancel: &CancellationToken) -> AppResult<()> {
    let mut command = tokio::process::Command::from(crate::core::platform::quiet_command(java));
    command.arg("-jar").arg(jar).args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let output = tokio::select! {
        output = command.output() => output.map_err(|error| fail("无法运行服务端安装器。", "Couldn't run the server installer.", error.to_string()))?,
        () = cancel.cancelled() => return Err(AppError::new("cancelled", pick("操作已取消。", "Cancelled."))),
    };
    if !output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = text.lines().rev().take(12).collect();
        return Err(fail("服务端安装器没有装好，看看详情里的输出。", "The server installer failed; see its output in the details.", tail.into_iter().rev().collect::<Vec<_>>().join("\n")));
    }
    Ok(())
}

/// How a Forge or NeoForge server starts: argument files since 1.17, a jar
/// before that.
fn forge_start(destination: &Path) -> AppResult<Start> {
    let argument_file = |script: &str, name: &str| {
        let text = fs::read_to_string(destination.join(script)).ok()?;
        text.split_whitespace().find(|word| word.starts_with("@libraries/") && word.ends_with(name)).map(str::to_string)
    };
    if let (Some(unix), Some(windows)) = (argument_file("run.sh", "unix_args.txt"), argument_file("run.bat", "win_args.txt")) {
        return Ok(Start::ArgFiles { unix, windows });
    }
    let jar = fs::read_dir(destination)?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .find(|name| (name.starts_with("forge-") || name.starts_with("neoforge-")) && name.ends_with(".jar") && !name.contains("installer"))
        .ok_or_else(|| fail("安装器没有生成服务端。", "The installer made no server.", "no server jar or argument files"))?;
    Ok(Start::Jar(jar))
}

/// The console `java` beside a `javaw`, which has no console for a server.
fn console_java(java: &Path) -> PathBuf {
    let name = java.file_name().unwrap_or_default().to_string_lossy();
    if name.eq_ignore_ascii_case("javaw.exe") || name.eq_ignore_ascii_case("javaw") {
        let console = java.with_file_name(if name.ends_with(".exe") { "java.exe" } else { "java" });
        if console.exists() {
            return console;
        }
    }
    java.to_path_buf()
}

/// G1 settings for game servers (Aikar's flags), the young generation
/// sized for heaps up to and over 12 GB. Options a newer Java has dropped
/// are skipped rather than stopping it.
const GC_FLAGS: &str = "-XX:+IgnoreUnrecognizedVMOptions -XX:+UseG1GC -XX:+ParallelRefProcEnabled -XX:MaxGCPauseMillis=200 -XX:+UnlockExperimentalVMOptions -XX:+DisableExplicitGC -XX:+AlwaysPreTouch -XX:G1HeapWastePercent=5 -XX:G1MixedGCCountTarget=4 -XX:G1MixedGCLiveThresholdPercent=90 -XX:G1RSetUpdatingPauseTimePercent=5 -XX:SurvivorRatio=32 -XX:+PerfDisableSharedMem -XX:MaxTenuringThreshold=1";
const GC_SMALL: &str = "-XX:G1NewSizePercent=30 -XX:G1MaxNewSizePercent=40 -XX:G1HeapRegionSize=8M -XX:G1ReservePercent=20 -XX:InitiatingHeapOccupancyPercent=15";
const GC_LARGE: &str = "-XX:G1NewSizePercent=40 -XX:G1MaxNewSizePercent=50 -XX:G1HeapRegionSize=16M -XX:G1ReservePercent=15 -XX:InitiatingHeapOccupancyPercent=20";

/// `start.sh` and `start.bat`: the world's Java unless `JAVA` says
/// otherwise or it isn't there (on another computer), then `java`. The
/// memory is `MC_MEMORY`; "auto" (from `memory_mb` 0) leaves a quarter of
/// the machine, at least 768 MB, to everything else. How far players see
/// follows the memory until `server.properties` says otherwise.
fn write_scripts(destination: &Path, java: &Path, memory_mb: u32, start: &Start) -> AppResult<()> {
    let java = console_java(java);
    let memory = if memory_mb == 0 { "auto".to_string() } else { memory_mb.clamp(512, 32768).to_string() };
    let (unix, windows) = match start {
        Start::Jar(jar) => (format!("-jar {jar} nogui"), format!("-jar {jar} nogui")),
        Start::ArgFiles { unix, windows } => {
            fs::write(destination.join("user_jvm_args.txt"), "# More Java options for the server; start.sh and start.bat set its memory.\n")?;
            (format!("@user_jvm_args.txt {unix} nogui"), format!("@user_jvm_args.txt {windows} nogui"))
        }
    };
    let sh = format!(
        r#"#!/usr/bin/env sh
# Starts the server. Set JAVA to use another Java, MC_MEMORY to give it
# another amount of memory (MB, or auto).
cd "$(dirname "$0")" || exit 1
JAVA="${{JAVA:-{java}}}"
[ -x "$JAVA" ] || JAVA=java
MC_MEMORY="${{MC_MEMORY:-{memory}}}"
if [ "$MC_MEMORY" = auto ]; then
	total=$(awk '/^MemTotal:/ {{ print int($2 / 1024) }}' /proc/meminfo 2>/dev/null)
	[ -n "$total" ] || total=$(( $(sysctl -n hw.memsize 2>/dev/null || echo 4294967296) / 1048576 ))
	for f in /sys/fs/cgroup/memory.max /sys/fs/cgroup/memory/memory.limit_in_bytes; do
		limit=$(cat "$f" 2>/dev/null)
		case "$limit" in ''|*[!0-9]*) ;; *) [ $((limit / 1048576)) -lt "$total" ] && total=$((limit / 1048576)) ;; esac
	done
	reserve=$((total / 4))
	[ "$reserve" -lt 768 ] && reserve=768
	MC_MEMORY=$((total - reserve))
	# What other programs hold stays theirs, and Java needs room beside
	# the heap: it takes all of the heap at start.
	free=$(awk '/^MemAvailable:/ {{ print int($2 / 1024) }}' /proc/meminfo 2>/dev/null)
	if [ -n "$free" ]; then
		room=$((free - free / 6 - 512))
		[ "$room" -lt "$MC_MEMORY" ] && MC_MEMORY=$room
	fi
	[ "$MC_MEMORY" -lt 512 ] && MC_MEMORY=512
	[ "$MC_MEMORY" -gt 32768 ] && MC_MEMORY=32768
fi
if ! grep -q '^view-distance=' server.properties 2>/dev/null; then
	if [ "$MC_MEMORY" -le 2048 ]; then v=6 s=4
	elif [ "$MC_MEMORY" -le 4096 ]; then v=8 s=6
	elif [ "$MC_MEMORY" -le 8192 ]; then v=10 s=8
	else v=12 s=10; fi
	printf 'view-distance=%s\nsimulation-distance=%s\n' "$v" "$s" >> server.properties
fi
if [ "$MC_MEMORY" -gt 12288 ]; then G1="{large}"; else G1="{small}"; fi
echo "Starting with ${{MC_MEMORY}} MB of memory"
"$JAVA" -Xms${{MC_MEMORY}}M -Xmx${{MC_MEMORY}}M {flags} $G1 {unix}
"#,
        java = java.display().to_string().replace('"', "\\\""),
        large = GC_LARGE,
        small = GC_SMALL,
        flags = GC_FLAGS,
    );
    let bat = format!(
        "@echo off\r\n\
rem Starts the server. Set JAVA to use another Java, MC_MEMORY to give it\r\n\
rem another amount of memory (MB, or auto).\r\n\
cd /d \"%~dp0\"\r\n\
if not defined JAVA set \"JAVA={java}\"\r\n\
if not exist \"%JAVA%\" set \"JAVA=java\"\r\n\
if not defined MC_MEMORY set \"MC_MEMORY={memory}\"\r\n\
if /i not \"%MC_MEMORY%\"==\"auto\" goto memory\r\n\
set \"TOTAL=4096\"\r\n\
for /f %%m in ('powershell -NoProfile -Command \"[int]((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory/1MB)\"') do set \"TOTAL=%%m\"\r\n\
set /a RESERVE=TOTAL/4\r\n\
if %RESERVE% LSS 768 set RESERVE=768\r\n\
set /a MC_MEMORY=TOTAL-RESERVE\r\n\
set \"FREE=\"\r\n\
for /f %%m in ('powershell -NoProfile -Command \"[int]((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory/1KB)\"') do set \"FREE=%%m\"\r\n\
if not defined FREE goto clamp\r\n\
set /a ROOM=FREE-FREE/6-512\r\n\
if %ROOM% LSS %MC_MEMORY% set MC_MEMORY=%ROOM%\r\n\
:clamp\r\n\
if %MC_MEMORY% LSS 512 set MC_MEMORY=512\r\n\
if %MC_MEMORY% GTR 32768 set MC_MEMORY=32768\r\n\
:memory\r\n\
findstr /b \"view-distance=\" server.properties >nul 2>&1 || call :distance\r\n\
set \"G1={small}\"\r\n\
if %MC_MEMORY% GTR 12288 set \"G1={large}\"\r\n\
echo Starting with %MC_MEMORY% MB of memory\r\n\
\"%JAVA%\" -Xms%MC_MEMORY%M -Xmx%MC_MEMORY%M {flags} %G1% {windows}\r\n\
pause\r\n\
exit /b\r\n\
\r\n\
:distance\r\n\
set \"V=12\" & set \"S=10\"\r\n\
if %MC_MEMORY% LEQ 8192 (set \"V=10\" & set \"S=8\")\r\n\
if %MC_MEMORY% LEQ 4096 (set \"V=8\" & set \"S=6\")\r\n\
if %MC_MEMORY% LEQ 2048 (set \"V=6\" & set \"S=4\")\r\n\
>>server.properties echo view-distance=%V%\r\n\
>>server.properties echo simulation-distance=%S%\r\n\
exit /b\r\n",
        java = java.display(),
        large = GC_LARGE,
        small = GC_SMALL,
        flags = GC_FLAGS,
    );
    let script = destination.join("start.sh");
    fs::write(&script, sh)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
    }
    fs::write(destination.join("start.bat"), bat)?;
    Ok(())
}

/// A save as the server's world. The host's player lives in `level.dat` in
/// singleplayer; it moves to `playerdata` so they keep their things.
fn copy_world(save: &Path, world: &Path, host_uuid: Option<&str>) -> AppResult<WorldRules> {
    use crate::core::servers::nbt::{self, Tag};
    use std::io::{Read, Write};
    crate::core::manage::copy_tree(save, world)?;
    let _ = fs::remove_file(world.join("session.lock"));
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(fs::File::open(world.join("level.dat"))?).read_to_end(&mut bytes)?;
    let Ok((_, root)) = nbt::read(&bytes) else {
        return Ok(WorldRules::default());
    };
    let Some(data) = root.get("Data") else {
        return Ok(WorldRules::default());
    };
    let rules = WorldRules {
        difficulty: data.get("Difficulty").and_then(Tag::as_byte),
        game_mode: match data.get("GameType") {
            Some(Tag::Int(mode)) => Some(*mode),
            _ => None,
        },
        hardcore: data.get("hardcore").and_then(Tag::as_byte).is_some_and(|flag| flag != 0),
    };
    if let (Some(uuid), Some(player @ Tag::Compound(_))) = (host_uuid, data.get("Player")) {
        let file = world.join("playerdata").join(format!("{uuid}.dat"));
        if !file.exists() {
            fs::create_dir_all(world.join("playerdata"))?;
            let mut out = flate2::write::GzEncoder::new(fs::File::create(&file)?, flate2::Compression::default());
            out.write_all(&nbt::write("", player))?;
            out.finish()?;
        }
    }
    Ok(rules)
}

/// `server.properties` for a new server. Numbers for difficulty and game
/// mode, which every version reads.
fn properties(instance: &Instance, options: &ServerOptions, rules: &WorldRules) -> String {
    let project = instance.server.as_ref();
    let mut lines = vec![
        "# Written by Islet; the server adds the rest on first start.".to_string(),
        format!("motd={}", properties_escape(&instance.name)),
        format!("server-port={}", if options.port == 0 { 25565 } else { options.port }),
        format!("online-mode={}", options.online_mode),
        "level-name=world".to_string(),
    ];
    if let Some(difficulty) = rules.difficulty {
        lines.push(format!("difficulty={}", difficulty.clamp(0, 3)));
    }
    if let Some(mode) = rules.game_mode {
        lines.push(format!("gamemode={}", mode.clamp(0, 3)));
    }
    if let Some(project) = project {
        // A project's own settings come last and win.
        lines.extend(project_properties(instance).into_iter().map(|(key, value)| format!("{key}={value}")));
        if !project.seed.trim().is_empty() {
            lines.push(format!("level-seed={}", properties_escape(project.seed.trim())));
        }
    }
    if rules.hardcore {
        lines.push("hardcore=true".into());
    }
    // Mods with jetpacks, wings and the like get players kicked for flying.
    if instance.loader != "vanilla" && !runs_plugins(&instance.loader) {
        lines.push("allow-flight=true".into());
    }
    // One line per key, the last one written.
    let mut seen = std::collections::HashSet::new();
    let mut kept: Vec<String> = lines.into_iter().rev().filter(|line| line.starts_with('#') || seen.insert(line.split('=').next().unwrap_or_default().to_string())).collect();
    kept.reverse();
    kept.join("\n") + "\n"
}

/// The `server.properties` lines a server project's settings decide, kept
/// up to date on the host by every upload.
pub fn project_properties(instance: &Instance) -> Vec<(&'static str, String)> {
    let Some(project) = &instance.server else { return Vec::new() };
    let motd = Some(project.motd.trim()).filter(|motd| !motd.is_empty()).unwrap_or(&instance.name);
    vec![
        ("motd", properties_escape(motd)),
        ("max-players", project.max_players.clamp(1, 1000).to_string()),
        ("difficulty", project.difficulty.min(3).to_string()),
        ("gamemode", project.gamemode.min(3).to_string()),
        ("pvp", project.pvp.to_string()),
        ("white-list", project.whitelist.to_string()),
        ("enforce-whitelist", project.whitelist.to_string()),
    ]
}

/// `.properties` values are Latin-1; anything else goes as `\uXXXX`.
fn properties_escape(text: &str) -> String {
    let mut out = String::new();
    for unit in text.encode_utf16() {
        match unit {
            0x20..=0x7e if !matches!(unit as u8, b'\\' | b'=' | b':' | b'#' | b'!') => out.push(unit as u8 as char),
            _ => out.push_str(&format!("\\u{unit:04x}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_are_escaped() {
        assert_eq!(properties_escape("My World"), "My World");
        assert_eq!(properties_escape("生存=1"), "\\u751f\\u5b58\\u003d1");
    }

    #[test]
    fn a_save_becomes_the_world_with_the_hosts_player() {
        use crate::core::servers::nbt::{self, Tag};
        use std::io::{Read, Write};
        let dir = tempfile::tempdir().unwrap();
        let save = dir.path().join("save");
        fs::create_dir_all(save.join("region")).unwrap();
        fs::write(save.join("region/r.0.0.mca"), b"chunks").unwrap();
        fs::write(save.join("session.lock"), b"").unwrap();
        let player = Tag::Compound(vec![("XpLevel".into(), Tag::Int(30))]);
        let level = Tag::Compound(vec![(
            "Data".into(),
            Tag::Compound(vec![
                ("Difficulty".into(), Tag::Byte(3)),
                ("GameType".into(), Tag::Int(1)),
                ("hardcore".into(), Tag::Byte(0)),
                ("Player".into(), player.clone()),
            ]),
        )]);
        let mut gz = flate2::write::GzEncoder::new(fs::File::create(save.join("level.dat")).unwrap(), flate2::Compression::default());
        gz.write_all(&nbt::write("", &level)).unwrap();
        gz.finish().unwrap();

        let world = dir.path().join("server/world");
        let rules = copy_world(&save, &world, Some("069a79f4-44e9-4726-a5be-fca90e38aaf5")).unwrap();
        assert_eq!(rules, WorldRules { difficulty: Some(3), game_mode: Some(1), hardcore: false });
        assert!(world.join("region/r.0.0.mca").is_file());
        assert!(!world.join("session.lock").exists());
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(fs::File::open(world.join("playerdata/069a79f4-44e9-4726-a5be-fca90e38aaf5.dat")).unwrap()).read_to_end(&mut bytes).unwrap();
        assert_eq!(nbt::read(&bytes).unwrap().1, player);

        let instance: Instance = serde_json::from_value(serde_json::json!({
            "id": "a", "name": "生存", "versionId": "1.21.1", "versionType": "release", "createdAt": 0, "installed": true, "loader": "fabric"
        }))
        .unwrap();
        let options = ServerOptions { online_mode: false, port: 25570, ..Default::default() };
        let text = properties(&instance, &options, &rules);
        for line in ["motd=\\u751f\\u5b58", "server-port=25570", "online-mode=false", "difficulty=3", "gamemode=1", "allow-flight=true"] {
            assert!(text.lines().any(|l| l == line), "{line} in {text}");
        }
        assert!(!text.contains("hardcore"));
    }

    #[test]
    fn modern_forge_starts_from_argument_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("run.sh"), "#!/usr/bin/env sh\njava @user_jvm_args.txt @libraries/net/neoforged/neoforge/21.1.1/unix_args.txt \"$@\"\n").unwrap();
        fs::write(dir.path().join("run.bat"), "java @user_jvm_args.txt @libraries/net/neoforged/neoforge/21.1.1/win_args.txt %*\r\npause\r\n").unwrap();
        let Start::ArgFiles { unix, windows } = forge_start(dir.path()).unwrap() else { panic!("expected argument files") };
        assert_eq!(unix, "@libraries/net/neoforged/neoforge/21.1.1/unix_args.txt");
        assert_eq!(windows, "@libraries/net/neoforged/neoforge/21.1.1/win_args.txt");

        write_scripts(dir.path(), Path::new("/opt/java/bin/java"), 4096, &Start::ArgFiles { unix, windows }).unwrap();
        let sh = fs::read_to_string(dir.path().join("start.sh")).unwrap();
        assert!(sh.contains("JAVA=\"${JAVA:-/opt/java/bin/java}\""));
        assert!(sh.contains("$G1 @user_jvm_args.txt @libraries/net/neoforged/neoforge/21.1.1/unix_args.txt nogui"));
        assert!(sh.contains("MC_MEMORY=\"${MC_MEMORY:-4096}\""));
        assert!(sh.contains("-Xmx${MC_MEMORY}M"));
        assert!(!fs::read_to_string(dir.path().join("user_jvm_args.txt")).unwrap().contains("-Xmx"));
        assert!(fs::read_to_string(dir.path().join("start.bat")).unwrap().contains("win_args.txt nogui"));
    }

    /// The automatic memory and view distance, run by `sh` with a made-up
    /// machine.
    #[cfg(unix)]
    #[test]
    fn automatic_memory_follows_the_machine() {
        let dir = tempfile::tempdir().unwrap();
        write_scripts(dir.path(), Path::new("/nonexistent/java"), 0, &Start::Jar("server.jar".into())).unwrap();
        let sh = fs::read_to_string(dir.path().join("start.sh")).unwrap();
        assert!(sh.contains("MC_MEMORY=\"${MC_MEMORY:-auto}\""));
        // Swap the Java for one that prints its arguments, and the machine's
        // memory for a fixed size.
        let script = sh.replace("[ -x \"$JAVA\" ] || JAVA=java", "JAVA=echo").replace("$(awk '/^MemTotal:/ { print int($2 / 1024) }' /proc/meminfo 2>/dev/null)", "${FAKE_TOTAL}").replace("/sys/fs/cgroup/memory.max /sys/fs/cgroup/memory/memory.limit_in_bytes", "/nonexistent").replace("$(awk '/^MemAvailable:/ { print int($2 / 1024) }' /proc/meminfo 2>/dev/null)", "${FAKE_FREE}");
        fs::write(dir.path().join("start.sh"), script).unwrap();
        // (total, available, heap, view distance); no MemAvailable goes by the total alone.
        for (total, free, heap, view) in [
            (1024, "", 512, 6),
            (2048, "", 1280, 6),
            (4096, "", 3072, 8),
            (8192, "", 6144, 10),
            (16384, "", 12288, 12),
            (32768, "", 24576, 12),
            // An idle 8 GB host: Java's own room comes off what is free.
            (8192, "7680", 5888, 10),
            // Busy: the heap fits what is left.
            (8192, "3072", 2048, 6),
            (2048, "900", 512, 6),
        ] {
            let _ = fs::remove_file(dir.path().join("server.properties"));
            let output = std::process::Command::new("sh").arg(dir.path().join("start.sh")).env("FAKE_TOTAL", total.to_string()).env("FAKE_FREE", free).env_remove("MC_MEMORY").output().unwrap();
            let text = String::from_utf8_lossy(&output.stdout);
            assert!(text.contains(&format!("-Xmx{heap}M")), "{total}/{free}: {text}");
            assert_eq!(text.contains("G1NewSizePercent=40"), heap > 12288, "{total}/{free}: {text}");
            let properties = fs::read_to_string(dir.path().join("server.properties")).unwrap();
            assert!(properties.contains(&format!("view-distance={view}\n")), "{total}/{free}: {properties}");
        }
        // The host's own choice stays.
        fs::write(dir.path().join("server.properties"), "view-distance=3\n").unwrap();
        let output = std::process::Command::new("sh").arg(dir.path().join("start.sh")).env("FAKE_TOTAL", "4096").env("MC_MEMORY", "2000").output().unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).contains("-Xmx2000M"));
        assert_eq!(fs::read_to_string(dir.path().join("server.properties")).unwrap(), "view-distance=3\n");
    }

    #[test]
    fn a_server_project_sets_its_properties() {
        let mut instance: Instance = serde_json::from_value(serde_json::json!({
            "id": "a", "name": "Plugins", "versionId": "1.21.1", "versionType": "release", "createdAt": 0, "loader": "paper"
        }))
        .unwrap();
        instance.server = Some(crate::core::instances::ServerProject { motd: "Hi".into(), max_players: 8, difficulty: 3, pvp: false, whitelist: true, seed: "42".into(), ..Default::default() });
        let text = properties(&instance, &ServerOptions::default(), &WorldRules::default());
        for line in ["motd=Hi", "max-players=8", "difficulty=3", "gamemode=0", "pvp=false", "white-list=true", "enforce-whitelist=true", "level-seed=42"] {
            assert!(text.lines().any(|l| l == line), "{line} in {text}");
        }
        assert!(!text.contains("allow-flight"), "plugin servers keep the anti-fly check");
        assert_eq!(text.matches("motd=").count(), 1, "{text}");
    }

    #[test]
    fn old_forge_starts_from_its_jar() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["forge-1.12.2-14.23.5.2860-installer.jar", "forge-1.12.2-14.23.5.2860.jar", "minecraft_server.1.12.2.jar"] {
            fs::write(dir.path().join(name), b"").unwrap();
        }
        let Start::Jar(jar) = forge_start(dir.path()).unwrap() else { panic!("expected a jar") };
        assert_eq!(jar, "forge-1.12.2-14.23.5.2860.jar");
    }
}
