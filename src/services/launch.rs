use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use super::accounts::offer_offline;
use super::{
    AppServices, CrashAnalysis, InstancePaths, LaunchTarget, STOP_GRACE, UiEvent,
    instance_mod_count, load_version, lock, projects,
};
use crate::core::accounts::{Account, AccountProvider};
use crate::core::auth::{offline, yggdrasil};
use crate::core::crash;
use crate::core::error::{AppError, AppResult};
use crate::core::instances::Instance;
use crate::core::java::runtime;
use crate::core::launch;
use crate::core::manage;
use crate::core::mojang::version::java_major;
use crate::core::options;
use crate::core::paths::AppPaths;
use crate::core::platform;
use crate::core::servers;
use crate::core::settings::{DownloadSource, Settings};

impl AppServices {
    pub fn configure_instance(
        &self,
        id: &str,
        name: &str,
        memory: Option<u32>,
        java: Option<String>,
        loader: &str,
    ) -> AppResult<()> {
        self.ensure_idle(id)?;
        self.instances.configure(id, name, memory, java, loader)
    }

    pub fn configure_launch(
        &self,
        id: &str,
        launch: crate::core::instances::LaunchConfig,
        account_id: Option<String>,
    ) -> AppResult<()> {
        if let Some(account) = account_id.as_deref().filter(|a| !a.is_empty())
            && self.accounts.get(account).is_none() {
                return Err(AppError::new("account", crate::core::i18n::pick("这个账号不存在。", "This account doesn't exist.")));
            }
        self.instances.configure_launch(id, launch, account_id)
    }

    pub fn organize_instance(&self, id: &str, favorite: bool, group: Option<String>) -> AppResult<()> {
        self.instances.organize(id, favorite, group)
    }

    pub fn change_version(&self, id: &str, version_id: &str, version_type: &str) -> AppResult<()> {
        self.ensure_idle(id)?;
        self.instances.change_version(id, version_id, version_type)
    }

    pub fn create_instance(
        &self,
        name: &str,
        version_id: &str,
        version_type: &str,
    ) -> AppResult<Instance> {
        self.ensure_storage_idle()?;
        let instance = self.instances.create(name, version_id, version_type)?;
        self.settings.update(|settings| {
            if settings.active_instance_id.is_none() {
                settings.active_instance_id = Some(instance.id.clone());
            }
        })?;
        Ok(instance)
    }

    pub fn delete_instance(&self, id: &str) -> AppResult<(Vec<Instance>, Settings)> {
        self.ensure_idle(id)?;
        self.instances.get(id)?;
        self.instances.remove(id)?;
        let remaining = self.instances.list();
        let settings = self.settings.update(|settings| {
            if settings.active_instance_id.as_deref() == Some(id) {
                settings.active_instance_id = remaining.iter().find(|instance| instance.server.is_none()).map(|instance| instance.id.clone());
            }
        })?;
        Ok((remaining, settings))
    }

    pub fn set_active_instance(&self, id: Option<String>) -> AppResult<Settings> {
        self.ensure_storage_idle()?;
        if let Some(ref instance_id) = id
            && self.instances.get(instance_id)?.server.is_some() {
                return Err(projects::not_playable());
            }
        self.settings
            .update(|settings| settings.active_instance_id = id)
    }

    pub fn instance_paths(&self, instance_id: &str) -> AppResult<InstancePaths> {
        AppPaths::validate_instance_id(instance_id)?;
        Ok(InstancePaths {
            game_dir: self
                .paths
                .instance_game_dir(instance_id)
                .display()
                .to_string(),
            log_dir: self
                .paths
                .instance_logs_dir(instance_id)
                .display()
                .to_string(),
        })
    }

    /// The last `lines` lines of the latest log. Only the tail of the file is
    /// read, so a modpack log of tens of megabytes opens instantly.
    ///
    /// This `latest.log` lives in the instance's launcher logs directory and
    /// is truncated at every launch; it contains the stdout/stderr captured
    /// from Java. The game's own log (`minecraft/logs/latest.log`) is a
    /// different file and is intentionally not used for crash analysis, so a
    /// previous run cannot leak into the current diagnosis.
    /// Whether the installed version can open a world straight from launch.
    pub fn quick_play_supported(&self, instance_id: &str) -> bool {
        self.instances
            .get(instance_id)
            .ok()
            .filter(|instance| instance.installed)
            .and_then(|instance| load_version(&self.paths, &instance).ok())
            .is_some_and(|version| launch::supports_quick_play(&version))
    }

    pub fn read_log(&self, instance_id: &str, lines: usize) -> AppResult<Vec<String>> {
        const TAIL_BYTES: u64 = 1024 * 1024;
        AppPaths::validate_instance_id(instance_id)?;
        let path = self.paths.instance_logs_dir(instance_id).join("latest.log");
        let Some((text, _)) = read_tail_text(&path, TAIL_BYTES)? else {
            return Ok(Vec::new());
        };
        let mut all: Vec<&str> = text.lines().collect();
        let max = lines.clamp(1, 2000);
        if all.len() > max {
            all.drain(..all.len() - max);
        }
        Ok(all.into_iter().map(str::to_string).collect())
    }

    /// Diagnose the current run only. The launcher's captured log is rewritten
    /// at every launch and both it and the crash report are gated by
    /// `since_ms`, so an old log file cannot produce a misleading diagnosis.
    /// Reads are bounded: 512 KiB of log tail and 256 KiB of crash report.
    pub fn analyze_crash(&self, instance_id: &str, since_ms: i64) -> AppResult<CrashAnalysis> {
        const LOG_BYTES: u64 = 512 * 1024;
        const REPORT_BYTES: u64 = 256 * 1024;
        AppPaths::validate_instance_id(instance_id)?;
        self.instances.get(instance_id)?;

        // Filesystems with 1-second mtime granularity (FAT32/exFAT/HFS+) truncate
        // write timestamps down to the whole second; flooring `since_ms` to its
        // second keeps crashes written in the same second from being dropped.
        let cutoff_ms = if since_ms > 0 {
            since_ms - since_ms.rem_euclid(1000)
        } else {
            since_ms
        };
        let log_path = self.paths.instance_logs_dir(instance_id).join("latest.log");
        // The header carries the resolved `memory=` value. Read it separately
        // from the tail: a long log would otherwise hide it from the scan.
        let log = match analysis_log_text(&log_path, LOG_BYTES)? {
            Some((text, modified)) if modified >= cutoff_ms => text,
            _ => String::new(),
        };
        let report_dir = self.paths.instance_game_dir(instance_id).join("crash-reports");
        let mut report = latest_crash_report(&report_dir, cutoff_ms, "txt", "")?
            .map(|path| read_head_text(&path, REPORT_BYTES))
            .transpose()?;
        // A crash of the Java VM itself leaves hs_err_pid<pid>.log in the
        // game folder, its working directory.
        let game_dir = self.paths.instance_game_dir(instance_id);
        if let Some(path) = latest_crash_report(&game_dir, cutoff_ms, "log", "hs_err_pid")? {
            let native = read_head_text(&path, REPORT_BYTES)?;
            report = Some(match report {
                Some(report) => format!("{native}\n{report}"),
                None => native,
            });
        }

        let diagnoses = crash::analyze_with_memory_limit(
            &log,
            report.as_deref(),
            platform::total_memory_mb(),
        );
        Ok(CrashAnalysis {
            diagnoses,
            excerpt: build_excerpt(&log, report.as_deref()),
        })
    }

    /// Starts the game; with `world`, it opens that singleplayer world
    /// directly instead of the title screen (Minecraft 1.20+); with `server`,
    /// it joins that server.
    /// `offline` plays as the account without signing in, for when its
    /// sign-in servers can't be reached: singleplayer works, online servers don't.
    pub fn launch_instance(
        self: &Arc<Self>,
        instance_id: &str,
        world: Option<String>,
        server: Option<String>,
        offline: bool,
    ) -> AppResult<()> {
        self.ensure_idle(instance_id)?;
        if self.instances.get(instance_id)?.server.is_some() {
            return Err(projects::not_playable());
        }
        if world.is_some() && server.is_some() {
            return Err(AppError::new("launch", crate::core::i18n::pick("不能同时进入存档和服务器。", "Can't open a save and a server at the same time.")));
        }
        if let Some(folder) = &world {
            crate::core::content::world_dir(&self.paths, instance_id, folder)?;
        }
        if let Some(address) = &server {
            servers::parse_address(address)?;
        }
        let settings = self.settings.get();
        let instance = self.instances.get(instance_id)?;
        if !instance.installed {
            return Err(AppError::new(
                "instance_not_installed",
                crate::core::i18n::pick("这个世界还没有下载完成，请先下载。", "This world isn't fully downloaded yet; install it first."),
            ));
        }
        // A world bound to an account plays as it while that account exists.
        let account_id = instance
            .account_id
            .clone()
            .filter(|id| self.accounts.get(id).is_some())
            .or_else(|| settings.active_account_id.clone())
            .ok_or_else(|| AppError::new("account", crate::core::i18n::pick("请先添加并选择一个账号。", "Add and select an account first.")))?;
        let account = self
            .accounts
            .get(&account_id)
            .ok_or_else(|| AppError::new("account", crate::core::i18n::pick("当前账号不存在，请重新选择。", "The selected account no longer exists; choose another.")))?;
        {
            if lock(&self.launches).contains_key(instance_id)
                || lock(&self.starting).contains(instance_id)
            {
                return Err(AppError::new("already_running", crate::core::i18n::pick("这个世界已经在运行。", "This world is already running.")));
            }
            lock(&self.starting).insert(instance_id.to_string());
        }

        let this = self.clone();
        let id = instance_id.to_string();
        self.spawn_task(async move {
            let run_this = this.clone();
            let target = match (&world, &server) {
                (Some(folder), _) => LaunchTarget::World(folder),
                (_, Some(address)) => LaunchTarget::Server(address),
                _ => LaunchTarget::Title,
            };
            let result = run_this
                .run_launch(&id, &instance, &settings, &account, target, offline)
                .await;
            lock(&this.starting).remove(&id);
            if let Err(error) = result {
                tracing::warn!(instance = %id, code = error.code(), error = %error, "launch failed");
                this.send(UiEvent::LaunchError {
                    instance_id: id,
                    error: error.to_payload(),
                });
            }
        });
        Ok(())
    }

    /// Live status of a server: players, latency, message of the day.
    /// The UUID the host plays as on a server, so their things from the save
    /// come along: the Microsoft one on an online-mode server, the one the
    /// server derives from the name otherwise.
    pub(super) fn host_uuid(&self, instance: &Instance, online_mode: bool) -> Option<String> {
        let account_id = instance.account_id.clone().filter(|id| self.accounts.get(id).is_some()).or_else(|| self.settings.get().active_account_id)?;
        let account = self.accounts.get(&account_id)?;
        let uuid = match (online_mode, account.provider) {
            (true, crate::core::accounts::AccountProvider::Microsoft) => account.uuid.replace('-', ""),
            (true, _) => return None,
            (false, _) => crate::core::auth::offline::offline_uuid(&account.username).replace('-', ""),
        };
        let uuid = uuid::Uuid::parse_str(&uuid).ok()?;
        Some(uuid.hyphenated().to_string())
    }

    pub async fn ping_server(&self, address: &str) -> AppResult<servers::ServerStatus> {
        servers::ping(&self.http()?, address).await
    }

    /// The EasyTier programs behind P2P rooms, downloaded on first use from
    /// the fastest source; GitHub mirrors rank first among failed sources
    /// when downloads go through BMCLAPI.
    pub async fn ensure_easytier(
        &self,
        cancel: &CancellationToken,
        report: Option<crate::core::p2p::easytier::Progress>,
    ) -> AppResult<crate::core::p2p::easytier::Binaries> {
        let mirror = matches!(self.settings.get().download_source, DownloadSource::Bmclapi);
        let http = self.http()?;
        let extra = if crate::core::p2p::easytier::installed(&self.paths).is_some() { Vec::new() } else { self.p2p_remote().await.sources };
        crate::core::p2p::easytier::ensure(&http, &self.paths, mirror, &extra, cancel, report).await
    }

    /// The published P2P node list and download sources (cached for hours).
    pub async fn p2p_remote(&self) -> crate::core::p2p::easytier::Remote {
        match self.http() {
            Ok(http) => crate::core::p2p::easytier::remote(&http, &self.paths).await,
            Err(_) => Default::default(),
        }
    }

    /// Asks the game to close the way its window's close button does, so it
    /// saves first, and ends it after `STOP_GRACE` if it is still running.
    /// `force` skips the wait: progress since the last save is lost.
    pub fn stop_instance(self: &Arc<Self>, instance_id: &str, force: bool) -> AppResult<()> {
        let pid = lock(&self.launches).get(instance_id).copied();
        let Some(pid) = pid.filter(|pid| *pid > 0) else {
            return Err(AppError::new("not_running", crate::core::i18n::pick("这个世界没有在运行。", "This world isn't running.")));
        };
        #[cfg(unix)]
        if !force {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
        let this = self.clone();
        let id = instance_id.to_string();
        self.spawn_task(async move {
            // taskkill takes a moment; it runs here instead of on the UI thread.
            if !force {
                #[cfg(windows)]
                let _ = tokio::process::Command::from(crate::core::platform::quiet_command(
                    "taskkill.exe",
                ))
                .args(["/PID", &pid.to_string(), "/T"])
                .status()
                .await;
                // Big saves and heavy modpacks can take a while to write out.
                tokio::time::sleep(STOP_GRACE).await;
            }
            let still_running = lock(&this.launches).get(&id).copied();
            if let Some(pid) = still_running.filter(|current| *current == pid) {
                #[cfg(unix)]
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
                #[cfg(windows)]
                {
                    let _ = tokio::process::Command::from(crate::core::platform::quiet_command(
                        "taskkill.exe",
                    ))
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .status()
                    .await;
                }
            }
        });
        Ok(())
    }

    async fn run_launch(
        self: Arc<Self>,
        instance_id: &str,
        instance: &Instance,
        settings: &Settings,
        account: &Account,
        target: LaunchTarget<'_>,
        offline: bool,
    ) -> AppResult<()> {
        // Every launch requires a verified Microsoft account that owns
        // Minecraft: Java Edition (either this account itself or an owning
        // Microsoft account signed in alongside offline/third-party profiles).
        if !self.accounts.has_owner()
            || (account.provider == AccountProvider::Microsoft && !account.owns_game())
        {
            return Err(crate::core::accounts::owner_required());
        }
        let session = if offline {
            offline::session(account)
        } else {
            self.session_for_account(account).await.map_err(|err| offer_offline(account, err))?
        };
        let version = load_version(&self.paths, instance)?;
        if matches!(target, LaunchTarget::World(_)) && !launch::supports_quick_play(&version) {
            return Err(AppError::new(
                "quick_play",
                crate::core::i18n::pick("这个版本不支持直接进入存档，需要 Minecraft 1.20 或更高版本。", "This version can't open saves directly; that needs Minecraft 1.20 or later."),
            ));
        }
        let cancel = CancellationToken::new();
        let http = self.http()?;
        let arch = crate::core::mojang::version::game_arch(&version);
        let java = runtime::ensure_java_with_source(
            &http,
            &self.paths,
            java_major(&version),
            instance
                .java_path
                .as_deref()
                .or(settings.java_path.as_deref()),
            arch,
            &cancel,
            None,
            settings.download_source,
        )
        .await?;
        let launch_config = instance.launch.over(&settings.launch);
        let mut jvm_args = launch::split_args(&launch_config.jvm_args)?;
        // In offline fallback mode, skip third-party auth agents and metadata
        // lookups so singleplayer starts without touching an unreachable server.
        if !offline
            && let Some(server_id) = account
                .server
                .as_deref()
                .filter(|_| account.provider == AccountProvider::Yggdrasil)
                .and_then(yggdrasil::nide8_server_id)
        {
            // 统一通行证 brings its own agent in place of authlib-injector.
            let agent = yggdrasil::ensure_nide8_agent(&http, &self.paths).await?;
            let mut agent_args = yggdrasil::nide8_args(&agent, server_id);
            agent_args.append(&mut jvm_args);
            jvm_args = agent_args;
        } else if !offline && account.provider == AccountProvider::Yggdrasil {
            // authlib-injector first, so the player's own arguments still come last.
            let agent = yggdrasil::ensure_agent(&http, &self.paths).await?;
            let metadata = match account.server.as_deref() {
                Some(api) => yggdrasil::metadata(&http, api).await,
                None => None,
            };
            let mut agent_args = yggdrasil::agent_args(&agent, account, metadata.as_deref());
            agent_args.append(&mut jvm_args);
            jvm_args = agent_args;
        }
        // An offline skin is cosmetic: if its server or the agent can't be
        // set up, the game still starts with the default skin.
        let local_skin = if account.provider == AccountProvider::Offline && account.skin_url.is_some() {
            let prepared = async {
                let server = crate::core::auth::local_skin::SkinServer::start(&self.paths, account)?;
                let agent = yggdrasil::ensure_agent(&http, &self.paths).await?;
                AppResult::Ok((server, agent))
            }
            .await;
            match prepared {
                Ok((server, agent)) => {
                    jvm_args.insert(0, format!("-javaagent:{}={}", agent.display(), server.url));
                    jvm_args.insert(1, "-Dauthlibinjector.noShowServerName=true".into());
                    Some(server)
                }
                Err(error) => {
                    tracing::warn!(error = %error, "offline skin unavailable; launching with the default skin");
                    None
                }
            }
        } else {
            None
        };
        let options = launch::LaunchOptions {
            arch: arch.to_string(),
            window: launch_config.window_width.zip(launch_config.window_height),
            jvm_args,
            game_args: launch::split_args(&launch_config.game_args)?,
        };
        let client_id = if account.provider == AccountProvider::Microsoft {
            settings.effective_client_id()
        } else {
            String::new()
        };
        let total_mb = platform::total_memory_mb();
        let mod_count = instance_mod_count(&self.paths, &instance.id);
        let memory_mb = resolve_memory_mb(
            instance.memory_mb,
            settings.memory_auto,
            settings.memory_mb,
            total_mb,
            mod_count,
        );
        tracing::info!(
            instance = %instance.id,
            memory_mb,
            automatic = instance.memory_mb.is_none() && settings.memory_auto,
            total_mb,
            mod_count,
            "resolved game memory"
        );
        let mut spec = launch::build_spec(
            &self.paths,
            instance,
            &version,
            &java,
            &session,
            &client_id,
            memory_mb,
            &options,
        )?;
        match target {
            LaunchTarget::Title => {}
            LaunchTarget::World(folder) => {
                spec.args.push("--quickPlaySingleplayer".to_string());
                spec.args.push(folder.to_string());
            }
            LaunchTarget::Server(address) if launch::supports_quick_play_multiplayer(&version) => {
                spec.args.push("--quickPlayMultiplayer".to_string());
                spec.args.push(address.to_string());
            }
            LaunchTarget::Server(address) => {
                // Older versions connect to exactly the host and port given,
                // so follow the SRV record here as the game's list would.
                let (host, port) = servers::resolve(&http, address).await?;
                spec.args.extend(["--server".to_string(), host, "--port".to_string(), port.to_string()]);
            }
        }
        // The game starts in English by itself, so only Chinese needs writing.
        if settings.auto_chinese && !crate::core::i18n::english() {
            match options::prepare_first_launch(&self.paths, &instance.id, &instance.version_id, "zh_cn") {
                Ok(true) => tracing::info!(instance = %instance.id, "set the game language to Chinese for the first launch"),
                Ok(false) => {}
                Err(error) => tracing::warn!(instance = %instance.id, error = %error, "could not prepare options.txt"),
            }
        }

        let log_dir = self.paths.instance_logs_dir(&instance.id);
        tokio::fs::create_dir_all(&log_dir).await?;
        let log_path = log_dir.join("latest.log");
        let header = format!(
            "[Islet] version={} memory={}MB java={} ({}, {}), user={}\n",
            instance.version_id, memory_mb, java.version, java.major, java.arch, session.username
        );
        tokio::fs::write(&log_path, header).await?;

        // The player's command before the game, in the game folder; a
        // failure stops the launch so its message is not missed.
        if !launch_config.pre_launch.trim().is_empty() {
            run_pre_launch(&launch_config.pre_launch, &spec.game_dir, &log_path).await?;
        }

        // A wrapper (gamemoderun, prime-run …) runs Java as its argument.
        let wrapper = launch::split_args(&launch_config.wrapper)?;
        let mut command = match wrapper.split_first() {
            Some((program, rest)) => {
                let mut command = Command::new(program);
                command.args(rest).arg(&spec.java_path);
                command
            }
            None => Command::new(&spec.java_path),
        };
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        command
            .args(&spec.args)
            .current_dir(&spec.game_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_remove("JAVA_TOOL_OPTIONS")
            .env_remove("_JAVA_OPTIONS")
            .env_remove("JDK_JAVA_OPTIONS")
            .envs(launch_config.env_pairs());
        if settings.dedicated_gpu {
            #[cfg(windows)]
            if let Err(error) = crate::core::platform::prefer_dedicated_gpu(&spec.java_path) {
                tracing::warn!(%error, "could not set the GPU preference for Java");
            }
            #[cfg(target_os = "linux")]
            if crate::core::platform::has_second_gpu() && !launch_config.env_pairs().iter().any(|(key, _)| key == "DRI_PRIME") {
                command.env("DRI_PRIME", "1");
            }
        }
        let mut child = command.spawn().map_err(|err| {
            AppError::detail(
                "launch",
                crate::core::i18n::pick("启动 Java 进程失败，请检查 Java 路径。", "Couldn't start the Java process; check the Java path."),
                err.to_string(),
            )
        })?;
        let pid = child
            .id()
            .ok_or_else(|| AppError::new("launch", crate::core::i18n::pick("无法获取 Minecraft 进程 ID。", "Couldn't get the Minecraft process ID.")))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let started = std::time::Instant::now();
        let started_ms = crate::core::util::now_millis();
        lock(&self.launches).insert(instance_id.to_string(), pid);
        if let Err(err) = self.instances.mark_launched(instance_id) {
            tracing::warn!(instance = %instance_id, error = %err, "failed to update last launch time");
        }
        self.send(UiEvent::LaunchStarted {
            instance_id: instance_id.to_string(),
            pid,
        });

        let mut readers = Vec::new();
        if let Some(stdout) = stdout {
            readers.push(self.spawn_log_reader(instance_id, stdout, "stdout", log_path.clone()));
        }
        if let Some(stderr) = stderr {
            readers.push(self.spawn_log_reader(instance_id, stderr, "stderr", log_path.clone()));
        }
        let post_exit = launch_config.post_exit.trim().to_string();
        let game_dir = spec.game_dir.clone();

        let this = self.clone();
        let id = instance_id.to_string();
        let id_for_hook = id.clone();
        tokio::spawn(async move {
            let _local_skin = local_skin;
            let status = child.wait().await;
            let code = status.ok().and_then(|status| status.code());
            // The child has exited, but the readers may still be draining
            // buffered stderr/stdout. Wait for them (bounded, in case a
            // grandchild inherited the pipe) before announcing the exit so a
            // crash dialog reads the complete log.
            drain_readers(readers, Duration::from_secs(2), Duration::from_secs(1)).await;
            lock(&this.launches).remove(&id);
            if let Err(err) = this.instances.add_play_time(&id, started.elapsed().as_secs()) {
                tracing::warn!(instance = %id, error = %err, "failed to record play time");
            }
            this.send(UiEvent::LaunchExit {
                instance_id: id,
                code,
                success: code == Some(0),
            });
            this.send(UiEvent::AccountsChanged(this.accounts.list()));
            let settings = this.settings.get();
            if settings.auto_backup {
                let (paths, id) = (this.paths.clone(), id_for_hook.clone());
                let keep = settings.auto_backup_keep as usize;
                match tokio::task::spawn_blocking(move || manage::auto_backup_saves(&paths, &id, started_ms, keep)).await {
                    Ok(Ok(saves)) if !saves.is_empty() => tracing::info!(instance = %id_for_hook, ?saves, "backed up played saves"),
                    Ok(Err(error)) => tracing::warn!(instance = %id_for_hook, %error, "automatic save backup failed"),
                    _ => {}
                }
            }
            // The player's command after the game; it can't undo anything,
            // so a failure is only written to the log.
            if !post_exit.is_empty()
                && let Err(error) = run_post_exit(&post_exit, &game_dir, &log_path, code).await {
                    tracing::warn!(instance = %id_for_hook, %error, "post-exit command failed");
                }
        });
        Ok(())
    }

    fn spawn_log_reader<R>(
        &self,
        instance_id: &str,
        reader: R,
        stream: &'static str,
        log_path: PathBuf,
    ) -> tokio::task::JoinHandle<()>
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let events = self.events.clone();
        let id = instance_id.to_string();
        tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            let mut buffer = Vec::new();
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .await
                .ok()
                .map(|file| tokio::io::BufWriter::with_capacity(64 * 1024, file));
            while let Some(line) = read_lossy_line(&mut reader, &mut buffer).await {
                if let Some(file) = &mut file {
                    // Buffered together in the 64 KiB BufWriter without a per-line String copy.
                    let _ = file.write_all(line.as_bytes()).await;
                    let _ = file.write_all(b"\n").await;
                    // A burst of output becomes one write; the file catches
                    // up as soon as the game pauses.
                    if reader.buffer().is_empty() {
                        let _ = file.flush().await;
                    }
                }
                let _ = events.try_send(UiEvent::LaunchLog {
                    instance_id: id.clone(),
                    line,
                    stream: stream.to_string(),
                });
            }
            if let Some(file) = &mut file {
                let _ = file.flush().await;
            }
        })
    }
}

/// Wait for the stdout/stderr readers to finish draining after the game has
/// exited. A grandchild can inherit a pipe and keep it open forever, so the
/// wait is bounded. On timeout the readers are aborted, not merely detached:
/// otherwise they would keep writing to the next launch's log and emit stale
/// log events. The abort wait is bounded too, so the exit event is not delayed
/// indefinitely.
pub(super) async fn drain_readers(
    mut readers: Vec<tokio::task::JoinHandle<()>>,
    drain_timeout: Duration,
    abort_timeout: Duration,
) {
    let drained = {
        let wait = async {
            for reader in &mut readers {
                let _ = reader.await;
            }
        };
        tokio::time::timeout(drain_timeout, wait).await.is_ok()
    };
    if drained {
        return;
    }
    for reader in &readers {
        reader.abort();
    }
    let _ = tokio::time::timeout(abort_timeout, async {
        for reader in readers {
            let _ = reader.await;
        }
    })
    .await;
}

/// Read at most `max_bytes` from the end of a file. Returns the lossy text and
/// the file's modification time in milliseconds. When the read starts
/// mid-file the first partial line is dropped.
pub(super) fn read_tail_text(path: &Path, max_bytes: u64) -> AppResult<Option<(String, i64)>> {
    use std::io::{Read, Seek, SeekFrom};
    use std::time::UNIX_EPOCH;
    let Ok(mut file) = std::fs::File::open(path) else {
        return Ok(None);
    };
    let metadata = file.metadata()?;
    let len = metadata.len();
    let start = len.saturating_sub(max_bytes);
    let mut bytes = Vec::new();
    file.seek(SeekFrom::Start(start))?;
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let text = if start > 0 {
        text.split_once('\n').map_or("", |(_, rest)| rest).to_string()
    } else {
        text.into_owned()
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    Ok(Some((text, modified)))
}

/// The analysis text for the current run: the launcher header (it records
/// `memory=`) plus the log tail. Reading only the tail loses the header once
/// the log grows past the tail window; only the header is taken from the
/// head, so early errors outside the tail stay outside the analysis window.
pub(super) fn analysis_log_text(path: &Path, max_bytes: u64) -> AppResult<Option<(String, i64)>> {
    const HEAD_BYTES: u64 = 16 * 1024;
    let Some((tail, modified)) = read_tail_text(path, max_bytes)? else {
        return Ok(None);
    };
    let head = read_head_text(path, HEAD_BYTES).unwrap_or_default();
    let Some(header) = launcher_header(&head) else {
        return Ok(Some((tail, modified)));
    };
    if tail.contains(header) {
        return Ok(Some((tail, modified)));
    }
    let mut text = String::with_capacity(header.len() + tail.len() + 1);
    text.push_str(header);
    text.push('\n');
    text.push_str(&tail);
    Ok(Some((text, modified)))
}

/// The `[Islet] ... memory=...` line written at the start of each run.
/// Logs from before the rename start with `[MC Launcher]`.
pub(super) fn launcher_header(head: &str) -> Option<&str> {
    head.lines().find(|line| {
        let lower = line.to_ascii_lowercase();
        (lower.starts_with("[islet]") || lower.starts_with("[mc launcher]")) && lower.contains("memory=")
    })
}

fn read_head_text(path: &Path, max_bytes: u64) -> AppResult<String> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(max_bytes).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The newest `<prefix>*.<extension>` file in `dir` written since the launch.
fn latest_crash_report(dir: &Path, since_ms: i64, extension: &str, prefix: &str) -> AppResult<Option<PathBuf>> {
    use std::time::UNIX_EPOCH;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(None);
    };
    let mut newest: Option<(i64, PathBuf)> = None;
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path();
        if !path
            .extension()
            .and_then(|found| found.to_str())
            .is_some_and(|found| found.eq_ignore_ascii_case(extension))
            || !entry.file_name().to_string_lossy().starts_with(prefix)
        {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);
        if modified < since_ms {
            continue;
        }
        if newest.as_ref().is_none_or(|(current, _)| modified > *current) {
            newest = Some((modified, path));
        }
    }
    Ok(newest.map(|(_, path)| path))
}

/// A compact report: up to 20 non-empty crash-report lines (the description
/// and top of the stack) plus the tail of the current launch log. Never more
/// than 60 lines.
pub(super) fn build_excerpt(log: &str, crash_report: Option<&str>) -> String {
    const MAX_LINES: usize = 60;
    const REPORT_LINES: usize = 20;
    let mut lines: Vec<&str> = Vec::new();
    if let Some(report) = crash_report {
        lines.extend(
            report
                .lines()
                .filter(|line| !line.trim().is_empty())
                .take(REPORT_LINES),
        );
    }
    let log_lines: Vec<&str> = log
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let remaining = MAX_LINES.saturating_sub(lines.len());
    let start = log_lines.len().saturating_sub(remaining);
    lines.extend(log_lines[start..].iter().copied());
    lines.truncate(MAX_LINES);
    lines.join("\n")
}

/// Count the enabled jars in an instance's mods directory. `.jar.disabled`
/// files are not loaded, so they do not influence the heap recommendation.
/// `.jar` files in the instance's mods folder; disabled mods do not count.
/// A player's command for the system shell, run in the game folder.
fn shell_command(command: &str, dir: &Path) -> Command {
    let mut shell = if cfg!(windows) {
        let mut shell = Command::new("cmd");
        shell.arg("/C").arg(command);
        shell
    } else {
        let mut shell = Command::new("/bin/sh");
        shell.arg("-c").arg(command);
        shell
    };
    #[cfg(windows)]
    shell.creation_flags(0x08000000);
    shell.current_dir(dir).stdin(Stdio::null()).kill_on_drop(true);
    shell
}

/// Runs the post-exit command and waits for it, up to five minutes. The
/// game's exit code is in `MC_EXIT_CODE`; output goes to the game log.
async fn run_post_exit(command: &str, dir: &Path, log: &Path, code: Option<i32>) -> AppResult<()> {
    let mut shell = shell_command(command, dir);
    shell.env("MC_EXIT_CODE", code.map_or_else(String::new, |c| c.to_string()));
    let output = tokio::time::timeout(Duration::from_secs(300), shell.output())
        .await
        .map_err(|_| AppError::new("post_exit", crate::core::i18n::pick("退出后命令超过 5 分钟没有结束，已停止。", "The post-exit command ran over 5 minutes and was stopped.")))?
        .map_err(|error| AppError::detail("post_exit", crate::core::i18n::pick("退出后命令无法运行。", "The post-exit command couldn't run."), error.to_string()))?;
    let mut text = format!("[Islet] post-exit: {command} (exit {})\n", output.status.code().map_or("?".to_string(), |c| c.to_string()));
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let mut file = tokio::fs::OpenOptions::new().append(true).open(log).await?;
    file.write_all(text.as_bytes()).await?;
    Ok(())
}

/// Runs the pre-launch command through the system shell and waits for it,
/// up to five minutes. Its output goes to the game log.
async fn run_pre_launch(command: &str, dir: &Path, log: &Path) -> AppResult<()> {
    let mut shell = shell_command(command, dir);
    let output = tokio::time::timeout(Duration::from_secs(300), shell.output())
        .await
        .map_err(|_| AppError::new("pre_launch", crate::core::i18n::pick("启动前命令超过 5 分钟没有结束，已停止启动。", "The pre-launch command ran over 5 minutes; the launch was stopped.")))?
        .map_err(|error| AppError::detail("pre_launch", crate::core::i18n::pick("启动前命令无法运行。", "The pre-launch command couldn't run."), error.to_string()))?;
    let mut text = format!("[Islet] pre-launch: {command}\n");
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let mut file = tokio::fs::OpenOptions::new().append(true).open(log).await?;
    file.write_all(text.as_bytes()).await?;
    if !output.status.success() {
        return Err(AppError::new(
            "pre_launch",
            crate::tr!("启动前命令失败（退出码 {}），游戏没有启动。输出在游戏日志里。", "The pre-launch command failed (exit code {}), so the game didn't start. Its output is in the game log.", output.status.code().map_or(crate::core::i18n::pick("未知", "unknown").into(), |c| c.to_string())),
        ));
    }
    Ok(())
}

/// Instance memory wins; otherwise automatic mode uses the machine-aware
/// recommendation and manual mode uses the global value.
pub(super) fn resolve_memory_mb(
    instance_memory: Option<u32>,
    memory_auto: bool,
    global_memory: u32,
    total_mb: u64,
    mod_count: usize,
) -> u32 {
    match instance_memory {
        Some(memory) => memory,
        None if memory_auto => platform::recommended_memory_mb(total_mb, mod_count),
        None => global_memory,
    }
}

/// Read one line without requiring UTF-8. Windows JVMs often print in the ANSI
/// code page (GBK on Chinese systems); a strict reader stops at the first such
/// line and every later log line is lost.
pub(super) async fn read_lossy_line<R>(reader: &mut R, buffer: &mut Vec<u8>) -> Option<String>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    buffer.clear();
    match reader.read_until(b'\n', buffer).await {
        Ok(0) | Err(_) => None,
        Ok(_) => {
            while matches!(buffer.last(), Some(b'\n' | b'\r')) {
                buffer.pop();
            }
            Some(String::from_utf8_lossy(buffer).into_owned())
        }
    }
}
