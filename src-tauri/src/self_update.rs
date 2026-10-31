//! Replacing the launcher with a downloaded update. On macOS the new app is
//! staged before quitting and the previous bundle kept for recovery; the
//! helper restores it if the final copy fails. On Linux an AppImage is
//! swapped for the new one. Later runs clear what updates left behind.
#[cfg(target_os = "macos")]
const REPLACE_SCRIPT: &[u8] = br#"#!/bin/sh
pid="$1"; staged="$2"; target="$3"; backup="$4"
while kill -0 "$pid" 2>/dev/null; do sleep 1; done
if [ -e "$backup" ]; then exit 1; fi
mv "$target" "$backup" || exit 1
if /usr/bin/ditto "$staged" "$target"; then
    /usr/bin/open "$target"
else
    rm -rf "$target"
    mv "$backup" "$target"
    /usr/bin/open "$target"
fi
"#;

#[cfg(target_os = "macos")]
pub async fn prepare(
    file: &std::path::Path,
    cache: &std::path::Path,
) -> Result<std::process::Command, String> {
    use std::{path::PathBuf, process::Stdio};
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = exe
        .ancestors()
        .nth(3)
        .filter(|p| p.extension().is_some_and(|x| x == "app"))
        .ok_or(islet::core::i18n::pick("自动替换需要从已安装的 .app 运行。", "Automatic replacement needs the launcher to run from an installed .app."))?
        .to_path_buf();
    let parent = target.parent().ok_or(islet::core::i18n::pick("应用位置无效。", "Invalid app location."))?;
    let work = cache.join(format!("update-{}", std::process::id()));
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    // Check write access before the current application exits.
    let probe = parent.join(format!(".mc-update-probe-{}", std::process::id()));
    std::fs::write(&probe, b"").map_err(|_| {
        islet::core::i18n::pick(
            "应用所在文件夹不可写，请把启动器移到你有写入权限的文件夹后重试。",
            "The app's folder isn't writable; move the launcher to a folder you can write to and try again.",
        )
        .to_string()
    })?;
    std::fs::remove_file(probe).map_err(|e| e.to_string())?;
    let mount = work.join("mounted");
    std::fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
    let result = tokio::process::Command::new("/usr/bin/hdiutil")
        .args(["attach", "-readonly", "-nobrowse", "-mountpoint"])
        .arg(&mount)
        .arg(file)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).into());
    }
    let staged = work.join("Islet.app");
    let stage_result = async {
        let bundles: Vec<PathBuf> = std::fs::read_dir(&mount)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "app"))
            .collect();
        if bundles.len() != 1 {
            return Err(islet::core::i18n::pick("更新镜像必须包含唯一的应用。", "The update image must contain exactly one app.").into());
        }
        let info = bundles[0].join("Contents/Info.plist");
        let id = tokio::process::Command::new("/usr/libexec/PlistBuddy")
            .args(["-c", "Print :CFBundleIdentifier"])
            .arg(info)
            .output()
            .await
            .map_err(|e| e.to_string())?;
        if !id.status.success()
            || String::from_utf8_lossy(&id.stdout).trim() != "com.saudade.mclauncher"
        {
            return Err(islet::core::i18n::pick("更新包的应用标识不匹配。", "The update's bundle identifier doesn't match.").into());
        }
        let copied = tokio::process::Command::new("/usr/bin/ditto")
            .arg(&bundles[0])
            .arg(&staged)
            .status()
            .await
            .map_err(|e| e.to_string())?;
        if !copied.success() {
            return Err(islet::core::i18n::pick("无法暂存更新应用。", "Couldn't stage the updated app.").into());
        }
        Ok::<_, String>(())
    }
    .await;
    let _ = tokio::process::Command::new("/usr/bin/hdiutil")
        .arg("detach")
        .arg(&mount)
        .status()
        .await;
    stage_result?;
    let script = work.join("replace.sh");
    std::fs::write(&script, REPLACE_SCRIPT).map_err(|e| e.to_string())?;
    let backup = parent.join(format!(
        "Islet.previous-{}.app.backup",
        std::process::id()
    ));
    if backup.exists() {
        return Err(islet::core::i18n::pick("旧版本备份已存在，请稍后重试。", "A backup of the old version already exists; try again later.").into());
    }
    let mut helper = std::process::Command::new("/bin/sh");
    helper
        .arg(script)
        .arg(std::process::id().to_string())
        .arg(staged)
        .arg(target)
        .arg(backup)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(helper)
}

/// On Linux, an AppImage replaces itself: the new image is copied next to
/// the running one and renamed over it (the running copy keeps its open
/// file), then a helper starts it once this process has quit.
#[cfg(target_os = "linux")]
const RESTART_SCRIPT: &str = r#"pid="$1"; target="$2"
while kill -0 "$pid" 2>/dev/null; do sleep 1; done
exec "$target""#;

#[cfg(target_os = "linux")]
pub fn prepare_appimage(file: &std::path::Path) -> Result<std::process::Command, String> {
    use islet::core::i18n::pick;
    use std::os::unix::fs::PermissionsExt;
    // The AppImage runtime names the image it was started from.
    let target = std::env::var_os("APPIMAGE")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_file())
        .ok_or(pick("自动替换需要从 AppImage 运行；其他安装方式请用下载的安装包更新。", "Automatic replacement needs the launcher to run as an AppImage; for other installs, update with the downloaded package."))?;
    let parent = target.parent().ok_or(pick("应用位置无效。", "Invalid app location."))?;
    let name = target.file_name().ok_or(pick("应用位置无效。", "Invalid app location."))?;
    let staged = parent.join(format!(".{}.update-{}", name.to_string_lossy(), std::process::id()));
    let copy = || -> std::io::Result<()> {
        std::fs::copy(file, &staged)?;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
        std::fs::File::open(&staged)?.sync_all()?;
        std::fs::rename(&staged, &target)
    };
    if let Err(error) = copy() {
        let _ = std::fs::remove_file(&staged);
        return Err(if error.kind() == std::io::ErrorKind::PermissionDenied {
            pick("AppImage 所在文件夹不可写，请把它移到你有写入权限的文件夹后重试。", "The AppImage's folder isn't writable; move it to a folder you can write to and try again.").into()
        } else {
            error.to_string()
        });
    }
    let mut helper = std::process::Command::new("/bin/sh");
    helper
        .args(["-c", RESTART_SCRIPT, "islet-restart"])
        .arg(std::process::id().to_string())
        .arg(&target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    Ok(helper)
}

/// Clears what earlier updates left behind, once this version has run for
/// a minute: macOS keeps the replaced app beside the new one in case the
/// copy fails, and every update leaves its download and work folder.
pub fn clean_up_later(cache: std::path::PathBuf) {
    let started = std::time::SystemTime::now();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(60));
        let current = format!("update-{}", std::process::id());
        let entries = |dir: &std::path::Path| std::fs::read_dir(dir).into_iter().flatten().flatten();
        for entry in entries(&cache) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("update-") && name != current {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
        // Downloads from this run stay: one may be waiting to be installed.
        for entry in entries(&cache.join("updates")) {
            if entry.metadata().and_then(|m| m.modified()).is_ok_and(|at| at < started) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        #[cfg(target_os = "macos")]
        for backup in old_backups() {
            match std::fs::remove_dir_all(&backup) {
                Ok(()) => tracing::info!(path = %backup.display(), "removed the app left from the last update"),
                Err(error) => tracing::warn!(path = %backup.display(), %error, "couldn't remove an old app backup"),
            }
        }
    });
}

/// "Islet.previous-<pid>.app.backup" bundles beside the running app.
#[cfg(target_os = "macos")]
fn old_backups() -> Vec<std::path::PathBuf> {
    let Some(app) = std::env::current_exe().ok().and_then(|exe| exe.ancestors().nth(3).map(std::path::Path::to_path_buf)) else { return Vec::new() };
    if !app.extension().is_some_and(|x| x == "app") {
        return Vec::new();
    }
    let Some(parent) = app.parent() else { return Vec::new() };
    std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            name.starts_with("Islet.previous-") && name.ends_with(".app.backup") && p.join("Contents/Info.plist").is_file()
        })
        .collect()
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    #[test]
    fn replacement_preserves_backup_and_rolls_back_a_failed_copy() {
        let root = std::env::temp_dir().join(format!("mc-update-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for fail_copy in [false, true] {
            let case = root.join(if fail_copy { "rollback" } else { "success" });
            let target = case.join("old app 'quoted'.app");
            let staged = case.join("new app.app");
            let backup = case.join("previous.app");
            std::fs::create_dir_all(&target).unwrap();
            std::fs::create_dir_all(&staged).unwrap();
            std::fs::write(target.join("version"), "old").unwrap();
            std::fs::write(staged.join("version"), "new").unwrap();
            let mut script = String::from_utf8(REPLACE_SCRIPT.to_vec())
                .unwrap()
                .replace("/usr/bin/open \"$target\"", "true");
            if fail_copy {
                script = script.replace("/usr/bin/ditto", "/usr/bin/false");
            }
            let file = case.join("apply.sh");
            std::fs::write(&file, script).unwrap();
            let status = std::process::Command::new("/bin/sh")
                .arg(file)
                .arg("99999999")
                .arg(&staged)
                .arg(&target)
                .arg(&backup)
                .status()
                .unwrap();
            assert!(status.success());
            assert_eq!(
                std::fs::read_to_string(target.join("version")).unwrap(),
                if fail_copy { "old" } else { "new" }
            );
            if !fail_copy {
                assert_eq!(
                    std::fs::read_to_string(backup.join("version")).unwrap(),
                    "old"
                );
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
