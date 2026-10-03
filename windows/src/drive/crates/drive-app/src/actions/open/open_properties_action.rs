//! OpenProperties (mirrors OpenPropertiesAction.cs)

use super::target;
use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `OpenPropertiesAction.cs`: the shell's properties sheet.
pub struct OpenProperties;
impl Action for OpenProperties {
    fn label(&self) -> &'static str {
        "Properties"
    }
    fn description(&self) -> &'static str {
        "OpenPropertiesDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("Properties")
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 0x0D, ctrl: false, shift: false, alt: true }) // Alt+Enter
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        // `FilePropertiesHelpers.OpenPropertiesWindow`: our ported properties
        // window, in place of the classic shell dialog
        // (`OpenClassicProperties` keeps the native verb).
        use crate::views::properties::PropertiesTarget;
        let sel = w.state.active().selected_paths();
        let target = if sel.len() > 1 {
            Some(PropertiesTarget::Multi(sel))
        } else {
            target(w, parameter).map(|path| {
                if crate::main_window::is_drive_root(&path) {
                    PropertiesTarget::Drive(path.chars().next().unwrap_or('C'))
                } else {
                    PropertiesTarget::Path(path)
                }
            })
        };
        if let Some(target) = target {
            crate::views::properties::open_properties_window(w.hwnd, target);
        }
    }
}
