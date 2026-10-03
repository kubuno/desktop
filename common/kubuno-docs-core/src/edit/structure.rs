//! Structural edits: deleting across blocks, splitting a block, lists.
//!
//! These follow ProseMirror's own algorithms as TipTap's commands drive them:
//!
//! * **deleting a range that spans blocks** (`replace` with an empty slice → `replaceTwoWay`): the
//!   block where the range ends is **joined** into the block where it starts (the first keeps its
//!   type and attributes), and so is every pair of ancestors of the same type below the common
//!   ancestor (two list items, two lists, two block quotes); where the two sides' ancestors differ,
//!   what remains of the right side stays in place after the joined block, empty containers
//!   dropped;
//! * **Enter** = `splitBlock` (the new block keeps the attributes — TipTap's `keepOnSplit` — and
//!   becomes a paragraph when a heading is split at its end), `splitListItem`, `liftEmptyBlock`;
//! * **lists** = `liftListItem` (out of the list, or to the outer list), `sinkListItem`,
//!   `wrapInList`, TipTap's `toggleList` (unwrap, change type, or wrap and join neighbours).
//!
//! Inside a table, a range across cells never changes the table's structure: the text of each
//! textblock in the range is cleared instead (what a cell selection's deletion does).
//!
//! Every function here applies its changes through a [`Tr`], so they undo exactly, and returns the
//! textblock path + offset the caret goes to (converted to a position by the caller after the
//! edit, since positions after the edit are only known then).

use serde_json::Value;

use super::inline;
use super::step::{EditError, Step, Tr};
use crate::model::Node;
use crate::pm::{content_start, is_container, is_textblock, node_at, node_size, textblock_at, textblocks, Path};

/// Where the caret goes after an edit: a textblock and an offset in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaretAt {
    pub path: Path,
    pub offset: usize,
}

impl CaretAt {
    pub fn new(path: Path, offset: usize) -> Self {
        Self { path, offset }
    }

    /// The position, in `doc` as it is now.
    pub fn pos(&self, doc: &Node) -> Option<usize> {
        content_start(doc, &self.path).map(|s| s + self.offset)
    }
}

fn ty(n: &Node) -> &str {
    n.node_type().unwrap_or("")
}

fn is_table_part(n: &Node) -> bool {
    matches!(ty(n), "table" | "tableRow" | "tableCell" | "tableHeader")
}

/// A copy of `node` with `children`.
pub fn with_children(node: &Node, children: Vec<Node>) -> Node {
    let mut n = node.clone();
    n.set_children(children);
    n
}

/// `node` cut after the descendant at `path` (everything after it removed at every depth), with
/// that descendant replaced by `leaf`.
fn left_cut(node: &Node, path: &[usize], leaf: &Node) -> Node {
    match path.split_first() {
        None => leaf.clone(),
        Some((&i, rest)) => {
            let kids = node.children();
            let mut out: Vec<Node> = kids[..i.min(kids.len())].to_vec();
            if let Some(child) = kids.get(i) {
                out.push(left_cut(child, rest, leaf));
            }
            with_children(node, out)
        }
    }
}

/// What remains of `node` after removing everything up to and including the descendant at `path`.
/// `None` when nothing remains.
fn right_cut(node: &Node, path: &[usize]) -> Option<Node> {
    let (&i, rest) = path.split_first()?;
    let kids = node.children();
    let mut out = Vec::new();
    if !rest.is_empty() {
        if let Some(r) = kids.get(i).and_then(|c| right_cut(c, rest)) {
            out.push(r);
        }
    }
    out.extend(kids.iter().skip(i + 1).cloned());
    if out.is_empty() {
        None
    } else {
        Some(with_children(node, out))
    }
}

/// Joins the left chain (down to A) with the right chain (down to B) — `replaceTwoWay`.
fn merge(l: &Node, lp: &[usize], r: &Node, rp: &[usize], a_new: &Node) -> Vec<Node> {
    if lp.is_empty() {
        let mut out = vec![a_new.clone()];
        if !rp.is_empty() {
            out.extend(right_cut(r, rp));
        }
        return out;
    }
    if rp.is_empty() {
        return vec![left_cut(l, lp, a_new)];
    }
    if ty(l) == ty(r) && !is_textblock(l) {
        let (li, lrest) = (lp[0], &lp[1..]);
        let (ri, rrest) = (rp[0], &rp[1..]);
        let (Some(lc), Some(rc)) = (l.children().get(li), r.children().get(ri)) else {
            return vec![left_cut(l, lp, a_new)];
        };
        let inner = merge(lc, lrest, rc, rrest, a_new);
        let mut kids: Vec<Node> = l.children()[..li].to_vec();
        kids.extend(inner);
        kids.extend(r.children().iter().skip(ri + 1).cloned());
        return vec![with_children(l, kids)];
    }
    let mut out = vec![left_cut(l, lp, a_new)];
    out.extend(right_cut(r, rp));
    out
}

/// Deletes `from..to` (both positions inside textblocks). Returns where the caret goes.
pub fn delete_range(tr: &mut Tr<'_>, from: usize, to: usize) -> Result<CaretAt, EditError> {
    let (from, to) = (from.min(to), from.max(to));
    let (pa, sa) = textblock_at(tr.doc, from).ok_or(EditError::BadPosition)?;
    let (pb, sb) = textblock_at(tr.doc, to).ok_or(EditError::BadPosition)?;
    let (off_a, off_b) = (from - sa, to - sb);
    if pa == pb {
        let a = node_at(tr.doc, &pa).ok_or(EditError::NoSuchNode)?;
        if from == to {
            return Ok(CaretAt::new(pa, off_a));
        }
        let kids = inline::replace(a.children(), off_a, off_b, vec![])?;
        let new = with_children(a, kids);
        tr.replace_node(&pa, new)?;
        return Ok(CaretAt::new(pa, off_a));
    }
    let c = pa.iter().zip(pb.iter()).take_while(|(x, y)| x == y).count();
    let cpath: Path = pa[..c].to_vec();
    let cnode = node_at(tr.doc, &cpath).ok_or(EditError::NoSuchNode)?;
    // Inside a table, across cells: clear the text, keep the structure.
    let mut crosses_cells = is_table_part(cnode);
    for (path, _) in [(&pa, ()), (&pb, ())] {
        for d in c + 1..path.len() {
            if node_at(tr.doc, &path[..d]).is_some_and(is_table_part) {
                crosses_cells = true;
            }
        }
    }
    if crosses_cells {
        return clear_text_range(tr, from, to);
    }
    let a = node_at(tr.doc, &pa).ok_or(EditError::NoSuchNode)?;
    let b = node_at(tr.doc, &pb).ok_or(EditError::NoSuchNode)?;
    let head = inline::cut(a.children(), off_a, off_a)?.prefix;
    let tail = inline::cut(b.children(), off_b, off_b)?.suffix;
    let a_new = with_children(a, inline::weld(head, vec![], tail));
    let (i, j) = (pa[c], pb[c]);
    let l = cnode.children().get(i).ok_or(EditError::NoSuchNode)?;
    let r = cnode.children().get(j).ok_or(EditError::NoSuchNode)?;
    let new = merge(l, &pa[c + 1..], r, &pb[c + 1..], &a_new);
    tr.step(Step { parent: cpath, from: i, to: j + 1, with: new })?;
    Ok(CaretAt::new(pa, off_a))
}

/// Clears the text of every textblock in `from..to`, keeping all structure.
pub fn clear_text_range(tr: &mut Tr<'_>, from: usize, to: usize) -> Result<CaretAt, EditError> {
    let blocks = textblocks(tr.doc);
    let mut caret = None;
    for (path, start) in blocks {
        let node = node_at(tr.doc, &path).ok_or(EditError::NoSuchNode)?.clone();
        let len = inline::content_len(node.children());
        let (bs, be) = (start, start + len);
        if be < from || bs > to {
            continue;
        }
        let a = from.max(bs) - bs;
        let b = to.min(be) - bs;
        if caret.is_none() {
            caret = Some(CaretAt::new(path.clone(), a));
        }
        if a < b {
            let kids = inline::replace(node.children(), a, b, vec![])?;
            tr.replace_node(&path, with_children(&node, kids))?;
        }
    }
    caret.ok_or(EditError::BadPosition)
}

/// The attributes a block keeps when it is split (TipTap `keepOnSplit`, default true), for a new
/// block of type `new_type`.
fn split_attrs(node: &Node, new_type: &str) -> Option<Value> {
    let mut attrs = node.attrs();
    if attrs.is_empty() {
        return None;
    }
    if new_type != "heading" {
        attrs.remove("level");
        attrs.remove("collapsed");
    }
    Some(Value::Object(attrs))
}

/// A new block of `new_type` with the split attributes of `node` and `children`.
fn split_node(node: &Node, new_type: &str, children: Vec<Node>) -> Node {
    if new_type == ty(node) {
        let mut n = node.clone();
        if children.is_empty() {
            n.remove("content");
        } else {
            n.set_children(children);
        }
        return n;
    }
    Node::element(new_type, split_attrs(node, new_type), children)
}

/// `splitBlock` at `pos` (inside a textblock). Returns the caret (start of the new block).
pub fn split_block(tr: &mut Tr<'_>, pos: usize) -> Result<CaretAt, EditError> {
    let (p, start) = textblock_at(tr.doc, pos).ok_or(EditError::BadPosition)?;
    let node = node_at(tr.doc, &p).ok_or(EditError::NoSuchNode)?.clone();
    let off = pos - start;
    let len = inline::content_len(node.children());
    let c = inline::cut(node.children(), off, off)?;
    // At the end of a heading the new block is a paragraph (`defaultBlockAt` after the split).
    let right_type = if off == len && ty(&node) == "heading" { "paragraph" } else { ty(&node) };
    let left = with_children(&node, c.prefix);
    let left = if left.children().is_empty() {
        let mut l = node.clone();
        l.remove("content");
        l
    } else {
        left
    };
    let right = split_node(&node, right_type, c.suffix);
    let (&i, parent) = p.split_last().ok_or(EditError::BadPosition)?;
    tr.step(Step { parent: parent.to_vec(), from: i, to: i + 1, with: vec![left, right] })?;
    let mut np = parent.to_vec();
    np.push(i + 1);
    Ok(CaretAt::new(np, 0))
}

/// The list item a textblock is directly in, as (item path, index of the textblock in it).
pub fn list_item_of(doc: &Node, tb: &[usize]) -> Option<(Path, usize)> {
    let (&i, parent) = tb.split_last()?;
    let item = node_at(doc, parent)?;
    matches!(ty(item), "listItem" | "taskItem").then(|| (parent.to_vec(), i))
}

/// `splitListItem` at `pos`. `Err(NotApplicable)` when not in a list item.
pub fn split_list_item(tr: &mut Tr<'_>, pos: usize) -> Result<CaretAt, EditError> {
    let (p, start) = textblock_at(tr.doc, pos).ok_or(EditError::BadPosition)?;
    let (item_path, ci) = list_item_of(tr.doc, &p).ok_or(EditError::NotApplicable)?;
    let item = node_at(tr.doc, &item_path).ok_or(EditError::NoSuchNode)?.clone();
    let tb = node_at(tr.doc, &p).ok_or(EditError::NoSuchNode)?.clone();
    let off = pos - start;
    let len = inline::content_len(tb.children());
    // An empty last block of the item: lift it out instead (Enter on an empty bullet ends the list).
    if len == 0 && ci + 1 == item.children().len() {
        return lift_list_item(tr, pos);
    }
    let c = inline::cut(tb.children(), off, off)?;
    let left_tb = if c.prefix.is_empty() {
        let mut l = tb.clone();
        l.remove("content");
        l
    } else {
        with_children(&tb, c.prefix)
    };
    let right_tb = split_node(&tb, ty(&tb), c.suffix);
    let mut left_kids: Vec<Node> = item.children()[..ci].to_vec();
    left_kids.push(left_tb);
    let mut right_kids = vec![right_tb];
    right_kids.extend(item.children().iter().skip(ci + 1).cloned());
    let left_item = with_children(&item, left_kids);
    let mut right_item = with_children(&item, right_kids);
    if ty(&item) == "taskItem" {
        let mut a = item.attrs();
        a.insert("checked".into(), Value::Bool(false));
        right_item.set_value("attrs", &Value::Object(a));
    }
    let (&k, list_path) = item_path.split_last().ok_or(EditError::BadPosition)?;
    tr.step(Step { parent: list_path.to_vec(), from: k, to: k + 1, with: vec![left_item, right_item] })?;
    let mut np = list_path.to_vec();
    np.push(k + 1);
    np.push(0);
    Ok(CaretAt::new(np, 0))
}

/// `liftListItem` for the item holding `pos`: out of the list (top level) or to the outer list.
pub fn lift_list_item(tr: &mut Tr<'_>, pos: usize) -> Result<CaretAt, EditError> {
    let (p, start) = textblock_at(tr.doc, pos).ok_or(EditError::BadPosition)?;
    let off = pos - start;
    let (item_path, ci) = list_item_of(tr.doc, &p).ok_or(EditError::NotApplicable)?;
    let below: Path = p[item_path.len() + 1..].to_vec(); // path below the item's child (empty for a direct textblock)
    let (&k, list_path) = item_path.split_last().ok_or(EditError::BadPosition)?;
    let list_path = list_path.to_vec();
    let list = node_at(tr.doc, &list_path).ok_or(EditError::NoSuchNode)?.clone();
    let item = list.children().get(k).ok_or(EditError::NoSuchNode)?.clone();
    let before: Vec<Node> = list.children()[..k].to_vec();
    let after: Vec<Node> = list.children()[k + 1..].to_vec();

    // Nested: the list is inside an outer list item → become an item of the outer list.
    if let Some((outer_item_path, li)) = list_path.split_last().and_then(|(&li, op)| {
        node_at(tr.doc, op).filter(|n| matches!(ty(n), "listItem" | "taskItem")).map(|_| (op.to_vec(), li))
    }) {
        let outer_item = node_at(tr.doc, &outer_item_path).ok_or(EditError::NoSuchNode)?.clone();
        let (&oi, outer_list_path) = outer_item_path.split_last().ok_or(EditError::BadPosition)?;
        let mut o_kids: Vec<Node> = outer_item.children()[..li].to_vec();
        if !before.is_empty() {
            o_kids.push(with_children(&list, before));
        }
        let o_rest: Vec<Node> = outer_item.children().iter().skip(li + 1).cloned().collect();
        let new_outer = with_children(&outer_item, o_kids);
        let mut lifted_kids = item.children().to_vec();
        if !after.is_empty() {
            lifted_kids.push(with_children(&list, after));
        }
        lifted_kids.extend(o_rest);
        let lifted = with_children(&item, lifted_kids);
        tr.step(Step { parent: outer_list_path.to_vec(), from: oi, to: oi + 1, with: vec![new_outer, lifted] })?;
        let mut np = outer_list_path.to_vec();
        np.push(oi + 1);
        np.push(ci);
        np.extend(below);
        return Ok(CaretAt::new(np, off));
    }

    // Top level (or inside a non-list container): split the list around the item's content.
    let mut with = Vec::new();
    if !before.is_empty() {
        with.push(with_children(&list, before.clone()));
    }
    let lifted_at = with.len();
    with.extend(item.children().iter().cloned());
    if !after.is_empty() {
        with.push(with_children(&list, after));
    }
    let (&li, parent) = list_path.split_last().ok_or(EditError::BadPosition)?;
    tr.step(Step { parent: parent.to_vec(), from: li, to: li + 1, with })?;
    let mut np = parent.to_vec();
    np.push(li + lifted_at + ci);
    np.extend(below);
    Ok(CaretAt::new(np, off))
}

/// `sinkListItem` for the item holding `pos` (Tab in a list). `Err(NotApplicable)` for a first
/// item or outside a list.
pub fn sink_list_item(tr: &mut Tr<'_>, pos: usize) -> Result<CaretAt, EditError> {
    let (p, start) = textblock_at(tr.doc, pos).ok_or(EditError::BadPosition)?;
    let off = pos - start;
    let (item_path, ci) = list_item_of(tr.doc, &p).ok_or(EditError::NotApplicable)?;
    let below: Path = p[item_path.len() + 1..].to_vec();
    let (&k, list_path) = item_path.split_last().ok_or(EditError::BadPosition)?;
    if k == 0 {
        return Err(EditError::NotApplicable);
    }
    let list = node_at(tr.doc, list_path).ok_or(EditError::NoSuchNode)?.clone();
    let prev = list.children()[k - 1].clone();
    let item = list.children()[k].clone();
    let mut prev_kids = prev.children().to_vec();
    let (nested_index, item_index);
    match prev_kids.last_mut() {
        Some(last) if ty(last) == ty(&list) => {
            let mut nk = last.children().to_vec();
            item_index = nk.len();
            nk.push(item);
            *last = with_children(last, nk);
            nested_index = prev_kids.len() - 1;
        }
        _ => {
            let nested = Node::element(ty(&list), None, vec![item]);
            prev_kids.push(nested);
            nested_index = prev_kids.len() - 1;
            item_index = 0;
        }
    }
    let new_prev = with_children(&prev, prev_kids);
    tr.step(Step { parent: list_path.to_vec(), from: k - 1, to: k + 1, with: vec![new_prev] })?;
    let mut np = list_path.to_vec();
    np.push(k - 1);
    np.push(nested_index);
    np.push(item_index);
    np.push(ci);
    np.extend(below);
    Ok(CaretAt::new(np, off))
}

/// The top-level-most sibling range of blocks covering `from..to` that share a parent which may
/// hold blocks: (parent path, first index, last index).
fn block_range(doc: &Node, from: usize, to: usize) -> Option<(Path, usize, usize)> {
    let (pa, _) = textblock_at(doc, from)?;
    let (pb, _) = textblock_at(doc, to)?;
    let c = pa.iter().zip(pb.iter()).take_while(|(x, y)| x == y).count();
    if pa == pb {
        let (&i, parent) = pa.split_last()?;
        return Some((parent.to_vec(), i, i));
    }
    Some((pa[..c].to_vec(), pa[c], pb[c]))
}

/// Wraps the blocks of `from..to` in a list of `list_type`, one item per block, joining an
/// adjacent list of the same type (TipTap `toggleList` → `wrapInList` + `joinListBackwards/
/// Forwards`). Returns the caret for `pos`.
pub fn wrap_in_list(tr: &mut Tr<'_>, from: usize, to: usize, list_type: &str, caret_pos: usize) -> Result<CaretAt, EditError> {
    let (parent, i, j) = block_range(tr.doc, from, to).ok_or(EditError::BadPosition)?;
    let (cp, cs) = textblock_at(tr.doc, caret_pos).ok_or(EditError::BadPosition)?;
    let coff = caret_pos - cs;
    let pnode = node_at(tr.doc, &parent).ok_or(EditError::NoSuchNode)?.clone();
    let item_type = if list_type == "taskList" { "taskItem" } else { "listItem" };
    let items: Vec<Node> = pnode.children()[i..=j]
        .iter()
        .map(|b| {
            let attrs = (item_type == "taskItem").then(|| serde_json::json!({ "checked": false }));
            Node::element(item_type, attrs, vec![b.clone()])
        })
        .collect();
    let n_items = items.len();
    let kids = pnode.children();
    let join_prev = i > 0 && ty(&kids[i - 1]) == list_type;
    let join_next = j + 1 < kids.len() && ty(&kids[j + 1]) == list_type;
    let (from_i, to_i, mut list_kids, base) = if join_prev {
        let prev = &kids[i - 1];
        (i - 1, j + 1, prev.children().to_vec(), prev.children().len())
    } else {
        (i, j + 1, Vec::new(), 0)
    };
    list_kids.extend(items);
    let mut to_i = to_i;
    if join_next {
        list_kids.extend(kids[j + 1].children().iter().cloned());
        to_i = j + 2;
    }
    let list = if join_prev { with_children(&kids[i - 1], list_kids) } else { Node::element(list_type, None, list_kids) };
    let _ = n_items;
    tr.step(Step { parent: parent.clone(), from: from_i, to: to_i, with: vec![list] })?;
    // The caret's block is item (base + its index - i), child 0, then whatever was below it.
    let rel = cp.get(parent.len()).copied().unwrap_or(i) - i;
    let mut np = parent;
    np.push(from_i);
    np.push(base + rel);
    np.push(0);
    np.extend(cp.iter().skip(np.len() - 2).copied());
    Ok(CaretAt::new(np, coff))
}

/// TipTap `toggleList(list_type)` for the selection `from..to` with the caret at `caret_pos`.
pub fn toggle_list(tr: &mut Tr<'_>, from: usize, to: usize, list_type: &str, caret_pos: usize) -> Result<CaretAt, EditError> {
    let (p, _) = textblock_at(tr.doc, caret_pos).ok_or(EditError::BadPosition)?;
    if let Some((item_path, _)) = list_item_of(tr.doc, &p) {
        let (_, list_path) = item_path.split_last().ok_or(EditError::BadPosition)?;
        let list = node_at(tr.doc, list_path).ok_or(EditError::NoSuchNode)?.clone();
        if ty(&list) == list_type {
            // Same list type: lift the items of the range out (unwrap).
            return lift_items_in_range(tr, from, to, caret_pos);
        }
        // Another list type: change the list's type (`setNodeMarkup`).
        let mut n = list.clone();
        n.set_value("type", &Value::String(list_type.to_string()));
        if list_type == "taskList" || ty(&list) == "taskList" {
            let item_type = if list_type == "taskList" { "taskItem" } else { "listItem" };
            let kids: Vec<Node> = list
                .children()
                .iter()
                .map(|it| {
                    let mut c = it.clone();
                    c.set_value("type", &Value::String(item_type.to_string()));
                    c
                })
                .collect();
            n.set_children(kids);
        }
        tr.replace_node(list_path, n)?;
        let (cp, cs) = textblock_at(tr.doc, caret_pos).ok_or(EditError::BadPosition)?;
        return Ok(CaretAt::new(cp, caret_pos - cs));
    }
    wrap_in_list(tr, from, to, list_type, caret_pos)
}

/// Lifts every list item whose first textblock is in `from..to`, last first: lifting an item never
/// moves anything before it, so the earlier items keep their positions, and the caret — measured
/// from the end of the document once its own item is lifted — keeps its place.
fn lift_items_in_range(tr: &mut Tr<'_>, from: usize, to: usize, caret_pos: usize) -> Result<CaretAt, EditError> {
    let starts: Vec<usize> = textblocks(tr.doc)
        .into_iter()
        .filter(|(path, s)| {
            let len = node_at(tr.doc, path).map(|n| inline::content_len(n.children())).unwrap_or(0);
            *s + len >= from && *s <= to && list_item_of(tr.doc, path).is_some_and(|(_, ci)| ci == 0)
        })
        .map(|(_, s)| s)
        .collect();
    let (_, cs) = textblock_at(tr.doc, caret_pos).ok_or(EditError::BadPosition)?;
    let mut from_end: Option<usize> = None;
    for s in starts.iter().rev() {
        let at = if *s == cs { caret_pos } else { *s };
        let c = lift_list_item(tr, at)?;
        if *s == cs {
            let pos = c.pos(tr.doc).ok_or(EditError::BadPosition)?;
            from_end = Some(crate::pm::content_size(tr.doc) - pos);
        }
    }
    let total = crate::pm::content_size(tr.doc);
    let pos = match from_end {
        Some(d) => total.saturating_sub(d),
        None => total.saturating_sub(crate::pm::content_size(tr.doc)).max(caret_pos.min(total)),
    };
    let (p, s) = textblock_at(tr.doc, pos).ok_or(EditError::BadPosition)?;
    Ok(CaretAt::new(p, pos - s))
}

/// The previous textblock in document order before the one at `path`, if any.
pub fn prev_textblock(doc: &Node, path: &[usize]) -> Option<(Path, usize)> {
    let all = textblocks(doc);
    let idx = all.iter().position(|(p, _)| p.as_slice() == path)?;
    idx.checked_sub(1).map(|i| all[i].clone())
}

/// The next textblock in document order after the one at `path`.
pub fn next_textblock(doc: &Node, path: &[usize]) -> Option<(Path, usize)> {
    let all = textblocks(doc);
    let idx = all.iter().position(|(p, _)| p.as_slice() == path)?;
    all.get(idx + 1).cloned()
}

/// The node just before the block at `path` at the shallowest level where that block is the first
/// child — the `$cut` of `joinBackward`. Returns (path of that sibling) when it is a leaf block
/// (an atom such as an image, a rule, a page break) — what Backspace deletes whole.
pub fn atom_before(doc: &Node, path: &[usize]) -> Option<Path> {
    let mut p = path.to_vec();
    loop {
        let (&i, parent) = p.split_last()?;
        if i > 0 {
            let mut sib = parent.to_vec();
            sib.push(i - 1);
            let n = node_at(doc, &sib)?;
            return (!is_container(n) && ty(n) != "text").then_some(sib);
        }
        if parent.is_empty() {
            return None;
        }
        p = parent.to_vec();
    }
}

/// The leaf block just after the block at `path` (Delete at the end of a block before an atom).
pub fn atom_after(doc: &Node, path: &[usize]) -> Option<Path> {
    let mut p = path.to_vec();
    loop {
        let (&i, parent) = p.split_last()?;
        let pnode = node_at(doc, parent)?;
        if i + 1 < pnode.children().len() {
            let mut sib = parent.to_vec();
            sib.push(i + 1);
            let n = node_at(doc, &sib)?;
            return (!is_container(n) && ty(n) != "text").then_some(sib);
        }
        if parent.is_empty() {
            return None;
        }
        p = parent.to_vec();
    }
}

/// Deletes the node at `path`.
pub fn delete_node(tr: &mut Tr<'_>, path: &[usize]) -> Result<(), EditError> {
    let (&i, parent) = path.split_last().ok_or(EditError::BadPosition)?;
    tr.step(Step { parent: parent.to_vec(), from: i, to: i + 1, with: vec![] })
}

/// Inserts block nodes at the caret `pos`: the textblock is split there (an empty half is dropped
/// when the caret is at an edge, except that a paragraph always follows the inserted blocks so
/// the caret has somewhere to go). Returns the caret (start of the block after the insertion).
pub fn insert_blocks(tr: &mut Tr<'_>, pos: usize, blocks: Vec<Node>) -> Result<CaretAt, EditError> {
    let (p, start) = textblock_at(tr.doc, pos).ok_or(EditError::BadPosition)?;
    let node = node_at(tr.doc, &p).ok_or(EditError::NoSuchNode)?.clone();
    let off = pos - start;
    let len = inline::content_len(node.children());
    let c = inline::cut(node.children(), off, off)?;
    let (&i, parent) = p.split_last().ok_or(EditError::BadPosition)?;
    let mut with = Vec::new();
    if off > 0 {
        with.push(with_children(&node, c.prefix));
    }
    with.extend(blocks);
    let after_index = with.len();
    if off == 0 {
        // At the start (or in an empty block): the blocks go before it, and it follows them.
        with.push(node.clone());
    } else if off < len {
        with.push(split_node(&node, ty(&node), c.suffix));
    } else {
        // At the end: an empty paragraph follows, so the caret has somewhere to go.
        with.push(Node::of_type("paragraph"));
    }
    tr.step(Step { parent: parent.to_vec(), from: i, to: i + 1, with })?;
    let mut np = parent.to_vec();
    np.push(i + after_index);
    // The block after may be a container (a table): find its first textblock.
    let base = np.clone();
    if let Some(n) = node_at(tr.doc, &base) {
        if !is_textblock(n) {
            if let Some((first, _)) = first_textblock_in(n) {
                np.extend(first);
            }
        }
    }
    Ok(CaretAt::new(np, 0))
}

/// The first textblock inside `node` (relative path), if any.
pub fn first_textblock_in(node: &Node) -> Option<(Path, usize)> {
    let tbs = textblocks(node);
    tbs.into_iter().next()
}

/// The size of a node — re-exported for commands.
pub fn size(node: &Node) -> usize {
    node_size(node)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(json: &str) -> Node {
        Node::from_slice(json.as_bytes()).expect("parses")
    }

    fn p(text: &str) -> String {
        if text.is_empty() {
            return r#"{"type":"paragraph"}"#.into();
        }
        format!(r#"{{"content":[{{"text":"{text}","type":"text"}}],"type":"paragraph"}}"#)
    }

    fn li(inner: &[String]) -> String {
        format!(r#"{{"content":[{}],"type":"listItem"}}"#, inner.join(","))
    }

    fn ul(items: &[String]) -> String {
        format!(r#"{{"content":[{}],"type":"bulletList"}}"#, items.join(","))
    }

    fn body(blocks: &[String]) -> Node {
        doc(&format!(r#"{{"content":[{}],"type":"doc"}}"#, blocks.join(",")))
    }

    /// A compact outline of the body: `p(text)`, `ul[li[...]]`.
    fn outline(n: &Node) -> String {
        match ty(n) {
            "text" => n.text().unwrap_or_default(),
            "doc" => n.children().iter().map(outline).collect::<Vec<_>>().join(" "),
            t => format!("{}[{}]", t, n.children().iter().map(outline).collect::<Vec<_>>().join(" ")),
        }
    }

    #[test]
    fn deleting_across_two_paragraphs_joins_them() {
        let mut d = body(&[p("hello"), p("big"), p("world")]);
        let mut tr = Tr::new(&mut d);
        // "he|llo" … "wo|rld": from 3, to = p3 start (7+5=12… p0 0..7, p1 7..12, p2 12..19) +1 +2 = 15
        let caret = delete_range(&mut tr, 3, 15).expect("deletes");
        assert_eq!(caret, CaretAt::new(vec![0], 2));
        drop(tr);
        assert_eq!(outline(&d), "paragraph[herld]");
    }

    #[test]
    fn deleting_between_list_items_joins_the_items() {
        let mut d = body(&[ul(&[li(&[p("one")]), li(&[p("two"), p("three")])])]);
        // list 0, item 1, p 2, "one" 3..6; p close 6, item close 7, item 8, p 9, "two" 10..13
        let mut tr = Tr::new(&mut d);
        delete_range(&mut tr, 5, 11).expect("deletes");
        drop(tr);
        assert_eq!(outline(&d), "bulletList[listItem[paragraph[onwo] paragraph[three]]]");
    }

    #[test]
    fn deleting_from_a_paragraph_into_a_list_keeps_the_rest_of_the_list() {
        let mut d = body(&[p("ab"), ul(&[li(&[p("cd"), p("ef")]), li(&[p("gh")])])]);
        // p 0..4 ("ab" 1..3); list 4, item 5, p 6, "cd" 7..9
        let mut tr = Tr::new(&mut d);
        delete_range(&mut tr, 2, 8).expect("deletes");
        drop(tr);
        assert_eq!(outline(&d), "paragraph[ad] bulletList[listItem[paragraph[ef]] listItem[paragraph[gh]]]");
    }

    #[test]
    fn splitting_keeps_attributes_and_a_heading_ends_in_a_paragraph() {
        let mut d = doc(r#"{"content":[{"attrs":{"level":1,"textAlign":"center"},"content":[{"text":"Title","type":"text"}],"type":"heading"}],"type":"doc"}"#);
        let mut tr = Tr::new(&mut d);
        let c = split_block(&mut tr, 6).expect("splits");
        assert_eq!(c, CaretAt::new(vec![1], 0));
        drop(tr);
        let second = &d.children()[1];
        assert_eq!(second.node_type(), Some("paragraph"));
        assert_eq!(second.attr("textAlign"), Some(Value::String("center".into())));
        assert!(second.attr("level").is_none());
        // In the middle: two headings.
        let mut d = doc(r#"{"content":[{"attrs":{"level":1},"content":[{"text":"Title","type":"text"}],"type":"heading"}],"type":"doc"}"#);
        let mut tr = Tr::new(&mut d);
        split_block(&mut tr, 3).expect("splits");
        drop(tr);
        assert_eq!(outline(&d), "heading[Ti] heading[tle]");
    }

    #[test]
    fn enter_in_a_list_item_makes_a_new_item_and_on_an_empty_one_ends_the_list() {
        let mut d = body(&[ul(&[li(&[p("one")])])]);
        let mut tr = Tr::new(&mut d);
        let c = split_list_item(&mut tr, 6).expect("splits");
        assert_eq!(c, CaretAt::new(vec![0, 1, 0], 0));
        let pos = c.pos(tr.doc).expect("pos");
        let c2 = split_list_item(&mut tr, pos).expect("lifts");
        drop(tr);
        assert_eq!(outline(&d), "bulletList[listItem[paragraph[one]]] paragraph[]");
        assert_eq!(c2, CaretAt::new(vec![1], 0));
    }

    #[test]
    fn tab_sinks_and_shift_tab_lifts_to_the_outer_list() {
        let mut d = body(&[ul(&[li(&[p("a")]), li(&[p("b")])])]);
        let mut tr = Tr::new(&mut d);
        // "b" at: list 0, item 1, p 2, a 3, /p 4, /item 5, item 6, p 7, b 8
        let c = sink_list_item(&mut tr, 8).expect("sinks");
        assert_eq!(c.path, vec![0, 0, 1, 0, 0]);
        let pos = c.pos(tr.doc).expect("pos");
        let c2 = lift_list_item(&mut tr, pos).expect("lifts");
        drop(tr);
        assert_eq!(outline(&d), "bulletList[listItem[paragraph[a]] listItem[paragraph[b]]]");
        assert_eq!(c2.path, vec![0, 1, 0]);
    }

    #[test]
    fn toggling_a_list_wraps_and_unwraps() {
        let mut d = body(&[p("a"), p("b")]);
        let mut tr = Tr::new(&mut d);
        let c = toggle_list(&mut tr, 1, 5, "orderedList", 5).expect("wraps");
        assert_eq!(c.path, vec![0, 1, 0]);
        drop(tr);
        assert_eq!(outline(&d), "orderedList[listItem[paragraph[a]] listItem[paragraph[b]]]");
        let mut tr = Tr::new(&mut d);
        let c = toggle_list(&mut tr, 3, 3, "bulletList", 3).expect("changes type");
        drop(tr);
        assert_eq!(outline(&d), "bulletList[listItem[paragraph[a]] listItem[paragraph[b]]]");
        let _ = c;
        let mut tr = Tr::new(&mut d);
        // Unwrap both: "a" at 3, "b" at 8.
        toggle_list(&mut tr, 3, 8, "bulletList", 3).expect("unwraps");
        drop(tr);
        assert_eq!(outline(&d), "paragraph[a] paragraph[b]");
    }

    #[test]
    fn a_page_break_goes_between_the_halves() {
        let mut d = body(&[p("abcd")]);
        let mut tr = Tr::new(&mut d);
        let c = insert_blocks(&mut tr, 3, vec![Node::of_type("pageBreak")]).expect("inserts");
        drop(tr);
        assert_eq!(outline(&d), "paragraph[ab] pageBreak[] paragraph[cd]");
        assert_eq!(c.path, vec![2]);
        let mut d = body(&[p("ab")]);
        let mut tr = Tr::new(&mut d);
        insert_blocks(&mut tr, 3, vec![Node::of_type("pageBreak")]).expect("inserts");
        drop(tr);
        assert_eq!(outline(&d), "paragraph[ab] pageBreak[] paragraph[]");
    }

    #[test]
    fn deleting_across_table_cells_keeps_the_table() {
        let mut d = doc(r#"{"content":[{"content":[{"content":[{"content":[{"content":[{"text":"ab","type":"text"}],"type":"paragraph"}],"type":"tableCell"},{"content":[{"content":[{"text":"cd","type":"text"}],"type":"paragraph"}],"type":"tableCell"}],"type":"tableRow"}],"type":"table"}],"type":"doc"}"#);
        // table 0, row 1, cell 2, p 3, "ab" 4..6, /p 6, /cell 7, cell 8, p 9, "cd" 10..12
        let mut tr = Tr::new(&mut d);
        delete_range(&mut tr, 5, 11).expect("clears");
        drop(tr);
        assert_eq!(outline(&d), "table[tableRow[tableCell[paragraph[a]] tableCell[paragraph[d]]]]");
    }
}
