//! SortFilesAndFoldersTogether (mirror of `Files.App/Actions/Display/SortFilesAndFoldersTogetherAction.cs`).

use super::apply;
use crate::actions::{Action, ToggleAction};
use crate::main_window::MainWindow;

/// `SortFilesAndFoldersTogetherAction` : `IsOn = Alongside`.
pub struct SortFilesAndFoldersTogether;
impl Action for SortFilesAndFoldersTogether {
    fn label(&self) -> &'static str {
        "SortFilesAndFoldersTogether"
    }
    fn description(&self) -> &'static str {
        "SortFilesAndFoldersTogetherDescription"
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        apply(w, false, true);
    }
}
impl ToggleAction for SortFilesAndFoldersTogether {
    fn is_on(&self, w: &MainWindow) -> bool {
        w.state.active().sort_directories_alongside_files
    }
}
