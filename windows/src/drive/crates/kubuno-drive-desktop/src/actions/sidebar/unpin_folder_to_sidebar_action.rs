//! UnpinFolderFromSidebar (mirror of UnpinFolderToSidebarAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::shell_verb;

use super::target;

/// `UnpinFolderToSidebarAction.cs` : verbe shell `unpinfromhome`.
pub struct UnpinFolderFromSidebar;
impl Action for UnpinFolderFromSidebar {
    fn label(&self) -> &'static str {
        "UnpinFolderFromSidebar"
    }
    fn description(&self) -> &'static str {
        "UnpinFolderFromSidebarDescription"
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        if let Some(path) = target(w, parameter) {
            shell_verb(&path, "unpinfromhome");
            w.model = crate::data::items::HomeModel::load();
            w.invalidate();
        }
    }
}
