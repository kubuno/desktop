//! The gallery's page furniture: titles, captioned cells, and the side-by-side
//! pair that the whole exercise turns on.
//!
//! Kept deliberately small. A page is a list of sections; a section is a list
//! of cells; a cell is a caption and a rectangle a primitive paints into. What
//! matters is [`pair`]: it hands the **same** rectangle, twice, to the old
//! hand-written control and to its rebuilt version, so a difference is a
//! difference in the paint and not in the geometry the caller chose.

#![allow(dead_code)] // Each page uses a subset; the helper set is shared.

use kubuno_drive_desktop_app_controls::themes::shape::{space, text as text_size};
use kubuno_desktop_ui::{Canvas, Rect};

/// Left margin and the vertical rhythm every page follows.
pub const MARGIN: f32 = 24.0;
pub const ROW_GAP: f32 = 16.0;
pub const CAPTION_H: f32 = 18.0;
/// The height reserved at the top of the window for the gallery's navigation
/// strip. Every page starts its content below it, so the one navigable window
/// can switch pages without any page having to know the nav exists.
pub const NAV_H: f32 = 44.0;

/// A cursor walking down a page.
pub struct Page<'a> {
    pub c:    &'a dyn Canvas,
    pub area: Rect,
    pub y:    f32,
}

impl<'a> Page<'a> {
    pub fn new(c: &'a dyn Canvas, width: f32, height: f32) -> Self {
        let area = Rect::new(0.0, 0.0, width, height);
        // No ground fill: the gallery paints the pane behind the tab panel, and
        // a fill here would spill past the panel's inset edges and make it
        // scroll for nothing.
        // Start below the navigation strip the gallery draws over the top edge,
        // so no page overlaps it.
        Self { c, area, y: NAV_H + MARGIN }
    }

    /// A section heading, and the rule under it.
    pub fn section(&mut self, title: &str) {
        let t = self.c.theme();
        let f = self.c.formats();
        let r = Rect::new(MARGIN, self.y, self.area.right - MARGIN, self.y + 24.0);
        self.c.text(title, &r, &f.heading, &t.text_primary, false);
        self.y += 28.0;
        let rule = Rect::new(MARGIN, self.y, self.area.right - MARGIN, self.y + 1.0);
        self.c.fill_rounded(&rule, 0.0, &t.card_stroke);
        self.y += space::MD;
    }

    /// A caption above a row of cells, then the row's top edge.
    pub fn caption(&mut self, label: &str) -> f32 {
        let t = self.c.theme();
        let f = self.c.formats();
        let r = Rect::new(MARGIN, self.y, self.area.right - MARGIN, self.y + CAPTION_H);
        self.c.text(label, &r, &f.caption, &t.text_secondary, false);
        self.y += CAPTION_H + space::XS;
        self.y
    }

    /// Advances past a row `h` tall.
    pub fn advance(&mut self, h: f32) {
        self.y += h + ROW_GAP;
    }
}

impl Drop for Page<'_> {
    /// Keeps a margin under the last row: an invisible fill that the tab
    /// panel's scroll area measures, so a page scrolled to its end does not
    /// stop flush on its last control.
    fn drop(&mut self) {
        let clear = windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
        self.c.fill_rounded(&Rect::new(0.0, self.y, 1.0, self.y + MARGIN), 0.0, &clear);
    }
}

/// Lays a row of `n` cells across `width`, each `w` wide with `gap` between.
pub fn cells(left: f32, top: f32, w: f32, h: f32, gap: f32, n: usize) -> Vec<Rect> {
    (0..n)
        .map(|i| {
            let x = left + i as f32 * (w + gap);
            Rect::new(x, top, x + w, top + h)
        })
        .collect()
}

/// **The non-regression cell.** Paints `old` and `new` into two rectangles of
/// identical size, labelled, so any difference is visible at a glance and
/// capturable for a pixel diff.
///
/// The two closures receive the same width and height on purpose: if a rebuilt
/// primitive needs a different rectangle to look right, it is not a
/// replacement.
pub fn pair(
    c: &dyn Canvas,
    left: f32,
    top: f32,
    w: f32,
    h: f32,
    old: impl FnOnce(Rect),
    new: impl FnOnce(Rect),
) -> f32 {
    let t = c.theme();
    let f = c.formats();
    let gap = 40.0;

    let a = Rect::new(left, top + CAPTION_H, left + w, top + CAPTION_H + h);
    let b = Rect::new(a.right + gap, a.top, a.right + gap + w, a.bottom);

    c.text(
        "actuel",
        &Rect::new(a.left, top, a.right, top + CAPTION_H),
        &f.caption,
        &t.text_tertiary,
        false,
    );
    c.text(
        "reconstruit",
        &Rect::new(b.left, top, b.right, top + CAPTION_H),
        &f.caption,
        &t.text_tertiary,
        false,
    );

    old(a);
    new(b);
    CAPTION_H + h
}

/// The body text size, for pages that need to size something themselves.
pub const BODY: f32 = text_size::BODY;
