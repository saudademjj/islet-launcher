//! End to end on a real computer: install Minecraft, start it with an
//! offline account until it renders, then export a Fabric world and a
//! NeoForge world as servers and run each until it is up. Downloads about
//! 1 GB and needs a display (Xvfb works):
//!
//! `xvfb-run -a cargo test --test e2e -- --ignored --nocapture`
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use async_channel::Receiver;
use islet::core::accounts::{Account, AccountProvider};
use islet::core::paths::AppPaths;
use islet::core::server_pack::ServerOptions;
use islet::core::settings::{DownloadSource, Settings};
use islet::services::{AppServices, FileOperation, UiEvent};

const VERSION: &str = "1.21.1";

struct Waiter {
    runtime: tokio::runtime::Runtime,
    events: Receiver<UiEvent>,
}

impl Waiter {
    /// The first event `pick` takes within `limit`.
    fn until<T>(&self, what: &str, limit: Duration, mut pick: impl FnMut(UiEvent) -> Option<T>) -> T {
        let started = Instant::now();
        loop {
            let left = limit.checked_sub(started.elapsed()).unwrap_or_else(|| panic!("timed out waiting for {what}"));
            match self.runtime.block_on(async { tokio::time::timeout(left, self.events.recv()).await }) {
                Ok(Ok(event)) => {
                    if let Some(found) = pick(event) {
                        return found;
                    }
                }
                Ok(Err(_)) => panic!("the event channel closed while waiting for {what}"),
                Err(_) => panic!("timed out waiting for {what}"),
            }
        }
    }

    fn install(&self, services: &std::sync::Arc<AppServices>, id: &str) {
        services.install_instance(id).unwrap();
        self.until("the install", Duration::from_secs(30 * 60), |event| match event {
            UiEvent::InstallComplete { instance_id, .. } if instance_id == id => Some(()),
            UiEvent::InstallError { instance_id, error, .. } if instance_id == id => panic!("install failed: {error:?}"),
            _ => None,
        });
    }

    fn operation(&self) -> String {
        self.until("the operation", Duration::from_secs(20 * 60), |event| match event {
            UiEvent::OperationComplete(result) => Some(result.unwrap_or_else(|error| panic!("operation failed: {error:?}"))),
            _ => None,
        })
    }
}

/// A mod's newest file for the version and loader, from Modrinth.
/// A Microsoft account that owns the game, as a sign-in would save it.
fn owner() -> Account {
    Account {
        id: "microsoft:00000000-0000-0000-0000-0000000000e2".into(),
        provider: AccountProvider::Microsoft,
        username: "Islet_Owner".into(),
        uuid: "00000000-0000-0000-0000-0000000000e2".into(),
        xuid: None,
        ownership: Some(true),
        skin_url: None,
        skin_slim: false,
        created_at: 0,
        server: None,
        server_name: None,
    }
}

fn modrinth_file(runtime: &tokio::runtime::Runtime, slug: &str, loader: &str, into: &Path) {
    let url = format!("https://api.modrinth.com/v2/project/{slug}/version?loaders=[\"{loader}\"]&game_versions=[\"{VERSION}\"]");
    let (name, bytes) = runtime.block_on(async {
        let client = reqwest::Client::builder().user_agent("saudademjj/islet-launcher e2e").build().unwrap();
        let versions: serde_json::Value = client.get(url).send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
        let file = &versions[0]["files"][0];
        let bytes = client.get(file["url"].as_str().unwrap()).send().await.unwrap().bytes().await.unwrap();
        (file["filename"].as_str().unwrap().to_string(), bytes)
    });
    std::fs::create_dir_all(into).unwrap();
    std::fs::write(into.join(name), bytes).unwrap();
}

/// Runs a server's start script until it says it is up, then stops it.
fn run_server(dir: &Path) {
    let mut child = Command::new("sh").arg("start.sh").current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().unwrap();
    let (lines, read) = mpsc::channel();
    let stdout = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = lines.send(line);
        }
    });
    let started = Instant::now();
    let mut tail = Vec::new();
    loop {
        match read.recv_timeout(Duration::from_secs(5)) {
            Ok(line) => {
                println!("server: {line}");
                if line.contains("Done (") {
                    break;
                }
                tail.push(line);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!("the server stopped before it was up: {:?}", &tail[tail.len().saturating_sub(20)..]),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        assert!(started.elapsed() < Duration::from_secs(10 * 60), "the server never came up");
    }
    child.stdin.as_mut().unwrap().write_all(b"stop\n").unwrap();
    let status = child.wait().unwrap();
    assert!(status.success(), "the server exited with {status}");
}

#[test]
#[ignore = "downloads Minecraft and runs it; needs a display"]
fn install_play_and_host() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_root(root.path()).unwrap();
    // From a data centre abroad the official servers are the quick ones.
    let settings = Settings { download_source: DownloadSource::Official, ..Settings::default() };
    std::fs::write(paths.settings_file(), serde_json::to_vec(&settings).unwrap()).unwrap();
    let (sender, events) = async_channel::unbounded();
    let services = AppServices::new(paths.clone(), sender).unwrap();
    let waiter = Waiter { runtime: tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap(), events };

    // The game: install, start offline, wait for it to render, stop it.
    // Offline accounts need an owning Microsoft account in the list first.
    services.accounts.upsert(owner()).unwrap();
    let (account, _) = services.add_offline_account("Islet_E2E").unwrap();
    services.set_active_account(Some(account.id)).unwrap();
    let vanilla = services.create_instance("E2E", VERSION, "release").unwrap();
    waiter.install(&services, &vanilla.id);
    services.launch_instance(&vanilla.id, None, None, false).unwrap();
    let mut log = Vec::new();
    waiter.until("the game to render", Duration::from_secs(10 * 60), |event| match event {
        UiEvent::LaunchLog { line, .. } => {
            println!("game: {line}");
            let rendered = line.contains("Created: ") && line.contains("textures/atlas");
            log.push(line);
            rendered.then_some(())
        }
        UiEvent::LaunchError { error, .. } => panic!("launch failed: {error:?}"),
        UiEvent::LaunchExit { code, .. } => panic!("the game exited with {code:?}: {:?}", &log[log.len().saturating_sub(30)..]),
        _ => None,
    });
    services.stop_instance(&vanilla.id, true).unwrap();
    waiter.until("the game to exit", Duration::from_secs(60), |event| matches!(event, UiEvent::LaunchExit { .. }).then_some(()));

    // Servers: a Fabric world with a client-only mod, and a NeoForge world.
    let servers = root.path().join("servers");
    for (loader, mods) in [("fabric", &["fabric-api", "sodium"][..]), ("neoforge", &[][..])] {
        let world = services.create_instance(loader, VERSION, "release").unwrap();
        services.configure_instance(&world.id, loader, None, None, loader).unwrap();
        for slug in mods {
            modrinth_file(&waiter.runtime, slug, loader, &paths.instance_game_dir(&world.id).join("mods"));
        }
        waiter.install(&services, &world.id);
        let dir = servers.join(loader);
        services
            .file_operation(&world.id, FileOperation::ExportServer { destination: dir.clone(), options: ServerOptions { accept_eula: true, memory_mb: 2048, ..Default::default() } })
            .unwrap();
        let message = waiter.operation();
        println!("{loader}: {message}");
        if loader == "fabric" {
            assert!(dir.join("mods").read_dir().unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("fabric-api")));
            assert!(!dir.join("mods").read_dir().unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("sodium")), "sodium is client-only");
        }
        run_server(&dir);
    }
}
