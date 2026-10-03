//! InvertSelection (mirror of InvertSelectionAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

/// `InvertSelectionAction.cs`.
pub struct InvertSelection;
impl Action for InvertSelection {
    fn label(&self) -> &'static str {
        "InvertSelection"
    }
    fn description(&self) -> &'static str {
        "InvertSelectionDescription"
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.state.active_mut().invert_selection();
        w.invalidate();
    }
}
