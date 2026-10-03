//! Search (mirror of SearchAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

/// `SearchAction.cs` (Ctrl+F, F3) : l'Omnibar passe en mode recherche.
pub struct Search;
impl Action for Search {
    fn label(&self) -> &'static str {
        "Search"
    }
    fn description(&self) -> &'static str {
        "SearchDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("OmnibarSearch")
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl('F' as u32))
    }
    fn second_hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 0x72, ctrl: false, shift: false, alt: false }) // F3
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        matches!(
            w.state.active().location,
            Location::Dir(_) | Location::RecycleBin | Location::SearchResults { .. }
        )
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.begin_filter_edit();
    }
}
