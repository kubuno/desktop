//! Conflict policies (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §7.4). A policy runs when the server answers 409/412
//! to an intent; ordering always comes from the server (`change_seq`, etags), never from a local clock.

use std::sync::Arc;

use serde_json::{Map, Value};

/// What to do with a rejected intent and the current server row.
#[derive(Clone, Default)]
pub enum ConflictPolicy {
    /// Field-level three-way merge with the intent's `base_row`: fields changed only locally are re-sent, fields
    /// changed only on the server are kept, fields changed on both sides to different values keep the server value
    /// and are recorded as a `field` conflict the user can resolve ("Garder la mienne" / "Garder celle du serveur").
    #[default]
    FieldMerge,
    /// Keep both versions: the local one becomes a copy (`<titre> (conflit <machine> <date>)`), the server one
    /// stays (note bodies and document contents without CRDT, files).
    KeepBoth,
    /// The server version wins silently (derived data, caches).
    ServerWins,
    /// The local intent is re-sent on top of the server version (only where the module allows last-writer-wins;
    /// "last" is the order in which the server receives the writes, not a timestamp).
    LastWriterWins,
    /// Decided by the app.
    Custom(Arc<dyn CustomPolicy>),
}

impl std::fmt::Debug for ConflictPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ConflictPolicy::FieldMerge => "FieldMerge",
            ConflictPolicy::KeepBoth => "KeepBoth",
            ConflictPolicy::ServerWins => "ServerWins",
            ConflictPolicy::LastWriterWins => "LastWriterWins",
            ConflictPolicy::Custom(_) => "Custom",
        })
    }
}

/// An app-defined policy: returns the fields to re-send (empty = accept the server row) and the fields to report.
pub trait CustomPolicy: Send + Sync {
    fn resolve(&self, base: Option<&Map<String, Value>>, local: &Map<String, Value>, server: &Map<String, Value>) -> MergeResult;
}

/// The outcome of a merge.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MergeResult {
    /// Fields to send again on top of the server version.
    pub resend: Map<String, Value>,
    /// Fields changed on both sides to different values (server value kept).
    pub conflicting: Vec<String>,
}

/// Equality across the SQLite round trip: a boolean comes back from a local row as 0/1.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Bool(x), Value::Number(n)) | (Value::Number(n), Value::Bool(x)) => n.as_i64() == Some(i64::from(*x)),
        _ => a == b,
    }
}

/// Three-way merge of a patch: `local` = the intent's fields, `base` = the row they were made on, `server` = the
/// current server row. A field missing from `base` counts as changed on the server when the server has it.
pub fn three_way_merge(base: Option<&Map<String, Value>>, local: &Map<String, Value>, server: &Map<String, Value>) -> MergeResult {
    let mut out = MergeResult::default();
    for (field, mine) in local {
        if field == "id" {
            continue;
        }
        let theirs = server.get(field).unwrap_or(&Value::Null);
        let original = base.and_then(|b| b.get(field));
        if same(theirs, mine) {
            continue; // both sides agree
        }
        match original {
            Some(o) if same(o, theirs) => {
                out.resend.insert(field.clone(), mine.clone()); // only changed locally
            }
            _ => out.conflicting.push(field.clone()), // changed on both sides (or unknown base)
        }
    }
    out.conflicting.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn m(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn different_fields_merge_without_conflict() {
        let base = m(json!({"title": "A", "body": "x"}));
        let local = m(json!({"title": "B"}));
        let server = m(json!({"title": "A", "body": "y"}));
        let r = three_way_merge(Some(&base), &local, &server);
        assert_eq!(r.resend, m(json!({"title": "B"})));
        assert!(r.conflicting.is_empty());
    }

    #[test]
    fn same_field_different_values_conflicts_server_kept() {
        let base = m(json!({"title": "A"}));
        let local = m(json!({"title": "B"}));
        let server = m(json!({"title": "C"}));
        let r = three_way_merge(Some(&base), &local, &server);
        assert!(r.resend.is_empty());
        assert_eq!(r.conflicting, vec!["title".to_string()]);
    }

    #[test]
    fn same_value_on_both_sides_is_no_conflict() {
        let base = m(json!({"title": "A"}));
        let r = three_way_merge(Some(&base), &m(json!({"title": "B"})), &m(json!({"title": "B"})));
        assert_eq!(r, MergeResult::default());
    }

    #[test]
    fn unknown_base_is_a_conflict_when_values_differ() {
        let r = three_way_merge(None, &m(json!({"title": "B"})), &m(json!({"title": "C"})));
        assert_eq!(r.conflicting, vec!["title".to_string()]);
    }
}
