//! `OmnibarModeSeparator` (mirror of
//! `Files.App.Controls/Omnibar/OmnibarModeSeparator.cs`).
//! The C# class is an almost-empty `Control` (`DefaultStyleKey`); this file
//! holds the separator's PORTABLE geometry (constants from `Omnibar.xaml` +
//! [`separator_rect`]).

use crate::geometry::Rect;

// ── Mode separator ───────────────────────────────────────────────────────────
pub const SEPARATOR_WIDTH: f32 = 1.0; // OmnibarModeSeparatorWidth
pub const SEPARATOR_HEIGHT: f32 = 20.0; // OmnibarModeSeparatorHeight
pub const SEPARATOR_PADDING_X: f32 = 4.0; // OmnibarModeSeparatorPadding "4,0,4,0"
/// Effective width of a separator (rect + padding on both sides).
pub const SEPARATOR_SLOT_WIDTH: f32 = SEPARATOR_WIDTH + 2.0 * SEPARATOR_PADDING_X; // 9

/// Rect (vertical, 1×20) of the separator between modes `i` and `i+1`,
/// centered in the gap between the two buttons.
pub fn separator_rect(mode_rects: &[Rect], i: usize) -> Option<Rect> {
    let a = mode_rects.get(i)?;
    let b = mode_rects.get(i + 1)?;
    let cx = (a.right + b.left) / 2.0;
    let cy = (a.top + a.bottom) / 2.0;
    Some(Rect::new(
        cx - SEPARATOR_WIDTH / 2.0,
        cy - SEPARATOR_HEIGHT / 2.0,
        cx + SEPARATOR_WIDTH / 2.0,
        cy + SEPARATOR_HEIGHT / 2.0,
    ))
}
