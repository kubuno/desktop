//! `textDocument/documentSymbol` — the element tree, `x:Name` as the symbol
//! name where present (§1: `x:Name` is the one stable identity a `.kbview`
//! element carries), the tag name otherwise.

use kubuno_desktop_views::ast::{AstNode, Document, Element};
use lsp_types::{DocumentSymbol, Range, SymbolKind};

use crate::documents::Document as OpenDocument;

/// The full symbol tree for `doc`'s current parse — `None` when the document
/// has no root element at all (an empty file, or one that failed to parse
/// even that far; `kubuno_desktop_views::syntax::parse` still returns *a* tree, but
/// [`Document::root_element`] is `None` in that case).
pub fn document_symbols(doc: &OpenDocument) -> Vec<DocumentSymbol> {
    let root = doc.parse.syntax();
    let Some(ast_doc) = Document::cast(root) else { return Vec::new() };
    match ast_doc.root_element() {
        Some(el) => vec![element_symbol(&el, doc)],
        None => Vec::new(),
    }
}

#[allow(deprecated)] // `DocumentSymbol::deprecated` has no replacement field yet in lsp-types 0.97.
fn element_symbol(el: &Element, doc: &OpenDocument) -> DocumentSymbol {
    let tag_name = el.name().unwrap_or_else(|| "<anonymous>".to_string());
    let x_name = el.attribute("x:Name").and_then(|a| a.value());
    let name = x_name.clone().unwrap_or_else(|| tag_name.clone());
    let detail = x_name.map(|_| tag_name.clone()); // When named, show the tag as the detail (e.g. name "proxy", detail "TextField").

    let full_range = to_range(doc, el.syntax().text_range());
    // The "selection range" is what an editor highlights when you click the
    // symbol in an outline view — the element's own name token (in the start
    // tag), not the whole subtree, exactly like `ast::Element::name_range`
    // already hands the validator for its "unknown element" diagnostics.
    let selection_range =
        el.name_range().map(|r| to_range(doc, r)).unwrap_or(full_range);

    let children: Vec<DocumentSymbol> = el.children().map(|child| element_symbol(&child, doc)).collect();

    DocumentSymbol {
        name,
        detail,
        kind: SymbolKind::OBJECT,
        tags: None,
        deprecated: None,
        range: full_range,
        selection_range,
        children: if children.is_empty() { None } else { Some(children) },
    }
}

fn to_range(doc: &OpenDocument, range: rowan::TextRange) -> Range {
    Range {
        start: doc.position_index.offset_to_position(&doc.text, range.start()),
        end: doc.position_index.offset_to_position(&doc.text, range.end()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno_desktop_views::syntax::parse;

    fn open(text: &str) -> OpenDocument {
        OpenDocument {
            position_index: crate::position::PositionIndex::new(text),
            parse: parse(text),
            text: text.to_string(),
            version: 1,
        }
    }

    #[test]
    fn root_element_becomes_a_symbol() {
        let d = open(r#"<Card Title="x"><Button Text="Ok"/></Card>"#);
        let syms = document_symbols(&d);
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "Card");
    }

    #[test]
    fn x_name_is_used_as_the_symbol_name_and_tag_as_detail() {
        let d = open(r#"<TextField x:Name="proxy" Text="hi"/>"#);
        let syms = document_symbols(&d);
        assert_eq!(syms[0].name, "proxy");
        assert_eq!(syms[0].detail.as_deref(), Some("TextField"));
    }

    #[test]
    fn children_are_nested() {
        let d = open(r#"<Stack><Button Text="a"/><Switch x:Name="s"/></Stack>"#);
        let syms = document_symbols(&d);
        let children = syms[0].children.as_ref().expect("stack has children");
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].name, "Button");
        assert_eq!(children[1].name, "s");
    }

    #[test]
    fn leaf_element_has_no_children_list() {
        let d = open(r#"<Button Text="Ok"/>"#);
        let syms = document_symbols(&d);
        assert!(syms[0].children.is_none());
    }

    #[test]
    fn empty_document_has_no_symbols() {
        let d = open("");
        assert!(document_symbols(&d).is_empty());
    }
}
