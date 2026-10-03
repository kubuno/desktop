//! CloseOtherTabsSelected (mirror of CloseOtherTabsSelectedAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

/// `CloseOtherTabsSelectedAction.cs`.
pub struct CloseOtherTabsSelected;
impl Action for CloseOtherTabsSelected {
    fn label(&self) -> &'static str {
        "CloseOtherTabs"
    }
    fn description(&self) -> &'static str {
        "CloseOtherTabsSelectedDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.tabs.len() > 1
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        while w.state.active_tab + 1 < w.state.tabs.len() {
            let last = w.state.tabs.len() - 1;
            w.close_tab(last);
        }
        while w.state.active_tab > 0 {
            w.close_tab(0);
        }
    }
}
