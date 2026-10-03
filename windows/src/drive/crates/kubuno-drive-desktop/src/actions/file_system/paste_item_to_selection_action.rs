//! PasteItemToSelection (mirrors PasteItemToSelectionAction.cs)

use super::current_dir;
use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `PasteItemToSelectionAction.cs` (Ctrl+Shift+V): pastes into the
/// SELECTED folder, or into the current folder if nothing is selected.
pub struct PasteItemToSelection;
impl Action for PasteItemToSelection {
    fn label(&self) -> &'static str {
        "PasteToSelectedFolder"
    }
    fn description(&self) -> &'static str {
        "PasteItemToSelectionDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("Paste")
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift('V' as u32))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        if current_dir(w).is_none() {
            return false;
        }
        // Without selection: the current folder. With: A SINGLE selected folder.
        match w.state.active().selected_entry() {
            None => true,
            Some(e) => e.is_dir && w.state.active().selected_paths().len() == 1,
        }
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let dest = w
            .state
            .active()
            .selected_entry()
            .filter(|e| e.is_dir)
            .map(|e| e.path.clone())
            .or_else(|| current_dir(w));
        if let Some(dest) = dest {
            w.paste_clipboard_into(dest);
        }
    }
}
