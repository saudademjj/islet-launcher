use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::error::{AppError, AppResult};
use crate::core::paths::AppPaths;
use crate::core::util::{atomic_write, now_millis};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub version_id: String,
    pub version_type: String,
    pub created_at: i64,
    pub last_launched_at: Option<i64>,
    pub installed: bool,
    /// "vanilla" for now; Fabric/Forge metadata can be added here later.
    pub loader: String,
    pub loader_version: Option<String>,
    pub memory_mb: Option<u32>,
    pub java_path: Option<String>,
    /// Seconds spent in game across all launches.
    pub play_seconds: u64,
    /// This world's own launch options; empty fields use the global ones.
    pub launch: LaunchConfig,
    /// Plays with this account instead of the one in use, when it exists.
    pub account_id: Option<String>,
    /// Starred on the worlds page.
    pub favorite: bool,
    /// The shelf it sits on in the worlds page; none for ungrouped.
    pub group: Option<String>,
}

/// Options for starting the game, kept globally and per world.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LaunchConfig {
    /// Extra JVM arguments, as typed in a shell.
    pub jvm_args: String,
    /// Extra game arguments.
    pub game_args: String,
    pub window_width: Option<u32>,
    pub window_height: Option<u32>,
    /// `KEY=VALUE` per line.
    pub env: String,
    /// A command the game runs inside, such as `gamemoderun` or `prime-run`.
    pub wrapper: String,
    /// A shell command run (and waited for) before each launch.
    pub pre_launch: String,
    /// A shell command run after the game exits, with its exit code in
    /// `MC_EXIT_CODE`.
    pub post_exit: String,
}

impl LaunchConfig {
    pub fn validate(&self) -> AppResult<()> {
        crate::core::launch::split_args(&self.jvm_args)?;
        crate::core::launch::split_args(&self.game_args)?;
        crate::core::launch::split_args(&self.wrapper)?;
        for size in [self.window_width, self.window_height].into_iter().flatten() {
            if !(200..=16384).contains(&size) {
                return Err(AppError::new("window_size", crate::core::i18n::pick("窗口宽高需要在 200–16384 像素之间。", "Window size must be between 200 and 16384 pixels.")));
            }
        }
        if self.window_width.is_some() != self.window_height.is_some() {
            return Err(AppError::new("window_size", crate::core::i18n::pick("窗口宽度和高度需要一起填写。", "Fill in both window width and height.")));
        }
        for line in self.env.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
            let Some((key, _)) = line.split_once('=') else {
                return Err(AppError::new("launch_env", crate::tr!("环境变量「{line}」需要写成 名称=值。", "Write the environment variable \"{line}\" as NAME=value.")));
            };
            if key.trim().is_empty() || key.contains(char::is_whitespace) {
                return Err(AppError::new("launch_env", crate::tr!("环境变量名「{key}」无效。", "Invalid environment variable name \"{key}\".")));
            }
        }
        Ok(())
    }

    /// Environment variables as pairs, comments and blank lines skipped.
    pub fn env_pairs(&self) -> Vec<(String, String)> {
        self.env
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
            .collect()
    }

    /// This world's options over the global ones: arguments add up, the
    /// window, wrapper and pre-launch and post-exit commands of the world win, and its
    /// environment variables override global ones of the same name.
    pub fn over(&self, global: &LaunchConfig) -> LaunchConfig {
        let join = |a: &str, b: &str| [a.trim(), b.trim()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
        let pick = |own: &str, other: &str| if own.trim().is_empty() { other.to_string() } else { own.to_string() };
        let (window_width, window_height) = if self.window_width.is_some() {
            (self.window_width, self.window_height)
        } else {
            (global.window_width, global.window_height)
        };
        LaunchConfig {
            jvm_args: join(&global.jvm_args, &self.jvm_args),
            game_args: join(&global.game_args, &self.game_args),
            window_width,
            window_height,
            env: format!("{}\n{}", global.env, self.env),
            wrapper: pick(&self.wrapper, &global.wrapper),
            pre_launch: pick(&self.pre_launch, &global.pre_launch),
            post_exit: pick(&self.post_exit, &global.post_exit),
        }
    }
}

impl Default for Instance {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            version_id: String::new(),
            version_type: "release".to_string(),
            created_at: 0,
            last_launched_at: None,
            installed: false,
            loader: "vanilla".to_string(),
            loader_version: None,
            memory_mb: None,
            java_path: None,
            play_seconds: 0,
            launch: LaunchConfig::default(),
            account_id: None,
            favorite: false,
            group: None,
        }
    }
}

/// Mod loaders a world can use.
pub const LOADERS: [&str; 8] = ["vanilla", "fabric", "quilt", "forge", "neoforge", "legacyfabric", "liteloader", "cleanroom"];

pub struct InstanceStore {
    paths: AppPaths,
    instances: RwLock<Vec<Instance>>,
}

impl InstanceStore {
    pub fn load(paths: AppPaths) -> AppResult<Self> {
        let mut instances = Vec::new();
        if let Ok(entries) = fs::read_dir(&paths.instances) {
            for entry in entries.flatten() {
                let dir = entry.path();
                if !dir.is_dir() {
                    continue;
                }
                let file = dir.join("instance.json");
                let Ok(bytes) = fs::read(&file) else {
                    continue;
                };
                match serde_json::from_slice::<Instance>(&bytes) {
                    Ok(instance)
                        if AppPaths::validate_instance_id(&instance.id).is_ok()
                            && dir
                                .file_name()
                                .is_some_and(|name| name == instance.id.as_str())
                            && !entry.file_type().is_ok_and(|ty| ty.is_symlink()) =>
                    {
                        instances.push(instance)
                    }
                    Ok(_) => {
                        tracing::warn!(path = %file.display(), "ignoring instance with invalid id")
                    }
                    Err(err) => {
                        tracing::warn!(path = %file.display(), error = %err, "ignoring invalid instance")
                    }
                }
            }
        }
        instances.sort_by_key(|instance| instance.created_at);
        Ok(Self {
            paths,
            instances: RwLock::new(instances),
        })
    }

    pub fn list(&self) -> Vec<Instance> {
        self.instances
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub fn get(&self, id: &str) -> AppResult<Instance> {
        AppPaths::validate_instance_id(id)?;
        self.instances
            .read()
            .ok()
            .and_then(|guard| guard.iter().find(|instance| instance.id == id).cloned())
            .ok_or_else(|| AppError::new("instance_not_found", crate::core::i18n::pick("世界不存在。", "The world doesn't exist.")))
    }

    pub fn create(&self, name: &str, version_id: &str, version_type: &str) -> AppResult<Instance> {
        let name = name.trim();
        let count = name.chars().count();
        if count == 0 || count > 40 {
            return Err(AppError::new("instance_name", crate::core::i18n::pick("世界名称需要 1–40 个字符。", "World names need 1–40 characters.")));
        }
        if version_id.trim().is_empty() {
            return Err(AppError::new("version", crate::core::i18n::pick("请选择 Minecraft 版本。", "Choose a Minecraft version.")));
        }

        let id = Uuid::new_v4().to_string();
        let instance = Instance {
            id: id.clone(),
            name: name.to_string(),
            version_id: version_id.to_string(),
            version_type: if version_type.trim().is_empty() {
                "release".to_string()
            } else {
                version_type.to_string()
            },
            created_at: now_millis(),
            last_launched_at: None,
            installed: false,
            loader: "vanilla".to_string(),
            loader_version: None,
            memory_mb: None,
            java_path: None,
            play_seconds: 0,
            ..Instance::default()
        };
        self.paths.ensure_instance_dirs(&id)?;
        self.save(&instance)?;

        let mut guard = self
            .instances
            .write()
            .map_err(|_| AppError::new("instances", crate::core::i18n::pick("世界存储被锁定，请重试。", "World storage is locked; try again.")))?;
        guard.push(instance.clone());
        guard.sort_by_key(|item| item.created_at);
        Ok(instance)
    }

    pub fn update(&self, instance: &Instance) -> AppResult<()> {
        self.save(instance)?;
        let mut guard = self
            .instances
            .write()
            .map_err(|_| AppError::new("instances", crate::core::i18n::pick("世界存储被锁定，请重试。", "World storage is locked; try again.")))?;
        if let Some(existing) = guard.iter_mut().find(|item| item.id == instance.id) {
            *existing = instance.clone();
        } else {
            guard.push(instance.clone());
        }
        Ok(())
    }

    /// Move the whole instance to a recoverable local recycle bin.
    pub fn remove(&self, id: &str) -> AppResult<()> {
        let instance = self.get(id)?;
        let dir = self.paths.instance_dir(id);
        let recycle = self.paths.root.join("recycle");
        fs::create_dir_all(&recycle)?;
        let target = recycle.join(id);
        if target.exists() {
            return Err(AppError::new(
                "instance_delete",
                crate::core::i18n::pick("回收站中已有同名世界，请先恢复它。", "The recycle bin already holds this world; restore it first."),
            ));
        }
        fs::rename(&dir, &target).map_err(|err| {
            AppError::detail(
                "instance_delete",
                crate::core::i18n::pick("移入回收站失败，世界没有删除。", "Couldn't move it to the recycle bin; the world wasn't removed."),
                err.to_string(),
            )
        })?;
        self.instances
            .write()
            .map_err(|_| AppError::new("instances", crate::core::i18n::pick("世界存储被锁定。", "World storage is locked.")))?
            .retain(|item| item.id != instance.id);
        Ok(())
    }

    pub fn recycled(&self) -> Vec<Instance> {
        let Ok(entries) = fs::read_dir(self.paths.root.join("recycle")) else {
            return Vec::new();
        };
        let mut result: Vec<Instance> = entries
            .flatten()
            .filter_map(|entry| {
                if !entry.file_type().ok()?.is_dir() {
                    return None;
                }
                let instance: Instance =
                    serde_json::from_slice(&fs::read(entry.path().join("instance.json")).ok()?)
                        .ok()?;
                (AppPaths::validate_instance_id(&instance.id).is_ok()
                    && entry.file_name() == instance.id.as_str())
                .then_some(instance)
            })
            .collect();
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        result
    }

    pub fn restore(&self, id: &str) -> AppResult<()> {
        AppPaths::validate_instance_id(id)?;
        let instance = self
            .recycled()
            .into_iter()
            .find(|i| i.id == id)
            .ok_or_else(|| AppError::new("instance_restore", crate::core::i18n::pick("回收站中没有这个世界。", "This world isn't in the recycle bin.")))?;
        if self.paths.instance_dir(id).exists() {
            return Err(AppError::new("instance_restore", crate::core::i18n::pick("目标世界已存在。", "The target world already exists.")));
        }
        fs::rename(
            self.paths.root.join("recycle").join(id),
            self.paths.instance_dir(id),
        )?;
        let mut guard = self
            .instances
            .write()
            .map_err(|_| AppError::new("instances", crate::core::i18n::pick("世界存储被锁定。", "World storage is locked.")))?;
        guard.push(instance);
        guard.sort_by_key(|item| item.created_at);
        Ok(())
    }

    pub fn configure(
        &self,
        id: &str,
        name: &str,
        memory_mb: Option<u32>,
        java_path: Option<String>,
        loader: &str,
    ) -> AppResult<()> {
        let mut instance = self.get(id)?;
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 40 {
            return Err(AppError::new("instance_name", crate::core::i18n::pick("世界名称需要 1–40 个字符。", "World names need 1–40 characters.")));
        }
        if memory_mb.is_some_and(|memory| !(512..=32768).contains(&memory)) {
            return Err(AppError::new(
                "memory",
                crate::core::i18n::pick("内存应在 512–32768 MB 之间，留空则继承全局设置。", "Memory must be between 512 and 32768 MB; leave it empty to use the global setting."),
            ));
        }
        if !LOADERS.contains(&loader) {
            return Err(AppError::new("loader", crate::core::i18n::pick("暂不支持此加载器。", "This loader isn't supported.")));
        }
        if instance.loader != loader {
            instance.loader = loader.to_string();
            instance.loader_version = None;
            instance.installed = false;
        }
        instance.name = name.to_string();
        instance.memory_mb = memory_mb;
        instance.java_path = java_path
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        self.update(&instance)
    }

    /// Launch options and the account this world plays with.
    pub fn configure_launch(&self, id: &str, launch: LaunchConfig, account_id: Option<String>) -> AppResult<()> {
        launch.validate()?;
        let mut instance = self.get(id)?;
        instance.launch = launch;
        instance.account_id = account_id.filter(|value| !value.is_empty());
        self.update(&instance)
    }

    /// Stars the world and puts it in a group; an empty group means none.
    pub fn organize(&self, id: &str, favorite: bool, group: Option<String>) -> AppResult<()> {
        let group = group.map(|g| g.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|g| !g.is_empty());
        if group.as_ref().is_some_and(|g| g.chars().count() > 24) {
            return Err(AppError::new("instance_group", crate::core::i18n::pick("分组名最多 24 个字。", "Group names can be at most 24 characters.")));
        }
        let mut instance = self.get(id)?;
        instance.favorite = favorite;
        instance.group = group;
        self.update(&instance)
    }

    /// Moves the world to another Minecraft version. The loader stays, its
    /// version is picked again, and the world needs installing again.
    pub fn change_version(&self, id: &str, version_id: &str, version_type: &str) -> AppResult<()> {
        if version_id.is_empty()
            || version_id.len() > 64
            || !version_id.chars().all(|c| c.is_ascii_alphanumeric() || "._- +".contains(c))
        {
            return Err(AppError::new("version_id", crate::core::i18n::pick("版本号无效。", "Invalid version number.")));
        }
        let mut instance = self.get(id)?;
        if instance.version_id == version_id {
            return Ok(());
        }
        instance.version_id = version_id.to_string();
        instance.version_type = version_type.to_string();
        instance.loader_version = None;
        instance.installed = false;
        // The merged loader profile belongs to the old version.
        let _ = fs::remove_file(self.paths.instance_dir(id).join("launch-version.json"));
        self.update(&instance)
    }

    pub fn configure_loader_version(&self, id: &str, version: Option<String>) -> AppResult<()> {
        let mut instance = self.get(id)?;
        let version = version.filter(|v| !v.trim().is_empty());
        if version
            .as_ref()
            .is_some_and(|v| !crate::core::fabric::valid_version(v))
        {
            return Err(AppError::new("loader_version", crate::core::i18n::pick("加载器版本号无效。", "Invalid loader version.")));
        }
        let version = if instance.loader == "vanilla" {
            None
        } else {
            version
        };
        if instance.loader_version != version {
            instance.loader_version = version;
            instance.installed = false;
            self.update(&instance)?;
        }
        Ok(())
    }

    pub fn duplicate(&self, id: &str) -> AppResult<Instance> {
        let original = self.get(id)?;
        let new_id = Uuid::new_v4().to_string();
        let staging = self.paths.instances.join(format!(".copy-{new_id}"));
        let result = (|| -> AppResult<Instance> {
            copy_tree(
                &self.paths.instance_game_dir(id),
                &staging.join("minecraft"),
            )?;
            let mut copy = original;
            copy.id = new_id.clone();
            copy.name = crate::tr!("{} 副本", "{} copy", copy.name.chars().take(37).collect::<String>());
            copy.created_at = now_millis();
            copy.last_launched_at = None;
            copy.play_seconds = 0;
            for name in ["launch-version.json", "modpack.json", ".overrides-applied"] {
                let profile = self.paths.instance_dir(id).join(name);
                if profile.exists() {
                    fs::copy(profile, staging.join(name))?;
                }
            }
            let overrides = self.paths.instance_dir(id).join("modpack-overrides");
            if overrides.is_dir() {
                copy_tree(&overrides, &staging.join("modpack-overrides"))?;
            }
            atomic_write(
                &staging.join("instance.json"),
                &serde_json::to_vec_pretty(&copy)?,
            )?;
            fs::rename(&staging, self.paths.instance_dir(&new_id))?;
            self.instances
                .write()
                .map_err(|_| AppError::new("instances", crate::core::i18n::pick("世界存储被锁定。", "World storage is locked.")))?
                .push(copy.clone());
            Ok(copy)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(staging);
        }
        result
    }

    pub fn mark_installed(&self, id: &str, installed: bool) -> AppResult<()> {
        let mut instance = self.get(id)?;
        instance.installed = installed;
        self.update(&instance)
    }

    pub fn mark_launched(&self, id: &str) -> AppResult<()> {
        let mut instance = self.get(id)?;
        instance.last_launched_at = Some(now_millis());
        self.update(&instance)
    }

    pub fn add_play_time(&self, id: &str, seconds: u64) -> AppResult<()> {
        let mut instance = self.get(id)?;
        instance.play_seconds = instance.play_seconds.saturating_add(seconds);
        self.update(&instance)
    }

    fn save(&self, instance: &Instance) -> AppResult<()> {
        let file: PathBuf = self.paths.instance_file(&instance.id);
        let encoded = serde_json::to_vec_pretty(instance)?;
        atomic_write(&file, &encoded)
    }
}

fn copy_tree(source: &std::path::Path, dest: &std::path::Path) -> AppResult<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            return Err(AppError::new(
                "instance_copy",
                crate::core::i18n::pick("世界里有符号链接，请先把链接换成真实文件。", "The world contains symbolic links; replace them with real files first."),
            ));
        }
        let target = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_launch_options_build_on_the_global_ones() {
        let global = LaunchConfig {
            jvm_args: "-XX:+UseG1GC".into(),
            window_width: Some(1280),
            window_height: Some(720),
            env: "A=1\nB=2".into(),
            wrapper: "gamemoderun".into(),
            ..LaunchConfig::default()
        };
        let own = LaunchConfig { jvm_args: "-Xss4M".into(), env: "B=3".into(), ..LaunchConfig::default() };
        let merged = own.over(&global);
        assert_eq!(merged.jvm_args, "-XX:+UseG1GC -Xss4M");
        assert_eq!((merged.window_width, merged.window_height), (Some(1280), Some(720)));
        assert_eq!(merged.wrapper, "gamemoderun");
        // The world's value comes last, so it wins when set in order.
        assert_eq!(merged.env_pairs().last().unwrap(), &("B".to_string(), "3".to_string()));
        assert!(LaunchConfig { window_width: Some(100), window_height: Some(100), ..LaunchConfig::default() }.validate().is_err());
        assert!(LaunchConfig { env: "NOVALUE".into(), ..LaunchConfig::default() }.validate().is_err());
    }

    #[test]
    fn changing_the_version_resets_the_install() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let store = InstanceStore::load(paths.clone()).unwrap();
        let instance = store.create("w", "1.20.1", "release").unwrap();
        store.mark_installed(&instance.id, true).unwrap();
        fs::write(paths.instance_dir(&instance.id).join("launch-version.json"), b"{}").unwrap();
        store.change_version(&instance.id, "1.21.4", "release").unwrap();
        let changed = store.get(&instance.id).unwrap();
        assert_eq!(changed.version_id, "1.21.4");
        assert!(!changed.installed);
        assert!(!paths.instance_dir(&instance.id).join("launch-version.json").exists());
        assert!(store.change_version(&instance.id, "../x", "release").is_err());
    }

    #[test]
    fn play_time_accumulates_and_old_files_default_to_zero() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let store = InstanceStore::load(paths).unwrap();
        let instance = store.create("世界", "1.21", "release").unwrap();
        store.add_play_time(&instance.id, 90).unwrap();
        store.add_play_time(&instance.id, 30).unwrap();
        assert_eq!(store.get(&instance.id).unwrap().play_seconds, 120);

        let old: Instance = serde_json::from_str(r#"{"id":"x","name":"x","versionId":"1.20"}"#).unwrap();
        assert_eq!(old.play_seconds, 0);
    }
    #[test]
    fn legacy_instance_migrates_and_delete_restore_preserves_world() {
        let legacy: Instance =
            serde_json::from_str(r#"{"id":"old","name":"旧实例","versionId":"1.21"}"#).unwrap();
        assert_eq!(legacy.memory_mb, None);
        assert_eq!(legacy.loader, "vanilla");
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(root.path()).unwrap();
        let store = InstanceStore::load(paths.clone()).unwrap();
        let item = store.create("生存", "1.21", "release").unwrap();
        let world = paths.instance_game_dir(&item.id).join("saves/world");
        fs::create_dir_all(&world).unwrap();
        fs::write(world.join("level.dat"), b"precious world").unwrap();
        let copy = store.duplicate(&item.id).unwrap();
        assert_eq!(
            fs::read(
                paths
                    .instance_game_dir(&copy.id)
                    .join("saves/world/level.dat")
            )
            .unwrap(),
            b"precious world"
        );
        store.remove(&item.id).unwrap();
        assert!(store.get(&item.id).is_err());
        store.restore(&item.id).unwrap();
        assert_eq!(
            fs::read(world.join("level.dat")).unwrap(),
            b"precious world"
        );
    }
}
