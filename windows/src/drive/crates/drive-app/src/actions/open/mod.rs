//! Port of `Files.App/Actions/Open/`.
//!
//! Module glue: one C# class = one `.rs` file. The shared free helpers
//! (`target`, `log_file_path`, `current_dir`, `git_root`) stay here
//! — on the C# side the logic is inline in each class or in
//! `Win32Helper`/`FilePropertiesHelpers` — and are imported by the action
//! files that use them.

use crate::actions::Action;
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

pub mod edit_in_notepad_action;
pub mod open_classic_properties_action;
pub mod open_command_palette_action;
pub mod open_in_ide_action;
pub mod open_log_file_action;
pub mod open_log_file_location_action;
pub mod open_properties_action;
pub mod open_repo_in_ide_action;
pub mod open_settings_action;
pub mod open_settings_file_action;
pub mod open_storage_sense_action;
pub mod open_terminal_action;
pub mod open_terminal_as_admin_action;

pub use edit_in_notepad_action::EditInNotepad;
pub use open_classic_properties_action::OpenClassicProperties;
pub use open_command_palette_action::OpenCommandPalette;
pub use open_in_ide_action::OpenInIDE;
pub use open_log_file_action::OpenLogFile;
pub use open_log_file_location_action::OpenLogFileLocation;
pub use open_properties_action::OpenProperties;
pub use open_repo_in_ide_action::OpenRepoInIDE;
pub use open_settings_action::OpenSettings;
pub use open_settings_file_action::OpenSettingsFile;
pub use open_storage_sense_action::OpenStorageSense;
pub use open_terminal_action::OpenTerminal;
pub use open_terminal_as_admin_action::OpenTerminalAsAdmin;

/// Shared helper (`target`, `selected`): the target path = the parameter if
/// given, otherwise the active tab's selected entry.
pub(super) fn target(w: &MainWindow, parameter: Option<&str>) -> Option<String> {
    parameter.map(str::to_owned).or_else(|| {
        let tab = w.state.active();
        tab.selected_entry().map(|e| e.path.clone())
    })
}

/// The application's `debug.log`, next to `settings.json`
/// (the original: `ApplicationData.Current.LocalFolder\debug.log`).
pub(crate) fn log_file_path() -> std::path::PathBuf {
    crate::services::settings::settings_file_path().with_file_name("debug.log")
}

/// The current folder, if any.
pub(super) fn current_dir(w: &MainWindow) -> Option<String> {
    match &w.state.active().location {
        Location::Dir(p) => Some(p.to_string_lossy().into_owned()),
        _ => None,
    }
}

/// The root of the git repo containing `dir` (walk up to `.git`).
pub(super) fn git_root(dir: &str) -> Option<std::path::PathBuf> {
    let mut cur = Some(std::path::Path::new(dir));
    while let Some(p) = cur {
        if p.join(".git").exists() {
            return Some(p.to_path_buf());
        }
        cur = p.parent();
    }
    None
}

/// "Open with" (application picker): `OpenAs_RunDLL`, like
/// `Win32Helper.OpenFileWithApplicationPicker`.
///
/// TRAP (cross-module): this action has NO counterpart in `Actions/Open`;
/// its C# class `OpenItemWithApplicationPickerAction` lives in
/// `Actions/FileSystem/OpenItemAction.cs`. It is kept here (rather than
/// split into an `open/` file) to preserve the API path
/// `crate::actions::open::OpenItemWithApplicationPicker` until the
/// `file_system` area takes it over.
pub struct OpenItemWithApplicationPicker;
impl Action for OpenItemWithApplicationPicker {
    fn label(&self) -> &'static str {
        "OpenWith"
    }
    fn description(&self) -> &'static str {
        "OpenItemWithApplicationPickerDescription"
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        if let Some(path) = target(w, parameter) {
            let _ = std::process::Command::new("rundll32.exe")
                .args(["shell32.dll,OpenAs_RunDLL", &path])
                .spawn();
        }
    }
}
