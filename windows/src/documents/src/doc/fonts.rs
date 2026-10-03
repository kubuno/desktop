//! The DirectWrite side of the layout engine: text formats, widths, and the font-global metrics
//! the line box is built from — the core's `Measure` on Windows.
//!
//! Everything here is cached, because the breaker measures **per token** and a page of prose is a
//! few hundred of them. The web caches the same things for the same reason
//! (`canvas-engine.ts:599-642`), keyed by its CSS font string; this keys by the same string
//! (`TextMark::font_key`), so "the same font" means the same thing on both sides.
//!
//! Cache lifetime: the whole set is dropped when the font collection changes (stale fallback widths
//! made text overlap on the web after a late `document.fonts.add()`).

use std::cell::RefCell;
use std::collections::HashMap;

use kubuno_office_docs_core::marks::TextMark;
use kubuno_office_docs_core::measure::{Measure, WIDTH_CACHE_MAX};
use windows::core::HSTRING;
use windows::Win32::Graphics::DirectWrite::*;

/// The widest layout box used for measurement. Never `f32::MAX`: DirectWrite's intermediate
/// arithmetic overflows there and returns NaN metrics for some fonts.
const UNBOUNDED: f32 = 65536.0;

pub struct Fonts {
    dwrite: IDWriteFactory,
    formats: RefCell<HashMap<String, IDWriteTextFormat>>,
    widths: RefCell<HashMap<(String, String), f32>>,
    metrics: RefCell<HashMap<String, (f32, f32)>>,
}

fn weight(m: &TextMark) -> DWRITE_FONT_WEIGHT {
    if m.bold { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL }
}

fn style(m: &TextMark) -> DWRITE_FONT_STYLE {
    if m.italic { DWRITE_FONT_STYLE_ITALIC } else { DWRITE_FONT_STYLE_NORMAL }
}

impl Fonts {
    pub fn new(dwrite: IDWriteFactory) -> Self {
        Self { dwrite, formats: RefCell::new(HashMap::new()), widths: RefCell::new(HashMap::new()), metrics: RefCell::new(HashMap::new()) }
    }

    /// The text format for a run's font (created once and reused).
    pub fn format(&self, m: &TextMark) -> Option<IDWriteTextFormat> {
        let key = m.font_key();
        if let Some(found) = self.formats.borrow().get(&key) {
            return Some(found.clone());
        }
        let format = unsafe {
            self.dwrite
                .CreateTextFormat(&HSTRING::from(m.family()), None, weight(m), style(m), DWRITE_FONT_STRETCH_NORMAL, m.font_px(), &HSTRING::from("fr-FR"))
                .ok()?
        };
        unsafe {
            // The breaker owns every break decision; DirectWrite must not make one of its own.
            let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            // The baseline exactly at the font's ascent (what the canvas draws at `fillText(x, baseline)`),
            // whatever line gap DirectWrite's default spacing would add.
            let (asc, desc) = self.font_metrics(m);
            let _ = format.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, asc + desc, asc);
        }
        self.formats.borrow_mut().insert(key, format.clone());
        Some(format)
    }

    /// Drops every cached format, width and metric (the fonts changed).
    pub fn clear(&self) {
        self.formats.borrow_mut().clear();
        self.widths.borrow_mut().clear();
        self.metrics.borrow_mut().clear();
    }

    /// The font's own ascent and descent, in document pixels — the browser's
    /// `fontBoundingBoxAscent/Descent` (line gap excluded).
    fn font_metrics(&self, m: &TextMark) -> (f32, f32) {
        let fs = m.font_px();
        let fallback = (fs * 0.92, fs * 0.28);
        let Some(face) = self.font_face(m) else { return fallback };
        let mut fm = DWRITE_FONT_METRICS::default();
        unsafe { face.GetMetrics(&mut fm) };
        if fm.designUnitsPerEm == 0 {
            return fallback;
        }
        let per_em = fm.designUnitsPerEm as f32;
        (fm.ascent as f32 / per_em * fs, fm.descent as f32 / per_em * fs)
    }

    fn font_face(&self, m: &TextMark) -> Option<IDWriteFontFace> {
        unsafe {
            let mut collection: Option<IDWriteFontCollection> = None;
            self.dwrite.GetSystemFontCollection(&mut collection, false).ok()?;
            let collection = collection?;
            let mut index = 0u32;
            let mut exists = windows::core::BOOL::default();
            collection.FindFamilyName(&HSTRING::from(m.family()), &mut index, &mut exists).ok()?;
            if !exists.as_bool() {
                return None;
            }
            let family = collection.GetFontFamily(index).ok()?;
            let font = family.GetFirstMatchingFont(weight(m), DWRITE_FONT_STRETCH_NORMAL, style(m)).ok()?;
            font.CreateFontFace().ok()
        }
    }
}

impl Measure for Fonts {
    fn width(&self, text: &str, m: &TextMark) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let key = (m.font_key(), text.to_string());
        if let Some(&w) = self.widths.borrow().get(&key) {
            return w;
        }
        let Some(format) = self.format(m) else { return 0.0 };
        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut width = unsafe {
            match self.dwrite.CreateTextLayout(&wide, &format, UNBOUNDED, UNBOUNDED) {
                Ok(layout) => {
                    let mut tm = DWRITE_TEXT_METRICS::default();
                    if layout.GetMetrics(&mut tm).is_ok() {
                        // Trailing whitespace included: a whitespace token is measured whole.
                        tm.widthIncludingTrailingWhitespace
                    } else {
                        0.0
                    }
                }
                Err(_) => 0.0,
            }
        };
        // Canvas `letterSpacing` adds after every character (the last one included).
        if m.letter_spacing != 0.0 {
            width += m.letter_spacing * text.chars().count() as f32;
        }
        let mut cache = self.widths.borrow_mut();
        if cache.len() >= WIDTH_CACHE_MAX {
            cache.clear();
        }
        cache.insert(key, width);
        width
    }

    fn ascent_descent(&self, m: &TextMark) -> (f32, f32) {
        let key = m.font_key();
        if let Some(&v) = self.metrics.borrow().get(&key) {
            return v;
        }
        let v = self.font_metrics(m);
        self.metrics.borrow_mut().insert(key, v);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno_office_docs_core::layout::{paragraph, parity};

    fn fonts() -> Option<Fonts> {
        let factory: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()? };
        Some(Fonts::new(factory))
    }

    /// The browser's lines are reproduced with the real fonts (the recording is in the core's
    /// fixtures; without DirectWrite or a recording the test says so and skips).
    #[test]
    fn the_browsers_lines_are_reproduced() {
        let Some(f) = fonts() else {
            eprintln!("parity: no DirectWrite factory, skipped");
            return;
        };
        let cases = parity::recorded();
        if cases.is_empty() {
            eprintln!("parity: no recording, skipped");
            return;
        }
        let mut failures = Vec::new();
        for case in &cases {
            let para = match parity::paragraph_of(case) {
                Ok(p) => p,
                Err(why) => {
                    failures.push(format!("{}: {why}", case.name));
                    continue;
                }
            };
            let lines = paragraph::layout_paragraph(&para, case.width, &f);
            for d in parity::compare(case, &lines) {
                failures.push(format!("{}: {d}", case.name));
            }
        }
        assert!(failures.is_empty(), "{} disagreements:\n{}", failures.len(), failures.join("\n"));
    }

    #[test]
    fn a_wider_font_measures_wider_and_spacing_adds_per_character() {
        let Some(f) = fonts() else { return };
        let plain = TextMark::default();
        let bold = TextMark { bold: true, ..Default::default() };
        let w = f.width("Kubuno", &plain);
        assert!(w > 20.0);
        assert!(f.width("Kubuno", &bold) > w);
        let spaced = TextMark { letter_spacing: 2.0, ..Default::default() };
        assert!((f.width("Kubuno", &spaced) - (w + 12.0)).abs() < 0.01);
    }
}
