//! P2P rooms: one EasyTier core at a time, either hosting a world opened to
//! LAN or, as a guest, reaching one through a port on this computer. Each
//! status poll reads the core's peers and pings the game to grade health.
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use islet::core::error::{AppError, ErrorPayload};
use islet::core::i18n::pick;
use islet::core::p2p::health::{self, Role};
use islet::core::p2p::room::{self, Room};
use islet::core::p2p::{easytier, lan};
use serde::Serialize;
use tauri::{Manager, State};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub struct P2p {
    session: Mutex<Option<Session>>,
    /// Held while a host or join call sets up, so two can't race.
    busy: AtomicBool,
    download: Arc<Mutex<Option<Download>>>,
    cancel: Mutex<Option<CancellationToken>>,
}

impl P2p {
    pub fn is_preparing(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    pub fn has_activity(&self) -> bool {
        self.is_preparing() || self.session.try_lock().map_or(true, |session| session.is_some())
    }

    /// Leaves the room (and stops a component download); also called when
    /// the launcher exits, since managed state isn't dropped then.
    pub fn stop_all(&self) {
        if let Some(cancel) = lock(&self.cancel).take() {
            cancel.cancel();
        }
        let session = lock(&self.session).take();
        drop(session);
    }
}

struct Session {
    role: Role,
    code: String,
    hostname: String,
    child: Child,
    cli: PathBuf,
    rpc_port: u16,
    started: Instant,
    log: PathBuf,
    game_port: u16,
    /// Guest: the port on this computer that leads to the host's game.
    local_port: Option<u16>,
    /// What the guest's LAN announcement calls the world.
    motd: Arc<Mutex<String>>,
    announcer: Option<tauri::async_runtime::JoinHandle<()>>,
    /// Set once the core has exited, with what its log said.
    exited: Option<String>,
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(task) = self.announcer.take() {
            task.abort();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    done: u64,
    total: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct World {
    motd: String,
    online: i64,
    max: i64,
    version: String,
    latency_ms: u32,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    active: bool,
    /// A host or join call is still setting up.
    preparing: bool,
    download: Option<Download>,
    role: Option<Role>,
    code: Option<String>,
    hostname: String,
    game_port: u16,
    /// Guest: where the game joins on this computer.
    address: Option<String>,
    uptime_seconds: u64,
    health: Option<health::Report>,
    world: Option<World>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Defaults {
    nodes: Vec<&'static str>,
    /// Bytes still to download before the first room, if any.
    download_size: Option<u64>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn fail(text: &str) -> ErrorPayload {
    AppError::new("p2p", text).to_payload()
}

fn payload(error: AppError) -> ErrorPayload {
    error.to_payload()
}

struct Busy<'a>(&'a AtomicBool);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Claims the setup slot and checks that no room is open.
fn begin(state: &P2p) -> Result<Busy<'_>, ErrorPayload> {
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err(fail(pick("正在准备房间，请稍候。", "A room is being set up; please wait.")));
    }
    let busy = Busy(&state.busy);
    let mut session = lock(&state.session);
    if let Some(open) = session.as_mut() {
        if open.exited.is_none() && matches!(open.child.try_wait(), Ok(None)) {
            return Err(fail(pick("已经在房间里了，请先退出当前房间。", "You're already in a room. Leave it first.")));
        }
        *session = None;
    }
    Ok(busy)
}

/// The EasyTier programs, downloading them with progress on first use.
async fn binaries(app: &tauri::AppHandle, state: &P2p) -> Result<easytier::Binaries, ErrorPayload> {
    let services = app.state::<crate::LauncherState>().services();
    let cancel = CancellationToken::new();
    *lock(&state.cancel) = Some(cancel.clone());
    let download = state.download.clone();
    let report: easytier::Progress = Arc::new(move |done, total| *lock(&download) = Some(Download { done, total }));
    let result = services.ensure_easytier(&cancel, Some(report)).await;
    *lock(&state.download) = None;
    *lock(&state.cancel) = None;
    result.map_err(payload)
}

fn open(app: &tauri::AppHandle, role: Role, room: &Room, name: &str, nodes: &[String], binaries: easytier::Binaries) -> Result<Session, ErrorPayload> {
    let services = app.state::<crate::LauncherState>().services();
    let mut taken = Vec::new();
    let mut port = |udp: bool| -> Result<u16, ErrorPayload> {
        for _ in 0..10 {
            let port = easytier::free_port(udp).map_err(payload)?;
            if !taken.contains(&port) {
                taken.push(port);
                return Ok(port);
            }
        }
        Err(fail(pick("找不到可用的本地端口。", "No free local port was found.")))
    };
    let (rpc_port, listen_port) = (port(false)?, port(true)?);
    let local_port = if role == Role::Guest { Some(port(false)?) } else { None };
    let hostname = easytier::hostname(name);
    let nodes = easytier::nodes(nodes);
    let (network, secret) = (room.network_name(), room.network_secret());
    let net = easytier::Network {
        name: &network,
        secret: &secret,
        hostname: &hostname,
        nodes: &nodes,
        rpc_port,
        listen_port,
        ip: (role == Role::Host).then_some(room::HOST_IP),
    };
    let args = match local_port {
        Some(local) => easytier::guest_args(&net, local, room::HOST_IP, room.game_port),
        None => easytier::host_args(&net, room.game_port),
    };
    let log = services.paths.logs.join(if role == Role::Host { "easytier-host.log" } else { "easytier-guest.log" });
    let child = easytier::spawn(&binaries, &args, &log).map_err(payload)?;
    let code = room.code();
    let motd = Arc::new(Mutex::new(islet::tr!("房间 {}", "Room {}", &code[..7])));
    let announcer = local_port.map(|local| tauri::async_runtime::spawn(lan::announce(motd.clone(), local)));
    Ok(Session {
        role,
        code,
        hostname,
        child,
        cli: binaries.cli,
        rpc_port,
        started: Instant::now(),
        log,
        game_port: room.game_port,
        local_port,
        motd,
        announcer,
        exited: None,
    })
}

#[tauri::command]
pub fn p2p_defaults(app: tauri::AppHandle) -> Defaults {
    let services = app.state::<crate::LauncherState>().services();
    Defaults { nodes: easytier::DEFAULT_NODES.to_vec(), download_size: easytier::download_size(&services.paths) }
}

/// Games opened to LAN on this computer.
#[tauri::command]
pub async fn p2p_detect_lan() -> Result<Vec<lan::LanGame>, ErrorPayload> {
    lan::detect(Duration::from_millis(3200)).await.map_err(payload)
}

#[tauri::command]
pub async fn p2p_host(app: tauri::AppHandle, state: State<'_, P2p>, port: u16, name: String, nodes: Vec<String>) -> Result<(), ErrorPayload> {
    let state = state.inner();
    if port == 0 {
        return Err(fail(pick("请填写游戏显示的局域网端口。", "Enter the LAN port the game shows.")));
    }
    let _busy = begin(state)?;
    if !lan::listening(port).await {
        return Err(fail(&islet::tr!(
            "本机端口 {port} 上没有游戏：先在游戏里“对局域网开放”，再填它显示的端口。",
            "No game on port {port}. Open the world to LAN in the game first, then enter the port it shows."
        )));
    }
    let binaries = binaries(&app, state).await?;
    let session = open(&app, Role::Host, &Room::new(port), &name, &nodes, binaries)?;
    *lock(&state.session) = Some(session);
    Ok(())
}

#[tauri::command]
pub async fn p2p_join(app: tauri::AppHandle, state: State<'_, P2p>, code: String, name: String, nodes: Vec<String>) -> Result<(), ErrorPayload> {
    let state = state.inner();
    let room = Room::parse(&code).map_err(payload)?;
    let _busy = begin(state)?;
    let binaries = binaries(&app, state).await?;
    let session = open(&app, Role::Guest, &room, &name, &nodes, binaries)?;
    *lock(&state.session) = Some(session);
    Ok(())
}

/// What a status poll needs from the session, copied out so the lock isn't
/// held while the core and the game are queried.
struct Probe {
    role: Role,
    code: String,
    hostname: String,
    cli: PathBuf,
    rpc_port: u16,
    started: Instant,
    game_port: u16,
    local_port: Option<u16>,
    motd: Arc<Mutex<String>>,
    exited: Option<String>,
}

#[tauri::command]
pub async fn p2p_status(app: tauri::AppHandle, state: State<'_, P2p>) -> Result<Status, ErrorPayload> {
    let state = state.inner();
    let preparing = state.busy.load(Ordering::SeqCst);
    let download = *lock(&state.download);
    let probe = {
        let mut session = lock(&state.session);
        let Some(session) = session.as_mut() else {
            return Ok(Status { preparing, download, ..Status::default() });
        };
        if session.exited.is_none() && matches!(session.child.try_wait(), Ok(Some(_)) | Err(_)) {
            session.exited = Some(easytier::last_error(&session.log).unwrap_or_default());
        }
        Probe {
            role: session.role,
            code: session.code.clone(),
            hostname: session.hostname.clone(),
            cli: session.cli.clone(),
            rpc_port: session.rpc_port,
            started: session.started,
            game_port: session.game_port,
            local_port: session.local_port,
            motd: session.motd.clone(),
            exited: session.exited.clone(),
        }
    };
    let running = probe.exited.is_none();
    let (peers, node) = if running {
        let (peers, node) = tokio::join!(
            easytier::query(&probe.cli, probe.rpc_port, &["peer", "list"]),
            easytier::query(&probe.cli, probe.rpc_port, &["node", "info"]),
        );
        (peers.ok().map(|value| health::peers(&value)), node.ok().map(|value| health::node(&value)))
    } else {
        (None, None)
    };
    // A guest pings through the room only once the host is there.
    let host_found = peers.as_ref().is_some_and(|peers| peers.iter().any(|p| !p.local && p.ipv4 == room::HOST_IP));
    let target = match probe.role {
        Role::Host => Some(probe.game_port),
        Role::Guest => probe.local_port.filter(|_| host_found),
    };
    let reply = match target.filter(|_| running) {
        Some(port) => {
            let services = app.state::<crate::LauncherState>().services();
            let ping = tokio::time::timeout(Duration::from_secs(3), services.ping_server(&format!("127.0.0.1:{port}"))).await;
            Some(ping.ok().and_then(Result::ok))
        }
        None => None,
    };
    let game = reply.as_ref().map(Option::is_some);
    let world = reply.flatten().map(|status| {
        let text: String = status.motd.iter().map(|span| span.text.as_str()).collect();
        World {
            motd: text.lines().next().unwrap_or_default().trim().to_string(),
            online: status.online,
            max: status.max,
            version: status.version,
            latency_ms: status.latency_ms,
        }
    });
    if probe.role == Role::Guest
        && let Some(world) = world.as_ref().filter(|w| !w.motd.is_empty())
    {
        *lock(&probe.motd) = world.motd.clone();
    }
    let game_latency_ms = world.as_ref().map(|world| world.latency_ms);
    let report = health::grade(&health::Inputs {
        role: probe.role,
        elapsed: probe.started.elapsed(),
        exited: probe.exited.as_deref(),
        peers: peers.as_deref(),
        node: node.as_ref(),
        game,
        game_latency_ms,
        game_port: probe.game_port,
    });
    Ok(Status {
        active: true,
        preparing,
        download,
        role: Some(probe.role),
        code: Some(probe.code),
        hostname: probe.hostname,
        game_port: probe.game_port,
        address: probe.local_port.map(|port| format!("127.0.0.1:{port}")),
        uptime_seconds: probe.started.elapsed().as_secs(),
        health: Some(report),
        world,
    })
}

#[tauri::command]
pub fn p2p_stop(state: State<'_, P2p>) -> Result<(), ErrorPayload> {
    state.stop_all();
    Ok(())
}
