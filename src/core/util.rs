use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::core::error::{AppError, AppResult};

pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Write a file atomically: write to a temp file in the same directory, fsync,
/// then rename over the target. On Unix the file mode is 0600 because settings
/// and account metadata are private.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("data");
    let tmp = path.with_file_name(format!(".{file_name}.tmp-{}", Uuid::new_v4()));
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }
    replace_file(&tmp, path)?;
    Ok(())
}

pub fn read_json_or_default<T>(path: &Path) -> AppResult<T>
where
    T: DeserializeOwned + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(value) => Ok(value),
            Err(err) => {
                // Keep a backup of a corrupted file instead of silently losing it.
                let backup = path.with_extension(format!("corrupt-{}", now_millis()));
                let _ = fs::rename(path, &backup);
                tracing::warn!(
                    path = %path.display(),
                    backup = %backup.display(),
                    error = %err,
                    "failed to parse json, using defaults"
                );
                Ok(T::default())
            }
        },
        Err(err) => Err(AppError::detail(
            "settings_read",
            crate::core::i18n::pick("读取配置文件失败。", "Couldn't read the settings file."),
            err.to_string(),
        )),
    }
}

pub fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Replace without a delete-before-rename gap, including on Windows.
pub fn replace_file(source: &Path, dest: &Path) -> AppResult<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let dest: Vec<u16> = dest.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe {
            MoveFileExW(
                source.as_ptr(),
                dest.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(not(windows))]
    fs::rename(source, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replaces_existing_file_in_a_unicode_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("中文 空格/settings.json");
        atomic_write(&path, b"old").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(fs::read(path).unwrap(), b"new");
    }
}
