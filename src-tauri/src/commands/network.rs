//! Multiplayer server list, status pings, mod sync checks, and remote SSH server management commands.
use tauri::State;

use islet::core::servers;

use crate::LauncherState;

use super::{CmdResult, blocking, fail};

#[tauri::command]
pub async fn list_servers(state: State<'_, LauncherState>, id: String) -> CmdResult<Vec<servers::ServerEntry>> {
    let services = state.services();
    blocking(move || servers::list(&services.paths, &id)).await
}

/// The game rewrites servers.dat while it runs, so edits wait until it exits.
#[tauri::command]
pub async fn edit_server(
    state: State<'_, LauncherState>,
    id: String,
    change: servers::ServerEdit,
) -> CmdResult<Vec<servers::ServerEntry>> {
    let services = state.services();
    blocking(move || {
        services.ensure_idle(&id)?;
        servers::edit(&services.paths, &id, change)?;
        servers::list(&services.paths, &id)
    })
    .await
}

#[tauri::command]
pub async fn ping_server(state: State<'_, LauncherState>, address: String) -> CmdResult<servers::ServerStatus> {
    state.services().ping_server(&address).await.map_err(fail)
}

/// The mods a world lacks for a server it was imported to join; none when
/// it has them all.
#[tauri::command]
pub async fn check_server_mods(state: State<'_, LauncherState>, id: String, address: String) -> CmdResult<Option<islet::core::server_mods::Check>> {
    let services = state.services();
    blocking(move || services.check_server_mods(&id, &address)).await
}

#[tauri::command]
pub async fn remote_info(state: State<'_, LauncherState>, id: String) -> CmdResult<islet::services::RemoteInfo> {
    state.services().remote_info(&id).map_err(fail)
}

#[tauri::command]
pub async fn remote_connect(state: State<'_, LauncherState>, id: String, target: islet::core::remote::Target, secret: Option<String>) -> CmdResult<islet::core::remote::Probe> {
    state.services().remote_connect(&id, target, secret).await.map_err(fail)
}

#[tauri::command]
pub async fn remote_settings(state: State<'_, LauncherState>, id: String, target: islet::core::remote::Target) -> CmdResult<()> {
    state.services().remote_settings(&id, target).await.map_err(fail)
}

#[tauri::command]
pub async fn remote_forget(state: State<'_, LauncherState>, id: String) -> CmdResult<()> {
    state.services().remote_forget(&id).map_err(fail)
}

#[tauri::command]
pub async fn remote_status(state: State<'_, LauncherState>, id: String) -> CmdResult<islet::services::RemoteStatus> {
    state.services().remote_status(&id).await.map_err(fail)
}

#[tauri::command]
pub async fn remote_action(state: State<'_, LauncherState>, id: String, action: String) -> CmdResult<String> {
    state.services().remote_action(&id, &action).await.map_err(fail)
}

#[tauri::command]
pub async fn remote_command(state: State<'_, LauncherState>, id: String, text: String) -> CmdResult<()> {
    state.services().remote_command(&id, &text).await.map_err(fail)
}

#[tauri::command]
pub async fn remote_log(state: State<'_, LauncherState>, id: String, lines: u32) -> CmdResult<String> {
    state.services().remote_log(&id, lines).await.map_err(fail)
}

#[tauri::command]
pub async fn remote_download_backup(state: State<'_, LauncherState>, id: String, name: String, destination: String) -> CmdResult<()> {
    state.services().remote_download_backup(&id, &name, std::path::Path::new(&destination)).await.map_err(fail)
}
