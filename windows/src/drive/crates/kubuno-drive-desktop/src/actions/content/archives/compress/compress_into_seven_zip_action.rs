//! CompressIntoSevenZip (mirror of CompressIntoSevenZipAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

use super::base_compress_archive_action::compress;

/// `CompressIntoSevenZipAction.cs`.
pub struct CompressIntoSevenZip;
impl Action for CompressIntoSevenZip {
    fn label(&self) -> &'static str {
        "CompressIntoSevenZip"
    }
    fn description(&self) -> &'static str {
        "CompressIntoSevenZipDescription"
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        compress(w, parameter, true);
    }
}
