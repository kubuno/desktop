//! DriveHelpers (mirrors DriveHelpers.cs)
//!
//! Port of `Files.App/Utils/Storage/Helpers/DriveHelpers.cs` (`EjectDeviceAsync`).
//! NOTE: the C# counterpart lives under `Utils/Storage/Helpers/`; a strict
//! mirror would place it in `utils/storage/helpers/drive_helpers.rs`. It
//! stays here to preserve `crate::utils::drive_helpers::eject_drive`
//! (imported by `main_window/context_menus.rs`).
//!
//! EJECTION invokes the shell verb "eject" (like `ContextMenu.InvokeVerb`).

use windows::core::HSTRING;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// `DriveHelpers.EjectDeviceAsync`: the shell verb "eject" on the drive.
pub fn eject_drive(hwnd: HWND, drive_path: &str) {
    let verb = HSTRING::from("eject");
    let path = HSTRING::from(drive_path);
    unsafe {
        ShellExecuteW(Some(hwnd), &verb, &path, None, None, SW_SHOWNORMAL);
    }
}
