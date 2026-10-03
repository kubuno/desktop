//! InstallInfDriver (mirrors InstallInfDriverAction.cs)

// NOT WIRED YET. See the module docs of actions::content::install: not registered yet.
// The allow is scoped to this file and must go once it is wired.
#![allow(dead_code)]

use crate::actions::Action;
use crate::main_window::MainWindow;
use kubuno_drive_desktop_shared::helpers::file_extensions as fe;

use super::{not_recycle_bin, selected};

/// `InstallInfDriverAction.cs`.
///
/// - `Label`: `Strings.InstallDriver` → "Install driver".
/// - `Description`: `Strings.InstallInfDriverDescription`.
/// - `Glyph`: `new("")` (Segoe MDL2 glyph — no ThemedIcon).
/// - `IsExecutable`: `SelectedItems.Count == 1 && IsInfFile(...) &&
///   !RecycleBin && !ZipFolder` (a SINGLE .inf file).
///
/// `ExecuteAsync` fidelity: `Win32Helper.InstallInf(path)`
/// (`Helpers/Win32/Win32Helper.Storage.cs:669`) launches
/// `InfDefaultInstall.exe` with `Verb = "runas"`, `UseShellExecute = true`,
/// `CreateNoWindow = true` and `Arguments = "\"{filePath}\""`. The C# does it
/// for each selected item (`Task.WhenAll(...)`), even though `IsExecutable`
/// only allows a single item.
pub struct InstallInfDriver;
impl Action for InstallInfDriver {
    fn label(&self) -> &'static str {
        "InstallDriver"
    }
    fn description(&self) -> &'static str {
        "InstallInfDriverDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        let sel = w.state.active().selected_paths();
        sel.len() == 1 && fe::is_inf_file(Some(&sel[0])) && not_recycle_bin(w)
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        for path in selected(w, parameter) {
            install_inf(w, &path);
        }
    }
}

/// `Win32Helper.InstallInf`: elevated `InfDefaultInstall.exe` (`runas`), hidden
/// window, argument = quoted path.
fn install_inf(w: &MainWindow, file_path: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let op = HSTRING::from("runas");
    let exe = HSTRING::from("InfDefaultInstall.exe");
    let args = HSTRING::from(format!("\"{file_path}\""));
    unsafe {
        ShellExecuteW(Some(w.hwnd), &op, &exe, &args, None, SW_HIDE);
    }
}
