//! Desktop shortcuts that start one world: the launcher runs with
//! `--launch <world id>`. If it is already open, the single-instance plugin
//! hands the argument over and the running launcher starts the world.
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use islet::core::error::{AppError, ErrorPayload};
use islet::core::paths::AppPaths;
use tauri::State;

use crate::LauncherState;

/// The world asked for on this start, handed to the page once.
static STARTUP: Mutex<Option<String>> = Mutex::new(None);

/// `--launch <id>` in a command line, when the id is a valid world id.
pub fn launch_argument(argv: &[String]) -> Option<String> {
    let at = argv.iter().position(|arg| arg == "--launch")?;
    let id = argv.get(at + 1)?;
    AppPaths::validate_instance_id(id).ok().map(|_| id.clone())
}

/// Called once at start with the process arguments.
pub fn remember_startup(argv: &[String]) {
    if let Ok(mut slot) = STARTUP.lock() {
        *slot = launch_argument(argv);
    }
}

#[tauri::command]
pub fn take_startup_launch() -> Option<String> {
    STARTUP.lock().ok().and_then(|mut slot| slot.take())
}

fn fail(message: impl Into<String>, detail: Option<String>) -> ErrorPayload {
    match detail {
        Some(detail) => AppError::detail("shortcut", message, detail).to_payload(),
        None => AppError::new("shortcut", message).to_payload(),
    }
}

/// A file name for the shortcut: the world's name without characters that
/// are not allowed in file names.
fn file_stem(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if r#"/\:*?"<>|"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() { "Minecraft".into() } else { cleaned }
}

fn desktop() -> Result<PathBuf, ErrorPayload> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let desktop = home.map(|h| h.join("Desktop")).filter(|d| d.is_dir());
    desktop.ok_or_else(|| fail(islet::core::i18n::pick("找不到桌面文件夹。", "Couldn't find the Desktop folder."), None))
}

/// Puts a shortcut for the world on the desktop and returns its path.
#[tauri::command]
pub async fn create_shortcut(state: State<'_, LauncherState>, id: String) -> Result<String, ErrorPayload> {
    let instance = state.services().instances.get(&id).map_err(|e| e.to_payload())?;
    let exe = std::env::current_exe().map_err(|e| fail(islet::core::i18n::pick("找不到启动器程序。", "Couldn't find the launcher program."), Some(e.to_string())))?;
    let stem = file_stem(&instance.name);
    let path = write_shortcut(&desktop()?, &stem, &exe, &id)?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(target_os = "macos")]
fn write_shortcut(desktop: &Path, stem: &str, exe: &Path, id: &str) -> Result<PathBuf, ErrorPayload> {
    // A tiny app bundle whose program starts the launcher with the world.
    // Made here, it carries no download quarantine and opens directly.
    let app = desktop.join(format!("{stem}.app"));
    if app.exists() {
        return Err(fail(islet::tr!("桌面上已经有「{stem}」了。", "\"{stem}\" is already on the Desktop."), None));
    }
    let contents = app.join("Contents");
    let io = |e: std::io::Error| fail(islet::core::i18n::pick("创建快捷方式失败。", "Couldn't create the shortcut."), Some(e.to_string()));
    std::fs::create_dir_all(contents.join("MacOS")).map_err(io)?;
    std::fs::create_dir_all(contents.join("Resources")).map_err(io)?;
    let quote = |p: &str| format!("'{}'", p.replace('\'', r"'\''"));
    let script = format!("#!/bin/sh\nexec {} --launch {} >/dev/null 2>&1 &\n", quote(&exe.to_string_lossy()), quote(id));
    let program = contents.join("MacOS/start");
    std::fs::write(&program, script).map_err(io)?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).map_err(io)?;
    }
    // The launcher's own icon, when it runs from an app bundle.
    let icon = exe.parent().and_then(Path::parent).map(|c| c.join("Resources/icon.icns")).filter(|p| p.is_file());
    if let Some(icon) = &icon {
        let _ = std::fs::copy(icon, contents.join("Resources/icon.icns"));
    }
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>start</string>
<key>CFBundleIdentifier</key><string>com.saudade.mclauncher.shortcut.{}</string>
<key>CFBundleName</key><string>{}</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleIconFile</key><string>icon</string>
<key>LSUIElement</key><true/>
</dict></plist>
"#,
        id.replace('-', ""),
        stem.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
    );
    std::fs::write(contents.join("Info.plist"), plist).map_err(io)?;
    Ok(app)
}

#[cfg(target_os = "windows")]
fn write_shortcut(desktop: &Path, stem: &str, exe: &Path, id: &str) -> Result<PathBuf, ErrorPayload> {
    use std::os::windows::process::CommandExt;
    let link = desktop.join(format!("{stem}.lnk"));
    if link.exists() {
        return Err(fail(islet::tr!("桌面上已经有「{stem}」了。", "\"{stem}\" is already on the Desktop."), None));
    }
    // Paths go in as environment variables, so no quoting games in the script.
    let script = "$s = (New-Object -ComObject WScript.Shell).CreateShortcut($env:MCL_LINK); \
                  $s.TargetPath = $env:MCL_EXE; $s.Arguments = '--launch ' + $env:MCL_ID; \
                  $s.IconLocation = $env:MCL_EXE + ',0'; $s.WorkingDirectory = Split-Path $env:MCL_EXE; $s.Save()";
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("MCL_LINK", &link)
        .env("MCL_EXE", exe)
        .env("MCL_ID", id)
        .creation_flags(0x08000000)
        .status()
        .map_err(|e| fail(islet::core::i18n::pick("创建快捷方式失败。", "Couldn't create the shortcut."), Some(e.to_string())))?;
    if !status.success() || !link.is_file() {
        return Err(fail(islet::core::i18n::pick("创建快捷方式失败。", "Couldn't create the shortcut."), None));
    }
    Ok(link)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn write_shortcut(desktop: &Path, stem: &str, exe: &Path, id: &str) -> Result<PathBuf, ErrorPayload> {
    use std::os::unix::fs::PermissionsExt;
    let file = desktop.join(format!("{stem}.desktop"));
    if file.exists() {
        return Err(fail(islet::tr!("桌面上已经有「{stem}」了。", "\"{stem}\" is already on the Desktop."), None));
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName={stem}\nExec=\"{}\" --launch {id}\nIcon=islet\nTerminal=false\nCategories=Game;\n",
        exe.to_string_lossy().replace('"', "\\\"")
    );
    let io = |e: std::io::Error| fail(islet::core::i18n::pick("创建快捷方式失败。", "Couldn't create the shortcut."), Some(e.to_string()));
    std::fs::write(&file, entry).map_err(io)?;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).map_err(io)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_arguments_need_a_world_id() {
        let id = "0f8fad5b-d9cb-469f-a165-70867728950e";
        assert_eq!(launch_argument(&["mc".into(), "--launch".into(), id.into()]).as_deref(), Some(id));
        assert_eq!(launch_argument(&["mc".into(), "--launch".into(), "../x".into()]), None);
        assert_eq!(launch_argument(&["mc".into()]), None);
        assert_eq!(file_stem("我的/世界:1"), "我的_世界_1");
    }
}
