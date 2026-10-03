//! ToggleFullScreen (mirror of ToggleFullScreenAction.cs)

use crate::actions::{Action, HotKey, ToggleAction};
use crate::main_window::MainWindow;

/// `ToggleFullScreenAction.cs` (F11).
pub struct ToggleFullScreen;
impl Action for ToggleFullScreen {
    fn label(&self) -> &'static str {
        "FullScreen"
    }
    fn description(&self) -> &'static str {
        "ToggleFullScreenDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 0x7A, ctrl: false, shift: false, alt: false }) // F11
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.toggle_fullscreen();
    }
}
impl ToggleAction for ToggleFullScreen {
    fn is_on(&self, w: &MainWindow) -> bool {
        w.is_fullscreen()
    }
}
