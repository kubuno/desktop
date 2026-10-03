//! Per-user locations of the desktop (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §11), aligned with the conventions
//! of `kubuno-paths` (`docs/MULTI-OS-AUDIT.md` §3.1, the server-side crate of the core): every location can be
//! overridden by an environment variable, Windows known folders come from `SHGetKnownFolderPath` (not from
//! environment variables), and a location is **never inferred from `is_dir()`**.
//!
//! | Function | Windows | macOS | Linux / BSD | Override |
//! |---|---|---|---|---|
//! | [`user_config_dir`] (settings, `shell.json`) | `%APPDATA%\Kubuno` (roaming) | `~/Library/Application Support/Kubuno` | `$XDG_CONFIG_HOME/kubuno` | `KUBUNO_USER_CONFIG_DIR` |
//! | [`user_data_dir`] (accounts, databases, outbox) | `%LOCALAPPDATA%\Kubuno` (**local**: a roaming profile must never copy an open SQLite database) | `~/Library/Application Support/Kubuno` | `$XDG_DATA_HOME/kubuno` | `KUBUNO_USER_DATA_DIR` |
//! | [`user_cache_dir`] (re-downloadable blobs) | `%LOCALAPPDATA%\Kubuno\Cache` | `~/Library/Caches/Kubuno` | `$XDG_CACHE_HOME/kubuno` | `KUBUNO_USER_CACHE_DIR` |
//! | [`user_runtime_dir`] (broker socket) | not used (named pipe) | `~/Library/Application Support/Kubuno/run` | `$XDG_RUNTIME_DIR/kubuno` | `KUBUNO_USER_RUNTIME_DIR` |
//!
//! Inside the data directory: `accounts/<account_key>/<app>.db` (+ `-wal`, `-shm`, `.sync.lock`),
//! `accounts/<account_key>/blobs/<app>/`, `accounts/<account_key>/account.json` (written by `kubuno-desktop-account`).
//!
//! When `kubuno-paths` gains its `client` module (feature `client`, no server dependencies) this module becomes a
//! thin re-export of it; the rules and variable names are the same so nothing moves on disk.

use std::io;
use std::path::{Path, PathBuf};

use kubuno_desktop_account::AccountKey;

// The per-user directories live in `kubuno-desktop-account` (the broker and the account store need them too, and a
// sandboxed profile, `KUBUNO_SANDBOX_DIR`, must move all of them at once); re-exported here unchanged.
pub use kubuno_desktop_account::paths::{
    legacy_config_dir, sandbox_dir, system_integration_allowed, user_cache_dir, user_config_dir, user_data_dir,
    user_runtime_dir,
};

/// Whether `name` can be used as an app database name (`drive`, `chat`, `notes`...): becomes a file name.
pub fn is_valid_app_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 40 && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// `<data>/accounts/<account_key>`.
pub fn account_dir(data_dir: &Path, account: &AccountKey) -> PathBuf {
    data_dir.join("accounts").join(account.as_str())
}

/// `<data>/accounts/<account_key>/<app>.db`.
pub fn app_db_path(data_dir: &Path, account: &AccountKey, app: &str) -> io::Result<PathBuf> {
    if !is_valid_app_name(app) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("invalid app name '{app}'")));
    }
    Ok(account_dir(data_dir, account).join(format!("{app}.db")))
}

/// `<data>/accounts/<account_key>/blobs/<app>`.
pub fn app_blobs_dir(data_dir: &Path, account: &AccountKey, app: &str) -> io::Result<PathBuf> {
    if !is_valid_app_name(app) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("invalid app name '{app}'")));
    }
    Ok(account_dir(data_dir, account).join("blobs").join(app))
}

/// Creates `dir` (and parents), owner-only on Unix.
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_paths_are_validated() {
        let key = AccountKey::parse("0123456789abcdef").expect("key");
        let root = Path::new("/data");
        assert_eq!(app_db_path(root, &key, "drive").expect("ok"), root.join("accounts").join("0123456789abcdef").join("drive.db"));
        assert!(app_db_path(root, &key, "../x").is_err());
        assert!(app_db_path(root, &key, "Drive").is_err());
        assert!(app_blobs_dir(root, &key, "chat").expect("ok").ends_with("blobs/chat"));
    }
}
