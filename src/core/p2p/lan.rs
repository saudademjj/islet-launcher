//! Minecraft's LAN discovery: a game opened to LAN sends
//! `[MOTD]name[/MOTD][AD]port[/AD]` to 224.0.2.60:4445 every 1.5 s, and the
//! multiplayer screen lists what it hears there. The host side listens for
//! its own game's port; the guest side announces the forwarded port so the
//! room shows up in the guest's list like a LAN world.
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::core::error::{AppError, AppResult};

const GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 2, 60);
const PORT: u16 = 4445;
/// Ends every announcement this launcher sends, so the host side never
/// mistakes a guest's re-announcement for a game.
pub const RELAY_MARK: &str = " · P2P";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanGame {
    pub motd: String,
    pub port: u16,
}

pub fn parse(packet: &str) -> Option<LanGame> {
    let motd = between(packet, "[MOTD]", "[/MOTD]")?;
    let port = between(packet, "[AD]", "[/AD]")?;
    // Old versions put "host:port" in the AD field.
    let port = port.rsplit(':').next()?.trim().parse::<u16>().ok().filter(|p| *p != 0)?;
    Some(LanGame { motd: motd.to_string(), port })
}

pub fn announcement(motd: &str, port: u16) -> String {
    // The brackets would end the field early.
    let motd: String = motd.chars().filter(|c| !matches!(c, '[' | ']') && !c.is_control()).take(64).collect();
    format!("[MOTD]{motd}{RELAY_MARK}[/MOTD][AD]{port}[/AD]")
}

fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = text.find(start)? + start.len();
    let to = from + text[from..].find(end)?;
    Some(&text[from..to])
}

/// Listens for `window` and returns the games opened to LAN on this
/// computer: announcements whose port takes a connection on 127.0.0.1.
pub async fn detect(window: Duration) -> AppResult<Vec<LanGame>> {
    let socket = listener().map_err(|error| {
        AppError::detail(
            "lan_listen",
            crate::core::i18n::pick("无法监听局域网游戏广播（端口 4445）。可以直接填写游戏显示的端口号。", "Couldn't listen for LAN game announcements (port 4445). Enter the port the game shows instead."),
            error.to_string(),
        )
    })?;
    let mut heard = BTreeMap::new();
    let mut buffer = [0u8; 1024];
    let deadline = tokio::time::Instant::now() + window;
    while let Ok(Ok((size, _))) = tokio::time::timeout_at(deadline, socket.recv_from(&mut buffer)).await {
        if let Some(game) = parse(&String::from_utf8_lossy(&buffer[..size])).filter(|g| !g.motd.ends_with(RELAY_MARK)) {
            heard.insert(game.port, game);
        }
    }
    let mut local = Vec::new();
    for game in heard.into_values() {
        if listening(game.port).await {
            local.push(game);
        }
    }
    Ok(local)
}

/// Whether something on this computer takes connections on `port`.
pub async fn listening(port: u16) -> bool {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    matches!(tokio::time::timeout(Duration::from_secs(1), tokio::net::TcpStream::connect(address)).await, Ok(Ok(_)))
}

/// Binds 4445 alongside a running game's own listener (it sets address
/// reuse too) and joins the LAN group.
fn listener() -> std::io::Result<tokio::net::UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, PORT)).into())?;
    socket.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED)?;
    socket.set_nonblocking(true)?;
    tokio::net::UdpSocket::from_std(socket.into())
}

/// Announces `port` until the task is dropped. `motd` can change while it
/// runs (the host's world name arrives with the first status reply).
pub async fn announce(motd: Arc<Mutex<String>>, port: u16) {
    let Ok(socket) = tokio::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await else {
        tracing::warn!("p2p: can't open a socket for LAN announcements");
        return;
    };
    let target = SocketAddrV4::new(GROUP, PORT);
    let mut tick = tokio::time::interval(Duration::from_millis(1500));
    loop {
        tick.tick().await;
        let text = announcement(&motd.lock().map(|m| m.clone()).unwrap_or_default(), port);
        if let Err(error) = socket.send_to(text.as_bytes(), target).await {
            tracing::debug!("p2p: LAN announcement failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_game_announcements() {
        assert_eq!(parse("[MOTD]Steve - New World[/MOTD][AD]51234[/AD]"), Some(LanGame { motd: "Steve - New World".into(), port: 51234 }));
        assert_eq!(parse("[MOTD]Old[/MOTD][AD]192.168.1.4:25565[/AD]").map(|g| g.port), Some(25565));
        assert_eq!(parse("[MOTD]x[/MOTD]"), None);
        assert_eq!(parse("[MOTD]x[/MOTD][AD]0[/AD]"), None);
        assert_eq!(parse("[MOTD]x[/MOTD][AD]70000[/AD]"), None);
    }

    #[test]
    fn announcements_round_trip_and_carry_the_mark() {
        let text = announcement("Alex [the] - World\n", 40001);
        let game = parse(&text).unwrap();
        assert_eq!(game.port, 40001);
        assert_eq!(game.motd, format!("Alex the - World{RELAY_MARK}"));
        assert!(game.motd.ends_with(RELAY_MARK));
    }
}
