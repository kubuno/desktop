//! OpenSettings (mirror of OpenSettingsAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

/// `OpenSettingsAction.cs` (Ctrl+,).
pub struct OpenSettings;
impl Action for OpenSettings {
    fn label(&self) -> &'static str {
        "Settings"
    }
    fn description(&self) -> &'static str {
        "OpenSettingsDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl(0xBC)) // VK_OEM_COMMA
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.navigate_active(Location::Settings);
    }
}
