//! The two kinds of view file (vskubuno `docs/VIEWS-SPEC.md`, "File kinds"): a form, window or
//! dialog is a `.kbview`; a user control - a view whose root is `<UserControl>`, or whose code-behind
//! is a `#[derive(UserControl)]` struct - is a `.kbcontrol`. Both are the same XML format; only the
//! role differs, which is what Solution Explorer's icon and the item templates show.
//!
//! This module warns when a file's extension disagrees with what it holds and offers the quick fix
//! that renames it ("Renommer en .kbcontrol" / "Renommer en .kbview"): the file itself (a
//! `RenameFile` resource operation) and every path naming it - the code-behind's
//! `#[kubuno::view("…")]` / `#[user_control(view = "…")]` and the `x:Inherits="…"` of the views
//! deriving from it.

use std::path::{Path, PathBuf};

use kubuno_views::ast::{AstNode, Document as AstDocument};
use lsp_types::{
    CodeAction, CodeActionKind, Diagnostic, DiagnosticSeverity, DocumentChangeOperation, DocumentChanges, NumberOrString,
    OneOf, OptionalVersionedTextDocumentIdentifier, Position, Range, RenameFile, ResourceOp, TextDocumentEdit, TextEdit,
    Uri, WorkspaceEdit,
};

use crate::documents::Document;
use crate::position::PositionIndex;
use crate::{fs_uri, sources};

// The file rules (extensions, kinds) live in the platform-neutral `kubuno-views-syntax`
// (`view_kind`, WV-1), shared with the web tooling; re-exported here under their historical paths.
pub use kubuno_views_syntax::view_kind::{is_view_file, kind_of_extension, ViewKind, CONTROL_EXTENSION, DIAGNOSTIC_CODE, VIEW_EXTENSION};

/// A string literal of a Rust file naming the view: where its extension sits, and whether it is
/// inside a `#[user_control(…)]` attribute.
struct PathLiteral {
    /// Byte range of the extension (without its dot) inside the `.rs` text.
    ext_start: usize,
    ext_end: usize,
    in_user_control: bool,
}

/// The string literals of `text` (a Rust file in `rs_dir`) that resolve to `view`.
fn literals_naming(text: &str, rs_dir: &Path, view: &Path) -> Vec<PathLiteral> {
    let target = sources::key(view);
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let Some(next) = crate::code_behind::skip_non_code(text, i) else {
            i += text[i..].chars().next().map_or(1, char::len_utf8);
            continue;
        };
        // Only plain `"…"` literals: a view path never needs a raw string.
        if bytes[i] == b'"' && next >= i + 2 && bytes[next - 1] == b'"' {
            let (start, end) = (i + 1, next - 1);
            let value = &text[start..end];
            if !value.contains('\\') && kind_of_extension(Path::new(value)).is_some() && sources::key(&rs_dir.join(value)) == target {
                let dot = value.rfind('.').map_or(end, |d| start + d);
                // Inside an attribute when the closest `#[` before it is not closed yet.
                let attr = text[..i].rfind("#[").map_or("", |a| &text[a..i]);
                let attr = if attr.contains(']') { "" } else { attr };
                out.push(PathLiteral { ext_start: dot + 1, ext_end: end, in_user_control: attr.contains("user_control") });
            }
        }
        i = next.max(i + 1);
    }
    out
}

/// The `.rs` files that may name the view: those next to it and those of its parent folder.
fn candidate_rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = crate::definition::sibling_rs_files(dir);
    if let Some(parent) = dir.parent() {
        files.extend(crate::definition::sibling_rs_files(parent));
    }
    files
}

/// The kind of the view `doc` (at `path`): a user control when its root is `<UserControl>` or when
/// its code-behind declares it with `#[user_control(view = "…")]`; a form otherwise. `None` when the
/// document has no root element yet.
pub fn kind_of(doc: &Document, path: &Path) -> Option<ViewKind> {
    let root = AstDocument::cast(doc.parse.syntax()).and_then(|d| d.root_element())?;
    let root_name = root.name()?;
    if root_name == "UserControl" {
        return Some(ViewKind::UserControl);
    }
    let dir = path.parent()?;
    let declared_as_control = candidate_rust_files(dir).iter().any(|rs| {
        let Some(text) = sources::read(rs) else { return false };
        let rs_dir = rs.parent().unwrap_or(dir);
        literals_naming(&text, rs_dir, path).iter().any(|l| l.in_user_control)
    });
    Some(if declared_as_control { ViewKind::UserControl } else { ViewKind::Form })
}

/// The range a kind diagnostic underlines: the root element's name.
fn root_name_range(doc: &Document) -> Range {
    let range = AstDocument::cast(doc.parse.syntax()).and_then(|d| d.root_element()).and_then(|r| r.name_range());
    match range {
        Some(r) => Range { start: doc.position_index.offset_to_position(&doc.text, r.start()), end: doc.position_index.offset_to_position(&doc.text, r.end()) },
        None => Range { start: Position::new(0, 0), end: Position::new(0, 0) },
    }
}

/// The kind the file should have, when its extension disagrees with what it holds.
fn mismatch(doc: &Document, uri: &Uri) -> Option<(PathBuf, ViewKind)> {
    let path = fs_uri::to_path(uri)?;
    let announced = kind_of_extension(&path)?;
    let actual = kind_of(doc, &path)?;
    (announced != actual).then_some((path, actual))
}

fn message(kind: ViewKind) -> String {
    match kind {
        ViewKind::UserControl => {
            "This view is a user control: its file should use the `.kbcontrol` extension (`.kbview` is for forms, windows and dialogs).".to_string()
        }
        ViewKind::Form => "This view is a form, a window or a dialog: its file should use the `.kbview` extension (`.kbcontrol` is for user controls).".to_string(),
    }
}

/// The warning of a view whose extension disagrees with its kind.
pub fn diagnostics(doc: &Document, uri: &Uri) -> Vec<Diagnostic> {
    let Some((_, kind)) = mismatch(doc, uri) else { return Vec::new() };
    vec![Diagnostic {
        range: root_name_range(doc),
        severity: Some(DiagnosticSeverity::WARNING),
        code: Some(NumberOrString::String(DIAGNOSTIC_CODE.to_string())),
        source: Some("kubuno-views".to_string()),
        message: message(kind),
        ..Default::default()
    }]
}

/// The `x:Inherits="…"` values of `text` (a view in `view_dir`) naming `base`: the byte ranges of
/// their extensions.
fn inherits_naming(text: &str, view_dir: &Path, base: &Path) -> Vec<(usize, usize)> {
    let target = sources::key(base);
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find("x:Inherits=") {
        let attr = from + found + "x:Inherits=".len();
        from = attr;
        let Some(quote) = text[attr..].chars().next().filter(|c| *c == '"' || *c == '\'') else { continue };
        let start = attr + 1;
        let Some(len) = text[start..].find(quote) else { break };
        let value = &text[start..start + len];
        if kind_of_extension(Path::new(value)).is_some() && sources::key(&view_dir.join(value)) == target {
            if let Some(dot) = value.rfind('.') {
                out.push((start + dot + 1, start + len));
            }
        }
    }
    out
}

/// The text edits of `path` replacing each byte range with `new_ext`.
fn edits_of(path: &Path, text: &str, ranges: &[(usize, usize)], new_ext: &str) -> Option<DocumentChangeOperation> {
    if ranges.is_empty() {
        return None;
    }
    let index = PositionIndex::new(text);
    let at = |o: usize| index.offset_to_position(text, rowan::TextSize::new(u32::try_from(o).unwrap_or(u32::MAX)));
    let edits = ranges.iter().map(|&(s, e)| OneOf::Left(TextEdit { range: Range { start: at(s), end: at(e) }, new_text: new_ext.to_string() })).collect();
    Some(DocumentChangeOperation::Edit(TextDocumentEdit {
        text_document: OptionalVersionedTextDocumentIdentifier { uri: fs_uri::from_path(path)?, version: None },
        edits,
    }))
}

/// The view files next to `dir` (both kinds), `skip` excepted.
fn sibling_views(dir: &Path, skip: &Path) -> Vec<PathBuf> {
    let skip = sources::key(skip);
    let mut views: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).filter(|p| is_view_file(p) && sources::key(p) != skip).collect())
        .unwrap_or_default();
    views.sort();
    views
}

/// The rename of a view to `kind`'s extension: the paths naming it first (while they still resolve),
/// then the file itself. `None` when a file of the new name already exists.
pub fn rename_edit(path: &Path, kind: ViewKind) -> Option<WorkspaceEdit> {
    let new_path = path.with_extension(kind.extension());
    if new_path.exists() {
        return None;
    }
    let dir = path.parent()?;
    let mut operations = Vec::new();
    for rs in candidate_rust_files(dir) {
        let Some(text) = sources::read(&rs) else { continue };
        let ranges: Vec<(usize, usize)> = literals_naming(&text, rs.parent().unwrap_or(dir), path).iter().map(|l| (l.ext_start, l.ext_end)).collect();
        operations.extend(edits_of(&rs, &text, &ranges, kind.extension()));
    }
    for view in sibling_views(dir, path) {
        let Some(text) = sources::read(&view) else { continue };
        operations.extend(edits_of(&view, &text, &inherits_naming(&text, dir, path), kind.extension()));
    }
    operations.push(DocumentChangeOperation::Op(ResourceOp::Rename(RenameFile {
        old_uri: fs_uri::from_path(path)?,
        new_uri: fs_uri::from_path(&new_path)?,
        options: None,
        annotation_id: None,
    })));
    Some(WorkspaceEdit { document_changes: Some(DocumentChanges::Operations(operations)), ..Default::default() })
}

/// The "Renommer en .kbcontrol" / "Renommer en .kbview" quick fix, when `range` touches the root's name.
pub fn quick_fixes(doc: &Document, uri: &Uri, range: &Range) -> Vec<CodeAction> {
    let Some((path, kind)) = mismatch(doc, uri) else { return Vec::new() };
    let root = root_name_range(doc);
    // The whole first lines count, so the fix is offered wherever the caret sits on the root's start tag line.
    if range.end.line < root.start.line || range.start.line > root.end.line {
        return Vec::new();
    }
    let Some(edit) = rename_edit(&path, kind) else { return Vec::new() };
    vec![CodeAction {
        title: format!("Renommer en .{}", kind.extension()),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(diagnostics(doc, uri)),
        is_preferred: Some(true),
        edit: Some(edit),
        ..Default::default()
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::DocumentStore;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kubuno-view-kind-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn open(dir: &Path, file: &str, text: &str) -> (DocumentStore, Uri) {
        let path = dir.join(file);
        fs::write(&path, text).unwrap();
        let uri = fs_uri::from_path(&path).unwrap();
        let mut store = DocumentStore::new();
        store.open(uri.clone(), text.to_string(), 1);
        (store, uri)
    }

    fn whole() -> Range {
        Range { start: Position::new(0, 0), end: Position::new(100, 0) }
    }

    #[test]
    fn a_form_in_a_kbview_and_a_control_in_a_kbcontrol_are_fine() {
        let dir = temp_dir("fine");
        let (store, uri) = open(&dir, "main_view.kbview", "<Panel DesignWidth=\"480\"/>");
        assert!(diagnostics(store.get(&uri).unwrap(), &uri).is_empty());
        let (store, uri) = open(&dir, "row.kbcontrol", "<UserControl x:Class=\"Row\"/>");
        assert!(diagnostics(store.get(&uri).unwrap(), &uri).is_empty());
        assert!(quick_fixes(store.get(&uri).unwrap(), &uri, &whole()).is_empty());
    }

    #[test]
    fn a_user_control_root_in_a_kbview_is_renamed_with_its_code_behind() {
        let dir = temp_dir("root");
        fs::write(dir.join("row.rs"), "#[derive(UserControl, Default)]\n#[user_control(view = \"row.kbview\")]\npub struct Row { base: UserControlCore }\n").unwrap();
        fs::write(dir.join("fancy_row.kbview"), "<UserControl x:Class=\"FancyRow\" x:Inherits=\"row.kbview\"/>").unwrap();
        let (store, uri) = open(&dir, "row.kbview", "<!-- a row -->\n<UserControl x:Class=\"Row\"/>");
        let doc = store.get(&uri).unwrap();
        let diags = diagnostics(doc, &uri);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].range.start, Position::new(1, 1));
        assert!(diags[0].message.contains(".kbcontrol"));
        let fixes = quick_fixes(doc, &uri, &Range { start: Position::new(1, 3), end: Position::new(1, 3) });
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0].title, "Renommer en .kbcontrol");
        let Some(DocumentChanges::Operations(ops)) = fixes[0].edit.as_ref().and_then(|e| e.document_changes.clone()) else { panic!("operations") };
        // The code-behind, the derived view, then the rename itself.
        assert_eq!(ops.len(), 3);
        let DocumentChangeOperation::Edit(rs) = &ops[0] else { panic!("an edit first") };
        assert!(rs.text_document.uri.as_str().ends_with("/row.rs"));
        let OneOf::Left(e) = &rs.edits[0] else { panic!("a plain edit") };
        assert_eq!((e.range.start, e.range.end, e.new_text.as_str()), (Position::new(1, 27), Position::new(1, 33), "kbcontrol"));
        let DocumentChangeOperation::Edit(derived) = &ops[1] else { panic!("the derived view") };
        assert!(derived.text_document.uri.as_str().ends_with("/fancy_row.kbview"));
        let DocumentChangeOperation::Op(ResourceOp::Rename(rename)) = &ops[2] else { panic!("the rename last") };
        assert!(rename.old_uri.as_str().ends_with("/row.kbview"));
        assert!(rename.new_uri.as_str().ends_with("/row.kbcontrol"));
    }

    #[test]
    fn a_user_control_code_behind_makes_a_control_even_with_another_root() {
        let dir = temp_dir("codebehind");
        fs::write(dir.join("pane.rs"), "#[derive(UserControl, Default)]\n#[user_control(view = \"pane.kbview\", default_event = \"Closed\")]\npub struct Pane { base: UserControlCore }\n").unwrap();
        let (store, uri) = open(&dir, "pane.kbview", "<Panel/>");
        let doc = store.get(&uri).unwrap();
        assert_eq!(kind_of(doc, &fs_uri::to_path(&uri).unwrap()), Some(ViewKind::UserControl));
        assert_eq!(diagnostics(doc, &uri).len(), 1);
    }

    #[test]
    fn a_form_in_a_kbcontrol_is_renamed_back_to_kbview() {
        let dir = temp_dir("form");
        fs::write(dir.join("settings_view.rs"), "#[kubuno::view(\"settings_view.kbcontrol\")]\n#[derive(Default)]\npub struct SettingsView {}\n").unwrap();
        let (store, uri) = open(&dir, "settings_view.kbcontrol", "<Panel Title=\"Settings\"/>");
        let doc = store.get(&uri).unwrap();
        assert!(diagnostics(doc, &uri)[0].message.contains("`.kbview`"));
        let fixes = quick_fixes(doc, &uri, &whole());
        assert_eq!(fixes[0].title, "Renommer en .kbview");
        let Some(DocumentChanges::Operations(ops)) = fixes[0].edit.as_ref().and_then(|e| e.document_changes.clone()) else { panic!("operations") };
        assert_eq!(ops.len(), 2);
    }

    #[test]
    fn a_relative_path_from_a_parent_folder_is_followed_and_no_fix_overwrites_a_file() {
        let dir = temp_dir("parent");
        fs::create_dir_all(dir.join("fixtures")).unwrap();
        fs::write(dir.join("tests.rs"), "#[user_control(view = \"fixtures/bar.kbview\")]\nstruct Bar;\n").unwrap();
        let (store, uri) = open(&dir.join("fixtures"), "bar.kbview", "<UserControl/>");
        let path = fs_uri::to_path(&uri).unwrap();
        let Some(DocumentChanges::Operations(ops)) = rename_edit(&path, ViewKind::UserControl).and_then(|e| e.document_changes) else { panic!("operations") };
        assert!(matches!(&ops[0], DocumentChangeOperation::Edit(e) if e.text_document.uri.as_str().ends_with("/tests.rs")));
        fs::write(dir.join("fixtures/bar.kbcontrol"), "<UserControl/>").unwrap();
        assert!(quick_fixes(store.get(&uri).unwrap(), &uri, &whole()).is_empty(), "never overwrites an existing file");
    }

    #[test]
    fn view_files_of_both_kinds_are_recognised() {
        assert!(is_view_file(Path::new("a/b.kbview")));
        assert!(is_view_file(Path::new("a/b.KBCONTROL")));
        assert!(!is_view_file(Path::new("a/b.kbres")));
        assert_eq!(kind_of_extension(Path::new("x.kbcontrol")), Some(ViewKind::UserControl));
    }
}
