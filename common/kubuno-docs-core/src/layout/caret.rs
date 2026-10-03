//! Caret and selection geometry on the continuous layout (`canvas-engine.ts:2208-2335`,
//! `:2963-3350`): `posToCoords`, `coordsToPos`, `selectionRects`, `adjacentLineCenter`, the word,
//! paragraph and line boundaries, and Ctrl+←/→.
//!
//! Rules carried over (each tested below):
//! * the hit test picks the **line** first (vertical distance, table cells by horizontal
//!   membership first), then the **nearest boundary** in it — a click past the end of a line lands
//!   at its end, a click in the margin at its start;
//! * a prefix width is measured, then rescaled by the span's placed width (`spanPrefixW`), so the
//!   caret follows justified spaces;
//! * an atom (tab, inline image, note reference, field) has exactly two caret sites, its edges;
//! * at a wrap boundary a position is both the end of a line and the start of the next: the
//!   affinity (`prefer_end`) chooses;
//! * the caret is as tall as the text portion at it, not the line box (`caretBox`);
//! * a selection rectangle stops at the end of the line's text, an empty line gets an 8 px ribbon,
//!   consecutive rectangles overlap by 1 px.
//!
//! Divergences, deliberate: list markers are not caret sites (the web lets the caret land on the
//! marker's x at a list item's start); a click never lands inside a surrogate pair or before a
//! combining mark (the web offers every UTF-16 index).

use super::{CursorMetrics, DocPx, DocumentLayout, LayoutLine, LayoutParagraph, LayoutSpan, SelectionRect, CELL_PAD_X, LH_RATIO};
use crate::marks::{TextMark, DEFAULT_PT, PT_PX};
use crate::measure::{line_metrics, Measure};

/// The lean of an italic caret (`italicAngle: 0.13`).
pub const ITALIC_ANGLE: f32 = 0.13;
const EMPTY_LINE_RIBBON: DocPx = 8.0;

fn caret_spans(line: &LayoutLine) -> impl Iterator<Item = &LayoutSpan> {
    line.spans.iter().filter(|s| !s.is_marker())
}

fn usable(line: &LayoutLine) -> bool {
    !line.phantom && !line.no_caret
}

/// `spanPrefixW`: the advance of the first `chars` UTF-16 units of a span, rescaled by its placed
/// width.
pub fn span_prefix_w(span: &LayoutSpan, chars: usize, m: &dyn Measure) -> DocPx {
    if chars == 0 {
        return 0.0;
    }
    let units: Vec<u16> = span.text.encode_utf16().collect();
    if chars >= units.len() {
        return span.width;
    }
    let full = m.width(&span.text, &span.marks);
    let mut end = chars;
    // Never measure half a surrogate pair.
    if String::from_utf16(&units[..end]).is_err() {
        end -= 1;
    }
    let pre = String::from_utf16(&units[..end]).map(|s| m.width(&s, &span.marks)).unwrap_or(0.0);
    if full > 0.0 {
        pre * (span.width / full)
    } else {
        0.0
    }
}

fn span_dx(span: &LayoutSpan, pos: usize, m: &dyn Measure) -> DocPx {
    if span.is_atom() {
        if pos > span.pm_pos { span.width } else { 0.0 }
    } else {
        span_prefix_w(span, pos - span.pm_pos, m)
    }
}

/// `xAtPosInLine`.
pub fn x_at_pos_in_line(line: &LayoutLine, pos: usize, m: &dyn Measure) -> DocPx {
    let mut spans = caret_spans(line).peekable();
    if let Some(first) = spans.peek() {
        if pos <= first.pm_pos {
            return first.x;
        }
    }
    for span in caret_spans(line) {
        let end = span.pm_pos + span.pm_len();
        if pos >= span.pm_pos && pos <= end {
            return span.x + span_dx(span, pos, m);
        }
    }
    match caret_spans(line).last() {
        Some(last) => last.x + last.width,
        None => line.caret_x.unwrap_or(0.0),
    }
}

/// `caretBox`: the text portion's box at the caret, inside the line box.
fn caret_box(marks: Option<&TextMark>, line: &LayoutLine, m: &dyn Measure) -> (DocPx, DocPx) {
    let default = TextMark::default();
    let lm = line_metrics(m, marks.unwrap_or(&default));
    let h = lm.ascent + lm.descent;
    if h >= line.height {
        return (line.y, line.height);
    }
    (line.baseline - lm.ascent, h)
}

fn metrics_at_end(line: &LayoutLine, m: &dyn Measure) -> CursorMetrics {
    let last = caret_spans(line).last();
    let empty_x = line.caret_x.unwrap_or_else(|| line.cell_x.map(|x| x + CELL_PAD_X).unwrap_or(0.0));
    let (top, height) = caret_box(last.map(|s| &s.marks), line, m);
    CursorMetrics {
        x: last.map(|s| s.x + s.width).unwrap_or(empty_x),
        y: top,
        height,
        italic_angle: if last.is_some_and(|s| s.marks.italic) { ITALIC_ANGLE } else { 0.0 },
        baseline: Some(line.baseline),
        line_top: Some(line.y),
        line_h: Some(line.height),
    }
}

/// `posToCoords`. `prefer_end`: at a wrap boundary, the end of the upper line (End key) rather
/// than the start of the lower one.
pub fn pos_to_coords(layout: &DocumentLayout, pos: usize, prefer_end: bool, m: &dyn Measure) -> CursorMetrics {
    for para in &layout.paragraphs {
        for (li, line) in para.lines.iter().enumerate() {
            if !usable(line) || pos < line.pm_start || pos > line.pm_end {
                continue;
            }
            if !prefer_end && pos == line.pm_end {
                if let Some(next) = para.lines.get(li + 1) {
                    if next.pm_start == pos && usable(next) {
                        continue;
                    }
                }
            }
            for span in caret_spans(line) {
                let end = span.pm_pos + span.pm_len();
                if pos < span.pm_pos || pos > end {
                    continue;
                }
                let dx = span_dx(span, pos, m);
                let (top, height) = caret_box(Some(&span.marks), line, m);
                return CursorMetrics {
                    x: span.x + dx,
                    y: top,
                    height,
                    italic_angle: if span.marks.italic { ITALIC_ANGLE } else { 0.0 },
                    baseline: Some(line.baseline),
                    line_top: Some(line.y),
                    line_h: Some(line.height),
                };
            }
            return metrics_at_end(line, m);
        }
    }
    if let Some(last) = layout.paragraphs.iter().rev().flat_map(|p| p.lines.iter().rev()).find(|l| usable(l)) {
        return metrics_at_end(last, m);
    }
    CursorMetrics { x: 0.0, y: 0.0, height: DEFAULT_PT * PT_PX * LH_RATIO, ..Default::default() }
}

/// `adjacentLineCenter`: the centre of the nearest line above (`dir < 0`) or below a line top.
pub fn adjacent_line_center(layout: &DocumentLayout, from_top: DocPx, dir: i32) -> Option<DocPx> {
    let mut best: Option<(DocPx, DocPx)> = None;
    for para in &layout.paragraphs {
        for line in &para.lines {
            if !usable(line) {
                continue;
            }
            let t = line.y;
            let candidate = if dir > 0 { t > from_top + 0.5 } else { t < from_top - 0.5 };
            if candidate {
                let better = match best {
                    None => true,
                    Some((bt, _)) => if dir > 0 { t < bt } else { t > bt },
                };
                if better {
                    best = Some((t, line.height));
                }
            }
        }
    }
    best.map(|(t, h)| t + h / 2.0)
}

/// Whether a caret may sit before UTF-16 unit `i` of a span's text.
fn caret_site_ok(units: &[u16], i: usize) -> bool {
    i == 0 || i >= units.len() || !super::paragraph::splits_a_cluster(units[i])
}

/// `coordsToPos`.
pub fn coords_to_pos(layout: &DocumentLayout, x: DocPx, y: DocPx, m: &dyn Measure) -> usize {
    let mut best: Option<&LayoutLine> = None;
    let mut best_score = f32::INFINITY;
    for para in &layout.paragraphs {
        for line in &para.lines {
            if !usable(line) {
                continue;
            }
            let dy = if y >= line.y && y <= line.y + line.height { 0.0 } else { (y - line.y).abs().min((y - line.y - line.height).abs()) };
            let mut dx = 0.0;
            if let (Some(cx), Some(cw)) = (line.cell_x, line.cell_w) {
                if x < cx {
                    dx = cx - x;
                } else if x > cx + cw {
                    dx = x - (cx + cw);
                }
            }
            let score = dx * 100000.0 + dy;
            if score < best_score {
                best_score = score;
                best = Some(line);
            }
        }
    }
    let Some(line) = best else {
        return layout.paragraphs.last().and_then(|p| p.lines.last()).map(|l| l.pm_end).unwrap_or(1);
    };
    let mut best_pos = line.pm_start;
    let mut best_d = f32::INFINITY;
    for span in caret_spans(line) {
        let len = span.pm_len();
        let units: Vec<u16> = span.text.encode_utf16().collect();
        for i in 0..=len {
            if !span.is_atom() && !caret_site_ok(&units, i) {
                continue;
            }
            let cx = span.x + if span.is_atom() { if i > 0 { span.width } else { 0.0 } } else { span_prefix_w(span, i, m) };
            let d = (x - cx).abs();
            if d < best_d {
                best_d = d;
                best_pos = span.pm_pos + i;
            }
        }
    }
    best_pos
}

/// `selectionRects` for `[from, to)`.
pub fn selection_rects(layout: &DocumentLayout, from: usize, to: usize, m: &dyn Measure) -> Vec<SelectionRect> {
    if from >= to {
        return Vec::new();
    }
    struct Sel {
        x1: DocPx,
        x2: DocPx,
        y: DocPx,
        h: DocPx,
    }
    let mut sel = Vec::new();
    for para in &layout.paragraphs {
        for line in &para.lines {
            if line.phantom {
                continue;
            }
            let empty = line.pm_start == line.pm_end;
            let outside = if empty { line.pm_end < from || line.pm_start > to } else { line.pm_end <= from || line.pm_start >= to };
            if outside || line.image.is_some() || line.no_caret {
                continue;
            }
            let x1 = x_at_pos_in_line(line, from.max(line.pm_start), m);
            let text_end = caret_spans(line).last().map(|s| s.x + s.width).unwrap_or(x1);
            let mut x2 = if to < line.pm_end { x_at_pos_in_line(line, to, m) } else { text_end };
            if x2 <= x1 {
                x2 = x1 + EMPTY_LINE_RIBBON;
            }
            sel.push(Sel { x1, x2, y: line.y, h: line.height });
        }
    }
    let mut rects = Vec::with_capacity(sel.len());
    for i in 0..sel.len() {
        let s = &sel[i];
        let h = if i + 1 < sel.len() { s.h.max(sel[i + 1].y - s.y) + 1.0 } else { s.h };
        rects.push(SelectionRect { x: s.x1, y: s.y, w: s.x2 - s.x1, h });
    }
    rects
}

/// `ATOM_CHAR`: an atom's stand-in in a paragraph's flat text — invisible and not a word char.
const ATOM_CHAR: char = '\u{2063}';

/// `paraFlatText`: the paragraph's text, one char per UTF-16 unit position, and each one's
/// position. (Chars are UTF-16 units here, so astral characters are two entries, as in JavaScript.)
fn para_flat_text(para: &LayoutParagraph) -> (Vec<u16>, Vec<usize>) {
    let mut text = Vec::new();
    let mut pos_of = Vec::new();
    for line in &para.lines {
        for span in &line.spans {
            let len = span.pm_len();
            let units: Vec<u16> = span.text.encode_utf16().collect();
            if len != units.len() {
                for i in 0..len {
                    pos_of.push(span.pm_pos + i);
                    text.push(ATOM_CHAR as u16);
                }
                continue;
            }
            for (i, u) in units.iter().enumerate() {
                pos_of.push(span.pm_pos + i);
                text.push(*u);
            }
        }
    }
    (text, pos_of)
}

/// `WORD_CHAR` (`[\p{L}\p{M}\p{N}_]`) on one UTF-16 unit (surrogates count as letters: an astral
/// character inside a word stays in it).
fn is_word_unit(u: u16) -> bool {
    if (0xD800..=0xDFFF).contains(&u) {
        return true;
    }
    match char::from_u32(u as u32) {
        Some(c) => c.is_alphanumeric() || c == '_' || super::paragraph::splits_a_cluster(u),
        None => false,
    }
}

/// `wordBoundariesAt`: the word under `pos` (double-click), or `[pos, pos]`.
pub fn word_boundaries_at(layout: &DocumentLayout, pos: usize) -> (usize, usize) {
    for para in &layout.paragraphs {
        if pos < para.pm_start + 1 || pos > para.pm_end {
            continue;
        }
        let (text, pos_of) = para_flat_text(para);
        if text.is_empty() {
            return (pos, pos);
        }
        let n = text.len();
        let mut offset = pos_of.iter().position(|&p| p >= pos).unwrap_or(n);
        if offset > n {
            offset = n;
        }
        let is_w = |i: isize| i >= 0 && (i as usize) < n && is_word_unit(text[i as usize]);
        let contig = |i: usize| i > 0 && i < pos_of.len() && pos_of[i] == pos_of[i - 1] + 1;
        let extend = |start: usize| -> (usize, usize) {
            let (mut lo, mut hi) = (start, start);
            while lo > 0 && is_w(lo as isize - 1) && contig(lo) {
                lo -= 1;
            }
            while hi < n && is_w(hi as isize) && (hi == start || contig(hi)) {
                hi += 1;
            }
            (lo, hi)
        };
        let (mut lo, mut hi) = extend(offset);
        if lo == hi {
            let start = if offset > 0 && contig(offset) { offset - 1 } else { offset };
            (lo, hi) = extend(start);
            if lo == hi {
                return (pos, pos);
            }
        }
        return (pos_of[lo], pos_of[hi - 1] + 1);
    }
    (pos, pos)
}

/// `paragraphBoundariesAt` (the layout's paragraph; a table cell's paragraph is the editor's job,
/// see `editor::paragraph_range_at`).
pub fn paragraph_boundaries_at(layout: &DocumentLayout, pos: usize) -> (usize, usize) {
    for para in &layout.paragraphs {
        if pos < para.pm_start || pos > para.pm_end {
            continue;
        }
        return (para.pm_start + 1, para.pm_end);
    }
    (pos, pos)
}

fn line_containing(layout: &DocumentLayout, pos: usize, prefer_end: bool) -> Option<&LayoutLine> {
    for para in &layout.paragraphs {
        for (li, line) in para.lines.iter().enumerate() {
            if !usable(line) || pos < line.pm_start || pos > line.pm_end {
                continue;
            }
            if !prefer_end && pos == line.pm_end {
                if let Some(next) = para.lines.get(li + 1) {
                    if next.pm_start == pos && usable(next) {
                        continue;
                    }
                }
            }
            return Some(line);
        }
    }
    None
}

/// `lineStartAt` (Home).
pub fn line_start_at(layout: &DocumentLayout, pos: usize, prefer_end: bool) -> usize {
    if let Some(line) = line_containing(layout, pos, prefer_end) {
        return caret_spans(line).next().map(|s| s.pm_pos).unwrap_or(line.pm_start);
    }
    doc_start(layout)
}

/// `lineEndAt` (End).
pub fn line_end_at(layout: &DocumentLayout, pos: usize, prefer_end: bool) -> usize {
    if let Some(line) = line_containing(layout, pos, prefer_end) {
        return line.pm_end;
    }
    doc_end(layout)
}

/// `docStart` (Ctrl+Home).
pub fn doc_start(layout: &DocumentLayout) -> usize {
    layout.paragraphs.iter().flat_map(|p| p.lines.iter()).find(|l| usable(l)).map(|l| l.pm_start).unwrap_or(1)
}

/// `docEnd` (Ctrl+End).
pub fn doc_end(layout: &DocumentLayout) -> usize {
    layout.paragraphs.iter().rev().flat_map(|p| p.lines.iter().rev()).find(|l| usable(l)).map(|l| l.pm_end).unwrap_or(1)
}

fn para_at(layout: &DocumentLayout, pos: usize) -> Option<&LayoutParagraph> {
    layout.paragraphs.iter().find(|p| pos > p.pm_start && pos <= p.pm_end && p.lines.iter().any(usable))
}

/// `prevWordPos` (Ctrl+←).
pub fn prev_word_pos(layout: &DocumentLayout, pos: usize) -> usize {
    let Some(para) = para_at(layout, pos) else { return doc_start(layout) };
    let (text, pos_of) = para_flat_text(para);
    // The flat index of `pos` (positions jump inside tables, so derive it from the map).
    let mut i = pos_of.iter().position(|&p| p >= pos).unwrap_or(text.len());
    let is_w = |k: usize| k < text.len() && is_word_unit(text[k]);
    while i > 0 && !is_w(i - 1) {
        i -= 1;
    }
    while i > 0 && is_w(i - 1) {
        i -= 1;
    }
    if i == 0 {
        para.lines.iter().find(|l| usable(l)).map(|l| l.pm_start).unwrap_or(para.pm_start + 1)
    } else {
        pos_of[i]
    }
}

/// `nextWordPos` (Ctrl+→).
pub fn next_word_pos(layout: &DocumentLayout, pos: usize) -> usize {
    let Some(para) = para_at(layout, pos) else { return doc_end(layout) };
    let (text, pos_of) = para_flat_text(para);
    let mut i = pos_of.iter().position(|&p| p >= pos).unwrap_or(text.len());
    let is_w = |k: usize| k < text.len() && is_word_unit(text[k]);
    while is_w(i) {
        i += 1;
    }
    while i < text.len() && !is_w(i) {
        i += 1;
    }
    if i >= text.len() {
        para.lines.iter().rev().find(|l| usable(l)).map(|l| l.pm_end).unwrap_or(para.pm_end)
    } else {
        pos_of[i]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::flow::Flow;
    use crate::measure::FixedMeasure;
    use crate::model::Node;

    const CH: f32 = 11.0 * 96.0 / 72.0 * 0.6;

    fn layout(json: &str, width: f32) -> DocumentLayout {
        let d = Node::from_slice(json.as_bytes()).expect("parses");
        Flow::new().layout(&d, &[], &|_| width, &FixedMeasure)
    }

    const P2: &str = r#"{"content":[{"content":[{"text":"hello world","type":"text"}],"type":"paragraph"},{"content":[{"text":"second","type":"text"}],"type":"paragraph"}],"type":"doc"}"#;

    #[test]
    fn coords_and_positions_are_inverses() {
        let l = layout(P2, 600.0);
        for pos in [1usize, 3, 6, 12, 14, 20] {
            let c = pos_to_coords(&l, pos, false, &FixedMeasure);
            let back = coords_to_pos(&l, c.x, c.y + c.height / 2.0, &FixedMeasure);
            assert_eq!(back, pos, "position {pos} at ({}, {})", c.x, c.y);
        }
    }

    #[test]
    fn a_click_past_the_end_of_a_line_lands_at_its_end_and_in_the_margin_at_its_start() {
        let l = layout(P2, 600.0);
        let y = pos_to_coords(&l, 1, false, &FixedMeasure).y + 2.0;
        assert_eq!(coords_to_pos(&l, 590.0, y, &FixedMeasure), 12);
        assert_eq!(coords_to_pos(&l, -50.0, y, &FixedMeasure), 1);
    }

    #[test]
    fn the_wrap_boundary_follows_the_affinity() {
        // "hello world" wraps after "hello " at this width.
        let l = layout(P2, CH * 7.0);
        let first = &l.paragraphs[0].lines[0];
        let boundary = first.pm_end;
        let down = pos_to_coords(&l, boundary, false, &FixedMeasure);
        let up = pos_to_coords(&l, boundary, true, &FixedMeasure);
        assert!(down.y > up.y, "start of the next line vs end of this one");
        assert_eq!(line_end_at(&l, 1, false), boundary);
        assert_eq!(line_start_at(&l, boundary, false), boundary);
        assert_eq!(line_start_at(&l, boundary, true), 1);
    }

    #[test]
    fn up_and_down_aim_at_the_adjacent_line() {
        let l = layout(P2, 600.0);
        let c = pos_to_coords(&l, 3, false, &FixedMeasure);
        let y = adjacent_line_center(&l, c.line_top.unwrap_or(c.y), 1).expect("a line below");
        let pos = coords_to_pos(&l, c.x, y, &FixedMeasure);
        assert_eq!(pos, 16, "same column in the next paragraph");
        assert!(adjacent_line_center(&l, c.line_top.unwrap_or(c.y), -1).is_none());
    }

    #[test]
    fn words_and_paragraphs() {
        let l = layout(P2, 600.0);
        assert_eq!(word_boundaries_at(&l, 3), (1, 6));
        assert_eq!(word_boundaries_at(&l, 9), (7, 12));
        // On the space: the word to the left.
        assert_eq!(word_boundaries_at(&l, 6), (1, 6));
        assert_eq!(paragraph_boundaries_at(&l, 3), (1, 12));
        assert_eq!(next_word_pos(&l, 1), 7);
        assert_eq!(next_word_pos(&l, 7), 12);
        assert_eq!(prev_word_pos(&l, 12), 7);
        assert_eq!(prev_word_pos(&l, 7), 1);
        assert_eq!((doc_start(&l), doc_end(&l)), (1, 20));
    }

    #[test]
    fn selection_rects_cover_lines_and_stop_at_the_text() {
        let l = layout(P2, 600.0);
        let rects = selection_rects(&l, 3, 16, &FixedMeasure);
        assert_eq!(rects.len(), 2);
        assert!((rects[0].x - 2.0 * CH).abs() < 1e-3);
        assert!((rects[0].x + rects[0].w - 11.0 * CH).abs() < 1e-3);
        assert!((rects[1].w - 2.0 * CH).abs() < 1e-3);
        // Continuous: the first reaches the second.
        assert!(rects[0].y + rects[0].h >= rects[1].y);
    }

    #[test]
    fn an_empty_paragraph_selected_shows_a_ribbon() {
        let l = layout(r#"{"content":[{"type":"paragraph"},{"content":[{"text":"x","type":"text"}],"type":"paragraph"}],"type":"doc"}"#, 600.0);
        let rects = selection_rects(&l, 1, 4, &FixedMeasure);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].w, EMPTY_LINE_RIBBON);
    }

    #[test]
    fn a_click_never_lands_inside_a_surrogate_pair() {
        let l = layout(r#"{"content":[{"content":[{"text":"a😀b","type":"text"}],"type":"paragraph"}],"type":"doc"}"#, 600.0);
        for x in 0..60 {
            let pos = coords_to_pos(&l, x as f32, 5.0, &FixedMeasure);
            assert_ne!(pos, 3, "inside the pair at x {x}");
        }
    }
}
