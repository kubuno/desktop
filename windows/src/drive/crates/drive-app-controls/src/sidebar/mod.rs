//! The PURE core of `Files.App.Controls`'s `SidebarView`: display mode,
//! width thresholds and row dimensions. Stateless, with no dependency on the
//! domain model — `SidebarEntry`, `sidebar_visual` and `draw_sidebar`
//! (coupled to `HomeModel`) stay in `drive-app`.
//!
//! Mirrors the FLAT `Files.App.Controls/Sidebar` folder (13 files, no
//! subfolder). This `mod.rs` is GLUE: it declares the submodules and
//! flat-re-exports the whole historical public API (callers import via
//! `drive_app_controls::sidebar::X`).

pub mod flat_sidebar_item;
pub mod i_sidebar_item_model;
pub mod sidebar_display_mode;
pub mod sidebar_item;
pub mod sidebar_item_drop_position;
pub mod sidebar_view;

// Re-exports: preserve the flat `sidebar::X` API from before the split.
pub use sidebar_display_mode::SidebarMode;

pub use sidebar_view::{
    display_mode_for_pane_width, draw, sidebar_mode, sidebar_pane_width, window_forces_minimal,
    SidebarView, RESIZER_WIDTH, SIDEBAR_ANIM_MS, SIDEBAR_COMPACT_MAX_WIDTH, SIDEBAR_COMPACT_WIDTH,
    SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH, SIDEBAR_MINIMAL_MAX_WINDOW, SIDEBAR_OPEN_PANE_LENGTH,
};

pub use sidebar_item::{
    determine_drop_target_position, sidebar_row_width, RowIcon, SidebarRowView,
    DROP_REPOSITION_THRESHOLD, INDENT_PER_LEVEL, ROW_ICON_SIZE, ROW_ICON_TEXT_GAP,
    ROW_TEXT_RIGHT_MARGIN, SIDEBAR_ROW_CORNER, SIDEBAR_ROW_HEIGHT,
};

pub use sidebar_item_drop_position::SidebarItemDropPosition;

pub use flat_sidebar_item::{FlatSidebarItem, SIDEBAR_SECTION_GAP};

pub use i_sidebar_item_model::ISidebarItemModel;
