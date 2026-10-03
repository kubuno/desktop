//! `layoutParagraphs` (`canvas-engine.ts:1089-1346`): paragraphs stacked into the continuous layout,
//! and [`Flow`], the incremental version the editor uses.
//!
//! Stacking rules (the web's): space before and after **add** (never collapse) except between two
//! paragraphs of the same style when either asks for contextual spacing; a table is laid out by
//! [`super::table`] and takes its height; a floating image takes no room (text does not wrap around
//! it yet — see [`super`]); each line's leading is split half above and half below its text.
//!
//! # Incremental layout
//!
//! A keystroke changes one top-level block. [`Flow`] keeps, per top-level block, the block's laid
//! out paragraphs **before stacking** — lines relative to their paragraph, positions relative to
//! the block's start — keyed by the block's revision (a counter the editor bumps on every edit of
//! the block), the column width, and the parse state entering the block (section, pending page
//! break, note counters). Assembling then only shifts positions and ordinates and applies the
//! spacing rules, which are the parts that depend on neighbours.

use std::collections::HashMap;

use super::parse::{parse_block, Ctx};
use super::{DocPx, DocumentLayout, LayoutLine, LayoutParagraph, LayoutTable, RenderParagraph};
use crate::measure::Measure;
use crate::model::Node;

/// A paragraph laid out but not yet stacked: lines relative to the top of its text (y = 0).
#[derive(Clone, Debug)]
pub struct Laid {
    pub lines: Vec<LayoutLine>,
    /// Height of the text (or of the table, or of the image line).
    pub text_h: DocPx,
    pub table: Option<LayoutTable>,
    pub space_before: DocPx,
    pub space_after: DocPx,
    pub style_key: String,
    pub contextual: bool,
    pub pm_start: usize,
    pub pm_end: usize,
    pub doc_idx: usize,
    pub sec_idx: usize,
    pub break_before: bool,
    pub keep_lines: bool,
    pub keep_next: bool,
}

/// Lays one render paragraph out (no stacking).
pub fn lay(para: &RenderParagraph, content_w: DocPx, m: &dyn Measure) -> Laid {
    let (lines, table, text_h) = if let Some(rt) = &para.table {
        let (lines, table, h) = super::table::layout_table(rt, content_w, m);
        (lines, Some(table), h)
    } else {
        let lines = super::paragraph::layout_paragraph(para, content_w, m);
        let h = lines.iter().map(|l| l.y + l.height).fold(0.0f32, f32::max);
        (lines, None, h)
    };
    Laid {
        lines,
        text_h,
        table,
        space_before: para.space_before,
        space_after: para.space_after,
        style_key: para.style_key.clone(),
        contextual: para.contextual_spacing,
        pm_start: para.pm_start,
        pm_end: para.pm_end,
        doc_idx: para.doc_idx,
        sec_idx: para.sec_idx,
        break_before: para.break_before,
        keep_lines: para.keep_lines,
        keep_next: para.keep_next,
    }
}

/// Stacks laid paragraphs from `y0`, applying the spacing rules; returns the paragraphs and the
/// total height.
pub fn stack(laid: Vec<Laid>, y0: DocPx) -> (Vec<LayoutParagraph>, DocPx) {
    let mut out = Vec::with_capacity(laid.len());
    let mut y = y0;
    let keys: Vec<(String, bool)> = laid.iter().map(|l| (l.style_key.clone(), l.contextual)).collect();
    for (i, mut l) in laid.into_iter().enumerate() {
        let same_prev = i > 0 && keys[i - 1].0 == l.style_key && (l.contextual || keys[i - 1].1);
        let same_next = i + 1 < keys.len() && keys[i + 1].0 == l.style_key && (l.contextual || keys[i + 1].1);
        let before = if same_prev { 0.0 } else { l.space_before };
        let after = if same_next { 0.0 } else { l.space_after };
        y += before;
        let p_y = y;
        for line in &mut l.lines {
            line.y += p_y;
            line.baseline += p_y;
        }
        if let Some(t) = l.table.as_mut() {
            t.dy = p_y;
        }
        y = p_y + l.text_h;
        y += after;
        out.push(LayoutParagraph {
            lines: l.lines,
            y: p_y - before,
            height: l.text_h + before + after,
            pm_start: l.pm_start,
            pm_end: l.pm_end,
            doc_idx: l.doc_idx,
            sec_idx: l.sec_idx,
            break_before: l.break_before,
            keep_lines: l.keep_lines,
            keep_next: l.keep_next,
            table: l.table,
        });
    }
    (out, y)
}

/// `layoutParagraphs` over a list of render paragraphs (a table cell, a text box…).
pub fn layout_paragraphs(paras: &[RenderParagraph], width_for: &dyn Fn(usize) -> DocPx, m: &dyn Measure) -> (Vec<LayoutParagraph>, DocPx) {
    let laid: Vec<Laid> = paras.iter().map(|p| lay(p, width_for(p.sec_idx), m)).collect();
    stack(laid, 0.0)
}

/// Shifts every position of a laid paragraph by `delta` (positive or negative).
fn shift(l: &mut Laid, delta: isize) {
    let s = |p: usize| (p as isize + delta).max(0) as usize;
    l.pm_start = s(l.pm_start);
    l.pm_end = s(l.pm_end);
    for line in &mut l.lines {
        line.pm_start = s(line.pm_start);
        line.pm_end = s(line.pm_end);
        for sp in &mut line.spans {
            sp.pm_pos = s(sp.pm_pos);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
    width: u32,
    sec_idx: usize,
    pending_break: bool,
    fn_counter: usize,
    en_counter: usize,
}

struct Entry {
    key: Key,
    /// The parse state after the block (relative position: the block's size).
    ctx_out: Ctx,
    size: usize,
    /// Positions relative to the block's start.
    laid: Vec<Laid>,
}

/// The incremental continuous layout (see the module doc).
#[derive(Default)]
pub struct Flow {
    cache: HashMap<u64, Entry>,
    /// How many blocks the last layout laid out afresh (diagnostics and tests).
    pub last_misses: usize,
}

impl Flow {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drops every cached block (fonts changed, zoom-independent otherwise).
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Lays the body out. `revs[i]` identifies the content of top-level block `i` (equal revisions
    /// mean byte-identical blocks); `width_for(section)` is the column width of a section.
    pub fn layout(&mut self, doc: &Node, revs: &[u64], width_for: &dyn Fn(usize) -> DocPx, m: &dyn Measure) -> DocumentLayout {
        let mut ctx = Ctx::default();
        let mut next: HashMap<u64, Entry> = HashMap::with_capacity(revs.len());
        let mut laid_all: Vec<Laid> = Vec::new();
        self.last_misses = 0;
        for (i, block) in doc.children().iter().enumerate() {
            let rev = revs.get(i).copied().unwrap_or_else(|| block_hash(block));
            let width = width_for(ctx.sec_idx);
            let key = Key { width: width.to_bits(), sec_idx: ctx.sec_idx, pending_break: ctx.pending_break, fn_counter: ctx.fn_counter, en_counter: ctx.en_counter };
            let entry = match self.cache.remove(&rev).or_else(|| next.remove(&rev)) {
                Some(e) if e.key == key => e,
                _ => {
                    self.last_misses += 1;
                    let start = ctx.pos;
                    let mut local = Ctx { pos: 0, ..ctx.clone() };
                    let mut paras = Vec::new();
                    parse_block(block, i, &mut local, &mut paras);
                    // Widths may differ per section inside one block only through a section break,
                    // which is a block of its own.
                    let laid = paras.iter().map(|p| lay(p, width_for(p.sec_idx), m)).collect();
                    let _ = start;
                    Entry { key, size: local.pos, ctx_out: Ctx { pos: 0, ..local }, laid }
                }
            };
            let delta = ctx.pos as isize;
            for l in &entry.laid {
                let mut l = l.clone();
                shift(&mut l, delta);
                l.doc_idx = i;
                laid_all.push(l);
            }
            ctx = Ctx { pos: ctx.pos + entry.size, ..entry.ctx_out.clone() };
            next.insert(rev, entry);
        }
        self.cache = next;
        let content_w = width_for(0);
        let (paragraphs, total_height) = stack(laid_all, 0.0);
        DocumentLayout { paragraphs, total_height, content_w }
    }
}

/// A content hash of a block, for callers without revision counters.
pub fn block_hash(block: &Node) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    block.to_vec().unwrap_or_default().hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::FixedMeasure;

    fn doc(json: &str) -> Node {
        Node::from_slice(json.as_bytes()).expect("fixture parses")
    }

    fn para_json(text: &str) -> String {
        format!(r#"{{"content":[{{"text":"{text}","type":"text"}}],"type":"paragraph"}}"#)
    }

    fn body(paras: &[&str]) -> Node {
        let inner: Vec<String> = paras.iter().map(|p| para_json(p)).collect();
        doc(&format!(r#"{{"content":[{}],"type":"doc"}}"#, inner.join(",")))
    }

    #[test]
    fn paragraphs_stack_with_additive_spacing() {
        let d = body(&["a", "b"]);
        let mut f = Flow::new();
        let revs = vec![1, 2];
        let l = f.layout(&d, &revs, &|_| 600.0, &FixedMeasure);
        assert_eq!(l.paragraphs.len(), 2);
        let p0 = &l.paragraphs[0];
        let p1 = &l.paragraphs[1];
        // body space after 2, no space before
        assert!((p1.lines[0].y - (p0.lines[0].y + p0.lines[0].height + 2.0)).abs() < 1e-3);
        assert_eq!(p1.lines[0].pm_start, 4);
    }

    #[test]
    fn an_unchanged_block_is_not_laid_out_again() {
        let d = body(&["aaa", "bbb", "ccc"]);
        let mut f = Flow::new();
        let first = f.layout(&d, &[1, 2, 3], &|_| 600.0, &FixedMeasure);
        assert_eq!(f.last_misses, 3);
        // Block 1 edited: one miss, and the following block's positions move.
        let d2 = body(&["aaa", "bbbbb", "ccc"]);
        let second = f.layout(&d2, &[1, 4, 3], &|_| 600.0, &FixedMeasure);
        assert_eq!(f.last_misses, 1);
        assert_eq!(second.paragraphs[2].pm_start, first.paragraphs[2].pm_start + 2);
        assert_eq!(second.paragraphs[2].lines[0].spans[0].pm_pos, first.paragraphs[2].lines[0].spans[0].pm_pos + 2);
    }

    #[test]
    fn contextual_spacing_collapses_between_same_style_paragraphs() {
        let d = doc(r#"{"content":[{"attrs":{"contextualSpacing":true,"spaceAfter":10,"spaceBefore":10},"content":[{"text":"a","type":"text"}],"type":"paragraph"},{"attrs":{"spaceAfter":10,"spaceBefore":10},"content":[{"text":"b","type":"text"}],"type":"paragraph"}],"type":"doc"}"#);
        let mut f = Flow::new();
        let l = f.layout(&d, &[1, 2], &|_| 600.0, &FixedMeasure);
        let gap = l.paragraphs[1].lines[0].y - (l.paragraphs[0].lines[0].y + l.paragraphs[0].lines[0].height);
        assert!(gap.abs() < 1e-3, "gap {gap}");
    }

    #[test]
    fn a_table_is_laid_out_with_its_cells_lines() {
        let d = doc(r#"{"content":[{"content":[{"content":[{"content":[{"content":[{"text":"x","type":"text"}],"type":"paragraph"}],"type":"tableCell"},{"content":[{"content":[{"text":"y","type":"text"}],"type":"paragraph"}],"type":"tableCell"}],"type":"tableRow"}],"type":"table"}],"type":"doc"}"#);
        let mut f = Flow::new();
        let l = f.layout(&d, &[1], &|_| 600.0, &FixedMeasure);
        let t = &l.paragraphs[0];
        assert!(t.table.is_some());
        assert_eq!(t.lines.len(), 2);
        assert!(t.lines[1].spans[0].x > t.lines[0].spans[0].x + 100.0, "the second cell is to the right");
        assert_eq!(t.lines[0].spans[0].pm_pos, 4);
        assert!(t.lines[0].cell_x.is_some());
    }
}
