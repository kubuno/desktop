//! The stored document: reading it, walking it, and writing it back without
//! losing anything.
//!
//! A document's `content_json` comes in two shapes and a client must handle
//! both:
//!
//! * a **bare** ProseMirror document — `{"type":"doc","content":[…]}`. One real
//!   document in five (70 of a 337-document sample).
//! * a **multi-page envelope** — `{"_type":"multi-page","pages":[…],
//!   "sections":[…], …}` plus a wall of sibling keys (headers, footers,
//!   comments, styles, watermark, reference settings…) that live NEXT TO the
//!   body rather than inside it.
//!
//! Two rules govern writing, and both exist because breaking them destroys user
//! content silently:
//!
//! 1. **A bare document is written back bare** ([`Shape::Bare`]) — no envelope,
//!    no minted ids. The web wraps bare documents on load with freshly minted
//!    uuids (`parseDocContent`, `DocumentEditorPage.tsx:511-517`), rewriting the
//!    document's identity on every open. We do not: a first save must not
//!    convert a fifth of the corpus to a different storage shape.
//! 2. **An envelope is written back with exactly one page**, keeping
//!    `pages[0].id`, and the extra page shells are **dropped** — never re-emitted
//!    without their `content`. `flattenToDoc` (`DocumentEditorPage.tsx:4715-4717`)
//!    reads `(pg.content as JSONContent).content`, a compile-time cast with no
//!    runtime check, so a page shell with no `content` throws and the browser can
//!    no longer open the file.
//!
//! Every root key we do not understand is carried through as its original bytes.
//! That is not politeness — the sibling wall is where tracked changes, anchored
//! comments and header/footer definitions live, and an "unknown node" escape
//! hatch inside the body catches none of it.

pub mod node;

#[cfg(test)]
mod fixtures;

use std::collections::BTreeMap;
use std::fmt;

use serde_json::value::RawValue;

pub use node::Node;

#[derive(Debug)]
pub enum Error {
    Json(serde_json::Error),
    /// The bytes parsed as JSON but are not a document in either shape.
    Shape(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Json(e) => write!(f, "invalid document JSON: {e}"),
            Error::Shape(what) => write!(f, "unexpected document shape: {what}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

/// Which storage shape a document was read from — and therefore which it will
/// be written back as.
#[derive(Debug, Clone)]
pub enum Shape {
    Bare(Node),
    Envelope(Envelope),
}

/// The multi-page envelope.
#[derive(Debug, Clone)]
pub struct Envelope {
    /// Every root key except `pages`, kept as the original bytes and in byte
    /// order. `sections` lives here too: we read `sections[0].id` out of it
    /// when we need it rather than modelling it, because anything we model is
    /// something we can degrade.
    root:  BTreeMap<String, Box<RawValue>>,
    /// The page shells, each `{content, id, sectionId}`.
    pages: Vec<Node>,
}

/// A document, as read from storage.
#[derive(Debug, Clone)]
pub struct Document {
    shape: Shape,
}

impl Document {
    /// Reads `content_json`.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, Error> {
        // The root is read key by key so that unknown keys keep their bytes and
        // their presence, exactly as nodes are.
        let root: BTreeMap<String, Box<RawValue>> = serde_json::from_slice(bytes)?;

        // A bare document is recognised by its own `type`, not by the absence
        // of `_type` — a future envelope key must not silently turn an envelope
        // into a bare doc.
        let is_doc = root
            .get("type")
            .and_then(|r| serde_json::from_str::<&str>(r.get()).ok())
            .is_some_and(|t| t == "doc");
        if is_doc {
            return Ok(Self { shape: Shape::Bare(Node::from_slice(bytes)?) });
        }

        let Some(pages_raw) = root.get("pages") else {
            return Err(Error::Shape("neither a doc node nor an envelope with pages"));
        };
        // Each shell is parsed with the page shape, whose `content` is one
        // document rather than an array of nodes.
        let shells: Vec<Box<RawValue>> = serde_json::from_str(pages_raw.get())?;
        let mut pages = Vec::with_capacity(shells.len());
        for shell in &shells {
            pages.push(Node::page_shell_from_str(shell.get())?);
        }

        let mut root = root;
        root.remove("pages");
        Ok(Self { shape: Shape::Envelope(Envelope { root, pages }) })
    }

    pub fn is_bare(&self) -> bool {
        matches!(self.shape, Shape::Bare(_))
    }

    pub fn page_count_stored(&self) -> usize {
        match &self.shape {
            Shape::Bare(_) => 1,
            Shape::Envelope(e) => e.pages.len(),
        }
    }

    /// The document body, flattened.
    ///
    /// The browser does the same on load (`flattenToDoc`,
    /// `DocumentEditorPage.tsx:4712-4727`): a stored document's pages are a
    /// snapshot of where the text happened to fall last time, not structure.
    /// Pagination is recomputed from the text every time it is laid out.
    pub fn blocks(&self) -> Vec<&Node> {
        match &self.shape {
            Shape::Bare(doc) => doc.children().iter().collect(),
            Shape::Envelope(e) => e
                .pages
                .iter()
                .filter_map(|shell| shell.document())
                .flat_map(|doc| doc.children().iter())
                .collect(),
        }
    }

    /// Replaces the body, keeping the storage shape and everything around it.
    pub fn set_blocks(&mut self, blocks: Vec<Node>) {
        match &mut self.shape {
            Shape::Bare(doc) => doc.set_children(blocks),
            Shape::Envelope(e) => {
                // One page out, whatever came in. Keeping `pages[0]` keeps its
                // `id` and `sectionId`; the rest are dropped rather than
                // emitted headless.
                if e.pages.is_empty() {
                    e.pages.push(Node::new());
                }
                e.pages.truncate(1);
                let mut body = e.pages[0].document().cloned().unwrap_or_else(|| Node::doc(vec![]));
                body.set_children(blocks);
                e.pages[0].set_document(body);
            }
        }
    }

    /// A root key of the envelope, parsed (`None` for a bare document or an absent key).
    pub fn root_value(&self, key: &str) -> Option<serde_json::Value> {
        match &self.shape {
            Shape::Bare(_) => None,
            Shape::Envelope(e) => serde_json::from_str(e.root.get(key)?.get()).ok(),
        }
    }

    /// Writes `sections[0].margins` (the page setup of the base section, as the web keeps it).
    ///
    /// A bare document has nowhere to keep margins, so this is the one write that turns it into
    /// an envelope (rule 1 of the module doc holds for every other edit): one page shell holding
    /// the body and one section, with fresh ids, as the web's `serializeDoc` writes them.
    pub fn set_section_margins(&mut self, margins: &serde_json::Value) {
        if let Shape::Bare(doc) = &self.shape {
            let section_id = new_id();
            let mut shell = Node::new();
            shell.set_document(doc.clone());
            shell.set_value("id", &serde_json::Value::String(new_id()));
            shell.set_value("sectionId", &serde_json::Value::String(section_id.clone()));
            let mut root = BTreeMap::new();
            if let Ok(raw) = RawValue::from_string("\"multi-page\"".to_string()) {
                root.insert("_type".to_string(), raw);
            }
            let sections = serde_json::json!([{ "id": section_id, "orientation": "portrait", "margins": margins }]);
            if let Ok(raw) = serde_json::value::to_raw_value(&sections) {
                root.insert("sections".to_string(), raw);
            }
            self.shape = Shape::Envelope(Envelope { root, pages: vec![shell] });
            return;
        }
        if let Shape::Envelope(e) = &mut self.shape {
            let mut sections: Vec<serde_json::Value> = e
                .root
                .get("sections")
                .and_then(|r| serde_json::from_str(r.get()).ok())
                .unwrap_or_default();
            if sections.is_empty() {
                sections.push(serde_json::json!({ "id": new_id(), "orientation": "portrait" }));
            }
            if let Some(obj) = sections[0].as_object_mut() {
                obj.insert("margins".into(), margins.clone());
            }
            if let Ok(raw) = serde_json::value::to_raw_value(&sections) {
                e.root.insert("sections".to_string(), raw);
            }
        }
    }

    /// Writes the document back to compact JSON.
    pub fn to_vec(&self) -> Result<Vec<u8>, Error> {
        match &self.shape {
            Shape::Bare(doc) => Ok(doc.to_vec()?),
            Shape::Envelope(e) => {
                // `pages` is put back into the root map so the whole object is
                // written in one pass, in byte order, like every other key.
                let mut root: BTreeMap<&str, &RawValue> =
                    e.root.iter().map(|(k, v)| (k.as_str(), v.as_ref())).collect();
                let pages_json = serde_json::to_string(&e.pages)?;
                let pages_raw = RawValue::from_string(pages_json)?;
                root.insert("pages", &pages_raw);
                Ok(serde_json::to_vec(&root)?)
            }
        }
    }
}

/// A random v4 UUID (the web uses `crypto.randomUUID()` for page and section ids). The bits come
/// from the standard library's per-process random hasher keys and a counter, so two ids of one
/// process never collide; no extra dependency, and nothing here is security-relevant.
pub fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let state = std::collections::hash_map::RandomState::new();
    let mut a = state.build_hasher();
    a.write_u64(n);
    a.write_usize(&COUNTER as *const _ as usize);
    let hi = a.finish();
    let mut b = state.build_hasher();
    b.write_u64(hi ^ 0x9E37_79B9_7F4A_7C15);
    b.write_u64(n.wrapping_mul(0x2545_F491_4F6C_DD1D));
    let lo = b.finish();
    let bytes: [u8; 16] = {
        let mut x = [0u8; 16];
        x[..8].copy_from_slice(&hi.to_le_bytes());
        x[8..].copy_from_slice(&lo.to_le_bytes());
        x[6] = (x[6] & 0x0f) | 0x40;
        x[8] = (x[8] & 0x3f) | 0x80;
        x
    };
    let h: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn margins_promote_a_bare_document_and_update_an_envelope() {
        let mut doc = Document::from_slice(BARE.as_bytes()).expect("parses");
        doc.set_section_margins(&serde_json::json!({"top":96,"right":96,"bottom":96,"left":120}));
        assert!(!doc.is_bare());
        let out = String::from_utf8(doc.to_vec().expect("serialises")).expect("utf-8");
        let back = Document::from_slice(out.as_bytes()).expect("reparses");
        assert_eq!(back.blocks().len(), 1);
        assert_eq!(back.root_value("sections").and_then(|s| s[0]["margins"]["left"].as_i64()), Some(120));
        let mut env = Document::from_slice(ENVELOPE.as_bytes()).expect("parses");
        env.set_section_margins(&serde_json::json!({"top":50,"right":96,"bottom":96,"left":96}));
        assert_eq!(env.root_value("sections").and_then(|s| s[0]["margins"]["top"].as_i64()), Some(50));
        assert_eq!(env.root_value("sections").and_then(|s| s[0]["id"].as_str().map(str::to_string)).as_deref(), Some("sec-0"));
        assert_ne!(new_id(), new_id());
        assert_eq!(new_id().len(), 36);
    }

    const BARE: &str = r#"{"content":[{"content":[{"text":"hello","type":"text"}],"type":"paragraph"}],"type":"doc"}"#;

    const ENVELOPE: &str = r#"{"_type":"multi-page","footer":{"content":[],"type":"doc"},"pages":[{"content":{"content":[{"type":"paragraph"}],"type":"doc"},"id":"page-0","sectionId":"sec-0"}],"sections":[{"id":"sec-0","orientation":"portrait"}]}"#;

    fn round_trip(src: &str) -> String {
        let doc = Document::from_slice(src.as_bytes()).expect("must parse");
        String::from_utf8(doc.to_vec().expect("must serialise")).expect("utf-8")
    }

    #[test]
    fn a_bare_document_round_trips_byte_for_byte() {
        assert_eq!(round_trip(BARE), BARE);
    }

    #[test]
    fn an_envelope_round_trips_byte_for_byte_with_its_sibling_keys() {
        // `footer` and `sections` are never modelled. That is the point: they
        // survive because nothing here can degrade them.
        assert_eq!(round_trip(ENVELOPE), ENVELOPE);
    }

    #[test]
    fn a_bare_document_is_recognised_as_bare() {
        let doc = Document::from_slice(BARE.as_bytes()).expect("parses");
        assert!(doc.is_bare());
        assert_eq!(doc.blocks().len(), 1);
        assert_eq!(doc.blocks()[0].node_type(), Some("paragraph"));
    }

    #[test]
    fn a_bare_document_is_never_promoted_to_an_envelope_by_a_save() {
        // The web mints fresh uuids and wraps; we do not. A first save must not
        // convert a fifth of the corpus to a different storage shape.
        let mut doc = Document::from_slice(BARE.as_bytes()).expect("parses");
        let blocks = doc.blocks().into_iter().cloned().collect();
        doc.set_blocks(blocks);
        let out = String::from_utf8(doc.to_vec().expect("serialises")).expect("utf-8");
        assert_eq!(out, BARE);
        assert!(!out.contains("_type"));
        assert!(!out.contains("pages"));
    }

    #[test]
    fn every_stored_page_contributes_to_the_body() {
        let three = r#"{"_type":"multi-page","pages":[{"content":{"content":[{"type":"a"}],"type":"doc"},"id":"p0","sectionId":"s0"},{"content":{"content":[{"type":"b"}],"type":"doc"},"id":"p1","sectionId":"s0"},{"content":{"content":[{"type":"c"}],"type":"doc"},"id":"p2","sectionId":"s0"}]}"#;
        let doc = Document::from_slice(three.as_bytes()).expect("parses");
        assert_eq!(doc.page_count_stored(), 3);
        let kinds: Vec<_> = doc.blocks().iter().filter_map(|n| n.node_type()).collect();
        assert_eq!(kinds, ["a", "b", "c"]);
    }

    #[test]
    fn saving_a_multi_page_envelope_emits_one_page_and_drops_the_rest() {
        // The extra shells must be DROPPED, not re-emitted without `content`:
        // `flattenToDoc` casts `pg.content` with no runtime check, so a headless
        // shell throws and the browser can no longer open the document.
        let three = r#"{"_type":"multi-page","pages":[{"content":{"content":[{"type":"a"}],"type":"doc"},"id":"p0","sectionId":"s0"},{"content":{"content":[{"type":"b"}],"type":"doc"},"id":"p1","sectionId":"s0"},{"content":{"content":[{"type":"c"}],"type":"doc"},"id":"p2","sectionId":"s0"}]}"#;
        let mut doc = Document::from_slice(three.as_bytes()).expect("parses");
        let blocks = doc.blocks().into_iter().cloned().collect();
        doc.set_blocks(blocks);
        let out = String::from_utf8(doc.to_vec().expect("serialises")).expect("utf-8");

        let back = Document::from_slice(out.as_bytes()).expect("reparses");
        assert_eq!(back.page_count_stored(), 1, "exactly one page shell survives");
        // The first shell's identity is kept; the others are gone entirely.
        assert!(out.contains(r#""id":"p0""#));
        assert!(!out.contains("p1") && !out.contains("p2"));
        // Nothing was lost from the body.
        let kinds: Vec<_> = back.blocks().iter().filter_map(|n| n.node_type()).collect();
        assert_eq!(kinds, ["a", "b", "c"]);
    }

    #[test]
    fn no_page_shell_is_ever_written_without_its_content() {
        let two = r#"{"_type":"multi-page","pages":[{"content":{"content":[],"type":"doc"},"id":"p0","sectionId":"s0"},{"content":{"content":[],"type":"doc"},"id":"p1","sectionId":"s0"}]}"#;
        let mut doc = Document::from_slice(two.as_bytes()).expect("parses");
        doc.set_blocks(vec![]);
        let out = String::from_utf8(doc.to_vec().expect("serialises")).expect("utf-8");
        let reparsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let pages = reparsed["pages"].as_array().expect("pages is an array");
        for page in pages {
            assert!(
                page.get("content").is_some(),
                "a page shell with no content crashes the web editor"
            );
        }
    }

    #[test]
    fn the_explicit_null_node_shape_survives_a_whole_document() {
        // Written by the server-side converters; 17 documents in 337 carry it.
        let src = r#"{"attrs":null,"content":[{"attrs":null,"content":null,"marks":null,"text":"x","type":"text"}],"marks":null,"text":null,"type":"doc"}"#;
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn something_that_is_neither_shape_is_refused_rather_than_guessed() {
        let err = Document::from_slice(br#"{"hello":"world"}"#).expect_err("must refuse");
        assert!(matches!(err, Error::Shape(_)));
    }

    #[test]
    fn a_document_with_no_pages_gains_exactly_one_when_a_body_is_written() {
        let mut doc = Document::from_slice(br#"{"_type":"multi-page","pages":[]}"#).expect("parses");
        doc.set_blocks(vec![Node::doc(vec![])]);
        let out = String::from_utf8(doc.to_vec().expect("serialises")).expect("utf-8");
        let back = Document::from_slice(out.as_bytes()).expect("reparses");
        assert_eq!(back.page_count_stored(), 1);
    }
}
