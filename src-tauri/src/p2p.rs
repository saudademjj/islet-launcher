//! P2P rooms: one EasyTier core at a time, either hosting a world opened to
//! LAN or, as a guest, reaching one through a port on this computer. Each
//! status poll reads the core's peers and pings the game to grade health.
//!
//! Rooms are Scaffolding rooms (`U/…` codes), which players of HMCL, PCL CE
//! and other Terracotta-based launchers can join too; codes from earlier
//! Islet versions can still be joined.
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use islet::core::error::{AppError, ErrorPayload};
use islet::core::i18n::pick;
use islet::core::p2p::health::{self, Role};
use islet::core::p2p::room::{self, Room};
use islet::core::p2p::scaffolding::{self, Profile};
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
    /// The guest's LAN announcement, a Scaffolding host's service or a
    /// Scaffolding guest's link to the host; stopped with the session.
    tasks: Vec<tauri::async_runtime::JoinHandle<()>>,
    /// Set once the core has exited, with what its log said.
    exited: Option<String>,
    /// Scaffolding rooms: who is in the room and how the guest's link is.
    scaffold: Option<Arc<Scaffold>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        for task in self.tasks.drain(..) {
            task.abort();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A Scaffolding room's own state, besides what EasyTier knows.
#[derive(Default)]
struct Scaffold {
    /// Set when this computer hosts.
    host: Option<Arc<scaffolding::Host>>,
    link: Mutex<Link>,
}

#[derive(Default)]
struct Link {
    /// The host's address in the room, once found.
    host_ip: Option<String>,
    /// Guest: the forward to the host's game is in place.
    ready: bool,
    /// Guest: the game's port on the host, as its service told.
    game_port: Option<u16>,
    /// Guest: the room's players, as the host last listed them.
    players: Vec<Profile>,
    /// Guest: why the link isn't up, while it isn't.
    problem: Option<String>,
}

impl Scaffold {
    fn players(&self) -> Vec<Profile> {
        match &self.host {
            Some(host) => host.players(),
            None => lock(&self.link).players.clone(),
        }
    }
}

/// Aborts a task when dropped, so one task's helpers end with it.
struct Guard(tauri::async_runtime::JoinHandle<()>);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    name: String,
    /// The launcher they play with, e.g. "HMCL 3.6".
    vendor: String,
    host: bool,
    /// This computer.
    me: bool,
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
    /// Scaffolding rooms: everyone in the room, by the launcher they use.
    members: Vec<Member>,
    /// Guest: what holds the link to the host back, while something does.
    notice: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Defaults {
    nodes: Vec<String>,
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

/// Free local ports, distinct from each other; `true` also needs UDP.
fn free_ports<const N: usize>(udp: [bool; N]) -> Result<[u16; N], ErrorPayload> {
    let mut ports = [0u16; N];
    for (index, udp) in udp.into_iter().enumerate() {
        ports[index] = (0..10)
            .find_map(|_| easytier::free_port(udp).ok().filter(|port| !ports[..index].contains(port)))
            .ok_or_else(|| fail(pick("找不到可用的本地端口。", "No free local port was found.")))?;
    }
    Ok(ports)
}

/// What every session starts the same way.
struct Start<'a> {
    role: Role,
    code: String,
    /// The name shown for this computer.
    display: String,
    args: Vec<String>,
    rpc_port: u16,
    game_port: u16,
    local_port: Option<u16>,
    binaries: &'a easytier::Binaries,
}

fn start(app: &tauri::AppHandle, start: Start) -> Result<Session, ErrorPayload> {
    let services = app.state::<crate::LauncherState>().services();
    let log = services.paths.logs.join(if start.role == Role::Host { "easytier-host.log" } else { "easytier-guest.log" });
    let child = easytier::spawn(start.binaries, &start.args, &log).map_err(payload)?;
    let motd = Arc::new(Mutex::new(islet::tr!("房间 {}", "Room {}", start.code.trim_start_matches("U/").get(..9).unwrap_or_default())));
    Ok(Session {
        role: start.role,
        code: start.code,
        hostname: start.display,
        child,
        cli: start.binaries.cli.clone(),
        rpc_port: start.rpc_port,
        started: Instant::now(),
        log,
        game_port: start.game_port,
        local_port: start.local_port,
        motd,
        tasks: Vec::new(),
        exited: None,
        scaffold: None,
    })
}

/// The name this player goes by in a room.
fn player_name(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() { easytier::hostname(name) } else { name.chars().take(32).collect() }
}

/// Hosts the world on `game_port` in a new Scaffolding room.
async fn open_host(app: &tauri::AppHandle, game_port: u16, name: &str, nodes: Vec<String>, binaries: easytier::Binaries) -> Result<Session, ErrorPayload> {
    let services = app.state::<crate::LauncherState>().services();
    let [rpc_port, listen_port, service_port] = free_ports([false, true, false])?;
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, service_port))
        .await
        .map_err(|error| AppError::detail("p2p_start", pick("无法启动房间服务。", "Couldn't start the room service."), error.to_string()).to_payload())?;
    let host = Arc::new(scaffolding::Host::new(Profile::new(&player_name(name), &scaffolding::machine_id(&services.paths)), game_port));
    let room = scaffolding::Room::new();
    let (network, secret, hostname) = (room.network_name(), room.network_secret(), scaffolding::hostname(service_port));
    let net = easytier::Network { name: &network, secret: &secret, hostname: &hostname, nodes: &nodes, rpc_port, listen_port, ip: Some(scaffolding::HOST_IP) };
    let args = easytier::scaffolding_host_args(&net, service_port, game_port);
    let mut session = start(app, Start { role: Role::Host, code: room.code(), display: player_name(name), args, rpc_port, game_port, local_port: None, binaries: &binaries })?;
    session.tasks.push(tauri::async_runtime::spawn(scaffolding::serve(listener, host.clone())));
    session.scaffold = Some(Arc::new(Scaffold { host: Some(host), link: Mutex::default() }));
    Ok(session)
}

/// Joins a Scaffolding room: the host's service and game are found once
/// the core is in the room, by `guest_link`.
fn open_guest(app: &tauri::AppHandle, room: &scaffolding::Room, name: &str, nodes: Vec<String>, binaries: easytier::Binaries) -> Result<Session, ErrorPayload> {
    let services = app.state::<crate::LauncherState>().services();
    let [rpc_port, listen_port, local_port] = free_ports([false, true, false])?;
    let (network, secret, hostname) = (room.network_name(), room.network_secret(), easytier::hostname(name));
    let net = easytier::Network { name: &network, secret: &secret, hostname: &hostname, nodes: &nodes, rpc_port, listen_port, ip: None };
    let args = easytier::scaffolding_guest_args(&net);
    let mut session = start(app, Start { role: Role::Guest, code: room.code(), display: player_name(name), args, rpc_port, game_port: 0, local_port: Some(local_port), binaries: &binaries })?;
    let scaffold = Arc::new(Scaffold::default());
    let me = Profile::new(&player_name(name), &scaffolding::machine_id(&services.paths));
    session.tasks.push(tauri::async_runtime::spawn(guest_link(session.cli.clone(), rpc_port, local_port, me, session.motd.clone(), scaffold.clone())));
    session.scaffold = Some(scaffold);
    Ok(session)
}

/// Joins a room by an earlier Islet version's code.
fn open_islet_guest(app: &tauri::AppHandle, room: &Room, name: &str, nodes: Vec<String>, binaries: easytier::Binaries) -> Result<Session, ErrorPayload> {
    let [rpc_port, listen_port, local_port] = free_ports([false, true, false])?;
    let hostname = easytier::hostname(name);
    let (network, secret) = (room.network_name(), room.network_secret());
    let net = easytier::Network { name: &network, secret: &secret, hostname: &hostname, nodes: &nodes, rpc_port, listen_port, ip: None };
    let args = easytier::guest_args(&net, local_port, room::HOST_IP, room.game_port);
    let mut session = start(app, Start { role: Role::Guest, code: room.code(), display: hostname.clone(), args, rpc_port, game_port: room.game_port, local_port: Some(local_port), binaries: &binaries })?;
    session.tasks.push(tauri::async_runtime::spawn(lan::announce(session.motd.clone(), local_port)));
    Ok(session)
}

/// A Scaffolding guest's way to the host: find its service in the room,
/// forward a local port to it, learn the game's port there, forward
/// `local_port` to the game and announce it on the LAN, then ping every
/// few seconds, starting over whenever the host stops answering.
async fn guest_link(cli: PathBuf, rpc_port: u16, local_port: u16, me: Profile, motd: Arc<Mutex<String>>, scaffold: Arc<Scaffold>) {
    let note = |problem: Option<String>| lock(&scaffold.link).problem = problem;
    let (host_ip, service_port) = loop {
        let found = easytier::query(&cli, rpc_port, &["peer", "list"]).await.ok().and_then(|value| {
            health::peers(&value).into_iter().find_map(|peer| scaffolding::service_port(&peer.hostname).filter(|_| !peer.local && !peer.ipv4.is_empty()).map(|port| (peer.ipv4, port)))
        });
        if let Some(found) = found {
            break found;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    lock(&scaffold.link).host_ip = Some(host_ip.clone());
    let service_local = loop {
        let added = match easytier::free_port(false) {
            Ok(port) => easytier::add_port_forward(&cli, rpc_port, &format!("127.0.0.1:{port}"), &format!("{host_ip}:{service_port}")).await.map(|()| port),
            Err(error) => Err(error),
        };
        match added {
            Ok(port) => break port,
            Err(error) => note(Some(error.user_message())),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    let mut game: Option<(u16, Guard)> = None;
    loop {
        let session = async {
            let mut stream = tokio::time::timeout(Duration::from_secs(8), tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, service_local)))
                .await
                .map_err(|_| AppError::new("scaffolding", pick("连不上房主的房间服务。", "Can't reach the host's room service.")))??;
            let port = scaffolding::handshake(&mut stream, &me).await?;
            if game.is_none() {
                easytier::add_port_forward(&cli, rpc_port, &format!("0.0.0.0:{local_port}"), &format!("{host_ip}:{port}")).await?;
                game = Some((port, Guard(tauri::async_runtime::spawn(lan::announce(motd.clone(), local_port)))));
            }
            {
                let mut link = lock(&scaffold.link);
                link.ready = true;
                link.game_port = Some(game.as_ref().map_or(port, |(port, _)| *port));
                link.problem = None;
            }
            loop {
                match scaffolding::heartbeat(&mut stream, &me).await {
                    Ok(players) => lock(&scaffold.link).players = players,
                    Err(error) => break Err::<(), AppError>(error),
                }
                tokio::time::sleep(scaffolding::HEARTBEAT).await;
            }
        };
        if let Err(error) = session.await {
            let mut link = lock(&scaffold.link);
            link.problem = Some(error.user_message());
            link.players.clear();
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

#[tauri::command]
pub async fn p2p_defaults(app: tauri::AppHandle) -> Result<Defaults, ErrorPayload> {
    let services = app.state::<crate::LauncherState>().services();
    let remote = services.p2p_remote().await;
    Ok(Defaults { nodes: easytier::nodes(&[], &remote), download_size: easytier::download_size(&services.paths) })
}

/// The nodes for a room: the player's own, else the published or built-in
/// ones; plus Terracotta's, which other launchers' rooms use.
async fn room_nodes(app: &tauri::AppHandle, custom: &[String]) -> Vec<String> {
    let remote = app.state::<crate::LauncherState>().services().p2p_remote().await;
    easytier::room_nodes(easytier::nodes(custom, &remote))
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
    let nodes = room_nodes(&app, &nodes).await;
    let session = open_host(&app, port, &name, nodes, binaries).await?;
    *lock(&state.session) = Some(session);
    Ok(())
}

#[tauri::command]
pub async fn p2p_join(app: tauri::AppHandle, state: State<'_, P2p>, code: String, name: String, nodes: Vec<String>) -> Result<(), ErrorPayload> {
    let state = state.inner();
    // A U/ code is a Scaffolding room; anything else, an earlier Islet's.
    let scaffolding_room = scaffolding::Room::parse(&code).transpose().map_err(payload)?;
    let islet_room = if scaffolding_room.is_none() { Some(Room::parse(&code).map_err(payload)?) } else { None };
    let _busy = begin(state)?;
    let binaries = binaries(&app, state).await?;
    let nodes = room_nodes(&app, &nodes).await;
    let session = match (&scaffolding_room, &islet_room) {
        (Some(room), _) => open_guest(&app, room, &name, nodes, binaries)?,
        (None, Some(room)) => open_islet_guest(&app, room, &name, nodes, binaries)?,
        (None, None) => unreachable!("one of the two parsed"),
    };
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
    scaffold: Option<Arc<Scaffold>>,
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
            scaffold: session.scaffold.clone(),
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
    let (host_ip, link_ready, notice, game_port) = match &probe.scaffold {
        Some(scaffold) if scaffold.host.is_some() => (scaffolding::HOST_IP.to_string(), true, None, probe.game_port),
        Some(scaffold) => {
            let link = lock(&scaffold.link);
            let ip = link.host_ip.clone().unwrap_or_else(|| scaffolding::HOST_IP.to_string());
            (ip, link.ready, link.problem.clone(), link.game_port.unwrap_or(probe.game_port))
        }
        None => (room::HOST_IP.to_string(), true, None, probe.game_port),
    };
    let players = probe.scaffold.as_ref().map(|scaffold| scaffold.players()).unwrap_or_default();
    let host_name = players.iter().find(|p| p.is_host()).map(|p| p.name.clone());
    // A guest pings through the room only once the host is there (and, in a
    // Scaffolding room, its game is forwarded).
    let host_found = link_ready && peers.as_ref().is_some_and(|peers| peers.iter().any(|p| !p.local && p.ipv4 == host_ip));
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
        game_port,
        host_ip: &host_ip,
        host_name: host_name.as_deref(),
    });
    let services = app.state::<crate::LauncherState>().services();
    let machine = scaffolding::machine_id(&services.paths);
    let members = players
        .into_iter()
        .map(|p| Member { me: p.machine_id == machine, host: p.is_host(), name: p.name, vendor: p.vendor })
        .collect();
    Ok(Status {
        active: true,
        preparing,
        download,
        role: Some(probe.role),
        code: Some(probe.code),
        hostname: probe.hostname,
        game_port,
        address: probe.local_port.filter(|_| link_ready).map(|port| format!("127.0.0.1:{port}")),
        uptime_seconds: probe.started.elapsed().as_secs(),
        health: Some(report),
        world,
        members,
        notice: notice.filter(|_| !link_ready),
    })
}

#[tauri::command]
pub fn p2p_stop(state: State<'_, P2p>) -> Result<(), ErrorPayload> {
    state.stop_all();
    Ok(())
}
