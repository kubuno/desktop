//! CreateFolder (mirror of CreateFolderAction.cs)

use super::current_dir;
use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `CreateFolderAction.cs` (Ctrl+Maj+N).
pub struct CreateFolder;
impl Action for CreateFolder {
    fn label(&self) -> &'static str {
        "Folder"
    }
    fn description(&self) -> &'static str {
        "CreateFolderDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("NewFolder")
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift('N' as u32))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        current_dir(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        if let Some(dir) = current_dir(w) {
            w.create_item(&dir, true);
            w.invalidate();
        }
    }
}
