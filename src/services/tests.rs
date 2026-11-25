use tokio::io::BufReader;

use super::accounts::offer_offline;
use super::launch::{
    analysis_log_text, build_excerpt, drain_readers, launcher_header, read_lossy_line,
    read_tail_text, resolve_memory_mb,
};
use super::*;
use crate::core::accounts::AccountProvider;
use crate::core::{crash, platform};


fn account(provider: AccountProvider) -> Account {
    Account {
        id: "microsoft:test".into(),
        provider,
        username: "Steve".into(),
        uuid: "00000000-0000-0000-0000-000000000001".into(),
        xuid: None,
        ownership: Some(true),
        skin_url: None,
        skin_slim: false,
        created_at: 0,
        server: None,
        server_name: None,
    }
}

#[test]
fn unreachable_sign_in_offers_offline_play() {
    let microsoft = account(AccountProvider::Microsoft);
    let down = AppError::detail("xbox", "Xbox 授权失败", "HTTP 503");
    assert_eq!(offer_offline(&microsoft, down).code(), "auth_offline");
    // Signing in again fixes these; playing offline would hide that.
    for code in ["auth_expired", "not_owned", "client_id", "keychain"] {
        assert_eq!(offer_offline(&microsoft, AppError::new(code, "x")).code(), code);
    }
    let offline = account(AccountProvider::Offline);
    assert_eq!(offer_offline(&offline, AppError::new("xbox", "x")).code(), "xbox");
}

#[tokio::test]
async fn log_reader_keeps_reading_after_non_utf8_line() {
    // "中文" in GBK, as a Chinese-locale Windows JVM prints it, then CRLF.
    let input: &[u8] = b"first\n\xd6\xd0\xce\xc4\r\nafter\n";
    let mut reader = BufReader::new(input);
    let mut buffer = Vec::new();
    let mut lines = Vec::new();
    while let Some(line) = read_lossy_line(&mut reader, &mut buffer).await {
        lines.push(line);
    }
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "first");
    assert!(!lines[1].ends_with('\r'));
    assert_eq!(lines[2], "after");
}

#[test]
fn instance_memory_overrides_automatic_and_manual_settings() {
    assert_eq!(
        resolve_memory_mb(Some(3072), true, 4096, 16384, 151),
        3072
    );
    assert_eq!(resolve_memory_mb(None, true, 4096, 16384, 0), 4096);
    assert_eq!(resolve_memory_mb(None, false, 4096, 16384, 151), 4096);
    assert_eq!(resolve_memory_mb(None, false, 6144, 16384, 0), 6144);
}

fn crash_services() -> (tempfile::TempDir, Arc<AppServices>, String) {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_root(root.path()).unwrap();
    let (events, _receiver) = async_channel::unbounded();
    let services = AppServices::new(paths, events).unwrap();
    let instance = services.instances.create("崩溃测试", "1.21", "release").unwrap();
    (root, services, instance.id)
}

#[test]
fn crash_analysis_reads_the_current_launcher_log_and_honours_since() {
    let (_root, services, id) = crash_services();
    let log = services
        .paths
        .instance_logs_dir(&id)
        .join("latest.log");
    std::fs::write(
        &log,
        "java.lang.UnsupportedClassVersionError: example/Main has been compiled by a more recent version of the Java Runtime (class file version 65.0)\n",
    )
    .unwrap();

    let current = services.analyze_crash(&id, 0).unwrap();
    assert_eq!(current.diagnoses[0].title, "需要 Java 21");

    let stale = services
        .analyze_crash(&id, crate::core::util::now_millis() + 60_000)
        .unwrap();
    assert!(stale.diagnoses.is_empty());
    assert!(stale.excerpt.is_empty());
}

#[test]
fn crash_analysis_only_uses_crash_reports_written_after_the_launch() {
    let (_root, services, id) = crash_services();
    let reports = services
        .paths
        .instance_game_dir(&id)
        .join("crash-reports");
    std::fs::create_dir_all(&reports).unwrap();
    std::fs::write(
        reports.join("crash-2026-10-28_12.00.00-client.txt"),
        "---- Minecraft Crash Report ----\nDescription: Initializing game\njava.lang.OutOfMemoryError: Java heap space\n",
    )
    .unwrap();

    let current = services.analyze_crash(&id, 0).unwrap();
    assert_eq!(current.diagnoses[0].title, "内存不足");

    let stale = services
        .analyze_crash(&id, crate::core::util::now_millis() + 60_000)
        .unwrap();
    assert!(stale.diagnoses.is_empty());
}

#[test]
fn crash_analysis_reads_fresh_native_vm_reports() {
    let (_root, services, id) = crash_services();
    let game = services.paths.instance_game_dir(&id);
    std::fs::create_dir_all(&game).unwrap();
    std::fs::write(game.join("hs_err_pid123.log"), "# A fatal error has been detected by the Java Runtime Environment:\n# SIGSEGV\n# Problematic frame:\n# C  [nvoglv64.dll+0x1234]\n").unwrap();
    let analysis = services.analyze_crash(&id, 0).unwrap();
    assert!(!analysis.diagnoses.is_empty());
    assert!(analysis.excerpt.contains("nvoglv64"));
    assert!(services.analyze_crash(&id, crate::core::util::now_millis() + 60_000).unwrap().diagnoses.is_empty());
}

#[test]
fn crash_analysis_reads_only_the_log_tail_and_caps_the_excerpt() {
    let (_root, services, id) = crash_services();
    let log = services
        .paths
        .instance_logs_dir(&id)
        .join("latest.log");
    let mut content = String::from(
        "java.lang.UnsupportedClassVersionError: old/Main (class file version 65.0)\n",
    );
    // Larger than the 512 KiB log window, so the first error must fall
    // outside and cannot be reported again for this launch.
    content.push_str(&"x".repeat(600 * 1024));
    content.push_str("\njava.lang.OutOfMemoryError: Java heap space\n");
    std::fs::write(&log, content).unwrap();

    let analysis = services.analyze_crash(&id, 0).unwrap();
    assert_eq!(analysis.diagnoses.len(), 1);
    assert_eq!(analysis.diagnoses[0].title, "内存不足");
    assert!(analysis.excerpt.lines().count() <= 60);
    assert!(!analysis.excerpt.contains("UnsupportedClassVersionError"));
}

#[test]
fn crash_analysis_picks_the_newest_fresh_crash_report() {
    let (_root, services, id) = crash_services();
    let reports = services
        .paths
        .instance_game_dir(&id)
        .join("crash-reports");
    std::fs::create_dir_all(&reports).unwrap();
    std::fs::write(
        reports.join("crash-older.txt"),
        "Error: Could not find or load main class net.minecraft.client.main.Main\n",
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(50));
    std::fs::write(
        reports.join("crash-newer.txt"),
        "java.lang.OutOfMemoryError: Java heap space\n",
    )
    .unwrap();

    let analysis = services.analyze_crash(&id, 0).unwrap();
    assert_eq!(analysis.diagnoses.len(), 1);
    assert_eq!(analysis.diagnoses[0].title, "内存不足");
}

#[test]
fn excerpt_prefers_crash_report_header_and_then_log_tail() {
    let report = (0..30)
        .map(|index| format!("report line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let log = (0..100)
        .map(|index| format!("log line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let excerpt = build_excerpt(&log, Some(&report));
    let lines: Vec<&str> = excerpt.lines().collect();
    assert_eq!(lines.len(), 60);
    assert_eq!(lines[0], "report line 0");
    assert_eq!(lines[19], "report line 19");
    assert_eq!(lines[20], "log line 60");
    assert_eq!(lines[59], "log line 99");
}

#[test]
fn mod_count_ignores_disabled_jars_and_other_files() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_root(root.path()).unwrap();
    let mods = paths
        .instance_game_dir("00000000-0000-0000-0000-000000000000")
        .join("mods");
    std::fs::create_dir_all(&mods).unwrap();
    for name in ["sodium.jar", "lithium.JAR", "iris.jar.disabled", "notes.txt"] {
        std::fs::write(mods.join(name), b"x").unwrap();
    }
    std::fs::create_dir_all(mods.join("folder.jar")).unwrap();
    assert_eq!(
        instance_mod_count(&paths, "00000000-0000-0000-0000-000000000000"),
        2
    );
    assert_eq!(instance_mod_count(&paths, "missing"), 0);
}

#[test]
fn launcher_header_reads_logs_from_before_the_rename() {
    let old = "[MC Launcher] version=1.21 memory=2560MB java=21\n[12:00:00] Booting";
    assert_eq!(launcher_header(old), old.lines().next());
    let new = "[Islet] version=1.21 memory=2560MB java=21\n";
    assert_eq!(launcher_header(new), new.lines().next());
    assert_eq!(launcher_header("[Islet] post-exit: x (exit 0)\n"), None);
}

#[test]
fn crash_analysis_keeps_the_memory_header_outside_the_log_tail() {
    let (_root, services, id) = crash_services();
    let log = services
        .paths
        .instance_logs_dir(&id)
        .join("latest.log");
    let mut content = String::from(
        "[Islet] version=1.21 memory=2560MB java=21.0.2 (21), user=Steve\n",
    );
    // Push the header outside the 512 KiB tail window.
    content.push_str(&"x".repeat(600 * 1024));
    content.push_str("\njava.lang.OutOfMemoryError: Java heap space\n");
    std::fs::write(&log, content).unwrap();

    let (tail, _) = read_tail_text(&log, 512 * 1024).unwrap().unwrap();
    assert!(!tail.contains("memory=2560MB"));

    let (text, _) = analysis_log_text(&log, 512 * 1024).unwrap().unwrap();
    assert!(text.contains("memory=2560MB"));
    assert!(text.contains("Java heap space"));

    let analysis = services.analyze_crash(&id, 0).unwrap();
    assert_eq!(analysis.diagnoses.len(), 1);
    let expected = crash::clamp_memory_mb(4096, platform::total_memory_mb());
    if expected > 2560 {
        assert_eq!(
            analysis.diagnoses[0].fix,
            Some(crate::core::crash::Fix::RaiseMemory { mb: expected })
        );
        assert!(
            analysis.diagnoses[0]
                .detail
                .contains(&format!("{expected} MB"))
        );
    } else {
        assert_eq!(
            analysis.diagnoses[0].fix,
            Some(crate::core::crash::Fix::None)
        );
    }
}

#[tokio::test]
async fn drain_readers_aborts_a_reader_whose_pipe_never_closes() {
    use std::sync::atomic::{AtomicBool, Ordering};

    struct MarkOnDrop(Arc<AtomicBool>);
    impl Drop for MarkOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let stopped = Arc::new(AtomicBool::new(false));
    let flag = stopped.clone();
    let reader = tokio::spawn(async move {
        let _mark = MarkOnDrop(flag);
        std::future::pending::<()>().await;
    });

    let started = std::time::Instant::now();
    drain_readers(
        vec![reader],
        Duration::from_millis(20),
        Duration::from_secs(1),
    )
    .await;
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(stopped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn drain_readers_waits_for_a_normal_reader() {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let reader = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let _ = sender.send(());
    });
    drain_readers(
        vec![reader],
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .await;
    assert!(receiver.await.is_ok());
}
