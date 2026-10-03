//! Painting the page canvas: the backdrop, the sheets, and the core's pages on them — a port of
//! `paintLayout` / `renderDocument` (`canvas-engine.ts:2418-2960`) onto Direct2D and DirectWrite.
//!
//! Every page is drawn under one transform (`scale(zoom)` then its place in the view), in
//! **document pixels**, its content box at the page's margins: scrolling and zooming move the
//! transform, never the content, so a cached layout stays valid across both.
//!
//! What is painted, in the web's order: table cell grounds, then each line's spans (highlight
//! ground, glyphs at the baseline — raised or lowered for super/subscript —, underline and strike
//! as 1 px bars at `baseline + 2` and `baseline − 0.35 ascent`), inline and block images, table
//! borders resolved per edge, then the selection over everything (translucent, the union filled
//! once: `rgba(87,133,253,0.5)` focused, `rgba(179,179,179,0.5)` not), then the caret.

use drive_app_controls::geometry::Rect;
use kubuno_docs_core::layout::tables::{self, BorderStyle, CellFill};
use kubuno_docs_core::layout::{LayoutLine, LayoutParagraph, LayoutSpan, LayoutTable, PageLayout, SelectionRect, SpanKind};
use kubuno_docs_core::marks::{Script, TextMark, PT_PX};
use kubuno_docs_core::measure::Measure;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows_numerics::Matrix3x2;

use super::fonts::Fonts;
use crate::model::state::PagePlace;
use crate::platform::images::ImageCache;
use crate::platform::painter::Painter;

/// Word's canvas grey (the light theme's backdrop).
pub const BACKDROP: D2D1_COLOR_F = rgb(0xE6, 0xE6, 0xE6);
const PAPER: D2D1_COLOR_F = rgb(0xFF, 0xFF, 0xFF);
/// The page edge: a flat outline, no shadow (measured off Word).
pub const PAGE_EDGE: D2D1_COLOR_F = rgb(0xD0, 0xD0, 0xD0);
/// `DEFAULT_CLR`: pure black ink.
const INK: D2D1_COLOR_F = rgb(0x00, 0x00, 0x00);
/// The selection over the text (`renderDocument`).
const SEL_FOCUSED: D2D1_COLOR_F = D2D1_COLOR_F { r: 87.0 / 255.0, g: 133.0 / 255.0, b: 253.0 / 255.0, a: 0.5 };
const SEL_BLURRED: D2D1_COLOR_F = D2D1_COLOR_F { r: 179.0 / 255.0, g: 179.0 / 255.0, b: 179.0 / 255.0, a: 0.5 };
/// An image that is not loaded (`#f1f3f4` with a `#dadce0` edge).
const IMG_PLACEHOLDER: D2D1_COLOR_F = rgb(0xF1, 0xF3, 0xF4);
const IMG_PLACEHOLDER_EDGE: D2D1_COLOR_F = rgb(0xDA, 0xDC, 0xE0);

pub const fn rgb(r: u8, g: u8, b: u8) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
}

/// A CSS colour: `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb()`/`rgba()`, and the named colours documents
/// and pasted HTML actually use. `None` for anything else (the caller keeps its default).
pub fn css_color(s: &str) -> Option<D2D1_COLOR_F> {
    let s = s.trim().to_ascii_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        let n = |i: usize, len: usize| u8::from_str_radix(&hex[i..i + len], 16).ok();
        return match hex.len() {
            3 => Some(rgb(n(0, 1)? * 17, n(1, 1)? * 17, n(2, 1)? * 17)),
            6 => Some(rgb(n(0, 2)?, n(2, 2)?, n(4, 2)?)),
            8 => {
                let mut c = rgb(n(0, 2)?, n(2, 2)?, n(4, 2)?);
                c.a = n(6, 2)? as f32 / 255.0;
                Some(c)
            }
            _ => None,
        };
    }
    if let Some(inner) = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb(")).and_then(|r| r.strip_suffix(')')) {
        let parts: Vec<f32> = inner.split(',').filter_map(|p| p.trim().trim_end_matches('%').parse::<f32>().ok()).collect();
        if parts.len() >= 3 {
            let a = parts.get(3).copied().unwrap_or(1.0);
            return Some(D2D1_COLOR_F { r: parts[0] / 255.0, g: parts[1] / 255.0, b: parts[2] / 255.0, a });
        }
        return None;
    }
    let named = match s.as_str() {
        "black" => rgb(0, 0, 0),
        "white" => rgb(255, 255, 255),
        "red" => rgb(255, 0, 0),
        "green" => rgb(0, 128, 0),
        "lime" => rgb(0, 255, 0),
        "blue" => rgb(0, 0, 255),
        "yellow" => rgb(255, 255, 0),
        "cyan" | "aqua" => rgb(0, 255, 255),
        "magenta" | "fuchsia" => rgb(255, 0, 255),
        "gray" | "grey" => rgb(128, 128, 128),
        "silver" => rgb(192, 192, 192),
        "orange" => rgb(255, 165, 0),
        "purple" => rgb(128, 0, 128),
        "navy" => rgb(0, 0, 128),
        "maroon" => rgb(128, 0, 0),
        "olive" => rgb(128, 128, 0),
        "teal" => rgb(0, 128, 128),
        "transparent" => D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
        _ => return None,
    };
    Some(named)
}

/// What a frame paints with.
pub struct Frame<'a, 'b> {
    pub p: &'a Painter<'b>,
    pub fonts: &'a Fonts,
    pub images: &'a mut ImageCache,
    pub zoom: f32,
    /// Device pixels per DIP.
    pub scale: f32,
    pub edge: D2D1_COLOR_F,
    pub focused: bool,
    /// Affichage › Marques ¶.
    pub show_marks: bool,
}

fn matrix(zoom: f32, x: f32, y: f32) -> Matrix3x2 {
    Matrix3x2 { M11: zoom, M12: 0.0, M21: 0.0, M22: zoom, M31: x, M32: y }
}

fn translate(x: f32, y: f32) -> Matrix3x2 {
    Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: x, M32: y }
}

/// One page: its sheet at `place` (screen DIP), its content at the margins, then the selection
/// rectangles and the caret given in content-box coordinates.
#[allow(clippy::too_many_arguments)]
pub fn draw_page(
    f: &mut Frame<'_, '_>,
    page: &PageLayout,
    place: &PagePlace,
    margins: (f32, f32),
    size: (f32, f32),
    selection: &[SelectionRect],
    caret: Option<(f32, f32, f32, f32)>,
) {
    let _page = f.p.push_transform(matrix(place.w / size.0.max(1.0), place.x, place.y));
    let paper = Rect::new(0.0, 0.0, size.0, size.1);
    f.p.fill_rect_raw(&paper, &PAPER);
    f.p.stroke_rect_raw(&paper, &f.edge, 1.0 / f.zoom.max(0.01));
    f.p.push_clip_page(&paper);
    {
        let _content = f.p.push_transform(translate(margins.0, margins.1));
        for para in &page.paragraphs {
            if let Some(t) = &para.table {
                draw_table_grounds(f, t);
            }
        }
        for para in &page.paragraphs {
            draw_paragraph(f, para);
        }
        for para in &page.paragraphs {
            if let Some(t) = &para.table {
                draw_table_borders(f, t);
            }
        }
        if f.show_marks {
            draw_pilcrows(f, page);
        }
        if !selection.is_empty() {
            let rects: Vec<Rect> = selection.iter().map(|r| Rect::new(r.x, r.y, r.x + r.w, r.y + r.h)).collect();
            f.p.fill_union_raw(&rects, if f.focused { &SEL_FOCUSED } else { &SEL_BLURRED });
        }
        if let Some((x, y, h, lean)) = caret {
            // One device pixel wide, whatever the zoom.
            let w = 1.0 / (f.scale * f.zoom).max(0.01);
            f.p.draw_caret_raw(x, y, h, w, lean, &INK);
        }
    }
    f.p.pop_clip_page();
}

fn draw_paragraph(f: &mut Frame<'_, '_>, para: &LayoutParagraph) {
    for line in &para.lines {
        if let Some(img) = &line.image {
            let top = line.y + if img.wrap != "inline" { img.wrap_y } else { 0.0 };
            draw_image(f, &img.src, &Rect::new(img.x, top, img.x + img.w, top + img.h), img.rotation);
            continue;
        }
        for span in &line.spans {
            draw_span(f, line, span);
        }
    }
}

/// The paragraph marks (« Marques ¶ »): a ¶ after the last line of every paragraph fragment of the
/// page, Arial 12 pt in `rgba(26,115,232,0.55)` (`DocumentEditorPage.tsx`, `marksRef`).
fn draw_pilcrows(f: &mut Frame<'_, '_>, page: &PageLayout) {
    const MARK: D2D1_COLOR_F = D2D1_COLOR_F { r: 26.0 / 255.0, g: 115.0 / 255.0, b: 232.0 / 255.0, a: 0.55 };
    let m = TextMark { font_size: Some(12.0), ..Default::default() };
    let Some(format) = f.fonts.format(&m) else { return };
    let (ascent, _) = f.fonts.ascent_descent(&m);
    for para in &page.paragraphs {
        if para.table.is_some() || para.lines.iter().any(|l| l.phantom) {
            continue;
        }
        let Some(last) = para.lines.last() else { continue };
        if last.no_caret {
            continue;
        }
        let end_x = last.spans.last().map(|s| s.x + s.width).or(last.caret_x).unwrap_or(0.0);
        let r = Rect::new(end_x + 1.0, last.baseline - ascent, end_x + 30.0, last.baseline + ascent);
        f.p.draw_text_raw("¶", &format, &r, &MARK);
    }
}

fn draw_image(f: &mut Frame<'_, '_>, src: &str, rect: &Rect, rotation: f32) {
    match f.images.bitmap(f.p.ctx, src) {
        Some(bmp) => f.p.draw_bitmap_raw(&bmp, rect, rotation),
        None => {
            f.p.fill_rect_raw(rect, &IMG_PLACEHOLDER);
            f.p.stroke_rect_raw(rect, &IMG_PLACEHOLDER_EDGE, 1.0);
        }
    }
}

fn ink_of(m: &TextMark) -> D2D1_COLOR_F {
    m.color.as_deref().and_then(css_color).unwrap_or(INK)
}

fn draw_span(f: &mut Frame<'_, '_>, line: &LayoutLine, span: &LayoutSpan) {
    if let SpanKind::Image(img) = &span.kind {
        let rect = Rect::new(span.x, line.baseline - img.h, span.x + img.w, line.baseline);
        draw_image(f, &img.src, &rect, img.rot);
        return;
    }
    let m = &span.marks;
    // The highlight ground covers the whole line box under the span (`fillRect(x, line.y, w, h)`).
    if let Some(bg) = m.background.as_deref().and_then(css_color) {
        f.p.fill_rect_raw(&Rect::new(span.x, line.y, span.x + span.width, line.y + line.height), &bg);
    }
    let base_px = m.size_pt() * PT_PX;
    let script_dy = match m.script {
        Some(Script::Super) => -base_px * 0.36,
        Some(Script::Sub) => base_px * 0.18,
        None => 0.0,
    };
    let baseline = line.baseline + script_dy;
    let ink = ink_of(m);
    if !span.text.trim().is_empty() && span.text != "\t" {
        if let Some(format) = f.fonts.format(m) {
            let (ascent, _) = f.fonts.ascent_descent(m);
            if m.letter_spacing != 0.0 {
                // Canvas letter spacing: every character advanced by its width plus the spacing.
                let mut x = span.x;
                let plain = TextMark { letter_spacing: 0.0, ..m.clone() };
                for ch in span.text.chars() {
                    let s = ch.to_string();
                    let w = f.fonts.width(&s, &plain);
                    f.p.draw_text_raw(&s, &format, &Rect::new(x, baseline - ascent, x + w + 4.0, baseline + ascent), &ink);
                    x += w + m.letter_spacing;
                }
            } else {
                let rect = Rect::new(span.x, baseline - ascent, span.x + span.width.max(1.0) + 4.0, baseline + line.height);
                f.p.draw_text_raw(&span.text, &format, &rect, &ink);
            }
        }
    }
    if span.is_marker() {
        return;
    }
    if m.underline || m.insertion {
        f.p.fill_rect_raw(&Rect::new(span.x, baseline + 2.0, span.x + span.width, baseline + 3.0), &ink);
    }
    if m.strike || m.deletion {
        let y = baseline - line.ascent * 0.35;
        f.p.fill_rect_raw(&Rect::new(span.x, y, span.x + span.width, y + 1.0), &ink);
    }
}

fn accent_tint(accent: &str, alpha: f32) -> D2D1_COLOR_F {
    let mut c = css_color(accent).unwrap_or(rgb(0x1a, 0x73, 0xe8));
    // `tint(hex, alpha)`: the accent at that alpha over white.
    c.r = 1.0 - (1.0 - c.r) * alpha;
    c.g = 1.0 - (1.0 - c.g) * alpha;
    c.b = 1.0 - (1.0 - c.b) * alpha;
    c.a = 1.0;
    c
}

fn draw_table_grounds(f: &mut Frame<'_, '_>, t: &LayoutTable) {
    for c in &t.cells {
        let fill = match &c.background {
            CellFill::None => None,
            CellFill::Explicit(css) => css_color(css),
            CellFill::Tint(a) => Some(accent_tint(&t.grid.accent, *a)),
        };
        if let Some(col) = fill {
            let r = Rect::new(c.x + t.dx, c.y + t.dy, c.x + t.dx + c.width, c.y + t.dy + c.height);
            f.p.fill_rect_raw(&r, &col);
        }
    }
}

fn draw_table_borders(f: &mut Frame<'_, '_>, t: &LayoutTable) {
    let min_w = 1.0 / (f.scale * f.zoom).max(0.01);
    for e in tables::edges(&t.grid, &t.cells) {
        if e.spec.width <= 0.0 {
            continue;
        }
        let Some(col) = css_color(&e.spec.color) else { continue };
        let dash = match e.spec.style {
            BorderStyle::Solid => None,
            BorderStyle::Dashed => Some("dashed"),
            BorderStyle::Dotted => Some("dotted"),
        };
        f.p.draw_line_raw(e.x0 + t.dx, e.y0 + t.dy, e.x1 + t.dx, e.y1 + t.dy, &col, e.spec.width.max(min_w), dash);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_colours_parse_as_the_documents_write_them() {
        let c = css_color("#ff0000").expect("hex");
        assert_eq!((c.r, c.g, c.b), (1.0, 0.0, 0.0));
        let c = css_color("#0f0").expect("short hex");
        assert_eq!(c.g, 1.0);
        let c = css_color("rgba(87, 133, 253, 0.5)").expect("rgba");
        assert!((c.a - 0.5).abs() < 1e-6);
        assert!(css_color("yellow").is_some());
        assert!(css_color("var(--x)").is_none());
    }

    #[test]
    fn a_tint_is_the_accent_over_white() {
        let t = accent_tint("#1a73e8", 0.16);
        assert!(t.r > 0.8 && t.b > 0.9);
    }
}
