//! Inline content of a textblock, by offset.
//!
//! Offsets count like ProseMirror positions inside the block: a text node contributes its UTF-16
//! length, every other inline node (hard break, inline image, field, note reference) exactly 1.
//!
//! Splitting a text node rewrites only its `text` (every other key keeps its bytes); two text nodes
//! are welded back only at the junctions an edit created, and only when they are byte-identical
//! apart from their text (so an explicit `"marks":null` stays distinct from an absent `marks`, as it
//! is in this format). (From the desktop's first editing module, `documents/src/edit/history.rs`.)

use serde_json::Value;

use super::step::EditError;
use crate::marks;
use crate::model::Node;
use crate::pm::{is_text, node_size, utf16_len, utf16_to_byte};

/// Marks that do not extend to text typed at their edge (`inclusive: false` in the web schema).
pub const NON_INCLUSIVE: &[&str] = &["comment", "bookmark", "spellLang", "insertion", "deletion", "indexEntry", "citationEntry"];

/// Positions one inline node takes.
pub fn inline_len(node: &Node) -> usize {
    node_size(node)
}

/// The content length of a textblock.
pub fn content_len(children: &[Node]) -> usize {
    children.iter().map(inline_len).sum()
}

/// The same text node with other text.
pub fn with_text(node: &Node, text: &str) -> Node {
    let mut out = node.clone();
    out.set_text(text);
    out
}

fn mergeable(a: &Node, b: &Node) -> bool {
    if !is_text(a) || !is_text(b) {
        return false;
    }
    let blank = |n: &Node| {
        let mut p = n.clone();
        p.set_text("");
        p.to_vec().ok()
    };
    matches!((blank(a), blank(b)), (Some(x), Some(y)) if x == y)
}

/// Appends `node`, welding it onto the previous text node when they are halves of one.
pub fn push_merged(out: &mut Vec<Node>, node: Node) {
    if let Some(last) = out.last() {
        if mergeable(last, &node) {
            if let (Some(head), Some(tail)) = (last.text(), node.text()) {
                let joined = with_text(last, &(head + &tail));
                let end = out.len() - 1;
                out[end] = joined;
                return;
            }
        }
    }
    if is_text(&node) && node.text().is_some_and(|t| t.is_empty()) {
        return; // ProseMirror never stores an empty text node
    }
    out.push(node);
}

/// A textblock's children cut at two offsets.
#[derive(Clone, Debug, Default)]
pub struct Cut {
    pub prefix: Vec<Node>,
    pub removed: Vec<Node>,
    pub suffix: Vec<Node>,
}

/// Cuts `children` at `from..to`.
pub fn cut(children: &[Node], from: usize, to: usize) -> Result<Cut, EditError> {
    if from > to || to > content_len(children) {
        return Err(EditError::BadRange);
    }
    let mut out = Cut::default();
    let mut pos = 0usize;
    for child in children {
        let start = pos;
        let end = start + inline_len(child);
        pos = end;
        if end <= from && !(end == start && start == from && from == to) {
            out.prefix.push(child.clone());
            continue;
        }
        if start >= to {
            out.suffix.push(child.clone());
            continue;
        }
        match (is_text(child), child.text()) {
            (true, Some(text)) => {
                let head = from.saturating_sub(start);
                let tail = (to - start).min(end - start);
                let hb = utf16_to_byte(&text, head).ok_or(EditError::BadPosition)?;
                let tb = utf16_to_byte(&text, tail).ok_or(EditError::BadPosition)?;
                if hb > 0 {
                    out.prefix.push(with_text(child, &text[..hb]));
                }
                if tb > hb {
                    out.removed.push(with_text(child, &text[hb..tb]));
                }
                if tb < text.len() {
                    out.suffix.push(with_text(child, &text[tb..]));
                }
            }
            _ => out.removed.push(child.clone()),
        }
    }
    Ok(out)
}

/// `prefix + with + suffix`, welding the two junctions.
pub fn weld(prefix: Vec<Node>, with: Vec<Node>, suffix: Vec<Node>) -> Vec<Node> {
    let mut out = Vec::with_capacity(prefix.len() + with.len() + suffix.len());
    for n in prefix {
        out.push(n);
    }
    let mut first = true;
    for n in with {
        if first {
            push_merged(&mut out, n);
            first = false;
        } else if is_text(&n) && n.text().is_some_and(|t| t.is_empty()) {
            continue;
        } else {
            out.push(n);
        }
    }
    let mut rest = suffix.into_iter();
    if let Some(n) = rest.next() {
        push_merged(&mut out, n);
    }
    out.extend(rest);
    out
}

/// Replaces `from..to` of a textblock's children with `with`.
pub fn replace(children: &[Node], from: usize, to: usize, with: Vec<Node>) -> Result<Vec<Node>, EditError> {
    let c = cut(children, from, to)?;
    Ok(weld(c.prefix, with, c.suffix))
}

/// The marks of the inline node `i`, parsed.
fn node_marks(node: &Node) -> Vec<Value> {
    marks::marks_of(node)
}

/// `ResolvedPos.marks()`: the marks text typed at `offset` takes — those of the node before it (the
/// node after it at the block's start), minus the non-inclusive ones the other side does not share.
pub fn marks_at(children: &[Node], offset: usize) -> Vec<Value> {
    if children.is_empty() {
        return Vec::new();
    }
    let mut pos = 0usize;
    let mut before: Option<&Node> = None;
    let mut after: Option<&Node> = None;
    for child in children {
        let len = inline_len(child);
        if offset > pos && offset < pos + len {
            // Inside a text node: its own marks.
            return node_marks(child);
        }
        if pos + len == offset && len > 0 {
            before = Some(child);
        }
        if pos == offset && after.is_none() && len > 0 {
            after = Some(child);
        }
        pos += len;
    }
    let (main, other) = match before {
        Some(b) => (b, after),
        None => match after {
            Some(a) => (a, None),
            None => return Vec::new(),
        },
    };
    let other_marks = other.map(node_marks).unwrap_or_default();
    node_marks(main)
        .into_iter()
        .filter(|m| {
            let ty = m.get("type").and_then(Value::as_str).unwrap_or("");
            !NON_INCLUSIVE.contains(&ty) || other_marks.iter().any(|o| o == m)
        })
        .collect()
}

/// Inserts `text` at `offset` with `marks`.
pub fn insert_text(children: &[Node], offset: usize, text: &str, marks: &[Value]) -> Result<Vec<Node>, EditError> {
    let raw = marks::to_raw(marks);
    let node = Node::text_node(text, raw.as_deref());
    replace(children, offset, offset, vec![node])
}

/// Rewrites the marks of every text node in `from..to` with `f`.
pub fn map_marks(children: &[Node], from: usize, to: usize, f: &dyn Fn(&[Value]) -> Vec<Value>) -> Result<Vec<Node>, EditError> {
    let c = cut(children, from, to)?;
    let mut out = c.prefix;
    let mut first = true;
    for node in c.removed {
        let mapped = if is_text(&node) {
            let new_marks = f(&node_marks(&node));
            let mut n = node.clone();
            match marks::to_raw(&new_marks) {
                Some(raw) => n.set_raw("marks", raw),
                None => {
                    // An emptied marks array: the browser omits the key.
                    if n.has("marks") {
                        n.remove("marks");
                    }
                }
            }
            n
        } else {
            node
        };
        if first {
            push_merged(&mut out, mapped);
            first = false;
        } else {
            push_merged(&mut out, mapped);
        }
    }
    let mut rest = c.suffix.into_iter();
    if let Some(n) = rest.next() {
        push_merged(&mut out, n);
    }
    out.extend(rest);
    Ok(out)
}

/// The text of `from..to` (atoms as `leaf`).
pub fn text_of(children: &[Node], from: usize, to: usize) -> String {
    let mut out = String::new();
    let mut pos = 0usize;
    for child in children {
        let len = inline_len(child);
        let (start, end) = (pos, pos + len);
        pos = end;
        if end <= from || start >= to {
            continue;
        }
        if let (true, Some(t)) = (is_text(child), child.text()) {
            let a = from.saturating_sub(start);
            let b = (to - start).min(len);
            if let (Some(x), Some(y)) = (utf16_to_byte(&t, a), utf16_to_byte(&t, b)) {
                out.push_str(&t[x..y]);
            }
        } else if child.node_type() == Some("hardBreak") {
            out.push('\n');
        }
    }
    out
}

/// Every text node in `from..to` and its marks, with the node's range (for "is the mark on the
/// whole selection?").
pub fn text_ranges(children: &[Node], from: usize, to: usize) -> Vec<(usize, usize, Vec<Value>, String)> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    for child in children {
        let len = inline_len(child);
        let (start, end) = (pos, pos + len);
        pos = end;
        if end <= from || start >= to || !is_text(child) {
            continue;
        }
        let t = child.text().unwrap_or_default();
        let a = from.max(start) - start;
        let b = to.min(end) - start;
        let piece = match (utf16_to_byte(&t, a), utf16_to_byte(&t, b)) {
            (Some(x), Some(y)) => t[x..y].to_string(),
            _ => String::new(),
        };
        out.push((from.max(start), to.min(end), node_marks(child), piece));
    }
    out
}

/// The content as UTF-16 units, one unit (U+FFFC) per atom — offsets index it directly.
pub fn text_units(children: &[Node]) -> Vec<u16> {
    let mut out = Vec::new();
    for child in children {
        match (is_text(child), child.text()) {
            (true, Some(t)) => out.extend(t.encode_utf16()),
            _ => out.extend(std::iter::repeat_n(0xFFFC, inline_len(child))),
        }
    }
    out
}

/// UTF-16 length helper re-exported for commands.
pub fn len16(s: &str) -> usize {
    utf16_len(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn kids(json: &str) -> Vec<Node> {
        Node::from_slice(json.as_bytes()).expect("parses").children().to_vec()
    }

    fn texts(children: &[Node]) -> Vec<String> {
        children.iter().map(|c| c.text().unwrap_or_else(|| format!("<{}>", c.node_type().unwrap_or("?")))).collect()
    }

    const P: &str = r#"{"content":[{"text":"ab","type":"text"},{"marks":[{"type":"bold"}],"text":"cd","type":"text"},{"type":"hardBreak"},{"text":"e","type":"text"}],"type":"paragraph"}"#;

    #[test]
    fn insertion_welds_into_a_matching_neighbour() {
        let k = kids(P);
        let out = insert_text(&k, 1, "X", &[]).expect("ok");
        assert_eq!(texts(&out), vec!["aXb", "cd", "<hardBreak>", "e"]);
        let out = insert_text(&k, 3, "Y", &[json!({"type":"bold"})]).expect("ok");
        assert_eq!(texts(&out), vec!["ab", "cYd", "<hardBreak>", "e"]);
    }

    #[test]
    fn marks_at_follow_prosemirror() {
        let k = kids(P);
        assert!(marks_at(&k, 1).is_empty());
        // At the end of "ab", before "cd": the node before wins.
        assert!(marks_at(&k, 2).is_empty());
        // Inside or right after "cd": bold.
        assert_eq!(marks_at(&k, 3), vec![json!({"type":"bold"})]);
        assert_eq!(marks_at(&k, 4), vec![json!({"type":"bold"})]);
        // At the start: the node after.
        let b = kids(r#"{"content":[{"marks":[{"type":"italic"}],"text":"x","type":"text"}],"type":"paragraph"}"#);
        assert_eq!(marks_at(&b, 0), vec![json!({"type":"italic"})]);
    }

    #[test]
    fn a_comment_mark_does_not_extend() {
        let k = kids(r#"{"content":[{"marks":[{"attrs":{"id":"c1"},"type":"comment"},{"type":"bold"}],"text":"x","type":"text"},{"text":"y","type":"text"}],"type":"paragraph"}"#);
        assert_eq!(marks_at(&k, 1), vec![json!({"type":"bold"})]);
    }

    #[test]
    fn cutting_across_an_atom_removes_it_whole() {
        let k = kids(P);
        let out = replace(&k, 3, 6, vec![]).expect("ok");
        assert_eq!(texts(&out), vec!["ab", "c"]);
        assert_eq!(text_of(&k, 0, 6), "abcd\ne");
    }

    #[test]
    fn marks_are_mapped_on_a_range_and_neighbours_weld() {
        let k = kids(P);
        let out = map_marks(&k, 0, 2, &|m| marks::add_mark(m, json!({"type":"bold"}))).expect("ok");
        assert_eq!(texts(&out), vec!["abcd", "<hardBreak>", "e"]);
        let out = map_marks(&out, 0, 4, &|m| marks::remove_mark(m, "bold")).expect("ok");
        assert_eq!(texts(&out), vec!["abcd", "<hardBreak>", "e"]);
        assert!(!out[0].has("marks"));
    }
}
