//! Port of `Files.App/Actions/Navigation/`.
//!
//! Module glue: each C# action has been split into its own file
//! (one `.cs` class = one `.rs`). This `mod.rs` declares the submodules,
//! re-exports all public actions (the `crate::actions::navigation::X` API
//! stays identical) and hosts the free helpers shared between several
//! actions (on the C# side this logic lives in the base classes
//! `BaseOpenInNewPaneAction`/`BaseOpenInOtherPaneAction`, not yet ported).

use crate::actions::{Action, HotKey, ToggleAction};
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

// --- Submodules (one action = one file) -------------------------------------

pub mod arrange_panes_horizontally_action;
pub mod arrange_panes_vertically_action;
pub mod close_active_pane_action;
pub mod close_all_tabs_action;
pub mod close_other_tabs_selected_action;
pub mod close_selected_tab_action;
pub mod close_tabs_to_the_left_selected_action;
pub mod close_tabs_to_the_right_selected_action;
pub mod duplicate_selected_tab_action;
pub mod focus_other_pane;
pub mod navigate_back_action;
pub mod navigate_forward_action;
pub mod navigate_home_action;
pub mod navigate_up_action;
pub mod new_tab_action;
pub mod new_window_action;
pub mod next_tab_action;
pub mod previous_tab_action;
pub mod reopen_closed_tab_action;
pub mod split_pane_horizontally_action;
pub mod split_pane_vertically_action;

pub mod open_current_folder_in_other_pane;
pub mod open_in_new_pane;
pub mod open_in_new_tab;
pub mod open_in_new_window;
pub mod open_in_other_pane;

// --- Re-exports (public API unchanged) --------------------------------------

pub use arrange_panes_horizontally_action::ArrangePanesHorizontally;
pub use arrange_panes_vertically_action::ArrangePanesVertically;
pub use close_active_pane_action::CloseActivePane;
pub use close_all_tabs_action::CloseAllTabs;
pub use close_other_tabs_selected_action::CloseOtherTabsSelected;
pub use close_selected_tab_action::CloseSelectedTab;
pub use close_tabs_to_the_left_selected_action::CloseTabsToTheLeftSelected;
pub use close_tabs_to_the_right_selected_action::CloseTabsToTheRightSelected;
pub use duplicate_selected_tab_action::DuplicateSelectedTab;
pub use focus_other_pane::FocusOtherPane;
pub use navigate_back_action::NavigateBack;
pub use navigate_forward_action::NavigateForward;
pub use navigate_home_action::NavigateHome;
pub use navigate_up_action::NavigateUp;
pub use new_tab_action::NewTab;
pub use new_window_action::NewWindow;
pub use next_tab_action::NextTab;
pub use previous_tab_action::PreviousTab;
pub use reopen_closed_tab_action::ReopenClosedTab;
pub use split_pane_horizontally_action::SplitPaneHorizontally;
pub use split_pane_vertically_action::SplitPaneVertically;

pub use open_current_folder_in_other_pane::OpenCurrentFolderInOtherPane;
pub use open_in_new_pane::OpenInNewPane;
pub use open_in_new_tab::OpenInNewTab;
pub use open_in_new_window::OpenInNewWindow;
pub use open_in_other_pane::OpenInOtherPane;

// ---------------------------------------------------------------------------
// Shared free helpers (unported C# base classes).
// ---------------------------------------------------------------------------

/// The selected folder, only if the selection is A SINGLE folder
/// (`BaseOpenInNewPaneAction.IsExecutable`).
pub(crate) fn selected_dir(w: &MainWindow) -> Option<String> {
    let tab = w.state.active();
    if tab.selected_paths().len() != 1 {
        return None;
    }
    tab.selected_entry().filter(|e| e.is_dir).map(|e| e.path.clone())
}

/// Navigates the OTHER pane to `location`, opening the second pane if
/// needed (arranging settings, like `ShellPanesPage.OpenSecondaryPane`).
pub(crate) fn navigate_other_pane(w: &mut MainWindow, location: Location) {
    let vertical = crate::services::settings::get().shell_pane_arrangement
        == crate::services::settings::ShellPaneArrangement::Vertical;
    let group = w.state.group_mut();
    if group.panes.len() == 1 {
        group.split(vertical);
    }
    let other = 1 - group.active_pane;
    group.panes[other].navigate(location);
    w.invalidate();
}

pub(crate) fn arrange_panes(w: &mut MainWindow, vertical: bool) {
    crate::services::settings::update(|s| {
        s.shell_pane_arrangement = if vertical {
            crate::services::settings::ShellPaneArrangement::Vertical
        } else {
            crate::services::settings::ShellPaneArrangement::Horizontal
        };
    });
    for group in &mut w.state.tabs {
        group.split_vertical = vertical;
    }
    w.invalidate();
}

// ---------------------------------------------------------------------------
// ToggleDualPane: on the C# side its counterpart is `Actions/Show/ToggleDualPaneAction.cs`
// (outside the Navigation area). It stays here to preserve the public API
// `crate::actions::navigation::ToggleDualPane` until the `actions/show` module
// takes it in.
// ---------------------------------------------------------------------------

/// `ToggleDualPaneAction.cs` (Ctrl+Shift+S): opens/closes the second pane.
pub struct ToggleDualPane;
impl Action for ToggleDualPane {
    fn label(&self) -> &'static str {
        "ToggleDualPane"
    }
    fn description(&self) -> &'static str {
        "ToggleDualPaneDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift('S' as u32))
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.state.group_mut().toggle_dual_pane();
        w.invalidate();
    }
}
impl ToggleAction for ToggleDualPane {
    fn is_on(&self, w: &MainWindow) -> bool {
        w.state.group().panes.len() == 2
    }
}
