//! SortFilesFirst (mirrors `Files.App/Actions/Display/SortFilesFirstAction.cs`).

use super::apply;
use crate::actions::{Action, ToggleAction};
use crate::main_window::MainWindow;

// Re-export of the two sibling structs to preserve the historical API
// `sort_files_first_action::{SortFoldersFirst, SortFilesAndFoldersTogether}`
// (used by actions/mod.rs, context_menus.rs and pointer_input.rs via the `sf` alias).
pub use super::sort_files_and_folders_together_action::SortFilesAndFoldersTogether;
pub use super::sort_folders_first_action::SortFoldersFirst;

/// `SortFilesFirstAction`: `IsOn = FilesFirst && !Alongside`.
pub struct SortFilesFirst;
impl Action for SortFilesFirst {
    fn label(&self) -> &'static str {
        "SortFilesFirst"
    }
    fn description(&self) -> &'static str {
        "SortFilesFirstDescription"
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        apply(w, true, false);
    }
}
impl ToggleAction for SortFilesFirst {
    fn is_on(&self, w: &MainWindow) -> bool {
        let tab = w.state.active();
        tab.sort_files_first && !tab.sort_directories_alongside_files
    }
}
