//! Port of `Files.App/Actions/Show/ToggleFilterHeaderAction.cs` (Ctrl+Shift+F).
//! The port has no persistent filter header: the toggle opens/closes the
//! Omnibar's filter input (same gesture as the toolbar button).

use crate::actions::{Action, HotKey, ToggleAction};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

pub struct ToggleFilterHeader;

impl Action for ToggleFilterHeader {
    fn label(&self) -> &'static str {
        "ToggleFilterHeader"
    }
    fn description(&self) -> &'static str {
        "ToggleFilterHeaderDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("Filter")
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift('F' as u32))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        matches!(
            w.state.active().location,
            Location::Dir(_) | Location::RecycleBin | Location::SearchResults { .. }
        )
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let open = w
            .state
            .edit
            .as_ref()
            .is_some_and(|e| e.entry == crate::ui::EDIT_SEARCH);
        if open {
            w.state.edit = None;
            w.invalidate();
        } else {
            w.begin_filter_edit();
        }
    }
}

impl ToggleAction for ToggleFilterHeader {
    fn is_on(&self, w: &MainWindow) -> bool {
        w.state
            .edit
            .as_ref()
            .is_some_and(|e| e.entry == crate::ui::EDIT_SEARCH)
    }
}
