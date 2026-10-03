//! NavigateForward (mirror of NavigateForwardAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `NavigateForwardAction.cs` (Alt+Droite).
pub struct NavigateForward;
impl Action for NavigateForward {
    fn label(&self) -> &'static str {
        "Forward"
    }
    fn description(&self) -> &'static str {
        "NavigateForwardDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 0x27, ctrl: false, shift: false, alt: true }) // Alt+VK_RIGHT
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.active().can_go_forward()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.state.active_mut().go_forward();
        w.invalidate();
    }
}
