use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::core::error::{AppError, AppResult};
use crate::core::util::{atomic_write, read_json_or_default};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum DownloadSource {
    Official,
    #[default]
    Bmclapi,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum ProxyMode {
    /// The system's proxy settings (or HTTP_PROXY and friends).
    #[default]
    System,
    None,
    Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Configs written before versioning existed have no field at all; they
    /// deserialize as 0 so `normalize` still applies every migration.
    #[serde(default = "legacy_schema_version")]
    pub schema_version: u32,
    pub client_id: String,
    pub download_source: DownloadSource,
    pub max_concurrent_downloads: usize,
    pub memory_mb: u32,
    pub memory_auto: bool,
    pub hide_on_launch: bool,
    /// The close button hides the window when a system tray is available.
    pub close_to_tray: bool,
    /// Set the game language to the launcher's before an instance's first
    /// launch (only while it has no options.txt).
    pub auto_chinese: bool,
    pub java_path: Option<String>,
    pub theme: Theme,
    pub active_account_id: Option<String>,
    pub active_instance_id: Option<String>,
    /// Launch options for every world; a world's own add to or replace them.
    pub launch: crate::core::instances::LaunchConfig,
    /// A CurseForge API key (console.curseforge.com); empty turns CurseForge off.
    pub curseforge_key: String,
    /// Check for a new launcher version at start.
    pub check_updates: bool,
    pub system_notifications: bool,
    pub discord_application_id: String,
    /// "system", "zh" or "en".
    pub language: String,
    pub proxy_mode: ProxyMode,
    /// The proxy for `ProxyMode::Custom`, e.g. `http://127.0.0.1:7890`.
    pub proxy_url: String,
    /// Back up each save the game changed, after the game exits.
    pub auto_backup: bool,
    /// Automatic backups kept per save; older ones are deleted.
    pub auto_backup_keep: u32,
    /// Run the game on a laptop's discrete GPU (Windows and Linux).
    pub dedicated_gpu: bool,
}

fn legacy_schema_version() -> u32 {
    0
}

/// Default number of files downloaded at once. Most files are small assets
/// where latency, not bandwidth, sets the pace.
pub const DEFAULT_CONCURRENT_DOWNLOADS: usize = 32;
pub const MAX_CONCURRENT_DOWNLOADS: usize = 64;
const SCHEMA_VERSION: u32 = 3;

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            client_id: String::new(),
            download_source: DownloadSource::Bmclapi,
            max_concurrent_downloads: DEFAULT_CONCURRENT_DOWNLOADS,
            memory_mb: 4096,
            memory_auto: true,
            hide_on_launch: true,
            close_to_tray: true,
            auto_chinese: true,
            java_path: None,
            theme: Theme::System,
            active_account_id: None,
            active_instance_id: None,
            launch: crate::core::instances::LaunchConfig::default(),
            curseforge_key: String::new(),
            check_updates: true,
            system_notifications: true,
            discord_application_id: String::new(),
            language: "system".into(),
            proxy_mode: ProxyMode::System,
            proxy_url: String::new(),
            auto_backup: true,
            auto_backup_keep: 5,
            dedicated_gpu: true,
        }
    }
}

impl Settings {
    pub fn effective_client_id(&self) -> String {
        if self.client_id.is_empty() {
            option_env!("MC_LAUNCHER_CLIENT_ID")
                .unwrap_or("")
                .trim()
                .to_string()
        } else {
            self.client_id.clone()
        }
    }

    pub fn normalize(&mut self) {
        // Schema 1 defaulted to 8 parallel downloads; move untouched values
        // to the new default.
        if self.schema_version < 2 && self.max_concurrent_downloads == 8 {
            self.max_concurrent_downloads = DEFAULT_CONCURRENT_DOWNLOADS;
        }
        // Before schema 3 the only memory option was the global value. 4096
        // was the untouched default; anything else means the user chose it.
        if self.schema_version < 3 {
            self.memory_auto = self.memory_mb == 4096;
        }
        self.schema_version = SCHEMA_VERSION;
        self.client_id = self.client_id.trim().to_string();
        self.max_concurrent_downloads = self
            .max_concurrent_downloads
            .clamp(1, MAX_CONCURRENT_DOWNLOADS);
        self.memory_mb = self.memory_mb.clamp(512, 32_768);
        self.auto_backup_keep = self.auto_backup_keep.clamp(1, 50);
        self.java_path = self
            .java_path
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.active_account_id = self
            .active_account_id
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.active_instance_id = self
            .active_instance_id
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
    }
}

pub struct SettingsStore {
    path: PathBuf,
    data: RwLock<Settings>,
}

impl SettingsStore {
    pub fn load(path: PathBuf) -> AppResult<Self> {
        let mut data: Settings = read_json_or_default(&path)?;
        data.normalize();
        Ok(Self {
            path,
            data: RwLock::new(data),
        })
    }

    pub fn get(&self) -> Settings {
        self.data
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub fn update<F>(&self, mutate: F) -> AppResult<Settings>
    where
        F: FnOnce(&mut Settings),
    {
        let mut guard = self
            .data
            .write()
            .map_err(|_| AppError::new("settings", crate::core::i18n::pick("设置存储被锁定，请重试。", "Settings storage is locked; try again.")))?;
        mutate(&mut guard);
        guard.normalize();
        let encoded = serde_json::to_vec_pretty(&*guard)?;
        atomic_write(&self.path, &encoded)?;
        Ok(guard.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_settings_gain_tray_close_without_losing_explicit_choices() {
        let mut settings: Settings = serde_json::from_str(r#"{"schemaVersion":3,"memoryMb":6144,"hideOnLaunch":false}"#).unwrap();
        settings.normalize();
        assert!(settings.close_to_tray);
        assert_eq!(settings.memory_mb, 6144);
        assert!(!settings.hide_on_launch);
        settings.close_to_tray = false;
        let restored: Settings = serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(!restored.close_to_tray);
    }

    #[test]
    fn untouched_schema_1_concurrency_moves_to_the_new_default() {
        let mut old: Settings =
            serde_json::from_str(r#"{"schemaVersion":1,"maxConcurrentDownloads":8}"#).unwrap();
        old.normalize();
        assert_eq!(old.max_concurrent_downloads, DEFAULT_CONCURRENT_DOWNLOADS);
        assert_eq!(old.schema_version, SCHEMA_VERSION);

        let mut chosen: Settings =
            serde_json::from_str(r#"{"schemaVersion":1,"maxConcurrentDownloads":4}"#).unwrap();
        chosen.normalize();
        assert_eq!(chosen.max_concurrent_downloads, 4);

        // Under schema 2, 8 is a deliberate choice and stays.
        let mut current = Settings {
            max_concurrent_downloads: 8,
            ..Settings::default()
        };
        current.normalize();
        assert_eq!(current.max_concurrent_downloads, 8);
    }

    #[test]
    fn schema_2_default_memory_turns_automatic_on() {
        let mut defaults: Settings =
            serde_json::from_str(r#"{"schemaVersion":2,"memoryMb":4096}"#).unwrap();
        defaults.normalize();
        assert!(defaults.memory_auto);
        assert_eq!(defaults.schema_version, SCHEMA_VERSION);

        let mut chosen: Settings =
            serde_json::from_str(r#"{"schemaVersion":2,"memoryMb":8192}"#).unwrap();
        chosen.normalize();
        assert!(!chosen.memory_auto);
    }

    #[test]
    fn configs_without_a_schema_version_are_migrated() {
        let mut defaults: Settings = serde_json::from_str(r#"{"memoryMb":4096}"#).unwrap();
        assert_eq!(defaults.schema_version, 0);
        defaults.normalize();
        assert!(defaults.memory_auto);
        assert!(defaults.hide_on_launch);
        assert_eq!(defaults.schema_version, SCHEMA_VERSION);

        let mut chosen: Settings = serde_json::from_str(r#"{"memoryMb":6144}"#).unwrap();
        chosen.normalize();
        assert!(!chosen.memory_auto);
    }

    #[test]
    fn schema_3_keeps_an_explicit_manual_choice() {
        let mut settings: Settings = serde_json::from_str(
            r#"{"schemaVersion":3,"memoryMb":4096,"memoryAuto":false,"hideOnLaunch":false}"#,
        )
        .unwrap();
        settings.normalize();
        assert!(!settings.memory_auto);
        assert!(!settings.hide_on_launch);
    }
}
