//! `layoutParagraph` (`canvas-engine.ts:1832-2205`): one render paragraph → its lines.
//!
//! # Why this is not the platform's text layout
//!
//! The web does not break lines the way any text engine does, and the same file must paginate
//! identically everywhere. Its only break opportunities are runs of JavaScript `\s` and the tab
//! (`:1934-1947`): no break after a hyphen, none between CJK characters, no UAX #14. It measures
//! **per token**, so kerning never crosses a word boundary. This is a transcription, quirks
//! included; a place where the web looks wrong but is still followed is marked **QUIRK**.
//!
//! (The breaker and its helpers come from the desktop's first port, `documents/src/doc/para.rs`,
//! rebased on ProseMirror positions and the web's span model.)
//!
//! Not ported: floating-object exclusion (multi-segment rows) and drop caps — see the module doc
//! of [`super`].

use super::{
    Align, LayoutLine, LayoutSpan, LineImage, LineSpacingMode, RenderParagraph, SpanKind,
};
use super::images::img_aabb;
use crate::marks::TextMark;
use crate::measure::{line_metrics, Measure};

/// `DEFAULT_TAB` (`:1853`).
pub const DEFAULT_TAB: f32 = 48.0;
/// The fit tolerance (`:2158`).
const FIT_SLACK: f32 = 0.5;
/// `MIN_SEG_W` (`:1069`).
pub const MIN_SEG_W: f32 = 40.0;
/// The minimum advance of a tab (`:2093`).
const MIN_TAB_ADVANCE: f32 = 2.0;

/// JavaScript's `\s`, exactly: U+00A0 is a break opportunity and a stretchable space, U+000A is an
/// ordinary space, U+200B is **not** a space.
pub fn is_js_space(c: char) -> bool {
    matches!(c,
        '\u{0009}' | '\u{000A}' | '\u{000B}' | '\u{000C}' | '\u{000D}' | '\u{0020}'
        | '\u{00A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}'
        | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}')
}

/// Whether cutting before UTF-16 unit `u` would split a grapheme: a low surrogate or a combining
/// mark (`:2124-2128`; `\p{M}` approximated by the common ranges). The caret uses the same rule.
pub fn splits_a_cluster(u: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&u)
        || matches!(u,
            0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2
            | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06DC
            | 0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E
            | 0x0F71..=0x0F84 | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF
            | 0x20D0..=0x20F0 | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F)
}

/// `text.split(/(sep)/g)`: every separator its own piece.
fn split_each(text: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if c == sep {
            if start < i {
                parts.push(&text[start..i]);
            }
            parts.push(&text[i..i + c.len_utf8()]);
            start = i + c.len_utf8();
        }
    }
    if start < text.len() {
        parts.push(&text[start..]);
    }
    parts
}

/// `text.split(/(\s+)/g)`: maximal runs of spaces kept as pieces.
pub fn split_keeping_spaces(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut current: Option<bool> = None;
    for (i, c) in text.char_indices() {
        let sep = is_js_space(c);
        match current {
            Some(prev) if prev == sep => {}
            Some(_) => {
                parts.push(&text[start..i]);
                start = i;
            }
            None => {}
        }
        current = Some(sep);
    }
    if start < text.len() {
        parts.push(&text[start..]);
    }
    parts
}

/// The next tab stop strictly after `x` (`:1855-1858`): a custom stop, else the 48 px grid
/// anchored at the content origin.
pub fn next_tab_stop(stops: Option<&[f32]>, x: f32) -> f32 {
    if let Some(stops) = stops {
        for &stop in stops {
            if stop > x + FIT_SLACK {
                return stop;
            }
        }
    }
    (x / DEFAULT_TAB + 1.0).floor() * DEFAULT_TAB
}

#[derive(Clone, Debug)]
struct Token {
    text: String,
    marks: TextMark,
    width: f32,
    pm_pos: usize,
    is_space: bool,
    is_tab: bool,
    kind: SpanKind,
    pm_len: Option<usize>,
}

impl Token {
    fn atom(&self) -> bool {
        !matches!(self.kind, SpanKind::Text)
    }
}

/// `lineH` (`:1845-1849`): the line height and the natural height.
pub fn line_h(para: &RenderParagraph, natural: f32) -> f32 {
    match (para.line_spacing_mode, para.line_spacing_pt) {
        (LineSpacingMode::Exactly, Some(v)) if v != 0.0 => v,
        (LineSpacingMode::AtLeast, Some(v)) if v != 0.0 => natural.max(v),
        _ => natural * para.line_spacing,
    }
}

/// The web's emergency character break (`:2129-2146`).
fn break_token(tok: &Token, avail: f32, m: &dyn Measure) -> Option<(Token, Token)> {
    if tok.atom() || tok.is_tab || tok.is_space {
        return None;
    }
    let units: Vec<u16> = tok.text.encode_utf16().collect();
    if units.len() < 2 {
        return None;
    }
    let prefix_w = |mid: usize| -> f32 {
        // `slice(0, mid)` may end in a lone high surrogate (~0 wide in the browser): the honest
        // match is the largest decodable prefix.
        let end = if String::from_utf16(&units[..mid]).is_ok() { mid } else { mid.saturating_sub(1) };
        String::from_utf16(&units[..end]).ok().map(|s| m.width(&s, &tok.marks)).unwrap_or(0.0)
    };
    let (mut lo, mut hi, mut best) = (1usize, units.len() - 1, 1usize);
    while lo <= hi {
        let mid = (lo + hi) / 2;
        if prefix_w(mid) <= avail + FIT_SLACK {
            best = mid;
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    while best > 1 && units.get(best).is_some_and(|&u| splits_a_cluster(u)) {
        best -= 1;
    }
    if String::from_utf16(&units[..best]).is_err() {
        best += 1;
    }
    let head = String::from_utf16(&units[..best]).ok()?;
    let rest = String::from_utf16(&units[best..]).ok()?;
    if rest.is_empty() {
        return None;
    }
    Some((
        Token { width: m.width(&head, &tok.marks), text: head, ..tok.clone() },
        Token { width: m.width(&rest, &tok.marks), text: rest, pm_pos: tok.pm_pos + best, ..tok.clone() },
    ))
}

/// Lays one paragraph out. Lines come back with `y` relative to the paragraph's first line (0) and
/// `baseline` on the same origin.
pub fn layout_paragraph(para: &RenderParagraph, content_w: f32, m: &dyn Measure) -> Vec<LayoutLine> {
    let indent_right = para.indent_right;
    let first_line = para.first_line_indent;
    let avail = content_w - para.indent - indent_right;
    let mut lines: Vec<LayoutLine> = Vec::new();

    // ── Block image: one "line" the image's height (`:1862-1905`). The natural size is not
    // known to the core: an image with no explicit size is laid out at 320×200 like a web image
    // that has not loaded yet (the platform reports the real size through the attributes it writes
    // on insert).
    if let Some(img) = &para.image {
        let (nat_w, nat_h) = (320.0f32, 200.0f32);
        let mut disp_w = if img.width > 0.0 { img.width.min(content_w) } else { nat_w.min(content_w) };
        let mut disp_h = if img.height > 0.0 { img.height } else { nat_h * (disp_w / nat_w) };
        if !disp_w.is_finite() || disp_w <= 0.0 {
            disp_w = nat_w.min(content_w);
        }
        if !disp_h.is_finite() || disp_h <= 0.0 {
            disp_h = nat_h * (disp_w / nat_w);
        }
        let aabb_h = img_aabb(disp_w, disp_h, img.rotation).h;
        let floating = img.floating();
        let align_x = match img.align {
            Align::Center => (content_w - disp_w) / 2.0,
            Align::Right => content_w - disp_w,
            _ => 0.0,
        };
        let x = if floating && img.wrap_x != 0.0 { img.wrap_x } else { align_x };
        let h = if floating { 0.0 } else { aabb_h };
        lines.push(LayoutLine {
            height: h,
            ascent: h,
            natural_h: h,
            pm_start: para.pm_start,
            pm_end: para.pm_end,
            image: Some(LineImage {
                src: img.src.clone(),
                w: disp_w,
                h: disp_h,
                x,
                rotation: img.rotation,
                wrap: img.wrap.clone(),
                wrap_y: img.wrap_y,
                alt: img.alt.clone(),
            }),
            no_caret: true,
            ..Default::default()
        });
        return lines;
    }

    // ── Tokens (`:1910-1945`).
    let mut tokens: Vec<Token> = Vec::new();
    for span in &para.spans {
        match &span.kind {
            SpanKind::Image(img) => {
                tokens.push(Token {
                    text: span.text.clone(),
                    marks: span.marks.clone(),
                    width: img_aabb(img.w, img.h, img.rot).w,
                    pm_pos: span.pm_pos,
                    is_space: false,
                    is_tab: false,
                    kind: span.kind.clone(),
                    pm_len: span.pm_len,
                });
                continue;
            }
            SpanKind::Footnote { .. } | SpanKind::Endnote { .. } | SpanKind::Field { .. } => {
                tokens.push(Token {
                    text: span.text.clone(),
                    marks: span.marks.clone(),
                    width: m.width(&span.text, &span.marks),
                    pm_pos: span.pm_pos,
                    is_space: false,
                    is_tab: false,
                    kind: span.kind.clone(),
                    pm_len: span.pm_len,
                });
                continue;
            }
            _ => {}
        }
        let mut p = span.pm_pos;
        for chunk in split_each(&span.text, '\t') {
            if chunk.is_empty() {
                continue;
            }
            if chunk == "\t" {
                tokens.push(Token { text: "\t".into(), marks: span.marks.clone(), width: 0.0, pm_pos: p, is_space: true, is_tab: true, kind: SpanKind::Text, pm_len: None });
                p += 1;
                continue;
            }
            for part in split_keeping_spaces(chunk) {
                if part.is_empty() {
                    continue;
                }
                let space = part.chars().all(is_js_space);
                tokens.push(Token {
                    text: part.to_string(),
                    marks: span.marks.clone(),
                    width: m.width(part, &span.marks),
                    pm_pos: p,
                    is_space: space,
                    is_tab: false,
                    kind: SpanKind::Text,
                    // A rule's 60 dashes cover no position at all.
                    pm_len: span.pm_len.map(|_| 0),
                });
                p += if span.pm_len == Some(0) { 0 } else { crate::pm::utf16_len(part) };
            }
        }
    }

    // ── Empty paragraph (`:1950-1964`): the caret lives at pmStart + 1.
    if tokens.is_empty() {
        let lm = line_metrics(m, &TextMark { font_size: para.empty_pt, ..Default::default() });
        let inner = para.pm_start + 1;
        let caret_x = match para.align {
            Align::Center => para.indent + avail / 2.0,
            Align::Right => para.indent + avail,
            _ => para.indent + first_line,
        };
        let h = line_h(para, lm.height);
        lines.push(LayoutLine {
            height: h,
            natural_h: lm.height,
            ascent: lm.ascent,
            baseline: (h - lm.height) / 2.0 + lm.ascent,
            pm_start: inner,
            pm_end: inner,
            caret_x: Some(caret_x),
            ..Default::default()
        });
        return lines;
    }

    let mut line_toks: Vec<Token> = Vec::new();
    let mut line_w = 0.0f32;
    let mut l_start = para.spans.first().map(|s| s.pm_pos).unwrap_or(para.pm_start);
    let mut row_rel = 0.0f32;
    let mut cur_left = para.indent + first_line;
    let mut cur_avail = (avail - first_line).max(MIN_SEG_W);

    let flush = |line_toks: &mut Vec<Token>, lines: &mut Vec<LayoutLine>, is_last: bool, cur_left: f32, cur_avail: f32, l_start: &mut usize, row_rel: &mut f32| {
        if line_toks.is_empty() {
            return;
        }
        let mut trail = line_toks.len();
        while trail > 0 && line_toks[trail - 1].is_space {
            trail -= 1;
        }
        let (mut max_asc, mut max_dsc, mut max_h) = (0.0f32, 0.0f32, 0.0f32);
        for t in line_toks.iter() {
            if let SpanKind::Image(img) = &t.kind {
                let ah = img_aabb(img.w, img.h, img.rot).h;
                max_asc = max_asc.max(ah);
                max_h = max_h.max(ah);
                continue;
            }
            let lm = line_metrics(m, &t.marks);
            max_asc = max_asc.max(lm.ascent);
            max_dsc = max_dsc.max(lm.descent);
            max_h = max_h.max(lm.height);
        }
        let _ = max_dsc;
        let tw: f32 = line_toks[..trail].iter().map(|t| t.width).sum();
        let mut extra = 0.0;
        if para.align == Align::Justify && !is_last {
            let n = line_toks[..trail].iter().filter(|t| t.is_space).count();
            if n > 0 {
                extra = (cur_avail - tw) / n as f32;
            }
        }
        let sx = match para.align {
            Align::Center => cur_left + (cur_avail - tw) / 2.0,
            Align::Right => cur_left + cur_avail - tw,
            _ => cur_left,
        };
        let mut spans = Vec::with_capacity(line_toks.len() + 1);
        if let (Some(marker), true) = (&para.marker, lines.is_empty()) {
            let mm = para.marker_marks.clone().unwrap_or_default();
            let mw = m.width(&format!("{marker} "), &mm);
            spans.push(LayoutSpan { text: marker.clone(), marks: mm, x: cur_left - mw, width: mw, pm_pos: *l_start, kind: SpanKind::Marker, pm_len: Some(0) });
        }
        let mut x = sx;
        let mut l_end = *l_start;
        for (i, t) in line_toks.iter().enumerate() {
            let w = if t.is_tab {
                (next_tab_stop(para.tab_stops.as_deref(), x) - x).max(MIN_TAB_ADVANCE)
            } else if t.is_space {
                t.width + if i < trail { extra } else { 0.0 }
            } else {
                t.width
            };
            spans.push(LayoutSpan { text: t.text.clone(), marks: t.marks.clone(), x, width: w, pm_pos: t.pm_pos, kind: t.kind.clone(), pm_len: t.pm_len });
            x += w;
            l_end = t.pm_pos + t.pm_len.unwrap_or_else(|| crate::pm::utf16_len(&t.text));
        }
        let h = line_h(para, max_h);
        lines.push(LayoutLine {
            spans,
            y: *row_rel,
            height: h,
            natural_h: max_h,
            ascent: max_asc,
            baseline: *row_rel + (h - max_h) / 2.0 + max_asc,
            pm_start: *l_start,
            pm_end: l_end,
            ..Default::default()
        });
        *row_rel += h;
        *l_start = l_end;
        line_toks.clear();
    };

    let mut ti = 0usize;
    while ti < tokens.len() {
        let tok = tokens[ti].clone();
        // (A) leading whitespace dropped on a WRAPPED line, kept on the first (`:2156`).
        if line_toks.is_empty() && tok.is_space && !lines.is_empty() {
            l_start = tok.pm_pos + crate::pm::utf16_len(&tok.text);
            ti += 1;
            continue;
        }
        // (B) fits (a tab is 0 wide here: QUIRK, it never wraps).
        if line_w + tok.width <= cur_avail + FIT_SLACK {
            line_w += tok.width;
            line_toks.push(tok);
            ti += 1;
            continue;
        }
        // (C) empty line: cut the word.
        if line_toks.is_empty() {
            if let Some((head, rest)) = break_token(&tok, cur_avail, m) {
                tokens.splice(ti..=ti, [head, rest]);
                continue;
            }
            line_w += tok.width;
            line_toks.push(tok);
            ti += 1;
            continue;
        }
        // (D) wrap.
        flush(&mut line_toks, &mut lines, false, cur_left, cur_avail, &mut l_start, &mut row_rel);
        line_w = 0.0;
        cur_left = para.indent;
        cur_avail = avail.max(MIN_SEG_W);
        if tok.is_space {
            l_start = tok.pm_pos + crate::pm::utf16_len(&tok.text);
            ti += 1;
        }
        // else: the same token is re-evaluated against the new line.
    }
    flush(&mut line_toks, &mut lines, true, cur_left, cur_avail, &mut l_start, &mut row_rel);

    // A paragraph of spaces only produced nothing (`:2194-2203`).
    if lines.is_empty() {
        let lm = line_metrics(m, &TextMark::default());
        let inner = para.pm_start + 1;
        let h = line_h(para, lm.height);
        lines.push(LayoutLine {
            y: row_rel,
            height: h,
            natural_h: lm.height,
            ascent: lm.ascent,
            baseline: row_rel + (h - lm.height) / 2.0 + lm.ascent,
            pm_start: inner,
            pm_end: inner,
            ..Default::default()
        });
    }
    if para.rule {
        for l in &mut lines {
            l.no_caret = true;
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{RenderSpan, LH_RATIO};
    use crate::measure::FixedMeasure;

    fn para(text: &str) -> RenderParagraph {
        RenderParagraph {
            spans: vec![RenderSpan { text: text.into(), marks: TextMark::default(), pm_pos: 1, kind: SpanKind::Text, pm_len: None }],
            pm_start: 0,
            pm_end: 1 + crate::pm::utf16_len(text),
            ..Default::default()
        }
    }

    fn texts(lines: &[LayoutLine]) -> Vec<String> {
        lines.iter().map(|l| l.spans.iter().map(|s| s.text.as_str()).collect()).collect()
    }

    // FixedMeasure: 11 pt = 14.667 px, a character is 0.6 em = 8.8 px.
    const CH: f32 = 11.0 * 96.0 / 72.0 * 0.6;

    #[test]
    fn words_wrap_and_the_wrapping_space_is_dropped() {
        let p = para("aaaa bbbb cccc");
        let lines = layout_paragraph(&p, CH * 9.5, &FixedMeasure);
        assert_eq!(texts(&lines), vec!["aaaa bbbb".to_string(), "cccc".into()]);
        // positions run on: line 2 starts after the dropped space
        assert_eq!(lines[0].pm_start, 1);
        assert_eq!(lines[0].pm_end, 10);
        assert_eq!(lines[1].pm_start, 11);
        assert_eq!(lines[1].pm_end, 15);
    }

    #[test]
    fn an_empty_paragraph_has_one_line_with_the_caret_inside() {
        let p = RenderParagraph { pm_start: 4, pm_end: 5, ..Default::default() };
        let lines = layout_paragraph(&p, 600.0, &FixedMeasure);
        assert_eq!(lines.len(), 1);
        assert_eq!((lines[0].pm_start, lines[0].pm_end), (5, 5));
        let fs = 11.0 * 96.0 / 72.0;
        assert!((lines[0].height - fs * 1.2 * LH_RATIO).abs() < 1e-3);
    }

    #[test]
    fn a_word_wider_than_the_column_is_cut() {
        let p = para("abcdefghij");
        let lines = layout_paragraph(&p, CH * 4.2, &FixedMeasure);
        assert_eq!(texts(&lines), vec!["abcd".to_string(), "efgh".into(), "ij".into()]);
        assert_eq!(lines[1].spans[0].pm_pos, 5);
    }

    #[test]
    fn tabs_advance_to_the_grid() {
        let p = para("a\tb");
        let lines = layout_paragraph(&p, 600.0, &FixedMeasure);
        let tab = &lines[0].spans[1];
        assert_eq!(tab.text, "\t");
        assert!((tab.x + tab.width - 48.0).abs() < 1e-3);
        let custom = RenderParagraph { tab_stops: Some(vec![100.0]), ..para("a\tb") };
        let lines = layout_paragraph(&custom, 600.0, &FixedMeasure);
        assert!((lines[0].spans[2].x - 100.0).abs() < 1e-3);
    }

    #[test]
    fn centre_and_right_alignment_ignore_trailing_spaces() {
        let p = RenderParagraph { align: Align::Right, ..para("ab  ") };
        let lines = layout_paragraph(&p, 100.0, &FixedMeasure);
        assert!((lines[0].spans[0].x - (100.0 - 2.0 * CH)).abs() < 1e-3);
    }

    #[test]
    fn a_marker_hangs_left_of_the_text() {
        let p = RenderParagraph { marker: Some("•".into()), indent: 32.0, ..para("x") };
        let lines = layout_paragraph(&p, 600.0, &FixedMeasure);
        assert!(lines[0].spans[0].is_marker());
        assert!((lines[0].spans[0].x + lines[0].spans[0].width - 32.0).abs() < 1e-3);
        assert_eq!(lines[0].spans[1].x, 32.0);
    }

    #[test]
    fn justified_lines_stretch_their_spaces_except_the_last() {
        let p = RenderParagraph { align: Align::Justify, ..para("aa bb cc dd") };
        let lines = layout_paragraph(&p, CH * 9.0, &FixedMeasure);
        assert_eq!(lines.len(), 2);
        let first = &lines[0];
        let end = first.spans.iter().filter(|s| !s.text.trim().is_empty()).map(|s| s.x + s.width).fold(0.0f32, f32::max);
        assert!((end - CH * 9.0).abs() < 1e-3);
    }
}
