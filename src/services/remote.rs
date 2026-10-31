//! A world's server on the player's own Linux host: connecting, uploading,
//! and keeping it running. See `core::remote` for the pieces.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::{AppServices, UiEvent, lock};
use crate::core::error::{AppError, AppResult};
use crate::core::i18n::pick;
use crate::core::remote::{self, Probe, Session, State, Target};
use crate::core::servers::ServerStatus;

/// What the dialog needs to show a world's server.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    pub target: Option<Target>,
    pub has_secret: bool,
    /// A folder on the host for a new server.
    pub default_dir: String,
    /// The pack friends import, from the last upload.
    pub pack: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeployOptions {
    pub accept_eula: bool,
    /// A save that becomes the server's world on the first upload.
    pub world: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    pub state: State,
    pub address: String,
    /// The server as friends see it, when it answers.
    pub ping: Option<ServerStatus>,
}

fn secret_key(id: &str) -> String {
    format!("ssh:{id}")
}

fn not_set_up() -> AppError {
    AppError::new("remote_missing", pick("这个世界还没有连接云服务器。", "This world has no server connected yet."))
}

/// The folder the friends' pack is kept in.
fn pack_dir(services: &AppServices, id: &str) -> PathBuf {
    services.paths.instance_dir(id).join("server-pack")
}

impl AppServices {
    fn remote_secret(&self, id: &str) -> Option<String> {
        if let Some(secret) = lock(&self.remote_secrets).get(id) {
            return Some(secret.clone());
        }
        crate::core::keychain::load_secret(&secret_key(id))
    }

    /// Keeps the password for this session, and in the system keychain when
    /// there is one.
    fn set_remote_secret(&self, id: &str, secret: &str) {
        lock(&self.remote_secrets).insert(id.to_string(), secret.to_string());
        if let Err(error) = crate::core::keychain::save_secret(&secret_key(id), secret) {
            tracing::warn!(%error, "the server password stays in memory only");
        }
    }

    pub fn remote_info(&self, id: &str) -> AppResult<RemoteInfo> {
        let instance = self.instances.get(id)?;
        let pack = std::fs::read_dir(pack_dir(self, id))
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|e| e == "mrpack"))
            .map(|path| path.display().to_string());
        Ok(RemoteInfo {
            target: remote::load(&self.paths, id),
            has_secret: self.remote_secret(id).is_some(),
            default_dir: remote::default_dir(&instance.name, id),
            pack,
        })
    }

    async fn remote_session(&self, id: &str) -> AppResult<(Target, Session)> {
        let target = remote::load(&self.paths, id).ok_or_else(not_set_up)?;
        let session = Session::connect(&target, self.remote_secret(id).as_deref()).await?;
        Ok((target, session))
    }

    /// Connects with these settings and looks around the host. On success
    /// the settings, the host key and the password are kept for the world.
    pub async fn remote_connect(&self, id: &str, mut target: Target, secret: Option<String>) -> AppResult<Probe> {
        self.instances.get(id)?;
        let previous = remote::load(&self.paths, id);
        // Changing the host or port means a new host key.
        if previous.as_ref().is_some_and(|p| p.host != target.host || p.port != target.port) {
            target.fingerprint = None;
        }
        target.deployed_at = previous.as_ref().and_then(|p| p.deployed_at).filter(|_| previous.as_ref().is_some_and(|p| p.host == target.host && p.dir == target.dir));
        let secret = secret.filter(|s| !s.is_empty()).or_else(|| self.remote_secret(id));
        let session = Session::connect(&target, secret.as_deref()).await?;
        let probe = remote::parse_probe(&session.run_ok(&remote::probe_script(&target.dir), "无法读取服务器信息。", "Couldn't look around the server.").await?);
        target.fingerprint = Some(session.fingerprint.clone());
        session.close().await;
        if probe.os != "Linux" {
            return Err(AppError::detail("remote", pick("目前只支持 Linux 服务器。", "Only Linux servers are supported for now."), probe.os));
        }
        remote::save(&self.paths, id, &target)?;
        if let Some(secret) = &secret {
            self.set_remote_secret(id, secret);
        }
        Ok(probe)
    }

    /// Changes how a deployed server is kept (memory, port, schedule…)
    /// without uploading it again.
    pub async fn remote_settings(&self, id: &str, target: Target) -> AppResult<()> {
        let mut saved = remote::load(&self.paths, id).ok_or_else(not_set_up)?;
        saved.memory_mb = target.memory_mb;
        saved.game_port = target.game_port;
        saved.online_mode = target.online_mode;
        saved.address = target.address;
        saved.autostart = target.autostart;
        saved.daily_backup = target.daily_backup;
        saved.keep_backups = target.keep_backups;
        saved.validate()?;
        if saved.deployed_at.is_some() {
            let session = Session::connect(&saved, self.remote_secret(id).as_deref()).await?;
            let result = session.run_ok(&remote::configure_script(&saved), "改设置失败。", "Couldn't change the settings.").await;
            session.close().await;
            result?;
        }
        remote::save(&self.paths, id, &saved)
    }

    pub fn remote_forget(&self, id: &str) -> AppResult<()> {
        remote::forget(&self.paths, id)?;
        lock(&self.remote_secrets).remove(id);
        crate::core::keychain::delete_secret(&secret_key(id));
        Ok(())
    }

    pub async fn remote_status(&self, id: &str) -> AppResult<RemoteStatus> {
        let (target, session) = self.remote_session(id).await?;
        let text = session.run_ok(&format!("{}&& sh islet.sh status", remote::cd(&target.dir)), "读取服务器状态失败。", "Couldn't read the server's state.").await;
        session.close().await;
        let state = remote::parse_state(&text?);
        let address = target.public_address();
        let ping = if state.running { crate::core::servers::ping(&self.http()?, &address).await.ok() } else { None };
        Ok(RemoteStatus { state, address, ping })
    }

    /// start, stop, restart or backup.
    pub async fn remote_action(&self, id: &str, action: &str) -> AppResult<String> {
        if !matches!(action, "start" | "stop" | "restart" | "backup") {
            return Err(AppError::new("remote", pick("未知操作。", "Unknown action.")));
        }
        let (target, session) = self.remote_session(id).await?;
        let output = session.run_ok(&format!("{}&& sh islet.sh {action}", remote::cd(&target.dir)), "服务器没有完成这个操作。", "The server couldn't do that.").await;
        session.close().await;
        Ok(output?.trim().to_string())
    }

    /// Types a line into the server console.
    pub async fn remote_command(&self, id: &str, text: &str) -> AppResult<()> {
        let text = text.trim().trim_start_matches('/');
        if text.is_empty() || text.len() > 1000 || text.contains(['\n', '\r']) {
            return Err(AppError::new("remote", pick("命令要写在一行里。", "Commands go on one line.")));
        }
        let (target, session) = self.remote_session(id).await?;
        let result = session.run_ok(&format!("{}&& sh islet.sh cmd {}", remote::cd(&target.dir), remote::quote(text)), "命令没有发出去。", "The command wasn't sent.").await;
        session.close().await;
        result.map(|_| ())
    }

    pub async fn remote_log(&self, id: &str, lines: u32) -> AppResult<String> {
        let (target, session) = self.remote_session(id).await?;
        let result = session.run_ok(&format!("{}&& sh islet.sh log {}", remote::cd(&target.dir), lines.clamp(10, 2000)), "读取日志失败。", "Couldn't read the log.").await;
        session.close().await;
        result
    }

    /// Copies a backup from the host to `destination`.
    pub async fn remote_download_backup(&self, id: &str, name: &str, destination: &Path) -> AppResult<()> {
        let name = remote::backup_name(name)?;
        let (target, session) = self.remote_session(id).await?;
        let partial = destination.with_extension("part");
        let output = session.download(&format!("{}&& cat {}", remote::cd(&target.dir), remote::quote(name)), &partial).await;
        session.close().await;
        match output {
            Ok(output) if output.ok() => {
                std::fs::rename(&partial, destination)?;
                Ok(())
            }
            other => {
                let _ = std::fs::remove_file(&partial);
                let detail = other.map(|o| o.stderr).unwrap_or_else(|e| e.to_string());
                Err(AppError::detail("remote", pick("下载备份失败。", "Couldn't download the backup."), detail))
            }
        }
    }

    fn remote_progress(&self, id: &str, stage: &str, done: u64, total: u64) {
        self.send(UiEvent::RemoteProgress { instance_id: id.to_string(), stage: stage.to_string(), done, total });
    }

    /// Builds the world's server, uploads it and starts it; an update keeps
    /// the world and the host's own settings. Runs on a blocking thread.
    pub(super) fn deploy_server(self: &Arc<Self>, id: &str, options: DeployOptions, cancel: &CancellationToken) -> AppResult<String> {
        let runtime = tokio::runtime::Handle::current();
        let mut target = remote::load(&self.paths, id).ok_or_else(not_set_up)?;
        let instance = self.instances.get(id)?;
        let report = |zh: &'static str, en: &'static str, done: u64, total: u64| self.remote_progress(id, pick(zh, en), done, total);

        report("连接服务器", "Connecting", 0, 0);
        let session = runtime.block_on(Session::connect(&target, self.remote_secret(id).as_deref()))?;
        if target.fingerprint.is_none() {
            target.fingerprint = Some(session.fingerprint.clone());
            remote::save(&self.paths, id, &target)?;
        }
        let probe = remote::parse_probe(&runtime.block_on(session.run_ok(&remote::probe_script(&target.dir), "无法读取服务器信息。", "Couldn't look around the server."))?);
        if probe.os != "Linux" {
            return Err(AppError::detail("remote", pick("目前只支持 Linux 服务器。", "Only Linux servers are supported for now."), probe.os));
        }
        if !matches!(probe.arch.as_str(), "x86_64" | "aarch64") {
            return Err(AppError::detail("remote", pick("服务器的处理器架构不受支持。", "The server's processor isn't supported."), probe.arch));
        }
        if let Some(tool) = ["tar", "mkfifo", "setsid"].into_iter().find(|tool| !probe.has(tool)) {
            return Err(AppError::detail("remote", pick("服务器缺少必要的系统命令。", "The server lacks a command Islet needs."), tool));
        }
        let update = probe.deployed;
        if !update && !options.accept_eula {
            return Err(AppError::new("remote", pick("要先同意 Minecraft EULA，服务器才能运行。", "Agree to the Minecraft EULA first; the server won't run otherwise.")));
        }

        let build = self.paths.cache.join("remote").join(format!("{id}-{}", uuid::Uuid::new_v4().simple()));
        let archive = build.with_extension("tar.gz");
        let result = (|| -> AppResult<String> {
            report("生成服务端", "Building the server", 0, 0);
            let server_options = crate::core::server_pack::ServerOptions {
                accept_eula: options.accept_eula,
                memory_mb: target.memory_mb,
                world: if update { None } else { options.world.clone() },
                port: target.game_port,
                online_mode: target.online_mode,
                client_pack: true,
                address: target.public_address(),
                host_uuid: None,
            };
            let (summary, pack) = self.export_server(id, &build, server_options, cancel)?;
            std::fs::write(build.join("islet.sh"), remote::SCRIPT)?;
            let pack = match pack {
                Some(file) => {
                    let dir = pack_dir(self, id);
                    let _ = std::fs::remove_dir_all(&dir);
                    std::fs::create_dir_all(&dir)?;
                    let kept = dir.join(file.file_name().unwrap_or_default());
                    std::fs::rename(&file, &kept).or_else(|_| std::fs::copy(&file, &kept).map(|_| ()))?;
                    Some(kept)
                }
                None => None,
            };

            let major = self.server_java_major(&instance)?;
            if probe.java.as_ref().is_none_or(|(_, have)| *have != major) {
                report("在服务器上安装 Java", "Installing Java on the server", 0, 0);
                self.remote_java(id, &session, &target, &probe, major, cancel)?;
            }

            if update {
                report("停止服务器", "Stopping the server", 0, 0);
                runtime.block_on(session.run(&format!("{}&& sh islet.sh stop", remote::cd(&target.dir))))?;
            }
            report("打包", "Packing", 0, 0);
            let size = remote::pack(&build, &archive, update)?;
            if probe.free_mb.is_some_and(|free| free < size / 1024 / 1024 * 3 + 512) {
                return Err(AppError::detail("remote", pick("服务器的磁盘空间不够。", "The server is low on disk space."), format!("{} MB free", probe.free_mb.unwrap_or_default())));
            }
            report("上传", "Uploading", 0, size);
            let progress = |sent: u64| report("上传", "Uploading", sent, size);
            let output = runtime.block_on(session.upload(&remote::unpack_script(&target.dir, update), &archive, &progress))?;
            if !output.ok() {
                return Err(AppError::detail("remote", pick("上传后解压失败。", "Unpacking the upload failed."), output.stderr));
            }
            runtime.block_on(session.run_ok(&remote::configure_script(&target), "设置服务器失败。", "Couldn't set the server up."))?;
            // A project's settings follow it to a server that already has its
            // own server.properties.
            let properties: Vec<String> = crate::core::server_pack::project_properties(&instance)
                .into_iter()
                .map(|(key, value)| format!("sh islet.sh set-prop {key} {}", remote::quote(&value)))
                .collect();
            if update && !properties.is_empty() {
                let script = format!("{}&& {}", remote::cd(&target.dir), properties.join(" && "));
                runtime.block_on(session.run_ok(&script, "设置服务器失败。", "Couldn't set the server up."))?;
            }

            report("启动服务器", "Starting the server", 0, 0);
            runtime.block_on(session.run_ok(&format!("{}&& sh islet.sh start", remote::cd(&target.dir)), "服务器启动失败。", "The server didn't start."))?;
            let started = std::time::Instant::now();
            loop {
                std::thread::sleep(Duration::from_secs(4));
                if cancel.is_cancelled() {
                    return Err(AppError::new("cancelled", pick("操作已取消。", "Cancelled.")));
                }
                let state = remote::parse_state(&runtime.block_on(session.run_ok(&format!("{}&& sh islet.sh status", remote::cd(&target.dir)), "读取服务器状态失败。", "Couldn't read the server's state."))?);
                if state.ready {
                    break;
                }
                if !state.running {
                    let log = runtime.block_on(session.run(&format!("{}&& sh islet.sh log 30", remote::cd(&target.dir)))).map(|o| o.stdout).unwrap_or_default();
                    return Err(AppError::detail("remote", pick("服务器启动后又退出了，看看详情里的日志。", "The server stopped right after starting; see the log in the details."), log));
                }
                let waited = started.elapsed().as_secs();
                report("等待服务器启动", "Waiting for the server", waited, 0);
                if waited > 20 * 60 {
                    return Err(AppError::new("remote", pick("服务器 20 分钟还没启动完，请在云服务器管理里看看日志。", "The server hasn't finished starting after 20 minutes; check its log.")));
                }
            }

            report("检查朋友能否连上", "Checking friends can reach it", 0, 0);
            let address = target.public_address();
            let reachable = runtime.block_on(crate::core::servers::ping(&self.http()?, &address)).is_ok();
            target.deployed_at = Some(crate::core::util::now_millis());
            remote::save(&self.paths, id, &target)?;

            let what = if crate::core::instances::runs_plugins(&instance.loader) { crate::core::i18n::pick("插件", "plugins") } else { crate::core::i18n::pick("模组", "mods") };
            let mut message = if update {
                crate::tr!("服务器已更新并重新启动，带 {} 个{what}。", "The server is updated and running again with {} {what}.", summary.mods)
            } else {
                crate::tr!("服务器已经在 {address} 运行，带 {} 个{what}。", "The server is running at {address} with {} {what}.", summary.mods)
            };
            if let Some(pack) = &pack {
                message += &crate::tr!("把「{}」发给朋友，导入后就能进服。", " Send \"{}\" to friends; once imported they can join.", pack.file_name().unwrap_or_default().to_string_lossy());
            }
            if !reachable {
                message += &crate::tr!(
                    "但从这台电脑连不上 {address}：请在云服务器控制台的安全组（防火墙）里放行 TCP {} 端口。",
                    " But {address} can't be reached from here: allow TCP port {} in the cloud provider's security group (firewall).",
                    target.game_port
                );
            }
            Ok(message)
        })();
        runtime.block_on(session.close());
        let _ = std::fs::remove_dir_all(&build);
        let _ = std::fs::remove_file(&archive);
        result
    }

    /// Puts the Java the game needs in the server folder: fetched by the
    /// host itself when it can, else downloaded here and uploaded.
    fn remote_java(&self, id: &str, session: &Session, target: &Target, probe: &Probe, major: u32, cancel: &CancellationToken) -> AppResult<()> {
        let runtime = tokio::runtime::Handle::current();
        let source = self.settings.get().download_source;
        let http = self.http()?;
        let java = runtime.block_on(crate::core::java::runtime::linux_runtime(&http, major, &probe.arch, source, cancel))?;
        let dir = remote::quote(&target.dir);
        let extract = "rm -rf java.new && mkdir java.new && tar -xzf .islet/jre.tar.gz -C java.new --strip-components=1 && rm -rf java && mv java.new java && rm -f .islet/jre.tar.gz && java/bin/java -version";
        let prepare = format!("cd \"$HOME\" && mkdir -p {dir} && cd {dir} && mkdir -p .islet && rm -f .islet/jre.tar.gz");
        if (probe.has("curl") || probe.has("wget")) && probe.has("sha256sum") {
            let urls: Vec<String> = java.urls.iter().map(|u| remote::quote(u)).collect();
            let script = format!(
                "{prepare} && ok=0
for u in {urls}; do
  if command -v curl >/dev/null 2>&1; then curl -fsSL --retry 2 --connect-timeout 15 -o .islet/jre.tar.gz \"$u\" && ok=1 && break; fi
  if command -v wget >/dev/null 2>&1; then wget -q -T 15 -O .islet/jre.tar.gz \"$u\" && ok=1 && break; fi
done
[ $ok = 1 ] || exit 3
echo {sum} | sha256sum -c - >/dev/null 2>&1 || {{ rm -f .islet/jre.tar.gz; exit 4; }}
{extract}",
                urls = urls.join(" "),
                sum = remote::quote(&format!("{}  .islet/jre.tar.gz", java.sha256)),
            );
            if runtime.block_on(session.run(&script)).is_ok_and(|o| o.ok()) {
                return Ok(());
            }
            tracing::warn!("the host couldn't fetch Java itself; uploading it");
        }
        let archive = runtime.block_on(crate::core::java::runtime::download_linux_runtime(&http, &self.paths, &java, cancel))?;
        let size = std::fs::metadata(&archive)?.len();
        let progress = |sent: u64| self.remote_progress(id, pick("上传 Java", "Uploading Java"), sent, size);
        let output = runtime.block_on(session.upload(&format!("{prepare} && cat > .islet/jre.tar.gz && {extract}"), &archive, &progress))?;
        if !output.ok() {
            return Err(AppError::detail("remote", pick("在服务器上安装 Java 失败。", "Couldn't install Java on the server."), output.stderr));
        }
        Ok(())
    }
}
