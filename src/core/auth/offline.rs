use uuid::Uuid;

use crate::core::accounts::{Account, AccountProvider};
use crate::core::auth::MinecraftSession;
use crate::core::error::{AppError, AppResult};
use crate::core::util::now_millis;

/// Java's UUID.nameUUIDFromBytes("OfflinePlayer:" + name) uses MD5 with the
/// version/variant bits patched in, so it is not the same as UUID v3 with an
/// arbitrary namespace.
pub fn offline_uuid(name: &str) -> String {
    let digest = md5::compute(format!("OfflinePlayer:{name}"));
    let mut bytes = digest.0;
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes).to_string()
}

pub fn create_account(username: &str) -> AppResult<Account> {
    let name = username.trim();
    let count = name.chars().count();
    if count == 0 || count > 16 {
        return Err(AppError::new(
            "offline_name",
            crate::core::i18n::pick("离线用户名需要 1-16 个字符。", "Offline usernames need 1–16 characters."),
        ));
    }
    if name.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err(AppError::new(
            "offline_name",
            crate::core::i18n::pick("离线用户名不能包含空格或控制字符。", "Offline usernames can't contain spaces or control characters."),
        ));
    }

    let uuid = offline_uuid(name);
    Ok(Account {
        id: format!("offline:{uuid}"),
        provider: AccountProvider::Offline,
        username: name.to_string(),
        uuid,
        xuid: None,
        ownership: None,
        skin_url: None,
        skin_slim: false,
        created_at: now_millis(),
        server: None,
        server_name: None,
    })
}

pub fn session(account: &Account) -> MinecraftSession {
    MinecraftSession {
        username: account.username.clone(),
        uuid: account.uuid.replace('-', ""),
        access_token: "0".to_string(),
        xuid: None,
        user_type: "legacy".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_java_algorithm() {
        assert_eq!(
            offline_uuid("Notch"),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
        assert_eq!(
            offline_uuid("E2EUser"),
            "b5aafda2-30b0-32ae-8aac-ea21cbc5841d"
        );
    }

    #[test]
    fn rejects_names_with_spaces() {
        assert!(create_account("two words").is_err());
        assert!(create_account("").is_err());
        assert!(create_account("Notch").is_ok());
    }
}
