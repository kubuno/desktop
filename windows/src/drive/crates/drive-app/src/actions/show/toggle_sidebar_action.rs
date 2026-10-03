//! Port de `Files.App/Actions/Show/ToggleSidebarAction.cs` (Ctrl+B).

use crate::actions::{Action, HotKey, ToggleAction};
use crate::main_window::MainWindow;

pub struct ToggleSidebar;

impl Action for ToggleSidebar {
    fn label(&self) -> &'static str {
        "ToggleSidebar"
    }
    fn description(&self) -> &'static str {
        "ToggleSidebarDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl('B' as u32))
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.state.sidebar_visible = !w.state.sidebar_visible;
        w.invalidate();
    }
}

impl ToggleAction for ToggleSidebar {
    fn is_on(&self, w: &MainWindow) -> bool {
        w.state.sidebar_visible
    }
}
