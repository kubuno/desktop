//! Port de `Files.App/Actions/Show/ToggleToolbarAction.cs` (Ctrl+Maj+B).

use crate::actions::{Action, HotKey, ToggleAction};
use crate::main_window::MainWindow;

pub struct ToggleToolbar;

impl Action for ToggleToolbar {
    fn label(&self) -> &'static str {
        "ToggleToolbar"
    }
    fn description(&self) -> &'static str {
        "ToggleToolbarDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift('B' as u32))
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        crate::services::settings::update(|s| s.show_toolbar = !s.show_toolbar);
        w.invalidate();
    }
}

impl ToggleAction for ToggleToolbar {
    fn is_on(&self, _w: &MainWindow) -> bool {
        crate::services::settings::get().show_toolbar
    }
}
