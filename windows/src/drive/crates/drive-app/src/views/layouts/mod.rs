//! Port of `Files.App/Views/Layouts/` — the layout pages.
//!
//! Same split as the original:
//!
//! * [`base_layout_page`] ↔ `BaseLayoutPage.cs` (the dispatch and what all
//!   pages share: display name, fallback glyph, unfocused pane);
//! * [`details_layout_page`] ↔ `DetailsLayoutPage.xaml` (rows + sortable
//!   headers);
//! * [`grid_layout_page`] ↔ `GridLayoutPage.xaml` (which hosts all THREE
//!   layouts List, Cards and Grid — see its `DataTemplate`s);
//! * [`columns_layout_page`] ↔ `ColumnsLayoutPage.xaml` (the blade `BladeView`);
//! * [`column_layout_page`] ↔ `ColumnLayoutPage.xaml` (the list of one blade).

pub mod base_layout_page;
pub mod column_layout_page;
pub mod columns_layout_page;
pub mod details_layout_page;
pub mod grid_layout_page;

// The shared helpers from `BaseLayoutPage` stay visible at the module level
// (sibling pages consume them via `super::file_glyph` / `super::display_name`).
pub(super) use base_layout_page::{display_name, file_glyph};
