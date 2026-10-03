//! SortFoldersFirst (mirror of `Files.App/Actions/Display/SortFoldersFirstAction.cs`).

use super::apply;
use crate::actions::{Action, ToggleAction};
use crate::main_window::MainWindow;

/// `SortFoldersFirstAction` : `IsOn = !FilesFirst && !Alongside`.
pub struct SortFoldersFirst;
impl Action for SortFoldersFirst {
    fn label(&self) -> &'static str {
        "SortFoldersFirst"
    }
    fn description(&self) -> &'static str {
        "SortFoldersFirstDescription"
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        apply(w, false, false);
    }
}
impl ToggleAction for SortFoldersFirst {
    fn is_on(&self, w: &MainWindow) -> bool {
        let tab = w.state.active();
        !tab.sort_files_first && !tab.sort_directories_alongside_files
    }
}
