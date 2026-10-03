//! Port de `Files.App/Actions/Show/ToggleShowFileExtensionsAction.cs`.

use crate::actions::{Action, ToggleAction};
use crate::main_window::MainWindow;

pub struct ToggleShowFileExtensions;

impl Action for ToggleShowFileExtensions {
    fn label(&self) -> &'static str {
        "ShowFileExtensions"
    }
    fn description(&self) -> &'static str {
        "ToggleShowFileExtensionsDescription"
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        crate::services::settings::update(|s| s.show_file_extensions = !s.show_file_extensions);
        w.invalidate();
    }
}

impl ToggleAction for ToggleShowFileExtensions {
    fn is_on(&self, _w: &MainWindow) -> bool {
        crate::services::settings::get().show_file_extensions
    }
}
