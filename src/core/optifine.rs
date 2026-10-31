//! OptiFine as a mod. On Forge the OptiFine jar loads straight from `mods`;
//! on Fabric and Quilt it needs OptiFabric next to it. Builds come from
//! BMCLAPI's OptiFine mirror (optifine.net has no API); a vanilla world is
//! patched through the OptiFine distribution installer when requested.
use std::{path::Path, sync::Arc, time::Duration};

use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::core::{
    download::{self, DownloadProgress, DownloadTask},
    error::{AppError, AppResult},
    instances::Instance,
    paths::AppPaths,
};

const MIRROR: &str = "https://bmclapi2.bangbang93.com/optifine";

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptiFineBuild {
    #[serde(rename(deserialize = "mcversion"))]
    pub minecraft: String,
    #[serde(rename(deserialize = "type"))]
    pub edition: String,
    pub patch: String,
    pub filename: String,
    /// The Forge build it was made for, e.g. "Forge 47.1.0".
    #[serde(default)]
    pub forge: Option<String>,
}

impl OptiFineBuild {
    fn preview(&self) -> bool {
        self.patch.starts_with("pre") || self.filename.contains("preview")
    }
}

/// Builds for one Minecraft version, newest and non-preview first.
pub async fn builds(client: &reqwest::Client, minecraft: &str) -> AppResult<Vec<OptiFineBuild>> {
    if !crate::core::fabric::valid_version(minecraft) {
        return Err(AppError::new("optifine", crate::core::i18n::pick("Minecraft 版本无效。", "Invalid Minecraft version.")));
    }
    let mut list: Vec<OptiFineBuild> = client
        .get(format!("{MIRROR}/{minecraft}"))
        .timeout(Duration::from_secs(30))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    list.retain(|build| build.minecraft == minecraft && safe_name(&build.filename));
    // The mirror lists oldest first.
    list.reverse();
    list.sort_by_key(|build| build.preview());
    Ok(list)
}

fn safe_name(name: &str) -> bool {
    name.ends_with(".jar")
        && name.len() <= 120
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))
}

/// Puts the newest suitable OptiFine into the world's `mods`. Returns its
/// file name. Fabric and Quilt also need OptiFabric, installed by the caller.
pub async fn install(client: &reqwest::Client, paths: &AppPaths, instance: &Instance, cancel: &CancellationToken) -> AppResult<String> {
    if !["forge", "fabric", "quilt"].contains(&instance.loader.as_str()) {
        return Err(AppError::new(
            "optifine",
            crate::core::i18n::pick("原版和 NeoForge 不能把 OptiFine 当模组加载。请换成 Forge，或用 Fabric + Iris / Sodium 代替。", "Vanilla and NeoForge can't load OptiFine as a mod. Switch to Forge, or use Fabric + Iris / Sodium instead."),
        ));
    }
    let list = builds(client, &instance.version_id).await?;
    let build = list
        .first()
        .ok_or_else(|| AppError::new("optifine", crate::tr!("OptiFine 还没有适配 {} 的版本。", "OptiFine has no version for {} yet.", instance.version_id)))?;
    let mods = paths.instance_game_dir(&instance.id).join("mods");
    std::fs::create_dir_all(&mods)?;
    let target = mods.join(&build.filename);
    if target.is_file() {
        return Ok(build.filename.clone());
    }
    let stage = paths.instance_dir(&instance.id).join(".optifine-download");
    std::fs::create_dir_all(&stage)?;
    let task = DownloadTask::new(
        format!("OptiFine {}", build.patch),
        format!("{MIRROR}/{}/{}/{}", build.minecraft, build.edition, build.patch),
        (0..3).map(|_| format!("{MIRROR}/{}/{}/{}?retry={}", build.minecraft, build.edition, build.patch, uuid::Uuid::new_v4())).collect(),
        stage.join(&build.filename),
        None,
        None,
    );
    let tasks = vec![task];
    let progress = Arc::new(DownloadProgress::new(&tasks));
    let result = download::download_all(tasks, client.clone(), 1, cancel.clone(), progress).await;
    if result.is_ok() && !cancel.is_cancelled() && looks_like_jar(&stage.join(&build.filename)) {
        std::fs::rename(stage.join(&build.filename), &target)?;
    }
    let _ = std::fs::remove_dir_all(&stage);
    result?;
    if cancel.is_cancelled() {
        return Err(AppError::new("cancelled", crate::core::i18n::pick("下载已取消。", "Download cancelled.")));
    }
    if !target.is_file() {
        return Err(AppError::new("optifine", crate::core::i18n::pick("下载的 OptiFine 文件不完整，请重试。", "The downloaded OptiFine file is incomplete; try again.")));
    }
    // An OptiFine already there is replaced, never doubled.
    for entry in std::fs::read_dir(&mods)?.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.starts_with("optifine") && (name.ends_with(".jar") || name.ends_with(".jar.disabled")) && name != build.filename.to_lowercase() {
            crate::core::manage::recycle_item(paths, &instance.id, crate::core::manage::ItemKind::Mod, &entry.file_name().to_string_lossy(), None)?;
        }
    }
    Ok(build.filename.clone())
}

/// No checksum is published, so at least make sure it is a zip with
/// OptiFine's own classes in it.
fn looks_like_jar(path: &Path) -> bool {
    std::fs::File::open(path)
        .ok()
        .and_then(|file| zip::ZipArchive::new(file).ok())
        .is_some_and(|mut zip| zip.by_name("optifine/Config.class").is_ok() || zip.by_name("net/optifine/Config.class").is_ok() || (zip.by_name("Config.class").is_ok() && zip.by_name("optifine/Installer.class").is_ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_entries_parse_and_names_are_checked() {
        let json = r#"[{"mcversion":"1.20.1","type":"HD_U","patch":"I6","filename":"OptiFine_1.20.1_HD_U_I6.jar","forge":"Forge 47.1.0"},
                       {"mcversion":"1.20.1","type":"HD_U","patch":"pre1_I7","filename":"preview_OptiFine_1.20.1_HD_U_I7_pre1.jar"}]"#;
        let list: Vec<OptiFineBuild> = serde_json::from_str(json).unwrap();
        assert_eq!(list[0].edition, "HD_U");
        assert!(!list[0].preview());
        assert!(list[1].preview());
        assert!(safe_name(&list[0].filename));
        assert!(!safe_name("../evil.jar"));
    }
}

/// Use the distribution's own patcher for a vanilla profile. This produces
/// its patched libraries without a Forge or Fabric requirement.
pub async fn install_vanilla(client: &reqwest::Client, paths: &AppPaths, instance: &Instance, cancel: &CancellationToken) -> AppResult<String> {
    if !instance.installed { return Err(AppError::new("optifine", "请先安装原版游戏，再安装 OptiFine。")); }
    let build = builds(client, &instance.version_id).await?.into_iter().next().ok_or_else(|| AppError::new("optifine", "没有匹配的 OptiFine。"))?;
    let stage = paths.cache.join("optifine"); std::fs::create_dir_all(&stage)?;
    let installer = stage.join(&build.filename);
    let task = DownloadTask::new("OptiFine", format!("{MIRROR}/{}/{}/{}", build.minecraft, build.edition, build.patch), (0..3).map(|_| format!("{MIRROR}/{}/{}/{}?retry={}", build.minecraft, build.edition, build.patch, uuid::Uuid::new_v4())).collect(), installer.clone(), None, None);
    download::download_all(vec![task.clone()], client.clone(), 1, cancel.clone(), Arc::new(DownloadProgress::new(&[task]))).await?;
    if !looks_like_jar(&installer) { return Err(AppError::new("optifine", "OptiFine 安装器无效。")); }
    let base: crate::core::mojang::version::VersionJson = serde_json::from_slice(&std::fs::read(paths.version_json(&instance.version_id))?)?;
    let dir = paths.version_cache_dir(&base.id); std::fs::create_dir_all(&dir)?;
    crate::core::util::atomic_write(&dir.join(format!("{}.json", base.id)), &serde_json::to_vec(&base)?)?;
    let jar = dir.join(format!("{}.jar", base.id));
    if !jar.exists() { std::fs::copy(paths.client_jar(&base.id), &jar)?; }
    let profiles = paths.cache.join("launcher_profiles.json");
    if !profiles.exists() { crate::core::util::atomic_write(&profiles, br#"{"profiles":{}}"#)?; }
    let helper = stage.join("InstallOptiFine.java");
    crate::core::util::atomic_write(&helper, br#"class InstallOptiFine { public static void main(String[] a) throws Exception { Class.forName("optifine.Installer").getMethod("doInstall", java.io.File.class).invoke(null, new java.io.File(a[0])); } }"#)?;
    let java = crate::core::java::runtime::ensure_java(client, paths, 17, None, crate::core::platform::normalize_arch(std::env::consts::ARCH), cancel, None).await?;
    let mut command = tokio::process::Command::from(crate::core::platform::quiet_command(&java.path));
    command.args(["-Djava.awt.headless=true", "-cp"]).arg(&installer).arg(&helper).arg(&paths.cache).current_dir(&stage).kill_on_drop(true);
    command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let output = tokio::select! { _ = cancel.cancelled() => return Err(AppError::new("cancelled", "Cancelled")), result = tokio::time::timeout(Duration::from_secs(180), command.output()) => result.map_err(|_| AppError::new("optifine", "OptiFine 安装超时。"))?? };
    if !output.status.success() { return Err(AppError::detail("optifine", "OptiFine 原版安装失败。", String::from_utf8_lossy(&output.stderr).to_string())); }
    let id = format!("{}-OptiFine_{}_{}", base.id, build.edition, build.patch);
    let profile = paths.versions.join(&id).join(format!("{id}.json"));
    let overlay: crate::core::mojang::version::VersionJson = serde_json::from_slice(&std::fs::read(profile)?)?;
    let merged = crate::core::loaders::merge(&base, overlay)?;
    crate::core::util::atomic_write(&paths.instance_dir(&instance.id).join("launch-version.json"), &serde_json::to_vec_pretty(&merged)?)?;
    crate::core::util::atomic_write(&paths.instance_dir(&instance.id).join("optifine.json"), &serde_json::to_vec(&serde_json::json!({"minecraft": instance.version_id}))?)?;
    Ok(build.filename)
}
