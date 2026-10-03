//! SelectAll (mirror of SelectAllAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `SelectAllAction.cs` (Ctrl+A).
pub struct SelectAll;
impl Action for SelectAll {
    fn label(&self) -> &'static str {
        "SelectAll"
    }
    fn description(&self) -> &'static str {
        "SelectAllDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl('A' as u32))
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.state.active_mut().select_all();
        w.invalidate();
    }
}
