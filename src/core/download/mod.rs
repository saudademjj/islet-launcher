mod segmented;
mod speed;
#[cfg(test)]
mod tests;

pub use speed::fastest_first;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use futures::StreamExt;
use sha1::{Digest, Sha1};
use sha2::Sha256;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::core::error::{AppError, AppResult};

use segmented::{PARALLEL_PARTS, RANGE_SPLIT_THRESHOLD, SegmentedOutcome, attempt_segmented};
use speed::{SLOW_WINDOW, SpeedWindow, content_range_start};

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
    pub(super) bytes_total: AtomicU64,
    pub(super) bytes_done: AtomicU64,
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

    pub(super) fn add_bytes(&self, bytes: u64, unknown_total: bool) {
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

/// Verifies an existing file on disk in a single blocking task using a stack
/// buffer, keeping SHA-1/SHA-256 CPU work off the async worker threads.
async fn verify_existing(task: &DownloadTask) -> bool {
    let dest = task.dest.clone();
    let expected_size = task.size;
    let expected_sha256 = task.sha256.clone();
    let expected_sha1 = task.sha1.clone();
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let Ok(metadata) = std::fs::metadata(&dest) else {
            return false;
        };
        if let Some(size) = expected_size
            && metadata.len() != size
        {
            return false;
        }
        let mut buf = [0u8; 32 * 1024];
        if let Some(expected) = expected_sha256 {
            let Ok(mut file) = std::fs::File::open(&dest) else {
                return false;
            };
            let mut hasher = Sha256::new();
            loop {
                match file.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => hasher.update(&buf[..n]),
                    Err(_) => return false,
                }
            }
            return hex::encode(hasher.finalize()).eq_ignore_ascii_case(&expected);
        }
        let Some(expected) = expected_sha1 else {
            // Without a hash, an existing file with the expected size is the best
            // signal we have. Metadata entries always include a SHA-1.
            return expected_size.is_some() || metadata.len() > 0;
        };
        let Ok(mut file) = std::fs::File::open(&dest) else {
            return false;
        };
        let mut hasher = Sha1::new();
        loop {
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => hasher.update(&buf[..n]),
                Err(_) => return false,
            }
        }
        hex::encode(hasher.finalize()).eq_ignore_ascii_case(&expected)
    })
    .await
    .unwrap_or(false)
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
    attempt_download_with_threshold(
        client,
        url,
        task,
        cancel,
        partial,
        accounting,
        floor,
        RANGE_SPLIT_THRESHOLD,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn attempt_download_with_threshold(
    client: &reqwest::Client,
    url: &str,
    task: &DownloadTask,
    cancel: &CancellationToken,
    partial: &mut Partial,
    accounting: &mut FileBytes<'_>,
    floor: Option<u64>,
    split_threshold: u64,
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

    if partial.len == 0
        && (task.sha1.is_some() || task.sha256.is_some())
        && let Some(total) = task.size.filter(|&size| size >= split_threshold && size >= PARALLEL_PARTS as u64)
    {
        match attempt_segmented(client, url, task, total, cancel, partial, accounting, floor).await {
            SegmentedOutcome::Completed(result) => return result,
            SegmentedOutcome::FallbackWhole(response) => {
                return finish_single_stream(response, false, task, cancel, partial, accounting, floor).await;
            }
            SegmentedOutcome::RetrySingle => {
                partial.restart(accounting);
            }
        }
    }

    let mut request = client.get(url);
    if partial.len > 0 {
        request = request
            .header(reqwest::header::RANGE, format!("bytes={}-", partial.len))
            .header(reqwest::header::ACCEPT_ENCODING, "identity");
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
    finish_single_stream(response, resumed, task, cancel, partial, accounting, floor).await
}

async fn finish_single_stream(
    response: reqwest::Response,
    resumed: bool,
    task: &DownloadTask,
    cancel: &CancellationToken,
    partial: &mut Partial,
    accounting: &mut FileBytes<'_>,
    floor: Option<u64>,
) -> AppResult<()> {
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

    verify_and_commit(task, partial, accounting)
}

pub(super) fn verify_and_commit(
    task: &DownloadTask,
    partial: &mut Partial,
    accounting: &mut FileBytes<'_>,
) -> AppResult<()> {
    if let Some(expected_size) = task.size
        && partial.len != expected_size
    {
        partial.restart(accounting);
        return Err(AppError::new(
            "download_size",
            crate::tr!("{} 的大小校验失败。", "{} failed the size check.", task.label),
        ));
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
    if let (Some(expected), Some(sha256)) = (&task.sha256, &partial.sha256)
        && !hex::encode(sha256.clone().finalize()).eq_ignore_ascii_case(expected)
    {
        partial.restart(accounting);
        return Err(AppError::new(
            "download_hash",
            crate::tr!("{} 的 SHA-256 校验失败。", "{} failed its SHA-256 check.", task.label),
        ));
    }
    crate::core::util::replace_file(&partial.path, &task.dest)?;
    Ok(())
}

/// The bytes of one file received so far, with the hashes of exactly those
/// bytes, so a later attempt can append instead of downloading again.
pub(super) struct Partial {
    pub(super) path: PathBuf,
    pub(super) len: u64,
    pub(super) sha1: Sha1,
    pub(super) sha256: Option<Sha256>,
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
    pub(super) fn restart(&mut self, accounting: &mut FileBytes<'_>) {
        accounting.remove(self.len);
        self.len = 0;
        self.sha1 = Sha1::new();
        if let Some(sha256) = &mut self.sha256 {
            *sha256 = Sha256::new();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(super) fn cancelled_error() -> AppError {
    AppError::new("cancelled", crate::core::i18n::pick("操作已取消。", "Cancelled."))
}

// Roll back on errors AND when another concurrent download fails and drops us.
pub(super) struct FileBytes<'a> {
    pub(super) progress: &'a DownloadProgress,
    pub(super) added: u64,
    pub(super) unknown: bool,
    pub(super) committed: bool,
}

impl FileBytes<'_> {
    fn add(&mut self, bytes: u64) {
        self.progress.add_bytes(bytes, self.unknown);
        self.added += bytes;
    }
    pub(super) fn remove(&mut self, bytes: u64) {
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
