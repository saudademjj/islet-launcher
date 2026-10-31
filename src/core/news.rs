//! What is new in Minecraft: Mojang's own Java Edition patch notes feed (the
//! one the official launcher shows), newest first. Only titles and the short
//! summary are used; the full notes open on the Chinese Minecraft Wiki.
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::error::AppResult;

const FEED: &str = "https://launchercontent.mojang.com/v2/javaPatchNotes.json";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchNote {
    pub title: String,
    pub version: String,
    /// "release" or "snapshot".
    #[serde(rename(deserialize = "type"))]
    pub kind: String,
    #[serde(default)]
    pub date: String,
    #[serde(default, rename(deserialize = "shortText"))]
    pub summary: String,
}

#[derive(Deserialize)]
struct Feed {
    entries: Vec<PatchNote>,
}

pub async fn patch_notes(client: &reqwest::Client, limit: usize) -> AppResult<Vec<PatchNote>> {
    let feed: Feed = client.get(FEED).timeout(Duration::from_secs(15)).send().await?.error_for_status()?.json().await?;
    let mut entries = feed.entries;
    entries.sort_by(|a, b| b.date.cmp(&a.date));
    entries.truncate(limit);
    for entry in &mut entries {
        entry.summary = entry.summary.chars().take(240).collect();
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_entries_parse() {
        let feed: Feed = serde_json::from_str(
            r#"{"version":1,"entries":[{"title":"Minecraft Java Edition 1.21.4","version":"1.21.4","type":"release","date":"2024-12-03T09:00:00Z","shortText":"The Garden Awakens","image":{"url":"x"},"id":"a","contentPath":"b"}]}"#,
        )
        .unwrap();
        assert_eq!(feed.entries[0].kind, "release");
        assert_eq!(feed.entries[0].summary, "The Garden Awakens");
    }
}
