//! Open-document store: full text sync (`TextDocumentSyncKind::FULL`).
//!
//! `.kbview` files are hand-sized UI descriptions (`XML_VIEWS.md` §6's own
//! framing — "not generated megabyte documents"), and `kubuno_desktop_views::syntax::
//! parse` is explicitly cheap to call on every change (§5: "parse+compile
//! once per file change", the exact rate this server reparses at). Full sync
//! avoids maintaining incremental-edit application logic (splicing LSP
//! `TextDocumentContentChangeEvent` ranges into a rope) for no measurable
//! benefit at this file size — the same "own a replica, never restate it"
//! economy `kubuno-desktop-ui`/`kubuno-desktop-views` apply elsewhere in this codebase.

use std::collections::HashMap;

use kubuno_desktop_views::syntax::{self, Parse};
use lsp_types::Uri;

use crate::position::PositionIndex;

/// One open `.kbview` file: its current full text, the parse of that text,
/// and the UTF-16 position index built from the same text — kept together so
/// a caller never accidentally mixes a stale index with fresh text.
pub struct Document {
    pub text: String,
    pub parse: Parse,
    pub position_index: PositionIndex,
    /// The LSP document version last reported by the client (`didOpen`'s
    /// `version` or the last `didChange`'s) — echoed back on
    /// `publishDiagnostics` so the client can discard stale diagnostics for a
    /// document it has since edited further.
    pub version: i32,
}

impl Document {
    fn new(text: String, version: i32) -> Self {
        let parse = syntax::parse(&text);
        let position_index = PositionIndex::new(&text);
        Self { text, parse, position_index, version }
    }
}

/// All currently open documents, keyed by their LSP URI.
#[derive(Default)]
pub struct DocumentStore {
    docs: HashMap<Uri, Document>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&mut self, uri: Uri, text: String, version: i32) {
        self.docs.insert(uri, Document::new(text, version));
    }

    /// Replaces a document's full text (a `didChange` with
    /// `TextDocumentSyncKind::FULL`'s single whole-text change event) and
    /// reparses it. A `didChange` for a URI never `didOpen`ed is treated as
    /// an implicit open — defensive, not spec-mandated, but it keeps a
    /// misordered notification from silently dropping edits.
    pub fn update(&mut self, uri: Uri, text: String, version: i32) {
        self.docs.insert(uri, Document::new(text, version));
    }

    pub fn close(&mut self, uri: &Uri) {
        self.docs.remove(uri);
    }

    pub fn get(&self, uri: &Uri) -> Option<&Document> {
        self.docs.get(uri)
    }

    /// Every open document.
    pub fn iter(&self) -> impl Iterator<Item = (&Uri, &Document)> {
        self.docs.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Uri {
        s.parse().expect("valid test URI")
    }

    #[test]
    fn open_then_get_round_trips_text() {
        let mut store = DocumentStore::new();
        let uri = url("file:///a.kbview");
        store.open(uri.clone(), "<Button/>".into(), 1);
        let doc = store.get(&uri).expect("document present");
        assert_eq!(doc.text, "<Button/>");
        assert_eq!(doc.version, 1);
    }

    #[test]
    fn update_replaces_text_and_reparses() {
        let mut store = DocumentStore::new();
        let uri = url("file:///a.kbview");
        store.open(uri.clone(), "<Button/>".into(), 1);
        store.update(uri.clone(), "<Switch/>".into(), 2);
        let doc = store.get(&uri).expect("document present");
        assert_eq!(doc.text, "<Switch/>");
        assert_eq!(doc.version, 2);
    }

    #[test]
    fn close_removes_the_document() {
        let mut store = DocumentStore::new();
        let uri = url("file:///a.kbview");
        store.open(uri.clone(), "<Button/>".into(), 1);
        store.close(&uri);
        assert!(store.get(&uri).is_none());
    }
}
