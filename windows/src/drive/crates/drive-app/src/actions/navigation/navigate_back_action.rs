//! NavigateBack (mirrors NavigateBackAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `NavigateBackAction.cs` (Alt+Left; Backspace as second — mouse buttons
/// 4/5 and GoBack will come with their own routing).
pub struct NavigateBack;
impl Action for NavigateBack {
    fn label(&self) -> &'static str {
        "Back"
    }
    fn description(&self) -> &'static str {
        "NavigateBackDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 0x25, ctrl: false, shift: false, alt: true }) // Alt+VK_LEFT
    }
    fn second_hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 0x08, ctrl: false, shift: false, alt: false }) // VK_BACK
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.active().can_go_back()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.state.active_mut().go_back();
        w.invalidate();
    }
}
