//! ArrangePanesHorizontally (mirrors ArrangePanesHorizontallyAction.cs)

use super::arrange_panes;
use crate::actions::{Action, ToggleAction};
use crate::main_window::MainWindow;

/// `ArrangePanesHorizontallyAction.cs`: stacked panes.
pub struct ArrangePanesHorizontally;
impl Action for ArrangePanesHorizontally {
    fn label(&self) -> &'static str {
        "ArrangePanesHorizontally"
    }
    fn description(&self) -> &'static str {
        "ArrangePanesHorizontallyDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("PanesHorizontal")
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.group().panes.len() == 2
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        arrange_panes(w, false);
    }
}
impl ToggleAction for ArrangePanesHorizontally {
    fn is_on(&self, _w: &MainWindow) -> bool {
        crate::services::settings::get().shell_pane_arrangement
            == crate::services::settings::ShellPaneArrangement::Horizontal
    }
}
