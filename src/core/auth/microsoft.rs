use std::collections::HashMap;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tiny_http::{Header, Response, Server};
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

use crate::core::accounts::{Account, AccountProvider};
use crate::core::auth::MinecraftSession;
use crate::core::error::{AppError, AppResult};
use crate::core::util::now_millis;

const OAUTH_BASE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
const SCOPE: &str = "XboxLive.signin offline_access";
const XBOX_USER_AUTH: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MINECRAFT_LOGIN: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MINECRAFT_PROFILE: &str = "https://api.minecraftservices.com/minecraft/profile";
const LOOPBACK_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DeviceCodeInfo {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    #[allow(dead_code)]
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OAuthError {
    error: String,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_interval() -> u64 {
    5
}

#[derive(Debug)]
struct XboxSession {
    xsts_token: String,
    uhs: String,
    xuid: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct XboxUserRequest<'a> {
    properties: XboxUserProperties<'a>,
    relying_party: &'a str,
    token_type: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct XboxUserProperties<'a> {
    auth_method: &'a str,
    site_name: &'a str,
    rps_ticket: &'a str,
}

#[derive(Debug, Deserialize)]
struct XboxAuthResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: DisplayClaims,
}

#[derive(Debug, Deserialize)]
struct DisplayClaims {
    xui: Vec<Xui>,
}

#[derive(Debug, Deserialize)]
struct Xui {
    uhs: String,
    xid: Option<String>,
}

#[derive(Debug, Deserialize)]
struct XstsError {
    #[serde(rename = "XErr")]
    xerr: Option<i64>,
    #[serde(rename = "Message")]
    message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct XstsRequest<'a> {
    properties: XstsProperties<'a>,
    relying_party: &'a str,
    token_type: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct XstsProperties<'a> {
    sandbox_id: &'a str,
    user_tokens: Vec<&'a str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MinecraftLoginRequest<'a> {
    identity_token: &'a str,
}

#[derive(Debug, Deserialize)]
struct MinecraftLoginResponse {
    access_token: String,
    /// Seconds the token stays valid; a day, so far.
    #[serde(default)]
    expires_in: Option<u64>,
}

/// How long a Minecraft token is assumed to last when the reply doesn't say.
const DEFAULT_TOKEN_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Deserialize)]
struct MinecraftProfile {
    id: String,
    name: String,
    skins: Option<Vec<Skin>>,
    #[serde(default)]
    capes: Vec<Cape>,
}

/// A cape the account owns.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cape {
    pub id: String,
    #[serde(default)]
    pub alias: String,
    pub url: String,
    /// "ACTIVE" for the one shown in game.
    #[serde(default)]
    pub state: String,
}

#[derive(Debug, Deserialize)]
struct Skin {
    url: String,
    /// "ACTIVE" for the skin in use.
    #[serde(default)]
    state: String,
    /// "SLIM" or "CLASSIC".
    #[serde(default)]
    variant: String,
}

pub fn validate_client_id(client_id: &str) -> AppResult<&str> {
    let client_id = client_id.trim();
    if client_id.is_empty() {
        return Err(AppError::new(
            "client_id",
            crate::core::i18n::pick("还没有配置 Azure 应用 Client ID，请先在设置中填写。", "No Azure app Client ID is set up; enter one in Settings first."),
        ));
    }
    Uuid::parse_str(client_id).map_err(|_| {
        AppError::new(
            "client_id",
            crate::core::i18n::pick("Client ID 格式不正确，应为 Azure 应用的 GUID。", "The Client ID format is wrong; it should be the Azure app's GUID."),
        )
    })?;
    Ok(client_id)
}

fn map_oauth_error(error: OAuthError) -> AppError {
    let detail = error.error_description.unwrap_or_default();
    match error.error.as_str() {
        "invalid_client" | "unauthorized_client" => AppError::detail(
            "client_id",
            crate::core::i18n::pick("Azure Client ID 无效，或应用没有启用公开客户端。", "The Azure Client ID is invalid, or the app doesn't allow public clients."),
            detail,
        ),
        "invalid_grant" | "expired_token" => {
            AppError::new("auth_expired", crate::core::i18n::pick("登录已过期，请重新登录。", "Sign-in expired; sign in again."))
        }
        "access_denied" => AppError::new("auth_denied", crate::core::i18n::pick("登录被取消或未获得授权。", "Sign-in was cancelled or not authorized.")),
        "authorization_pending" => AppError::new("auth_pending", crate::core::i18n::pick("等待授权中。", "Waiting for authorization.")),
        "slow_down" => AppError::new("auth_slow_down", crate::core::i18n::pick("请求过于频繁，请稍后重试。", "Too many requests; try again later.")),
        _ => AppError::detail(
            "auth",
            crate::core::i18n::pick("微软登录失败，请重试。", "Microsoft sign-in failed; try again."),
            format!("{}: {detail}", error.error),
        ),
    }
}

async fn parse_oauth_response(response: reqwest::Response) -> AppResult<OAuthToken> {
    let status = response.status();
    let text = response.text().await?;
    if status.is_success() {
        let token: OAuthTokenResponse = serde_json::from_str(&text).map_err(|err| {
            AppError::detail("auth", crate::core::i18n::pick("微软返回了无法解析的登录结果。", "Microsoft returned a sign-in result that couldn't be read."), err.to_string())
        })?;
        return Ok(OAuthToken {
            access_token: token.access_token,
            refresh_token: token.refresh_token,
        });
    }
    if let Ok(error) = serde_json::from_str::<OAuthError>(&text) {
        return Err(map_oauth_error(error));
    }
    Err(AppError::detail(
        "auth",
        crate::core::i18n::pick("微软登录失败，请重试。", "Microsoft sign-in failed; try again."),
        format!("HTTP {status}"),
    ))
}

pub async fn exchange_code(
    client: &reqwest::Client,
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> AppResult<OAuthToken> {
    let response = client
        .post(format!("{OAUTH_BASE}/token"))
        .form(&[
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
            ("scope", SCOPE),
        ])
        .send()
        .await?;
    parse_oauth_response(response).await
}

pub async fn refresh_ms_token(
    client: &reqwest::Client,
    client_id: &str,
    refresh_token: &str,
) -> AppResult<OAuthToken> {
    let response = client
        .post(format!("{OAUTH_BASE}/token"))
        .form(&[
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", SCOPE),
        ])
        .send()
        .await?;
    parse_oauth_response(response).await
}

pub async fn request_device_code(
    client: &reqwest::Client,
    client_id: &str,
) -> AppResult<DeviceCodeInfo> {
    let response = client
        .post(format!("{OAUTH_BASE}/devicecode"))
        .form(&[("client_id", client_id), ("scope", SCOPE)])
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        if let Ok(error) = serde_json::from_str::<OAuthError>(&text) {
            return Err(map_oauth_error(error));
        }
        return Err(AppError::detail(
            "auth",
            crate::core::i18n::pick("无法获取设备登录代码。", "Couldn't get a device code."),
            format!("HTTP {status}"),
        ));
    }
    let info: DeviceCodeResponse = serde_json::from_str(&text)
        .map_err(|err| AppError::detail("auth", crate::core::i18n::pick("无法解析设备登录代码。", "Couldn't read the device code."), err.to_string()))?;
    Ok(DeviceCodeInfo {
        device_code: info.device_code,
        user_code: info.user_code,
        verification_uri: info.verification_uri,
        expires_in: info.expires_in,
        interval: info.interval.max(1),
    })
}

pub async fn poll_device_code(
    client: &reqwest::Client,
    client_id: &str,
    info: &DeviceCodeInfo,
    cancel: &CancellationToken,
) -> AppResult<OAuthToken> {
    let deadline = Instant::now() + Duration::from_secs(info.expires_in);
    let mut interval = info.interval.max(1);

    loop {
        if Instant::now() >= deadline {
            return Err(AppError::new(
                "auth_expired",
                crate::core::i18n::pick("设备登录代码已过期，请重新登录。", "The device code expired; sign in again."),
            ));
        }
        tokio::select! {
            _ = cancel.cancelled() => {
                return Err(AppError::new("auth_denied", crate::core::i18n::pick("登录已取消。", "Sign-in cancelled.")));
            }
            _ = tokio::time::sleep(Duration::from_secs(interval)) => {}
        }

        let response = client
            .post(format!("{OAUTH_BASE}/token"))
            .form(&[
                ("client_id", client_id),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", &info.device_code),
            ])
            .send()
            .await?;
        let status = response.status();
        let text = response.text().await?;

        if status.is_success() {
            let token: OAuthTokenResponse = serde_json::from_str(&text).map_err(|err| {
                AppError::detail("auth", crate::core::i18n::pick("无法解析设备登录结果。", "Couldn't read the device sign-in result."), err.to_string())
            })?;
            return Ok(OAuthToken {
                access_token: token.access_token,
                refresh_token: token.refresh_token,
            });
        }

        if let Ok(error) = serde_json::from_str::<OAuthError>(&text) {
            match error.error.as_str() {
                "authorization_pending" => continue,
                "slow_down" => {
                    interval = interval.saturating_add(5);
                    continue;
                }
                _ => return Err(map_oauth_error(error)),
            }
        } else {
            return Err(AppError::detail(
                "auth",
                crate::core::i18n::pick("设备登录失败，请重试。", "Device sign-in failed; try again."),
                format!("HTTP {status}"),
            ));
        }
    }
}

async fn authenticate_xbox(
    client: &reqwest::Client,
    ms_access_token: &str,
) -> AppResult<XboxSession> {
    let rps_ticket = format!("d={ms_access_token}");
    let body = XboxUserRequest {
        properties: XboxUserProperties {
            auth_method: "RPS",
            site_name: "user.auth.xboxlive.com",
            rps_ticket: &rps_ticket,
        },
        relying_party: "http://auth.xboxlive.com",
        token_type: "JWT",
    };
    let response = client.post(XBOX_USER_AUTH).json(&body).send().await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        return Err(AppError::detail(
            "xbox",
            crate::core::i18n::pick("Xbox Live 用户认证失败。", "Xbox Live sign-in failed."),
            format!("HTTP {status}"),
        ));
    }
    let auth: XboxAuthResponse = serde_json::from_str(&text).map_err(|err| {
        AppError::detail("xbox", crate::core::i18n::pick("无法解析 Xbox Live 认证结果。", "Couldn't read the Xbox Live sign-in result."), err.to_string())
    })?;
    let xui = auth
        .display_claims
        .xui
        .first()
        .ok_or_else(|| AppError::new("xbox", crate::core::i18n::pick("Xbox Live 未返回用户信息。", "Xbox Live returned no user info.")))?;
    let uhs = xui.uhs.clone();

    let xsts_body = XstsRequest {
        properties: XstsProperties {
            sandbox_id: "RETAIL",
            user_tokens: vec![&auth.token],
        },
        relying_party: "rp://api.minecraftservices.com/",
        token_type: "JWT",
    };
    let xsts_response = client.post(XSTS_AUTH).json(&xsts_body).send().await?;
    let xsts_status = xsts_response.status();
    let xsts_text = xsts_response.text().await?;
    if !xsts_status.is_success() {
        if let Ok(error) = serde_json::from_str::<XstsError>(&xsts_text) {
            let code = error.xerr.unwrap_or(0);
            let message = match code {
                2148916233 => crate::core::i18n::pick("该微软账号还没有 Xbox 档案，请先在 xbox.com 创建。", "This Microsoft account has no Xbox profile yet; create one at xbox.com first."),
                2148916235 => crate::core::i18n::pick("该账号所在地区不支持 Xbox Live。", "Xbox Live isn't available in this account's region."),
                2148916238 => crate::core::i18n::pick("儿童账号需要加入 Microsoft 家庭组后才能登录。", "Child accounts must be added to a Microsoft family before signing in."),
                _ => crate::core::i18n::pick("Xbox 授权失败，请稍后重试。", "Xbox authorization failed; try again later."),
            };
            return Err(AppError::detail(
                "xbox",
                message,
                format!("XErr {code}: {}", error.message.unwrap_or_default()),
            ));
        }
        return Err(AppError::detail(
            "xbox",
            crate::core::i18n::pick("Xbox 授权失败，请稍后重试。", "Xbox authorization failed; try again later."),
            format!("HTTP {xsts_status}"),
        ));
    }
    let xsts: XboxAuthResponse = serde_json::from_str(&xsts_text)
        .map_err(|err| AppError::detail("xbox", crate::core::i18n::pick("无法解析 Xbox 授权结果。", "Couldn't read the Xbox authorization result."), err.to_string()))?;
    let xsts_xui = xsts
        .display_claims
        .xui
        .first()
        .ok_or_else(|| AppError::new("xbox", crate::core::i18n::pick("Xbox 授权未返回用户信息。", "Xbox authorization returned no user info.")))?;

    Ok(XboxSession {
        xsts_token: xsts.token,
        uhs,
        xuid: xsts_xui.xid.clone(),
    })
}

async fn login_with_xbox(client: &reqwest::Client, xbox: &XboxSession) -> AppResult<(String, Duration)> {
    let identity_token = format!("XBL3.0 x={};{}", xbox.uhs, xbox.xsts_token);
    let body = MinecraftLoginRequest {
        identity_token: &identity_token,
    };
    let response = client.post(MINECRAFT_LOGIN).json(&body).send().await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        return Err(AppError::detail(
            "minecraft_auth",
            crate::core::i18n::pick("Minecraft 登录失败，请重试。", "Minecraft sign-in failed; try again."),
            format!("HTTP {status}"),
        ));
    }
    let login: MinecraftLoginResponse = serde_json::from_str(&text).map_err(|err| {
        AppError::detail(
            "minecraft_auth",
            crate::core::i18n::pick("无法解析 Minecraft 登录结果。", "Couldn't read the Minecraft sign-in result."),
            err.to_string(),
        )
    })?;
    let lifetime = login.expires_in.map(Duration::from_secs).unwrap_or(DEFAULT_TOKEN_LIFETIME);
    Ok((login.access_token, lifetime))
}

async fn fetch_profile(
    client: &reqwest::Client,
    minecraft_access_token: &str,
) -> AppResult<MinecraftProfile> {
    let response = client
        .get(MINECRAFT_PROFILE)
        .bearer_auth(minecraft_access_token)
        .send()
        .await?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(AppError::new(
            "not_owned",
            crate::core::i18n::pick("该微软账号未拥有 Minecraft Java 版，无法使用正版启动。", "This Microsoft account doesn't own Minecraft: Java Edition."),
        ));
    }
    let text = response.text().await?;
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(AppError::new(
            "auth_expired",
            crate::core::i18n::pick("Minecraft 登录已过期，请重新登录。", "Minecraft sign-in expired; sign in again."),
        ));
    }
    if !status.is_success() {
        return Err(AppError::detail(
            "minecraft_profile",
            crate::core::i18n::pick("无法读取 Minecraft 账号信息。", "Couldn't read the Minecraft account info."),
            format!("HTTP {status}"),
        ));
    }
    serde_json::from_str(&text).map_err(|err| {
        AppError::detail(
            "minecraft_profile",
            crate::core::i18n::pick("无法解析 Minecraft 账号信息。", "Couldn't parse the Minecraft account info."),
            err.to_string(),
        )
    })
}

/// The skin in use (URL and whether slim) and the owned capes.
pub async fn appearance(client: &reqwest::Client, token: &str) -> AppResult<(Option<String>, bool, Vec<Cape>)> {
    let profile = fetch_profile(client, token).await?;
    let skin = active_skin(profile.skins.unwrap_or_default());
    let slim = skin.as_ref().is_some_and(|skin| skin.variant.eq_ignore_ascii_case("slim"));
    Ok((skin.map(|skin| skin.url), slim, profile.capes))
}

async fn checked(response: reqwest::Response, what: &str) -> AppResult<()> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = response.text().await.unwrap_or_default();
    Err(match status.as_u16() {
        401 => AppError::new("auth_expired", crate::core::i18n::pick("Minecraft 登录已过期，请重新登录。", "Minecraft sign-in expired; sign in again.")),
        400 => AppError::detail("minecraft_skin", crate::tr!("{what}失败：皮肤图片不符合要求（64×64 或 64×32 的 PNG）。", "{what} failed: the skin must be a 64×64 or 64×32 PNG."), body),
        429 => AppError::new("minecraft_skin", crate::tr!("{what}太频繁了，请过一会儿再试。", "{what}: too many tries; wait a bit and try again.")),
        _ => AppError::detail("minecraft_skin", crate::tr!("{what}失败（HTTP {status}）。", "{what} failed (HTTP {status})."), body),
    })
}

/// Uploads a skin PNG; `slim` picks the 3-pixel arms.
pub async fn upload_skin(client: &reqwest::Client, token: &str, png: Vec<u8>, slim: bool) -> AppResult<()> {
    let (content_type, body) = crate::core::auth::skin_form(&[("variant", if slim { "slim" } else { "classic" })], png);
    let response = client
        .post(format!("{MINECRAFT_PROFILE}/skins"))
        .bearer_auth(token)
        .header("content-type", content_type)
        .body(body)
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    checked(response, crate::core::i18n::pick("上传皮肤", "Uploading the skin")).await
}

/// Back to the default skin.
pub async fn reset_skin(client: &reqwest::Client, token: &str) -> AppResult<()> {
    let response = client.delete(format!("{MINECRAFT_PROFILE}/skins/active")).bearer_auth(token).timeout(Duration::from_secs(20)).send().await?;
    checked(response, crate::core::i18n::pick("恢复默认皮肤", "Resetting the skin")).await
}

/// Shows a cape, or none.
pub async fn set_cape(client: &reqwest::Client, token: &str, cape: Option<&str>) -> AppResult<()> {
    let request = match cape {
        Some(id) => client.put(format!("{MINECRAFT_PROFILE}/capes/active")).json(&serde_json::json!({ "capeId": id })),
        None => client.delete(format!("{MINECRAFT_PROFILE}/capes/active")),
    };
    let response = request.bearer_auth(token).timeout(Duration::from_secs(20)).send().await?;
    checked(response, crate::core::i18n::pick("更换披风", "Changing the cape")).await
}

fn normalize_uuid(raw: &str) -> AppResult<String> {
    Uuid::parse_str(raw)
        .map(|uuid| uuid.to_string())
        .map_err(|_| AppError::new("minecraft_profile", crate::core::i18n::pick("Minecraft 返回了无效的账号 UUID。", "Minecraft returned an invalid account UUID.")))
}

/// The skin the profile has in use; the first one when none is marked.
fn active_skin(skins: Vec<Skin>) -> Option<Skin> {
    let active = skins.iter().position(|skin| skin.state.eq_ignore_ascii_case("active")).unwrap_or(0);
    skins.into_iter().nth(active)
}

/// Exchange a Microsoft OAuth token for a Minecraft session. Returns the
/// account metadata, the launch session, the (possibly rotated) refresh
/// token and how long the session's token lasts. The caller is responsible
/// for storing the refresh token in Keychain.
pub async fn complete_login(
    client: &reqwest::Client,
    ms_token: OAuthToken,
) -> AppResult<(Account, MinecraftSession, Option<String>, Duration)> {
    let xbox = authenticate_xbox(client, &ms_token.access_token).await?;
    let (mc_access_token, lifetime) = login_with_xbox(client, &xbox).await?;
    let profile = fetch_profile(client, &mc_access_token).await?;
    let uuid = normalize_uuid(&profile.id)?;

    let skin = active_skin(profile.skins.unwrap_or_default());
    let skin_slim = skin.as_ref().is_some_and(|skin| skin.variant.eq_ignore_ascii_case("slim"));
    let skin_url = skin.map(|skin| skin.url);

    let account = Account {
        id: format!("microsoft:{uuid}"),
        provider: AccountProvider::Microsoft,
        username: profile.name,
        uuid: uuid.clone(),
        xuid: xbox.xuid,
        ownership: Some(true),
        skin_url,
        skin_slim,
        created_at: now_millis(),
        server: None,
        server_name: None,
    };
    let session = MinecraftSession {
        username: account.username.clone(),
        uuid: uuid.replace('-', ""),
        access_token: mc_access_token,
        xuid: account.xuid.clone(),
        user_type: "msa".to_string(),
    };
    Ok((account, session, ms_token.refresh_token, lifetime))
}

pub async fn refresh_login(
    client: &reqwest::Client,
    client_id: &str,
    refresh_token: &str,
) -> AppResult<(Account, MinecraftSession, Option<String>, Duration)> {
    let token = refresh_ms_token(client, client_id, refresh_token).await?;
    complete_login(client, token).await
}

pub fn pkce_pair() -> (String, String) {
    let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn build_authorize_url(
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> AppResult<String> {
    let mut url = Url::parse(&format!("{OAUTH_BASE}/authorize"))
        .map_err(|err| AppError::detail("auth", crate::core::i18n::pick("无法构建微软登录地址。", "Couldn't build the Microsoft sign-in address."), err.to_string()))?;
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", SCOPE)
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("prompt", "select_account");
    Ok(url.to_string())
}

/// A one-shot loopback OAuth receiver. `start` binds an ephemeral port and
/// builds the authorize URL; `wait_for_code` blocks until the browser redirect
/// arrives (or times out / is cancelled).
pub struct LoopbackFlow {
    server: Server,
    pub redirect_uri: String,
    pub authorize_url: String,
    state: String,
    verifier: String,
}

impl LoopbackFlow {
    pub fn start(client_id: &str) -> AppResult<Self> {
        let server = Server::http("127.0.0.1:0").map_err(|err| {
            AppError::detail("auth_server", crate::core::i18n::pick("无法启动本地登录回调服务。", "Couldn't start the local sign-in callback."), err.to_string())
        })?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|addr| addr.port())
            .ok_or_else(|| AppError::new("auth_server", crate::core::i18n::pick("无法确定本地登录回调端口。", "Couldn't find the local sign-in callback port.")))?;
        let redirect_uri = format!("http://localhost:{port}/callback");
        let (verifier, challenge) = pkce_pair();
        let state = Uuid::new_v4().to_string();
        let authorize_url = build_authorize_url(client_id, &redirect_uri, &state, &challenge)?;
        Ok(Self {
            server,
            redirect_uri,
            authorize_url,
            state,
            verifier,
        })
    }

    pub fn verifier(&self) -> &str {
        &self.verifier
    }

    pub fn wait_for_code(self, cancel: &CancellationToken) -> AppResult<String> {
        let deadline = Instant::now() + LOOPBACK_TIMEOUT;
        loop {
            if cancel.is_cancelled() {
                return Err(AppError::new("auth_denied", crate::core::i18n::pick("登录已取消。", "Sign-in cancelled.")));
            }
            if Instant::now() >= deadline {
                return Err(AppError::new("auth_timeout", crate::core::i18n::pick("登录超时，请重试。", "Sign-in timed out; try again.")));
            }
            match self.server.recv_timeout(Duration::from_millis(500)) {
                Ok(Some(request)) => {
                    let request_url = request.url().to_string();
                    let parsed = Url::parse(&format!("http://localhost{request_url}"));
                    let Ok(parsed) = parsed else {
                        respond(request, 400, crate::core::i18n::pick("请求无效", "Bad request"));
                        continue;
                    };
                    if parsed.path() != "/callback" {
                        respond(request, 404, crate::core::i18n::pick("未找到", "Not found"));
                        continue;
                    }
                    let params: HashMap<String, String> =
                        parsed.query_pairs().into_owned().collect::<HashMap<_, _>>();
                    if let Some(error) = params.get("error") {
                        respond(request, 400, crate::core::i18n::pick("登录未完成，可以关闭此页面。", "Sign-in didn't finish. You can close this page."));
                        return Err(AppError::detail(
                            "auth_denied",
                            crate::core::i18n::pick("登录未完成，请重试。", "Sign-in didn't finish; try again."),
                            error.clone(),
                        ));
                    }
                    let state = params.get("state").cloned().unwrap_or_default();
                    if state != self.state {
                        respond(request, 400, crate::core::i18n::pick("登录状态校验失败，请重试。", "The sign-in state check failed; try again."));
                        return Err(AppError::new(
                            "auth_state",
                            crate::core::i18n::pick("登录状态校验失败，请重新登录。", "The sign-in state check failed; sign in again."),
                        ));
                    }
                    let Some(code) = params.get("code").cloned() else {
                        respond(request, 400, crate::core::i18n::pick("缺少授权码，请重试。", "The authorization code is missing; try again."));
                        continue;
                    };
                    respond(request, 200, crate::core::i18n::pick("登录完成，可以关闭此页面并返回启动器。", "Signed in. You can close this page and go back to the launcher."));
                    return Ok(code);
                }
                Ok(None) => continue,
                Err(err) => {
                    return Err(AppError::detail(
                        "auth_server",
                        crate::core::i18n::pick("本地登录回调服务出错。", "The local sign-in callback failed."),
                        err.to_string(),
                    ));
                }
            }
        }
    }
}

fn respond(request: tiny_http::Request, status: u16, message: &str) {
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Islet</title></head>\
         <body style=\"font-family:-apple-system,BlinkMacSystemFont,sans-serif;background:#121417;color:#e8eaed;\
         display:flex;align-items:center;justify-content:center;height:100vh;margin:0\">\
         <p>{message}</p></body></html>"
    );
    let mut response = Response::from_string(html).with_status_code(status);
    if let Ok(header) = Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]) {
        response = response.with_header(header);
    }
    let _ = request.respond(response);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_active_skin_and_its_arms_are_taken() {
        let skins: Vec<Skin> = serde_json::from_str(
            r#"[{"url":"https://a","state":"INACTIVE","variant":"CLASSIC"},{"url":"https://b","state":"ACTIVE","variant":"SLIM"}]"#,
        )
        .unwrap();
        let skin = active_skin(skins).unwrap();
        assert_eq!(skin.url, "https://b");
        assert!(skin.variant.eq_ignore_ascii_case("slim"));
        let bare: Vec<Skin> = serde_json::from_str(r#"[{"url":"https://c"}]"#).unwrap();
        assert_eq!(active_skin(bare).unwrap().url, "https://c");
        assert!(active_skin(Vec::new()).is_none());
    }

    #[test]
    fn pkce_challenge_is_sha256_base64url() {
        let (verifier, challenge) = pkce_pair();
        assert!(verifier.len() >= 43);
        assert_eq!(challenge.len(), 43);
        assert!(!challenge.contains('='));
        assert!(!challenge.contains('+'));
        assert!(!challenge.contains('/'));
    }

    #[test]
    fn authorize_url_contains_required_parameters() {
        let url = build_authorize_url(
            "00000000-0000-0000-0000-000000000000",
            "http://localhost:1234/callback",
            "state-value",
            "challenge-value",
        )
        .unwrap();
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("XboxLive.signin"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1234%2Fcallback"));
        assert!(url.contains("state=state-value"));
    }

    #[test]
    fn validates_azure_client_id() {
        assert!(validate_client_id("00000000-0000-0000-0000-000000000000").is_ok());
        assert!(validate_client_id("not-a-guid").is_err());
        assert!(validate_client_id("").is_err());
    }
}
