//! Port of `Windows/Managers/WindowsFolderChangeWatcher.cs`.
//!
//! Watches a known folder for shell change notifications. A dedicated STA
//! thread hosts a message-only window registered with
//! `SHChangeNotifyRegister`; notifications are decoded with
//! `SHChangeNotification_Lock` and relayed as [`FolderChangeEvent`] values on
//! a tokio broadcast channel (replacing the C# `FolderChanged` event).

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;

use tokio::sync::broadcast;
use windows::core::{GUID, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    SHChangeNotification_Lock, SHChangeNotification_Unlock, SHChangeNotifyDeregister,
    SHChangeNotifyEntry, SHChangeNotifyRegister, SHGetKnownFolderIDList, SHGetPathFromIDListW,
    SHCNE_ID, SHCNRF_InterruptLevel, SHCNRF_NewDelivery, SHCNRF_ShellLevel,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetWindowLongPtrW,
    PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SetWindowLongPtrW,
    TranslateMessage, UnregisterClassW, GWLP_USERDATA, HWND_MESSAGE, MSG, WINDOW_EX_STYLE,
    WM_CLOSE, WM_DESTROY, WNDCLASSEXW, WS_OVERLAPPED,
};

const WINDOW_CLASS_PREFIX: &str = "DriveFolderChangeNotificationWindow";

/// Port of `WindowsFolderChangeEventArgs`.
#[derive(Debug, Clone)]
pub struct FolderChangeEvent {
    /// The `SHCNE_ID` event mask value.
    pub change_type: SHCNE_ID,
    pub path: Option<String>,
    pub other_path: Option<String>,
}

/// Per-window state read by the window procedure.
struct WatcherState {
    notification_message: u32,
    events: broadcast::Sender<FolderChangeEvent>,
}

/// Port of `WindowsFolderChangeWatcher`.
pub struct WindowsFolderChangeWatcher {
    known_folder_id: GUID,
    change_mask: SHCNE_ID,
    recursive: bool,
    events: broadcast::Sender<FolderChangeEvent>,
    hwnd: Arc<AtomicIsize>,
    thread: Option<std::thread::JoinHandle<()>>,
    start_count: i32,
}

impl WindowsFolderChangeWatcher {
    pub fn new(known_folder_id: GUID, change_mask: SHCNE_ID, recursive: bool) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            known_folder_id,
            change_mask,
            recursive,
            events,
            hwnd: Arc::new(AtomicIsize::new(0)),
            thread: None,
            start_count: 0,
        }
    }

    /// Subscribes to folder change notifications (replaces `FolderChanged`).
    pub fn subscribe(&self) -> broadcast::Receiver<FolderChangeEvent> {
        self.events.subscribe()
    }

    /// Port of `Start` — ref-counted like the C# version; the first call
    /// spawns the notification thread.
    pub fn start(&mut self) {
        self.start_count += 1;
        if self.start_count != 1 || self.thread.is_some() {
            return;
        }

        let known_folder_id = self.known_folder_id;
        let change_mask = self.change_mask;
        let recursive = self.recursive;
        let events = self.events.clone();
        let hwnd_slot = Arc::clone(&self.hwnd);

        let thread = std::thread::Builder::new()
            .name("files-folder-change-watcher".into())
            .spawn(move || watcher_thread(known_folder_id, change_mask, recursive, events, hwnd_slot))
            .expect("failed to spawn folder change watcher thread");

        self.thread = Some(thread);
    }

    /// Port of `Stop`.
    pub fn stop(&mut self) {
        if self.start_count <= 0 {
            return;
        }
        self.start_count -= 1;
        if self.start_count != 0 {
            return;
        }
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let hwnd = self.hwnd.swap(0, Ordering::SeqCst);
        if hwnd != 0 {
            // SAFETY: the window is alive as long as the slot is non-zero; a
            // stale post is harmless.
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for WindowsFolderChangeWatcher {
    /// Port of `Dispose` — RAII replaces the C# disposer.
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Body of the notification thread: message-only window + shell registration
/// + message loop.
fn watcher_thread(
    known_folder_id: GUID,
    change_mask: SHCNE_ID,
    recursive: bool,
    events: broadcast::Sender<FolderChangeEvent>,
    hwnd_slot: Arc<AtomicIsize>,
) {
    // SAFETY: dedicated thread; every acquired resource is released before
    // returning, in reverse order.
    unsafe {
        let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let class_name = format!("{WINDOW_CLASS_PREFIX}_{:x}", std::process::id() as u64 ^ (&events as *const _ as u64));
        let class_name = HSTRING::from(class_name.as_str());
        let hinstance: windows::Win32::Foundation::HINSTANCE =
            GetModuleHandleW(PCWSTR::null()).unwrap_or_default().into();

        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            hInstance: hinstance,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            lpfnWndProc: Some(wnd_proc),
            ..Default::default()
        };
        if RegisterClassExW(&wnd_class) == 0 {
            tracing::warn!("failed to register the folder change watcher window class");
            if com.is_ok() {
                CoUninitialize();
            }
            return;
        }

        let notification_message = RegisterWindowMessageW(PCWSTR(class_name.as_ptr()));

        let state = Box::new(WatcherState { notification_message, events: events.clone() });

        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR::null(),
            WS_OVERLAPPED,
            0,
            0,
            1,
            1,
            // Message-only window: never visible, receives no broadcast spam.
            Some(HWND_MESSAGE),
            None,
            Some(hinstance),
            None,
        ) {
            Ok(hwnd) => hwnd,
            Err(error) => {
                tracing::warn!(?error, "failed to create the folder change watcher window");
                let _ = UnregisterClassW(PCWSTR(class_name.as_ptr()), Some(hinstance));
                if com.is_ok() {
                    CoUninitialize();
                }
                return;
            }
        };

        let state_ptr = Box::into_raw(state);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
        hwnd_slot.store(hwnd.0 as isize, Ordering::SeqCst);

        // Register the shell notifications (port of RegisterShellNotifications).
        let mut registration_id = 0u32;
        let mut folder_pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        match SHGetKnownFolderIDList(&known_folder_id, 0, None) {
            Ok(pidl) if !pidl.is_null() => {
                folder_pidl = pidl;
                let entry = SHChangeNotifyEntry { pidl: folder_pidl, fRecursive: recursive.into() };
                registration_id = SHChangeNotifyRegister(
                    hwnd,
                    SHCNRF_ShellLevel | SHCNRF_InterruptLevel | SHCNRF_NewDelivery,
                    change_mask.0 as i32,
                    notification_message,
                    1,
                    &entry,
                );
            }
            _ => {
                tracing::warn!(?known_folder_id, "failed to resolve the known folder id list");
            }
        }

        // Message loop until WM_CLOSE → WM_DESTROY → PostQuitMessage.
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Port of UnregisterShellNotifications + Dispose.
        hwnd_slot.store(0, Ordering::SeqCst);
        if registration_id != 0 {
            let _ = SHChangeNotifyDeregister(registration_id);
        }
        if !folder_pidl.is_null() {
            CoTaskMemFree(Some(folder_pidl as _));
        }
        drop(Box::from_raw(state_ptr));
        let _ = UnregisterClassW(PCWSTR(class_name.as_ptr()), Some(hinstance));
        if com.is_ok() {
            CoUninitialize();
        }
    }
}

/// Port of `WndProc` + `ProcessShellNotification`.
extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: GWLP_USERDATA either is null (before initialization) or points
    // to the `WatcherState` owned by the watcher thread, which outlives the
    // window.
    unsafe {
        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WatcherState;
        if let Some(state) = state_ptr.as_ref() {
            if msg == state.notification_message {
                process_shell_notification(state, wparam, lparam);
                return LRESULT(0);
            }
        }

        if msg == WM_DESTROY {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        let _ = msg == WM_CLOSE; // handled by DefWindowProc (DestroyWindow)

        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

/// Decodes a `SHCNRF_NewDelivery` notification and broadcasts it.
///
/// # Safety
/// Must only be called with the `wparam`/`lparam` of the registered
/// notification message.
unsafe fn process_shell_notification(state: &WatcherState, wparam: WPARAM, lparam: LPARAM) {
    let mut ppidls: *mut *mut ITEMIDLIST = std::ptr::null_mut();
    let mut event_id: i32 = 0;

    let lock = SHChangeNotification_Lock(
        HANDLE(wparam.0 as _),
        lparam.0 as u32,
        Some(&mut ppidls),
        Some(&mut event_id),
    );
    if lock.is_invalid() {
        return;
    }

    let (path, other_path) = if ppidls.is_null() {
        (None, None)
    } else {
        (path_from_pidl(*ppidls), path_from_pidl(*ppidls.add(1)))
    };

    let _ = state.events.send(FolderChangeEvent {
        change_type: SHCNE_ID(event_id as u32),
        path,
        other_path,
    });

    let _ = SHChangeNotification_Unlock(lock);
}

/// Port of `GetPathFromPidl`.
///
/// # Safety
/// `pidl` must be null or a valid item id list.
unsafe fn path_from_pidl(pidl: *mut ITEMIDLIST) -> Option<String> {
    if pidl.is_null() {
        return None;
    }
    let mut buffer = [0u16; 260];
    if !SHGetPathFromIDListW(pidl, &mut buffer).as_bool() {
        return None;
    }
    let length = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..length]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Shell::SHCNE_ALLEVENTS;

    #[test]
    fn start_stop_lifecycle() {
        // FOLDERID_Recent — watching it must start and stop cleanly.
        let mut watcher = WindowsFolderChangeWatcher::new(
            crate::guids::FOLDERID_RECENT,
            SHCNE_ALLEVENTS,
            false,
        );
        let _rx = watcher.subscribe();
        watcher.start();
        // Give the thread a moment to set up its window and registration.
        std::thread::sleep(std::time::Duration::from_millis(300));
        watcher.stop();
        assert!(watcher.thread.is_none(), "thread should be joined after stop");
    }
}
