//! OpenLogFile (mirrors OpenLogFileAction.cs)

use super::log_file_path;
use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::shell_verb;

/// `OpenLogFileAction.cs` (Ctrl+.): opens `debug.log` with the default
/// application.
pub struct OpenLogFile;
impl Action for OpenLogFile {
    fn label(&self) -> &'static str {
        "OpenLogFile"
    }
    fn description(&self) -> &'static str {
        "OpenLogFileDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl(0xBE)) // VK_OEM_PERIOD
    }
    fn is_executable(&self, _w: &MainWindow) -> bool {
        log_file_path().exists()
    }
    fn execute(&self, _w: &mut MainWindow, _parameter: Option<&str>) {
        shell_verb(&log_file_path().to_string_lossy(), "open");
    }
}
