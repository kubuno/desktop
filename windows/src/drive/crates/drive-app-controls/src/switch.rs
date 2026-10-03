//! The toggle switch, drawn through the [`Canvas`] contract so both binaries
//! render the same control from one implementation.
//!
//! Geometry and colours come from the web's `toggleCanvas.ts`: a 36x20 track
//! with a 6px radius, a 14px thumb inset by 3 with a 4px radius, and a thumb
//! that stays WHITE in both states — the switch reads by its track, not its
//! knob.

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::{Canvas, Rect};

pub const WIDTH: f32 = 36.0;
pub const HEIGHT: f32 = 20.0;
const TRACK_RADIUS: f32 = 6.0;
const THUMB: f32 = 14.0;
const THUMB_RADIUS: f32 = 4.0;
const INSET: f32 = 3.0;

/// The same colour at a different alpha — how a disabled control dims.
pub fn fade(color: &D2D1_COLOR_F, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: color.a * alpha, ..*color }
}

/// Draws a switch with its top-left at `origin` and returns its bounds, which
/// is also its hit target.
pub fn draw(c: &dyn Canvas, origin: (f32, f32), on: bool, enabled: bool) -> Rect {
    let t = c.theme();
    let alpha = if enabled { 1.0 } else { 0.4 };
    let track = Rect::new(origin.0, origin.1, origin.0 + WIDTH, origin.1 + HEIGHT);
    if on {
        c.fill_rounded(&track, TRACK_RADIUS, &fade(&t.accent, alpha));
    } else {
        // Off: `--color-surface-3` filled, outlined with `--color-border`.
        c.fill_rounded(&track, TRACK_RADIUS, &fade(&t.control_fill_hover, alpha));
        c.stroke_rounded(&track, TRACK_RADIUS, &fade(&t.card_stroke, alpha));
    }
    let travel = WIDTH - THUMB - INSET * 2.0;
    let left = track.left + INSET + if on { travel } else { 0.0 };
    let thumb = Rect::new(left, track.top + INSET, left + THUMB, track.top + INSET + THUMB);
    c.fill_rounded(&thumb, THUMB_RADIUS, &fade(&t.accent_foreground, alpha));
    track
}

/// Where the switch sits when it is right-aligned on a settings row.
pub fn bounds_right_aligned(row: &Rect, margin: f32) -> Rect {
    let left = row.right - margin - WIDTH;
    let top = (row.top + row.bottom) / 2.0 - HEIGHT / 2.0;
    Rect::new(left, top, left + WIDTH, top + HEIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The thumb travels the full track, and only horizontally.
    #[test]
    fn the_thumb_travels_within_the_track() {
        let travel = WIDTH - THUMB - INSET * 2.0;
        assert_eq!(travel, 16.0);
        assert!(INSET + THUMB + travel <= WIDTH - INSET + f32::EPSILON);
    }

    /// Right-aligned, the switch sits inside its row and is vertically centred.
    #[test]
    fn right_aligned_stays_inside_the_row() {
        let row = Rect::new(0.0, 100.0, 400.0, 140.0);
        let b = bounds_right_aligned(&row, 16.0);
        assert_eq!(b.right, 384.0);
        assert_eq!(b.right - b.left, WIDTH);
        assert_eq!((b.top + b.bottom) / 2.0, (row.top + row.bottom) / 2.0);
        assert!(b.top > row.top && b.bottom < row.bottom);
    }
}
