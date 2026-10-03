//! `Editor`: the document being edited, its selection, its history, its layout and its pages —
//! the façade a platform drives (the desktop's `PageCanvas` now; a WASM build later).
//!
//! The platform supplies text measurement ([`Measure`]) and the time of each change; it forwards
//! keys and pointer events as calls ([`Editor::move_caret`], [`Editor::press`],
//! [`Editor::drag_to`], the edit commands), paints [`Editor::pages`], and asks where the caret and
//! the selection are. Everything that is behaviour lives here, ported from the web's
//! `nav-keys.ts`, `pointer-select.ts` and its TipTap configuration.

use serde_json::Value;

use crate::edit::commands::{self, Change, EditState};
use crate::edit::history::{History, Selection};
use crate::edit::step::{EditError, Step, TopTouch};
use crate::layout::caret;
use crate::layout::flow::Flow;
use crate::layout::paginate::{self, SectionGeom};
use crate::layout::{CursorMetrics, DocPx, DocumentLayout, PageLayout, SelectionRect};
use crate::marks;
use crate::measure::Measure;
use crate::model::{self, Node};
use crate::pm::{self, node_at, textblock_at};

/// A4 at 96 dpi and the factory margin (`DocumentEditorPage.tsx`, `content_files.rs`).
pub const PAGE_W: DocPx = 794.0;
pub const PAGE_H: DocPx = 1123.0;
pub const MARGIN: DocPx = 96.0;
/// `COL_GAP` (`DocumentEditorPage.tsx:286`).
pub const COL_GAP: DocPx = 36.0;

/// The page of a section: size, margins, columns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSetup {
    pub width: DocPx,
    pub height: DocPx,
    pub top: DocPx,
    pub right: DocPx,
    pub bottom: DocPx,
    pub left: DocPx,
    pub columns: usize,
}

impl Default for PageSetup {
    fn default() -> Self {
        Self { width: PAGE_W, height: PAGE_H, top: MARGIN, right: MARGIN, bottom: MARGIN, left: MARGIN, columns: 1 }
    }
}

impl PageSetup {
    pub fn content_w(&self) -> DocPx {
        (self.width - self.left - self.right).max(1.0)
    }

    pub fn content_h(&self) -> DocPx {
        (self.height - self.top - self.bottom).max(1.0)
    }

    pub fn col_gap(&self) -> DocPx {
        if self.columns > 1 { COL_GAP } else { 0.0 }
    }

    /// The width of one text column.
    pub fn col_w(&self) -> DocPx {
        let n = self.columns.max(1) as f32;
        ((self.content_w() - (n - 1.0) * self.col_gap()) / n).max(1.0)
    }

    fn geom(&self) -> SectionGeom {
        SectionGeom { content_h: self.content_h(), columns: self.columns.max(1), col_w: self.col_w(), col_gap: self.col_gap() }
    }
}

/// Paper sizes in cm (`PAPER_SIZES`).
pub fn paper_size(name: &str) -> (f32, f32) {
    match name {
        "a5" => (14.8, 21.0),
        "a3" => (29.7, 42.0),
        "letter" => (21.59, 27.94),
        "legal" => (21.59, 35.56),
        _ => (21.0, 29.7),
    }
}

/// `PX_PER_CM`.
pub const PX_PER_CM: f32 = 96.0 / 2.54;

/// Caret motions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    Up,
    Down,
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
    /// One view height (document px) up or down.
    PageUp(i32),
    PageDown(i32),
}

/// What the ribbon shows of the selection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectionInfo {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub subscript: bool,
    pub superscript: bool,
    /// `None` when mixed.
    pub font_family: Option<String>,
    pub font_size: Option<String>,
    pub color: Option<String>,
    pub highlight: Option<String>,
    pub align: String,
    /// `paragraph`, `heading`, `codeBlock`.
    pub block: String,
    pub level: u32,
    /// `bulletList`, `orderedList`, `taskList` when in one.
    pub list: Option<String>,
    pub line_height: f32,
    pub space_before: f32,
    pub space_after: f32,
    pub indent_left: f32,
    pub indent_first_line: f32,
    pub indent_right: f32,
    /// `(position, type)`.
    pub tab_stops: Vec<(f32, String)>,
    pub style_name: Option<String>,
    pub link: Option<String>,
    pub in_table: bool,
    pub has_selection: bool,
    pub keep_next: bool,
    pub keep_lines: bool,
    pub page_break_before: bool,
    /// Small capitals on the typing marks.
    pub small_caps: bool,
}

/// The document being edited.
pub struct Editor {
    stored: model::Document,
    state: EditState,
    history: History,
    revs: Vec<u64>,
    next_rev: u64,
    flow: Flow,
    layout: Option<DocumentLayout>,
    pages: Vec<PageLayout>,
    setups: Vec<PageSetup>,
    /// `nav.goalX`: the column kept across ↑/↓/Page moves.
    goal_x: Option<DocPx>,
    /// `nav.atEnd`: the caret's affinity at a wrap boundary.
    at_end: bool,
    /// The unit and range of a press in progress (word / paragraph selection by drag).
    press: Option<Press>,
    /// The inverses of a live preview not yet recorded.
    transient: Vec<Step>,
    dirty: bool,
    /// Bumped on every change of the document (not of the selection).
    pub revision: u64,
}

#[derive(Clone, Copy, Debug)]
struct Press {
    unit: u8,
    anchor: usize,
    range: (usize, usize),
}

impl Editor {
    /// Opens stored bytes (`content_json`).
    pub fn open(bytes: &[u8]) -> Result<Self, model::Error> {
        Ok(Self::from_document(model::Document::from_slice(bytes)?))
    }

    pub fn from_document(stored: model::Document) -> Self {
        let blocks: Vec<Node> = stored.blocks().into_iter().cloned().collect();
        let doc = Node::doc(if blocks.is_empty() { vec![Node::of_type("paragraph")] } else { blocks });
        let n = doc.children().len();
        let setups = Self::setups_of(&stored, &doc);
        let mut e = Self {
            stored,
            state: EditState { doc, sel: Selection::caret(1), stored_marks: None },
            history: History::new(),
            revs: (1..=n as u64).collect(),
            next_rev: n as u64 + 1,
            flow: Flow::new(),
            layout: None,
            pages: Vec::new(),
            setups,
            goal_x: None,
            at_end: false,
            press: None,
            transient: Vec::new(),
            dirty: false,
            revision: 0,
        };
        // The caret starts at the first text position.
        if let Some((_, s)) = pm::textblocks(&e.state.doc).first() {
            e.state.sel = Selection::caret(*s);
        }
        e
    }

    /// The page setup of each section: section 0 from the envelope's `sections[0].margins` and
    /// `paperSize`, the others from their `sectionBreak` node.
    fn setups_of(stored: &model::Document, doc: &Node) -> Vec<PageSetup> {
        let mut base = PageSetup::default();
        let paper = stored.root_value("paperSize").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| "a4".into());
        let (w, h) = paper_size(&paper);
        let sec0 = stored.root_value("sections").and_then(|v| v.as_array().and_then(|a| a.first().cloned()));
        let landscape = sec0.as_ref().and_then(|s| s.get("orientation")).and_then(Value::as_str) == Some("landscape");
        base.width = ((if landscape { h } else { w }) * PX_PER_CM).round();
        base.height = ((if landscape { w } else { h }) * PX_PER_CM).round();
        if let Some(m) = sec0.as_ref().and_then(|s| s.get("margins")) {
            let g = |k: &str, d: f32| m.get(k).and_then(Value::as_f64).map(|v| v as f32).unwrap_or(d);
            base.top = g("top", MARGIN);
            base.right = g("right", MARGIN);
            base.bottom = g("bottom", MARGIN);
            base.left = g("left", MARGIN);
        }
        if let Some(c) = sec0.as_ref().and_then(|s| s.get("columns")).and_then(Value::as_u64) {
            base.columns = (c as usize).clamp(1, 3);
        }
        let mut out = vec![base];
        for node in doc.children() {
            if node.node_type() == Some("sectionBreak") {
                let a = node.attrs();
                let g = |k: &str, d: f32| a.get(k).and_then(Value::as_f64).map(|v| v as f32).unwrap_or(d);
                let land = a.get("orientation").and_then(Value::as_str) == Some("landscape");
                out.push(PageSetup {
                    width: ((if land { h } else { w }) * PX_PER_CM).round(),
                    height: ((if land { w } else { h }) * PX_PER_CM).round(),
                    top: g("top", base.top),
                    right: g("right", base.right),
                    bottom: g("bottom", base.bottom),
                    left: g("left", base.left),
                    columns: 1,
                });
            }
        }
        out
    }

    /// The document as stored, with the edits: the envelope and every sibling key kept.
    pub fn to_bytes(&mut self) -> Result<Vec<u8>, model::Error> {
        self.stored.set_blocks(self.state.doc.children().to_vec());
        self.stored.to_vec()
    }

    pub fn doc(&self) -> &Node {
        &self.state.doc
    }

    pub fn stored(&self) -> &model::Document {
        &self.stored
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// After a successful save.
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    pub fn selection(&self) -> Selection {
        self.state.sel
    }

    pub fn state(&self) -> &EditState {
        &self.state
    }

    pub fn page_setup(&self, section: usize) -> PageSetup {
        self.setups.get(section).or(self.setups.first()).copied().unwrap_or_default()
    }

    pub fn page_setups(&self) -> &[PageSetup] {
        &self.setups
    }

    /// Changes section 0's margins (the rulers). Live: `record` false while dragging; the release
    /// records it (written to `sections[0].margins`; a bare document becomes an envelope then,
    /// since it has nowhere else to keep its margins).
    pub fn set_margins(&mut self, left: f32, right: f32, top: f32, bottom: f32, record: bool) {
        if let Some(s) = self.setups.first_mut() {
            s.left = left.max(0.0);
            s.right = right.max(0.0);
            s.top = top.max(0.0);
            s.bottom = bottom.max(0.0);
        }
        self.invalidate_layout();
        if record {
            let m = serde_json::json!({ "top": top.round(), "right": right.round(), "bottom": bottom.round(), "left": left.round() });
            self.stored.set_section_margins(&m);
            self.dirty = true;
            self.revision += 1;
        }
    }

    /// The measurer changed (fonts were loaded, the device was recreated): lay everything out again.
    pub fn mark_measure_changed(&mut self) {
        self.flow.clear();
        self.invalidate_layout();
    }

    fn invalidate_layout(&mut self) {
        self.layout = None;
        self.pages.clear();
    }

    /// Lays the document out and paginates it if anything changed.
    pub fn relayout(&mut self, m: &dyn Measure) {
        if self.layout.is_some() {
            return;
        }
        // Sections are re-derived from the body (a section break may have been typed or deleted).
        let base = self.setups.first().copied().unwrap_or_default();
        let mut setups = vec![base];
        for node in self.state.doc.children() {
            if node.node_type() == Some("sectionBreak") {
                let a = node.attrs();
                let g = |k: &str, d: f32| a.get(k).and_then(Value::as_f64).map(|v| v as f32).unwrap_or(d);
                let land = a.get("orientation").and_then(Value::as_str) == Some("landscape");
                let (w, h) = if land { (base.height.max(base.width), base.width.min(base.height)) } else { (base.width.min(base.height), base.height.max(base.width)) };
                setups.push(PageSetup { width: w, height: h, top: g("top", base.top), right: g("right", base.right), bottom: g("bottom", base.bottom), left: g("left", base.left), columns: 1 });
            }
        }
        self.setups = setups;
        let widths: Vec<DocPx> = self.setups.iter().map(PageSetup::col_w).collect();
        let w0 = widths[0];
        let layout = self.flow.layout(&self.state.doc, &self.revs, &|s| widths.get(s).copied().unwrap_or(w0), m);
        let geoms: Vec<SectionGeom> = self.setups.iter().map(PageSetup::geom).collect();
        self.pages = paginate::paginate(&layout, &geoms);
        self.layout = Some(layout);
    }

    /// The continuous layout (after [`Editor::relayout`]).
    pub fn layout(&self) -> Option<&DocumentLayout> {
        self.layout.as_ref()
    }

    /// The pages (after [`Editor::relayout`]).
    pub fn pages(&self) -> &[PageLayout] {
        &self.pages
    }

    /// How many blocks the last layout laid out afresh.
    pub fn last_layout_misses(&self) -> usize {
        self.flow.last_misses
    }

    // ── Coordinates: continuous layout ↔ page ───────────────────────────────

    /// The page and the column band showing the continuous ordinate `y` of a line holding `pos`
    /// (page index, band start, band x shift).
    pub fn page_band_of(&self, pos: usize, y: DocPx) -> Option<(usize, DocPx, DocPx)> {
        let page = paginate::page_of_pos(&self.pages, pos).unwrap_or_else(|| paginate::page_of_y(&self.pages, y));
        let p = self.pages.get(page)?;
        let band = p.columns.iter().rev().find(|b| b.start_y <= y + 0.01).or(p.columns.first()).copied().unwrap_or(crate::layout::ColumnBand { start_y: p.start_y, x_shift: 0.0 });
        Some((page, band.start_y, band.x_shift))
    }

    /// A point on page `page` (content-box coordinates) → the continuous layout's coordinates.
    pub fn page_to_continuous(&self, page: usize, x: DocPx, y: DocPx) -> (DocPx, DocPx) {
        let Some(p) = self.pages.get(page) else { return (x, y) };
        if p.columns.len() > 1 {
            let col_w = self.page_setup(p.sec_idx).col_w();
            for b in p.columns.iter().rev() {
                if x >= b.x_shift - COL_GAP / 2.0 || b.x_shift == 0.0 {
                    let _ = col_w;
                    return (x - b.x_shift, b.start_y + y);
                }
            }
        }
        let start = p.columns.first().map(|b| b.start_y).unwrap_or(p.start_y);
        (x, start + y)
    }

    // ── Caret and selection geometry ────────────────────────────────────────

    /// The caret (continuous coordinates).
    pub fn caret_metrics(&self, m: &dyn Measure) -> Option<CursorMetrics> {
        let layout = self.layout.as_ref()?;
        Some(caret::pos_to_coords(layout, self.state.sel.head, self.at_end, m))
    }

    /// The caret's page and its box on that page (content-box coordinates): `(page, x, y, h, lean)`.
    pub fn caret_on_page(&self, m: &dyn Measure) -> Option<(usize, DocPx, DocPx, DocPx, f32)> {
        let c = self.caret_metrics(m)?;
        let (page, start, shift) = self.page_band_of(self.state.sel.head, c.line_top.unwrap_or(c.y))?;
        Some((page, c.x + shift, c.y - start, c.height, c.italic_angle))
    }

    /// The selection's rectangles per page (content-box coordinates).
    pub fn selection_on_pages(&self, m: &dyn Measure) -> Vec<(usize, SelectionRect)> {
        let Some(layout) = self.layout.as_ref() else { return Vec::new() };
        let (from, to) = (self.state.sel.from(), self.state.sel.to());
        let mut out = Vec::new();
        for r in caret::selection_rects(layout, from, to, m) {
            let pos = caret::coords_to_pos(layout, r.x + 0.5, r.y + 0.5, m);
            if let Some((page, start, shift)) = self.page_band_of(pos, r.y) {
                let ch = self.page_setup(self.pages[page].sec_idx).content_h();
                let mut rr = SelectionRect { x: r.x + shift, y: r.y - start, w: r.w, h: r.h };
                // Never past the page's content box (the joint overlap reaches the next page).
                if rr.y + rr.h > ch {
                    rr.h = (ch - rr.y).max(0.0);
                }
                out.push((page, rr));
            }
        }
        out
    }

    // ── Selection and navigation ────────────────────────────────────────────

    pub fn set_selection(&mut self, sel: Selection) {
        let size = pm::content_size(&self.state.doc);
        self.state.sel = Selection::new(sel.anchor.min(size), sel.head.min(size));
        self.state.stored_marks = None;
        self.goal_x = None;
    }

    /// Ctrl+A.
    pub fn select_all(&mut self) {
        let tbs = pm::textblocks(&self.state.doc);
        if let (Some((_, s)), Some((lp, ls))) = (tbs.first(), tbs.last()) {
            let len = node_at(&self.state.doc, lp).map(|n| crate::edit::inline::content_len(n.children())).unwrap_or(0);
            self.state.sel = Selection::new(*s, ls + len);
        }
        self.goal_x = None;
        self.at_end = true;
    }

    /// Moves the caret (`extend`: Shift held), as `nav-keys.ts` and the browser do.
    pub fn move_caret(&mut self, motion: Motion, extend: bool, m: &dyn Measure) {
        let Some(layout) = self.layout.as_ref() else { return };
        let sel = self.state.sel;
        let head = sel.head;
        let vertical = matches!(motion, Motion::Up | Motion::Down | Motion::PageUp(_) | Motion::PageDown(_));
        let mut keep_goal = false;
        let mut new_at_end = false;
        let new_head = match motion {
            // A selection collapses to its edge (the browser's ←/→).
            Motion::Left if !extend && !sel.is_empty() => sel.from(),
            Motion::Right if !extend && !sel.is_empty() => sel.to(),
            Motion::Left => commands::prev_char_pos(&self.state.doc, head).unwrap_or(head),
            Motion::Right => commands::next_char_pos(&self.state.doc, head).unwrap_or(head),
            Motion::WordLeft => {
                let p = caret::prev_word_pos(layout, head);
                if p == head {
                    commands::prev_char_pos(&self.state.doc, head).map(|q| caret::prev_word_pos(layout, q).min(q)).unwrap_or(head)
                } else {
                    p
                }
            }
            Motion::WordRight => {
                let p = caret::next_word_pos(layout, head);
                if p == head {
                    commands::next_char_pos(&self.state.doc, head).unwrap_or(head)
                } else {
                    p
                }
            }
            Motion::LineStart => caret::line_start_at(layout, head, self.at_end),
            Motion::LineEnd => {
                new_at_end = true;
                caret::line_end_at(layout, head, self.at_end)
            }
            Motion::DocStart => caret::doc_start(layout),
            Motion::DocEnd => caret::doc_end(layout),
            Motion::Up | Motion::Down => {
                let cm = caret::pos_to_coords(layout, head, self.at_end, m);
                let gx = *self.goal_x.get_or_insert(cm.x);
                let dir = if motion == Motion::Up { -1 } else { 1 };
                match caret::adjacent_line_center(layout, cm.line_top.unwrap_or(cm.y), dir) {
                    None => head,
                    Some(ty) => {
                        let p = caret::coords_to_pos(layout, gx, ty, m);
                        let e = caret::pos_to_coords(layout, p, true, m);
                        let s = caret::pos_to_coords(layout, p, false, m);
                        let (te, ts) = (e.line_top.unwrap_or(e.y), s.line_top.unwrap_or(s.y));
                        new_at_end = te != ts
                            && (te + e.line_h.unwrap_or(e.height) / 2.0 - ty).abs() < (ts + s.line_h.unwrap_or(s.height) / 2.0 - ty).abs();
                        keep_goal = true;
                        p
                    }
                }
            }
            Motion::PageUp(vh) | Motion::PageDown(vh) => {
                let cm = caret::pos_to_coords(layout, head, self.at_end, m);
                let gx = *self.goal_x.get_or_insert(cm.x);
                let dy = if matches!(motion, Motion::PageDown(_)) { vh as f32 } else { -(vh as f32) };
                keep_goal = true;
                caret::coords_to_pos(layout, gx, cm.y + dy, m)
            }
        };
        self.at_end = new_at_end;
        if !(vertical && keep_goal) {
            self.goal_x = None;
        }
        let anchor = if extend { sel.anchor } else { new_head };
        self.state.sel = Selection::new(anchor, new_head);
        self.state.stored_marks = None;
    }

    /// A press at continuous `(x, y)`: `clicks` 1 = caret (Shift: extend), 2 = word, 3 = paragraph.
    pub fn press(&mut self, x: DocPx, y: DocPx, clicks: u32, shift: bool, m: &dyn Measure) {
        let Some(layout) = self.layout.as_ref() else { return };
        let pos = caret::coords_to_pos(layout, x, y, m);
        self.goal_x = None;
        self.state.stored_marks = None;
        // A click past the end of a wrapped line keeps the caret on that line.
        self.at_end = Self::is_line_end_hit(layout, pos, y);
        let unit = if clicks >= 3 { 2 } else if clicks == 2 { 1 } else { 0 };
        if unit == 0 {
            let anchor = if shift { self.state.sel.anchor } else { pos };
            self.state.sel = Selection::new(anchor, pos);
            self.press = Some(Press { unit: 0, anchor, range: (pos, pos) });
            return;
        }
        let range = if unit == 1 { caret::word_boundaries_at(layout, pos) } else { self.paragraph_range_at(pos) };
        self.state.sel = Selection::new(range.0, range.1);
        self.press = Some(Press { unit, anchor: range.0, range });
    }

    fn is_line_end_hit(layout: &DocumentLayout, pos: usize, y: DocPx) -> bool {
        for p in &layout.paragraphs {
            for (i, l) in p.lines.iter().enumerate() {
                if y >= l.y && y <= l.y + l.height && pos == l.pm_end {
                    return p.lines.get(i + 1).is_some_and(|n| n.pm_start == pos);
                }
            }
        }
        false
    }

    /// Triple-click: inside a table cell, the cell's deepest paragraph; elsewhere the layout's
    /// paragraph (`paragraphRangeAt`).
    pub fn paragraph_range_at(&self, pos: usize) -> (usize, usize) {
        if let Some((path, start)) = textblock_at(&self.state.doc, pos) {
            let in_cell = (0..path.len()).any(|d| node_at(&self.state.doc, &path[..d]).is_some_and(|n| matches!(n.node_type(), Some("tableCell") | Some("tableHeader"))));
            if in_cell {
                let len = node_at(&self.state.doc, &path).map(|n| crate::edit::inline::content_len(n.children())).unwrap_or(0);
                return (start, start + len);
            }
        }
        match self.layout.as_ref() {
            Some(l) => caret::paragraph_boundaries_at(l, pos),
            None => (pos, pos),
        }
    }

    /// Dragging with the button held: extends by character, word or paragraph.
    pub fn drag_to(&mut self, x: DocPx, y: DocPx, m: &dyn Measure) {
        let Some(press) = self.press else { return };
        let Some(layout) = self.layout.as_ref() else { return };
        let p2 = caret::coords_to_pos(layout, x, y, m);
        if press.unit == 0 {
            self.state.sel = Selection::new(press.anchor, p2);
            self.at_end = Self::is_line_end_hit(layout, p2, y);
            return;
        }
        let r2 = if press.unit == 1 { caret::word_boundaries_at(layout, p2) } else { self.paragraph_range_at(p2) };
        let (rf, rt) = press.range;
        self.state.sel = if p2 >= rf { Selection::new(rf, r2.1.max(rt)) } else { Selection::new(rt, r2.0.min(rf)) };
    }

    /// The button was released.
    pub fn release(&mut self) {
        self.press = None;
    }

    pub fn pressing(&self) -> bool {
        self.press.is_some()
    }

    // ── Editing ─────────────────────────────────────────────────────────────

    /// Runs an edit command, records it in the history (time `now_ms`), and keeps the layout's
    /// revision counters, the dirty flag and the trailing paragraph (TipTap `TrailingNode`).
    pub fn apply(&mut self, now_ms: u64, f: impl FnOnce(&mut EditState) -> Result<Change, EditError>) -> Result<(), EditError> {
        let mut change = f(&mut self.state)?;
        if change.is_empty() {
            // A stored-marks-only change (a toggle on a caret).
            self.revision += 1;
            return Ok(());
        }
        // TrailingNode: the body always ends with a paragraph.
        let ends_with_paragraph = self.state.doc.children().last().is_some_and(|n| n.node_type() == Some("paragraph"));
        if !ends_with_paragraph {
            let n = self.state.doc.children().len();
            let step = Step { parent: vec![], from: n, to: n, with: vec![Node::of_type("paragraph")] };
            if let Ok(inv) = step.apply(&mut self.state.doc) {
                change.steps.push(step);
                change.inverses.push(inv);
            }
        }
        self.touch(&change.steps);
        self.history.record(change.steps, change.inverses, change.before, change.after, now_ms, change.merge);
        self.after_change();
        Ok(())
    }

    fn after_change(&mut self) {
        self.dirty = true;
        self.revision += 1;
        self.goal_x = None;
        self.at_end = false;
        self.invalidate_layout();
    }

    /// Bumps the revision of every top-level block the steps touched.
    fn touch(&mut self, steps: &[Step]) {
        for s in steps {
            match s.top_level_touch() {
                TopTouch::Block(i) => {
                    if let Some(r) = self.revs.get_mut(i) {
                        *r = self.next_rev;
                        self.next_rev += 1;
                    }
                }
                TopTouch::Splice { from, to, inserted } => {
                    let to = to.min(self.revs.len());
                    let from = from.min(to);
                    let fresh: Vec<u64> = (0..inserted as u64).map(|k| self.next_rev + k).collect();
                    self.next_rev += inserted as u64;
                    self.revs.splice(from..to, fresh);
                }
            }
        }
    }

    /// A live preview (a ruler drag in progress): applied and laid out, not recorded. Successive
    /// previews replace each other; [`Editor::commit_transient`] records one step from the state
    /// before the first preview, [`Editor::cancel_transient`] restores it.
    pub fn apply_transient(&mut self, f: impl FnOnce(&mut EditState) -> Result<Change, EditError>) -> Result<(), EditError> {
        self.revert_transient();
        let sel = self.state.sel;
        let change = f(&mut self.state)?;
        self.state.sel = sel;
        self.touch(&change.steps);
        self.transient = change.inverses;
        self.revision += 1;
        self.invalidate_layout();
        Ok(())
    }

    fn revert_transient(&mut self) {
        let inverses = std::mem::take(&mut self.transient);
        let mut applied = Vec::new();
        for inv in inverses.iter().rev() {
            if let Ok(redo) = inv.apply(&mut self.state.doc) {
                applied.push(inv.clone());
                let _ = redo;
            }
        }
        if !applied.is_empty() {
            self.touch(&applied);
            self.invalidate_layout();
        }
    }

    /// Ends a preview with the final change, recorded as one undo step.
    pub fn commit_transient(&mut self, now_ms: u64, f: impl FnOnce(&mut EditState) -> Result<Change, EditError>) -> Result<(), EditError> {
        self.revert_transient();
        let r = self.apply(now_ms, f);
        self.history.stop_capturing();
        r
    }

    /// Ends a preview without a change.
    pub fn cancel_transient(&mut self) {
        self.revert_transient();
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo(&mut self) -> bool {
        match self.history.undo(&mut self.state.doc) {
            Some((sel, steps)) => {
                self.touch(&steps);
                self.state.sel = sel;
                self.state.stored_marks = None;
                self.after_change();
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.history.redo(&mut self.state.doc) {
            Some((sel, steps)) => {
                self.touch(&steps);
                self.state.sel = sel;
                self.state.stored_marks = None;
                self.after_change();
                true
            }
            None => false,
        }
    }

    /// The previous / next word position from the caret (for Ctrl+Backspace / Ctrl+Delete).
    pub fn word_targets(&self) -> (Option<usize>, Option<usize>) {
        match self.layout.as_ref() {
            Some(l) => (Some(caret::prev_word_pos(l, self.state.sel.head)), Some(caret::next_word_pos(l, self.state.sel.head))),
            None => (None, None),
        }
    }

    /// The selection as blocks and as plain text (Copy / Cut).
    pub fn selection_payload(&self) -> Option<(Vec<Node>, String)> {
        let (f, t) = (self.state.sel.from(), self.state.sel.to());
        if f == t {
            return None;
        }
        Some((crate::edit::clipboard::slice(&self.state.doc, f, t), crate::edit::clipboard::plain_text(&self.state.doc, f, t)))
    }

    // ── What the ribbon shows ───────────────────────────────────────────────

    pub fn selection_info(&self) -> SelectionInfo {
        let s = &self.state;
        let mut info = SelectionInfo {
            bold: commands::mark_active(s, "bold"),
            italic: commands::mark_active(s, "italic"),
            underline: commands::mark_active(s, "underline"),
            strike: commands::mark_active(s, "strike"),
            subscript: commands::mark_active(s, "subscript"),
            superscript: commands::mark_active(s, "superscript"),
            font_family: commands::uniform_text_style(s, "fontFamily", "Arial"),
            font_size: commands::uniform_text_style(s, "fontSize", "11").map(|v| {
                marks::parse_float(&Value::String(v.clone())).map(|f| format!("{}", f.round() as i64)).unwrap_or(v)
            }),
            has_selection: !s.sel.is_empty(),
            line_height: crate::layout::LH_RATIO,
            align: "left".into(),
            block: "paragraph".into(),
            ..Default::default()
        };
        let typing = commands::typing_marks(&s.doc, s.sel.from(), s.stored_marks.as_ref());
        let tm = marks::extract(&typing);
        info.color = tm.color.clone();
        info.highlight = typing
            .iter()
            .find(|m| m.get("type").and_then(Value::as_str) == Some("highlight"))
            .map(|m| m.get("attrs").and_then(|a| a.get("color")).and_then(Value::as_str).unwrap_or("#fff176").to_string());
        info.link = tm.link;
        info.small_caps = tm.caps;
        if let Some((path, _)) = textblock_at(&s.doc, s.sel.head) {
            if let Some(n) = node_at(&s.doc, &path) {
                let a = n.attrs();
                let num = |k: &str| a.get(k).and_then(marks::parse_float).unwrap_or(0.0);
                info.block = n.node_type().unwrap_or("paragraph").to_string();
                info.level = crate::layout::parse::heading_level(n) as u32;
                info.align = a.get("textAlign").and_then(Value::as_str).unwrap_or("left").to_string();
                info.line_height = a.get("lineHeight").and_then(marks::parse_float).filter(|v| *v > 0.0).unwrap_or(crate::layout::LH_RATIO);
                info.space_before = num("spaceBefore");
                info.space_after = num("spaceAfter");
                info.indent_left = num("indentLeft");
                info.indent_first_line = num("indentFirstLine");
                info.indent_right = num("indentRight");
                info.keep_next = a.get("keepNext").is_some_and(marks::truthy);
                info.keep_lines = a.get("keepLines").is_some_and(marks::truthy);
                info.page_break_before = a.get("pageBreakBefore").is_some_and(marks::truthy);
                info.style_name = a.get("styleName").and_then(Value::as_str).map(str::to_string);
                if let Some(Value::Array(ts)) = a.get("tabStops") {
                    info.tab_stops = ts
                        .iter()
                        .filter_map(|t| match t {
                            Value::Number(n) => n.as_f64().map(|p| (p as f32, "left".to_string())),
                            Value::Object(o) => o.get("pos").and_then(Value::as_f64).map(|p| (p as f32, o.get("type").and_then(Value::as_str).unwrap_or("left").to_string())),
                            _ => None,
                        })
                        .collect();
                }
            }
            for d in (0..path.len()).rev() {
                if let Some(n) = node_at(&s.doc, &path[..d]) {
                    match n.node_type() {
                        Some(t @ ("bulletList" | "orderedList" | "taskList")) if info.list.is_none() => info.list = Some(t.to_string()),
                        Some("tableCell") | Some("tableHeader") => info.in_table = true,
                        _ => {}
                    }
                }
            }
        }
        info
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::commands as cmd;
    use crate::measure::FixedMeasure;

    fn editor(json: &str) -> Editor {
        let mut e = Editor::open(json.as_bytes()).expect("opens");
        e.relayout(&FixedMeasure);
        e
    }

    fn text(e: &Editor) -> String {
        crate::edit::clipboard::plain_text(e.doc(), 0, pm::content_size(e.doc()))
    }

    const DOC: &str = r#"{"content":[{"content":[{"text":"hello world","type":"text"}],"type":"paragraph"},{"content":[{"text":"second","type":"text"}],"type":"paragraph"}],"type":"doc"}"#;

    #[test]
    fn typing_enter_backspace_and_undo() {
        let mut e = editor(DOC);
        e.set_selection(Selection::caret(6));
        e.apply(1000, |s| cmd::insert_text(s, ",")).expect("types");
        e.apply(1100, cmd::enter).expect("splits");
        e.relayout(&FixedMeasure);
        assert_eq!(text(&e), "hello,\n world\nsecond");
        e.apply(1200, |s| cmd::delete_backward(s, false, None)).expect("joins");
        assert_eq!(text(&e), "hello, world\nsecond");
        // All within 500 ms of each other: one undo step.
        assert!(e.undo());
        assert_eq!(text(&e), "hello world\nsecond");
        assert_eq!(e.selection(), Selection::caret(6));
        assert!(e.redo());
        assert_eq!(text(&e), "hello, world\nsecond");
        assert!(e.is_dirty());
    }

    #[test]
    fn enter_at_a_wrap_point_keeps_the_space() {
        let mut e = editor(r#"{"content":[{"content":[{"text":"aaaa bbbb cccc","type":"text"}],"type":"paragraph"}],"type":"doc"}"#);
        e.set_selection(Selection::caret(5));
        e.apply(1000, cmd::enter).expect("splits");
        e.apply(1001, |s| cmd::insert_text(s, "X")).expect("types");
        assert_eq!(text(&e), "aaaa
X bbbb cccc");
    }

    #[test]
    fn the_layout_is_incremental() {
        let mut e = editor(DOC);
        e.set_selection(Selection::caret(3));
        e.apply(1000, |s| cmd::insert_text(s, "x")).expect("types");
        e.relayout(&FixedMeasure);
        assert_eq!(e.last_layout_misses(), 1);
    }

    #[test]
    fn arrows_move_across_blocks_and_keep_the_goal_column() {
        let mut e = editor(DOC);
        e.set_selection(Selection::caret(12));
        e.move_caret(Motion::Right, false, &FixedMeasure);
        assert_eq!(e.selection().head, 14, "end of the first block → start of the second");
        e.move_caret(Motion::Left, false, &FixedMeasure);
        assert_eq!(e.selection().head, 12);
        e.set_selection(Selection::caret(4));
        e.move_caret(Motion::Down, false, &FixedMeasure);
        assert_eq!(e.selection().head, 17);
        e.move_caret(Motion::Up, true, &FixedMeasure);
        assert_eq!(e.selection(), Selection::new(17, 4));
        e.move_caret(Motion::LineEnd, false, &FixedMeasure);
        assert_eq!(e.selection().head, 12);
        e.move_caret(Motion::DocEnd, true, &FixedMeasure);
        assert_eq!(e.selection(), Selection::new(12, 20));
    }

    #[test]
    fn double_and_triple_click_select_word_and_paragraph() {
        let mut e = editor(DOC);
        let layout = e.layout().cloned().expect("laid out");
        let c = caret::pos_to_coords(&layout, 9, false, &FixedMeasure);
        e.press(c.x, c.y + 2.0, 2, false, &FixedMeasure);
        assert_eq!(e.selection(), Selection::new(7, 12));
        e.release();
        e.press(c.x, c.y + 2.0, 3, false, &FixedMeasure);
        assert_eq!(e.selection(), Selection::new(1, 12));
        e.release();
    }

    #[test]
    fn bold_on_a_selection_and_on_a_caret() {
        let mut e = editor(DOC);
        e.set_selection(Selection::new(1, 6));
        e.apply(1000, |s| cmd::toggle_mark(s, "bold")).expect("bold");
        assert!(e.selection_info().bold);
        e.set_selection(Selection::caret(12));
        assert!(!e.selection_info().bold);
        e.apply(1000, |s| cmd::toggle_mark(s, "bold")).expect("stored");
        assert!(e.selection_info().bold);
        e.apply(1001, |s| cmd::insert_text(s, "!")).expect("types");
        let p = &e.doc().children()[0];
        assert!(crate::marks::text_mark_of(p.children().last().expect("a run")).bold);
    }

    #[test]
    fn a_ruler_drag_previews_then_records_one_step() {
        let mut e = editor(DOC);
        e.set_selection(Selection::caret(3));
        for left in [10.0, 20.0, 30.0] {
            e.apply_transient(|s| cmd::set_indents(s, left, 0.0, 0.0)).expect("previews");
        }
        assert_eq!(e.selection_info().indent_left, 30.0);
        assert!(!e.can_undo());
        e.commit_transient(1000, |s| cmd::set_indents(s, 40.0, 0.0, 0.0)).expect("commits");
        assert_eq!(e.selection_info().indent_left, 40.0);
        assert!(e.undo());
        assert_eq!(e.selection_info().indent_left, 0.0, "undo goes back to before the drag");
    }

    #[test]
    fn saving_keeps_the_envelope() {
        let env = r#"{"_type":"multi-page","footer":{"content":[],"type":"doc"},"pages":[{"content":{"content":[{"content":[{"text":"a","type":"text"}],"type":"paragraph"}],"type":"doc"},"id":"p0","sectionId":"s0"}],"sections":[{"id":"s0","margins":{"bottom":96,"left":120,"right":96,"top":96},"orientation":"portrait"}]}"#;
        let mut e = editor(env);
        assert_eq!(e.page_setup(0).left, 120.0);
        e.set_selection(Selection::caret(2));
        e.apply(1000, |s| cmd::insert_text(s, "b")).expect("types");
        let out = String::from_utf8(e.to_bytes().expect("serialises")).expect("utf-8");
        assert!(out.contains(r#""footer":{"content":[],"type":"doc"}"#));
        assert!(out.contains(r#""text":"ab""#));
        assert!(out.contains(r#""id":"p0""#));
    }
}
