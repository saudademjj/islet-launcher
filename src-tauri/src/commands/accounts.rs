//! Account, Microsoft/Yggdrasil sign-in, skin, and cape commands.
use std::path::PathBuf;

use tauri::State;

use islet::services::{LocalSnapshot, LoginMode};

use crate::LauncherState;

use super::{CmdResult, fail};

/// Microsoft login needs a Client ID from settings or baked into the build.
#[tauri::command]
pub async fn login_configured(state: State<'_, LauncherState>) -> CmdResult<bool> {
    let settings = state.services().settings.get();
    Ok(!settings.effective_client_id().trim().is_empty())
}

#[tauri::command]
pub async fn add_offline_account(
    state: State<'_, LauncherState>,
    username: String,
) -> CmdResult<LocalSnapshot> {
    let services = state.services();
    services
        .add_offline_account(username.trim())
        .map_err(fail)?;
    Ok(services.snapshot())
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YggdrasilResult {
    /// Characters to choose from; none when signed in.
    pub profiles: Option<Vec<islet::core::auth::yggdrasil::GameProfile>>,
    pub snapshot: LocalSnapshot,
}

#[tauri::command]
pub async fn account_capes(state: State<'_, LauncherState>, id: String) -> CmdResult<Vec<islet::core::auth::microsoft::Cape>> {
    state.services().account_capes(&id).await.map_err(fail)
}

#[tauri::command]
pub async fn change_skin(state: State<'_, LauncherState>, id: String, path: Option<String>, slim: bool) -> CmdResult<LocalSnapshot> {
    let services = state.services();
    services.change_skin(&id, path.map(PathBuf::from), slim).await.map_err(fail)?;
    Ok(services.snapshot())
}

#[tauri::command]
pub async fn set_cape(state: State<'_, LauncherState>, id: String, cape: Option<String>) -> CmdResult<()> {
    state.services().set_cape(&id, cape.as_deref()).await.map_err(fail)
}

/// Third-party login. The password is used for this request only.
#[tauri::command]
pub async fn login_yggdrasil(
    state: State<'_, LauncherState>,
    server: String,
    username: String,
    password: String,
    profile: Option<String>,
) -> CmdResult<YggdrasilResult> {
    let services = state.services();
    let profiles = services.login_yggdrasil(&server, &username, &password, profile.as_deref()).await.map_err(fail)?;
    Ok(YggdrasilResult { profiles, snapshot: services.snapshot() })
}

#[tauri::command]
pub async fn remove_account(state: State<'_, LauncherState>, id: String) -> CmdResult<LocalSnapshot> {
    let services = state.services();
    services.remove_account(&id).map_err(fail)?;
    Ok(services.snapshot())
}

#[tauri::command]
pub async fn set_active_account(
    state: State<'_, LauncherState>,
    id: Option<String>,
) -> CmdResult<LocalSnapshot> {
    let services = state.services();
    services.set_active_account(id).map_err(fail)?;
    Ok(services.snapshot())
}

#[tauri::command]
pub async fn start_login(state: State<'_, LauncherState>, mode: LoginMode) -> CmdResult<String> {
    state.services().start_login(mode).map_err(fail)
}

#[tauri::command]
pub async fn cancel_login(state: State<'_, LauncherState>, login_id: String) -> CmdResult<()> {
    state.services().cancel_login(&login_id);
    Ok(())
}
