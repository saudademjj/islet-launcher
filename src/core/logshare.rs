//! Sharing a game log for help: the tail of the log goes to mclo.gs (the
//! paste site Minecraft players use), which returns a public link. Paths
//! under the home folder and anything that looks like a token are masked
//! first; the player confirms before anything is sent.
use std::time::Duration;

use crate::core::error::{AppError, AppResult};

const LIMIT_BYTES: usize = 4 * 1024 * 1024;
const LIMIT_LINES: usize = 20_000;

/// Masks the user's home folder and common secrets/tokens before public upload.
pub fn redact(text: &str, home: Option<&str>) -> String {
    let home_variants: Vec<String> = home
        .filter(|h| h.len() > 2)
        .map(|h| {
            let trimmed = h.trim_end_matches(['/', '\\']);
            let mut variants = vec![trimmed.to_string()];
            let slash = trimmed.replace('\\', "/");
            if !variants.contains(&slash) {
                variants.push(slash);
            }
            let escaped = trimmed.replace('\\', "\\\\");
            if !variants.contains(&escaped) {
                variants.push(escaped);
            }
            variants
        })
        .unwrap_or_default();

    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut line = line.to_string();
        for variant in &home_variants {
            line = replace_ci(&line, variant, "~");
        }
        // CLI flags, URL query params, key=value pairs, and Bearer headers.
        for marker in [
            "--accessToken ",
            "--accessToken=",
            "accessToken=",
            "access_token=",
            "refresh_token=",
            "refreshToken=",
            "clientToken=",
            "sessionId=",
            "x-api-key=",
            "x-api-key: ",
            "Authorization: Bearer ",
            "Bearer ",
            "token:",
            "password=",
        ] {
            redact_after_marker(&mut line, marker);
        }
        // JSON fields like `"accessToken": "..."` or `"password":"..."`.
        for key in [
            "accessToken",
            "access_token",
            "refreshToken",
            "refresh_token",
            "clientToken",
            "sessionId",
            "x-api-key",
            "password",
        ] {
            redact_json_field(&mut line, key);
        }
        // CurseForge API keys are 60-character bcrypt hashes (`$2a$10$...`).
        redact_bcrypt_keys(&mut line);
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn replace_ci(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() || needle.len() > haystack.len() {
        return haystack.to_string();
    }
    let hay_bytes = haystack.as_bytes();
    let needle_bytes = needle.as_bytes();
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0;
    while i < hay_bytes.len() {
        if i + needle_bytes.len() <= hay_bytes.len()
            && haystack.is_char_boundary(i)
            && haystack.is_char_boundary(i + needle_bytes.len())
            && hay_bytes[i..i + needle_bytes.len()].eq_ignore_ascii_case(needle_bytes)
        {
            out.push_str(replacement);
            i += needle_bytes.len();
        } else if let Some(ch) = haystack[i..].chars().next() {
            out.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    out
}

fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return Some(0);
    }
    if n.len() > h.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

fn redact_after_marker(line: &mut String, marker: &str) {
    let mut offset = 0;
    while let Some(haystack) = line.get(offset..) {
        let Some(at) = find_ci(haystack, marker) else {
            break;
        };
        let start = offset + at + marker.len();
        let rest = &line[start..];
        let end = rest
            .find([' ', '\t', ',', '"', '\'', '&', ';', ')', ']', '}'])
            .map_or(line.len(), |e| start + e);
        if end > start {
            line.replace_range(start..end, "<hidden>");
            offset = start + "<hidden>".len();
        } else {
            offset = start;
        }
    }
}

fn redact_json_field(line: &mut String, key: &str) {
    let pattern = format!("\"{key}\"");
    let mut offset = 0;
    while let Some(haystack) = line.get(offset..) {
        let Some(at) = find_ci(haystack, &pattern) else {
            break;
        };
        let after_key = offset + at + pattern.len();
        let rest = &line[after_key..];
        let trimmed = rest.trim_start();
        let Some(after_colon) = trimmed.strip_prefix(':') else {
            offset = after_key;
            continue;
        };
        let after_colon_ws = after_colon.trim_start();
        let Some(quoted) = after_colon_ws.strip_prefix('"') else {
            offset = after_key;
            continue;
        };
        let value_start = line.len() - quoted.len();
        // Scan until unescaped closing quote.
        let mut escaped = false;
        let mut value_end = line.len();
        for (idx, ch) in quoted.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                value_end = value_start + idx;
                break;
            }
        }
        if value_end > value_start {
            line.replace_range(value_start..value_end, "<hidden>");
            offset = value_start + "<hidden>".len();
        } else {
            offset = value_start;
        }
    }
}

fn redact_bcrypt_keys(line: &mut String) {
    let mut offset = 0;
    while let Some(haystack) = line.get(offset..) {
        let Some(at) = haystack
            .find("$2a$")
            .or_else(|| haystack.find("$2b$"))
            .or_else(|| haystack.find("$2y$"))
        else {
            break;
        };
        let start = offset + at;
        let rest = &line[start..];
        let len = rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || b"$./".contains(b))
            .count();
        if len >= 40 {
            line.replace_range(start..start + len, "<hidden>");
            offset = start + "<hidden>".len();
        } else {
            offset = start + 4;
        }
    }
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
        let text = "Loading /Users/alex/Library/mods\n--username Alex --accessToken eyJ.abc.def --uuid 1 --accessToken eyJ.second\n";
        let out = redact(text, Some("/Users/alex"));
        assert!(out.contains("Loading ~/Library/mods"));
        assert!(out.contains("--accessToken <hidden> --uuid 1 --accessToken <hidden>"));
        assert!(!out.contains("eyJ"));
        let long = "x\n".repeat(LIMIT_LINES + 10);
        assert_eq!(tail(&long).lines().count(), LIMIT_LINES);
    }

    #[test]
    fn redacts_json_bearer_bcrypt_and_windows_escaped_home() {
        let cf_key = "$2a$10$abcdefghijklmnopqrstuvABCDEFGHIJKLMNOPQRSTUVWXYZ01234";
        let text = format!(
            "Path c:\\users\\Alice\\mods and \"C:\\\\Users\\\\Alice\\\\logs\"\n\
             {{\"accessToken\": \"secret1\", \"refresh_token\":\"secret2\"}}\n\
             Authorization: Bearer jwt.token.here x-api-key: {cf_key}\n"
        );
        let out = redact(&text, Some(r"C:\Users\Alice"));
        assert!(!out.contains("Alice"), "{out}");
        assert!(!out.contains("secret1"), "{out}");
        assert!(!out.contains("secret2"), "{out}");
        assert!(!out.contains("jwt.token.here"), "{out}");
        assert!(!out.contains("$2a$10$"), "{out}");
        assert!(out.contains("~\\mods"), "{out}");
        assert!(out.contains("\"~\\\\logs\""), "{out}");
    }
}
