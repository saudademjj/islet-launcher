//! Speed window monitoring, source speed probing, and HTTP Content-Range parsing.
use std::time::{Duration, Instant};

use futures::StreamExt;
use tokio_util::sync::CancellationToken;

/// How long a source gets to show its speed each time `min_speed` judges it.
pub(super) const SLOW_WINDOW: Duration = if cfg!(test) { Duration::from_millis(300) } else { Duration::from_secs(10) };

/// Bytes received in the current stretch of `SLOW_WINDOW`. A new stretch
/// starts each time one is judged, so a source that slows down later is
/// caught too.
pub(super) struct SpeedWindow {
    started: Instant,
    bytes: u64,
}

impl SpeedWindow {
    pub(super) fn new() -> Self {
        Self { started: Instant::now(), bytes: 0 }
    }

    pub(super) fn add(&mut self, bytes: u64) {
        self.bytes += bytes;
    }

    pub(super) fn below(&mut self, floor: u64) -> bool {
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

/// Bounds `(start, end, total)` of a `Content-Range: bytes <start>-<end>/<total>` header.
pub(super) fn content_range_bounds(response: &reqwest::Response) -> Option<(u64, u64, u64)> {
    let value = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?;
    let (span, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = span.split_once('-')?;
    Some((
        start.trim().parse().ok()?,
        end.trim().parse().ok()?,
        total.trim().parse().ok()?,
    ))
}

/// Start offset of a `Content-Range: bytes <start>-<end>/<total>` header.
pub(super) fn content_range_start(response: &reqwest::Response) -> Option<u64> {
    content_range_bounds(response).map(|(start, _, _)| start)
}
