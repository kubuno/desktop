//! Breadcrumb segment layout — PURE logic ported from `BreadcrumbBarLayout`
//! (`Files.App.Controls/BreadcrumbBar/BreadcrumbBarLayout.cs`).
//!
//! The WinUI control measures its `BreadcrumbBarItem`s, then `MeasureOverride`
//! / `ArrangeOverride` compute which HEAD segments to fold behind an ellipsis
//! (…) button when the path overflows, always keeping the TAIL (the current
//! folder) visible. This module only exposes the COMPUTATION: it receives
//! already-measured widths (DirectWrite upstream, on the application side)
//! and returns rectangles. No `Painter`, no application state.

use crate::geometry::Rect;

/// Layout parameters, all in DIP. Counterparts of `BreadcrumbBar.xaml`'s
/// resources.
pub struct BreadcrumbLayoutParams {
    /// X origin of the first segment's left edge.
    pub start_x: f32,
    /// Available right edge: beyond this, fold into the ellipsis.
    pub avail_right: f32,
    /// Top / bottom of the produced rectangles.
    pub top: f32,
    pub bottom: f32,
    /// Chevron block between two segments (margin 2 + padding 4 + glyph 12 +
    /// padding 4). Port: 22.0.
    pub chevron_block: f32,
    /// Width of the ellipsis (…) button on overflow. Port: 24.0.
    pub ellipsis_width: f32,
}

/// Result of the layout computation.
pub struct BreadcrumbLayout {
    /// Rectangles of the VISIBLE segments, in order, starting at `start_index`
    /// (counterpart of items arranged by `ArrangeOverride`).
    pub items: Vec<Rect>,
    /// Ellipsis button: `Some` when head segments are folded (counterpart of
    /// `BreadcrumbBarLayout.EllipsisIsRendered`).
    pub ellipsis: Option<Rect>,
    /// Index of the first visible segment in the original collection
    /// (counterpart of `BreadcrumbBarLayout.IndexAfterEllipsis`).
    pub start_index: usize,
}

/// Lays out segments from their already-measured widths (padding included).
/// Reproduces `BreadcrumbBarLayout`: if everything fits, render left to right;
/// otherwise keep the suffix that fits and fold the prefix behind the
/// ellipsis (equivalent to `GetFirstIndexToRender`, which accumulates from
/// the end).
///
/// `widths[i]` = total width of segment i (label + `BreadcrumbBarItemPadding`).
pub fn layout_breadcrumbs(widths: &[f32], p: &BreadcrumbLayoutParams) -> BreadcrumbLayout {
    let gap = p.chevron_block;
    let n = widths.len();
    let total: f32 = widths.iter().sum::<f32>() + gap * n.saturating_sub(1) as f32;

    // Everything fits (or 0/1 segment): plain left-to-right rendering.
    if n <= 1 || p.start_x + total <= p.avail_right {
        let mut items = Vec::with_capacity(n);
        let mut bx = p.start_x;
        for &w in widths {
            items.push(Rect::new(bx, p.top, bx + w, p.bottom));
            bx += w + gap;
        }
        return BreadcrumbLayout { items, ellipsis: None, start_index: 0 };
    }

    // Overflow: ellipsis + suffix that fits. Always keep the last segment and
    // add towards the left as long as the budget allows.
    let budget = p.avail_right - p.start_x - (p.ellipsis_width + gap);
    let mut start = n - 1;
    let mut used = widths[start];
    while start > 0 {
        let add = widths[start - 1] + gap;
        if used + add > budget {
            break;
        }
        used += add;
        start -= 1;
    }

    let ellipsis = Some(Rect::new(p.start_x, p.top, p.start_x + p.ellipsis_width, p.bottom));
    let mut items = Vec::with_capacity(n - start);
    let mut bx = p.start_x + p.ellipsis_width + gap;
    for &w in &widths[start..] {
        items.push(Rect::new(bx, p.top, bx + w, p.bottom));
        bx += w + gap;
    }
    BreadcrumbLayout { items, ellipsis, start_index: start }
}
