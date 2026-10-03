//! OpenLogFileLocation (mirrors OpenLogFileLocationAction.cs)

use super::log_file_path;
use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

/// `OpenLogFileLocationAction.cs` (Ctrl+Shift+.): the log folder within the
/// application itself.
pub struct OpenLogFileLocation;
impl Action for OpenLogFileLocation {
    fn label(&self) -> &'static str {
        "OpenLogLocation"
    }
    fn description(&self) -> &'static str {
        "OpenLogFileLocationDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift(0xBE)) // VK_OEM_PERIOD
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        if let Some(dir) = log_file_path().parent() {
            w.navigate_active(Location::Dir(dir.to_path_buf()));
        }
    }
}
