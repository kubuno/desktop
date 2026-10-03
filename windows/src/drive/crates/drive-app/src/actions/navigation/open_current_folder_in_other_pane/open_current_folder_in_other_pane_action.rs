//! OpenCurrentFolderInOtherPane (mirrors OpenCurrentFolderInOtherPane/OpenCurrentFolderInOtherPaneAction.cs)

use crate::actions::navigation::navigate_other_pane;
use crate::actions::Action;
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

/// `OpenCurrentFolderInOtherPaneAction.cs`: the CURRENT folder in the other
/// pane.
pub struct OpenCurrentFolderInOtherPane;
impl Action for OpenCurrentFolderInOtherPane {
    fn label(&self) -> &'static str {
        "OpenCurrentFolderInOtherPane"
    }
    fn description(&self) -> &'static str {
        "OpenCurrentFolderInOtherPaneDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.group().panes.len() == 2
            && matches!(w.state.active().location, Location::Dir(_))
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let location = w.state.active().location.clone();
        navigate_other_pane(w, location);
    }
}
