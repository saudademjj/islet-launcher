use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::core::error::{AppError, AppResult};
use crate::core::util::{atomic_write, read_json_or_default};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AccountProvider {
    Microsoft,
    Offline,
    /// A third-party Yggdrasil server (LittleSkin and others), played
    /// through authlib-injector.
    Yggdrasil,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub provider: AccountProvider,
    pub username: String,
    pub uuid: String,
    pub xuid: Option<String>,
    pub ownership: Option<bool>,
    pub skin_url: Option<String>,
    /// The skin uses the slim (3-pixel) arms. Filled in at sign-in and on
    /// each session refresh; accounts saved before it read as classic.
    #[serde(default)]
    pub skin_slim: bool,
    pub created_at: i64,
    /// Third-party accounts: the server's API root and its display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
}

impl Account {
    pub fn owns_game(&self) -> bool {
        self.provider == AccountProvider::Microsoft && self.ownership == Some(true)
    }
}

/// Offline accounts need a Microsoft account that owns the game first.
pub fn owner_required() -> AppError {
    AppError::new(
        "owner_required",
        crate::core::i18n::pick(
            "离线账号需要先登录一个拥有 Minecraft Java 版的微软账号。",
            "Offline accounts need a Microsoft account that owns Minecraft: Java Edition signed in first.",
        ),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountFile {
    pub accounts: Vec<Account>,
}

pub struct AccountStore {
    path: PathBuf,
    accounts: RwLock<Vec<Account>>,
}

impl AccountStore {
    pub fn load(path: PathBuf) -> AppResult<Self> {
        let mut file: AccountFile = read_json_or_default(&path)?;
        file.accounts.sort_by_key(|account| account.created_at);
        file.accounts.dedup_by(|a, b| a.id == b.id);
        Ok(Self {
            path,
            accounts: RwLock::new(file.accounts),
        })
    }

    pub fn list(&self) -> Vec<Account> {
        self.accounts
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub fn get(&self, id: &str) -> Option<Account> {
        self.accounts
            .read()
            .ok()
            .and_then(|guard| guard.iter().find(|account| account.id == id).cloned())
    }

    /// Whether a Microsoft account that owns Java Edition has signed in.
    /// Offline accounts open only after that, so the launcher doesn't play
    /// the game for people who don't own it. Microsoft accounts are saved
    /// only once their Minecraft profile loads, so any one here owns it.
    pub fn has_owner(&self) -> bool {
        self.accounts.read().map(|guard| guard.iter().any(Account::owns_game)).unwrap_or(false)
    }

    pub fn upsert(&self, account: Account) -> AppResult<Account> {
        let mut guard = self
            .accounts
            .write()
            .map_err(|_| AppError::new("accounts", crate::core::i18n::pick("账号存储被锁定，请重试。", "Account storage is locked; try again.")))?;
        guard.retain(|existing| existing.id != account.id);
        guard.push(account.clone());
        guard.sort_by_key(|item| item.created_at);
        self.save_locked(&guard)?;
        Ok(account)
    }

    pub fn remove(&self, id: &str) -> AppResult<Option<Account>> {
        let mut guard = self
            .accounts
            .write()
            .map_err(|_| AppError::new("accounts", crate::core::i18n::pick("账号存储被锁定，请重试。", "Account storage is locked; try again.")))?;
        let removed = guard
            .iter()
            .position(|account| account.id == id)
            .map(|index| guard.remove(index));
        if removed.is_some() {
            self.save_locked(&guard)?;
        }
        Ok(removed)
    }

    fn save_locked(&self, accounts: &[Account]) -> AppResult<()> {
        let file = AccountFile {
            accounts: accounts.to_vec(),
        };
        let encoded = serde_json::to_vec_pretty(&file)?;
        atomic_write(&self.path, &encoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, provider: AccountProvider, ownership: Option<bool>) -> Account {
        Account {
            id: id.into(),
            provider,
            username: "Steve".into(),
            uuid: "00000000-0000-0000-0000-000000000001".into(),
            xuid: None,
            ownership,
            skin_url: None,
            skin_slim: false,
            created_at: 0,
            server: None,
            server_name: None,
        }
    }

    #[test]
    fn only_a_microsoft_account_that_owns_the_game_counts() {
        let dir = tempfile::tempdir().unwrap();
        let store = AccountStore::load(dir.path().join("accounts.json")).unwrap();
        assert!(!store.has_owner());
        store.upsert(account("offline:a", AccountProvider::Offline, None)).unwrap();
        store.upsert(account("yggdrasil:b", AccountProvider::Yggdrasil, None)).unwrap();
        store.upsert(account("microsoft:c", AccountProvider::Microsoft, Some(false))).unwrap();
        assert!(!store.has_owner());
        store.upsert(account("microsoft:d", AccountProvider::Microsoft, Some(true))).unwrap();
        assert!(store.has_owner());
        store.remove("microsoft:d").unwrap();
        assert!(!store.has_owner());
    }
}
