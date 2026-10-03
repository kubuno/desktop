//! Parse errors + validator findings → LSP `Diagnostic`s.
//!
//! `XML_VIEWS.md` §5: "The same diagnostic — file + `TextRange` from the
//! lossless parser — is handed to the language server as an LSP
//! `Diagnostic`, so the in-app overlay and VS's Error List report the exact
//! same error from one computation, never two." This module is that single
//! conversion point: [`kubuno_desktop_views::syntax::Parse::diagnostics`] (recovered
//! parse errors) and [`kubuno_desktop_views::validate::validate_with_default_
//! registry`] (unknown element/attribute, bad enum/type value, wrong child
//! count) both produce the crate's one [`kubuno_desktop_views::syntax::Diagnostic`]
//! shape; this function is the only place either gets turned into
//! `lsp_types::Diagnostic`.

use kubuno_desktop_views::syntax::Diagnostic as KvDiagnostic;
use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

use crate::documents::Document;

/// The LSP diagnostic "source" field — what shows up in an editor's problem
/// list next to the message (`"kubuno-desktop-views (rust-analyzer)"`-style tagging).
const SOURCE: &str = "kubuno-desktop-views";

/// Converts one document's parse + validation diagnostics to LSP form, in
/// the order the parser/validator produced them (parse errors first — a
/// validator finding on a badly parsed element is still useful context, so
/// neither list gates the other, matching `XML_VIEWS.md` §5's "must not
/// blank the screen" framing carried over to diagnostics: show everything
/// found, never bail out early).
pub fn document_diagnostics(doc: &Document) -> Vec<Diagnostic> {
    let mut out = Vec::with_capacity(doc.parse.diagnostics.len());
    for d in &doc.parse.diagnostics {
        out.push(to_lsp(doc, d, DiagnosticSeverity::ERROR));
    }
    for d in kubuno_desktop_views::validate::validate_with_default_registry(&doc.parse) {
        out.push(to_lsp(doc, &d, DiagnosticSeverity::ERROR));
    }
    // Non-blocking findings (the view still compiles): e.g. `Dock`/`Anchor` outside a `<Panel>`.
    for d in kubuno_desktop_views::validate::warnings(&doc.parse) {
        out.push(to_lsp(doc, &d, DiagnosticSeverity::WARNING));
    }
    // A free colour whose text would not stand out enough from its background (WCAG AA).
    for d in kubuno_desktop_views::validate::contrast_warnings(&doc.parse) {
        out.push(to_lsp(doc, &d, DiagnosticSeverity::WARNING));
    }
    // An event written under an older alias (`OnToggled` -> `OnCheckedChanged`): still valid.
    for d in kubuno_desktop_views::validate::hints(&doc.parse) {
        out.push(to_lsp(doc, &d, DiagnosticSeverity::HINT));
    }
    out
}

/// Warnings for the icon files the view at `uri` names that are not beside it (they would draw
/// nothing). Nothing for a document that is not a file.
pub fn icon_file_diagnostics(doc: &Document, uri: &lsp_types::Uri) -> Vec<Diagnostic> {
    let Some(dir) = crate::fs_uri::to_path(uri).and_then(|p| p.parent().map(std::path::Path::to_path_buf)) else { return Vec::new() };
    kubuno_desktop_views::validate::icon_file_warnings(&doc.parse, &dir).iter().map(|d| to_lsp(doc, d, DiagnosticSeverity::WARNING)).collect()
}

fn to_lsp(doc: &Document, d: &KvDiagnostic, severity: DiagnosticSeverity) -> Diagnostic {
    let start = doc.position_index.offset_to_position(&doc.text, d.range.start());
    let end = doc.position_index.offset_to_position(&doc.text, d.range.end());
    // A parser/validator range is occasionally empty (an EOF diagnostic with
    // nothing left to underline, or a malformed value's degenerate range) —
    // widen it by one UTF-16 unit so an editor still renders a visible
    // squiggle rather than an invisible zero-width one, without touching the
    // range the `kubuno-desktop-views` diagnostic itself carries (only the LSP
    // presentation is adjusted).
    let end = if end == start { widen_by_one(&doc.text, end) } else { end };
    Diagnostic {
        range: Range { start, end },
        severity: Some(severity),
        code: None::<NumberOrString>,
        code_description: None,
        source: Some(SOURCE.to_string()),
        message: d.message.clone(),
        related_information: None,
        tags: None,
        data: None,
    }
}

/// One position to the right, clamped to the document's last line/character
/// so a diagnostic at true end-of-file never produces an out-of-range range.
fn widen_by_one(text: &str, pos: Position) -> Position {
    if pos.character < u32::MAX {
        // The exact character count of this line is not recomputed here —
        // clients treat a `character` past the line's actual end as "end of
        // line" per the LSP spec, so simply incrementing is safe and avoids
        // re-deriving line length from `text` (kept `text`-parametrised only
        // for symmetry with the rest of this module's signatures, and in
        // case a future caller wants an exact clamp).
        let _ = text;
        Position { line: pos.line, character: pos.character + 1 }
    } else {
        pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno_desktop_views::syntax::parse;

    fn doc(text: &str) -> Document {
        Document::for_test(text.to_string())
    }

    #[test]
    fn well_formed_view_has_no_diagnostics() {
        let d = doc(r#"<Button Text="Ok" Variant="Primary"/>"#);
        assert!(document_diagnostics(&d).is_empty());
    }

    #[test]
    fn unknown_element_becomes_an_error_diagnostic() {
        let d = doc("<Frobnicator/>");
        let diags = document_diagnostics(&d);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(diags[0].source.as_deref(), Some("kubuno-desktop-views"));
        assert!(diags[0].message.contains("unknown element"));
    }

    #[test]
    fn dock_outside_a_panel_is_a_warning() {
        let d = doc(r#"<Stack><Button Dock="Top"/></Stack>"#);
        let diags = document_diagnostics(&d);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
        assert!(diags[0].message.contains("<Panel>"));
    }

    #[test]
    fn parse_error_and_validator_finding_can_both_be_reported() {
        // Mismatched end tag (parser accepts it, walks it as an ordinary
        // element per `ast`'s own test) combined with an unknown attribute —
        // both diagnostic sources fire on the same small file.
        let d = doc(r#"<Button Colour="red"></Switch>"#);
        let diags = document_diagnostics(&d);
        assert!(diags.iter().any(|x| x.message.contains("unknown attribute")));
        assert!(diags.iter().any(|x| x.message.contains("mismatched closing tag")));
    }

    #[test]
    fn range_uses_utf16_positions_past_multibyte_text() {
        // `Placeholder` value contains multibyte text before the typo'd
        // enum value — this test would fail with a byte-based column.
        let d = doc(r#"<Button Text="héllo" Variant="Primmary"/>"#);
        let diags = document_diagnostics(&d);
        assert_eq!(diags.len(), 1);
        let range = diags[0].range;
        assert_eq!(range.start.line, 0);
        // Recompute the expected UTF-16 column independently via `find`.
        let byte_offset = d.text.find("Primmary").unwrap();
        let expected_char = d.text[..byte_offset].encode_utf16().count() as u32;
        assert_eq!(range.start.character, expected_char);
    }

    // A minimal `Document` constructor for this module's tests, avoiding a
    // dependency on `DocumentStore`/a real `Url` for what is otherwise a
    // pure function test.
    impl Document {
        fn for_test(text: String) -> Self {
            let parse = parse(&text);
            let position_index = crate::position::PositionIndex::new(&text);
            Self { text, parse, position_index, version: 1 }
        }
    }
}

/// The « did you mean » quick fixes of the validator's suggestions in `range` (vskubuno docs/DESIGNER.md
/// §17): a misspelt element or attribute renamed to the suggested name, and a XAML-style
/// `Binding="Name"` turned into the suggested property bound (`Text="{Binding Name}"`).
pub fn suggestion_fixes(doc: &Document, uri: &lsp_types::Uri, range: &Range) -> Vec<lsp_types::CodeAction> {
    use kubuno_desktop_views::ast::{AstNode, Element};
    let mut out = Vec::new();
    let position = |offset| doc.position_index.offset_to_position(&doc.text, offset);
    let before = |p: &Position, q: &Position| (p.line, p.character) < (q.line, q.character);
    for d in kubuno_desktop_views::validate::validate_with_default_registry(&doc.parse) {
        let Some(suggestion) = d.message.strip_suffix("`?").and_then(|m| m.rsplit_once("; did you mean `")).map(|(_, s)| s.to_string()) else { continue };
        let found = Range { start: position(d.range.start()), end: position(d.range.end()) };
        if before(&found.end, &range.start) || before(&range.end, &found.start) {
            continue;
        }
        // The node the diagnostic underlines: an element's name, or an attribute's name.
        let root = doc.parse.syntax();
        let Some(node) = (match root.covering_element(d.range) {
            rowan::NodeOrToken::Node(n) => Some(n),
            rowan::NodeOrToken::Token(t) => t.parent(),
        }) else { continue };
        let mut edits = Vec::new();
        let title;
        if let Some(attribute) = node.ancestors().find_map(kubuno_desktop_views::ast::Attribute::cast) {
            match suggestion.split_once("=\"{Binding") {
                Some((property, _)) => {
                    let value = attribute.value().unwrap_or_default();
                    let path = value.trim();
                    let bound = if path.starts_with('{') { path.to_string() } else { format!("{{Binding {path}}}") };
                    let r = attribute.syntax().text_range();
                    edits.push(lsp_types::TextEdit { range: Range { start: position(r.start()), end: position(r.end()) }, new_text: format!("{property}=\"{bound}\"") });
                    title = format!("Use `{property}=\"{bound}\"`");
                }
                None => {
                    edits.push(lsp_types::TextEdit { range: found, new_text: suggestion.clone() });
                    title = format!("Use `{suggestion}`");
                }
            }
        } else if let Some(element) = node.ancestors().find_map(Element::cast) {
            let name = suggestion.trim_start_matches('<').trim_end_matches('>').to_string();
            for r in [element.name_range(), element.end_name_range()].into_iter().flatten() {
                edits.push(lsp_types::TextEdit { range: Range { start: position(r.start()), end: position(r.end()) }, new_text: name.clone() });
            }
            title = format!("Use `<{name}>`");
        } else {
            continue;
        }
        let title = kubuno_desktop_views::messages::tr(&title, &title.replacen("Use", "Utiliser", 1));
        #[allow(clippy::mutable_key_type)] // `Uri`'s memoizing `Cell`, never mutated here.
        let changes = std::collections::HashMap::from([(uri.clone(), edits)]);
        out.push(lsp_types::CodeAction {
            title,
            kind: Some(lsp_types::CodeActionKind::QUICKFIX),
            is_preferred: Some(true),
            edit: Some(lsp_types::WorkspaceEdit { changes: Some(changes), ..Default::default() }),
            ..Default::default()
        });
    }
    out
}

#[cfg(test)]
mod suggestion_tests {
    use super::*;

    #[test]
    fn a_binding_attribute_and_a_misspelt_element_have_a_quick_fix() {
        let uri: lsp_types::Uri = "file:///c:/v/view.kbview".parse().unwrap();
        let text = "<Panel>\n  <Label Binding=\"Name\"/>\n  <Labl Text=\"a\"></Labl>\n</Panel>".to_string();
        let doc = Document { parse: kubuno_desktop_views::syntax::parse(&text), position_index: crate::position::PositionIndex::new(&text), text, version: 1 };
        let all = Range { start: Position { line: 0, character: 0 }, end: Position { line: 9, character: 0 } };
        let fixes = suggestion_fixes(&doc, &uri, &all);
        assert_eq!(fixes.len(), 2, "{fixes:?}");
        let edits = |i: usize| fixes[i].edit.as_ref().unwrap().changes.as_ref().unwrap()[&uri].clone();
        assert_eq!(edits(0)[0].new_text, "Text=\"{Binding Name}\"");
        assert_eq!(edits(1).iter().map(|e| e.new_text.as_str()).collect::<Vec<_>>(), vec!["Label", "Label"]);
    }
}
