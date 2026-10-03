//! The notification-area icon: Shell_NotifyIcon instead of Tauri's tray.
//!
//! Same four entries as before — Synchroniser / Ouvrir le dossier / Afficher /
//! Quitter — because the window only hides on close and this menu is the one
//! way out of the app.
//!
//! The icon's callback reaches the window as [`WM_TRAY`], which the window
//! hands to [`on_message`] through its message hook (`Form::on_message`). The
//! menu is a MODAL system menu with its own message loop, so the window keeps
//! painting underneath it; the command chosen is handed back to the window.

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Private message the tray icon posts back to the window.
pub const WM_TRAY: u32 = WM_APP + 1;

const ID_SYNC: usize = 1;
const ID_FOLDER: usize = 2;
const ID_SHOW: usize = 3;
const ID_QUIT: usize = 4;

pub fn add(hwnd: HWND) {
    // SAFETY: `data` is a fully initialised NOTIFYICONDATAW living across the
    // call; the icon handle comes from our own module's resources.
    unsafe {
        let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
        let icon = LoadIconW(Some(instance.into()), windows::core::w!("app_icon")).unwrap_or_default();
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: icon,
            ..Default::default()
        };
        for (i, ch) in crate::Resources::tray_tip().encode_utf16().enumerate().take(127) {
            data.szTip[i] = ch;
        }
        let _ = Shell_NotifyIconW(NIM_ADD, &data);
    }
}

pub fn remove(hwnd: HWND) {
    // SAFETY: as in `add` — a plain, initialised structure passed by reference.
    unsafe {
        let data = NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, ..Default::default() };
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

/// What a tray menu entry asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    SyncNow,
    OpenFolder,
    Show,
    Quit,
}

/// The menu entry a `TrackPopupMenuEx` result names, `None` for a dismissal.
pub fn command_of(id: usize) -> Option<Command> {
    match id {
        ID_SYNC => Some(Command::SyncNow),
        ID_FOLDER => Some(Command::OpenFolder),
        ID_SHOW => Some(Command::Show),
        ID_QUIT => Some(Command::Quit),
        _ => None,
    }
}

/// The icon's callback: a left click brings the window back, a right click
/// opens the menu. `event` is the mouse message the icon reports (the
/// callback's `lParam`). Returns what the window must do.
pub fn on_message(hwnd: HWND, event: u32) -> Option<Command> {
    match event {
        WM_LBUTTONUP => Some(Command::Show),
        WM_RBUTTONUP => show_menu(hwnd),
        _ => None,
    }
}

/// Shows the menu at the pointer and returns the entry chosen.
fn show_menu(hwnd: HWND) -> Option<Command> {
    let entries = [
        (ID_SYNC, crate::Resources::tray_sync_now()),
        (ID_FOLDER, crate::Resources::tray_open_folder()),
        (ID_SHOW, crate::Resources::tray_show()),
    ];
    let quit = HSTRING::from(crate::Resources::tray_quit());
    // SAFETY: the menu is created, used and destroyed within this call; `hwnd`
    // is the host's own window, which owns the menu while it tracks; the
    // strings outlive the calls that read them.
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        let texts: Vec<HSTRING> = entries.iter().map(|(_, t)| HSTRING::from(*t)).collect();
        for ((id, _), text) in entries.iter().zip(&texts) {
            let _ = AppendMenuW(menu, MF_STRING, *id, PCWSTR(text.as_ptr()));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, PCWSTR(quit.as_ptr()));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Required by the docs: the owner must come forward, otherwise the menu
        // never dismisses when the user clicks elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenuEx(menu, (TPM_RETURNCMD | TPM_RIGHTBUTTON).0, pt.x, pt.y, hwnd, None);
        let _ = DestroyMenu(menu);
        command_of(cmd.0 as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_map_to_commands() {
        assert_eq!(command_of(ID_SYNC), Some(Command::SyncNow));
        assert_eq!(command_of(ID_FOLDER), Some(Command::OpenFolder));
        assert_eq!(command_of(ID_SHOW), Some(Command::Show));
        assert_eq!(command_of(ID_QUIT), Some(Command::Quit));
        // 0 is what TrackPopupMenuEx returns for a dismissal.
        assert_eq!(command_of(0), None);
    }

    #[test]
    fn a_left_click_shows_the_window() {
        assert_eq!(on_message(HWND::default(), WM_LBUTTONUP), Some(Command::Show));
        assert_eq!(on_message(HWND::default(), WM_MOUSEMOVE), None);
    }
}
