//! OpenTerminalAsAdmin (mirrors OpenTerminalAsAdminAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

/// `OpenTerminalAsAdminAction.cs` (Ctrl+Shift+ù): elevated Windows Terminal
/// in the current folder (`runas` on `wt.exe`).
pub struct OpenTerminalAsAdmin;
impl Action for OpenTerminalAsAdmin {
    fn label(&self) -> &'static str {
        "OpenTerminalAsAdmin"
    }
    fn description(&self) -> &'static str {
        "OpenTerminalAsAdminDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift(0xC0)) // VK_OEM_3
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        matches!(w.state.active().location, Location::Dir(_))
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        let dir = parameter.map(str::to_owned).or_else(|| match &w.state.active().location {
            Location::Dir(p) => Some(p.to_string_lossy().into_owned()),
            _ => None,
        });
        if let Some(dir) = dir {
            use windows::core::HSTRING;
            use windows::Win32::UI::Shell::ShellExecuteW;
            use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
            let op = HSTRING::from("runas");
            let exe = HSTRING::from("wt.exe");
            let args = HSTRING::from(format!("-d \"{dir}\""));
            unsafe {
                ShellExecuteW(Some(w.hwnd), &op, &exe, &args, None, SW_SHOWNORMAL);
            }
        }
    }
}
