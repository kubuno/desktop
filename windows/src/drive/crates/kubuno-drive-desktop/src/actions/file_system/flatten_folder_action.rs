//! FlattenFolder (mirrors FlattenFolderAction.cs)

use super::target;
use crate::actions::Action;
use crate::main_window::MainWindow;

/// `FlattenFolderAction.cs`: moves subfolder contents up to this level.
pub struct FlattenFolder;
impl Action for FlattenFolder {
    fn label(&self) -> &'static str {
        "FlattenFolder"
    }
    fn description(&self) -> &'static str {
        "FlattenFolderDescription"
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        if let Some(path) = target(w, parameter) {
            crate::utils::storage::flatten_folder(&path);
            w.state.active_mut().refresh();
            w.invalidate();
        }
    }
}
