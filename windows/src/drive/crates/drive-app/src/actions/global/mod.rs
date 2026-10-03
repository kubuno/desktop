//! Port de `Files.App/Actions/Global/`.

pub mod base_app_theme_action;
pub mod edit_path_action;
pub mod undo_action;
pub mod redo_action;
pub mod search_action;
pub mod toggle_full_screen_action;
pub mod enter_compact_overlay_action;
pub mod exit_compact_overlay_action;
pub mod toggle_compact_overlay_action;
pub mod set_light_theme_action;
pub mod set_dark_theme_action;
pub mod set_default_theme_action;
pub mod toggle_app_theme_action;
pub mod open_help_action;

pub use edit_path_action::EditPath;
pub use enter_compact_overlay_action::EnterCompactOverlay;
pub use exit_compact_overlay_action::ExitCompactOverlay;
pub use open_help_action::OpenHelp;
pub use redo_action::Redo;
pub use search_action::Search;
pub use set_dark_theme_action::SetDarkTheme;
pub use set_default_theme_action::SetDefaultTheme;
pub use set_light_theme_action::SetLightTheme;
pub use toggle_app_theme_action::ToggleAppTheme;
pub use toggle_compact_overlay_action::ToggleCompactOverlay;
pub use toggle_full_screen_action::ToggleFullScreen;
pub use undo_action::Undo;
