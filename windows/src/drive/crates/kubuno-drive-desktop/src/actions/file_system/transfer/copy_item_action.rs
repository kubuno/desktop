//! CopyItem (mirrors CopyItemAction.cs)

use crate::actions::{Action, HotKey};
use crate::actions::file_system::{not_recycle, selected_path};
use crate::main_window::MainWindow;

/// `CopyItemAction.cs` (Ctrl+C).
pub struct CopyItem;
impl Action for CopyItem {
    fn label(&self) -> &'static str {
        "Copy"
    }
    fn description(&self) -> &'static str {
        "CopyItemDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("Copy")
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl('C' as u32))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        not_recycle(w) && selected_path(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        // The entire selection goes to the clipboard (`SelectedItems`).
        let paths = w.state.active().selected_paths();
        if !paths.is_empty() {
            crate::utils::storage::clipboard_set_files(w.hwnd, &paths, false);
        }
    }
}
