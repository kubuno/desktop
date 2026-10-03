//! Win32Helper.Storage (mirrors Win32Helper.Storage.cs, drives part)
//!
//! Port of `Files.App/Helpers/Win32/Win32Helper.Storage.cs::OpenFormatDriveDialog`.
//!
//! FORMATTING opens Windows' standard dialog (`SHFormatDrive`) — nothing is
//! erased without the user's click inside that dialog.

use windows::Win32::Foundation::HWND;

// `SHFormatDrive` isn't in the windows-rs metadata: declare it.
#[link(name = "shell32")]
extern "system" {
    fn SHFormatDrive(hwnd: HWND, drive: u32, fmt_id: u32, options: u32) -> u32;
}

/// `Win32Helper.OpenFormatDriveDialog`: opens the drive formatting dialog.
/// `drive_path` starts with the letter (e.g. "E:\"). `SHFMT_ID_DEFAULT` +
/// `SHFMT_OPT_FULL`, exactly like the original.
pub fn open_format_drive_dialog(hwnd: HWND, drive_path: &str) {
    let Some(letter) = drive_path.chars().next().map(|c| c.to_ascii_uppercase()) else {
        return;
    };
    if !letter.is_ascii_alphabetic() {
        return;
    }
    let drive_index = (letter as u32) - ('A' as u32);
    const SHFMT_ID_DEFAULT: u32 = 0xFFFF;
    const SHFMT_OPT_FULL: u32 = 0x0001;
    unsafe {
        SHFormatDrive(hwnd, drive_index, SHFMT_ID_DEFAULT, SHFMT_OPT_FULL);
    }
}
