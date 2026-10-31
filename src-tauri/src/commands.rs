//! Commands the web UI calls through `invoke`. Mutations return a fresh
//! snapshot so the UI keeps a single copy of settings, accounts and instances;
//! long work runs on the service runtime and reports back through events.

use std::path::PathBuf;

use serde::Deserialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use islet::core::content::{self, ContentKind, ModFile};
use islet::core::manage::{self, Cleanup, ItemKind};
use islet::core::{configs, options, screenshots, servers};
use islet::core::error::{AppError, ErrorPayload};
use islet::core::instances::{Instance, LaunchConfig};
use islet::core::platform;
use islet::core::settings::Settings;
use islet::services::{CrashAnalysis, FileOperation, LocalSnapshot, LoginMode};

use crate::LauncherState;

type CmdResult<T> = Result<T, ErrorPayload>;

fn fail(error: AppError) -> ErrorPayload {
    error.to_payload()
}

/// Run blocking file work off the async runtime so other commands stay live.
async fn blocking<T: Send + 'static>(
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
pub async fn search_mods(
    state: State<'_, LauncherState>,
    id: String,
    key: String,
    query: String,
    offset: usize,
    kind: Option<ContentKind>,
    categories: Option<Vec<String>>,
    sort: Option<String>,
    source: Option<String>,
) -> CmdResult<()> {
    state
        .services()
        .search_mods(&id, key, query, offset, kind.unwrap_or_default(), categories.unwrap_or_default(), sort.unwrap_or_default(), source.unwrap_or_default())
        .map_err(fail)
}

#[tauri::command]
pub async fn load_mod_versions(
    state: State<'_, LauncherState>,
    id: String,
    key: String,
    project: String,
    kind: Option<ContentKind>,
) -> CmdResult<()> {
    state
        .services()
        .load_mod_versions(&id, key, project, kind.unwrap_or_default())
        .map_err(fail)
}

#[tauri::command]
pub async fn install_mod(
    state: State<'_, LauncherState>,
    id: String,
    version: String,
) -> CmdResult<String> {
    state.services().install_mod(&id, vec![version]).map_err(fail)
}

/// Updates several mods in one task ("update all").
#[tauri::command]
pub async fn update_mods(
    state: State<'_, LauncherState>,
    id: String,
    versions: Vec<String>,
) -> CmdResult<String> {
    state.services().install_mod(&id, versions).map_err(fail)
}

#[tauri::command]
pub async fn install_pack(
    state: State<'_, LauncherState>,
    id: String,
    version: String,
    kind: ContentKind,
    save: Option<String>,
) -> CmdResult<String> {
    state.services().install_pack(&id, version, kind, save).map_err(fail)
}

#[tauri::command]
pub async fn inspect_mods(
    state: State<'_, LauncherState>,
    id: String,
) -> CmdResult<Vec<islet::core::modrinth::ModInsight>> {
    state.services().inspect_mods(&id).await.map_err(fail)
}

#[tauri::command]
pub async fn inspect_packs(
    state: State<'_, LauncherState>,
    id: String,
) -> CmdResult<Vec<islet::core::modrinth::PackInsight>> {
    state.services().inspect_packs(&id).await.map_err(fail)
}

/// What each mod jar says about itself, by file name.
#[tauri::command]
pub async fn mod_meta(state: State<'_, LauncherState>, id: String) -> CmdResult<std::collections::BTreeMap<String, content::ModMeta>> {
    let services = state.services();
    blocking(move || content::mod_meta(&services.paths, &id)).await
}

/// Icons from inside the mod jars, by file name.
#[tauri::command]
pub async fn mod_icons(state: State<'_, LauncherState>, id: String) -> CmdResult<std::collections::BTreeMap<String, String>> {
    let services = state.services();
    blocking(move || content::mod_icons(&services.paths, &id)).await
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackLists {
    pub resourcepacks: Vec<content::PackFile>,
    pub shaderpacks: Vec<content::PackFile>,
}

#[tauri::command]
pub async fn list_packs(state: State<'_, LauncherState>, id: String) -> CmdResult<PackLists> {
    let services = state.services();
    blocking(move || {
        Ok(PackLists {
            resourcepacks: content::packs(&services.paths, &id, ContentKind::ResourcePack)?,
            shaderpacks: content::packs(&services.paths, &id, ContentKind::Shader)?,
        })
    })
    .await
}

#[tauri::command]
pub async fn list_mods(state: State<'_, LauncherState>, id: String) -> CmdResult<Vec<ModFile>> {
    let services = state.services();
    blocking(move || content::mods(&services.paths, &id)).await
}

/// The latest mod install or update that can still be undone.
#[tauri::command]
pub async fn last_mod_change(
    state: State<'_, LauncherState>,
    id: String,
) -> CmdResult<Option<islet::core::modrinth::LastModChange>> {
    let services = state.services();
    blocking(move || islet::core::modrinth::last_change(&services.paths, &id)).await
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldList {
    pub worlds: Vec<content::WorldInfo>,
    /// The installed version can open a world directly (1.20+).
    pub quick_play: bool,
}

#[tauri::command]
pub async fn list_worlds(state: State<'_, LauncherState>, id: String) -> CmdResult<WorldList> {
    let services = state.services();
    blocking(move || {
        Ok(WorldList {
            worlds: content::worlds(&services.paths, &id)?,
            quick_play: services.quick_play_supported(&id),
        })
    })
    .await
}

#[tauri::command]
pub async fn recycled_instances(state: State<'_, LauncherState>) -> CmdResult<Vec<Instance>> {
    let services = state.services();
    blocking(move || Ok(services.instances.recycled())).await
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FileOp {
    Duplicate,
    Backup,
    RestoreBackup { path: String },
    RestoreDeleted,
    AddMods { paths: Vec<String> },
    ToggleMod { name: String },
    SetMods { names: Vec<String>, enabled: bool },
    RecycleMods { names: Vec<String> },
    ImportPack { path: String },
    InstallModpack { version: String },
    Recycle { item: ItemKind, name: String, save: Option<String> },
    AddPacks { item: ItemKind, paths: Vec<String> },
    AddDatapacks { save: String, paths: Vec<String> },
    RenameSave { folder: String, name: String },
    BackupSave { folder: String },
    RestoreSave { folder: String, backup: String },
    ImportSave { path: String },
    ExportSave { folder: String, destination: String },
    PurgeRecycle { only: Option<String> },
    Clean { target: Cleanup },
    ImportGame { source: String },
    UpdatePack { path: String },
    UpdateModpackOnline { version: String },
    ExportPack { destination: String, options: islet::core::modpacks::ExportOptions },
    ChangeVersion { version_id: String, version_type: String },
    UndoModUpdate,
}

/// File work runs in the background and ends with an `operationComplete` event.
#[tauri::command]
pub async fn file_operation(
    state: State<'_, LauncherState>,
    id: Option<String>,
    operation: FileOp,
) -> CmdResult<()> {
    let operation = match operation {
        FileOp::Duplicate => FileOperation::Duplicate,
        FileOp::Backup => FileOperation::Backup,
        FileOp::RestoreBackup { path } => FileOperation::RestoreBackup(path.into()),
        FileOp::RestoreDeleted => FileOperation::RestoreDeleted,
        FileOp::AddMods { paths } => {
            FileOperation::AddMods(paths.into_iter().map(PathBuf::from).collect())
        }
        FileOp::ToggleMod { name } => FileOperation::ToggleMod(name),
        FileOp::SetMods { names, enabled } => FileOperation::SetMods { names, enabled },
        FileOp::RecycleMods { names } => FileOperation::RecycleMods(names),
        FileOp::ImportPack { path } => FileOperation::ImportPack(path.into()),
        FileOp::InstallModpack { version } => FileOperation::InstallModpack(version),
        FileOp::Recycle { item, name, save } => FileOperation::Recycle { kind: item, name, save },
        FileOp::AddPacks { item, paths } => FileOperation::AddPacks {
            kind: item,
            paths: paths.into_iter().map(PathBuf::from).collect(),
        },
        FileOp::AddDatapacks { save, paths } => FileOperation::AddDatapacks {
            save,
            paths: paths.into_iter().map(PathBuf::from).collect(),
        },
        FileOp::RenameSave { folder, name } => FileOperation::RenameSave { folder, name },
        FileOp::BackupSave { folder } => FileOperation::BackupSave(folder),
        FileOp::RestoreSave { folder, backup } => FileOperation::RestoreSave { folder, backup: backup.into() },
        FileOp::ImportSave { path } => FileOperation::ImportSave(path.into()),
        FileOp::ExportSave { folder, destination } => FileOperation::ExportSave { folder, destination: destination.into() },
        FileOp::PurgeRecycle { only } => FileOperation::PurgeRecycle(only),
        FileOp::Clean { target } => FileOperation::Clean(target),
        FileOp::ImportGame { source } => FileOperation::ImportGame(source.into()),
        FileOp::UpdatePack { path } => FileOperation::UpdatePack(path.into()),
        FileOp::UpdateModpackOnline { version } => FileOperation::UpdateModpackOnline(version),
        FileOp::ExportPack { destination, options } => FileOperation::ExportPack { destination: destination.into(), options },
        FileOp::ChangeVersion { version_id, version_type } => FileOperation::ChangeVersion { version_id, version_type },
        FileOp::UndoModUpdate => FileOperation::UndoModUpdate,
    };
    // Importing creates a new instance, so it locks a placeholder id.
    let id = id.unwrap_or_else(|| "import-pack".into());
    state
        .services()
        .file_operation(&id, operation)
        .map_err(fail)
}

#[tauri::command]
pub async fn list_datapacks(state: State<'_, LauncherState>, id: String, save: String) -> CmdResult<Vec<manage::Datapack>> {
    let services = state.services();
    blocking(move || manage::datapacks(&services.paths, &id, &save)).await
}

#[tauri::command]
pub async fn list_configs(state: State<'_, LauncherState>, id: String) -> CmdResult<Vec<configs::ConfigFile>> {
    let services = state.services();
    blocking(move || configs::list(&services.paths, &id)).await
}

#[tauri::command]
pub async fn read_config(state: State<'_, LauncherState>, id: String, path: String) -> CmdResult<String> {
    let services = state.services();
    blocking(move || configs::read(&services.paths, &id, &path)).await
}

#[tauri::command]
pub async fn write_config(state: State<'_, LauncherState>, id: String, path: String, text: String) -> CmdResult<()> {
    let services = state.services();
    blocking(move || configs::write(&services.paths, &id, &path, &text)).await
}

#[tauri::command]
pub async fn save_backups(state: State<'_, LauncherState>, id: String, save: String) -> CmdResult<Vec<manage::SaveBackup>> {
    let services = state.services();
    blocking(move || manage::save_backups(&services.paths, &id, &save)).await
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
pub async fn install_optifine(state: State<'_, LauncherState>, id: String) -> CmdResult<String> {
    state.services().install_optifine(&id).map_err(fail)
}

#[tauri::command]
pub async fn modpack_info(state: State<'_, LauncherState>, id: String) -> CmdResult<Option<islet::core::modpacks::PackInfo>> {
    let services = state.services();
    blocking(move || Ok(islet::core::modpacks::info(&services.paths, &id))).await
}

/// The newest version of the online pack a world was installed from, when
/// it is newer than the one the world has.
#[tauri::command]
pub async fn modpack_update(state: State<'_, LauncherState>, id: String) -> CmdResult<Option<islet::core::modrinth::ModVersion>> {
    let services = state.services();
    let paths = services.paths.clone();
    let (Some(source), Some(info)) = (islet::core::modpacks::source(&paths, &id), islet::core::modpacks::info(&paths, &id)) else {
        return Ok(None);
    };
    let client = services.http_client().map_err(fail)?;
    let versions = if source.project.starts_with(islet::core::curseforge::PREFIX) {
        let key = services.settings.get().curseforge_key;
        islet::core::curseforge::modpack_versions(&client, &key, &source.project).await.map_err(fail)?
    } else {
        islet::core::modrinth::modpack_versions(&client, &source.project).await.map_err(fail)?
    };
    Ok(islet::core::modpacks::newer_version(&versions, &source.version, &info.version).cloned())
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
    if cfg!(windows) {
        if file.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("msi")) {
            std::process::Command::new("msiexec.exe").arg("/i").arg(&file).spawn().map_err(|e| failed(e.to_string()))?;
        } else if file.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("exe")) {
            std::process::Command::new(&file).spawn().map_err(|e| failed(e.to_string()))?;
        } else { return Err(fail(AppError::new("update", pick("更新包不是 Windows 安装程序。", "This package isn't a Windows installer.")))); }
        crate::quit(&app);
        return Ok(String::new());
    }
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
    {
        app.opener().reveal_item_in_dir(&file).map_err(|e| failed(e.to_string()))?;
        Ok(pick("更新包已下载并校验，已在文件夹中显示。", "The update is downloaded and checked, and shown in its folder.").into())
    }
}

#[tauri::command]
pub async fn update_source() -> CmdResult<bool> {
    Ok(islet::core::update::source().is_some())
}

/// Which resource packs are on, in the game's order (winning pack first).
#[tauri::command]
pub async fn set_resource_packs(state: State<'_, LauncherState>, id: String, packs: Vec<String>) -> CmdResult<()> {
    let services = state.services();
    services.ensure_idle(&id).map_err(fail)?;
    blocking(move || content::set_resource_packs(&services.paths, &id, &packs)).await
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

#[tauri::command]
pub async fn patch_notes(state: State<'_, LauncherState>) -> CmdResult<Vec<islet::core::news::PatchNote>> {
    let client = state.services().http_client().map_err(fail)?;
    islet::core::news::patch_notes(&client, 6).await.map_err(fail)
}

/// Modpacks on Modrinth or CurseForge, optionally for one Minecraft version,
/// loader and category.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn search_modpacks(
    state: State<'_, LauncherState>,
    query: String,
    offset: usize,
    source: String,
    sort: String,
    version: Option<String>,
    loader: Option<String>,
    category: Option<String>,
) -> CmdResult<islet::core::modrinth::SearchResults> {
    let services = state.services();
    let client = services.http_client().map_err(fail)?;
    if source == "curseforge" {
        let key = services.settings.get().curseforge_key;
        return islet::core::curseforge::search_modpacks(&client, &key, query.trim(), offset, version.as_deref(), loader.as_deref(), category.as_deref(), &sort).await.map_err(fail);
    }
    islet::core::modrinth::search_modpacks(&client, query.trim(), offset, version.as_deref(), loader.as_deref(), category.as_deref(), &sort).await.map_err(fail)
}

#[tauri::command]
pub async fn modpack_versions(state: State<'_, LauncherState>, project: String) -> CmdResult<Vec<islet::core::modrinth::ModVersion>> {
    let services = state.services();
    let client = services.http_client().map_err(fail)?;
    if project.starts_with(islet::core::curseforge::PREFIX) {
        let key = services.settings.get().curseforge_key;
        return islet::core::curseforge::modpack_versions(&client, &key, &project).await.map_err(fail);
    }
    islet::core::modrinth::modpack_versions(&client, &project).await.map_err(fail)
}

/// The description, gallery and links of a Modrinth or CurseForge project.
#[tauri::command]
pub async fn mod_details(state: State<'_, LauncherState>, project: String) -> CmdResult<islet::core::modrinth::ProjectDetails> {
    let services = state.services();
    let client = services.http_client().map_err(fail)?;
    if project.starts_with(islet::core::curseforge::PREFIX) {
        let key = services.settings.get().curseforge_key;
        return islet::core::curseforge::details(&client, &key, &project).await.map_err(fail);
    }
    islet::core::modrinth::details(&client, &project).await.map_err(fail)
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

#[tauri::command]
pub async fn list_screenshots(state: State<'_, LauncherState>, id: String) -> CmdResult<Vec<screenshots::Screenshot>> {
    let services = state.services();
    blocking(move || screenshots::list(&services.paths, &id)).await
}

#[tauri::command]
pub async fn recycle_screenshot(state: State<'_, LauncherState>, id: String, name: String) -> CmdResult<()> {
    let services = state.services();
    blocking(move || screenshots::recycle(&services.paths, &id, &name)).await
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

#[tauri::command]
pub async fn stop_instance(state: State<'_, LauncherState>, id: String, force: Option<bool>) -> CmdResult<()> {
    state.services().stop_instance(&id, force.unwrap_or(false)).map_err(fail)
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
            (content::world_dir(paths, &id, &folder).map_err(fail)?, islet::core::i18n::pick("存档目录", "Save folder"))
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
    if !url.starts_with("https://") && !url.starts_with("http://localhost") {
        return Err(fail(AppError::new("open", islet::core::i18n::pick("不支持打开这个链接。", "This link can't be opened."))));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| fail(AppError::detail("open", islet::core::i18n::pick("无法打开链接。", "Couldn't open the link."), error.to_string())))
}

#[tauri::command]
pub async fn mod_changelog(state: State<'_, LauncherState>, version: String) -> CmdResult<String> {
    let services = state.services(); let client = services.http_client().map_err(fail)?;
    if version.starts_with(islet::core::curseforge::PREFIX) {
        islet::core::curseforge::changelog(&client, &services.settings.get().curseforge_key, &version).await.map_err(fail)
    } else { islet::core::modrinth::changelog(&client, &version).await.map_err(fail) }
}
