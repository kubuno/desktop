//! What the engine needs from fonts — the one door to the platform's text system.
//!
//! The engine never breaks a line with the platform's text layout: the web's `canvas-engine.ts`
//! has its own breaker (JavaScript `\s` and tabs only, per-token measurement) and the same file must
//! paginate identically everywhere. A platform only answers two questions — the advance width of a
//! string in a font, and the font's ascent and descent — the way the browser's `measureText`
//! answers them (`width`, `fontBoundingBoxAscent/Descent`). On Windows that is DirectWrite
//! (`documents/src/doc/fonts.rs`); on the web it would be the canvas itself.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::marks::TextMark;

/// `NATURAL_EM` (`canvas-engine.ts:638`): a line is never shorter than 1.2 × the font size.
pub const NATURAL_EM: f32 = 1.20;

pub trait Measure {
    /// The advance width of `text` in `marks`' font (px), letter spacing included.
    fn width(&self, text: &str, marks: &TextMark) -> f32;
    /// The font's own ascent and descent in px (the browser's `fontBoundingBoxAscent/Descent`:
    /// font metrics, line gap excluded).
    fn ascent_descent(&self, marks: &TextMark) -> (f32, f32);
}

/// `lineMetrics` (`:617-642`): ascent, descent and the natural line height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub height: f32,
}

pub fn line_metrics(m: &dyn Measure, marks: &TextMark) -> LineMetrics {
    let fs = marks.size_pt() * crate::marks::PT_PX;
    let (mut asc, mut dsc) = m.ascent_descent(marks);
    if asc <= 0.0 {
        asc = fs * 0.92;
    }
    if dsc <= 0.0 {
        dsc = fs * 0.28;
    }
    LineMetrics { ascent: asc, descent: dsc, height: (fs * NATURAL_EM).max(asc + dsc) }
}

/// A cache in front of any measurer, keyed like the web's (`fontStr + ' ' + text`), purged
/// wholesale at 100 000 entries (`WIDTH_CACHE_MAX`, `:546`).
pub struct MeasureCache<M: Measure> {
    inner: M,
    widths: RefCell<HashMap<(String, String), f32>>,
    metrics: RefCell<HashMap<String, (f32, f32)>>,
}

pub const WIDTH_CACHE_MAX: usize = 100_000;

impl<M: Measure> MeasureCache<M> {
    pub fn new(inner: M) -> Self {
        Self { inner, widths: RefCell::new(HashMap::new()), metrics: RefCell::new(HashMap::new()) }
    }

    pub fn inner(&self) -> &M {
        &self.inner
    }

    /// Drops every cached measurement (fonts changed).
    pub fn clear(&self) {
        self.widths.borrow_mut().clear();
        self.metrics.borrow_mut().clear();
    }
}

impl<M: Measure> Measure for MeasureCache<M> {
    fn width(&self, text: &str, marks: &TextMark) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let key = (marks.font_key(), text.to_string());
        if let Some(&w) = self.widths.borrow().get(&key) {
            return w;
        }
        let w = self.inner.width(text, marks);
        let mut cache = self.widths.borrow_mut();
        if cache.len() >= WIDTH_CACHE_MAX {
            cache.clear();
        }
        cache.insert(key, w);
        w
    }

    fn ascent_descent(&self, marks: &TextMark) -> (f32, f32) {
        let key = marks.font_key();
        if let Some(&m) = self.metrics.borrow().get(&key) {
            return m;
        }
        let m = self.inner.ascent_descent(marks);
        self.metrics.borrow_mut().insert(key, m);
        m
    }
}

/// A deterministic measurer for tests (here and in the platforms' tests): every character is
/// `0.6 em` wide plus the letter spacing, ascent 0.8 em, descent 0.2 em.
pub struct FixedMeasure;

impl Measure for FixedMeasure {
    fn width(&self, text: &str, marks: &TextMark) -> f32 {
        let n = text.encode_utf16().count() as f32;
        n * (marks.font_px() * 0.6 + marks.letter_spacing)
    }
    fn ascent_descent(&self, marks: &TextMark) -> (f32, f32) {
        let fs = marks.font_px();
        (fs * 0.8, fs * 0.2)
    }
}
