//! DecompressArchiveHere (mirror of DecompressArchiveHere.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

use super::base_decompress_archive_action::{decompress, is_archive_selected};

/// `DecompressArchiveHere.cs`.
pub struct DecompressArchiveHere;
impl Action for DecompressArchiveHere {
    fn label(&self) -> &'static str {
        "ExtractHere"
    }
    fn description(&self) -> &'static str {
        "DecompressArchiveHereDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        is_archive_selected(w)
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        decompress(w, parameter, true);
    }
}
