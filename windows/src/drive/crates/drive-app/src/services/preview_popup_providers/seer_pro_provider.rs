//! SeerProProvider (mirror of `SeerProProvider.cs`).
//!
//! Falls back to Seer / Seer Pro when QuickLook is absent: communicates via
//! `WM_COPYDATA` (`dwData = 5000`) to the `SeerWindowClass` window, tracking
//! the last sent path (`CurrentPath`) for the selection switch.

#![allow(dead_code)]

use std::sync::Mutex;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, IsWindowVisible, SendMessageW, WM_COPYDATA};

/// Last path sent to Seer while its window was visible
/// (`private string? CurrentPath;`), for selection tracking.
static SEER_CURRENT: Mutex<Option<String>> = Mutex::new(None);

/// `GetSeerWindowHandle()`: `FindWindow("SeerWindowClass", null)`.
fn seer_window() -> Option<SeerHwnd> {
    unsafe {
        FindWindowW(windows::core::w!("SeerWindowClass"), PCWSTR::null())
            .ok()
            .filter(|h| !h.is_invalid())
            .map(SeerHwnd)
    }
}

// Local wrapper to distinguish the Seer HWND from the pipe HANDLE.
struct SeerHwnd(windows::Win32::Foundation::HWND);

/// Seer's `DetectAvailability()`: the `SeerWindowClass` window exists.
pub(super) fn seer_available() -> bool {
    seer_window().is_some()
}

/// Seer's `TogglePreviewPopupAsync`: `SendMessage(WM_COPYDATA)` with
/// `COPYDATASTRUCT{ dwData = 5000, cbData = (path.Length + 1) * 2, lpData =
/// UTF-16 of the path }`. Updates `CurrentPath` based on window visibility
/// after sending.
pub(super) fn seer_toggle(path: &str) {
    let Some(SeerHwnd(hwnd)) = seer_window() else {
        return;
    };
    // `Marshal.StringToHGlobalUni`: NUL-terminated UTF-16 string.
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let cds = COPYDATASTRUCT {
        // `data.dwData = 5000u;`
        dwData: 5000,
        // `data.cbData = (uint)(path.Length + 1) * 2;` — trailing NUL included,
        // in bytes (2 per UTF-16 unit).
        cbData: (wide.len() * 2) as u32,
        lpData: wide.as_ptr() as *mut core::ffi::c_void,
    };
    unsafe {
        SendMessageW(
            hwnd,
            WM_COPYDATA,
            Some(WPARAM(0)),
            Some(LPARAM(&cds as *const _ as isize)),
        );
        // `CurrentPath = isVisible ? path : null;`
        let visible = IsWindowVisible(hwnd).as_bool();
        *SEER_CURRENT.lock().unwrap() = visible.then(|| path.to_string());
    }
}

/// Seer's `SwitchPreviewAsync` — simplified version (Seer is the fallback):
/// refreshes the popup if the window is visible and the path has changed.
///
/// NB: the original additionally reads the `tracking_file` setting (Corey\Seer
/// registry key or `uwp.ini`) to potentially CLOSE the popup when tracking is
/// disabled. Here we assume tracking is enabled (Seer's default behavior)
/// and skip this configuration read.
pub(super) fn seer_switch(path: &str) {
    let Some(SeerHwnd(hwnd)) = seer_window() else {
        return;
    };
    let visible = unsafe { IsWindowVisible(hwnd).as_bool() };
    let mut current = SEER_CURRENT.lock().unwrap();

    // `if (CurrentPath is not null && !isWindowVisible) { CurrentPath = null; return; }`
    if current.is_some() && !visible {
        *current = None;
        return;
    }
    // `if (CurrentPath is not null && path != CurrentPath && isWindowVisible) …`
    let changed = current.as_deref().is_some_and(|c| c != path);
    if current.is_some() && changed && visible {
        drop(current);
        seer_toggle(path);
    }
}
