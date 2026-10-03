//! RunAsAnotherUser (mirror of RunAsAnotherUserAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::shell_verb;

use super::base_run_as_action::target;

/// `RunAsAnotherUserAction.cs` (verbe `runasuser`).
pub struct RunAsAnotherUser;
impl Action for RunAsAnotherUser {
    fn label(&self) -> &'static str {
        "BaseLayoutContextFlyoutRunAsAnotherUser.Text"
    }
    fn description(&self) -> &'static str {
        "RunAsAnotherUserDescription"
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        if let Some(path) = target(w, parameter) {
            shell_verb(&path, "runasuser");
        }
    }
}
