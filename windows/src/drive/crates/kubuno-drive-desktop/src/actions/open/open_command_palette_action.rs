//! OpenCommandPalette (mirror of OpenCommandPaletteAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `OpenCommandPaletteAction.cs` (Ctrl+Maj+P) : l'Omnibar passe en palette.
pub struct OpenCommandPalette;
impl Action for OpenCommandPalette {
    fn label(&self) -> &'static str {
        "CommandPalette"
    }
    fn description(&self) -> &'static str {
        "OpenCommandPaletteDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift('P' as u32))
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.state.edit = Some(crate::ui::EditState {
            entry: crate::ui::EDIT_PALETTE,
            caret: 0,
            anchor: 0,
            text: String::new(),
        });
        w.update_palette_suggestions();
        w.invalidate();
    }
}
