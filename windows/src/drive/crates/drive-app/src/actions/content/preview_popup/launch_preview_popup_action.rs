//! LaunchPreviewPopup (mirrors LaunchPreviewPopupAction.cs)
//!
//! Port of `Files.App/Actions/Content/PreviewPopup/LaunchPreviewPopupAction.cs`.

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `LaunchPreviewPopupAction` (Space): toggles the external quick preview
/// (QuickLook / Seer) for the single selected item.
pub struct LaunchPreviewPopup;
impl Action for LaunchPreviewPopup {
    fn label(&self) -> &'static str {
        "LaunchPreviewPopup"
    }
    fn description(&self) -> &'static str {
        "LaunchPreviewPopupDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 0x20, ctrl: false, shift: false, alt: false }) // VK_SPACE
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        // `IsExecutable => SelectedItems.Count == 1` (renaming is already
        // excluded upstream: `on_key_down` returns early if an edit is open).
        w.state.active().selected_paths().len() == 1
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let paths = w.state.active().selected_paths();
        if let [path] = paths.as_slice() {
            crate::services::preview_popup::toggle_preview(path);
        }
    }
}
