//! (mirror of BladeView.cs) — container level: horizontal layout of blades
//! (`ItemsStackPanel Orientation="Horizontal"` inside a horizontal-only
//! ScrollViewer). Only the port's pure geometric helpers live here (rect of a
//! blade in the stack, ScrollViewer extent, viewport culling). The C# logic
//! (CycleBlades, ActiveBlades, AdjustBladeItemSize Fullscreen, ChangeView)
//! stays coupled to WinUI and is not ported.

use crate::geometry::Rect;

/// Rect of blade `index` in a stack of uniform-width `width` blades, scrolled
/// by `scroll_x`. `origin_left` = left edge of the 1st blade
/// (`content.left + pad`); `top`/`bottom` = viewport height (Normal mode).
pub fn blade_rect(index: usize, width: f32, origin_left: f32, top: f32, bottom: f32, scroll_x: f32) -> Rect {
    let x0 = origin_left + index as f32 * width - scroll_x;
    Rect::new(x0, top, x0 + width, bottom)
}

/// ScrollViewer's `ExtentWidth`: cumulative blade width + padding on both sides.
pub fn content_extent(total_blades_width: f32, pad: f32) -> f32 {
    total_blades_width + pad * 2.0
}
/// Uniform case: `count` blades of width `width`.
pub fn content_extent_uniform(count: usize, width: f32, pad: f32) -> f32 {
    content_extent(count as f32 * width, pad)
}

/// True if the blade is outside the horizontal viewport and can be skipped
/// when drawing (culling).
pub fn is_culled(blade: &Rect, view_left: f32, view_right: f32) -> bool {
    blade.right < view_left || blade.left > view_right
}
