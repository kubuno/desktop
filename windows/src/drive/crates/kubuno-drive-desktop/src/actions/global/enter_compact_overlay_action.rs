//! EnterCompactOverlay (mirror of EnterCompactOverlayAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `EnterCompactOverlayAction.cs` (Ctrl+Alt+Haut).
pub struct EnterCompactOverlay;
impl Action for EnterCompactOverlay {
    fn label(&self) -> &'static str {
        "EnterCompactOverlay"
    }
    fn description(&self) -> &'static str {
        "EnterCompactOverlayDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_alt(0x26)) // VK_UP
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        !w.is_compact_overlay()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        if !w.is_compact_overlay() {
            w.toggle_compact_overlay();
        }
    }
}
