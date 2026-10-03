//! OpenInOtherPane (mirrors OpenInOtherPane/OpenInOtherPaneAction.cs)

use crate::actions::navigation::{navigate_other_pane, selected_dir};
use crate::actions::Action;
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

/// `OpenInOtherPaneAction.cs`: opens the selected folder in the other
/// (already open) pane.
pub struct OpenInOtherPane;
impl Action for OpenInOtherPane {
    fn label(&self) -> &'static str {
        "OpenInOtherPane"
    }
    fn description(&self) -> &'static str {
        "OpenDirectoryInOtherPaneDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.group().panes.len() == 2 && selected_dir(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        let dir = parameter.map(str::to_owned).or_else(|| selected_dir(w));
        if let Some(dir) = dir {
            navigate_other_pane(w, Location::Dir(dir.into()));
        }
    }
}
