//! The Kubuno Delta Protocol v1 (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §7.1):
//! `GET …/delta?cursor=<opaque>&limit=` -> `{changes, cursor, has_more}`.
//!
//! The types are deliberately tolerant: a change keeps every field it carries in [`Change::data`], and an unknown
//! `kind` is kept as [`ChangeKind::Other`] (the engine skips it, it never reads it as a delete).

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

/// An opaque feed position. KDP feeds send an integer (`change_seq` of the last change of the page), the mail feed
/// a modseq string; both are kept as text. `"0"` asks for a full snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Cursor(String);

impl Cursor {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The start of a feed (full snapshot).
    pub fn zero() -> Self {
        Self("0".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_zero(&self) -> bool {
        self.0 == "0" || self.0.is_empty()
    }
}

impl Default for Cursor {
    fn default() -> Self {
        Self::zero()
    }
}

impl fmt::Display for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Cursor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match serde_json::Value::deserialize(d)? {
            serde_json::Value::Number(n) => Ok(Cursor(n.to_string())),
            serde_json::Value::String(s) => Ok(Cursor(s)),
            serde_json::Value::Null => Ok(Cursor::zero()),
            other => Err(serde::de::Error::custom(format!("a cursor must be a number or a string, not {other}"))),
        }
    }
}

/// What happened to the item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// Created or updated; the change carries the full row. A soft trash is a `modified` with `trashed = true`.
    Modified,
    /// Hard delete (tombstone).
    Deleted,
    /// The user lost access (sharing, CORE-S4).
    Revoked,
    /// Any other value: per-module kinds (drive's `file` / `folder`) or a kind this client does not know. Never
    /// interpreted as a delete.
    Other(String),
}

impl ChangeKind {
    pub fn as_str(&self) -> &str {
        match self {
            ChangeKind::Modified => "modified",
            ChangeKind::Deleted => "deleted",
            ChangeKind::Revoked => "revoked",
            ChangeKind::Other(s) => s,
        }
    }
}

impl<'de> Deserialize<'de> for ChangeKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "modified" | "upsert" | "created" | "updated" => ChangeKind::Modified,
            "deleted" => ChangeKind::Deleted,
            "revoked" => ChangeKind::Revoked,
            _ => ChangeKind::Other(s),
        })
    }
}

impl Serialize for ChangeKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// One change of a feed.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Change {
    /// The item id (`uuid` in KDP; drive's feed says `id`).
    #[serde(alias = "id")]
    pub uuid: String,
    pub kind: ChangeKind,
    /// The server's commit-ordered sequence number of this change (the only ordering the client trusts).
    #[serde(default)]
    pub change_seq: i64,
    /// Every other field: the entity under its own key (`"note": {…}`) or inline, `etag`, `full`...
    #[serde(flatten)]
    pub data: serde_json::Map<String, serde_json::Value>,
}

impl Change {
    /// The entity row: `data[key]` when the feed nests it under the entity name, else the change's own fields.
    pub fn row(&self, key: &str) -> serde_json::Value {
        match self.data.get(key) {
            Some(v @ serde_json::Value::Object(_)) => v.clone(),
            _ => serde_json::Value::Object(self.data.clone()),
        }
    }

    /// The row's etag/version, when the change carries one (top level or inside the nested row).
    pub fn etag(&self, key: &str) -> Option<String> {
        let pick = |m: &serde_json::Map<String, serde_json::Value>| {
            m.get("etag").or_else(|| m.get("version")).and_then(|v| match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
        };
        pick(&self.data).or_else(|| self.data.get(key).and_then(|v| v.as_object()).and_then(pick))
    }
}

/// One page of a feed.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeltaPage<C = Change> {
    pub changes: Vec<C>,
    pub cursor: Cursor,
    #[serde(default)]
    pub has_more: bool,
}

/// The paging guard of §7.2: a page that says `has_more` but does not move the cursor would loop forever.
pub fn check_progress(previous: &Cursor, page_cursor: &Cursor, has_more: bool) -> Result<(), String> {
    if has_more && previous == page_cursor {
        return Err(format!("the feed returned has_more with an unchanged cursor ({previous})"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kdp_page_parses_with_numeric_cursor() {
        let page: DeltaPage = serde_json::from_str(
            r#"{"changes":[{"uuid":"a","kind":"modified","change_seq":7,"note":{"title":"x","etag":"e1"}},
                           {"uuid":"b","kind":"deleted","change_seq":8},
                           {"id":"c","kind":"folder","change_seq":9,"name":"Docs"}],
                "cursor":9,"has_more":false}"#,
        )
        .expect("parse");
        assert_eq!(page.cursor.as_str(), "9");
        assert_eq!(page.changes[0].kind, ChangeKind::Modified);
        assert_eq!(page.changes[0].row("note")["title"], "x");
        assert_eq!(page.changes[0].etag("note").as_deref(), Some("e1"));
        assert_eq!(page.changes[1].kind, ChangeKind::Deleted);
        assert_eq!(page.changes[2].uuid, "c");
        assert_eq!(page.changes[2].kind, ChangeKind::Other("folder".into()));
        assert_eq!(page.changes[2].row("folder")["name"], "Docs");
    }

    #[test]
    fn string_cursor_and_guard() {
        let page: DeltaPage = serde_json::from_str(r#"{"changes":[],"cursor":"m-42","has_more":true}"#).expect("parse");
        assert_eq!(page.cursor, Cursor::new("m-42"));
        assert!(check_progress(&Cursor::new("m-42"), &page.cursor, page.has_more).is_err());
        assert!(check_progress(&Cursor::new("m-41"), &page.cursor, page.has_more).is_ok());
        assert!(check_progress(&Cursor::new("m-42"), &page.cursor, false).is_ok());
    }
}
