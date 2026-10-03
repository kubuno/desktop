//! The editing commands, as the web runs them (TipTap/ProseMirror commands configured by
//! `DocumentEditorPage.tsx`, keys by `documents/text-surface/nav-keys.ts`).
//!
//! A command takes the [`EditState`] (document, selection, stored marks), applies its steps and
//! returns a [`Change`] (forward steps, inverses, selections, and whether it may merge into the open
//! undo group). It never touches the history or the layout: the editor does.

use serde_json::{json, Map, Value};

use super::history::Selection;
use super::inline;
use super::step::{EditError, Step, Tr};
use super::structure::{self, CaretAt};
use crate::marks;
use crate::model::Node;
use crate::pm::{self, node_at, textblock_at, textblocks, Path};

/// `INDENT_STEP` (`nav-keys.ts`): Tab / Shift+Tab / Backspace indent step, px.
pub const INDENT_STEP: f32 = 48.0;
/// Font size bounds of grow/shrink (`setSize`, `DocumentEditorPage.tsx:11905`).
pub const MIN_FONT_PT: f32 = 6.0;
pub const MAX_FONT_PT: f32 = 96.0;
/// The marks an empty paragraph inherits from the text before it (`INHERIT_MARK_TYPES`).
pub const INHERIT_MARK_TYPES: &[&str] = &["textStyle", "bold", "italic", "underline", "strike", "superscript", "subscript"];

/// The document, the selection and the stored marks (ProseMirror `storedMarks`: the marks the next
/// typed character takes, set by toggling a mark on an empty selection or by Enter).
#[derive(Clone, Debug, Default)]
pub struct EditState {
    pub doc: Node,
    pub sel: Selection,
    pub stored_marks: Option<Vec<Value>>,
}

/// An applied change.
#[derive(Clone, Debug, Default)]
pub struct Change {
    pub steps: Vec<Step>,
    pub inverses: Vec<Step>,
    pub before: Selection,
    pub after: Selection,
    /// May join the open undo group (all edits may, per the web's time rule; kept for callers that
    /// want a command on its own).
    pub merge: bool,
}

impl Change {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// Runs `f` on a transaction over the state's document; on error everything is rolled back.
fn run(state: &mut EditState, f: impl FnOnce(&mut Tr<'_>, Selection, Option<Vec<Value>>) -> Result<Selection, EditError>) -> Result<Change, EditError> {
    let before = state.sel;
    let stored = state.stored_marks.clone();
    let mut tr = Tr::new(&mut state.doc);
    match f(&mut tr, before, stored) {
        Ok(after) => {
            let (steps, inverses) = (std::mem::take(&mut tr.steps), std::mem::take(&mut tr.inverses));
            drop(tr);
            state.sel = after;
            Ok(Change { steps, inverses, before, after, merge: true })
        }
        Err(e) => {
            tr.rollback();
            Err(e)
        }
    }
}

fn caret(doc: &Node, at: &CaretAt) -> Result<Selection, EditError> {
    at.pos(doc).map(Selection::caret).ok_or(EditError::BadPosition)
}

fn ty(n: &Node) -> &str {
    n.node_type().unwrap_or("")
}

/// Deletes the selection if it is not empty; returns the caret position after it.
fn delete_selection(tr: &mut Tr<'_>, sel: Selection) -> Result<usize, EditError> {
    if sel.is_empty() {
        return Ok(sel.head);
    }
    let at = structure::delete_range(tr, sel.from(), sel.to())?;
    at.pos(tr.doc).ok_or(EditError::BadPosition)
}

/// The marks text typed at `pos` takes: the stored marks, else `$pos.marks()`, else what an empty
/// paragraph inherits (`InheritFontExt`: its `fontMarks`, else the marks of the last marked text
/// before it, restricted to [`INHERIT_MARK_TYPES`]).
pub fn typing_marks(doc: &Node, pos: usize, stored: Option<&Vec<Value>>) -> Vec<Value> {
    if let Some(s) = stored {
        return s.clone();
    }
    let Some((path, start)) = textblock_at(doc, pos) else { return Vec::new() };
    let Some(block) = node_at(doc, &path) else { return Vec::new() };
    if ty(block) == "codeBlock" {
        return Vec::new();
    }
    let here = inline::marks_at(block.children(), pos - start);
    if !here.is_empty() || inline::content_len(block.children()) > 0 {
        return here;
    }
    inherited_marks(doc, &path, pos)
}

/// `InheritFontExt` for an empty textblock at `path`.
pub fn inherited_marks(doc: &Node, path: &[usize], pos: usize) -> Vec<Value> {
    let Some(block) = node_at(doc, path) else { return Vec::new() };
    if let Some(fm) = block.attr("fontMarks").and_then(|v| v.as_object().cloned()) {
        let mut out = Vec::new();
        let mut ts = Map::new();
        if let Some(ff) = fm.get("ff").filter(|v| marks::truthy(v)) {
            ts.insert("fontFamily".into(), ff.clone());
        }
        if let Some(fs) = fm.get("fs").filter(|v| marks::truthy(v)) {
            ts.insert("fontSize".into(), fs.clone());
        }
        if !ts.is_empty() {
            out.push(json!({ "type": "textStyle", "attrs": Value::Object(ts) }));
        }
        for (k, mark) in [("b", "bold"), ("i", "italic"), ("u", "underline"), ("s", "strike")] {
            if fm.get(k).is_some_and(marks::truthy) {
                out.push(marks::simple(mark));
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    // The last marked text before the caret, anywhere before it in the document.
    let mut inherited: Vec<Value> = Vec::new();
    for (p, s) in textblocks(doc) {
        if s > pos {
            break;
        }
        if let Some(n) = node_at(doc, &p) {
            for child in n.children() {
                if pm::is_text(child) {
                    let m = marks::marks_of(child);
                    if !m.is_empty() {
                        inherited = m;
                    }
                }
            }
        }
    }
    inherited
        .into_iter()
        .filter(|m| m.get("type").and_then(Value::as_str).is_some_and(|t| INHERIT_MARK_TYPES.contains(&t)))
        .collect()
}

/// Typing: replaces the selection with `text`.
pub fn insert_text(state: &mut EditState, text: &str) -> Result<Change, EditError> {
    if text.is_empty() {
        return Err(EditError::NotApplicable);
    }
    // Marks are read before the deletion (`marksAcross` of the selection, approximated by the marks
    // at its start).
    let marks = typing_marks(&state.doc, state.sel.from(), state.stored_marks.as_ref());
    let change = run(state, |tr, sel, _| {
        let pos = delete_selection(tr, sel)?;
        let (path, start) = textblock_at(tr.doc, pos).ok_or(EditError::BadPosition)?;
        let block = node_at(tr.doc, &path).ok_or(EditError::NoSuchNode)?.clone();
        let kids = inline::insert_text(block.children(), pos - start, text, &marks)?;
        tr.replace_node(&path, structure::with_children(&block, kids))?;
        Ok(Selection::caret(pos + inline::len16(text)))
    })?;
    state.stored_marks = None;
    Ok(change)
}

/// Inserts inline nodes (an inline image, a field) at the selection.
pub fn insert_inline(state: &mut EditState, nodes: Vec<Node>) -> Result<Change, EditError> {
    let len: usize = nodes.iter().map(pm::node_size).sum();
    run(state, |tr, sel, _| {
        let pos = delete_selection(tr, sel)?;
        let (path, start) = textblock_at(tr.doc, pos).ok_or(EditError::BadPosition)?;
        let block = node_at(tr.doc, &path).ok_or(EditError::NoSuchNode)?.clone();
        let kids = inline::replace(block.children(), pos - start, pos - start, nodes)?;
        tr.replace_node(&path, structure::with_children(&block, kids))?;
        Ok(Selection::caret(pos + len))
    })
}

/// Shift+Enter: a hard break (QUIRK: the web's canvas draws nothing for it).
pub fn insert_hard_break(state: &mut EditState) -> Result<Change, EditError> {
    insert_inline(state, vec![Node::of_type("hardBreak")])
}

/// Enter: `newlineInCode`, `splitListItem`, `liftEmptyBlock`, `splitBlock` (keeping the marks).
pub fn enter(state: &mut EditState) -> Result<Change, EditError> {
    let keep = {
        let pos = state.sel.from();
        textblock_at(&state.doc, pos).and_then(|(p, s)| {
            let b = node_at(&state.doc, &p)?;
            (pos > s).then(|| inline::marks_at(b.children(), pos - s))
        })
    };
    // In a code block: a newline character.
    if let Some((p, _)) = textblock_at(&state.doc, state.sel.from()) {
        if node_at(&state.doc, &p).is_some_and(|n| ty(n) == "codeBlock") {
            return insert_text(state, "\n");
        }
    }
    let change = run(state, |tr, sel, _| {
        let pos = delete_selection(tr, sel)?;
        let (path, _) = textblock_at(tr.doc, pos).ok_or(EditError::BadPosition)?;
        if structure::list_item_of(tr.doc, &path).is_some() {
            let at = structure::split_list_item(tr, pos)?;
            return caret(tr.doc, &at);
        }
        // liftEmptyBlock: an empty block inside a block quote leaves it.
        let block = node_at(tr.doc, &path).ok_or(EditError::NoSuchNode)?;
        if inline::content_len(block.children()) == 0 && path.len() > 1 {
            if let Some(parent) = node_at(tr.doc, &path[..path.len() - 1]) {
                if ty(parent) == "blockquote" {
                    return lift_out_of_parent(tr, &path).and_then(|at| caret(tr.doc, &at));
                }
            }
        }
        let at = structure::split_block(tr, pos)?;
        caret(tr.doc, &at)
    })?;
    state.stored_marks = keep.filter(|m| !m.is_empty());
    Ok(change)
}

/// Moves the block at `path` out of its parent (a block quote), splitting the parent around it.
fn lift_out_of_parent(tr: &mut Tr<'_>, path: &[usize]) -> Result<CaretAt, EditError> {
    let (&i, parent_path) = path.split_last().ok_or(EditError::BadPosition)?;
    let parent = node_at(tr.doc, parent_path).ok_or(EditError::NoSuchNode)?.clone();
    let block = parent.children()[i].clone();
    let before: Vec<Node> = parent.children()[..i].to_vec();
    let after: Vec<Node> = parent.children()[i + 1..].to_vec();
    let mut with = Vec::new();
    if !before.is_empty() {
        with.push(structure::with_children(&parent, before));
    }
    let at = with.len();
    with.push(block);
    if !after.is_empty() {
        with.push(structure::with_children(&parent, after));
    }
    let (&pi, gp) = parent_path.split_last().ok_or(EditError::BadPosition)?;
    tr.step(Step { parent: gp.to_vec(), from: pi, to: pi + 1, with })?;
    let mut np = gp.to_vec();
    np.push(pi + at);
    Ok(CaretAt::new(np, 0))
}

/// Ctrl+Enter: a page break (`insertPageBreak`: `[pageBreak, paragraph]` at the caret).
pub fn insert_page_break(state: &mut EditState) -> Result<Change, EditError> {
    insert_blocks(state, vec![Node::of_type("pageBreak")])
}

/// Inserts block nodes at the selection (a table, a block image, a rule, a page break).
pub fn insert_blocks(state: &mut EditState, blocks: Vec<Node>) -> Result<Change, EditError> {
    run(state, |tr, sel, _| {
        let pos = delete_selection(tr, sel)?;
        let at = structure::insert_blocks(tr, pos, blocks)?;
        caret(tr.doc, &at)
    })
}

/// The previous caret position in the text (one character, or a whole atom; across blocks, the end
/// of the previous textblock).
pub fn prev_char_pos(doc: &Node, pos: usize) -> Option<usize> {
    let (path, start) = textblock_at(doc, pos)?;
    if pos > start {
        let block = node_at(doc, &path)?;
        let text = inline::text_units(block.children());
        let mut p = pos - start - 1;
        // Step over a whole surrogate pair.
        if p > 0 && text.get(p).is_some_and(|u| (0xDC00..=0xDFFF).contains(u)) {
            p -= 1;
        }
        return Some(start + p);
    }
    let (pp, ps) = structure::prev_textblock(doc, &path)?;
    let len = inline::content_len(node_at(doc, &pp)?.children());
    Some(ps + len)
}

/// The next caret position in the text (a character with its combining marks, or an atom).
pub fn next_char_pos(doc: &Node, pos: usize) -> Option<usize> {
    let (path, start) = textblock_at(doc, pos)?;
    let block = node_at(doc, &path)?;
    let len = inline::content_len(block.children());
    if pos < start + len {
        let text = inline::text_units(block.children());
        let mut p = pos - start + 1;
        while p < len && text.get(p).is_some_and(|&u| crate::layout::paragraph::splits_a_cluster(u)) {
            p += 1;
        }
        return Some(start + p);
    }
    let (_, ns) = structure::next_textblock(doc, &path)?;
    Some(ns)
}

/// Backspace (`word`: Ctrl+Backspace): the selection, else one character, else the web's
/// start-of-block rules — an indented paragraph loses one indent step; the first block of a list
/// item joins the previous item or, in the first item, leaves the list; an atom block before is
/// deleted; otherwise the block joins the previous textblock.
pub fn delete_backward(state: &mut EditState, word: bool, prev_word: Option<usize>) -> Result<Change, EditError> {
    if !state.sel.is_empty() {
        return run(state, |tr, sel, _| delete_selection(tr, sel).map(Selection::caret));
    }
    let pos = state.sel.head;
    let (path, start) = textblock_at(&state.doc, pos).ok_or(EditError::BadPosition)?;
    if pos > start {
        let target = if word { prev_word.filter(|&p| p >= start && p < pos).unwrap_or(start) } else { prev_char_pos(&state.doc, pos).unwrap_or(start).max(start) };
        return run(state, |tr, _, _| {
            let at = structure::delete_range(tr, target, pos)?;
            caret(tr.doc, &at)
        });
    }
    let block = node_at(&state.doc, &path).ok_or(EditError::NoSuchNode)?.clone();
    // An indented paragraph: remove one indent step (first line first).
    let fl = block.attr("indentFirstLine").and_then(|v| marks::parse_float(&v)).unwrap_or(0.0);
    let il = block.attr("indentLeft").and_then(|v| marks::parse_float(&v)).unwrap_or(0.0);
    if fl > 0.0 || il > 0.0 {
        let (key, cur) = if fl > 0.0 { ("indentFirstLine", fl) } else { ("indentLeft", il) };
        let next = (((cur / INDENT_STEP).ceil() - 1.0) * INDENT_STEP).max(0.0);
        let v = if next > 0.0 { json!(next) } else { Value::Null };
        return set_attrs_at(state, &[path], &[(key, v)]);
    }
    // In a list item.
    if let Some((item_path, ci)) = structure::list_item_of(&state.doc, &path) {
        if ci == 0 {
            let k = *item_path.last().unwrap_or(&0);
            if k == 0 {
                return run(state, |tr, _, _| {
                    let at = structure::lift_list_item(tr, pos)?;
                    caret(tr.doc, &at)
                });
            }
        }
    }
    // An empty non-paragraph block at the very start of the document becomes a paragraph
    // (TipTap's `clearNodes` rule of Backspace).
    let first = textblocks(&state.doc).first().map(|(p, _)| p.clone());
    if inline::content_len(block.children()) == 0 && ty(&block) != "paragraph" && first.as_deref() == Some(path.as_slice()) {
        return set_block_type_at(state, &path, "paragraph", None);
    }
    // An atom block before: delete it.
    if let Some(atom) = structure::atom_before(&state.doc, &path) {
        return run(state, |tr, sel, _| {
            structure::delete_node(tr, &atom)?;
            // The caret's block moved up by the atom's size (1).
            Ok(Selection::caret(sel.head - 1))
        });
    }
    let Some((pp, ps)) = structure::prev_textblock(&state.doc, &path) else { return Err(EditError::NotApplicable) };
    let plen = inline::content_len(node_at(&state.doc, &pp).ok_or(EditError::NoSuchNode)?.children());
    let prev_end = ps + plen;
    run(state, |tr, _, _| {
        let at = structure::delete_range(tr, prev_end, pos)?;
        caret(tr.doc, &at)
    })
}

/// Delete (`word`: Ctrl+Delete): the selection, else one character, else an atom block after, else
/// the next textblock joins this one.
pub fn delete_forward(state: &mut EditState, word: bool, next_word: Option<usize>) -> Result<Change, EditError> {
    if !state.sel.is_empty() {
        return run(state, |tr, sel, _| delete_selection(tr, sel).map(Selection::caret));
    }
    let pos = state.sel.head;
    let (path, start) = textblock_at(&state.doc, pos).ok_or(EditError::BadPosition)?;
    let block = node_at(&state.doc, &path).ok_or(EditError::NoSuchNode)?.clone();
    let len = inline::content_len(block.children());
    if pos < start + len {
        let target = if word { next_word.filter(|&p| p > pos && p <= start + len).unwrap_or(start + len) } else { next_char_pos(&state.doc, pos).unwrap_or(start + len).min(start + len) };
        return run(state, |tr, _, _| {
            let at = structure::delete_range(tr, pos, target)?;
            caret(tr.doc, &at)
        });
    }
    if let Some(atom) = structure::atom_after(&state.doc, &path) {
        return run(state, |tr, sel, _| {
            structure::delete_node(tr, &atom)?;
            Ok(sel)
        });
    }
    let Some((_, ns)) = structure::next_textblock(&state.doc, &path) else { return Err(EditError::NotApplicable) };
    run(state, |tr, _, _| {
        let at = structure::delete_range(tr, pos, ns)?;
        caret(tr.doc, &at)
    })
}

/// The table cell a textblock is in, and the start of every cell's first textblock in its table.
fn table_cells_around(doc: &Node, path: &[usize]) -> Option<(usize, Vec<usize>)> {
    let depth = (0..path.len()).rev().find(|&d| node_at(doc, &path[..d]).is_some_and(|n| ty(n) == "tableCell" || ty(n) == "tableHeader"))?;
    let cell_path = &path[..depth];
    let table_path = &cell_path[..cell_path.len().checked_sub(2)?];
    let table = node_at(doc, table_path)?;
    let tstart = pm::content_start(doc, table_path)?;
    let mut starts = Vec::new();
    let mut cur = 0usize;
    let mut pos = tstart;
    for (ri, row) in table.children().iter().enumerate() {
        pos += 1;
        for (ci, cell) in row.children().iter().enumerate() {
            if ri == cell_path[cell_path.len() - 2] && ci == cell_path[cell_path.len() - 1] {
                cur = starts.len();
            }
            // First textblock of the cell: cell content start + its inner offset.
            let inner = structure::first_textblock_in(cell).map(|(_, s)| s).unwrap_or(0);
            starts.push(pos + 1 + inner);
            pos += pm::node_size(cell);
        }
        pos += 1;
    }
    Some((cur, starts))
}

/// Tab / Shift+Tab (`nav-keys.ts`): next/previous table cell; in a list, nest / un-nest the item;
/// at the start of a paragraph, the first-line indent by 48 px steps; elsewhere a tab character
/// (Shift+Tab removes a tab just before the caret). `Err(NotApplicable)` when nothing happens.
pub fn tab(state: &mut EditState, shift: bool) -> Result<TabOutcome, EditError> {
    let pos = state.sel.from();
    let (path, start) = textblock_at(&state.doc, pos).ok_or(EditError::BadPosition)?;
    if let Some((cur, starts)) = table_cells_around(&state.doc, &path) {
        let target = if shift { cur.checked_sub(1) } else { Some(cur + 1) };
        return match target.and_then(|t| starts.get(t).copied()) {
            Some(p) => {
                state.sel = Selection::caret(p);
                Ok(TabOutcome::Moved)
            }
            None => Ok(TabOutcome::Nothing),
        };
    }
    if structure::list_item_of(&state.doc, &path).is_some() {
        let r = run(state, |tr, sel, _| {
            let at = if shift { structure::lift_list_item(tr, sel.head)? } else { structure::sink_list_item(tr, sel.head)? };
            caret(tr.doc, &at)
        });
        return match r {
            Ok(c) => Ok(TabOutcome::Changed(c)),
            Err(EditError::NotApplicable) => Ok(TabOutcome::Nothing),
            Err(e) => Err(e),
        };
    }
    let block = node_at(&state.doc, &path).ok_or(EditError::NoSuchNode)?.clone();
    let at_start = state.sel.is_empty() && pos == start;
    let cur = block.attr("indentFirstLine").and_then(|v| marks::parse_float(&v)).unwrap_or(0.0);
    if shift {
        if at_start || cur > 0.0 {
            let next = (((cur / INDENT_STEP).ceil() - 1.0) * INDENT_STEP).max(0.0);
            let v = if next > 0.0 { json!(next) } else { Value::Null };
            return set_attrs_at(state, &[path], &[("indentFirstLine", v)]).map(TabOutcome::Changed);
        }
        let off = pos - start;
        if off > 0 && inline::text_of(block.children(), off - 1, off) == "\t" {
            return run(state, |tr, _, _| {
                let at = structure::delete_range(tr, pos - 1, pos)?;
                caret(tr.doc, &at)
            })
            .map(TabOutcome::Changed);
        }
        return Ok(TabOutcome::Nothing);
    }
    if at_start {
        let next = ((cur / INDENT_STEP).floor() + 1.0) * INDENT_STEP;
        return set_attrs_at(state, &[path], &[("indentFirstLine", json!(next))]).map(TabOutcome::Changed);
    }
    insert_text(state, "\t").map(TabOutcome::Changed)
}

/// What Tab did.
#[derive(Debug)]
pub enum TabOutcome {
    Changed(Change),
    /// Only the selection moved (to another cell).
    Moved,
    Nothing,
}

// ── Marks ───────────────────────────────────────────────────────────────────

/// The textblocks the selection touches: `(path, content start, from offset, to offset)`.
pub fn blocks_in(doc: &Node, from: usize, to: usize) -> Vec<(Path, usize, usize, usize)> {
    let mut out = Vec::new();
    for (path, start) in textblocks(doc) {
        let Some(n) = node_at(doc, &path) else { continue };
        let len = inline::content_len(n.children());
        let end = start + len;
        if end < from || start > to {
            continue;
        }
        // A block the range only touches at its edge is included when the range is a caret in it.
        if from != to && (end == from || start == to) && len > 0 {
            continue;
        }
        out.push((path, start, from.max(start) - start, to.min(end) - start));
    }
    out
}

/// Whether `mark_type` is on all the (non-blank) text of the selection, or — for a caret — in the
/// marks typing would use (TipTap `isActive`).
pub fn mark_active(state: &EditState, mark_type: &str) -> bool {
    if state.sel.is_empty() {
        return marks::has_mark(&typing_marks(&state.doc, state.sel.head, state.stored_marks.as_ref()), mark_type);
    }
    let mut any = false;
    for (path, _, a, b) in blocks_in(&state.doc, state.sel.from(), state.sel.to()) {
        let Some(n) = node_at(&state.doc, &path) else { continue };
        for (_, _, m, text) in inline::text_ranges(n.children(), a, b) {
            if text.trim().is_empty() {
                continue;
            }
            any = true;
            if !marks::has_mark(&m, mark_type) {
                return false;
            }
        }
    }
    any
}

/// Maps the marks of the selection's text with `f`; for a caret, the stored marks. Also records the
/// change on every EMPTY textblock of the range as `fontMarks` when `font_marks` is given
/// (`applyInlineFormat` step 2).
pub fn map_selection_marks(state: &mut EditState, f: &dyn Fn(&[Value]) -> Vec<Value>, font_marks: Option<Map<String, Value>>) -> Result<Change, EditError> {
    if state.sel.is_empty() {
        let cur = typing_marks(&state.doc, state.sel.head, state.stored_marks.as_ref());
        state.stored_marks = Some(f(&cur));
        // An empty paragraph remembers the formatting (fontMarks), so its line height follows.
        if let Some(fm) = font_marks {
            let (path, _) = textblock_at(&state.doc, state.sel.head).ok_or(EditError::BadPosition)?;
            let empty = node_at(&state.doc, &path).is_some_and(|n| inline::content_len(n.children()) == 0);
            if empty {
                let stored = state.stored_marks.clone();
                let r = tag_font_marks(state, &[path], &fm);
                state.stored_marks = stored;
                return r;
            }
        }
        return Ok(Change { before: state.sel, after: state.sel, merge: true, ..Default::default() });
    }
    let (from, to) = (state.sel.from(), state.sel.to());
    let fm = font_marks.clone();
    run(state, |tr, sel, _| {
        for (path, _, a, b) in blocks_in(tr.doc, from, to) {
            let n = node_at(tr.doc, &path).ok_or(EditError::NoSuchNode)?.clone();
            if inline::content_len(n.children()) == 0 {
                if let Some(fm) = &fm {
                    let mut attrs = n.attrs();
                    let mut cur = attrs.get("fontMarks").and_then(Value::as_object).cloned().unwrap_or_default();
                    for (k, v) in fm {
                        cur.insert(k.clone(), v.clone());
                    }
                    attrs.insert("fontMarks".into(), Value::Object(cur));
                    let mut nn = n.clone();
                    nn.set_value("attrs", &Value::Object(attrs));
                    tr.replace_node(&path, nn)?;
                }
                continue;
            }
            if a == b {
                continue;
            }
            let kids = inline::map_marks(n.children(), a, b, f)?;
            tr.replace_node(&path, structure::with_children(&n, kids))?;
        }
        Ok(sel)
    })
}

fn tag_font_marks(state: &mut EditState, paths: &[Path], fm: &Map<String, Value>) -> Result<Change, EditError> {
    let paths = paths.to_vec();
    let fm = fm.clone();
    run(state, |tr, sel, _| {
        for path in &paths {
            let n = node_at(tr.doc, path).ok_or(EditError::NoSuchNode)?.clone();
            let mut attrs = n.attrs();
            let mut cur = attrs.get("fontMarks").and_then(Value::as_object).cloned().unwrap_or_default();
            for (k, v) in &fm {
                cur.insert(k.clone(), v.clone());
            }
            attrs.insert("fontMarks".into(), Value::Object(cur));
            let mut nn = n.clone();
            nn.set_value("attrs", &Value::Object(attrs));
            tr.replace_node(path, nn)?;
        }
        Ok(sel)
    })
}

/// The `fontMarks` key of a boolean mark (`applyInlineFormat`).
fn fm_key(mark_type: &str) -> Option<&'static str> {
    match mark_type {
        "bold" => Some("b"),
        "italic" => Some("i"),
        "underline" => Some("u"),
        "strike" => Some("s"),
        _ => None,
    }
}

/// Bold, italic, underline, strike, sub/superscript, code: on when not on the whole selection.
pub fn toggle_mark(state: &mut EditState, mark_type: &str) -> Result<Change, EditError> {
    let on = !mark_active(state, mark_type);
    let fm = fm_key(mark_type).map(|k| {
        let mut m = Map::new();
        m.insert(k.into(), Value::Bool(on));
        m
    });
    let mt = mark_type.to_string();
    let f = move |m: &[Value]| if on { marks::add_mark(m, marks::simple(&mt)) } else { marks::remove_mark(m, &mt) };
    map_selection_marks(state, &f, fm)
}

/// `setMark('textStyle', {key: value})` (font family, size, colour…), merged with each run's
/// existing text style; `None` clears the attribute.
pub fn set_text_style(state: &mut EditState, key: &str, value: Option<Value>) -> Result<Change, EditError> {
    let fm = match key {
        "fontFamily" => value.clone().map(|v| ("ff", v)),
        "fontSize" => value.clone().map(|v| ("fs", v)),
        _ => None,
    }
    .map(|(k, v)| {
        let mut m = Map::new();
        m.insert(k.into(), v);
        m
    });
    let k = key.to_string();
    let f = move |m: &[Value]| marks::set_text_style(m, &k, value.clone());
    map_selection_marks(state, &f, fm)
}

/// Highlight (`setHighlight({color})` / `unsetHighlight`).
pub fn set_highlight(state: &mut EditState, color: Option<&str>) -> Result<Change, EditError> {
    let c = color.map(str::to_string);
    let f = move |m: &[Value]| match &c {
        Some(c) => marks::add_mark(m, json!({ "type": "highlight", "attrs": { "color": c } })),
        None => marks::remove_mark(m, "highlight"),
    };
    map_selection_marks(state, &f, None)
}

/// A link on the selection (`setLink({href})`), or removed (`unsetLink`).
pub fn set_link(state: &mut EditState, href: Option<&str>) -> Result<Change, EditError> {
    let h = href.map(str::to_string);
    let f = move |m: &[Value]| match &h {
        Some(h) => marks::add_mark(m, json!({ "type": "link", "attrs": { "href": h, "target": "_blank", "rel": "noopener noreferrer nofollow", "class": Value::Null } })),
        None => marks::remove_mark(m, "link"),
    };
    map_selection_marks(state, &f, None)
}

/// The uniform `textStyle` attribute of the selection (`uniformTextStyle`): `Some(value)` when
/// every run agrees (the default when unset), `None` when mixed.
pub fn uniform_text_style(state: &EditState, key: &str, default: &str) -> Option<String> {
    let norm = |v: Option<Value>| -> String {
        match v {
            Some(Value::String(s)) if !s.is_empty() => s,
            Some(Value::Number(n)) => n.to_string(),
            _ => default.to_string(),
        }
    };
    if state.sel.is_empty() {
        let m = typing_marks(&state.doc, state.sel.head, state.stored_marks.as_ref());
        return Some(norm(marks::text_style_attr(&m, key)));
    }
    let mut val: Option<String> = None;
    for (path, _, a, b) in blocks_in(&state.doc, state.sel.from(), state.sel.to()) {
        let Some(n) = node_at(&state.doc, &path) else { continue };
        for (_, _, m, _) in inline::text_ranges(n.children(), a, b) {
            let cur = norm(marks::text_style_attr(&m, key));
            match &val {
                None => val = Some(cur),
                Some(v) if *v != cur => return None,
                _ => {}
            }
        }
    }
    Some(val.unwrap_or_else(|| default.to_string()))
}

/// Grow / shrink: the uniform size (11 when mixed) ± 1 pt, clamped to 6–96 (`setSize`).
pub fn grow_font(state: &mut EditState, delta: f32) -> Result<Change, EditError> {
    let cur = uniform_text_style(state, "fontSize", "11").and_then(|s| marks::parse_float(&Value::String(s))).map(|v| v.round()).unwrap_or(11.0);
    let n = (cur + delta).clamp(MIN_FONT_PT, MAX_FONT_PT);
    set_text_style(state, "fontSize", Some(Value::String(format!("{n}pt"))))
}

/// « Effacer la mise en forme »: `clearNodes()` (headings become paragraphs, list items leave
/// their lists) + `unsetAllMarks()`.
pub fn clear_formatting(state: &mut EditState) -> Result<Change, EditError> {
    let (from, to) = (state.sel.from(), state.sel.to());
    run(state, |tr, sel, _| {
        let mut sel = sel;
        // unsetAllMarks.
        if from != to {
            for (path, _, a, b) in blocks_in(tr.doc, from, to) {
                let n = node_at(tr.doc, &path).ok_or(EditError::NoSuchNode)?.clone();
                if a < b {
                    let kids = inline::map_marks(n.children(), a, b, &|_| Vec::new())?;
                    tr.replace_node(&path, structure::with_children(&n, kids))?;
                }
            }
        }
        // clearNodes: headings → paragraphs (sizes stay: positions do not move).
        for (path, _, _, _) in blocks_in(tr.doc, from, to) {
            let n = node_at(tr.doc, &path).ok_or(EditError::NoSuchNode)?.clone();
            if ty(&n) == "heading" {
                let mut attrs = n.attrs();
                attrs.remove("level");
                attrs.remove("collapsed");
                let mut p = Node::element("paragraph", (!attrs.is_empty()).then_some(Value::Object(attrs)), n.children().to_vec());
                if !n.has("content") {
                    p.remove("content");
                }
                tr.replace_node(&path, p)?;
            }
        }
        // … and list items leave their lists (last first).
        let starts: Vec<usize> = blocks_in(tr.doc, from, to)
            .into_iter()
            .filter(|(p, ..)| structure::list_item_of(tr.doc, p).is_some_and(|(_, ci)| ci == 0))
            .map(|(_, s, ..)| s)
            .collect();
        if !starts.is_empty() {
            let size_before = pm::content_size(tr.doc);
            let (ha, hb) = (size_before - sel.anchor, size_before - sel.head);
            for s in starts.iter().rev() {
                structure::lift_list_item(tr, *s)?;
            }
            let size = pm::content_size(tr.doc);
            // Positions after the lifted items keep their distance to the end; the selection's
            // ends are inside or after them.
            sel = Selection::new(size.saturating_sub(ha), size.saturating_sub(hb));
        }
        Ok(sel)
    })
}

/// Changes the case of the selection's text (`applyCaseTransform`).
pub fn change_case(state: &mut EditState, mode: &str) -> Result<Change, EditError> {
    let (from, to) = (state.sel.from(), state.sel.to());
    if from == to {
        return Err(EditError::NotApplicable);
    }
    let mode = mode.to_string();
    run(state, |tr, sel, _| {
        for (path, _, a, b) in blocks_in(tr.doc, from, to) {
            let n = node_at(tr.doc, &path).ok_or(EditError::NoSuchNode)?.clone();
            let c = inline::cut(n.children(), a, b)?;
            let mut changed = Vec::new();
            let mut sentence_start = true;
            for node in c.removed {
                if pm::is_text(&node) {
                    let t = node.text().unwrap_or_default();
                    let nt = transform_case(&t, &mode, &mut sentence_start);
                    // Keep the length: positions must not drift (the web's assumption too).
                    if pm::utf16_len(&nt) == pm::utf16_len(&t) {
                        changed.push(inline::with_text(&node, &nt));
                        continue;
                    }
                }
                changed.push(node);
            }
            let kids = inline::weld(c.prefix, changed, c.suffix);
            tr.replace_node(&path, structure::with_children(&n, kids))?;
        }
        Ok(sel)
    })
}

/// `transformCaseText`.
pub fn transform_case(s: &str, mode: &str, sentence_start: &mut bool) -> String {
    match mode {
        "upper" => s.to_uppercase(),
        "lower" => s.to_lowercase(),
        "toggle" => s.chars().map(|c| if c.is_lowercase() { c.to_uppercase().collect::<String>() } else { c.to_lowercase().collect() }).collect(),
        "title" => {
            let mut out = String::new();
            let mut in_word = false;
            for c in s.chars() {
                if c.is_alphabetic() || c == '\'' || c == '’' {
                    if !in_word && c.is_alphabetic() {
                        out.extend(c.to_uppercase());
                        in_word = true;
                    } else {
                        out.extend(c.to_lowercase());
                    }
                } else {
                    in_word = false;
                    out.push(c);
                }
            }
            out
        }
        "sentence" => {
            let mut out = String::new();
            let mut after_stop = false;
            for c in s.to_lowercase().chars() {
                if c.is_alphabetic() && (*sentence_start || after_stop) {
                    out.extend(c.to_uppercase());
                    *sentence_start = false;
                    after_stop = false;
                    continue;
                }
                if matches!(c, '.' | '!' | '?' | '…') {
                    after_stop = true;
                } else if !c.is_whitespace() && after_stop && !c.is_alphabetic() {
                    after_stop = false;
                }
                if c.is_alphabetic() {
                    *sentence_start = false;
                }
                out.push(c);
            }
            out
        }
        _ => s.to_string(),
    }
}

// ── Block attributes ────────────────────────────────────────────────────────

/// Sets attributes on the textblocks at `paths` (`null` removes the value, as TipTap writes it).
pub fn set_attrs_at(state: &mut EditState, paths: &[Path], patch: &[(&str, Value)]) -> Result<Change, EditError> {
    let paths = paths.to_vec();
    let patch: Vec<(String, Value)> = patch.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    run(state, |tr, sel, _| {
        for path in &paths {
            let n = node_at(tr.doc, path).ok_or(EditError::NoSuchNode)?.clone();
            let mut attrs = n.attrs();
            let mut changed = false;
            for (k, v) in &patch {
                if attrs.get(k) != Some(v) {
                    attrs.insert(k.clone(), v.clone());
                    changed = true;
                }
            }
            if !changed {
                continue;
            }
            let mut nn = n.clone();
            nn.set_value("attrs", &Value::Object(attrs));
            tr.replace_node(path, nn)?;
        }
        if !tr.changed() {
            return Err(EditError::NotApplicable);
        }
        Ok(sel)
    })
}

/// The paths of the paragraphs and headings the selection touches.
pub fn selected_paragraph_paths(state: &EditState) -> Vec<Path> {
    blocks_in(&state.doc, state.sel.from(), state.sel.to())
        .into_iter()
        .filter(|(p, ..)| node_at(&state.doc, p).is_some_and(|n| matches!(ty(n), "paragraph" | "heading")))
        .map(|(p, ..)| p)
        .collect()
}

/// `updateAttributes('paragraph', …).updateAttributes('heading', …)` over the selection.
pub fn set_paragraph_attrs(state: &mut EditState, patch: &[(&str, Value)]) -> Result<Change, EditError> {
    let paths = selected_paragraph_paths(state);
    set_attrs_at(state, &paths, patch)
}

/// Alignment (`setTextAlign`).
pub fn set_align(state: &mut EditState, align: &str) -> Result<Change, EditError> {
    set_paragraph_attrs(state, &[("textAlign", json!(align))])
}

/// The ribbon's indent buttons: the paragraph `indent` level ± 1, clamped to 0–10 (each level is
/// 32 px).
pub fn indent(state: &mut EditState, delta: i32) -> Result<Change, EditError> {
    let paths = selected_paragraph_paths(state);
    let cur = paths
        .first()
        .and_then(|p| node_at(&state.doc, p))
        .and_then(|n| n.attr("indent"))
        .and_then(|v| marks::parse_float(&v))
        .unwrap_or(0.0) as i32;
    let next = (cur + delta).clamp(0, 10);
    set_attrs_at(state, &paths, &[("indent", json!(next))])
}

/// Turns the textblock at `path` into `new_type` (`setNode`), keeping its content and attributes
/// (minus `level` when it stops being a heading).
pub fn set_block_type_at(state: &mut EditState, path: &[usize], new_type: &str, level: Option<u32>) -> Result<Change, EditError> {
    let paths = vec![path.to_vec()];
    set_block_types(state, &paths, new_type, level, &[])
}

fn set_block_types(state: &mut EditState, paths: &[Path], new_type: &str, level: Option<u32>, extra: &[(&str, Value)]) -> Result<Change, EditError> {
    let paths = paths.to_vec();
    let extra: Vec<(String, Value)> = extra.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    let new_type = new_type.to_string();
    run(state, |tr, sel, _| {
        for path in &paths {
            let n = node_at(tr.doc, path).ok_or(EditError::NoSuchNode)?.clone();
            let mut attrs = n.attrs();
            if new_type == "heading" {
                attrs.insert("level".into(), json!(level.unwrap_or(1)));
            } else {
                attrs.remove("level");
                attrs.remove("collapsed");
            }
            for (k, v) in &extra {
                attrs.insert(k.clone(), v.clone());
            }
            let mut nn = n.clone();
            nn.set_value("type", &Value::String(new_type.clone()));
            if attrs.is_empty() {
                if nn.has("attrs") {
                    nn.remove("attrs");
                }
            } else {
                nn.set_value("attrs", &Value::Object(attrs));
            }
            tr.replace_node(path, nn)?;
        }
        Ok(sel)
    })
}

/// A named style of the gallery (`NamedStyle`, `DEFAULT_STYLES`).
#[derive(Clone, Debug, Default)]
pub struct NamedStyle {
    pub id: &'static str,
    pub heading: Option<u32>,
    pub font: Option<&'static str>,
    pub size: Option<f32>,
    pub bold: bool,
    pub italic: bool,
    pub color: Option<&'static str>,
    pub align: Option<&'static str>,
    pub line_height: Option<f32>,
    pub space_before: Option<f32>,
    pub space_after: Option<f32>,
}

/// The web's built-in styles (`DEFAULT_STYLES`, `DocumentEditorPage.tsx:11024-11034`).
pub fn default_styles() -> Vec<NamedStyle> {
    let s = |id| NamedStyle { id, font: Some("Arial"), ..Default::default() };
    vec![
        NamedStyle { size: Some(11.0), line_height: Some(1.15), ..s("normal") },
        NamedStyle { size: Some(11.0), line_height: Some(1.0), space_before: Some(0.0), space_after: Some(0.0), ..s("noSpacing") },
        NamedStyle { size: Some(28.0), color: Some("#202124"), space_before: Some(4.0), space_after: Some(6.0), ..s("title") },
        NamedStyle { size: Some(15.0), italic: true, color: Some("#5f6368"), space_after: Some(12.0), ..s("subtitle") },
        NamedStyle { heading: Some(1), size: Some(24.0), bold: true, color: Some("#202124"), ..s("heading1") },
        NamedStyle { heading: Some(2), size: Some(18.0), bold: true, color: Some("#202124"), ..s("heading2") },
        NamedStyle { heading: Some(3), size: Some(14.0), bold: true, color: Some("#434649"), ..s("heading3") },
        NamedStyle { heading: Some(4), size: Some(13.0), bold: true, color: Some("#434649"), ..s("heading4") },
        NamedStyle { font: Some("Georgia"), size: Some(11.0), italic: true, color: Some("#5f6368"), align: Some("left"), space_before: Some(8.0), space_after: Some(8.0), ..s("quote") },
    ]
}

/// `applyNamedStyle`: the block type and spacing, the alignment, then the marks from a clean base.
pub fn apply_named_style(state: &mut EditState, s: &NamedStyle) -> Result<Change, EditError> {
    // The selection is widened to whole blocks (`$from.start()`..`$to.end()`).
    let blocks = blocks_in(&state.doc, state.sel.from(), state.sel.to());
    let (Some(first), Some(last)) = (blocks.first(), blocks.last()) else { return Err(EditError::BadPosition) };
    let from = first.1;
    let last_len = node_at(&state.doc, &last.0).map(|n| inline::content_len(n.children())).unwrap_or(0);
    let to = last.1 + last_len;
    let original = state.sel;
    let paths: Vec<Path> = blocks.iter().map(|b| b.0.clone()).collect();
    let null_or = |v: Option<f32>| v.map(|f| json!(f)).unwrap_or(Value::Null);
    let extra = [
        ("styleName", json!(s.id)),
        ("lineHeight", null_or(s.line_height)),
        ("spaceBefore", null_or(s.space_before)),
        ("spaceAfter", null_or(s.space_after)),
        ("textAlign", json!(s.align.unwrap_or("left"))),
    ];
    let mut all = set_block_types(state, &paths, if s.heading.is_some() { "heading" } else { "paragraph" }, s.heading, &extra)?;
    state.sel = Selection::new(from, to);
    let mut ts = Map::new();
    if let Some(f) = s.font {
        ts.insert("fontFamily".into(), json!(f));
    }
    if let Some(sz) = s.size {
        ts.insert("fontSize".into(), json!(format!("{sz}pt")));
    }
    if let Some(c) = s.color {
        ts.insert("color".into(), json!(c));
    }
    let (bold, italic) = (s.bold, s.italic);
    let f = move |_: &[Value]| {
        let mut m = Vec::new();
        if !ts.is_empty() {
            m.push(json!({ "type": "textStyle", "attrs": Value::Object(ts.clone()) }));
        }
        if bold {
            m.push(marks::simple("bold"));
        }
        if italic {
            m.push(marks::simple("italic"));
        }
        m
    };
    if from < to {
        let c = map_selection_marks(state, &f, None)?;
        all.steps.extend(c.steps);
        all.inverses.extend(c.inverses);
    }
    state.sel = original;
    all.after = original;
    all.before = original;
    Ok(all)
}

/// Turns the selected paragraphs and headings into `new_type` (`setNode`).
pub fn set_block_type(state: &mut EditState, new_type: &str, level: Option<u32>) -> Result<Change, EditError> {
    let paths: Vec<Path> = blocks_in(&state.doc, state.sel.from(), state.sel.to())
        .into_iter()
        .filter(|(p, ..)| node_at(&state.doc, p).is_some_and(|n| matches!(ty(n), "paragraph" | "heading" | "codeBlock")))
        .map(|(p, ..)| p)
        .collect();
    set_block_types(state, &paths, new_type, level, &[])
}

/// The formatting the format painter copies (`captureFormat`, `format-painter.ts`): the character
/// marks at the start of the selection (the typing marks for a caret), and the paragraph look.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CapturedFormat {
    pub marks: Vec<Value>,
    pub para: Map<String, Value>,
}

/// The marks the painter copies and clears (`FORMAT_MARKS`).
pub const FORMAT_MARKS: &[&str] = &["bold", "italic", "underline", "strike", "subscript", "superscript", "textStyle", "highlight"];
/// The paragraph attributes it copies (`PARA_ATTRS`).
pub const PARA_ATTRS: &[&str] = &["textAlign", "lineHeight", "spaceBefore", "spaceAfter", "indent", "indentLeft", "indentFirstLine", "indentRight"];

pub fn capture_format(state: &EditState) -> CapturedFormat {
    let at = if state.sel.is_empty() { state.sel.head } else { (state.sel.from() + 1).min(state.sel.to()) };
    let marks = if state.sel.is_empty() {
        typing_marks(&state.doc, at, state.stored_marks.as_ref())
    } else {
        textblock_at(&state.doc, at).and_then(|(p, s)| node_at(&state.doc, &p).map(|n| inline::marks_at(n.children(), at - s))).unwrap_or_default()
    };
    let marks = marks.into_iter().filter(|m| m.get("type").and_then(Value::as_str).is_some_and(|t| FORMAT_MARKS.contains(&t))).collect();
    let mut para = Map::new();
    if let Some((p, _)) = textblock_at(&state.doc, state.sel.from()) {
        if let Some(n) = node_at(&state.doc, &p) {
            let a = n.attrs();
            for k in PARA_ATTRS {
                if let Some(v) = a.get(*k).filter(|v| !v.is_null()) {
                    para.insert((*k).to_string(), v.clone());
                }
            }
        }
    }
    CapturedFormat { marks, para }
}

/// `applyFormat`: the captured look onto the selection (a caret: the word under it). Formatting
/// marks are replaced, links/comments/tracked changes kept; the paragraph look goes on every
/// textblock touched. One change.
pub fn apply_format(state: &mut EditState, cap: &CapturedFormat, word: Option<(usize, usize)>) -> Result<Change, EditError> {
    let (a, b) = if state.sel.is_empty() { word.filter(|w| w.0 < w.1).ok_or(EditError::NotApplicable)? } else { (state.sel.from(), state.sel.to()) };
    let marks = cap.marks.clone();
    let para = cap.para.clone();
    run(state, |tr, sel, _| {
        for (path, _, x, y) in blocks_in(tr.doc, a, b) {
            let n = node_at(tr.doc, &path).ok_or(EditError::NoSuchNode)?.clone();
            let mut nn = n.clone();
            if x < y {
                let kids = inline::map_marks(n.children(), x, y, &|m| {
                    let mut kept: Vec<Value> = m.iter().filter(|v| !v.get("type").and_then(Value::as_str).is_some_and(|t| FORMAT_MARKS.contains(&t))).cloned().collect();
                    for add in &marks {
                        kept = marks::add_mark(&kept, add.clone());
                    }
                    kept
                })?;
                nn.set_children(kids);
            }
            if !para.is_empty() {
                let mut attrs = nn.attrs();
                for (k, v) in &para {
                    attrs.insert(k.clone(), v.clone());
                }
                nn.set_value("attrs", &Value::Object(attrs));
            }
            tr.replace_node(&path, nn)?;
        }
        Ok(sel)
    })
}

/// The number of words of the body (`CharacterCount.words()`: runs of non-space characters).
pub fn word_count(doc: &Node) -> usize {
    let mut n = 0;
    for (p, _) in textblocks(doc) {
        if let Some(b) = node_at(doc, &p) {
            let text = inline::text_of(b.children(), 0, inline::content_len(b.children()));
            n += text.split_whitespace().count();
        }
    }
    n
}

/// Bullets / numbering / task list (TipTap `toggleList`).
pub fn toggle_list(state: &mut EditState, list_type: &str) -> Result<Change, EditError> {
    let (from, to) = (state.sel.from(), state.sel.to());
    let head = state.sel.head;
    let anchor_off = state.sel.anchor as isize - head as isize;
    let lt = list_type.to_string();
    run(state, |tr, _, _| {
        let at = structure::toggle_list(tr, from, to, &lt, head)?;
        let h = at.pos(tr.doc).ok_or(EditError::BadPosition)?;
        let a = (h as isize + anchor_off).max(0) as usize;
        // A range selection keeps its extent only when it stayed in the same block; otherwise a caret.
        if anchor_off != 0 && textblock_at(tr.doc, a).map(|t| t.0) == textblock_at(tr.doc, h).map(|t| t.0) {
            Ok(Selection::new(a, h))
        } else {
            Ok(Selection::caret(h))
        }
    })
}

/// Sets the current paragraph's indents (the ruler): px, rounded, `null` when 0
/// (`setParaIndentAttrs`).
pub fn set_indents(state: &mut EditState, left: f32, first: f32, right: f32) -> Result<Change, EditError> {
    let v = |x: f32| if x.round() != 0.0 { json!(x.round() as i64) } else { Value::Null };
    set_paragraph_attrs(state, &[("indentLeft", v(left)), ("indentFirstLine", v(first)), ("indentRight", v(right))])
}

/// Sets the current paragraph's tab stops (the ruler): `[{pos, type}]` sorted, `null` when none
/// (`setParaTabStops`).
pub fn set_tab_stops(state: &mut EditState, stops: &[(f32, &str)]) -> Result<Change, EditError> {
    let mut sorted: Vec<(f32, &str)> = stops.to_vec();
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let v = if sorted.is_empty() {
        Value::Null
    } else {
        Value::Array(sorted.iter().map(|(p, t)| json!({ "pos": p, "type": t })).collect())
    };
    set_paragraph_attrs(state, &[("tabStops", v)])
}

// ── Paste ───────────────────────────────────────────────────────────────────

/// Pastes plain text: the first line into the current block, every other line its own paragraph
/// (ProseMirror's plain-text paste), all with the marks typing would use.
pub fn paste_text(state: &mut EditState, text: &str) -> Result<Change, EditError> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let marks = typing_marks(&state.doc, state.sel.from(), state.stored_marks.as_ref());
    run(state, |tr, sel, _| {
        let mut pos = delete_selection(tr, sel)?;
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                let at = structure::split_block(tr, pos)?;
                pos = at.pos(tr.doc).ok_or(EditError::BadPosition)?;
            }
            if !line.is_empty() {
                let (path, start) = textblock_at(tr.doc, pos).ok_or(EditError::BadPosition)?;
                let block = node_at(tr.doc, &path).ok_or(EditError::NoSuchNode)?.clone();
                let kids = inline::insert_text(block.children(), pos - start, line, &marks)?;
                tr.replace_node(&path, structure::with_children(&block, kids))?;
                pos += pm::utf16_len(line);
            }
        }
        Ok(Selection::caret(pos))
    })
}

/// Pastes a slice of blocks (from our own clipboard format or parsed HTML): a single textblock's
/// content goes inline; otherwise the first block's content joins the text before the caret, the
/// last block's joins the text after it, and the blocks between are inserted whole (an open slice,
/// as ProseMirror fits one).
pub fn paste_blocks(state: &mut EditState, blocks: Vec<Node>) -> Result<Change, EditError> {
    if blocks.is_empty() {
        return Err(EditError::NotApplicable);
    }
    run(state, |tr, sel, _| {
        let pos = delete_selection(tr, sel)?;
        let (path, start) = textblock_at(tr.doc, pos).ok_or(EditError::BadPosition)?;
        let block = node_at(tr.doc, &path).ok_or(EditError::NoSuchNode)?.clone();
        let off = pos - start;
        if blocks.len() == 1 && pm::is_textblock(&blocks[0]) {
            let inserted = blocks[0].children().to_vec();
            let len: usize = inserted.iter().map(pm::node_size).sum();
            let kids = inline::replace(block.children(), off, off, inserted)?;
            tr.replace_node(&path, structure::with_children(&block, kids))?;
            return Ok(Selection::caret(pos + len));
        }
        let c = inline::cut(block.children(), off, off)?;
        let n = blocks.len();
        let mut with: Vec<Node> = Vec::new();
        let first_tb = pm::is_textblock(&blocks[0]);
        let last_tb = n > 1 && pm::is_textblock(&blocks[n - 1]);
        // Left: the text before the caret, joined with the first block's content when it is text.
        if first_tb {
            let kids = inline::weld(c.prefix.clone(), blocks[0].children().to_vec(), vec![]);
            with.push(structure::with_children(&block, kids));
        } else {
            let mut l = block.clone();
            if c.prefix.is_empty() {
                l.remove("content");
            } else {
                l.set_children(c.prefix.clone());
            }
            with.push(l);
            with.push(blocks[0].clone());
        }
        let middle_end = if last_tb { n - 1 } else { n };
        for b in blocks.iter().take(middle_end).skip(1) {
            with.push(b.clone());
        }
        // Right: the last block's content, then the text after the caret.
        let (right, caret_off) = if last_tb {
            let lb = &blocks[n - 1];
            let lead = inline::content_len(lb.children());
            let kids = inline::weld(lb.children().to_vec(), vec![], c.suffix.clone());
            (structure::with_children(lb, kids), lead)
        } else {
            let mut r = block.clone();
            if c.suffix.is_empty() {
                r.remove("content");
            } else {
                r.set_children(c.suffix.clone());
            }
            (r, 0)
        };
        with.push(right);
        let count = with.len();
        let (&i, parent) = path.split_last().ok_or(EditError::BadPosition)?;
        tr.step(Step { parent: parent.to_vec(), from: i, to: i + 1, with })?;
        let mut np = parent.to_vec();
        np.push(i + count - 1);
        caret(tr.doc, &CaretAt::new(np, caret_off))
    })
}

/// Replaces `from..to` (inside one textblock) with `text`, keeping the marks of the replaced text
/// (Find & Replace).
pub fn replace_text(state: &mut EditState, from: usize, to: usize, text: &str) -> Result<Change, EditError> {
    let marks = textblock_at(&state.doc, from)
        .and_then(|(p, s)| node_at(&state.doc, &p).map(|n| inline::marks_at(n.children(), (from - s + 1).min(inline::content_len(n.children())))))
        .unwrap_or_default();
    run(state, |tr, _, _| {
        let (path, start) = textblock_at(tr.doc, from).ok_or(EditError::BadPosition)?;
        let block = node_at(tr.doc, &path).ok_or(EditError::NoSuchNode)?.clone();
        let node = Node::text_node(text, marks::to_raw(&marks).as_deref());
        let with = if text.is_empty() { vec![] } else { vec![node] };
        let kids = inline::replace(block.children(), from - start, to - start, with)?;
        tr.replace_node(&path, structure::with_children(&block, kids))?;
        Ok(Selection::new(from, from + pm::utf16_len(text)))
    })
}

/// Replaces every match of `query` with `with`, in one change (last first, so the earlier matches
/// keep their positions). Returns the change and how many were replaced.
pub fn replace_all(state: &mut EditState, query: &str, with: &str, opts: super::find::FindOptions) -> Result<(Change, usize), EditError> {
    let matches = super::find::find_all(&state.doc, query, opts);
    if matches.is_empty() {
        return Err(EditError::NotApplicable);
    }
    let n = matches.len();
    let with = with.to_string();
    let change = run(state, |tr, sel, _| {
        for &(from, to) in matches.iter().rev() {
            let (path, start) = textblock_at(tr.doc, from).ok_or(EditError::BadPosition)?;
            let block = node_at(tr.doc, &path).ok_or(EditError::NoSuchNode)?.clone();
            let m = inline::marks_at(block.children(), (from - start + 1).min(inline::content_len(block.children())));
            let node = Node::text_node(&with, marks::to_raw(&m).as_deref());
            let ins = if with.is_empty() { vec![] } else { vec![node] };
            let kids = inline::replace(block.children(), from - start, to - start, ins)?;
            tr.replace_node(&path, structure::with_children(&block, kids))?;
        }
        Ok(Selection::caret(sel.head.min(pm::content_size(tr.doc))))
    })?;
    Ok((change, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(json: &str) -> EditState {
        EditState { doc: Node::from_slice(json.as_bytes()).expect("parses"), sel: Selection::caret(1), stored_marks: None }
    }

    fn text(s: &EditState) -> String {
        super::super::clipboard::plain_text(&s.doc, 0, pm::content_size(&s.doc))
    }

    #[test]
    fn replace_all_is_one_change_keeping_the_marks() {
        let mut s = state(r#"{"content":[{"content":[{"marks":[{"type":"bold"}],"text":"chat noir, chat blanc","type":"text"}],"type":"paragraph"}],"type":"doc"}"#);
        let (c, n) = replace_all(&mut s, "chat", "chien", Default::default()).expect("replaces");
        assert_eq!(n, 2);
        assert_eq!(text(&s), "chien noir, chien blanc");
        assert_eq!(c.steps.len(), 2);
        assert!(crate::marks::text_mark_of(&s.doc.children()[0].children()[0]).bold);
    }

    #[test]
    fn a_tab_at_the_start_indents_the_first_line_and_elsewhere_inserts_a_tab() {
        let mut s = state(r#"{"content":[{"content":[{"text":"ab","type":"text"}],"type":"paragraph"}],"type":"doc"}"#);
        assert!(matches!(tab(&mut s, false), Ok(TabOutcome::Changed(_))));
        assert_eq!(s.doc.children()[0].attr("indentFirstLine"), Some(serde_json::json!(48.0)));
        s.sel = Selection::caret(2);
        tab(&mut s, false).expect("tabs");
        assert_eq!(text(&s), "a\tb");
        // Backspace at the start of the indented paragraph removes the indent first.
        s.sel = Selection::caret(1);
        delete_backward(&mut s, false, None).expect("unindents");
        assert!(s.doc.children()[0].attr("indentFirstLine").is_none());
    }

    #[test]
    fn backspace_at_the_start_of_the_first_list_item_leaves_the_list() {
        let mut s = state(r#"{"content":[{"content":[{"content":[{"content":[{"text":"a","type":"text"}],"type":"paragraph"}],"type":"listItem"}],"type":"bulletList"}],"type":"doc"}"#);
        s.sel = Selection::caret(3);
        delete_backward(&mut s, false, None).expect("lifts");
        assert_eq!(s.doc.children()[0].node_type(), Some("paragraph"));
    }

    #[test]
    fn backspace_after_an_image_deletes_it() {
        let mut s = state(r#"{"content":[{"attrs":{"src":"x"},"type":"image"},{"content":[{"text":"a","type":"text"}],"type":"paragraph"}],"type":"doc"}"#);
        s.sel = Selection::caret(2);
        delete_backward(&mut s, false, None).expect("deletes");
        assert_eq!(s.doc.children().len(), 1);
        assert_eq!(s.sel, Selection::caret(1));
    }

    #[test]
    fn pasting_plain_text_makes_paragraphs() {
        let mut s = state(r#"{"content":[{"content":[{"text":"ab","type":"text"}],"type":"paragraph"}],"type":"doc"}"#);
        s.sel = Selection::caret(2);
        paste_text(&mut s, "x\r\ny\nz").expect("pastes");
        assert_eq!(text(&s), "ax\ny\nzb");
    }

    #[test]
    fn pasting_blocks_opens_the_slice_at_both_ends() {
        let mut s = state(r#"{"content":[{"content":[{"text":"ab","type":"text"}],"type":"paragraph"}],"type":"doc"}"#);
        s.sel = Selection::caret(2);
        let blocks = super::super::html::html_to_blocks("<p>1</p><h2>2</h2><p>3</p>");
        paste_blocks(&mut s, blocks).expect("pastes");
        assert_eq!(text(&s), "a1\n2\n3b");
        assert_eq!(s.doc.children()[1].node_type(), Some("heading"));
    }

    #[test]
    fn clear_formatting_removes_marks_and_turns_headings_into_paragraphs() {
        let mut s = state(r#"{"content":[{"attrs":{"level":1},"content":[{"marks":[{"type":"bold"}],"text":"T","type":"text"}],"type":"heading"}],"type":"doc"}"#);
        s.sel = Selection::new(1, 2);
        clear_formatting(&mut s).expect("clears");
        assert_eq!(s.doc.children()[0].node_type(), Some("paragraph"));
        assert!(!s.doc.children()[0].children()[0].has("marks"));
    }

    #[test]
    fn the_format_painter_copies_marks_and_paragraph_look() {
        let mut s = state(r#"{"content":[{"attrs":{"textAlign":"center"},"content":[{"marks":[{"type":"italic"}],"text":"src","type":"text"}],"type":"paragraph"},{"content":[{"text":"dst","type":"text"}],"type":"paragraph"}],"type":"doc"}"#);
        s.sel = Selection::new(1, 4);
        let cap = capture_format(&s);
        s.sel = Selection::new(6, 9);
        apply_format(&mut s, &cap, None).expect("paints");
        assert!(crate::marks::text_mark_of(&s.doc.children()[1].children()[0]).italic);
        assert_eq!(s.doc.children()[1].attr("textAlign"), Some(serde_json::json!("center")));
    }

    #[test]
    fn a_case_change_keeps_positions() {
        let mut s = state(r#"{"content":[{"content":[{"text":"bonjour le monde","type":"text"}],"type":"paragraph"}],"type":"doc"}"#);
        s.sel = Selection::new(1, 17);
        change_case(&mut s, "title").expect("changes");
        assert_eq!(text(&s), "Bonjour Le Monde");
        change_case(&mut s, "upper").expect("changes");
        assert_eq!(text(&s), "BONJOUR LE MONDE");
    }

    #[test]
    fn grow_font_steps_from_the_uniform_size() {
        let mut s = state(r#"{"content":[{"content":[{"text":"ab","type":"text"}],"type":"paragraph"}],"type":"doc"}"#);
        s.sel = Selection::new(1, 3);
        grow_font(&mut s, 1.0).expect("grows");
        assert_eq!(uniform_text_style(&s, "fontSize", "11").as_deref(), Some("12pt"));
    }
}
