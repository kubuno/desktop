//! FocusOtherPane (mirrors FocusOtherPane.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `FocusOtherPane.cs` (Ctrl+Shift+Right): focus moves to the other pane.
pub struct FocusOtherPane;
impl Action for FocusOtherPane {
    fn label(&self) -> &'static str {
        "FocusOtherPane"
    }
    fn description(&self) -> &'static str {
        "FocusOtherPaneDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift(0x27)) // VK_RIGHT
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.group().panes.len() == 2
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let group = w.state.group_mut();
        group.active_pane = 1 - group.active_pane;
        w.invalidate();
    }
}
