//! DecompressArchiveToChildFolder (mirror of DecompressArchiveToChildFolderAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

use super::base_decompress_archive_action::{decompress, is_archive_selected};

/// `DecompressArchiveToChildFolderAction.cs`.
pub struct DecompressArchiveToChildFolder;
impl Action for DecompressArchiveToChildFolder {
    fn label(&self) -> &'static str {
        "ExtractToChildFolder"
    }
    fn description(&self) -> &'static str {
        "DecompressArchiveToChildFolderDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        is_archive_selected(w)
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        decompress(w, parameter, false);
    }
}
