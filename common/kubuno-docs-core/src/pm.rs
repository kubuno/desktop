//! ProseMirror positions over the stored document.
//!
//! A position counts **between** things, over the whole body, with ProseMirror's rules: a text
//! node is as long as its text in **UTF-16 code units** (JavaScript string length — every `pmPos`
//! of the web engine is one), a leaf node (hard break, image, page break, field, note reference…)
//! is 1, and any other node is 2 (its opening and closing tokens) plus its content. Position 0 is
//! before the first block of the body; the body's own tokens are not counted (the body is the
//! `doc` node, whose content starts at 0).
//!
//! The office schema decides what is a leaf and what is a textblock. [`is_container`] lists the
//! node types that hold content in the web editor's schema (TipTap StarterKit + the office
//! extensions, `DocumentEditorPage.tsx` `PAGE_EXTENSIONS`); a node whose `content` is an array is a
//! container whatever its type (so a type added to the schema next year still sizes correctly when
//! it has content), and everything else is a leaf.
//!
//! A [`Path`] addresses a node by child indices from the body: `[3, 0, 1]` is
//! `body.content[3].content[0].content[1]`.

use crate::model::Node;

/// Child indices from the body to a node.
pub type Path = Vec<usize>;

/// Node types that hold content in the office schema (StarterKit + office extensions).
const CONTAINERS: &[&str] = &[
    "doc",
    "paragraph",
    "heading",
    "codeBlock",
    "blockquote",
    "bulletList",
    "orderedList",
    "listItem",
    "taskList",
    "taskItem",
    "table",
    "tableRow",
    "tableCell",
    "tableHeader",
];

/// Block types whose content is inline (text and inline atoms).
const TEXTBLOCKS: &[&str] = &["paragraph", "heading", "codeBlock"];

/// Whether a node holds content (has opening and closing tokens).
pub fn is_container(node: &Node) -> bool {
    if node.has_child_array() {
        return true;
    }
    node.node_type().is_some_and(|t| CONTAINERS.contains(&t))
}

/// Whether a node is a textblock: its children are inline.
pub fn is_textblock(node: &Node) -> bool {
    node.node_type().is_some_and(|t| TEXTBLOCKS.contains(&t))
}

/// Whether a node is a text node.
pub fn is_text(node: &Node) -> bool {
    node.node_type() == Some("text")
}

/// UTF-16 length of a string — the unit positions count in.
pub fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// Byte index of a UTF-16 offset, `None` inside a surrogate pair or past the end.
pub fn utf16_to_byte(text: &str, offset: usize) -> Option<usize> {
    let mut units = 0usize;
    for (byte, ch) in text.char_indices() {
        if units == offset {
            return Some(byte);
        }
        units += ch.len_utf16();
        if units > offset {
            return None;
        }
    }
    (units == offset).then_some(text.len())
}

/// The ProseMirror size of a node.
pub fn node_size(node: &Node) -> usize {
    if is_text(node) {
        return node.text().map(|t| utf16_len(&t)).unwrap_or(0);
    }
    if is_container(node) {
        return 2 + content_size(node);
    }
    1
}

/// The size of a node's content (the sum of its children's sizes).
pub fn content_size(node: &Node) -> usize {
    node.children().iter().map(node_size).sum()
}

/// The node at `path` from `root`.
pub fn node_at<'a>(root: &'a Node, path: &[usize]) -> Option<&'a Node> {
    let mut node = root;
    for &i in path {
        node = node.children().get(i)?;
    }
    Some(node)
}

/// The node at `path`, mutable.
pub fn node_at_mut<'a>(root: &'a mut Node, path: &[usize]) -> Option<&'a mut Node> {
    let mut node = root;
    for &i in path {
        node = node.children_mut()?.get_mut(i)?;
    }
    Some(node)
}

/// The position just before the node at `path` (its opening token).
pub fn pos_before(root: &Node, path: &[usize]) -> Option<usize> {
    let mut node = root;
    let mut pos = 0usize;
    for (depth, &i) in path.iter().enumerate() {
        let children = node.children();
        if i > children.len() {
            return None;
        }
        pos += children[..i].iter().map(node_size).sum::<usize>();
        if depth + 1 < path.len() {
            node = children.get(i)?;
            pos += 1; // into the child's content
        }
    }
    Some(pos)
}

/// The position where the content of the node at `path` starts (just inside it).
pub fn content_start(root: &Node, path: &[usize]) -> Option<usize> {
    if path.is_empty() {
        return Some(0);
    }
    pos_before(root, path).map(|p| p + 1)
}

/// A position resolved against the document: which node it is directly inside, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub pos: usize,
    /// The path of the node the position is directly inside (empty: the body).
    pub path: Path,
    /// `starts[d]` is where the content of the node at depth `d` of the path starts (`starts[0]`
    /// is the body's, 0). `starts.len() == path.len() + 1`.
    pub starts: Vec<usize>,
    /// The index of the child at or after the position in its parent.
    pub index: usize,
    /// Inside a text node at `index`: how far into it (UTF-16 units); 0 at a child boundary.
    pub text_offset: usize,
}

impl Resolved {
    pub fn depth(&self) -> usize {
        self.path.len()
    }

    /// The offset of the position inside its parent's content.
    pub fn parent_offset(&self) -> usize {
        self.pos - self.starts[self.path.len()]
    }

    /// Where the parent's content starts.
    pub fn start(&self) -> usize {
        self.starts[self.path.len()]
    }

    /// The parent node.
    pub fn parent<'a>(&self, root: &'a Node) -> Option<&'a Node> {
        node_at(root, &self.path)
    }

    /// The ancestor at `depth` (0 = the body).
    pub fn node<'a>(&self, root: &'a Node, depth: usize) -> Option<&'a Node> {
        node_at(root, &self.path[..depth.min(self.path.len())])
    }
}

/// Resolves a position. `None` past the end of the document.
pub fn resolve(root: &Node, pos: usize) -> Option<Resolved> {
    let mut path = Vec::new();
    let mut starts = vec![0usize];
    let mut node = root;
    let mut start = 0usize;
    loop {
        let offset = pos.checked_sub(start)?;
        let mut acc = 0usize;
        let mut found = None;
        for (i, child) in node.children().iter().enumerate() {
            if offset == acc {
                found = Some((i, 0usize, false));
                break;
            }
            let size = node_size(child);
            if offset < acc + size {
                if is_text(child) {
                    found = Some((i, offset - acc, false));
                } else if is_container(child) {
                    found = Some((i, acc, true));
                } else {
                    // Strictly inside a leaf of size 1 cannot happen; a bigger leaf does not exist.
                    found = Some((i, 0, false));
                }
                break;
            }
            acc += size;
        }
        match found {
            Some((i, child_acc, true)) => {
                path.push(i);
                start = start + child_acc + 1;
                starts.push(start);
                node = &node.children()[i];
            }
            Some((i, text_offset, false)) => {
                return Some(Resolved { pos, path, starts, index: i, text_offset });
            }
            None => {
                if offset == acc {
                    let index = node.children().len();
                    return Some(Resolved { pos, path, starts, index, text_offset: 0 });
                }
                return None;
            }
        }
    }
}

/// The textblock a position is in, as `(path, content start)`. `None` when the position is
/// between blocks.
pub fn textblock_at(root: &Node, pos: usize) -> Option<(Path, usize)> {
    let r = resolve(root, pos)?;
    let parent = r.parent(root)?;
    is_textblock(parent).then(|| (r.path.clone(), r.start()))
}

/// Every textblock in document order with its path and content start — the order the layout
/// lays them out in.
pub fn textblocks(root: &Node) -> Vec<(Path, usize)> {
    let mut out = Vec::new();
    fn walk(node: &Node, path: &mut Path, start: usize, out: &mut Vec<(Path, usize)>) {
        let mut pos = start;
        for (i, child) in node.children().iter().enumerate() {
            let size = node_size(child);
            if is_textblock(child) {
                path.push(i);
                out.push((path.clone(), pos + 1));
                path.pop();
            } else if is_container(child) {
                path.push(i);
                walk(child, path, pos + 1, out);
                path.pop();
            }
            pos += size;
        }
    }
    walk(root, &mut Vec::new(), 0, &mut out);
    out
}

/// The plain text between two positions, textblocks separated by `block_sep`, leaf atoms
/// rendered by `leaf` (ProseMirror's `textBetween`).
pub fn text_between(root: &Node, from: usize, to: usize, block_sep: &str, leaf: &dyn Fn(&Node) -> String) -> String {
    let mut out = String::new();
    let mut first_block = true;
    #[allow(clippy::too_many_arguments)]
    fn walk(
        node: &Node,
        start: usize,
        from: usize,
        to: usize,
        sep: &str,
        leaf: &dyn Fn(&Node) -> String,
        out: &mut String,
        first: &mut bool,
    ) {
        let mut pos = start;
        for child in node.children() {
            let size = node_size(child);
            let end = pos + size;
            if end <= from && size > 0 {
                pos = end;
                continue;
            }
            if pos >= to && !(size == 0 && pos == to) {
                break;
            }
            if is_text(child) {
                if let Some(text) = child.text() {
                    let a = from.saturating_sub(pos);
                    let b = (to - pos).min(size);
                    if let (Some(x), Some(y)) = (utf16_to_byte(&text, a), utf16_to_byte(&text, b)) {
                        out.push_str(&text[x..y]);
                    }
                }
            } else if is_container(child) {
                if is_textblock(child) {
                    if !*first {
                        out.push_str(sep);
                    }
                    *first = false;
                }
                walk(child, pos + 1, from, to, sep, leaf, out, first);
            } else if pos >= from && end <= to {
                out.push_str(&leaf(child));
            }
            pos = end;
        }
    }
    walk(root, 0, from, to, block_sep, leaf, &mut out, &mut first_block);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(json: &str) -> Node {
        Node::from_slice(json.as_bytes()).expect("fixture parses")
    }

    const TWO_PARAS: &str = r#"{"content":[{"content":[{"text":"hello","type":"text"}],"type":"paragraph"},{"content":[{"text":"ab","type":"text"},{"type":"hardBreak"},{"text":"c","type":"text"}],"type":"paragraph"}],"type":"doc"}"#;

    #[test]
    fn sizes_follow_prosemirror() {
        let d = doc(TWO_PARAS);
        // p(5) = 7, p(2 + 1 + 1) = 6
        assert_eq!(node_size(&d.children()[0]), 7);
        assert_eq!(node_size(&d.children()[1]), 6);
        assert_eq!(content_size(&d), 13);
    }

    #[test]
    fn an_empty_paragraph_is_two_whatever_its_content_key() {
        for p in [r#"{"type":"paragraph"}"#, r#"{"content":null,"type":"paragraph"}"#, r#"{"content":[],"type":"paragraph"}"#] {
            assert_eq!(node_size(&doc(p)), 2, "{p}");
        }
    }

    #[test]
    fn a_horizontal_rule_is_one_position_not_two() {
        // The web's parseDoc counts it as 2 (a bug); ProseMirror counts 1, and edits follow the model.
        assert_eq!(node_size(&doc(r#"{"type":"horizontalRule"}"#)), 1);
    }

    #[test]
    fn utf16_units_are_what_count() {
        let p = doc(r#"{"content":[{"text":"a😀b","type":"text"}],"type":"paragraph"}"#);
        assert_eq!(node_size(&p), 2 + 4);
    }

    #[test]
    fn resolving_inside_text_and_at_boundaries() {
        let d = doc(TWO_PARAS);
        // 0 = before p0, 1 = start of "hello", 3 = "he|llo", 6 = end of p0, 7 = between, 8 = start p1
        let r = resolve(&d, 3).expect("resolves");
        assert_eq!(r.path, vec![0]);
        assert_eq!(r.index, 0);
        assert_eq!(r.text_offset, 2);
        assert_eq!(r.parent_offset(), 2);
        let r = resolve(&d, 7).expect("resolves");
        assert!(r.path.is_empty());
        assert_eq!(r.index, 1);
        let r = resolve(&d, 10).expect("resolves"); // "ab|<br>c"
        assert_eq!(r.path, vec![1]);
        assert_eq!(r.index, 1);
        assert_eq!(r.parent_offset(), 2);
        assert_eq!(resolve(&d, 13).map(|r| r.index), Some(2));
        assert!(resolve(&d, 14).is_none());
    }

    #[test]
    fn textblocks_inside_lists_and_tables_are_found_in_order() {
        let d = doc(r#"{"content":[{"content":[{"content":[{"content":[{"text":"x","type":"text"}],"type":"paragraph"}],"type":"listItem"}],"type":"bulletList"},{"content":[{"content":[{"content":[{"type":"paragraph"}],"type":"tableCell"}],"type":"tableRow"}],"type":"table"}],"type":"doc"}"#);
        let tbs = textblocks(&d);
        assert_eq!(tbs.len(), 2);
        assert_eq!(tbs[0], (vec![0, 0, 0], 3));
        // list = 2 + item(2 + p(3)) = 7; table opens at 7, row 8, cell 9, p 10 → content 11
        assert_eq!(tbs[1], (vec![1, 0, 0, 0], 11));
        assert_eq!(textblock_at(&d, 3).map(|t| t.0), Some(vec![0, 0, 0]));
        assert_eq!(pos_before(&d, &[1, 0, 0, 0]), Some(10));
        assert_eq!(content_start(&d, &[1, 0, 0, 0]), Some(11));
    }

    #[test]
    fn text_between_joins_blocks() {
        let d = doc(TWO_PARAS);
        let t = text_between(&d, 0, 13, "\n", &|n| if n.node_type() == Some("hardBreak") { "\n".into() } else { String::new() });
        assert_eq!(t, "hello\nab\nc");
        assert_eq!(text_between(&d, 2, 4, "\n", &|_| String::new()), "el");
    }
}
