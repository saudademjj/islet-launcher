//! The game's own settings file (`options.txt`): a few common options edited
//! from the launcher, and the language set before a first launch so a new
//! instance opens in Chinese instead of English. Lines the launcher does not
//! know are kept as they are.

use std::fs;
use std::path::PathBuf;

use crate::core::error::{AppError, AppResult};
use crate::core::paths::AppPaths;
use crate::core::util::atomic_write;

/// `None` means options.txt does not set it and the game uses its default.
/// Writing leaves `None` fields untouched.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameOptions {
    /// options.txt exists (the game has run, or the launcher prepared it).
    #[serde(skip_deserializing)]
    pub exists: bool,
    pub lang: Option<String>,
    pub render_distance: Option<u32>,
    /// 260 is the game's "unlimited".
    pub max_fps: Option<u32>,
    /// 0 is "auto".
    pub gui_scale: Option<u32>,
    /// Degrees; the file stores (fov − 70) / 40.
    pub fov: Option<u32>,
    pub fullscreen: Option<bool>,
    pub vsync: Option<bool>,
    /// Master volume, 0–100.
    pub volume: Option<u32>,
}

fn options_file(paths: &AppPaths, id: &str) -> AppResult<PathBuf> {
    AppPaths::validate_instance_id(id)?;
    Ok(paths.instance_game_dir(id).join("options.txt"))
}

fn read_text(path: &PathBuf) -> Option<String> {
    fs::read(path).ok().map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

pub fn read(paths: &AppPaths, id: &str) -> AppResult<GameOptions> {
    let Some(text) = read_text(&options_file(paths, id)?) else {
        return Ok(GameOptions::default());
    };
    let mut options = GameOptions { exists: true, ..Default::default() };
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            "lang" => options.lang = Some(value.to_ascii_lowercase()),
            "renderDistance" => options.render_distance = value.parse().ok(),
            "maxFps" => options.max_fps = value.parse().ok(),
            "guiScale" => options.gui_scale = value.parse().ok(),
            "fov" => {
                options.fov = value
                    .parse::<f64>()
                    .ok()
                    .map(|v| (70.0 + v * 40.0).round().clamp(30.0, 110.0) as u32)
            }
            "fullscreen" => options.fullscreen = value.parse().ok(),
            "enableVsync" => options.vsync = value.parse().ok(),
            "soundCategory_master" => {
                options.volume = value
                    .parse::<f64>()
                    .ok()
                    .map(|v| (v * 100.0).round().clamp(0.0, 100.0) as u32)
            }
            _ => {}
        }
    }
    Ok(options)
}

/// Before 1.11 language codes had an upper-case region (`zh_CN`).
fn legacy_language(version_id: &str) -> bool {
    let mut parts = version_id.split(['.', '-', ' ', '_']);
    parts.next() == Some("1")
        && parts
            .next()
            .and_then(|minor| minor.parse::<u32>().ok())
            .is_some_and(|minor| minor <= 10)
}

pub fn language_code(code: &str, version_id: &str) -> String {
    let lower = code.trim().to_ascii_lowercase();
    match lower.split_once('_') {
        Some((language, region)) if legacy_language(version_id) => {
            format!("{language}_{}", region.to_ascii_uppercase())
        }
        _ => lower,
    }
}

pub fn write(paths: &AppPaths, id: &str, version_id: &str, patch: &GameOptions) -> AppResult<()> {
    let path = options_file(paths, id)?;
    let text = read_text(&path).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut set = |key: &str, value: String| {
        let line = format!("{key}:{value}");
        match lines
            .iter_mut()
            .find(|line| line.split_once(':').is_some_and(|(k, _)| k.trim() == key))
        {
            Some(existing) => *existing = line,
            None => lines.push(line),
        }
    };
    if let Some(lang) = &patch.lang {
        let valid = (2..=16).contains(&lang.len())
            && lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err(AppError::new("game_options", crate::core::i18n::pick("语言代码无效。", "Invalid language code.")));
        }
        set("lang", language_code(lang, version_id));
    }
    if let Some(value) = patch.render_distance {
        set("renderDistance", value.clamp(2, 32).to_string());
    }
    if let Some(value) = patch.max_fps {
        set("maxFps", value.clamp(10, 260).to_string());
    }
    if let Some(value) = patch.gui_scale {
        set("guiScale", value.min(8).to_string());
    }
    if let Some(value) = patch.fov {
        set("fov", format!("{:?}", (value.clamp(30, 110) as f64 - 70.0) / 40.0));
    }
    if let Some(value) = patch.fullscreen {
        set("fullscreen", value.to_string());
    }
    if let Some(value) = patch.vsync {
        set("enableVsync", value.to_string());
    }
    if let Some(value) = patch.volume {
        set("soundCategory_master", format!("{:?}", value.min(100) as f64 / 100.0));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    atomic_write(&path, out.as_bytes())
}

/// Before the first launch (no options.txt yet), set the game language.
/// Returns whether it wrote the file.
pub fn prepare_first_launch(
    paths: &AppPaths,
    id: &str,
    version_id: &str,
    lang: &str,
) -> AppResult<bool> {
    if options_file(paths, id)?.exists() {
        return Ok(false);
    }
    write(paths, id, version_id, &GameOptions { lang: Some(lang.into()), ..Default::default() })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_keep_unknown_lines_and_convert_units() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        assert_eq!(read(&paths, &id).unwrap(), GameOptions::default());

        // First launch: only the language, in the version's own format.
        assert!(prepare_first_launch(&paths, &id, "1.8.9", "zh_cn").unwrap());
        assert!(!prepare_first_launch(&paths, &id, "1.8.9", "en_us").unwrap());
        let file = paths.instance_game_dir(&id).join("options.txt");
        assert_eq!(fs::read_to_string(&file).unwrap(), "lang:zh_CN\n");

        fs::write(&file, "version:3955\nfov:0.25\nrenderDistance:12\nkey_key.jump:key.keyboard.space\nlang:zh_cn\nsoundCategory_master:0.5\n").unwrap();
        let options = read(&paths, &id).unwrap();
        assert!(options.exists);
        assert_eq!(options.fov, Some(80));
        assert_eq!(options.render_distance, Some(12));
        assert_eq!(options.volume, Some(50));
        assert_eq!(options.lang.as_deref(), Some("zh_cn"));
        assert_eq!(options.fullscreen, None);

        let patch = GameOptions {
            render_distance: Some(40),
            fov: Some(70),
            fullscreen: Some(true),
            volume: Some(100),
            ..Default::default()
        };
        write(&paths, &id, "1.21.1", &patch).unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "version:3955\nfov:0.0\nrenderDistance:32\nkey_key.jump:key.keyboard.space\nlang:zh_cn\nsoundCategory_master:1.0\nfullscreen:true\n"
        );
        let bad = GameOptions { lang: Some("zh cn;".into()), ..Default::default() };
        assert!(write(&paths, &id, "1.21.1", &bad).is_err());
    }

    #[test]
    fn language_codes_follow_the_version() {
        assert_eq!(language_code("zh_cn", "1.10.2"), "zh_CN");
        assert_eq!(language_code("zh_cn", "1.8.9"), "zh_CN");
        assert_eq!(language_code("zh_CN", "1.11"), "zh_cn");
        assert_eq!(language_code("zh_cn", "1.21.1"), "zh_cn");
        assert_eq!(language_code("zh_cn", "24w14a"), "zh_cn");
    }
}
