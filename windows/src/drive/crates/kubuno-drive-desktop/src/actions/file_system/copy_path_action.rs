//! CopyPath (mirrors CopyPathAction.cs)

use super::current_dir;
use crate::actions::Action;
use crate::main_window::MainWindow;

/// `CopyPathAction.cs`: copies the CURRENT folder's path.
pub struct CopyPath;
impl Action for CopyPath {
    fn label(&self) -> &'static str {
        "CopyPath"
    }
    fn description(&self) -> &'static str {
        "CopyPathDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        current_dir(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        if let Some(path) = current_dir(w) {
            crate::utils::storage::clipboard_set_text(w.hwnd, &path);
        }
    }
}
