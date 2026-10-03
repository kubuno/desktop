//! Theme facade: the palette + system detection live in
//! `drive-app-controls` (mirrors `Files.App.Controls/Themes`); only the
//! selector that reads the application's settings remains here.

pub use drive_app_controls::themes::{system_uses_light_theme, Theme, ThemeMode};

/// Resolves the theme from user settings (System follows Windows).
/// (`Theme` now being a type from another crate, we can't add an inherent
/// method to it — hence this free function.)
pub fn from_settings() -> Theme {
    match crate::services::settings::get().theme {
        crate::services::settings::ThemeSetting::Light => Theme::light(),
        crate::services::settings::ThemeSetting::Dark => Theme::dark(),
        crate::services::settings::ThemeSetting::System => Theme::detect(),
    }
}
