//! The menus of a Kubuno header — the app launcher (the web's `WaffleMenu`) and the account panel
//! (the web's `AccountMenu`), which every module's header shows — as user controls any Kubuno desktop
//! app can place in its views. Names, properties, events and data shapes are shared with the web
//! (vskubuno `docs/SHELL-CONTROLS.md`).
//!
//! - [`WaffleMenu`] (`controls/waffle_menu.kbcontrol` + `.rs`): the favourites card (its title and
//!   pencil, or « Annuler », « OK » and the help line while editing) over every other app, scrolling
//!   as one, drawn by [`AppTileGrid`]. Data: `Apps`, `Favorites`, `Editing`, or a [`LauncherService`].
//! - [`AccountMenu`] (`controls/account_menu.kbcontrol` + `.rs`): the active account's address and
//!   close button, its avatar, the greeting, « Gérer votre compte », the other accounts and the
//!   actions, drawn with [`PanelMenu`] and [`AccentPill`]. Data: `User`, `Accounts`, `ShowAdmin`, or an
//!   [`AccountService`].
//! - [`Draft`] (`model/favorites.rs`): the favourites being edited, which never loses an id this
//!   build does not know.
//!
//! The chrome that carries a menu (a `WindowKind="Flyout"` window with its rounded corners, blur and
//! tint, closing on a click outside) stays with the host app: the shell's `waffle_flyout.kbview` and
//! `user_flyout.kbview` are the reference hosts. The crate depends on the `kubuno-desktop` facade only, and on
//! no app crate: reuse goes through shared crates (vskubuno `docs/VIEWS-SPEC.md`, "Module isolation").
//! Its strings are its own (`resources/shell_controls.kbres`, `launcher_*` and `account_*` keys, so they never
//! collide with an app's).

pub mod controls;
pub mod model;

pub use controls::account_button::{set_default_accounts, AccountButton};
pub use controls::account_menu::{AccountEventArgs, AccountMenu};
pub use controls::accounts_card::AccountsCard;
pub use controls::avatar_decor::AvatarDecor;
pub use controls::header_actions::HeaderActions;
pub use controls::app_tile_grid::{AppTileGrid, FavoritesEventArgs, Tile, TileEventArgs};
pub use controls::panel_menu::{AccentPill, MenuItemEventArgs, MenuRow, PanelMenu};
pub use controls::waffle_button::{set_default_launcher, WaffleButton};
pub use controls::waffle_menu::{AppEventArgs, ContentHeightEventArgs, EditModeEventArgs, WaffleMenu};
pub use model::account::{AccountAction, AccountEntry, AccountService, AccountUser};
pub use model::favorites::Draft;
pub use model::launcher::LauncherService;

// `ShellControlsResources::launcher_title()`, `ShellControlsResources::account_manage()`… — the strings of
// `resources/shell_controls.kbres` (neutral English) and `resources/shell_controls.fr.kbres`, in the current UI culture;
// `{Res launcher_…}` / `{Res account_…}` in the views.
kubuno_desktop::resources!(pub ShellControlsResources, "resources/shell_controls.kbres");
