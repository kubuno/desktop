//! InstallFont (mirrors InstallFontAction.cs)

// NOT WIRED YET. See the module docs of actions::content::install: not registered yet.
// The allow is scoped to this file and must go once it is wired.
#![allow(dead_code)]

use crate::actions::Action;
use crate::main_window::MainWindow;
use kubuno_drive_desktop_shared::helpers::file_extensions as fe;

use super::{not_recycle_bin, selected};

/// `InstallFontAction.cs`.
///
/// - `Label`: `Strings.InstallFont.GetLocalizedResource()` → "Install font".
/// - `Description`: `Strings.InstallFontDescription` → "Install selected
///   {0, plural, one {font} other {fonts}}".
/// - `Glyph`: `themedIconStyle: "App.ThemedIcons.Actions.FontInstall"`.
/// - `IsExecutable`: `SelectedItems.Any() && All(IsFontFile) && !RecycleBin &&
///   !ZipFolder`.
///
/// `ExecuteAsync` fidelity: the original does NOT use an "install" shell
/// verb; it calls `Win32Helper.InstallFontsAsync(paths, false)`
/// (`Helpers/Win32/Win32Helper.Storage.cs:693`) which, for the current user
/// (`forAllUsers = false`), builds an **elevated and hidden** PowerShell
/// command that copies each font into `%LocalAppData%\Microsoft\Windows\Fonts`
/// then registers it under `HKCU:\Software\Microsoft\Windows NT\CurrentVersion\
/// Fonts`. We reproduce this exact command, launched via `runas`.
pub struct InstallFont;
impl Action for InstallFont {
    fn label(&self) -> &'static str {
        "InstallFont"
    }
    fn description(&self) -> &'static str {
        "InstallFontDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("FontInstall")
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        let sel = w.state.active().selected_paths();
        !sel.is_empty()
            && sel.iter().all(|p| fe::is_font_file(Some(p)))
            && not_recycle_bin(w)
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        let paths = selected(w, parameter);
        if paths.is_empty() {
            return;
        }

        // `Win32Helper.InstallFontsAsync(paths, forAllUsers: false)`:
        //   fontDirectory = %LocalAppData%\Microsoft\Windows\Fonts
        //   registryKey   = HKCU:\Software\Microsoft\Windows NT\CurrentVersion\Fonts
        let Ok(local_app_data) = std::env::var("LOCALAPPDATA") else { return };
        let font_directory =
            format!("{local_app_data}\\Microsoft\\Windows\\Fonts");
        let registry_key =
            "HKCU:\\Software\\Microsoft\\Windows NT\\CurrentVersion\\Fonts";

        // Faithful reconstruction of `psCommand` (C#'s StringBuilder).
        let mut ps_command = String::from("-command \"");
        for font in &paths {
            let path = std::path::Path::new(font);
            let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let destination_path = format!("{font_directory}\\{file_name}");
            // `$"Copy-Item '{fontFilePath}' '{fontDirectory}'; New-ItemProperty
            //   -Name '{name}' -Path '{registryKey}' -PropertyType string
            //   -Value '{destinationPath}';"`
            ps_command.push_str(&format!(
                "Copy-Item '{font}' '{font_directory}'; New-ItemProperty -Name '{stem}' -Path '{registry_key}' -PropertyType string -Value '{destination_path}';"
            ));
        }
        ps_command.push('"');

        // `PowerShellExecutionOptions.Elevated | Hidden`: UseShellExecute + `runas`
        // verb, hidden window.
        run_powershell_elevated_hidden(w, &ps_command);
    }
}

/// `RunPowershellCommandAsync(cmd, Elevated | Hidden)`: elevated `powershell.exe`
/// (`runas`), hidden window.
fn run_powershell_elevated_hidden(w: &MainWindow, command: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let op = HSTRING::from("runas");
    let exe = HSTRING::from("powershell.exe");
    let args = HSTRING::from(command);
    unsafe {
        ShellExecuteW(Some(w.hwnd), &op, &exe, &args, None, SW_HIDE);
    }
}
