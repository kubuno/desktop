//! OpenTerminal (mirrors OpenTerminalAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

/// `OpenTerminalAction.cs` (Ctrl+`): Windows Terminal in the current folder.
pub struct OpenTerminal;
impl Action for OpenTerminal {
    fn label(&self) -> &'static str {
        "OpenTerminal"
    }
    fn description(&self) -> &'static str {
        "OpenTerminalDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl(0xC0)) // VK_OEM_3
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        let dir = parameter.map(str::to_owned).or_else(|| match &w.state.active().location {
            Location::Dir(p) => Some(p.to_string_lossy().into_owned()),
            _ => None,
        });
        if let Some(dir) = dir {
            let _ = std::process::Command::new("cmd")
                .args(["/c", "start", "wt.exe", "-d", &dir])
                .spawn();
        }
    }
}
