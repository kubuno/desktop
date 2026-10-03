//! NewTab (mirror of NewTabAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::TabGroup;

/// `NewTabAction.cs` (Ctrl+T).
pub struct NewTab;
impl Action for NewTab {
    fn label(&self) -> &'static str {
        "NewTab"
    }
    fn description(&self) -> &'static str {
        "NewTabDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl('T' as u32))
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.state.tabs.push(TabGroup::new_home());
        w.state.active_tab = w.state.tabs.len() - 1;
        w.invalidate();
    }
}
