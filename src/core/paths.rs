use std::fs;
use std::path::PathBuf;

use crate::core::error::{AppError, AppResult};

/// Keep the original Tauri bundle identifier as the data directory name so
/// accounts, settings and installed instances survive the native rewrite.
const APP_DATA_DIR: &str = "com.saudade.mclauncher";

/// All on-disk locations used by the launcher. Shared caches are kept separate
/// from per-instance game directories so several instances can reuse the same
/// assets, libraries and Java runtimes.
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub instances: PathBuf,
    pub cache: PathBuf,
    pub versions: PathBuf,
    pub libraries: PathBuf,
    pub assets: PathBuf,
    pub asset_indexes: PathBuf,
    pub asset_objects: PathBuf,
    pub java: PathBuf,
    pub logs: PathBuf,
}

impl AppPaths {
    /// Resolve the platform data directory, matching Tauri's app_data_dir:
    /// macOS `~/Library/Application Support`, Windows `%APPDATA%`, Linux
    /// `$XDG_DATA_HOME` (or `~/.local/share`).
    pub fn default_root() -> AppResult<PathBuf> {
        let base = directories::BaseDirs::new()
            .ok_or_else(|| AppError::new("paths", crate::core::i18n::pick("无法确定用户数据目录，请检查系统环境变量。", "Couldn't find the user data folder; check the system environment variables.")))?;
        Ok(base.data_dir().join(APP_DATA_DIR))
    }

    pub fn new() -> AppResult<Self> {
        let default = Self::default_root()?;
        let locator = default.join("storage-location.json");
        if locator.is_file() {
            let root: PathBuf = serde_json::from_slice(&fs::read(locator)?)?;
            if !root.is_absolute() || !root.join("settings.json").is_file() {
                return Err(AppError::new(
                    "storage",
                    crate::core::i18n::pick("自定义数据目录不可用，请连接对应磁盘后重试。", "The custom data folder is unavailable; connect its disk and try again."),
                ));
            }
            Self::from_root(root)
        } else {
            Self::from_root(default)
        }
    }

    pub fn from_root(root: impl Into<PathBuf>) -> AppResult<Self> {
        let root = root.into();
        let instances = root.join("instances");
        let cache = root.join("cache");
        let versions = cache.join("versions");
        let libraries = cache.join("libraries");
        let assets = cache.join("assets");
        let asset_indexes = assets.join("indexes");
        let asset_objects = assets.join("objects");
        let java = cache.join("java");
        let logs = root.join("logs");

        let paths = Self {
            root,
            instances,
            cache,
            versions,
            libraries,
            assets,
            asset_indexes,
            asset_objects,
            java,
            logs,
        };
        for dir in [
            &paths.root,
            &paths.instances,
            &paths.versions,
            &paths.libraries,
            &paths.asset_indexes,
            &paths.asset_objects,
            &paths.java,
            &paths.logs,
        ] {
            fs::create_dir_all(dir)?;
        }
        Ok(paths)
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn accounts_file(&self) -> PathBuf {
        self.root.join("accounts.json")
    }

    pub fn version_manifest_file(&self) -> PathBuf {
        self.versions.join("manifest.json")
    }

    pub fn instance_dir(&self, id: &str) -> PathBuf {
        self.instances.join(id)
    }

    pub fn instance_file(&self, id: &str) -> PathBuf {
        self.instance_dir(id).join("instance.json")
    }

    pub fn instance_game_dir(&self, id: &str) -> PathBuf {
        self.instance_dir(id).join("minecraft")
    }

    pub fn instance_natives_dir(&self, id: &str) -> PathBuf {
        self.instance_dir(id).join("natives")
    }

    pub fn instance_logs_dir(&self, id: &str) -> PathBuf {
        self.instance_dir(id).join("logs")
    }

    pub fn ensure_instance_dirs(&self, id: &str) -> AppResult<()> {
        for dir in [
            self.instance_dir(id),
            self.instance_game_dir(id),
            self.instance_game_dir(id).join("mods"),
            self.instance_natives_dir(id),
            self.instance_logs_dir(id),
        ] {
            fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn version_cache_dir(&self, id: &str) -> PathBuf {
        self.versions.join(id)
    }

    pub fn version_json(&self, id: &str) -> PathBuf {
        self.version_cache_dir(id).join("version.json")
    }

    pub fn client_jar(&self, id: &str) -> PathBuf {
        self.version_cache_dir(id).join("client.jar")
    }

    pub fn library_path(&self, relative: &str) -> PathBuf {
        self.libraries.join(relative)
    }

    pub fn asset_object(&self, hash: &str) -> PathBuf {
        let prefix = hash.get(0..2).unwrap_or("00");
        self.asset_objects.join(prefix).join(hash)
    }

    pub fn asset_index(&self, id: &str) -> PathBuf {
        self.asset_indexes.join(format!("{id}.json"))
    }

    pub fn java_dir(&self, vendor: &str, major: u32, arch: &str) -> PathBuf {
        self.java.join(vendor).join(major.to_string()).join(arch)
    }

    pub fn validate_instance_id(id: &str) -> AppResult<()> {
        uuid::Uuid::parse_str(id)
            .map(|_| ())
            .map_err(|_| AppError::new("invalid_instance", crate::core::i18n::pick("世界 ID 无效。", "Invalid world ID.")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_root_matches_the_previous_bundle_identifier() {
        let root = AppPaths::default_root().unwrap();
        assert!(root.ends_with("com.saudade.mclauncher"));
        #[cfg(target_os = "macos")]
        {
            let expected = PathBuf::from(std::env::var("HOME").unwrap())
                .join("Library/Application Support/com.saudade.mclauncher");
            assert_eq!(root, expected);
        }
    }
}
