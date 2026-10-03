//! ToggleCompactOverlay (mirror of ToggleCompactOverlayAction.cs)

use crate::actions::{Action, HotKey, ToggleAction};
use crate::main_window::MainWindow;

/// `ToggleCompactOverlayAction.cs` (F12).
pub struct ToggleCompactOverlay;
impl Action for ToggleCompactOverlay {
    fn label(&self) -> &'static str {
        "ToggleCompactOverlay"
    }
    fn description(&self) -> &'static str {
        "ToggleCompactOverlayDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 0x7B, ctrl: false, shift: false, alt: false }) // F12
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.toggle_compact_overlay();
    }
}
impl ToggleAction for ToggleCompactOverlay {
    fn is_on(&self, w: &MainWindow) -> bool {
        w.is_compact_overlay()
    }
}
