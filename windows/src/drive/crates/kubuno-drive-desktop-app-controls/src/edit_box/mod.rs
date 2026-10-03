//! Single-line editors (inline rename, dialog fields, omnibar): selection +
//! scrolling text + caret, and the box around them.
//!
//! The box is `@ui/Input` (`core/frontend/src/ui/Input.tsx`), NOT a WinUI
//! TextBox: `h-9` · `rounded-md` (= `--radius-md`, 4) · `bg-white` ·
//! `border-border` · `px-3` · `text-sm text-text-primary` ·
//! `focus:ring-2 focus:ring-primary focus:border-primary`. The former accent
//! UNDERLINE (WinUI) has no equivalent in the web system.
//!
//! The control does NOT know `EditState` (which holds `entry` + the app-side
//! input logic): it receives a VIEW in primitives, borrowed for the duration
//! of the draw.

use crate::geometry::Rect;
use crate::themes::shape::radius;
use crate::Canvas;

/// `@ui/Input`: `px-3` — the padding inside a full input box.
const BOX_PAD_X: f32 = 12.0;
/// The omnibar pill supplies its own gutters (mode icon on the left, ✕ and
/// buttons on the right), so the text it hands us only needs a hair of inset.
const PILL_PAD_X: f32 = 6.0;
/// `@ui/Input`: `focus:ring-2` — the accent ring of a focused field. Drawn
/// inward, as `stroke_rounded_w` does, and it subsumes `focus:border-primary`.
const FOCUS_RING: f32 = 2.0;
/// Height of the line box the selection and the caret cover: 14 DIP of text at
/// `leading-normal`. The browser highlights the LINE, not the whole field.
const LINE_BOX: f32 = 20.0;

/// Primitives view of a single-line editor, for drawing only. Borrows the
/// app's text (`&'a str`): built and consumed in the same call statement, it
/// never outlives the borrow.
pub struct EditView<'a> {
    pub text: &'a str,
    /// Caret position in bytes (always on a char boundary).
    pub caret: usize,
    /// Selection anchor; equal to `caret` when nothing is selected.
    pub anchor: usize,
}

impl EditView<'_> {
    pub fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }
}

/// Inline rename editor: box, selection, text, caret — `@ui/Input` at
/// `--radius-md` (4), on an OPAQUE `bg-white` surface (`--color-surface-0`),
/// not the frosted flyout tint the WinUI port used.
///
/// The ring is unconditional because this box only exists while it owns the
/// caret: an inline editor is by construction the focused field, so it always
/// reads `focus:ring-2 focus:ring-primary` (same chrome as a dialog field).
pub fn draw_box(c: &dyn Canvas, rect: &Rect, v: &EditView) {
    let t = c.theme();
    c.fill_rounded(rect, radius::SM, &t.layer_background);
    c.stroke_rounded_w(rect, radius::SM, &t.accent, FOCUS_RING);
    draw_padded(c, rect, v, BOX_PAD_X);
}

/// Selection + text + caret only — the omnibar draws its own chrome (the
/// rounded pill bar IS the text box).
pub fn draw_text(c: &dyn Canvas, rect: &Rect, v: &EditView) {
    draw_padded(c, rect, v, PILL_PAD_X);
}

fn draw_padded(c: &dyn Canvas, rect: &Rect, v: &EditView, pad_x: f32) {
    let t = c.theme();
    let f = c.formats();
    // The TextBox scrolls horizontally to keep the caret visible; everything
    // is clipped to the box (otherwise a long path overflows the card).
    let inner_w = rect.right - rect.left - 2.0 * pad_x;
    let caret_w = c.measure(&v.text[..v.caret], &f.body);
    let origin = rect.left + pad_x - (caret_w - inner_w).max(0.0);
    // The line box the highlight and the caret cover, centred in the field.
    let cy = (rect.top + rect.bottom) / 2.0;
    let line_top = (cy - LINE_BOX / 2.0).max(rect.top + 2.0);
    let line_bottom = (cy + LINE_BOX / 2.0).min(rect.bottom - 2.0);
    c.push_clip(rect);
    let (a, b) = v.selection();
    if a != b {
        let x0 = origin + c.measure(&v.text[..a], &f.body);
        let x1 = origin + c.measure(&v.text[..b], &f.body);
        // The web declares no `::selection` rule, so the field keeps the
        // browser highlight: `--color-primary` at 35%, a PLAIN rectangle.
        let mut sel_color = t.accent;
        sel_color.a = 0.35;
        c.fill_rounded(&Rect::new(x0, line_top, x1, line_bottom), 0.0, &sel_color);
    }
    let text_w = c.measure(v.text, &f.body).max(inner_w);
    c.text(
        v.text,
        &Rect::new(origin, rect.top, origin + text_w + pad_x, rect.bottom),
        &f.body,
        &t.text_primary,
        false,
    );

    // `caret-color` is never set in the web: the caret takes `currentColor`,
    // i.e. `text-text-primary`, and spans the same line box as the selection.
    let caret_x = origin + caret_w;
    c.fill_rounded(&Rect::new(caret_x, line_top, caret_x + 1.0, line_bottom), 0.0, &t.text_primary);
    c.pop_clip();
}
