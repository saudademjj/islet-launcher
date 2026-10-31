//! EasyTier (https://github.com/EasyTier/EasyTier, LGPL-3.0), run unmodified
//! as a separate process in no-TUN mode: no virtual adapter and no admin
//! rights, only port forwarding through its userspace network stack. The
//! launcher downloads the official release on first use, pinned by version
//! and SHA-256, unless a copy ships next to the launcher.
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::core::download::{DownloadProgress, DownloadTask, download_all, fastest_first};
use crate::core::error::{AppError, AppResult};
use crate::core::i18n::pick;
use crate::core::paths::AppPaths;

pub const VERSION: &str = "2.6.4";

/// Release assets of v2.6.4: platform, zip size, SHA-256.
const RELEASES: &[(&str, u64, &str)] = &[
    ("windows-x86_64", 32_657_720, "27af91e270e554709b048bd32327fefd2dfce5062ae1e8701af7550c6f525f84"),
    ("windows-arm64", 30_136_892, "37023f8a3451c9234b17ee2089a03dc344ce90d803b5b359cb6c46682b0549b4"),
    ("macos-aarch64", 30_174_017, "4be1882d1aa36d31c1d6ba0596f2cf8a097e371f8da124212324b2e0f8df7e4b"),
    ("macos-x86_64", 33_733_545, "89fc28a6e6995259d76ce3f11775220e8a21c760e94df91a6a9db30a69b6982e"),
    ("linux-x86_64", 25_506_523, "61b659eaedba658fa66fe47d17e1426cdd77e5d02fa15fed447bb4357c09dfd6"),
    ("linux-aarch64", 20_487_230, "f533ec25a7ea714e09f645615012200278058525795cc3bb690ff011aec1a70f"),
];

/// GitHub download proxies; the SHA-256 check makes them safe to use. They
/// come and go and their speeds vary wildly (2026-10-31, from a home line in
/// China: ghfast.top 9.7 MB/s, gh-proxy.com 20 KB/s, ghproxy.net down,
/// github.com timing out), so every download first races them all.
const MIRRORS: &[&str] = &[
    "https://ghfast.top/",
    "https://ghproxy.it/",
    "https://gh-proxy.org/",
    "https://gh.catmak.name/",
    "https://ghproxy.net/",
    "https://gh-proxy.com/",
];

/// Below this a source is dropped for the next fastest one.
const MIN_SPEED: u64 = 128 * 1024;

/// Community-run shared nodes (the official public node is gone). They only
/// introduce peers and relay when a direct connection can't be made; room
/// traffic stays encrypted with the room secret. Both sides must share at
/// least one node to find each other.
pub const DEFAULT_NODES: &[&str] = &[
    "tcp://225284.xyz:11010",
    "tcp://easytier.weiai.org.cn:11010",
    "tcp://183.230.36.171:11010",
    "udp://et.basd1.de:11010",
    "wss://et.chinokou.cn",
    "txt://net.qicwken.top",
];

/// An updated node list and extra download sources, published in the
/// project's repository so both can change without a new release.
const REMOTE: &[&str] = &[
    "https://cdn.jsdelivr.net/gh/saudademjj/islet-launcher@main/p2p.json",
    "https://raw.githubusercontent.com/saudademjj/islet-launcher/main/p2p.json",
];
const REMOTE_FRESH: Duration = Duration::from_secs(6 * 3600);

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Remote {
    /// Replaces `DEFAULT_NODES` when not empty.
    #[serde(default)]
    pub nodes: Vec<String>,
    /// Extra places to download EasyTier from, tried alongside GitHub and
    /// its proxies: HTTPS URLs where `{name}` is the archive's file name,
    /// `{version}` the EasyTier version and `{url}` the GitHub download URL.
    /// The pinned SHA-256 still decides what is installed.
    #[serde(default)]
    pub sources: Vec<String>,
}

impl Remote {
    fn clean(self) -> Self {
        Self {
            nodes: self.nodes.into_iter().map(|n| n.trim().to_string()).filter(|n| valid_node(n)).take(32).collect(),
            sources: self
                .sources
                .into_iter()
                .filter(|s| s.starts_with("https://") && s.len() <= 300 && (s.contains("{name}") || s.contains("{url}")) && !s.contains(char::is_whitespace))
                .take(16)
                .collect(),
        }
    }

    fn source(template: &str, name: &str, official: &str) -> String {
        template.replace("{url}", official).replace("{name}", name).replace("{version}", VERSION)
    }
}

/// The published node list and sources: a copy fetched in the last six
/// hours, else a fresh one, else the last copy that was fetched, else none.
pub async fn remote(client: &reqwest::Client, paths: &AppPaths) -> Remote {
    let cache = paths.cache.join("p2p.json");
    let cached = std::fs::read(&cache).ok().and_then(|bytes| serde_json::from_slice::<Remote>(&bytes).ok()).map(Remote::clean);
    let fresh = std::fs::metadata(&cache).and_then(|m| m.modified()).ok().and_then(|at| at.elapsed().ok()).is_some_and(|age| age < REMOTE_FRESH);
    if let Some(cached) = cached.clone().filter(|_| fresh) {
        return cached;
    }
    for url in REMOTE {
        let fetched = async { client.get(*url).timeout(Duration::from_secs(4)).send().await?.error_for_status()?.json::<Remote>().await }.await;
        if let Ok(remote) = fetched {
            let remote = remote.clean();
            if let Ok(bytes) = serde_json::to_vec(&remote) {
                let _ = std::fs::create_dir_all(&paths.cache);
                let _ = std::fs::write(&cache, bytes);
            }
            return remote;
        }
    }
    cached.unwrap_or_default()
}

pub type Progress = Arc<dyn Fn(u64, u64) + Send + Sync>;

pub fn asset_platform() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x86_64",
        ("windows", "aarch64") => "windows-arm64",
        ("macos", "aarch64") => "macos-aarch64",
        ("macos", "x86_64") => "macos-x86_64",
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        _ => return None,
    })
}

#[derive(Debug, Clone)]
pub struct Binaries {
    pub core: PathBuf,
    pub cli: PathBuf,
}

impl Binaries {
    fn in_dir(dir: &Path) -> Self {
        Self { core: dir.join(exe("easytier-core")), cli: dir.join(exe("easytier-cli")) }
    }

    fn present(&self) -> bool {
        self.core.is_file() && self.cli.is_file()
    }
}

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// A copy packaged with the launcher (Tauri puts sidecars next to the exe).
pub fn bundled() -> Option<Binaries> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    Some(Binaries::in_dir(&dir)).filter(Binaries::present)
}

pub fn install_dir(paths: &AppPaths) -> PathBuf {
    paths.root.join("tools").join(format!("easytier-{VERSION}"))
}

pub fn installed(paths: &AppPaths) -> Option<Binaries> {
    bundled().or_else(|| {
        let dir = install_dir(paths);
        Some(Binaries::in_dir(&dir)).filter(|b| dir.join(".complete").is_file() && b.present())
    })
}

/// Size of the download `ensure` would make, or None when nothing is needed.
pub fn download_size(paths: &AppPaths) -> Option<u64> {
    if installed(paths).is_some() {
        return None;
    }
    let platform = asset_platform()?;
    RELEASES.iter().find(|(p, ..)| *p == platform).map(|(_, size, _)| *size)
}

/// Returns the EasyTier binaries, downloading them first if needed from the
/// source that is fastest right now. `prefer_mirror` puts the GitHub proxies
/// before GitHub itself among sources that fail the speed test; `extra` are
/// the published sources (see `Remote::sources`), tried first.
pub async fn ensure(
    client: &reqwest::Client,
    paths: &AppPaths,
    prefer_mirror: bool,
    extra: &[String],
    cancel: &CancellationToken,
    report: Option<Progress>,
) -> AppResult<Binaries> {
    if let Some(binaries) = installed(paths) {
        return Ok(binaries);
    }
    let unsupported = || AppError::new("p2p_platform", pick("联机组件不支持这台电脑的系统或架构。", "P2P multiplayer isn't available for this system or architecture."));
    let platform = asset_platform().ok_or_else(unsupported)?;
    let (_, size, sha256) = RELEASES.iter().find(|(p, ..)| *p == platform).copied().ok_or_else(unsupported)?;
    let name = format!("easytier-{platform}-v{VERSION}.zip");
    let official = format!("https://github.com/EasyTier/EasyTier/releases/download/v{VERSION}/{name}");
    let mut sources: Vec<String> = MIRRORS.iter().map(|m| format!("{m}{official}")).collect();
    if prefer_mirror {
        sources.insert(1, official.clone());
    } else {
        sources.insert(0, official.clone());
    }
    for template in extra.iter().rev() {
        sources.insert(0, Remote::source(template, &name, &official));
    }
    let root = install_dir(paths);
    let archive = root.with_file_name(&name);
    if let Some(report) = &report {
        report(0, size);
    }
    // An archive left by a download whose unpacking failed needs no new source.
    let fetched = std::fs::metadata(&archive).is_ok_and(|meta| meta.len() == size);
    let mut sources = if fetched { sources } else { fastest_first(client, sources, Some(size), cancel).await };
    let url = sources.remove(0);
    let task = DownloadTask::new(pick("联机组件 EasyTier", "EasyTier networking"), url, sources, archive.clone(), None, Some(size))
        .with_sha256(sha256.to_string())
        .with_min_speed(MIN_SPEED);
    let progress = Arc::new(DownloadProgress::new(std::slice::from_ref(&task)));
    let download = download_all(vec![task], client.clone(), 1, cancel.clone(), progress.clone());
    tokio::pin!(download);
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    loop {
        tokio::select! {
            result = &mut download => { result?; break; },
            _ = tick.tick() => if let Some(report) = &report { let p = progress.snapshot(); report(p.bytes_done, p.bytes_total); },
        }
    }
    let stage = root.with_file_name(format!(".install-{}", uuid::Uuid::new_v4()));
    let (source, destination) = (archive.clone(), stage.clone());
    let extracted = tokio::task::spawn_blocking(move || extract(&source, &destination))
        .await
        .map_err(|error| AppError::detail("p2p_extract", pick("解压联机组件失败。", "Couldn't unpack the networking component."), error.to_string()))
        .and_then(|result| result)
        .and_then(|()| Ok(std::fs::write(stage.join(".complete"), b"1")?));
    if let Err(error) = extracted {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(error);
    }
    // Without the marker this is a leftover from an interrupted install.
    if root.exists() {
        let _ = std::fs::remove_dir_all(&root);
    }
    if let Err(error) = std::fs::rename(&stage, &root) {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(error.into());
    }
    let _ = std::fs::remove_file(&archive);
    Ok(Binaries::in_dir(&root))
}

/// Takes only the two programs (and Packet.dll, which easytier-core.exe
/// loads on Windows) by file name, so nothing lands outside `dest`.
fn extract(archive: &Path, dest: &Path) -> AppResult<()> {
    let unreadable = |error: zip::result::ZipError| AppError::detail("p2p_extract", pick("无法读取联机组件压缩包。", "Couldn't read the networking component archive."), error.to_string());
    std::fs::create_dir_all(dest)?;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?).map_err(unreadable)?;
    let programs = [exe("easytier-core"), exe("easytier-cli")];
    let mut found = 0;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(unreadable)?;
        let name = entry.name().map_err(unreadable)?.rsplit(['/', '\\']).next().unwrap_or_default().to_string();
        let program = programs.contains(&name);
        if entry.is_dir() || !(program || name == "Packet.dll") {
            continue;
        }
        let target = dest.join(&name);
        std::io::copy(&mut entry, &mut std::fs::File::create(&target)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
        }
        found += usize::from(program);
    }
    if found < programs.len() {
        return Err(AppError::new("p2p_extract", pick("下载的联机组件不完整。", "The downloaded networking component is incomplete.")));
    }
    Ok(())
}

/// A shared node address as EasyTier takes it, e.g. `tcp://host:11010`.
pub fn valid_node(url: &str) -> bool {
    const SCHEMES: &[&str] = &["tcp", "udp", "ws", "wss", "quic", "wg", "txt", "srv", "http", "https", "faketcp"];
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    SCHEMES.contains(&scheme)
        && !rest.is_empty()
        && url.len() <= 200
        && url.bytes().all(|b| b.is_ascii_alphanumeric() || b".-_:/[]@%?=&~+".contains(&b))
}

/// The nodes to use: the user's list when it has any valid entry, else the
/// published list, else the built-in one.
pub fn nodes(custom: &[String], remote: &Remote) -> Vec<String> {
    let custom: Vec<String> = custom.iter().map(|n| n.trim().to_string()).filter(|n| valid_node(n)).collect();
    if !custom.is_empty() {
        custom
    } else if !remote.nodes.is_empty() {
        remote.nodes.clone()
    } else {
        DEFAULT_NODES.iter().map(|n| n.to_string()).collect()
    }
}

/// The nodes for a Scaffolding room: `nodes` plus the ones Terracotta-based
/// launchers use, so either side can find the other.
pub fn room_nodes(mut nodes: Vec<String>) -> Vec<String> {
    for node in super::scaffolding::NODES {
        if !nodes.iter().any(|n| n == node) {
            nodes.push(node.to_string());
        }
    }
    nodes
}

/// The name other players see: ASCII letters, digits, `_` and `-`.
pub fn hostname(name: &str) -> String {
    let clean: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')).take(32).collect();
    if clean.is_empty() {
        format!("mcl-{}", &uuid::Uuid::new_v4().simple().to_string()[..6])
    } else if clean.starts_with("PublicServer") {
        // Shared nodes use that prefix; don't look like one.
        format!("p-{clean}")
    } else {
        clean
    }
}

/// A port that is free for TCP (and UDP when `udp`) on all interfaces.
pub fn free_port(udp: bool) -> AppResult<u16> {
    for _ in 0..20 {
        let tcp = std::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        let port = tcp.local_addr()?.port();
        if !udp || std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)).is_ok() {
            return Ok(port);
        }
    }
    Err(AppError::new("p2p_port", pick("找不到可用的本地端口。", "No free local port was found.")))
}

pub struct Network<'a> {
    pub name: &'a str,
    pub secret: &'a str,
    pub hostname: &'a str,
    pub nodes: &'a [String],
    pub rpc_port: u16,
    pub listen_port: u16,
    /// This side's virtual IPv4 address; None takes one from EasyTier's
    /// DHCP, which picks a free one in the room's /24.
    pub ip: Option<&'a str>,
}

fn common_args(net: &Network) -> Vec<String> {
    let mut args: Vec<String> = [
        "--no-tun",
        "--network-name", net.name,
        "--network-secret", net.secret,
        "--hostname", net.hostname,
        "--console-log-level", "warn",
        "--latency-first",
        "--multi-thread",
        // Carries TCP over KCP between peers: steadier on lossy UDP paths.
        "--enable-kcp-proxy",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    match net.ip {
        Some(ip) => args.extend(["-i".into(), ip.to_string()]),
        None => args.push("-d".into()),
    }
    args.extend(["-r".into(), format!("127.0.0.1:{}", net.rpc_port)]);
    args.extend(["-l".into(), format!("tcp://0.0.0.0:{}", net.listen_port), format!("udp://0.0.0.0:{}", net.listen_port)]);
    args.push("-p".into());
    args.extend(net.nodes.iter().cloned());
    args
}

/// The host accepts only its game's TCP port from the room.
pub fn host_args(net: &Network, game_port: u16) -> Vec<String> {
    let mut args = common_args(net);
    args.extend(["--tcp-whitelist".into(), game_port.to_string(), "--udp-whitelist".into(), "0".into()]);
    args
}

/// A Scaffolding host accepts its service port and its game from the room.
pub fn scaffolding_host_args(net: &Network, service_port: u16, game_port: u16) -> Vec<String> {
    let mut args = common_args(net);
    args.extend(["--tcp-whitelist".into(), service_port.to_string(), game_port.to_string(), "--udp-whitelist".into(), game_port.to_string()]);
    args
}

/// A Scaffolding guest starts with no forwards: it learns where the host's
/// service is from the room, then adds them with `add_port_forward`.
pub fn scaffolding_guest_args(net: &Network) -> Vec<String> {
    let mut args = common_args(net);
    args.extend(["--tcp-whitelist".into(), "0".into(), "--udp-whitelist".into(), "0".into()]);
    args
}

/// Forwards TCP `bind` on this computer to `destination` in the room, on a
/// running core.
pub async fn add_port_forward(cli: &Path, rpc_port: u16, bind: &str, destination: &str) -> AppResult<()> {
    let failed = |detail: String| AppError::detail("p2p_forward", pick("无法建立到房主的转发。", "Couldn't set up forwarding to the host."), detail);
    let mut command = tokio::process::Command::from(crate::core::platform::quiet_command(cli));
    command.args(["-p", &format!("127.0.0.1:{rpc_port}"), "port-forward", "add", "tcp", bind, destination]).stdin(Stdio::null()).kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(6), command.output())
        .await
        .map_err(|_| failed("timeout".into()))?
        .map_err(|error| failed(error.to_string()))?;
    if output.status.success() { Ok(()) } else { Err(failed(String::from_utf8_lossy(&output.stderr).trim().chars().take(300).collect())) }
}

/// A guest accepts nothing from the room and forwards `local_port` on this
/// computer to the host's game.
pub fn guest_args(net: &Network, local_port: u16, host_ip: &str, game_port: u16) -> Vec<String> {
    let mut args = common_args(net);
    args.extend(["--tcp-whitelist".into(), "0".into(), "--udp-whitelist".into(), "0".into()]);
    // All interfaces: Minecraft joins a LAN world at the announcement's
    // source address, which is this computer's LAN address.
    args.extend(["--port-forward".into(), format!("tcp://0.0.0.0:{local_port}/{host_ip}:{game_port}")]);
    args
}

pub fn spawn(binaries: &Binaries, args: &[String], log: &Path) -> AppResult<Child> {
    let output = std::fs::File::create(log)?;
    let errors = output.try_clone()?;
    let mut command = crate::core::platform::quiet_command(&binaries.core);
    if let Some(dir) = binaries.core.parent() {
        command.current_dir(dir);
    }
    let child = command
        .args(args)
        .stdin(Stdio::null())
        .stdout(output)
        .stderr(errors)
        .spawn()
        .map_err(|error| AppError::detail("p2p_start", pick("无法启动联机组件。", "Couldn't start the networking component."), error.to_string()))?;
    #[cfg(windows)]
    job::adopt(&child);
    Ok(child)
}

/// Runs `easytier-cli -o json <args>` against a running core.
pub async fn query(cli: &Path, rpc_port: u16, args: &[&str]) -> AppResult<serde_json::Value> {
    let failed = |detail: String| AppError::detail("p2p_query", pick("读取联机状态失败。", "Couldn't read the connection status."), detail);
    let mut command = tokio::process::Command::from(crate::core::platform::quiet_command(cli));
    command
        .args(["-p", &format!("127.0.0.1:{rpc_port}"), "-o", "json"])
        .args(args)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(4), command.output())
        .await
        .map_err(|_| failed("timeout".into()))?
        .map_err(|error| failed(error.to_string()))?;
    if !output.status.success() {
        return Err(failed(String::from_utf8_lossy(&output.stderr).trim().chars().take(300).collect()));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| failed(error.to_string()))
}

/// The most telling line of a core's log: its last error, else its last line.
pub fn last_error(log: &Path) -> Option<String> {
    let text = std::fs::read(log).ok()?;
    let text = String::from_utf8_lossy(&text[text.len().saturating_sub(16 * 1024)..]).into_owned();
    let lines: Vec<String> = text.lines().map(strip_ansi).map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    let line = lines.iter().rev().find(|l| l.contains("ERROR") || l.starts_with("Error")).or(lines.last())?;
    // Drop the timestamp and level: "2026-…+08:00 ERROR easytier::x: message".
    let message = match line.split_once(" ERROR ").or_else(|| line.split_once(" WARN ")) {
        Some((_, rest)) => rest.split_once(": ").map_or(rest, |(_, m)| m),
        None => line,
    };
    Some(message.chars().take(240).collect())
}

fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // CSI sequences end with a letter.
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Windows: a kill-on-close job object holds every core, so they end with
/// the launcher even when it crashes.
#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    pub fn adopt(child: &std::process::Child) {
        // Kept open for the launcher's lifetime; the system closes it on exit.
        static JOB: OnceLock<usize> = OnceLock::new();
        let job = *JOB.get_or_init(|| unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return 0;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
            SetInformationJobObject(job, JobObjectExtendedLimitInformation, (&raw const info).cast(), size);
            job as usize
        });
        if job != 0 {
            unsafe { AssignProcessToJobObject(job as _, child.as_raw_handle() as _) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_addresses_are_checked() {
        for good in DEFAULT_NODES {
            assert!(valid_node(good), "{good}");
        }
        for bad in ["", "tcp://", "225284.xyz:11010", "-p tcp://x", "ftp://x", "tcp://a b", "tcp://x;rm"] {
            assert!(!valid_node(bad), "{bad}");
        }
        let none = Remote::default();
        assert_eq!(nodes(&[" udp://my.node:1 ".into(), "junk".into()], &none), vec!["udp://my.node:1".to_string()]);
        assert_eq!(nodes(&["junk".into()], &none).len(), DEFAULT_NODES.len());
        let published = Remote { nodes: vec!["tcp://new.node:1".into(), "junk".into()], sources: vec!["https://cdn.example/et/{name}".into(), "http://plain/{name}".into(), "https://no-placeholder/".into()] }.clean();
        assert_eq!(nodes(&[], &published), vec!["tcp://new.node:1".to_string()]);
        assert_eq!(published.sources, vec!["https://cdn.example/et/{name}".to_string()]);
        assert_eq!(Remote::source(&published.sources[0], "easytier-x.zip", "https://github.com/x"), "https://cdn.example/et/easytier-x.zip");
        let room = room_nodes(vec!["tcp://public.easytier.top:11010".into()]);
        assert_eq!(room.len(), crate::core::p2p::scaffolding::NODES.len());
        assert!(room.iter().all(|n| valid_node(n)));
    }

    #[test]
    fn hostnames_are_plain() {
        assert_eq!(hostname("Steve_01"), "Steve_01");
        assert_eq!(hostname("史蒂夫 Steve!"), "Steve");
        assert!(hostname("史蒂夫").starts_with("mcl-"));
        assert_eq!(hostname("PublicServer_x"), "p-PublicServer_x");
    }

    #[test]
    fn arguments_lock_down_each_side() {
        let nodes = vec!["tcp://a:1".to_string(), "udp://b:2".to_string()];
        let net = Network { name: "mcl-x", secret: "s", hostname: "h", nodes: &nodes, rpc_port: 15000, listen_port: 31000, ip: Some("10.126.126.1") };
        let host = host_args(&net, 25565).join(" ");
        assert!(host.contains("--no-tun"));
        assert!(host.contains("-i 10.126.126.1"));
        assert!(host.contains("-r 127.0.0.1:15000"));
        assert!(host.contains("-p tcp://a:1 udp://b:2"));
        assert!(host.contains("--tcp-whitelist 25565 --udp-whitelist 0"));
        let guest = guest_args(&Network { ip: None, ..net }, 40000, "10.126.126.1", 25565).join(" ");
        assert!(guest.contains(" -d "));
        assert!(guest.contains("--tcp-whitelist 0 --udp-whitelist 0"));
        assert!(guest.contains("--port-forward tcp://0.0.0.0:40000/10.126.126.1:25565"));
        let host = scaffolding_host_args(&net, 31234, 25565).join(" ");
        assert!(host.contains("--tcp-whitelist 31234 25565 --udp-whitelist 25565"));
        assert!(!scaffolding_guest_args(&net).join(" ").contains("--port-forward"));
    }

    #[test]
    fn last_error_finds_the_message() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("core.log");
        std::fs::write(
            &log,
            "\u{1b}[2m2026-10-29T23:18:47+08:00\u{1b}[0m ERROR easytier::gateway::socks5: port forward: failed to connect\n2026-10-29T23:18:48+08:00  WARN easytier::connector: reconnecting\n\n",
        )
        .unwrap();
        assert_eq!(last_error(&log).as_deref(), Some("port forward: failed to connect"));
        std::fs::write(&log, "Error: invalid network secret\n").unwrap();
        assert_eq!(last_error(&log).as_deref(), Some("Error: invalid network secret"));
        assert_eq!(last_error(&dir.path().join("missing.log")), None);
    }
}
