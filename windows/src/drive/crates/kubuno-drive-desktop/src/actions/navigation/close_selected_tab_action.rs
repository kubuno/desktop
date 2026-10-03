//! CloseSelectedTab (mirror of CloseSelectedTabAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `CloseSelectedTabAction.cs` (Ctrl+W).
pub struct CloseSelectedTab;
impl Action for CloseSelectedTab {
    fn label(&self) -> &'static str {
        "CloseTab"
    }
    fn description(&self) -> &'static str {
        "CloseSelectedTabDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl('W' as u32))
    }
    fn second_hotkey(&self) -> Option<HotKey> {
        // `CloseSelectedTabAction` : SecondHotKey Ctrl+F4 (VK_F4 = 0x73).
        Some(HotKey { key: 0x73, ctrl: true, shift: false, alt: false })
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.close_tab(w.state.active_tab);
    }
}
