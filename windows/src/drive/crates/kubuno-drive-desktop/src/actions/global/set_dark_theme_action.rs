//! SetDarkTheme (mirror of SetDarkThemeAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

use super::base_app_theme_action::set_theme;

/// `SetDarkThemeAction.cs`.
pub struct SetDarkTheme;
impl Action for SetDarkTheme {
    fn label(&self) -> &'static str {
        "DarkTheme"
    }
    fn description(&self) -> &'static str {
        "SwitchToDarkThemeDescription"
    }
    fn is_executable(&self, _w: &MainWindow) -> bool {
        crate::services::settings::get().theme
            != crate::services::settings::ThemeSetting::Dark
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        set_theme(w, crate::services::settings::ThemeSetting::Dark);
    }
}
