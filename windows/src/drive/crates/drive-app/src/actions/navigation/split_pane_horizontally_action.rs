//! SplitPaneHorizontally (mirrors SplitPaneHorizontallyAction.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `SplitPaneHorizontallyAction.cs` (Alt+Shift+H): second pane stacked.
pub struct SplitPaneHorizontally;
impl Action for SplitPaneHorizontally {
    fn label(&self) -> &'static str {
        "SplitPaneHorizontally"
    }
    fn description(&self) -> &'static str {
        "SplitPaneHorizontallyDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 'H' as u32, ctrl: false, shift: true, alt: true })
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.group().panes.len() == 1
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.state.group_mut().split(false);
        w.invalidate();
    }
}
