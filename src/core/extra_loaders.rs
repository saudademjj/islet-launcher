//! LiteLoader and Cleanroom profiles from their published metadata.
use crate::core::{
    download::{self, DownloadProgress, DownloadTask},
    error::{AppError, AppResult},
    loaders::LoaderVersion,
    mojang::version::{self, JavaVersion, Library, VersionJson},
    paths::AppPaths,
};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
async fn json(client: &reqwest::Client, url: &str) -> AppResult<Value> {
    Ok(client
        .get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
fn error(message: &str) -> AppError {
    AppError::new("loader", message)
}
async fn metadata(client: &reqwest::Client, loader: &str) -> AppResult<Value> {
    json(
        client,
        if loader == "liteloader" {
            "https://bmclapi2.bangbang93.com/liteloader/list"
        } else {
            "https://api.github.com/repos/CleanroomMC/Cleanroom/releases?per_page=100"
        },
    )
    .await
}
pub async fn list(
    client: &reqwest::Client,
    loader: &str,
    minecraft: &str,
) -> AppResult<Vec<LoaderVersion>> {
    if loader == "cleanroom" && minecraft != "1.12.2" {
        return Ok(vec![]);
    }
    let value = metadata(client, loader).await?;
    Ok(value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| {
            if loader == "liteloader" {
                if v["mcversion"].as_str() != Some(minecraft) {
                    return None;
                }
                Some(LoaderVersion {
                    version: v["version"].as_str()?.into(),
                    stable: v["type"].as_str() == Some("RELEASE"),
                })
            } else {
                if v["draft"].as_bool() == Some(true)
                    || !v["assets"].as_array()?.iter().any(|a| {
                        a["name"].as_str().is_some_and(|n| n.ends_with(".zip"))
                            && a["digest"]
                                .as_str()
                                .is_some_and(|h| h.starts_with("sha256:"))
                    })
                {
                    return None;
                }
                Some(LoaderVersion {
                    version: v["tag_name"].as_str()?.trim_start_matches('v').into(),
                    stable: !v["prerelease"].as_bool().unwrap_or(false),
                })
            }
        })
        .collect())
}
pub async fn prepare(
    client: &reqwest::Client,
    paths: &AppPaths,
    base: &VersionJson,
    loader: &str,
    pinned: Option<&str>,
    cancel: &CancellationToken,
) -> AppResult<(VersionJson, Vec<DownloadTask>, String)> {
    let available = list(client, loader, &base.id).await?;
    let selected = pinned
        .or_else(|| available.first().map(|v| v.version.as_str()))
        .ok_or_else(|| error(crate::core::i18n::pick("这个游戏版本没有可用的加载器。", "No loader is available for this game version.")))?;
    if !available.iter().any(|v| v.version == selected)
        || !crate::core::fabric::valid_version(selected)
    {
        return Err(error(crate::core::i18n::pick("加载器版本与游戏不匹配。", "The loader version doesn't match the game.")));
    }
    let meta = metadata(client, loader).await?;
    let mut version = base.clone();
    let mut tasks = Vec::new();
    if loader == "liteloader" {
        let entry = meta
            .as_array()
            .into_iter()
            .flatten()
            .find(|v| {
                v["mcversion"].as_str() == Some(&base.id) && v["version"].as_str() == Some(selected)
            })
            .ok_or_else(|| error(crate::core::i18n::pick("LiteLoader 元数据缺失。", "LiteLoader metadata is missing.")))?;
        let build = &entry["build"];
        let relative = version::maven_path(&format!("com.mumfrey:liteloader:{selected}"))
            .ok_or_else(|| error("Invalid LiteLoader version"))?;
        let target = paths.library_path(&relative);
        let hash = build["md5"]
            .as_str()
            .ok_or_else(|| error(crate::core::i18n::pick("LiteLoader 缺少 MD5。", "LiteLoader is missing its MD5.")))?;
        let valid = std::fs::read(&target)
            .ok()
            .is_some_and(|bytes| format!("{:x}", md5::compute(bytes)) == hash);
        if !valid {
            if target.exists() {
                std::fs::remove_file(&target)?;
            }
            let task = DownloadTask::new(
                "LiteLoader",
                format!("https://dl.liteloader.com/versions/{relative}"),
                vec![
                    format!("https://repo.mumfrey.com/content/repositories/liteloader/{relative}"),
                    format!("https://bmclapi2.bangbang93.com/maven/{relative}"),
                ],
                target.clone(),
                None,
                None,
            );
            download::download_all(
                vec![task.clone()],
                client.clone(),
                1,
                cancel.clone(),
                Arc::new(DownloadProgress::new(&[task])),
            )
            .await?;
            if format!("{:x}", md5::compute(std::fs::read(&target)?)) != hash {
                std::fs::remove_file(&target)?;
                return Err(error(crate::core::i18n::pick("LiteLoader MD5 校验失败。", "LiteLoader failed its MD5 check.")));
            }
        }
        let mut libraries = build["libraries"].as_array().cloned().unwrap_or_default();
        libraries.push(serde_json::json!({"name": format!("com.mumfrey:liteloader:{selected}")}));
        for raw in libraries {
            let name = raw["name"]
                .as_str()
                .ok_or_else(|| error("Invalid LiteLoader library"))?;
            let relative =
                version::maven_path(name).ok_or_else(|| error("Invalid library path"))?;
            crate::core::java::runtime::safe_archive_path(&relative)?;
            if !paths.library_path(&relative).is_file() {
                let root = raw["url"]
                    .as_str()
                    .unwrap_or("https://libraries.minecraft.net/")
                    .replace("http://", "https://");
                if !root.starts_with("https://") {
                    return Err(error("Invalid library URL"));
                }
                tasks.push(DownloadTask::new(
                    name,
                    format!("{}/{relative}", root.trim_end_matches('/')),
                    vec![format!("https://repo.maven.apache.org/maven2/{relative}")],
                    paths.library_path(&relative),
                    None,
                    None,
                ));
            }
            replace(&mut version, serde_json::from_value(raw)?);
        }
        version.main_class = "net.minecraft.launchwrapper.Launch".into();
        tweak(
            &mut version,
            build["tweakClass"]
                .as_str()
                .unwrap_or("com.mumfrey.liteloader.launch.LiteLoaderTweaker"),
        );
    } else {
        let release = meta
            .as_array()
            .into_iter()
            .flatten()
            .find(|v| {
                v["tag_name"]
                    .as_str()
                    .is_some_and(|v| v.trim_start_matches('v') == selected)
            })
            .ok_or_else(|| error("Cleanroom release missing"))?;
        let asset = release["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|a| a["name"].as_str() == Some(&format!("cleanroom-{selected}.zip")))
            .ok_or_else(|| error("Cleanroom profile archive missing"))?;
        let url = asset["browser_download_url"]
            .as_str()
            .filter(|u| {
                u.starts_with("https://github.com/CleanroomMC/Cleanroom/releases/download/")
            })
            .ok_or_else(|| error("Invalid Cleanroom asset URL"))?;
        let hash = asset["digest"]
            .as_str()
            .and_then(|h| h.strip_prefix("sha256:"))
            .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| error("Cleanroom release has no SHA-256"))?;
        let file = paths.cache.join(format!("cleanroom-{selected}.zip"));
        let task = DownloadTask::new(
            "Cleanroom",
            url,
            vec![],
            file.clone(),
            None,
            asset["size"].as_u64(),
        )
        .with_sha256(hash.into());
        download::download_all(
            vec![task.clone()],
            client.clone(),
            1,
            cancel.clone(),
            Arc::new(DownloadProgress::new(&[task])),
        )
        .await?;
        let mut zip =
            zip::ZipArchive::new(std::fs::File::open(file)?).map_err(|e| error(&e.to_string()))?;
        let profile: Value = serde_json::from_reader(
            zip.by_name("patches/net.minecraftforge.json")
                .map_err(|e| error(&e.to_string()))?,
        )?;
        let lwjgl: Value = serde_json::from_reader(
            zip.by_name("patches/org.lwjgl.json")
                .map_err(|e| error(&e.to_string()))?,
        )?;
        let mut libraries: Vec<Value> =
            profile["libraries"].as_array().cloned().unwrap_or_default();
        libraries.extend(lwjgl["libraries"].as_array().cloned().unwrap_or_default());
        let keys: HashSet<String> = libraries
            .iter()
            .filter_map(|l| l["name"].as_str().map(key))
            .collect();
        version.libraries.retain(|l| !keys.contains(&key(&l.name)));
        for raw in libraries {
            if raw["MMC-hint"].as_str() == Some("local") {
                continue;
            } // removal stubs replace vanilla dependencies
            let mut library: Library = serde_json::from_value(raw)?;
            let coordinate = library.name.clone();
            if let Some(downloads) = &mut library.downloads {
                if let Some(artifact) = &mut downloads.artifact {
                    artifact.path = artifact
                        .path
                        .take()
                        .or_else(|| version::maven_path(&coordinate));
                }
                if let Some(classifiers) = &mut downloads.classifiers {
                    for (classifier, artifact) in classifiers {
                        artifact.path = artifact
                            .path
                            .take()
                            .or_else(|| version::maven_path(&format!("{coordinate}:{classifier}")));
                    }
                }
            }
            version.libraries.push(library);
        }
        version.main_class = profile["mainClass"]
            .as_str()
            .ok_or_else(|| error("Cleanroom main class missing"))?
            .into();
        let major = profile["compatibleJavaMajors"]
            .as_array()
            .and_then(|v| v.first())
            .and_then(Value::as_u64)
            .ok_or_else(|| error("Cleanroom Java requirement missing"))? as u32;
        version.java_version = Some(JavaVersion {
            component: profile["compatibleJavaName"]
                .as_str()
                .unwrap_or("java-runtime-epsilon")
                .into(),
            major_version: major,
        });
        for tweaker in profile["+tweakers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            tweak(&mut version, tweaker);
        }
        let arch = version::game_arch(&version);
        let env = crate::core::mojang::rules::RuleEnv {
            arch,
            custom_resolution: false,
        };
        for library in &version.libraries {
            if !crate::core::mojang::rules::rules_allow_in(&library.rules, env) {
                continue;
            }
            let artifact = library.downloads.as_ref().and_then(|d| d.artifact.as_ref());
            for info in artifact
                .into_iter()
                .chain(version::native_download_for(library, arch))
            {
                let path = info
                    .path
                    .as_ref()
                    .ok_or_else(|| error("Library path missing"))?;
                crate::core::java::runtime::safe_archive_path(path)?;
                let url = info
                    .url
                    .as_ref()
                    .filter(|u| u.starts_with("https://"))
                    .ok_or_else(|| error("Invalid library URL"))?;
                tasks.push(DownloadTask::new(
                    &library.name,
                    url,
                    vec![],
                    paths.library_path(path),
                    info.sha1.clone(),
                    info.size,
                ));
            }
        }
    }
    Ok((version, tasks, selected.into()))
}
fn key(name: &str) -> String {
    name.split(':').take(2).collect::<Vec<_>>().join(":")
}
fn replace(version: &mut VersionJson, library: Library) {
    version
        .libraries
        .retain(|l| key(&l.name) != key(&library.name));
    version.libraries.push(library);
}
fn tweak(version: &mut VersionJson, tweaker: &str) {
    if let Some(args) = &mut version.minecraft_arguments {
        args.push_str(&format!(" --tweakClass {tweaker}"));
    } else {
        version
            .arguments
            .get_or_insert_with(Default::default)
            .game
            .extend([
                version::Argument::Plain("--tweakClass".into()),
                version::Argument::Plain(tweaker.into()),
            ]);
    }
}
