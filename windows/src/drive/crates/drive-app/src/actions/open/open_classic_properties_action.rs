//! OpenClassicProperties (mirrors OpenClassicPropertiesAction.cs)

use super::target;
use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::{shell_verb, Location};

/// `OpenClassicPropertiesAction.cs` (Alt+Shift+Enter): the shell's CLASSIC
/// Properties dialog — selection if any, otherwise the current folder.
pub struct OpenClassicProperties;
impl Action for OpenClassicProperties {
    fn label(&self) -> &'static str {
        "OpenClassicProperties"
    }
    fn description(&self) -> &'static str {
        "OpenClassicPropertiesDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("Properties")
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 0x0D, ctrl: false, shift: true, alt: true }) // Alt+Shift+Enter
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        matches!(w.state.active().location, Location::Dir(_))
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        let path = target(w, parameter).or_else(|| match &w.state.active().location {
            Location::Dir(p) => Some(p.to_string_lossy().into_owned()),
            _ => None,
        });
        if let Some(path) = path {
            shell_verb(&path, "properties");
        }
    }
}
