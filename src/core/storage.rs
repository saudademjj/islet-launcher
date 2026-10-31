use crate::core::{
    error::{AppError, AppResult},
    util::atomic_write,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Copy first, publish only after every file succeeds. The source remains a backup.
pub fn relocate(source: &Path, destination: &Path, locator: &Path) -> AppResult<PathBuf> {
    if !destination.is_absolute() {
        return Err(AppError::new("storage", crate::core::i18n::pick("请选择绝对路径。", "Choose an absolute path.")));
    }
    fs::create_dir_all(destination)?;
    let source_alias = source.to_path_buf();
    let source = fs::canonicalize(source)?;
    let target = fs::canonicalize(destination)?;
    if target.starts_with(&source) || source.starts_with(&target) {
        return Err(AppError::new(
            "storage",
            crate::core::i18n::pick("新目录不能与当前数据目录相同，也不能互相包含。", "The new folder can't be the current data folder, inside it, or contain it."),
        ));
    }
    if fs::read_dir(&target)?.next().is_some() {
        return Err(AppError::new(
            "storage",
            crate::core::i18n::pick("请选择空目录；已有文件不会被覆盖。", "Choose an empty folder; existing files are never overwritten."),
        ));
    }
    let stage = target.with_file_name(format!(".mc-migrate-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        copy_tree(&source, &stage, true)?;
        rebase_java_paths(&stage, &source, &source_alias, &target)?;
        // A running log belongs to this session and can stay at the old root.
        if !stage.join("settings.json").exists() {
            atomic_write(&stage.join("settings.json"), b"{}")?;
        }
        fs::remove_dir(&target)?;
        fs::rename(&stage, &target)?;
        let plain = PathBuf::from(crate::core::platform::simplify_verbatim(
            &target.to_string_lossy(),
        ));
        atomic_write(locator, &serde_json::to_vec_pretty(&plain)?)?;
        Ok(plain)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

fn copy_tree(source: &Path, target: &Path, top: bool) -> AppResult<()> {
    fs::create_dir_all(target)?;
    for item in fs::read_dir(source)? {
        let item = item?;
        let name = item.file_name();
        if top && (name == "logs" || name == "storage-location.json") {
            continue;
        }
        let kind = item.file_type()?;
        if kind.is_symlink() {
            return Err(AppError::new(
                "storage",
                crate::core::i18n::pick("数据目录包含符号链接，请手动迁移后重试。", "The data folder has symbolic links; move it by hand and try again."),
            ));
        }
        if kind.is_dir() {
            copy_tree(&item.path(), &target.join(name), false)?;
        } else if kind.is_file() {
            fs::copy(item.path(), target.join(name))?;
        }
    }
    Ok(())
}

fn rebase_java_paths(
    stage: &Path,
    source: &Path,
    source_alias: &Path,
    target: &Path,
) -> AppResult<()> {
    let plain_source = PathBuf::from(crate::core::platform::simplify_verbatim(
        &source.to_string_lossy(),
    ));
    let plain_target = PathBuf::from(crate::core::platform::simplify_verbatim(
        &target.to_string_lossy(),
    ));
    let mut files = vec![stage.join("settings.json")];
    if let Ok(entries) = fs::read_dir(stage.join("instances")) {
        files.extend(entries.flatten().map(|e| e.path().join("instance.json")));
    }
    for file in files.into_iter().filter(|p| p.is_file()) {
        let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&file)?)?;
        if let Some(path) = value
            .get("javaPath")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
        {
            let canonical = fs::canonicalize(&path).ok().map(|p| {
                PathBuf::from(crate::core::platform::simplify_verbatim(
                    &p.to_string_lossy(),
                ))
            });
            if let Some(relative) = path
                .strip_prefix(&plain_source)
                .ok()
                .or_else(|| path.strip_prefix(source_alias).ok())
                .or_else(|| {
                    canonical
                        .as_ref()
                        .and_then(|p| p.strip_prefix(&plain_source).ok())
                })
            {
                value["javaPath"] = serde_json::Value::String(
                    plain_target.join(relative).to_string_lossy().into_owned(),
                );
                atomic_write(&file, &serde_json::to_vec_pretty(&value)?)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_preserves_original_and_rejects_nonempty_or_nested_targets() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("old");
        fs::create_dir_all(source.join("instances/a/minecraft/saves")).unwrap();
        fs::write(
            source.join("instances/a/minecraft/saves/world"),
            b"world-data",
        )
        .unwrap();
        fs::create_dir_all(source.join("instances/a/minecraft/saves/logs")).unwrap();
        fs::write(
            source.join("instances/a/minecraft/saves/logs/level.dat"),
            b"world",
        )
        .unwrap();
        let java = source.join("cache/java/jre/bin/java.exe");
        let config = serde_json::to_vec(&serde_json::json!({"javaPath":java})).unwrap();
        fs::write(source.join("settings.json"), &config).unwrap();
        fs::write(source.join("instances/a/instance.json"), &config).unwrap();
        let target = tmp.path().join("new");
        let locator = tmp.path().join("config/location.json");
        relocate(&source, &target, &locator).unwrap();
        assert_eq!(
            fs::read(target.join("instances/a/minecraft/saves/world")).unwrap(),
            b"world-data"
        );
        assert!(source.join("instances/a/minecraft/saves/world").exists());
        assert_eq!(
            fs::read(target.join("instances/a/minecraft/saves/logs/level.dat")).unwrap(),
            b"world"
        );
        for name in ["settings.json", "instances/a/instance.json"] {
            let config: serde_json::Value =
                serde_json::from_slice(&fs::read(target.join(name)).unwrap()).unwrap();
            let root = fs::canonicalize(&target).unwrap();
            let root = PathBuf::from(crate::core::platform::simplify_verbatim(
                &root.to_string_lossy(),
            ));
            assert_eq!(
                PathBuf::from(config["javaPath"].as_str().unwrap()),
                root.join("cache/java/jre/bin/java.exe")
            );
        }
        assert!(relocate(&source, &target, &locator).is_err());
        assert!(relocate(&source, &source.join("nested"), &locator).is_err());
    }
}
