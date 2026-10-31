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
