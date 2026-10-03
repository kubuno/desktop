//! Pure dimensions and stacking of the toolbar's items.
//! Port of `Files.App.Controls/Toolbar/`: sizes come from
//! `Toolbar.ThemeResources.xaml` + `ToolbarButton.ThemeResources.xaml`, the
//! items/overflow model from `ToolbarSizes.cs` / `OverflowBehaviors.cs`, and
//! the arrangement algorithm intended (documented but never coded in
//! `Primitives/ToolbarLayout.cs`): "from last to first; if too narrow, draw
//! the ellipsis button".
//!
//! This `mod.rs` is GLUE: it splits the content across files mirroring the
//! C# ones and flat-re-exports the public API so no caller breaks
//! (`drive_app_controls::toolbar::X` unchanged).

pub mod primitives;
pub mod toolbar;
pub mod toolbar_button;
pub mod toolbar_item;

pub use primitives::{
    arrange, ItemMeasure, OverflowBehavior, ToolbarArrangement, ToolbarSize,
    OVERFLOW_BUTTON_HEIGHT, OVERFLOW_BUTTON_WIDTH,
};
pub use toolbar::{
    draw, ToolbarView, BORDER_THICKNESS, BUTTON_PADDING, CORNER_RADIUS, ICON_SIZE, INNER_PADDING,
    ITEM_SPACING,
};
pub use toolbar_button::{ButtonVisual, ToolbarButton};
pub use toolbar_item::ToolbarItemType;
