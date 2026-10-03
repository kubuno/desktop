//! `kubuno/applyEdit`, `kubuno/elementAtOffset` and `kubuno/rangeOfElement` —
//! the three custom, non-`textDocument/*` methods the visual designer needs
//! (`vskubuno/docs/DESIGNER.md`, work package DSG-2). See that document's
//! "DSG-2 protocol" section (appended by this same change) for the wire
//! shapes this module's types serialize to/from.
//!
//! Every element the designer names is addressed by its **stable id**
//! ([`kubuno_views::ast::Element::stable_id`]) — a dot-separated path of
//! child-ordinal indices from the document root, independent of `x:Name` —
//! rather than by a live tree reference: `DESIGNER.md`'s cross-cutting note
//! asks DSG-2 to settle exactly this scheme so DSG-6 (a separate process,
//! `kubuno-views-designer`, hit-testing its own copy of the same text) can
//! independently compute the same id and agree with this server on "which
//! element". [`kubuno_views::ast`] owns the scheme itself (both processes
//! link `kubuno_views`); this module only resolves ids against the
//! currently-open document and turns [`kubuno_views::edit`]'s `Vec<Edit>`
//! into the LSP-`Position`-shaped JSON the client speaks.
//!
//! An id that does not resolve (stale — computed against a since-edited
//! document — or simply malformed) is never an error: every function here
//! degrades to "no edits"/`None`, exactly like [`kubuno_views::edit`]'s own
//! out-of-range behavior, so a client racing its own debounced re-parse
//! against a fresh user edit never gets a hard failure for it.

use kubuno_views::ast::{AstNode, Document as AstDocument};
use kubuno_views::edit::{self, Edit};
use lsp_types::{Position, Range, Uri};
use serde::{Deserialize, Serialize};

use crate::documents::Document;

// ── wire types ─────────────────────────────────────────────────────────

/// One minimal `{range, newText}` replacement, in the LSP `TextEdit` shape —
/// a plain struct rather than `lsp_types::TextEdit` itself, since
/// `kubuno/applyEdit` is a custom method (not `textDocument/*`) and gains
/// nothing from also carrying that type's `annotation_id` field.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEditJson {
    pub range: Range,
    pub new_text: String,
}

#[derive(Debug, Deserialize)]
pub struct ApplyEditParams {
    pub uri: Uri,
    pub op: EditOp,
}

/// The designer's edit intents, addressed by [`kubuno_views::ast::Element::stable_id`] (see the
/// module doc). Tagged on `"kind"` in JSON (not `"op"`, to avoid a field
/// named the same as its own containing object). `rename_all` on the enum
/// itself only camel-cases the `"kind"` tag values (`SetAttribute` →
/// `"setAttribute"`) — serde does not cascade it into a struct variant's own
/// fields, so every variant repeats it for its `snake_case` Rust field names
/// to serialize/deserialize as `camelCase` JSON too.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EditOp {
    #[serde(rename_all = "camelCase")]
    SetAttribute { element_id: String, name: String, value: String },
    #[serde(rename_all = "camelCase")]
    RemoveAttribute { element_id: String, name: String },
    /// `xml` is a well-formed `.kbview` fragment, inserted byte-for-byte —
    /// see [`kubuno_views::edit::insert_child`]'s own doc. `parent_id`
    /// addresses the *container*, since the new element has no id of its own
    /// yet.
    #[serde(rename_all = "camelCase")]
    InsertChild { parent_id: String, index: usize, xml: String },
    #[serde(rename_all = "camelCase")]
    RemoveElement { element_id: String },
    /// Moves `element_id` to index `index` under `new_parent_id` — the same
    /// parent as today for a same-container reorder, a different one for a
    /// cross-container drop (`DESIGNER.md` §4's Dock/Flow drop targets).
    #[serde(rename_all = "camelCase")]
    MoveElement { element_id: String, new_parent_id: String, index: usize },
    #[serde(rename_all = "camelCase")]
    RenameElement { element_id: String, new_name: String },
    /// Inserts a copied fragment (the designer's Paste/Duplicate) as `parent_id`'s child at
    /// `index`, laid out on its own indented line, colliding `x:Name`s renamed — see
    /// [`kubuno_views::edit::insert_fragment`]. Unlike `insertChild` (a verbatim splice).
    #[serde(rename_all = "camelCase")]
    InsertFragment { parent_id: String, index: usize, xml: String },
    /// Wraps `element_id` in a new `<wrapper>` container (the designer's "Wrap in").
    #[serde(rename_all = "camelCase")]
    WrapElement { element_id: String, wrapper: String },
    /// Replaces the container `element_id` by its children (the designer's "Remove container").
    #[serde(rename_all = "camelCase")]
    UnwrapElement { element_id: String },
    /// Reorders `parent_id`'s children: slot `i` receives the child currently at `order[i]` (the
    /// designer's "Bring to Front"/"Send to Back" on a multi-selection, `DESIGNER.md` §13) - see
    /// [`kubuno_views::edit::reorder_children`].
    #[serde(rename_all = "camelCase")]
    ReorderChildren { parent_id: String, order: Vec<usize> },
}

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApplyEditResult {
    pub edits: Vec<TextEditJson>,
}

#[derive(Debug, Deserialize)]
pub struct ElementAtOffsetParams {
    pub uri: Uri,
    /// Deliberately an LSP [`Position`] (0-based line, UTF-16 character) —
    /// see the "DSG-2 protocol" section of `DESIGNER.md` for why this
    /// refines the design note's own "`{uri, offset}`" sketch: every other
    /// position this server accepts (`textDocument/hover`, `/completion`,
    /// `/definition`) already uses this shape and its existing
    /// [`crate::position::PositionIndex`] conversion, so reusing it here
    /// needs no new coordinate system on either side of the wire.
    pub position: Position,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementAtOffsetResult {
    pub element_id: String,
    pub range: Range,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeOfElementParams {
    pub uri: Uri,
    pub element_id: String,
}

#[derive(Debug, Serialize)]
pub struct RangeOfElementResult {
    pub range: Range,
}

// ── implementation ────────────────────────────────────────────────────

fn parsed(doc: &Document) -> Option<AstDocument> {
    AstDocument::cast(doc.parse.syntax())
}

fn to_range(doc: &Document, range: rowan::TextRange) -> Range {
    Range {
        start: doc.position_index.offset_to_position(&doc.text, range.start()),
        end: doc.position_index.offset_to_position(&doc.text, range.end()),
    }
}

fn to_text_edit(doc: &Document, edit: Edit) -> TextEditJson {
    // The document's own line endings, whatever the fragment was written with.
    let new_text = kubuno_views::edit::match_line_endings(&doc.text, edit.new_text);
    TextEditJson { range: to_range(doc, edit.range), new_text }
}

/// Splits a stable id into `(parent_id, index)` — a pure string operation,
/// no tree walk needed, because [`kubuno_views::ast::Element::stable_id`]'s own construction
/// *is* the ordinal path: `"2.0.3"` → `("2.0", 3)`; a top-level element's id
/// (e.g. `"3"`) → `("", 3)`. The document root itself (`""`) has no parent,
/// so `None` — removing/moving/renaming the root element is not a supported
/// designer gesture (a `.kbview` file always has exactly one root).
fn split_parent(element_id: &str) -> Option<(String, usize)> {
    if element_id.is_empty() {
        return None;
    }
    match element_id.rsplit_once('.') {
        Some((parent, last)) => Some((parent.to_string(), last.parse().ok()?)),
        None => Some((String::new(), element_id.parse().ok()?)),
    }
}

fn apply_remove_element(ast_doc: &AstDocument, element_id: &str) -> Vec<Edit> {
    let Some((parent_id, index)) = split_parent(element_id) else { return Vec::new() };
    let Some(parent) = ast_doc.resolve_id(&parent_id) else { return Vec::new() };
    edit::remove_child(&parent, index)
}

fn apply_move_element(ast_doc: &AstDocument, element_id: &str, new_parent_id: &str, index: usize) -> Vec<Edit> {
    let Some((source_parent_id, from)) = split_parent(element_id) else { return Vec::new() };
    let Some(source_parent) = ast_doc.resolve_id(&source_parent_id) else { return Vec::new() };
    let Some(dest_parent) = ast_doc.resolve_id(new_parent_id) else { return Vec::new() };
    edit::move_element(&source_parent, from, &dest_parent, index)
}

/// Computes the `Vec<Edit>` a [`ApplyEditParams::op`] intent produces against
/// `doc`'s *current* text, still in `kubuno_views` coordinates — split out
/// from [`apply_edit`] so tests can assert on ranges/text directly rather
/// than through the JSON `Range` conversion.
fn compute_edits(doc: &Document, op: &EditOp) -> Vec<Edit> {
    let Some(ast_doc) = parsed(doc) else { return Vec::new() };
    match op {
        EditOp::SetAttribute { element_id, name, value } => ast_doc
            .resolve_id(element_id)
            .map(|el| edit::set_attribute(&el, name, value))
            .unwrap_or_default(),
        EditOp::RemoveAttribute { element_id, name } => {
            ast_doc.resolve_id(element_id).map(|el| edit::remove_attribute(&el, name)).unwrap_or_default()
        }
        EditOp::InsertChild { parent_id, index, xml } => {
            ast_doc.resolve_id(parent_id).map(|el| edit::insert_child(&el, *index, xml)).unwrap_or_default()
        }
        EditOp::RemoveElement { element_id } => apply_remove_element(&ast_doc, element_id),
        EditOp::MoveElement { element_id, new_parent_id, index } => {
            apply_move_element(&ast_doc, element_id, new_parent_id, *index)
        }
        EditOp::RenameElement { element_id, new_name } => {
            ast_doc.resolve_id(element_id).map(|el| edit::rename_element(&el, new_name)).unwrap_or_default()
        }
        EditOp::InsertFragment { parent_id, index, xml } => {
            let taken = ast_doc.root_element().map(|root| edit::collect_names(&root)).unwrap_or_default();
            ast_doc.resolve_id(parent_id).map(|el| edit::insert_fragment(&el, *index, xml, &taken)).unwrap_or_default()
        }
        EditOp::WrapElement { element_id, wrapper } => {
            ast_doc.resolve_id(element_id).map(|el| edit::wrap_element(&el, wrapper)).unwrap_or_default()
        }
        EditOp::UnwrapElement { element_id } => {
            ast_doc.resolve_id(element_id).map(|el| edit::unwrap_element(&el)).unwrap_or_default()
        }
        EditOp::ReorderChildren { parent_id, order } => {
            ast_doc.resolve_id(parent_id).map(|el| edit::reorder_children(&el, order)).unwrap_or_default()
        }
    }
}

/// `kubuno/applyEdit`: turns one designer edit intent into the minimal
/// `TextEdit`s the client applies to its `ITextBuffer` — never a whole-file
/// replace. `doc` is `None` for a `uri` the server has no open document for,
/// which yields an empty edit list (the same "degrade to no-op" rule as an
/// unresolved element id).
pub fn apply_edit(doc: Option<&Document>, op: &EditOp) -> ApplyEditResult {
    let Some(doc) = doc else { return ApplyEditResult::default() };
    let edits = compute_edits(doc, op).into_iter().map(|e| to_text_edit(doc, e)).collect();
    ApplyEditResult { edits }
}

/// `kubuno/elementAtOffset`: the innermost element whose subtree contains
/// `position` (XML pane → design surface selection sync, `DESIGNER.md` §1).
/// `None` when `doc` is not open, the document has no root element, or
/// `position` does not land on any token (an empty file).
pub fn element_at_offset(doc: &Document, position: Position) -> Option<ElementAtOffsetResult> {
    let offset = doc.position_index.position_to_offset(&doc.text, position);
    let root = doc.parse.syntax();
    let token = crate::tree::token_at_offset(&root, offset)?;
    let start_node = token.parent()?;
    let element = crate::tree::enclosing_element(&start_node)?;
    Some(ElementAtOffsetResult {
        element_id: element.stable_id(),
        range: to_range(doc, element.syntax().text_range()),
    })
}

/// `kubuno/rangeOfElement`: the inverse selection-sync direction (design
/// surface → XML pane, `DESIGNER.md` §1). `None` when `element_id` does not
/// resolve against `doc`'s current parse (a stale id, or a malformed one).
pub fn range_of_element(doc: &Document, element_id: &str) -> Option<RangeOfElementResult> {
    let ast_doc = parsed(doc)?;
    let element = ast_doc.resolve_id(element_id)?;
    Some(RangeOfElementResult { range: to_range(doc, element.syntax().text_range()) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno_views::syntax::parse;

    fn open(text: &str) -> Document {
        Document {
            position_index: crate::position::PositionIndex::new(text),
            parse: parse(text),
            text: text.to_string(),
            version: 1,
        }
    }

    #[test]
    fn set_attribute_by_stable_id_produces_one_edit() {
        let doc = open(r#"<Stack><Button Text="Ok"/></Stack>"#);
        let op = EditOp::SetAttribute {
            element_id: "0".to_string(),
            name: "Text".to_string(),
            value: "Annuler".to_string(),
        };
        let result = apply_edit(Some(&doc), &op);
        assert_eq!(result.edits.len(), 1);
        assert_eq!(result.edits[0].new_text, "Annuler");
    }

    #[test]
    fn set_attribute_on_missing_document_is_a_no_op() {
        let op = EditOp::SetAttribute {
            element_id: "0".to_string(),
            name: "Text".to_string(),
            value: "x".to_string(),
        };
        let result = apply_edit(None, &op);
        assert!(result.edits.is_empty());
    }

    #[test]
    fn set_attribute_on_unknown_id_is_a_no_op() {
        let doc = open(r#"<Button Text="Ok"/>"#);
        let op = EditOp::SetAttribute {
            element_id: "9.9".to_string(),
            name: "Text".to_string(),
            value: "x".to_string(),
        };
        assert!(apply_edit(Some(&doc), &op).edits.is_empty());
    }

    #[test]
    fn insert_child_addresses_the_parent_id() {
        let doc = open(r#"<Stack><A/></Stack>"#);
        let op =
            EditOp::InsertChild { parent_id: String::new(), index: 1, xml: "<B/>".to_string() };
        let result = apply_edit(Some(&doc), &op);
        assert_eq!(result.edits.len(), 1);
        assert_eq!(result.edits[0].new_text, "<B/>");
    }

    /// A designer edit keeps the document's line endings: an LF fragment dropped into a CRLF view
    /// is written with CRLF (Visual Studio otherwise asks to normalise the file's mixed endings), and
    /// a CRLF fragment into an LF view with LF.
    #[test]
    fn designer_edits_keep_the_document_line_endings() {
        let crlf = "<Stack>\r\n  <A/>\r\n</Stack>\r\n";
        let doc = open(crlf);
        let op = EditOp::InsertChild { parent_id: String::new(), index: 1, xml: "<Panel>\n  <B/>\n</Panel>\n  ".to_string() };
        let result = apply_edit(Some(&doc), &op);
        assert_eq!(result.edits.len(), 1);
        let text = &result.edits[0].new_text;
        assert_eq!(text.matches("\r\n").count(), text.matches('\n').count(), "no bare LF in {text:?}");
        assert_eq!(text.matches('\n').count(), 3);

        let lf = open("<Stack>\n  <A/>\n</Stack>\n");
        let op = EditOp::InsertChild { parent_id: String::new(), index: 1, xml: "<Panel>\r\n  <B/>\r\n</Panel>".to_string() };
        let result = apply_edit(Some(&lf), &op);
        assert!(!result.edits[0].new_text.contains('\r'), "{:?}", result.edits[0].new_text);
    }

    #[test]
    fn remove_element_resolves_its_own_parent_and_index_from_the_id() {
        let doc = open("<Stack>\n  <A/>\n  <B/>\n</Stack>");
        let op = EditOp::RemoveElement { element_id: "0".to_string() };
        let result = apply_edit(Some(&doc), &op);
        assert_eq!(result.edits.len(), 1);
        assert_eq!(result.edits[0].new_text, "");
    }

    #[test]
    fn remove_root_element_is_a_no_op() {
        let doc = open("<Stack><A/></Stack>");
        let op = EditOp::RemoveElement { element_id: String::new() };
        assert!(apply_edit(Some(&doc), &op).edits.is_empty());
    }

    #[test]
    fn move_element_across_parents_produces_two_edits() {
        let doc = open(
            r#"<Stack><Panel x:Name="left"><A/></Panel><Panel x:Name="right"></Panel></Stack>"#,
        );
        let op = EditOp::MoveElement { element_id: "0.0".to_string(), new_parent_id: "1".to_string(), index: 0 };
        let result = apply_edit(Some(&doc), &op);
        assert_eq!(result.edits.len(), 2);
    }

    #[test]
    fn rename_element_renames_both_tags() {
        let doc = open(r#"<Stack><Panel></Panel></Stack>"#);
        let op = EditOp::RenameElement { element_id: "0".to_string(), new_name: "Card".to_string() };
        let result = apply_edit(Some(&doc), &op);
        assert_eq!(result.edits.len(), 2);
        assert!(result.edits.iter().all(|e| e.new_text == "Card"));
    }

    fn applied(doc: &Document, op: &EditOp) -> String {
        kubuno_views::edit::apply_edits(&doc.text, &compute_edits(doc, op))
    }

    #[test]
    fn insert_fragment_formats_and_renames_against_the_whole_document() {
        let doc = open("<Card>\n  <Stack>\n    <Button x:Name=\"ok\"/>\n  </Stack>\n</Card>");
        let op = EditOp::InsertFragment { parent_id: "0".to_string(), index: 1, xml: r#"<Button x:Name="ok"/>"#.to_string() };
        assert_eq!(
            applied(&doc, &op),
            "<Card>\n  <Stack>\n    <Button x:Name=\"ok\"/>\n    <Button x:Name=\"ok2\"/>\n  </Stack>\n</Card>"
        );
    }

    #[test]
    fn insert_fragment_of_garbage_is_a_no_op() {
        let doc = open("<Stack/>");
        let op = EditOp::InsertFragment { parent_id: String::new(), index: 0, xml: "<A><B/>".to_string() };
        assert!(apply_edit(Some(&doc), &op).edits.is_empty());
    }

    #[test]
    fn wrap_and_unwrap_element_round_trip() {
        let doc = open("<Card>\n  <Stack/>\n</Card>");
        let wrapped = applied(&doc, &EditOp::WrapElement { element_id: "0".to_string(), wrapper: "ScrollArea".to_string() });
        assert_eq!(wrapped, "<Card>\n  <ScrollArea>\n    <Stack/>\n  </ScrollArea>\n</Card>");
        let unwrapped = applied(&open(&wrapped), &EditOp::UnwrapElement { element_id: "0".to_string() });
        assert_eq!(unwrapped, "<Card>\n  <Stack/>\n</Card>");
    }

    #[test]
    fn wrap_and_unwrap_ops_deserialize_from_camel_case() {
        let op: EditOp = serde_json::from_str(r#"{"kind":"wrapElement","elementId":"0","wrapper":"Card"}"#).unwrap();
        assert!(matches!(op, EditOp::WrapElement { .. }));
        let op: EditOp = serde_json::from_str(r#"{"kind":"unwrapElement","elementId":"0"}"#).unwrap();
        assert!(matches!(op, EditOp::UnwrapElement { .. }));
        let op: EditOp =
            serde_json::from_str(r#"{"kind":"insertFragment","parentId":"","index":2,"xml":"<A/>"}"#).unwrap();
        assert!(matches!(op, EditOp::InsertFragment { index: 2, .. }));
    }

    #[test]
    fn reorder_children_moves_several_siblings_in_one_edit_set() {
        let doc = open("<Stack>\n  <A/>\n  <B/>\n  <C/>\n</Stack>");
        let op: EditOp = serde_json::from_str(r#"{"kind":"reorderChildren","parentId":"","order":[1,2,0]}"#).unwrap();
        assert_eq!(applied(&doc, &op), "<Stack>\n  <B/>\n  <C/>\n  <A/>\n</Stack>");
    }

    #[test]
    fn insert_fragment_accepts_several_elements() {
        let doc = open("<Stack>\n  <A/>\n</Stack>");
        let op = EditOp::InsertFragment { parent_id: String::new(), index: 1, xml: "<B/>\n<C/>".to_string() };
        assert_eq!(applied(&doc, &op), "<Stack>\n  <A/>\n  <B/>\n  <C/>\n</Stack>");
    }

    #[test]
    fn element_at_offset_finds_the_innermost_element() {
        let doc = open(r#"<Stack><Button Text="Ok"/></Stack>"#);
        // Offset inside `Text="Ok"`'s value — still within `Button`, not `Stack`.
        let pos = Position { line: 0, character: 22 };
        let found = element_at_offset(&doc, pos).expect("an element under the cursor");
        assert_eq!(found.element_id, "0");
    }

    #[test]
    fn element_at_offset_on_the_root_itself() {
        let doc = open(r#"<Button Text="Ok"/>"#);
        let pos = Position { line: 0, character: 2 }; // Inside "Button".
        let found = element_at_offset(&doc, pos).expect("the root element");
        assert_eq!(found.element_id, "");
    }

    #[test]
    fn element_at_offset_on_empty_document_is_none() {
        let doc = open("");
        assert!(element_at_offset(&doc, Position { line: 0, character: 0 }).is_none());
    }

    #[test]
    fn range_of_element_round_trips_with_stable_id() {
        let doc = open(r#"<Stack><A/><B/></Stack>"#);
        let ast_doc = parsed(&doc).unwrap();
        let stack = ast_doc.root_element().unwrap();
        let b = stack.children().nth(1).unwrap();
        let id = b.stable_id();
        let result = range_of_element(&doc, &id).expect("B should resolve");
        assert_eq!(result.range, to_range(&doc, b.syntax().text_range()));
    }

    #[test]
    fn range_of_element_on_unknown_id_is_none() {
        let doc = open(r#"<Button/>"#);
        assert!(range_of_element(&doc, "3.3").is_none());
    }
}
