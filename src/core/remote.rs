//! A server on a Linux computer of the player's own, usually a rented cloud
//! host, reached over SSH. Islet uploads the server it builds there, keeps
//! it running with a small control script (`islet.sh`), backs the world up
//! and brings mod changes over. Nothing on the host needs root, and nothing
//! goes through anyone else's servers.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{ChannelMsg, client};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::core::error::{AppError, AppResult};
use crate::core::i18n::pick;
use crate::core::paths::AppPaths;

/// The control script, uploaded beside the server.
pub const SCRIPT: &str = include_str!("remote/islet.sh");

/// Where a world's server lives and how it is kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub user: String,
    /// "password" or "key".
    pub auth: String,
    pub key_path: Option<String>,
    /// The server's folder, under the user's home unless absolute.
    pub dir: String,
    pub game_port: u16,
    /// Where friends join; the host and game port when empty.
    pub address: String,
    /// The host key seen on the first connection ("SHA256:…"). A different
    /// key later stops every connection.
    pub fingerprint: Option<String>,
    /// Start the server when the host boots.
    pub autostart: bool,
    pub daily_backup: bool,
    pub keep_backups: u32,
    pub memory_mb: u32,
    pub online_mode: bool,
    /// When the server was last uploaded, in milliseconds.
    pub deployed_at: Option<i64>,
}

impl Default for Target {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 22,
            user: "root".into(),
            auth: "password".into(),
            key_path: None,
            dir: String::new(),
            game_port: 25565,
            address: String::new(),
            fingerprint: None,
            autostart: true,
            daily_backup: true,
            keep_backups: 7,
            memory_mb: 4096,
            online_mode: true,
            deployed_at: None,
        }
    }
}

impl Target {
    pub fn public_address(&self) -> String {
        if !self.address.trim().is_empty() {
            return self.address.trim().to_string();
        }
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        if self.game_port == 25565 { host } else { format!("{host}:{}", self.game_port) }
    }

    /// The fields a person typed, checked before anything connects.
    pub fn validate(&self) -> AppResult<()> {
        let bad = |zh: &'static str, en: &'static str| Err(AppError::new("remote_target", pick(zh, en)));
        if self.host.trim().is_empty() || self.host.chars().any(|c| c.is_whitespace() || c == '/' || c == '@') {
            return bad("请填写服务器的 IP 或域名。", "Enter the server's IP address or domain.");
        }
        if self.user.trim().is_empty() || !self.user.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
            return bad("用户名只能有字母、数字和 . _ -。", "The user name may only have letters, digits and . _ -.");
        }
        if self.port == 0 || self.game_port == 0 {
            return bad("端口要在 1 到 65535 之间。", "Ports go from 1 to 65535.");
        }
        safe_dir(&self.dir)?;
        if self.auth == "key" && self.key_path.as_deref().is_none_or(|p| p.trim().is_empty()) {
            return bad("请选择私钥文件。", "Choose the private key file.");
        }
        if self.memory_mb != 0 && !(512..=65536).contains(&self.memory_mb) {
            return bad("内存要在 512 到 65536 MB 之间，或者选自动。", "Memory goes from 512 to 65536 MB, or automatic.");
        }
        Ok(())
    }
}

/// Folders the control script can live in: no spaces, quotes or `..`, so
/// they can stand unquoted in cron and systemd lines.
fn safe_dir(dir: &str) -> AppResult<()> {
    let ok = !dir.is_empty()
        && dir.len() <= 200
        && dir.chars().all(|c| c.is_ascii_alphanumeric() || "._-/".contains(c))
        && !dir.split('/').any(|part| part == "..")
        && !dir.trim_end_matches('/').is_empty()
        && !dir.starts_with('-');
    if ok {
        Ok(())
    } else {
        Err(AppError::new("remote_target", pick("服务器目录只能用字母、数字和 . _ - /，例如 islet/survival。", "The server folder may only use letters, digits and . _ - /, e.g. islet/survival.")))
    }
}

/// A folder name for a world on the host, from its name when that is ASCII.
pub fn default_dir(name: &str, id: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug = if slug.len() >= 3 { slug.chars().take(32).collect() } else { format!("world-{}", id.chars().filter(char::is_ascii_alphanumeric).take(8).collect::<String>()) };
    format!("islet/{slug}")
}

fn target_file(paths: &AppPaths, id: &str) -> AppResult<PathBuf> {
    AppPaths::validate_instance_id(id)?;
    Ok(paths.instance_dir(id).join("remote.json"))
}

pub fn load(paths: &AppPaths, id: &str) -> Option<Target> {
    serde_json::from_slice(&std::fs::read(target_file(paths, id).ok()?).ok()?).ok()
}

pub fn save(paths: &AppPaths, id: &str, target: &Target) -> AppResult<()> {
    crate::core::util::atomic_write(&target_file(paths, id)?, &serde_json::to_vec_pretty(target)?)
}

pub fn forget(paths: &AppPaths, id: &str) -> AppResult<()> {
    match std::fs::remove_file(target_file(paths, id)?) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

/// `'text'` for a POSIX shell (and bash, zsh and fish, which read it alike).
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn ssh_error(zh: &'static str, en: &'static str, detail: impl ToString) -> AppError {
    AppError::detail("remote", pick(zh, en), detail.to_string())
}

/// Checks the host key: any key the first time (it is then remembered),
/// only that key afterwards.
struct Handler {
    expected: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
}

impl client::Handler for Handler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let fingerprint = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.fingerprint(HashAlg::Sha256).to_string(),
            PublicKeyOrCertificate::Certificate(certificate) => certificate.public_key().fingerprint(HashAlg::Sha256).to_string(),
        };
        let trusted = self.expected.as_ref().is_none_or(|expected| *expected == fingerprint);
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = Some(fingerprint);
        Ok(trusted)
    }
}

/// What a command printed and how it ended.
#[derive(Debug, Default)]
pub struct Output {
    pub status: Option<u32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.status == Some(0)
    }
}

/// One SSH connection; every command runs in a channel of its own.
pub struct Session {
    handle: client::Handle<Handler>,
    /// The host key, for the target to remember.
    pub fingerprint: String,
}

/// What to feed a command, and what to tell about the bytes sent so far.
type Input<'a> = (&'a mut (dyn tokio::io::AsyncRead + Unpin + Send), &'a (dyn Fn(u64) + Sync));

impl Session {
    pub async fn connect(target: &Target, secret: Option<&str>) -> AppResult<Self> {
        target.validate()?;
        let config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(120)),
            keepalive_interval: Some(Duration::from_secs(20)),
            nodelay: true,
            ..Default::default()
        });
        let seen = Arc::new(Mutex::new(None));
        let handler = Handler { expected: target.fingerprint.clone(), seen: seen.clone() };
        let address = (target.host.trim().trim_start_matches('[').trim_end_matches(']').to_string(), target.port);
        let connected = tokio::time::timeout(Duration::from_secs(20), client::connect(config, address, handler)).await;
        let seen = seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut handle = match connected {
            Err(_) => return Err(ssh_error("连不上服务器的 SSH 端口，检查地址、端口和云服务器的安全组。", "Couldn't reach the server's SSH port; check the address, port and the cloud firewall.", "timed out")),
            Ok(Ok(handle)) => handle,
            Ok(Err(error)) => {
                if let (Some(expected), Some(seen)) = (&target.fingerprint, &seen)
                    && expected != seen
                {
                    return Err(ssh_error(
                        "服务器的身份指纹和上次不一样，为了安全已停止连接。如果你重装过系统，请在「编辑连接」里清除指纹后再连。",
                        "The server's key fingerprint has changed since last time, so Islet stopped. If you reinstalled the system, clear the fingerprint under Edit connection.",
                        format!("expected {expected}, got {seen}"),
                    ));
                }
                return Err(ssh_error("连不上服务器的 SSH，检查地址和端口。", "Couldn't connect over SSH; check the address and port.", error));
            }
        };
        let user = target.user.trim().to_string();
        let accepted = if target.auth == "key" {
            let path = target.key_path.clone().unwrap_or_default();
            let key = russh::keys::load_secret_key(&path, secret.filter(|s| !s.is_empty())).map_err(|error| {
                ssh_error("读不了私钥文件；如果它有口令，请填在密码框里。", "Couldn't read the private key; if it has a passphrase, enter it in the password field.", error)
            })?;
            let hash = handle.best_supported_rsa_hash().await.map_err(|e| ssh_error("SSH 握手失败。", "The SSH handshake failed.", e))?.flatten();
            handle.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash)).await
        } else {
            handle.authenticate_password(user, secret.unwrap_or_default()).await
        }
        .map_err(|error| ssh_error("SSH 登录失败。", "SSH sign-in failed.", error))?;
        if !accepted.success() {
            return Err(ssh_error("用户名、密码或密钥不对，服务器拒绝了登录。", "The server refused the user name, password or key.", format!("{accepted:?}")));
        }
        Ok(Self { handle, fingerprint: seen.unwrap_or_default() })
    }

    /// Runs `script` with `sh` in the user's home, feeding it `input`.
    async fn exec(&self, script: &str, input: Option<Input<'_>>, mut sink: Option<&mut tokio::fs::File>) -> AppResult<Output> {
        let mut channel = self.handle.channel_open_session().await.map_err(|e| ssh_error("SSH 连接断开了。", "The SSH connection dropped.", e))?;
        channel.exec(true, format!("sh -c {}", quote(script))).await.map_err(|e| ssh_error("无法在服务器上运行命令。", "Couldn't run a command on the server.", e))?;
        if let Some((reader, progress)) = input {
            let mut buffer = vec![0u8; 256 * 1024];
            let mut sent = 0u64;
            // A few reports a second are enough for a progress bar.
            let mut reported = std::time::Instant::now();
            loop {
                let read = reader.read(&mut buffer).await?;
                if read == 0 {
                    progress(sent);
                    break;
                }
                channel.data_bytes(buffer[..read].to_vec()).await.map_err(|e| ssh_error("上传中断了。", "The upload was interrupted.", e))?;
                sent += read as u64;
                if reported.elapsed() >= Duration::from_millis(250) {
                    reported = std::time::Instant::now();
                    progress(sent);
                }
            }
            channel.eof().await.map_err(|e| ssh_error("上传中断了。", "The upload was interrupted.", e))?;
        }
        let mut output = Output::default();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Data { data } => match sink.as_mut() {
                    Some(file) => file.write_all(&data).await?,
                    None => stdout.extend_from_slice(&data),
                },
                ChannelMsg::ExtendedData { data, .. } => stderr.extend_from_slice(&data),
                ChannelMsg::ExitStatus { exit_status } => output.status = Some(exit_status),
                _ => {}
            }
        }
        output.stdout = String::from_utf8_lossy(&stdout).into_owned();
        output.stderr = String::from_utf8_lossy(&stderr).into_owned();
        Ok(output)
    }

    pub async fn run(&self, script: &str) -> AppResult<Output> {
        self.exec(script, None, None).await
    }

    /// Runs `script`, failing with its error output unless it succeeds.
    pub async fn run_ok(&self, script: &str, zh: &'static str, en: &'static str) -> AppResult<String> {
        let output = self.run(script).await?;
        if !output.ok() {
            let detail = if output.stderr.trim().is_empty() { &output.stdout } else { &output.stderr };
            return Err(ssh_error(zh, en, tail(detail, 12)));
        }
        Ok(output.stdout)
    }

    /// Streams a local file into `script`'s input.
    pub async fn upload(&self, script: &str, file: &Path, progress: &(dyn Fn(u64) + Sync)) -> AppResult<Output> {
        let mut reader = tokio::fs::File::open(file).await?;
        self.exec(script, Some((&mut reader, progress)), None).await
    }

    /// Writes what `script` prints to a local file.
    pub async fn download(&self, script: &str, file: &Path) -> AppResult<Output> {
        let mut writer = tokio::fs::File::create(file).await?;
        let output = self.exec(script, None, Some(&mut writer)).await?;
        writer.flush().await?;
        Ok(output)
    }

    pub async fn close(self) {
        let _ = self.handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    }
}

fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// `cd` into the server folder, from the home folder.
pub fn cd(dir: &str) -> String {
    format!("cd \"$HOME\" && cd {} ", quote(dir))
}

/// What the host is and has, from one round trip.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub os: String,
    pub arch: String,
    pub root: bool,
    pub tools: Vec<String>,
    /// Java's major version: the one in the server folder, else the system's.
    pub java: Option<(String, u32)>,
    /// The control script is already there.
    pub deployed: bool,
    pub free_mb: Option<u64>,
}

impl Probe {
    pub fn has(&self, tool: &str) -> bool {
        self.tools.iter().any(|t| t == tool)
    }
}

pub fn probe_script(dir: &str) -> String {
    format!(
        r#"cd "$HOME"; D={dir}
echo "os=$(uname -s)"; echo "arch=$(uname -m)"; echo "uid=$(id -u)"
for c in tar curl wget sha256sum setsid mkfifo crontab systemctl; do command -v $c >/dev/null 2>&1 && echo "has=$c"; done
for j in "$D/java/bin/java" java; do v=$("$j" -version 2>&1 | head -n 1) && echo "java=$j|$v"; done
[ -f "$D/islet.sh" ] && echo "deployed=1"
p="$D"; while [ ! -d "$p" ]; do p=$(dirname "$p"); done; echo "free=$(df -Pk "$p" | awk 'NR==2 {{print $4}}')"
true"#,
        dir = quote(dir)
    )
}

/// Java's major version from the first line of `java -version`.
pub fn java_major(line: &str) -> Option<u32> {
    let version = line.split('"').nth(1)?;
    let mut parts = version.split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 { parts.next()?.parse().ok() } else { Some(first) }
}

pub fn parse_probe(text: &str) -> Probe {
    let mut probe = Probe::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        match key {
            "os" => probe.os = value.trim().to_string(),
            "arch" => probe.arch = crate::core::platform::normalize_arch(value.trim()).to_string(),
            "uid" => probe.root = value.trim() == "0",
            "has" => probe.tools.push(value.trim().to_string()),
            "java" if probe.java.is_none() => {
                if let Some((path, version)) = value.split_once('|') {
                    probe.java = java_major(version).map(|major| (path.to_string(), major));
                }
            }
            "deployed" => probe.deployed = true,
            "free" => probe.free_mb = value.trim().parse::<u64>().ok().map(|kb| kb / 1024),
            _ => {}
        }
    }
    probe
}

/// The server's state, as `islet.sh status` reports it.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub running: bool,
    /// The log says the server finished starting.
    pub ready: bool,
    /// Backups on the host, newest first: (file, bytes).
    pub backups: Vec<(String, u64)>,
    pub scheduled: String,
}

pub fn parse_state(text: &str) -> State {
    let mut state = State::default();
    for line in text.lines() {
        match line.split_once('=') {
            Some(("state", value)) => state.running = value.trim() == "running",
            Some(("ready", value)) => state.ready = value.trim() == "1",
            Some(("backup", value)) => {
                if let Some((size, name)) = value.trim().split_once(' ') {
                    state.backups.push((name.to_string(), size.parse().unwrap_or(0)));
                }
            }
            Some(("schedule", value)) => state.scheduled = value.trim().to_string(),
            _ => {}
        }
    }
    state
}

/// The server folder as a gzipped tar, without the files that must survive
/// an update on the host when `update` is set, and without packs for
/// friends.
pub fn pack(source: &Path, archive: &Path, update: bool) -> AppResult<u64> {
    let keep_remote = ["world", "server.properties", "ops.json", "whitelist.json", "banned-players.json", "banned-ips.json", "usercache.json"];
    let file = std::fs::File::create(archive)?;
    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(file, flate2::Compression::fast()));
    tar.follow_symlinks(false);
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".mrpack") || (update && keep_remote.contains(&name.as_str())) {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            tar.append_dir_all(&name, &path)?;
        } else {
            tar.append_path_with_name(&path, &name)?;
        }
    }
    tar.into_inner()?.finish()?.sync_all()?;
    Ok(std::fs::metadata(archive)?.len())
}

/// The shell lines that unpack an upload into the server folder. An update
/// replaces the mods and plugins outright, so removed ones go too.
pub fn unpack_script(dir: &str, update: bool) -> String {
    let clear = if update { "rm -rf mods && rm -f plugins/*.jar && " } else { "" };
    format!("mkdir -p {d} && cd \"$HOME\" && cd {d} && {clear}tar -xzf - && chmod +x start.sh islet.sh", d = quote(dir))
}

/// Settings the launcher manages in `server.properties` and beside it.
pub fn configure_script(target: &Target) -> String {
    format!(
        "{cd}&& sh islet.sh set-prop server-port {port} && sh islet.sh set-prop online-mode {online} && echo {keep} > .islet/keep && sh islet.sh memory {memory} && sh islet.sh schedule {autostart} {backup}",
        cd = cd(&target.dir),
        port = target.game_port,
        online = target.online_mode,
        keep = target.keep_backups.clamp(1, 100),
        memory = target.memory_mb,
        autostart = u8::from(target.autostart),
        backup = u8::from(target.daily_backup),
    )
}

/// The newest backup's name, checked to be one the script wrote.
pub fn backup_name(name: &str) -> AppResult<&str> {
    let ok = name.starts_with("backups/world-") && name.ends_with(".tar.gz") && name.len() < 80 && name["backups/".len()..].chars().all(|c| c.is_ascii_alphanumeric() || "-._".contains(c));
    if ok { Ok(name) } else { Err(AppError::new("remote", pick("备份文件名不对。", "That isn't a backup file."))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_survives_quotes() {
        assert_eq!(quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn folders_are_checked() {
        for good in ["islet/survival", "/srv/mc", "mc-1.21_a"] {
            assert!(safe_dir(good).is_ok(), "{good}");
        }
        for bad in ["", "a b", "../x", "x/../y", "a'b", "-rf", "/", "$(id)"] {
            assert!(safe_dir(bad).is_err(), "{bad}");
        }
        assert_eq!(default_dir("My Survival!", "abc"), "islet/my-survival");
        assert_eq!(default_dir("生存", "0f3a9b2c-11"), "islet/world-0f3a9b2c");
    }

    #[test]
    fn addresses_for_friends() {
        let mut target = Target { host: "1.2.3.4".into(), ..Default::default() };
        assert_eq!(target.public_address(), "1.2.3.4");
        target.game_port = 25570;
        assert_eq!(target.public_address(), "1.2.3.4:25570");
        target.host = "2001:db8::1".into();
        assert_eq!(target.public_address(), "[2001:db8::1]:25570");
        target.address = "mc.example.com".into();
        assert_eq!(target.public_address(), "mc.example.com");
    }

    #[test]
    fn java_versions_are_read() {
        assert_eq!(java_major(r#"openjdk version "21.0.4" 2024-07-16"#), Some(21));
        assert_eq!(java_major(r#"java version "1.8.0_402""#), Some(8));
        assert_eq!(java_major(r#"openjdk version "17" 2021-09-14"#), Some(17));
        assert_eq!(java_major("bash: java: command not found"), None);
    }

    #[test]
    fn probe_and_state_are_parsed() {
        let probe = parse_probe("os=Linux\narch=x86_64\nuid=1000\nhas=tar\nhas=curl\njava=java|openjdk version \"21.0.4\"\nfree=10485760\n");
        assert_eq!(probe.os, "Linux");
        assert!(!probe.root && probe.has("curl") && !probe.has("wget"));
        assert_eq!(probe.java, Some(("java".into(), 21)));
        assert_eq!(probe.free_mb, Some(10240));
        let state = parse_state("state=running\nready=1\nbackup=2048 backups/world-20261101-051700.tar.gz\nschedule=cron\n");
        assert!(state.running && state.ready);
        assert_eq!(state.backups, [("backups/world-20261101-051700.tar.gz".to_string(), 2048)]);
        assert!(backup_name("backups/world-20261101-051700.tar.gz").is_ok());
        assert!(backup_name("backups/../../etc/passwd").is_err());
    }

    #[test]
    fn updates_keep_the_world_and_drop_friend_packs() {
        let dir = tempfile::tempdir().unwrap();
        let server = dir.path().join("server");
        for file in ["mods/a.jar", "world/level.dat", "server.properties", "start.sh", "生存-给朋友.mrpack"] {
            let path = server.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        }
        let names = |update: bool| {
            let archive = dir.path().join("a.tar.gz");
            pack(&server, &archive, update).unwrap();
            let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(&archive).unwrap()));
            let mut names: Vec<String> = tar.entries().unwrap().map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned()).filter(|n| !n.ends_with('/')).collect();
            names.sort();
            names
        };
        assert_eq!(names(false), ["mods/a.jar", "server.properties", "start.sh", "world/level.dat"]);
        assert_eq!(names(true), ["mods/a.jar", "start.sh"]);
    }
}
