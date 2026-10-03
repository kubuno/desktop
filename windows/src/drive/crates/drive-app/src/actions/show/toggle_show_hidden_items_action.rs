//! Port of `Files.App/Actions/Show/ToggleShowHiddenItemsAction.cs` (Ctrl+H).

use crate::actions::{Action, HotKey, ToggleAction};
use crate::main_window::MainWindow;

pub struct ToggleShowHiddenItems;

impl Action for ToggleShowHiddenItems {
    fn label(&self) -> &'static str {
        "HiddenItems"
    }
    fn description(&self) -> &'static str {
        "ToggleShowHiddenItemsDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl('H' as u32))
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        crate::services::settings::update(|s| s.show_hidden_items = !s.show_hidden_items);
        // The setting is global: all open folders re-list.
        w.refresh_all_dirs();
        w.invalidate();
    }
}

impl ToggleAction for ToggleShowHiddenItems {
    fn is_on(&self, _w: &MainWindow) -> bool {
        crate::services::settings::get().show_hidden_items
    }
}
