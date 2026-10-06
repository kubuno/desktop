//! `SidebarItemDropPosition` (mirrors
//! `Files.App.Controls/Sidebar/SidebarItemDropPosition.cs`).

/// The position at which an element was dropped onto a `SidebarItem`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SidebarItemDropPosition {
    /// Dropped on top: insert ABOVE this element.
    Top,
    /// Dropped on bottom: insert BELOW this element.
    Bottom,
    /// Dropped at center: insert as a CHILD of this element.
    Center,
}
