use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures::StreamExt;
use sha1::{Digest, Sha1};
use sha2::Sha256;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::core::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub label: String,
    /// Primary URL (mirror when BMCLAPI is selected).
    pub url: String,
    /// Fallback URLs, tried after the primary URL fails.
    pub fallback_urls: Vec<String>,
    pub dest: PathBuf,
    pub sha1: Option<String>,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    /// Bytes per second below which a source is dropped for the next one,
    /// which resumes where it stopped. None keeps a working source however
    /// slow it is; the last source left is always kept.
    pub min_speed: Option<u64>,
}

impl DownloadTask {
    pub fn with_sha256(mut self, hash: String) -> Self {
        self.sha256 = Some(hash);
        self
    }

    pub fn with_min_speed(mut self, bytes_per_second: u64) -> Self {
        self.min_speed = Some(bytes_per_second);
        self
    }

    pub fn new(
        label: impl Into<String>,
        url: impl Into<String>,
        fallback_urls: Vec<String>,
        dest: PathBuf,
        sha1: Option<String>,
        size: Option<u64>,
    ) -> Self {
        Self {
            label: label.into(),
            url: url.into(),
            fallback_urls,
            dest,
            sha1,
            sha256: None,
            size,
            min_speed: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct DownloadProgress {
    files_total: AtomicU64,
    files_done: AtomicU64,
    bytes_total: AtomicU64,
    bytes_done: AtomicU64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ProgressSnapshot {
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

impl DownloadProgress {
    pub fn new(tasks: &[DownloadTask]) -> Self {
        let bytes_total = tasks.iter().filter_map(|task| task.size).sum();
        Self {
            files_total: AtomicU64::new(tasks.len() as u64),
            files_done: AtomicU64::new(0),
            bytes_total: AtomicU64::new(bytes_total),
            bytes_done: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            files_done: self.files_done.load(Ordering::Relaxed),
            files_total: self.files_total.load(Ordering::Relaxed),
            bytes_done: self.bytes_done.load(Ordering::Relaxed),
            bytes_total: self.bytes_total.load(Ordering::Relaxed),
        }
    }

    fn add_bytes(&self, bytes: u64, unknown_total: bool) {
        self.bytes_done.fetch_add(bytes, Ordering::Relaxed);
        if unknown_total {
            self.bytes_total.fetch_add(bytes, Ordering::Relaxed);
        }
    }

    fn file_done(&self, skipped_bytes: u64) {
        if skipped_bytes > 0 {
            self.bytes_done.fetch_add(skipped_bytes, Ordering::Relaxed);
        }
        self.files_done.fetch_add(1, Ordering::Relaxed);
    }
}

pub async fn download_all(
    tasks: Vec<DownloadTask>,
    client: reqwest::Client,
    concurrency: usize,
    cancel: CancellationToken,
    progress: Arc<DownloadProgress>,
) -> AppResult<()> {
    if tasks.is_empty() {
        return Ok(());
    }
    let mirror = Arc::new(MirrorHealth::default());
    let stream = futures::stream::iter(tasks.into_iter().map(|task| {
        let client = client.clone();
        let cancel = cancel.clone();
        let progress = progress.clone();
        let mirror = mirror.clone();
        async move { download_one(task, client, cancel, progress, &mirror).await }
    }));
    let mut buffered = stream.buffer_unordered(concurrency.max(1));
    while let Some(result) = buffered.next().await {
        result?;
    }
    Ok(())
}

/// Mirror nodes fail in bulk (for example a node serving a certificate for the
/// wrong host). After this many consecutive mirror failures the rest of the
/// batch goes straight to the official source.
const MIRROR_FAILURE_LIMIT: u32 = 8;

#[derive(Default)]
struct MirrorHealth {
    consecutive_failures: AtomicU32,
    /// Latched: BMCLAPI hands out random nodes, so an occasional success from
    /// a healthy node must not re-enable a mirror that keeps failing.
    disabled: AtomicBool,
}

impl MirrorHealth {
    fn usable(&self) -> bool {
        !self.disabled.load(Ordering::Relaxed)
    }
    fn record(&self, ok: bool) {
        if ok {
            self.consecutive_failures.store(0, Ordering::Relaxed);
        } else if self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1
            >= MIRROR_FAILURE_LIMIT
            && !self.disabled.swap(true, Ordering::Relaxed)
        {
            tracing::warn!("mirror keeps failing, using the official source for this batch");
        }
    }
}

async fn download_one(
    task: DownloadTask,
    client: reqwest::Client,
    cancel: CancellationToken,
    progress: Arc<DownloadProgress>,
    mirror: &MirrorHealth,
) -> AppResult<()> {
    if verify_existing(&task).await {
        progress.file_done(task.size.unwrap_or(0));
        return Ok(());
    }

    // With fallbacks, the primary URL is a mirror: try it once and move on
    // immediately, keeping the retries for the authoritative source.
    let has_fallback = !task.fallback_urls.is_empty();
    let mut sources = Vec::with_capacity(1 + task.fallback_urls.len());
    if !has_fallback || mirror.usable() {
        sources.push((
            task.url.clone(),
            if has_fallback { 1 } else { 3 },
            has_fallback,
        ));
    }
    sources.extend(
        task.fallback_urls
            .iter()
            .map(|url| (url.clone(), 3u32, false)),
    );

    // One partial file per task, kept across attempts and sources: a dropped
    // connection resumes with a Range request instead of starting over.
    let mut partial = Partial::new(&task.dest, task.sha256.is_some());
    let _cleanup = PartialFile(partial.path.clone());
    let mut accounting = FileBytes {
        progress: &progress,
        added: 0,
        unknown: task.size.is_none(),
        committed: false,
    };

    let mut sources = std::collections::VecDeque::from(sources);
    let mut slow: Vec<String> = Vec::new();
    let mut last_error = None;
    while let Some((url, attempts, is_mirror)) = sources.pop_front() {
        // Only a source with others behind it may be dropped for being slow.
        let floor = task.min_speed.filter(|_| !sources.is_empty());
        for attempt in 0..attempts {
            if cancel.is_cancelled() {
                return Err(cancelled_error());
            }
            match attempt_download(&client, &url, &task, &cancel, &mut partial, &mut accounting, floor)
                .await
            {
                Ok(()) => {
                    if is_mirror {
                        mirror.record(true);
                    }
                    accounting.committed = true;
                    progress.file_done(0);
                    return Ok(());
                }
                Err(err) if err.code() == "cancelled" => return Err(err),
                Err(err) if err.code() == "download_slow" => {
                    tracing::info!(file = %task.label, url = %url, resume_at = partial.len, "source too slow, trying the next one");
                    // It may still be the only one that works: it comes back
                    // once, after the others.
                    if !slow.contains(&url) {
                        slow.push(url.clone());
                        sources.push_back((url.clone(), 1, is_mirror));
                    }
                    last_error = Some(err);
                    break;
                }
                Err(err) => {
                    tracing::warn!(
                        file = %task.label,
                        attempt = attempt + 1,
                        mirror = is_mirror,
                        resume_at = partial.len,
                        error = %err,
                        "download attempt failed"
                    );
                    last_error = Some(err);
                    if is_mirror {
                        mirror.record(false);
                    } else if attempt + 1 < attempts {
                        tokio::select! {
                            _ = cancel.cancelled() => return Err(cancelled_error()),
                            _ = tokio::time::sleep(Duration::from_millis(400 * (1 << attempt))) => {}
                        }
                    }
                }
            }
        }
    }
    // Sources that only serve copies failing the checksum are a different
    // problem from sources that can't be reached; the page says which.
    let code = match last_error.as_ref().map(AppError::code) {
        Some("download_hash") => "download_hash",
        _ => "download",
    };
    Err(AppError::detail(
        code,
        crate::tr!("下载 {} 失败，请重试或切换下载源。", "Downloading {} failed; try again or switch the download source.", task.label),
        last_error.map(|e| e.to_string()).unwrap_or_default(),
    ))
}

async fn verify_existing(task: &DownloadTask) -> bool {
    let Ok(metadata) = tokio::fs::metadata(&task.dest).await else {
        return false;
    };
    if let Some(size) = task.size {
        if metadata.len() != size {
            return false;
        }
    }
    if let Some(expected) = &task.sha256 {
        return hash_sha256(&task.dest)
            .await
            .is_ok_and(|actual| actual.eq_ignore_ascii_case(expected));
    }
    let Some(expected) = &task.sha1 else {
        // Without a hash, an existing file with the expected size is the best
        // signal we have. Metadata entries always include a SHA-1.
        return task.size.is_some() || metadata.len() > 0;
    };
    match hash_file(&task.dest).await {
        Ok(actual) => actual.eq_ignore_ascii_case(expected),
        Err(_) => false,
    }
}

async fn hash_file(path: &Path) -> AppResult<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha1::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

async fn attempt_download(
    client: &reqwest::Client,
    url: &str,
    task: &DownloadTask,
    cancel: &CancellationToken,
    partial: &mut Partial,
    accounting: &mut FileBytes<'_>,
    floor: Option<u64>,
) -> AppResult<()> {
    if let Some(parent) = task.dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    // A failed write can leave more on disk than was hashed; only resume
    // from a file that matches what the hashers have seen.
    if partial.len > 0
        && tokio::fs::metadata(&partial.path)
            .await
            .map_or(true, |meta| meta.len() != partial.len)
    {
        partial.restart(accounting);
    }

    let mut request = client.get(url);
    if partial.len > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={}-", partial.len));
    }
    let response = tokio::select! {
        _ = cancel.cancelled() => return Err(cancelled_error()),
        response = request.send() => response?,
    };
    let status = response.status();
    if !status.is_success() {
        if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            partial.restart(accounting);
        }
        return Err(AppError::new(
            "download",
            crate::tr!("下载 {} 失败（HTTP {status}）。", "Downloading {} failed (HTTP {status}).", task.label),
        ));
    }

    // Servers that ignore the Range header answer 200 with the whole file.
    let resumed = partial.len > 0
        && status == reqwest::StatusCode::PARTIAL_CONTENT
        && content_range_start(&response) == Some(partial.len);
    let mut file = if resumed {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(&partial.path)
            .await?
    } else {
        partial.restart(accounting);
        tokio::fs::File::create(&partial.path).await?
    };
    let mut stream = response.bytes_stream();
    // A stalled stream sends no chunks, so the speed is checked on a timer.
    let mut speed = SpeedWindow::new();
    let mut check = tokio::time::interval(SLOW_WINDOW / 10);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Err(cancelled_error()),
            _ = check.tick(), if floor.is_some() => {
                if floor.is_some_and(|floor| speed.below(floor)) {
                    let _ = file.flush().await;
                    return Err(AppError::new("download_slow", crate::tr!("{} 的下载源太慢。", "The source for {} is too slow.", task.label)));
                }
            }
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(bytes)) => {
                        if task.size.is_some_and(|expected| partial.len.saturating_add(bytes.len() as u64) > expected) {
                            drop(file);
                            partial.restart(accounting);
                            return Err(AppError::new("download_size", crate::tr!("{} 超出预期大小。", "{} is larger than expected.", task.label)));
                        }
                        file.write_all(&bytes).await?;
                        partial.update(&bytes);
                        accounting.add(bytes.len() as u64);
                        speed.add(bytes.len() as u64);
                    }
                    Some(Err(err)) => {
                        // Keep what arrived; the next attempt continues from here.
                        let _ = file.flush().await;
                        return Err(AppError::from(err));
                    }
                    None => break,
                }
            }
        }
    }

    file.flush().await?;
    // Hashed files are verified again before every install, so they skip the
    // full-disk flush (F_FULLFSYNC on macOS) that dominates small-file
    // downloads; only files without a hash are synced before they are used.
    if task.sha1.is_none() && task.sha256.is_none() {
        file.sync_all().await?;
    }
    drop(file);

    if let Some(expected_size) = task.size {
        if partial.len != expected_size {
            partial.restart(accounting);
            return Err(AppError::new(
                "download_size",
                crate::tr!("{} 的大小校验失败。", "{} failed the size check.", task.label),
            ));
        }
    }
    if let Some(expected_hash) = &task.sha1 {
        let actual = hex::encode(partial.sha1.clone().finalize());
        if !actual.eq_ignore_ascii_case(expected_hash) {
            partial.restart(accounting);
            return Err(AppError::new(
                "download_hash",
                crate::tr!("{} 的 SHA-1 校验失败，镜像文件可能已损坏。", "{} failed its SHA-1 check; the mirror's copy may be damaged.", task.label),
            ));
        }
    }
    if let (Some(expected), Some(sha256)) = (&task.sha256, &partial.sha256) {
        if !hex::encode(sha256.clone().finalize()).eq_ignore_ascii_case(expected) {
            partial.restart(accounting);
            return Err(AppError::new(
                "download_hash",
                crate::tr!("{} 的 SHA-256 校验失败。", "{} failed its SHA-256 check.", task.label),
            ));
        }
    }
    crate::core::util::replace_file(&partial.path, &task.dest)?;
    Ok(())
}

/// How long a source gets to show its speed each time `min_speed` judges it.
const SLOW_WINDOW: Duration = if cfg!(test) { Duration::from_millis(300) } else { Duration::from_secs(10) };

/// Bytes received in the current stretch of `SLOW_WINDOW`. A new stretch
/// starts each time one is judged, so a source that slows down later is
/// caught too.
struct SpeedWindow {
    started: Instant,
    bytes: u64,
}

impl SpeedWindow {
    fn new() -> Self {
        Self { started: Instant::now(), bytes: 0 }
    }

    fn add(&mut self, bytes: u64) {
        self.bytes += bytes;
    }

    fn below(&mut self, floor: u64) -> bool {
        let elapsed = self.started.elapsed();
        if elapsed < SLOW_WINDOW {
            return false;
        }
        let slow = (self.bytes as f64) < floor as f64 * elapsed.as_secs_f64();
        *self = Self::new();
        slow
    }
}

/// How much of the file the speed test reads from each source, at most.
const PROBE_BYTES: u64 = 512 * 1024;
const PROBE_TIME: Duration = if cfg!(test) { Duration::from_millis(500) } else { Duration::from_secs(5) };

/// Orders the sources of one file by a speed test run on all of them at
/// once, each reading the start of the file for a few seconds. Download
/// mirrors come and go, and their speeds differ a hundredfold by network and
/// by hour, so no fixed order stays right. Sources that fail, send nothing
/// or offer a file of another size keep their given order at the end.
pub async fn fastest_first(
    client: &reqwest::Client,
    urls: Vec<String>,
    size: Option<u64>,
    cancel: &CancellationToken,
) -> Vec<String> {
    let deadline = tokio::time::Instant::now() + PROBE_TIME;
    let probes = futures::future::join_all(urls.iter().map(|url| probe(client, url, size, deadline)));
    let speeds = tokio::select! {
        _ = cancel.cancelled() => return urls,
        speeds = probes => speeds,
    };
    let mut ranked: Vec<(f64, String)> = speeds.into_iter().zip(urls).collect();
    // A stable sort: the failures (all 0) keep their order.
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    tracing::info!(
        sources = ?ranked.iter().map(|(speed, url)| format!("{url} {:.0} KB/s", speed / 1024.0)).collect::<Vec<_>>(),
        "download sources ranked"
    );
    ranked.into_iter().map(|(_, url)| url).collect()
}

/// Bytes per second a source delivered of the start of the file; 0 when it
/// failed or offers a file of another size (an error page, say).
async fn probe(client: &reqwest::Client, url: &str, size: Option<u64>, deadline: tokio::time::Instant) -> f64 {
    let started = Instant::now();
    let request = client
        .get(url)
        .header(reqwest::header::RANGE, format!("bytes=0-{}", PROBE_BYTES - 1));
    let Ok(Ok(response)) = tokio::time::timeout_at(deadline, request.send()).await else {
        return 0.0;
    };
    // A 206 gives the whole size after the slash; a 200 sends the whole file.
    let total = match response.status() {
        reqwest::StatusCode::PARTIAL_CONTENT => response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.rsplit('/').next())
            .and_then(|total| total.parse::<u64>().ok()),
        reqwest::StatusCode::OK => response.content_length(),
        _ => return 0.0,
    };
    if size.zip(total).is_some_and(|(expected, offered)| expected != offered) {
        return 0.0;
    }
    let mut stream = response.bytes_stream();
    let mut received = 0u64;
    while received < PROBE_BYTES {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(bytes))) => received += bytes.len() as u64,
            _ => break,
        }
    }
    received as f64 / started.elapsed().as_secs_f64().max(0.001)
}

/// Start offset of a `Content-Range: bytes <start>-<end>/<total>` header.
fn content_range_start(response: &reqwest::Response) -> Option<u64> {
    let value = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?;
    value
        .strip_prefix("bytes ")?
        .split('-')
        .next()?
        .trim()
        .parse()
        .ok()
}

/// The bytes of one file received so far, with the hashes of exactly those
/// bytes, so a later attempt can append instead of downloading again.
struct Partial {
    path: PathBuf,
    len: u64,
    sha1: Sha1,
    sha256: Option<Sha256>,
}

impl Partial {
    fn new(dest: &Path, with_sha256: bool) -> Self {
        let file_name = dest
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("download");
        Self {
            path: dest.with_file_name(format!(".{file_name}.part-{}", Uuid::new_v4().simple())),
            len: 0,
            sha1: Sha1::new(),
            sha256: with_sha256.then(Sha256::new),
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        self.sha1.update(bytes);
        if let Some(sha256) = &mut self.sha256 {
            sha256.update(bytes);
        }
        self.len += bytes.len() as u64;
    }

    /// Forget the received bytes and take them back out of the progress.
    fn restart(&mut self, accounting: &mut FileBytes<'_>) {
        accounting.remove(self.len);
        self.len = 0;
        self.sha1 = Sha1::new();
        if let Some(sha256) = &mut self.sha256 {
            *sha256 = Sha256::new();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

fn cancelled_error() -> AppError {
    AppError::new("cancelled", crate::core::i18n::pick("操作已取消。", "Cancelled."))
}

// Roll back on errors AND when another concurrent download fails and drops us.
struct FileBytes<'a> {
    progress: &'a DownloadProgress,
    added: u64,
    unknown: bool,
    committed: bool,
}
impl FileBytes<'_> {
    fn add(&mut self, bytes: u64) {
        self.progress.add_bytes(bytes, self.unknown);
        self.added += bytes;
    }
    fn remove(&mut self, bytes: u64) {
        let bytes = bytes.min(self.added);
        self.progress
            .bytes_done
            .fetch_sub(bytes, Ordering::Relaxed);
        if self.unknown {
            self.progress
                .bytes_total
                .fetch_sub(bytes, Ordering::Relaxed);
        }
        self.added -= bytes;
    }
}
impl Drop for FileBytes<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.remove(self.added);
        }
    }
}
struct PartialFile(PathBuf);
impl Drop for PartialFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
async fn hash_sha256(path: &Path) -> AppResult<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

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
}
