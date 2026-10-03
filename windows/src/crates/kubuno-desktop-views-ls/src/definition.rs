//! `textDocument/definition` from `Click="handler"`-style event attribute
//! values to a Rust `fn handler_name` in a sibling `.rs` file.
//!
//! ## Deliberately simple: plain text search, not name resolution
//!
//! A real "go to the Rust definition" would mean understanding which
//! `handlers! { … }` table (`XML_VIEWS.md` §2/§7) the view's code-behind
//! registers and resolving through it — that requires the interpreter/
//! binding layer (`crate::binding`, `crate::compile`) another agent is
//! actively building in `kubuno-desktop-views` right now, and this crate is asked
//! not to depend on that work-in-progress shape. So this is intentionally a
//! **simple text search**: given the handler name from the attribute value,
//! scan every `*.rs` file in the same directory as the open `.kbview` file
//! for a line matching `fn <handler_name>` (word-boundaried, so `fn
//! offline_toggled2` does not match a lookup for `offline_toggled`) and
//! return each match as a [`lsp_types::Location`]. This covers the common
//! case §7's worked example shows (code-behind living beside its view, e.g.
//! `settings_view.kbview` next to `settings_view.rs`) without guessing at a
//! module-resolution scheme this phase does not define.

use std::fs;
use std::path::Path;

use kubuno_desktop_views::ast::AstNode;
use kubuno_desktop_views::syntax::SyntaxKind;
use lsp_types::{Location, Position, Range, Uri};

use crate::documents::Document;
use crate::fs_uri;
use crate::position::PositionIndex;
use crate::tree;

/// Finds `fn <name>` definitions in every sibling `.rs` file of `uri`'s
/// directory, when the cursor sits on an event attribute's value
/// (`OnClick="handler_name"` — any attribute the element's registry entry
/// declares as an [`kubuno_desktop_views::registry::EventMeta`], not just `OnClick`
/// by name, per `XML_VIEWS.md` §2's "Events are dispatched by string name").
pub fn goto_definition(doc: &Document, uri: &Uri, pos: Position) -> Vec<Location> {
    if let Some(location) = class_definition(doc, pos) {
        return vec![location];
    }
    let Some(handler_name) = event_handler_name_at(doc, pos) else { return Vec::new() };
    let Some(file_path) = fs_uri::to_path(uri) else { return Vec::new() };
    let Some(dir) = file_path.parent() else { return Vec::new() };

    sibling_rs_files(dir).iter().flat_map(|path| find_fn_in_file(path, &handler_name)).collect()
}

/// EVT-7b: on the name of an element whose class belongs to the project, its `struct` (the
/// project scan records the file and line).
fn class_definition(doc: &Document, pos: Position) -> Option<Location> {
    let offset = doc.position_index.position_to_offset(&doc.text, pos);
    let token = tree::token_at_offset(&doc.parse.syntax(), offset)?;
    if !tree::is_start_tag_name(&token) {
        return None;
    }
    let info = kubuno_desktop_views::registry::project_info(token.text())?;
    let path = Path::new(info.source_file?);
    let line = info.source_line.unwrap_or(1).saturating_sub(1);
    let uri = fs_uri::from_path(path)?;
    let start = Position { line, character: 0 };
    Some(Location { uri, range: Range { start, end: start } })
}

/// Every `*.rs` file directly inside `dir`, in `fs::read_dir`'s own order —
/// the "sibling `.rs` files of the open `.kbview` file" set this module's doc
/// comment describes. Shared with [`crate::handler_insert`] (DSG-10, `vskubuno
/// /docs/DESIGNER.md`), which needs the exact same set to decide whether a
/// handler a designer gesture is about to wire up already exists somewhere,
/// before creating a new one — "found the same way `definition.rs` finds
/// handlers" per that package's own brief.
pub(crate) fn sibling_rs_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("rs"))
        .collect()
}

/// `None` unless the cursor is inside an attribute value whose attribute is
/// a declared event on the enclosing element — in which case, the value text
/// (the handler name) is returned.
fn event_handler_name_at(doc: &Document, pos: Position) -> Option<String> {
    let offset = doc.position_index.position_to_offset(&doc.text, pos);
    let root = doc.parse.syntax();
    let token = tree::token_at_offset(&root, offset)?;
    if token.kind() != SyntaxKind::STRING {
        return None;
    }
    let attribute_node = token.parent()?;
    let attr = kubuno_desktop_views::ast::Attribute::cast(attribute_node.clone())?;
    let attr_name = attr.name()?;
    let element = tree::enclosing_element(&attribute_node)?;
    let element_name = element.name()?;
    let meta = kubuno_desktop_views::registry::lookup(&element_name)?;
    // Only offer this for a declared event attribute (the view's own events included).
    meta.event(&attr_name).or_else(|| kubuno_desktop_views::registry::view_event(&attr_name))?;
    attr.value()
}

/// Scans `path` line by line for a top-level-looking `fn <name>` — a
/// standalone regex-free word-boundary check (`fn` then whitespace then
/// `name` then `(`/whitespace), intentionally not a Rust parser: this is the
/// "simple text search is acceptable" fallback the brief calls for.
pub(crate) fn find_fn_in_file(path: &Path, name: &str) -> Vec<Location> {
    let Some(text) = crate::sources::read(path) else { return Vec::new() };
    let Some(uri) = fs_uri::from_path(path) else { return Vec::new() };
    let index = PositionIndex::new(&text);

    let mut out = Vec::new();
    let needle_pattern = format!("fn {name}");
    let mut search_from = 0usize;
    while let Some(rel) = text[search_from..].find(&needle_pattern) {
        let match_start = search_from + rel;
        let after = match_start + needle_pattern.len();
        // Word boundary right after the name: the next byte (if any) must not
        // continue an identifier (`fn offline_toggled2` must not match a
        // lookup for `offline_toggled`).
        let boundary_ok =
            text.as_bytes().get(after).is_none_or(|&b| !(b.is_ascii_alphanumeric() || b == b'_'));
        if boundary_ok {
            let start = index.offset_to_position(&text, rowan::TextSize::from(match_start as u32));
            let end = index.offset_to_position(&text, rowan::TextSize::from(after as u32));
            out.push(Location { uri: uri.clone(), range: Range { start, end } });
        }
        search_from = after;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno_desktop_views::syntax::parse;
    use std::io::Write;

    fn open(text: &str) -> Document {
        Document {
            position_index: crate::position::PositionIndex::new(text),
            parse: parse(text),
            text: text.to_string(),
            version: 1,
        }
    }

    #[test]
    fn event_handler_name_is_read_from_the_attribute_value() {
        let d = open(r#"<Switch OnToggled="offline_toggled"/>"#);
        let name_pos = Position { line: 0, character: 25 }; // Inside the quoted value.
        let name = event_handler_name_at(&d, name_pos);
        assert_eq!(name.as_deref(), Some("offline_toggled"));
    }

    #[test]
    fn non_event_attribute_value_yields_no_handler_name() {
        let d = open(r#"<Button Text="offline_toggled"/>"#); // Same text, but `Text` is not an event.
        let name_pos = Position { line: 0, character: 20 };
        assert!(event_handler_name_at(&d, name_pos).is_none());
    }

    #[test]
    fn find_fn_in_file_locates_a_matching_function() {
        let dir = std::env::temp_dir().join(format!("kubuno-views-ls-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings_view.rs");
        {
            let mut f = std::fs::File::create(&file).unwrap();
            writeln!(f, "fn unrelated() {{}}").unwrap();
            writeln!(f, "fn offline_toggled(s: &mut State, on: bool) {{ s.offline = on; }}").unwrap();
        }
        let locations = find_fn_in_file(&file, "offline_toggled");
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].range.start.line, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn find_fn_in_file_respects_word_boundaries() {
        let dir = std::env::temp_dir().join(format!("kubuno-views-ls-test-wb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("code.rs");
        {
            let mut f = std::fs::File::create(&file).unwrap();
            writeln!(f, "fn offline_toggled2() {{}}").unwrap();
        }
        let locations = find_fn_in_file(&file, "offline_toggled");
        assert!(locations.is_empty(), "must not match `offline_toggled2` for `offline_toggled`");
        std::fs::remove_dir_all(&dir).ok();
    }
}
