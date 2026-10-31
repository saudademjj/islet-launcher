//! Discord local RPC. Opt-in; no account tokens or Internet connections.
use std::{
    io::{Read, Write},
    sync::{Mutex, OnceLock, mpsc},
    time::Duration,
};
type Activity = (String, Option<(String, String)>);
static CHANNEL: OnceLock<Mutex<mpsc::Sender<Activity>>> = OnceLock::new();
pub fn set(id: &str, playing: Option<(&str, &str)>) {
    let sender = CHANNEL.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Activity>();
        std::thread::Builder::new()
            .name("discord-presence".into())
            .spawn(move || {
                let mut activity: Activity = (String::new(), None);
                let mut connection: Option<Connection> = None;
                loop {
                    match rx.recv_timeout(Duration::from_secs(15)) {
                        Ok(next) => {
                            if activity.0 != next.0 {
                                connection = None;
                            }
                            activity = next;
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(_) => {}
                    }
                    if activity.0.len() < 17 || !activity.0.bytes().all(|b| b.is_ascii_digit()) {
                        connection = None;
                        continue;
                    }
                    if connection.is_none() {
                        match connect(&activity.0) {
                            Ok(stream) => connection = Some(stream),
                            Err(error) => {
                                tracing::debug!(%error, "Discord unavailable");
                                continue;
                            }
                        }
                    }
                    if let Some(stream) = &mut connection {
                        if update(stream, &activity).is_err() {
                            connection = None;
                        }
                    }
                }
            })
            .ok();
        Mutex::new(tx)
    });
    if let Ok(tx) = sender.lock() {
        let _ = tx.send((id.into(), playing.map(|(n, v)| (n.into(), v.into()))));
    }
}
fn frame(stream: &mut impl Write, op: u32, value: serde_json::Value) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(&value)?;
    stream.write_all(&op.to_le_bytes())?;
    stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stream.write_all(&bytes)
}
#[cfg(unix)]
type Connection = std::os::unix::net::UnixStream;
#[cfg(windows)]
type Connection = std::fs::File;
fn connect(id: &str) -> std::io::Result<Connection> {
    #[cfg(unix)]
    let mut stream = {
        let mut found = None;
        let roots = [
            std::env::var_os("XDG_RUNTIME_DIR"),
            std::env::var_os("TMPDIR"),
            Some("/tmp".into()),
        ];
        for root in roots.into_iter().flatten() {
            for i in 0..10 {
                if let Ok(s) = std::os::unix::net::UnixStream::connect(
                    std::path::PathBuf::from(&root).join(format!("discord-ipc-{i}")),
                ) {
                    found = Some(s);
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        let s = found.ok_or_else(|| std::io::Error::other("Discord IPC not found"))?;
        s.set_read_timeout(Some(Duration::from_secs(2)))?;
        s.set_write_timeout(Some(Duration::from_secs(2)))?;
        s
    };
    #[cfg(windows)]
    let mut stream = (0..10)
        .find_map(|i| {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(format!(r"\\.\pipe\discord-ipc-{i}"))
                .ok()
        })
        .ok_or_else(|| std::io::Error::other("Discord IPC not found"))?;
    frame(
        &mut stream,
        0,
        serde_json::json!({ "v": 1, "client_id": id }),
    )?;
    acknowledge(&mut stream)?;
    Ok(stream)
}
fn acknowledge(stream: &mut impl Read) -> std::io::Result<()> {
    let mut header = [0; 8];
    stream.read_exact(&mut header)?;
    let length = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
    if length > 65536 {
        return Err(std::io::Error::other("Discord frame too large"));
    }
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload)?;
    let value: serde_json::Value = serde_json::from_slice(&payload)?;
    if value["evt"].as_str() == Some("ERROR") {
        return Err(std::io::Error::other("Discord rejected activity"));
    }
    Ok(())
}
fn update(stream: &mut Connection, activity: &Activity) -> std::io::Result<()> {
    let payload = activity.1.as_ref().map(|(name, version)| serde_json::json!({ "details": name.chars().take(100).collect::<String>(), "state": format!("Minecraft {version}"), "instance": false }));
    frame(
        stream,
        1,
        serde_json::json!({ "cmd": "SET_ACTIVITY", "args": { "pid": std::process::id(), "activity": payload }, "nonce": format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()) }),
    )?;
    acknowledge(stream)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rpc_frames_use_lengths_and_reject_oversize_responses() {
        let mut encoded = vec![];
        frame(&mut encoded, 1, serde_json::json!({"test":"汉字"})).unwrap();
        assert_eq!(
            u32::from_le_bytes(encoded[4..8].try_into().unwrap()) as usize,
            encoded.len() - 8
        );
        let mut invalid = vec![0; 8];
        invalid[4..8].copy_from_slice(&70000u32.to_le_bytes());
        assert!(acknowledge(&mut invalid.as_slice()).is_err());
    }
}
