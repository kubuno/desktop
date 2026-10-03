//! CreateShortcutFromDialog (mirrors CreateShortcutFromDialogAction.cs)

use super::current_dir;
use crate::actions::Action;
use crate::main_window::MainWindow;

/// `CreateShortcutFromDialogAction.cs`: the `CreateShortcutDialog` (typed
/// target path).
pub struct CreateShortcutFromDialog;
impl Action for CreateShortcutFromDialog {
    fn label(&self) -> &'static str {
        "Shortcut"
    }
    fn description(&self) -> &'static str {
        "CreateShortcutFromDialogDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        current_dir(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.open_create_shortcut_dialog();
    }
}
