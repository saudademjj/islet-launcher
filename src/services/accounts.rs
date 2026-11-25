use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{AppServices, LoginMode, LoginStartData, SESSION_MARGIN, UiEvent, lock};
use crate::core::accounts::{Account, AccountProvider};
use crate::core::auth::{MinecraftSession, microsoft, offline, yggdrasil};
use crate::core::error::{AppError, AppResult};
use crate::core::keychain;
use crate::core::settings::Settings;

impl AppServices {
    pub fn add_offline_account(&self, username: &str) -> AppResult<(Account, Settings)> {
        self.ensure_storage_idle()?;
        if !self.accounts.has_owner() {
            return Err(crate::core::accounts::owner_required());
        }
        let account = offline::create_account(username)?;
        self.accounts.upsert(account.clone())?;
        let settings = self.settings.update(|settings| {
            settings.active_account_id = Some(account.id.clone());
        })?;
        Ok((account, settings))
    }

    pub fn remove_account(&self, id: &str) -> AppResult<(Vec<Account>, Settings)> {
        self.ensure_storage_idle()?;
        if let Some(account) = self.accounts.remove(id)? {
            // Third-party tokens are signed out on their server too.
            if account.provider == AccountProvider::Yggdrasil {
                let tokens = keychain::load_refresh_token(&account.id).ok().flatten().and_then(|t| serde_json::from_str::<yggdrasil::Tokens>(&t).ok());
                if let (Some(tokens), Some(api), Ok(http)) = (tokens, account.server.clone(), self.http()) {
                    self.spawn_task(async move { yggdrasil::invalidate(&http, &api, &tokens).await });
                }
            }
            lock(&self.sessions).remove(&account.id);
            if account.provider != AccountProvider::Offline
                && let Err(err) = keychain::delete_refresh_token(&account.id) {
                    tracing::warn!(account = %account.id, error = %err, "failed to delete refresh token");
                }
        }
        let remaining = self.accounts.list();
        let settings = self.settings.update(|settings| {
            if settings.active_account_id.as_deref() == Some(id) {
                settings.active_account_id = remaining.first().map(|account| account.id.clone());
            }
        })?;
        Ok((remaining, settings))
    }

    /// Signs in to a third-party Yggdrasil server. Returns the characters to
    /// pick from when the login has several and none was chosen.
    pub async fn login_yggdrasil(
        &self,
        address: &str,
        username: &str,
        password: &str,
        profile: Option<&str>,
    ) -> AppResult<Option<Vec<yggdrasil::GameProfile>>> {
        self.ensure_storage_idle()?;
        if !self.accounts.has_owner() {
            return Err(crate::core::accounts::owner_required());
        }
        let http = self.http()?;
        let server = yggdrasil::resolve(&http, address).await?;
        match yggdrasil::login(&http, &server, username, password, profile).await? {
            yggdrasil::Login::Choose(profiles) => Ok(Some(profiles)),
            yggdrasil::Login::Done(account, tokens) => {
                keychain::save_refresh_token(&account.id, &serde_json::to_string(&tokens)?)?;
                let id = account.id.clone();
                self.accounts.upsert(*account)?;
                self.settings.update(|settings| settings.active_account_id = Some(id))?;
                self.send(UiEvent::AccountsChanged(self.accounts.list()));
                Ok(None)
            }
        }
    }

    /// Capes a Microsoft account owns (empty for other accounts).
    pub async fn account_capes(&self, id: &str) -> AppResult<Vec<microsoft::Cape>> {
        let account = self.accounts.get(id).ok_or_else(|| AppError::new("account", crate::core::i18n::pick("账号不存在。", "The account doesn't exist.")))?;
        if account.provider != AccountProvider::Microsoft {
            return Ok(Vec::new());
        }
        let session = self.session_for_account(&account).await?;
        Ok(microsoft::appearance(&self.http()?, &session.access_token).await?.2)
    }

    /// Uploads a skin (`Some(path)`) or goes back to the default (`None`).
    pub async fn change_skin(&self, id: &str, path: Option<PathBuf>, slim: bool) -> AppResult<()> {
        let account = self.accounts.get(id).ok_or_else(|| AppError::new("account", crate::core::i18n::pick("账号不存在。", "The account doesn't exist.")))?;
        let png = match path {
            Some(path) => {
                let bytes = tokio::fs::read(&path).await?;
                crate::core::auth::check_skin(&bytes)?;
                Some(bytes)
            }
            None => None,
        };
        let http = self.http()?;
        let session = self.session_for_account(&account).await?;
        let mut updated = self.accounts.get(id).unwrap_or(account);
        match updated.provider {
            AccountProvider::Microsoft => {
                match png {
                    Some(png) => microsoft::upload_skin(&http, &session.access_token, png, slim).await?,
                    None => microsoft::reset_skin(&http, &session.access_token).await?,
                }
                let (url, slim, _) = microsoft::appearance(&http, &session.access_token).await?;
                updated.skin_url = url;
                updated.skin_slim = slim;
            }
            AccountProvider::Yggdrasil => {
                let tokens: yggdrasil::Tokens = keychain::load_refresh_token(&updated.id)?
                    .and_then(|text| serde_json::from_str(&text).ok())
                    .ok_or_else(|| AppError::new("auth_expired", crate::core::i18n::pick("登录已过期，请重新登录。", "Sign-in expired; sign in again.")))?;
                yggdrasil::change_skin(&http, &updated, &tokens, png.map(|png| (png, slim))).await?;
                if let Some(api) = updated.server.clone() {
                    let (url, slim) = yggdrasil::skin(&http, &api, &updated.uuid).await.unwrap_or((None, slim));
                    updated.skin_url = url;
                    updated.skin_slim = slim;
                }
            }
            AccountProvider::Offline => {
                crate::core::auth::local_skin::save(&self.paths, &mut updated, png, slim)?;
            }
        }
        self.accounts.upsert(updated)?;
        self.send(UiEvent::AccountsChanged(self.accounts.list()));
        Ok(())
    }

    pub async fn set_cape(&self, id: &str, cape: Option<&str>) -> AppResult<()> {
        let account = self.accounts.get(id).ok_or_else(|| AppError::new("account", crate::core::i18n::pick("账号不存在。", "The account doesn't exist.")))?;
        if account.provider != AccountProvider::Microsoft {
            return Err(AppError::new("cape", crate::core::i18n::pick("只有微软账号有披风。", "Only Microsoft accounts have capes.")));
        }
        let session = self.session_for_account(&account).await?;
        microsoft::set_cape(&self.http()?, &session.access_token, cape).await
    }

    pub fn set_active_account(&self, id: Option<String>) -> AppResult<Settings> {
        self.ensure_storage_idle()?;
        if let Some(ref account_id) = id
            && self.accounts.get(account_id).is_none() {
                return Err(AppError::new("account_not_found", crate::core::i18n::pick("账号不存在。", "The account doesn't exist.")));
            }
        self.settings
            .update(|settings| settings.active_account_id = id)
    }

    pub fn start_login(self: &Arc<Self>, mode: LoginMode) -> AppResult<String> {
        self.ensure_storage_idle()?;
        let client_id =
            microsoft::validate_client_id(&self.settings.get().effective_client_id())?.to_string();
        let login_id = Uuid::new_v4().to_string();
        let cancel = CancellationToken::new();
        lock(&self.logins).insert(login_id.clone(), cancel.clone());

        if mode == LoginMode::Device {
            let this = self.clone();
            let id = login_id.clone();
            let client = client_id.clone();
            let cancel_task = cancel.clone();
            self.spawn_task(async move {
                let result = async {
                    let http = this.http()?;
                    let info = microsoft::request_device_code(&http, &client).await?;
                    if cancel_task.is_cancelled() {
                        return Err(AppError::new("cancelled", crate::core::i18n::pick("登录已取消。", "Sign-in cancelled.")));
                    }
                    this.send(UiEvent::LoginStarted(LoginStartData {
                        login_id: id.clone(),
                        mode: "device".to_string(),
                        authorize_url: None,
                        user_code: Some(info.user_code.clone()),
                        verification_uri: Some(info.verification_uri.clone()),
                        expires_in: Some(info.expires_in),
                    }));
                    let token =
                        microsoft::poll_device_code(&http, &client, &info, &cancel_task).await?;
                    this.finish_login(&id, token, &cancel_task).await
                }
                .await;
                this.finish_login_result(&id, result);
            });
            return Ok(login_id);
        }

        let flow = match microsoft::LoopbackFlow::start(&client_id) {
            Ok(flow) => flow,
            Err(error) => {
                lock(&self.logins).remove(&login_id);
                return Err(error);
            }
        };
        let authorize_url = flow.authorize_url.clone();
        let redirect_uri = flow.redirect_uri.clone();
        let verifier = flow.verifier().to_string();
        self.send(UiEvent::LoginStarted(LoginStartData {
            login_id: login_id.clone(),
            mode: "browser".to_string(),
            authorize_url: Some(authorize_url),
            user_code: None,
            verification_uri: None,
            expires_in: None,
        }));

        let this = self.clone();
        let id = login_id.clone();
        let client = client_id.clone();
        let cancel_task = cancel.clone();
        self.spawn_task(async move {
            let result = async {
                let wait_cancel = cancel_task.clone();
                let code = tokio::task::spawn_blocking(move || flow.wait_for_code(&wait_cancel))
                    .await
                    .map_err(|err| {
                        AppError::detail("auth", crate::core::i18n::pick("登录等待任务失败。", "The sign-in wait failed."), err.to_string())
                    })??;
                let http = this.http()?;
                let token =
                    microsoft::exchange_code(&http, &client, &code, &redirect_uri, &verifier)
                        .await?;
                this.finish_login(&id, token, &cancel_task).await
            }
            .await;
            this.finish_login_result(&id, result);
        });
        Ok(login_id)
    }

    pub fn cancel_login(&self, login_id: &str) {
        if let Some(token) = lock(&self.logins).remove(login_id) {
            token.cancel();
        }
    }

    pub(super) async fn session_for_account(&self, account: &Account) -> AppResult<MinecraftSession> {
        match account.provider {
            AccountProvider::Offline => Ok(offline::session(account)),
            AccountProvider::Yggdrasil => {
                let tokens: yggdrasil::Tokens = keychain::load_refresh_token(&account.id)?
                    .and_then(|text| serde_json::from_str(&text).ok())
                    .ok_or_else(|| AppError::new("auth_expired", crate::core::i18n::pick("登录已过期，请在账号页重新登录。", "Sign-in expired; sign in again on the Accounts page.")))?;
                let (session, fresh) = yggdrasil::session(&self.http()?, account, &tokens).await?;
                if let Some(fresh) = fresh {
                    keychain::save_refresh_token(&account.id, &serde_json::to_string(&fresh)?)?;
                }
                Ok(session)
            }
            AccountProvider::Microsoft => {
                let settings = self.settings.get();
                let client_id =
                    microsoft::validate_client_id(&settings.effective_client_id())?.to_string();
                let refresh_token =
                    keychain::load_refresh_token(&account.id)?.ok_or_else(|| {
                        AppError::new("auth_expired", crate::core::i18n::pick("登录已过期，请重新登录微软账号。", "Sign-in expired; sign in to your Microsoft account again."))
                    })?;
                if let Some(session) = self.cached_session(&account.id) {
                    return Ok(session);
                }
                let http = self.http()?;
                let (updated, session, rotated, lifetime) =
                    microsoft::refresh_login(&http, &client_id, &refresh_token).await?;
                if let Some(rotated) = rotated {
                    keychain::save_refresh_token(&account.id, &rotated)?;
                }
                self.remember_session(&account.id, &session, lifetime);
                self.accounts.upsert(updated)?;
                self.send(UiEvent::AccountsChanged(self.accounts.list()));
                Ok(session)
            }
        }
    }

    /// The remembered session of a Microsoft account, while more than
    /// `SESSION_MARGIN` of it is left; after that it signs in again.
    fn cached_session(&self, account_id: &str) -> Option<MinecraftSession> {
        let mut sessions = lock(&self.sessions);
        match sessions.get(account_id) {
            Some((session, until)) if std::time::Instant::now() < *until => Some(session.clone()),
            Some(_) => {
                sessions.remove(account_id);
                None
            }
            None => None,
        }
    }

    fn remember_session(&self, account_id: &str, session: &MinecraftSession, lifetime: Duration) {
        if let Some(reusable) = lifetime.checked_sub(SESSION_MARGIN) {
            let until = std::time::Instant::now() + reusable;
            lock(&self.sessions).insert(account_id.to_string(), (session.clone(), until));
        }
    }

    async fn finish_login(
        &self,
        login_id: &str,
        token: microsoft::OAuthToken,
        cancel: &CancellationToken,
    ) -> AppResult<Account> {
        let cancelled = || AppError::new("cancelled", crate::core::i18n::pick("登录已取消。", "Sign-in cancelled."));
        let http = self.http()?;
        // The Xbox and Minecraft exchanges take a few seconds; a cancel
        // during them stops here instead of waiting.
        let (account, session, refresh_token, lifetime) = tokio::select! {
            result = microsoft::complete_login(&http, token) => result?,
            _ = cancel.cancelled() => return Err(cancelled()),
        };
        // The last check before anything is stored. A cancel after this
        // point is too late; the interface then just shows the new account.
        // The login stays in the table until finish_login_result, so storage
        // is not moved while the account is being written.
        if cancel.is_cancelled() || !lock(&self.logins).contains_key(login_id) {
            return Err(cancelled());
        }
        if let Some(refresh_token) = refresh_token {
            keychain::save_refresh_token(&account.id, &refresh_token)?;
        }
        self.remember_session(&account.id, &session, lifetime);
        self.accounts.upsert(account.clone())?;
        self.settings.update(|settings| {
            settings.active_account_id = Some(account.id.clone());
        })?;
        self.send(UiEvent::LoginComplete {
            login_id: login_id.to_string(),
            account: account.clone(),
        });
        Ok(account)
    }

    fn finish_login_result(&self, login_id: &str, result: AppResult<Account>) {
        lock(&self.logins).remove(login_id);
        if let Err(error) = result {
            if error.code() != "cancelled" {
                tracing::warn!(code = error.code(), error = %error, "microsoft login failed");
            }
            self.send(UiEvent::LoginError {
                login_id: login_id.to_string(),
                error: error.to_payload(),
            });
        }
    }
}

/// A sign-in that failed for a reason that may pass (no network, Xbox or
/// Minecraft services down) becomes `auth_offline`, which the interface
/// answers with "play offline". Expired or invalid sign-ins stay as they are:
/// signing in again fixes those.
pub(super) fn offer_offline(account: &Account, err: AppError) -> AppError {
    let passing = matches!(
        err.code(),
        "network" | "auth" | "auth_slow_down" | "xbox" | "minecraft_auth" | "minecraft_profile" | "yggdrasil"
    );
    if account.provider == AccountProvider::Offline || !passing {
        return err;
    }
    let reason = match err.detail_text() {
        Some(detail) => format!("{} ({detail})", err.user_message()),
        None => err.user_message(),
    };
    AppError::detail(
        "auth_offline",
        crate::core::i18n::pick(
            "连不上登录服务器。可以先以离线身份玩单人游戏，存档和进度都在；联机服务器要等网络恢复。",
            "Couldn't reach the sign-in servers. You can play singleplayer offline for now with your saves intact; online servers need the connection back.",
        ),
        reason,
    )
}
