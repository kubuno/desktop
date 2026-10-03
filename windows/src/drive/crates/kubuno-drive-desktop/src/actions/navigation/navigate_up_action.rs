//! NavigateUp (mirror of NavigateUpAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `NavigateUpAction.cs` (Alt+Haut).
pub struct NavigateUp;
impl Action for NavigateUp {
    fn label(&self) -> &'static str {
        "Up"
    }
    fn description(&self) -> &'static str {
        "NavigateUpDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 0x26, ctrl: false, shift: false, alt: true }) // Alt+VK_UP
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.active().can_go_up()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.state.active_mut().go_up();
        w.invalidate();
    }
}
