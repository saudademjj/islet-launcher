//! Instance lifecycle, Java runtimes, launch/install, logs, crash analysis, and game options commands.
use std::path::PathBuf;

use tauri::State;

use islet::core::error::AppError;
use islet::core::instances::{Instance, LaunchConfig};
use islet::core::options;
use islet::services::{CrashAnalysis, LocalSnapshot};

use crate::LauncherState;

use super::{CmdResult, blocking, fail};

/// Create, apply the chosen loader, and make the new instance current.
#[tauri::command]
pub async fn create_instance(
    state: State<'_, LauncherState>,
    name: String,
    version_id: String,
    version_type: String,
    loader: String,
    loader_version: Option<String>,
) -> CmdResult<LocalSnapshot> {
    let services = state.services();
    let instance = services
        .create_instance(name.trim(), &version_id, &version_type)
        .map_err(fail)?;
    if loader != "vanilla" {
        services
            .configure_instance(&instance.id, &instance.name, None, None, &loader)
            .map_err(fail)?;
    }
    services
        .instances
        .configure_loader_version(&instance.id, loader_version)
        .map_err(fail)?;
    services
        .set_active_instance(Some(instance.id))
        .map_err(fail)?;
    Ok(services.snapshot())
}

/// A new server project; the snapshot and the project's id.
#[tauri::command]
pub async fn create_server_project(state: State<'_, LauncherState>, name: String, version_id: String, loader: String) -> CmdResult<(LocalSnapshot, String)> {
    let services = state.services();
    let project = services.create_server_project(&name, &version_id, &loader).map_err(fail)?;
    Ok((services.snapshot(), project.id))
}

#[tauri::command]
pub async fn configure_server_project(
    state: State<'_, LauncherState>,
    id: String,
    name: String,
    version_id: String,
    loader: String,
    project: islet::core::instances::ServerProject,
) -> CmdResult<LocalSnapshot> {
    let services = state.services();
    services.configure_server_project(&id, &name, &version_id, &loader, project).map_err(fail)?;
    Ok(services.snapshot())
}

#[tauri::command]
pub async fn server_versions(state: State<'_, LauncherState>, loader: String) -> CmdResult<Vec<String>> {
    state.services().server_versions(&loader).await.map_err(fail)
}

#[tauri::command]
pub async fn delete_instance(state: State<'_, LauncherState>, id: String) -> CmdResult<LocalSnapshot> {
    let services = state.services();
    services.delete_instance(&id).map_err(fail)?;
    Ok(services.snapshot())
}

#[tauri::command]
pub async fn set_active_instance(
    state: State<'_, LauncherState>,
    id: Option<String>,
) -> CmdResult<LocalSnapshot> {
    let services = state.services();
    services.set_active_instance(id).map_err(fail)?;
    Ok(services.snapshot())
}

#[tauri::command]
pub async fn configure_instance(
    state: State<'_, LauncherState>,
    id: String,
    name: String,
    memory_mb: Option<u32>,
    java_path: Option<String>,
    loader: String,
    loader_version: Option<String>,
) -> CmdResult<LocalSnapshot> {
    let services = state.services();
    services
        .configure_instance(&id, name.trim(), memory_mb, java_path, &loader)
        .map_err(fail)?;
    services
        .instances
        .configure_loader_version(&id, loader_version)
        .map_err(fail)?;
    Ok(services.snapshot())
}

#[tauri::command]
pub async fn configure_launch(
    state: State<'_, LauncherState>,
    id: String,
    launch: LaunchConfig,
    account_id: Option<String>,
) -> CmdResult<LocalSnapshot> {
    let services = state.services();
    services.configure_launch(&id, launch, account_id).map_err(fail)?;
    Ok(services.snapshot())
}

/// Stars a world and puts it in a group.
#[tauri::command]
pub async fn organize_instance(state: State<'_, LauncherState>, id: String, favorite: bool, group: Option<String>) -> CmdResult<LocalSnapshot> {
    let services = state.services();
    services.organize_instance(&id, favorite, group).map_err(fail)?;
    Ok(services.snapshot())
}

#[tauri::command]
pub async fn change_version(
    state: State<'_, LauncherState>,
    id: String,
    version_id: String,
    version_type: String,
) -> CmdResult<LocalSnapshot> {
    let services = state.services();
    services.change_version(&id, &version_id, &version_type).map_err(fail)?;
    Ok(services.snapshot())
}

/// True while the game runs, downloads, or a file operation holds the instance.
#[tauri::command]
pub async fn instance_busy(state: State<'_, LauncherState>, id: String) -> CmdResult<bool> {
    Ok(state.services().ensure_idle(&id).is_err())
}

#[tauri::command]
pub async fn read_log(state: State<'_, LauncherState>, id: String) -> CmdResult<Vec<String>> {
    let services = state.services();
    blocking(move || services.read_log(&id, 2000)).await
}

/// Diagnose a crash using only the log/crash report produced since the launch
/// started. File reads are bounded on the service side.
#[tauri::command]
pub async fn analyze_crash(
    state: State<'_, LauncherState>,
    instance_id: String,
    since_ms: i64,
) -> CmdResult<CrashAnalysis> {
    let services = state.services();
    blocking(move || services.analyze_crash(&instance_id, since_ms)).await
}

/// Uploads the world's latest game log to mclo.gs and returns the link.
/// The UI asks first: this makes the log public.
#[tauri::command]
pub async fn upload_log(state: State<'_, LauncherState>, id: String) -> CmdResult<String> {
    let services = state.services();
    islet::core::paths::AppPaths::validate_instance_id(&id).map_err(fail)?;
    let path = services.paths.instance_logs_dir(&id).join("latest.log");
    let text = tokio::fs::read(&path)
        .await
        .map_err(|_| fail(AppError::new("log_missing", islet::core::i18n::pick("这个世界还没有游戏日志。", "This world has no game log yet."))))?;
    let text = String::from_utf8_lossy(&text);
    let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).ok();
    let content = islet::core::logshare::redact(islet::core::logshare::tail(&text), home.as_deref());
    let client = services.http_client().map_err(fail)?;
    islet::core::logshare::upload(&client, &content).await.map_err(fail)
}

/// Download/detect the required Java and save it on the instance. Unlike the
/// generic download command this resolves only after the runtime is usable,
/// so the UI can mark the crash fix handled at the right time.
#[tauri::command]
pub async fn install_java_for_instance(
    state: State<'_, LauncherState>,
    id: String,
    major: u32,
) -> CmdResult<islet::core::java::detect::JavaInstallation> {
    state
        .services()
        .install_java_for_instance(&id, major)
        .await
        .map_err(fail)
}

#[tauri::command]
pub async fn load_versions(state: State<'_, LauncherState>, force: bool) -> CmdResult<()> {
    state.services().load_versions(force);
    Ok(())
}

#[tauri::command]
pub async fn load_java_runtimes(state: State<'_, LauncherState>) -> CmdResult<()> {
    state.services().load_java_runtimes();
    Ok(())
}

#[tauri::command]
pub async fn scan_java_directory(state: State<'_, LauncherState>, path: String) -> CmdResult<()> {
    state.services().scan_java_directory(PathBuf::from(path));
    Ok(())
}

#[tauri::command]
pub async fn download_java(state: State<'_, LauncherState>, major: u32) -> CmdResult<String> {
    state.services().download_java(major).map_err(fail)
}

#[tauri::command]
pub async fn load_loader_versions(
    state: State<'_, LauncherState>,
    key: String,
    loader: String,
    minecraft: String,
) -> CmdResult<()> {
    state
        .services()
        .load_loader_versions(key, loader, minecraft);
    Ok(())
}

#[tauri::command]
pub async fn recycled_instances(state: State<'_, LauncherState>) -> CmdResult<Vec<Instance>> {
    let services = state.services();
    blocking(move || Ok(services.instances.recycled())).await
}

/// Games found in a folder of another launcher, ready to import.
#[tauri::command]
pub async fn scan_games(path: String) -> CmdResult<Vec<islet::core::import::ImportCandidate>> {
    blocking(move || Ok(islet::core::import::scan(std::path::Path::new(&path)))).await
}

/// Folders of other launchers found on this machine: (label, path).
#[tauri::command]
pub async fn known_game_dirs() -> CmdResult<Vec<(String, String)>> {
    blocking(|| Ok(islet::core::import::known_dirs())).await
}

#[tauri::command]
pub async fn install_instance(state: State<'_, LauncherState>, id: String) -> CmdResult<String> {
    state.services().install_instance(&id).map_err(fail)
}

#[tauri::command]
pub async fn cancel_task(state: State<'_, LauncherState>, task_id: String) -> CmdResult<()> {
    state.services().cancel_task(&task_id);
    Ok(())
}

#[tauri::command]
pub async fn launch_instance(
    state: State<'_, LauncherState>,
    id: String,
    world: Option<String>,
    server: Option<String>,
    offline: Option<bool>,
) -> CmdResult<()> {
    state.services().launch_instance(&id, world, server, offline.unwrap_or(false)).map_err(fail)
}

#[tauri::command]
pub async fn stop_instance(state: State<'_, LauncherState>, id: String, force: Option<bool>) -> CmdResult<()> {
    state.services().stop_instance(&id, force.unwrap_or(false)).map_err(fail)
}

#[tauri::command]
pub async fn read_game_options(state: State<'_, LauncherState>, id: String) -> CmdResult<options::GameOptions> {
    let services = state.services();
    blocking(move || options::read(&services.paths, &id)).await
}

/// The game saves options.txt when it exits, so edits wait until then.
#[tauri::command]
pub async fn write_game_options(
    state: State<'_, LauncherState>,
    id: String,
    options: options::GameOptions,
) -> CmdResult<options::GameOptions> {
    let services = state.services();
    blocking(move || {
        services.ensure_idle(&id)?;
        let instance = services.instances.get(&id)?;
        options::write(&services.paths, &id, &instance.version_id, &options)?;
        options::read(&services.paths, &id)
    })
    .await
}
