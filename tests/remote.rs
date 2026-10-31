//! A world deployed to a Linux host over SSH, end to end: a throwaway sshd
//! on this machine (its own host key and authorized keys, nothing of the
//! user's touched) takes a Fabric server, which is then started, commanded,
//! backed up, updated and stopped; a friend imports the pack it left and
//! gets missing mods back before joining. Needs Linux with OpenSSH and
//! downloads Minecraft and a Java 17:
//!
//! `cargo test --test remote -- --ignored --nocapture`
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use async_channel::Receiver;
use islet::core::paths::AppPaths;
use islet::core::remote::{Session, Target};
use islet::core::settings::{DownloadSource, Settings};
use islet::services::{AppServices, DeployOptions, FileOperation, UiEvent};

/// Needs Java 17, so the host gets one installed.
const VERSION: &str = "1.20.1";

struct Sshd(Child);

impl Drop for Sshd {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn keygen(file: &Path) {
    let status = Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", "", "-f"]).arg(file).status().unwrap();
    assert!(status.success());
}

/// sshd on 127.0.0.1:`port` that lets in only the key at `dir/client`.
fn sshd(dir: &Path, port: u16) -> Sshd {
    keygen(&dir.join("host"));
    keygen(&dir.join("client"));
    std::fs::copy(dir.join("client.pub"), dir.join("authorized_keys")).unwrap();
    let config = dir.join("sshd_config");
    std::fs::write(
        &config,
        format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {d}/host\nAuthorizedKeysFile {d}/authorized_keys\nPidFile {d}/sshd.pid\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nStrictModes no\n",
            d = dir.display()
        ),
    )
    .unwrap();
    let child = Command::new("/usr/sbin/sshd").args(["-D", "-e", "-f"]).arg(&config).stdout(Stdio::null()).spawn().unwrap();
    let started = Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(started.elapsed() < Duration::from_secs(10), "sshd didn't start");
        std::thread::sleep(Duration::from_millis(100));
    }
    Sshd(child)
}

fn wait<T>(events: &Receiver<UiEvent>, runtime: &tokio::runtime::Runtime, what: &str, limit: Duration, mut pick: impl FnMut(UiEvent) -> Option<T>) -> T {
    let started = Instant::now();
    loop {
        let left = limit.checked_sub(started.elapsed()).unwrap_or_else(|| panic!("timed out waiting for {what}"));
        match runtime.block_on(async { tokio::time::timeout(left, events.recv()).await }) {
            Ok(Ok(event)) => {
                if let Some(found) = pick(event) {
                    return found;
                }
            }
            _ => panic!("timed out waiting for {what}"),
        }
    }
}

fn operation(events: &Receiver<UiEvent>, runtime: &tokio::runtime::Runtime) -> String {
    wait(events, runtime, "the operation", Duration::from_secs(30 * 60), |event| match event {
        UiEvent::RemoteProgress { stage, done, total, .. } => {
            println!("  {stage} {done}/{total}");
            None
        }
        UiEvent::OperationComplete(result) => Some(result.unwrap_or_else(|error| panic!("operation failed: {error:?}"))),
        _ => None,
    })
}

/// The newest Fabric API for the version, from Modrinth.
fn fabric_api(runtime: &tokio::runtime::Runtime, into: &Path) {
    let url = format!("https://api.modrinth.com/v2/project/fabric-api/version?loaders=[\"fabric\"]&game_versions=[\"{VERSION}\"]");
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

#[test]
#[ignore = "runs sshd and a Minecraft server; downloads about 300 MB"]
fn deploy_manage_update_and_join() {
    let root = tempfile::tempdir().unwrap();
    let ssh = root.path().join("ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    let ssh_port = 22000 + (std::process::id() % 1000) as u16;
    let game_port = 25600 + (std::process::id() % 300) as u16;
    let _sshd = sshd(&ssh, ssh_port);

    let paths = AppPaths::from_root(root.path().join("data")).unwrap();
    let settings = Settings { download_source: DownloadSource::Official, ..Settings::default() };
    std::fs::write(paths.settings_file(), serde_json::to_vec(&settings).unwrap()).unwrap();
    let (sender, events) = async_channel::unbounded();
    let services = AppServices::new(paths.clone(), sender).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();

    let world = services.create_instance("Remote", VERSION, "release").unwrap();
    services.configure_instance(&world.id, "Remote", None, None, "fabric").unwrap();
    fabric_api(&runtime, &paths.instance_game_dir(&world.id).join("mods"));
    services.install_instance(&world.id).unwrap();
    wait(&events, &runtime, "the install", Duration::from_secs(30 * 60), |event| match event {
        UiEvent::InstallComplete { instance_id, .. } if instance_id == world.id => Some(()),
        UiEvent::InstallError { error, .. } => panic!("install failed: {error:?}"),
        _ => None,
    });

    // Connect: the host key is remembered.
    let host_dir = root.path().join("host").join("server");
    let target = Target {
        host: "127.0.0.1".into(),
        port: ssh_port,
        user: std::env::var("USER").unwrap(),
        auth: "key".into(),
        key_path: Some(ssh.join("client").display().to_string()),
        dir: host_dir.display().to_string(),
        game_port,
        memory_mb: 1536,
        ..Target::default()
    };
    let probe = runtime.block_on(services.remote_connect(&world.id, target.clone(), None)).unwrap();
    println!("probe: {probe:?}");
    assert_eq!(probe.os, "Linux");
    assert!(!probe.deployed);
    let info = services.remote_info(&world.id).unwrap();
    assert!(info.target.as_ref().unwrap().fingerprint.as_deref().unwrap().starts_with("SHA256:"));

    // A host with another key is refused.
    let mut forged = info.target.clone().unwrap();
    forged.fingerprint = Some("SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into());
    assert!(runtime.block_on(Session::connect(&forged, None)).is_err(), "a changed host key must stop the connection");

    // Deploy: Java 17 goes on the host, the server starts and answers.
    services.file_operation(&world.id, FileOperation::DeployServer(DeployOptions { accept_eula: true, world: None })).unwrap();
    println!("deploy: {}", operation(&events, &runtime));
    assert!(host_dir.join("java/bin/java").exists(), "Java 17 was installed in the server folder");
    let status = runtime.block_on(services.remote_status(&world.id)).unwrap();
    assert!(status.state.running && status.state.ready, "{status:?}");
    assert!(status.ping.is_some(), "the server answers at {}", status.address);
    let pack = services.remote_info(&world.id).unwrap().pack.expect("a pack for friends");

    // Console, backup, download.
    runtime.block_on(services.remote_command(&world.id, "say hello from islet")).unwrap();
    std::thread::sleep(Duration::from_secs(2));
    let log = runtime.block_on(services.remote_log(&world.id, 50)).unwrap();
    assert!(log.contains("hello from islet"), "{log}");
    let backup = runtime.block_on(services.remote_action(&world.id, "backup")).unwrap();
    let status = runtime.block_on(services.remote_status(&world.id)).unwrap();
    assert_eq!(status.state.backups.len(), 1, "{backup}");
    let copy = root.path().join("backup.tar.gz");
    runtime.block_on(services.remote_download_backup(&world.id, &status.state.backups[0].0, &copy)).unwrap();
    let names: Vec<String> = tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(&copy).unwrap()))
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().display().to_string())
        .collect();
    assert!(names.iter().any(|n| n == "world/level.dat"), "{names:?}");

    // The host's own edits and the world survive an update.
    std::fs::write(host_dir.join("ops.json"), "[]").unwrap();
    let properties = std::fs::read_to_string(host_dir.join("server.properties")).unwrap();
    std::fs::write(host_dir.join("server.properties"), properties.replace("motd=", "motd=kept-by-host ")).unwrap();
    std::fs::write(host_dir.join("mods/stale.jar"), b"old").unwrap();
    services.file_operation(&world.id, FileOperation::DeployServer(DeployOptions { accept_eula: true, world: None })).unwrap();
    println!("update: {}", operation(&events, &runtime));
    assert!(std::fs::read_to_string(host_dir.join("server.properties")).unwrap().contains("kept-by-host"));
    assert!(host_dir.join("world/level.dat").exists());
    assert!(!host_dir.join("mods/stale.jar").exists(), "mods are replaced, not merged");

    // Stop.
    runtime.block_on(services.remote_action(&world.id, "stop")).unwrap();
    assert!(!runtime.block_on(services.remote_status(&world.id)).unwrap().state.running);

    // A friend imports the pack: nothing is missing until a mod goes.
    let before: Vec<String> = services.snapshot().instances.into_iter().map(|i| i.id).collect();
    services.file_operation(&world.id, FileOperation::ImportPack(pack.into())).unwrap();
    operation(&events, &runtime);
    let friend = services.snapshot().instances.into_iter().find(|i| !before.contains(&i.id)).expect("the imported world");
    services.install_instance(&friend.id).unwrap();
    wait(&events, &runtime, "the friend's install", Duration::from_secs(30 * 60), |event| match event {
        UiEvent::InstallComplete { instance_id, .. } if instance_id == friend.id => Some(()),
        UiEvent::InstallError { error, .. } => panic!("install failed: {error:?}"),
        _ => None,
    });
    let address = format!("127.0.0.1:{game_port}");
    assert!(services.check_server_mods(&friend.id, &address).unwrap().is_none());
    let mods = paths.instance_game_dir(&friend.id).join("mods");
    let api = mods.read_dir().unwrap().flatten().find(|e| e.file_name().to_string_lossy().starts_with("fabric-api")).unwrap().path();
    std::fs::remove_file(&api).unwrap();
    let check = services.check_server_mods(&friend.id, &address).unwrap().expect("fabric-api is missing");
    assert!(check.fixable, "{check:?}");
    services.file_operation(&friend.id, FileOperation::FixServerMods { address: address.clone() }).unwrap();
    println!("fix: {}", operation(&events, &runtime));
    assert!(services.check_server_mods(&friend.id, &address).unwrap().is_none());
    assert!(api.exists());

    services.remote_forget(&world.id).unwrap();
    assert!(services.remote_info(&world.id).unwrap().target.is_none());
}
