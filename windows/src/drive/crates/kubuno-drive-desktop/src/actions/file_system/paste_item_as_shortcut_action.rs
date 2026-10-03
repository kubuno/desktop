//! PasteItemAsShortcut (mirrors PasteItemAsShortcutAction.cs)

use super::current_dir;
use crate::actions::Action;
use crate::main_window::MainWindow;

/// `PasteItemAsShortcutAction.cs`: the shortcut lands in the displayed
/// folder, like a regular "Paste".
pub struct PasteItemAsShortcut;
impl Action for PasteItemAsShortcut {
    fn label(&self) -> &'static str {
        "PasteShortcut"
    }
    fn description(&self) -> &'static str {
        "PasteShortcutDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        current_dir(w).is_some() && crate::utils::storage::clipboard_has_files()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        if let Some(dir) = current_dir(w) {
            if let Some((paths, _)) = crate::utils::storage::clipboard_get_files(w.hwnd) {
                for t in &paths {
                    let link = crate::utils::storage::shortcut_name(&dir, t);
                    crate::utils::storage::create_shortcut(t, &link);
                }
                w.state.active_mut().refresh();
                w.invalidate();
            }
        }
    }
}
