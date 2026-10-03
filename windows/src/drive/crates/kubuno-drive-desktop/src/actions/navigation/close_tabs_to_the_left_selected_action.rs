//! CloseTabsToTheLeftSelected (mirror of CloseTabsToTheLeftSelectedAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

/// `CloseTabsToTheLeftSelectedAction.cs`.
pub struct CloseTabsToTheLeftSelected;
impl Action for CloseTabsToTheLeftSelected {
    fn label(&self) -> &'static str {
        "CloseTabsToTheLeft"
    }
    fn description(&self) -> &'static str {
        "CloseTabsToTheLeftSelectedDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.active_tab > 0
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        while w.state.active_tab > 0 {
            w.close_tab(0);
        }
    }
}
