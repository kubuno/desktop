//! OpenSettingsFile (mirrors OpenSettingsFileAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::shell_verb;

/// `OpenSettingsFileAction.cs` (Ctrl+Shift+,): `settings.json` with the
/// default editor.
pub struct OpenSettingsFile;
impl Action for OpenSettingsFile {
    fn label(&self) -> &'static str {
        "EditSettingsFile"
    }
    fn description(&self) -> &'static str {
        "EditSettingsFileDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift(0xBC)) // VK_OEM_COMMA
    }
    fn is_executable(&self, _w: &MainWindow) -> bool {
        crate::services::settings::settings_file_path().exists()
    }
    fn execute(&self, _w: &mut MainWindow, _parameter: Option<&str>) {
        let path = crate::services::settings::settings_file_path();
        shell_verb(&path.to_string_lossy(), "open");
    }
}
