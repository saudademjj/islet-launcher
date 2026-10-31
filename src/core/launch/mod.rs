use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::copy;
use std::path::PathBuf;

use crate::core::auth::MinecraftSession;
use crate::core::error::{AppError, AppResult};
use crate::core::instances::Instance;
use crate::core::java::detect::JavaInstallation;
use crate::core::mojang::rules::{RuleEnv, rules_allow_in};
use crate::core::mojang::version::{
    Argument, Library, StringOrVec, VersionJson, artifact_path, native_download_for,
    version_asset_index_id,
};
use crate::core::paths::AppPaths;

/// How this launch differs from the defaults: the architecture the game runs
/// as, a window size, and the player's own extra arguments.
#[derive(Debug, Clone, Default)]
pub struct LaunchOptions {
    pub arch: String,
    pub window: Option<(u32, u32)>,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
}

/// Splits arguments the way a shell would for plain words: spaces separate,
/// quotes group, a backslash escapes the next character.
pub fn split_args(text: &str) -> AppResult<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (_, '\\') if quote != Some('\'') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                    started = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (_, c) => {
                current.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err(AppError::new("launch_args", crate::core::i18n::pick("启动参数里有没配对的引号。", "Unmatched quote in the launch arguments.")));
    }
    if started {
        args.push(current);
    }
    Ok(args)
}

#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub java_path: PathBuf,
    pub args: Vec<String>,
    pub game_dir: PathBuf,
}

/// Whether the version's launch arguments know `--quickPlaySingleplayer`
/// (Minecraft 1.20 and later), which opens a world straight from launch.
pub fn supports_quick_play(version: &VersionJson) -> bool {
    offers(version, "--quickPlaySingleplayer")
}

/// Whether the version joins a server through `--quickPlayMultiplayer`
/// (1.20+). Older versions take `--server` and `--port` instead.
pub fn supports_quick_play_multiplayer(version: &VersionJson) -> bool {
    offers(version, "--quickPlayMultiplayer")
}

fn offers(version: &VersionJson, flag: &str) -> bool {
    version
        .arguments
        .as_ref()
        .is_some_and(|arguments| offers_argument(&arguments.game, flag))
}

fn offers_argument(game: &[Argument], flag: &str) -> bool {
    game.iter().any(|argument| match argument {
        Argument::Plain(value) => value == flag,
        Argument::Conditional { value, .. } => value.values().iter().any(|v| v == flag),
    })
}

pub fn build_spec(
    paths: &AppPaths,
    instance: &Instance,
    version: &VersionJson,
    java: &JavaInstallation,
    session: &MinecraftSession,
    client_id: &str,
    memory_mb: u32,
    options: &LaunchOptions,
) -> AppResult<LaunchSpec> {
    let arch = if options.arch.is_empty() { std::env::consts::ARCH } else { options.arch.as_str() };
    let env = RuleEnv { arch, custom_resolution: options.window.is_some() };
    paths.ensure_instance_dirs(&instance.id)?;
    validate_files(paths, instance, version, env)?;
    prepare_natives(paths, &instance.id, version, env)?;
    let (width, height) = options.window.unwrap_or((854, 480));

    let classpath = classpath(paths, instance, version, env);
    let separator = if cfg!(windows) { ";" } else { ":" };
    let classpath_string = classpath.join(separator);
    let game_dir = paths.instance_game_dir(&instance.id);
    let natives_dir = paths.instance_natives_dir(&instance.id);
    let asset_index = version_asset_index_id(version)
        .unwrap_or(&version.assets)
        .to_string();
    let game_assets = legacy_assets(paths, &game_dir, &asset_index)?;

    let log_config = version
        .logging
        .as_ref()
        .and_then(|logging| logging.client.as_ref())
        .and_then(|client| {
            let file_name = crate::core::mojang::version::log_config_name(client);
            let path = paths.assets.join("log_configs").join(file_name);
            path.exists().then_some(path)
        });

    let placeholders: HashMap<&str, String> = HashMap::from([
        ("auth_player_name", session.username.clone()),
        ("version_name", instance.version_id.clone()),
        ("game_directory", game_dir.to_string_lossy().to_string()),
        ("assets_root", paths.assets.to_string_lossy().to_string()),
        ("assets_index_name", asset_index),
        ("game_assets", game_assets.to_string_lossy().to_string()),
        ("auth_uuid", session.uuid.clone()),
        ("auth_access_token", session.access_token.clone()),
        ("auth_xuid", session.xuid.clone().unwrap_or_default()),
        ("clientid", client_id.to_string()),
        (
            "auth_session",
            format!("token:{}:{}", session.access_token, session.uuid),
        ),
        ("user_properties", "{}".to_string()),
        ("user_type", session.user_type.clone()),
        ("version_type", instance.version_type.clone()),
        (
            "natives_directory",
            natives_dir.to_string_lossy().to_string(),
        ),
        ("classpath", classpath_string),
        ("classpath_separator", separator.to_string()),
        (
            "library_directory",
            paths.libraries.to_string_lossy().to_string(),
        ),
        ("launcher_name", "Islet".to_string()),
        ("launcher_version", env!("CARGO_PKG_VERSION").to_string()),
        ("resolution_width", width.to_string()),
        ("resolution_height", height.to_string()),
        (
            "path",
            log_config
                .as_ref()
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_default(),
        ),
    ]);

    let mut jvm_args = Vec::new();
    if let Some(arguments) = &version.arguments {
        push_arguments(&mut jvm_args, &arguments.jvm, &placeholders, env);
    }
    // Forge's bootstrap excludes the vanilla jar by its launcher filename.
    // Our shared cache calls it client.jar instead of <version>.jar.
    adapt_client_ignore_list(&mut jvm_args, &instance.version_id);
    if !jvm_args
        .iter()
        .any(|arg| arg == "-cp" || arg == "-classpath" || arg == "--class-path")
    {
        jvm_args.push("-cp".to_string());
        jvm_args.push(classpath.join(separator));
    }
    if !jvm_args
        .iter()
        .any(|arg| arg.starts_with("-Djava.library.path="))
    {
        jvm_args.push(format!(
            "-Djava.library.path={}",
            natives_dir.to_string_lossy()
        ));
    }
    if !jvm_args
        .iter()
        .any(|arg| arg.starts_with("-Dminecraft.launcher.brand="))
    {
        jvm_args.push("-Dminecraft.launcher.brand=Islet".to_string());
    }
    if !jvm_args
        .iter()
        .any(|arg| arg.starts_with("-Dminecraft.launcher.version="))
    {
        jvm_args.push(format!(
            "-Dminecraft.launcher.version={}",
            env!("CARGO_PKG_VERSION")
        ));
    }
    if let Some(logging) = version
        .logging
        .as_ref()
        .and_then(|logging| logging.client.as_ref())
    {
        if log_config.is_some() {
            let argument = fill_placeholders(&logging.argument, &placeholders);
            if !jvm_args.iter().any(|existing| existing == &argument) {
                jvm_args.push(argument);
            }
        }
    }

    let mut args = vec![
        format!("-Xmx{memory_mb}M"),
        format!("-Xms{}M", memory_mb.min(1024)),
    ];
    args.extend(jvm_args);
    // The player's own JVM arguments come last, so an -Xmx of theirs wins.
    args.extend(options.jvm_args.iter().cloned());
    args.push(version.main_class.clone());

    let mut game_args = Vec::new();
    if let Some(arguments) = &version.arguments {
        push_arguments(&mut game_args, &arguments.game, &placeholders, env);
    } else if let Some(legacy) = &version.minecraft_arguments {
        for token in legacy.split_whitespace() {
            game_args.push(fill_placeholders(token, &placeholders));
        }
        // Old versions take the window size as plain arguments.
        if options.window.is_some() && !game_args.iter().any(|arg| arg == "--width") {
            game_args.extend(["--width".to_string(), width.to_string(), "--height".to_string(), height.to_string()]);
        }
    }
    args.extend(game_args);
    args.extend(options.game_args.iter().cloned());

    for arg in &args {
        if arg.contains("${") {
            tracing::warn!(argument = %arg, "unresolved launch placeholder");
        }
    }

    Ok(LaunchSpec {
        java_path: PathBuf::from(&java.path),
        args,
        game_dir,
    })
}

fn push_arguments(
    output: &mut Vec<String>,
    arguments: &[Argument],
    placeholders: &HashMap<&str, String>,
    env: RuleEnv,
) {
    for argument in arguments {
        match argument {
            Argument::Plain(value) => {
                output.push(fill_placeholders(value, placeholders));
            }
            Argument::Conditional { rules, value } => {
                if rules_allow_in(rules, env) {
                    match value {
                        StringOrVec::Single(value) => {
                            output.push(fill_placeholders(value, placeholders));
                        }
                        StringOrVec::Many(values) => {
                            for value in values {
                                output.push(fill_placeholders(value, placeholders));
                            }
                        }
                    }
                }
            }
        }
    }
}

fn fill_placeholders(value: &str, placeholders: &HashMap<&str, String>) -> String {
    let mut result = value.to_string();
    for (key, replacement) in placeholders {
        result = result.replace(&format!("${{{key}}}"), replacement);
    }
    result
}

fn adapt_client_ignore_list(args: &mut [String], version: &str) {
    let vanilla = format!("{version}.jar");
    for arg in args {
        if let Some(list) = arg.strip_prefix("-DignoreList=") {
            if list.split(',').any(|name| name == vanilla)
                && !list.split(',').any(|name| name == "client.jar")
            {
                arg.push_str(",client.jar");
            }
        }
    }
}

fn classpath(paths: &AppPaths, instance: &Instance, version: &VersionJson, env: RuleEnv) -> Vec<String> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for library in &version.libraries {
        if !rules_allow_in(&library.rules, env) {
            continue;
        }
        if let Some(relative) = classpath_artifact(library) {
            let path = paths.library_path(&relative);
            if path.exists() && seen.insert(path.clone()) {
                entries.push(path.to_string_lossy().to_string());
            }
        }
    }
    let client = paths.client_jar(&instance.version_id);
    if seen.insert(client.clone()) {
        entries.push(client.to_string_lossy().to_string());
    }
    entries
}

fn validate_files(paths: &AppPaths, instance: &Instance, version: &VersionJson, env: RuleEnv) -> AppResult<()> {
    if !paths.client_jar(&instance.version_id).exists() {
        return Err(not_ready());
    }
    for library in &version.libraries {
        if !rules_allow_in(&library.rules, env) {
            continue;
        }
        if let Some(relative) = classpath_artifact(library) {
            if !paths.library_path(&relative).exists() {
                return Err(not_ready());
            }
        }
    }
    if let Some(index) = &version.asset_index {
        if !paths.asset_index(&index.id).exists() {
            return Err(not_ready());
        }
    }
    Ok(())
}

/// Native-only libraries (jinput-platform and similar in pre-1.13 versions)
/// ship just a classifier jar, so there is no main jar to require or load.
fn classpath_artifact(library: &Library) -> Option<String> {
    let native_only = library.natives.is_some()
        && library
            .downloads
            .as_ref()
            .is_some_and(|downloads| downloads.artifact.is_none());
    if native_only {
        None
    } else {
        artifact_path(library)
    }
}

fn not_ready() -> AppError {
    AppError::new("instance_not_ready", crate::core::i18n::pick("世界文件不完整，请先下载或修复。", "The world's files are incomplete; install or repair it first."))
}

fn prepare_natives(paths: &AppPaths, instance_id: &str, version: &VersionJson, env: RuleEnv) -> AppResult<()> {
    let natives_dir = paths.instance_natives_dir(instance_id);
    if natives_dir.exists() {
        fs::remove_dir_all(&natives_dir)?;
    }
    fs::create_dir_all(&natives_dir)?;
    // Modern versions point java.library.path / jna / lwjgl / netty at
    // subdirectories of the instance natives directory.
    for subdir in ["java", "jna", "lwjgl", "netty"] {
        fs::create_dir_all(natives_dir.join(subdir))?;
    }

    for library in &version.libraries {
        if !rules_allow_in(&library.rules, env) || library.natives.is_none() {
            continue;
        }
        let relative = if let Some(native) = native_download_for(library, env.arch) {
            native.path.clone()
        } else {
            None
        }
        .or_else(|| artifact_path(library));
        let Some(relative) = relative else {
            continue;
        };
        let jar = paths.library_path(&relative);
        if !jar.exists() {
            return Err(not_ready());
        }
        let exclude = library
            .extract
            .as_ref()
            .and_then(|extract| extract.exclude.clone())
            .unwrap_or_default();
        extract_native_jar(&jar, &natives_dir, &exclude)?;
    }
    Ok(())
}

fn extract_native_jar(jar: &PathBuf, natives_dir: &PathBuf, exclude: &[String]) -> AppResult<()> {
    let file = fs::File::open(jar).map_err(|err| {
        AppError::detail(
            "natives",
            crate::core::i18n::pick("打开本地库失败，请重新下载版本。", "Couldn't open a native library; download the version again."),
            err.to_string(),
        )
    })?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|err| AppError::detail("natives", crate::core::i18n::pick("读取本地库压缩包失败。", "Couldn't read a native library archive."), err.to_string()))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| AppError::detail("natives", crate::core::i18n::pick("读取本地库条目失败。", "Couldn't read a native library entry."), err.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        // enclosed_name rebuilds the path with `\` on Windows, while the
        // exclude rules in version JSON always use `/`.
        let name = relative.to_string_lossy().replace('\\', "/");
        if name.starts_with("META-INF/")
            || exclude
                .iter()
                .any(|excluded| name.starts_with(excluded.as_str()))
        {
            continue;
        }
        let dest = natives_dir.join(relative);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = fs::File::create(&dest)?;
        copy(&mut entry, &mut output)?;
    }
    Ok(())
}

/// Minecraft 1.7.2 and older read assets by name, not by hash: 1.6–1.7.2
/// from `assets/virtual/<index>`, earlier versions from the game folder's
/// `resources`. The shared hashed objects are copied (or hard-linked) there
/// before each launch, as the official launcher does. Returns the folder for
/// `${game_assets}`; newer versions just get the assets root.
fn legacy_assets(paths: &AppPaths, game_dir: &std::path::Path, index_id: &str) -> AppResult<PathBuf> {
    let Ok(bytes) = fs::read(paths.asset_index(index_id)) else {
        return Ok(paths.assets.clone());
    };
    let index: crate::core::mojang::version::AssetIndexJson = serde_json::from_slice(&bytes)?;
    let root = if index.map_to_resources {
        game_dir.join("resources")
    } else if index.virtual_ {
        paths.assets.join("virtual").join(index_id)
    } else {
        return Ok(paths.assets.clone());
    };
    for (name, object) in &index.objects {
        let Ok(relative) = crate::core::java::runtime::safe_archive_path(name) else {
            continue;
        };
        let target = root.join(relative);
        if fs::metadata(&target).is_ok_and(|meta| meta.len() == object.size) {
            continue;
        }
        let source = paths.asset_object(&object.hash);
        if !source.is_file() {
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let _ = fs::remove_file(&target);
        if fs::hard_link(&source, &target).is_err() {
            fs::copy(&source, &target)?;
        }
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_arguments_split_like_a_shell() {
        assert_eq!(split_args("").unwrap(), Vec::<String>::new());
        assert_eq!(
            split_args(r#"-Xss4M  -Dname="a b" 'c d' e\ f"#).unwrap(),
            vec!["-Xss4M", "-Dname=a b", "c d", "e f"]
        );
        assert_eq!(split_args(r#"-Dempty="""#).unwrap(), vec!["-Dempty="]);
        assert!(split_args(r#"-Dbad="open"#).is_err());
    }

    #[test]
    fn quick_play_follows_the_version_arguments() {
        // Shape of the 1.20+ feature-gated argument in Mojang's version JSON.
        let modern: Vec<Argument> = serde_json::from_str(
            r#"["--username","${auth_player_name}",
                {"rules":[{"action":"allow","features":{"is_quick_play_singleplayer":true}}],
                 "value":["--quickPlaySingleplayer","${quickPlaySingleplayer}"]}]"#,
        )
        .unwrap();
        assert!(offers_argument(&modern, "--quickPlaySingleplayer"));
        assert!(!offers_argument(&modern, "--quickPlayMultiplayer"));
        let older: Vec<Argument> =
            serde_json::from_str(r#"["--username","${auth_player_name}","--demo"]"#).unwrap();
        assert!(!offers_argument(&older, "--quickPlaySingleplayer"));
    }

    #[test]
    fn forge_excludes_cached_vanilla_jar_without_excluding_client_extra() {
        let mut args = vec!["-DignoreList=asm,1.20.1.jar".to_string()];
        adapt_client_ignore_list(&mut args, "1.20.1");
        assert_eq!(args[0], "-DignoreList=asm,1.20.1.jar,client.jar");
        adapt_client_ignore_list(&mut args, "1.20.1");
        assert_eq!(args[0].matches("client.jar").count(), 1);
        let mut older = vec!["-DignoreList=asm".into()];
        adapt_client_ignore_list(&mut older, "1.12.2");
        assert_eq!(older[0], "-DignoreList=asm");
    }

    #[test]
    fn native_only_library_has_no_classpath_jar() {
        // Shape of jinput-platform 2.0.5 in the 1.12.2 version JSON.
        let native_only: Library = serde_json::from_str(
            r#"{"name":"net.java.jinput:jinput-platform:2.0.5",
                "downloads":{"classifiers":{"natives-windows":{"path":"x.jar","sha1":"0","size":1,"url":"https://e/x.jar"}}},
                "natives":{"windows":"natives-windows"}}"#,
        )
        .unwrap();
        assert_eq!(classpath_artifact(&native_only), None);
        // Fabric libraries have neither downloads nor natives.
        let fabric: Library =
            serde_json::from_str(r#"{"name":"net.fabricmc:fabric-loader:0.16.0"}"#).unwrap();
        assert!(classpath_artifact(&fabric).is_some());
    }

    #[test]
    fn old_versions_get_assets_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let hash = "0123456789abcdef0123456789abcdef01234567";
        fs::create_dir_all(paths.asset_object(hash).parent().unwrap()).unwrap();
        fs::write(paths.asset_object(hash), b"ogg").unwrap();
        fs::create_dir_all(&paths.asset_indexes).unwrap();
        let object = format!(r#"{{"sound/step/grass1.ogg":{{"hash":"{hash}","size":3}},"../escape":{{"hash":"{hash}","size":3}}}}"#);
        fs::write(paths.asset_index("legacy"), format!(r#"{{"virtual":true,"objects":{object}}}"#)).unwrap();
        fs::write(paths.asset_index("pre-1.6"), format!(r#"{{"map_to_resources":true,"objects":{object}}}"#)).unwrap();
        fs::write(paths.asset_index("17"), format!(r#"{{"objects":{object}}}"#)).unwrap();
        let game = dir.path().join("game");

        let virtual_root = legacy_assets(&paths, &game, "legacy").unwrap();
        assert_eq!(virtual_root, paths.assets.join("virtual/legacy"));
        assert_eq!(fs::read(virtual_root.join("sound/step/grass1.ogg")).unwrap(), b"ogg");
        assert!(!paths.assets.join("virtual/escape").exists());

        let resources = legacy_assets(&paths, &game, "pre-1.6").unwrap();
        assert_eq!(resources, game.join("resources"));
        assert!(resources.join("sound/step/grass1.ogg").is_file());

        assert_eq!(legacy_assets(&paths, &game, "17").unwrap(), paths.assets);
    }
}
