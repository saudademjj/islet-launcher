//! Server projects from the server page: a kind of server, a game version,
//! the mods or plugins it runs and its settings, built into a folder ready
//! to upload or deployed over SSH. They live with the worlds but are never
//! installed or played.
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use super::AppServices;
use crate::core::error::{AppError, AppResult};
use crate::core::i18n::pick;
use crate::core::instances::{Instance, ServerProject, SERVER_LOADERS};
use crate::core::mojang::version::VersionJson;

pub(super) fn not_playable() -> AppError {
    AppError::new("server_project", pick("这是开服项目，只能生成服务端，不能直接进入游戏。", "This is a server project; it builds a server and can't be played."))
}

impl AppServices {
    pub fn create_server_project(&self, name: &str, version_id: &str, loader: &str) -> AppResult<Instance> {
        self.ensure_storage_idle()?;
        if !SERVER_LOADERS.contains(&loader) {
            return Err(AppError::new("loader", pick("暂不支持这种服务端。", "This kind of server isn't supported.")));
        }
        if !crate::core::fabric::valid_version(version_id) {
            return Err(AppError::new("version", pick("请选择 Minecraft 版本。", "Choose a Minecraft version.")));
        }
        let mut instance = self.instances.create(name, version_id, "release")?;
        instance.loader = loader.to_string();
        instance.server = Some(ServerProject::default());
        self.instances.update(&instance)?;
        Ok(instance)
    }

    pub fn configure_server_project(&self, id: &str, name: &str, version_id: &str, loader: &str, project: ServerProject) -> AppResult<Instance> {
        self.ensure_idle(id)?;
        self.instances.configure_server(id, name, version_id, loader, project)
    }

    /// The game versions Paper or Purpur has builds for, newest first; the
    /// other kinds follow Mojang's list, so this is empty for them.
    pub async fn server_versions(&self, loader: &str) -> AppResult<Vec<String>> {
        let http = self.http()?;
        let mut versions: Vec<String> = match loader {
            "paper" => {
                #[derive(Deserialize)]
                struct Project {
                    versions: std::collections::BTreeMap<String, Vec<String>>,
                }
                let project: Project = http.get("https://fill.papermc.io/v3/projects/paper").timeout(Duration::from_secs(20)).send().await?.error_for_status()?.json().await?;
                project.versions.into_values().flatten().collect()
            }
            "purpur" => {
                #[derive(Deserialize)]
                struct Project {
                    versions: Vec<String>,
                }
                let project: Project = http.get("https://api.purpurmc.org/v2/purpur").timeout(Duration::from_secs(20)).send().await?.error_for_status()?.json().await?;
                project.versions
            }
            _ => return Ok(Vec::new()),
        };
        // Releases only: no pre-releases or release candidates.
        versions.retain(|v| crate::core::fabric::valid_version(v) && v.bytes().all(|b| b.is_ascii_digit() || b == b'.'));
        let key = |v: &String| v.split('.').map(|part| part.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
        versions.sort_by_key(|v| std::cmp::Reverse(key(v)));
        versions.dedup();
        Ok(versions)
    }

    /// What `export_server` needs for a project, which has no client:
    /// the version's profile alone, and the newest stable loader when none
    /// is fixed yet (kept, so later builds match).
    pub(super) fn prepare_server_project(&self, instance: &mut Instance, cancel: &CancellationToken) -> AppResult<VersionJson> {
        let runtime = tokio::runtime::Handle::current();
        let http = self.http()?;
        let base = runtime.block_on(crate::core::mojang::install::fetch_version_json(&http, &self.paths, &instance.version_id, cancel))?;
        if matches!(instance.loader.as_str(), "fabric" | "quilt" | "forge" | "neoforge") && instance.loader_version.is_none() {
            let versions = runtime.block_on(crate::core::loaders::list_with_source(&http, &instance.loader, &instance.version_id, self.settings.get().download_source))?;
            let newest = versions.into_iter().next().ok_or_else(|| {
                AppError::new("loader", crate::tr!("{} 还不支持 Minecraft {}，换一个版本试试。", "{} doesn't support Minecraft {} yet; try another version.", instance.loader, instance.version_id))
            })?;
            instance.loader_version = Some(newest.version);
            self.instances.update(instance)?;
        }
        Ok(base)
    }

    /// The Java a world's server runs on: a project's from its version alone.
    pub(super) fn server_java_major(&self, instance: &Instance) -> AppResult<u32> {
        let version = if instance.server.is_some() {
            std::fs::read(self.paths.version_json(&instance.version_id))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<VersionJson>(&bytes).ok())
                .ok_or_else(|| AppError::new("version_missing", pick("版本文件不存在。", "The version file is missing.")))?
        } else {
            super::load_version(&self.paths, instance)?
        };
        Ok(crate::core::mojang::version::java_major(&version))
    }

    /// `islet.sh`, the Java it installs on a Linux host of either kind, and
    /// how to use them, beside a project's server.
    pub(super) fn write_deploy_kit(&self, destination: &Path, java_major: u32, cancel: &CancellationToken) -> AppResult<()> {
        let runtime = tokio::runtime::Handle::current();
        let http = self.http()?;
        let source = self.settings.get().download_source;
        std::fs::write(destination.join("islet.sh"), crate::core::remote::SCRIPT)?;
        std::fs::create_dir_all(destination.join(".islet"))?;
        for arch in ["x86_64", "aarch64"] {
            match runtime.block_on(crate::core::java::runtime::linux_runtime(&http, java_major, arch, source, cancel)) {
                Ok(java) => std::fs::write(destination.join(".islet").join(format!("java-{arch}")), format!("{}\n{}\n", java.sha256, java.urls.join("\n")))?,
                // `islet.sh setup` then asks for a Java on that kind of host.
                Err(error) => tracing::warn!(%error, arch, "no Java listed for the server kit"),
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(destination.join("islet.sh"), std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::write(destination.join(pick("开服说明.txt", "HOW-TO-RUN.txt")), pick(KIT_README_ZH, KIT_README_EN))?;
        Ok(())
    }
}

const KIT_README_ZH: &str = "\
这个文件夹就是一台完整的 Minecraft 服务器，由草岛 Islet 生成。

在 Linux 云服务器上（推荐）
1. 把整个文件夹上传到服务器，例如：scp -r 文件夹 用户@服务器IP:~/
2. 登录服务器，进入文件夹，运行：sh islet.sh setup
   它会自动装好合适的 Java，按这台机器的内存设置服务器内存和视距，
   启动服务器，并设成开机自启、每晚自动备份。
3. 在云服务商控制台的安全组（防火墙）里放行 server.properties 里的端口（默认 TCP 25565）。

以后常用：
  sh islet.sh status | stop | start | restart
  sh islet.sh cmd \"say 你好\"     向服务器控制台输入命令
  sh islet.sh log 100            看最近 100 行日志
  sh islet.sh backup             立即备份世界到 backups/
  sh islet.sh memory 4096        固定内存（MB），0 为自动

在 Windows 或 macOS 上
  双击 start.bat（Windows）或运行 sh start.sh。需要已安装对应版本的 Java。

也可以在草岛的「开服」页面里填好服务器的 SSH 信息，一键上传和管理。
";

const KIT_README_EN: &str = "\
This folder is a complete Minecraft server, made by the Islet launcher.

On a Linux server (recommended)
1. Upload the whole folder, for example: scp -r folder user@server-ip:~/
2. Log in, go into the folder and run: sh islet.sh setup
   It installs the right Java, sizes the server's memory and view distance
   to the machine, starts the server, and makes it start at boot and back
   up nightly.
3. Allow the port from server.properties (TCP 25565 by default) in the
   cloud provider's security group (firewall).

Later:
  sh islet.sh status | stop | start | restart
  sh islet.sh cmd \"say hello\"   type a command into the server console
  sh islet.sh log 100            the last 100 lines of the log
  sh islet.sh backup             back the world up to backups/ now
  sh islet.sh memory 4096        fixed memory in MB; 0 for automatic

On Windows or macOS
  Run start.bat (Windows) or sh start.sh, with the right Java installed.

Islet's server page can also upload and manage it over SSH for you.
";
