use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::core::mojang::rules::current_os_name;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub main_class: String,
    #[serde(default)]
    pub assets: String,
    #[serde(default)]
    pub asset_index: Option<AssetIndex>,
    #[serde(default)]
    pub downloads: VersionDownloads,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(default)]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub java_version: Option<JavaVersion>,
    #[serde(default)]
    pub logging: Option<Logging>,
    #[serde(default)]
    pub release_time: String,
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub inherits_from: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionDownloads {
    pub client: Option<DownloadInfo>,
    pub server: Option<DownloadInfo>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndex {
    pub id: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub total_size: Option<u64>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadInfo {
    pub id: Option<String>,
    pub path: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub extract: Option<Extract>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDownloads {
    pub artifact: Option<DownloadInfo>,
    pub classifiers: Option<HashMap<String, DownloadInfo>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Extract {
    #[serde(default)]
    pub exclude: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    Conditional {
        #[serde(default)]
        rules: Vec<Rule>,
        value: StringOrVec,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum StringOrVec {
    Single(String),
    Many(Vec<String>),
}

impl StringOrVec {
    pub fn values(&self) -> Vec<String> {
        match self {
            Self::Single(value) => vec![value.clone()],
            Self::Many(values) => values.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Logging {
    #[serde(default)]
    pub client: Option<LoggingClient>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingClient {
    pub argument: String,
    pub file: DownloadInfo,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(rename = "type", default)]
    pub log_type: Option<String>,
}

pub fn log_config_name(client: &LoggingClient) -> String {
    client
        .file
        .path
        .clone()
        .or_else(|| client.file.id.clone())
        .or_else(|| client.id.clone())
        .unwrap_or_else(|| "client.xml".to_string())
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub component: String,
    pub major_version: u32,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexJson {
    #[serde(default)]
    pub objects: HashMap<String, AssetObject>,
    #[serde(rename = "virtual", default)]
    pub virtual_: bool,
    #[serde(rename = "map_to_resources", default)]
    pub map_to_resources: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

/// Convert a Maven coordinate ("org.lwjgl:lwjgl:3.3.1" or
/// "org.lwjgl:lwjgl:3.3.1:natives-macos") to a relative path.
pub fn maven_path(name: &str) -> Option<String> {
    let (coordinates, extension) = match name.split_once('@') {
        Some((coords, ext)) => (coords, ext),
        None => (name, "jar"),
    };
    let parts: Vec<&str> = coordinates.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    let (classifier, extension) = match parts.len() {
        4 => {
            if parts[3].contains('.') {
                (None, parts[3])
            } else {
                (Some(parts[3]), extension)
            }
        }
        5 => (Some(parts[3]), parts[4]),
        _ => (None, extension),
    };
    let file = match classifier {
        Some(classifier) => format!("{artifact}-{version}-{classifier}.{extension}"),
        None => format!("{artifact}-{version}.{extension}"),
    };
    Some(format!("{group}/{artifact}/{version}/{file}"))
}

pub fn artifact_path(library: &Library) -> Option<String> {
    library
        .downloads
        .as_ref()
        .and_then(|downloads| downloads.artifact.as_ref())
        .and_then(|artifact| artifact.path.clone())
        .or_else(|| maven_path(&library.name))
}

/// Pick the native classifier for the current OS/arch. On Apple Silicon we try
/// the arm64 classifier before falling back to the x64 one.
pub fn native_classifier(library: &Library) -> Option<String> {
    native_classifier_for(library, std::env::consts::ARCH)
}

/// The native classifier for a game running as `arch`.
pub fn native_classifier_for(library: &Library, arch: &str) -> Option<String> {
    let arch = crate::core::mojang::rules::mojang_arch(arch);
    let template = library
        .natives
        .as_ref()?
        .get(current_os_name())?
        .replace("${arch}", if arch == "x86" { "32" } else { "64" });

    let classifiers = library
        .downloads
        .as_ref()
        .and_then(|downloads| downloads.classifiers.as_ref())?;

    let mut candidates = Vec::new();
    if arch == "arm64" {
        candidates.push(format!("{template}-arm64"));
    }
    candidates.push(template.clone());
    candidates
        .into_iter()
        .find(|candidate| classifiers.contains_key(candidate))
}

pub fn native_download(library: &Library) -> Option<&DownloadInfo> {
    native_download_for(library, std::env::consts::ARCH)
}

pub fn native_download_for<'a>(library: &'a Library, arch: &str) -> Option<&'a DownloadInfo> {
    let classifier = native_classifier_for(library, arch)?;
    library
        .downloads
        .as_ref()?
        .classifiers
        .as_ref()?
        .get(&classifier)
}

/// The architecture the game runs as. Usually the machine's own; but
/// versions up to 1.18 ship macOS native libraries (LWJGL 2 and 3.2) only
/// for Intel, so on Apple Silicon they run as x86_64 under Rosetta 2, with
/// an Intel Java.
pub fn game_arch(version: &VersionJson) -> &'static str {
    let host = crate::core::platform::normalize_arch(std::env::consts::ARCH);
    if !cfg!(target_os = "macos") || host != "aarch64" {
        return host;
    }
    let intel_only = version.libraries.iter().any(|library| {
        crate::core::mojang::rules::rules_allow_in(
            &library.rules,
            crate::core::mojang::rules::RuleEnv { arch: "aarch64", custom_resolution: false },
        ) && native_classifier_for(library, "aarch64").is_some_and(|classifier| !classifier.ends_with("-arm64"))
    });
    if intel_only { "x86_64" } else { host }
}

pub fn version_asset_index_id(version: &VersionJson) -> Option<&str> {
    version
        .asset_index
        .as_ref()
        .map(|index| index.id.as_str())
        .or(if version.assets.is_empty() {
            None
        } else {
            Some(version.assets.as_str())
        })
}

pub fn java_major(version: &VersionJson) -> u32 {
    version
        .java_version
        .as_ref()
        .map(|java| java.major_version)
        .unwrap_or(8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version_with(libraries: serde_json::Value) -> VersionJson {
        serde_json::from_value(serde_json::json!({
            "id": "test", "type": "release", "mainClass": "net.minecraft.client.main.Main",
            "libraries": libraries
        }))
        .unwrap()
    }

    #[test]
    fn intel_only_natives_run_under_rosetta_on_apple_silicon() {
        let old = version_with(serde_json::json!([{
            "name": "org.lwjgl:lwjgl-platform:2.9.4",
            "natives": { "osx": "natives-osx", "linux": "natives-linux", "windows": "natives-windows" },
            "downloads": { "classifiers": {
                "natives-osx": { "url": "https://x/a.jar", "sha1": "0", "size": 1 },
                "natives-linux": { "url": "https://x/b.jar", "sha1": "0", "size": 1 },
                "natives-windows": { "url": "https://x/c.jar", "sha1": "0", "size": 1 }
            } }
        }]));
        let modern = version_with(serde_json::json!([{
            "name": "org.lwjgl:lwjgl:3.3.3:natives-macos-arm64",
            "downloads": { "artifact": { "path": "a.jar", "url": "https://x/a.jar", "sha1": "0", "size": 1 } },
            "rules": [{ "action": "allow", "os": { "name": "osx" } }]
        }]));
        let host = crate::core::platform::normalize_arch(std::env::consts::ARCH);
        assert_eq!(game_arch(&modern), host);
        if cfg!(target_os = "macos") && host == "aarch64" {
            assert_eq!(game_arch(&old), "x86_64");
        } else {
            assert_eq!(game_arch(&old), host);
        }
        // Natives follow the arch the game runs as.
        if cfg!(target_os = "macos") {
            assert_eq!(native_classifier_for(&old.libraries[0], "x86_64").as_deref(), Some("natives-osx"));
        }
    }

    #[test]
    fn maven_paths() {
        assert_eq!(
            maven_path("org.lwjgl:lwjgl:3.3.1").as_deref(),
            Some("org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar")
        );
        assert_eq!(
            maven_path("org.lwjgl:lwjgl:3.3.1:natives-macos").as_deref(),
            Some("org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-macos.jar")
        );
        assert_eq!(
            maven_path("com.example:demo:1.0@zip").as_deref(),
            Some("com/example/demo/1.0/demo-1.0.zip")
        );
    }

    #[test]
    fn parses_logging_client_without_top_level_id() {
        let json = r#"{
            "id": "test",
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "assetIndex": {"id": "1"},
            "logging": {
                "client": {
                    "argument": "-Dlog4j.configurationFile=${path}",
                    "file": {
                        "id": "client.xml",
                        "sha1": "00",
                        "size": 1,
                        "url": "https://example.com/client.xml"
                    },
                    "type": "log4j2-xml"
                }
            }
        }"#;
        let version: super::VersionJson = serde_json::from_str(json).unwrap();
        let client = version.logging.unwrap().client.unwrap();
        assert_eq!(client.id, None);
        assert_eq!(client.file.id.as_deref(), Some("client.xml"));
        assert_eq!(super::log_config_name(&client), "client.xml");
    }
}
