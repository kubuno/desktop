//! Where the launcher's data comes from: the host app's [`LauncherService`].
//!
//! The launcher knows no server and no account. The app that places it says which apps there are,
//! which of them the user made favourites, how one is opened and where an edited list is saved —
//! the shell from its own fetch of the instance's modules; another app, later, through the shell's
//! token broker (`kubuno-account`), see this crate's README.

use crate::controls::app_tile_grid::Tile;

/// The launcher's data and actions, provided by the app that places a [`crate::WaffleMenu`]
/// ([`crate::WaffleMenu::set_service`]). Called on the UI thread: a slow answer blocks it, so an
/// implementation hands back what it already holds and does its network work on a thread of its own.
pub trait LauncherService {
    /// The apps to show, in the server's order.
    fn apps(&self) -> Vec<Tile>;

    /// The favourites exactly as the server holds them (`preferences.waffle_favorites`), ids this
    /// build does not know included: they are carried through an edit untouched ([`crate::Draft`]).
    fn favorites(&self) -> Vec<String>;

    /// Opens the app `id` (one of [`Self::apps`]).
    fn launch(&self, id: &str);

    /// Persists the favourites the user confirmed (« OK »): the shown ones in order, then the ids
    /// this build does not know.
    fn save_favorites(&self, favorites: &[String]);
}
