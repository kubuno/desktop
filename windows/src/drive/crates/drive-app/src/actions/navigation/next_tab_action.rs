//! NextTab (mirror of NextTabAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `NextTabAction.cs` (Ctrl+Tab) / `PreviousTabAction.cs` (Ctrl+Maj+Tab).
pub struct NextTab;
impl Action for NextTab {
    fn label(&self) -> &'static str {
        "NextTab"
    }
    fn description(&self) -> &'static str {
        "NextTabDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl(0x09)) // VK_TAB
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.tabs.len() > 1
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let count = w.state.tabs.len();
        w.state.active_tab = (w.state.active_tab + 1) % count;
        w.invalidate();
    }
}
