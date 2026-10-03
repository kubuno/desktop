//! Scroll bar. The BEHAVIOUR is WinUI's (a thin resting indicator that
//! unfolds into a full gutter with two `RepeatButton` chevrons on hover); the
//! SKIN is the web's `::-webkit-scrollbar` from `core/frontend/src/index.css`:
//! an 8px thumb at `--scrollbar-thumb` (#dadce0), `border-radius: 4px`,
//! `--scrollbar-thumb-hover` (#bdc1c6) on hover, transparent track.

use crate::geometry::Rect;
use crate::themes::{shape::radius, ThemeMode};
use crate::Canvas;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

/// `ScrollBarSize`: the gutter's width once expanded. Wider than the web's
/// 8px rail because the expanded state also has to hold the two chevrons.
pub const SCROLLBAR_SIZE: f32 = 12.0;
/// The thumb's width at rest (the *panning indicator*).
pub const INDICATOR_WIDTH: f32 = 2.0;
/// The thumb's width when expanded — the web's `::-webkit-scrollbar { width: 8px }`.
pub const THUMB_WIDTH: f32 = 8.0;
/// `ScrollBarVerticalThumbMinHeight`: the thumb never shrinks below 24 DIP.
pub const MIN_THUMB: f32 = 24.0;
/// The `RepeatButton`s at both ends.
pub const ARROW_SIZE: f32 = 12.0;
/// How long the indicator stays after the last scroll, and over how long it
/// fades out.
pub const FADE_AFTER_MS: f32 = 1500.0;
pub const FADE_MS: f32 = 250.0;

// Chevrons for the `RepeatButton`s (Segoe Fluent Icons) — a control carries
// its own glyphs, like `breadcrumb_bar` re-declares `ChevronRight`.
const GLYPH_CHEVRON_UP: &str = "\u{E70E}";
const GLYPH_CHEVRON_DOWN: &str = "\u{E70D}";
const GLYPH_CHEVRON_LEFT: &str = "\u{E76B}";
const GLYPH_CHEVRON_RIGHT: &str = "\u{E76C}";

/// A bar computed for a scrollable area.
#[derive(Clone, Copy)]
pub struct Scrollbar {
    /// The full gutter (12 DIP wide, or tall if horizontal).
    pub rail: Rect,
    /// The thumb, already positioned per the current scroll.
    pub thumb: Rect,
    pub horizontal: bool,
    /// The pointer is inside the gutter: the bar is expanded.
    pub expanded: bool,
}

impl Scrollbar {
    /// Computes the bar for a `content` area whose content measures `extent`
    /// and which is scrolled by `scroll`. `None` if there's nothing to scroll.
    pub fn new(
        content: &Rect,
        extent: f32,
        scroll: f32,
        horizontal: bool,
        expanded: bool,
    ) -> Option<Self> {
        let viewport = if horizontal {
            content.right - content.left
        } else {
            content.bottom - content.top
        };
        if extent <= viewport + 0.5 {
            return None;
        }
        let rail = if horizontal {
            Rect::new(content.left, content.bottom - SCROLLBAR_SIZE, content.right, content.bottom)
        } else {
            Rect::new(content.right - SCROLLBAR_SIZE, content.top, content.right, content.bottom)
        };
        // Each arrow eats into one end of the track, but only when the bar is
        // expanded (at rest there's only the thumb).
        let inset = if expanded { ARROW_SIZE } else { 0.0 };
        let track_len = viewport - 2.0 * inset;
        let thumb_len = (viewport / extent * track_len).max(MIN_THUMB).min(track_len);
        let max_scroll = (extent - viewport).max(1.0);
        let t = (scroll / max_scroll).clamp(0.0, 1.0);
        let start = inset + t * (track_len - thumb_len);

        let width = if expanded { THUMB_WIDTH } else { INDICATOR_WIDTH };
        let thumb = if horizontal {
            // The thumb hugs the edge, with the same inset on both sides.
            let cy = rail.bottom - (SCROLLBAR_SIZE - width) / 2.0 - width / 2.0;
            Rect::new(content.left + start, cy - width / 2.0, content.left + start + thumb_len, cy + width / 2.0)
        } else {
            let cx = rail.right - (SCROLLBAR_SIZE - width) / 2.0 - width / 2.0;
            Rect::new(cx - width / 2.0, content.top + start, cx + width / 2.0, content.top + start + thumb_len)
        };
        Some(Self { rail, thumb, horizontal, expanded })
    }

    /// The scroll designated by pointer position `pos` while dragging the
    /// thumb, `grab` being the offset grabbed within the thumb.
    pub fn scroll_at(&self, content: &Rect, extent: f32, pos: f32, grab: f32) -> f32 {
        let (viewport, origin, thumb_len) = if self.horizontal {
            (content.right - content.left, content.left, self.thumb.right - self.thumb.left)
        } else {
            (content.bottom - content.top, content.top, self.thumb.bottom - self.thumb.top)
        };
        let inset = if self.expanded { ARROW_SIZE } else { 0.0 };
        let track_len = viewport - 2.0 * inset;
        let span = (track_len - thumb_len).max(1.0);
        let start = (pos - grab - origin - inset).clamp(0.0, span);
        start / span * (extent - viewport).max(0.0)
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
}

/// `--scrollbar-thumb-hover`: #bdc1c6 in the light theme (theme.css), #80868b
/// in `kubuno-dark/theme.json`. Resolved here rather than read from `Theme`,
/// which only carries `scrollbar_thumb` — see the note in the task report.
fn thumb_hover(mode: ThemeMode) -> D2D1_COLOR_F {
    match mode {
        ThemeMode::Light => rgb(189, 193, 198),
        ThemeMode::Dark => rgb(128, 134, 139),
    }
}

/// Draws the bar. `alpha` carries the resting indicator's fade-out.
pub fn draw(c: &dyn Canvas, bar: &Scrollbar, alpha: f32, hot_thumb: bool) {
    let t = c.theme();
    let fade = |col: D2D1_COLOR_F| D2D1_COLOR_F { a: col.a * alpha, ..col };

    if bar.expanded {
        // The track and its two `RepeatButton`s.
        c.fill_rounded(&bar.rail, 0.0, &fade(t.scrollbar_track));
        let f = c.formats();
        let (up, down) = if bar.horizontal {
            (GLYPH_CHEVRON_LEFT, GLYPH_CHEVRON_RIGHT)
        } else {
            (GLYPH_CHEVRON_UP, GLYPH_CHEVRON_DOWN)
        };
        let (a, b) = if bar.horizontal {
            (
                Rect::new(bar.rail.left, bar.rail.top, bar.rail.left + ARROW_SIZE, bar.rail.bottom),
                Rect::new(bar.rail.right - ARROW_SIZE, bar.rail.top, bar.rail.right, bar.rail.bottom),
            )
        } else {
            (
                Rect::new(bar.rail.left, bar.rail.top, bar.rail.right, bar.rail.top + ARROW_SIZE),
                Rect::new(bar.rail.left, bar.rail.bottom - ARROW_SIZE, bar.rail.right, bar.rail.bottom),
            )
        };
        c.text(up, &a, &f.icon_tiny, &fade(t.text_secondary), true);
        c.text(down, &b, &f.icon_tiny, &fade(t.text_secondary), true);
    }

    // `::-webkit-scrollbar-thumb { border-radius: 4px }` — capped at half the
    // thickness so the 2 DIP resting indicator still reads as a pill.
    let thickness = if bar.horizontal {
        bar.thumb.bottom - bar.thumb.top
    } else {
        bar.thumb.right - bar.thumb.left
    };
    let r = radius::SM.min(thickness / 2.0);
    // `--scrollbar-thumb` at rest, `--scrollbar-thumb-hover` under the pointer:
    // the web CHANGES the colour, it does not brighten the same one (which was
    // a no-op here, both themes carrying an opaque thumb).
    let color = fade(if hot_thumb { thumb_hover(t.mode) } else { t.scrollbar_thumb });
    c.fill_rounded(&bar.thumb, r, &color);
}
