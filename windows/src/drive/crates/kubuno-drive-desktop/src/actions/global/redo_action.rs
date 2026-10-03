//! Redo (mirrors RedoAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

/// `RedoAction.cs` (Ctrl+Y): redoes the last undone operation.
pub struct Redo;
impl Action for Redo {
    fn label(&self) -> &'static str {
        "Redo"
    }
    fn description(&self) -> &'static str {
        "RedoDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl('Y' as u32))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        matches!(w.state.active().location, Location::Dir(_)) && w.can_redo()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.try_redo();
    }
}
