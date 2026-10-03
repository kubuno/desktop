//! CloseTabsToTheRightSelected (mirror of CloseTabsToTheRightSelectedAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

/// `CloseTabsToTheRightSelectedAction.cs`.
pub struct CloseTabsToTheRightSelected;
impl Action for CloseTabsToTheRightSelected {
    fn label(&self) -> &'static str {
        "CloseTabsToTheRight"
    }
    fn description(&self) -> &'static str {
        "CloseTabsToTheRightSelectedDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.active_tab + 1 < w.state.tabs.len()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        while w.state.active_tab + 1 < w.state.tabs.len() {
            let last = w.state.tabs.len() - 1;
            w.close_tab(last);
        }
    }
}
