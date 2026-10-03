//! FormatDrive (mirrors FormatDriveAction.cs)

use super::current_dir;
use crate::actions::Action;
use crate::main_window::MainWindow;

/// A drive ROOT path ("X:\"), non-system (not "C:\").
fn is_formattable_drive(path: &str) -> bool {
    let p = path.trim_end_matches('\\');
    p.len() == 2
        && p.as_bytes()[1] == b':'
        && p.as_bytes()[0].is_ascii_alphabetic()
        && !p.eq_ignore_ascii_case("C:")
}

/// `FormatDriveAction.cs`: opens the Windows format dialog on the drive
/// (`Win32Helper.OpenFormatDriveDialog`). Nothing is erased without the
/// user clicking through that dialog.
pub struct FormatDrive;
impl Action for FormatDrive {
    fn label(&self) -> &'static str {
        "FormatDriveText"
    }
    fn description(&self) -> &'static str {
        "FormatDriveDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        current_dir(w).is_some_and(|d| is_formattable_drive(&d))
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        let path = parameter.map(str::to_owned).or_else(|| current_dir(w));
        if let Some(path) = path {
            if is_formattable_drive(&path) {
                crate::helpers::win32::win32_helper_storage::open_format_drive_dialog(w.hwnd, &path);
            }
        }
    }
}
