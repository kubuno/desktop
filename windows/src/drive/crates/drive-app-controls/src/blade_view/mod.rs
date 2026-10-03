//! PURE layout logic for the `BladeView` (Columns view / Miller columns).
//! Port of `Files.App.Controls/BladeView`. Blades stack horizontally
//! (`ItemsStackPanel Orientation="Horizontal"`) inside a horizontal-only
//! ScrollViewer. Only covers `Normal` mode; each blade's content (entry list)
//! stays with the caller, coupled to the model.
//!
//! This module is glue: the content lives in the sub-files mirroring the C#
//! folder (`blade_mode.rs`, `blade_item.rs`, `blade_view.rs`) and is
//! re-exported here to preserve the flat `blade_view::X` API.

pub mod blade_item;
pub mod blade_mode;
pub mod blade_view;

pub use blade_item::{
    blade_divider, optimal_width, resized_width, resizer_rect, BORDER_THICKNESS, DEFAULT_WIDTH,
    MINIMUM_WIDTH, OPTIMAL_WIDTH_PADDING, RESIZER_WIDTH,
};
pub use blade_mode::BladeMode;
pub use blade_view::{blade_rect, content_extent, content_extent_uniform, is_culled};
