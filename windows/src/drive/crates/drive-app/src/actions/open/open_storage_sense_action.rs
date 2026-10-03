//! OpenStorageSense (mirrors OpenStorageSenseAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

/// `OpenStorageSenseAction.cs`: opens Windows' "Storage Sense" settings
/// (`Launcher.LaunchUriAsync("ms-settings:storagesense")`), here via
/// `ShellExecuteW` on the URI.
pub struct OpenStorageSense;
impl Action for OpenStorageSense {
    fn label(&self) -> &'static str {
        "OpenStorageSense"
    }
    fn description(&self) -> &'static str {
        "OpenStorageSenseDescription"
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        use windows::core::HSTRING;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let op = HSTRING::from("open");
        let uri = HSTRING::from("ms-settings:storagesense");
        unsafe {
            ShellExecuteW(Some(w.hwnd), &op, &uri, None, None, SW_SHOWNORMAL);
        }
    }
}
