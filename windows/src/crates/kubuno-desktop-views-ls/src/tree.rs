//! Small shared helpers for walking the `kubuno_desktop_views::syntax` tree at a
//! cursor offset — used by [`crate::hover`], [`crate::completion`] and
//! [`crate::definition`], which all start from "what token is under (or
//! right next to) the cursor, and what does it belong to".

use kubuno_desktop_views::ast::{AstNode, Attribute, Element};
use kubuno_desktop_views::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::{TextSize, TokenAtOffset};

/// The token at `offset`, preferring the non-trivia side when `offset` sits
/// exactly between two tokens (e.g. right after `<`, where the left token is
/// `L_ANGLE` and there is no right token yet — `Single` — or right after a
/// space, where `Between(WHITESPACE, next)` should resolve to `next`, the
/// token the user is about to type into). When both sides are non-trivia,
/// the left one wins — the conventional "what did the user just finish
/// typing" reading a cursor position implies.
pub fn token_at_offset(root: &SyntaxNode, offset: TextSize) -> Option<SyntaxToken> {
    match root.token_at_offset(offset) {
        TokenAtOffset::None => None,
        TokenAtOffset::Single(t) => Some(t),
        TokenAtOffset::Between(l, r) => {
            if l.kind() == SyntaxKind::WHITESPACE && r.kind() != SyntaxKind::WHITESPACE {
                Some(r)
            } else {
                Some(l)
            }
        }
    }
}

/// The nearest ancestor [`Element`] of `node` (inclusive: `node` itself if it
/// already is one), or `None` at the document root.
pub fn enclosing_element(node: &SyntaxNode) -> Option<Element> {
    node.ancestors().find_map(Element::cast)
}

/// The nearest ancestor [`Attribute`] of `node` (inclusive), when the cursor
/// is anywhere inside one (its name, its `=`, or its quoted value).
pub fn enclosing_attribute(node: &SyntaxNode) -> Option<Attribute> {
    node.ancestors().find_map(Attribute::cast)
}

/// Whether `token` is the tag-name `IDENT` of a `START_TAG` — i.e. the
/// element's own name, as opposed to an attribute name (also an `IDENT`, but
/// wrapped in an `ATTRIBUTE` node) or an end-tag name.
pub fn is_start_tag_name(token: &SyntaxToken) -> bool {
    token.kind() == SyntaxKind::IDENT
        && token.parent().is_some_and(|p| p.kind() == SyntaxKind::START_TAG)
}

/// Whether `token` is an attribute's name `IDENT` (wrapped in an
/// `ATTRIBUTE` node — distinct from [`is_start_tag_name`]'s bare
/// `START_TAG`-child `IDENT`).
pub fn is_attribute_name(token: &SyntaxToken) -> bool {
    token.kind() == SyntaxKind::IDENT && token.parent().is_some_and(|p| p.kind() == SyntaxKind::ATTRIBUTE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno_desktop_views::ast::Document;
    use kubuno_desktop_views::syntax::parse;

    #[test]
    fn token_at_offset_between_prefers_the_token_being_typed_into() {
        let p = parse(r#"<Button Text="Ok"/>"#);
        let root = p.syntax();
        // Offset right after "<Button" (before the space and "Text") sits
        // between the "Button" IDENT and WHITESPACE — the left, non-trivia
        // token should win.
        let offset = TextSize::from(7); // "<Button".len()
        let t = token_at_offset(&root, offset).expect("a token");
        assert_eq!(t.kind(), SyntaxKind::IDENT);
        assert_eq!(t.text(), "Button");

        // Offset right after the following space, before "Text", sits
        // between WHITESPACE and the "Text" IDENT — should resolve to
        // "Text", the token about to be typed/completed.
        let offset2 = TextSize::from(8); // "<Button ".len()
        let t2 = token_at_offset(&root, offset2).expect("a token");
        assert_eq!(t2.kind(), SyntaxKind::IDENT);
        assert_eq!(t2.text(), "Text");
    }

    #[test]
    fn is_start_tag_name_true_for_the_element_name() {
        let p = parse(r#"<Button Text="Ok"/>"#);
        let root = p.syntax();
        let doc = Document::cast(root.clone()).unwrap();
        let el = doc.root_element().unwrap();
        let name_tok = el.name_token().unwrap();
        assert!(is_start_tag_name(&name_tok));
        assert!(!is_attribute_name(&name_tok));
    }

    #[test]
    fn is_attribute_name_true_for_an_attribute_ident() {
        let p = parse(r#"<Button Text="Ok"/>"#);
        let root = p.syntax();
        let doc = Document::cast(root).unwrap();
        let el = doc.root_element().unwrap();
        let attr = el.attribute("Text").unwrap();
        let name_tok = attr
            .syntax()
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .find(|t| t.kind() == SyntaxKind::IDENT)
            .unwrap();
        assert!(is_attribute_name(&name_tok));
        assert!(!is_start_tag_name(&name_tok));
    }

    #[test]
    fn enclosing_element_finds_the_nearest_ancestor() {
        let p = parse(r#"<Stack><Button Text="Ok"/></Stack>"#);
        let root = p.syntax();
        let doc = Document::cast(root).unwrap();
        let stack = doc.root_element().unwrap();
        let button = stack.children().next().unwrap();
        let found = enclosing_element(button.syntax()).unwrap();
        assert_eq!(found.name().as_deref(), Some("Button"));
    }
}
