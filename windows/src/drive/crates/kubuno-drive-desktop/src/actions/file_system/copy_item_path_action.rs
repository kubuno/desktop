//! CopyItemPath (mirror of CopyItemPathAction.cs)

use super::{selected_path, target};
use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `CopyItemPathAction.cs` (Ctrl+Maj+C).
pub struct CopyItemPath;
impl Action for CopyItemPath {
    fn label(&self) -> &'static str {
        "CopyPath"
    }
    fn description(&self) -> &'static str {
        "CopyItemPathDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift('C' as u32))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        selected_path(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        if let Some(path) = target(w, parameter) {
            crate::utils::storage::clipboard_set_text(w.hwnd, &path);
        }
    }
}
