//! CompressIntoZip (mirror of CompressIntoZipAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

use super::base_compress_archive_action::compress;

/// `CompressIntoZipAction.cs`.
pub struct CompressIntoZip;
impl Action for CompressIntoZip {
    fn label(&self) -> &'static str {
        "CompressIntoZip"
    }
    fn description(&self) -> &'static str {
        "CompressIntoZipDescription"
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        compress(w, parameter, false);
    }
}
