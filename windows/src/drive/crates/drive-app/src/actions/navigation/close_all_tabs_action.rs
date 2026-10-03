//! CloseAllTabs (mirrors CloseAllTabsAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `CloseAllTabsAction.cs` (Ctrl+Shift+W): closes all tabs — the last one
/// closes the window, like the original TabView.
pub struct CloseAllTabs;
impl Action for CloseAllTabs {
    fn label(&self) -> &'static str {
        "CloseAllTabs"
    }
    fn description(&self) -> &'static str {
        "CloseAllTabsDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift('W' as u32))
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        while w.state.tabs.len() > 1 {
            let last = w.state.tabs.len() - 1;
            w.close_tab(last);
        }
        w.close_tab(0);
    }
}
