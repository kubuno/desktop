//! The underlined letter of a mnemonic (`&Save`): WinForms' keyboard cue, drawn under the shortcut
//! letter of a control's text while Alt is held (`vskubuno/docs/EVENTS.md` §16, `UseMnemonic`).
//! The widgets draw it where they drew their text; which letter, and when, is their caller's
//! decision (`mnemonic` fields set only while the cues show).

use drive_app_controls::Canvas;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::{IDWriteTextFormat, DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_TRAILING};

use crate::Rect;

/// Where the underline of character `index` of `line` goes, drawn in `band` with `align` (the
/// text's own alignment, vertically centred as `Canvas::text` draws it): `(left, right, y)`, or
/// `None` when `index` is outside the line. `measure` gives a string's width.
pub fn underline_span(line: &str, index: usize, band: Rect, align: DWRITE_TEXT_ALIGNMENT, size: f32, measure: &dyn Fn(&str) -> f32) -> Option<(f32, f32, f32)> {
    let chars: Vec<char> = line.chars().collect();
    let ch = *chars.get(index)?;
    let width = measure(line);
    let start = if align == DWRITE_TEXT_ALIGNMENT_CENTER {
        (band.left + band.right - width) / 2.0
    } else if align == DWRITE_TEXT_ALIGNMENT_TRAILING {
        band.right - width
    } else {
        band.left
    };
    let prefix: String = chars[..index].iter().collect();
    let left = start + measure(&prefix);
    let right = left + measure(&ch.to_string());
    let y = (band.top + band.bottom) / 2.0 + size * 0.42;
    Some((left, right, y))
}

/// Draws the underline of character `index` of `line` (see [`underline_span`]).
pub fn underline(c: &dyn Canvas, line: &str, index: usize, band: &Rect, format: &IDWriteTextFormat, colour: &D2D1_COLOR_F, align: DWRITE_TEXT_ALIGNMENT) {
    // SAFETY: a plain COM getter on a live text format.
    let size = unsafe { format.GetFontSize() };
    let measure = |s: &str| c.measure(s, format);
    if let Some((left, right, y)) = underline_span(line, index, *band, align, size, &measure) {
        let thickness = (1.0 / c.scale().max(0.01)).max(size / 16.0);
        c.fill_rounded(&Rect::new(left, y, right.max(left + 1.0), y + thickness), 0.0, colour);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_LEADING;

    #[test]
    fn the_underline_sits_under_its_letter_for_each_alignment() {
        let measure = |s: &str| s.chars().count() as f32 * 10.0;
        let band = Rect::new(0.0, 0.0, 100.0, 20.0);
        let (l, r, y) = underline_span("Save", 0, band, DWRITE_TEXT_ALIGNMENT_LEADING, 14.0, &measure).unwrap();
        assert_eq!((l, r), (0.0, 10.0));
        assert!((y - (10.0 + 14.0 * 0.42)).abs() < 1e-4);
        let (l, r, _) = underline_span("Save", 2, band, DWRITE_TEXT_ALIGNMENT_CENTER, 14.0, &measure).unwrap();
        assert_eq!((l, r), (50.0, 60.0));
        let (l, _, _) = underline_span("Save", 3, band, DWRITE_TEXT_ALIGNMENT_TRAILING, 14.0, &measure).unwrap();
        assert_eq!(l, 90.0);
        assert!(underline_span("Save", 4, band, DWRITE_TEXT_ALIGNMENT_LEADING, 14.0, &measure).is_none());
    }
}
