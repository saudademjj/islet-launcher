//! Commands the web UI calls through `invoke`. Mutations return a fresh
//! snapshot so the UI keeps a single copy of settings, accounts and instances;
//! long work runs on the service runtime and reports back through events.
mod accounts;
mod content;
mod instances;
mod network;

pub use accounts::*;
pub use content::*;
pub use instances::*;
pub use network::*;

use std::path::PathBuf;

use serde::Deserialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use islet::core::content::{self as core_content, ContentKind};
use islet::core::error::{AppError, ErrorPayload};
use islet::core::manage;
use islet::core::platform;
use islet::core::screenshots;
use islet::core::settings::Settings;
use islet::services::LocalSnapshot;

use crate::LauncherState;

pub(super) type CmdResult<T> = Result<T, ErrorPayload>;

pub(super) fn fail(error: AppError) -> ErrorPayload {
    error.to_payload()
}

/// Run blocking file work off the async runtime so other commands stay live.
pub(super) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|error| fail(AppError::detail("runtime", islet::core::i18n::pick("后台任务失败。", "A background task failed."), error.to_string())))?
        .map_err(fail)
}

/// The interface language as resolved by the page (system follows the
/// WebView's locale); the backend writes its messages in it.
#[tauri::command]
pub fn set_language(english: bool) {
    islet::core::i18n::set_english(english);
}

/// Quits even though work is still running (the user confirmed it).
#[tauri::command]
pub fn quit_launcher(app: AppHandle) {
    use tauri::Manager;
    if app.state::<LauncherState>().services().running_instance_ids().is_empty() {
        crate::quit(&app);
    } else {
        // Even if the page has stale state, do not detach running games and
        // lose their logs. The tray can stop them or cancel the scheduled exit.
        crate::schedule_quit(&app);
    }
}

#[tauri::command]
pub fn quit_when_idle(app: AppHandle) {
    crate::schedule_quit(&app);
}

/// Read once by the page as it starts; later changes arrive as events.
#[tauri::command]
pub fn quit_scheduled() -> bool {
    crate::quit_is_scheduled()
}

#[tauri::command]
pub fn cancel_scheduled_quit(app: AppHandle) {
    crate::keep_running(&app);
}

/// The page reports unsaved edits, so quitting from the tray, the window or
/// the app menu asks about them instead of dropping them.
#[tauri::command]
pub fn set_unsaved_changes(unsaved: bool) {
    crate::set_unsaved_changes(unsaved);
}

/// Quits the way the tray does, once the page has saved or discarded its
/// edits; work still running asks again.
#[tauri::command]
pub fn request_quit(app: AppHandle, unsaved: bool) {
    crate::set_unsaved_changes(unsaved);
    crate::request_quit(&app);
}

#[tauri::command]
pub async fn bootstrap(state: State<'_, LauncherState>) -> CmdResult<LocalSnapshot> {
    Ok(state.services().snapshot())
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemMemory {
    pub total_mb: u64,
    pub recommended_mb: u32,
}

/// Physical memory and the automatic allocation. With an instance id the
/// recommendation counts that instance's mods, exactly as a launch would.
#[tauri::command]
pub async fn system_memory(
    state: State<'_, LauncherState>,
    id: Option<String>,
) -> CmdResult<SystemMemory> {
    let total_mb = platform::total_memory_mb();
    let mod_count = match id {
        Some(id) => {
            islet::core::paths::AppPaths::validate_instance_id(&id).map_err(fail)?;
            islet::services::instance_mod_count(&state.services().paths, &id)
        }
        None => 0,
    };
    Ok(SystemMemory {
        total_mb,
        recommended_mb: platform::recommended_memory_mb(total_mb, mod_count),
    })
}

/// Page errors (and the dev self-test) end up in the launcher log.
#[tauri::command]
pub fn frontend_log(level: String, message: String) {
    if level == "error" {
        tracing::error!(target: "web", "{message}");
    } else {
        tracing::info!(target: "web", "{message}");
    }
}

#[tauri::command]
pub async fn update_settings(
    state: State<'_, LauncherState>,
    settings: Settings,
) -> CmdResult<LocalSnapshot> {
    let services = state.services();
    let old_discord_id = services.settings.get().discord_application_id;
    let new_discord_id = settings.discord_application_id.clone();
    services.update_settings(settings).map_err(fail)?;
    if old_discord_id != new_discord_id {
        let playing = services.running_instance_ids().first().and_then(|id| services.instances.get(id).ok());
        crate::presence::set(&new_discord_id, playing.as_ref().map(|i| (i.name.as_str(), i.version_id.as_str())));
    }
    Ok(services.snapshot())
}

#[tauri::command]
pub async fn storage_usage(state: State<'_, LauncherState>) -> CmdResult<manage::StorageUsage> {
    let services = state.services();
    blocking(move || Ok(services.storage_usage())).await
}

#[tauri::command]
pub async fn relocate_storage(state: State<'_, LauncherState>, destination: String) -> CmdResult<()> {
    state
        .services()
        .relocate(PathBuf::from(destination))
        .map_err(fail)
}

/// A newer launcher release, when an update source is built in.
#[tauri::command]
pub async fn check_update(state: State<'_, LauncherState>) -> CmdResult<Option<islet::core::update::Release>> {
    let services = state.services();
    let client = services.http_client().map_err(fail)?;
    islet::core::update::check(&client, env!("CARGO_PKG_VERSION")).await.map_err(fail)
}

/// Downloads this computer's package of the newer release, checks its
/// SHA-256 and opens it: the disk image on macOS, the installer on Windows,
/// the new AppImage on Linux (the launcher then quits so its files can be
/// replaced). Other Linux packages are shown in their folder.
#[tauri::command]
pub async fn install_update(app: AppHandle, state: State<'_, LauncherState>) -> CmdResult<String> {
    use islet::core::i18n::pick;
    let services = state.services();
    if services.activity() != islet::services::Activity::Idle { return Err(fail(AppError::new("update", pick("请先结束游戏和后台任务再更新。", "Finish games and background tasks before updating.")))); }
    let client = services.http_client().map_err(fail)?;
    // The address comes from the manifest again, never from the page.
    let release = islet::core::update::check(&client, env!("CARGO_PKG_VERSION"))
        .await
        .map_err(fail)?
        .ok_or_else(|| fail(AppError::new("update", pick("已经是最新版本。", "You're on the latest version."))))?;
    let package = release
        .package
        .ok_or_else(|| fail(AppError::new("update", pick("这个版本没有可以直接安装的更新包，请从下载页获取。", "This release has no package to install directly; get it from the download page."))))?;
    let file = islet::core::update::download(&client, &services.paths, &package).await.map_err(fail)?;
    let failed = |e: String| fail(AppError::detail("update", pick("无法打开更新包。", "Couldn't open the update package."), e));
    if services.activity() != islet::services::Activity::Idle { return Err(fail(AppError::new("update", pick("更新包已下载；请先结束游戏和后台任务再安装。", "Update downloaded; finish games and background tasks before installing.")))); }
    #[cfg(windows)]
    {
        if file.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("msi")) {
            std::process::Command::new("msiexec.exe").arg("/i").arg(&file).spawn().map_err(|e| failed(e.to_string()))?;
        } else if file.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("exe")) {
            std::process::Command::new(&file).spawn().map_err(|e| failed(e.to_string()))?;
        } else {
            return Err(fail(AppError::new("update", pick("更新包不是 Windows 安装程序。", "This package isn't a Windows installer."))));
        }
        crate::quit(&app);
        Ok(String::new())
    }
    #[cfg(not(windows))]
    {
        #[cfg(target_os = "macos")]
        {
            if file.extension().is_some_and(|ext| ext == "dmg") {
                let mut helper = crate::self_update::prepare(&file, &services.paths.cache).await.map_err(&failed)?;
                if services.activity() != islet::services::Activity::Idle { return Err(fail(AppError::new("update", pick("请先结束游戏和后台任务再安装更新。", "Finish games and background tasks before installing.")))); }
                helper.spawn().map_err(|e| failed(e.to_string()))?;
                crate::quit(&app);
                return Ok(String::new());
            }
        }
        #[cfg(target_os = "linux")]
        {
            if file.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("appimage")) && std::env::var_os("APPIMAGE").is_some() {
                let mut helper = crate::self_update::prepare_appimage(&file).map_err(&failed)?;
                helper.spawn().map_err(|e| failed(e.to_string()))?;
                crate::quit(&app);
                return Ok(String::new());
            }
        }
        app.opener().reveal_item_in_dir(&file).map_err(|e| failed(e.to_string()))?;
        Ok(pick("更新包已下载，已在文件夹中显示。", "Update downloaded and shown in its folder.").into())
    }
}

#[tauri::command]
pub async fn update_source() -> CmdResult<bool> {
    Ok(islet::core::update::source().is_some())
}

#[tauri::command]
pub async fn patch_notes(state: State<'_, LauncherState>) -> CmdResult<Vec<islet::core::news::PatchNote>> {
    let client = state.services().http_client().map_err(fail)?;
    islet::core::news::patch_notes(&client, 6).await.map_err(fail)
}

/// Folders the UI may reveal; paths are resolved here so the web side never
/// passes arbitrary locations to the system opener.
#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Location {
    DataDir,
    RecycleDir,
    GameDir { id: String },
    LogsDir { id: String },
    BackupsDir { id: String },
    World { id: String, folder: String },
    /// The mods, resourcepacks or shaderpacks folder; created when missing.
    ContentDir { id: String, content: ContentKind },
    ScreenshotsDir { id: String },
    /// One screenshot, opened in the system's image viewer.
    Screenshot { id: String, name: String },
    /// The pack friends import to join a world's server.
    ServerPack { id: String },
}

#[tauri::command]
pub async fn open_location(
    app: AppHandle,
    state: State<'_, LauncherState>,
    location: Location,
) -> CmdResult<()> {
    let services = state.services();
    let paths = &services.paths;
    let (path, label) = match location {
        Location::DataDir => (paths.root.clone(), islet::core::i18n::pick("数据目录", "Data folder")),
        Location::RecycleDir => (paths.root.join("recycle"), islet::core::i18n::pick("回收站目录", "Recycle folder")),
        Location::GameDir { id } => {
            let dirs = services.instance_paths(&id).map_err(fail)?;
            (PathBuf::from(dirs.game_dir), islet::core::i18n::pick("游戏目录", "Game folder"))
        }
        Location::LogsDir { id } => {
            let dirs = services.instance_paths(&id).map_err(fail)?;
            (PathBuf::from(dirs.log_dir), islet::core::i18n::pick("日志目录", "Logs folder"))
        }
        Location::BackupsDir { id } => {
            services.instance_paths(&id).map_err(fail)?;
            (paths.root.join("backups").join(id), islet::core::i18n::pick("备份目录", "Backups folder"))
        }
        Location::World { id, folder } => {
            services.instance_paths(&id).map_err(fail)?;
            (core_content::world_dir(paths, &id, &folder).map_err(fail)?, islet::core::i18n::pick("存档目录", "Save folder"))
        }
        Location::ContentDir { id, content } => {
            let dirs = services.instance_paths(&id).map_err(fail)?;
            let dir = PathBuf::from(dirs.game_dir).join(content.folder());
            std::fs::create_dir_all(&dir)
                .map_err(|error| fail(AppError::detail("open", islet::core::i18n::pick("无法创建目录。", "Couldn't create the folder."), error.to_string())))?;
            (dir, content.label())
        }
        Location::ScreenshotsDir { id } => {
            services.instance_paths(&id).map_err(fail)?;
            (screenshots::folder(paths, &id).map_err(fail)?, islet::core::i18n::pick("截图目录", "Screenshots folder"))
        }
        Location::Screenshot { id, name } => {
            services.instance_paths(&id).map_err(fail)?;
            (screenshots::file(paths, &id, &name).map_err(fail)?, islet::core::i18n::pick("截图", "Screenshot"))
        }
        Location::ServerPack { id } => {
            services.instance_paths(&id).map_err(fail)?;
            (paths.instance_dir(&id).join("server-pack"), islet::core::i18n::pick("好友用的整合包", "The pack for friends"))
        }
    };
    if !path.exists() {
        return Err(fail(AppError::new("not_found", islet::tr!("{label}还不存在", "{label} doesn't exist yet"))));
    }
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|error| fail(AppError::detail("open", islet::core::i18n::pick("无法打开目录。", "Couldn't open the folder."), error.to_string())))
}

/// Only web links (Microsoft login, Azure portal) leave the app.
#[tauri::command]
pub async fn open_url(app: AppHandle, url: String) -> CmdResult<()> {
    if !islet::core::util::is_safe_web_url(&url) {
        return Err(fail(AppError::new("open", islet::core::i18n::pick("不支持打开这个链接。", "This link can't be opened."))));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| fail(AppError::detail("open", islet::core::i18n::pick("无法打开链接。", "Couldn't open the link."), error.to_string())))
}
