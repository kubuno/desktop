//! (mirror of `EventArgs.cs`) — payloads for the breadcrumb trail's events.
//!
//! The C# `record class`es also carry WinUI references (`BreadcrumbBarItem`,
//! `MenuFlyout`, `PointerRoutedEventArgs?`) with no equivalent in the
//! immediate-mode port; only the portable data fields are transcribed here.

/// A breadcrumb segment was clicked (counterpart of
/// `BreadcrumbBarItemClickedEventArgs`).
pub struct BreadcrumbBarItemClickedEventArgs {
    /// Index of the segment in the original collection.
    pub index: i32,
    /// True if the clicked segment is the root.
    pub is_root_item: bool,
}

/// A segment's dropdown menu (chevron) is about to open (counterpart of
/// `BreadcrumbBarItemDropDownFlyoutEventArgs`).
pub struct BreadcrumbBarItemDropDownFlyoutEventArgs {
    /// Index of the segment involved, `-1` if none.
    pub index: i32,
    /// True if the segment involved is the root.
    pub is_root_item: bool,
}
