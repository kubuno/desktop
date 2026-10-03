//! EditPath (mirror of EditPathAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

/// `EditPathAction.cs` (Ctrl+L, Alt+D) : l'Omnibar passe en saisie de chemin.
pub struct EditPath;
impl Action for EditPath {
    fn label(&self) -> &'static str {
        "EditPath"
    }
    fn description(&self) -> &'static str {
        "EditPathDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl('L' as u32))
    }
    fn second_hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 'D' as u32, ctrl: false, shift: false, alt: true })
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let current = match &w.state.active().location {
            Location::Dir(p) => p.to_string_lossy().into_owned(),
            _ => String::new(),
        };
        w.state.edit = Some(crate::ui::EditState {
            entry: crate::ui::EDIT_PATH,
            caret: current.len(),
            anchor: 0,
            text: current,
        });
        w.update_path_suggestions();
        w.invalidate();
    }
}
