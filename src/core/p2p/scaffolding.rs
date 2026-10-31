//! Scaffolding (https://github.com/Scaffolding-MC/Scaffolding-MC, MIT), the
//! room protocol Terracotta brought to HMCL, PCL CE and other launchers, so
//! their players and ours can share a room. On top of an EasyTier network
//! named after the room code, the host runs a small TCP service that tells
//! guests the game's port and keeps the list of who is in the room.
//!
//! Room codes look like `U/YNZE-U61D-2206-HXRG`: sixteen characters from
//! 0-9 and A-Z without I and O, whose value read little-endian in base 34 is
//! a multiple of 7. The first eight name the network, the last eight are its
//! secret.
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::core::error::{AppError, AppResult};
use crate::core::i18n::pick;
use crate::core::paths::AppPaths;

const ALPHABET: &[u8; 34] = b"0123456789ABCDEFGHJKLMNPQRSTUVWXYZ";
const CODE_CHARS: usize = 16;

/// Where Scaffolding hosts sit in the room's network.
pub const HOST_IP: &str = "10.144.144.1";
/// A host's EasyTier hostname is this followed by its service port.
pub const HOSTNAME_PREFIX: &str = "scaffolding-mc-server-";
/// Terracotta's shared nodes; a room needs one node both sides use, so
/// rooms always include these.
pub const NODES: &[&str] = &[
    "tcp://public.easytier.top:11010",
    "tcp://public2.easytier.cn:54321",
    "https://etnode.zkitefly.eu.org/node1",
    "https://etnode.zkitefly.eu.org/node2",
];

/// The protocols this side speaks, all of the standard set.
const PROTOCOLS: &[&str] = &["c:ping", "c:protocols", "c:server_port", "c:player_ping", "c:player_profiles_list"];
/// Bodies are tiny; anything bigger is not a Scaffolding peer.
const MAX_BODY: u32 = 64 * 1024;
/// A guest that hasn't pinged for this long has left.
const GUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_GUESTS: usize = 64;
pub const HEARTBEAT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    /// The code's characters, as values 0..34.
    values: [u8; CODE_CHARS],
}

impl Room {
    pub fn new() -> Self {
        loop {
            let mut values = [0u8; CODE_CHARS];
            let mut filled = 0;
            while filled < CODE_CHARS {
                for byte in uuid::Uuid::new_v4().into_bytes() {
                    // 238 = 7 × 34: keeps every character equally likely.
                    if byte < 238 && filled < CODE_CHARS {
                        values[filled] = byte % 34;
                        filled += 1;
                    }
                }
            }
            let room = Self { values };
            if room.valid() {
                return room;
            }
        }
    }

    fn valid(&self) -> bool {
        // Σ vᵢ·34ⁱ mod 7, first character least significant.
        let (mut sum, mut power) = (0u32, 1u32);
        for value in self.values {
            sum = (sum + u32::from(value) * power) % 7;
            power = power * 34 % 7;
        }
        sum == 0
    }

    fn text(&self) -> String {
        self.values.iter().map(|&v| ALPHABET[usize::from(v)] as char).collect()
    }

    pub fn code(&self) -> String {
        let text = self.text();
        format!("U/{}-{}-{}-{}", &text[..4], &text[4..8], &text[8..12], &text[12..])
    }

    /// Reads a code as typed or pasted: the `U/` prefix, case, spaces and
    /// dashes don't matter, and O and I are read as 0 and 1. None when it
    /// isn't a Scaffolding code at all; an error when it looks like one but
    /// doesn't check out.
    pub fn parse(code: &str) -> Option<AppResult<Self>> {
        let trimmed = code.trim();
        let prefixed = trimmed.get(..2).is_some_and(|start| start.eq_ignore_ascii_case("U/"));
        let body = if prefixed { &trimmed[2..] } else { trimmed };
        let chars: Vec<char> = body.chars().filter(|c| !matches!(c, '-' | ' ' | '\t' | '\n' | '\r')).collect();
        if !prefixed && chars.len() != CODE_CHARS {
            return None;
        }
        let invalid = || AppError::new("room_code", pick("房间码不对：应为 U/ 开头的 16 位字母数字，请向房主重新要一次。", "That room code isn't right: it's U/ followed by 16 letters and digits. Ask the host for it again."));
        if chars.len() != CODE_CHARS {
            return Some(Err(invalid()));
        }
        let mut values = [0u8; CODE_CHARS];
        for (slot, c) in values.iter_mut().zip(chars) {
            let c = match c.to_ascii_uppercase() {
                'O' => '0',
                'I' => '1',
                other => other,
            };
            match ALPHABET.iter().position(|&a| a as char == c) {
                Some(value) => *slot = value as u8,
                None => return Some(Err(invalid())),
            }
        }
        let room = Self { values };
        Some(if room.valid() { Ok(room) } else { Err(invalid()) })
    }

    pub fn network_name(&self) -> String {
        let text = self.text();
        format!("scaffolding-mc-{}-{}", &text[..4], &text[4..8])
    }

    pub fn network_secret(&self) -> String {
        let text = self.text();
        format!("{}-{}", &text[8..12], &text[12..])
    }
}

impl Default for Room {
    fn default() -> Self {
        Self::new()
    }
}

pub fn hostname(service_port: u16) -> String {
    format!("{HOSTNAME_PREFIX}{service_port}")
}

/// The service port a host's hostname announces.
pub fn service_port(hostname: &str) -> Option<u16> {
    hostname.strip_prefix(HOSTNAME_PREFIX)?.parse().ok().filter(|&port| port > 1024)
}

/// One player in the room, as the protocol describes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub machine_id: String,
    /// The launcher they use, e.g. `Islet 0.4.0` or `HMCL 3.6`.
    pub vendor: String,
    /// `HOST` or `GUEST` in the player list; absent in a ping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

impl Profile {
    pub fn new(name: &str, machine_id: &str) -> Self {
        Self { name: name.chars().take(64).collect(), machine_id: machine_id.to_string(), vendor: format!("Islet {}", env!("CARGO_PKG_VERSION")), kind: None }
    }

    pub fn is_host(&self) -> bool {
        self.kind.as_deref() == Some("HOST")
    }
}

/// A random id kept in the launcher's folder, so this computer stays the
/// same player across rooms.
pub fn machine_id(paths: &AppPaths) -> String {
    let file = paths.root.join("machine-id");
    if let Ok(id) = std::fs::read_to_string(&file) {
        let id = id.trim();
        if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return id.to_string();
        }
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let _ = std::fs::write(&file, &id);
    id
}

async fn read_frame<S: AsyncRead + Unpin>(stream: &mut S) -> std::io::Result<Vec<u8>> {
    let length = stream.read_u32().await?;
    if length > MAX_BODY {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "body too large"));
    }
    let mut body = vec![0; length as usize];
    stream.read_exact(&mut body).await?;
    Ok(body)
}

/// Sends one request and reads its answer: a status (0 is success) and a body.
pub async fn request<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, kind: &str, body: &[u8]) -> AppResult<(u8, Vec<u8>)> {
    let exchange = async {
        let mut message = Vec::with_capacity(5 + kind.len() + body.len());
        message.push(kind.len() as u8);
        message.extend_from_slice(kind.as_bytes());
        message.extend_from_slice(&(body.len() as u32).to_be_bytes());
        message.extend_from_slice(body);
        stream.write_all(&message).await?;
        stream.flush().await?;
        let status = stream.read_u8().await?;
        Ok::<_, std::io::Error>((status, read_frame(stream).await?))
    };
    tokio::time::timeout(Duration::from_secs(10), exchange)
        .await
        .map_err(|_| AppError::new("scaffolding", pick("房主没有回应。", "The host didn't answer.")))?
        .map_err(|error| AppError::detail("scaffolding", pick("和房主的连接断了。", "Lost the connection to the host."), error.to_string()))
}

async fn expect_ok<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, kind: &str, body: &[u8]) -> AppResult<Vec<u8>> {
    match request(stream, kind, body).await? {
        (0, body) => Ok(body),
        (32, _) if kind == "c:server_port" => Err(AppError::new("scaffolding_no_server", pick("房主的世界还没开好，稍等一下。", "The host's world isn't open yet; hold on."))),
        (status, body) => Err(AppError::detail("scaffolding", pick("房主拒绝了请求。", "The host refused the request."), format!("{kind}: {status} {}", String::from_utf8_lossy(&body)))),
    }
}

/// A guest's first exchange: say hello, agree on protocols, and learn the
/// port the host's game listens on.
pub async fn handshake<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, me: &Profile) -> AppResult<u16> {
    expect_ok(stream, "c:player_ping", &serde_json::to_vec(me)?).await?;
    expect_ok(stream, "c:protocols", PROTOCOLS.join("\0").as_bytes()).await?;
    let port = expect_ok(stream, "c:server_port", &[]).await?;
    match port[..] {
        [high, low] if u16::from_be_bytes([high, low]) != 0 => Ok(u16::from_be_bytes([high, low])),
        _ => Err(AppError::new("scaffolding", pick("房主给出的游戏端口无效。", "The host gave an invalid game port."))),
    }
}

/// The guest's regular ping, which also fetches who is in the room.
pub async fn heartbeat<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, me: &Profile) -> AppResult<Vec<Profile>> {
    expect_ok(stream, "c:player_ping", &serde_json::to_vec(me)?).await?;
    let list = expect_ok(stream, "c:player_profiles_list", &[]).await?;
    Ok(serde_json::from_slice(&list).unwrap_or_default())
}

/// The host's side: its own profile, its game's port, and the guests that
/// keep pinging.
pub struct Host {
    me: Profile,
    game_port: u16,
    guests: Mutex<Vec<(Profile, Instant)>>,
}

impl Host {
    pub fn new(me: Profile, game_port: u16) -> Self {
        Self { me, game_port, guests: Mutex::new(Vec::new()) }
    }

    /// The host first, then the guests seen recently.
    pub fn players(&self) -> Vec<Profile> {
        let mut guests = self.guests.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        guests.retain(|(_, seen)| seen.elapsed() < GUEST_TIMEOUT);
        let host = Profile { kind: Some("HOST".into()), ..self.me.clone() };
        std::iter::once(host).chain(guests.iter().map(|(p, _)| Profile { kind: Some("GUEST".into()), ..p.clone() })).collect()
    }

    fn seen(&self, profile: Profile) {
        let mut guests = self.guests.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        guests.retain(|(p, seen)| p.machine_id != profile.machine_id && seen.elapsed() < GUEST_TIMEOUT);
        if guests.len() < MAX_GUESTS && profile.machine_id != self.me.machine_id {
            guests.push((profile, Instant::now()));
        }
    }

    /// The answer to one request: status and body.
    fn answer(&self, kind: &str, body: Vec<u8>) -> (u8, Vec<u8>) {
        let bad = |what: &str| (255, what.as_bytes().to_vec());
        match kind {
            "c:ping" => (0, body),
            "c:protocols" => (0, PROTOCOLS.join("\0").into_bytes()),
            "c:server_port" => (0, self.game_port.to_be_bytes().to_vec()),
            "c:player_ping" => match serde_json::from_slice::<Profile>(&body) {
                Ok(profile) if !profile.machine_id.is_empty() => {
                    self.seen(Profile { name: profile.name.chars().take(64).collect(), vendor: profile.vendor.chars().take(64).collect(), kind: None, ..profile });
                    (0, Vec::new())
                }
                _ => bad("invalid player profile"),
            },
            "c:player_profiles_list" => (0, serde_json::to_vec(&self.players()).unwrap_or_default()),
            _ => bad("unsupported protocol"),
        }
    }
}

/// Answers requests on one connection until the guest hangs up or goes
/// quiet for a minute.
async fn handle<S: AsyncRead + AsyncWrite + Unpin>(mut stream: S, host: &Host) -> std::io::Result<()> {
    loop {
        let read = async {
            let length = stream.read_u8().await?;
            let mut kind = vec![0; usize::from(length)];
            stream.read_exact(&mut kind).await?;
            Ok::<_, std::io::Error>((kind, read_frame(&mut stream).await?))
        };
        let Ok(request) = tokio::time::timeout(Duration::from_secs(60), read).await else {
            return Ok(());
        };
        let (kind, body) = request?;
        let (status, answer) = host.answer(&String::from_utf8_lossy(&kind), body);
        stream.write_u8(status).await?;
        stream.write_u32(answer.len() as u32).await?;
        stream.write_all(&answer).await?;
        stream.flush().await?;
    }
}

/// The host's service, until the task running it is aborted.
pub async fn serve(listener: tokio::net::TcpListener, host: std::sync::Arc<Host>) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        };
        let host = host.clone();
        tokio::spawn(async move {
            let _ = handle(stream, &host).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_follow_the_spec() {
        // The example from the protocol's README.
        let room = Room::parse("U/YNZE-U61D-2206-HXRG").unwrap().unwrap();
        assert_eq!(room.network_name(), "scaffolding-mc-YNZE-U61D");
        assert_eq!(room.network_secret(), "2206-HXRG");
        assert_eq!(room.code(), "U/YNZE-U61D-2206-HXRG");
        assert_eq!(Room::parse(" u/ynze u61d 2206 hxrg ").unwrap().unwrap(), room);
        for _ in 0..50 {
            let room = Room::new();
            assert_eq!(Room::parse(&room.code()).unwrap().unwrap(), room);
        }
        // Off by one character: the multiple of 7 no longer holds.
        assert!(Room::parse("U/YNZE-U61D-2206-HXRH").unwrap().is_err());
        assert!(Room::parse("U/YNZE-U61D").unwrap().is_err());
        // An Islet code is left to the Islet parser.
        assert!(Room::parse("7KQ2M9A-XC40PRT-1BZ8V3W").is_none());
        // Text that isn't a code at all, starting with a wide character.
        assert!(Room::parse("房间码").is_none());
        assert_eq!(service_port("scaffolding-mc-server-33768"), Some(33768));
        assert_eq!(service_port("scaffolding-mc-server-77844"), None);
        assert_eq!(service_port("scaffolding-mc-server-n8"), None);
        assert_eq!(service_port("scaffolding-mc-server-80"), None);
    }

    #[tokio::test]
    async fn a_guest_learns_the_port_and_the_players() {
        let host = std::sync::Arc::new(Host::new(Profile::new("Steve", "host-id"), 25565));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(listener, host.clone()));

        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        let me = Profile { vendor: "HMCL 3.6".into(), ..Profile::new("Alex", "guest-id") };
        assert_eq!(handshake(&mut stream, &me).await.unwrap(), 25565);
        let players = heartbeat(&mut stream, &me).await.unwrap();
        assert_eq!(players.len(), 2);
        assert!(players[0].is_host() && players[0].name == "Steve");
        assert_eq!((players[1].name.as_str(), players[1].vendor.as_str(), players[1].kind.as_deref()), ("Alex", "HMCL 3.6", Some("GUEST")));
        // Pinging again doesn't add the same player twice.
        assert_eq!(heartbeat(&mut stream, &me).await.unwrap().len(), 2);
        assert_eq!(request(&mut stream, "c:ping", b"hello").await.unwrap(), (0, b"hello".to_vec()));
        assert_eq!(request(&mut stream, "x:unknown", b"").await.unwrap().0, 255);
        server.abort();
    }

    /// Two real EasyTier cores on this computer, peered directly: the guest
    /// finds the host's service by its hostname, forwards a port to it, is
    /// told the game's port, and reaches the game through a second forward.
    /// `ISLET_EASYTIER_DIR=<dir with easytier-core and easytier-cli>
    /// cargo test scaffolding -- --ignored`
    #[tokio::test]
    #[ignore = "needs EasyTier binaries in ISLET_EASYTIER_DIR"]
    async fn two_cores_link_up() {
        use crate::core::p2p::{easytier, health};
        use std::time::Instant;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let dir = std::path::PathBuf::from(std::env::var("ISLET_EASYTIER_DIR").expect("ISLET_EASYTIER_DIR"));
        let binaries = easytier::Binaries { core: dir.join("easytier-core"), cli: dir.join("easytier-cli") };
        let logs = tempfile::tempdir().unwrap();
        // Stops a core however the test ends.
        struct Core(std::process::Child);
        impl Drop for Core {
            fn drop(&mut self) {
                let _ = (self.0.kill(), self.0.wait());
            }
        }

        // The "game": echoes what it is sent.
        let game = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
        let game_port = game.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = game.accept().await {
                tokio::spawn(async move {
                    let mut buffer = [0u8; 64];
                    while let Ok(read @ 1..) = socket.read(&mut buffer).await {
                        let _ = socket.write_all(&buffer[..read]).await;
                    }
                });
            }
        });

        let room = Room::new();
        let (name, secret) = (room.network_name(), room.network_secret());
        let port = || easytier::free_port(true).unwrap();
        let (host_rpc, host_listen, guest_rpc, guest_listen) = (port(), port(), port(), port());
        let service = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
        let service_port = service.local_addr().unwrap().port();
        let host = std::sync::Arc::new(Host::new(Profile::new("Steve", "host-id"), game_port));
        tokio::spawn(serve(service, host.clone()));

        let host_name = hostname(service_port);
        let nowhere = vec!["tcp://127.0.0.1:9".to_string()];
        let host_net = easytier::Network { name: &name, secret: &secret, hostname: &host_name, nodes: &nowhere, rpc_port: host_rpc, listen_port: host_listen, ip: Some(HOST_IP) };
        let host_core = Core(easytier::spawn(&binaries, &easytier::scaffolding_host_args(&host_net, service_port, game_port), &logs.path().join("host.log")).unwrap());
        // EasyTier doesn't peer over loopback; this computer's LAN address.
        let probe = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        probe.connect("192.0.2.1:9").unwrap();
        let lan = probe.local_addr().unwrap().ip();
        let to_host = vec![format!("tcp://{lan}:{host_listen}")];
        let guest_net = easytier::Network { name: &name, secret: &secret, hostname: "Alex", nodes: &to_host, rpc_port: guest_rpc, listen_port: guest_listen, ip: None };
        let guest_core = Core(easytier::spawn(&binaries, &easytier::scaffolding_guest_args(&guest_net), &logs.path().join("guest.log")).unwrap());

        let outcome = async {
            let started = Instant::now();
            let (host_ip, found_port) = loop {
                assert!(started.elapsed() < Duration::from_secs(60), "the guest never saw the host");
                let peers = easytier::query(&binaries.cli, guest_rpc, &["peer", "list"]).await.map(|value| health::peers(&value)).unwrap_or_default();
                if let Some(found) = peers.into_iter().find_map(|p| super::service_port(&p.hostname).filter(|_| !p.local && !p.ipv4.is_empty()).map(|port| (p.ipv4, port))) {
                    break found;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            };
            assert_eq!((host_ip.as_str(), found_port), (HOST_IP, service_port));

            let local_service = easytier::free_port(false).unwrap();
            easytier::add_port_forward(&binaries.cli, guest_rpc, &format!("127.0.0.1:{local_service}"), &format!("{host_ip}:{service_port}")).await.unwrap();
            let me = Profile::new("Alex", "guest-id");
            let mut stream = loop {
                assert!(started.elapsed() < Duration::from_secs(90), "the host's service never answered");
                if let Ok(mut stream) = tokio::net::TcpStream::connect(("127.0.0.1", local_service)).await
                    && let Ok(Ok(port)) = tokio::time::timeout(Duration::from_secs(5), handshake(&mut stream, &me)).await
                {
                    assert_eq!(port, game_port);
                    break stream;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            };
            let players = heartbeat(&mut stream, &me).await.unwrap();
            assert_eq!(players.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Steve", "Alex"]);
            assert_eq!(host.players().len(), 2);

            let local_game = easytier::free_port(false).unwrap();
            easytier::add_port_forward(&binaries.cli, guest_rpc, &format!("127.0.0.1:{local_game}"), &format!("{host_ip}:{game_port}")).await.unwrap();
            let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", local_game)).await.unwrap();
            socket.write_all(b"hello through the room").await.unwrap();
            let mut echoed = [0u8; 22];
            tokio::time::timeout(Duration::from_secs(10), socket.read_exact(&mut echoed)).await.unwrap().unwrap();
            assert_eq!(&echoed, b"hello through the room");
        };
        let result = tokio::time::timeout(Duration::from_secs(120), outcome).await;
        drop((host_core, guest_core));
        if result.is_err() {
            for log in ["host.log", "guest.log"] {
                eprintln!("--- {log}\n{}", std::fs::read_to_string(logs.path().join(log)).unwrap_or_default());
            }
            panic!("timed out");
        }
    }
}
