//! PinFolderToSidebar (mirrors PinFolderToSidebarAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::shell_verb;

use super::target;

/// `PinFolderToSidebarAction.cs`: `pintohome` shell verb, then Home
/// reloads (the Pinned pane follows `QuickAccessService`).
pub struct PinFolderToSidebar;
impl Action for PinFolderToSidebar {
    fn label(&self) -> &'static str {
        "PinFolderToSidebar"
    }
    fn description(&self) -> &'static str {
        "PinFolderToSidebarDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("Pin")
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        if let Some(path) = target(w, parameter) {
            shell_verb(&path, "pintohome");
            w.model = crate::data::items::HomeModel::load();
            w.invalidate();
        }
    }
}
