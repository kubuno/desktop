//! The account the account-scoped data belongs to (`docs/STORAGE-COMPONENTS.md`, decision Q6): settings, key/value
//! stores, files and local databases declared `AccountScoped` live below `<user_data_dir>/accounts/<key>/<app>/` — the
//! layout of `kubuno-sync-engine`, so signing an account out (which deletes `accounts/<key>`) wipes them too — and,
//! for the Registry back-end, below `HKCU\Software\Kubuno\Accounts\<key>\Apps\<app>`.
//!
//! The app says which account is current ([`set_current_account`], after a sign-in or an account switch; the shell's
//! current account in a multi-account app). With no current account, account-scoped data is kept in memory for the
//! session (and a warning is logged once): it is never written under another account or under the app's own folder.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use crate::app::AppId;
use crate::{paths, StorageError};

/// An account key (`kubuno-account`'s `AccountKey`: 16 lower-case hex characters; any `[a-z0-9._-]{1,64}` is accepted
/// so tests and tools can name accounts).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AccountKey(String);

impl AccountKey {
    pub fn new(key: &str) -> Result<Self, StorageError> {
        let ok = !key.is_empty()
            && key.len() <= 64
            && key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
            && key.chars().next().is_some_and(|c| c.is_ascii_alphanumeric());
        if ok {
            Ok(Self(key.to_string()))
        } else {
            Err(StorageError::InvalidName(format!("account key '{key}'")))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AccountKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

static CURRENT: Mutex<Option<AccountKey>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Sets (or clears) the current account: account-scoped components reopen their data for it at their next frame.
pub fn set_current_account(key: Option<AccountKey>) {
    let mut c = CURRENT.lock().unwrap_or_else(PoisonError::into_inner);
    if *c != key {
        *c = key;
        GENERATION.fetch_add(1, Ordering::AcqRel);
    }
}

/// The current account, if one is set.
pub fn current_account() -> Option<AccountKey> {
    CURRENT.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Bumped by every change of the current account (what components compare to reopen).
pub fn account_generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

/// `<user_data_dir>/accounts/<key>/<app>`: the account-scoped folder of an app.
pub fn account_app_dir(account: &AccountKey, app: &AppId) -> Result<PathBuf, StorageError> {
    let data = paths::user_data_dir().map_err(|e| StorageError::Backend { backend: "file", message: format!("the data directory: {e}") })?;
    Ok(data.join("accounts").join(account.as_str()).join(app.as_str()))
}

/// Removes every account-scoped file of `app` for `account` (the account's own sign-out removes the whole
/// `accounts/<key>` folder; this is for an app that forgets one account's data). `Ok(false)` when there was none.
pub fn delete_account_app_data(account: &AccountKey, app: &AppId) -> Result<bool, StorageError> {
    let dir = account_app_dir(account, app)?;
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(StorageError::io(&dir, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_validated_and_the_current_account_is_tracked() {
        assert!(AccountKey::new("0123456789abcdef").is_ok());
        for bad in ["", "A", "a/b", "..", &"a".repeat(65)] {
            assert!(AccountKey::new(bad).is_err(), "{bad:?}");
        }
        let g = account_generation();
        set_current_account(Some(AccountKey::new("acc1").expect("key")));
        assert_eq!(current_account().map(|k| k.0), Some("acc1".into()));
        assert!(account_generation() > g);
        let g = account_generation();
        set_current_account(Some(AccountKey::new("acc1").expect("key")));
        assert_eq!(account_generation(), g, "the same account changes nothing");
        set_current_account(None);
    }
}
