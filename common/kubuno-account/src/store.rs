//! The accounts of this machine: `<data>/accounts/<account_key>/account.json` (server URL, user id, display name,
//! linked file-sync instances; **no secret**). The databases of an account live next to it
//! (`<app>.db`, `blobs/<app>/`), so removing an account removes one directory.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::key::{normalize_server_url, AccountKey};

/// What is known about an account without the network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountInfo {
    pub key: AccountKey,
    /// Normalized server URL.
    pub server_url: String,
    pub user_id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    /// File-sync instances (`%APPDATA%\kubuno-desktop\instances\<id>`) that are sync roots of this account.
    #[serde(default)]
    pub linked_instances: Vec<String>,
}

impl AccountInfo {
    pub fn new(server_url: &str, user_id: &str) -> Result<Self, AccountError> {
        Ok(Self {
            key: AccountKey::new(server_url, user_id)?,
            server_url: normalize_server_url(server_url)?,
            user_id: user_id.trim().to_string(),
            display_name: None,
            email: None,
            linked_instances: Vec::new(),
        })
    }
}

/// Errors of the account layer.
#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error(transparent)]
    Key(#[from] crate::key::KeyError),
    #[error("account storage: {0}")]
    Io(String),
    #[error("account file {path} is invalid: {message}")]
    Invalid { path: String, message: String },
    #[error(transparent)]
    Secret(#[from] kubuno_secrets::SecretError),
    #[error("unknown account {0}")]
    Unknown(AccountKey),
    #[error(transparent)]
    Api(#[from] kubuno_api_client::ApiError),
}

fn io(context: &str, path: &Path, e: std::io::Error) -> AccountError {
    AccountError::Io(format!("{context} {}: {e}", path.display()))
}

/// The directory of all accounts.
#[derive(Debug, Clone)]
pub struct AccountStore {
    root: PathBuf,
}

impl AccountStore {
    /// `data_dir` is the per-user data directory (`kubuno_sync_engine::paths::user_data_dir()`); accounts go to
    /// `<data_dir>/accounts`.
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self { root: data_dir.as_ref().join("accounts") }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory of an account (databases, blobs, `account.json`).
    pub fn account_dir(&self, key: &AccountKey) -> PathBuf {
        self.root.join(key.as_str())
    }

    fn file(&self, key: &AccountKey) -> PathBuf {
        self.account_dir(key).join("account.json")
    }

    pub fn load(&self, key: &AccountKey) -> Result<Option<AccountInfo>, AccountError> {
        let path = self.file(key);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io("cannot read", &path, e)),
        };
        let info: AccountInfo = serde_json::from_str(&text)
            .map_err(|e| AccountError::Invalid { path: path.display().to_string(), message: e.to_string() })?;
        if &info.key != key {
            return Err(AccountError::Invalid { path: path.display().to_string(), message: "the key does not match its directory".to_string() });
        }
        Ok(Some(info))
    }

    /// Writes `account.json` atomically.
    pub fn save(&self, info: &AccountInfo) -> Result<(), AccountError> {
        let dir = self.account_dir(&info.key);
        std::fs::create_dir_all(&dir).map_err(|e| io("cannot create", &dir, e))?;
        let path = self.file(&info.key);
        let text = serde_json::to_vec_pretty(info).map_err(|e| AccountError::Io(e.to_string()))?;
        kubuno_secrets::write_private(&path, &text).map_err(|e| io("cannot write", &path, e))
    }

    /// Every account with a readable `account.json`, sorted by key. Unreadable ones are logged and skipped.
    pub fn list(&self) -> Result<Vec<AccountInfo>, AccountError> {
        let rd = match std::fs::read_dir(&self.root) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io("cannot list", &self.root, e)),
        };
        let mut out = Vec::new();
        for entry in rd.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Ok(key) = AccountKey::parse(name) else { continue };
            match self.load(&key) {
                Ok(Some(info)) => out.push(info),
                Ok(None) => {}
                Err(e) => tracing::warn!(account = %key, error = %e, "skipping an unreadable account"),
            }
        }
        out.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(out)
    }

    /// Records that file-sync instance `instance` is a sync root of account `key` (idempotent).
    pub fn link_instance(&self, key: &AccountKey, instance: &str) -> Result<AccountInfo, AccountError> {
        let mut info = self.load(key)?.ok_or_else(|| AccountError::Unknown(key.clone()))?;
        if !info.linked_instances.iter().any(|i| i == instance) {
            info.linked_instances.push(instance.to_string());
            info.linked_instances.sort();
            self.save(&info)?;
        }
        Ok(info)
    }

    /// Forgets the link of `instance` to `key`. Returns the account as saved (`None` when it does not exist).
    pub fn unlink_instance(&self, key: &AccountKey, instance: &str) -> Result<Option<AccountInfo>, AccountError> {
        let Some(mut info) = self.load(key)? else { return Ok(None) };
        let before = info.linked_instances.len();
        info.linked_instances.retain(|i| i != instance);
        if info.linked_instances.len() != before {
            self.save(&info)?;
        }
        Ok(Some(info))
    }

    /// The account a file-sync instance belongs to, if any.
    pub fn account_of_instance(&self, instance: &str) -> Result<Option<AccountInfo>, AccountError> {
        Ok(self.list()?.into_iter().find(|a| a.linked_instances.iter().any(|i| i == instance)))
    }

    /// Deletes the account directory (databases, blobs, `account.json`). The caller deletes the secrets and asks
    /// the user first when the outbox is not empty (`Envoyer d'abord`, §10).
    pub fn remove_dir(&self, key: &AccountKey) -> Result<(), AccountError> {
        let dir = self.account_dir(key);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io("cannot remove", &dir, e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_list_remove() {
        let dir = tempfile::tempdir().expect("tmp");
        let store = AccountStore::new(dir.path());
        assert!(store.list().expect("list").is_empty());
        let mut a = AccountInfo::new("https://a.example", "u1").expect("info");
        a.display_name = Some("Alice".into());
        store.save(&a).expect("save");
        let b = AccountInfo::new("https://b.example", "u1").expect("info");
        store.save(&b).expect("save");
        assert_eq!(store.load(&a.key).expect("load"), Some(a.clone()));
        assert_eq!(store.list().expect("list").len(), 2);
        store.link_instance(&b.key, "host-1234abcd").expect("link");
        store.link_instance(&b.key, "host-1234abcd").expect("link twice");
        assert_eq!(store.account_of_instance("host-1234abcd").expect("find").map(|i| i.key), Some(b.key.clone()));
        assert_eq!(store.load(&b.key).expect("load").expect("some").linked_instances, vec!["host-1234abcd".to_string()]);
        store.remove_dir(&a.key).expect("remove");
        let b2 = store.unlink_instance(&b.key, "host-1234abcd").expect("unlink").expect("some");
        assert!(b2.linked_instances.is_empty());
        assert!(store.account_of_instance("host-1234abcd").expect("find").is_none());
        assert_eq!(store.list().expect("list"), vec![b2]);
    }
}
