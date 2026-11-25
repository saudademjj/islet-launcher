//! Mods, packs, modpacks, saves, configs, screenshots, and background file operations.
use std::path::PathBuf;

use serde::Deserialize;
use tauri::State;

use islet::core::content::{self as core_content, ContentKind, ModFile};
use islet::core::manage::{self, Cleanup, ItemKind};
use islet::core::{configs, screenshots};
use islet::services::FileOperation;

use crate::LauncherState;

use super::{CmdResult, blocking, fail};

#[tauri::command]
#[allow(clippy::too_many_arguments)]
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
pub async fn mod_meta(state: State<'_, LauncherState>, id: String) -> CmdResult<std::collections::BTreeMap<String, core_content::ModMeta>> {
    let services = state.services();
    blocking(move || core_content::mod_meta(&services.paths, &id)).await
}

/// Icons from inside the mod jars, by file name.
#[tauri::command]
pub async fn mod_icons(state: State<'_, LauncherState>, id: String) -> CmdResult<std::collections::BTreeMap<String, String>> {
    let services = state.services();
    blocking(move || core_content::mod_icons(&services.paths, &id)).await
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackLists {
    pub resourcepacks: Vec<core_content::PackFile>,
    pub shaderpacks: Vec<core_content::PackFile>,
}

#[tauri::command]
pub async fn list_packs(state: State<'_, LauncherState>, id: String) -> CmdResult<PackLists> {
    let services = state.services();
    blocking(move || {
        Ok(PackLists {
            resourcepacks: core_content::packs(&services.paths, &id, ContentKind::ResourcePack)?,
            shaderpacks: core_content::packs(&services.paths, &id, ContentKind::Shader)?,
        })
    })
    .await
}

#[tauri::command]
pub async fn list_mods(state: State<'_, LauncherState>, id: String) -> CmdResult<Vec<ModFile>> {
    let services = state.services();
    blocking(move || core_content::mods(&services.paths, &id)).await
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
    pub worlds: Vec<core_content::WorldInfo>,
    /// The installed version can open a world directly (1.20+).
    pub quick_play: bool,
}

#[tauri::command]
pub async fn list_worlds(state: State<'_, LauncherState>, id: String) -> CmdResult<WorldList> {
    let services = state.services();
    blocking(move || {
        Ok(WorldList {
            worlds: core_content::worlds(&services.paths, &id)?,
            quick_play: services.quick_play_supported(&id),
        })
    })
    .await
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
    ExportServer { destination: String, options: islet::core::server_pack::ServerOptions },
    DeployServer { options: islet::services::DeployOptions },
    FixServerMods { address: String },
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
        FileOp::ExportServer { destination, options } => FileOperation::ExportServer { destination: destination.into(), options },
        FileOp::DeployServer { options } => FileOperation::DeployServer(options),
        FileOp::FixServerMods { address } => FileOperation::FixServerMods { address },
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
        let key = services.settings.get().effective_curseforge_key();
        islet::core::curseforge::modpack_versions(&client, &key, &source.project).await.map_err(fail)?
    } else {
        islet::core::modrinth::modpack_versions(&client, &source.project).await.map_err(fail)?
    };
    Ok(islet::core::modpacks::newer_version(&versions, &source.version, &info.version).cloned())
}

/// Which resource packs are on, in the game's order (winning pack first).
#[tauri::command]
pub async fn set_resource_packs(state: State<'_, LauncherState>, id: String, packs: Vec<String>) -> CmdResult<()> {
    let services = state.services();
    services.ensure_idle(&id).map_err(fail)?;
    blocking(move || core_content::set_resource_packs(&services.paths, &id, &packs)).await
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
        let key = services.settings.get().effective_curseforge_key();
        return islet::core::curseforge::search_modpacks(&client, &key, query.trim(), offset, version.as_deref(), loader.as_deref(), category.as_deref(), &sort).await.map_err(fail);
    }
    islet::core::modrinth::search_modpacks(&client, query.trim(), offset, version.as_deref(), loader.as_deref(), category.as_deref(), &sort).await.map_err(fail)
}

#[tauri::command]
pub async fn modpack_versions(state: State<'_, LauncherState>, project: String) -> CmdResult<Vec<islet::core::modrinth::ModVersion>> {
    let services = state.services();
    let client = services.http_client().map_err(fail)?;
    if project.starts_with(islet::core::curseforge::PREFIX) {
        let key = services.settings.get().effective_curseforge_key();
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
        let key = services.settings.get().effective_curseforge_key();
        return islet::core::curseforge::details(&client, &key, &project).await.map_err(fail);
    }
    islet::core::modrinth::details(&client, &project).await.map_err(fail)
}

#[tauri::command]
pub async fn mod_changelog(state: State<'_, LauncherState>, version: String) -> CmdResult<String> {
    let services = state.services();
    let client = services.http_client().map_err(fail)?;
    if version.starts_with(islet::core::curseforge::PREFIX) {
        islet::core::curseforge::changelog(&client, &services.settings.get().effective_curseforge_key(), &version).await.map_err(fail)
    } else {
        islet::core::modrinth::changelog(&client, &version).await.map_err(fail)
    }
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
