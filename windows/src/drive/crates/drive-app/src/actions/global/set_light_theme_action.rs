//! SetLightTheme (mirror of SetLightThemeAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

use super::base_app_theme_action::set_theme;

/// `SetLightThemeAction.cs` (l'original compose « {LightTheme} Theme »).
pub struct SetLightTheme;
impl Action for SetLightTheme {
    fn label(&self) -> &'static str {
        "LightTheme"
    }
    fn description(&self) -> &'static str {
        "SwitchToLightThemeDescription"
    }
    fn is_executable(&self, _w: &MainWindow) -> bool {
        crate::services::settings::get().theme
            != crate::services::settings::ThemeSetting::Light
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        set_theme(w, crate::services::settings::ThemeSetting::Light);
    }
}
