//! A world as a dedicated server: the official server for its game and
//! loader, the mods a server runs (Modrinth says which are client-only),
//! its config folders and start scripts, in a folder ready to run here or
//! to upload to a host.
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
use crate::core::instances::Instance;
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
        "vanilla" => String::new(),
        "fabric" | "quilt" | "forge" | "neoforge" => instance.loader_version.clone().ok_or_else(|| {
            AppError::new("server_pack", pick("先安装一次这个世界，确定加载器版本后再导出服务端。", "Install this world once so its loader version is fixed, then export the server."))
        })?,
        _ => {
            return Err(AppError::new(
                "server_pack",
                pick("这个加载器没有可以一键生成的服务端；原版、Fabric、Quilt、Forge 和 NeoForge 可以。", "This loader has no server Islet can set up; vanilla, Fabric, Quilt, Forge and NeoForge do."),
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

/// Mods the server runs, and the config folders.
async fn copy_content(client: &reqwest::Client, paths: &AppPaths, instance: &Instance, destination: &Path) -> AppResult<Summary> {
    let game = paths.instance_game_dir(&instance.id);
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
        fs::create_dir_all(destination.join("mods"))?;
        fs::copy(path, destination.join("mods").join(name))?;
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

/// `start.sh` and `start.bat`: the world's Java unless `JAVA` says
/// otherwise or it isn't there (on another computer), then `java`.
fn write_scripts(destination: &Path, java: &Path, memory_mb: u32, start: &Start) -> AppResult<()> {
    let java = console_java(java);
    let memory = format!("-Xms{0}M -Xmx{0}M", memory_mb.clamp(512, 32768));
    let (unix, windows) = match start {
        Start::Jar(jar) => (format!("\"$JAVA\" {memory} -jar {jar} nogui"), format!("\"%JAVA%\" {memory} -jar {jar} nogui")),
        Start::ArgFiles { unix, windows } => {
            // The installer's own run scripts take the memory from here.
            fs::write(destination.join("user_jvm_args.txt"), format!("# Memory for the server; edit as needed.\n{memory}\n"))?;
            (format!("\"$JAVA\" @user_jvm_args.txt {unix} nogui"), format!("\"%JAVA%\" @user_jvm_args.txt {windows} nogui"))
        }
    };
    let sh = format!(
        "#!/usr/bin/env sh\n# Starts the server. Set JAVA to use another Java.\ncd \"$(dirname \"$0\")\" || exit 1\nJAVA=\"${{JAVA:-{java}}}\"\n[ -x \"$JAVA\" ] || JAVA=java\n{unix}\n",
        java = java.display().to_string().replace('"', "\\\""),
    );
    let bat = format!(
        "@echo off\r\nrem Starts the server. Set JAVA to use another Java.\r\ncd /d \"%~dp0\"\r\nif not defined JAVA set \"JAVA={java}\"\r\nif not exist \"%JAVA%\" set \"JAVA=java\"\r\n{windows}\r\npause\r\n",
        java = java.display(),
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
    if rules.hardcore {
        lines.push("hardcore=true".into());
    }
    // Mods with jetpacks, wings and the like get players kicked for flying.
    if instance.loader != "vanilla" {
        lines.push("allow-flight=true".into());
    }
    lines.join("\n") + "\n"
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
        assert!(sh.contains("\"$JAVA\" @user_jvm_args.txt @libraries/net/neoforged/neoforge/21.1.1/unix_args.txt nogui"));
        assert!(fs::read_to_string(dir.path().join("user_jvm_args.txt")).unwrap().contains("-Xmx4096M"));
        assert!(fs::read_to_string(dir.path().join("start.bat")).unwrap().contains("win_args.txt nogui"));
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
