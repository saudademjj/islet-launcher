//! Multi-range parallel downloading for large files with clean rollback on failure.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::StreamExt;
use sha1::{Digest, Sha1};
use sha2::Sha256;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::core::error::{AppError, AppResult};

use super::speed::{SLOW_WINDOW, SpeedWindow, content_range_bounds};
use super::{DownloadProgress, DownloadTask, FileBytes, Partial, cancelled_error, verify_and_commit};

/// Files at least this large are split across `PARALLEL_PARTS` concurrent
/// HTTP Range streams when the server answers 206 Partial Content.
pub(super) const RANGE_SPLIT_THRESHOLD: u64 = 8 * 1024 * 1024;
pub(super) const PARALLEL_PARTS: usize = 4;

pub(super) enum SegmentedOutcome {
    Completed(AppResult<()>),
    /// Server ignored `Range` and returned `200 OK` with the entire body; stream it directly.
    FallbackWhole(reqwest::Response),
    /// A sibling range failed after 206; fall back cleanly to a single-stream attempt.
    RetrySingle,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn attempt_segmented(
    client: &reqwest::Client,
    url: &str,
    task: &DownloadTask,
    total: u64,
    cancel: &CancellationToken,
    partial: &mut Partial,
    accounting: &mut FileBytes<'_>,
    floor: Option<u64>,
) -> SegmentedOutcome {
    let chunk_size = total.div_ceil(PARALLEL_PARTS as u64);
    let ranges: Vec<(u64, u64)> = (0..PARALLEL_PARTS as u64)
        .filter_map(|i| {
            let start = i * chunk_size;
            (start < total).then_some((start, (start + chunk_size - 1).min(total - 1)))
        })
        .collect();
    if ranges.len() < 2 {
        return SegmentedOutcome::RetrySingle;
    }

    let (first_start, first_end) = ranges[0];
    let first_req = client
        .get(url)
        .header(reqwest::header::RANGE, format!("bytes={first_start}-{first_end}"))
        .header(reqwest::header::ACCEPT_ENCODING, "identity");
    let first_resp = tokio::select! {
        _ = cancel.cancelled() => return SegmentedOutcome::Completed(Err(cancelled_error())),
        resp = first_req.send() => match resp {
            Ok(resp) => resp,
            Err(err) => return SegmentedOutcome::Completed(Err(AppError::from(err))),
        },
    };
    let status = first_resp.status();
    if status == reqwest::StatusCode::OK {
        return SegmentedOutcome::FallbackWhole(first_resp);
    }
    if status != reqwest::StatusCode::PARTIAL_CONTENT
        || content_range_bounds(&first_resp) != Some((first_start, first_end, total))
    {
        if !status.is_success() {
            return SegmentedOutcome::Completed(Err(AppError::new(
                "download",
                crate::tr!("下载 {} 失败（HTTP {status}）。", "Downloading {} failed (HTTP {status}).", task.label),
            )));
        }
        return SegmentedOutcome::RetrySingle;
    }

    let part_paths: Vec<PathBuf> = (0..ranges.len())
        .map(|i| {
            let mut os = partial.path.as_os_str().to_os_string();
            os.push(format!(".seg{i}"));
            PathBuf::from(os)
        })
        .collect();
    struct SegCleanup(Vec<PathBuf>);
    impl Drop for SegCleanup {
        fn drop(&mut self) {
            for p in &self.0 {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    let _seg_cleanup = SegCleanup(part_paths.clone());

    // Rolls back segment bytes even if this future is dropped mid-download by a
    // failing sibling task in `download_all`.
    struct SegBytes<'a> {
        progress: &'a DownloadProgress,
        unknown: bool,
        added: Arc<AtomicU64>,
        transferred: bool,
    }
    impl Drop for SegBytes<'_> {
        fn drop(&mut self) {
            if !self.transferred {
                let bytes = self.added.load(Ordering::Relaxed);
                self.progress.bytes_done.fetch_sub(bytes, Ordering::Relaxed);
                if self.unknown {
                    self.progress.bytes_total.fetch_sub(bytes, Ordering::Relaxed);
                }
            }
        }
    }

    let added = Arc::new(AtomicU64::new(0));
    let mut seg_bytes = SegBytes {
        progress: accounting.progress,
        unknown: accounting.unknown,
        added: added.clone(),
        transferred: false,
    };
    let sub_cancel = cancel.child_token();
    let per_part_floor = floor.map(|f| (f / ranges.len() as u64).max(16 * 1024));

    let mut futures_vec: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = AppResult<()>> + Send + '_>>> =
        Vec::with_capacity(ranges.len());

    let p0 = part_paths[0].clone();
    let added0 = added.clone();
    let progress_ref = accounting.progress;
    let unknown = accounting.unknown;
    let sc0 = sub_cancel.clone();
    futures_vec.push(Box::pin(async move {
        stream_range_body(
            first_resp,
            &p0,
            first_end - first_start + 1,
            &sc0,
            per_part_floor,
            &task.label,
            progress_ref,
            unknown,
            &added0,
        )
        .await
    }));

    for (idx, &(start, end)) in ranges.iter().enumerate().skip(1) {
        let path = part_paths[idx].clone();
        let added_i = added.clone();
        let sc = sub_cancel.clone();
        futures_vec.push(Box::pin(async move {
            let req = client
                .get(url)
                .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
                .header(reqwest::header::ACCEPT_ENCODING, "identity");
            let resp = tokio::select! {
                _ = sc.cancelled() => return Err(cancelled_error()),
                r = req.send() => r?,
            };
            if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT
                || content_range_bounds(&resp) != Some((start, end, total))
            {
                return Err(AppError::new("download_range", "range rejected"));
            }
            stream_range_body(
                resp,
                &path,
                end - start + 1,
                &sc,
                per_part_floor,
                &task.label,
                progress_ref,
                unknown,
                &added_i,
            )
            .await
        }));
    }

    let joined = futures::future::try_join_all(futures_vec).await;
    seg_bytes.transferred = true;
    accounting.added += added.load(Ordering::Relaxed);
    if let Err(err) = joined {
        sub_cancel.cancel();
        if err.code() == "cancelled" || err.code() == "download_slow" {
            partial.restart(accounting);
            accounting.remove(accounting.added);
            return SegmentedOutcome::Completed(Err(err));
        }
        accounting.remove(accounting.added);
        return SegmentedOutcome::RetrySingle;
    }

    // Stitch the verified segments sequentially into `partial.path` on a blocking
    // thread while updating SHA-1/SHA-256, keeping disk+hash CPU off the reactor.
    let stitch_path = partial.path.clone();
    let stitch_parts = part_paths.clone();
    let with_sha256 = partial.sha256.is_some();
    let stitch = tokio::task::spawn_blocking(move || -> AppResult<(u64, Sha1, Option<Sha256>)> {
        use std::io::{Read, Write};
        let mut out = std::fs::File::create(&stitch_path)?;
        let mut buf = [0u8; 64 * 1024];
        let mut len = 0u64;
        let mut sha1 = Sha1::new();
        let mut sha256 = with_sha256.then(Sha256::new);
        for seg in &stitch_parts {
            let mut f = std::fs::File::open(seg)?;
            loop {
                let n = f.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n])?;
                sha1.update(&buf[..n]);
                if let Some(h) = &mut sha256 {
                    h.update(&buf[..n]);
                }
                len += n as u64;
            }
        }
        out.flush()?;
        Ok((len, sha1, sha256))
    })
    .await
    .map_err(|e| AppError::detail("download", crate::tr!("合并 {} 失败。", "Combining {} failed.", task.label), e.to_string()))
    .and_then(|res| res);
    match stitch {
        Ok((len, sha1, sha256)) => {
            partial.len = len;
            partial.sha1 = sha1;
            partial.sha256 = sha256;
        }
        Err(err) => {
            partial.restart(accounting);
            accounting.remove(accounting.added);
            return SegmentedOutcome::Completed(Err(err));
        }
    }
    SegmentedOutcome::Completed(verify_and_commit(task, partial, accounting))
}

#[allow(clippy::too_many_arguments)]
async fn stream_range_body(
    response: reqwest::Response,
    dest: &Path,
    expected_len: u64,
    cancel: &CancellationToken,
    floor: Option<u64>,
    label: &str,
    progress: &DownloadProgress,
    unknown_total: bool,
    added: &AtomicU64,
) -> AppResult<()> {
    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = response.bytes_stream();
    let mut written = 0u64;
    let mut speed = SpeedWindow::new();
    let mut check = tokio::time::interval(SLOW_WINDOW / 10);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Err(cancelled_error()),
            _ = check.tick(), if floor.is_some() => {
                if floor.is_some_and(|f| speed.below(f)) {
                    return Err(AppError::new("download_slow", crate::tr!("{label} 的下载源太慢。", "The source for {label} is too slow.")));
                }
            }
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(bytes)) => {
                        let n = bytes.len() as u64;
                        if written.saturating_add(n) > expected_len {
                            return Err(AppError::new("download_size", crate::tr!("{label} 超出预期大小。", "{label} is larger than expected.")));
                        }
                        file.write_all(&bytes).await?;
                        written += n;
                        progress.add_bytes(n, unknown_total);
                        added.fetch_add(n, Ordering::Relaxed);
                        speed.add(n);
                    }
                    Some(Err(err)) => return Err(AppError::from(err)),
                    None => break,
                }
            }
        }
    }
    if written != expected_len {
        return Err(AppError::new("download_size", crate::tr!("{label} 分片不完整。", "A segment of {label} was cut short.")));
    }
    file.flush().await?;
    Ok(())
}
