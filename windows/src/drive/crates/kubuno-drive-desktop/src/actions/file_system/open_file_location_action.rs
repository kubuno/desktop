//! OpenFileLocation (mirrors OpenFileLocationAction.cs)

use super::target;
use crate::actions::Action;
use crate::main_window::MainWindow;

/// `OpenFileLocationAction.cs`: resolves the .lnk and navigates to its folder.
pub struct OpenFileLocation;
impl Action for OpenFileLocation {
    fn label(&self) -> &'static str {
        "OpenFileLocation"
    }
    fn description(&self) -> &'static str {
        "OpenFileLocationDescription"
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        if let Some(path) = target(w, parameter) {
            if let Some(t) = crate::utils::storage::resolve_shortcut(&path) {
                if let Some(parent) = std::path::Path::new(&t).parent() {
                    w.navigate_active(crate::view_models::shell_view_model::Location::Dir(
                        parent.to_path_buf(),
                    ));
                }
            }
        }
    }
}
