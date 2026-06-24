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

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::Result;

pub use config::{db_path, migrate_legacy, Config, Creds};

/// Per-instance lock serializing sync cycles and folder moves, so a folder move
/// never runs while a push/pull is touching the same folder (which would make
/// the daemon push spurious deletions for files being relocated).
pub(crate) fn sync_lock(id: &str) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    let map = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut m = map.lock().unwrap_or_else(|p| p.into_inner());
    m.entry(id.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
}

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

/// Authenticate against a server and register it as a new instance. Returns the
/// freshly generated instance id. Multiple instances (even to the same server)
/// can coexist, each with its own credentials, sync folder and local state.
pub fn login(server: &str, login: &str, password: &str, folder: &str) -> Result<String> {
    let creds = api::Api::login(server, login, password)?;
    let id = config::new_instance_id(server);
    creds.save(&id)?;
    let cfg = Config {
        id:         id.clone(),
        server_url: server.to_string(),
        sync_root:  folder.into(),
        label:      None,
    };
    std::fs::create_dir_all(&cfg.sync_root)?;
    cfg.save()?;
    Ok(id)
}

/// Run one push+pull cycle for a single instance and return a summary.
pub fn sync_once(id: &str) -> Result<Summary> {
    let lock = sync_lock(id);
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone(), Creds::load(id)?);
    let store = store::Store::open(&db_path(id)?)?;

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

/// True if at least one instance is configured.
pub fn is_logged_in() -> bool {
    !list_instances().is_empty()
}

/// Every configured instance.
pub fn list_instances() -> Vec<Config> {
    Config::list().unwrap_or_default()
}

/// The configured outbound proxy URL, if any.
pub fn get_proxy() -> Option<String> {
    config::proxy_url()
}

/// Set (or clear, with `None`/empty) the outbound proxy URL.
pub fn set_proxy(url: Option<String>) -> Result<()> {
    config::set_proxy(url.as_deref())
}

/// The config of a single instance, if it exists.
pub fn current_config(id: &str) -> Option<Config> {
    Config::load(id).ok()
}

/// Whether the instance's server is currently reachable (quick `/healthz` ping).
pub fn is_online(id: &str) -> bool {
    match current_config(id) {
        Some(cfg) => api::ping(&cfg.server_url),
        None => false,
    }
}

/// Disconnect an instance: drop its credentials and local sync state (the
/// already-downloaded files on disk are kept).
pub fn remove_instance(id: &str) -> Result<()> {
    Config::remove(id)
}

/// Move an instance's sync folder to `new_path`: relocate the files on disk,
/// rebase the stored absolute paths, then update the config. The running daemon
/// reloads its config each cycle, so it picks up the new location without a
/// restart (and never re-downloads into the old folder).
pub fn move_instance_folder(id: &str, new_path: &str) -> Result<()> {
    // Hold the sync lock for the whole move so the daemon can't run a push/pull
    // against the half-moved folder (which would delete files on the server).
    let lock = sync_lock(id);
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    let mut cfg = Config::load(id)?;
    let old = cfg.sync_root.clone();
    let new = std::path::PathBuf::from(new_path);
    if old == new {
        return Ok(());
    }
    if old.exists() {
        move_into(&old, &new)?;
        let _ = std::fs::remove_dir(&old);
    } else {
        std::fs::create_dir_all(&new)?;
    }
    // Rewrite the stored absolute paths so deletions/conflict handling keep
    // pointing at the real files after the move.
    let old_root = old.to_string_lossy();
    let new_root = new.to_string_lossy();
    let store = store::Store::open(&db_path(id)?)?;
    store.rebase_paths(
        old_root.trim_end_matches(['/', '\\']),
        new_root.trim_end_matches(['/', '\\']),
    )?;
    cfg.sync_root = new;
    cfg.save()?;
    Ok(())
}

/// Recursively move the contents of `from` into `to` (creating/merging `to`),
/// falling back to copy+delete across volumes where `rename` fails.
fn move_into(from: &std::path::Path, to: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            move_into(&src, &dst)?;
            let _ = std::fs::remove_dir(&src);
        } else if std::fs::rename(&src, &dst).is_err() {
            std::fs::copy(&src, &dst)?;
            std::fs::remove_file(&src)?;
        }
    }
    Ok(())
}

/// Fetch an instance's authenticated user profile (`GET /api/v1/me`).
/// Requires a live session; returns an error if offline. Retried once so a
/// transient server hiccup (or a token just rotated by the background sync)
/// doesn't blank out the account display.
pub fn current_user(id: &str) -> Result<api::User> {
    let cfg = Config::load(id)?;
    let mut last: Option<anyhow::Error> = None;
    for attempt in 0..2 {
        // Reload creds each try: the background sync may have saved a fresh token.
        let mut api = api::Api::new(id.to_string(), cfg.server_url.clone(), Creds::load(id)?);
        match api.me() {
            Ok(u) => return Ok(u),
            Err(e) => {
                last = Some(e);
                if attempt == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("profil indisponible")))
}
