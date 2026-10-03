//! (mirror of BladeItem.cs) — PURE logic of a single blade: dimension
//! constants, width after resize (`BladeResizer_ManipulationDelta`), optimal
//! width (`SetWidth`/`CalculateOptimalWidth`), and divider/handle geometry.
//! WinUI machinery (OnApplyTemplate, VisualStateManager, cursors, visual tree
//! walk) is ignored.

use crate::geometry::Rect;

/// `BladeItem.MINIMUM_WIDTH` — floor for a blade's resize.
pub const MINIMUM_WIDTH: f32 = 150.0;
/// `BladeItem.DEFAULT_WIDTH` — default width (= the port's `COLUMN_PANE_WIDTH`).
pub const DEFAULT_WIDTH: f32 = 200.0;
/// `BladeResizer` handle on the right edge (`Width="4"`).
pub const RESIZER_WIDTH: f32 = 4.0;
/// `BladeItem.BorderThickness="1"` — vertical divider between two blades.
pub const BORDER_THICKNESS: f32 = 1.0;
/// `CalculateOptimalWidth`: padding added to the measured text width (icon 32
/// + margins 24 + padding 24 + chevron/tags 40).
pub const OPTIMAL_WIDTH_PADDING: f32 = 120.0;

/// Width after dragging the `BladeResizer`: width before + cumulative pointer
/// delta, never below `MINIMUM_WIDTH` (`BladeItem.cs:98-106`).
pub fn resized_width(pre_manipulation_width: f32, cumulative_dx: f32) -> f32 {
    (pre_manipulation_width + cumulative_dx).max(MINIMUM_WIDTH)
}

/// Optimal width (double-click, `SetWidth`) from the measured text width: +
/// padding, floored at `MINIMUM_WIDTH`; 0 → `DEFAULT_WIDTH`. Text measurement
/// stays with the caller (coupled to the model).
pub fn optimal_width(measured_text_width: f32) -> f32 {
    if measured_text_width > 0.0 {
        (measured_text_width + OPTIMAL_WIDTH_PADDING).max(MINIMUM_WIDTH)
    } else {
        DEFAULT_WIDTH
    }
}

/// Vertical divider on a blade's right edge (`BorderThickness` on the right side).
pub fn blade_divider(blade: &Rect) -> Rect {
    Rect::new(blade.right - BORDER_THICKNESS, blade.top, blade.right, blade.bottom)
}

/// Rect of the resize handle (`BladeResizer`, right-aligned).
pub fn resizer_rect(blade: &Rect) -> Rect {
    Rect::new(blade.right - RESIZER_WIDTH, blade.top, blade.right, blade.bottom)
}
