//! Local configuration and credential storage.
//!
//! The client can be connected to several Kubuno instances at once. Each
//! instance gets its own sub-directory under the OS config dir:
//!
//! ```text
//! <config_dir>/kubuno-desktop/
//!   instances/
//!     <instance-id>/
//!       config.json   server URL + sync folder
//!       creds.json    refresh + access token (rotates)
//!       state.db      sync cursor, folder tree, file index, outbox
//! ```
//!
//! The refresh token is the long-lived secret; for now it is stored in a 0600
//! file. Moving it to the OS keyring is a follow-up.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Root config directory (`%APPDATA%/kubuno-desktop` on Windows).
pub fn config_dir() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .context("dossier de configuration introuvable")?
        .join("kubuno-desktop");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Directory holding one sub-directory per connected instance.
pub fn instances_dir() -> Result<PathBuf> {
    let dir = config_dir()?.join("instances");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Per-instance directory (created on demand).
pub fn instance_dir(id: &str) -> Result<PathBuf> {
    let dir = instances_dir()?.join(id);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Path to an instance's local sync-state database.
pub fn db_path(id: &str) -> Result<PathBuf> {
    Ok(instance_dir(id)?.join("state.db"))
}

/// Derive a unique, filesystem-safe instance id from the server URL: a hostname
/// slug plus a short random suffix so the same server can be added twice.
pub fn new_instance_id(server: &str) -> String {
    let host = server
        .split("://")
        .last()
        .unwrap_or(server)
        .split('/')
        .next()
        .unwrap_or(server)
        .split(':')
        .next()
        .unwrap_or(server);
    let slug: String = host
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "kubuno" } else { slug };
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    format!("{slug}-{}", &suffix[..8])
}

/// Persistent per-instance settings written by `login`.
#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    /// Stable instance identifier (also the sub-directory name).
    pub id:         String,
    pub server_url: String,
    pub sync_root:  PathBuf,
    /// Optional human label shown in the account switcher (defaults to the host).
    #[serde(default)]
    pub label:      Option<String>,
}

impl Config {
    fn path(id: &str) -> Result<PathBuf> {
        Ok(instance_dir(id)?.join("config.json"))
    }

    /// Load one instance's config by id.
    pub fn load(id: &str) -> Result<Self> {
        let p = Self::path(id)?;
        let s = std::fs::read_to_string(&p).with_context(|| {
            format!("configuration absente pour l'instance « {id} » ({}).", p.display())
        })?;
        Ok(serde_json::from_str(&s)?)
    }

    pub fn save(&self) -> Result<()> {
        std::fs::write(Self::path(&self.id)?, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Every configured instance (each `instances/<id>/config.json` that parses),
    /// sorted by id for a stable display order.
    pub fn list() -> Result<Vec<Config>> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(instances_dir()?) {
            for entry in rd.flatten() {
                if !entry.path().is_dir() {
                    continue;
                }
                let cfg = entry.path().join("config.json");
                if let Ok(s) = std::fs::read_to_string(&cfg) {
                    if let Ok(c) = serde_json::from_str::<Config>(&s) {
                        out.push(c);
                    }
                }
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    /// Remove an instance: its config, credentials and local sync state. The
    /// downloaded files in `sync_root` are left untouched.
    pub fn remove(id: &str) -> Result<()> {
        let dir = instance_dir(id)?;
        std::fs::remove_dir_all(&dir)
            .with_context(|| format!("suppression de l'instance « {id} »"))?;
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
    fn path(id: &str) -> Result<PathBuf> {
        Ok(instance_dir(id)?.join("creds.json"))
    }

    pub fn load(id: &str) -> Result<Self> {
        let s = std::fs::read_to_string(Self::path(id)?)
            .with_context(|| format!("identifiants absents pour l'instance « {id} »."))?;
        Ok(serde_json::from_str(&s)?)
    }

    pub fn save(&self, id: &str) -> Result<()> {
        let p = Self::path(id)?;
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

/// Migrate a legacy single-instance layout (config.json / creds.json / state.db
/// directly under the config dir) into `instances/<id>/`. No-op once migrated.
pub fn migrate_legacy() -> Result<()> {
    let root = config_dir()?;
    let legacy_cfg = root.join("config.json");
    if !legacy_cfg.exists() {
        return Ok(());
    }
    let s = std::fs::read_to_string(&legacy_cfg)?;
    let mut v: serde_json::Value = serde_json::from_str(&s)?;
    let server = v
        .get("server_url")
        .and_then(|x| x.as_str())
        .unwrap_or("kubuno")
        .to_string();
    let id = new_instance_id(&server);
    let dir = instance_dir(&id)?;

    // Stamp the id into the migrated config.
    v["id"] = serde_json::Value::String(id.clone());
    std::fs::write(dir.join("config.json"), serde_json::to_string_pretty(&v)?)?;
    for f in ["creds.json", "state.db"] {
        let src = root.join(f);
        if src.exists() {
            let _ = std::fs::rename(&src, dir.join(f));
        }
    }
    let _ = std::fs::remove_file(&legacy_cfg);
    Ok(())
}
