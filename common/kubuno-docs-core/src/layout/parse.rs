//! `parseDoc` (`canvas-engine.ts:716-1050`): the ProseMirror body → render paragraphs.
//!
//! Ported branch for branch, quirks included (each marked **QUIRK**):
//!
//! * a `hardBreak` advances the position and emits **nothing** (no span, no line break);
//! * `blockquote`, `taskList` and any unknown container are not drawn — their positions are counted
//!   and skipped;
//! * list markers are `•` and `n.` at every depth, every block child of an item wears the marker,
//!   the counter is per list node (`attrs.start` ignored);
//! * a heading run without a size gets the heading size, and is bold when the level is ≤ 4.
//!
//! One divergence, deliberate: positions are counted with ProseMirror's own sizes
//! ([`crate::pm::node_size`]), so a `horizontalRule` is 1 position (the web counts 2, which shifts
//! every later position by one on the web) and an unknown inline node is its real size.
//!
//! The parse is driven block by block ([`parse_block`]) with the cross-block state in [`Ctx`]
//! (position, section, pending page break, note counters), which is what lets the flow cache one
//! top-level block's result and reuse it when only another block changed.

use serde_json::Value;

use super::{Align, BlockImage, InlineImage, LineSpacingMode, RenderParagraph, RenderSpan, RenderTable, RenderTableCell, SpanKind, LH_RATIO, LIST_INDENT};
use crate::marks::{self, parse_float, truthy, Script, TextMark};
use crate::model::Node;
use crate::pm::{is_container, node_size, utf16_len};

/// `H_SIZE` (`:261`), `H_BEFORE` (`:262`), `H_AFTER` (`:263`), by level (index 0 unused).
pub const H_SIZE: [f32; 7] = [11.0, 24.0, 18.0, 14.0, 13.0, 12.0, 11.0];
pub const H_BEFORE: [f32; 7] = [10.0, 20.0, 16.0, 12.0, 10.0, 8.0, 8.0];
pub const H_AFTER: [f32; 7] = [4.0, 6.0, 4.0, 4.0, 4.0, 4.0, 4.0];

/// The ink of a footnote/endnote reference (`:880`).
pub const NOTE_REF_COLOR: &str = "#1a73e8";

/// The cross-block state of the parse.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ctx {
    /// The current position (before the next block).
    pub pos: usize,
    pub sec_idx: usize,
    pub pending_break: bool,
    pub fn_counter: usize,
    pub en_counter: usize,
}

/// A heading's level (1..=6; 0 for anything else). A heading without `level` is level 1.
pub fn heading_level(node: &Node) -> usize {
    if node.node_type() != Some("heading") {
        return 0;
    }
    node.attr("level").and_then(|v| parse_float(&v)).map(|l| (l as i64).clamp(1, 6) as usize).unwrap_or(1)
}

fn num(attrs: &serde_json::Map<String, Value>, key: &str) -> Option<f32> {
    attrs.get(key).filter(|v| !v.is_null()).and_then(parse_float).filter(|v| v.is_finite())
}

/// `Number(x) || 0`.
fn num_or0(attrs: &serde_json::Map<String, Value>, key: &str) -> f32 {
    num(attrs, key).unwrap_or(0.0)
}

fn string(attrs: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    match attrs.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

/// `romanLower` (`:702-712`).
pub fn roman_lower(n: usize) -> String {
    if n < 1 {
        return n.to_string();
    }
    const UNITS: [(usize, &str); 13] = [
        (1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"),
        (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i"),
    ];
    let mut rest = n;
    let mut out = String::new();
    for (v, s) in UNITS {
        while rest >= v {
            out.push_str(s);
            rest -= v;
        }
    }
    out
}

/// The text a field paints: its cached result, else the label of its kind (`fieldText`).
pub fn field_text(node: &Node) -> String {
    let attrs = node.attrs();
    if let Some(Value::String(c)) = attrs.get("cached") {
        if !c.is_empty() {
            return c.clone();
        }
    }
    match attrs.get("kind").and_then(Value::as_str) {
        Some("page") => "N° de page",
        Some("pages") => "Nb pages",
        Some("date") => "Date",
        Some("title") => "Titre",
        Some("ref") => "Renvoi",
        _ => "Champ",
    }
    .to_string()
}

/// The list context a block is laid out in.
#[derive(Clone, Copy)]
struct ListCtx {
    bullet: bool,
    idx: usize,
}

/// Parses the whole body.
pub fn parse_doc(doc: &Node) -> Vec<RenderParagraph> {
    let mut ctx = Ctx::default();
    let mut out = Vec::new();
    for (i, block) in doc.children().iter().enumerate() {
        parse_block(block, i, &mut ctx, &mut out);
    }
    out
}

/// Parses one top-level block (`block(node, 0, dIdx)`), advancing `ctx`.
pub fn parse_block(node: &Node, doc_idx: usize, ctx: &mut Ctx, out: &mut Vec<RenderParagraph>) {
    block(node, 0, doc_idx, None, ctx, out);
}

fn inline_spans(node: &Node, level: usize, ctx: &mut Ctx, pos: &mut usize, spans: &mut Vec<RenderSpan>) {
    for inline in node.children() {
        match inline.node_type() {
            Some("text") => {
                let text = inline.text().unwrap_or_default();
                let mut m = marks::text_mark_of(inline);
                if level > 0 {
                    if m.font_size.is_none() {
                        m.font_size = Some(H_SIZE[level]);
                    }
                    if !m.bold && level <= 4 {
                        m.bold = true;
                    }
                }
                let len = utf16_len(&text);
                spans.push(RenderSpan { text, marks: m, pm_pos: *pos, kind: SpanKind::Text, pm_len: None });
                *pos += len;
            }
            // QUIRK (`:848-849`): a hard break emits nothing.
            Some("hardBreak") => *pos += 1,
            Some("inlineImage") => {
                let a = inline.attrs();
                let m = marks::text_mark_of(inline);
                let img = InlineImage {
                    src: string(&a, "src").unwrap_or_default(),
                    w: num_or0(&a, "width").max(1.0),
                    h: num_or0(&a, "height").max(1.0),
                    alt: string(&a, "alt"),
                    rot: num_or0(&a, "rotation"),
                };
                spans.push(RenderSpan { text: "\u{200B}".into(), marks: m, pm_pos: *pos, kind: SpanKind::Image(img), pm_len: Some(1) });
                *pos += 1;
            }
            Some("footnote") => {
                let rev = marks::text_mark_of(inline);
                ctx.fn_counter += 1;
                let n = ctx.fn_counter;
                let m = TextMark { script: Some(Script::Super), color: Some(NOTE_REF_COLOR.into()), insertion: rev.insertion, deletion: rev.deletion, ..Default::default() };
                let text = inline.attr("text").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                spans.push(RenderSpan { text: n.to_string(), marks: m, pm_pos: *pos, kind: SpanKind::Footnote { n, text }, pm_len: Some(1) });
                *pos += 1;
            }
            Some("endnote") => {
                let rev = marks::text_mark_of(inline);
                ctx.en_counter += 1;
                let n = ctx.en_counter;
                let m = TextMark { script: Some(Script::Super), color: Some(NOTE_REF_COLOR.into()), insertion: rev.insertion, deletion: rev.deletion, ..Default::default() };
                let text = inline.attr("text").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                spans.push(RenderSpan { text: roman_lower(n), marks: m, pm_pos: *pos, kind: SpanKind::Endnote { n, text }, pm_len: Some(1) });
                *pos += 1;
            }
            Some("field") => {
                let a = inline.attrs();
                let mut m = marks::text_mark_of(inline);
                if level > 0 {
                    if m.font_size.is_none() {
                        m.font_size = Some(H_SIZE[level]);
                    }
                    if !m.bold && level <= 4 {
                        m.bold = true;
                    }
                }
                let kind = SpanKind::Field { kind: string(&a, "kind").unwrap_or_else(|| "other".into()), instr: string(&a, "instr").unwrap_or_default() };
                spans.push(RenderSpan { text: field_text(inline), marks: m, pm_pos: *pos, kind, pm_len: Some(1) });
                *pos += 1;
            }
            _ => *pos += node_size(inline),
        }
    }
}

fn block(node: &Node, depth: usize, doc_idx: usize, list: Option<ListCtx>, ctx: &mut Ctx, out: &mut Vec<RenderParagraph>) {
    let ty = node.node_type().unwrap_or("");
    match ty {
        "sectionBreak" => {
            ctx.sec_idx += 1;
            ctx.pending_break = true;
            ctx.pos += 1;
            return;
        }
        "pageBreak" => {
            ctx.pending_break = true;
            ctx.pos += 1;
            return;
        }
        "image" => {
            let a = node.attrs();
            let wrap = string(&a, "wrap").filter(|w| !w.is_empty()).unwrap_or_else(|| "inline".into());
            let floating = super::images::is_floating_wrap(&wrap);
            out.push(RenderParagraph {
                space_before: if floating { 0.0 } else { 6.0 },
                space_after: if floating { 0.0 } else { 6.0 },
                pm_start: ctx.pos,
                pm_end: ctx.pos + 1,
                doc_idx,
                sec_idx: ctx.sec_idx,
                break_before: ctx.pending_break,
                image: Some(BlockImage {
                    src: string(&a, "src").unwrap_or_default(),
                    width: num_or0(&a, "width"),
                    height: num_or0(&a, "height"),
                    align: Align::parse(a.get("align").and_then(Value::as_str)),
                    rotation: num_or0(&a, "rotation"),
                    wrap,
                    wrap_x: num_or0(&a, "wrapX"),
                    wrap_y: num_or0(&a, "wrapY"),
                    alt: string(&a, "alt"),
                }),
                ..Default::default()
            });
            ctx.pending_break = false;
            ctx.pos += 1;
            return;
        }
        "table" => {
            let t_start = ctx.pos;
            let brk = ctx.pending_break;
            ctx.pending_break = false;
            ctx.pos += 1; // table open
            let mut rows = Vec::new();
            for row in node.children() {
                ctx.pos += 1; // row open
                let mut cells = Vec::new();
                for cell in row.children() {
                    ctx.pos += 1; // cell open
                    let mut paras = Vec::new();
                    for (ci, child) in cell.children().iter().enumerate() {
                        block(child, 0, ci, None, ctx, &mut paras);
                    }
                    ctx.pos += 1; // cell close
                    cells.push(RenderTableCell { paras });
                }
                ctx.pos += 1; // row close
                rows.push(cells);
            }
            ctx.pos += 1; // table close
            out.push(RenderParagraph {
                space_before: 6.0,
                space_after: 6.0,
                pm_start: t_start,
                pm_end: ctx.pos,
                doc_idx,
                sec_idx: ctx.sec_idx,
                break_before: brk,
                table: Some(RenderTable { rows, node: node.clone() }),
                ..Default::default()
            });
            return;
        }
        _ => {}
    }

    if !is_container(node) {
        // A leaf block we do not draw (a horizontal rule is handled below: it is a container in the
        // web's accounting but a leaf in ProseMirror's).
        if ty == "horizontalRule" {
            let b_start = ctx.pos;
            let break_before = std::mem::take(&mut ctx.pending_break);
            out.push(RenderParagraph {
                spans: vec![RenderSpan {
                    text: "─".repeat(60),
                    marks: TextMark { color: Some("#dadce0".into()), font_size: Some(8.0), ..Default::default() },
                    pm_pos: b_start,
                    kind: SpanKind::Text,
                    pm_len: Some(0),
                }],
                space_before: 8.0,
                space_after: 8.0,
                pm_start: b_start,
                pm_end: b_start + 1,
                doc_idx,
                sec_idx: ctx.sec_idx,
                break_before,
                rule: true,
                ..Default::default()
            });
            ctx.pos += 1;
            return;
        }
        ctx.pos += node_size(node);
        return;
    }

    let b_start = ctx.pos;
    let attrs = node.attrs();
    let break_before = ctx.pending_break || attrs.get("pageBreakBefore").is_some_and(truthy);
    ctx.pending_break = false;
    // `(attrs.lineHeight as number) || LH_RATIO` — a string multiplies too in JavaScript.
    let line_spacing = num(&attrs, "lineHeight").filter(|v| *v != 0.0).unwrap_or(LH_RATIO);
    ctx.pos += 1; // opening token

    match ty {
        "paragraph" | "heading" => {
            let level = heading_level(node);
            let align = Align::parse(attrs.get("textAlign").and_then(Value::as_str));
            let mut spans = Vec::new();
            let mut pos = ctx.pos;
            inline_spans(node, level, ctx, &mut pos, &mut spans);
            ctx.pos = pos;

            let indent_level = num_or0(&attrs, "indent");
            let ind_l = num_or0(&attrs, "indentLeft");
            let ind_f = num_or0(&attrs, "indentFirstLine");
            let ind_r = num_or0(&attrs, "indentRight");
            let tab_stops = match attrs.get("tabStops") {
                Some(Value::Array(a)) => {
                    let mut v: Vec<f32> = a
                        .iter()
                        .filter_map(|t| match t {
                            Value::Number(n) => n.as_f64().map(|f| f as f32),
                            Value::Object(o) => o.get("pos").and_then(Value::as_f64).map(|f| f as f32),
                            _ => None,
                        })
                        .collect();
                    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    (!v.is_empty()).then_some(v)
                }
                _ => None,
            };
            let empty_pt = attrs
                .get("fontMarks")
                .and_then(|fm| fm.get("fs"))
                .and_then(parse_float)
                .filter(|v| v.is_finite() && *v > 0.0);
            let mode = match attrs.get("lineSpacingMode").and_then(Value::as_str) {
                Some("atLeast") => LineSpacingMode::AtLeast,
                Some("exactly") => LineSpacingMode::Exactly,
                _ => LineSpacingMode::Multiple,
            };
            let space_before = match attrs.get("spaceBefore") {
                Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) as f32,
                _ if level > 0 => H_BEFORE[level],
                _ if list.is_some() => 2.0,
                _ => 0.0,
            };
            let space_after = match attrs.get("spaceAfter") {
                Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) as f32,
                _ if level > 0 => H_AFTER[level],
                _ => 2.0,
            };
            out.push(RenderParagraph {
                spans,
                align,
                indent: depth as f32 * LIST_INDENT + indent_level * LIST_INDENT + ind_l,
                first_line_indent: ind_f,
                indent_right: ind_r,
                tab_stops,
                marker: list.map(|l| if l.bullet { "•".to_string() } else { format!("{}.", l.idx) }),
                marker_marks: None,
                space_before,
                space_after,
                pm_start: b_start,
                pm_end: ctx.pos,
                doc_idx,
                sec_idx: ctx.sec_idx,
                break_before,
                line_spacing,
                line_spacing_mode: mode,
                line_spacing_pt: match attrs.get("lineSpacingPt") {
                    Some(Value::Number(n)) => n.as_f64().map(|f| f as f32),
                    _ => None,
                },
                contextual_spacing: attrs.get("contextualSpacing").is_some_and(truthy),
                style_key: if level > 0 { format!("h{level}") } else if list.is_some() { format!("li{depth}") } else { "p".into() },
                keep_lines: attrs.get("keepLines").is_some_and(truthy),
                keep_next: attrs.get("keepNext").is_some_and(truthy),
                empty_pt,
                ..Default::default()
            });
        }
        "bulletList" | "orderedList" => {
            let bullet = ty == "bulletList";
            let mut idx = 1;
            for item in node.children() {
                if item.node_type() == Some("listItem") {
                    ctx.pos += 1; // item open
                    for child in item.children() {
                        block(child, depth + 1, doc_idx, Some(ListCtx { bullet, idx }), ctx, out);
                    }
                    ctx.pos += 1; // item close
                    idx += 1;
                } else {
                    // QUIRK (`:989`): a non-item child is skipped whole (its positions still count —
                    // the web forgets them, which would desynchronise every later position).
                    ctx.pos += node_size(item);
                }
            }
        }
        "codeBlock" => {
            let code = TextMark { font_family: Some("Courier New".into()), font_size: Some(10.0), background: Some("#f8f9fa".into()), ..Default::default() };
            let mut spans = Vec::new();
            for inline in node.children() {
                if inline.node_type() == Some("text") {
                    let text = inline.text().unwrap_or_default();
                    let len = utf16_len(&text);
                    spans.push(RenderSpan { text, marks: code.clone(), pm_pos: ctx.pos, kind: SpanKind::Text, pm_len: None });
                    ctx.pos += len;
                } else {
                    ctx.pos += node_size(inline);
                }
            }
            out.push(RenderParagraph {
                spans,
                indent: 8.0,
                space_before: 4.0,
                space_after: 4.0,
                pm_start: b_start,
                pm_end: ctx.pos,
                doc_idx,
                sec_idx: ctx.sec_idx,
                break_before,
                line_spacing,
                ..Default::default()
            });
        }
        _ => {
            // QUIRK (`:1012-1014`): blockquote, taskList and unknown containers are not drawn.
            for child in node.children() {
                ctx.pos += node_size(child);
            }
        }
    }
    ctx.pos += 1; // closing token
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(json: &str) -> Node {
        Node::from_slice(json.as_bytes()).expect("fixture parses")
    }

    #[test]
    fn paragraphs_carry_prosemirror_positions() {
        let d = doc(r#"{"content":[{"content":[{"text":"hello","type":"text"}],"type":"paragraph"},{"content":[{"text":"ab","type":"text"},{"type":"hardBreak"},{"text":"c","type":"text"}],"type":"paragraph"}],"type":"doc"}"#);
        let ps = parse_doc(&d);
        assert_eq!(ps.len(), 2);
        assert_eq!((ps[0].pm_start, ps[0].pm_end), (0, 6));
        assert_eq!(ps[0].spans[0].pm_pos, 1);
        assert_eq!((ps[1].pm_start, ps[1].pm_end), (7, 12));
        // QUIRK: the hard break emits nothing but takes its position.
        assert_eq!(ps[1].spans.len(), 2);
        assert_eq!(ps[1].spans[1].pm_pos, 11);
        assert_eq!(ps[0].space_after, 2.0);
    }

    #[test]
    fn headings_get_their_size_and_weight() {
        let d = doc(r#"{"content":[{"attrs":{"level":2},"content":[{"text":"T","type":"text"}],"type":"heading"},{"attrs":{"level":5},"content":[{"text":"U","type":"text"}],"type":"heading"}],"type":"doc"}"#);
        let ps = parse_doc(&d);
        assert_eq!(ps[0].spans[0].marks.font_size, Some(18.0));
        assert!(ps[0].spans[0].marks.bold);
        assert!(!ps[1].spans[0].marks.bold, "levels 5 and 6 are not bold");
        assert_eq!((ps[0].space_before, ps[0].space_after), (16.0, 4.0));
        assert_eq!(ps[0].style_key, "h2");
    }

    #[test]
    fn list_items_are_indented_and_marked() {
        let d = doc(r#"{"content":[{"content":[{"content":[{"content":[{"text":"a","type":"text"}],"type":"paragraph"}],"type":"listItem"},{"content":[{"content":[{"text":"b","type":"text"}],"type":"paragraph"}],"type":"listItem"}],"type":"orderedList"}],"type":"doc"}"#);
        let ps = parse_doc(&d);
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0].marker.as_deref(), Some("1."));
        assert_eq!(ps[1].marker.as_deref(), Some("2."));
        assert_eq!(ps[0].indent, LIST_INDENT);
        // list(0) item(1) p(2) "a" at 3
        assert_eq!(ps[0].spans[0].pm_pos, 3);
        // p closes at 4, item at 5, item2 opens 6, p 7, "b" at 8
        assert_eq!(ps[1].spans[0].pm_pos, 8);
        assert_eq!(ps[0].space_before, 2.0);
    }

    #[test]
    fn breaks_set_the_next_blocks_flags() {
        let d = doc(r#"{"content":[{"type":"paragraph"},{"type":"pageBreak"},{"type":"paragraph"},{"type":"sectionBreak"},{"type":"paragraph"}],"type":"doc"}"#);
        let ps = parse_doc(&d);
        assert_eq!(ps.len(), 3);
        assert!(!ps[0].break_before);
        assert!(ps[1].break_before && ps[1].sec_idx == 0);
        assert!(ps[2].break_before && ps[2].sec_idx == 1);
        assert_eq!(ps[1].pm_start, 3);
    }

    #[test]
    fn tables_collect_their_cells_paragraphs() {
        let d = doc(r#"{"content":[{"content":[{"content":[{"content":[{"content":[{"text":"x","type":"text"}],"type":"paragraph"}],"type":"tableCell"},{"content":[{"type":"paragraph"}],"type":"tableCell"}],"type":"tableRow"}],"type":"table"},{"type":"paragraph"}],"type":"doc"}"#);
        let ps = parse_doc(&d);
        assert_eq!(ps.len(), 2);
        let t = ps[0].table.as_ref().expect("a table");
        assert_eq!(t.rows.len(), 1);
        assert_eq!(t.rows[0].len(), 2);
        // table 0, row 1, cell 2, p 3, "x" 4
        assert_eq!(t.rows[0][0].paras[0].spans[0].pm_pos, 4);
        assert_eq!(ps[0].pm_end, crate::pm::node_size(&d.children()[0]));
        assert_eq!(ps[1].pm_start, ps[0].pm_end);
    }

    #[test]
    fn a_horizontal_rule_takes_one_position() {
        let d = doc(r#"{"content":[{"type":"horizontalRule"},{"content":[{"text":"x","type":"text"}],"type":"paragraph"}],"type":"doc"}"#);
        let ps = parse_doc(&d);
        assert!(ps[0].rule);
        assert_eq!(ps[1].spans[0].pm_pos, 2);
    }

    #[test]
    fn notes_are_numbered_across_the_document() {
        let d = doc(r#"{"content":[{"content":[{"text":"a","type":"text"},{"attrs":{"text":"n1"},"type":"footnote"}],"type":"paragraph"},{"content":[{"attrs":{"text":"n2"},"type":"footnote"},{"type":"endnote"}],"type":"paragraph"}],"type":"doc"}"#);
        let ps = parse_doc(&d);
        assert_eq!(ps[0].spans[1].text, "1");
        assert_eq!(ps[1].spans[0].text, "2");
        assert_eq!(ps[1].spans[1].text, "i");
        assert_eq!(ps[1].spans[1].pm_len, Some(1));
    }
}
