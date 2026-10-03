//! `ISidebarItemModel` (mirrors
//! `Files.App.Controls/Sidebar/ISidebarItemModel.cs`).
//!
//! The data contract for a sidebar element. On the C# side it inherits from
//! `INotifyPropertyChanged` (WinUI notification machinery, not ported); only
//! the data properties and their defaults (`false`) are portable.

/// Data contract for a sidebar element.
pub trait ISidebarItemModel {
    /// This element's children, rendered as child rows of the `SidebarItem`.
    /// `true` if the element exposes a children collection (even empty).
    fn has_children(&self) -> bool;

    /// Is the element expanded (children visible) or collapsed?
    fn is_expanded(&self) -> bool;
    fn set_is_expanded(&mut self, value: bool);

    /// Optional path associated with this element (drag-and-drop scenarios).
    fn path(&self) -> Option<&str>;

    /// Rendered as expandable even when `Children` is empty (children
    /// lazily loaded on first expansion). Default: `false`.
    fn has_unrealized_children(&self) -> bool {
        false
    }

    /// Participates in expansion while keeping the normal row appearance
    /// (icon + regular text) instead of the section header style.
    /// Default: `false`.
    fn is_leaf_with_children(&self) -> bool {
        false
    }
}
