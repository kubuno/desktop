//! ClearSelection (mirror of ClearSelectionAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

/// `ClearSelectionAction.cs`.
pub struct ClearSelection;
impl Action for ClearSelection {
    fn label(&self) -> &'static str {
        "ClearSelection"
    }
    fn description(&self) -> &'static str {
        "ClearSelectionDescription"
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.state.active_mut().clear_selection();
        w.invalidate();
    }
}
