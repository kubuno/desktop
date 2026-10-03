//! ExitCompactOverlay (mirror of ExitCompactOverlayAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `ExitCompactOverlayAction.cs` (Ctrl+Alt+Bas).
pub struct ExitCompactOverlay;
impl Action for ExitCompactOverlay {
    fn label(&self) -> &'static str {
        "ExitCompactOverlay"
    }
    fn description(&self) -> &'static str {
        "ExitCompactOverlayDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_alt(0x28)) // VK_DOWN
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.is_compact_overlay()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        if w.is_compact_overlay() {
            w.toggle_compact_overlay();
        }
    }
}
