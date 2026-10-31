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
