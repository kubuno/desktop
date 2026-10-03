//! NewWindow (mirror of NewWindowAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `NewWindowAction.cs` (Ctrl+N).
pub struct NewWindow;
impl Action for NewWindow {
    fn label(&self) -> &'static str {
        "NewWindow"
    }
    fn description(&self) -> &'static str {
        "NewWindowDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl('N' as u32))
    }
    fn execute(&self, _w: &mut MainWindow, _parameter: Option<&str>) {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).spawn();
        }
    }
}
