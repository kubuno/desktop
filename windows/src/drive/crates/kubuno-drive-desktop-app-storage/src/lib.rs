//! Windows storage implementations (Shell/Win32).
//! Rust port of `Files.App.Storage`.
//!
//! Module map (C# file → Rust module):
//! - `Windows/WindowsStorable|WindowsFile|WindowsFolder` → [`windows_storage`]
//! - `Windows/WindowsBulkOperations*` → [`bulk_operations`]
//! - `Windows/Helpers/WindowsStorableHelpers.{Shell,Storage}` → [`helpers`]
//! - `Windows/Helpers/WindowsStorableHelpers.Icon` → [`icons`]
//! - `Windows/Managers/WindowsFolderChangeWatcher` → [`watcher`]
//! - `Windows/Managers/STATask` → [`sta_thread`]
//! - `Legacy/HomeFolder` → [`home_folder`]
//! - `Ftp/*` → [`ftp`] (behind the `ftp` cargo feature)
//!
//! COM threading: shell-backed types require COM to be initialized on the
//! calling thread; interactive shell calls additionally require an STA (see
//! [`sta_thread`]).

pub mod bulk_operations;
pub mod guids;
pub mod helpers;
pub mod home_folder;
pub mod icons;
pub mod sta_thread;
pub mod watcher;
pub mod windows_storage;

#[cfg(feature = "ftp")]
pub mod ftp;

pub use bulk_operations::{BulkOperationsEvent, WindowsBulkOperations, DEFAULT_OPERATION_FLAGS};
pub use helpers::{WindowsContextMenuItem, WindowsContextMenuType};
pub use home_folder::HomeFolder;
pub use icons::ShellBitmap;
pub use watcher::{FolderChangeEvent, WindowsFolderChangeWatcher};
pub use windows_storage::{WindowsFile, WindowsFolder, WindowsStorable, WindowsStorableItem};

#[cfg(test)]
pub(crate) mod tests_support {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

    /// Initializes COM (STA) on the current test thread. Tests run on
    /// dedicated threads, so a per-thread init is enough; `S_FALSE`
    /// (already initialized) is ignored.
    pub fn init_com() {
        // SAFETY: initializing COM on the test thread; never uninitialized, as
        // the thread dies with the test.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
    }
}
