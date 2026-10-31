mod remote;
pub use remote::{DeployOptions, RemoteInfo, RemoteStatus};

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_channel::Sender;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::core::accounts::{Account, AccountProvider};
use crate::core::content::ContentKind;
use crate::core::auth::{MinecraftSession, microsoft, offline, yggdrasil};
use crate::core::crash::{self, Diagnosis};
use crate::core::download::{self, DownloadProgress};
use crate::core::error::{AppError, AppResult, ErrorPayload};
use crate::core::instances::{Instance, InstanceStore};
use crate::core::java::detect::{self, JavaInstallation};
use crate::core::java::runtime;
use crate::core::keychain;
use crate::core::launch;
use crate::core::manage::{self, Cleanup, ItemKind};
use crate::core::mojang::install as mojang_install;
use crate::core::mojang::manifest::{self, VersionList};
use crate::core::mojang::version::{VersionJson, java_major};
use crate::core::options;
use crate::core::paths::AppPaths;
use crate::core::servers;
use crate::core::platform;
use crate::core::settings::{DownloadSource, Settings, SettingsStore};

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

    pub fn configure_instance(
        &self,
        id: &str,
        name: &str,
        memory: Option<u32>,
        java: Option<String>,
        loader: &str,
    ) -> AppResult<()> {
        self.ensure_idle(id)?;
        self.instances.configure(id, name, memory, java, loader)
    }

    pub fn configure_launch(
        &self,
        id: &str,
        launch: crate::core::instances::LaunchConfig,
        account_id: Option<String>,
    ) -> AppResult<()> {
        if let Some(account) = account_id.as_deref().filter(|a| !a.is_empty()) {
            if self.accounts.get(account).is_none() {
                return Err(AppError::new("account", crate::core::i18n::pick("这个账号不存在。", "This account doesn't exist.")));
            }
        }
        self.instances.configure_launch(id, launch, account_id)
    }

    pub fn organize_instance(&self, id: &str, favorite: bool, group: Option<String>) -> AppResult<()> {
        self.instances.organize(id, favorite, group)
    }

    pub fn change_version(&self, id: &str, version_id: &str, version_type: &str) -> AppResult<()> {
        self.ensure_idle(id)?;
        self.instances.change_version(id, version_id, version_type)
    }

    pub fn file_operation(self: &Arc<Self>, id: &str, operation: FileOperation) -> AppResult<()> {
        self.ensure_idle(id)?;
        // Shared files: nothing may be installing or running meanwhile.
        if matches!(operation, FileOperation::Clean(_) | FileOperation::PurgeRecycle(None)) {
            self.ensure_all_idle()?;
        }
        lock(&self.operations).insert(id.to_string());
        let this = self.clone();
        let id = id.to_string();
        let Some(runtime) = &self.runtime else {
            lock(&self.operations).remove(&id);
            return Err(AppError::new("runtime", crate::core::i18n::pick("后台任务不可用。", "Background tasks are unavailable.")));
        };
        runtime.spawn_blocking(move || {
            let result = (|| -> AppResult<String> {
                match operation {
                    FileOperation::Duplicate => {
                        this.instances.duplicate(&id)?;
                        Ok(crate::core::i18n::pick("世界已复制，包含存档和模组。", "World duplicated, saves and mods included.").into())
                    }
                    FileOperation::Backup => {
                        let path =
                            crate::core::content::backup(&this.paths, &this.instances.get(&id)?)?;
                        Ok(crate::tr!("备份已保存：{}", "Backup saved: {}", path.display()))
                    }
                    FileOperation::RestoreBackup(path) => {
                        let saved = crate::core::content::restore_backup(
                            &this.paths,
                            &this.instances.get(&id)?,
                            &path,
                        )?;
                        Ok(crate::tr!("已恢复。恢复前的数据已备份到：{}", "Restored. The data from before was backed up to: {}", saved.display()))
                    }
                    FileOperation::RestoreDeleted => {
                        this.instances.restore(&id)?;
                        Ok(crate::core::i18n::pick("世界已从回收站恢复。", "World restored from the recycle bin.").into())
                    }
                    FileOperation::AddMods(paths) => {
                        crate::core::content::add_mods(&this.paths, &id, &paths)?;
                        Ok(crate::core::i18n::pick("模组已添加。", "Mods added.").into())
                    }
                    FileOperation::ToggleMod(name) => {
                        crate::core::content::toggle_mod(&this.paths, &id, &name)?;
                        Ok(crate::core::i18n::pick("模组状态已更新。", "Mod updated.").into())
                    }
                    FileOperation::SetMods { names, enabled } => {
                        let count = crate::core::content::set_mods_enabled(&this.paths, &id, &names, enabled)?;
                        Ok(if enabled {
                            crate::tr!("已启用 {count} 个模组。", "Enabled {count} mods.")
                        } else {
                            crate::tr!("已禁用 {count} 个模组。", "Disabled {count} mods.")
                        })
                    }
                    FileOperation::RecycleMods(names) => {
                        for name in &names {
                            manage::recycle_item(&this.paths, &id, ItemKind::Mod, name, None)?;
                        }
                        Ok(crate::tr!("{} 个模组已移到回收站。", "{} mods moved to the recycle bin.", names.len()))
                    }
                    FileOperation::ImportPack(path) => {
                        this.import_pack(&path)?;
                        Ok(crate::core::i18n::pick("整合包已导入，点击安装并启动即可准备游戏。", "Modpack imported; press Install and play to get it ready.").into())
                    }
                    FileOperation::InstallModpack(version) => {
                        let key = this.settings.get().curseforge_key;
                        let (file, source) = tokio::runtime::Handle::current().block_on(crate::core::modpacks::download_online(&this.http()?, &key, &this.paths, &version))?;
                        let imported = this.import_pack(&file);
                        let _ = std::fs::remove_file(&file);
                        // Without the record the world still plays; it just can't update online.
                        if let Err(error) = crate::core::modpacks::record_source(&this.paths, &imported?, &source) {
                            tracing::warn!(%error, "couldn't record the modpack source");
                        }
                        Ok(crate::core::i18n::pick("整合包已下载并导入，点击安装并启动即可准备游戏。", "Modpack downloaded and imported; press Install and play to get it ready.").into())
                    }
                    FileOperation::ExportPack { destination, options } => {
                        let instance = this.instances.get(&id)?;
                        let candidates = crate::core::modpacks::export_candidates(&this.paths, &id)?;
                        let http = this.http()?;
                        let runtime = tokio::runtime::Handle::current();
                        // Without the lookup every file is bundled; the pack still works.
                        let (linked, bundled, site) = if options.curseforge() {
                            let key = this.settings.get().curseforge_key;
                            let prints: Vec<u32> = candidates.iter().map(|c| c.fingerprint).collect();
                            let hosted = if key.trim().is_empty() {
                                HashMap::new()
                            } else {
                                runtime.block_on(crate::core::curseforge::files_by_fingerprint(&http, &key, &prints)).unwrap_or_else(|error| {
                                    tracing::warn!(%error, "curseforge lookup for export failed");
                                    HashMap::new()
                                })
                            };
                            let (linked, bundled) = crate::core::modpacks::export(&this.paths, &instance, &candidates, crate::core::modpacks::Hosted::CurseForge(&hosted), &options, &destination)?;
                            (linked, bundled, "CurseForge")
                        } else {
                            let hashes: Vec<String> = candidates.iter().map(|c| c.sha1.clone()).collect();
                            let hosted = runtime.block_on(crate::core::modrinth::files_by_hash(&http, &hashes)).unwrap_or_else(|error| {
                                tracing::warn!(%error, "modrinth lookup for export failed");
                                HashMap::new()
                            });
                            let (linked, bundled) = crate::core::modpacks::export(&this.paths, &instance, &candidates, crate::core::modpacks::Hosted::Modrinth(&hosted), &options, &destination)?;
                            (linked, bundled, "Modrinth")
                        };
                        Ok(crate::tr!("已导出 {}：{linked} 个文件从 {site} 下载{}。", "Exported {}: {linked} files download from {site}{}.",
                            destination.file_name().unwrap_or_default().to_string_lossy(),
                            if bundled > 0 { crate::tr!("，{bundled} 个直接打包", ", {bundled} are packed in") } else { String::new() }
                        ))
                    }
                    FileOperation::ExportServer { destination, options } => {
                        let (summary, pack) = this.export_server(&id, &destination, options.clone(), &CancellationToken::new())?;
                        let mut message = crate::tr!("服务端已生成，带 {} 个模组：运行文件夹里的 start.bat（Windows）或 start.sh 启动。", "Server ready with {} mods: run start.bat (Windows) or start.sh in its folder.", summary.mods);
                        if !summary.client_only.is_empty() {
                            message += &crate::tr!("{} 个只在客户端运行的模组没有放进去。", " {} client-only mods were left out.", summary.client_only.len());
                        }
                        if let Some(pack) = pack {
                            message += &crate::tr!("把「{}」发给朋友，导入后就能进服。", " Send \"{}\" to friends; once imported they can join.", pack.file_name().unwrap_or_default().to_string_lossy());
                        }
                        if !options.accept_eula {
                            message += crate::core::i18n::pick("首次启动后要在 eula.txt 里同意 EULA。", " Agree to the EULA in eula.txt after the first start.");
                        }
                        Ok(message)
                    }
                    FileOperation::DeployServer(options) => this.deploy_server(&id, options, &CancellationToken::new()),
                    FileOperation::FixServerMods { address } => {
                        let (added, removed) = tokio::runtime::Handle::current().block_on(crate::core::server_mods::fix(&this.http()?, &this.paths, &id, &address, &CancellationToken::new()))?;
                        Ok(crate::tr!("已补齐 {added} 个模组{}，现在可以进服了。", "Added {added} mods{}; you can join now.",
                            if removed > 0 { crate::tr!("，{removed} 个旧版本移到了回收站", ", {removed} old versions went to the recycle bin") } else { String::new() }))
                    }
                    FileOperation::UpdatePack(path) => this.update_pack(&id, &path),
                    FileOperation::UpdateModpackOnline(version) => {
                        let key = this.settings.get().curseforge_key;
                        let (file, source) = tokio::runtime::Handle::current().block_on(crate::core::modpacks::download_online(&this.http()?, &key, &this.paths, &version))?;
                        let updated = this.update_pack(&id, &file);
                        let _ = std::fs::remove_file(&file);
                        let message = updated?;
                        crate::core::modpacks::record_source(&this.paths, &id, &source)?;
                        Ok(message)
                    }
                    FileOperation::Recycle { kind, name, save } => {
                        manage::recycle_item(&this.paths, &id, kind, &name, save.as_deref())?;
                        Ok(crate::tr!("「{name}」已移到回收站。", "\"{name}\" moved to the recycle bin."))
                    }
                    FileOperation::AddPacks { kind, paths } => {
                        let count = manage::add_packs(&this.paths, &id, kind, &paths)?;
                        Ok(crate::tr!("已添加 {count} 个{}。", "Added {count} {}.", if kind == ItemKind::Shader { crate::core::i18n::pick("光影", "shader packs") } else { crate::core::i18n::pick("资源包", "resource packs") }))
                    }
                    FileOperation::AddDatapacks { save, paths } => {
                        let count = manage::add_datapacks(&this.paths, &id, &save, &paths)?;
                        Ok(crate::tr!("已添加 {count} 个数据包，下次进入存档时生效。", "Added {count} datapacks; they apply next time you open the save."))
                    }
                    FileOperation::RenameSave { folder, name } => {
                        manage::rename_save(&this.paths, &id, &folder, &name)?;
                        Ok(crate::tr!("存档已改名为「{}」。", "Save renamed to \"{}\".", name.trim()))
                    }
                    FileOperation::ChangeVersion { version_id, version_type } => {
                        let saves = crate::core::content::worlds(&this.paths, &id)?;
                        for save in &saves {
                            manage::backup_save(&this.paths, &id, &save.folder)?;
                        }
                        this.instances.change_version(&id, &version_id, &version_type)?;
                        Ok(if saves.is_empty() {
                            crate::tr!("已换成 {}，开始游戏时会下载需要的文件。", "Switched to {}; the files it needs download when you play.", version_id)
                        } else {
                            crate::tr!(
                                "已换成 {}；换之前把 {} 个存档各备份了一份，在存档的「备份」里可以恢复。",
                                "Switched to {}. All {} saves were backed up first; restore one from its Backups.",
                                version_id,
                                saves.len()
                            )
                        })
                    }
                    FileOperation::UndoModUpdate => crate::core::modrinth::undo_last_change(&this.paths, &id),
                    FileOperation::BackupSave(folder) => {
                        let path = manage::backup_save(&this.paths, &id, &folder)?;
                        Ok(crate::tr!("存档已备份：{}", "Save backed up: {}", path.display()))
                    }
                    FileOperation::RestoreSave { folder, backup } => {
                        manage::restore_save(&this.paths, &id, &folder, &backup)?;
                        Ok(crate::core::i18n::pick("存档已恢复，恢复前的存档在回收站里。", "Save restored; the version from before is in the recycle bin.").into())
                    }
                    FileOperation::ImportSave(path) => {
                        let folder = manage::import_save(&this.paths, &id, &path)?;
                        Ok(crate::tr!("存档已导入为「{folder}」。", "Save imported as \"{folder}\"."))
                    }
                    FileOperation::ExportSave { folder, destination } => {
                        manage::export_save(&this.paths, &id, &folder, &destination)?;
                        Ok(crate::tr!("存档已导出：{}", "Save exported: {}", destination.display()))
                    }
                    FileOperation::PurgeRecycle(only) => {
                        manage::purge_recycle(&this.paths, only.as_deref())?;
                        Ok(if only.is_some() { crate::core::i18n::pick("已永久删除这个世界。", "World deleted for good.").into() } else { crate::core::i18n::pick("回收站已清空。", "Recycle bin emptied.").into() })
                    }
                    FileOperation::ImportGame(source) => {
                        let instance = crate::core::import::import(&this.paths, &this.instances, &source)?;
                        this.set_active_instance(Some(instance.id))?;
                        Ok(crate::tr!("已导入「{}」，开始游戏时会下载所需文件。", "Imported \"{}\"; the files it needs download when you play.", instance.name))
                    }
                    FileOperation::Clean(what) => {
                        let freed = manage::clean(&this.paths, &this.instances.list(), &this.instances.recycled(), &what)?;
                        Ok(crate::tr!("已释放 {}。", "Freed {}.", human_bytes(freed)))
                    }
                }
            })();
            lock(&this.operations).remove(&id);
            this.send(UiEvent::OperationComplete(
                result.map_err(|e| e.to_payload()),
            ));
        });
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
        let saved = self.settings.update(|current| *current = settings)?;
        // A changed proxy applies to the next request; running downloads
        // keep the client they started with.
        crate::core::net::configure(&saved)?;
        *lock(&self.http) = None;
        Ok(saved)
    }

    pub fn add_offline_account(&self, username: &str) -> AppResult<(Account, Settings)> {
        self.ensure_storage_idle()?;
        let account = offline::create_account(username)?;
        self.accounts.upsert(account.clone())?;
        let settings = self.settings.update(|settings| {
            settings.active_account_id = Some(account.id.clone());
        })?;
        Ok((account, settings))
    }

    pub fn remove_account(&self, id: &str) -> AppResult<(Vec<Account>, Settings)> {
        self.ensure_storage_idle()?;
        if let Some(account) = self.accounts.remove(id)? {
            // Third-party tokens are signed out on their server too.
            if account.provider == AccountProvider::Yggdrasil {
                let tokens = keychain::load_refresh_token(&account.id).ok().flatten().and_then(|t| serde_json::from_str::<yggdrasil::Tokens>(&t).ok());
                if let (Some(tokens), Some(api), Ok(http)) = (tokens, account.server.clone(), self.http()) {
                    self.spawn_task(async move { yggdrasil::invalidate(&http, &api, &tokens).await });
                }
            }
            lock(&self.sessions).remove(&account.id);
            if account.provider != AccountProvider::Offline {
                if let Err(err) = keychain::delete_refresh_token(&account.id) {
                    tracing::warn!(account = %account.id, error = %err, "failed to delete refresh token");
                }
            }
        }
        let remaining = self.accounts.list();
        let settings = self.settings.update(|settings| {
            if settings.active_account_id.as_deref() == Some(id) {
                settings.active_account_id = remaining.first().map(|account| account.id.clone());
            }
        })?;
        Ok((remaining, settings))
    }

    /// Signs in to a third-party Yggdrasil server. Returns the characters to
    /// pick from when the login has several and none was chosen.
    pub async fn login_yggdrasil(
        &self,
        address: &str,
        username: &str,
        password: &str,
        profile: Option<&str>,
    ) -> AppResult<Option<Vec<yggdrasil::GameProfile>>> {
        self.ensure_storage_idle()?;
        let http = self.http()?;
        let server = yggdrasil::resolve(&http, address).await?;
        match yggdrasil::login(&http, &server, username, password, profile).await? {
            yggdrasil::Login::Choose(profiles) => Ok(Some(profiles)),
            yggdrasil::Login::Done(account, tokens) => {
                keychain::save_refresh_token(&account.id, &serde_json::to_string(&tokens)?)?;
                let id = account.id.clone();
                self.accounts.upsert(*account)?;
                self.settings.update(|settings| settings.active_account_id = Some(id))?;
                self.send(UiEvent::AccountsChanged(self.accounts.list()));
                Ok(None)
            }
        }
    }

    /// Capes a Microsoft account owns (empty for other accounts).
    pub async fn account_capes(&self, id: &str) -> AppResult<Vec<microsoft::Cape>> {
        let account = self.accounts.get(id).ok_or_else(|| AppError::new("account", crate::core::i18n::pick("账号不存在。", "The account doesn't exist.")))?;
        if account.provider != AccountProvider::Microsoft {
            return Ok(Vec::new());
        }
        let session = self.session_for_account(&account).await?;
        Ok(microsoft::appearance(&self.http()?, &session.access_token).await?.2)
    }

    /// Uploads a skin (`Some(path)`) or goes back to the default (`None`).
    pub async fn change_skin(&self, id: &str, path: Option<PathBuf>, slim: bool) -> AppResult<()> {
        let account = self.accounts.get(id).ok_or_else(|| AppError::new("account", crate::core::i18n::pick("账号不存在。", "The account doesn't exist.")))?;
        let png = match path {
            Some(path) => {
                let bytes = tokio::fs::read(&path).await?;
                crate::core::auth::check_skin(&bytes)?;
                Some(bytes)
            }
            None => None,
        };
        let http = self.http()?;
        let session = self.session_for_account(&account).await?;
        let mut updated = self.accounts.get(id).unwrap_or(account);
        match updated.provider {
            AccountProvider::Microsoft => {
                match png {
                    Some(png) => microsoft::upload_skin(&http, &session.access_token, png, slim).await?,
                    None => microsoft::reset_skin(&http, &session.access_token).await?,
                }
                let (url, slim, _) = microsoft::appearance(&http, &session.access_token).await?;
                updated.skin_url = url;
                updated.skin_slim = slim;
            }
            AccountProvider::Yggdrasil => {
                let tokens: yggdrasil::Tokens = keychain::load_refresh_token(&updated.id)?
                    .and_then(|text| serde_json::from_str(&text).ok())
                    .ok_or_else(|| AppError::new("auth_expired", crate::core::i18n::pick("登录已过期，请重新登录。", "Sign-in expired; sign in again.")))?;
                yggdrasil::change_skin(&http, &updated, &tokens, png.map(|png| (png, slim))).await?;
                if let Some(api) = updated.server.clone() {
                    let (url, slim) = yggdrasil::skin(&http, &api, &updated.uuid).await.unwrap_or((None, slim));
                    updated.skin_url = url;
                    updated.skin_slim = slim;
                }
            }
            AccountProvider::Offline => {
                crate::core::auth::local_skin::save(&self.paths, &mut updated, png, slim)?;
            }
        }
        self.accounts.upsert(updated)?;
        self.send(UiEvent::AccountsChanged(self.accounts.list()));
        Ok(())
    }

    pub async fn set_cape(&self, id: &str, cape: Option<&str>) -> AppResult<()> {
        let account = self.accounts.get(id).ok_or_else(|| AppError::new("account", crate::core::i18n::pick("账号不存在。", "The account doesn't exist.")))?;
        if account.provider != AccountProvider::Microsoft {
            return Err(AppError::new("cape", crate::core::i18n::pick("只有微软账号有披风。", "Only Microsoft accounts have capes.")));
        }
        let session = self.session_for_account(&account).await?;
        microsoft::set_cape(&self.http()?, &session.access_token, cape).await
    }

    pub fn set_active_account(&self, id: Option<String>) -> AppResult<Settings> {
        self.ensure_storage_idle()?;
        if let Some(ref account_id) = id {
            if self.accounts.get(account_id).is_none() {
                return Err(AppError::new("account_not_found", crate::core::i18n::pick("账号不存在。", "The account doesn't exist.")));
            }
        }
        self.settings
            .update(|settings| settings.active_account_id = id)
    }

    pub fn create_instance(
        &self,
        name: &str,
        version_id: &str,
        version_type: &str,
    ) -> AppResult<Instance> {
        self.ensure_storage_idle()?;
        let instance = self.instances.create(name, version_id, version_type)?;
        self.settings.update(|settings| {
            if settings.active_instance_id.is_none() {
                settings.active_instance_id = Some(instance.id.clone());
            }
        })?;
        Ok(instance)
    }

    pub fn delete_instance(&self, id: &str) -> AppResult<(Vec<Instance>, Settings)> {
        self.ensure_idle(id)?;
        self.instances.get(id)?;
        self.instances.remove(id)?;
        let remaining = self.instances.list();
        let settings = self.settings.update(|settings| {
            if settings.active_instance_id.as_deref() == Some(id) {
                settings.active_instance_id = remaining.first().map(|instance| instance.id.clone());
            }
        })?;
        Ok((remaining, settings))
    }

    pub fn set_active_instance(&self, id: Option<String>) -> AppResult<Settings> {
        self.ensure_storage_idle()?;
        if let Some(ref instance_id) = id {
            self.instances.get(instance_id)?;
        }
        self.settings
            .update(|settings| settings.active_instance_id = id)
    }

    /// Looks a CurseForge pack's files up; runs on a blocking thread, so it
    /// waits on the runtime for the network part.
    /// Moves a modpack world to the pack version in `path`.
    fn update_pack(&self, id: &str, path: &Path) -> AppResult<String> {
        let update = match crate::core::curseforge::read_manifest(path)? {
            Some(manifest) => {
                let files = self.resolve_curseforge_pack(&manifest)?;
                crate::core::modpacks::update(&self.paths, &self.instances, id, path, Some((&manifest, files)))?
            }
            None => crate::core::modpacks::update(&self.paths, &self.instances, id, path, None)?,
        };
        Ok(crate::tr!("整合包已更新{}{}，开始游戏时会下载新文件。", "Modpack updated{}{}; new files download when you play.",
            if update.version_changed { crate::core::i18n::pick("（游戏或加载器版本也变了）", " (the game or loader version changed too)") } else { "" },
            if update.removed > 0 { crate::tr!("，{} 个旧文件移到了回收站", "; {} old files went to the recycle bin", update.removed) } else { String::new() }
        ))
    }

    /// Makes a new world from a `.mrpack` or CurseForge `.zip` and selects
    /// it; returns its id.
    fn import_pack(&self, path: &Path) -> AppResult<String> {
        let instance = match crate::core::curseforge::read_manifest(path)? {
            Some(manifest) => {
                let files = self.resolve_curseforge_pack(&manifest)?;
                crate::core::modpacks::import_curseforge(&self.paths, &self.instances, path, &manifest, files)?
            }
            None => crate::core::modpacks::import(&self.paths, &self.instances, path)?,
        };
        self.set_active_instance(Some(instance.id.clone()))?;
        Ok(instance.id)
    }

    fn resolve_curseforge_pack(&self, manifest: &crate::core::curseforge::PackManifest) -> AppResult<Vec<crate::core::curseforge::ResolvedFile>> {
        let key = self.settings.get().curseforge_key;
        let http = self.http()?;
        tokio::runtime::Handle::current().block_on(crate::core::curseforge::resolve_pack(&http, &key, manifest))
    }

    /// Java majors the installed worlds need, for telling unused runtimes apart.
    pub fn java_needs(&self) -> std::collections::HashSet<u32> {
        self.instances
            .list()
            .iter()
            .filter(|instance| instance.installed)
            .filter_map(|instance| load_version(&self.paths, instance).ok())
            .map(|version| crate::core::mojang::version::java_major(&version))
            .collect()
    }

    pub fn storage_usage(&self) -> manage::StorageUsage {
        manage::usage(&self.paths, &self.instances.list(), &self.instances.recycled(), &self.java_needs())
    }

    pub fn instance_paths(&self, instance_id: &str) -> AppResult<InstancePaths> {
        AppPaths::validate_instance_id(instance_id)?;
        Ok(InstancePaths {
            game_dir: self
                .paths
                .instance_game_dir(instance_id)
                .display()
                .to_string(),
            log_dir: self
                .paths
                .instance_logs_dir(instance_id)
                .display()
                .to_string(),
        })
    }

    /// The last `lines` lines of the latest log. Only the tail of the file is
    /// read, so a modpack log of tens of megabytes opens instantly.
    ///
    /// This `latest.log` lives in the instance's launcher logs directory and
    /// is truncated at every launch; it contains the stdout/stderr captured
    /// from Java. The game's own log (`minecraft/logs/latest.log`) is a
    /// different file and is intentionally not used for crash analysis, so a
    /// previous run cannot leak into the current diagnosis.
    /// Whether the installed version can open a world straight from launch.
    pub fn quick_play_supported(&self, instance_id: &str) -> bool {
        self.instances
            .get(instance_id)
            .ok()
            .filter(|instance| instance.installed)
            .and_then(|instance| load_version(&self.paths, &instance).ok())
            .is_some_and(|version| launch::supports_quick_play(&version))
    }

    pub fn read_log(&self, instance_id: &str, lines: usize) -> AppResult<Vec<String>> {
        const TAIL_BYTES: u64 = 1024 * 1024;
        AppPaths::validate_instance_id(instance_id)?;
        let path = self.paths.instance_logs_dir(instance_id).join("latest.log");
        let Some((text, _)) = read_tail_text(&path, TAIL_BYTES)? else {
            return Ok(Vec::new());
        };
        let mut all: Vec<&str> = text.lines().collect();
        let max = lines.clamp(1, 2000);
        if all.len() > max {
            all.drain(..all.len() - max);
        }
        Ok(all.into_iter().map(str::to_string).collect())
    }

    /// Diagnose the current run only. The launcher's captured log is rewritten
    /// at every launch and both it and the crash report are gated by
    /// `since_ms`, so an old log file cannot produce a misleading diagnosis.
    /// Reads are bounded: 512 KiB of log tail and 256 KiB of crash report.
    pub fn analyze_crash(&self, instance_id: &str, since_ms: i64) -> AppResult<CrashAnalysis> {
        const LOG_BYTES: u64 = 512 * 1024;
        const REPORT_BYTES: u64 = 256 * 1024;
        AppPaths::validate_instance_id(instance_id)?;
        self.instances.get(instance_id)?;

        let log_path = self.paths.instance_logs_dir(instance_id).join("latest.log");
        // The header carries the resolved `memory=` value. Read it separately
        // from the tail: a long log would otherwise hide it from the scan.
        let log = match analysis_log_text(&log_path, LOG_BYTES)? {
            Some((text, modified)) if modified >= since_ms => text,
            _ => String::new(),
        };
        let report_dir = self.paths.instance_game_dir(instance_id).join("crash-reports");
        let mut report = latest_crash_report(&report_dir, since_ms, "txt", "")?
            .map(|path| read_head_text(&path, REPORT_BYTES))
            .transpose()?;
        // A crash of the Java VM itself leaves hs_err_pid<pid>.log in the
        // game folder, its working directory.
        let game_dir = self.paths.instance_game_dir(instance_id);
        if let Some(path) = latest_crash_report(&game_dir, since_ms, "log", "hs_err_pid")? {
            let native = read_head_text(&path, REPORT_BYTES)?;
            report = Some(match report {
                Some(report) => format!("{native}\n{report}"),
                None => native,
            });
        }

        let diagnoses = crash::analyze_with_memory_limit(
            &log,
            report.as_deref(),
            platform::total_memory_mb(),
        );
        Ok(CrashAnalysis {
            diagnoses,
            excerpt: build_excerpt(&log, report.as_deref()),
        })
    }

    /// Download (or detect) the requested Java and make it this instance's
    /// explicit Java. The UI awaits this command, so the crash fix is only
    /// marked handled after the runtime really exists.
    pub async fn install_java_for_instance(
        self: &Arc<Self>,
        instance_id: &str,
        major: u32,
    ) -> AppResult<JavaInstallation> {
        self.ensure_idle(instance_id)?;
        let target = self.instances.get(instance_id)?;
        // The Java must match what the game runs as (Intel under Rosetta
        // for old versions on Apple Silicon).
        let arch = load_version(&self.paths, &target)
            .map(|version| crate::core::mojang::version::game_arch(&version))
            .unwrap_or(std::env::consts::ARCH);
        if !(8..=64).contains(&major) {
            return Err(AppError::new("java_version", crate::core::i18n::pick("不支持的 Java 版本。", "Unsupported Java version.")));
        }
        lock(&self.operations).insert(instance_id.to_string());
        let result = async {
            let cancel = CancellationToken::new();
            let java = runtime::ensure_java_with_source(
                &self.http()?,
                &self.paths,
                major,
                None,
                arch,
                &cancel,
                None,
                self.settings.get().download_source,
            )
            .await?;
            let mut instance = self.instances.get(instance_id)?;
            instance.java_path = Some(java.path.clone());
            self.instances.update(&instance)?;
            Ok(java)
        }
        .await;
        lock(&self.operations).remove(instance_id);
        if result.is_ok() {
            self.load_java_runtimes();
        }
        result
    }

    pub fn load_versions(self: &Arc<Self>, force_refresh: bool) {
        let this = self.clone();
        self.spawn_task(async move {
            let result = match this.http() {
                Ok(http) => manifest::load_manifest(&http, &this.paths, force_refresh)
                    .await
                    .map_err(|error| error.to_payload()),
                Err(error) => Err(error.to_payload()),
            };
            this.send(UiEvent::Versions(result));
        });
    }

    pub fn load_java_runtimes(self: &Arc<Self>) {
        if let Some(runtime) = &self.runtime {
            let this = self.clone();
            runtime.spawn_blocking(move || {
                let explicit = this
                    .instances
                    .list()
                    .into_iter()
                    .filter_map(|i| i.java_path)
                    .chain(this.settings.get().java_path)
                    .filter(|p| p != "auto")
                    .map(PathBuf::from)
                    .collect::<Vec<_>>();
                this.send(UiEvent::JavaRuntimes(detect::find_all_in(
                    &[this.paths.java.clone()],
                    &explicit,
                )));
            });
        }
    }

    pub fn load_loader_versions(self: &Arc<Self>, key: String, loader: String, minecraft: String) {
        let this = self.clone();
        self.spawn_task(async move {
            let result =
                async { crate::core::loaders::list_with_source(&this.http()?, &loader, &minecraft, this.settings.get().download_source).await }
                    .await;
            this.send(UiEvent::LoaderVersions {
                key,
                result: result.map_err(|e| e.to_payload()),
            });
        });
    }

    pub fn search_mods(
        self: &Arc<Self>,
        id: &str,
        key: String,
        query: String,
        offset: usize,
        kind: ContentKind,
        categories: Vec<String>,
        sort: String,
        source: String,
    ) -> AppResult<()> {
        let instance = self.instances.get(id)?;
        let this = self.clone();
        self.spawn_task(async move {
            let result = async {
                if source == "curseforge" {
                    let key = this.settings.get().curseforge_key;
                    return crate::core::curseforge::search(&this.http()?, &key, &instance, &query, offset, kind, &sort).await;
                }
                crate::core::modrinth::search(&this.http()?, &instance, &query, offset, kind, &categories, &sort).await
            }
            .await;
            this.send(UiEvent::ModSearch {
                key,
                result: result.map_err(|e| e.to_payload()),
            });
        });
        Ok(())
    }
    pub fn load_mod_versions(
        self: &Arc<Self>,
        id: &str,
        key: String,
        project: String,
        kind: ContentKind,
    ) -> AppResult<()> {
        let instance = self.instances.get(id)?;
        let this = self.clone();
        self.spawn_task(async move {
            let result = async {
                if project.starts_with(crate::core::curseforge::PREFIX) {
                    let key = this.settings.get().curseforge_key;
                    return crate::core::curseforge::versions(&this.http()?, &key, &instance, &project, kind).await;
                }
                crate::core::modrinth::versions(&this.http()?, &instance, &project, kind).await
            }
            .await;
            this.send(UiEvent::ModVersions {
                key,
                result: result.map_err(|e| e.to_payload()),
            });
        });
        Ok(())
    }
    /// Installs or updates mods (with required dependencies) in the background;
    /// the result arrives as `OperationComplete`.
    pub fn install_mod(self: &Arc<Self>, id: &str, versions: Vec<String>) -> AppResult<String> {
        self.ensure_idle(id)?;
        let instance = self.instances.get(id)?;
        let task_id = Uuid::new_v4().to_string();
        let cancel = CancellationToken::new();
        lock(&self.tasks).insert(task_id.clone(), cancel.clone());
        lock(&self.operations).insert(id.to_string());
        let this = self.clone();
        let task = task_id.clone();
        self.spawn_task(async move {
            let result = async {
                // "Update all" can mix Modrinth and CurseForge mods.
                let (curseforge, modrinth): (Vec<String>, Vec<String>) = versions.into_iter().partition(|v| v.starts_with(crate::core::curseforge::PREFIX));
                let mut done = Vec::new();
                if !modrinth.is_empty() {
                    done.push(crate::core::modrinth::install(&this.http()?, &this.paths, &instance, &modrinth, &cancel).await?);
                }
                if !curseforge.is_empty() {
                    let key = this.settings.get().curseforge_key;
                    done.push(crate::core::curseforge::install(&this.http()?, &key, &this.paths, &instance, &curseforge, ContentKind::Mod, None, &cancel).await?);
                }
                Ok::<String, AppError>(done.join(" "))
            }
            .await;
            lock(&this.operations).remove(&instance.id);
            lock(&this.tasks).remove(&task);
            this.send(UiEvent::OperationComplete(
                result.map_err(|e| e.to_payload()),
            ));
        });
        Ok(task_id)
    }
    /// OptiFine into `mods`, with OptiFabric on Fabric and Quilt. Runs in
    /// the background; the result arrives as `OperationComplete`.
    pub fn install_optifine(self: &Arc<Self>, id: &str) -> AppResult<String> {
        self.ensure_idle(id)?;
        let instance = self.instances.get(id)?;
        let task_id = Uuid::new_v4().to_string();
        let cancel = CancellationToken::new();
        lock(&self.tasks).insert(task_id.clone(), cancel.clone());
        lock(&self.operations).insert(id.to_string());
        let this = self.clone();
        let task = task_id.clone();
        self.spawn_task(async move {
            let result = async {
                let http = this.http()?;
                if instance.loader == "vanilla" {
                    let file = crate::core::optifine::install_vanilla(&http, &this.paths, &instance, &cancel).await?;
                    return Ok(crate::tr!("已为原版安装 {file}。", "Installed {file} for vanilla."));
                }
                let file = crate::core::optifine::install(&http, &this.paths, &instance, &cancel).await?;
                if instance.loader == "forge" {
                    return Ok(crate::tr!("已安装 {file}，进游戏后在「选项 → 视频设置」里调整。", "Installed {file}; adjust it in game under Options → Video Settings."));
                }
                let versions = crate::core::modrinth::versions(&http, &instance, "optifabric", ContentKind::Mod).await?;
                let first = versions.first().ok_or_else(|| {
                    AppError::new("optifine", crate::tr!("已安装 {file}，但 OptiFabric 还没有适配 {}，暂时无法在 Fabric 上加载 OptiFine。", "Installed {file}, but OptiFabric doesn't support {} yet, so OptiFine can't load on Fabric for now.", instance.version_id))
                })?;
                crate::core::modrinth::install(&http, &this.paths, &instance, &[first.id.clone()], &cancel).await?;
                Ok(crate::tr!("已安装 {file} 和 OptiFabric。", "Installed {file} and OptiFabric."))
            }
            .await;
            lock(&this.operations).remove(&instance.id);
            lock(&this.tasks).remove(&task);
            this.send(UiEvent::OperationComplete(result.map_err(|e: AppError| e.to_payload())));
        });
        Ok(task_id)
    }

    /// Downloads a resource pack or shader pack in the background; the result
    /// arrives as `OperationComplete`.
    pub fn install_pack(
        self: &Arc<Self>,
        id: &str,
        version: String,
        kind: ContentKind,
        save: Option<String>,
    ) -> AppResult<String> {
        self.ensure_idle(id)?;
        let instance = self.instances.get(id)?;
        let task_id = Uuid::new_v4().to_string();
        let cancel = CancellationToken::new();
        lock(&self.tasks).insert(task_id.clone(), cancel.clone());
        lock(&self.operations).insert(id.to_string());
        let this = self.clone();
        let task = task_id.clone();
        self.spawn_task(async move {
            let result = async {
                if version.starts_with(crate::core::curseforge::PREFIX) {
                    let key = this.settings.get().curseforge_key;
                    return crate::core::curseforge::install(&this.http()?, &key, &this.paths, &instance, std::slice::from_ref(&version), kind, save.as_deref(), &cancel).await;
                }
                crate::core::modrinth::install_pack(
                    &this.http()?,
                    &this.paths,
                    &instance,
                    &version,
                    kind,
                    save.as_deref(),
                    &cancel,
                )
                .await
            }
            .await;
            lock(&this.operations).remove(&instance.id);
            lock(&this.tasks).remove(&task);
            this.send(UiEvent::OperationComplete(
                result.map_err(|e| e.to_payload()),
            ));
        });
        Ok(task_id)
    }

    /// Names, icons and available updates for the instance's local mods.
    pub async fn inspect_mods(&self, id: &str) -> AppResult<Vec<crate::core::modrinth::ModInsight>> {
        let instance = self.instances.get(id)?;
        let http = self.http()?;
        let (mut found, modrinth_error) = match crate::core::modrinth::inspect(&http, &self.paths, &instance).await {
            Ok(found) => (found, None),
            Err(error) => { tracing::warn!(%error, "Modrinth lookup failed; trying CurseForge independently"); (vec![], Some(error)) }
        };
        let mut curseforge_available = false;
        // Files Modrinth does not know may be on CurseForge, when a key is set.
        let key = self.settings.get().curseforge_key;
        if !key.trim().is_empty() {
            let known: HashSet<&str> = found.iter().map(|insight| insight.file.as_str()).collect();
            let rest: HashSet<String> = crate::core::content::mods(&self.paths, id)?
                .into_iter()
                .map(|item| item.name)
                .filter(|name| !known.contains(name.as_str()))
                .collect();
            if !rest.is_empty() {
                match crate::core::curseforge::inspect(&http, &key, &self.paths, &instance, &rest).await {
                    Ok(more) => { curseforge_available = true; found.extend(more); }
                    Err(error) => tracing::warn!(%error, "CurseForge mod lookup failed"),
                }
            }
        }
        if !curseforge_available { if let Some(error) = modrinth_error { return Err(error); } }
        Ok(found)
    }

    /// Names and icons for the instance's resource packs and shader packs.
    pub async fn inspect_packs(&self, id: &str) -> AppResult<Vec<crate::core::modrinth::PackInsight>> {
        let instance = self.instances.get(id)?;
        crate::core::modrinth::inspect_packs(&self.http()?, &self.paths, &instance).await
    }

    pub fn scan_java_directory(self: &Arc<Self>, path: PathBuf) {
        let this = self.clone();
        if let Some(runtime) = &self.runtime {
            runtime.spawn_blocking(move || {
                let explicit = this
                    .instances
                    .list()
                    .into_iter()
                    .filter_map(|i| i.java_path)
                    .chain(this.settings.get().java_path)
                    .filter(|p| p != "auto")
                    .map(PathBuf::from)
                    .collect::<Vec<_>>();
                this.send(UiEvent::JavaRuntimes(detect::find_all_in(
                    &[this.paths.java.clone(), path],
                    &explicit,
                )));
            });
        }
    }
    pub fn download_java(self: &Arc<Self>, major: u32) -> AppResult<String> {
        self.ensure_all_idle()?;
        let task_id = Uuid::new_v4().to_string();
        let cancel = CancellationToken::new();
        lock(&self.operations).insert("java-download".into());
        lock(&self.tasks).insert(task_id.clone(), cancel.clone());
        let this = self.clone();
        let task = task_id.clone();
        self.spawn_task(async move {
            let result = async {
                let java =
                    runtime::ensure_java_with_source(&this.http()?, &this.paths, major, None, std::env::consts::ARCH, &cancel, None, this.settings.get().download_source)
                        .await?;
                Ok(crate::tr!("Java {} 已就绪：{}", "Java {} is ready: {}", java.version, java.path))
            }
            .await;
            lock(&this.operations).remove("java-download");
            lock(&this.tasks).remove(&task);
            this.send(UiEvent::OperationComplete(
                result.map_err(|e: AppError| e.to_payload()),
            ));
            this.load_java_runtimes();
        });
        Ok(task_id)
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
    pub fn relocate(self: &Arc<Self>, destination: PathBuf) -> AppResult<()> {
        self.ensure_all_idle()?;
        if std::env::var_os("MC_LAUNCHER_DATA_DIR").is_some() {
            return Err(AppError::new(
                "storage",
                crate::core::i18n::pick("测试环境使用独立数据目录，不能切换正式存储位置。", "This test setup uses its own data folder and can't move the real one."),
            ));
        }
        let locator = AppPaths::default_root()?.join("storage-location.json");
        lock(&self.operations).insert("storage-migration".into());
        let this = self.clone();
        self.runtime.as_ref().unwrap().spawn_blocking(move || {
            let result = crate::core::storage::relocate(&this.paths.root, &destination, &locator);
            lock(&this.operations).remove("storage-migration");
            this.send(UiEvent::StorageChanged(result.map_err(|e| e.to_payload())));
        });
        Ok(())
    }
    pub fn reopen(&self, root: PathBuf) -> AppResult<Arc<Self>> {
        Self::new(AppPaths::from_root(root)?, self.events.clone())
    }

    pub fn install_instance(self: &Arc<Self>, instance_id: &str) -> AppResult<String> {
        self.ensure_idle(instance_id)?;
        let instance = self.instances.get(instance_id)?;
        let settings = self.settings.get();
        {
            let mut installing = lock(&self.installing);
            if !installing.is_empty() {
                return Err(AppError::new("install_running", crate::core::i18n::pick("这个世界正在下载，请稍候。", "This world is downloading; please wait.")));
            }
            installing.insert(instance_id.to_string(), String::new());
        }

        let task_id = Uuid::new_v4().to_string();
        let cancel = CancellationToken::new();
        lock(&self.tasks).insert(task_id.clone(), cancel.clone());
        if let Some(entry) = lock(&self.installing).get_mut(instance_id) {
            *entry = task_id.clone();
        }

        let this = self.clone();
        let task = task_id.clone();
        let instance_for_task = instance.clone();
        self.spawn_task(async move {
            let result = this
                .run_install(
                    &instance_for_task,
                    &task,
                    settings.download_source,
                    settings.max_concurrent_downloads,
                    instance_for_task
                        .java_path
                        .as_deref()
                        .or(settings.java_path.as_deref()),
                    &cancel,
                )
                .await;
            lock(&this.tasks).remove(&task);
            lock(&this.installing).remove(&instance_for_task.id);
            match result {
                Ok(()) => this.send(UiEvent::InstallComplete {
                    task_id: task,
                    instance_id: instance_for_task.id,
                    version_id: instance_for_task.version_id,
                }),
                Err(error) => {
                    tracing::warn!(
                        instance = %instance_for_task.id,
                        code = error.code(),
                        error = %error,
                        "instance install failed"
                    );
                    this.send(UiEvent::InstallError {
                        task_id: task,
                        instance_id: instance_for_task.id,
                        error: error.to_payload(),
                    });
                }
            }
        });
        Ok(task_id)
    }

    pub fn cancel_task(&self, task_id: &str) {
        if let Some(token) = lock(&self.tasks).remove(task_id) {
            token.cancel();
        }
    }

    /// Starts the game; with `world`, it opens that singleplayer world
    /// directly instead of the title screen (Minecraft 1.20+); with `server`,
    /// it joins that server.
    /// `offline` plays as the account without signing in, for when its
    /// sign-in servers can't be reached: singleplayer works, online servers don't.
    pub fn launch_instance(
        self: &Arc<Self>,
        instance_id: &str,
        world: Option<String>,
        server: Option<String>,
        offline: bool,
    ) -> AppResult<()> {
        self.ensure_idle(instance_id)?;
        if world.is_some() && server.is_some() {
            return Err(AppError::new("launch", crate::core::i18n::pick("不能同时进入存档和服务器。", "Can't open a save and a server at the same time.")));
        }
        if let Some(folder) = &world {
            crate::core::content::world_dir(&self.paths, instance_id, folder)?;
        }
        if let Some(address) = &server {
            servers::parse_address(address)?;
        }
        let settings = self.settings.get();
        let instance = self.instances.get(instance_id)?;
        if !instance.installed {
            return Err(AppError::new(
                "instance_not_installed",
                crate::core::i18n::pick("这个世界还没有下载完成，请先下载。", "This world isn't fully downloaded yet; install it first."),
            ));
        }
        // A world bound to an account plays as it while that account exists.
        let account_id = instance
            .account_id
            .clone()
            .filter(|id| self.accounts.get(id).is_some())
            .or_else(|| settings.active_account_id.clone())
            .ok_or_else(|| AppError::new("account", crate::core::i18n::pick("请先添加并选择一个账号。", "Add and select an account first.")))?;
        let account = self
            .accounts
            .get(&account_id)
            .ok_or_else(|| AppError::new("account", crate::core::i18n::pick("当前账号不存在，请重新选择。", "The selected account no longer exists; choose another.")))?;
        {
            if lock(&self.launches).contains_key(instance_id)
                || lock(&self.starting).contains(instance_id)
            {
                return Err(AppError::new("already_running", crate::core::i18n::pick("这个世界已经在运行。", "This world is already running.")));
            }
            lock(&self.starting).insert(instance_id.to_string());
        }

        let this = self.clone();
        let id = instance_id.to_string();
        self.spawn_task(async move {
            let run_this = this.clone();
            let target = match (&world, &server) {
                (Some(folder), _) => LaunchTarget::World(folder),
                (_, Some(address)) => LaunchTarget::Server(address),
                _ => LaunchTarget::Title,
            };
            let result = run_this
                .run_launch(&id, &instance, &settings, &account, target, offline)
                .await;
            lock(&this.starting).remove(&id);
            if let Err(error) = result {
                tracing::warn!(instance = %id, code = error.code(), error = %error, "launch failed");
                this.send(UiEvent::LaunchError {
                    instance_id: id,
                    error: error.to_payload(),
                });
            }
        });
        Ok(())
    }

    /// Live status of a server: players, latency, message of the day.
    /// The UUID the host plays as on a server, so their things from the save
    /// come along: the Microsoft one on an online-mode server, the one the
    /// server derives from the name otherwise.
    fn host_uuid(&self, instance: &Instance, online_mode: bool) -> Option<String> {
        let account_id = instance.account_id.clone().filter(|id| self.accounts.get(id).is_some()).or_else(|| self.settings.get().active_account_id)?;
        let account = self.accounts.get(&account_id)?;
        let uuid = match (online_mode, account.provider) {
            (true, crate::core::accounts::AccountProvider::Microsoft) => account.uuid.replace('-', ""),
            (true, _) => return None,
            (false, _) => crate::core::auth::offline::offline_uuid(&account.username).replace('-', ""),
        };
        let uuid = uuid::Uuid::parse_str(&uuid).ok()?;
        Some(uuid.hyphenated().to_string())
    }

    /// Builds the server for a world in `destination`, and beside it the
    /// pack friends import to join: every mod of the world, the server in
    /// their list, and which mods the server runs. Runs on a blocking thread.
    pub fn export_server(
        self: &Arc<Self>,
        id: &str,
        destination: &Path,
        mut options: crate::core::server_pack::ServerOptions,
        cancel: &CancellationToken,
    ) -> AppResult<(crate::core::server_pack::Summary, Option<PathBuf>)> {
        let instance = self.instances.get(id)?;
        let settings = self.settings.get();
        let base: VersionJson = std::fs::read(self.paths.version_json(&instance.version_id))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or_else(|| AppError::new("version_missing", crate::core::i18n::pick("版本文件不存在，请先下载这个世界。", "The version file is missing; install this world first.")))?;
        let version = load_version(&self.paths, &instance)?;
        let (http, runtime) = (self.http()?, tokio::runtime::Handle::current());
        let java = runtime.block_on(runtime::ensure_java_with_source(
            &http,
            &self.paths,
            java_major(&version),
            instance.java_path.as_deref().or(settings.java_path.as_deref()),
            crate::core::mojang::version::game_arch(&version),
            cancel,
            None,
            settings.download_source,
        ))?;
        if options.world.is_some() {
            options.host_uuid = self.host_uuid(&instance, options.online_mode);
        }
        let summary = runtime.block_on(crate::core::server_pack::create(&http, &self.paths, &instance, &base, Path::new(&java.path), settings.download_source, &options, destination, cancel))?;
        if !options.client_pack {
            return Ok((summary, None));
        }
        let candidates = crate::core::modpacks::export_candidates(&self.paths, id)?;
        let mut hashes: Vec<String> = candidates.iter().map(|c| c.sha1.clone()).collect();
        hashes.extend(summary.included.iter().map(|(_, sha1)| sha1.clone()));
        hashes.sort();
        hashes.dedup();
        // Without the lookup every file is bundled and nothing is fetched
        // later; the pack still works.
        let hosted = runtime.block_on(crate::core::modrinth::files_by_hash(&http, &hashes)).unwrap_or_else(|error| {
            tracing::warn!(%error, "modrinth lookup for the client pack failed");
            HashMap::new()
        });
        let meta = crate::core::content::mod_meta(&self.paths, id).unwrap_or_default();
        let address = options.address.trim().to_string();
        let manifest = crate::core::server_mods::Manifest {
            name: instance.name.clone(),
            address: address.clone(),
            mods: summary
                .included
                .iter()
                .map(|(file, sha1)| crate::core::server_mods::ManifestMod {
                    file: file.clone(),
                    sha1: sha1.clone(),
                    id: meta.get(file).and_then(|m| m.id.clone()),
                    url: hosted.get(sha1).map(|f| f.url.clone()).filter(|url| crate::core::modpacks::trusted_url(url)),
                })
                .collect(),
        };
        let mut extra = vec![(crate::core::server_mods::FILE.to_string(), serde_json::to_vec_pretty(&manifest)?)];
        if !address.is_empty() {
            extra.push(("servers.dat".to_string(), crate::core::server_mods::servers_dat(&instance.name, &address)));
        }
        let pack_options = crate::core::modpacks::ExportOptions {
            name: instance.name.clone(),
            version: chrono::Local::now().format("%Y.%m.%d-%H%M").to_string(),
            summary: crate::core::i18n::pick("导入后就能进服务器。", "Import to join the server.").to_string(),
            extra,
            ..Default::default()
        };
        let file = destination.join(crate::tr!("{}-给朋友.mrpack", "{}-for-friends.mrpack", crate::core::util::file_stem_safe(&instance.name)));
        crate::core::modpacks::export(&self.paths, &instance, &candidates, crate::core::modpacks::Hosted::Modrinth(&hosted), &pack_options, &file)?;
        Ok((summary, Some(file)))
    }

    /// The mods a world lacks for a server it was imported to join.
    pub fn check_server_mods(&self, id: &str, address: &str) -> AppResult<Option<crate::core::server_mods::Check>> {
        crate::core::server_mods::check(&self.paths, id, address)
    }

    pub async fn ping_server(&self, address: &str) -> AppResult<servers::ServerStatus> {
        servers::ping(&self.http()?, address).await
    }

    /// The EasyTier programs behind P2P rooms, downloaded on first use from
    /// the fastest source; GitHub mirrors rank first among failed sources
    /// when downloads go through BMCLAPI.
    pub async fn ensure_easytier(
        &self,
        cancel: &CancellationToken,
        report: Option<crate::core::p2p::easytier::Progress>,
    ) -> AppResult<crate::core::p2p::easytier::Binaries> {
        let mirror = matches!(self.settings.get().download_source, DownloadSource::Bmclapi);
        let http = self.http()?;
        let extra = if crate::core::p2p::easytier::installed(&self.paths).is_some() { Vec::new() } else { self.p2p_remote().await.sources };
        crate::core::p2p::easytier::ensure(&http, &self.paths, mirror, &extra, cancel, report).await
    }

    /// The published P2P node list and download sources (cached for hours).
    pub async fn p2p_remote(&self) -> crate::core::p2p::easytier::Remote {
        match self.http() {
            Ok(http) => crate::core::p2p::easytier::remote(&http, &self.paths).await,
            Err(_) => Default::default(),
        }
    }

    /// Asks the game to close the way its window's close button does, so it
    /// saves first, and ends it after `STOP_GRACE` if it is still running.
    /// `force` skips the wait: progress since the last save is lost.
    pub fn stop_instance(self: &Arc<Self>, instance_id: &str, force: bool) -> AppResult<()> {
        let pid = lock(&self.launches).get(instance_id).copied();
        let Some(pid) = pid.filter(|pid| *pid > 0) else {
            return Err(AppError::new("not_running", crate::core::i18n::pick("这个世界没有在运行。", "This world isn't running.")));
        };
        #[cfg(unix)]
        if !force {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
        let this = self.clone();
        let id = instance_id.to_string();
        self.spawn_task(async move {
            // taskkill takes a moment; it runs here instead of on the UI thread.
            if !force {
                #[cfg(windows)]
                let _ = tokio::process::Command::from(crate::core::platform::quiet_command(
                    "taskkill.exe",
                ))
                .args(["/PID", &pid.to_string(), "/T"])
                .status()
                .await;
                // Big saves and heavy modpacks can take a while to write out.
                tokio::time::sleep(STOP_GRACE).await;
            }
            let still_running = lock(&this.launches).get(&id).copied();
            if let Some(pid) = still_running.filter(|current| *current == pid) {
                #[cfg(unix)]
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
                #[cfg(windows)]
                {
                    let _ = tokio::process::Command::from(crate::core::platform::quiet_command(
                        "taskkill.exe",
                    ))
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .status()
                    .await;
                }
            }
        });
        Ok(())
    }

    pub fn start_login(self: &Arc<Self>, mode: LoginMode) -> AppResult<String> {
        self.ensure_storage_idle()?;
        let client_id =
            microsoft::validate_client_id(&self.settings.get().effective_client_id())?.to_string();
        let login_id = Uuid::new_v4().to_string();
        let cancel = CancellationToken::new();
        lock(&self.logins).insert(login_id.clone(), cancel.clone());

        if mode == LoginMode::Device {
            let this = self.clone();
            let id = login_id.clone();
            let client = client_id.clone();
            let cancel_task = cancel.clone();
            self.spawn_task(async move {
                let result = async {
                    let http = this.http()?;
                    let info = microsoft::request_device_code(&http, &client).await?;
                    if cancel_task.is_cancelled() {
                        return Err(AppError::new("cancelled", crate::core::i18n::pick("登录已取消。", "Sign-in cancelled.")));
                    }
                    this.send(UiEvent::LoginStarted(LoginStartData {
                        login_id: id.clone(),
                        mode: "device".to_string(),
                        authorize_url: None,
                        user_code: Some(info.user_code.clone()),
                        verification_uri: Some(info.verification_uri.clone()),
                        expires_in: Some(info.expires_in),
                    }));
                    let token =
                        microsoft::poll_device_code(&http, &client, &info, &cancel_task).await?;
                    this.finish_login(&id, token, &cancel_task).await
                }
                .await;
                this.finish_login_result(&id, result);
            });
            return Ok(login_id);
        }

        let flow = match microsoft::LoopbackFlow::start(&client_id) {
            Ok(flow) => flow,
            Err(error) => {
                lock(&self.logins).remove(&login_id);
                return Err(error);
            }
        };
        let authorize_url = flow.authorize_url.clone();
        let redirect_uri = flow.redirect_uri.clone();
        let verifier = flow.verifier().to_string();
        self.send(UiEvent::LoginStarted(LoginStartData {
            login_id: login_id.clone(),
            mode: "browser".to_string(),
            authorize_url: Some(authorize_url),
            user_code: None,
            verification_uri: None,
            expires_in: None,
        }));

        let this = self.clone();
        let id = login_id.clone();
        let client = client_id.clone();
        let cancel_task = cancel.clone();
        self.spawn_task(async move {
            let result = async {
                let wait_cancel = cancel_task.clone();
                let code = tokio::task::spawn_blocking(move || flow.wait_for_code(&wait_cancel))
                    .await
                    .map_err(|err| {
                        AppError::detail("auth", crate::core::i18n::pick("登录等待任务失败。", "The sign-in wait failed."), err.to_string())
                    })??;
                let http = this.http()?;
                let token =
                    microsoft::exchange_code(&http, &client, &code, &redirect_uri, &verifier)
                        .await?;
                this.finish_login(&id, token, &cancel_task).await
            }
            .await;
            this.finish_login_result(&id, result);
        });
        Ok(login_id)
    }

    pub fn cancel_login(&self, login_id: &str) {
        if let Some(token) = lock(&self.logins).remove(login_id) {
            token.cancel();
        }
    }

    fn spawn_task<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        if let Some(runtime) = &self.runtime {
            runtime.spawn(future);
        }
    }

    async fn run_install(
        &self,
        instance: &Instance,
        task_id: &str,
        source: DownloadSource,
        concurrency: usize,
        java_path: Option<&str>,
        cancel: &CancellationToken,
    ) -> AppResult<()> {
        self.progress(
            task_id,
            &instance.id,
            "metadata",
            0,
            0,
            0,
            0,
            crate::core::i18n::pick("正在获取 Minecraft 版本信息", "Getting Minecraft version info"),
        );
        let http = self.http()?;
        let plan =
            mojang_install::build_plan(&http, &self.paths, &instance.version_id, source, cancel)
                .await?;
        let mut tasks = plan
            .files
            .iter()
            .map(|file| file.task.clone())
            .collect::<Vec<_>>();
        let mut launch_version = plan.version.clone();
        let mut loader_version = instance.loader_version.clone();
        if let Some(flavor) = crate::core::fabric::Flavor::of(&instance.loader) {
            let (merged, libraries, pinned) = crate::core::fabric::prepare_flavor_with_source(
                flavor,
                &http,
                &self.paths,
                &plan.version,
                instance.loader_version.as_deref(),
                cancel,
                source,
            )
            .await?;
            launch_version = merged;
            loader_version = Some(pinned);
            tasks.extend(libraries);
        }
        if ["liteloader", "cleanroom"].contains(&instance.loader.as_str()) {
            let (merged, libraries, pinned) = crate::core::extra_loaders::prepare(&http, &self.paths, &plan.version, &instance.loader, instance.loader_version.as_deref(), cancel).await?;
            launch_version = merged; loader_version = Some(pinned); tasks.extend(libraries);
        }
        tasks.extend(crate::core::modpacks::downloads(&self.paths, instance)?);
        let progress = Arc::new(DownloadProgress::new(&tasks));
        let initial = progress.snapshot();
        self.progress(
            task_id,
            &instance.id,
            "download",
            initial.files_done,
            initial.files_total,
            initial.bytes_done,
            initial.bytes_total,
            crate::core::i18n::pick("正在下载游戏文件", "Downloading game files"),
        );

        let reporter_cancel = CancellationToken::new();
        let reporter_events = self.events.clone();
        let reporter_task_id = task_id.to_string();
        let reporter_instance_id = instance.id.clone();
        let reporter_progress = progress.clone();
        let reporter_stop = reporter_cancel.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(200));
            loop {
                tokio::select! {
                    _ = reporter_stop.cancelled() => break,
                    _ = interval.tick() => {
                        let snapshot = reporter_progress.snapshot();
                        let _ = reporter_events.try_send(UiEvent::InstallProgress(InstallProgressData {
                            task_id: reporter_task_id.clone(),
                            instance_id: reporter_instance_id.clone(),
                            stage: "download".to_string(),
                            done: snapshot.files_done,
                            total: snapshot.files_total,
                            bytes: snapshot.bytes_done,
                            total_bytes: snapshot.bytes_total,
                            message: crate::core::i18n::pick("正在下载游戏文件", "Downloading game files").to_string(),
                        }));
                    }
                }
            }
        });

        let result = download::download_all(
            tasks,
            http.clone(),
            concurrency,
            cancel.clone(),
            progress.clone(),
        )
        .await;
        reporter_cancel.cancel();
        result?;

        let snapshot = progress.snapshot();
        self.progress(
            task_id,
            &instance.id,
            "download",
            snapshot.files_done,
            snapshot.files_total,
            snapshot.bytes_done,
            snapshot.bytes_total,
            crate::core::i18n::pick("游戏文件下载完成", "Game files downloaded"),
        );
        self.progress(
            task_id,
            &instance.id,
            "java",
            snapshot.files_done,
            snapshot.files_total,
            snapshot.bytes_done,
            snapshot.bytes_total,
            crate::core::i18n::pick("正在准备 Java 运行时", "Preparing the Java runtime"),
        );
        let events = self.events.clone();
        let task = task_id.to_string();
        let instance_id = instance.id.clone();
        let callback: runtime::JavaProgress = Arc::new(move |stage, bytes, total_bytes| {
            let _ = events.try_send(UiEvent::InstallProgress(InstallProgressData {
                task_id: task.clone(),
                instance_id: instance_id.clone(),
                stage: "java".into(),
                done: 0,
                total: 0,
                bytes,
                total_bytes,
                message: stage.to_string(),
            }));
        });
        let java = runtime::ensure_java_with_source(
            &http,
            &self.paths,
            java_major(&launch_version),
            java_path,
            crate::core::mojang::version::game_arch(&launch_version),
            cancel,
            Some(callback),
            source,
        )
        .await?;
        tracing::info!(java = %java.path, version = java.major, "java runtime ready");
        if ["forge", "neoforge"].contains(&instance.loader.as_str()) {
            let events = self.events.clone();
            let task = task_id.to_string();
            let id = instance.id.clone();
            let report: runtime::JavaProgress = Arc::new(move |message, bytes, total_bytes| {
                let _ = events.try_send(UiEvent::InstallProgress(InstallProgressData {
                    task_id: task.clone(),
                    instance_id: id.clone(),
                    stage: "loader".into(),
                    done: 0,
                    total: 0,
                    bytes,
                    total_bytes,
                    message: message.into(),
                }));
            });
            let (merged, pinned) = crate::core::loaders::install(
                &http,
                &self.paths,
                &plan.version,
                &instance.loader,
                instance.loader_version.as_deref(),
                &java,
                cancel,
                report,
            )
            .await?;
            launch_version = merged;
            loader_version = Some(pinned);
        }
        self.progress(
            task_id,
            &instance.id,
            "finalize",
            snapshot.files_done,
            snapshot.files_total,
            snapshot.bytes_done,
            snapshot.bytes_total,
            crate::core::i18n::pick("正在完成安装", "Finishing the install"),
        );
        if cancel.is_cancelled() {
            return Err(AppError::new("cancelled", crate::core::i18n::pick("安装已取消。", "Install cancelled.")));
        }
        crate::core::util::atomic_write(
            &self
                .paths
                .instance_dir(&instance.id)
                .join("launch-version.json"),
            &serde_json::to_vec_pretty(&launch_version)?,
        )?;
        crate::core::modpacks::apply_overrides(&self.paths, instance)?;
        let mut completed = self.instances.get(&instance.id)?;
        completed.installed = true;
        completed.loader_version = loader_version;
        let optifine_marker = self.paths.instance_dir(&instance.id).join("optifine.json");
        if instance.loader == "vanilla" && std::fs::read(&optifine_marker).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()).is_some_and(|v| v["minecraft"].as_str() == Some(&instance.version_id)) {
            crate::core::optifine::install_vanilla(&http, &self.paths, &completed, cancel).await?;
        }
        self.instances.update(&completed)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn progress(
        &self,
        task_id: &str,
        instance_id: &str,
        stage: &str,
        done: u64,
        total: u64,
        bytes: u64,
        total_bytes: u64,
        message: &str,
    ) {
        self.send(UiEvent::InstallProgress(InstallProgressData {
            task_id: task_id.to_string(),
            instance_id: instance_id.to_string(),
            stage: stage.to_string(),
            done,
            total,
            bytes,
            total_bytes,
            message: message.to_string(),
        }));
    }

    async fn session_for_account(&self, account: &Account) -> AppResult<MinecraftSession> {
        match account.provider {
            AccountProvider::Offline => Ok(offline::session(account)),
            AccountProvider::Yggdrasil => {
                let tokens: yggdrasil::Tokens = keychain::load_refresh_token(&account.id)?
                    .and_then(|text| serde_json::from_str(&text).ok())
                    .ok_or_else(|| AppError::new("auth_expired", crate::core::i18n::pick("登录已过期，请在账号页重新登录。", "Sign-in expired; sign in again on the Accounts page.")))?;
                let (session, fresh) = yggdrasil::session(&self.http()?, account, &tokens).await?;
                if let Some(fresh) = fresh {
                    keychain::save_refresh_token(&account.id, &serde_json::to_string(&fresh)?)?;
                }
                Ok(session)
            }
            AccountProvider::Microsoft => {
                let settings = self.settings.get();
                let client_id =
                    microsoft::validate_client_id(&settings.effective_client_id())?.to_string();
                let refresh_token =
                    keychain::load_refresh_token(&account.id)?.ok_or_else(|| {
                        AppError::new("auth_expired", crate::core::i18n::pick("登录已过期，请重新登录微软账号。", "Sign-in expired; sign in to your Microsoft account again."))
                    })?;
                if let Some(session) = self.cached_session(&account.id) {
                    return Ok(session);
                }
                let http = self.http()?;
                let (updated, session, rotated, lifetime) =
                    microsoft::refresh_login(&http, &client_id, &refresh_token).await?;
                if let Some(rotated) = rotated {
                    keychain::save_refresh_token(&account.id, &rotated)?;
                }
                self.remember_session(&account.id, &session, lifetime);
                self.accounts.upsert(updated)?;
                self.send(UiEvent::AccountsChanged(self.accounts.list()));
                Ok(session)
            }
        }
    }

    /// The remembered session of a Microsoft account, while more than
    /// `SESSION_MARGIN` of it is left; after that it signs in again.
    fn cached_session(&self, account_id: &str) -> Option<MinecraftSession> {
        let mut sessions = lock(&self.sessions);
        match sessions.get(account_id) {
            Some((session, until)) if std::time::Instant::now() < *until => Some(session.clone()),
            Some(_) => {
                sessions.remove(account_id);
                None
            }
            None => None,
        }
    }

    fn remember_session(&self, account_id: &str, session: &MinecraftSession, lifetime: Duration) {
        if let Some(reusable) = lifetime.checked_sub(SESSION_MARGIN) {
            let until = std::time::Instant::now() + reusable;
            lock(&self.sessions).insert(account_id.to_string(), (session.clone(), until));
        }
    }

    async fn run_launch(
        self: Arc<Self>,
        instance_id: &str,
        instance: &Instance,
        settings: &Settings,
        account: &Account,
        target: LaunchTarget<'_>,
        offline: bool,
    ) -> AppResult<()> {
        let session = if offline {
            offline::session(account)
        } else {
            self.session_for_account(account).await.map_err(|err| offer_offline(account, err))?
        };
        let version = load_version(&self.paths, instance)?;
        if matches!(target, LaunchTarget::World(_)) && !launch::supports_quick_play(&version) {
            return Err(AppError::new(
                "quick_play",
                crate::core::i18n::pick("这个版本不支持直接进入存档，需要 Minecraft 1.20 或更高版本。", "This version can't open saves directly; that needs Minecraft 1.20 or later."),
            ));
        }
        let cancel = CancellationToken::new();
        let http = self.http()?;
        let arch = crate::core::mojang::version::game_arch(&version);
        let java = runtime::ensure_java_with_source(
            &http,
            &self.paths,
            java_major(&version),
            instance
                .java_path
                .as_deref()
                .or(settings.java_path.as_deref()),
            arch,
            &cancel,
            None,
            settings.download_source,
        )
        .await?;
        let launch_config = instance.launch.over(&settings.launch);
        let mut jvm_args = launch::split_args(&launch_config.jvm_args)?;
        if let Some(server_id) = account.server.as_deref().filter(|_| account.provider == AccountProvider::Yggdrasil).and_then(yggdrasil::nide8_server_id) {
            // 统一通行证 brings its own agent in place of authlib-injector.
            let agent = yggdrasil::ensure_nide8_agent(&http, &self.paths).await?;
            let mut agent_args = yggdrasil::nide8_args(&agent, server_id);
            agent_args.append(&mut jvm_args);
            jvm_args = agent_args;
        } else if account.provider == AccountProvider::Yggdrasil {
            // authlib-injector first, so the player's own arguments still come last.
            let agent = yggdrasil::ensure_agent(&http, &self.paths).await?;
            let metadata = match account.server.as_deref() {
                Some(api) => yggdrasil::metadata(&http, api).await,
                None => None,
            };
            let mut agent_args = yggdrasil::agent_args(&agent, account, metadata.as_deref());
            agent_args.append(&mut jvm_args);
            jvm_args = agent_args;
        }
        // An offline skin is cosmetic: if its server or the agent can't be
        // set up, the game still starts with the default skin.
        let local_skin = if account.provider == AccountProvider::Offline && account.skin_url.is_some() {
            let prepared = async {
                let server = crate::core::auth::local_skin::SkinServer::start(&self.paths, account)?;
                let agent = yggdrasil::ensure_agent(&http, &self.paths).await?;
                AppResult::Ok((server, agent))
            }
            .await;
            match prepared {
                Ok((server, agent)) => {
                    jvm_args.insert(0, format!("-javaagent:{}={}", agent.display(), server.url));
                    jvm_args.insert(1, "-Dauthlibinjector.noShowServerName=true".into());
                    Some(server)
                }
                Err(error) => {
                    tracing::warn!(error = %error, "offline skin unavailable; launching with the default skin");
                    None
                }
            }
        } else {
            None
        };
        let options = launch::LaunchOptions {
            arch: arch.to_string(),
            window: launch_config.window_width.zip(launch_config.window_height),
            jvm_args,
            game_args: launch::split_args(&launch_config.game_args)?,
        };
        let client_id = if account.provider == AccountProvider::Microsoft {
            settings.effective_client_id()
        } else {
            String::new()
        };
        let total_mb = platform::total_memory_mb();
        let mod_count = instance_mod_count(&self.paths, &instance.id);
        let memory_mb = resolve_memory_mb(
            instance.memory_mb,
            settings.memory_auto,
            settings.memory_mb,
            total_mb,
            mod_count,
        );
        tracing::info!(
            instance = %instance.id,
            memory_mb,
            automatic = instance.memory_mb.is_none() && settings.memory_auto,
            total_mb,
            mod_count,
            "resolved game memory"
        );
        let mut spec = launch::build_spec(
            &self.paths,
            instance,
            &version,
            &java,
            &session,
            &client_id,
            memory_mb,
            &options,
        )?;
        match target {
            LaunchTarget::Title => {}
            LaunchTarget::World(folder) => {
                spec.args.push("--quickPlaySingleplayer".to_string());
                spec.args.push(folder.to_string());
            }
            LaunchTarget::Server(address) if launch::supports_quick_play_multiplayer(&version) => {
                spec.args.push("--quickPlayMultiplayer".to_string());
                spec.args.push(address.to_string());
            }
            LaunchTarget::Server(address) => {
                // Older versions connect to exactly the host and port given,
                // so follow the SRV record here as the game's list would.
                let (host, port) = servers::resolve(&http, address).await?;
                spec.args.extend(["--server".to_string(), host, "--port".to_string(), port.to_string()]);
            }
        }
        // The game starts in English by itself, so only Chinese needs writing.
        if settings.auto_chinese && !crate::core::i18n::english() {
            match options::prepare_first_launch(&self.paths, &instance.id, &instance.version_id, "zh_cn") {
                Ok(true) => tracing::info!(instance = %instance.id, "set the game language to Chinese for the first launch"),
                Ok(false) => {}
                Err(error) => tracing::warn!(instance = %instance.id, error = %error, "could not prepare options.txt"),
            }
        }

        let log_dir = self.paths.instance_logs_dir(&instance.id);
        tokio::fs::create_dir_all(&log_dir).await?;
        let log_path = log_dir.join("latest.log");
        let header = format!(
            "[Islet] version={} memory={}MB java={} ({}, {}), user={}\n",
            instance.version_id, memory_mb, java.version, java.major, java.arch, session.username
        );
        tokio::fs::write(&log_path, header).await?;

        // The player's command before the game, in the game folder; a
        // failure stops the launch so its message is not missed.
        if !launch_config.pre_launch.trim().is_empty() {
            run_pre_launch(&launch_config.pre_launch, &spec.game_dir, &log_path).await?;
        }

        // A wrapper (gamemoderun, prime-run …) runs Java as its argument.
        let wrapper = launch::split_args(&launch_config.wrapper)?;
        let mut command = match wrapper.split_first() {
            Some((program, rest)) => {
                let mut command = Command::new(program);
                command.args(rest).arg(&spec.java_path);
                command
            }
            None => Command::new(&spec.java_path),
        };
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        command
            .args(&spec.args)
            .current_dir(&spec.game_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_remove("JAVA_TOOL_OPTIONS")
            .env_remove("_JAVA_OPTIONS")
            .env_remove("JDK_JAVA_OPTIONS")
            .envs(launch_config.env_pairs());
        if settings.dedicated_gpu {
            #[cfg(windows)]
            if let Err(error) = crate::core::platform::prefer_dedicated_gpu(&spec.java_path) {
                tracing::warn!(%error, "could not set the GPU preference for Java");
            }
            #[cfg(target_os = "linux")]
            if crate::core::platform::has_second_gpu() && !launch_config.env_pairs().iter().any(|(key, _)| key == "DRI_PRIME") {
                command.env("DRI_PRIME", "1");
            }
        }
        let mut child = command.spawn().map_err(|err| {
            AppError::detail(
                "launch",
                crate::core::i18n::pick("启动 Java 进程失败，请检查 Java 路径。", "Couldn't start the Java process; check the Java path."),
                err.to_string(),
            )
        })?;
        let pid = child
            .id()
            .ok_or_else(|| AppError::new("launch", crate::core::i18n::pick("无法获取 Minecraft 进程 ID。", "Couldn't get the Minecraft process ID.")))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let started = std::time::Instant::now();
        let started_ms = crate::core::util::now_millis();
        lock(&self.launches).insert(instance_id.to_string(), pid);
        if let Err(err) = self.instances.mark_launched(instance_id) {
            tracing::warn!(instance = %instance_id, error = %err, "failed to update last launch time");
        }
        self.send(UiEvent::LaunchStarted {
            instance_id: instance_id.to_string(),
            pid,
        });

        let mut readers = Vec::new();
        if let Some(stdout) = stdout {
            readers.push(self.spawn_log_reader(instance_id, stdout, "stdout", log_path.clone()));
        }
        if let Some(stderr) = stderr {
            readers.push(self.spawn_log_reader(instance_id, stderr, "stderr", log_path.clone()));
        }
        let post_exit = launch_config.post_exit.trim().to_string();
        let game_dir = spec.game_dir.clone();

        let this = self.clone();
        let id = instance_id.to_string();
        let id_for_hook = id.clone();
        tokio::spawn(async move {
            let _local_skin = local_skin;
            let status = child.wait().await;
            let code = status.ok().and_then(|status| status.code());
            // The child has exited, but the readers may still be draining
            // buffered stderr/stdout. Wait for them (bounded, in case a
            // grandchild inherited the pipe) before announcing the exit so a
            // crash dialog reads the complete log.
            drain_readers(readers, Duration::from_secs(2), Duration::from_secs(1)).await;
            lock(&this.launches).remove(&id);
            if let Err(err) = this.instances.add_play_time(&id, started.elapsed().as_secs()) {
                tracing::warn!(instance = %id, error = %err, "failed to record play time");
            }
            this.send(UiEvent::LaunchExit {
                instance_id: id,
                code,
                success: code == Some(0),
            });
            this.send(UiEvent::AccountsChanged(this.accounts.list()));
            let settings = this.settings.get();
            if settings.auto_backup {
                let (paths, id) = (this.paths.clone(), id_for_hook.clone());
                let keep = settings.auto_backup_keep as usize;
                match tokio::task::spawn_blocking(move || manage::auto_backup_saves(&paths, &id, started_ms, keep)).await {
                    Ok(Ok(saves)) if !saves.is_empty() => tracing::info!(instance = %id_for_hook, ?saves, "backed up played saves"),
                    Ok(Err(error)) => tracing::warn!(instance = %id_for_hook, %error, "automatic save backup failed"),
                    _ => {}
                }
            }
            // The player's command after the game; it can't undo anything,
            // so a failure is only written to the log.
            if !post_exit.is_empty() {
                if let Err(error) = run_post_exit(&post_exit, &game_dir, &log_path, code).await {
                    tracing::warn!(instance = %id_for_hook, %error, "post-exit command failed");
                }
            }
        });
        Ok(())
    }

    fn spawn_log_reader<R>(
        &self,
        instance_id: &str,
        reader: R,
        stream: &'static str,
        log_path: PathBuf,
    ) -> tokio::task::JoinHandle<()>
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let events = self.events.clone();
        let id = instance_id.to_string();
        tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            let mut buffer = Vec::new();
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .await
                .ok()
                .map(|file| tokio::io::BufWriter::with_capacity(64 * 1024, file));
            while let Some(line) = read_lossy_line(&mut reader, &mut buffer).await {
                if let Some(file) = &mut file {
                    // Line and newline in one write: stdout and stderr
                    // append to the same file.
                    let _ = file.write_all(format!("{line}\n").as_bytes()).await;
                    // A burst of output becomes one write; the file catches
                    // up as soon as the game pauses.
                    if reader.buffer().is_empty() {
                        let _ = file.flush().await;
                    }
                }
                let _ = events.try_send(UiEvent::LaunchLog {
                    instance_id: id.clone(),
                    line,
                    stream: stream.to_string(),
                });
            }
            if let Some(file) = &mut file {
                let _ = file.flush().await;
            }
        })
    }

    async fn finish_login(
        &self,
        login_id: &str,
        token: microsoft::OAuthToken,
        cancel: &CancellationToken,
    ) -> AppResult<Account> {
        let cancelled = || AppError::new("cancelled", crate::core::i18n::pick("登录已取消。", "Sign-in cancelled."));
        let http = self.http()?;
        // The Xbox and Minecraft exchanges take a few seconds; a cancel
        // during them stops here instead of waiting.
        let (account, session, refresh_token, lifetime) = tokio::select! {
            result = microsoft::complete_login(&http, token) => result?,
            _ = cancel.cancelled() => return Err(cancelled()),
        };
        // The last check before anything is stored. A cancel after this
        // point is too late; the interface then just shows the new account.
        // The login stays in the table until finish_login_result, so storage
        // is not moved while the account is being written.
        if cancel.is_cancelled() || !lock(&self.logins).contains_key(login_id) {
            return Err(cancelled());
        }
        if let Some(refresh_token) = refresh_token {
            keychain::save_refresh_token(&account.id, &refresh_token)?;
        }
        self.remember_session(&account.id, &session, lifetime);
        self.accounts.upsert(account.clone())?;
        self.settings.update(|settings| {
            settings.active_account_id = Some(account.id.clone());
        })?;
        self.send(UiEvent::LoginComplete {
            login_id: login_id.to_string(),
            account: account.clone(),
        });
        Ok(account)
    }

    fn finish_login_result(&self, login_id: &str, result: AppResult<Account>) {
        lock(&self.logins).remove(login_id);
        if let Err(error) = result {
            if error.code() != "cancelled" {
                tracing::warn!(code = error.code(), error = %error, "microsoft login failed");
            }
            self.send(UiEvent::LoginError {
                login_id: login_id.to_string(),
                error: error.to_payload(),
            });
        }
    }
}

/// Wait for the stdout/stderr readers to finish draining after the game has
/// exited. A grandchild can inherit a pipe and keep it open forever, so the
/// wait is bounded. On timeout the readers are aborted, not merely detached:
/// otherwise they would keep writing to the next launch's log and emit stale
/// log events. The abort wait is bounded too, so the exit event is not delayed
/// indefinitely.
async fn drain_readers(
    mut readers: Vec<tokio::task::JoinHandle<()>>,
    drain_timeout: Duration,
    abort_timeout: Duration,
) {
    let drained = {
        let wait = async {
            for reader in &mut readers {
                let _ = reader.await;
            }
        };
        tokio::time::timeout(drain_timeout, wait).await.is_ok()
    };
    if drained {
        return;
    }
    for reader in &readers {
        reader.abort();
    }
    let _ = tokio::time::timeout(abort_timeout, async {
        for reader in readers {
            let _ = reader.await;
        }
    })
    .await;
}

impl Drop for AppServices {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

/// A sign-in that failed for a reason that may pass (no network, Xbox or
/// Minecraft services down) becomes `auth_offline`, which the interface
/// answers with "play offline". Expired or invalid sign-ins stay as they are:
/// signing in again fixes those.
fn offer_offline(account: &Account, err: AppError) -> AppError {
    let passing = matches!(
        err.code(),
        "network" | "auth" | "auth_slow_down" | "xbox" | "minecraft_auth" | "minecraft_profile" | "yggdrasil"
    );
    if account.provider == AccountProvider::Offline || !passing {
        return err;
    }
    let reason = match err.detail_text() {
        Some(detail) => format!("{} ({detail})", err.user_message()),
        None => err.user_message(),
    };
    AppError::detail(
        "auth_offline",
        crate::core::i18n::pick(
            "连不上登录服务器。可以先以离线身份玩单人游戏，存档和进度都在；联机服务器要等网络恢复。",
            "Couldn't reach the sign-in servers. You can play singleplayer offline for now with your saves intact; online servers need the connection back.",
        ),
        reason,
    )
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Read at most `max_bytes` from the end of a file. Returns the lossy text and
/// the file's modification time in milliseconds. When the read starts
/// mid-file the first partial line is dropped.
fn read_tail_text(path: &Path, max_bytes: u64) -> AppResult<Option<(String, i64)>> {
    use std::io::{Read, Seek, SeekFrom};
    use std::time::UNIX_EPOCH;
    let Ok(mut file) = std::fs::File::open(path) else {
        return Ok(None);
    };
    let metadata = file.metadata()?;
    let len = metadata.len();
    let start = len.saturating_sub(max_bytes);
    let mut bytes = Vec::new();
    file.seek(SeekFrom::Start(start))?;
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let text = if start > 0 {
        text.split_once('\n').map_or("", |(_, rest)| rest).to_string()
    } else {
        text.into_owned()
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    Ok(Some((text, modified)))
}

/// The analysis text for the current run: the launcher header (it records
/// `memory=`) plus the log tail. Reading only the tail loses the header once
/// the log grows past the tail window; only the header is taken from the
/// head, so early errors outside the tail stay outside the analysis window.
fn analysis_log_text(path: &Path, max_bytes: u64) -> AppResult<Option<(String, i64)>> {
    const HEAD_BYTES: u64 = 16 * 1024;
    let Some((tail, modified)) = read_tail_text(path, max_bytes)? else {
        return Ok(None);
    };
    let head = read_head_text(path, HEAD_BYTES).unwrap_or_default();
    let Some(header) = launcher_header(&head) else {
        return Ok(Some((tail, modified)));
    };
    if tail.contains(header) {
        return Ok(Some((tail, modified)));
    }
    let mut text = String::with_capacity(header.len() + tail.len() + 1);
    text.push_str(header);
    text.push('\n');
    text.push_str(&tail);
    Ok(Some((text, modified)))
}

/// The `[Islet] ... memory=...` line written at the start of each run.
/// Logs from before the rename start with `[MC Launcher]`.
fn launcher_header(head: &str) -> Option<&str> {
    head.lines().find(|line| {
        let lower = line.to_ascii_lowercase();
        (lower.starts_with("[islet]") || lower.starts_with("[mc launcher]")) && lower.contains("memory=")
    })
}

fn read_head_text(path: &Path, max_bytes: u64) -> AppResult<String> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(max_bytes).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The newest `<prefix>*.<extension>` file in `dir` written since the launch.
fn latest_crash_report(dir: &Path, since_ms: i64, extension: &str, prefix: &str) -> AppResult<Option<PathBuf>> {
    use std::time::UNIX_EPOCH;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(None);
    };
    let mut newest: Option<(i64, PathBuf)> = None;
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path();
        if !path
            .extension()
            .and_then(|found| found.to_str())
            .is_some_and(|found| found.eq_ignore_ascii_case(extension))
            || !entry.file_name().to_string_lossy().starts_with(prefix)
        {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);
        if modified < since_ms {
            continue;
        }
        if newest.as_ref().is_none_or(|(current, _)| modified > *current) {
            newest = Some((modified, path));
        }
    }
    Ok(newest.map(|(_, path)| path))
}

/// A compact report: up to 20 non-empty crash-report lines (the description
/// and top of the stack) plus the tail of the current launch log. Never more
/// than 60 lines.
fn build_excerpt(log: &str, crash_report: Option<&str>) -> String {
    const MAX_LINES: usize = 60;
    const REPORT_LINES: usize = 20;
    let mut lines: Vec<&str> = Vec::new();
    if let Some(report) = crash_report {
        lines.extend(
            report
                .lines()
                .filter(|line| !line.trim().is_empty())
                .take(REPORT_LINES),
        );
    }
    let log_lines: Vec<&str> = log
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let remaining = MAX_LINES.saturating_sub(lines.len());
    let start = log_lines.len().saturating_sub(remaining);
    lines.extend(log_lines[start..].iter().copied());
    lines.truncate(MAX_LINES);
    lines.join("\n")
}

/// Count the enabled jars in an instance's mods directory. `.jar.disabled`
/// files are not loaded, so they do not influence the heap recommendation.
/// `.jar` files in the instance's mods folder; disabled mods do not count.
/// A player's command for the system shell, run in the game folder.
fn shell_command(command: &str, dir: &Path) -> Command {
    let mut shell = if cfg!(windows) {
        let mut shell = Command::new("cmd");
        shell.arg("/C").arg(command);
        shell
    } else {
        let mut shell = Command::new("/bin/sh");
        shell.arg("-c").arg(command);
        shell
    };
    #[cfg(windows)]
    shell.creation_flags(0x08000000);
    shell.current_dir(dir).stdin(Stdio::null()).kill_on_drop(true);
    shell
}

/// Runs the post-exit command and waits for it, up to five minutes. The
/// game's exit code is in `MC_EXIT_CODE`; output goes to the game log.
async fn run_post_exit(command: &str, dir: &Path, log: &Path, code: Option<i32>) -> AppResult<()> {
    let mut shell = shell_command(command, dir);
    shell.env("MC_EXIT_CODE", code.map_or_else(String::new, |c| c.to_string()));
    let output = tokio::time::timeout(Duration::from_secs(300), shell.output())
        .await
        .map_err(|_| AppError::new("post_exit", crate::core::i18n::pick("退出后命令超过 5 分钟没有结束，已停止。", "The post-exit command ran over 5 minutes and was stopped.")))?
        .map_err(|error| AppError::detail("post_exit", crate::core::i18n::pick("退出后命令无法运行。", "The post-exit command couldn't run."), error.to_string()))?;
    let mut text = format!("[Islet] post-exit: {command} (exit {})\n", output.status.code().map_or("?".to_string(), |c| c.to_string()));
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let mut file = tokio::fs::OpenOptions::new().append(true).open(log).await?;
    file.write_all(text.as_bytes()).await?;
    Ok(())
}

/// Runs the pre-launch command through the system shell and waits for it,
/// up to five minutes. Its output goes to the game log.
async fn run_pre_launch(command: &str, dir: &Path, log: &Path) -> AppResult<()> {
    let mut shell = shell_command(command, dir);
    let output = tokio::time::timeout(Duration::from_secs(300), shell.output())
        .await
        .map_err(|_| AppError::new("pre_launch", crate::core::i18n::pick("启动前命令超过 5 分钟没有结束，已停止启动。", "The pre-launch command ran over 5 minutes; the launch was stopped.")))?
        .map_err(|error| AppError::detail("pre_launch", crate::core::i18n::pick("启动前命令无法运行。", "The pre-launch command couldn't run."), error.to_string()))?;
    let mut text = format!("[Islet] pre-launch: {command}\n");
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let mut file = tokio::fs::OpenOptions::new().append(true).open(log).await?;
    file.write_all(text.as_bytes()).await?;
    if !output.status.success() {
        return Err(AppError::new(
            "pre_launch",
            crate::tr!("启动前命令失败（退出码 {}），游戏没有启动。输出在游戏日志里。", "The pre-launch command failed (exit code {}), so the game didn't start. Its output is in the game log.", output.status.code().map_or(crate::core::i18n::pick("未知", "unknown").into(), |c| c.to_string())),
        ));
    }
    Ok(())
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

/// Instance memory wins; otherwise automatic mode uses the machine-aware
/// recommendation and manual mode uses the global value.
fn resolve_memory_mb(
    instance_memory: Option<u32>,
    memory_auto: bool,
    global_memory: u32,
    total_mb: u64,
    mod_count: usize,
) -> u32 {
    match instance_memory {
        Some(memory) => memory,
        None if memory_auto => platform::recommended_memory_mb(total_mb, mod_count),
        None => global_memory,
    }
}

/// Read one line without requiring UTF-8. Windows JVMs often print in the ANSI
/// code page (GBK on Chinese systems); a strict reader stops at the first such
/// line and every later log line is lost.
async fn read_lossy_line<R>(reader: &mut R, buffer: &mut Vec<u8>) -> Option<String>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    buffer.clear();
    match reader.read_until(b'\n', buffer).await {
        Ok(0) | Err(_) => None,
        Ok(_) => {
            while matches!(buffer.last(), Some(b'\n' | b'\r')) {
                buffer.pop();
            }
            Some(String::from_utf8_lossy(buffer).into_owned())
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn account(provider: AccountProvider) -> Account {
        Account {
            id: "microsoft:test".into(),
            provider,
            username: "Steve".into(),
            uuid: "00000000-0000-0000-0000-000000000001".into(),
            xuid: None,
            ownership: Some(true),
            skin_url: None,
            skin_slim: false,
            created_at: 0,
            server: None,
            server_name: None,
        }
    }

    #[test]
    fn unreachable_sign_in_offers_offline_play() {
        let microsoft = account(AccountProvider::Microsoft);
        let down = AppError::detail("xbox", "Xbox 授权失败", "HTTP 503");
        assert_eq!(offer_offline(&microsoft, down).code(), "auth_offline");
        // Signing in again fixes these; playing offline would hide that.
        for code in ["auth_expired", "not_owned", "client_id", "keychain"] {
            assert_eq!(offer_offline(&microsoft, AppError::new(code, "x")).code(), code);
        }
        let offline = account(AccountProvider::Offline);
        assert_eq!(offer_offline(&offline, AppError::new("xbox", "x")).code(), "xbox");
    }

    #[tokio::test]
    async fn log_reader_keeps_reading_after_non_utf8_line() {
        // "中文" in GBK, as a Chinese-locale Windows JVM prints it, then CRLF.
        let input: &[u8] = b"first\n\xd6\xd0\xce\xc4\r\nafter\n";
        let mut reader = BufReader::new(input);
        let mut buffer = Vec::new();
        let mut lines = Vec::new();
        while let Some(line) = read_lossy_line(&mut reader, &mut buffer).await {
            lines.push(line);
        }
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "first");
        assert!(!lines[1].ends_with('\r'));
        assert_eq!(lines[2], "after");
    }

    #[test]
    fn instance_memory_overrides_automatic_and_manual_settings() {
        assert_eq!(
            resolve_memory_mb(Some(3072), true, 4096, 16384, 151),
            3072
        );
        assert_eq!(resolve_memory_mb(None, true, 4096, 16384, 0), 4096);
        assert_eq!(resolve_memory_mb(None, false, 4096, 16384, 151), 4096);
        assert_eq!(resolve_memory_mb(None, false, 6144, 16384, 0), 6144);
    }

    fn crash_services() -> (tempfile::TempDir, Arc<AppServices>, String) {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(root.path()).unwrap();
        let (events, _receiver) = async_channel::unbounded();
        let services = AppServices::new(paths, events).unwrap();
        let instance = services.instances.create("崩溃测试", "1.21", "release").unwrap();
        (root, services, instance.id)
    }

    #[test]
    fn crash_analysis_reads_the_current_launcher_log_and_honours_since() {
        let (_root, services, id) = crash_services();
        let log = services
            .paths
            .instance_logs_dir(&id)
            .join("latest.log");
        std::fs::write(
            &log,
            "java.lang.UnsupportedClassVersionError: example/Main has been compiled by a more recent version of the Java Runtime (class file version 65.0)\n",
        )
        .unwrap();

        let current = services.analyze_crash(&id, 0).unwrap();
        assert_eq!(current.diagnoses[0].title, "需要 Java 21");

        let stale = services
            .analyze_crash(&id, crate::core::util::now_millis() + 60_000)
            .unwrap();
        assert!(stale.diagnoses.is_empty());
        assert!(stale.excerpt.is_empty());
    }

    #[test]
    fn crash_analysis_only_uses_crash_reports_written_after_the_launch() {
        let (_root, services, id) = crash_services();
        let reports = services
            .paths
            .instance_game_dir(&id)
            .join("crash-reports");
        std::fs::create_dir_all(&reports).unwrap();
        std::fs::write(
            reports.join("crash-2026-10-28_12.00.00-client.txt"),
            "---- Minecraft Crash Report ----\nDescription: Initializing game\njava.lang.OutOfMemoryError: Java heap space\n",
        )
        .unwrap();

        let current = services.analyze_crash(&id, 0).unwrap();
        assert_eq!(current.diagnoses[0].title, "内存不足");

        let stale = services
            .analyze_crash(&id, crate::core::util::now_millis() + 60_000)
            .unwrap();
        assert!(stale.diagnoses.is_empty());
    }

    #[test]
    fn crash_analysis_reads_fresh_native_vm_reports() {
        let (_root, services, id) = crash_services();
        let game = services.paths.instance_game_dir(&id);
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("hs_err_pid123.log"), "# A fatal error has been detected by the Java Runtime Environment:\n# SIGSEGV\n# Problematic frame:\n# C  [nvoglv64.dll+0x1234]\n").unwrap();
        let analysis = services.analyze_crash(&id, 0).unwrap();
        assert!(!analysis.diagnoses.is_empty());
        assert!(analysis.excerpt.contains("nvoglv64"));
        assert!(services.analyze_crash(&id, crate::core::util::now_millis() + 60_000).unwrap().diagnoses.is_empty());
    }

    #[test]
    fn crash_analysis_reads_only_the_log_tail_and_caps_the_excerpt() {
        let (_root, services, id) = crash_services();
        let log = services
            .paths
            .instance_logs_dir(&id)
            .join("latest.log");
        let mut content = String::from(
            "java.lang.UnsupportedClassVersionError: old/Main (class file version 65.0)\n",
        );
        // Larger than the 512 KiB log window, so the first error must fall
        // outside and cannot be reported again for this launch.
        content.push_str(&"x".repeat(600 * 1024));
        content.push_str("\njava.lang.OutOfMemoryError: Java heap space\n");
        std::fs::write(&log, content).unwrap();

        let analysis = services.analyze_crash(&id, 0).unwrap();
        assert_eq!(analysis.diagnoses.len(), 1);
        assert_eq!(analysis.diagnoses[0].title, "内存不足");
        assert!(analysis.excerpt.lines().count() <= 60);
        assert!(!analysis.excerpt.contains("UnsupportedClassVersionError"));
    }

    #[test]
    fn crash_analysis_picks_the_newest_fresh_crash_report() {
        let (_root, services, id) = crash_services();
        let reports = services
            .paths
            .instance_game_dir(&id)
            .join("crash-reports");
        std::fs::create_dir_all(&reports).unwrap();
        std::fs::write(
            reports.join("crash-older.txt"),
            "Error: Could not find or load main class net.minecraft.client.main.Main\n",
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        std::fs::write(
            reports.join("crash-newer.txt"),
            "java.lang.OutOfMemoryError: Java heap space\n",
        )
        .unwrap();

        let analysis = services.analyze_crash(&id, 0).unwrap();
        assert_eq!(analysis.diagnoses.len(), 1);
        assert_eq!(analysis.diagnoses[0].title, "内存不足");
    }

    #[test]
    fn excerpt_prefers_crash_report_header_and_then_log_tail() {
        let report = (0..30)
            .map(|index| format!("report line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let log = (0..100)
            .map(|index| format!("log line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let excerpt = build_excerpt(&log, Some(&report));
        let lines: Vec<&str> = excerpt.lines().collect();
        assert_eq!(lines.len(), 60);
        assert_eq!(lines[0], "report line 0");
        assert_eq!(lines[19], "report line 19");
        assert_eq!(lines[20], "log line 60");
        assert_eq!(lines[59], "log line 99");
    }

    #[test]
    fn mod_count_ignores_disabled_jars_and_other_files() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(root.path()).unwrap();
        let mods = paths
            .instance_game_dir("00000000-0000-0000-0000-000000000000")
            .join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        for name in ["sodium.jar", "lithium.JAR", "iris.jar.disabled", "notes.txt"] {
            std::fs::write(mods.join(name), b"x").unwrap();
        }
        std::fs::create_dir_all(mods.join("folder.jar")).unwrap();
        assert_eq!(
            instance_mod_count(&paths, "00000000-0000-0000-0000-000000000000"),
            2
        );
        assert_eq!(instance_mod_count(&paths, "missing"), 0);
    }

    #[test]
    fn launcher_header_reads_logs_from_before_the_rename() {
        let old = "[MC Launcher] version=1.21 memory=2560MB java=21\n[12:00:00] Booting";
        assert_eq!(launcher_header(old), old.lines().next());
        let new = "[Islet] version=1.21 memory=2560MB java=21\n";
        assert_eq!(launcher_header(new), new.lines().next());
        assert_eq!(launcher_header("[Islet] post-exit: x (exit 0)\n"), None);
    }

    #[test]
    fn crash_analysis_keeps_the_memory_header_outside_the_log_tail() {
        let (_root, services, id) = crash_services();
        let log = services
            .paths
            .instance_logs_dir(&id)
            .join("latest.log");
        let mut content = String::from(
            "[Islet] version=1.21 memory=2560MB java=21.0.2 (21), user=Steve\n",
        );
        // Push the header outside the 512 KiB tail window.
        content.push_str(&"x".repeat(600 * 1024));
        content.push_str("\njava.lang.OutOfMemoryError: Java heap space\n");
        std::fs::write(&log, content).unwrap();

        let (tail, _) = read_tail_text(&log, 512 * 1024).unwrap().unwrap();
        assert!(!tail.contains("memory=2560MB"));

        let (text, _) = analysis_log_text(&log, 512 * 1024).unwrap().unwrap();
        assert!(text.contains("memory=2560MB"));
        assert!(text.contains("Java heap space"));

        let analysis = services.analyze_crash(&id, 0).unwrap();
        assert_eq!(analysis.diagnoses.len(), 1);
        let expected = crash::clamp_memory_mb(4096, platform::total_memory_mb());
        if expected > 2560 {
            assert_eq!(
                analysis.diagnoses[0].fix,
                Some(crate::core::crash::Fix::RaiseMemory { mb: expected })
            );
            assert!(
                analysis.diagnoses[0]
                    .detail
                    .contains(&format!("{expected} MB"))
            );
        } else {
            assert_eq!(
                analysis.diagnoses[0].fix,
                Some(crate::core::crash::Fix::None)
            );
        }
    }

    #[tokio::test]
    async fn drain_readers_aborts_a_reader_whose_pipe_never_closes() {
        use std::sync::atomic::{AtomicBool, Ordering};

        struct MarkOnDrop(Arc<AtomicBool>);
        impl Drop for MarkOnDrop {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let stopped = Arc::new(AtomicBool::new(false));
        let flag = stopped.clone();
        let reader = tokio::spawn(async move {
            let _mark = MarkOnDrop(flag);
            std::future::pending::<()>().await;
        });

        let started = std::time::Instant::now();
        drain_readers(
            vec![reader],
            Duration::from_millis(20),
            Duration::from_secs(1),
        )
        .await;
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(stopped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn drain_readers_waits_for_a_normal_reader() {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let reader = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = sender.send(());
        });
        drain_readers(
            vec![reader],
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .await;
        assert!(receiver.await.is_ok());
    }
}
