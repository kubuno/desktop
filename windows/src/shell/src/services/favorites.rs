//! The shell's [`LauncherService`]: what the app launcher (`kubuno_shell_controls::WaffleMenu`, in the
//! header's waffle flyout) shows and does.
//!
//! The apps are the connected instance's modules (`services::apps`, fetched in the background);
//! the favourites are the SAME list the web shows — `preferences.waffle_favorites` on the account,
//! an ordered array of app ids, ids this build does not know included (`kubuno_shell_controls::Draft`
//! carries them through an edit). Opening an app opens its route in the user's browser; a saved
//! list goes back to the server, which keeps the web and the desktop showing one list.

use kubuno_shell_controls::{LauncherService, Tile};

use crate::services::apps::AppEntry;
use crate::services::backend;

pub use kubuno_header_data::modules::tile;

/// The launcher's data for the instance the window shows (see the module doc).
pub struct ShellLauncher {
    apps: Vec<AppEntry>,
    favorites: Vec<String>,
    /// The instance's address, the apps' routes are opened under.
    server: String,
    /// Hears a saved list (the window keeps it for the next opening).
    saved: Box<dyn Fn(Vec<String>)>,
    /// Opens the administration console: the account may enter it, so the launcher lists it last, as
    /// the web grafts its console tile (`core-admin`) for an administrator.
    admin: Option<Box<dyn Fn()>>,
}

/// The console's tile id, the web's (`useAdminConsoleApp`'s `ADMIN_APP_ID`).
pub const ADMIN_APP_ID: &str = "core-admin";

impl ShellLauncher {
    /// The launcher for `apps` of the instance at `server`, with the account's `favorites`; `saved`
    /// hears every list the user confirms (the window keeps it and sends it to the server with
    /// [`persist`]).
    pub fn new(apps: Vec<AppEntry>, favorites: Vec<String>, server: &str, saved: impl Fn(Vec<String>) + 'static) -> Self {
        Self { apps, favorites, server: server.to_string(), saved: Box::new(saved), admin: None }
    }

    /// Lists the administration console (an administrator's launcher), opened by `open`.
    pub fn with_admin(mut self, open: impl Fn() + 'static) -> Self {
        self.admin = Some(Box::new(open));
        self
    }

    /// The URL app `id` opens at, `None` for an app the instance does not have.
    pub fn url_of(&self, id: &str) -> Option<String> {
        let app = self.apps.iter().find(|a| a.id == id)?;
        Some(kubuno_header_data::modules::web_url(&self.server, &app.path))
    }
}

impl LauncherService for ShellLauncher {
    fn apps(&self) -> Vec<Tile> {
        let mut tiles: Vec<Tile> = self.apps.iter().map(tile).collect();
        if self.admin.is_some() {
            tiles.push(Tile {
                id: ADMIN_APP_ID.into(),
                label: kubuno_shell_controls::ShellControlsResources::account_admin().to_string(),
                icon: "Shield".into(),
                logo: crate::services::logos::builtin_for_id("admin").map(|p| p.to_string_lossy().into_owned()),
                module: Some(ADMIN_APP_ID.into()),
                module_label: None,
            });
        }
        tiles
    }

    fn favorites(&self) -> Vec<String> {
        self.favorites.clone()
    }

    fn launch(&self, id: &str) {
        if id == ADMIN_APP_ID {
            if let Some(open) = &self.admin {
                open();
            }
            return;
        }
        if let Some(url) = self.url_of(id) {
            crate::platform::actions::open_in_browser(&url);
        }
    }

    fn save_favorites(&self, favorites: &[String]) {
        (self.saved)(favorites.to_vec());
    }
}

/// Persists `favorites` to the SERVER of the active instance, which keeps the web and the desktop
/// showing one list (off the UI thread, except in the offline sample).
pub fn persist(favorites: Vec<String>) {
    let Some(id) = crate::services::apps::active_instance().map(|c| c.id) else { return };
    let save = move || {
        if let Err(e) = backend::set_waffle_favorites(&id, &favorites) {
            kubuno::tracing::warn!("[waffle] enregistrement des favoris : {e}");
        }
    };
    if backend::is_sample() {
        save();
    } else {
        std::thread::spawn(save);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn app(id: &str, path: &str) -> AppEntry {
        AppEntry { id: id.into(), label: id.to_uppercase(), path: path.into(), icon: "Cloud".into(), logo_url: None, logo_path: Some("C:/cache/logo.img".into()), module: id.into(), module_label: String::new() }
    }

    #[test]
    fn the_apps_become_tiles_and_open_under_the_server() {
        let launcher = ShellLauncher::new(vec![app("drive", "/drive"), app("documents", "office/documents")], vec!["drive".into()], "https://cloud.exemple.fr/", |_| {});
        let tiles = launcher.apps();
        assert_eq!(tiles[0].id, "drive");
        assert_eq!(tiles[0].logo.as_deref(), Some("C:/cache/logo.img"), "the server logo wins over the icon");
        assert_eq!(launcher.url_of("documents").as_deref(), Some("https://cloud.exemple.fr/office/documents"));
        assert_eq!(launcher.url_of("nope"), None);
        assert_eq!(launcher.favorites(), ["drive"]);
    }

    #[test]
    fn a_saved_list_is_handed_to_the_window() {
        let heard = Rc::new(RefCell::new(Vec::new()));
        let sink = heard.clone();
        let launcher = ShellLauncher::new(vec![app("drive", "/drive")], Vec::new(), "https://x", move |l| sink.borrow_mut().push(l));
        launcher.save_favorites(&["drive".into(), "gone".into()]);
        assert_eq!(*heard.borrow(), [vec!["drive".to_string(), "gone".into()]]);
    }
}
