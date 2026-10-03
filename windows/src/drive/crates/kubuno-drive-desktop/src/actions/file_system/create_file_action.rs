//! CreateFile (mirrors CreateFileAction.cs)

use super::current_dir;
use crate::actions::Action;
use crate::main_window::MainWindow;

/// `CreateFileAction.cs`: the naming dialog for a new empty file.
pub struct CreateFile;
impl Action for CreateFile {
    fn label(&self) -> &'static str {
        "File"
    }
    fn description(&self) -> &'static str {
        "NewFile"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("File")
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        current_dir(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.open_create_item_dialog(false);
    }
}
