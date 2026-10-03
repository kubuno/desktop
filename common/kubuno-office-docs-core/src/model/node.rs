//! The ProseMirror node, held so that writing it back cannot lose anything.
//!
//! # Why this is not a struct
//!
//! The obvious shape —
//!
//! ```ignore
//! struct Node { #[serde(rename = "type")] node_type: String, attrs: Option<Value>, … }
//! ```
//!
//! — cannot round-trip this format, whichever way it is tuned. **Two different
//! serialisers write these documents.** The browser's `JSON.stringify` on
//! `editor.getJSON()` omits keys that are absent. The server's
//! `serde::Serialize` on `PmNode` (`office/src/converters/types.rs:4-16`, which
//! declares `#[serde(default)]` with no `skip_serializing_if`) writes them as
//! explicit `null`. Both shapes are in the wild, and both occur **inside the
//! same file**: of a 337-document sample, 17 contain the explicit-null shape and
//! 5 are entirely it. A struct with `skip_serializing_if` destroys those 17; one
//! without it destroys the other 320.
//!
//! So a node is an ordered map of whatever keys it actually had, and each value
//! is kept as the original bytes unless we need to descend into it. Presence is
//! preserved as distinct from `null`, because in this format they are different
//! documents.
//!
//! # What this buys
//!
//! Losslessness becomes a property of the type rather than of a checklist
//! someone has to remember. A tracked-changes mark, a comment anchor, a rich
//! text box's entire sub-document hidden in an `alt` attribute, an attribute
//! added to the web editor next year — none of them need to be modelled here to
//! survive a save, because none of them are ever parsed.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

/// One key's value inside a node or page object.
#[derive(Debug, Clone)]
pub enum Slot {
    /// The original bytes, never parsed and never reformatted.
    Raw(Box<RawValue>),
    /// A `content` array of child nodes.
    Nodes(Vec<Node>),
    /// A `content` value that is a single node — a page shell's body, which is
    /// a whole `{"type":"doc",…}` rather than an array.
    Doc(Box<Node>),
}

impl Slot {
    fn raw_is_null(raw: &RawValue) -> bool {
        raw.get().trim() == "null"
    }
}

/// A ProseMirror node, or a page shell — anything shaped like a JSON object
/// whose `content` we may need to walk.
#[derive(Debug, Clone, Default)]
pub struct Node {
    /// Every key the object actually carried. `BTreeMap` orders by key bytes,
    /// which is the order the browser's and the server's serialisers both
    /// already emit, so a compact document re-serialises to the same bytes.
    fields: BTreeMap<String, Slot>,
}

/// How `content` should be read for a given object.
#[derive(Clone, Copy, PartialEq)]
enum ContentShape {
    /// `content` is an array of nodes — an ordinary ProseMirror node.
    Array,
    /// `content` is one node — a page shell.
    Single,
}

impl Node {
    pub fn new() -> Self {
        Self::default()
    }

    /// The node's `type`, if it has one and it is a string.
    pub fn node_type(&self) -> Option<&str> {
        self.raw("type").and_then(|r| serde_json::from_str::<&str>(r.get()).ok())
    }

    /// The node's `text`, if it has one and it is a string.
    ///
    /// Returns an owned `String` rather than a borrow: JSON strings may carry
    /// escapes, so the decoded text does not always exist inside the buffer.
    pub fn text(&self) -> Option<String> {
        self.raw("text").and_then(|r| serde_json::from_str::<String>(r.get()).ok())
    }

    /// The raw bytes of one key, if present. `None` for an absent key; a
    /// present-but-null key returns the four bytes `null`.
    pub fn raw(&self, key: &str) -> Option<&RawValue> {
        match self.fields.get(key) {
            Some(Slot::Raw(r)) => Some(r),
            _ => None,
        }
    }

    /// Whether the key is present at all — which is NOT the same question as
    /// whether it is null. See the module comment.
    pub fn has(&self, key: &str) -> bool {
        self.fields.contains_key(key)
    }

    /// The child nodes, for a node whose `content` is an array.
    pub fn children(&self) -> &[Node] {
        match self.fields.get("content") {
            Some(Slot::Nodes(n)) => n,
            _ => &[],
        }
    }

    /// The single child node, for a page shell whose `content` is one document.
    pub fn document(&self) -> Option<&Node> {
        match self.fields.get("content") {
            Some(Slot::Doc(n)) => Some(n),
            _ => None,
        }
    }

    /// Replaces the `content` array, keeping every other key untouched.
    ///
    /// Inserts `content` if the node had none — which changes the bytes, so it
    /// is only ever called on a node whose body we are deliberately rewriting.
    pub fn set_children(&mut self, children: Vec<Node>) {
        self.fields.insert("content".into(), Slot::Nodes(children));
    }

    /// Replaces the single `content` document of a page shell.
    pub fn set_document(&mut self, doc: Node) {
        self.fields.insert("content".into(), Slot::Doc(Box::new(doc)));
    }

    /// Sets one key from raw JSON bytes. The caller owns their validity —
    /// `RawValue` does not revalidate.
    pub fn set_raw(&mut self, key: &str, raw: Box<RawValue>) {
        self.fields.insert(key.into(), Slot::Raw(raw));
    }

    /// Removes a key, returning whether it was there.
    ///
    /// Removing a key is not the same as setting it to null — in this format the
    /// two are different documents (see the module comment) — so undo of a
    /// "clear formatting" needs to put the key back exactly as it was, absent or
    /// null, which is why this returns the distinction to the caller.
    pub fn remove(&mut self, key: &str) -> bool {
        self.fields.remove(key).is_some()
    }

    /// Builds a `{"type":"doc","content":[…]}` node.
    pub fn doc(children: Vec<Node>) -> Self {
        let mut node = Node::new();
        // `RawValue::from_string` only fails on invalid JSON; this literal is a
        // valid JSON string, so the fallback is unreachable in practice and is
        // an empty node rather than a panic.
        if let Ok(raw) = RawValue::from_string("\"doc\"".to_string()) {
            node.set_raw("type", raw);
        }
        node.set_children(children);
        node
    }

    /// The child nodes, mutable — `None` when `content` is absent, `null` or not an array (an edit
    /// that must *create* the array calls [`Node::set_children`] instead, deliberately).
    pub fn children_mut(&mut self) -> Option<&mut Vec<Node>> {
        match self.fields.get_mut("content") {
            Some(Slot::Nodes(n)) => Some(n),
            _ => None,
        }
    }

    /// Whether `content` is an array of nodes (possibly empty).
    pub fn has_child_array(&self) -> bool {
        matches!(self.fields.get("content"), Some(Slot::Nodes(_)))
    }

    /// The keys this node carries, in byte order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.fields.keys().map(String::as_str)
    }

    /// Replaces the `text` of a text node. Re-encodes that one string (serde's escaping); every
    /// other key keeps its bytes.
    pub fn set_text(&mut self, text: &str) {
        if let Ok(json) = serde_json::to_string(text) {
            if let Ok(raw) = RawValue::from_string(json) {
                self.set_raw("text", raw);
            }
        }
    }

    /// Sets a key from a JSON value (serialised compactly).
    pub fn set_value(&mut self, key: &str, value: &serde_json::Value) {
        if let Ok(json) = serde_json::to_string(value) {
            if let Ok(raw) = RawValue::from_string(json) {
                self.set_raw(key, raw);
            }
        }
    }

    /// A key parsed as a JSON value (`None` when absent or not valid JSON).
    pub fn value(&self, key: &str) -> Option<serde_json::Value> {
        serde_json::from_str(self.raw(key)?.get()).ok()
    }

    /// `attrs` as a JSON object map, empty when absent, `null` or not an object.
    pub fn attrs(&self) -> serde_json::Map<String, serde_json::Value> {
        match self.value("attrs") {
            Some(serde_json::Value::Object(m)) => m,
            _ => serde_json::Map::new(),
        }
    }

    /// One attribute (`attrs.key`), `None` when absent or `null`.
    pub fn attr(&self, key: &str) -> Option<serde_json::Value> {
        self.attrs().remove(key).filter(|v| !v.is_null())
    }

    /// A node of `type` with nothing else.
    pub fn of_type(node_type: &str) -> Self {
        let mut node = Node::new();
        node.set_value("type", &serde_json::Value::String(node_type.to_string()));
        node
    }

    /// A container node `{"content":[…],"type":…}` (an empty `content` array included, which is
    /// what ProseMirror's `toJSON` omits but the server writes; both read back identically).
    pub fn element(node_type: &str, attrs: Option<serde_json::Value>, children: Vec<Node>) -> Self {
        let mut node = Node::of_type(node_type);
        if let Some(attrs) = attrs {
            node.set_value("attrs", &attrs);
        }
        if !children.is_empty() {
            node.set_children(children);
        }
        node
    }

    /// A text node `{"marks":…,"text":…,"type":"text"}`; `marks` is the raw JSON of the marks
    /// array (`None`: no `marks` key).
    pub fn text_node(text: &str, marks: Option<&RawValue>) -> Self {
        let mut node = Node::of_type("text");
        node.set_text(text);
        if let Some(m) = marks {
            if let Ok(raw) = RawValue::from_string(m.get().to_string()) {
                node.set_raw("marks", raw);
            }
        }
        node
    }

    /// Reads a node from JSON bytes.
    pub fn from_slice(bytes: &[u8]) -> serde_json::Result<Self> {
        serde_json::from_slice(bytes)
    }

    /// Writes the node back to compact JSON.
    pub fn to_vec(&self) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(self)
    }

    fn deserialize_with<'de, D>(d: D, shape: ContentShape) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct NodeVisitor(ContentShape);

        impl<'de> Visitor<'de> for NodeVisitor {
            type Value = Node;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a ProseMirror node object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
                let mut fields = BTreeMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    let raw: Box<RawValue> = map.next_value()?;
                    // `content` is the only key we descend into, and only when
                    // it actually holds something. An explicit null stays raw,
                    // so `"content":null` comes back out as `"content":null`
                    // rather than as an empty array.
                    let slot = if key == "content" && !Slot::raw_is_null(&raw) {
                        match self.0 {
                            ContentShape::Array => {
                                match serde_json::from_str::<Vec<Node>>(raw.get()) {
                                    Ok(nodes) => Slot::Nodes(nodes),
                                    // Not an array after all: keep the bytes
                                    // rather than refusing to open the document.
                                    Err(_) => Slot::Raw(raw),
                                }
                            }
                            ContentShape::Single => {
                                match Node::page_content_from_str(raw.get()) {
                                    Ok(node) => Slot::Doc(Box::new(node)),
                                    Err(_) => Slot::Raw(raw),
                                }
                            }
                        }
                    } else {
                        Slot::Raw(raw)
                    };
                    fields.insert(key, slot);
                }
                Ok(Node { fields })
            }
        }

        d.deserialize_map(NodeVisitor(shape))
    }

    /// A page shell's `content`: one node whose own `content` is an array.
    fn page_content_from_str(s: &str) -> serde_json::Result<Node> {
        serde_json::from_str(s)
    }

    /// Reads an object whose `content` key holds a single node rather than an
    /// array — a page shell.
    pub fn page_shell_from_str(s: &str) -> serde_json::Result<Node> {
        let mut de = serde_json::Deserializer::from_str(s);
        Node::deserialize_with(&mut de, ContentShape::Single)
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Node::deserialize_with(d, ContentShape::Array)
    }
}

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(self.fields.len()))?;
        for (key, slot) in &self.fields {
            match slot {
                Slot::Raw(raw) => map.serialize_entry(key, raw)?,
                Slot::Nodes(nodes) => map.serialize_entry(key, nodes)?,
                Slot::Doc(node) => map.serialize_entry(key, node)?,
            }
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(src: &str) -> String {
        let node = Node::from_slice(src.as_bytes()).expect("fixture must parse");
        String::from_utf8(node.to_vec().expect("must serialise")).expect("utf-8")
    }

    #[test]
    fn a_compact_node_round_trips_byte_for_byte() {
        let src = r#"{"attrs":{"textAlign":"justify"},"content":[{"text":"hi","type":"text"}],"type":"paragraph"}"#;
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn the_servers_explicit_null_shape_survives() {
        // Written by office/src/converters: #[serde(default)] with no
        // skip_serializing_if, so absent keys are emitted as null.
        let src = r#"{"attrs":null,"content":null,"marks":null,"text":"x","type":"text"}"#;
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn the_browsers_compact_shape_survives_in_the_same_document() {
        // Both shapes occur in one file; a model that normalises either way
        // corrupts one of them.
        let src = r#"{"content":[{"attrs":null,"content":null,"marks":null,"text":"a","type":"text"},{"text":"b","type":"text"}],"type":"paragraph"}"#;
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn an_absent_key_and_a_null_key_stay_different_documents() {
        let absent = r#"{"text":"x","type":"text"}"#;
        let null = r#"{"attrs":null,"text":"x","type":"text"}"#;
        assert_eq!(round_trip(absent), absent);
        assert_eq!(round_trip(null), null);
        assert_ne!(round_trip(absent), round_trip(null));
    }

    #[test]
    fn a_float_that_does_not_survive_a_naive_parse_is_kept_verbatim() {
        // The DOCX importer's twips-to-px conversion produces exactly this
        // shape. It is never parsed, so it cannot be degraded.
        let src = r#"{"attrs":{"indent":47.999999999999996,"width":2.0},"type":"paragraph"}"#;
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn an_unknown_node_type_with_unknown_attributes_survives_whole() {
        let src = r#"{"attrs":{"nested":{"deep":[1,2,{"x":null}]}},"content":[{"type":"alsoUnknown"}],"type":"somethingFromNextYear"}"#;
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn tracked_changes_marks_survive_without_being_modelled() {
        let src = r#"{"marks":[{"attrs":{"author":"Lina Vasseur","date":"2026-03-04T09:12:00Z","id":"rev-7"},"type":"insertion"}],"text":"new","type":"text"}"#;
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn a_whole_sub_document_hidden_in_an_attribute_survives() {
        // A rich text box stores its entire ProseMirror sub-document in
        // `image.alt`. Trimming or re-encoding that attribute deletes user prose.
        let src = r#"{"attrs":{"alt":"kbtextrich:%7B%22type%22%3A%22doc%22%7D","src":"data:image/svg+xml,x"},"type":"image"}"#;
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn font_size_is_read_back_as_written_whether_string_or_number() {
        // Two serialisers, two shapes, both common in real data.
        for src in [
            r#"{"marks":[{"attrs":{"fontSize":"13pt"},"type":"textStyle"}],"text":"a","type":"text"}"#,
            r#"{"marks":[{"attrs":{"fontSize":11.0},"type":"textStyle"}],"text":"a","type":"text"}"#,
            r#"{"marks":[{"attrs":{"fontSize":18},"type":"textStyle"}],"text":"a","type":"text"}"#,
        ] {
            assert_eq!(round_trip(src), src);
        }
    }

    #[test]
    fn accessors_read_what_the_bytes_say() {
        let node = Node::from_slice(
            br#"{"content":[{"text":"hello","type":"text"}],"type":"paragraph"}"#,
        )
        .expect("parses");
        assert_eq!(node.node_type(), Some("paragraph"));
        assert_eq!(node.children().len(), 1);
        assert_eq!(node.children()[0].text().as_deref(), Some("hello"));
        assert!(node.has("content"));
        assert!(!node.has("marks"));
    }

    #[test]
    fn an_escaped_string_decodes_when_read_and_stays_escaped_when_written() {
        let src = r#"{"text":"a\nbé","type":"text"}"#;
        let node = Node::from_slice(src.as_bytes()).expect("parses");
        assert_eq!(node.text().as_deref(), Some("a\nbé"));
        // The bytes are never re-encoded, so the document does not drift just
        // because we looked at it.
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn a_page_shell_holds_one_document_not_an_array() {
        let src = r#"{"content":{"content":[{"type":"paragraph"}],"type":"doc"},"id":"page-0","sectionId":"sec-0"}"#;
        let shell = Node::page_shell_from_str(src).expect("parses");
        assert_eq!(shell.document().and_then(|d| d.node_type()), Some("doc"));
        let out = String::from_utf8(shell.to_vec().expect("serialises")).expect("utf-8");
        assert_eq!(out, src);
    }
}
