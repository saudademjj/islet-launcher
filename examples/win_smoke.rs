//! Windows smoke test for the launcher services. Lives only in the Windows test
//! copy (examples/), never in the user's source tree. Uses a throwaway data root.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use islet::core::paths::AppPaths;
use islet::services::{AppServices, FileOperation, UiEvent};

type Rx = async_channel::Receiver<UiEvent>;

fn wait<T>(rx: &Rx, timeout: Duration, mut f: impl FnMut(UiEvent) -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match rx.try_recv() {
            Ok(event) => {
                if let Some(value) = f(event) {
                    return Some(value);
                }
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    None
}

fn op(
    services: &Arc<AppServices>,
    rx: &Rx,
    id: &str,
    operation: FileOperation,
) -> Result<String, String> {
    services
        .file_operation(id, operation)
        .map_err(|e| e.user_message())?;
    wait(rx, Duration::from_secs(600), |event| match event {
        UiEvent::OperationComplete(result) => {
            Some(result.map_err(|e| format!("{} | {}", e.message, e.detail.unwrap_or_default())))
        }
        _ => None,
    })
    .unwrap_or_else(|| Err("timeout".into()))
}

fn install(services: &Arc<AppServices>, rx: &Rx, id: &str) -> Result<(), String> {
    let started = Instant::now();
    services
        .install_instance(id)
        .map_err(|e| e.user_message())?;
    let mut last_stage = String::new();
    let mut last_print = Instant::now();
    let result = wait(rx, Duration::from_secs(1800), |event| match event {
        UiEvent::InstallProgress(p) => {
            if p.stage != last_stage || last_print.elapsed() > Duration::from_secs(10) {
                println!(
                    "  [{}] {} files {}/{} bytes {}/{}",
                    p.stage, p.message, p.done, p.total, p.bytes, p.total_bytes
                );
                last_stage = p.stage;
                last_print = Instant::now();
            }
            None
        }
        UiEvent::InstallComplete { .. } => Some(Ok(())),
        UiEvent::InstallError { error, .. } => Some(Err(format!(
            "{} ({}) | {}",
            error.message,
            error.code,
            error.detail.unwrap_or_default()
        ))),
        _ => None,
    })
    .unwrap_or_else(|| Err("install timeout".into()));
    println!("  install took {:?}", started.elapsed());
    result
}

fn launch(services: &Arc<AppServices>, rx: &Rx, id: &str, log_dir: &Path) {
    if let Err(e) = services.launch_instance(id, None, None, false) {
        println!("  LAUNCH REFUSED: {}", e.user_message());
        return;
    }
    let mut lines = Vec::new();
    {
        // Stop independently of log events; a hung game may print nothing.
        let services = services.clone();
        let id = id.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(60));
            if services.stop_instance(&id, false).is_ok() {
                println!("  still running after 60s, sent stop");
            }
        });
    }
    let outcome = wait(rx, Duration::from_secs(150), |event| {
        match event {
            UiEvent::LaunchStarted { pid, .. } => println!("  started pid={pid}"),
            UiEvent::LaunchLog { line, .. } => lines.push(line),
            UiEvent::LaunchExit { code, .. } => return Some(format!("exit code {code:?}")),
            UiEvent::LaunchError { error, .. } => {
                return Some(format!(
                    "LAUNCH ERROR {} | {}",
                    error.message,
                    error.detail.unwrap_or_default()
                ));
            }
            _ => {}
        }
        None
    });
    println!("  outcome: {}", outcome.unwrap_or_else(|| "timeout".into()));
    let replaced = lines.iter().filter(|l| l.contains('\u{FFFD}')).count();
    let non_ascii = lines.iter().filter(|l| !l.is_ascii()).count();
    println!(
        "  log lines={} non_ascii={} with_replacement_char={}",
        lines.len(),
        non_ascii,
        replaced
    );
    for line in lines.iter().filter(|l| !l.is_ascii()).take(5) {
        println!(
            "  | non-ascii: {}",
            line.chars().take(160).collect::<String>()
        );
    }
    for line in lines
        .iter()
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        println!("  | {}", line.chars().take(200).collect::<String>());
    }
    let file = log_dir.join("latest.log");
    let file_lines = std::fs::read(&file)
        .map(|b| String::from_utf8_lossy(&b).lines().count())
        .unwrap_or(0);
    println!("  latest.log lines={file_lines} (header + captured)");
}

fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with('.'))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(windows)]
fn hold_open(path: &Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    // Share read/write but not delete, like a running game or scanner.
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(path)
        .unwrap()
}
#[cfg(not(windows))]
fn hold_open(path: &Path) -> std::fs::File {
    std::fs::File::open(path).unwrap()
}

fn old_style_probe(java: &Path) -> String {
    // The pre-fix implementation: wait for exit first, read pipes afterwards.
    use std::process::Stdio;
    let mut child = islet::core::platform::quiet_command(java)
        .args(["-XshowSettings:properties", "-version"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            let out = child.wait_with_output().unwrap();
            return format!(
                "finished in {:?}, stderr {} bytes",
                started.elapsed(),
                out.stderr.len()
            );
        }
        if started.elapsed() > Duration::from_secs(4) {
            let _ = child.kill();
            let _ = child.wait();
            return "TIMED OUT after 4s (blocked on full pipe)".into();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("warn,islet=info")
        .with_writer(std::io::stderr)
        .init();
    let root = PathBuf::from(std::env::args().nth(1).expect("data root"));
    let only: Option<String> = std::env::args().nth(2);
    let paths = AppPaths::from_root(&root).unwrap();
    println!("data root: {}", paths.root.display());
    let (tx, rx) = async_channel::unbounded();
    let services = AppServices::new(paths.clone(), tx).unwrap();
    services.add_offline_account("Smoke").unwrap();

    services.load_versions(false);
    let latest = wait(&rx, Duration::from_secs(60), |e| match e {
        UiEvent::Versions(Ok(list)) => Some(list.latest.release.clone()),
        UiEvent::Versions(Err(err)) => panic!("versions: {}", err.message),
        _ => None,
    })
    .expect("version list");
    println!("latest release: {latest}");

    let mut versions = vec!["1.12.2".to_string(), latest];
    if let Some(v) = only {
        versions = vec![v];
    }
    let mut first_instance = None;
    for version in &versions {
        println!("\n=== {version}: create + install ===");
        let instance = services
            .create_instance(&format!("冒烟 {version}"), version, "release")
            .unwrap();
        match install(&services, &rx, &instance.id) {
            Ok(()) => println!("  INSTALL OK"),
            Err(e) => {
                println!("  INSTALL FAILED: {e}");
                continue;
            }
        }
        let natives = paths.instance_natives_dir(&instance.id);
        println!(
            "  java dirs: {:?}",
            std::fs::read_dir(&paths.java.join("adoptium"))
                .map(|d| d.flatten().map(|e| e.file_name()).collect::<Vec<_>>())
                .unwrap_or_default()
        );
        println!(
            "\n=== {version}: launch (no desktop in SSH, window creation is expected to fail) ==="
        );
        launch(
            &services,
            &rx,
            &instance.id,
            &paths.instance_logs_dir(&instance.id),
        );
        println!(
            "  natives has META-INF: {}",
            natives.join("META-INF").exists()
        );
        first_instance.get_or_insert(instance.id.clone());
    }

    if let Some(id) = first_instance.clone() {
        println!("\n=== backup / restore ===");
        let game = paths.instance_game_dir(&id);
        let world = game.join("saves").join("中文 世界");
        std::fs::create_dir_all(&world).unwrap();
        std::fs::write(world.join("level.dat"), b"version-A").unwrap();
        let backup = op(&services, &rx, &id, FileOperation::Backup);
        println!("  backup: {backup:?}");
        let backup_path = std::fs::read_dir(paths.root.join("backups").join(&id))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "zip"))
            .unwrap();
        std::fs::write(world.join("level.dat"), b"version-B").unwrap();
        {
            let _held = hold_open(&world.join("level.dat"));
            let locked = op(
                &services,
                &rx,
                &id,
                FileOperation::RestoreBackup(backup_path.clone()),
            );
            println!("  restore while file held open: {locked:?}");
            println!(
                "  instance dir leftovers: {:?}",
                leftovers(&paths.instance_dir(&id))
            );
            println!(
                "  world still version-B: {}",
                std::fs::read(world.join("level.dat")).unwrap() == b"version-B"
            );
            let delete = services
                .delete_instance(&id)
                .map(|_| ())
                .map_err(|e| e.user_message());
            println!("  delete while file held open: {delete:?}");
        }
        let restored = op(
            &services,
            &rx,
            &id,
            FileOperation::RestoreBackup(backup_path),
        );
        println!("  restore after release: {restored:?}");
        println!(
            "  world back to version-A: {}",
            std::fs::read(world.join("level.dat")).unwrap() == b"version-A"
        );
        println!(
            "  instance dir leftovers: {:?}",
            leftovers(&paths.instance_dir(&id))
        );

        println!("\n=== recycle bin ===");
        let deleted = services
            .delete_instance(&id)
            .map(|_| ())
            .map_err(|e| e.user_message());
        println!(
            "  delete: {deleted:?}, recycled={}",
            services.instances.recycled().len()
        );
        let back = op(&services, &rx, &id, FileOperation::RestoreDeleted);
        println!(
            "  restore deleted: {back:?}, world kept: {}",
            std::fs::read(world.join("level.dat")).is_ok_and(|b| b == b"version-A")
        );

        println!("\n=== duplicate ===");
        let dup = op(&services, &rx, &id, FileOperation::Duplicate);
        println!(
            "  duplicate: {dup:?}, instances={}",
            services.instances.list().len()
        );
    }

    println!("\n=== java detection ===");
    let java_bin = walk_find(&paths.java, "java.exe").or_else(|| walk_find(&paths.java, "java"));
    if let Some(java_bin) = java_bin {
        let home = java_bin.parent().unwrap().parent().unwrap().to_path_buf();
        unsafe { std::env::set_var("JAVA_HOME", &home) };
        let found = islet::core::java::detect::find_all();
        for j in &found {
            println!("  find_all: {} (java {}, {})", j.path, j.major, j.arch);
        }
        let original = std::env::var("PATH").unwrap_or_default();
        let filler: Vec<String> = (0..120)
            .map(|i| format!(r"C:\Program Files\Long Vendor Directory {i:03}\bin"))
            .collect();
        let long = format!("{};{}", filler.join(";"), original);
        unsafe { std::env::set_var("PATH", &long) };
        println!("  PATH length now {}", long.len());
        println!("  old-style probe: {}", old_style_probe(&java_bin));
        let t = Instant::now();
        let fixed = islet::core::java::detect::inspect(&java_bin);
        println!(
            "  fixed inspect: {:?} in {:?}",
            fixed.map(|j| format!("java {} {}", j.major, j.path)),
            t.elapsed()
        );
        unsafe { std::env::set_var("PATH", &original) };
    }
    println!("\nDONE");
}

fn walk_find(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = walk_find(&path, name) {
                return Some(found);
            }
        } else if path.file_name().is_some_and(|n| n == name)
            && path
                .parent()
                .and_then(|p| p.file_name())
                .is_some_and(|n| n == "bin")
        {
            return Some(path);
        }
    }
    None
}
