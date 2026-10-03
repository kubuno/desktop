//! Where the system thinks the caret is: the IME's composition and candidate windows, and the
//! Win32 system caret.
//!
//! The page canvas draws its own caret, so the system has no idea where text is being typed. Two
//! things need it:
//!
//! * **the IME** — a Japanese, Chinese or Korean composition window, or the Windows emoji panel,
//!   opens at the caret only if told where it is (`ImmSetCompositionWindow`,
//!   `ImmSetCandidateWindow` excluding the caret's line). The composed text itself arrives through
//!   `WM_CHAR` like any other character (the host's `InputEvent::Text`);
//! * **accessibility** — screen readers and the Magnifier follow the **system caret**
//!   (`GUITHREADINFO.rcCaret`), so an invisible one is created on focus and moved with ours
//!   (`CreateCaret` / `SetCaretPos`, never shown: ours is the one drawn).
//!
//! Coordinates come in client DIP and are converted with the window's scale.

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::Input::Ime::{
    ImmGetContext, ImmReleaseContext, ImmSetCandidateWindow, ImmSetCompositionWindow, CANDIDATEFORM, CFS_EXCLUDE, CFS_POINT, COMPOSITIONFORM,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::WindowsAndMessaging::{CreateCaret, DestroyCaret, SetCaretPos};

/// The window that has the keyboard focus on this thread (the document's window when the canvas
/// is focused) — the IME's and the caret's owner.
pub fn focus_window() -> Option<HWND> {
    let h = unsafe { GetFocus() };
    (!h.is_invalid()).then_some(h)
}

/// The document's window: the active window of this thread, else its largest visible top-level
/// window (the clipboard needs an owner even when the window is not in the foreground).
pub fn main_window() -> Option<HWND> {
    use windows::Win32::Foundation::{LPARAM, TRUE};
    use windows::core::BOOL;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
    use windows::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, GetWindowRect, IsWindowVisible};
    let active = unsafe { GetActiveWindow() };
    if !active.is_invalid() {
        return Some(active);
    }
    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
        let best = unsafe { &mut *(l.0 as *mut (isize, i64)) };
        if unsafe { IsWindowVisible(h) }.as_bool() {
            let mut r = RECT::default();
            if unsafe { GetWindowRect(h, &mut r) }.is_ok() {
                let area = i64::from(r.right - r.left) * i64::from(r.bottom - r.top);
                if area > best.1 {
                    *best = (h.0 as isize, area);
                }
            }
        }
        TRUE
    }
    let mut best: (isize, i64) = (0, 0);
    unsafe {
        let _ = EnumThreadWindows(GetCurrentThreadId(), Some(each), LPARAM(&mut best as *mut _ as isize));
    }
    (best.0 != 0).then_some(HWND(best.0 as *mut _))
}

/// Tells the IME where the caret is: `(x, y)` top of the caret, `h` its height, client DIP.
pub fn place_composition(hwnd: HWND, x: f32, y: f32, h: f32, scale: f32) {
    let px = |v: f32| (v * scale).round() as i32;
    unsafe {
        let himc = ImmGetContext(hwnd);
        if himc.is_invalid() {
            return;
        }
        let form = COMPOSITIONFORM { dwStyle: CFS_POINT, ptCurrentPos: POINT { x: px(x), y: px(y) }, rcArea: RECT::default() };
        let _ = ImmSetCompositionWindow(himc, &form);
        let cand = CANDIDATEFORM {
            dwIndex: 0,
            dwStyle: CFS_EXCLUDE,
            ptCurrentPos: POINT { x: px(x), y: px(y + h) },
            rcArea: RECT { left: px(x), top: px(y), right: px(x) + 1, bottom: px(y + h) },
        };
        let _ = ImmSetCandidateWindow(himc, &cand);
        let _ = ImmReleaseContext(hwnd, himc);
    }
}

/// The invisible system caret, kept where ours is drawn.
#[derive(Default)]
pub struct SystemCaret {
    owner: Option<isize>,
    size: (i32, i32),
}

impl SystemCaret {
    /// Moves (creating or resizing as needed) the system caret to `(x, y)`, `h` tall, client DIP.
    pub fn place(&mut self, hwnd: HWND, x: f32, y: f32, h: f32, scale: f32) {
        let px = |v: f32| (v * scale).round() as i32;
        let size = (1.max(px(1.0)), px(h).max(1));
        unsafe {
            if self.owner != Some(hwnd.0 as isize) || self.size != size {
                if self.owner.is_some() {
                    let _ = DestroyCaret();
                }
                if CreateCaret(hwnd, None, size.0, size.1).is_ok() {
                    self.owner = Some(hwnd.0 as isize);
                    self.size = size;
                } else {
                    self.owner = None;
                    return;
                }
            }
            let _ = SetCaretPos(px(x), px(y));
        }
    }

    /// Removes it (the canvas lost the focus).
    pub fn destroy(&mut self) {
        if self.owner.take().is_some() {
            unsafe {
                let _ = DestroyCaret();
            }
        }
    }
}
