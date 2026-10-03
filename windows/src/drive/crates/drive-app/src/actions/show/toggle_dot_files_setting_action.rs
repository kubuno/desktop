//! Port of `Files.App/Actions/Show/ToggleDotFilesSettingAction.cs`.

use crate::actions::{Action, ToggleAction};
use crate::main_window::MainWindow;

pub struct ToggleDotFilesSetting;

impl Action for ToggleDotFilesSetting {
    fn label(&self) -> &'static str {
        "ShowDotFiles"
    }
    fn description(&self) -> &'static str {
        "ToggleDotFilesSettingDescription"
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        crate::services::settings::update(|s| s.show_dot_files = !s.show_dot_files);
        // Global setting: all open folders re-list.
        w.refresh_all_dirs();
        w.invalidate();
    }
}

impl ToggleAction for ToggleDotFilesSetting {
    fn is_on(&self, _w: &MainWindow) -> bool {
        crate::services::settings::get().show_dot_files
    }
}
