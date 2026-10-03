//! ReopenClosedTab (mirror of ReopenClosedTabAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `ReopenClosedTabAction.cs` (Ctrl+Maj+T).
pub struct ReopenClosedTab;
impl Action for ReopenClosedTab {
    fn label(&self) -> &'static str {
        "ReopenClosedTab"
    }
    fn description(&self) -> &'static str {
        "ReopenClosedTabDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift('T' as u32))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.has_closed_tabs()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.reopen_closed_tab();
    }
}
