//! DuplicateSelectedTab (mirrors DuplicateSelectedTabAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::{Location, TabGroup};

/// `DuplicateSelectedTabAction.cs` (Ctrl+Shift+K): same location, inserted
/// right after the current tab (`atIndex: SelectedTabIndex + 1`).
pub struct DuplicateSelectedTab;
impl Action for DuplicateSelectedTab {
    fn label(&self) -> &'static str {
        "DuplicateTab"
    }
    fn description(&self) -> &'static str {
        "DuplicateSelectedTabDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift('K' as u32))
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let location = w.state.active().location.clone();
        let mut group = TabGroup::new_home();
        if location != Location::Home {
            group.active_mut().navigate(location);
        }
        let i = w.state.active_tab + 1;
        w.state.tabs.insert(i, group);
        w.state.active_tab = i;
        w.invalidate();
    }
}
