//! CutItem (mirror of CutItemAction.cs)

use crate::actions::{Action, HotKey};
use crate::actions::file_system::{not_recycle, selected_path};
use crate::main_window::MainWindow;

/// `CutItemAction.cs` (Ctrl+X).
pub struct CutItem;
impl Action for CutItem {
    fn label(&self) -> &'static str {
        "Cut"
    }
    fn description(&self) -> &'static str {
        "CutItemDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("Cut")
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl('X' as u32))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        not_recycle(w) && selected_path(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let paths = w.state.active().selected_paths();
        if !paths.is_empty() {
            crate::utils::storage::clipboard_set_files(w.hwnd, &paths, true);
        }
    }
}
