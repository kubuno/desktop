//! EditInNotepad (mirror of EditInNotepadAction.cs)

use super::target;
use crate::actions::Action;
use crate::main_window::MainWindow;

/// `EditInNotepadAction.cs`.
pub struct EditInNotepad;
impl Action for EditInNotepad {
    fn label(&self) -> &'static str {
        "EditInNotepad"
    }
    fn description(&self) -> &'static str {
        "EditInNotepadDescription"
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        if let Some(path) = target(w, parameter) {
            let _ = std::process::Command::new("notepad.exe").arg(&path).spawn();
        }
    }
}
