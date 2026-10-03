//! CreateFolderWithSelection (mirrors CreateFolderWithSelectionAction.cs)

use super::{selected_path, target};
use crate::actions::Action;
use crate::main_window::MainWindow;

/// `CreateFolderWithSelectionAction.cs`: the folder takes the item's name
/// (without extension), and the item is moved into it.
pub struct CreateFolderWithSelection;
impl Action for CreateFolderWithSelection {
    fn label(&self) -> &'static str {
        "CreateFolderWithSelection"
    }
    fn description(&self) -> &'static str {
        "CreateFolderWithSelectionDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        selected_path(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        let Some(path) = target(w, parameter) else { return };
        let source = std::path::PathBuf::from(&path);
        if let (Some(parent), Some(stem)) = (source.parent(), source.file_stem()) {
            let dir = parent.join(stem);
            if std::fs::create_dir_all(&dir).is_ok() {
                if let Some(name) = source.file_name() {
                    let _ = std::fs::rename(&source, dir.join(name));
                }
                w.state.active_mut().refresh();
                w.invalidate();
            }
        }
    }
}
