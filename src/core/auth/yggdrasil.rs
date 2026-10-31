//! Third-party accounts on a Yggdrasil server (LittleSkin, Blessing Skin
//! sites and the like), played through authlib-injector: the game gets a
//! Java agent that sends its logins and skins to that server instead of
//! Mojang's. The password goes to the server once and is never stored; the
//! access and client tokens live in the system keychain.
//!
//! 统一通行证 (Nide8) servers speak the same API under
//! `auth.mc-user.com:233/<server id>/`, but the game is hooked up through
//! their own agent, `nide8auth.jar`, instead of authlib-injector.
use std::{path::PathBuf, time::Duration};

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::core::{
    accounts::{Account, AccountProvider},
    auth::MinecraftSession,
    error::{AppError, AppResult},
    paths::AppPaths,
    util::now_millis,
};

/// A Yggdrasil server: where its API is and what it calls itself.
#[derive(Debug, Clone)]
pub struct Server {
    pub api: String,
    pub name: String,
    /// The API root's metadata, handed to authlib-injector so the game does
    /// not fetch it again.
    pub metadata: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameProfile {
    pub id: String,
    pub name: String,
}

/// What the keychain keeps for a third-party account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub access_token: String,
    pub client_token: String,
}

/// Either the signed-in account, or the characters to choose from when the
/// login owns several and none was named.
pub enum Login {
    Done(Box<Account>, Tokens),
    Choose(Vec<GameProfile>),
}

fn fail(message: impl Into<String>) -> AppError {
    AppError::new("yggdrasil", message)
}

/// Where every 统一通行证 server's API lives, followed by its server id.
const NIDE8_API: &str = "https://auth.mc-user.com:233/";
const NIDE8_AGENT: &str = "https://login.mc-user.com:233/download/nide8auth.jar";

/// The 统一通行证 server id an API address belongs to, if it is one.
pub fn nide8_server_id(api: &str) -> Option<&str> {
    let id = api.strip_prefix(NIDE8_API)?.trim_end_matches('/');
    is_nide8_id(id).then_some(id)
}

/// Server ids are 32 hex digits.
fn is_nide8_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `littleskin.cn`, a full URL, a URL that points elsewhere through the
/// `X-Authlib-Injector-API-Location` header (API Location Indication), or a
/// bare 统一通行证 server id.
pub async fn resolve(client: &reqwest::Client, address: &str) -> AppResult<Server> {
    let address = address.trim();
    if address.is_empty() || address.len() > 300 {
        return Err(fail(crate::core::i18n::pick("请填写认证服务器地址。", "Enter the authentication server address.")));
    }
    let mut url = if is_nide8_id(address) {
        format!("{NIDE8_API}{}", address.to_ascii_lowercase())
    } else if address.contains("://") {
        address.to_string()
    } else {
        format!("https://{address}")
    };
    let parsed = url::Url::parse(&url).map_err(|_| fail(crate::core::i18n::pick("认证服务器地址无效。", "Invalid authentication server address.")))?;
    check_scheme(&parsed)?;
    let response = client.get(parsed.clone()).timeout(Duration::from_secs(15)).send().await?;
    if let Some(location) = response.headers().get("x-authlib-injector-api-location").and_then(|v| v.to_str().ok()) {
        let api = parsed.join(location).map_err(|_| fail(crate::core::i18n::pick("认证服务器给出的 API 地址无效。", "The authentication server gave an invalid API address.")))?;
        check_scheme(&api)?;
        url = api.to_string();
    }
    let api = url.trim_end_matches('/').to_string();
    let metadata = client
        .get(format!("{api}/"))
        .timeout(Duration::from_secs(15))
        .send()
        .await?
        .error_for_status()
        .map_err(|_| fail(crate::core::i18n::pick("这个地址不是 Yggdrasil 认证服务器。", "This address isn't a Yggdrasil authentication server.")))?
        .text()
        .await?;
    let json: serde_json::Value = serde_json::from_str(&metadata).map_err(|_| fail(crate::core::i18n::pick("这个地址不是 Yggdrasil 认证服务器。", "This address isn't a Yggdrasil authentication server.")))?;
    let name = json
        .pointer("/meta/serverName")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| url::Url::parse(&api).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default());
    Ok(Server { api, name, metadata })
}

/// HTTPS, except a server on this machine (for testing your own).
fn check_scheme(url: &url::Url) -> AppResult<()> {
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() == "https" || (url.scheme() == "http" && local) {
        Ok(())
    } else {
        Err(fail(crate::core::i18n::pick("认证服务器必须使用 HTTPS，密码才不会被明文发送。", "The authentication server must use HTTPS so your password isn't sent in plain text.")))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthResponse {
    access_token: String,
    client_token: String,
    #[serde(default)]
    available_profiles: Vec<GameProfile>,
    #[serde(default)]
    selected_profile: Option<GameProfile>,
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(default, rename = "errorMessage")]
    message: Option<String>,
}

async fn post(client: &reqwest::Client, url: &str, body: serde_json::Value) -> AppResult<reqwest::Response> {
    let response = client.post(url).json(&body).timeout(Duration::from_secs(20)).send().await?;
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let message = response.json::<ErrorBody>().await.ok().and_then(|b| b.message);
    Err(match (status.as_u16(), message) {
        (403, Some(message)) => fail(crate::tr!("登录被拒绝：{message}", "Sign-in refused: {message}")),
        (403, None) => fail(crate::core::i18n::pick("账号或密码不对。", "Wrong account or password.")),
        (_, Some(message)) => fail(crate::tr!("认证服务器出错：{message}", "Authentication server error: {message}")),
        (code, None) => fail(crate::tr!("认证服务器出错（{code}）。", "Authentication server error ({code}).")),
    })
}

/// Signs in with the account's email (or name) and password. With several
/// characters and no `profile` chosen, returns them to choose from.
pub async fn login(client: &reqwest::Client, server: &Server, username: &str, password: &str, profile: Option<&str>) -> AppResult<Login> {
    if username.trim().is_empty() || password.is_empty() {
        return Err(fail(crate::core::i18n::pick("请填写账号和密码。", "Enter the account and password.")));
    }
    let client_token = uuid::Uuid::new_v4().simple().to_string();
    let auth: AuthResponse = post(
        client,
        &format!("{}/authserver/authenticate", server.api),
        serde_json::json!({
            "agent": { "name": "Minecraft", "version": 1 },
            "username": username.trim(),
            "password": password,
            "clientToken": client_token,
            "requestUser": false,
        }),
    )
    .await?
    .json()
    .await?;
    let tokens = Tokens { access_token: auth.access_token, client_token: auth.client_token };
    let chosen = match (auth.selected_profile, profile) {
        (Some(selected), None) => selected,
        (_, wanted) => {
            if auth.available_profiles.is_empty() {
                return Err(fail(crate::core::i18n::pick("这个账号还没有角色，请先在认证服务器的网站上创建一个。", "This account has no character yet; create one on the server's website first.")));
            }
            let Some(wanted) = wanted.or_else(|| (auth.available_profiles.len() == 1).then(|| auth.available_profiles[0].id.as_str())) else {
                return Ok(Login::Choose(auth.available_profiles));
            };
            let profile = auth
                .available_profiles
                .iter()
                .find(|p| p.id == wanted)
                .cloned()
                .ok_or_else(|| fail(crate::core::i18n::pick("找不到选中的角色。", "The chosen character wasn't found.")))?;
            // Bind the token to the chosen character.
            let bound: AuthResponse = post(
                client,
                &format!("{}/authserver/refresh", server.api),
                serde_json::json!({
                    "accessToken": tokens.access_token,
                    "clientToken": tokens.client_token,
                    "selectedProfile": { "id": profile.id, "name": profile.name },
                }),
            )
            .await?
            .json()
            .await?;
            let account = account_for(client, server, &profile).await;
            return Ok(Login::Done(Box::new(account), Tokens { access_token: bound.access_token, client_token: bound.client_token }));
        }
    };
    let account = account_for(client, server, &chosen).await;
    Ok(Login::Done(Box::new(account), tokens))
}

fn dashed(id: &str) -> String {
    uuid::Uuid::parse_str(id).map(|u| u.to_string()).unwrap_or_else(|_| id.to_string())
}

async fn account_for(client: &reqwest::Client, server: &Server, profile: &GameProfile) -> Account {
    let host = url::Url::parse(&server.api).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let uuid = dashed(&profile.id);
    let (skin_url, skin_slim) = skin(client, &server.api, &profile.id).await.unwrap_or((None, false));
    Account {
        id: format!("yggdrasil:{host}:{uuid}"),
        provider: AccountProvider::Yggdrasil,
        username: profile.name.clone(),
        uuid,
        xuid: None,
        ownership: None,
        skin_url,
        skin_slim,
        created_at: now_millis(),
        server: Some(server.api.clone()),
        server_name: Some(server.name.clone()),
    }
}

/// The character's skin as a data URL (the launcher only shows images from
/// a few known hosts, and skin sites are many), and whether it is slim.
pub async fn skin(client: &reqwest::Client, api: &str, id: &str) -> AppResult<(Option<String>, bool)> {
    let profile: serde_json::Value = client
        .get(format!("{api}/sessionserver/session/minecraft/profile/{}", id.replace('-', "")))
        .timeout(Duration::from_secs(15))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let Some(encoded) = profile
        .get("properties")
        .and_then(|p| p.as_array())
        .and_then(|list| list.iter().find(|p| p.get("name").and_then(|n| n.as_str()) == Some("textures")))
        .and_then(|p| p.get("value"))
        .and_then(|v| v.as_str())
    else {
        return Ok((None, false));
    };
    let textures: serde_json::Value = serde_json::from_slice(&base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| fail(crate::core::i18n::pick("皮肤数据无效。", "Invalid skin data.")))?)?;
    let Some(url) = textures.pointer("/textures/SKIN/url").and_then(|v| v.as_str()) else {
        return Ok((None, false));
    };
    let slim = textures.pointer("/textures/SKIN/metadata/model").and_then(|v| v.as_str()) == Some("slim");
    let parsed = url::Url::parse(url).map_err(|_| fail(crate::core::i18n::pick("皮肤地址无效。", "Invalid skin address.")))?;
    if !matches!(parsed.scheme(), "https" | "http") {
        return Ok((None, slim));
    }
    let bytes = client.get(parsed).timeout(Duration::from_secs(15)).send().await?.error_for_status()?.bytes().await?;
    if bytes.len() > 256 * 1024 || !bytes.starts_with(b"\x89PNG") {
        return Ok((None, slim));
    }
    Ok((Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes))), slim))
}

/// Uploads (or with `None`, removes) the character's skin, where the server
/// supports the texture upload API.
pub async fn change_skin(client: &reqwest::Client, account: &Account, tokens: &Tokens, png: Option<(Vec<u8>, bool)>) -> AppResult<()> {
    let api = account.server.as_deref().ok_or_else(|| fail(crate::core::i18n::pick("这个账号缺少认证服务器地址。", "This account has no authentication server address.")))?;
    let url = format!("{api}/api/user/profile/{}/skin", account.uuid.replace('-', ""));
    let request = match png {
        Some((png, slim)) => {
            let (content_type, body) = crate::core::auth::skin_form(&[("model", if slim { "slim" } else { "" })], png);
            client.put(url).header("content-type", content_type).body(body)
        }
        None => client.delete(url),
    };
    let response = request.bearer_auth(&tokens.access_token).timeout(Duration::from_secs(30)).send().await?;
    match response.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err(AppError::new("auth_expired", crate::core::i18n::pick("登录已过期，请重新登录这个账号。", "Sign-in expired; sign in to this account again."))),
        404 | 405 | 501 => Err(fail(crate::tr!("{} 不支持在启动器里换皮肤，请到它的网站上更换。", "{} doesn't support changing skins from the launcher; change it on its website.", account.server_name.as_deref().unwrap_or(crate::core::i18n::pick("这个认证服务器", "This authentication server"))))),
        code => Err(fail(crate::tr!("换皮肤失败（{code}）。", "Changing the skin failed ({code})."))),
    }
}

/// A launch session: the stored token if the server still accepts it,
/// else a refreshed one (returned so the caller can store it).
pub async fn session(client: &reqwest::Client, account: &Account, tokens: &Tokens) -> AppResult<(MinecraftSession, Option<Tokens>)> {
    let api = account.server.as_deref().ok_or_else(|| fail(crate::core::i18n::pick("这个账号缺少认证服务器地址，请重新登录。", "This account has no authentication server address; sign in again.")))?;
    let valid = client
        .post(format!("{api}/authserver/validate"))
        .json(&serde_json::json!({ "accessToken": tokens.access_token, "clientToken": tokens.client_token }))
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .is_ok_and(|r| r.status().is_success());
    let fresh = if valid {
        None
    } else {
        let refreshed: AuthResponse = post(
            client,
            &format!("{api}/authserver/refresh"),
            serde_json::json!({ "accessToken": tokens.access_token, "clientToken": tokens.client_token }),
        )
        .await
        .map_err(|_| AppError::new("auth_expired", crate::tr!("{} 的登录已过期，请在账号页重新登录。", "The sign-in to {} expired; sign in again on the Accounts page.", account.server_name.as_deref().unwrap_or(crate::core::i18n::pick("认证服务器", "the authentication server")))))?
        .json()
        .await?;
        Some(Tokens { access_token: refreshed.access_token, client_token: refreshed.client_token })
    };
    let token = fresh.as_ref().map_or(&tokens.access_token, |t| &t.access_token).clone();
    Ok((
        MinecraftSession {
            username: account.username.clone(),
            uuid: account.uuid.replace('-', ""),
            access_token: token,
            xuid: None,
            user_type: "mojang".into(),
        },
        fresh,
    ))
}

/// Signs the token out on the server; best effort when removing an account.
pub async fn invalidate(client: &reqwest::Client, api: &str, tokens: &Tokens) {
    let _ = client
        .post(format!("{api}/authserver/invalidate"))
        .json(&serde_json::json!({ "accessToken": tokens.access_token, "clientToken": tokens.client_token }))
        .timeout(Duration::from_secs(10))
        .send()
        .await;
}

#[derive(Deserialize)]
struct Artifact {
    version: String,
    download_url: String,
    checksums: Checksums,
}

#[derive(Deserialize)]
struct Checksums {
    sha256: String,
}

/// How long a cached authlib-injector is used before looking for a newer one.
const AGENT_CHECK_EVERY: Duration = Duration::from_secs(24 * 3600);

/// authlib-injector itself, downloaded once and checked by its SHA-256.
/// A cached copy is used straight away so launching never waits on the
/// network; once a day a newer one is looked for in the background, used
/// from the next launch on. Only the very first launch waits for a download.
pub async fn ensure_agent(client: &reqwest::Client, paths: &AppPaths) -> AppResult<PathBuf> {
    let dir = paths.cache.join("authlib-injector");
    std::fs::create_dir_all(&dir)?;
    let marker = dir.join("checked");
    if let Some(cached) = newest_agent(&dir) {
        let fresh = std::fs::metadata(&marker).and_then(|m| m.modified()).ok().and_then(|at| at.elapsed().ok()).is_some_and(|age| age < AGENT_CHECK_EVERY);
        if !fresh {
            let (client, dir) = (client.clone(), dir.clone());
            tokio::spawn(async move {
                match download_agent(&client, &dir).await {
                    Ok(_) => {
                        let _ = std::fs::write(dir.join("checked"), b"");
                    }
                    Err(error) => tracing::info!(%error, "authlib-injector update check failed; keeping the cached one"),
                }
            });
        }
        return Ok(cached);
    }
    let agent = download_agent(client, &dir).await?;
    let _ = std::fs::write(&marker, b"");
    Ok(agent)
}

/// The latest authlib-injector, from BMCLAPI's mirror or the project's own
/// site, unless it is already in `dir`.
async fn download_agent(client: &reqwest::Client, dir: &std::path::Path) -> AppResult<PathBuf> {
    let mut last_error = None;
    for base in ["https://bmclapi2.bangbang93.com/mirrors/authlib-injector", "https://authlib-injector.yushi.moe"] {
        let result = async {
            let artifact: Artifact = client
                .get(format!("{base}/artifact/latest.json"))
                .timeout(Duration::from_secs(15))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            if !crate::core::fabric::valid_version(&artifact.version) {
                return Err(fail(crate::core::i18n::pick("authlib-injector 版本号无效。", "Invalid authlib-injector version.")));
            }
            let target = dir.join(format!("authlib-injector-{}.jar", artifact.version));
            if target.is_file() && sha256(&std::fs::read(&target)?) == artifact.checksums.sha256.to_lowercase() {
                return Ok(target);
            }
            // The mirror rewrites download links to itself.
            let url = url::Url::parse(&artifact.download_url).map_err(|_| fail(crate::core::i18n::pick("authlib-injector 下载地址无效。", "Invalid authlib-injector download address.")))?;
            if url.scheme() != "https" {
                return Err(fail(crate::core::i18n::pick("authlib-injector 下载地址无效。", "Invalid authlib-injector download address.")));
            }
            let bytes = client.get(url).timeout(Duration::from_secs(60)).send().await?.error_for_status()?.bytes().await?;
            if sha256(&bytes) != artifact.checksums.sha256.to_lowercase() {
                return Err(fail(crate::core::i18n::pick("authlib-injector 校验失败，已丢弃。", "authlib-injector failed verification and was discarded.")));
            }
            let temporary = target.with_extension("jar.part");
            std::fs::write(&temporary, &bytes)?;
            std::fs::rename(&temporary, &target)?;
            Ok(target)
        }
        .await;
        match result {
            Ok(path) => return Ok(path),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| fail(crate::core::i18n::pick("无法下载 authlib-injector。", "Couldn't download authlib-injector."))))
}

/// The newest authlib-injector already downloaded.
fn newest_agent(dir: &std::path::Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jar"))
        // By version number, so 1.2.10 beats 1.2.9.
        .max_by_key(|p| agent_version(p))
}

fn agent_version(path: &std::path::Path) -> Vec<u32> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix("authlib-injector-"))
        .map(|v| v.split('.').map(|n| n.parse().unwrap_or(0)).collect())
        .unwrap_or_default()
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The JVM arguments that hook the game up to the server.
pub fn agent_args(agent: &std::path::Path, account: &Account, metadata: Option<&str>) -> Vec<String> {
    let api = account.server.clone().unwrap_or_default();
    let mut args = vec![format!("-javaagent:{}={api}", agent.to_string_lossy())];
    if let Some(metadata) = metadata {
        args.push(format!("-Dauthlibinjector.yggdrasil.prefetched={}", base64::engine::general_purpose::STANDARD.encode(metadata)));
    }
    args
}

/// 统一通行证's own agent, downloaded once and refreshed weekly in the
/// background like authlib-injector. The site publishes no checksum, so a
/// download is only kept if it is a plausible jar.
pub async fn ensure_nide8_agent(client: &reqwest::Client, paths: &AppPaths) -> AppResult<PathBuf> {
    let dir = paths.cache.join("nide8auth");
    std::fs::create_dir_all(&dir)?;
    let target = dir.join("nide8auth.jar");
    if target.is_file() {
        let fresh = std::fs::metadata(&target).and_then(|m| m.modified()).ok().and_then(|at| at.elapsed().ok()).is_some_and(|age| age < AGENT_CHECK_EVERY * 7);
        if !fresh {
            let (client, target) = (client.clone(), target.clone());
            tokio::spawn(async move {
                if let Err(error) = download_nide8_agent(&client, &target).await {
                    tracing::info!(%error, "nide8auth update check failed; keeping the cached one");
                }
            });
        }
        return Ok(target);
    }
    download_nide8_agent(client, &target).await?;
    Ok(target)
}

async fn download_nide8_agent(client: &reqwest::Client, target: &std::path::Path) -> AppResult<()> {
    let failed = || fail(crate::core::i18n::pick("无法下载统一通行证的 nide8auth.jar，请稍后再试。", "Couldn't download 统一通行证's nide8auth.jar; try again later."));
    let bytes = client.get(NIDE8_AGENT).timeout(Duration::from_secs(60)).send().await?.error_for_status().map_err(|_| failed())?.bytes().await?;
    if bytes.len() < 1024 || bytes.len() > 16 * 1024 * 1024 || !bytes.starts_with(b"PK\x03\x04") {
        return Err(failed());
    }
    let temporary = target.with_extension("jar.part");
    std::fs::write(&temporary, &bytes)?;
    std::fs::rename(&temporary, target)?;
    Ok(())
}

/// The JVM arguments for a 统一通行证 account.
pub fn nide8_args(agent: &std::path::Path, server_id: &str) -> Vec<String> {
    vec![format!("-javaagent:{}={server_id}", agent.to_string_lossy()), "-Dnide8auth.client=true".into()]
}

/// The server's metadata, for the prefetch argument; none when unreachable
/// (authlib-injector then fetches it itself).
pub async fn metadata(client: &reqwest::Client, api: &str) -> Option<String> {
    let text = client.get(format!("{api}/")).timeout(Duration::from_secs(10)).send().await.ok()?.error_for_status().ok()?.text().await.ok()?;
    serde_json::from_str::<serde_json::Value>(&text).ok().map(|_| text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_or_local_servers_get_passwords() {
        assert!(check_scheme(&url::Url::parse("https://littleskin.cn/api/yggdrasil").unwrap()).is_ok());
        assert!(check_scheme(&url::Url::parse("http://127.0.0.1:8080/").unwrap()).is_ok());
        assert!(check_scheme(&url::Url::parse("http://example.com/").unwrap()).is_err());
        assert_eq!(dashed("0123456789abcdef0123456789abcdef"), "01234567-89ab-cdef-0123-456789abcdef");
        let account = Account {
            id: "yggdrasil:x:1".into(),
            provider: AccountProvider::Yggdrasil,
            username: "Alex".into(),
            uuid: "1".into(),
            xuid: None,
            ownership: None,
            skin_url: None,
            skin_slim: false,
            created_at: 0,
            server: Some("https://littleskin.cn/api/yggdrasil".into()),
            server_name: None,
        };
        let args = agent_args(std::path::Path::new("/a/authlib.jar"), &account, Some("{}"));
        assert_eq!(args[0], "-javaagent:/a/authlib.jar=https://littleskin.cn/api/yggdrasil");
        assert_eq!(args[1], "-Dauthlibinjector.yggdrasil.prefetched=e30=");
    }

    #[test]
    fn nide8_servers_are_recognised_by_their_api_address() {
        let id = "aa9441c0487a11e88feb525400b59b6a";
        assert_eq!(nide8_server_id(&format!("https://auth.mc-user.com:233/{id}")), Some(id));
        assert_eq!(nide8_server_id(&format!("https://auth.mc-user.com:233/{id}/")), Some(id));
        assert_eq!(nide8_server_id("https://auth.mc-user.com:233/short"), None);
        assert_eq!(nide8_server_id("https://littleskin.cn/api/yggdrasil"), None);
        assert!(is_nide8_id(id) && !is_nide8_id("littleskin.cn"));
        let args = nide8_args(std::path::Path::new("/a/nide8auth.jar"), id);
        assert_eq!(args, vec![format!("-javaagent:/a/nide8auth.jar={id}"), "-Dnide8auth.client=true".to_string()]);
    }
}
