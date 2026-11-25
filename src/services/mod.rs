mod accounts;
mod content;
mod install;
mod launch;
mod projects;
mod remote;
#[cfg(test)]
mod tests;

pub use remote::{DeployOptions, RemoteInfo, RemoteStatus};

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_channel::Sender;
use tokio_util::sync::CancellationToken;

use crate::core::accounts::Account;
use crate::core::auth::MinecraftSession;
use crate::core::crash::Diagnosis;
use crate::core::error::{AppError, AppResult, ErrorPayload};
use crate::core::instances::{Instance, InstanceStore};
use crate::core::java::detect::JavaInstallation;
use crate::core::manage::{Cleanup, ItemKind};
use crate::core::mojang::manifest::VersionList;
use crate::core::mojang::version::VersionJson;
use crate::core::paths::AppPaths;
use crate::core::settings::{Settings, SettingsStore};

pub const DEFAULT_MEMORY_MB: u32 = 4096;
pub const DEFAULT_CONCURRENCY: usize = 8;
/// How long a stopped game gets to save and quit before it is ended.
const STOP_GRACE: Duration = Duration::from_secs(30);
/// A remembered Minecraft token is reused only while at least this much of
/// it is left, so it doesn't run out during the evening's game.
const SESSION_MARGIN: Duration = Duration::from_secs(8 * 60 * 60);

/// Where a launch goes after the game starts.
#[derive(Debug, Clone, Copy)]
enum LaunchTarget<'a> {
    Title,
    World(&'a str),
    Server(&'a str),
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub platform: String,
    pub arch: String,
    pub curseforge_configured: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSnapshot {
    pub settings: Settings,
    pub accounts: Vec<Account>,
    pub instances: Vec<Instance>,
    pub app_info: AppInfo,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgressData {
    pub task_id: String,
    pub instance_id: String,
    pub stage: String,
    pub done: u64,
    pub total: u64,
    pub bytes: u64,
    pub total_bytes: u64,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStartData {
    pub login_id: String,
    pub mode: String,
    pub authorize_url: Option<String>,
    pub user_code: Option<String>,
    pub verification_uri: Option<String>,
    pub expires_in: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstancePaths {
    pub game_dir: String,
    pub log_dir: String,
}

/// Crash findings plus a bounded excerpt the UI can copy into a bug report.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashAnalysis {
    pub diagnoses: Vec<Diagnosis>,
    pub excerpt: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Idle,
    /// Only games are running; the launcher can wait for them out of sight.
    Playing,
    /// Downloads, installs or file work that quitting would interrupt.
    Working,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LoginMode {
    Browser,
    Device,
}

/// Serialized for the web UI as `{ type, data }` with camelCase names.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UiEvent {
    OperationComplete(Result<String, ErrorPayload>),
    /// A world's server being uploaded: the stage, and bytes or seconds.
    RemoteProgress {
        instance_id: String,
        stage: String,
        done: u64,
        total: u64,
    },
    /// A shortcut asked to start this world.
    LaunchRequest { instance_id: String },
    /// The window was closed while work was still running or edits were
    /// unsaved; the page asks about the edits, then whether to quit anyway.
    QuitRequested,
    /// Quitting once work finishes was scheduled or cancelled.
    QuitScheduled { scheduled: bool },
    Versions(Result<VersionList, ErrorPayload>),
    JavaRuntimes(Vec<JavaInstallation>),
    LoaderVersions {
        key: String,
        result: Result<Vec<crate::core::loaders::LoaderVersion>, ErrorPayload>,
    },
    ModSearch {
        key: String,
        result: Result<crate::core::modrinth::SearchResults, ErrorPayload>,
    },
    ModVersions {
        key: String,
        result: Result<Vec<crate::core::modrinth::ModVersion>, ErrorPayload>,
    },
    StorageChanged(Result<PathBuf, ErrorPayload>),
    InstallProgress(InstallProgressData),
    InstallComplete {
        task_id: String,
        instance_id: String,
        version_id: String,
    },
    InstallError {
        task_id: String,
        instance_id: String,
        error: ErrorPayload,
    },
    LaunchStarted {
        instance_id: String,
        pid: u32,
    },
    LaunchLog {
        instance_id: String,
        line: String,
        stream: String,
    },
    LaunchExit {
        instance_id: String,
        code: Option<i32>,
        success: bool,
    },
    LaunchError {
        instance_id: String,
        error: ErrorPayload,
    },
    AccountsChanged(Vec<Account>),
    LoginStarted(LoginStartData),
    LoginComplete {
        login_id: String,
        account: Account,
    },
    LoginError {
        login_id: String,
        error: ErrorPayload,
    },
}

#[derive(Debug, Clone)]
pub enum FileOperation {
    Duplicate,
    Backup,
    RestoreBackup(PathBuf),
    RestoreDeleted,
    AddMods(Vec<PathBuf>),
    ToggleMod(String),
    /// Several mods on or off at once.
    SetMods { names: Vec<String>, enabled: bool },
    /// Several mods to the recycle bin at once.
    RecycleMods(Vec<String>),
    ImportPack(PathBuf),
    /// Downloads a version of an online modpack, then imports it.
    InstallModpack(String),
    /// Moves a mod, pack, save or datapack to the recycle bin.
    Recycle { kind: ItemKind, name: String, save: Option<String> },
    AddPacks { kind: ItemKind, paths: Vec<PathBuf> },
    AddDatapacks { save: String, paths: Vec<PathBuf> },
    RenameSave { folder: String, name: String },
    BackupSave(String),
    RestoreSave { folder: String, backup: PathBuf },
    ImportSave(PathBuf),
    ExportSave { folder: String, destination: PathBuf },
    /// Empties the recycle bin: one recycled world, or all of it.
    PurgeRecycle(Option<String>),
    Clean(Cleanup),
    /// A newer version of the pack this world came from.
    UpdatePack(PathBuf),
    /// A newer online version (Modrinth or `cf:` id) of the pack this world
    /// was installed from.
    UpdateModpackOnline(String),
    /// The world as a Modrinth pack (.mrpack).
    ExportPack { destination: PathBuf, options: crate::core::modpacks::ExportOptions },
    /// The world as a dedicated server in an empty folder.
    ExportServer { destination: PathBuf, options: crate::core::server_pack::ServerOptions },
    /// Builds the server and runs it on the world's own host.
    DeployServer(DeployOptions),
    /// Fetches the mods a server needs that the world lacks.
    FixServerMods { address: String },
    /// A game from another launcher (a version folder or Prism instance).
    ImportGame(PathBuf),
    /// Another game version, after backing up every save: opening a save in
    /// an older version can break it.
    ChangeVersion { version_id: String, version_type: String },
    /// Takes back the latest mod install or update.
    UndoModUpdate,
}

pub struct AppServices {
    pub paths: AppPaths,
    pub settings: SettingsStore,
    pub accounts: crate::core::accounts::AccountStore,
    pub instances: InstanceStore,
    http: Mutex<Option<reqwest::Client>>,
    runtime: Option<tokio::runtime::Runtime>,
    events: Sender<UiEvent>,
    tasks: Mutex<HashMap<String, CancellationToken>>,
    logins: Mutex<HashMap<String, CancellationToken>>,
    launches: Mutex<HashMap<String, u32>>,
    /// Minecraft sessions of Microsoft accounts and when to stop reusing
    /// them. Kept in memory only: the tokens never touch the disk.
    sessions: Mutex<HashMap<String, (MinecraftSession, std::time::Instant)>>,
    starting: Mutex<HashSet<String>>,
    installing: Mutex<HashMap<String, String>>,
    operations: Mutex<HashSet<String>>,
    /// Passwords of worlds' servers for this session, by world id.
    remote_secrets: Mutex<HashMap<String, String>>,
}

impl AppServices {
    pub fn new(paths: AppPaths, events: Sender<UiEvent>) -> AppResult<Arc<Self>> {
        let settings = SettingsStore::load(paths.settings_file())?;
        let accounts = crate::core::accounts::AccountStore::load(paths.accounts_file())?;
        let instances = InstanceStore::load(paths.clone())?;
        if let Err(error) = crate::core::net::configure(&settings.get()) {
            tracing::warn!(error = %error, "the saved proxy is invalid; using the system's");
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("islet-worker")
            .enable_all()
            .build()
            .map_err(|err| {
                AppError::detail("runtime", crate::core::i18n::pick("启动后台任务运行时失败。", "Couldn't start the background task runtime."), err.to_string())
            })?;

        Ok(Arc::new(Self {
            paths,
            settings,
            accounts,
            instances,
            http: Mutex::new(None),
            runtime: Some(runtime),
            events,
            tasks: Mutex::new(HashMap::new()),
            logins: Mutex::new(HashMap::new()),
            launches: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            starting: Mutex::new(HashSet::new()),
            installing: Mutex::new(HashMap::new()),
            operations: Mutex::new(HashSet::new()),
            remote_secrets: Mutex::new(HashMap::new()),
        }))
    }

    pub fn send(&self, event: UiEvent) {
        let _ = self.events.try_send(event);
    }

    /// Build the TLS client on the first network operation. Loading the
    /// system root store takes ~80 ms on macOS, and most launches never touch
    /// the network before the user starts a download or signs in.
    /// The shared HTTP client, for commands that talk to the network directly.
    pub fn http_client(&self) -> AppResult<reqwest::Client> {
        self.http()
    }

    fn http(&self) -> AppResult<reqwest::Client> {
        let mut guard = lock(&self.http);
        if let Some(client) = guard.as_ref() {
            return Ok(client.clone());
        }
        // No overall timeout: a large file on a slow line can take longer
        // than any fixed limit. A stalled connection fails on the read
        // timeout instead, and the download resumes from where it stopped.
        let client = crate::core::net::builder()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(60))
            .http2_adaptive_window(true)
            .build()
            .map_err(|err| AppError::detail("http", crate::core::i18n::pick("初始化网络客户端失败。", "Couldn't set up the network client."), err.to_string()))?;
        *guard = Some(client.clone());
        Ok(client)
    }

    pub fn running_instance_ids(&self) -> Vec<String> { lock(&self.launches).keys().cloned().collect() }

    /// What quitting now would cut short.
    pub fn activity(&self) -> Activity {
        if !lock(&self.installing).is_empty() || !lock(&self.operations).is_empty() || !lock(&self.starting).is_empty() {
            Activity::Working
        } else if !lock(&self.launches).is_empty() {
            Activity::Playing
        } else {
            Activity::Idle
        }
    }

    pub fn ensure_idle(&self, id: &str) -> AppResult<()> {
        if lock(&self.launches).contains_key(id)
            || lock(&self.starting).contains(id)
            || lock(&self.installing).contains_key(id)
            || !lock(&self.operations).is_empty()
        {
            return Err(AppError::new(
                "instance_busy",
                crate::core::i18n::pick("请先停止游戏，并等待下载或文件操作完成。", "Stop the game and wait for downloads or file work to finish first."),
            ));
        }
        Ok(())
    }

    pub fn snapshot(&self) -> LocalSnapshot {
        LocalSnapshot {
            settings: self.settings.get(),
            accounts: self.accounts.list(),
            instances: self.instances.list(),
            app_info: AppInfo {
                version: env!("CARGO_PKG_VERSION").to_string(),
                data_dir: self.paths.root.display().to_string(),
                platform: std::env::consts::OS.to_string(),
                arch: std::env::consts::ARCH.to_string(),
                curseforge_configured: crate::core::settings::Settings::has_builtin_curseforge(),
            },
        }
    }

    fn ensure_storage_idle(&self) -> AppResult<()> {
        if lock(&self.operations).contains("storage-migration") {
            return Err(AppError::new("storage", crate::core::i18n::pick("正在迁移数据，请等待完成。", "Data is being moved; wait for it to finish.")));
        }
        Ok(())
    }

    pub fn update_settings(&self, settings: Settings) -> AppResult<Settings> {
        self.ensure_storage_idle()?;
        settings.launch.validate()?;
        crate::core::net::check(&settings)?;
        let previous = self.settings.get();
        let saved = self.settings.update(|current| *current = settings)?;
        // Only rebuild the pooled HTTP client (and reload OS TLS roots) when
        // the proxy route actually changed.
        if previous.proxy_mode != saved.proxy_mode || previous.proxy_url != saved.proxy_url {
            crate::core::net::configure(&saved)?;
            *lock(&self.http) = None;
        }
        Ok(saved)
    }

    fn ensure_all_idle(&self) -> AppResult<()> {
        if !lock(&self.launches).is_empty()
            || !lock(&self.starting).is_empty()
            || !lock(&self.installing).is_empty()
            || !lock(&self.operations).is_empty()
            || !lock(&self.logins).is_empty()
        {
            return Err(AppError::new(
                "busy",
                crate::core::i18n::pick("请等待任务完成，并先关闭正在运行的游戏。", "Wait for tasks to finish and close any running game first."),
            ));
        }
        Ok(())
    }

    pub fn reopen(&self, root: PathBuf) -> AppResult<Arc<Self>> {
        Self::new(AppPaths::from_root(root)?, self.events.clone())
    }

    fn spawn_task<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        if let Some(runtime) = &self.runtime {
            runtime.spawn(future);
        }
    }
}

impl Drop for AppServices {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Bytes for a message: "1.2 GB", "340 MB".
pub fn human_bytes(bytes: u64) -> String {
    let mb = bytes as f64 / 1024.0 / 1024.0;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

pub fn instance_mod_count(paths: &AppPaths, instance_id: &str) -> usize {
    let Ok(entries) = std::fs::read_dir(paths.instance_game_dir(instance_id).join("mods")) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && entry
                    .file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with(".jar")
        })
        .count()
}

fn load_version(paths: &AppPaths, instance: &Instance) -> AppResult<VersionJson> {
    let profile = paths.instance_dir(&instance.id).join("launch-version.json");
    let path = if profile.is_file() {
        profile
    } else {
        if instance.loader != "vanilla" {
            return Err(AppError::new(
                "version_missing",
                crate::core::i18n::pick("加载器配置丢失，请使用校验并修复。", "The loader profile is missing; use Verify and repair."),
            ));
        }
        paths.version_json(&instance.version_id)
    };
    let bytes = std::fs::read(&path)
        .map_err(|_| AppError::new("version_missing", crate::core::i18n::pick("版本文件不存在，请先下载这个世界。", "The version file is missing; install this world first.")))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| AppError::detail("version_json", crate::core::i18n::pick("读取版本配置失败。", "Couldn't read the version profile."), err.to_string()))
}
