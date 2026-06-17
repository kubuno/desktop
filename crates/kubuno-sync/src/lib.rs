//! Kubuno desktop sync engine.
//!
//! Exposes the sync modules plus a small high-level API so both the CLI
//! (`kubuno-sync`) and the Tauri desktop app can drive the same engine.

pub mod api;
pub mod config;
pub mod daemon;
pub mod engine;
pub mod push;
pub mod store;
pub mod ws;

use anyhow::Result;

pub use config::{db_path, Config, Creds};

/// Combined result of one push+pull cycle.
#[derive(Default, Clone)]
pub struct Summary {
    pub uploaded:     u32,
    pub modified:     u32,
    pub deleted_up:   u32,
    pub conflicts:    u32,
    pub pending:      u32,
    pub downloaded:   u32,
    pub folders:      u32,
    pub up_to_date:   u32,
    pub deleted_down: u32,
    pub cursor:       i64,
}

/// Authenticate, store credentials and persist the config (sync folder).
pub fn login(server: &str, login: &str, password: &str, folder: &str) -> Result<()> {
    let creds = api::Api::login(server, login, password)?;
    creds.save()?;
    let cfg = Config {
        server_url: server.to_string(),
        sync_root: folder.into(),
    };
    std::fs::create_dir_all(&cfg.sync_root)?;
    cfg.save()?;
    Ok(())
}

/// Run one push+pull cycle and return a summary.
pub fn sync_once() -> Result<Summary> {
    let cfg = Config::load()?;
    let mut api = api::Api::new(cfg.server_url.clone(), Creds::load()?);
    let store = store::Store::open(&db_path()?)?;

    let p = push::push(&mut api, &store, &cfg)?;
    let s = engine::sync(&mut api, &store, &cfg)?;

    Ok(Summary {
        uploaded:     p.uploaded,
        modified:     p.modified,
        deleted_up:   p.deleted,
        conflicts:    p.conflicts,
        pending:      p.pending,
        downloaded:   s.downloaded,
        folders:      s.folders,
        up_to_date:   s.up_to_date,
        deleted_down: s.deleted,
        cursor:       store.cursor()?,
    })
}

/// True if a session and config exist (i.e. the user has logged in).
pub fn is_logged_in() -> bool {
    Creds::load().is_ok() && Config::load().is_ok()
}

/// The configured server URL and sync folder, if logged in.
pub fn current_config() -> Option<Config> {
    Config::load().ok()
}
