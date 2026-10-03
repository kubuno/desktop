//! The launcher's contents: the connected instance and the modules it exposes.
//!
//! Same source as before — the core's `/api/v1/modules` — but the result feeds
//! the waffle's tiles instead of a JSON payload crossing an IPC bridge. The
//! fetch runs on a background thread and hands its result back to the window
//! (its `UiDispatcher`), where the view state is only ever touched.

use kubuno_header_data::modules::{initials_of, migrate_favorites, parse_modules};

use crate::services::{backend, logos};

pub use kubuno_header_data::AppEntry;

/// Connection state of the active instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Conn {
    Online,
    #[default]
    Offline,
    Expired,
}

/// The logo files the last refresh settled on, by app id (`logo_for` reads them).
static RESOLVED: std::sync::Mutex<Vec<(String, std::path::PathBuf)>> = std::sync::Mutex::new(Vec::new());

/// Module `module`'s brand logo as the web shows it, as an icon value the painters draw at any DPI
/// (`Canvas::vector_icon` takes an image file: an SVG as vectors, a raster resampled): the logo the
/// last refresh downloaded from the server, else the web's own file embedded at build time. `None`
/// for a module that ships no logo — it falls back to its glyph, exactly like the web.
pub(crate) fn logo_for(module: &str) -> Option<&'static str> {
    let resolved = RESOLVED.lock().unwrap_or_else(|p| p.into_inner()).iter().find(|(id, _)| id == module).map(|(_, p)| p.clone());
    let path = resolved.or_else(|| logos::builtin_for_id(module))?;
    kubuno::views::icon::resolve(&path.to_string_lossy())
}

/// The instance the launcher shows: the chosen one, else the first configured one.
pub fn active_instance() -> Option<kubuno_sync::Config> {
    let instances = backend::list_instances();
    let chosen = crate::services::settings::get().active_instance;
    let picked = instances.iter().position(|c| c.id == chosen).unwrap_or(0);
    instances.into_iter().nth(picked)
}

/// What the header needs about the signed-in user.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Identity {
    pub initials: String,
    /// The display name and the address, for the account panel's greeting and
    /// its header — the header itself only ever needed the initials.
    pub name:     String,
    pub email:    String,
    pub used:     u64,
    pub quota:    u64,
    /// The profile photo on disk, once downloaded. `None` keeps the initials.
    pub avatar:   Option<std::path::PathBuf>,
    /// The waffle's favourites, exactly as stored on the server.
    pub favorites: Vec<String>,
    /// Whether this account may enter the administration console. The server
    /// enforces every privilege regardless; this only decides whether the entry
    /// is shown, as the web does.
    pub is_admin:  bool,
}

/// Outcome of one launcher refresh: the tiles (or why there are none), and the identity.
#[derive(Debug, Clone, Default)]
pub struct Refreshed {
    pub apps: Vec<AppEntry>,
    pub conn: Conn,
    pub identity: Option<Identity>,
}

/// Fetches the modules and the identity of `instance` on a background thread, then hands them
/// to `done` (still on that thread: `done` posts them to the window).
pub fn refresh_in_background(instance: kubuno_sync::Config, done: impl FnOnce(Refreshed) + Send + 'static) {
    std::thread::spawn(move || {
        let id = instance.id.clone();
        let (apps, conn) = fetch(&id);
        let identity = backend::current_user(&id).ok().map(|(u, privileges)| {
            let name = u
                .display_name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .or_else(|| u.username.clone())
                .unwrap_or_else(|| u.email.clone());
            Identity {
                initials: initials_of(&name),
                name,
                email: u.email.clone(),
                is_admin: privileges.is_admin,
                used: u.used_bytes,
                quota: u.quota_bytes,
                avatar: cache_avatar(&id, u.avatar_url.as_deref()),
                // The waffle's favourites live in the user's preferences, which
                // is what keeps this list identical to the web's (ids an older desktop wrote mapped to the web's).
                favorites: migrate_favorites(&kubuno_sync::waffle_favorites(&u), &apps),
            }
        });
        done(Refreshed { apps, conn, identity });
    });
}

/// The profile photo the server serves at `url`, as a local file (`services::logos::cached`).
///
/// It goes through a file because the renderer decodes images from a path. The file is named after
/// its content: the `Avatar` control decodes a path once per process, so a photo changed on the web
/// gets a new path and shows at the next refresh instead of the old one. A failed download keeps the
/// photo last downloaded from that same URL; an account without a photo forgets it (initials).
fn cache_avatar(id: &str, url: Option<&str>) -> Option<std::path::PathBuf> {
    match url.map(str::trim).filter(|u| !u.is_empty()) {
        Some(url) => logos::cached(id, "avatar", url, true),
        None => {
            logos::forget(id, "avatar");
            None
        }
    }
}

fn fetch(id: &str) -> (Vec<AppEntry>, Conn) {
    // Through the engine, not the stored access token: it refreshes (and
    // rotates) an expired one instead of silently returning an empty launcher.
    logos::remove_legacy(id);
    let (value, conn) = match backend::modules_for(id) {
        Ok(v) => {
            logos::save_modules(id, &v);
            (v, Conn::Online)
        }
        Err(e) => {
            // Only a refused refresh token really ends the session; a network
            // blip or a 5xx leaves it valid, so it reads as offline.
            let conn = match e.downcast_ref::<kubuno_sync::api::AuthFailure>() {
                Some(kubuno_sync::api::AuthFailure::Genuine) => Conn::Expired,
                _ => Conn::Offline,
            };
            // Offline-first: the launcher of a start without the server is the last one it sent.
            match logos::load_modules(id) {
                Some(v) => (v, conn),
                None => return (Vec::new(), conn),
            }
        }
    };
    let mut apps = parse_modules(&value);
    resolve_logos(id, &mut apps, conn == Conn::Online);
    (apps, conn)
}

/// Gives each app its logo file, still on the background thread so the UI thread only decodes a
/// local file: the server's logo (downloaded when `online`, else the copy last downloaded), else the
/// web's own file embedded at build time — the logo the server names, or the one the core's lookup
/// rule finds for the app's id (a server that does not know a format yet, like Office's SVG). With
/// none, `icon` stays in charge: the launcher never comes up empty-handed.
fn resolve_logos(id: &str, apps: &mut [AppEntry], online: bool) {
    let mut resolved = Vec::new();
    for app in apps.iter_mut() {
        let url = app.logo_url.as_deref();
        let path = url
            .and_then(|u| logos::cached(id, &app.id, u, online))
            .or_else(|| url.and_then(logos::builtin_for_url))
            .or_else(|| logos::builtin_for_id(&app.id));
        if let Some(p) = &path {
            resolved.push((app.id.clone(), p.clone()));
        }
        app.logo_path = path.filter(|p| kubuno_header_data::logos::tile_grid_draws(p));
    }
    *RESOLVED.lock().unwrap_or_else(|p| p.into_inner()) = resolved;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Before any download, the logo is the web's own file embedded at build time: by the name the
    /// server serves it under, or by the core's lookup rule for the app's id (Office's SVG).
    #[test]
    fn the_fallback_is_the_web_file() {
        if kubuno_header_data::logos::builtin_names().next().is_none() {
            return; // built without `core/frontend/public` next to the checkout
        }
        let drive = logos::builtin_for_url("/drive-logo.png").expect("drive's web logo is embedded");
        assert_eq!(std::fs::read(&drive).ok().as_deref().and_then(kubuno_header_data::logos::image_extension), Some("png"));
        let office = logos::builtin_for_id("office").expect("office's web logo is embedded");
        assert_eq!(office.extension().and_then(|e| e.to_str()), Some("svg"));
        assert!(logo_for("drive").is_some_and(|v| v.ends_with(".png")), "the admin console draws the web logo too");
        assert_eq!(logos::builtin_for_id("no-such-module"), None);
    }
}
