//! CreateShortcut (mirrors CreateShortcutAction.cs)

use super::target;
use crate::actions::Action;
use crate::main_window::MainWindow;

/// `CreateShortcutAction.cs`: "ItemName - Shortcut.lnk" next to it.
pub struct CreateShortcut;
impl Action for CreateShortcut {
    fn label(&self) -> &'static str {
        "CreateShortcut"
    }
    fn description(&self) -> &'static str {
        "CreateShortcutDescription"
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        if let Some(path) = target(w, parameter) {
            if let Some(parent) =
                std::path::Path::new(&path).parent().map(|p| p.to_string_lossy().into_owned())
            {
                let link = crate::utils::storage::shortcut_name(&parent, &path);
                crate::utils::storage::create_shortcut(&path, &link);
                w.state.active_mut().refresh();
                w.invalidate();
            }
        }
    }
}
