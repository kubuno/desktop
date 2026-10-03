//! Shell layout and Direct2D drawing for the main window.
//! Owns the low-level `Painter` (pixel snapping, text, images, vector
//! icons), the `Rect`/`Hot`/`UiState`/`Layout` types with per-frame layout
//! and hit-testing, the sidebar and the Settings page. The individual
//! controls (tab bar, toolbars, file area, info pane, widgets, flyouts…)
//! live in `crate::user_controls`, mirroring `Files.App/UserControls/`.

// The immediate-mode UI engine (state, layout, drawing) has no equivalent in
// the original (the XAML framework handles that) — we split it by role. Each
// submodule re-exports via `pub use *`, so external `crate::ui::…` paths
// stay unchanged.
mod metrics;
mod hot;
mod state;
mod sidebar;
mod layout;
mod painter;

pub use metrics::*;
pub use hot::*;
pub use kubuno_drive_desktop_app_controls::Rect;
pub use state::*;
pub use sidebar::*;
pub use layout::*;
pub use painter::*;

pub use crate::user_controls::edit_box::{EditState, EDIT_DIALOG, EDIT_PALETTE, EDIT_PATH, EDIT_SEARCH};
pub use crate::user_controls::flyout::{
    flyout_progress, Flyout, FlyoutHit, FlyoutItem, FlyoutKind, MenuCommand, ACCEL_GAP,
    CONTENT_RIGHT, FLYOUT_WIDTH, LABEL_LEFT, SUBMENU_WIDTH,
};
pub use crate::user_controls::navigation_toolbar::suggestion_rect;
