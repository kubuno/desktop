//! ToggleSelect (mirrors ToggleSelectAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `ToggleSelectAction.cs` (Ctrl+Space): toggles the keyboard-focused item.
pub struct ToggleSelect;
impl Action for ToggleSelect {
    fn label(&self) -> &'static str {
        "ToggleSelect"
    }
    fn description(&self) -> &'static str {
        "ToggleSelectDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl(0x20)) // VK_SPACE
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.active().focused.is_some()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        if let Some(i) = w.state.active().focused {
            w.state.active_mut().toggle_select(i);
            w.invalidate();
        }
    }
}
