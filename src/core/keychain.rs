use crate::core::error::{AppError, AppResult};

const SERVICE: &str = "com.saudade.mclauncher.auth";

fn entry(account_id: &str) -> AppResult<keyring::Entry> {
    keyring::Entry::new(SERVICE, &format!("refresh-token:{account_id}")).map_err(|err| {
        AppError::detail(
            "keychain",
            crate::core::i18n::pick("无法访问系统钥匙串。", "Couldn't access the system keychain."),
            format!("{err} (account {account_id})"),
        )
    })
}

pub fn save_refresh_token(account_id: &str, token: &str) -> AppResult<()> {
    entry(account_id)?
        .set_password(token)
        .map_err(|err| AppError::detail("keychain", crate::core::i18n::pick("保存登录凭据失败。", "Couldn't save the sign-in credentials."), err.to_string()))
}

pub fn load_refresh_token(account_id: &str) -> AppResult<Option<String>> {
    match entry(account_id)?.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(AppError::detail(
            "keychain",
            crate::core::i18n::pick("读取登录凭据失败。", "Couldn't read the sign-in credentials."),
            err.to_string(),
        )),
    }
}

pub fn delete_refresh_token(account_id: &str) -> AppResult<()> {
    match entry(account_id)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(AppError::detail(
            "keychain",
            crate::core::i18n::pick("删除登录凭据失败。", "Couldn't delete the sign-in credentials."),
            err.to_string(),
        )),
    }
}

/// Other secrets, such as the password of a server the player runs, under
/// their own names ("ssh:<world id>").
fn secret_entry(key: &str) -> AppResult<keyring::Entry> {
    keyring::Entry::new(SERVICE, key).map_err(|err| AppError::detail("keychain", crate::core::i18n::pick("无法访问系统钥匙串。", "Couldn't access the system keychain."), err.to_string()))
}

pub fn save_secret(key: &str, secret: &str) -> AppResult<()> {
    secret_entry(key)?
        .set_password(secret)
        .map_err(|err| AppError::detail("keychain", crate::core::i18n::pick("保存密码失败。", "Couldn't save the password."), err.to_string()))
}

pub fn load_secret(key: &str) -> Option<String> {
    secret_entry(key).ok()?.get_password().ok()
}

pub fn delete_secret(key: &str) {
    if let Ok(entry) = secret_entry(key) {
        let _ = entry.delete_credential();
    }
}
