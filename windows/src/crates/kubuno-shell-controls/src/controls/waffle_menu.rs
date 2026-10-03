//! Code-behind of the user control `WaffleMenu` (`waffle_menu.kbcontrol`, see its comment): the app
//! launcher every Kubuno desktop app can show under its header's waffle.
//!
//! Its data come from the host app, either through a [`LauncherService`] ([`WaffleMenu::set_service`]:
//! the menu then launches apps and saves favourites itself) or through its `Apps` / `Favorites`
//! properties (a binding, or code), the host then acting on `AppLaunched` and `FavoritesEdited`.
//! Either way it raises its events, so the host can close its flyout after a launch and size it to
//! `ContentHeightChanged`:
//!
//! | Event | When |
//! |---|---|
//! | `AppLaunched` | an app's tile was clicked (outside the edit mode) |
//! | `FavoritesEdited` | « OK » confirmed an edit: the list to save |
//! | `EditModeChanged` | the pencil started an edit, « Annuler » / « OK » / Escape ended it |
//! | `ContentHeightChanged` | the content's height changed (an edit, a favourite added or removed) |
//! | `CloseRequested` | Escape outside the edit mode ([`WaffleMenu::escape`]) |

use std::rc::Rc;

use kubuno::views::component::Shared;
use kubuno::views::prelude::*;

use crate::controls::app_tile_grid::{self, FavoritesEventArgs, Tile, TileEventArgs};
use crate::model::favorites::Draft;
use crate::model::launcher::LauncherService;

/// The launcher panel's width (`w-[360px]`).
pub const WIDTH: f32 = 360.0;
/// `max-h: min(580px, …)` outside the edit mode; editing takes the room it needs, so both zones
/// show while dragging.
pub const MAX_HEIGHT: f32 = 580.0;

/// `AppLaunched`: the app opened.
#[derive(kubuno::views::events::EventArgs, Debug, Clone, Default, PartialEq)]
pub struct AppEventArgs {
    /// The app's id (one of the `Apps`).
    pub id: String,
}

/// `EditModeChanged`.
#[derive(kubuno::views::events::EventArgs, Debug, Clone, Default, PartialEq)]
pub struct EditModeEventArgs {
    /// Whether the favourites are now being edited.
    pub editing: bool,
}

/// `ContentHeightChanged`.
#[derive(kubuno::views::events::EventArgs, Debug, Clone, Default, PartialEq)]
pub struct ContentHeightEventArgs {
    /// The height the whole content needs, in DIP ([`WaffleMenu::content_height`]).
    pub height: f32,
}

/// The height of the launcher's content for `apps` with the saved `favorites` (the draft's list
/// while `editing`): what the host sizes its panel to (at most [`MAX_HEIGHT`] outside the edit mode).
pub fn content_height(apps: &[Tile], favorites: &[String], editing: bool) -> f32 {
    app_tile_grid::content_height(apps, favorites, editing)
}

/// The app launcher (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "waffle_menu.kbcontrol")]
#[category("Kubuno")]
#[toolbox(icon = "layout-grid")]
#[default_event("AppLaunched")]
pub struct WaffleMenu {
    base: UserControlCore,
    /// The apps to show (a binding to a `Shared<Vec<Tile>>`; or [`WaffleMenu::set_service`]).
    #[property(bindable, on_change = "content_changed")]
    #[category("Data")]
    pub apps: Shared<Vec<Tile>>,
    /// The favourites exactly as the server holds them (unknown ids included, kept by an edit).
    #[property(bindable, on_change = "content_changed")]
    #[category("Data")]
    pub favorites: Vec<String>,
    /// Whether the favourites are being edited (the pencil sets it, « Annuler » / « OK » clear it).
    #[property(bindable, on_change = "editing_changed")]
    #[category("Behavior")]
    pub editing: bool,
    /// Occurs when an app's tile is clicked: the app to open.
    #[event]
    #[category("Action")]
    pub app_launched: Event<AppEventArgs>,
    /// Occurs when « OK » confirms an edit of the favourites: the list to save.
    #[event]
    #[category("Action")]
    pub favorites_edited: Event<FavoritesEventArgs>,
    /// Occurs when the edit mode starts or ends.
    #[event]
    #[category("Behavior")]
    pub edit_mode_changed: Event<EditModeEventArgs>,
    /// Occurs when the content's height changes (the host resizes its panel).
    #[event]
    #[category("Layout")]
    pub content_height_changed: Event<ContentHeightEventArgs>,
    /// Occurs when Escape asks to close the launcher (outside the edit mode).
    #[event]
    #[category("Action")]
    pub close_requested: Event<EmptyEventArgs>,
    /// The card's title and pencil show (outside the edit mode).
    #[property(bindable)]
    #[browsable(false)]
    pub viewing: bool,
    /// The list an « OK » would save now (the grid's draft, mirrored from its `FavoritesEdited`).
    draft: Vec<String>,
    /// The last height reported by `ContentHeightChanged`.
    reported: f32,
    service: Option<Rc<dyn LauncherService>>,
}

impl WaffleMenu {
    /// Takes the apps and the favourites from `service`, which then opens the apps and saves the
    /// edited favourites.
    pub fn set_service(&mut self, service: Rc<dyn LauncherService>) {
        self.apps = Shared::new(service.apps());
        self.favorites = service.favorites();
        self.service = Some(service);
        self.content_changed();
    }

    /// The height the content needs now (the draft while editing), in DIP.
    pub fn content_height(&self) -> f32 {
        content_height(&self.apps, if self.editing { &self.draft } else { &self.favorites }, self.editing)
    }

    /// What Escape does: abandons an edit first, as on the web; otherwise raises `CloseRequested`.
    pub fn escape(&mut self) {
        if self.editing {
            self.set_editing(false);
        } else {
            self.raise_close_requested(EmptyEventArgs);
        }
    }

    /// Starts editing the favourites.
    pub fn begin_edit(&mut self) {
        self.set_editing(true);
    }

    /// Ends the edit, keeping the draft (« OK »): raises `FavoritesEdited` and saves it through
    /// the service.
    pub fn commit_edit(&mut self) {
        if !self.editing {
            return;
        }
        let list = std::mem::take(&mut self.draft);
        self.favorites = list.clone();
        self.set_editing(false);
        if let Some(service) = self.service.clone() {
            service.save_favorites(&list);
        }
        self.raise_favorites_edited(FavoritesEventArgs { favorites: list });
    }

    fn set_editing(&mut self, editing: bool) {
        if self.editing == editing && self.viewing != editing {
            return;
        }
        self.editing = editing;
        self.editing_changed();
    }

    fn editing_changed(&mut self) {
        let was = !self.viewing;
        self.viewing = !self.editing;
        if self.editing {
            // What the grid's draft starts from, saved as an « OK » right away would save it.
            let installed: Vec<String> = self.apps.iter().map(|t| t.id.clone()).collect();
            self.draft = Draft::new(&self.favorites, &installed).to_saved();
        }
        if was != self.editing {
            self.raise_edit_mode_changed(EditModeEventArgs { editing: self.editing });
        }
        self.report_height();
    }

    fn content_changed(&mut self) {
        self.report_height();
    }

    fn report_height(&mut self) {
        let height = self.content_height();
        if (height - self.reported).abs() > 0.01 {
            self.reported = height;
            self.raise_content_height_changed(ContentHeightEventArgs { height });
        }
    }
}

#[kubuno::views::event_handlers]
impl WaffleMenu {
    fn waffle_menu_load(&mut self) {
        // The designer shows the sample instance (`design/apps.json`).
        if self.design_mode() && self.apps.is_empty() {
            self.apps = Shared::new(app_tile_grid::design_tiles());
            self.favorites = app_tile_grid::design_favorites();
        }
        self.viewing = !self.editing;
        self.report_height();
    }

    fn edit_button_click(&mut self) {
        self.begin_edit();
    }

    fn cancel_button_click(&mut self) {
        self.set_editing(false);
    }

    fn ok_button_click(&mut self) {
        self.commit_edit();
    }

    fn grid_tile_invoked(&mut self, e: &TileEventArgs) {
        if let Some(service) = self.service.clone() {
            service.launch(&e.id);
        }
        self.raise_app_launched(AppEventArgs { id: e.id.clone() });
    }

    fn grid_favorites_edited(&mut self, e: &FavoritesEventArgs) {
        // A favourite added or removed can add or remove a row of either zone.
        self.draft = e.favorites.clone();
        self.report_height();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn tiles(n: usize) -> Vec<Tile> {
        (0..n).map(|i| Tile { id: format!("a{i}"), label: format!("App {i}"), icon: "Cloud".into(), ..Tile::default() }).collect()
    }

    /// A service that remembers what the menu asked of it.
    #[derive(Default)]
    struct Recorder {
        apps: Vec<Tile>,
        favorites: Vec<String>,
        launched: RefCell<Vec<String>>,
        saved: RefCell<Vec<Vec<String>>>,
    }

    impl LauncherService for Recorder {
        fn apps(&self) -> Vec<Tile> {
            self.apps.clone()
        }
        fn favorites(&self) -> Vec<String> {
            self.favorites.clone()
        }
        fn launch(&self, id: &str) {
            self.launched.borrow_mut().push(id.to_string());
        }
        fn save_favorites(&self, favorites: &[String]) {
            self.saved.borrow_mut().push(favorites.to_vec());
        }
    }

    fn menu(n: usize, saved: &[&str]) -> (WaffleMenu, Rc<Recorder>) {
        let service = Rc::new(Recorder { apps: tiles(n), favorites: saved.iter().map(|s| s.to_string()).collect(), ..Recorder::default() });
        let mut m = WaffleMenu::default();
        m.set_service(service.clone());
        m.waffle_menu_load();
        (m, service)
    }

    #[test]
    fn the_content_is_as_tall_as_the_grid_and_grows_while_editing() {
        let (small, _) = menu(2, &["a0"]);
        assert!(small.content_height() < MAX_HEIGHT);
        let (mut large, _) = menu(14, &["a0", "a1", "a2", "a3", "a4", "a5"]);
        let viewing = large.content_height();
        assert!(viewing > MAX_HEIGHT, "fourteen apps scroll");
        large.edit_button_click();
        assert!(large.editing && !large.viewing);
        assert!(large.content_height() > viewing, "the help line takes room");
        large.cancel_button_click();
        assert!(large.viewing && !large.editing);
        assert_eq!(large.content_height(), viewing);
    }

    #[test]
    fn ok_saves_the_draft_through_the_service_and_cancel_saves_nothing() {
        let (mut m, service) = menu(6, &["a0", "gone", "a1"]);
        m.edit_button_click();
        // The draft starts as an « OK » right away would save it: unknown ids last.
        assert_eq!(m.draft, ["a0", "a1", "gone"]);
        m.grid_favorites_edited(&FavoritesEventArgs { favorites: vec!["a2".into(), "a0".into(), "gone".into()] });
        m.cancel_button_click();
        assert!(service.saved.borrow().is_empty(), "nothing saved");
        assert_eq!(m.favorites, ["a0", "gone", "a1"]);
        m.edit_button_click();
        m.grid_favorites_edited(&FavoritesEventArgs { favorites: vec!["a2".into(), "a0".into(), "gone".into()] });
        m.ok_button_click();
        assert_eq!(*service.saved.borrow(), [vec!["a2".to_string(), "a0".into(), "gone".into()]]);
        assert_eq!(m.favorites, ["a2", "a0", "gone"]);
        assert!(m.viewing);
    }

    #[test]
    fn a_tile_launches_its_app_and_escape_abandons_an_edit_first() {
        let (mut m, service) = menu(3, &[]);
        m.grid_tile_invoked(&TileEventArgs { id: "a2".into() });
        assert_eq!(*service.launched.borrow(), ["a2"]);
        m.edit_button_click();
        m.escape();
        assert!(!m.editing && m.viewing, "the first Escape leaves the edit mode");
        assert!(service.saved.borrow().is_empty());
    }

    #[test]
    fn without_a_service_the_properties_drive_it() {
        let mut m = WaffleMenu { apps: Shared::new(tiles(9)), favorites: vec!["a0".into(), "a1".into(), "a2".into()], ..WaffleMenu::default() };
        m.waffle_menu_load();
        assert_eq!(m.content_height(), content_height(&tiles(9), &m.favorites, false));
        m.editing = true;
        m.editing_changed();
        assert!(!m.viewing);
        m.commit_edit();
        assert_eq!(m.favorites, ["a0", "a1", "a2"]);
    }
}
