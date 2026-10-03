//! macOS Keychain and Linux Secret Service back-ends, through the `keyring` crate. The service is `Kubuno`, the
//! account field the full target (`Kubuno/<scope>/<item>`), so the entries are recognisable in Keychain Access or
//! Seahorse.

use crate::{Secret, SecretError, SecretName, SERVICE};

#[cfg(target_os = "macos")]
const BACKEND: &str = "macos-keychain";
#[cfg(not(target_os = "macos"))]
const BACKEND: &str = "secret-service";

#[derive(Debug, Default)]
pub(crate) struct KeyringStore;

fn map_err(name: &SecretName, e: keyring::Error) -> SecretError {
    match e {
        keyring::Error::NoStorageAccess(inner) => SecretError::Unavailable(inner.to_string()),
        keyring::Error::PlatformFailure(inner) => SecretError::Unavailable(inner.to_string()),
        keyring::Error::TooLong(attr, max) => {
            SecretError::Backend { backend: BACKEND, message: format!("{name}: attribute {attr} longer than {max}") }
        }
        keyring::Error::BadEncoding(_) => SecretError::Corrupted(name.target()),
        other => SecretError::Backend { backend: BACKEND, message: format!("{name}: {other}") },
    }
}

fn entry(name: &SecretName) -> Result<keyring::Entry, SecretError> {
    keyring::Entry::new(SERVICE, &name.target()).map_err(|e| map_err(name, e))
}

impl KeyringStore {
    pub(crate) fn backend(&self) -> &'static str {
        BACKEND
    }

    pub(crate) fn get(&self, name: &SecretName) -> Result<Option<Secret>, SecretError> {
        match entry(name)?.get_secret() {
            Ok(bytes) => Ok(Some(Secret::new(bytes))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_err(name, e)),
        }
    }

    pub(crate) fn set(&self, name: &SecretName, value: &Secret) -> Result<(), SecretError> {
        entry(name)?.set_secret(value.expose()).map_err(|e| map_err(name, e))
    }

    pub(crate) fn delete(&self, name: &SecretName) -> Result<bool, SecretError> {
        match entry(name)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(map_err(name, e)),
        }
    }
}
