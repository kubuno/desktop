//! Rename (mirrors RenameAction.cs)

use super::{not_recycle, selected_path};
use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `RenameAction.cs` (F2): inline renaming.
pub struct Rename;
impl Action for Rename {
    fn label(&self) -> &'static str {
        "Rename"
    }
    fn description(&self) -> &'static str {
        "RenameDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("Rename")
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 0x71, ctrl: false, shift: false, alt: false }) // VK_F2
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        // `RenameAction.IsExecutable` excludes the Recycle Bin.
        not_recycle(w) && selected_path(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.begin_rename();
    }
}
