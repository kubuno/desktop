//! RefreshItems (mirrors RefreshItemsAction.cs)
//!
//! NB: on the C# side this class lives in `Actions/Content/RefreshItemsAction.cs`,
//! not in `Actions/FileSystem/`. The cross-area move to `actions/content/`
//! is still to be done; kept here (and re-exported by `file_system::RefreshItems`)
//! so as not to break `actions/mod.rs`.

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

/// `RefreshItemsAction.cs` (Ctrl+R, F5).
pub struct RefreshItems;
impl Action for RefreshItems {
    fn label(&self) -> &'static str {
        "Refresh"
    }
    fn description(&self) -> &'static str {
        "RefreshItemsDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl('R' as u32))
    }
    fn second_hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 0x74, ctrl: false, shift: false, alt: false }) // VK_F5
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        if w.state.active().location == Location::Home {
            w.model = crate::data::items::HomeModel::load();
        } else {
            w.state.active_mut().refresh();
        }
        w.invalidate();
    }
}
