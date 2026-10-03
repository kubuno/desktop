//! PreviousTab (mirror of PreviousTabAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

pub struct PreviousTab;
impl Action for PreviousTab {
    fn label(&self) -> &'static str {
        "PreviousTab"
    }
    fn description(&self) -> &'static str {
        "PreviousTabDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift(0x09))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.tabs.len() > 1
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let count = w.state.tabs.len();
        w.state.active_tab = w.state.active_tab.checked_sub(1).unwrap_or(count - 1);
        w.invalidate();
    }
}
