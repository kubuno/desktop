//! Per-directory change watcher — port of `ShellViewModel.WatchForDirectoryChanges`
//! (ReadDirectoryChangesW on a worker thread, notifying the UI thread).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY,
    FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::CancelIoEx;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

/// Message posted to the UI thread when a watched directory changed;
/// `wparam` carries the watcher id.
pub const WM_APP_DIR_CHANGED: u32 = 0x8000 + 1; // WM_APP + 1

pub struct DirWatcher {
    pub path: String,
    pub id: usize,
    handle_raw: isize,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl DirWatcher {
    /// Starts watching `path`; posts `WM_APP_DIR_CHANGED` with `id` to `hwnd`
    /// whenever contents change. Returns None if the directory can't be opened.
    pub fn start(path: String, hwnd: HWND, id: usize) -> Option<Self> {
        let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
        let handle = unsafe {
            CreateFileW(
                windows::core::PCWSTR(wide.as_ptr()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        }
        .ok()?;

        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let hwnd_raw = hwnd.0 as isize;
        let handle_raw = handle.0 as isize;

        let thread = std::thread::spawn(move || {
            let handle = HANDLE(handle_raw as *mut _);
            let mut buffer = vec![0u8; 16 * 1024];
            while !stop_thread.load(Ordering::Relaxed) {
                let mut returned = 0u32;
                // Synchronous: unblocks on the first change batch, or with an
                // error when Drop cancels the I/O.
                let ok = unsafe {
                    ReadDirectoryChangesW(
                        handle,
                        buffer.as_mut_ptr() as *mut _,
                        buffer.len() as u32,
                        false,
                        FILE_NOTIFY_CHANGE_FILE_NAME
                            | FILE_NOTIFY_CHANGE_DIR_NAME
                            | FILE_NOTIFY_CHANGE_ATTRIBUTES
                            | FILE_NOTIFY_CHANGE_SIZE
                            | FILE_NOTIFY_CHANGE_LAST_WRITE,
                        Some(&mut returned),
                        None,
                        None,
                    )
                };
                if ok.is_err() || stop_thread.load(Ordering::Relaxed) {
                    break;
                }
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(hwnd_raw as *mut _)),
                        WM_APP_DIR_CHANGED,
                        WPARAM(id),
                        LPARAM(0),
                    );
                }
            }
        });

        Some(Self {
            path,
            id,
            handle_raw,
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for DirWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let handle = HANDLE(self.handle_raw as *mut _);
        unsafe {
            // Unblock the pending ReadDirectoryChangesW, then release.
            let _ = CancelIoEx(handle, None);
            let _ = CloseHandle(handle);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
