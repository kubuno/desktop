//! OpenInNewTab (mirrors OpenInNewTab/OpenInNewTabAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

/// `OpenInNewTab/OpenInNewTabAction.cs`: the menu target, in a tab.
pub struct OpenInNewTab;
impl Action for OpenInNewTab {
    fn label(&self) -> &'static str {
        "OpenInNewTab"
    }
    fn description(&self) -> &'static str {
        "OpenDirectoryInNewTabDescription"
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        if let Some(path) = parameter {
            w.open_tab_at(Location::Dir(path.into()));
        }
    }
}
