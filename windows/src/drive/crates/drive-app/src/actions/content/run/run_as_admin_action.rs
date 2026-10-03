//! RunAsAdmin (mirror of RunAsAdminAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::shell_verb;

use super::base_run_as_action::target;

/// `RunAsAdminAction.cs` (verbe `runas`).
pub struct RunAsAdmin;
impl Action for RunAsAdmin {
    fn label(&self) -> &'static str {
        "RunAsAdministrator"
    }
    fn description(&self) -> &'static str {
        "RunAsAdminDescription"
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        if let Some(path) = target(w, parameter) {
            shell_verb(&path, "runas");
        }
    }
}
