//! Per-user and machine locations of the desktop (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §11,
//! `docs/STORAGE-COMPONENTS.md` §3), aligned with the conventions of `kubuno-paths` (`docs/MULTI-OS-AUDIT.md` §3.1):
//! every location can be overridden by an environment variable, Windows known folders come from
//! `SHGetKnownFolderPath` (not from environment variables), and a location is **never inferred from `is_dir()`**.
//! `kubuno-desktop-account::paths` (and through it `kubuno-desktop-sync-engine::paths`) re-exports this module: it moved here from
//! `kubuno-desktop-account` so that an app keeps its settings without linking the account and network stack.
//!
//! | Function | Windows | macOS | Linux / BSD | Override |
//! |---|---|---|---|---|
//! | [`user_config_dir`] (roaming settings) | `%APPDATA%\Kubuno` (roaming) | `~/Library/Application Support/Kubuno` | `$XDG_CONFIG_HOME/kubuno` | `KUBUNO_USER_CONFIG_DIR` |
//! | [`user_data_dir`] (accounts, databases, local settings) | `%LOCALAPPDATA%\Kubuno` (local) | `~/Library/Application Support/Kubuno` | `$XDG_DATA_HOME/kubuno` | `KUBUNO_USER_DATA_DIR` |
//! | [`user_cache_dir`] | `%LOCALAPPDATA%\Kubuno\Cache` | `~/Library/Caches/Kubuno` | `$XDG_CACHE_HOME/kubuno` | `KUBUNO_USER_CACHE_DIR` |
//! | [`user_runtime_dir`] (broker socket) | not used (named pipe) | `~/Library/Application Support/Kubuno/run` | `$XDG_RUNTIME_DIR/kubuno` | `KUBUNO_USER_RUNTIME_DIR` |
//! | [`machine_config_dir`] (what an administrator deploys for every user; read-only for apps) | `%ProgramData%\Kubuno\Desktop` | `/Library/Application Support/Kubuno/Desktop` | first of `$XDG_CONFIG_DIRS` (`/etc/xdg`) + `/kubuno` | `KUBUNO_MACHINE_CONFIG_DIR` |
//! | [`legacy_config_dir`] (file-sync instances, `shell.json`) | `%APPDATA%\kubuno-desktop` | `~/Library/Application Support/kubuno-desktop` | `$XDG_CONFIG_HOME/kubuno-desktop` | `KUBUNO_LEGACY_CONFIG_DIR` |
//!
//! The machine directory of the desktop apps is `Desktop` below the server core's own `%ProgramData%\Kubuno`
//! (`kubuno-paths`): a desktop app on a machine that also runs a Kubuno server never mixes its files with the
//! server's `config.toml`.
//!
//! # Sandboxed profile
//!
//! `KUBUNO_SANDBOX_DIR=<absolute dir>` moves **everything** of a run under that directory (it wins over the
//! variables above): `config/`, `data/`, `cache/`, `run/`, `machine/` and `legacy/kubuno-desktop/`. A sandboxed run
//! also gets its own broker endpoint and its own secret namespace ([`sandbox_tag`]), its Registry accesses go below
//! `HKCU\Software\Kubuno\Sandbox\<tag>` (`crate::registry`), and the programs skip every system registration (the
//! `Run` key, Explorer, Cloud Files, the `kubuno://` protocol: [`system_integration_allowed`]). It lets the shell and
//! the apps run for real (sign-in, broker, file sync, settings) against a fake server without reading or touching
//! the user's real profile.

use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Directory name under the platform folders.
#[cfg(any(windows, target_os = "macos"))]
const APP_DIR: &str = "Kubuno";
#[cfg(not(any(windows, target_os = "macos")))]
const APP_DIR: &str = "kubuno";

/// The sandbox variable (see the module doc).
pub const SANDBOX_ENV: &str = "KUBUNO_SANDBOX_DIR";

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute())
}

fn not_found(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("cannot determine the {what} directory"))
}

/// The sandbox directory of this run, when `KUBUNO_SANDBOX_DIR` names an absolute directory.
pub fn sandbox_dir() -> Option<PathBuf> {
    env_dir(SANDBOX_ENV)
}

/// A short stable tag of the sandbox (8 hex characters of a hash of its directory), used to keep its broker
/// endpoint and its secrets apart from the real ones. `None` outside a sandbox.
pub fn sandbox_tag() -> Option<String> {
    sandbox_dir().map(|d| tag_of(&d))
}

fn tag_of(dir: &Path) -> String {
    let text = dir.to_string_lossy();
    let text = if cfg!(any(windows, target_os = "macos")) { text.to_lowercase() } else { text.into_owned() };
    hex::encode(&Sha256::digest(text.trim_end_matches(['/', '\\']).as_bytes())[..4])
}

/// Whether this run may register anything with the system (the `Run` key, Explorer, Cloud Files, URL protocols):
/// never in a sandbox.
pub fn system_integration_allowed() -> bool {
    sandbox_dir().is_none()
}

#[cfg(windows)]
mod known {
    use std::path::PathBuf;

    use windows_sys::core::GUID;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_LocalAppData, FOLDERID_ProgramData, FOLDERID_RoamingAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

    fn known_folder(id: &GUID) -> Option<PathBuf> {
        let mut out: windows_sys::core::PWSTR = std::ptr::null_mut();
        // SAFETY: `id` is a valid KNOWNFOLDERID; `out` receives a CoTaskMemAlloc'ed string freed below (also on
        // failure, as documented).
        let hr = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT as _, std::ptr::null_mut(), &mut out) };
        let result = if hr >= 0 && !out.is_null() {
            // SAFETY: on success `out` is a NUL-terminated UTF-16 string.
            let s = unsafe {
                let mut n = 0usize;
                while *out.add(n) != 0 {
                    n += 1;
                }
                String::from_utf16_lossy(std::slice::from_raw_parts(out, n))
            };
            Some(PathBuf::from(s))
        } else {
            None
        };
        // SAFETY: freeing the buffer the API allocated (null is accepted).
        unsafe { CoTaskMemFree(out.cast()) };
        result
    }

    pub(super) fn roaming_app_data() -> Option<PathBuf> {
        known_folder(&FOLDERID_RoamingAppData)
    }

    pub(super) fn local_app_data() -> Option<PathBuf> {
        known_folder(&FOLDERID_LocalAppData)
    }

    pub(super) fn program_data() -> Option<PathBuf> {
        known_folder(&FOLDERID_ProgramData)
    }
}

#[cfg(unix)]
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn xdg(var: &str, default_under_home: &str) -> Option<PathBuf> {
    env_dir(var).or_else(|| home().map(|h| h.join(default_under_home)))
}

/// Settings (roaming on Windows).
pub fn user_config_dir() -> io::Result<PathBuf> {
    if let Some(s) = sandbox_dir() {
        return Ok(s.join("config"));
    }
    if let Some(p) = env_dir("KUBUNO_USER_CONFIG_DIR") {
        return Ok(p);
    }
    #[cfg(windows)]
    let base = known::roaming_app_data();
    #[cfg(target_os = "macos")]
    let base = home().map(|h| h.join("Library").join("Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = xdg("XDG_CONFIG_HOME", ".config");
    base.map(|b| b.join(APP_DIR)).ok_or_else(|| not_found("configuration"))
}

/// Accounts, databases and outboxes (local, never roaming).
pub fn user_data_dir() -> io::Result<PathBuf> {
    if let Some(s) = sandbox_dir() {
        return Ok(s.join("data"));
    }
    if let Some(p) = env_dir("KUBUNO_USER_DATA_DIR") {
        return Ok(p);
    }
    #[cfg(windows)]
    let base = known::local_app_data();
    #[cfg(target_os = "macos")]
    let base = home().map(|h| h.join("Library").join("Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = xdg("XDG_DATA_HOME", ".local/share");
    base.map(|b| b.join(APP_DIR)).ok_or_else(|| not_found("data"))
}

/// Re-downloadable content (thumbnails, document snapshots, attachments).
pub fn user_cache_dir() -> io::Result<PathBuf> {
    if let Some(s) = sandbox_dir() {
        return Ok(s.join("cache"));
    }
    if let Some(p) = env_dir("KUBUNO_USER_CACHE_DIR") {
        return Ok(p);
    }
    #[cfg(windows)]
    let base = known::local_app_data().map(|b| b.join(APP_DIR).join("Cache"));
    #[cfg(target_os = "macos")]
    let base = home().map(|h| h.join("Library").join("Caches").join(APP_DIR));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = xdg("XDG_CACHE_HOME", ".cache").map(|b| b.join(APP_DIR));
    base.ok_or_else(|| not_found("cache"))
}

/// The broker's socket directory. On Linux without `$XDG_RUNTIME_DIR` (no systemd-logind session) it falls back to
/// `<cache>/run`, created `0700` by the broker. Unused on Windows (named pipe).
pub fn user_runtime_dir() -> io::Result<PathBuf> {
    if let Some(s) = sandbox_dir() {
        return Ok(s.join("run"));
    }
    if let Some(p) = env_dir("KUBUNO_USER_RUNTIME_DIR") {
        return Ok(p);
    }
    #[cfg(windows)]
    let base = user_data_dir().ok().map(|d| d.join("run"));
    #[cfg(target_os = "macos")]
    let base = home().map(|h| h.join("Library").join("Application Support").join(APP_DIR).join("run"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = env_dir("XDG_RUNTIME_DIR").map(|r| r.join(APP_DIR)).or_else(|| user_cache_dir().ok().map(|c| c.join("run")));
    base.ok_or_else(|| not_found("runtime"))
}

/// What an administrator deploys for every user of the machine: application-scoped settings and machine-wide
/// defaults of user settings. Apps read it; they never write it unless explicitly allowed (an installer, an admin
/// tool). See the module doc for the locations.
pub fn machine_config_dir() -> io::Result<PathBuf> {
    if let Some(s) = sandbox_dir() {
        return Ok(s.join("machine"));
    }
    if let Some(p) = env_dir("KUBUNO_MACHINE_CONFIG_DIR") {
        return Ok(p);
    }
    #[cfg(windows)]
    let base = known::program_data().map(|b| b.join(APP_DIR).join("Desktop"));
    #[cfg(target_os = "macos")]
    let base = Some(PathBuf::from("/Library/Application Support").join(APP_DIR).join("Desktop"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = Some(
        std::env::var("XDG_CONFIG_DIRS")
            .ok()
            .and_then(|v| v.split(':').map(PathBuf::from).find(|p| p.is_absolute()))
            .unwrap_or_else(|| PathBuf::from("/etc/xdg"))
            .join(APP_DIR),
    );
    base.ok_or_else(|| not_found("machine configuration"))
}

/// The configuration root of the file-sync client (`dirs::config_dir()/kubuno-desktop`): its instances
/// (`instances/<id>/{config.json, state.db}`), `settings.json` and the shell's `shell.json`. The legacy plaintext
/// `creds.json` files found there are moved into the OS store once (`crate::migrate`).
pub fn legacy_config_dir() -> io::Result<PathBuf> {
    if let Some(s) = sandbox_dir() {
        return Ok(s.join("legacy").join("kubuno-desktop"));
    }
    if let Some(p) = env_dir("KUBUNO_LEGACY_CONFIG_DIR") {
        return Ok(p);
    }
    #[cfg(windows)]
    let base = known::roaming_app_data();
    #[cfg(target_os = "macos")]
    let base = home().map(|h| h.join("Library").join("Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = xdg("XDG_CONFIG_HOME", ".config");
    base.map(|b| b.join("kubuno-desktop")).ok_or_else(|| not_found("legacy configuration"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_defaults_exist_and_differ_where_they_must() {
        if sandbox_dir().is_some() {
            return; // The defaults are not observable inside a sandboxed test run.
        }
        let config = user_config_dir().expect("config");
        let data = user_data_dir().expect("data");
        let cache = user_cache_dir().expect("cache");
        assert!(config.is_absolute() && data.is_absolute() && cache.is_absolute());
        #[cfg(windows)]
        {
            // Databases must not live in the roaming profile.
            assert_ne!(config, data);
            assert!(data.to_string_lossy().contains("Local"), "{data:?}");
            assert!(config.ends_with("Kubuno") && data.ends_with("Kubuno"));
        }
        assert!(legacy_config_dir().expect("legacy").ends_with("kubuno-desktop"));
        let machine = machine_config_dir().expect("machine");
        assert!(machine.is_absolute() && machine != config && machine != data, "{machine:?}");
    }

    #[test]
    fn sandbox_tags_are_stable_and_distinct() {
        let a = tag_of(Path::new("/tmp/sbx-a"));
        assert_eq!(a.len(), 8);
        assert_eq!(a, tag_of(Path::new("/tmp/sbx-a/")));
        assert_ne!(a, tag_of(Path::new("/tmp/sbx-b")));
    }
}
