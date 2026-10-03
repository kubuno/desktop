//! Port of `Files.App/Actions/Show/ToggleShelfPaneAction.cs`.
//!
//! The original restricts the action to the Dev build (`IsExecutable`/
//! `IsAccessibleGlobally` guarded by `AppEnvironment.Dev`, with a TODO
//! "Remove when shelf feature is ready"). Here the Shelf is ported as a
//! full-fledged feature, so the action is always executable.

use crate::actions::{Action, ToggleAction};
use crate::main_window::MainWindow;

pub struct ToggleShelfPane;

impl Action for ToggleShelfPane {
    fn label(&self) -> &'static str {
        "ToggleShelfPane"
    }
    fn description(&self) -> &'static str {
        "ToggleShelfPaneDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("App.ThemedIcons.Shelf")
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        crate::services::settings::update(|s| s.show_shelf_pane = !s.show_shelf_pane);
        w.invalidate();
    }
}

impl ToggleAction for ToggleShelfPane {
    fn is_on(&self, _w: &MainWindow) -> bool {
        crate::services::settings::get().show_shelf_pane
    }
}
