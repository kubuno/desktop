//! OpenInNewWindow (mirror of OpenInNewWindow/OpenInNewWindowAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

/// `OpenInNewWindow/OpenInNewWindowAction.cs`.
pub struct OpenInNewWindow;
impl Action for OpenInNewWindow {
    fn label(&self) -> &'static str {
        "OpenInNewWindow"
    }
    fn description(&self) -> &'static str {
        "OpenInNewWindowDescription"
    }
    fn execute(&self, _w: &mut MainWindow, parameter: Option<&str>) {
        if let (Ok(exe), Some(path)) = (std::env::current_exe(), parameter) {
            let _ = std::process::Command::new(exe).arg(path).spawn();
        }
    }
}
