//! `SidebarDisplayMode` (mirrors `Files.App.Controls/Sidebar/SidebarDisplayMode.cs`).

/// The `SidebarView`'s display mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SidebarMode {
    /// Full docked pane, width `OpenPaneLength`.
    Expanded,
    /// Docked icon rail of `SIDEBAR_COMPACT_WIDTH`.
    Compact,
    /// `SIDEBAR_OPEN_PANE_LENGTH` pane as an OVERLAY, hidden unless open
    /// (window < `SIDEBAR_MINIMAL_MAX_WINDOW`).
    Minimal,
}
