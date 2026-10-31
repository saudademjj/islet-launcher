//! Sharing a game log for help: the tail of the log goes to mclo.gs (the
//! paste site Minecraft players use), which returns a public link. Paths
//! under the home folder and anything that looks like a token are masked
//! first; the player confirms before anything is sent.
use std::time::Duration;

use crate::core::error::{AppError, AppResult};

const LIMIT_BYTES: usize = 4 * 1024 * 1024;
const LIMIT_LINES: usize = 20_000;

/// Masks the user's home folder and access tokens.
pub fn redact(text: &str, home: Option<&str>) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut line = line.to_string();
        if let Some(home) = home.filter(|h| h.len() > 2) {
            line = line.replace(home, "~");
            // Windows logs often use forward slashes too.
            line = line.replace(&home.replace('\\', "/"), "~");
        }
        for marker in ["--accessToken ", "accessToken=", "token:"] {
            if let Some(at) = line.find(marker) {
                let start = at + marker.len();
                let end = line[start..].find([' ', ',', '"', '\'']).map_or(line.len(), |e| start + e);
                line.replace_range(start..end, "<hidden>");
            }
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// The last part of a log that fits the paste site's limits.
pub fn tail(text: &str) -> &str {
    let mut start = text.len().saturating_sub(LIMIT_BYTES);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let mut part = &text[start..];
    let lines = part.lines().count();
    if lines > LIMIT_LINES {
        let skip = lines - LIMIT_LINES;
        let offset = part.match_indices('\n').nth(skip - 1).map_or(0, |(i, _)| i + 1);
        part = &part[offset..];
    }
    part
}

/// Uploads and returns the link.
pub async fn upload(client: &reqwest::Client, content: &str) -> AppResult<String> {
    #[derive(serde::Deserialize)]
    struct Reply {
        success: bool,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        error: Option<String>,
    }
    let reply: Reply = client
        .post("https://api.mclo.gs/1/log")
        .form(&[("content", content)])
        .timeout(Duration::from_secs(30))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    match (reply.success, reply.url) {
        (true, Some(url)) if url.starts_with("https://") => Ok(url),
        _ => Err(AppError::new("log_upload", crate::tr!("上传失败：{}", "Upload failed: {}", reply.error.unwrap_or_else(|| crate::core::i18n::pick("mclo.gs 没有返回链接", "mclo.gs returned no link").into())))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_lose_home_paths_and_tokens() {
        let text = "Loading /Users/alex/Library/mods\n--username Alex --accessToken eyJ.abc.def --uuid 1\n";
        let out = redact(text, Some("/Users/alex"));
        assert!(out.contains("Loading ~/Library/mods"));
        assert!(out.contains("--accessToken <hidden> --uuid 1"));
        assert!(!out.contains("eyJ"));
        let long = "x\n".repeat(LIMIT_LINES + 10);
        assert_eq!(tail(&long).lines().count(), LIMIT_LINES);
    }
}
