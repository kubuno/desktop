//! The namespace declarations of a view (`xmlns`, `xmlns:x`, `xmlns:d`, vskubuno `docs/VIEWS-SPEC.md`
//! §3): an information diagnostic on a prefix the root element does not declare (Kubuno's tools never
//! need the declarations, but a generic XML editor reports an undeclared prefix as an error), its
//! quick fix « Add missing namespace declarations », and the completion of the declarations and of
//! their standard URIs on the root element.

use kubuno_desktop_views::ast::{AstNode, Attribute, Element};
use kubuno_desktop_views_syntax::namespaces::{self, UndeclaredPrefix};
use lsp_types::{CodeAction, CodeActionKind, CompletionItem, CompletionItemKind, Diagnostic, DiagnosticSeverity, NumberOrString, Range, TextEdit, Uri, WorkspaceEdit};

use crate::documents::Document;

/// The diagnostic code of an undeclared prefix.
pub const DIAGNOSTIC_CODE: &str = "undeclared-prefix";

/// The English message of an undeclared `prefix` (localized on its way out, `kubuno_desktop_views::messages`).
pub fn message(prefix: &str) -> String {
    let declaration = format!("{}=\"{}\"", namespaces::declaration_attribute(prefix), namespaces::standard_namespace(prefix).unwrap_or_default());
    format!("namespace prefix `{prefix}` is not declared: add `{declaration}` to the root element")
}

fn range_of(doc: &Document, r: rowan::TextRange) -> Range {
    Range { start: doc.position_index.offset_to_position(&doc.text, r.start()), end: doc.position_index.offset_to_position(&doc.text, r.end()) }
}

/// One information diagnostic per undeclared prefix, on its first use.
pub fn diagnostics(doc: &Document) -> Vec<Diagnostic> {
    namespaces::undeclared_prefixes(&doc.parse).iter().map(|u| diagnostic(doc, u)).collect()
}

fn diagnostic(doc: &Document, u: &UndeclaredPrefix) -> Diagnostic {
    Diagnostic {
        range: range_of(doc, u.first_use),
        severity: Some(DiagnosticSeverity::INFORMATION),
        code: Some(NumberOrString::String(DIAGNOSTIC_CODE.to_string())),
        source: Some("kubuno-desktop-views".to_string()),
        message: message(u.prefix),
        ..Default::default()
    }
}

/// « Add missing namespace declarations » when `range` touches an attribute written with an
/// undeclared prefix or the root element's name.
pub fn quick_fixes(doc: &Document, uri: &Uri, range: &Range) -> Vec<CodeAction> {
    let undeclared = namespaces::undeclared_prefixes(&doc.parse);
    if undeclared.is_empty() {
        return Vec::new();
    }
    let Some((at, text)) = namespaces::missing_declarations(&doc.parse, false) else { return Vec::new() };
    let overlaps = |r: &Range| !((r.end.line, r.end.character) < (range.start.line, range.start.character) || (range.end.line, range.end.character) < (r.start.line, r.start.character));
    let root = doc.parse.syntax();
    // On the root element's name (where the declarations go), or on any attribute written with the prefix.
    let on_root = kubuno_desktop_views::ast::Document::cast(root.clone()).and_then(|d| d.root_element()).and_then(|e| e.name_range()).is_some_and(|r| overlaps(&range_of(doc, r)));
    let on_use = on_root
        || root.descendants().filter_map(Element::cast).flat_map(|e| e.attributes().collect::<Vec<Attribute>>()).any(|a| {
            let (Some(name), Some(r)) = (a.name(), a.name_range()) else { return false };
            undeclared.iter().any(|u| name.strip_prefix(u.prefix).is_some_and(|rest| rest.starts_with(':'))) && overlaps(&range_of(doc, r))
        });
    if !on_use {
        return Vec::new();
    }
    let position = doc.position_index.offset_to_position(&doc.text, at);
    #[allow(clippy::mutable_key_type)] // `Uri`'s memoizing `Cell`, never mutated here.
    let changes = std::collections::HashMap::from([(uri.clone(), vec![TextEdit { range: Range { start: position, end: position }, new_text: text }])]);
    vec![CodeAction {
        title: kubuno_desktop_views::messages::tr("Add missing namespace declarations", "Ajouter les déclarations d'espaces de noms manquantes"),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(undeclared.iter().map(|u| diagnostic(doc, u)).collect()),
        is_preferred: Some(true),
        edit: Some(WorkspaceEdit { changes: Some(changes), ..Default::default() }),
        ..Default::default()
    }]
}

/// The declarations the root element does not carry yet, as attribute-name completion items.
pub fn attribute_items(element: &Element) -> Vec<CompletionItem> {
    let is_root = element.syntax().parent().is_none_or(|p| p.kind() != kubuno_desktop_views::syntax::SyntaxKind::ELEMENT);
    if !is_root {
        return Vec::new();
    }
    let doc = |prefix: &str| match prefix {
        "" => "The components' namespace (Kubuno views). Optional for Kubuno's tools; lets any XML editor read the view.",
        "x" => "The namespace of the `x:` directives (`x:Name`, `x:Inherits`). Without it, an XML editor reports every `x:` as an undeclared prefix.",
        _ => "The namespace of the `d:` design-time attributes (`d:Text`, `d:Visible`), read by the designer only.",
    };
    ["", namespaces::DIRECTIVE_PREFIX, namespaces::DESIGN_PREFIX]
        .into_iter()
        .filter(|p| element.attribute(&namespaces::declaration_attribute(p)).is_none())
        .map(|p| {
            let name = namespaces::declaration_attribute(p);
            let uri = namespaces::standard_namespace(p).unwrap_or_default();
            CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some(uri.to_string()),
                documentation: Some(lsp_types::Documentation::String(doc(p).to_string())),
                insert_text: Some(format!("{name}=\"{uri}\"")),
                sort_text: Some(format!("~{name}")),
                ..Default::default()
            }
        })
        .collect()
}

/// The standard URI of a declaration being written (`xmlns:x="|"`), as value completion items.
pub fn value_items(attribute: &str) -> Option<Vec<CompletionItem>> {
    if !namespaces::is_declaration(attribute) {
        return None;
    }
    let prefix = attribute.strip_prefix("xmlns").map(|p| p.trim_start_matches(':')).unwrap_or_default();
    Some(namespaces::standard_namespace(prefix).map(|uri| vec![CompletionItem { label: uri.to_string(), kind: Some(CompletionItemKind::VALUE), ..Default::default() }]).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::Position;

    fn doc(text: &str) -> Document {
        Document { parse: kubuno_desktop_views::syntax::parse(text), position_index: crate::position::PositionIndex::new(text), text: text.to_string(), version: 1 }
    }

    fn whole() -> Range {
        Range { start: Position { line: 0, character: 0 }, end: Position { line: 99, character: 0 } }
    }

    #[test]
    fn an_undeclared_x_is_one_information_diagnostic_with_a_fix() {
        let uri: Uri = "file:///c:/v/view.kbview".parse().unwrap();
        let d = doc("<Panel>\n  <Button x:Name=\"a\"/>\n  <Button x:Name=\"b\"/>\n</Panel>");
        let diags = diagnostics(&d);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::INFORMATION));
        assert_eq!(diags[0].range.start, Position { line: 1, character: 10 });
        assert!(diags[0].message.contains("`x`") && diags[0].message.contains(namespaces::DIRECTIVE_NAMESPACE), "{}", diags[0].message);
        // Offered from the second use too, not only the underlined first one.
        let caret = Range { start: Position { line: 2, character: 12 }, end: Position { line: 2, character: 12 } };
        let fixes = quick_fixes(&d, &uri, &caret);
        assert_eq!(fixes.len(), 1);
        let edits = &fixes[0].edit.as_ref().unwrap().changes.as_ref().unwrap()[&uri];
        assert_eq!(edits[0].range.start, Position { line: 0, character: 6 });
        assert_eq!(edits[0].new_text, format!(" xmlns=\"{}\" xmlns:x=\"{}\"", namespaces::VIEWS_NAMESPACE, namespaces::DIRECTIVE_NAMESPACE));
        // Far from any use and from the root line: nothing.
        let elsewhere = Range { start: Position { line: 3, character: 0 }, end: Position { line: 3, character: 0 } };
        assert!(quick_fixes(&d, &uri, &elsewhere).is_empty());
    }

    #[test]
    fn a_declared_or_prefix_free_view_is_silent() {
        let uri: Uri = "file:///c:/v/view.kbview".parse().unwrap();
        for text in [
            format!("<Panel xmlns=\"{}\" xmlns:x=\"{}\"><Button x:Name=\"a\"/></Panel>", namespaces::VIEWS_NAMESPACE, namespaces::DIRECTIVE_NAMESPACE),
            "<Panel><Button Text=\"a\"/></Panel>".to_string(),
        ] {
            let d = doc(&text);
            assert!(diagnostics(&d).is_empty(), "{text}");
            assert!(quick_fixes(&d, &uri, &whole()).is_empty(), "{text}");
            assert!(crate::diagnostics::document_diagnostics(&d).is_empty(), "declarations are never unknown attributes: {text}");
        }
    }

    #[test]
    fn declarations_and_their_uris_are_completed_on_the_root_only() {
        let d = doc(r#"<Panel xmlns:x="u"><Button/></Panel>"#);
        let root = kubuno_desktop_views::ast::Document::cast(d.parse.syntax()).unwrap().root_element().unwrap();
        let labels: Vec<String> = attribute_items(&root).into_iter().map(|i| i.label).collect();
        assert_eq!(labels, vec!["xmlns", "xmlns:d"]);
        let child = root.children().next().unwrap();
        assert!(attribute_items(&child).is_empty());
        assert_eq!(value_items("xmlns:x").unwrap()[0].label, namespaces::DIRECTIVE_NAMESPACE);
        assert!(value_items("Text").is_none());
    }
}
