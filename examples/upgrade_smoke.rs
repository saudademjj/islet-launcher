//! Opt-in integration acceptance, always with an explicit isolated data root.
use islet::{
    core::{content::ContentKind, java::detect, loaders, modrinth, paths::AppPaths},
    services::{AppServices, UiEvent},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .try_init();
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(args.get(1).ok_or("supply isolated data root")?);
    let action = args.get(2).map(String::as_str).unwrap_or("metadata");
    let loader = args.get(3).map(String::as_str).unwrap_or("fabric");
    let mc = args.get(4).map(String::as_str).unwrap_or("1.20.1");
    let paths = AppPaths::from_root(root)?;
    let client = reqwest::Client::builder()
        .user_agent("saudade/Islet/0.3.0 acceptance")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(600))
        .build()?;
    let rt = tokio::runtime::Runtime::new()?;
    if action == "java" {
        let found = detect::find_all_in(&[paths.java.clone()], &[]);
        for major in [17, 21] {
            let java = found
                .iter()
                .find(|j| j.major == major)
                .ok_or("managed Java missing")?;
            let binary = PathBuf::from(&java.path);
            let home = binary.parent().unwrap().parent().unwrap();
            assert_eq!(detect::inspect(home).unwrap().major, major);
            #[cfg(windows)]
            assert_eq!(
                detect::inspect(&binary.with_file_name("javaw.exe"))
                    .unwrap()
                    .major,
                major
            );
            islet::core::java::runtime::validate(java, major)?;
            assert!(
                islet::core::java::runtime::validate(java, if major == 17 { 21 } else { 17 })
                    .is_err()
            );
            println!(
                "PASS Java {major}: cache discovery, Java home, javaw normalization, mismatch rejection"
            );
        }
        return Ok(());
    }
    if action == "metadata" {
        for (kind, game) in [
            ("fabric", "1.20.1"),
            ("forge", "1.20.1"),
            ("neoforge", "1.21.1"),
        ] {
            let list = rt.block_on(loaders::list(&client, kind, game))?;
            assert!(!list.is_empty());
            println!(
                "PASS {kind} / {game}: {} versions; first {}",
                list.len(),
                list[0].version
            );
        }
        return Ok(());
    }
    let (tx, rx) = async_channel::unbounded();
    let svc = AppServices::new(paths.clone(), tx)?;
    let name = format!("Acceptance {loader} {mc}");
    let instance = svc
        .instances
        .list()
        .into_iter()
        .find(|i| i.name == name)
        .map(Ok)
        .unwrap_or_else(|| svc.create_instance(&name, mc, "release"))?;
    svc.configure_instance(&instance.id, &name, Some(2048), Some("auto".into()), loader)?;
    let instance = svc.instances.get(&instance.id)?;
    if action == "install" {
        svc.install_instance(&instance.id)?;
        let deadline = Instant::now() + Duration::from_secs(2400);
        let mut last = Instant::now() - Duration::from_secs(10);
        let mut stage = String::new();
        loop {
            if Instant::now() > deadline {
                return Err("install timeout".into());
            }
            match rx.try_recv() {
                Ok(UiEvent::InstallProgress(p)) => {
                    if last.elapsed() > Duration::from_secs(10) || stage != p.stage {
                        println!(
                            "{} {} files={}/{} bytes={}/{}",
                            p.stage, p.message, p.done, p.total, p.bytes, p.total_bytes
                        );
                        last = Instant::now();
                        stage = p.stage;
                    }
                }
                Ok(UiEvent::InstallComplete { .. }) => {
                    println!("PASS installed {loader} {mc}");
                    break;
                }
                Ok(UiEvent::InstallError { error, .. }) => {
                    return Err(format!("{} {:?}", error.message, error.detail).into());
                }
                _ => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        let found = detect::find_all_in(&[paths.java.clone()], &[]);
        assert!(!found.is_empty());
        for j in found {
            println!("JAVA {} {} {}", j.major, j.version, j.path);
        }
    } else if action == "mods" {
        let project = args.get(5).map(String::as_str).unwrap_or("modmenu");
        let result = rt.block_on(modrinth::search(&client, &instance, project, 0, ContentKind::Mod, &[], "relevance"))?;
        assert!(!result.hits.is_empty());
        println!("PASS search {} hits", result.hits.len());
        let versions = rt.block_on(modrinth::versions(&client, &instance, project, ContentKind::Mod))?;
        let v = versions.first().ok_or("no compatible mod version")?;
        println!("Selected {} {}", v.name, v.id);
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = rt.block_on(modrinth::install(
            &client, &paths, &instance, std::slice::from_ref(&v.id), &cancel,
        ))?;
        println!("PASS {result}");
    } else if action == "launch" {
        svc.add_offline_account("AcceptanceUser")?;
        svc.launch_instance(&instance.id, None, None, false)?;
        let deadline = Instant::now() + Duration::from_secs(90);
        let mut ready = false;
        let mut stopped = false;
        loop {
            if Instant::now() > deadline {
                let _ = svc.stop_instance(&instance.id, false);
                return Err("launch timeout".into());
            }
            match rx.try_recv() {
                Ok(UiEvent::LaunchStarted { pid, .. }) => println!("START pid={pid}"),
                Ok(UiEvent::LaunchLog { line, .. }) => {
                    if line.contains("Created:")
                        || line.contains("OpenAL initialized")
                        || line.contains("Sound engine started")
                    {
                        ready = true;
                    }
                    println!("{line}");
                    if ready && !stopped {
                        std::thread::sleep(Duration::from_secs(3));
                        svc.stop_instance(&instance.id, false)?;
                        stopped = true;
                    }
                }
                Ok(UiEvent::LaunchExit { code, .. }) => {
                    if !ready {
                        return Err(format!("exited before renderer ready: {code:?}").into());
                    }
                    println!("PASS renderer initialized {loader} {mc}");
                    break;
                }
                Ok(UiEvent::LaunchError { error, .. }) => {
                    return Err(format!("{} {:?}", error.message, error.detail).into());
                }
                _ => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }
    Ok(())
}
