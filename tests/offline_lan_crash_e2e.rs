//! Isolated end-to-end regression tests covering real launcher entry points
//! outside unit tests:
//! 1. Offline account ownership gate + offline launch process spawn & CLI args.
//! 2. Unreachable Yggdrasil account offline fallback (`offline = true`).
//! 3. Real crashing process -> log & crash-report capture -> `analyze_crash`
//!    (including 1-second filesystem mtime truncation boundary).
//! 4. Real UDP multicast LAN discovery (`224.0.2.60:4445`), P2P relay-mark
//!    filtering, and local TCP port verification.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use islet::core::p2p::lan::{self, LanGame, RELAY_MARK};

#[cfg(unix)]
mod unix_e2e {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use async_channel::Receiver;
    use islet::core::accounts::{Account, AccountProvider};
    use islet::core::crash::Fix;
    use islet::core::instances::LaunchConfig;
    use islet::core::paths::AppPaths;
    use islet::services::{AppServices, UiEvent};

    const VERSION: &str = "1.21.4";

    fn owner_account() -> Account {
        Account {
            id: "microsoft:00000000-0000-0000-0000-00000000e2e1".into(),
            provider: AccountProvider::Microsoft,
            username: "SyntheticOwner".into(),
            uuid: "00000000-0000-0000-0000-00000000e2e1".into(),
            xuid: None,
            ownership: Some(true),
            skin_url: None,
            skin_slim: false,
            created_at: 0,
            server: None,
            server_name: None,
        }
    }

    fn unreachable_yggdrasil_account() -> Account {
        Account {
            id: "yggdrasil:dead-server:00000000-0000-0000-0000-00000000e2e2".into(),
            provider: AccountProvider::Yggdrasil,
            username: "ThirdPartySteve".into(),
            uuid: "00000000-0000-0000-0000-00000000e2e2".into(),
            xuid: None,
            ownership: None,
            skin_url: None,
            skin_slim: false,
            created_at: 0,
            server: Some("http://127.0.0.1:1/api/yggdrasil".into()),
            server_name: Some("Unreachable Yggdrasil".into()),
        }
    }

    fn install_minimal_version(paths: &AppPaths) {
        let dir = paths.version_cache_dir(VERSION);
        std::fs::create_dir_all(&dir).unwrap();
        let json = serde_json::json!({
            "id": VERSION,
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "assets": "17",
            "javaVersion": { "component": "java-runtime-delta", "majorVersion": 21 },
            "arguments": {
                "jvm": ["-Djava.library.path=${natives_directory}", "-cp", "${classpath}"],
                "game": [
                    "--username", "${auth_player_name}",
                    "--version", "${version_name}",
                    "--gameDir", "${game_directory}",
                    "--uuid", "${auth_uuid}",
                    "--accessToken", "${auth_access_token}",
                    "--userType", "${user_type}",
                    { "rules": [{ "action": "allow", "features": { "is_quick_play_singleplayer": true } }], "value": ["--quickPlaySingleplayer", "${quickPlaySingleplayer}"] }
                ]
            }
        });
        std::fs::write(paths.version_json(VERSION), serde_json::to_vec(&json).unwrap()).unwrap();
        std::fs::write(paths.client_jar(VERSION), b"fake-client-jar").unwrap();
    }

    fn write_fake_java(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let arch = match std::env::consts::ARCH {
            "aarch64" => "aarch64",
            "x86_64" => "amd64",
            other => other,
        };
        let script = format!(
            "#!/bin/sh\n\
             if [ \"$1\" = \"-XshowSettings:properties\" ]; then\n\
               echo '    java.version = 21.0.4' >&2\n\
               echo '    java.specification.version = 21' >&2\n\
               echo '    os.arch = {arch}' >&2\n\
               echo '    java.vendor = Islet Test' >&2\n\
               exit 0\n\
             fi\n\
             {body}\n"
        );
        let path = dir.join(name);
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    async fn wait_event<T>(
        events: &Receiver<UiEvent>,
        what: &str,
        limit: Duration,
        mut pick: impl FnMut(UiEvent) -> Option<T>,
    ) -> T {
        let deadline = Instant::now() + limit;
        loop {
            let left = deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_else(|| panic!("timed out waiting for {what}"));
            match tokio::time::timeout(left, events.recv()).await {
                Ok(Ok(event)) => {
                    if let Some(found) = pick(event) {
                        return found;
                    }
                }
                Ok(Err(_)) => panic!("event channel closed while waiting for {what}"),
                Err(_) => panic!("timed out waiting for {what}"),
            }
        }
    }

    #[tokio::test]
    async fn offline_launch_enforces_owner_gate_and_spawns_isolated_process() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(root.path()).unwrap();
        install_minimal_version(&paths);
        let args_dump = root.path().join("captured-args.txt");
        let fake_java = write_fake_java(
            root.path(),
            "fake-java",
            &format!(
                "printf '%s\\n' \"$@\" > \"{}\"\n\
                 echo '[Render thread/INFO]: Setting user: OfflineHero'\n\
                 exit 0",
                args_dump.display()
            ),
        );

        let (sender, events) = async_channel::unbounded();
        let services = AppServices::new(paths.clone(), sender).unwrap();
        let instance = services.create_instance("Offline E2E", VERSION, "release").unwrap();
        services.instances.mark_installed(&instance.id, true).unwrap();
        services
            .configure_instance(
                &instance.id,
                &instance.name,
                Some(2048),
                Some(fake_java.to_string_lossy().into_owned()),
                "vanilla",
            )
            .unwrap();

        // 1. Without an owning Microsoft account, adding an offline account is rejected.
        let err = services.add_offline_account("OfflineHero").unwrap_err();
        assert_eq!(err.code(), "owner_required");

        // 2. With a synthetic owning Microsoft account, offline account creation & launch succeed end-to-end.
        services.accounts.upsert(owner_account()).unwrap();
        let (offline_acc, _) = services.add_offline_account("OfflineHero").unwrap();
        services.set_active_account(Some(offline_acc.id)).unwrap();

        services.launch_instance(&instance.id, None, None, false).unwrap();

        let mut saw_started = false;
        let mut saw_log = false;
        let exit_ok = wait_event(&events, "offline launch exit", Duration::from_secs(10), |ev| match ev {
            UiEvent::LaunchStarted { instance_id, .. } if instance_id == instance.id => {
                saw_started = true;
                None
            }
            UiEvent::LaunchLog { instance_id, line, .. } if instance_id == instance.id => {
                if line.contains("Setting user: OfflineHero") {
                    saw_log = true;
                }
                None
            }
            UiEvent::LaunchError { instance_id, error } if instance_id == instance.id => {
                panic!("unexpected launch error: {error:?}");
            }
            UiEvent::LaunchExit { instance_id, code, success } if instance_id == instance.id => {
                Some(code == Some(0) && success)
            }
            _ => None,
        })
        .await;

        assert!(saw_started, "expected LaunchStarted");
        assert!(saw_log, "expected streamed LaunchLog");
        assert!(exit_ok, "expected clean exit 0");

        let args = std::fs::read_to_string(&args_dump).unwrap();
        assert!(args.contains("--username\nOfflineHero\n"), "args:\n{args}");
        assert!(args.contains("--accessToken\n0\n"), "args:\n{args}");
        assert!(args.contains("-Xmx2048M\n"), "args:\n{args}");
        assert!(!args.contains("-javaagent:"), "offline launch should not attach authlib agent:\n{args}");

        let log_lines = services.read_log(&instance.id, 100).unwrap();
        assert!(
            log_lines.iter().any(|l| l.starts_with("[Islet]") && l.contains("memory=2048MB")),
            "missing launcher header in latest.log: {log_lines:?}"
        );
    }

    #[tokio::test]
    async fn yggdrasil_offline_fallback_launches_without_network_or_javaagent() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(root.path()).unwrap();
        install_minimal_version(&paths);
        let args_dump = root.path().join("yggdrasil-offline-args.txt");
        let fake_java = write_fake_java(
            root.path(),
            "fake-java-ygg",
            &format!(
                "printf '%s\\n' \"$@\" > \"{}\"\n\
                 echo '[Render thread/INFO]: Offline fallback ok'\n\
                 exit 0",
                args_dump.display()
            ),
        );

        let (sender, events) = async_channel::unbounded();
        let services = AppServices::new(paths.clone(), sender).unwrap();
        // Point proxy to an unreachable loopback port so any unexpected network download fails immediately.
        let mut settings = services.settings.get();
        settings.proxy_mode = islet::core::settings::ProxyMode::Custom;
        settings.proxy_url = "http://127.0.0.1:1".into();
        services.update_settings(settings).unwrap();

        let ygg = unreachable_yggdrasil_account();
        // 1. Without an owning Microsoft account, both adding and launching a Yggdrasil account are rejected.
        let login_err = services
            .login_yggdrasil("https://littleskin.cn/api/yggdrasil", "user", "pass", None)
            .await
            .unwrap_err();
        assert_eq!(login_err.code(), "owner_required");

        services.accounts.upsert(ygg.clone()).unwrap();
        services.set_active_account(Some(ygg.id)).unwrap();

        let instance = services.create_instance("Ygg Offline", VERSION, "release").unwrap();
        services.instances.mark_installed(&instance.id, true).unwrap();
        services
            .configure_instance(
                &instance.id,
                &instance.name,
                Some(2048),
                Some(fake_java.to_string_lossy().into_owned()),
                "vanilla",
            )
            .unwrap();

        services.launch_instance(&instance.id, None, None, true).unwrap();
        let blocked_code = wait_event(&events, "yggdrasil owner gate error", Duration::from_secs(10), |ev| match ev {
            UiEvent::LaunchError { instance_id, error } if instance_id == instance.id => Some(error.code),
            UiEvent::LaunchStarted { instance_id, .. } if instance_id == instance.id => {
                panic!("Yggdrasil launch must not start without an owning Microsoft account");
            }
            _ => None,
        })
        .await;
        assert_eq!(blocked_code, "owner_required");

        // 2. Once an owning Microsoft account is present, offline fallback launches without network or javaagent.
        services.accounts.upsert(owner_account()).unwrap();
        services.launch_instance(&instance.id, None, None, true).unwrap();

        let exit_ok = wait_event(&events, "yggdrasil offline exit", Duration::from_secs(10), |ev| match ev {
            UiEvent::LaunchError { instance_id, error } if instance_id == instance.id => {
                panic!("offline fallback should not require network or authlib-injector, got: {error:?}");
            }
            UiEvent::LaunchExit { instance_id, code, success } if instance_id == instance.id => {
                Some(code == Some(0) && success)
            }
            _ => None,
        })
        .await;

        assert!(exit_ok);
        let args = std::fs::read_to_string(&args_dump).unwrap();
        assert!(args.contains("--username\nThirdPartySteve\n"), "args:\n{args}");
        assert!(
            !args.contains("-javaagent:"),
            "offline fallback must not inject Yggdrasil javaagent:\n{args}"
        );
    }

    #[tokio::test]
    async fn crashing_process_end_to_end_flows_into_crash_diagnosis_and_second_granularity_mtime() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(root.path()).unwrap();
        install_minimal_version(&paths);
        let fake_java = write_fake_java(
            root.path(),
            "fake-java-crash",
            "mkdir -p crash-reports\n\
             cat <<'EOF' > crash-reports/crash-2026-e2e.txt\n\
             ---- Minecraft Crash Report ----\n\
             Description: Initializing game\n\
             org.spongepowered.asm.mixin.transformer.throwables.MixinTransformerError: OptiFine is not compatible with Iris / Sodium\n\
             EOF\n\
             echo 'net.minecraftforge.fml.config.ConfigFileTypeHandler$ConfigLoadingException: Failed loading config file create-client.toml of type CLIENT' >&2\n\
             echo 'Caused by: com.electronwill.nightconfig.core.io.ParsingException: Not enough data available' >&2\n\
             exit 1",
        );

        let (sender, events) = async_channel::unbounded();
        let services = AppServices::new(paths.clone(), sender).unwrap();
        services.accounts.upsert(owner_account()).unwrap();
        let (acc, _) = services.add_offline_account("CrashTester").unwrap();
        services.set_active_account(Some(acc.id)).unwrap();

        let instance = services.create_instance("Crash E2E", VERSION, "release").unwrap();
        services.instances.mark_installed(&instance.id, true).unwrap();
        services
            .configure_instance(
                &instance.id,
                &instance.name,
                Some(2048),
                Some(fake_java.to_string_lossy().into_owned()),
                "vanilla",
            )
            .unwrap();
        // Ensure launch config hook works cleanly alongside crash capture.
        services
            .configure_launch(&instance.id, LaunchConfig::default(), None)
            .unwrap();

        let since_ms = islet::core::util::now_millis();
        services.launch_instance(&instance.id, None, None, false).unwrap();

        let exit_code = wait_event(&events, "crash launch exit", Duration::from_secs(10), |ev| match ev {
            UiEvent::LaunchError { instance_id, error } if instance_id == instance.id => {
                panic!("unexpected launch error before spawn: {error:?}");
            }
            UiEvent::LaunchExit { instance_id, code, success } if instance_id == instance.id => {
                assert!(!success);
                Some(code)
            }
            _ => None,
        })
        .await;
        assert_eq!(exit_code, Some(1));

        // Normal millisecond timestamp diagnosis.
        let analysis = services.analyze_crash(&instance.id, since_ms).unwrap();
        assert!(
            analysis
                .diagnoses
                .iter()
                .any(|d| d.kind == "modConflict" && d.fix == Some(Fix::OpenMods)),
            "expected OptiFine conflict diagnosis, got: {analysis:?}"
        );
        assert!(
            analysis.diagnoses.iter().any(|d| {
                d.kind == "corruptConfig"
                    && d.title.contains("create-client.toml")
                    && d.fix == Some(Fix::OpenTools)
            }),
            "expected corruptConfig OpenTools diagnosis, got: {analysis:?}"
        );
        assert!(analysis.excerpt.contains("Minecraft Crash Report"));
        assert!(analysis.excerpt.contains("create-client.toml"));

        // Simulated 1-second-granularity filesystem (e.g. FAT32/exFAT/HFS+):
        // file mtime truncated to whole second while JS Date.now() was 450ms into that same second.
        let log_path = paths.instance_logs_dir(&instance.id).join("latest.log");
        let report_path = paths
            .instance_game_dir(&instance.id)
            .join("crash-reports/crash-2026-e2e.txt");
        let base_sec = (islet::core::util::now_millis() / 1000) as u64;
        let truncated = std::time::UNIX_EPOCH + Duration::from_secs(base_sec);
        for file in [&log_path, &report_path] {
            std::fs::File::options()
                .write(true)
                .open(file)
                .unwrap()
                .set_modified(truncated)
                .unwrap();
        }
        let same_second_since_ms = (base_sec as i64) * 1000 + 450;
        let truncated_analysis = services
            .analyze_crash(&instance.id, same_second_since_ms)
            .unwrap();
        assert_eq!(
            truncated_analysis.diagnoses.len(),
            analysis.diagnoses.len(),
            "1-second mtime truncation must not discard a crash written in the same second: {truncated_analysis:?}"
        );

        // Stale crash from an earlier second must still be rejected.
        let next_second_since_ms = ((base_sec + 2) as i64) * 1000 + 100;
        let stale_analysis = services
            .analyze_crash(&instance.id, next_second_since_ms)
            .unwrap();
        assert!(
            stale_analysis.diagnoses.is_empty() && stale_analysis.excerpt.is_empty(),
            "older crash must remain excluded after second-floor alignment: {stale_analysis:?}"
        );
    }
}

#[tokio::test]
async fn lan_discovery_hears_local_broadcast_and_filters_p2p_and_closed_ports() {
    // 1. Live local TCP listener simulating a Minecraft "Open to LAN" world.
    let live_listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let live_port = live_listener.local_addr().unwrap().port();
    assert!(lan::listening(live_port).await);

    // 2. Closed port simulating a stale or remote LAN announcement.
    let dead_port = {
        let tmp = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        tmp.local_addr().unwrap().port()
    };
    assert!(!lan::listening(dead_port).await);

    // 3. Concurrently run `lan::detect`, real `lan::announce` (guest relay),
    //    and raw Minecraft multicast broadcasts.
    let announce_task = tokio::spawn(lan::announce(
        std::sync::Arc::new(std::sync::Mutex::new("Live Relay".into())),
        live_port,
    ));
    let detect_task = tokio::spawn(lan::detect(Duration::from_millis(1200)));
    tokio::time::sleep(Duration::from_millis(150)).await;

    let default_sender = tokio::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await.ok();
    let loopback_sender = {
        use socket2::{Domain, Protocol, Socket, Type};
        Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
            .and_then(|sock| {
                sock.bind(&std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, 0)).into())?;
                let _ = sock.set_multicast_loop_v4(true);
                let _ = sock.set_multicast_if_v4(&Ipv4Addr::LOCALHOST);
                sock.set_nonblocking(true)?;
                tokio::net::UdpSocket::from_std(sock.into())
            })
            .ok()
    };
    let target = SocketAddrV4::new(Ipv4Addr::new(224, 0, 2, 60), 4445);
    let packets = [
        format!("[MOTD]Steve - Isolated E2E[/MOTD][AD]{live_port}[/AD]"),
        lan::announcement("Relayed Room", live_port),
        format!("[MOTD]Ghost World[/MOTD][AD]{dead_port}[/AD]"),
    ];
    for _ in 0..3 {
        for pkt in &packets {
            if let Some(sock) = &default_sender {
                let _ = sock.send_to(pkt.as_bytes(), target).await;
            }
            if let Some(sock) = &loopback_sender {
                let _ = sock.send_to(pkt.as_bytes(), target).await;
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let result = detect_task.await.unwrap();
    announce_task.abort();
    drop(live_listener);
    match result {
        Ok(games) => {
            assert!(
                games.contains(&LanGame {
                    motd: "Steve - Isolated E2E".into(),
                    port: live_port,
                }),
                "expected local LAN broadcast to be discovered, got: {games:?}"
            );
            assert!(
                !games.iter().any(|g| g.motd.ends_with(RELAY_MARK)),
                "P2P relay announcements must be filtered out: {games:?}"
            );
            assert!(
                !games.iter().any(|g| g.port == dead_port),
                "non-listening ports must be filtered out: {games:?}"
            );
        }
        Err(err) => {
            // In restricted sandboxes where multicast join/bind is denied, the error must
            // remain structured (`lan_listen`) with manual-port guidance.
            assert_eq!(err.code(), "lan_listen", "unexpected LAN error: {err:?}");
        }
    }
}
