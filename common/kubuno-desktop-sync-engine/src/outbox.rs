//! The outbox of intents (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §7.3) and the row bookkeeping around it: shadow
//! rows (the latest server version of rows that have pending ops), rebase, `_pending` counts, conflict records and
//! the activity log.
//!
//! An intent gets an `op_id` and an `idem_key` (UUID v4, never derived from a clock). The key is kept across
//! retries and token refreshes, and renewed only when the body changes (coalescing, conflict re-queue), so a
//! request that may have been applied is replayed with the same key and the server runs it once.

use std::time::Duration;

use serde_json::{Map, Value};
use sqlx::{FromRow, SqliteConnection};

use crate::adapter::EntityAdapter;
use crate::error::SyncError;

/// States of an op that still matter (not `dead`).
pub const LIVE_STATES: &str = "('pending','inflight','blocked','conflict')";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpState {
    Pending,
    Inflight,
    Blocked,
    Conflict,
    Dead,
}

impl OpState {
    pub fn as_str(self) -> &'static str {
        match self {
            OpState::Pending => "pending",
            OpState::Inflight => "inflight",
            OpState::Blocked => "blocked",
            OpState::Conflict => "conflict",
            OpState::Dead => "dead",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "inflight" => OpState::Inflight,
            "blocked" => OpState::Blocked,
            "conflict" => OpState::Conflict,
            "dead" => OpState::Dead,
            _ => OpState::Pending,
        }
    }
}

#[derive(Debug, Clone, FromRow)]
struct OpRow {
    seq: i64,
    op_id: String,
    idem_key: String,
    feed: String,
    entity: String,
    entity_id: String,
    op: String,
    payload: String,
    base_etag: Option<String>,
    base_row: Option<String>,
    depends_on: Option<i64>,
    state: String,
    attempts: i64,
    next_attempt_at: Option<i64>,
    last_error: Option<String>,
    created_at: i64,
}

/// One op of the outbox.
#[derive(Debug, Clone)]
pub struct OutboxOp {
    pub seq: i64,
    pub op_id: String,
    pub idem_key: String,
    pub feed: String,
    pub entity: String,
    pub entity_id: String,
    /// `create` | `patch` | `delete` | app-specific.
    pub op: String,
    /// The fields of the intent (not the whole row, for a patch).
    pub payload: Value,
    pub base_etag: Option<String>,
    pub base_row: Option<Value>,
    pub depends_on: Option<i64>,
    pub state: OpState,
    pub attempts: i64,
    pub next_attempt_at: Option<i64>,
    pub last_error: Option<String>,
    pub created_at: i64,
}

impl From<OpRow> for OutboxOp {
    fn from(r: OpRow) -> Self {
        Self {
            seq: r.seq,
            op_id: r.op_id,
            idem_key: r.idem_key,
            feed: r.feed,
            entity: r.entity,
            entity_id: r.entity_id,
            op: r.op,
            payload: serde_json::from_str(&r.payload).unwrap_or(Value::Null),
            base_etag: r.base_etag,
            base_row: r.base_row.and_then(|b| serde_json::from_str(&b).ok()),
            depends_on: r.depends_on,
            state: OpState::parse(&r.state),
            attempts: r.attempts,
            next_attempt_at: r.next_attempt_at,
            last_error: r.last_error,
            created_at: r.created_at,
        }
    }
}

/// A local change to send.
#[derive(Debug, Clone)]
pub struct Intent {
    pub feed: String,
    pub entity: String,
    pub entity_id: String,
    pub op: String,
    pub payload: Value,
    pub base_etag: Option<String>,
    pub base_row: Option<Value>,
    pub depends_on: Option<i64>,
}

impl Intent {
    pub fn new(feed: &str, entity: &str, entity_id: &str, op: &str, payload: Value) -> Self {
        Self {
            feed: feed.into(),
            entity: entity.into(),
            entity_id: entity_id.into(),
            op: op.into(),
            payload,
            base_etag: None,
            base_row: None,
            depends_on: None,
        }
    }

    pub fn base(mut self, etag: Option<String>, row: Option<Value>) -> Self {
        self.base_etag = etag;
        self.base_row = row;
        self
    }
}

/// What `enqueue` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnqueueOutcome {
    /// A new op.
    Queued { seq: i64, op_id: String },
    /// Merged into an op not sent yet (new idempotency key, original base kept).
    Coalesced { seq: i64, op_id: String },
    /// A create followed by a delete before anything was sent: both are gone.
    Cancelled,
}

const SELECT_OP: &str = "SELECT seq, op_id, idem_key, feed, entity, entity_id, op, payload, base_etag, base_row, depends_on, state, attempts, next_attempt_at, last_error, created_at FROM _sync_outbox";

pub fn new_key() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn merge_fields(into: &mut Value, from: &Value) {
    if let (Value::Object(a), Value::Object(b)) = (into, from) {
        for (k, v) in b {
            a.insert(k.clone(), v.clone());
        }
    }
}

/// Live ops of one row, in drain order.
pub async fn live_ops(conn: &mut SqliteConnection, entity: &str, id: &str) -> Result<Vec<OutboxOp>, SyncError> {
    let rows: Vec<OpRow> = sqlx::query_as(&format!("{SELECT_OP} WHERE entity = ? AND entity_id = ? AND state IN {LIVE_STATES} ORDER BY seq"))
        .bind(entity)
        .bind(id)
        .fetch_all(conn)
        .await
        .map_err(SyncError::db("live ops"))?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get_op(conn: &mut SqliteConnection, seq: i64) -> Result<Option<OutboxOp>, SyncError> {
    let row: Option<OpRow> = sqlx::query_as(&format!("{SELECT_OP} WHERE seq = ?")).bind(seq).fetch_optional(conn).await.map_err(SyncError::db("get op"))?;
    Ok(row.map(Into::into))
}

pub async fn all_ops(conn: &mut SqliteConnection) -> Result<Vec<OutboxOp>, SyncError> {
    let rows: Vec<OpRow> = sqlx::query_as(&format!("{SELECT_OP} ORDER BY seq")).fetch_all(conn).await.map_err(SyncError::db("all ops"))?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// The next op to send: pending, due, no earlier live op on the same row, dependency done.
pub async fn next_due(conn: &mut SqliteConnection, now_ms: i64, skip_entities: &[(String, String)]) -> Result<Option<OutboxOp>, SyncError> {
    let rows: Vec<OpRow> = sqlx::query_as(&format!(
        "{SELECT_OP} o WHERE o.state = 'pending' AND (o.next_attempt_at IS NULL OR o.next_attempt_at <= ?)
           AND NOT EXISTS (SELECT 1 FROM _sync_outbox p WHERE p.entity = o.entity AND p.entity_id = o.entity_id
                           AND p.seq < o.seq AND p.state IN {LIVE_STATES})
           AND (o.depends_on IS NULL OR NOT EXISTS (SELECT 1 FROM _sync_outbox d WHERE d.seq = o.depends_on AND d.state IN {LIVE_STATES}))
         ORDER BY o.seq LIMIT 50"
    ))
    .bind(now_ms)
    .fetch_all(conn)
    .await
    .map_err(SyncError::db("next due op"))?;
    Ok(rows
        .into_iter()
        .map(OutboxOp::from)
        .find(|op| !skip_entities.iter().any(|(e, i)| *e == op.entity && *i == op.entity_id)))
}

/// Adds an intent inside the caller's transaction, coalescing with an op of the same row that was **never sent**
/// (`attempts = 0`, `pending`): an op that was sent may have been applied, so it is never rewritten.
pub async fn enqueue(conn: &mut SqliteConnection, adapter: Option<&dyn EntityAdapter>, now_ms: i64, intent: Intent) -> Result<EnqueueOutcome, SyncError> {
    let last: Option<OpRow> = sqlx::query_as(&format!(
        "{SELECT_OP} WHERE entity = ? AND entity_id = ? AND state IN {LIVE_STATES} ORDER BY seq DESC LIMIT 1"
    ))
    .bind(&intent.entity)
    .bind(&intent.entity_id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(SyncError::db("enqueue: last op"))?;
    let last: Option<OutboxOp> = last.map(Into::into);
    let unsent = last.filter(|l| l.state == OpState::Pending && l.attempts == 0);
    let outcome = match (unsent, intent.op.as_str()) {
        (Some(l), "patch") if l.op == "patch" || l.op == "create" => {
            let mut payload = l.payload.clone();
            merge_fields(&mut payload, &intent.payload);
            sqlx::query("UPDATE _sync_outbox SET payload = ?, idem_key = ? WHERE seq = ?")
                .bind(payload.to_string())
                .bind(new_key())
                .bind(l.seq)
                .execute(&mut *conn)
                .await
                .map_err(SyncError::db("enqueue: coalesce"))?;
            EnqueueOutcome::Coalesced { seq: l.seq, op_id: l.op_id }
        }
        (Some(l), "delete") if l.op == "create" => {
            // Nothing reached the server: drop the create and every other live op of the row.
            sqlx::query(&format!("DELETE FROM _sync_outbox WHERE entity = ? AND entity_id = ? AND state IN {LIVE_STATES}"))
                .bind(&intent.entity)
                .bind(&intent.entity_id)
                .execute(&mut *conn)
                .await
                .map_err(SyncError::db("enqueue: cancel"))?;
            drop_shadow(conn, &intent.entity, &intent.entity_id).await?;
            let _ = l;
            EnqueueOutcome::Cancelled
        }
        (Some(l), "delete") if l.op == "patch" => {
            // The delete replaces the unsent patch and keeps its base (what the server had).
            sqlx::query("UPDATE _sync_outbox SET op = 'delete', payload = '{}', idem_key = ? WHERE seq = ?")
                .bind(new_key())
                .bind(l.seq)
                .execute(&mut *conn)
                .await
                .map_err(SyncError::db("enqueue: patch to delete"))?;
            EnqueueOutcome::Coalesced { seq: l.seq, op_id: l.op_id }
        }
        _ => {
            let op_id = new_key();
            let seq: i64 = sqlx::query_scalar(
                "INSERT INTO _sync_outbox (op_id, idem_key, feed, entity, entity_id, op, payload, base_etag, base_row, depends_on, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING seq",
            )
            .bind(&op_id)
            .bind(new_key())
            .bind(&intent.feed)
            .bind(&intent.entity)
            .bind(&intent.entity_id)
            .bind(&intent.op)
            .bind(intent.payload.to_string())
            .bind(&intent.base_etag)
            .bind(intent.base_row.as_ref().map(|r| r.to_string()))
            .bind(intent.depends_on)
            .bind(now_ms)
            .fetch_one(&mut *conn)
            .await
            .map_err(SyncError::db("enqueue: insert"))?;
            EnqueueOutcome::Queued { seq, op_id }
        }
    };
    if let Some(a) = adapter {
        refresh_pending(conn, a, &intent.entity_id).await?;
    }
    Ok(outcome)
}

/// Recomputes `_pending` of a row from the outbox.
pub async fn refresh_pending(conn: &mut SqliteConnection, adapter: &dyn EntityAdapter, id: &str) -> Result<i64, SyncError> {
    let n: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM _sync_outbox WHERE entity = ? AND entity_id = ? AND state IN {LIVE_STATES}"
    ))
    .bind(adapter.entity())
    .bind(id)
    .fetch_one(&mut *conn)
    .await
    .map_err(SyncError::db("count pending"))?;
    adapter.set_pending(conn, id, n).await?;
    Ok(n)
}

pub async fn set_state(conn: &mut SqliteConnection, seq: i64, state: OpState, error: Option<&str>) -> Result<(), SyncError> {
    sqlx::query("UPDATE _sync_outbox SET state = ?, last_error = COALESCE(?, last_error) WHERE seq = ?")
        .bind(state.as_str())
        .bind(error)
        .bind(seq)
        .execute(conn)
        .await
        .map_err(SyncError::db("set op state"))?;
    Ok(())
}

pub async fn delete_op(conn: &mut SqliteConnection, seq: i64) -> Result<(), SyncError> {
    sqlx::query("DELETE FROM _sync_outbox WHERE seq = ?").bind(seq).execute(conn).await.map_err(SyncError::db("delete op"))?;
    Ok(())
}

/// The shadow (latest server version) of a row with pending ops.
#[derive(Debug, Clone)]
pub struct Shadow {
    pub row: Value,
    pub etag: Option<String>,
    pub change_seq: Option<i64>,
    pub deleted: bool,
}

pub async fn get_shadow(conn: &mut SqliteConnection, entity: &str, id: &str) -> Result<Option<Shadow>, SyncError> {
    let r: Option<(String, Option<String>, Option<i64>, i64)> =
        sqlx::query_as("SELECT server_row, etag, change_seq, deleted FROM _sync_shadow WHERE entity = ? AND entity_id = ?")
            .bind(entity)
            .bind(id)
            .fetch_optional(conn)
            .await
            .map_err(SyncError::db("get shadow"))?;
    Ok(r.map(|(row, etag, change_seq, deleted)| Shadow { row: serde_json::from_str(&row).unwrap_or(Value::Null), etag, change_seq, deleted: deleted != 0 }))
}

pub async fn put_shadow(conn: &mut SqliteConnection, entity: &str, id: &str, s: &Shadow) -> Result<(), SyncError> {
    sqlx::query(
        "INSERT INTO _sync_shadow (entity, entity_id, server_row, etag, change_seq, deleted) VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(entity, entity_id) DO UPDATE SET server_row = excluded.server_row, etag = excluded.etag,
           change_seq = excluded.change_seq, deleted = excluded.deleted",
    )
    .bind(entity)
    .bind(id)
    .bind(s.row.to_string())
    .bind(&s.etag)
    .bind(s.change_seq)
    .bind(i64::from(s.deleted))
    .execute(conn)
    .await
    .map_err(SyncError::db("put shadow"))?;
    Ok(())
}

pub async fn drop_shadow(conn: &mut SqliteConnection, entity: &str, id: &str) -> Result<(), SyncError> {
    sqlx::query("DELETE FROM _sync_shadow WHERE entity = ? AND entity_id = ?").bind(entity).bind(id).execute(conn).await.map_err(SyncError::db("drop shadow"))?;
    Ok(())
}

/// Shows "server + my pending changes": the visible row becomes the shadow with every live op of the row
/// re-applied on top, in order (§7.3, PowerSync's rebase). Without a shadow the visible row is left as is (it
/// already is the local state). Ops in `conflict` are re-applied too, so a conflicting edit stays visible until
/// the user resolves it.
pub async fn rebase(conn: &mut SqliteConnection, adapter: &dyn EntityAdapter, id: &str) -> Result<(), SyncError> {
    let Some(shadow) = get_shadow(conn, adapter.entity(), id).await? else {
        refresh_pending(conn, adapter, id).await?;
        return Ok(());
    };
    let ops = live_ops(conn, adapter.entity(), id).await?;
    let mut row: Option<Value> = if shadow.deleted { None } else { Some(shadow.row.clone()) };
    for op in &ops {
        match op.op.as_str() {
            "create" => {
                let mut base = row.take().unwrap_or_else(|| Value::Object(Map::new()));
                merge_fields(&mut base, &op.payload);
                row = Some(base);
            }
            // A delete that hit a newer server version waits for the user: the server row stays visible.
            "delete" if op.state == OpState::Conflict => {}
            "delete" => row = None,
            // patch and app-specific ops carrying fields
            _ => {
                if let Some(r) = row.as_mut() {
                    merge_fields(r, &op.payload);
                }
            }
        }
    }
    match row {
        Some(r) => adapter.upsert_server(conn, id, &r, shadow.etag.as_deref(), shadow.change_seq).await?,
        None => adapter.delete_row(conn, id).await?,
    }
    if ops.is_empty() {
        drop_shadow(conn, adapter.entity(), id).await?;
    }
    refresh_pending(conn, adapter, id).await?;
    Ok(())
}

/// Conflict kinds.
pub mod conflict_kind {
    pub const FIELD: &str = "field";
    pub const DELETED_REMOTELY: &str = "deleted_remotely";
    pub const DELETE_VS_EDIT: &str = "delete_vs_edit";
    pub const REJECTED: &str = "rejected";
    pub const CONTENT: &str = "content";
    pub const REVOKED: &str = "revoked";
}

/// A conflict record (`_sync_conflicts`), surfaced to the user instead of being dropped.
#[derive(Debug, Clone)]
pub struct NewConflict<'a> {
    pub entity: &'a str,
    pub entity_id: &'a str,
    pub op_id: Option<&'a str>,
    pub kind: &'a str,
    pub local: Option<&'a Value>,
    pub server: Option<&'a Value>,
    pub base: Option<&'a Value>,
    pub fields: &'a [String],
    pub message: Option<&'a str>,
    /// Already resolved automatically (keep-both): recorded for the activity panel only.
    pub resolution: Option<&'a str>,
}

pub async fn record_conflict(conn: &mut SqliteConnection, now_ms: i64, c: NewConflict<'_>) -> Result<i64, SyncError> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO _sync_conflicts (entity, entity_id, op_id, kind, local, server, base, fields, message, created_at, resolved_at, resolution)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(c.entity)
    .bind(c.entity_id)
    .bind(c.op_id)
    .bind(c.kind)
    .bind(c.local.map(|v| v.to_string()))
    .bind(c.server.map(|v| v.to_string()))
    .bind(c.base.map(|v| v.to_string()))
    .bind(serde_json::to_string(c.fields).unwrap_or_else(|_| "[]".into()))
    .bind(c.message)
    .bind(now_ms)
    .bind(c.resolution.map(|_| now_ms))
    .bind(c.resolution)
    .fetch_one(conn)
    .await
    .map_err(SyncError::db("record conflict"))?;
    Ok(id)
}

/// Appends to the activity log (ring buffer of 1000). `detail` must be codes and ids only.
pub async fn log(conn: &mut SqliteConnection, now_ms: i64, level: &str, feed: Option<&str>, event: &str, detail: Option<&str>) -> Result<(), SyncError> {
    sqlx::query("INSERT INTO _sync_log (at, level, feed, event, detail) VALUES (?, ?, ?, ?, ?)")
        .bind(now_ms)
        .bind(level)
        .bind(feed)
        .bind(event)
        .bind(detail)
        .execute(&mut *conn)
        .await
        .map_err(SyncError::db("log"))?;
    sqlx::query("DELETE FROM _sync_log WHERE id <= (SELECT max(id) - 1000 FROM _sync_log)")
        .execute(conn)
        .await
        .map_err(SyncError::db("trim log"))?;
    Ok(())
}

/// Backoff of a transient failure: `min(2^attempts * 2 s, 15 min)` ± 20 %, or the server's `Retry-After`.
pub fn backoff(attempts: i64, retry_after: Option<Duration>) -> Duration {
    if let Some(ra) = retry_after {
        return ra.min(Duration::from_secs(15 * 60));
    }
    let exp = 2u64.saturating_pow(u32::try_from(attempts.clamp(0, 20)).unwrap_or(20)).saturating_mul(2);
    let base = Duration::from_secs(exp.min(15 * 60));
    kubuno_desktop_api_client::jittered(base, 0.2)
}

/// `YYYY-MM-DD HH-MM` (UTC) for keep-both copy names; display only.
pub fn local_date_label() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}-{:02}", rem / 3600, (rem % 3600) / 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        let a = backoff(0, None);
        assert!(a >= Duration::from_millis(1599) && a <= Duration::from_millis(2401));
        assert!(backoff(30, None) <= Duration::from_secs(15 * 60 * 6 / 5 + 1));
        assert_eq!(backoff(3, Some(Duration::from_secs(7))), Duration::from_secs(7));
    }

    #[test]
    fn date_label_shape() {
        let s = local_date_label();
        assert_eq!(s.len(), 16, "{s}");
    }
}
