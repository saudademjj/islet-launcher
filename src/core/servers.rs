//! Multiplayer: the instance's server list (`servers.dat`, the file the
//! game's multiplayer screen edits) and live status over the Server List Ping
//! protocol. Bare host names follow `_minecraft._tcp` SRV records, resolved
//! over DNS-over-HTTPS because the system resolver API has no SRV lookup.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::core::error::{AppError, AppResult};
use crate::core::paths::AppPaths;
use crate::core::util::atomic_write;

use nbt::Tag;

/// A minimal NBT reader and writer. Unknown tags survive a round trip, so
/// editing one server never drops data the game (or a mod) stored there.
pub mod nbt {
    use crate::core::error::{AppError, AppResult};

    #[derive(Debug, Clone, PartialEq)]
    pub enum Tag {
        Byte(i8),
        Short(i16),
        Int(i32),
        Long(i64),
        Float(f32),
        Double(f64),
        ByteArray(Vec<u8>),
        String(String),
        /// Element type id and items; an empty list keeps the type it had.
        List(u8, Vec<Tag>),
        Compound(Vec<(String, Tag)>),
        IntArray(Vec<i32>),
        LongArray(Vec<i64>),
    }

    impl Tag {
        pub fn id(&self) -> u8 {
            match self {
                Tag::Byte(_) => 1,
                Tag::Short(_) => 2,
                Tag::Int(_) => 3,
                Tag::Long(_) => 4,
                Tag::Float(_) => 5,
                Tag::Double(_) => 6,
                Tag::ByteArray(_) => 7,
                Tag::String(_) => 8,
                Tag::List(..) => 9,
                Tag::Compound(_) => 10,
                Tag::IntArray(_) => 11,
                Tag::LongArray(_) => 12,
            }
        }

        pub fn get(&self, key: &str) -> Option<&Tag> {
            match self {
                Tag::Compound(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
                _ => None,
            }
        }

        pub fn as_str(&self) -> Option<&str> {
            match self {
                Tag::String(value) => Some(value),
                _ => None,
            }
        }

        pub fn as_byte(&self) -> Option<i8> {
            match self {
                Tag::Byte(value) => Some(*value),
                _ => None,
            }
        }

        /// Sets a compound entry, keeping its position when it exists.
        pub fn set(&mut self, key: &str, value: Tag) {
            if let Tag::Compound(entries) = self {
                match entries.iter_mut().find(|(k, _)| k == key) {
                    Some(entry) => entry.1 = value,
                    None => entries.push((key.to_string(), value)),
                }
            }
        }

        pub fn remove(&mut self, key: &str) {
            if let Tag::Compound(entries) = self {
                entries.retain(|(k, _)| k != key);
            }
        }
    }

    const MAX_DEPTH: usize = 512;

    fn invalid(detail: &str) -> AppError {
        AppError::detail("nbt", crate::core::i18n::pick("NBT 数据已损坏。", "The NBT data is damaged."), detail)
    }

    struct Reader<'a> {
        data: &'a [u8],
        pos: usize,
    }

    impl<'a> Reader<'a> {
        fn take(&mut self, n: usize) -> AppResult<&'a [u8]> {
            let end = self
                .pos
                .checked_add(n)
                .filter(|end| *end <= self.data.len())
                .ok_or_else(|| invalid(crate::core::i18n::pick("数据提前结束", "data ended early")))?;
            let bytes = &self.data[self.pos..end];
            self.pos = end;
            Ok(bytes)
        }

        fn array<const N: usize>(&mut self) -> AppResult<[u8; N]> {
            Ok(self.take(N)?.try_into().expect("take returns N bytes"))
        }

        fn i32(&mut self) -> AppResult<i32> {
            Ok(i32::from_be_bytes(self.array()?))
        }

        fn i64(&mut self) -> AppResult<i64> {
            Ok(i64::from_be_bytes(self.array()?))
        }

        /// A length prefix, checked against the bytes left so a corrupt
        /// file cannot ask for a huge allocation.
        fn len(&mut self, min_item: usize) -> AppResult<usize> {
            let n = usize::try_from(self.i32()?).map_err(|_| invalid(crate::core::i18n::pick("长度为负", "negative length")))?;
            if n.saturating_mul(min_item) > self.data.len() - self.pos {
                return Err(invalid(crate::core::i18n::pick("长度超出数据范围", "length past the end of the data")));
            }
            Ok(n)
        }

        fn string(&mut self) -> AppResult<String> {
            let n = u16::from_be_bytes(self.array()?) as usize;
            Ok(decode_mutf8(self.take(n)?))
        }

        fn payload(&mut self, id: u8, depth: usize) -> AppResult<Tag> {
            if depth > MAX_DEPTH {
                return Err(invalid(crate::core::i18n::pick("嵌套过深", "nested too deeply")));
            }
            Ok(match id {
                1 => Tag::Byte(self.array::<1>()?[0] as i8),
                2 => Tag::Short(i16::from_be_bytes(self.array()?)),
                3 => Tag::Int(self.i32()?),
                4 => Tag::Long(self.i64()?),
                5 => Tag::Float(f32::from_bits(self.i32()? as u32)),
                6 => Tag::Double(f64::from_bits(self.i64()? as u64)),
                7 => {
                    let n = self.len(1)?;
                    Tag::ByteArray(self.take(n)?.to_vec())
                }
                8 => Tag::String(self.string()?),
                9 => {
                    let kind = self.array::<1>()?[0];
                    let n = self.len(1)?;
                    if kind == 0 && n > 0 {
                        return Err(invalid(crate::core::i18n::pick("列表元素类型无效", "invalid list element type")));
                    }
                    let mut items = Vec::with_capacity(n.min(1024));
                    for _ in 0..n {
                        items.push(self.payload(kind, depth + 1)?);
                    }
                    Tag::List(kind, items)
                }
                10 => {
                    let mut entries = Vec::new();
                    loop {
                        let kind = self.array::<1>()?[0];
                        if kind == 0 {
                            break;
                        }
                        let name = self.string()?;
                        entries.push((name, self.payload(kind, depth + 1)?));
                    }
                    Tag::Compound(entries)
                }
                11 => {
                    let n = self.len(4)?;
                    Tag::IntArray((0..n).map(|_| self.i32()).collect::<AppResult<_>>()?)
                }
                12 => {
                    let n = self.len(8)?;
                    Tag::LongArray((0..n).map(|_| self.i64()).collect::<AppResult<_>>()?)
                }
                _ => return Err(invalid(crate::core::i18n::pick("未知的标签类型", "unknown tag type"))),
            })
        }
    }

    /// Reads an uncompressed file whose root is a named compound.
    pub fn read(data: &[u8]) -> AppResult<(String, Tag)> {
        let mut reader = Reader { data, pos: 0 };
        if reader.array::<1>()?[0] != 10 {
            return Err(invalid(crate::core::i18n::pick("根标签不是复合标签", "the root tag isn't a compound")));
        }
        let name = reader.string()?;
        let root = reader.payload(10, 0)?;
        Ok((name, root))
    }

    pub fn write(name: &str, root: &Tag) -> Vec<u8> {
        let mut out = vec![root.id()];
        put_string(&mut out, name);
        put_payload(&mut out, root);
        out
    }

    fn put_string(out: &mut Vec<u8>, value: &str) {
        let bytes = encode_mutf8(value);
        // Callers keep strings far below the 64 KiB limit; cut rather than
        // write a length that does not match.
        let len = bytes.len().min(u16::MAX as usize);
        out.extend((len as u16).to_be_bytes());
        out.extend(&bytes[..len]);
    }

    fn put_payload(out: &mut Vec<u8>, tag: &Tag) {
        match tag {
            Tag::Byte(v) => out.push(*v as u8),
            Tag::Short(v) => out.extend(v.to_be_bytes()),
            Tag::Int(v) => out.extend(v.to_be_bytes()),
            Tag::Long(v) => out.extend(v.to_be_bytes()),
            Tag::Float(v) => out.extend(v.to_bits().to_be_bytes()),
            Tag::Double(v) => out.extend(v.to_bits().to_be_bytes()),
            Tag::ByteArray(v) => {
                out.extend((v.len() as i32).to_be_bytes());
                out.extend(v);
            }
            Tag::String(v) => put_string(out, v),
            Tag::List(kind, items) => {
                out.push(items.first().map_or(*kind, Tag::id));
                out.extend((items.len() as i32).to_be_bytes());
                for item in items {
                    put_payload(out, item);
                }
            }
            Tag::Compound(entries) => {
                for (key, value) in entries {
                    out.push(value.id());
                    put_string(out, key);
                    put_payload(out, value);
                }
                out.push(0);
            }
            Tag::IntArray(v) => {
                out.extend((v.len() as i32).to_be_bytes());
                for x in v {
                    out.extend(x.to_be_bytes());
                }
            }
            Tag::LongArray(v) => {
                out.extend((v.len() as i32).to_be_bytes());
                for x in v {
                    out.extend(x.to_be_bytes());
                }
            }
        }
    }

    fn continuation(bytes: &[u8], at: usize) -> u32 {
        bytes.get(at).map_or(0, |byte| (byte & 0x3F) as u32)
    }

    /// Java's modified UTF-8 (NUL as two bytes, characters outside the BMP
    /// as surrogate pairs). Plain UTF-8 written by other tools also decodes.
    pub fn decode_mutf8(bytes: &[u8]) -> String {
        if let Ok(text) = std::str::from_utf8(bytes) {
            return text.to_string();
        }
        let mut units: Vec<u16> = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b < 0x80 {
                units.push(b as u16);
                i += 1;
            } else if b & 0xE0 == 0xC0 {
                units.push((((b & 0x1F) as u32) << 6 | continuation(bytes, i + 1)) as u16);
                i += 2;
            } else if b & 0xF0 == 0xE0 {
                let unit = ((b & 0x0F) as u32) << 12
                    | continuation(bytes, i + 1) << 6
                    | continuation(bytes, i + 2);
                units.push(unit as u16);
                i += 3;
            } else if b & 0xF8 == 0xF0 {
                let point = ((b & 0x07) as u32) << 18
                    | continuation(bytes, i + 1) << 12
                    | continuation(bytes, i + 2) << 6
                    | continuation(bytes, i + 3);
                let mut buf = [0u16; 2];
                units.extend_from_slice(char::from_u32(point).unwrap_or('\u{FFFD}').encode_utf16(&mut buf));
                i += 4;
            } else {
                units.push(0xFFFD);
                i += 1;
            }
        }
        String::from_utf16_lossy(&units)
    }

    pub fn encode_mutf8(text: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len());
        for unit in text.encode_utf16() {
            match unit {
                0x01..=0x7F => out.push(unit as u8),
                0x00 | 0x80..=0x7FF => {
                    out.push(0xC0 | (unit >> 6) as u8);
                    out.push(0x80 | (unit & 0x3F) as u8);
                }
                _ => {
                    out.push(0xE0 | (unit >> 12) as u8);
                    out.push(0x80 | ((unit >> 6) & 0x3F) as u8);
                    out.push(0x80 | (unit & 0x3F) as u8);
                }
            }
        }
        out
    }
}

/// One entry of the multiplayer list.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerEntry {
    /// Position in `servers.dat`; edits name it together with the address.
    pub index: usize,
    pub name: String,
    pub address: String,
    /// The icon the game cached from the server, as a data URL.
    pub icon: Option<String>,
}

/// A change to the list. Edits carry the address they expect at `index`, so
/// a list the game rewrote in the meantime is not edited blindly.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ServerEdit {
    Add { name: String, address: String },
    Update { index: usize, expected: String, name: String, address: String },
    Remove { index: usize, expected: String },
    /// Moves an entry one place up (toward the top of the game's list).
    Raise { index: usize, expected: String },
}

const ICON_LIMIT: usize = 96 * 1024;
const DEFAULT_PORT: u16 = 25565;

fn servers_file(paths: &AppPaths, id: &str) -> AppResult<PathBuf> {
    AppPaths::validate_instance_id(id)?;
    Ok(paths.instance_game_dir(id).join("servers.dat"))
}

fn load(path: &Path) -> AppResult<(String, Tag)> {
    match fs::read(path) {
        Ok(bytes) => nbt::read(&bytes).map_err(|error| {
            AppError::detail(
                "servers_corrupt",
                crate::core::i18n::pick("服务器列表文件已损坏，无法读取。", "The server list file is damaged and can't be read."),
                error.detail_text().unwrap_or_default(),
            )
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok((String::new(), Tag::Compound(vec![("servers".into(), Tag::List(10, Vec::new()))])))
        }
        Err(error) => Err(error.into()),
    }
}

fn items(root: &Tag) -> &[Tag] {
    match root.get("servers") {
        Some(Tag::List(_, items)) => items,
        _ => &[],
    }
}

pub fn list(paths: &AppPaths, id: &str) -> AppResult<Vec<ServerEntry>> {
    let (_, root) = load(&servers_file(paths, id)?)?;
    Ok(items(&root)
        .iter()
        .enumerate()
        .filter_map(|(index, server)| {
            // Quick play keeps the servers it opened, hidden from the list.
            if server.get("hidden").and_then(Tag::as_byte).unwrap_or(0) != 0 {
                return None;
            }
            let address = server.get("ip")?.as_str()?.trim().to_string();
            if address.is_empty() {
                return None;
            }
            let icon = server
                .get("icon")
                .and_then(Tag::as_str)
                .filter(|icon| {
                    icon.len() <= ICON_LIMIT
                        && icon.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
                })
                .map(|icon| format!("data:image/png;base64,{icon}"));
            Some(ServerEntry {
                index,
                name: server.get("name").and_then(Tag::as_str).unwrap_or("").trim().to_string(),
                address,
                icon,
            })
        })
        .collect())
}

fn clean(name: &str, address: &str) -> AppResult<(String, String)> {
    let address = address.trim();
    parse_address(address)?;
    let name = name.trim();
    let name = if name.is_empty() { crate::core::i18n::pick("Minecraft 服务器", "Minecraft Server") } else { name };
    Ok((name.chars().take(64).collect(), address.to_string()))
}

fn locate<'a>(items: &'a mut [Tag], index: usize, expected: &str) -> AppResult<&'a mut Tag> {
    items
        .get_mut(index)
        .filter(|server| server.get("ip").and_then(Tag::as_str).map(str::trim) == Some(expected.trim()))
        .ok_or_else(|| AppError::new("servers_changed", crate::core::i18n::pick("服务器列表已经变化，请刷新后再试。", "The server list changed; refresh and try again.")))
}

pub fn edit(paths: &AppPaths, id: &str, change: ServerEdit) -> AppResult<()> {
    let path = servers_file(paths, id)?;
    let (root_name, mut root) = load(&path)?;
    let mut list = items(&root).to_vec();
    match change {
        ServerEdit::Add { name, address } => {
            let (name, address) = clean(&name, &address)?;
            list.push(Tag::Compound(vec![
                ("ip".into(), Tag::String(address)),
                ("name".into(), Tag::String(name)),
            ]));
        }
        ServerEdit::Update { index, expected, name, address } => {
            let (name, address) = clean(&name, &address)?;
            let server = locate(&mut list, index, &expected)?;
            if server.get("ip").and_then(Tag::as_str).map(str::trim) != Some(address.as_str()) {
                // The cached icon belonged to the old address.
                server.remove("icon");
                server.set("ip", Tag::String(address));
            }
            server.set("name", Tag::String(name));
        }
        ServerEdit::Remove { index, expected } => {
            locate(&mut list, index, &expected)?;
            list.remove(index);
        }
        ServerEdit::Raise { index, expected } => {
            locate(&mut list, index, &expected)?;
            if index > 0 {
                list.swap(index - 1, index);
            }
        }
    }
    root.set("servers", Tag::List(10, list));
    let bytes = nbt::write(&root_name, &root);
    // The game keeps the previous list as servers.dat_old; do the same.
    if path.is_file() {
        let _ = fs::copy(&path, path.with_file_name("servers.dat_old"));
    }
    atomic_write(&path, &bytes)
}

/// Host and optional port as players type them: `host`, `host:port`,
/// `[v6]:port`, or a bare IPv6 literal.
pub fn parse_address(address: &str) -> AppResult<(String, Option<u16>)> {
    let bad = || AppError::new("server_address", crate::core::i18n::pick("服务器地址无效，格式如 mc.example.com 或 1.2.3.4:25565。", "Invalid server address; use something like mc.example.com or 1.2.3.4:25565."));
    let address = address.trim();
    let is_v6 = |host: &str| host.parse::<std::net::Ipv6Addr>().is_ok();
    let (host, port) = if let Some(rest) = address.strip_prefix('[') {
        let (host, after) = rest.split_once(']').ok_or_else(bad)?;
        let port = match after {
            "" => None,
            _ => Some(after.strip_prefix(':').ok_or_else(bad)?),
        };
        if !is_v6(host) {
            return Err(bad());
        }
        (host, port)
    } else if address.matches(':').count() > 1 {
        // Several colons only make sense as a bare IPv6 literal.
        if !is_v6(address) {
            return Err(bad());
        }
        (address, None)
    } else if let Some((host, port)) = address.split_once(':') {
        (host, Some(port))
    } else {
        (address, None)
    };
    let port = port
        .map(|port| port.parse::<u16>().ok().filter(|port| *port > 0).ok_or_else(bad))
        .transpose()?;
    // Host names: letters (including other scripts), digits, dots, dashes.
    let valid_name = |host: &str| {
        host.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
            && !host.starts_with('.')
            && !host.contains("..")
    };
    if host.is_empty() || host.len() > 253 || !(is_v6(host) || valid_name(host)) {
        return Err(bad());
    }
    Ok((host.to_string(), port))
}

const DNS_OVER_HTTPS: [&str; 2] = [
    "https://dns.alidns.com/resolve",
    "https://cloudflare-dns.com/dns-query",
];

/// Where to connect: the address's own port, else the SRV record of a host
/// name, else port 25565 — the order the game itself uses.
pub async fn resolve(http: &reqwest::Client, address: &str) -> AppResult<(String, u16)> {
    let (host, port) = parse_address(address)?;
    if let Some(port) = port {
        return Ok((host, port));
    }
    if host.parse::<std::net::IpAddr>().is_ok() || !host.contains('.') {
        return Ok((host, DEFAULT_PORT));
    }
    Ok(lookup_srv(http, &host).await.unwrap_or((host, DEFAULT_PORT)))
}

async fn lookup_srv(http: &reqwest::Client, host: &str) -> Option<(String, u16)> {
    let name = format!("_minecraft._tcp.{host}");
    for endpoint in DNS_OVER_HTTPS {
        let response = http
            .get(endpoint)
            .query(&[("name", name.as_str()), ("type", "SRV")])
            .header("accept", "application/dns-json")
            .timeout(Duration::from_millis(2500))
            .send()
            .await;
        let Ok(body) = (match response {
            Ok(response) => response.json::<Value>().await,
            Err(_) => continue,
        }) else {
            continue;
        };
        // NOERROR and NXDOMAIN are answers; anything else asks the next one.
        match body.get("Status").and_then(Value::as_u64) {
            Some(0 | 3) => return best_srv(&body),
            _ => continue,
        }
    }
    None
}

/// The record to use: lowest priority, then highest weight.
fn best_srv(body: &Value) -> Option<(String, u16)> {
    body.get("Answer")?
        .as_array()?
        .iter()
        .filter(|answer| answer.get("type").and_then(Value::as_u64) == Some(33))
        .filter_map(|answer| {
            let mut parts = answer.get("data")?.as_str()?.split_whitespace();
            let priority: u16 = parts.next()?.parse().ok()?;
            let weight: u16 = parts.next()?.parse().ok()?;
            let port: u16 = parts.next()?.parse().ok()?;
            let target = parts.next()?.trim_end_matches('.');
            (!target.is_empty() && port > 0)
                .then(|| (priority, std::cmp::Reverse(weight), target.to_string(), port))
        })
        .min_by_key(|(priority, weight, ..)| (*priority, *weight))
        .map(|(_, _, target, port)| (target, port))
}

/// A run of MOTD text in one style. Colors are `#rrggbb`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct MotdSpan {
    pub text: String,
    pub color: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underlined: bool,
    pub strikethrough: bool,
    pub obfuscated: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub latency_ms: u32,
    /// The version name the server reports, e.g. "Paper 1.21.1".
    pub version: String,
    pub protocol: i64,
    pub online: i64,
    pub max: i64,
    /// Some of the players online, when the server shares them.
    pub players: Vec<String>,
    pub motd: Vec<MotdSpan>,
    pub favicon: Option<String>,
}

pub async fn ping(http: &reqwest::Client, address: &str) -> AppResult<ServerStatus> {
    let (host, port) = resolve(http, address).await?;
    let (json, latency_ms) = tokio::time::timeout(Duration::from_secs(6), query_status(&host, port))
        .await
        .map_err(|_| AppError::new("server_timeout", crate::core::i18n::pick("服务器没有响应。", "The server didn't respond.")))?
        .map_err(|error| AppError::detail("server_offline", crate::core::i18n::pick("无法连接到服务器。", "Can't reach the server."), error.to_string()))?;
    let value: Value = serde_json::from_str(&json).map_err(|error| {
        AppError::detail("server_status", crate::core::i18n::pick("服务器返回的状态无法识别。", "The server's status reply wasn't understood."), error.to_string())
    })?;
    Ok(status_from_json(&value, latency_ms))
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

fn put_varint(out: &mut Vec<u8>, value: i32) {
    let mut value = value as u32;
    loop {
        if value & !0x7F == 0 {
            out.push(value as u8);
            return;
        }
        out.push((value & 0x7F | 0x80) as u8);
        value >>= 7;
    }
}

fn take_varint(cursor: &mut &[u8]) -> io::Result<i32> {
    let mut value = 0u32;
    for shift in 0..5 {
        let (&byte, rest) = cursor.split_first().ok_or_else(|| invalid_data("truncated varint"))?;
        *cursor = rest;
        value |= ((byte & 0x7F) as u32) << (7 * shift);
        if byte & 0x80 == 0 {
            return Ok(value as i32);
        }
    }
    Err(invalid_data("varint too long"))
}

async fn read_varint(stream: &mut TcpStream) -> io::Result<i32> {
    let mut value = 0u32;
    for shift in 0..5 {
        let byte = stream.read_u8().await?;
        value |= ((byte & 0x7F) as u32) << (7 * shift);
        if byte & 0x80 == 0 {
            return Ok(value as i32);
        }
    }
    Err(invalid_data("varint too long"))
}

fn frame(out: &mut Vec<u8>, packet: &[u8]) {
    put_varint(out, packet.len() as i32);
    out.extend(packet);
}

/// One packet: its id and the bytes after it.
async fn read_packet(stream: &mut TcpStream) -> io::Result<(i32, Vec<u8>)> {
    let len = read_varint(stream).await?;
    if !(1..=2 * 1024 * 1024).contains(&len) {
        return Err(invalid_data("bad packet length"));
    }
    let mut body = vec![0; len as usize];
    stream.read_exact(&mut body).await?;
    let mut cursor = &body[..];
    let id = take_varint(&mut cursor)?;
    Ok((id, cursor.to_vec()))
}

/// Handshake, status request, then ping/pong for the round trip. Servers
/// that close after the status answer get its round trip instead.
async fn query_status(host: &str, port: u16) -> io::Result<(String, u32)> {
    let mut stream = TcpStream::connect((host, port)).await?;
    stream.set_nodelay(true)?;
    let mut handshake = Vec::new();
    put_varint(&mut handshake, 0x00);
    // Protocol -1: "pinging to find out the version", by convention.
    put_varint(&mut handshake, -1);
    put_varint(&mut handshake, host.len() as i32);
    handshake.extend(host.as_bytes());
    handshake.extend(port.to_be_bytes());
    put_varint(&mut handshake, 1);
    let mut out = Vec::new();
    frame(&mut out, &handshake);
    frame(&mut out, &[0x00]);
    let started = Instant::now();
    stream.write_all(&out).await?;
    let (id, body) = read_packet(&mut stream).await?;
    let status_ms = started.elapsed().as_millis() as u32;
    if id != 0x00 {
        return Err(invalid_data("unexpected packet"));
    }
    let mut cursor = &body[..];
    let len = usize::try_from(take_varint(&mut cursor)?).map_err(|_| invalid_data("bad string"))?;
    let json = String::from_utf8_lossy(cursor.get(..len).ok_or_else(|| invalid_data("truncated status"))?)
        .into_owned();

    let mut ping = Vec::new();
    put_varint(&mut ping, 0x01);
    ping.extend(0x4D43_4C41_554E_4348i64.to_be_bytes());
    let mut framed = Vec::new();
    frame(&mut framed, &ping);
    let sent = Instant::now();
    let pong = async {
        stream.write_all(&framed).await?;
        read_packet(&mut stream).await
    };
    let latency = match tokio::time::timeout(Duration::from_secs(2), pong).await {
        Ok(Ok((0x01, _))) => sent.elapsed().as_millis() as u32,
        _ => status_ms,
    };
    Ok((json, latency.max(1)))
}

fn status_from_json(value: &Value, latency_ms: u32) -> ServerStatus {
    let players = value.get("players");
    let count = |key: &str| players.and_then(|p| p.get(key)).and_then(Value::as_i64).unwrap_or(0);
    ServerStatus {
        latency_ms,
        version: value
            .pointer("/version/name")
            .and_then(Value::as_str)
            .map(strip_codes)
            .unwrap_or_default(),
        protocol: value.pointer("/version/protocol").and_then(Value::as_i64).unwrap_or(-1),
        online: count("online"),
        max: count("max"),
        players: players
            .and_then(|p| p.get("sample"))
            .and_then(Value::as_array)
            .map(|sample| {
                sample
                    .iter()
                    .filter_map(|player| player.get("name")?.as_str())
                    .map(strip_codes)
                    .filter(|name| !name.trim().is_empty())
                    .take(12)
                    .collect()
            })
            .unwrap_or_default(),
        motd: value.get("description").map(motd).unwrap_or_default(),
        favicon: value
            .get("favicon")
            .and_then(Value::as_str)
            .map(|icon| icon.replace(['\n', '\r'], ""))
            .filter(|icon| icon.starts_with("data:image/png;base64,") && icon.len() <= 256 * 1024),
    }
}

/// The 16 chat colors, by legacy code (`§0`–`§f`) and by name.
const COLORS: [(&str, &str); 16] = [
    ("black", "#000000"),
    ("dark_blue", "#0000aa"),
    ("dark_green", "#00aa00"),
    ("dark_aqua", "#00aaaa"),
    ("dark_red", "#aa0000"),
    ("dark_purple", "#aa00aa"),
    ("gold", "#ffaa00"),
    ("gray", "#aaaaaa"),
    ("dark_gray", "#555555"),
    ("blue", "#5555ff"),
    ("green", "#55ff55"),
    ("aqua", "#55ffff"),
    ("red", "#ff5555"),
    ("light_purple", "#ff55ff"),
    ("yellow", "#ffff55"),
    ("white", "#ffffff"),
];

pub fn strip_codes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Style {
    color: Option<String>,
    bold: bool,
    italic: bool,
    underlined: bool,
    strikethrough: bool,
    obfuscated: bool,
}

fn style_of(value: &Value, parent: &Style) -> Style {
    let mut style = parent.clone();
    let Value::Object(map) = value else {
        return style;
    };
    if let Some(color) = map.get("color").and_then(Value::as_str) {
        style.color = if color.starts_with('#') && color.len() == 7 && color[1..].chars().all(|c| c.is_ascii_hexdigit()) {
            Some(color.to_ascii_lowercase())
        } else {
            COLORS.iter().find(|(name, _)| *name == color).map(|(_, hex)| hex.to_string())
        };
    }
    let flag = |key: &str, current: bool| map.get(key).and_then(Value::as_bool).unwrap_or(current);
    style.bold = flag("bold", style.bold);
    style.italic = flag("italic", style.italic);
    style.underlined = flag("underlined", style.underlined);
    style.strikethrough = flag("strikethrough", style.strikethrough);
    style.obfuscated = flag("obfuscated", style.obfuscated);
    style
}

fn push_span(out: &mut Vec<MotdSpan>, text: &mut String, style: &Style) {
    if text.is_empty() {
        return;
    }
    let text = std::mem::take(text);
    if let Some(last) = out.last_mut()
        && last.color == style.color
        && last.bold == style.bold
        && last.italic == style.italic
        && last.underlined == style.underlined
        && last.strikethrough == style.strikethrough
        && last.obfuscated == style.obfuscated
    {
        last.text.push_str(&text);
        return;
    }
    out.push(MotdSpan {
        text,
        color: style.color.clone(),
        bold: style.bold,
        italic: style.italic,
        underlined: style.underlined,
        strikethrough: style.strikethrough,
        obfuscated: style.obfuscated,
    });
}

/// Text that may still carry `§` codes, which servers mix into JSON text.
fn legacy_text(text: &str, base: &Style, out: &mut Vec<MotdSpan>) {
    let mut style = base.clone();
    let mut buffer = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '§' {
            buffer.push(c);
            continue;
        }
        let Some(code) = chars.next() else { break };
        push_span(out, &mut buffer, &style);
        match code.to_ascii_lowercase() {
            digit @ ('0'..='9' | 'a'..='f') => {
                // A color code also clears bold, italic and the rest.
                let index = digit.to_digit(16).unwrap_or(15) as usize;
                style = Style { color: Some(COLORS[index].1.to_string()), ..Style::default() };
            }
            'k' => style.obfuscated = true,
            'l' => style.bold = true,
            'm' => style.strikethrough = true,
            'n' => style.underlined = true,
            'o' => style.italic = true,
            'r' => style = base.clone(),
            _ => {}
        }
    }
    push_span(out, &mut buffer, &style);
}

fn component(value: &Value, parent: &Style, out: &mut Vec<MotdSpan>, depth: usize) {
    if depth > 16 {
        return;
    }
    match value {
        Value::String(text) => legacy_text(text, parent, out),
        Value::Number(number) => legacy_text(&number.to_string(), parent, out),
        Value::Array(items) => {
            // In a text array the first element styles the rest.
            let Some((first, rest)) = items.split_first() else { return };
            let base = style_of(first, parent);
            component(first, parent, out, depth + 1);
            for item in rest {
                component(item, &base, out, depth + 1);
            }
        }
        Value::Object(map) => {
            let style = style_of(value, parent);
            let text = map
                .get("text")
                .and_then(Value::as_str)
                .or_else(|| map.get("translate").and_then(Value::as_str))
                .unwrap_or("");
            legacy_text(text, &style, out);
            if let Some(Value::Array(extra)) = map.get("extra") {
                for item in extra {
                    component(item, &style, out, depth + 1);
                }
            }
        }
        _ => {}
    }
}

/// The description as styled runs: at most two lines, like the game's list.
pub fn motd(value: &Value) -> Vec<MotdSpan> {
    let mut spans = Vec::new();
    component(value, &Style::default(), &mut spans, 0);
    let mut lines = 0;
    let mut budget = 240usize;
    let mut result: Vec<MotdSpan> = Vec::new();
    for mut span in spans {
        let mut kept = String::new();
        for c in span.text.chars() {
            if c == '\n' {
                lines += 1;
                if lines >= 2 {
                    break;
                }
            }
            if budget == 0 {
                break;
            }
            budget -= 1;
            kept.push(c);
        }
        let stop = lines >= 2 || budget == 0;
        span.text = kept;
        if !span.text.is_empty() {
            result.push(span);
        }
        if stop {
            break;
        }
    }
    // Leading spaces center each line in the game's fixed-width list; the
    // launcher lays text out itself, so drop them. Trailing ones and an
    // empty last line only add height.
    let mut line_start = true;
    for span in &mut result {
        let mut text = String::with_capacity(span.text.len());
        for c in span.text.chars() {
            if line_start && c == ' ' {
                continue;
            }
            line_start = c == '\n';
            text.push(c);
        }
        span.text = text;
    }
    result.retain(|span| !span.text.is_empty());
    while let Some(last) = result.last_mut() {
        let trimmed = last.text.trim_end().len();
        last.text.truncate(trimmed);
        if last.text.is_empty() {
            result.pop();
        } else {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths() -> (tempfile::TempDir, AppPaths, String) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        fs::create_dir_all(paths.instance_game_dir(&id)).unwrap();
        (dir, paths, id)
    }

    #[test]
    fn nbt_round_trips_every_tag_and_modified_utf8() {
        let root = Tag::Compound(vec![
            ("b".into(), Tag::Byte(-3)),
            ("s".into(), Tag::Short(300)),
            ("i".into(), Tag::Int(-70000)),
            ("l".into(), Tag::Long(1 << 40)),
            ("f".into(), Tag::Float(1.5)),
            ("d".into(), Tag::Double(-2.25)),
            ("ba".into(), Tag::ByteArray(vec![1, 2, 255])),
            ("str".into(), Tag::String("服务器 🎮 \u{0}end".into())),
            ("empty".into(), Tag::List(8, vec![])),
            ("list".into(), Tag::List(3, vec![Tag::Int(1), Tag::Int(2)])),
            ("ia".into(), Tag::IntArray(vec![7, -7])),
            ("la".into(), Tag::LongArray(vec![9])),
            ("nested".into(), Tag::Compound(vec![("x".into(), Tag::String("y".into()))])),
        ]);
        let bytes = nbt::write("", &root);
        // Java writes NUL as C0 80 and the emoji as a surrogate pair.
        assert!(bytes.windows(2).any(|pair| pair == [0xC0, 0x80]));
        assert!(bytes.windows(2).any(|pair| pair == [0xED, 0xA0]));
        assert_eq!(nbt::read(&bytes).unwrap(), (String::new(), root));
        // Truncated or hostile lengths fail instead of allocating.
        assert!(nbt::read(&bytes[..bytes.len() - 3]).is_err());
        assert!(nbt::read(&[10, 0, 0, 7, 0, 1, b'a', 0x7f, 0xff, 0xff, 0xff]).is_err());
    }

    #[test]
    fn server_list_edits_keep_unknown_fields_and_check_the_target() {
        let (_dir, paths, id) = temp_paths();
        assert!(list(&paths, &id).unwrap().is_empty());
        edit(&paths, &id, ServerEdit::Add { name: " 生存服 ".into(), address: "play.example.com".into() }).unwrap();
        edit(&paths, &id, ServerEdit::Add { name: "".into(), address: "10.0.0.2:25570".into() }).unwrap();

        // Fields the game adds (icon, acceptTextures) and hidden entries stay.
        let file = paths.instance_game_dir(&id).join("servers.dat");
        let (name, mut root) = nbt::read(&fs::read(&file).unwrap()).unwrap();
        let Some(Tag::List(_, items)) = root.get("servers").cloned() else { panic!() };
        let mut items = items;
        items[0].set("icon", Tag::String("aWNvbg==".into()));
        items[0].set("acceptTextures", Tag::Byte(1));
        items.push(Tag::Compound(vec![
            ("ip".into(), Tag::String("hidden.example.com".into())),
            ("name".into(), Tag::String("quick".into())),
            ("hidden".into(), Tag::Byte(1)),
        ]));
        root.set("servers", Tag::List(10, items));
        fs::write(&file, nbt::write(&name, &root)).unwrap();

        let servers = list(&paths, &id).unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "生存服");
        assert_eq!(servers[0].icon.as_deref(), Some("data:image/png;base64,aWNvbg=="));
        assert_eq!(servers[1].name, "Minecraft 服务器");

        // Renaming keeps the icon; a new address drops it.
        edit(&paths, &id, ServerEdit::Update { index: 0, expected: "play.example.com".into(), name: "新名字".into(), address: "play.example.com".into() }).unwrap();
        assert!(list(&paths, &id).unwrap()[0].icon.is_some());
        edit(&paths, &id, ServerEdit::Update { index: 0, expected: "play.example.com".into(), name: "新名字".into(), address: "mc.example.org".into() }).unwrap();
        assert!(list(&paths, &id).unwrap()[0].icon.is_none());

        // A stale index is refused rather than editing the wrong entry.
        let stale = edit(&paths, &id, ServerEdit::Remove { index: 0, expected: "play.example.com".into() });
        assert_eq!(stale.unwrap_err().code(), "servers_changed");

        edit(&paths, &id, ServerEdit::Raise { index: 1, expected: "10.0.0.2:25570".into() }).unwrap();
        edit(&paths, &id, ServerEdit::Remove { index: 1, expected: "mc.example.org".into() }).unwrap();
        let servers = list(&paths, &id).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].address, "10.0.0.2:25570");
        let (_, root) = nbt::read(&fs::read(&file).unwrap()).unwrap();
        assert_eq!(items_len(&root), 2, "the hidden quick-play entry is kept");
        assert!(file.with_file_name("servers.dat_old").is_file());
    }

    fn items_len(root: &Tag) -> usize {
        items(root).len()
    }

    #[test]
    fn addresses_parse_like_the_game() {
        assert_eq!(parse_address("mc.example.com").unwrap(), ("mc.example.com".into(), None));
        assert_eq!(parse_address(" 1.2.3.4:25570 ").unwrap(), ("1.2.3.4".into(), Some(25570)));
        assert_eq!(parse_address("[::1]:25566").unwrap(), ("::1".into(), Some(25566)));
        assert_eq!(parse_address("fe80::1").unwrap(), ("fe80::1".into(), None));
        assert_eq!(parse_address("我的世界.中国").unwrap(), ("我的世界.中国".into(), None));
        for bad in ["", "host:0", "host:99999", "has space", "a/b", "[::1", "a.com:25570a.com:25570", "[host]:1", "x..y", "-p.com;"] {
            assert!(parse_address(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn srv_picks_the_lowest_priority_then_the_highest_weight() {
        let body = serde_json::json!({"Status": 0, "Answer": [
            {"type": 5, "data": "alias.example.com."},
            {"type": 33, "data": "20 100 25570 backup.example.com."},
            {"type": 33, "data": "10 1 25571 light.example.com."},
            {"type": 33, "data": "10 50 25572 main.example.com."}
        ]});
        assert_eq!(best_srv(&body), Some(("main.example.com".into(), 25572)));
        assert_eq!(best_srv(&serde_json::json!({"Status": 3})), None);
    }

    #[test]
    fn motd_reads_json_text_and_legacy_codes() {
        let json = serde_json::json!({
            "text": "",
            "extra": [
                {"text": "Hy", "color": "gold", "bold": true},
                {"text": "pixel", "color": "#33CCFF"},
                "\n§aGreen §lbold§r plain"
            ]
        });
        let spans = motd(&json);
        assert_eq!(spans[0], MotdSpan { text: "Hy".into(), color: Some("#ffaa00".into()), bold: true, ..Default::default() });
        assert_eq!(spans[1].color.as_deref(), Some("#33ccff"));
        assert_eq!(spans[2].text, "\n");
        assert_eq!(spans[3], MotdSpan { text: "Green ".into(), color: Some("#55ff55".into()), ..Default::default() });
        assert!(spans[4].bold && spans[4].color.as_deref() == Some("#55ff55"));
        assert_eq!(spans[5], MotdSpan { text: " plain".into(), ..Default::default() });

        // Centering spaces at the start of each line go.
        let spans = motd(&serde_json::json!("   §6Hy\n    second"));
        assert_eq!(spans.iter().map(|s| s.text.as_str()).collect::<String>(), "Hy\nsecond");

        // Arrays style the rest after the first; a third line is dropped.
        let spans = motd(&serde_json::json!([{"text": "a", "color": "red"}, "b\nc\nd"]));
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "ab\nc");
        assert_eq!(strip_codes("§6Paper §r1.21"), "Paper 1.21");
    }

    #[test]
    fn status_json_maps_players_version_and_icon() {
        let value = serde_json::json!({
            "version": {"name": "§6Paper 1.21.1", "protocol": 767},
            "players": {"max": 100, "online": 3, "sample": [{"name": "Alex", "id": "0"}, {"name": "§7", "id": "1"}]},
            "description": "欢迎",
            "favicon": "data:image/png;base64,AAAA\n"
        });
        let status = status_from_json(&value, 42);
        assert_eq!(status.version, "Paper 1.21.1");
        assert_eq!((status.online, status.max, status.protocol), (3, 100, 767));
        assert_eq!(status.players, vec!["Alex".to_string()]);
        assert_eq!(status.motd[0].text, "欢迎");
        assert_eq!(status.favicon.as_deref(), Some("data:image/png;base64,AAAA"));
    }

    /// A local server that speaks the status protocol answers a real ping.
    #[tokio::test]
    async fn ping_talks_the_status_protocol() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (id, handshake) = read_packet(&mut socket).await.unwrap();
            assert_eq!(id, 0);
            assert_eq!(*handshake.last().unwrap(), 1, "next state is status");
            let (id, _) = read_packet(&mut socket).await.unwrap();
            assert_eq!(id, 0);
            let json = br#"{"version":{"name":"1.21.1","protocol":767},"players":{"max":20,"online":2},"description":{"text":"hi"}}"#;
            let mut packet = Vec::new();
            put_varint(&mut packet, 0);
            put_varint(&mut packet, json.len() as i32);
            packet.extend(json);
            let mut out = Vec::new();
            frame(&mut out, &packet);
            socket.write_all(&out).await.unwrap();
            let (id, payload) = read_packet(&mut socket).await.unwrap();
            assert_eq!(id, 1);
            let mut pong = vec![1];
            pong.extend(payload);
            let mut out = Vec::new();
            frame(&mut out, &pong);
            socket.write_all(&out).await.unwrap();
        });
        let http = reqwest::Client::new();
        let status = ping(&http, &format!("127.0.0.1:{port}")).await.unwrap();
        assert_eq!((status.online, status.max), (2, 20));
        assert_eq!(status.version, "1.21.1");
        assert_eq!(status.motd[0].text, "hi");
        assert!(status.latency_ms >= 1);
    }
}

#[cfg(test)]
mod network_tests {
    /// Real servers over the internet; run by hand with `--ignored`.
    #[tokio::test]
    #[ignore]
    async fn ping_real_servers() {
        let http = reqwest::Client::new();
        println!("srv: {:?}", super::resolve(&http, "2b2t.org").await);
        for address in ["2b2t.org", "mc.hypixel.net"] {
            match super::ping(&http, address).await {
                Ok(status) => println!(
                    "{address}: {} ms, {}/{} online, {} ({}), motd {:?}, icon {}",
                    status.latency_ms,
                    status.online,
                    status.max,
                    status.version,
                    status.protocol,
                    status.motd.iter().map(|s| s.text.as_str()).collect::<String>(),
                    status.favicon.is_some()
                ),
                Err(error) => println!("{address}: {error:?}"),
            }
        }
    }
}
