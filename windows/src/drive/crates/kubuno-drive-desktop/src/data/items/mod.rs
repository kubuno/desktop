//! Port of `Files.App/Data/Items/`: the Home page's data — quick access,
//! drives, recent files (`DirEntryItem` ≈ `ListedItem`, `DriveItem`,
//! `WidgetsListedItem`).

use windows::core::PWSTR;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT};

pub mod branch_item;
pub mod drive_item;
pub mod home_model;
pub mod listed_item;
pub mod quick_access_item;
pub mod shelf_item;
pub mod shell_new_entry;
pub mod shell_new_entry_extensions;
pub mod widget_recent_item;

pub use branch_item::BranchItem;
pub use drive_item::{load_drives, DriveItem};
pub use home_model::HomeModel;
pub use listed_item::{load_directory, DirEntryItem};
pub use quick_access_item::{load_quick_access, QuickAccessItem, QuickAccessKind};
pub use shelf_item::ShelfItem;
pub use shell_new_entry_extensions::create_from_template;
pub use widget_recent_item::{load_recent_files, RecentFileItem};

/// The path of a known folder (`SHGetKnownFolderPath`). Helper shared by
/// quick access and recent files.
pub(crate) fn known_folder_path(id: &windows::core::GUID) -> Option<String> {
    unsafe {
        let pwstr: PWSTR = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = pwstr.to_string().ok();
        CoTaskMemFree(Some(pwstr.as_ptr() as *const _));
        path
    }
}

/// Formats a byte count the way Files does in French Windows:
/// "137,95 Gio", "1,13 Tio", "35,11 Mio".
pub fn format_bytes_fr(bytes: u64) -> String {
    const UNITS: [(&str, f64); 5] = [
        ("Tio", 1024f64 * 1024.0 * 1024.0 * 1024.0),
        ("Gio", 1024f64 * 1024.0 * 1024.0),
        ("Mio", 1024f64 * 1024.0),
        ("Kio", 1024f64),
        ("octet(s)", 1.0),
    ];
    for (unit, factor) in UNITS {
        if bytes as f64 >= factor {
            let value = bytes as f64 / factor;
            return format!("{} {unit}", format!("{value:.2}").replace('.', ","));
        }
    }
    "0 octet(s)".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_formatting_matches_screenshot_style() {
        assert_eq!(format_bytes_fr(148_143_000_000), "137,97 Gio");
        assert_eq!(format_bytes_fr(0), "0 octet(s)");
        assert!(format_bytes_fr(1_242_000_000_000).starts_with("1,1"));
    }
}
