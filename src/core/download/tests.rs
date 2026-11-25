use super::segmented::PARALLEL_PARTS;
use super::*;
use std::io::{Read, Write};
use std::time::Instant;

/// These servers live on loopback; an HTTP proxy from the environment
/// must not intercept the test traffic.
fn loopback_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("loopback client")
}

#[tokio::test]
async fn retries_hash_failure_without_double_counting_and_reuses_verified_file() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for body in ["bad!", "good"] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n{body}"
            )
            .unwrap();
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let task = DownloadTask::new(
        "test",
        format!("http://{address}/file"),
        vec![],
        dir.path().join("file"),
        Some(crate::core::util::sha1_hex(b"good")),
        Some(4),
    );
    let tasks = vec![task.clone()];
    let progress = Arc::new(DownloadProgress::new(&tasks));
    download_all(
        tasks,
        loopback_client(),
        1,
        CancellationToken::new(),
        progress.clone(),
    )
    .await
    .unwrap();
    server.join().unwrap();
    assert_eq!(progress.snapshot().bytes_done, 4);
    assert_eq!(progress.snapshot().files_done, 1);
    assert!(verify_existing(&task).await);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn broken_mirror_falls_back_once_and_is_skipped_after_repeated_failures() {
    use std::sync::atomic::AtomicUsize;
    // The mirror drops every connection; the official server answers.
    let mirror = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mirror_address = mirror.local_addr().unwrap();
    let mirror_hits = Arc::new(AtomicUsize::new(0));
    let hits = mirror_hits.clone();
    std::thread::spawn(move || {
        for stream in mirror.incoming() {
            hits.fetch_add(1, Ordering::SeqCst);
            drop(stream);
        }
    });
    let official = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let official_address = official.local_addr().unwrap();
    let files = MIRROR_FAILURE_LIMIT as usize + 4;
    let server = std::thread::spawn(move || {
        for _ in 0..files {
            let (mut stream, _) = official.accept().unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
            )
            .unwrap();
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let tasks: Vec<DownloadTask> = (0..files)
        .map(|i| {
            DownloadTask::new(
                format!("file {i}"),
                format!("http://{mirror_address}/m{i}"),
                vec![format!("http://{official_address}/o{i}")],
                dir.path().join(format!("f{i}")),
                Some(crate::core::util::sha1_hex(b"ok")),
                Some(2),
            )
        })
        .collect();
    let progress = Arc::new(DownloadProgress::new(&tasks));
    let started = std::time::Instant::now();
    download_all(
        tasks,
        loopback_client(),
        1,
        CancellationToken::new(),
        progress.clone(),
    )
    .await
    .unwrap();
    server.join().unwrap();
    assert_eq!(progress.snapshot().files_done, files as u64);
    // One attempt per file until the breaker trips, then none at all.
    assert_eq!(
        mirror_hits.load(Ordering::SeqCst),
        MIRROR_FAILURE_LIMIT as usize
    );
    // Mirror failures never wait for retry back-off.
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[tokio::test]
async fn dropped_connection_resumes_with_range_request() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        // First answer promises 8 bytes and hangs up after 4.
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request);
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nabcd").unwrap();
        drop(stream);
        let (mut stream, _) = listener.accept().unwrap();
        let read = stream.read(&mut request).unwrap();
        let head = String::from_utf8_lossy(&request[..read]).to_ascii_lowercase();
        write!(
            stream,
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 4-7/8\r\nContent-Length: 4\r\nConnection: close\r\n\r\nefgh"
        )
        .unwrap();
        head
    });
    let dir = tempfile::tempdir().unwrap();
    let task = DownloadTask::new(
        "test",
        format!("http://{address}/file"),
        vec![],
        dir.path().join("file"),
        Some(crate::core::util::sha1_hex(b"abcdefgh")),
        Some(8),
    );
    let tasks = vec![task.clone()];
    let progress = Arc::new(DownloadProgress::new(&tasks));
    download_all(
        tasks,
        loopback_client(),
        1,
        CancellationToken::new(),
        progress.clone(),
    )
    .await
    .unwrap();
    let second_request = server.join().unwrap();
    assert!(second_request.contains("range: bytes=4-"), "{second_request}");
    assert_eq!(std::fs::read(&task.dest).unwrap(), b"abcdefgh");
    assert_eq!(progress.snapshot().bytes_done, 8);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn slow_source_is_dropped_for_the_next_one_which_resumes() {
    // The first source sends half the file, then stalls.
    let slow = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let slow_address = slow.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = slow.accept().unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request);
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nabcd").unwrap();
        std::thread::sleep(Duration::from_secs(3));
    });
    let fast = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fast_address = fast.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = fast.accept().unwrap();
        let mut request = [0; 4096];
        let read = stream.read(&mut request).unwrap();
        let head = String::from_utf8_lossy(&request[..read]).to_ascii_lowercase();
        write!(
            stream,
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 4-7/8\r\nContent-Length: 4\r\nConnection: close\r\n\r\nefgh"
        )
        .unwrap();
        head
    });
    let dir = tempfile::tempdir().unwrap();
    let task = DownloadTask::new(
        "test",
        format!("http://{slow_address}/file"),
        vec![format!("http://{fast_address}/file")],
        dir.path().join("file"),
        Some(crate::core::util::sha1_hex(b"abcdefgh")),
        Some(8),
    )
    .with_min_speed(1024);
    let tasks = vec![task.clone()];
    let progress = Arc::new(DownloadProgress::new(&tasks));
    let started = Instant::now();
    download_all(
        tasks,
        loopback_client(),
        1,
        CancellationToken::new(),
        progress.clone(),
    )
    .await
    .unwrap();
    assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    let second_request = server.join().unwrap();
    assert!(second_request.contains("range: bytes=4-"), "{second_request}");
    assert_eq!(std::fs::read(&task.dest).unwrap(), b"abcdefgh");
    assert_eq!(progress.snapshot().bytes_done, 8);
}

#[tokio::test]
async fn sources_are_ranked_by_a_speed_test() {
    fn serve(head: &'static str, body: Vec<u8>, stall: bool) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
            if stall {
                std::thread::sleep(Duration::from_secs(2));
            }
        });
        format!("http://{address}/file")
    }
    let whole = "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-65535/65536\r\nContent-Length: 65536\r\nConnection: close\r\n\r\n";
    let fast = serve(whole, vec![7; 65536], false);
    let slow = serve(whole, vec![7; 16], true);
    // An error page that answers 200 is not the file.
    let wrong = serve("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n", b"nope!".to_vec(), false);
    let dead = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}/file", listener.local_addr().unwrap())
    };
    let ranked = fastest_first(
        &loopback_client(),
        vec![dead.clone(), wrong.clone(), slow.clone(), fast.clone()],
        Some(65536),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(ranked, vec![fast, slow, dead, wrong]);
}

#[test]
fn dropped_download_rolls_back_unknown_size_accounting() {
    let progress = DownloadProgress::default();
    {
        let mut attempt = FileBytes {
            progress: &progress,
            added: 0,
            unknown: true,
            committed: false,
        };
        attempt.add(123);
    }
    assert_eq!(progress.snapshot().bytes_done, 0);
    assert_eq!(progress.snapshot().bytes_total, 0);
}

#[tokio::test]
async fn large_files_download_in_parallel_ranges_and_fall_back_on_200() {
    use std::sync::atomic::AtomicUsize;

    let payload: Vec<u8> = (0..4096u32).map(|i| (i * 31 % 251) as u8).collect();
    let expected_sha1 = crate::core::util::sha1_hex(&payload);

    // 1. Server that honours Range: 4 concurrent 206 requests reassemble in order.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let data = Arc::new(payload.clone());
    let range_hits = Arc::new(AtomicUsize::new(0));
    let hits = range_hits.clone();
    let server_data = data.clone();
    std::thread::spawn(move || {
        for _ in 0..PARALLEL_PARTS {
            let (mut stream, _) = listener.accept().unwrap();
            let data = server_data.clone();
            let hits = hits.clone();
            std::thread::spawn(move || {
                let mut req = [0u8; 4096];
                let n = stream.read(&mut req).unwrap();
                let text = String::from_utf8_lossy(&req[..n]);
                let range_line = text
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("range: bytes="))
                    .unwrap();
                let span = range_line.split('=').nth(1).unwrap().trim();
                let (s, e) = span.split_once('-').unwrap();
                let (start, end): (usize, usize) = (s.parse().unwrap(), e.parse().unwrap());
                let slice = &data[start..=end];
                hits.fetch_add(1, Ordering::SeqCst);
                write!(
                    stream,
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    data.len(),
                    slice.len()
                )
                .unwrap();
                stream.write_all(slice).unwrap();
            });
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let task = DownloadTask::new(
        "segmented.bin",
        format!("http://{addr}/segmented.bin"),
        vec![],
        dir.path().join("segmented.bin"),
        Some(expected_sha1.clone()),
        Some(payload.len() as u64),
    );
    let progress = Arc::new(DownloadProgress::new(std::slice::from_ref(&task)));
    let mut partial = Partial::new(&task.dest, false);
    let mut accounting = FileBytes {
        progress: &progress,
        added: 0,
        unknown: false,
        committed: false,
    };
    attempt_download_with_threshold(
        &loopback_client(),
        &task.url,
        &task,
        &CancellationToken::new(),
        &mut partial,
        &mut accounting,
        None,
        1024,
    )
    .await
    .unwrap();
    accounting.committed = true;
    assert_eq!(range_hits.load(Ordering::SeqCst), PARALLEL_PARTS);
    assert_eq!(std::fs::read(&task.dest).unwrap(), payload);
    assert_eq!(progress.snapshot().bytes_done, payload.len() as u64);

    // 2. Server that ignores Range and returns 200 OK: streams the whole body once without extra requests.
    let plain = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let plain_addr = plain.local_addr().unwrap();
    let plain_data = payload.clone();
    let plain_server = std::thread::spawn(move || {
        let (mut stream, _) = plain.accept().unwrap();
        let mut req = [0u8; 4096];
        let _ = stream.read(&mut req);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            plain_data.len()
        )
        .unwrap();
        stream.write_all(&plain_data).unwrap();
    });
    let task2 = DownloadTask::new(
        "whole.bin",
        format!("http://{plain_addr}/whole.bin"),
        vec![],
        dir.path().join("whole.bin"),
        Some(expected_sha1),
        Some(payload.len() as u64),
    );
    let progress2 = Arc::new(DownloadProgress::new(std::slice::from_ref(&task2)));
    let mut partial2 = Partial::new(&task2.dest, false);
    let mut accounting2 = FileBytes {
        progress: &progress2,
        added: 0,
        unknown: false,
        committed: false,
    };
    attempt_download_with_threshold(
        &loopback_client(),
        &task2.url,
        &task2,
        &CancellationToken::new(),
        &mut partial2,
        &mut accounting2,
        None,
        1024,
    )
    .await
    .unwrap();
    accounting2.committed = true;
    plain_server.join().unwrap();
    assert_eq!(std::fs::read(&task2.dest).unwrap(), payload);

    // 3. Server rejects a sibling range (e.g. 429 rate-limit) after answering 206 on part 0:
    // rolls back segment progress and falls back cleanly to a single-stream GET without double-counting.
    let picky = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let picky_addr = picky.local_addr().unwrap();
    let picky_data = Arc::new(payload.clone());
    let picky_server = std::thread::spawn(move || {
        for _ in 0..(PARALLEL_PARTS + 1) {
            let Ok((mut stream, _)) = picky.accept() else { break };
            let data = picky_data.clone();
            std::thread::spawn(move || {
                let mut req = [0u8; 4096];
                let n = stream.read(&mut req).unwrap_or(0);
                let text = String::from_utf8_lossy(&req[..n]);
                if let Some(range_line) = text.lines().find(|l| l.to_ascii_lowercase().starts_with("range: bytes=")) {
                    let span = range_line.split('=').nth(1).unwrap().trim();
                    if let Some((s, e)) = span.split_once('-') {
                        let (start, end): (usize, usize) = (s.parse().unwrap(), e.parse().unwrap());
                        if start == 0 {
                            let slice = &data[start..=end];
                            let _ = write!(
                                stream,
                                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                data.len(),
                                slice.len()
                            );
                            let _ = stream.write_all(slice);
                            return;
                        }
                        let _ = write!(stream, "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        return;
                    }
                }
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    data.len()
                );
                let _ = stream.write_all(&data);
            });
        }
    });
    let task3 = DownloadTask::new(
        "fallback.bin",
        format!("http://{picky_addr}/fallback.bin"),
        vec![],
        dir.path().join("fallback.bin"),
        Some(crate::core::util::sha1_hex(&payload)),
        Some(payload.len() as u64),
    );
    let progress3 = Arc::new(DownloadProgress::new(std::slice::from_ref(&task3)));
    let mut partial3 = Partial::new(&task3.dest, false);
    let mut accounting3 = FileBytes {
        progress: &progress3,
        added: 0,
        unknown: false,
        committed: false,
    };
    attempt_download_with_threshold(
        &loopback_client(),
        &task3.url,
        &task3,
        &CancellationToken::new(),
        &mut partial3,
        &mut accounting3,
        None,
        1024,
    )
    .await
    .unwrap();
    accounting3.committed = true;
    drop(picky_server);
    assert_eq!(std::fs::read(&task3.dest).unwrap(), payload);
    assert_eq!(progress3.snapshot().bytes_done, payload.len() as u64);
}
