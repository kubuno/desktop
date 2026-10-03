//! SetDefaultTheme (mirror of SetDefaultThemeAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

use super::base_app_theme_action::set_theme;

/// `SetDefaultThemeAction.cs` : suivre Windows.
pub struct SetDefaultTheme;
impl Action for SetDefaultTheme {
    fn label(&self) -> &'static str {
        "DefaultTheme"
    }
    fn description(&self) -> &'static str {
        "SwitchToDefaultThemeDescription"
    }
    fn is_executable(&self, _w: &MainWindow) -> bool {
        crate::services::settings::get().theme
            != crate::services::settings::ThemeSetting::System
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        set_theme(w, crate::services::settings::ThemeSetting::System);
    }
}
