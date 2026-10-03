//! How the engine reads and writes an app's entity table and maps outbox intents to HTTP requests.
//!
//! [`EntityAdapter`] is the extension point (one per synced entity, written by the app or generated later from a
//! `.kbdata [sync]` section, lot SE-4). [`JsonTableAdapter`] implements it by convention for the common case: a
//! table whose columns are the JSON fields of the entity plus the engine columns `_etag`, `_seq`, `_pending`, and
//! a REST collection (`POST {collection}`, `PATCH {collection}/{id}` with `If-Match`, `DELETE {collection}/{id}`
//! with `If-Match`).

use kubuno_desktop_api_client::{ApiClient, ApiError, ApiRequest, ApiResponse, Change};
use serde_json::{Map, Value};
use sqlx::SqliteConnection;

use crate::conflict::ConflictPolicy;
use crate::error::SyncError;
use crate::outbox::OutboxOp;

/// One synced entity.
#[async_trait::async_trait]
pub trait EntityAdapter: Send + Sync {
    /// Outbox entity name (`note`).
    fn entity(&self) -> &str;
    /// The table holding the rows (`notes`): used for the `_pending` column and change notifications.
    fn table(&self) -> &str;
    fn conflict_policy(&self) -> ConflictPolicy {
        ConflictPolicy::FieldMerge
    }

    /// Writes a server version of a row (pull, push answer, rollback): the entity's fields plus `_etag`/`_seq`.
    async fn upsert_server(&self, conn: &mut SqliteConnection, id: &str, row: &Value, etag: Option<&str>, seq: Option<i64>) -> Result<(), SyncError>;
    /// Writes a local version (local create/patch): only the given fields; `_etag`/`_seq` untouched.
    async fn write_local(&self, conn: &mut SqliteConnection, id: &str, fields: &Map<String, Value>) -> Result<(), SyncError>;
    async fn delete_row(&self, conn: &mut SqliteConnection, id: &str) -> Result<(), SyncError>;
    /// The entity's fields of the visible row (no engine column), `None` when absent.
    async fn read_row(&self, conn: &mut SqliteConnection, id: &str) -> Result<Option<Map<String, Value>>, SyncError>;
    /// `(_etag, _seq)` of the visible row.
    async fn row_meta(&self, conn: &mut SqliteConnection, id: &str) -> Result<Option<(Option<String>, Option<i64>)>, SyncError>;
    /// Every id of the table (full resync: rows the server no longer has).
    async fn list_ids(&self, conn: &mut SqliteConnection) -> Result<Vec<String>, SyncError>;
    /// Sets `_pending` of a row.
    async fn set_pending(&self, conn: &mut SqliteConnection, id: &str, pending: i64) -> Result<(), SyncError>;

    /// The row carried by a feed change.
    fn row_from_change(&self, change: &Change) -> Value;
    /// The etag carried by a feed change.
    fn etag_from_change(&self, change: &Change) -> Option<String>;
    /// The HTTP request of an intent. The engine adds the `Idempotency-Key`.
    fn build_request(&self, op: &OutboxOp) -> Result<ApiRequest, SyncError>;
    /// The authoritative row in a successful answer (`None`: keep the local row).
    fn row_from_response(&self, op: &OutboxOp, resp: &ApiResponse) -> Option<Value>;
    /// The current server row in a 409/412 answer body.
    fn row_from_conflict(&self, body: &Value) -> Option<Value>;
    /// Fetches the current server row when a conflict answer did not carry it.
    async fn fetch_row(&self, _api: &ApiClient, _id: &str) -> Result<Option<Value>, ApiError> {
        Ok(None)
    }
    /// Keep-both policy: the copy to create from the local version (new id, fields), `None` if not supported.
    fn keep_both_copy(&self, _op: &OutboxOp, _local: &Map<String, Value>) -> Option<(String, Map<String, Value>)> {
        None
    }
    /// The etag of a server row.
    fn etag_of(&self, row: &Value) -> Option<String> {
        match row.get("etag").or_else(|| row.get("version")) {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Number(n)) => Some(n.to_string()),
            _ => None,
        }
    }
}

/// Percent-encodes an id for use as one URL path segment (an id is data: it must never add a `/` or `..`).
pub fn encode_segment(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for b in id.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'~' => out.push(b as char),
            b'.' if id != "." && id != ".." => out.push('.'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Whether `s` is a plain SQL identifier (table and column names are interpolated, so they are validated).
pub fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_') && s.len() <= 64
}

/// The convention-based adapter.
#[derive(Debug, Clone)]
pub struct JsonTableAdapter {
    entity: String,
    table: String,
    key: String,
    columns: Vec<String>,
    collection: String,
    row_key: String,
    policy: ConflictPolicy,
    title_field: Option<String>,
    send_if_match: bool,
    machine: String,
}

impl JsonTableAdapter {
    /// `entity` (outbox name and the key nesting rows in feeds/answers), `table`, synced `columns` (JSON field =
    /// column), `collection` (REST base path, e.g. `/api/v1/notes/notes`).
    pub fn new(entity: &str, table: &str, columns: &[&str], collection: &str) -> Result<Self, SyncError> {
        for name in std::iter::once(table).chain(columns.iter().copied()) {
            if !is_identifier(name) {
                return Err(SyncError::Config(format!("'{name}' is not a valid SQL identifier")));
            }
        }
        if columns.contains(&"id") {
            return Err(SyncError::Config("'id' is the key column, do not list it among the columns".into()));
        }
        Ok(Self {
            entity: entity.to_string(),
            table: table.to_string(),
            key: "id".to_string(),
            columns: columns.iter().map(|c| c.to_string()).collect(),
            collection: collection.trim_end_matches('/').to_string(),
            row_key: entity.to_string(),
            policy: ConflictPolicy::FieldMerge,
            title_field: None,
            send_if_match: true,
            machine: std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "desktop".into()),
        })
    }

    pub fn policy(mut self, policy: ConflictPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// The field that names an item (`title`): keep-both copies get `<title> (conflit <machine> <date>)`.
    pub fn title_field(mut self, field: &str) -> Self {
        self.title_field = Some(field.to_string());
        self
    }

    /// Whether PATCH/DELETE carry `If-Match` (false for modules without etags: pure last-writer-wins).
    pub fn send_if_match(mut self, yes: bool) -> Self {
        self.send_if_match = yes;
        self
    }

    pub fn row_key(mut self, key: &str) -> Self {
        self.row_key = key.to_string();
        self
    }

    /// Name used in keep-both copies (tests).
    pub fn machine_name(mut self, name: &str) -> Self {
        self.machine = name.to_string();
        self
    }

    fn json_object_sql(&self) -> String {
        let pairs: Vec<String> = self.columns.iter().map(|c| format!("'{c}', \"{c}\"")).collect();
        format!("json_object('id', \"{}\"{}{})", self.key, if pairs.is_empty() { "" } else { ", " }, pairs.join(", "))
    }
}

fn bind_value<'q>(
    q: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    v: Option<&Value>,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    match v {
        None | Some(Value::Null) => q.bind(None::<String>),
        Some(Value::Bool(b)) => q.bind(i64::from(*b)),
        Some(Value::Number(n)) => match n.as_i64() {
            Some(i) => q.bind(i),
            None => q.bind(n.as_f64()),
        },
        Some(Value::String(s)) => q.bind(s.clone()),
        Some(other) => q.bind(other.to_string()),
    }
}

#[async_trait::async_trait]
impl EntityAdapter for JsonTableAdapter {
    fn entity(&self) -> &str {
        &self.entity
    }

    fn table(&self) -> &str {
        &self.table
    }

    fn conflict_policy(&self) -> ConflictPolicy {
        self.policy.clone()
    }

    async fn upsert_server(&self, conn: &mut SqliteConnection, id: &str, row: &Value, etag: Option<&str>, seq: Option<i64>) -> Result<(), SyncError> {
        let cols: Vec<String> = self.columns.iter().map(|c| format!("\"{c}\"")).collect();
        let placeholders = vec!["?"; self.columns.len() + 3].join(", ");
        let updates: Vec<String> = self.columns.iter().map(|c| format!("\"{c}\" = excluded.\"{c}\"")).collect();
        let sql = format!(
            "INSERT INTO \"{t}\" (\"{k}\"{sep}{cols}, _etag, _seq) VALUES ({placeholders})
             ON CONFLICT(\"{k}\") DO UPDATE SET {updates}{sep2} _etag = excluded._etag, _seq = COALESCE(excluded._seq, \"{t}\"._seq)",
            t = self.table,
            k = self.key,
            sep = if cols.is_empty() { "" } else { ", " },
            cols = cols.join(", "),
            updates = updates.join(", "),
            sep2 = if updates.is_empty() { "" } else { "," },
        );
        let mut q = sqlx::query(&sql).bind(id.to_string());
        for c in &self.columns {
            q = bind_value(q, row.get(c));
        }
        q = q.bind(etag.map(str::to_string)).bind(seq);
        q.execute(conn).await.map_err(SyncError::db("upsert server row"))?;
        Ok(())
    }

    async fn write_local(&self, conn: &mut SqliteConnection, id: &str, fields: &Map<String, Value>) -> Result<(), SyncError> {
        let known: Vec<&String> = self.columns.iter().filter(|c| fields.contains_key(c.as_str())).collect();
        let cols: Vec<String> = known.iter().map(|c| format!("\"{c}\"")).collect();
        let placeholders = vec!["?"; known.len() + 1].join(", ");
        let updates: Vec<String> = known.iter().map(|c| format!("\"{c}\" = excluded.\"{c}\"")).collect();
        let sql = if updates.is_empty() {
            format!("INSERT INTO \"{t}\" (\"{k}\") VALUES (?) ON CONFLICT(\"{k}\") DO NOTHING", t = self.table, k = self.key)
        } else {
            format!(
                "INSERT INTO \"{t}\" (\"{k}\", {cols}) VALUES ({placeholders}) ON CONFLICT(\"{k}\") DO UPDATE SET {updates}",
                t = self.table,
                k = self.key,
                cols = cols.join(", "),
                updates = updates.join(", "),
            )
        };
        let mut q = sqlx::query(&sql).bind(id.to_string());
        for c in &known {
            q = bind_value(q, fields.get(c.as_str()));
        }
        q.execute(conn).await.map_err(SyncError::db("write local row"))?;
        Ok(())
    }

    async fn delete_row(&self, conn: &mut SqliteConnection, id: &str) -> Result<(), SyncError> {
        sqlx::query(&format!("DELETE FROM \"{}\" WHERE \"{}\" = ?", self.table, self.key))
            .bind(id)
            .execute(conn)
            .await
            .map_err(SyncError::db("delete row"))?;
        Ok(())
    }

    async fn read_row(&self, conn: &mut SqliteConnection, id: &str) -> Result<Option<Map<String, Value>>, SyncError> {
        let text: Option<String> = sqlx::query_scalar(&format!("SELECT {} FROM \"{}\" WHERE \"{}\" = ?", self.json_object_sql(), self.table, self.key))
            .bind(id)
            .fetch_optional(conn)
            .await
            .map_err(SyncError::db("read row"))?;
        match text {
            None => Ok(None),
            Some(t) => match serde_json::from_str::<Value>(&t) {
                Ok(Value::Object(mut m)) => {
                    m.remove("id");
                    Ok(Some(m))
                }
                _ => Err(SyncError::Db("json_object returned no object".into())),
            },
        }
    }

    async fn row_meta(&self, conn: &mut SqliteConnection, id: &str) -> Result<Option<(Option<String>, Option<i64>)>, SyncError> {
        sqlx::query_as::<_, (Option<String>, Option<i64>)>(&format!("SELECT _etag, _seq FROM \"{}\" WHERE \"{}\" = ?", self.table, self.key))
            .bind(id)
            .fetch_optional(conn)
            .await
            .map_err(SyncError::db("row meta"))
    }

    async fn list_ids(&self, conn: &mut SqliteConnection) -> Result<Vec<String>, SyncError> {
        sqlx::query_scalar(&format!("SELECT \"{}\" FROM \"{}\"", self.key, self.table)).fetch_all(conn).await.map_err(SyncError::db("list ids"))
    }

    async fn set_pending(&self, conn: &mut SqliteConnection, id: &str, pending: i64) -> Result<(), SyncError> {
        sqlx::query(&format!("UPDATE \"{}\" SET _pending = ? WHERE \"{}\" = ?", self.table, self.key))
            .bind(pending)
            .bind(id)
            .execute(conn)
            .await
            .map_err(SyncError::db("set pending"))?;
        Ok(())
    }

    fn row_from_change(&self, change: &Change) -> Value {
        change.row(&self.row_key)
    }

    fn etag_from_change(&self, change: &Change) -> Option<String> {
        change.etag(&self.row_key)
    }

    fn build_request(&self, op: &OutboxOp) -> Result<ApiRequest, SyncError> {
        let item = format!("{}/{}", self.collection, encode_segment(&op.entity_id));
        let with_etag = |r: ApiRequest| match (&op.base_etag, self.send_if_match) {
            (Some(e), true) => r.if_match(e.clone()),
            _ => r,
        };
        Ok(match op.op.as_str() {
            "create" => {
                let mut body = op.payload.clone();
                if let Value::Object(m) = &mut body {
                    m.insert("id".into(), Value::String(op.entity_id.clone()));
                }
                ApiRequest::post(self.collection.clone()).json(body)
            }
            "patch" => with_etag(ApiRequest::patch(item).json(op.payload.clone())),
            "delete" => with_etag(ApiRequest::delete(item)),
            other => return Err(SyncError::Config(format!("JsonTableAdapter does not know the op '{other}'"))),
        })
    }

    fn row_from_response(&self, _op: &OutboxOp, resp: &ApiResponse) -> Option<Value> {
        let body = resp.json_value();
        match body.get(&self.row_key) {
            Some(v @ Value::Object(_)) => Some(v.clone()),
            _ if body.get("id").is_some() => Some(body),
            _ => None,
        }
    }

    fn row_from_conflict(&self, body: &Value) -> Option<Value> {
        for key in ["current", self.row_key.as_str()] {
            if let Some(v @ Value::Object(_)) = body.get(key) {
                return Some(v.clone());
            }
        }
        None
    }

    async fn fetch_row(&self, api: &ApiClient, id: &str) -> Result<Option<Value>, ApiError> {
        match api.send(ApiRequest::get(format!("{}/{}", self.collection, encode_segment(id)))).await {
            Ok(resp) => {
                let body = resp.json_value();
                Ok(match body.get(&self.row_key) {
                    Some(v @ Value::Object(_)) => Some(v.clone()),
                    _ => Some(body).filter(|b| b.is_object()),
                })
            }
            Err(e) if e.status() == Some(404) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn keep_both_copy(&self, _op: &OutboxOp, local: &Map<String, Value>) -> Option<(String, Map<String, Value>)> {
        let mut copy = local.clone();
        if let Some(f) = &self.title_field {
            let title = copy.get(f).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let date = crate::outbox::local_date_label();
            copy.insert(f.clone(), Value::String(format!("{title} (conflit {} {date})", self.machine)));
        }
        Some((uuid::Uuid::new_v4().to_string(), copy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers() {
        assert!(is_identifier("notes"));
        assert!(is_identifier("_x1"));
        assert!(!is_identifier("1x"));
        assert!(!is_identifier("a;drop"));
        assert!(!is_identifier("a\"b"));
        assert!(JsonTableAdapter::new("note", "notes", &["title", "bad name"], "/x").is_err());
    }

    #[test]
    fn ids_never_escape_their_path_segment() {
        assert_eq!(encode_segment("0f8c-ab_1.x~"), "0f8c-ab_1.x~");
        assert_eq!(encode_segment("../admin"), "..%2Fadmin");
        assert_eq!(encode_segment(".."), "%2E%2E");
        assert_eq!(encode_segment("a b?c"), "a%20b%3Fc");
    }
}
