//! CloseActivePane (mirrors CloseActivePaneAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `CloseActivePaneAction.cs` (Ctrl+Alt+W): closes the active pane.
pub struct CloseActivePane;
impl Action for CloseActivePane {
    fn label(&self) -> &'static str {
        "CloseActivePane"
    }
    fn description(&self) -> &'static str {
        "CloseActivePaneDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_alt('W' as u32))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.group().panes.len() == 2
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.state.group_mut().close_active_pane();
        w.invalidate();
    }
}
