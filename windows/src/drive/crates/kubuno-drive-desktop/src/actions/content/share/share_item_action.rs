//! ShareItem (mirrors ShareItemAction.cs)
//!
//! Port of `Files.App/Actions/Content/Share/ShareItemAction.cs`.

use crate::actions::Action;
use crate::main_window::MainWindow;

/// The Windows share sheet (`IDataTransferManagerInterop`).
pub struct ShareItem;
impl Action for ShareItem {
    fn label(&self) -> &'static str {
        "Share"
    }
    fn description(&self) -> &'static str {
        "ShareItemDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("Share")
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        // Original `IsExecutable`: non-empty selection, ALL shareable
        // (neither folder nor shortcut).
        let tab = w.state.active();
        !tab.selected.is_empty()
            && tab
                .selected
                .iter()
                .all(|&i| crate::helpers::share_item_helpers::is_shareable(&tab.entries[i].path))
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        // The share sheet receives the ENTIRE selection.
        let paths = match parameter {
            Some(p) => vec![p.to_owned()],
            None => w.state.active().selected_paths(),
        };
        if !paths.is_empty() {
            if let Err(e) = crate::helpers::share_item_helpers::share_items(w.hwnd, &paths) {
                tracing::warn!("share failed: {e}");
            }
        }
    }
}
