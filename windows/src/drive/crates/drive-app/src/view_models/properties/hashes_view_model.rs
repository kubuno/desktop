//! `hashes_view_model` (mirrors `ViewModels/Properties/HashesViewModel.cs`,
//! item `HashInfoItem` inline) + `Files.Shared.Helpers.ChecksumHelpers`.
//!
//! Like the original, hashes are computed in a BACKGROUND TASK (never on the
//! UI thread: a large file would freeze the window), cancellable on close.
//! The worker reads the file ONCE, feeds all enabled algorithms in parallel,
//! then posts [`WM_APP_HASH_READY`] to wake up the Properties window, which
//! drains the results.
//!
//! Algorithms enabled by default (`HashesViewModel`: `ShowHashes`): CRC32,
//! MD5, SHA1, SHA256. SHA384/SHA512 are in the original's table but disabled
//! by default — not computed here (the algorithm picker,
//! `SelectAlgorithmsButton`, is a TODO).
//!
//! Output formats faithful to `ChecksumHelpers`: CRC32 in UPPERCASE HEX
//! (reversed bytes → `Convert.ToHexString`), MD5/SHA in lowercase
//! (`Convert.ToHexStringLower`).

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crc32fast::Hasher as Crc32Hasher;
use md5::{Digest, Md5};
use sha1::Sha1;
use sha2::Sha256;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

/// A batch of hashes is ready to be drained.
///
/// Registry of application messages `WM_APP (0x8000) + n` (cf.
/// `services/folder_search.rs`):
/// - `+1` `WM_APP_DIR_CHANGED`   - `+2` `WM_APP_OPS_PROGRESS`
/// - `+3` `WM_APP_SHELL_MENU`    - `+4` `WM_APP_GIT_DONE`
/// - `+5` `WM_APP_TAB_PREVIEW`   - `+6` `WM_APP_ICON_READY`
/// - `+7` `WM_APP_FOLDER_SIZE`   - `+8` `WM_APP_SEARCH_RESULT`
/// - `+9` `WM_APP_HASH_READY`    ← THIS module (first free value).
pub const WM_APP_HASH_READY: u32 = 0x8000 + 9;

/// The computed algorithms (the subset enabled by default in
/// `HashesViewModel.Hashes`).
const ALGORITHMS: [&str; 4] = ["CRC32", "MD5", "SHA1", "SHA256"];

/// A row of the hash list (`HashInfoItem`).
pub struct HashRow {
    pub algorithm: &'static str,
    /// Computed value (`HashValue`), or error message.
    pub value: Option<String>,
    /// Calculation in progress (`IsCalculating` → indeterminate ProgressBar).
    pub calculating: bool,
}

/// The state of the Hashes tab: the rows + the background worker.
pub struct HashesState {
    pub rows: Vec<HashRow>,
    /// Results deposited by the worker: (row index, value).
    results: Arc<Mutex<Vec<(usize, String)>>>,
    /// Cancellation flag (`_cancellationTokenSource.Cancel()` on Dispose).
    cancel: Arc<AtomicBool>,
    started: bool,
}

impl Default for HashesState {
    fn default() -> Self {
        Self::new()
    }
}

impl HashesState {
    pub fn new() -> Self {
        let rows = ALGORITHMS
            .iter()
            .map(|a| HashRow { algorithm: a, value: None, calculating: true })
            .collect();
        HashesState {
            rows,
            results: Arc::new(Mutex::new(Vec::new())),
            cancel: Arc::new(AtomicBool::new(false)),
            started: false,
        }
    }

    /// `true` while at least one row is still calculating (drives the anim timer).
    pub fn calculating(&self) -> bool {
        self.rows.iter().any(|r| r.calculating)
    }

    /// Starts the worker (once): reads the file and computes all
    /// algorithms, then posts `WM_APP_HASH_READY`.
    pub fn start(&mut self, path: &str, hwnd_raw: isize) {
        if self.started {
            return;
        }
        self.started = true;
        let path = path.to_string();
        let results = Arc::clone(&self.results);
        let cancel = Arc::clone(&self.cancel);
        std::thread::spawn(move || {
            compute(&path, &results, &cancel, hwnd_raw);
        });
    }

    /// Drains the worker's results into the rows (on `WM_APP_HASH_READY`).
    pub fn drain(&mut self) {
        let batch: Vec<(usize, String)> = std::mem::take(&mut *self.results.lock().unwrap());
        for (idx, value) in batch {
            if let Some(row) = self.rows.get_mut(idx) {
                row.value = Some(value);
                row.calculating = false;
            }
        }
    }

    /// `Dispose`: cancels the ongoing calculation (on window close).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for HashesState {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// The background computation: a single read pass feeds all hashes.
fn compute(
    path: &str,
    results: &Arc<Mutex<Vec<(usize, String)>>>,
    cancel: &Arc<AtomicBool>,
    hwnd_raw: isize,
) {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            // `IOException`: file open elsewhere → dedicated message.
            let key = if e.kind() == std::io::ErrorKind::PermissionDenied {
                "CalculationErrorFileIsOpen"
            } else {
                "CalculationError"
            };
            let msg = drive_localization::tr(key).to_string();
            let mut lock = results.lock().unwrap();
            for i in 0..ALGORITHMS.len() {
                lock.push((i, msg.clone()));
            }
            drop(lock);
            post(hwnd_raw);
            return;
        }
    };

    let mut reader = std::io::BufReader::new(file);
    let mut crc = Crc32Hasher::new();
    let mut md5 = Md5::new();
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut buf = [0u8; 64 * 1024];

    loop {
        if cancel.load(Ordering::Relaxed) {
            // Cancelled (`OperationCanceledException`): nothing to show.
            return;
        }
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let chunk = &buf[..n];
                crc.update(chunk);
                md5.update(chunk);
                sha1.update(chunk);
                sha256.update(chunk);
            }
            Err(_) => {
                let msg = drive_localization::tr("CalculationError").to_string();
                let mut lock = results.lock().unwrap();
                for i in 0..ALGORITHMS.len() {
                    lock.push((i, msg.clone()));
                }
                drop(lock);
                post(hwnd_raw);
                return;
            }
        }
    }

    if cancel.load(Ordering::Relaxed) {
        return;
    }

    // Formats faithful to `ChecksumHelpers`: CRC32 uppercase (reversed bytes →
    // MSB-first value = `{:08X}`), the rest in lowercase.
    let crc_hex = format!("{:08X}", crc.finalize());
    let md5_hex = hex::encode(md5.finalize());
    let sha1_hex = hex::encode(sha1.finalize());
    let sha256_hex = hex::encode(sha256.finalize());

    {
        let mut lock = results.lock().unwrap();
        lock.push((0, crc_hex));
        lock.push((1, md5_hex));
        lock.push((2, sha1_hex));
        lock.push((3, sha256_hex));
    }
    post(hwnd_raw);
}

fn post(hwnd_raw: isize) {
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd_raw as *mut _)), WM_APP_HASH_READY, WPARAM(0), LPARAM(0));
    }
}
