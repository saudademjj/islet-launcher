use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{AppServices, InstallProgressData, UiEvent, load_version, lock, projects};
use crate::core::download::{self, DownloadProgress};
use crate::core::error::{AppError, AppResult};
use crate::core::instances::Instance;
use crate::core::java::detect::{self, JavaInstallation};
use crate::core::java::runtime;
use crate::core::manage;
use crate::core::mojang::install as mojang_install;
use crate::core::mojang::manifest;
use crate::core::mojang::version::java_major;
use crate::core::paths::AppPaths;
use crate::core::settings::DownloadSource;

impl AppServices {
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
                    std::slice::from_ref(&this.paths.java),
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

    pub fn install_instance(self: &Arc<Self>, instance_id: &str) -> AppResult<String> {
        self.ensure_idle(instance_id)?;
        let instance = self.instances.get(instance_id)?;
        if instance.server.is_some() {
            return Err(projects::not_playable());
        }
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
            let (merged, pinned) = crate::core::loaders::install_with_source(
                &http,
                &self.paths,
                &plan.version,
                &instance.loader,
                instance.loader_version.as_deref(),
                &java,
                cancel,
                report,
                source,
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
}
