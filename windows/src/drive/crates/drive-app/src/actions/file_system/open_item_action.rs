//! OpenItem (mirrors OpenItemAction.cs)

use super::{selected_path, target};
use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `OpenItemAction.cs` (Enter): folder → navigation, file → shell.
pub struct OpenItem;
impl Action for OpenItem {
    fn label(&self) -> &'static str {
        "Open"
    }
    fn description(&self) -> &'static str {
        "OpenItemDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 0x0D, ctrl: false, shift: false, alt: false }) // VK_RETURN
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        selected_path(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        if let Some(path) = target(w, parameter) {
            if std::path::Path::new(&path).is_dir() {
                w.navigate_active(crate::view_models::shell_view_model::Location::Dir(path.into()));
            } else {
                crate::view_models::shell_view_model::shell_open(&path);
            }
        }
    }
}
