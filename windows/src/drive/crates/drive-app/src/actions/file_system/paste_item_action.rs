//! PasteItem (mirrors PasteItemAction.cs)

use super::current_dir;
use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `PasteItemAction.cs` (Ctrl+V): background paste (StatusCenter).
pub struct PasteItem;
impl Action for PasteItem {
    fn label(&self) -> &'static str {
        "Paste"
    }
    fn description(&self) -> &'static str {
        "PasteItemDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("Paste")
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl('V' as u32))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        current_dir(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.paste_clipboard();
    }
}
