//! UserSizeProvider (mirror of `UserSizeProvider.cs`).
//!
//! Background orchestration: queuing a folder (`UpdateAsync` =
//! `request`), the worker thread (`spawn_worker`) that computes via
//! `CachedSizeProvider`'s `Calculate`, and reporting batches back to the UI
//! (`push` + `WM_APP_FOLDER_SIZE` message).

use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::cached_size_provider::{walk, SizeProvider, WalkContext};
use super::size_changed_event_args::SizeResult;

/// A batch of computed sizes is ready (WM_APP registry in thumbnails.rs).
pub const WM_APP_FOLDER_SIZE: u32 = 0x8000 + 7;

impl SizeProvider {
    /// `UpdateAsync`: queues the folder for the worker if it's neither
    /// cached nor already in progress.
    pub fn request(&mut self, path: &str, hwnd_raw: isize) {
        if self.cache.contains_key(path) || self.pending.contains(path) {
            return;
        }
        self.pending.insert(path.to_string());
        let sender = self
            .worker
            .get_or_insert_with(|| spawn_worker(Arc::clone(&self.results), hwnd_raw));
        let _ = sender.send(path.to_string());
    }
}

fn spawn_worker(
    results: Arc<Mutex<Vec<SizeResult>>>,
    hwnd_raw: isize,
) -> std::sync::mpsc::Sender<String> {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        while let Ok(root) = rx.recv() {
            let mut ctx = WalkContext {
                results: &results,
                hwnd_raw,
                last_post: std::time::Instant::now(),
                root: root.clone(),
            };
            let size = walk(&root, 0, &mut ctx);
            push(&results, hwnd_raw, vec![(root, size, true)]);
        }
    });
    tx
}

pub(super) fn push(results: &Arc<Mutex<Vec<SizeResult>>>, hwnd_raw: isize, batch: Vec<SizeResult>) {
    results.lock().unwrap().extend(batch);
    unsafe {
        let _ = PostMessageW(
            Some(HWND(hwnd_raw as *mut _)),
            WM_APP_FOLDER_SIZE,
            WPARAM(0),
            LPARAM(0),
        );
    }
}
