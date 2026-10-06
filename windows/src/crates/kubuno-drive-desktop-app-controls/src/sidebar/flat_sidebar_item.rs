//! `FlatSidebarItem` (mirrors
//! `Files.App.Controls/Sidebar/FlatSidebarItem.cs`).
//!
//! Per-row wrapper used by the sidebar's virtualized `ItemsRepeater`: it
//! carries the tree depth and the section-gap flag, so the underlying data
//! doesn't have to. The `INotifyPropertyChanged` wiring is ignored; only the
//! `SectionGapMargin` logic is ported.

/// `FlatSidebarItem.SectionGapMargin`: the gap above a section that follows
/// an expanded predecessor (Thickness top = 12).
pub const SIDEBAR_SECTION_GAP: f32 = 12.0;

/// Per-row wrapper: carries depth and section-gap flags. Generic over the
/// underlying item type (`M`, a [`super::i_sidebar_item_model::ISidebarItemModel`]).
pub struct FlatSidebarItem<M> {
    pub item: M,
    pub depth: i32,
    /// Provided at construction; hidden filesystem elements are dimmed to
    /// follow the file list's dimming convention.
    pub row_opacity: f64,
    pub has_expanded_predecessor: bool,
}

impl<M> FlatSidebarItem<M> {
    pub fn new(item: M, depth: i32, row_opacity: f64) -> Self {
        Self {
            item,
            depth,
            row_opacity,
            has_expanded_predecessor: false,
        }
    }

    /// `SectionGapMargin`: top margin `12` if a predecessor is expanded,
    /// else `0` (mirrors `Thickness(0, 12, 0, 0)` vs `Thickness(0)`).
    pub fn section_gap_margin_top(&self) -> f32 {
        if self.has_expanded_predecessor {
            SIDEBAR_SECTION_GAP
        } else {
            0.0
        }
    }
}
