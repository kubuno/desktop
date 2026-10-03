//! OpenInNewPane (mirrors OpenInNewPane/OpenInNewPaneAction.cs)

use crate::actions::navigation::{navigate_other_pane, selected_dir};
use crate::actions::Action;
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

/// `OpenInNewPaneAction.cs`: opens the selected folder in a new pane.
pub struct OpenInNewPane;
impl Action for OpenInNewPane {
    fn label(&self) -> &'static str {
        "OpenInNewPane"
    }
    fn description(&self) -> &'static str {
        "OpenDirectoryInNewPaneDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        selected_dir(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        let dir = parameter.map(str::to_owned).or_else(|| selected_dir(w));
        if let Some(dir) = dir {
            navigate_other_pane(w, Location::Dir(dir.into()));
        }
    }
}
