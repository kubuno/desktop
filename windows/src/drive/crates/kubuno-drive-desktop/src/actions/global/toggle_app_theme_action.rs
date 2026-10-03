//! ToggleAppTheme (mirrors ToggleAppThemeAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

use super::base_app_theme_action::set_theme;

/// `ToggleAppThemeAction.cs` (Ctrl+Alt+T): toggles light ↔ dark based on
/// the EFFECTIVE theme (System follows Windows).
pub struct ToggleAppTheme;
impl Action for ToggleAppTheme {
    fn label(&self) -> &'static str {
        "ToggleTheme"
    }
    fn description(&self) -> &'static str {
        "ToggleThemeDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_alt('T' as u32))
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        use crate::services::settings::ThemeSetting;
        let light_now = match crate::services::settings::get().theme {
            ThemeSetting::Light => true,
            ThemeSetting::Dark => false,
            ThemeSetting::System => crate::styles::theme::system_uses_light_theme(),
        };
        set_theme(w, if light_now { ThemeSetting::Dark } else { ThemeSetting::Light });
    }
}
