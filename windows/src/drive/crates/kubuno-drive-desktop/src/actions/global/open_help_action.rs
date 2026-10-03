//! OpenHelp (mirror of OpenHelpAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `OpenHelpAction.cs` (F1): the online documentation.
pub struct OpenHelp;
impl Action for OpenHelp {
    fn label(&self) -> &'static str {
        "Help"
    }
    fn description(&self) -> &'static str {
        "OpenHelpDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 0x70, ctrl: false, shift: false, alt: false }) // F1
    }
    fn execute(&self, _w: &mut MainWindow, _parameter: Option<&str>) {
        crate::view_models::shell_view_model::shell_verb(
            crate::views::settings::about_page::DOCUMENTATION_URL,
            "open",
        );
    }
}
