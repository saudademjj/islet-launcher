//! The room's health light: one overall level plus the checks behind it,
//! worked out from the core's peer list and node info and a status ping of
//! the game through the room.
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use super::room::HOST_IP;
use crate::core::i18n::pick;
use crate::tr;

/// Declared from best to worst, so the overall level is the maximum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Nothing to judge yet, e.g. no players have joined.
    Idle,
    Good,
    Fair,
    Starting,
    Poor,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Host,
    Guest,
}

/// One row of `easytier-cli peer list`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Peer {
    pub hostname: String,
    pub ipv4: String,
    /// This computer.
    pub local: bool,
    /// A shared node rather than a player.
    pub public: bool,
    /// Connected peer-to-peer rather than through a relay.
    pub direct: bool,
    pub latency_ms: Option<f64>,
    pub loss_percent: Option<f64>,
    pub tunnel: String,
}

pub fn peers(value: &Value) -> Vec<Peer> {
    let text = |row: &Value, key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default().trim().to_string();
    let number = |row: &Value, key: &str| text(row, key).trim_end_matches('%').parse::<f64>().ok().filter(|n| n.is_finite());
    let rows = value.as_array().map(Vec::as_slice).unwrap_or_default();
    rows.iter()
        .map(|row| {
            let (hostname, ipv4, cost) = (text(row, "hostname"), text(row, "ipv4"), text(row, "cost"));
            let local = cost == "Local";
            Peer {
                public: !local && (hostname.starts_with("PublicServer_") || ipv4.is_empty()),
                direct: cost == "p2p",
                latency_ms: number(row, "lat_ms"),
                loss_percent: number(row, "loss_rate"),
                tunnel: text(row, "tunnel_proto"),
                hostname,
                ipv4,
                local,
            }
        })
        .collect()
}

/// What `easytier-cli node info` says about this computer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Node {
    /// The virtual address without its prefix; empty until assigned.
    pub ipv4: String,
    /// EasyTier's UDP NAT type code, 0 while unknown.
    pub nat: u64,
}

pub fn node(value: &Value) -> Node {
    let ipv4 = value.get("ipv4_addr").and_then(Value::as_str).unwrap_or_default();
    Node {
        ipv4: ipv4.split('/').next().unwrap_or_default().to_string(),
        nat: value.pointer("/stun_info/udp_nat_type").and_then(Value::as_u64).unwrap_or(0),
    }
}

pub fn nat_name(code: u64) -> &'static str {
    match code {
        1 => pick("公网直连", "open internet"),
        2 => pick("公网（无端口转换）", "no port translation"),
        3 => pick("全锥形 NAT", "full-cone NAT"),
        4 => pick("限制锥形 NAT", "restricted-cone NAT"),
        5 => pick("端口限制锥形 NAT", "port-restricted NAT"),
        6 => pick("对称型 NAT", "symmetric NAT"),
        7 => pick("对称型 NAT（UDP 受限）", "symmetric NAT with UDP firewall"),
        8 | 9 => pick("对称型 NAT（端口可预测）", "symmetric NAT (predictable ports)"),
        _ => pick("未知 NAT", "unknown NAT"),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub id: &'static str,
    pub level: Level,
    pub label: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub name: String,
    pub ip: String,
    /// The room's host (a guest sees the host among its players).
    pub host: bool,
    pub direct: bool,
    pub latency_ms: Option<f64>,
    pub level: Level,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub level: Level,
    pub summary: String,
    pub checks: Vec<Check>,
    pub players: Vec<Player>,
    /// This computer's address in the room, once it has one.
    pub ip: Option<String>,
}

pub struct Inputs<'a> {
    pub role: Role,
    pub elapsed: Duration,
    /// Set once the core has exited, with the reason when one is known.
    pub exited: Option<&'a str>,
    /// None when the core didn't answer.
    pub peers: Option<&'a [Peer]>,
    pub node: Option<&'a Node>,
    /// Whether the game answered a status ping through the room; None while
    /// that can't be tried yet.
    pub game: Option<bool>,
    /// That ping's round trip, in milliseconds.
    pub game_latency_ms: Option<u32>,
    pub game_port: u16,
}

/// The time a link is judged by. EasyTier's latency for a relayed peer
/// counts the shared node's relay penalty, not time (1000 ms where the game
/// answered in 32), so a relayed link goes by `measured`, a game ping's round
/// trip through the room, and has no time without one.
fn latency(peer: &Peer, measured: Option<f64>) -> Option<f64> {
    if peer.direct { peer.latency_ms } else { measured }
}

/// How good one player's link is.
pub fn link(peer: &Peer, measured: Option<f64>) -> (Level, String) {
    let loss = peer.loss_percent.unwrap_or(0.0);
    let by_loss = |good: bool, fair: bool| if good { Level::Good } else if fair { Level::Fair } else { Level::Poor };
    let latency = latency(peer, measured);
    let level = match latency {
        Some(ms) => by_loss(ms < 80.0 && loss < 2.0, ms < 200.0 && loss < 10.0),
        None => by_loss(loss < 2.0, loss < 10.0),
    };
    // A relay adds a hop through someone else's server.
    let level = if peer.direct { level } else { level.max(Level::Fair) };
    let way = if peer.direct { pick("直连", "direct") } else { pick("中转", "relayed") };
    let detail = match latency {
        Some(ms) => tr!("{way} · {ms:.0} ms · 丢包 {loss:.0}%", "{way} · {ms:.0} ms · {loss:.0}% loss"),
        None => tr!("{way} · 丢包 {loss:.0}%", "{way} · {loss:.0}% loss"),
    };
    (level, detail)
}

pub fn grade(inputs: &Inputs) -> Report {
    let within = |seconds: u64| inputs.elapsed < Duration::from_secs(seconds);
    let mut checks = Vec::new();
    let mut check = |id, level, label, detail: String| checks.push(Check { id, level, label, detail });
    let core = pick("联机组件", "Networking");

    if let Some(reason) = inputs.exited {
        let detail = if reason.is_empty() {
            pick("联机组件已退出，请重新创建或加入房间。", "The networking component stopped. Create or join the room again.").to_string()
        } else {
            tr!("联机组件已退出：{reason}", "The networking component stopped: {reason}")
        };
        check("core", Level::Down, core, detail);
        return finish(inputs.role, checks, Vec::new(), None);
    }
    let Some(peers) = inputs.peers else {
        let (level, detail) = if within(10) {
            (Level::Starting, pick("正在启动…", "Starting…"))
        } else {
            (Level::Poor, pick("联机组件没有响应，可以停止后重试。", "The networking component isn't responding. Stop and try again."))
        };
        check("core", level, core, detail.to_string());
        return finish(inputs.role, checks, Vec::new(), None);
    };
    check("core", Level::Good, core, tr!("EasyTier {} 运行中", "EasyTier {} running", super::easytier::VERSION));

    let room: Vec<&Peer> = peers.iter().filter(|p| !p.local && !p.public).collect();
    let nodes: Vec<&Peer> = peers.iter().filter(|p| p.public).collect();
    let fastest = nodes.iter().filter_map(|p| p.latency_ms).min_by(f64::total_cmp);
    let (level, detail) = match (nodes.len(), fastest) {
        (0, _) if !room.is_empty() => (Level::Fair, pick("没连上共享节点，但已和房间里的玩家连通。", "No shared node reached, but the room's players are connected.").to_string()),
        (0, _) if within(15) => (Level::Starting, pick("正在连接共享节点…", "Reaching the shared nodes…").to_string()),
        (0, _) => (Level::Poor, pick("连不上任何共享节点：检查网络，或在高级设置里换一组节点。", "Can't reach any shared node. Check the network, or try other nodes under Advanced.").to_string()),
        (n, Some(ms)) => (Level::Good, tr!("已连接 {n} 个，最快 {ms:.0} ms", "{n} connected, fastest {ms:.0} ms")),
        (n, None) => (Level::Good, tr!("已连接 {n} 个", "{n} connected")),
    };
    check("nodes", level, pick("共享节点", "Shared nodes"), detail);

    let nat = inputs.node.map_or(0, |n| n.nat);
    let name = nat_name(nat);
    let (level, detail) = match nat {
        _ if room.iter().any(|p| p.direct) => (Level::Good, tr!("{name}，已和玩家直连", "{name}, connected directly")),
        0 if within(20) => (Level::Starting, pick("正在检测…", "Detecting…").to_string()),
        0 => (Level::Fair, pick("检测不出 NAT 类型，可能只能通过中转连接。", "The NAT type couldn't be detected; connections may need a relay.").to_string()),
        1..=5 => (Level::Good, tr!("{name}，通常可以直连", "{name}, usually connects directly")),
        _ => (Level::Fair, tr!("{name}，和部分玩家可能只能走中转，延迟会高一些", "{name}; some players may need a relay, with more delay")),
    };
    check("nat", level, pick("NAT 类型", "NAT type"), detail);

    let own_ip = inputs.node.map(|n| n.ipv4.clone()).filter(|ip| !ip.is_empty()).or_else(|| peers.iter().find(|p| p.local).map(|p| p.ipv4.clone()).filter(|ip| !ip.is_empty()));
    // Only a guest's ping crosses the room, and only to the host.
    let measured = |peer: &Peer| match inputs.role {
        Role::Guest if peer.ipv4 == HOST_IP => inputs.game_latency_ms.map(f64::from),
        _ => None,
    };
    let players: Vec<Player> = room
        .iter()
        .map(|peer| {
            let (level, detail) = link(peer, measured(peer));
            let latency_ms = latency(peer, measured(peer));
            Player { name: peer.hostname.clone(), ip: peer.ipv4.clone(), host: peer.ipv4 == HOST_IP, direct: peer.direct, latency_ms, level, detail }
        })
        .collect();
    let port = inputs.game_port;
    let world = pick("游戏世界", "Game world");
    match inputs.role {
        Role::Host => {
            let (level, detail) = match inputs.game {
                Some(true) => (Level::Good, tr!("已对局域网开放（端口 {port}）", "Open to LAN (port {port})")),
                Some(false) => (
                    Level::Down,
                    tr!("端口 {port} 上没有游戏：回到游戏重新“对局域网开放”，再重新创建房间。", "No game on port {port}. Open the world to LAN again, then create a new room."),
                ),
                None => (Level::Starting, pick("正在检查游戏…", "Checking the game…").to_string()),
            };
            check("game", level, world, detail);
            let (level, detail) = match players.iter().map(|p| p.level).max() {
                None => (Level::Idle, pick("还没有人加入，把房间码发给好友吧。", "No one has joined yet. Send your friends the room code.").to_string()),
                Some(worst) => {
                    let relayed = players.iter().filter(|p| !p.direct).count();
                    let count = players.len();
                    let detail = if relayed == 0 {
                        tr!("{count} 人已连接", "{count} connected")
                    } else {
                        tr!("{count} 人已连接，其中 {relayed} 人走中转", "{count} connected, {relayed} through a relay")
                    };
                    (worst, detail)
                }
            };
            check("players", level, pick("玩家", "Players"), detail);
        }
        Role::Guest => {
            let (level, detail) = match own_ip.as_deref() {
                Some(HOST_IP) => (Level::Fair, pick("地址和房主冲突，正在重新分配…", "Address clashes with the host's; getting a new one…").to_string()),
                Some(ip) => (Level::Good, ip.to_string()),
                None if within(20) => (Level::Starting, pick("正在分配地址…", "Getting an address…").to_string()),
                None => (Level::Poor, pick("还没分到地址：可能还没找到房间里的其他人。", "No address yet: no one else in the room has been found.").to_string()),
            };
            check("ip", level, pick("房间内地址", "Room address"), detail);
            let host = room.iter().find(|p| p.ipv4 == HOST_IP);
            let (level, detail) = match host {
                Some(peer) => link(peer, measured(peer)),
                None if within(25) => (Level::Starting, pick("正在寻找房主…", "Looking for the host…").to_string()),
                None => (
                    Level::Down,
                    pick(
                        "找不到房主：确认房主还开着房间、房间码没错，并且双方至少有一个相同的共享节点。",
                        "The host can't be found. Check that the room is still open, the code is right, and you share at least one node.",
                    )
                    .to_string(),
                ),
            };
            check("host", level, pick("房主连接", "Host link"), detail);
            let (level, detail) = match (host.is_some(), inputs.game) {
                (true, Some(true)) => (Level::Good, pick("房主的世界在线，可以加入。", "The host's world is up; you can join.").to_string()),
                (true, Some(false)) => (
                    Level::Poor,
                    pick("连上了房主，但他的世界没有响应：可能已关闭或换了端口。", "Connected to the host, but the world isn't answering. It may have closed or changed port.").to_string(),
                ),
                _ => (Level::Starting, pick("连上房主后检查…", "Checked once the host is connected…").to_string()),
            };
            check("game", level, world, detail);
        }
    }
    finish(inputs.role, checks, players, own_ip)
}

fn finish(role: Role, checks: Vec<Check>, players: Vec<Player>, ip: Option<String>) -> Report {
    let level = checks.iter().map(|c| c.level).max().unwrap_or(Level::Idle).max(Level::Good);
    let worst = checks.iter().find(|c| c.level == level);
    let summary = match (level, worst) {
        (Level::Good, _) => match role {
            Role::Host if players.is_empty() => pick("房间已就绪，等待玩家加入", "Room ready, waiting for players").to_string(),
            Role::Host => tr!("{} 位玩家已连接，一切正常", "{} connected, all good", players.len()),
            Role::Guest => pick("已连上房主，可以进入游戏", "Connected to the host; ready to play").to_string(),
        },
        (Level::Fair, Some(check)) => tr!("可以联机。{}：{}", "Playable. {}: {}", check.label, check.detail),
        (_, Some(check)) => check.detail.clone(),
        _ => String::new(),
    };
    Report { level, summary, checks, players, ip }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Peer> {
        let json = serde_json::json!([
            {"cidr": "10.126.126.1/24", "ipv4": "10.126.126.1", "hostname": "host", "cost": "Local", "lat_ms": "-", "loss_rate": "-", "tunnel_proto": "-"},
            {"cidr": "", "ipv4": "", "hostname": "PublicServer_225284", "cost": "p2p", "lat_ms": "41.50", "loss_rate": "0.0%", "tunnel_proto": "tcp"},
            {"cidr": "10.126.126.2/24", "ipv4": "10.126.126.2", "hostname": "guest-dhcp", "cost": "p2p", "lat_ms": "0.23", "loss_rate": "0.0%", "tunnel_proto": "tcp"},
            {"cidr": "10.126.126.9/24", "ipv4": "10.126.126.9", "hostname": "far", "cost": "relay(2)", "lat_ms": "120.4", "loss_rate": "3.0%", "tunnel_proto": "-"}
        ]);
        peers(&json)
    }

    fn inputs<'a>(role: Role, peers: &'a [Peer], node: &'a Node, game: Option<bool>, seconds: u64) -> Inputs<'a> {
        Inputs { role, elapsed: Duration::from_secs(seconds), exited: None, peers: Some(peers), node: Some(node), game, game_latency_ms: None, game_port: 25565 }
    }

    fn level_of(report: &Report, id: &str) -> Level {
        report.checks.iter().find(|c| c.id == id).map(|c| c.level).unwrap()
    }

    #[test]
    fn reads_the_cli_output() {
        let peers = sample();
        assert_eq!(peers.len(), 4);
        assert!(peers[0].local && !peers[0].public);
        assert!(peers[1].public && peers[1].direct);
        assert_eq!(peers[2].latency_ms, Some(0.23));
        assert_eq!(peers[3].loss_percent, Some(3.0));
        assert!(!peers[3].direct && !peers[3].public);
        assert_eq!(peers[0].latency_ms, None);
        let node = node(&serde_json::json!({"ipv4_addr": "10.126.126.2/24", "stun_info": {"udp_nat_type": 5}}));
        assert_eq!(node, Node { ipv4: "10.126.126.2".into(), nat: 5 });
        assert_eq!(super::node(&serde_json::json!({})), Node::default());
    }

    #[test]
    fn links_are_graded_and_relays_capped() {
        let peers = sample();
        assert_eq!(link(&peers[2], None).0, Level::Good);
        assert_eq!(link(&peers[3], None).0, Level::Fair);
        let relayed_fast = Peer { direct: false, latency_ms: Some(10.0), ..peers[2].clone() };
        assert_eq!(link(&relayed_fast, None).0, Level::Fair);
        let lossy = Peer { loss_percent: Some(25.0), ..peers[2].clone() };
        assert_eq!(link(&lossy, None).0, Level::Poor);
    }

    #[test]
    fn host_report() {
        let node = Node { ipv4: HOST_IP.into(), nat: 5 };
        let peers = sample();
        let report = grade(&inputs(Role::Host, &peers, &node, Some(true), 30));
        // One relayed player keeps the whole room at Fair.
        assert_eq!(report.level, Level::Fair);
        assert_eq!(report.players.len(), 2);
        assert_eq!(level_of(&report, "nodes"), Level::Good);
        assert_eq!(level_of(&report, "nat"), Level::Good);

        let alone = &peers[..2];
        let report = grade(&inputs(Role::Host, alone, &node, Some(true), 30));
        assert_eq!(report.level, Level::Good);
        assert_eq!(level_of(&report, "players"), Level::Idle);
        assert_eq!(report.ip.as_deref(), Some(HOST_IP));

        let report = grade(&inputs(Role::Host, alone, &node, Some(false), 30));
        assert_eq!(report.level, Level::Down);
        assert_eq!(report.summary, report.checks.iter().find(|c| c.id == "game").unwrap().detail);
    }

    #[test]
    fn guest_report() {
        let host = Peer { hostname: "host".into(), ipv4: HOST_IP.into(), local: false, public: false, direct: true, latency_ms: Some(30.0), loss_percent: Some(0.0), tunnel: "udp".into() };
        let me = Peer { hostname: "me".into(), ipv4: "10.126.126.2".into(), local: true, ..host.clone() };
        let shared = sample()[1].clone();
        let node = Node { ipv4: "10.126.126.2".into(), nat: 6 };

        let peers = vec![me.clone(), shared.clone(), host.clone()];
        let report = grade(&inputs(Role::Guest, &peers, &node, Some(true), 40));
        assert_eq!(report.level, Level::Good, "{report:?}");
        assert!(report.players[0].host);

        // Early on, a missing host is still being looked for.
        let peers = vec![me.clone(), shared.clone()];
        assert_eq!(grade(&inputs(Role::Guest, &peers, &node, None, 5)).level, Level::Starting);
        let report = grade(&inputs(Role::Guest, &peers, &node, None, 60));
        assert_eq!(report.level, Level::Down);
        assert_eq!(level_of(&report, "nat"), Level::Fair);

        // A relayed host goes by the game ping, not EasyTier's relay figure.
        let relayed = Peer { direct: false, latency_ms: Some(1000.0), ..host.clone() };
        let peers = vec![me.clone(), shared, relayed];
        let report = grade(&Inputs { game_latency_ms: Some(32), ..inputs(Role::Guest, &peers, &node, Some(true), 40) });
        assert_eq!(report.level, Level::Fair);
        assert_eq!(level_of(&report, "host"), Level::Fair);
        assert_eq!(report.players[0].latency_ms, Some(32.0));
        assert!(report.checks.iter().any(|c| c.id == "host" && c.detail.contains("32 ms")), "{report:?}");
        let report = grade(&inputs(Role::Guest, &peers, &node, Some(true), 40));
        assert_eq!(level_of(&report, "host"), Level::Fair);
        assert_eq!(report.players[0].latency_ms, None);
        assert!(!format!("{report:?}").contains("1000"), "{report:?}");
        let slow = grade(&Inputs { game_latency_ms: Some(450), ..inputs(Role::Guest, &peers, &node, Some(true), 40) });
        assert_eq!(level_of(&slow, "host"), Level::Poor);

        let clash = Node { ipv4: HOST_IP.into(), nat: 3 };
        let peers = vec![me, host];
        let report = grade(&inputs(Role::Guest, &peers, &clash, Some(false), 40));
        assert_eq!(level_of(&report, "ip"), Level::Fair);
        assert_eq!(level_of(&report, "game"), Level::Poor);
        assert_eq!(level_of(&report, "nodes"), Level::Fair);
        assert_eq!(report.level, Level::Poor);
    }

    #[test]
    fn a_stopped_or_silent_core_is_reported() {
        let node = Node::default();
        let stopped = Inputs { exited: Some("invalid network secret"), ..inputs(Role::Guest, &[], &node, None, 3) };
        let report = grade(&stopped);
        assert_eq!(report.level, Level::Down);
        assert!(report.summary.contains("invalid network secret"));

        let silent = Inputs { peers: None, ..inputs(Role::Host, &[], &node, None, 3) };
        assert_eq!(grade(&silent).level, Level::Starting);
        let silent = Inputs { peers: None, ..inputs(Role::Host, &[], &node, None, 30) };
        assert_eq!(grade(&silent).level, Level::Poor);
    }
}
