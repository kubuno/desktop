//! SplitPaneVertically (mirrors SplitPaneVerticallyAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `SplitPaneVerticallyAction.cs` (Alt+Shift+V): opens a second pane side by
/// side. Executable only in single-pane mode (`!IsMultiPaneActive`).
pub struct SplitPaneVertically;
impl Action for SplitPaneVertically {
    fn label(&self) -> &'static str {
        "SplitPaneVertically"
    }
    fn description(&self) -> &'static str {
        "SplitPaneVerticallyDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 'V' as u32, ctrl: false, shift: true, alt: true })
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.group().panes.len() == 1
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.state.group_mut().split(true);
        w.invalidate();
    }
}
