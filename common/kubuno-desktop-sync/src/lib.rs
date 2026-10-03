//! Kubuno desktop sync engine.
//!
//! Exposes the sync modules plus a small high-level API so both the CLI
//! (`kubuno-sync`) and the desktop shell can drive the same engine.

pub mod api;
pub mod config;
pub mod daemon;
pub mod engine;
pub mod push;
pub mod store;
pub mod tokens;
pub mod ws;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::Result;

pub use api::{
    AdminGroup, AdminModule, AdminSetting, AdminUser, AdminUsers, Audience, Label, OrgUnit,
    Privileges, QuotaStates, StorageCategory, StorageOverview, StorageVolume, UnitUsage,
};
pub use config::{db_path, migrate_legacy, Config};
pub use tokens::AccountRef;

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

/// Registers a sync folder of `server` as an instance and returns its id. The
/// sign-in itself is the shell's (`kubuno_desktop_account`: the tokens go to the OS
/// credential store, never here); the caller links the returned instance to
/// the signed-in account (`AccountStore::link_instance`). Multiple instances
/// (even to the same server) can coexist, each with its own folder and state.
///
/// Signing in again to the SAME server + sync folder (e.g. after a revocation)
/// reuses the existing instance instead of minting a duplicate — which would
/// orphan its local stores (offline documents, drive data, sync cursors) and
/// leave a zombie entry pointing at a dead session.
pub fn register_instance(server: &str, folder: &str) -> Result<String> {
    let norm = |s: &str| s.trim_end_matches('/').to_ascii_lowercase();
    if let Ok(existing) = Config::list() {
        if let Some(cfg) = existing.into_iter().find(|c| {
            norm(&c.server_url) == norm(server) && c.sync_root == std::path::Path::new(folder)
        }) {
            return Ok(cfg.id);
        }
    }
    let id = config::new_instance_id(server);
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
    // Forced offline → no network; report an up-to-date (no-op) cycle.
    if config::is_offline() {
        return Ok(Summary::default());
    }
    let lock = sync_lock(id);
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
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

/// Whether the user has forced offline mode (no core communication).
pub fn is_offline() -> bool {
    config::is_offline()
}

/// Turn forced offline mode on/off.
pub fn set_offline(offline: bool) -> Result<()> {
    config::set_offline(offline)
}

/// The config of a single instance, if it exists.
pub fn current_config(id: &str) -> Option<Config> {
    Config::load(id).ok()
}

/// Connection state of an instance: "online" (reachable + session valid),
/// "expired" (reachable but the session/refresh token is no longer accepted —
/// the user must reconnect) or "offline" (server unreachable).
pub fn connection_state(id: &str) -> &'static str {
    // User-forced offline mode short-circuits everything.
    if config::is_offline() {
        return "offline";
    }
    let Some(cfg) = current_config(id) else { return "offline" };
    if !api::ping(&cfg.server_url) {
        return "offline";
    }
    // Reachable — is the session still valid? `current_user` refreshes on a 401.
    // Only a GENUINE rejection (the refresh token itself was refused) means the
    // session is over; a transient failure (rate-limit, 5xx, network blip) leaves
    // the refresh token valid, so we stay "online" and retry rather than alarming
    // the user with "session expired".
    match current_user(id) {
        Ok(_) => "online",
        Err(e) => match e.downcast_ref::<api::AuthFailure>() {
            Some(api::AuthFailure::Genuine) => "expired",
            _ => "online",
        },
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

/// A valid access token of an instance's account, borrowed from the process's
/// token provider (a WebSocket URL carries one). Never a refresh token.
pub fn access_token(id: &str) -> Result<String> {
    let cfg = Config::load(id)?;
    api::Api::new(id.to_string(), cfg.server_url).access_token()
}

// ── Account-level calls (apps without a file-sync instance, such as chat) ────

/// The account the apps show: the shell's current account, else the first
/// active one (`None`: nobody is signed in).
pub fn current_account() -> Result<Option<AccountRef>> {
    Ok(tokens::current_account()?)
}

/// A client of `account`'s server with `account`'s tokens.
pub fn account_api(account: &AccountRef) -> api::Api {
    api::Api::for_account(account.key.clone(), account.server_url.clone())
}

/// A valid access token of `account` (a WebSocket URL carries one).
pub fn account_access_token(account: &AccountRef) -> Result<String> {
    account_api(account).access_token()
}

/// Authenticated GET of a core route as `account`, returning raw JSON.
pub fn account_get_json(account: &AccountRef, path: &str) -> Result<serde_json::Value> {
    account_api(account).get_json(path)
}

/// Authenticated POST of a JSON body as `account`, returning the JSON reply.
pub fn account_post_json(account: &AccountRef, path: &str, body: serde_json::Value) -> Result<serde_json::Value> {
    account_api(account).post_json(path, body)
}

/// The changes of an instance that are queued but not yet sent (its outbox).
/// Asked before a sign-out: « N modifications n'ont pas été envoyées ».
pub fn unsent_changes(id: &str) -> Result<Vec<store::OutboxOp>> {
    let store = store::Store::open(&db_path(id)?)?;
    store.outbox()
}

/// Copies the local files of the unsent changes of an instance into `dest`
/// (created), keeping their path relative to the sync folder, and returns how
/// many files were copied. Deletions have nothing to copy. Used by the
/// « Exporter » choice of a sign-out with unsent changes.
pub fn export_unsent(id: &str, dest: &std::path::Path) -> Result<u32> {
    let cfg = Config::load(id)?;
    let mut copied = 0u32;
    for op in unsent_changes(id)? {
        let Some(local) = op.local_path.as_deref().map(std::path::Path::new) else { continue };
        if !local.is_file() {
            continue;
        }
        let rel = local.strip_prefix(&cfg.sync_root).ok().map(std::path::Path::to_path_buf).unwrap_or_else(|| local.file_name().map(std::path::PathBuf::from).unwrap_or_default());
        if rel.as_os_str().is_empty() {
            continue;
        }
        let target = dest.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(local, &target)?;
        copied += 1;
    }
    Ok(copied)
}

/// The upstream core URL of an instance (proxy target).
pub fn server_url(id: &str) -> Option<String> {
    Config::load(id).ok().map(|c| c.server_url)
}

/// Download a file's content by server id, for the given instance. Used by the
/// on-demand hydration callback when a virtual (online-only) file is opened.
pub fn download_for(id: &str, file_id: &str) -> Result<Vec<u8>> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url);
    api.download(file_id)
}

/// The waffle's favourites, as the web stores them: an ordered list of app ids
/// under `preferences.waffle_favorites`.
pub fn waffle_favorites(user: &api::User) -> Vec<String> {
    user.preferences
        .get("waffle_favorites")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Writes the favourites back, so the web sees the same list.
pub fn set_waffle_favorites(id: &str, favorites: &[String]) -> Result<()> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url);
    api.patch_preferences(serde_json::json!({ "waffle_favorites": favorites }))
}

/// Fetch a server-relative path as raw bytes, authenticated — the profile
/// photo. Goes through `Api`, so an expired token is refreshed like anywhere
/// else.
pub fn fetch_bytes(id: &str, path: &str) -> Result<Vec<u8>> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url);
    api.get_bytes(path)
}

/// Fetch an instance's activated modules (`GET /api/v1/modules`).
///
/// Goes through `Api` rather than the stored access token so an expired token is
/// refreshed (and rotated) instead of yielding an empty launcher.
/// The account's cross-module labels. Like every read here, it goes through
/// `Api` so an expired access token is refreshed and rotated rather than
/// yielding an empty list.
pub fn labels(id: &str) -> Result<Vec<api::Label>> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
    api.labels()
}

pub fn create_label(id: &str, name: &str, color: &str) -> Result<()> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
    api.create_label(name, color)
}

pub fn update_label(id: &str, label: &str, name: Option<&str>, color: Option<&str>) -> Result<()> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
    api.update_label(label, name, color)
}

pub fn delete_label(id: &str, label: &str) -> Result<()> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
    api.delete_label(label)
}

/// The console's instance-wide aggregates.
pub fn admin_stats(id: &str) -> Result<serde_json::Value> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
    api.admin_stats()
}

/// One page of the directory.
pub fn admin_users(id: &str, offset: u32, limit: u32, query: &str) -> Result<api::AdminUsers> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
    api.admin_users(offset, limit, query)
}

pub fn admin_org_units(id: &str) -> Result<Vec<api::OrgUnit>> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
    api.admin_org_units()
}

/// The units tree together with each unit's account count, in one trip — what
/// the Unités organisationnelles page needs to draw the tree and its effectives.
pub fn admin_org_units_with_counts(
    id: &str,
) -> Result<(Vec<api::OrgUnit>, std::collections::HashMap<String, i64>)> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
    let units = api.admin_org_units()?;
    // The counts require reading accounts; an operator who may only see the tree
    // still gets the tree, with zero counts.
    let counts = api.admin_org_unit_counts().unwrap_or_default();
    Ok((units, counts))
}

pub fn admin_groups(id: &str) -> Result<Vec<api::AdminGroup>> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
    api.admin_groups()
}

pub fn admin_audiences(id: &str) -> Result<Vec<api::Audience>> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
    api.admin_audiences()
}

pub fn admin_modules(id: &str) -> Result<Vec<api::AdminModule>> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
    api.admin_modules()
}

/// The installed modules together with the instance's default-module path (the
/// `navigation.default_module` setting), in one trip — what the Applications
/// page needs to both list the modules and badge the default one.
pub fn admin_modules_and_default(id: &str) -> Result<(Vec<api::AdminModule>, Option<String>)> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
    let modules = api.admin_modules()?;
    let default = api.admin_default_module()?;
    Ok((modules, default))
}

pub fn set_module_enabled(id: &str, module: &str, enabled: bool) -> Result<()> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
    api.set_module_enabled(module, enabled)
}

pub fn admin_settings(id: &str) -> Result<Vec<api::AdminSetting>> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
    api.admin_settings()
}

pub fn admin_storage(id: &str) -> Result<api::StorageOverview> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
    api.admin_storage()
}

pub fn modules_for(id: &str) -> Result<serde_json::Value> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url);
    api.modules()
}

/// Authenticated GET of a core route, returning raw JSON — for module APIs the
/// core proxies (the desktop Chat calls `/api/v1/chat/*` through this). Handles
/// the bearer token and its rotation for the caller.
pub fn get_json(id: &str, path: &str) -> Result<serde_json::Value> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url);
    api.get_json(path)
}

/// Authenticated request of any method with the caller's headers, returning the status, headers and
/// body whatever the status (see [`api::Api::request`]).
pub fn request(id: &str, method: &str, path: &str, headers: &[(String, String)], body: Option<Vec<u8>>) -> Result<api::RawResponse> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url);
    api.request(method, path, headers, body)
}

/// Authenticated POST of a JSON body to a core route, returning the JSON reply.
pub fn post_json(id: &str, path: &str, body: serde_json::Value) -> Result<serde_json::Value> {
    let cfg = Config::load(id)?;
    let mut api = api::Api::new(id.to_string(), cfg.server_url);
    api.post_json(path, body)
}

/// Fetch an instance's authenticated user profile (`GET /api/v1/me`).
/// Requires a live session; returns an error if offline. Retried once so a
/// transient server hiccup (or a token just rotated by the background sync)
/// doesn't blank out the account display.
pub fn current_user(id: &str) -> Result<(api::User, api::Privileges)> {
    let cfg = Config::load(id)?;
    let mut last: Option<anyhow::Error> = None;
    for attempt in 0..2 {
        // Reload creds each try: the background sync may have saved a fresh token.
        let mut api = api::Api::new(id.to_string(), cfg.server_url.clone());
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
