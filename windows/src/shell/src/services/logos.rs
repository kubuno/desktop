//! The launcher's pictures — the modules' brand logos and the account's photo — and the module list,
//! kept on disk so the header looks the same offline as online.
//!
//! The source of truth is the web's: the core answers `/api/v1/modules` with each app's `logo_url`,
//! the very file the web launcher shows (`core/frontend/public/<id>-logo.png`, or `.svg`). The
//! desktop downloads that file and keeps it per instance, under a name derived from its CONTENT
//! (`<key>-<hash>.<ext>`): a logo or a photo that changes on the server gets a new path, so no
//! path-keyed decode cache of the renderer can keep showing the old one, and an unchanged one is not
//! rewritten. A small index remembers which file answers which URL, for the next start offline.
//!
//! Before the first download (a first start offline, the `--sample`), the fallback is the same web
//! files again, embedded at BUILD time (by `kubuno-header-data`'s build script) from `core/frontend/public` — never a copy kept
//! by hand — and written out once to the user's cache directory, because the painters decode from a
//! path.
//!
//! The mechanics (content-addressed files, index, embedded fallback) live in `kubuno_header_data::logos`,
//! shared with the other desktop apps; this module only picks the directories and does the download.

use std::path::PathBuf;

use kubuno_header_data::logos::PictureCache;

use crate::services::backend;

/// The embedded copy of the logo a server serves at `url` (`/drive-logo.png`, or a full URL ending so),
/// written out under the user's cache directory.
pub fn builtin_for_url(url: &str) -> Option<PathBuf> {
    kubuno_header_data::logos::builtin_for_url(&kubuno_header_data::logos::default_builtin_dir()?, url)
}

/// The embedded web logo of app or module `id` (see `kubuno_header_data::logos::builtin_for_id`).
pub fn builtin_for_id(id: &str) -> Option<PathBuf> {
    kubuno_header_data::logos::builtin_for_id(&kubuno_header_data::logos::default_builtin_dir()?, id)
}

fn cache_dir(instance: &str) -> Option<PathBuf> {
    Some(kubuno_sync::config::instance_dir(instance).ok()?.join("launcher-cache"))
}

fn cache(instance: &str) -> Option<PictureCache> {
    cache_dir(instance).map(PictureCache::new)
}

/// The picture the server serves at `url` for `key` (an app id, `avatar`) of `instance`, as a local file.
///
/// `online`: downloaded (the server is the source of truth), stored under its content's name, and
/// recorded in the index — the previous file of that key goes. When the download fails, or `online`
/// is false, the file last downloaded from that same URL, if any. The offline sample has no cache.
pub fn cached(instance: &str, key: &str, url: &str, online: bool) -> Option<PathBuf> {
    if backend::is_sample() {
        return None;
    }
    let cache = cache(instance)?;
    if online {
        if let Some(found) = download(instance, &cache, key, url) {
            return Some(found);
        }
    }
    cache.lookup(key, url)
}

fn download(instance: &str, cache: &PictureCache, key: &str, url: &str) -> Option<PathBuf> {
    if url.starts_with("http://") || url.starts_with("https://") {
        // Only the instance's own channel is trusted with its token; a foreign URL is not fetched.
        return None;
    }
    let path = if url.starts_with('/') { url.to_string() } else { format!("/{url}") };
    let bytes = match backend::fetch_bytes(instance, &path) {
        Ok(b) => b,
        Err(e) => {
            kubuno::tracing::debug!("[launcher] {path} : {e}");
            return None;
        }
    };
    let stored = cache.store(key, url, &bytes);
    if stored.is_none() {
        kubuno::tracing::warn!("[launcher] {path} was not cached ({} bytes): kept the previous one", bytes.len());
    }
    stored
}

/// Forgets the cached picture of `key` (the account no longer has a photo).
pub fn forget(instance: &str, key: &str) {
    if backend::is_sample() {
        return;
    }
    if let Some(cache) = cache(instance) {
        cache.forget(key);
    }
}

/// Removes the files earlier builds cached under a fixed name (`avatar.img`, `logo_<id>.img`): a fixed
/// path kept showing a picture the server had changed.
pub fn remove_legacy(instance: &str) {
    if backend::is_sample() {
        return;
    }
    let Ok(dir) = kubuno_sync::config::instance_dir(instance) else { return };
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "avatar.img" || (name.starts_with("logo_") && name.ends_with(".img")) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}


// ── The module list ──────────────────────────────────────────────────────────────────────────────

/// Keeps the last `/api/v1/modules` answer of `instance`, for the launcher of a start offline.
pub fn save_modules(instance: &str, value: &serde_json::Value) {
    if backend::is_sample() {
        return;
    }
    if let Some(cache) = cache(instance) {
        cache.save_json("modules.json", value);
    }
}

/// The last `/api/v1/modules` answer kept for `instance`.
pub fn load_modules(instance: &str) -> Option<serde_json::Value> {
    if backend::is_sample() {
        return None;
    }
    cache(instance)?.load_json("modules.json")
}
