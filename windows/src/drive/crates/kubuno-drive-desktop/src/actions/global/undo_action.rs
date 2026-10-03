//! Undo (mirrors UndoAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

/// `UndoAction.cs` (Ctrl+Z): undoes the last file operation
/// (`StorageHistoryHelpers.TryUndo`).
pub struct Undo;
impl Action for Undo {
    fn label(&self) -> &'static str {
        "Undo"
    }
    fn description(&self) -> &'static str {
        "UndoDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl('Z' as u32))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        matches!(w.state.active().location, Location::Dir(_)) && w.can_undo()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.try_undo();
    }
}
