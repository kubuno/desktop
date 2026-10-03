//! Port de `Files.App/Actions/Show/ToggleInfoPaneAction.cs` (Ctrl+Alt+I).

use crate::actions::{Action, HotKey, ToggleAction};
use crate::main_window::MainWindow;

pub struct ToggleInfoPane;

impl Action for ToggleInfoPane {
    fn label(&self) -> &'static str {
        "ToggleInfoPane"
    }
    fn description(&self) -> &'static str {
        "ToggleInfoPaneDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_alt('I' as u32))
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        crate::services::settings::update(|s| s.show_info_pane = !s.show_info_pane);
        w.invalidate();
    }
}

impl ToggleAction for ToggleInfoPane {
    fn is_on(&self, _w: &MainWindow) -> bool {
        crate::services::settings::get().show_info_pane
    }
}
