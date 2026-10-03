//! A thin typed layer over the untyped [`crate::syntax`] tree — "Element,
//! Attribute, …" per the phase 2a brief, the same `rust-analyzer`/`taplo`
//! pattern `XML_VIEWS.md` §6 points at: the interpreter (2c), the validator
//! (`kubuno_desktop_views::validate`) and, later, the language server (§8's phase 3) all
//! read views through this layer rather than walking raw [`SyntaxNode`]s.

use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::TextRange;

/// A typed wrapper around one [`SyntaxNode`] of a known [`SyntaxKind`].
pub trait AstNode: Sized {
    fn can_cast(kind: SyntaxKind) -> bool;
    fn cast(node: SyntaxNode) -> Option<Self>;
    fn syntax(&self) -> &SyntaxNode;
}

macro_rules! ast_node {
    ($name:ident, $kind:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(SyntaxNode);

        impl AstNode for $name {
            fn can_cast(kind: SyntaxKind) -> bool {
                kind == $kind
            }

            fn cast(node: SyntaxNode) -> Option<Self> {
                Self::can_cast(node.kind()).then_some(Self(node))
            }

            fn syntax(&self) -> &SyntaxNode {
                &self.0
            }
        }
    };
}

ast_node!(Document, SyntaxKind::DOCUMENT);
ast_node!(Element, SyntaxKind::ELEMENT);
ast_node!(Attribute, SyntaxKind::ATTRIBUTE);
ast_node!(Prolog, SyntaxKind::PROLOG);

impl Document {
    /// The single root element, if the file has one (a document that failed
    /// to parse even that far has none — see [`crate::syntax::Parse`]'s own
    /// diagnostics for why).
    pub fn root_element(&self) -> Option<Element> {
        self.0.children().find_map(Element::cast)
    }

    pub fn prolog(&self) -> Option<Prolog> {
        self.0.children().find_map(Prolog::cast)
    }

    /// The inverse of [`Element::stable_id`]: walks `id`'s dot-separated
    /// ordinal path from this document's root element. `None` for a
    /// malformed id (a non-numeric segment), an out-of-range index at any
    /// level, or a document with no root element at all — never a panic, so
    /// a caller (e.g. the language server's `kubuno/applyEdit` bridge,
    /// `vskubuno/docs/DESIGNER.md`'s "DSG-2 protocol") can treat a stale or
    /// bogus id from a client as a plain no-op.
    pub fn resolve_id(&self, id: &str) -> Option<Element> {
        let mut current = self.root_element()?;
        if id.is_empty() {
            return Some(current);
        }
        for part in id.split('.') {
            let index: usize = part.parse().ok()?;
            current = current.children().nth(index)?;
        }
        Some(current)
    }
}

impl Element {
    /// The `START_TAG` child node — `pub(crate)` because only [`crate::edit`]
    /// needs to reach past the typed accessors below (e.g. to find the
    /// tag's closing token as an insertion point).
    pub(crate) fn start_tag(&self) -> Option<SyntaxNode> {
        self.0.children().find(|n| n.kind() == SyntaxKind::START_TAG)
    }

    /// The `END_TAG` child node, `None` when the element self-closes.
    pub(crate) fn end_tag(&self) -> Option<SyntaxNode> {
        self.0.children().find(|n| n.kind() == SyntaxKind::END_TAG)
    }

    /// The element name token in the start tag — the first [`SyntaxKind::IDENT`]
    /// child.
    pub fn name_token(&self) -> Option<SyntaxToken> {
        self.start_tag()?.children_with_tokens().filter_map(|e| e.into_token()).find(|t| t.kind() == SyntaxKind::IDENT)
    }

    pub fn name(&self) -> Option<String> {
        self.name_token().map(|t| t.text().to_string())
    }

    /// The end tag's own name token, when there is an end tag — used to
    /// check a `<Foo>…</Bar>` mismatch, which the parser accepts
    /// syntactically (error tolerance) but a validator should flag.
    pub fn end_name_token(&self) -> Option<SyntaxToken> {
        self.end_tag()?.children_with_tokens().filter_map(|e| e.into_token()).find(|t| t.kind() == SyntaxKind::IDENT)
    }

    pub fn is_self_closing(&self) -> bool {
        self.end_tag().is_none()
    }

    pub fn attributes(&self) -> impl Iterator<Item = Attribute> {
        self.start_tag().into_iter().flat_map(|tag| tag.children().filter_map(Attribute::cast))
    }

    pub fn attribute(&self, name: &str) -> Option<Attribute> {
        self.attributes().find(|a| a.name().as_deref() == Some(name))
    }

    /// Direct child elements, in document order. Does not recurse — a
    /// grandchild is reached through its own parent's `children()`.
    pub fn children(&self) -> impl Iterator<Item = Element> {
        self.0.children().filter_map(Element::cast)
    }

    /// The concatenation of every direct `TEXT`/`CDATA` token's raw text
    /// (no entity decoding, no trimming — callers that want "is this
    /// element's body just whitespace" trim themselves).
    pub fn text(&self) -> String {
        self.0
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .filter(|t| matches!(t.kind(), SyntaxKind::TEXT | SyntaxKind::CDATA))
            .map(|t| t.text().to_string())
            .collect()
    }

    /// The byte range of the name in the *opening* tag — what a
    /// "unknown element" diagnostic should underline.
    pub fn name_range(&self) -> Option<TextRange> {
        self.name_token().map(|t| t.text_range())
    }

    /// The byte range of the name in the *closing* tag, `None` when the
    /// element self-closes (mirrors [`Self::name_range`] for
    /// [`crate::edit::rename_element`], which needs to rewrite both tags at
    /// once).
    pub fn end_name_range(&self) -> Option<TextRange> {
        self.end_name_token().map(|t| t.text_range())
    }

    /// The element's **stable id**: a dot-separated path of child-ordinal
    /// indices from the document root, counting element children only (text/
    /// comments/attributes never consume an index) — independent of
    /// `x:Name`, per `vskubuno/docs/DESIGNER.md`'s cross-cutting note ("DSG-2
    /// and DSG-6 must agree on one stable element-id scheme … a path of
    /// child-ordinal indices from the document root, independent of
    /// `x:Name`"). The document's root element is the empty path `""`; its
    /// first child is `"0"`, that child's second child is `"0.1"`, and so on.
    /// [`Document::resolve_id`] is the exact inverse.
    ///
    /// Stable across edits that do not reorder/insert/remove elements above
    /// this one in the tree — good enough for the designer's selection/drag
    /// state across the debounced re-parses `DESIGNER.md` §2 describes,
    /// without needing every node to carry an `x:Name`.
    pub fn stable_id(&self) -> String {
        let mut indices = Vec::new();
        let mut node = self.0.clone();
        // Only a step whose *parent* is itself an `ELEMENT` counts as an
        // ordinal in the path — the outermost step (an element's parent is
        // the `DOCUMENT` node, not another element) must not contribute an
        // index, or the root element itself would wrongly get `"0"` instead
        // of the empty path.
        while let Some(parent) = node.parent() {
            if parent.kind() == SyntaxKind::ELEMENT {
                if let Some(index) =
                    parent.children().filter(|c| c.kind() == SyntaxKind::ELEMENT).position(|c| c == node)
                {
                    indices.push(index);
                }
            }
            node = parent;
        }
        indices.reverse();
        indices.iter().map(usize::to_string).collect::<Vec<_>>().join(".")
    }
}

impl Attribute {
    fn name_token(&self) -> Option<SyntaxToken> {
        self.0.children_with_tokens().filter_map(|e| e.into_token()).find(|t| t.kind() == SyntaxKind::IDENT)
    }

    pub fn name(&self) -> Option<String> {
        self.name_token().map(|t| t.text().to_string())
    }

    pub fn name_range(&self) -> Option<TextRange> {
        self.name_token().map(|t| t.text_range())
    }

    fn value_token(&self) -> Option<SyntaxToken> {
        self.0.children_with_tokens().filter_map(|e| e.into_token()).find(|t| t.kind() == SyntaxKind::STRING)
    }

    /// The value's raw token text, quotes included — `None` when the
    /// attribute has no `="value"` part at all (a parse error already
    /// recorded it; see [`crate::syntax::Parse::diagnostics`]).
    pub fn raw_value(&self) -> Option<String> {
        self.value_token().map(|t| t.text().to_string())
    }

    /// The value with its surrounding quotes stripped and its XML character references decoded
    /// (`&amp;` → `&`, `&#65;` → `A`; an unknown or unterminated `&…` is kept as written) — `None` when the raw
    /// value is missing or malformed (e.g. an unterminated string, whose
    /// token does not end with the quote it started with).
    pub fn value(&self) -> Option<String> {
        let raw = self.raw_value()?;
        let mut chars = raw.chars();
        let quote = chars.next()?;
        if (quote != '"' && quote != '\'') || !raw.ends_with(quote) || raw.len() < 2 {
            return None;
        }
        Some(decode_entities(&raw[quote.len_utf8()..raw.len() - quote.len_utf8()]))
    }

    /// The value's byte range *inside* the quotes — what a "not a valid
    /// value for this enum" diagnostic should underline.
    pub fn value_range(&self) -> Option<TextRange> {
        let t = self.value_token()?;
        let r = t.text_range();
        let text = t.text();
        if text.len() < 2 {
            return Some(r); // Malformed (e.g. a lone quote) — underline it whole.
        }
        let start = r.start() + rowan::TextSize::from(1);
        let end = r.end() - rowan::TextSize::from(1);
        if start > end {
            Some(r)
        } else {
            Some(TextRange::new(start, end))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::parse;

    #[test]
    fn reads_name_and_attributes() {
        let p = parse(r#"<Button Text="Ok" Dock="Left"/>"#);
        let doc = Document::cast(p.syntax()).unwrap();
        let root = doc.root_element().unwrap();
        assert_eq!(root.name().as_deref(), Some("Button"));
        assert!(root.is_self_closing());
        assert_eq!(root.attribute("Text").and_then(|a| a.value()), Some("Ok".to_string()));
        assert_eq!(root.attribute("Dock").and_then(|a| a.value()), Some("Left".to_string()));
        assert!(root.attribute("Missing").is_none());
    }

    #[test]
    fn reads_children_and_text() {
        let p = parse("<Panel><Label>hi</Label><Label>there</Label></Panel>");
        let doc = Document::cast(p.syntax()).unwrap();
        let root = doc.root_element().unwrap();
        let kids: Vec<_> = root.children().collect();
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[0].text(), "hi");
        assert_eq!(kids[1].text(), "there");
    }

    #[test]
    fn namespaced_attribute_name_round_trips() {
        let p = parse(r#"<View x:Name="root"/>"#);
        let doc = Document::cast(p.syntax()).unwrap();
        let root = doc.root_element().unwrap();
        assert_eq!(root.attribute("x:Name").and_then(|a| a.value()), Some("root".to_string()));
    }

    #[test]
    fn end_tag_mismatch_is_visible_to_ast() {
        let p = parse("<Foo></Bar>");
        let doc = Document::cast(p.syntax()).unwrap();
        let root = doc.root_element().unwrap();
        assert_eq!(root.name().as_deref(), Some("Foo"));
        assert_eq!(root.end_name_token().map(|t| t.text().to_string()), Some("Bar".to_string()));
    }

    #[test]
    fn root_element_stable_id_is_the_empty_path() {
        let p = parse(r#"<Stack><A/><B/></Stack>"#);
        let doc = Document::cast(p.syntax()).unwrap();
        assert_eq!(doc.root_element().unwrap().stable_id(), "");
    }

    #[test]
    fn nested_children_get_dot_separated_ordinal_ids() {
        let p = parse(r#"<Stack><Panel><A/><B/></Panel><C/></Stack>"#);
        let doc = Document::cast(p.syntax()).unwrap();
        let stack = doc.root_element().unwrap();
        let mut top = stack.children();
        let panel = top.next().unwrap();
        let c = top.next().unwrap();
        assert_eq!(panel.stable_id(), "0");
        assert_eq!(c.stable_id(), "1");
        let mut panel_children = panel.children();
        let a = panel_children.next().unwrap();
        let b = panel_children.next().unwrap();
        assert_eq!(a.stable_id(), "0.0");
        assert_eq!(b.stable_id(), "0.1");
    }

    #[test]
    fn resolve_id_is_the_inverse_of_stable_id() {
        let p = parse(r#"<Stack><Panel><A/><B/></Panel><C/></Stack>"#);
        let doc = Document::cast(p.syntax()).unwrap();
        let stack = doc.root_element().unwrap();
        for element in [
            stack.clone(),
            stack.children().next().unwrap(),
            stack.children().nth(1).unwrap(),
            stack.children().next().unwrap().children().next().unwrap(),
            stack.children().next().unwrap().children().nth(1).unwrap(),
        ] {
            let id = element.stable_id();
            let resolved = doc.resolve_id(&id).unwrap_or_else(|| panic!("id {id:?} should resolve"));
            assert_eq!(resolved.name(), element.name(), "id {id:?}");
            assert_eq!(resolved.syntax().text_range(), element.syntax().text_range(), "id {id:?}");
        }
    }

    #[test]
    fn resolve_id_rejects_out_of_range_and_malformed_ids() {
        let p = parse(r#"<Stack><A/></Stack>"#);
        let doc = Document::cast(p.syntax()).unwrap();
        assert!(doc.resolve_id("5").is_none());
        assert!(doc.resolve_id("not-a-number").is_none());
        assert!(doc.resolve_id("0.0").is_none()); // `A` has no children of its own.
    }

    #[test]
    fn resolve_id_on_a_document_with_no_root_element_is_none() {
        let p = parse("");
        let doc = Document::cast(p.syntax()).unwrap();
        assert!(doc.resolve_id("").is_none());
    }
}

/// Decodes the XML character references of an attribute value: the five predefined entities and
/// numeric references. Anything else starting with `&` (a bare ampersand, an unknown name) is kept.
pub fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let decoded = tail.find(';').filter(|end| *end <= 12).and_then(|end| {
            let name = &tail[1..end];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => {
                    let code = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        name.strip_prefix('#').and_then(|d| d.parse::<u32>().ok())
                    };
                    code.and_then(char::from_u32)
                }
            };
            c.map(|c| (c, end + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &tail[len..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod entity_tests {
    #[test]
    fn attribute_values_decode_their_character_references() {
        assert_eq!(super::decode_entities("&amp;Save &lt;b&gt; &quot;x&quot; &apos; &#65;&#x42;"), "&Save <b> \"x\" ' AB");
        assert_eq!(super::decode_entities("&Save && R&D &unknown; &"), "&Save && R&D &unknown; &");
    }
}
