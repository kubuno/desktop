//! [`AppSecrets`]: an app's own secrets (an API key of a third-party service, a mail account's password, a local
//! encryption key) in the OS credential store of `kubuno-desktop-secrets`, scoped to the app (`docs/STORAGE-COMPONENTS.md`
//! §4.2).
//!
//! | OS | Store | Target |
//! |---|---|---|
//! | Windows | Credential Manager (`CRED_PERSIST_LOCAL_MACHINE`: never roams) | `Kubuno/app.<app>/<name>` |
//! | macOS | Keychain (device-local) | service `Kubuno/app.<app>/<name>` |
//! | Linux | Secret Service (GNOME Keyring, KWallet) | idem |
//!
//! A sandboxed profile (`KUBUNO_SANDBOX_DIR`) keeps its secrets under `Kubuno/sandbox-<tag>.app.<app>/…`, apart from
//! the real ones. Account secrets (refresh tokens, database keys) are `kubuno-desktop-account`'s and live under the account
//! key's scope: an app never reaches them through this type.
//!
//! Values are [`Secret`]s (zeroed on drop, redacted in `Debug`); errors name the secret, never its value. The calls
//! block (a D-Bus round trip, a Keychain prompt): async code wraps them in `spawn_blocking`.

use std::sync::Arc;

pub use kubuno_desktop_secrets::Secret;
use kubuno_desktop_secrets::{MemorySecretStore, PrefixedSecretStore, SecretName, SecretStore};

use crate::app::AppId;
use crate::{paths, StorageError};

/// The longest secret name.
pub const MAX_SECRET_NAME: usize = 64;

/// Whether `name` is a valid secret name: 1 to 64 characters `[A-Za-z0-9._-]`.
pub fn valid_secret_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_SECRET_NAME && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// The secrets of one app (see the module doc). Cheap to clone.
#[derive(Debug, Clone)]
pub struct AppSecrets {
    app: AppId,
    store: Arc<dyn SecretStore>,
}

impl AppSecrets {
    /// The OS credential store (sandbox-aware). Linux without a Secret Service: the calls fail with
    /// `Unavailable` — check [`AppSecrets::probe`] first and offer to keep the secret for the session only.
    #[cfg(feature = "os")]
    pub fn open(app: &AppId) -> Result<Self, StorageError> {
        Self::with_store(app, Arc::new(kubuno_desktop_secrets::OsSecretStore::new()))
    }

    /// On a store of the caller's (tests: `MemorySecretStore`); the sandbox prefix still applies.
    pub fn with_store(app: &AppId, store: Arc<dyn SecretStore>) -> Result<Self, StorageError> {
        let store: Arc<dyn SecretStore> = match paths::sandbox_tag() {
            Some(tag) => Arc::new(PrefixedSecretStore::new(ArcStore(store), &format!("sandbox-{tag}"))?),
            None => store,
        };
        Ok(Self { app: app.clone(), store })
    }

    /// Secrets kept in memory for the life of the process (the designer, `--sample` runs, a Linux session without a
    /// Secret Service when the user declined the file fallback).
    pub fn in_memory(app: &AppId) -> Self {
        Self { app: app.clone(), store: Arc::new(MemorySecretStore::new()) }
    }

    pub fn app(&self) -> &AppId {
        &self.app
    }

    /// The store's name (`"windows-credential-manager"`, `"memory"`…).
    pub fn backend(&self) -> &'static str {
        self.store.backend()
    }

    fn name(&self, name: &str) -> Result<SecretName, StorageError> {
        if !valid_secret_name(name) {
            return Err(StorageError::InvalidName(format!("secret '{name}' (1 to {MAX_SECRET_NAME} characters A-Z, a-z, 0-9, '.', '-', '_')")));
        }
        Ok(SecretName::new(&format!("app.{}", self.app), name)?)
    }

    /// Whether the store can be used now (see [`AppSecrets::open`]).
    pub fn probe(&self) -> Result<(), StorageError> {
        self.store.get(&self.name("probe")?).map(|_| ()).map_err(StorageError::from)
    }

    pub fn get(&self, name: &str) -> Result<Option<Secret>, StorageError> {
        Ok(self.store.get(&self.name(name)?)?)
    }

    /// Creates or replaces the secret; returns once the store has it.
    pub fn set(&self, name: &str, value: &Secret) -> Result<(), StorageError> {
        Ok(self.store.set(&self.name(name)?, value)?)
    }

    /// Stores a text secret.
    pub fn set_str(&self, name: &str, value: &str) -> Result<(), StorageError> {
        self.set(name, &Secret::from_str_value(value))
    }

    /// Removes the secret; `Ok(false)` when there was none.
    pub fn delete(&self, name: &str) -> Result<bool, StorageError> {
        Ok(self.store.delete(&self.name(name)?)?)
    }

    pub fn contains(&self, name: &str) -> Result<bool, StorageError> {
        Ok(self.get(name)?.is_some())
    }
}

/// An `Arc<dyn SecretStore>` as a `SecretStore` (what `PrefixedSecretStore` wraps).
#[derive(Debug)]
struct ArcStore(Arc<dyn SecretStore>);

impl SecretStore for ArcStore {
    fn backend(&self) -> &'static str {
        self.0.backend()
    }
    fn get(&self, name: &SecretName) -> Result<Option<Secret>, kubuno_desktop_secrets::SecretError> {
        self.0.get(name)
    }
    fn set(&self, name: &SecretName, value: &Secret) -> Result<(), kubuno_desktop_secrets::SecretError> {
        self.0.set(name, value)
    }
    fn delete(&self, name: &SecretName) -> Result<bool, kubuno_desktop_secrets::SecretError> {
        self.0.delete(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_scoped_to_their_app() {
        let shared: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
        let a = AppSecrets::with_store(&AppId::new("app-a").expect("id"), shared.clone()).expect("a");
        let b = AppSecrets::with_store(&AppId::new("app-b").expect("id"), shared.clone()).expect("b");
        a.set_str("ApiKey", "a-value").expect("set");
        assert!(b.get("ApiKey").expect("get").is_none(), "another app never sees it");
        assert_eq!(a.get("ApiKey").expect("get").map(|s| s.expose().to_vec()), Some(b"a-value".to_vec()));
        assert!(a.delete("ApiKey").expect("delete"));
        assert!(!a.contains("ApiKey").expect("contains"));
        assert!(a.set_str("bad name", "x").is_err());
        let err = a.set_str("x".repeat(65).as_str(), "secret-value").expect_err("too long").to_string();
        assert!(!err.contains("secret-value"), "{err}");
    }
}
