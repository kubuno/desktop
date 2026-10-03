//! Port of `Files.App/Actions/Sidebar/`.

use crate::main_window::MainWindow;

pub mod pin_folder_to_sidebar_action;
pub mod unpin_folder_to_sidebar_action;

pub use pin_folder_to_sidebar_action::PinFolderToSidebar;
pub use unpin_folder_to_sidebar_action::UnpinFolderFromSidebar;

/// Helper shared by both actions (target = parameter, otherwise selection).
pub(super) fn target(w: &MainWindow, parameter: Option<&str>) -> Option<String> {
    parameter.map(str::to_owned).or_else(|| {
        let tab = w.state.active();
        tab.selected_entry().map(|e| e.path.clone())
    })
}
