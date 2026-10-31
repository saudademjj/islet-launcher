//! Reverse TCP forwarding through the user's own SSH host and existing key.
//! SSH verifies known_hosts and never invokes a shell with interpolated text.
use islet::core::error::{AppError, ErrorPayload};
use serde::Serialize;
use std::{
    collections::HashMap,
    process::{Child, Stdio},
    sync::Mutex,
};
use tauri::{Manager, State};
#[derive(Default)]
pub struct Tunnels(Mutex<HashMap<String, Running>>);
impl Tunnels {
    pub fn has_activity(&self) -> bool {
        self.0.try_lock().map_or(true, |map| !map.is_empty())
    }

    /// Kills every forwarding ssh; called when the launcher exits.
    pub fn stop_all(&self) {
        if let Ok(mut map) = self.0.lock() {
            map.clear();
        }
    }
}
struct Running {
    child: Child,
    address: String,
    log: std::path::PathBuf,
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    running: bool,
    address: String,
    detail: String,
}
fn fail(text: &str) -> ErrorPayload {
    AppError::new("tunnel", text).to_payload()
}
fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() < 254
        && !host.starts_with('-')
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
}
#[tauri::command]
pub fn start_tunnel(
    app: tauri::AppHandle,
    state: State<'_, Tunnels>,
    id: String,
    host: String,
    user: String,
    local_port: u16,
    remote_port: u16,
    ssh_port: u16,
) -> Result<Status, ErrorPayload> {
    if !valid_host(&host)
        || user.is_empty()
        || !user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        || user.starts_with('-')
        || [local_port, remote_port, ssh_port].contains(&0)
    {
        return Err(fail(&islet::tr!("请填写有效的 SSH 主机、用户和端口。", "Enter a valid SSH host, user and ports.")));
    }
    let services = app.state::<crate::LauncherState>().services();
    services.instances.get(&id).map_err(|e| e.to_payload())?;
    let mut running = state.0.lock().map_err(|_| fail(&islet::tr!("隧道状态被锁定", "Tunnel state locked")))?;
    if let Some(existing) = running.get_mut(&id) {
        if existing
            .child
            .try_wait()
            .map_err(|e| fail(&e.to_string()))?
            .is_none()
        {
            return Err(fail(&islet::tr!("这个世界已有联机隧道，请先停止。", "This world already has a tunnel; stop it first.")));
        }
        running.remove(&id);
    }
    let log = services.paths.cache.join(format!("tunnel-{id}.log"));
    let output = std::fs::File::create(&log).map_err(|e| fail(&e.to_string()))?;
    let mut cmd = islet::core::platform::quiet_command("ssh");
    let child = cmd
        .args([
            "-N",
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ConnectTimeout=15",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "-p",
            &ssh_port.to_string(),
            "-R",
            &format!("0.0.0.0:{remote_port}:127.0.0.1:{local_port}"),
            &format!("{user}@{host}"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(output)
        .spawn()
        .map_err(|e| fail(&islet::tr!("无法启动系统 SSH：{e}", "Couldn't start the system SSH: {e}")))?;
    let address = format!("{host}:{remote_port}");
    running.insert(
        id,
        Running {
            child,
            address: address.clone(),
            log,
        },
    );
    Ok(Status {
        running: true,
        address,
        detail: islet::tr!(
            "正在连接；SSH 主机需允许 GatewayPorts clientspecified，且开放转发端口。",
            "Connecting; the SSH host must allow GatewayPorts clientspecified and open the forwarded port."
        ),
    })
}
#[tauri::command]
pub fn tunnel_status(state: State<'_, Tunnels>, id: String) -> Result<Status, ErrorPayload> {
    let mut map = state.0.lock().map_err(|_| fail(&islet::tr!("隧道状态被锁定", "Tunnel state locked")))?;
    let Some(tunnel) = map.get_mut(&id) else {
        return Ok(Status {
            running: false,
            address: String::new(),
            detail: String::new(),
        });
    };
    let running = tunnel
        .child
        .try_wait()
        .map_err(|e| fail(&e.to_string()))?
        .is_none();
    let detail = std::fs::read_to_string(&tunnel.log)
        .unwrap_or_default()
        .chars()
        .rev()
        .take(4000)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let status = Status {
        running,
        address: tunnel.address.clone(),
        detail,
    };
    Ok(status)
}
#[tauri::command]
pub fn stop_tunnel(state: State<'_, Tunnels>, id: String) -> Result<(), ErrorPayload> {
    state
        .0
        .lock()
        .map_err(|_| fail(&islet::tr!("隧道状态被锁定", "Tunnel state locked")))?
        .remove(&id);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_ssh_options_and_shell_input() {
        for bad in ["-oProxyCommand=evil", "a;whoami", "user@host", "a/b", "a b"] {
            assert!(!valid_host(bad));
        }
        assert!(valid_host("relay.example.org"));
    }
}
