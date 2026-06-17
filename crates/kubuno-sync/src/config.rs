//! Local configuration and credential storage.
//!
//! Lives under the OS config dir (`~/.config/kubuno-desktop` on Linux). The
//! refresh token is the long-lived secret; for now it is stored in a 0600 file.
//! Moving it to the OS keyring (Secret Service / Keychain / Credential Manager)
//! is a follow-up.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub fn config_dir() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .context("dossier de configuration introuvable")?
        .join("kubuno-desktop");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn db_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("state.db"))
}

/// Persistent settings written by `login`.
#[derive(Serialize, Deserialize)]
pub struct Config {
    pub server_url: String,
    pub sync_root:  PathBuf,
}

impl Config {
    fn path() -> Result<PathBuf> {
        Ok(config_dir()?.join("config.json"))
    }

    pub fn load() -> Result<Self> {
        let p = Self::path()?;
        let s = std::fs::read_to_string(&p)
            .with_context(|| format!("configuration absente ({}). Lance `kubuno-sync login` d'abord.", p.display()))?;
        Ok(serde_json::from_str(&s)?)
    }

    pub fn save(&self) -> Result<()> {
        std::fs::write(Self::path()?, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

/// Stored session tokens. The refresh token rotates on every refresh.
#[derive(Serialize, Deserialize, Default)]
pub struct Creds {
    pub refresh_token: String,
    pub access_token:  String,
}

impl Creds {
    fn path() -> Result<PathBuf> {
        Ok(config_dir()?.join("creds.json"))
    }

    pub fn load() -> Result<Self> {
        let s = std::fs::read_to_string(Self::path()?)
            .context("identifiants absents. Lance `kubuno-sync login` d'abord.")?;
        Ok(serde_json::from_str(&s)?)
    }

    pub fn save(&self) -> Result<()> {
        let p = Self::path()?;
        std::fs::write(&p, serde_json::to_string_pretty(self)?)?;
        // The refresh token has password-equivalent value: keep it owner-only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
}
