//! OS file-open requests are queued until the frontend has bootstrapped.
use std::{path::PathBuf, sync::Mutex};
use tauri::Emitter;
static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());
pub fn enqueue(paths: impl IntoIterator<Item = PathBuf>, app: Option<&tauri::AppHandle>) {
    let mut queue = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    for path in paths {
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("mrpack"))
            && path.is_file()
        {
            let path = path.to_string_lossy().to_string();
            if !queue.contains(&path) {
                queue.push(path);
            }
        }
    }
    drop(queue);
    if let Some(app) = app {
        let _ = app.emit("open-files-ready", ());
    }
}
pub fn argv(args: &[String], cwd: &str, app: Option<&tauri::AppHandle>) {
    enqueue(
        args.iter()
            .skip(1)
            .filter(|a| !a.starts_with('-'))
            .map(|a| {
                let path = tauri::Url::parse(a)
                    .ok()
                    .and_then(|url| url.to_file_path().ok())
                    .unwrap_or_else(|| PathBuf::from(a));
                if path.is_absolute() {
                    path
                } else {
                    PathBuf::from(cwd).join(path)
                }
            }),
        app,
    );
}
#[tauri::command]
pub fn take_open_files() -> Vec<String> {
    std::mem::take(&mut *PENDING.lock().unwrap_or_else(|e| e.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queues_pack_paths_and_file_urls_once_until_the_ui_is_ready() {
        let dir = std::env::temp_dir().join(format!("mc-open-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Pack with spaces.mrpack");
        std::fs::write(&file, b"fixture").unwrap();
        enqueue(
            [file.clone(), file.clone(), dir.join("missing.mrpack")],
            None,
        );
        assert_eq!(take_open_files(), vec![file.to_string_lossy()]);
        assert!(take_open_files().is_empty());
        let url = tauri::Url::from_file_path(&file).unwrap();
        argv(&["launcher".into(), url.to_string()], "", None);
        assert_eq!(take_open_files(), vec![file.to_string_lossy()]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
