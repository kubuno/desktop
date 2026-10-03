//! `BreadcrumbBar` (mirror of `BreadcrumbBar.cs`): the DRAWING of the
//! breadcrumb trail (segments + chevrons + overflow ellipsis). The C# class is
//! a WinUI `Control` (`OnApplyTemplate`, `ItemsRepeater`,
//! `RaiseItemClickedEvent`) that isn't portable; this file holds the ported
//! immediate-mode rendering, the control's drawing counterpart. The PURE
//! layout algorithm lives alongside, in [`super::breadcrumb_bar_layout`]
//! (← `BreadcrumbBarLayout.cs`).

use crate::geometry::Rect;
use crate::themes::shape::radius;
use crate::Canvas;

/// Chevron glyph `>` between two segments (`ChevronRight`, Segoe Fluent Icons).
const GLYPH_CHEVRON_RIGHT: &str = "\u{E76C}";
/// Ellipsis glyph `…` for the overflow button (`More`).
const GLYPH_ELLIPSIS: &str = "\u{E712}";

/// A visible breadcrumb segment: its rectangle (= hover area), its label, and
/// whether it's hovered.
pub struct BreadcrumbSegment {
    pub rect: Rect,
    pub label: String,
    pub hot: bool,
}

/// Drawing snapshot of the breadcrumb trail, in PRIMITIVES: enough to paint
/// without knowing `UiState`/`Layout`/`Tab`. Built by the application (it
/// measures labels, resolves hover) then passed to [`draw`].
pub struct BreadcrumbView {
    /// Chevron after the home icon (root).
    pub home_chevron: Rect,
    /// Ellipsis `…` button + its chevron, when the path overflows (`Some`).
    pub ellipsis: Option<(Rect, bool)>,
    /// The visible segments, left→right.
    pub segments: Vec<BreadcrumbSegment>,
}

/// Draws the breadcrumb trail through a [`Canvas`] — the control's DRAWING now
/// lives WITH the control (mirror of `Files.App.Controls`'s `BreadcrumbBar`).
/// Reproduces the omnibar's rendering identically: root chevron, overflow
/// ellipsis, then each segment (hover fill, label at +8, chevron at +6).
pub fn draw(c: &dyn Canvas, v: &BreadcrumbView) {
    let t = c.theme();
    let f = c.formats();
    // Separators are `--color-text-tertiary` in the web trail; only the
    // CURRENT folder gets `--color-text-primary`, its ancestors stay secondary.
    c.text(GLYPH_CHEVRON_RIGHT, &v.home_chevron, &f.icon_small, &t.text_tertiary, true);

    if let Some((ell, hot)) = &v.ellipsis {
        if *hot {
            c.fill_rounded(ell, radius::SM, &t.control_fill_hover);
        }
        c.text(GLYPH_ELLIPSIS, ell, &f.icon_small, &t.text_secondary, true);
        // Chevron block: 12 glyph after margin 2 + padding 4 = +6.
        let chevron = Rect::new(ell.right + 6.0, ell.top, ell.right + 18.0, ell.bottom);
        c.text(GLYPH_CHEVRON_RIGHT, &chevron, &f.icon_small, &t.text_tertiary, true);
    }

    // The layout always keeps the TAIL visible (`layout_breadcrumbs` folds the
    // HEAD behind the ellipsis), so the last drawn segment IS the current one.
    let last = v.segments.len().saturating_sub(1);
    for (i, seg) in v.segments.iter().enumerate() {
        if seg.hot {
            c.fill_rounded(&seg.rect, radius::SM, &t.control_fill_hover);
        }
        let color = if i == last { &t.text_primary } else { &t.text_secondary };
        // Label after `BreadcrumbBarItemPadding` left = 8.
        let label_rect = Rect::new(seg.rect.left + 8.0, seg.rect.top, seg.rect.right, seg.rect.bottom);
        c.text(&seg.label, &label_rect, &f.body, color, false);
        // Chevron block: 12 glyph after margin 2 + padding 4 = +6.
        let chevron = Rect::new(seg.rect.right + 6.0, seg.rect.top, seg.rect.right + 18.0, seg.rect.bottom);
        c.text(GLYPH_CHEVRON_RIGHT, &chevron, &f.icon_small, &t.text_tertiary, true);
    }
}
