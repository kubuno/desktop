//! [`AppId`]: the identity every piece of stored data is scoped to (module isolation, `docs/STORAGE-COMPONENTS.md`
//! §6.1). It becomes a folder name (`<config>/<app>/`), a Registry key (`Software\Kubuno\Apps\<app>`) and part of a
//! credential target (`Kubuno/app.<app>/<item>`), so it is restricted to what is safe in all three on every OS.

use std::fmt;
use std::sync::{Mutex, PoisonError};

use crate::StorageError;

/// The longest app id: `sandbox-<tag>.app.<id>` must stay within the 64 characters of a credential scope.
pub const MAX_APP_ID: usize = 40;

/// The names Windows reserves for devices: a folder with one of them cannot be created.
const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7",
    "lpt8", "lpt9",
];

/// An app's id: 1 to 40 characters `[a-z0-9._-]`, starting with a letter or a digit, not ending with a dot, not a
/// Windows device name. Lower case only, so two ids never collide on a case-insensitive file system or in the
/// Registry. Usually the Cargo package name (`kubuno-notes`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AppId(String);

impl AppId {
    pub fn new(id: &str) -> Result<Self, StorageError> {
        let ok = !id.is_empty()
            && id.len() <= MAX_APP_ID
            && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
            && id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
            && !id.ends_with('.')
            && !RESERVED.contains(&id.split('.').next().unwrap_or(id));
        if ok {
            Ok(Self(id.to_string()))
        } else {
            Err(StorageError::InvalidName(format!(
                "app id '{id}' (1 to {MAX_APP_ID} characters a-z, 0-9, '.', '-', '_', starting with a letter or a digit)"
            )))
        }
    }

    /// The id of a Cargo package or an executable name: lower-cased, every other character replaced by `-`,
    /// truncated to [`MAX_APP_ID`]. `None` when nothing usable is left.
    pub fn from_name(name: &str) -> Option<Self> {
        let mut id: String = name
            .trim()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c.to_ascii_lowercase() } else { '-' })
            .collect();
        id = id.trim_start_matches(|c: char| !c.is_ascii_alphanumeric()).to_string();
        id.truncate(MAX_APP_ID);
        let id = id.trim_end_matches('.').to_string();
        Self::new(&id).ok()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

static DEFAULT: Mutex<Option<AppId>> = Mutex::new(None);

/// Sets the id the components use when they name none (`AppId=""`): `settings!` sets it to the package name, an
/// app can call it first thing in `main`.
pub fn set_default_app_id(id: AppId) {
    *DEFAULT.lock().unwrap_or_else(PoisonError::into_inner) = Some(id);
}

/// [`set_default_app_id`] unless one was set already (the first registered settings class).
pub(crate) fn set_default_app_id_if_unset(id: AppId) {
    let mut d = DEFAULT.lock().unwrap_or_else(PoisonError::into_inner);
    if d.is_none() {
        *d = Some(id);
    }
}

/// The app id of this process: the one set by [`set_default_app_id`], else the executable's stem (`notes.exe` →
/// `notes`), else `kubuno-app`.
pub fn default_app_id() -> AppId {
    if let Some(id) = DEFAULT.lock().unwrap_or_else(PoisonError::into_inner).clone() {
        return id;
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().and_then(|s| s.to_str()).and_then(AppId::from_name))
        .unwrap_or_else(|| AppId("kubuno-app".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_validated() {
        assert!(AppId::new("kubuno-notes").is_ok());
        assert!(AppId::new("a.b_c-1").is_ok());
        for bad in ["", "Notes", "-x", ".x", "x.", "a/b", "a\\b", "..", "con", "nul.txt", "a b", &"x".repeat(41)] {
            assert!(AppId::new(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn names_are_turned_into_ids() {
        assert_eq!(AppId::from_name("Storage Desktop").map(|a| a.0), Some("storage-desktop".into()));
        assert_eq!(AppId::from_name("printing_desktop").map(|a| a.0), Some("printing_desktop".into()));
        assert_eq!(AppId::from_name("--x").map(|a| a.0), Some("x".into()));
        assert!(AppId::from_name("///").is_none());
    }
}
