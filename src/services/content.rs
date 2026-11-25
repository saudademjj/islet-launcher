use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{AppServices, FileOperation, UiEvent, human_bytes, load_version, lock};
use crate::core::content::ContentKind;
use crate::core::error::{AppError, AppResult};
use crate::core::java::runtime;
use crate::core::manage::{self, ItemKind};
use crate::core::mojang::version::{VersionJson, java_major};

impl AppServices {
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
                        Ok(crate::core::i18n::pick("整合包已导入，首次开始游戏时安装。", "Modpack imported; it installs the first time you play.").into())
                    }
                    FileOperation::InstallModpack(version) => {
                        let key = this.settings.get().effective_curseforge_key();
                        let (file, source) = tokio::runtime::Handle::current().block_on(crate::core::modpacks::download_online(&this.http()?, &key, &this.paths, &version))?;
                        let imported = this.import_pack(&file);
                        let _ = std::fs::remove_file(&file);
                        // Without the record the world still plays; it just can't update online.
                        if let Err(error) = crate::core::modpacks::record_source(&this.paths, &imported?, &source) {
                            tracing::warn!(%error, "couldn't record the modpack source");
                        }
                        Ok(crate::core::i18n::pick("整合包已下载并导入，首次开始游戏时安装。", "Modpack downloaded and imported; it installs the first time you play.").into())
                    }
                    FileOperation::ExportPack { destination, options } => {
                        let instance = this.instances.get(&id)?;
                        let candidates = crate::core::modpacks::export_candidates(&this.paths, &id)?;
                        let http = this.http()?;
                        let runtime = tokio::runtime::Handle::current();
                        // Without the lookup every file is bundled; the pack still works.
                        let (linked, bundled, site) = if options.curseforge() {
                            let settings = this.settings.get();
                            let key = settings.effective_curseforge_key();
                            let prints: Vec<u32> = candidates.iter().map(|c| c.fingerprint).collect();
                            let hosted = if !settings.curseforge_configured() {
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
                        let cancel = CancellationToken::new();
                        let (summary, pack) = this.export_server(&id, &destination, options.clone(), &cancel)?;
                        let instance = this.instances.get(&id)?;
                        let what = if crate::core::instances::runs_plugins(&instance.loader) { crate::core::i18n::pick("插件", "plugins") } else { crate::core::i18n::pick("模组", "mods") };
                        let mut message = if instance.server.is_some() {
                            this.write_deploy_kit(&destination, this.server_java_major(&instance)?, &cancel)?;
                            crate::tr!("服务端已生成，带 {} 个{what}：上传到 Linux 服务器后运行 sh islet.sh setup 即可开服，详见文件夹里的说明。", "Server ready with {} {what}: upload it to a Linux server and run sh islet.sh setup; the folder's readme has more.", summary.mods)
                        } else {
                            crate::tr!("服务端已生成，带 {} 个{what}：运行文件夹里的 start.bat（Windows）或 start.sh 启动。", "Server ready with {} {what}: run start.bat (Windows) or start.sh in its folder.", summary.mods)
                        };
                        if !summary.client_only.is_empty() {
                            message += &crate::tr!("{} 个只在客户端运行的模组没有放进去。", " {} client-only mods were left out.", summary.client_only.len());
                        }
                        if let Some(pack) = pack {
                            message += &crate::tr!("把「{}」发给好友，导入后就能进服。", " Send \"{}\" to friends; once imported they can join.", pack.file_name().unwrap_or_default().to_string_lossy());
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
                            if removed > 0 { crate::tr!("，{removed} 个旧版本已移入回收站", ", {removed} old versions moved to the recycle bin") } else { String::new() }))
                    }
                    FileOperation::UpdatePack(path) => this.update_pack(&id, &path),
                    FileOperation::UpdateModpackOnline(version) => {
                        let key = this.settings.get().effective_curseforge_key();
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
        let key = self.settings.get().effective_curseforge_key();
        let http = self.http()?;
        tokio::runtime::Handle::current().block_on(crate::core::curseforge::resolve_pack(&http, &key, manifest))
    }

    #[allow(clippy::too_many_arguments)]
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
                    let key = this.settings.get().effective_curseforge_key();
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
                    let key = this.settings.get().effective_curseforge_key();
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
                    let key = this.settings.get().effective_curseforge_key();
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
                let build = crate::core::optifine::install_matched(&http, &this.paths, &instance, &cancel).await?;
                let file = build.filename;
                if instance.loader == "forge" {
                    let note = match (&build.forge, &instance.loader_version) {
                        (Some(rec), Some(cur)) if !crate::core::optifine::forge_matches(rec, cur) => {
                            crate::tr!("（官方推荐适配 {rec}，当前世界为 Forge {cur}）", " (built for {rec}; this world uses Forge {cur})")
                        }
                        _ => String::new(),
                    };
                    return Ok(crate::tr!("已安装 {file}{note}，进游戏后在「选项 → 视频设置」里调整。", "Installed {file}{note}; adjust it in game under Options → Video Settings."));
                }
                let versions = crate::core::modrinth::versions(&http, &instance, "optifabric", ContentKind::Mod).await?;
                let first = versions.first().ok_or_else(|| {
                    AppError::new("optifine", crate::tr!("已安装 {file}，但 OptiFabric 还没有适配 {}，暂时无法在 Fabric 上加载 OptiFine。", "Installed {file}, but OptiFabric doesn't support {} yet, so OptiFine can't load on Fabric for now.", instance.version_id))
                })?;
                crate::core::modrinth::install(&http, &this.paths, &instance, std::slice::from_ref(&first.id), &cancel).await?;
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
                    let key = this.settings.get().effective_curseforge_key();
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
        // Files Modrinth does not know may be on CurseForge, when configured.
        let settings = self.settings.get();
        if settings.curseforge_configured() {
            let key = settings.effective_curseforge_key();
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
        if !curseforge_available
            && let Some(error) = modrinth_error
        {
            return Err(error);
        }
        Ok(found)
    }

    /// Names and icons for the instance's resource packs and shader packs.
    pub async fn inspect_packs(&self, id: &str) -> AppResult<Vec<crate::core::modrinth::PackInsight>> {
        let instance = self.instances.get(id)?;
        crate::core::modrinth::inspect_packs(&self.http()?, &self.paths, &instance).await
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
        let mut instance = self.instances.get(id)?;
        let settings = self.settings.get();
        let (base, version) = if instance.server.is_some() {
            let base = self.prepare_server_project(&mut instance, cancel)?;
            // Only mod servers need friends to bring anything.
            if matches!(instance.loader.as_str(), "vanilla" | "paper" | "purpur") {
                options.client_pack = false;
            }
            (base.clone(), base)
        } else {
            let base: VersionJson = std::fs::read(self.paths.version_json(&instance.version_id))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .ok_or_else(|| AppError::new("version_missing", crate::core::i18n::pick("版本文件不存在，请先下载这个世界。", "The version file is missing; install this world first.")))?;
            (base, load_version(&self.paths, &instance)?)
        };
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
}
