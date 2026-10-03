//! The engine: local writes with their outbox entries, push (drain), pull (feeds), conflicts, status.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use kubuno_desktop_api_client::{ApiClient, ApiError, ApiRequest, ChangeKind, Cursor, ErrorClass};
use serde_json::{Map, Value};
use sqlx::{Sqlite, SqliteConnection, Transaction};
use tokio::sync::{broadcast, mpsc, watch};

use crate::adapter::EntityAdapter;
use crate::clock::{Clock, SystemClock};
use crate::conflict::{three_way_merge, ConflictPolicy};
use crate::db::LocalDb;
use crate::error::SyncError;
use crate::outbox::{self, conflict_kind, EnqueueOutcome, Intent, NewConflict, OpState, OutboxOp, Shadow, LIVE_STATES};
use crate::scheduler::Trigger;
use crate::status::{FeedStatus, SyncEvent, SyncState, SyncStatus};

/// A feed of the Kubuno Delta Protocol v1 and the entity its changes go to.
#[derive(Debug, Clone)]
pub struct FeedSpec {
    /// `notes.notes`, `drive.delta`...
    pub name: String,
    /// `/api/v1/notes/notes/delta`.
    pub path: String,
    /// The adapter's entity.
    pub entity: String,
    /// Changes per page (500-2000).
    pub page_limit: u32,
    /// Extra query parameters (drive's `full=true`).
    pub extra_query: Vec<(String, String)>,
    /// Change kinds other than modified/deleted/revoked that carry a row of this entity (drive's `file`).
    pub row_kinds: Vec<String>,
}

impl FeedSpec {
    pub fn new(name: &str, path: &str, entity: &str) -> Self {
        Self { name: name.into(), path: path.into(), entity: entity.into(), page_limit: 500, extra_query: Vec::new(), row_kinds: Vec::new() }
    }

    pub fn page_limit(mut self, limit: u32) -> Self {
        self.page_limit = limit.clamp(1, 2000);
        self
    }
}

/// A hook called at named points of a cycle (crash tests kill the process there).
pub type Failpoint = Arc<dyn Fn(&str) + Send + Sync>;

/// What one push did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PushReport {
    pub sent: u32,
    pub retry_later: u32,
    pub conflicts: u32,
    pub rejected: u32,
}

/// What one pull of a feed did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PullReport {
    pub pages: u32,
    pub applied: u32,
    pub shadowed: u32,
    pub deleted: u32,
    pub skipped: u32,
    pub stale: u32,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CycleReport {
    pub push: PushReport,
    pub pulls: Vec<(String, PullReport)>,
}

/// A conflict record as shown to the user.
#[derive(Debug, Clone)]
pub struct ConflictRecord {
    pub id: i64,
    pub entity: String,
    pub entity_id: String,
    pub op_id: Option<String>,
    pub kind: String,
    pub local: Option<Value>,
    pub server: Option<Value>,
    pub base: Option<Value>,
    pub fields: Vec<String>,
    pub message: Option<String>,
    pub created_at: i64,
    pub resolution: Option<String>,
}

/// The user's choice for a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// "Garder la mienne": re-send the local version on top of the server's.
    KeepMine,
    /// "Garder celle du serveur": drop the local change.
    KeepServer,
}

struct Inner {
    db: LocalDb,
    api: ApiClient,
    adapters: HashMap<String, Arc<dyn EntityAdapter>>,
    feeds: Vec<FeedSpec>,
    clock: Arc<dyn Clock>,
    status_tx: watch::Sender<SyncStatus>,
    events: broadcast::Sender<SyncEvent>,
    failpoint: RwLock<Option<Failpoint>>,
    cycle: tokio::sync::Mutex<()>,
    forced_offline: AtomicBool,
    network_down: AtomicBool,
    session_expired: AtomicBool,
    syncing: AtomicBool,
    last_sync_at: Mutex<Option<i64>>,
    last_error: Mutex<Option<String>>,
    kick: Mutex<Option<mpsc::UnboundedSender<Trigger>>>,
    pushes: AtomicU64,
    pulls: AtomicU64,
}

/// The sync engine of one app database. Cheap to clone.
#[derive(Clone)]
pub struct SyncEngine {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for SyncEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncEngine").field("db", &self.inner.db.path()).field("feeds", &self.inner.feeds.len()).finish()
    }
}

/// Builder of a [`SyncEngine`].
pub struct EngineBuilder {
    db: LocalDb,
    api: ApiClient,
    adapters: HashMap<String, Arc<dyn EntityAdapter>>,
    feeds: Vec<FeedSpec>,
    clock: Arc<dyn Clock>,
}

impl EngineBuilder {
    pub fn adapter(mut self, a: Arc<dyn EntityAdapter>) -> Self {
        self.adapters.insert(a.entity().to_string(), a);
        self
    }

    pub fn feed(mut self, f: FeedSpec) -> Self {
        self.feeds.push(f);
        self
    }

    pub fn clock(mut self, c: Arc<dyn Clock>) -> Self {
        self.clock = c;
        self
    }

    pub async fn build(self) -> Result<SyncEngine, SyncError> {
        for f in &self.feeds {
            if !self.adapters.contains_key(&f.entity) {
                return Err(SyncError::UnknownEntity(f.entity.clone()));
            }
            sqlx::query("INSERT INTO _sync_feeds (feed) VALUES (?) ON CONFLICT(feed) DO NOTHING")
                .bind(&f.name)
                .execute(self.db.pool())
                .await
                .map_err(SyncError::db("register feed"))?;
        }
        let (status_tx, _) = watch::channel(SyncStatus::default());
        let (events, _) = broadcast::channel(256);
        let engine = SyncEngine {
            inner: Arc::new(Inner {
                db: self.db,
                api: self.api,
                adapters: self.adapters,
                feeds: self.feeds,
                clock: self.clock,
                status_tx,
                events,
                failpoint: RwLock::new(None),
                cycle: tokio::sync::Mutex::new(()),
                forced_offline: AtomicBool::new(false),
                network_down: AtomicBool::new(false),
                session_expired: AtomicBool::new(false),
                syncing: AtomicBool::new(false),
                last_sync_at: Mutex::new(None),
                last_error: Mutex::new(None),
                kick: Mutex::new(None),
                pushes: AtomicU64::new(0),
                pulls: AtomicU64::new(0),
            }),
        };
        engine.refresh_status().await?;
        Ok(engine)
    }
}

fn as_map(v: &Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

fn error_detail(e: &ApiError) -> String {
    match e {
        ApiError::Http { status, code, message, .. } => {
            format!("{status} {}{}", code.as_deref().unwrap_or(""), message.as_deref().map(|m| format!(": {m}")).unwrap_or_default())
        }
        other => other.to_string(),
    }
}

impl SyncEngine {
    pub fn builder(db: LocalDb, api: ApiClient) -> EngineBuilder {
        EngineBuilder { db, api, adapters: HashMap::new(), feeds: Vec::new(), clock: Arc::new(SystemClock) }
    }

    pub fn db(&self) -> &LocalDb {
        &self.inner.db
    }

    pub fn api(&self) -> &ApiClient {
        &self.inner.api
    }

    pub fn feeds(&self) -> &[FeedSpec] {
        &self.inner.feeds
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SyncEvent> {
        self.inner.events.subscribe()
    }

    pub fn status(&self) -> watch::Receiver<SyncStatus> {
        self.inner.status_tx.subscribe()
    }

    pub fn now_ms(&self) -> i64 {
        self.inner.clock.now_ms()
    }

    /// Number of push / pull runs (tests of the scheduler's coalescing).
    pub fn run_counts(&self) -> (u64, u64) {
        (self.inner.pushes.load(Ordering::SeqCst), self.inner.pulls.load(Ordering::SeqCst))
    }

    pub fn set_failpoint(&self, f: Option<Failpoint>) {
        *self.inner.failpoint.write().unwrap_or_else(PoisonError::into_inner) = f;
    }

    fn fail(&self, point: &str) {
        let f = self.inner.failpoint.read().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(f) = f {
            f(point);
        }
    }

    /// The forced offline mode of `settings.json`: no network work, local writes and the outbox keep working.
    pub fn set_forced_offline(&self, offline: bool) {
        self.inner.forced_offline.store(offline, Ordering::SeqCst);
    }

    /// The OS's connectivity (network listeners call this through the scheduler).
    pub fn set_network_available(&self, up: bool) {
        self.inner.network_down.store(!up, Ordering::SeqCst);
    }

    pub fn is_offline(&self) -> bool {
        self.inner.forced_offline.load(Ordering::SeqCst) || self.inner.network_down.load(Ordering::SeqCst)
    }

    pub fn is_session_expired(&self) -> bool {
        self.inner.session_expired.load(Ordering::SeqCst)
    }

    /// The user signed in again with the same account: feeds resume and the kept outbox drains.
    pub fn session_restored(&self) {
        self.inner.session_expired.store(false, Ordering::SeqCst);
        self.kick(Trigger::Manual);
    }

    pub(crate) fn attach_scheduler(&self, tx: mpsc::UnboundedSender<Trigger>) {
        *self.inner.kick.lock().unwrap_or_else(PoisonError::into_inner) = Some(tx);
    }

    fn kick(&self, t: Trigger) {
        if let Some(tx) = self.inner.kick.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
            let _ = tx.send(t);
        }
    }

    fn adapter(&self, entity: &str) -> Result<Arc<dyn EntityAdapter>, SyncError> {
        self.inner.adapters.get(entity).cloned().ok_or_else(|| SyncError::UnknownEntity(entity.to_string()))
    }

    fn feed_of(&self, entity: &str) -> String {
        self.inner.feeds.iter().find(|f| f.entity == entity).map(|f| f.name.clone()).unwrap_or_else(|| entity.to_string())
    }

    fn publish(&self, e: SyncEvent) {
        let _ = self.inner.events.send(e);
    }

    async fn begin(&self) -> Result<Transaction<'static, Sqlite>, SyncError> {
        self.inner.db.pool().begin_with("BEGIN IMMEDIATE").await.map_err(SyncError::db("begin"))
    }

    fn check_writable(&self) -> Result<(), SyncError> {
        match self.inner.db.degraded() {
            Some(e) => Err(SyncError::ReadOnly(e.to_string())),
            None => Ok(()),
        }
    }

    // ---------------------------------------------------------------- local writes

    /// Starts a local write: the app's DML and the outbox entries in **one** transaction (`BEGIN IMMEDIATE`). No
    /// network inside: saving never fails because the server is unreachable.
    pub async fn begin_local(&self) -> Result<LocalTx, SyncError> {
        self.check_writable()?;
        Ok(LocalTx { tx: Some(self.begin().await?), engine: self.clone(), tables: BTreeSet::new(), enqueued: false })
    }

    // ---------------------------------------------------------------- cycle

    fn network_allowed(&self) -> Result<(), SyncError> {
        self.check_writable()?;
        if self.is_offline() {
            return Err(SyncError::Offline);
        }
        if self.is_session_expired() {
            return Err(SyncError::SessionExpired);
        }
        Ok(())
    }

    /// A full cycle: push, then pull every feed (push first, as in `kubuno-sync` and the Android apps).
    pub async fn sync_once(&self) -> Result<CycleReport, SyncError> {
        let _g = self.inner.cycle.lock().await;
        self.network_allowed()?;
        self.inner.syncing.store(true, Ordering::SeqCst);
        let _ = self.refresh_status().await;
        let result = async {
            let push = self.push_locked().await?;
            let mut pulls = Vec::new();
            for f in self.inner.feeds.clone() {
                pulls.push((f.name.clone(), self.pull_locked(&f).await?));
            }
            Ok::<_, SyncError>(CycleReport { push, pulls })
        }
        .await;
        self.inner.syncing.store(false, Ordering::SeqCst);
        match &result {
            Ok(_) => {
                *self.inner.last_sync_at.lock().unwrap_or_else(PoisonError::into_inner) = Some(self.now_ms());
                *self.inner.last_error.lock().unwrap_or_else(PoisonError::into_inner) = None;
            }
            Err(SyncError::SessionExpired) | Err(SyncError::Offline) => {}
            Err(e) => *self.inner.last_error.lock().unwrap_or_else(PoisonError::into_inner) = Some(e.to_string()),
        }
        let _ = self.refresh_status().await;
        result
    }

    /// Manual "Synchroniser": ignores the backoff of pending ops.
    pub async fn sync_now(&self) -> Result<CycleReport, SyncError> {
        sqlx::query("UPDATE _sync_outbox SET next_attempt_at = NULL WHERE state = 'pending'")
            .execute(self.inner.db.pool())
            .await
            .map_err(SyncError::db("reset backoff"))?;
        self.sync_once().await
    }

    /// Drains the outbox.
    pub async fn push(&self) -> Result<PushReport, SyncError> {
        let _g = self.inner.cycle.lock().await;
        self.network_allowed()?;
        let r = self.push_locked().await;
        let _ = self.refresh_status().await;
        r
    }

    /// Pulls one feed.
    pub async fn pull(&self, feed: &str) -> Result<PullReport, SyncError> {
        let _g = self.inner.cycle.lock().await;
        self.network_allowed()?;
        let f = self.inner.feeds.iter().find(|f| f.name == feed).cloned().ok_or_else(|| SyncError::UnknownFeed(feed.to_string()))?;
        let r = self.pull_locked(&f).await;
        let _ = self.refresh_status().await;
        r
    }

    /// The earliest `next_attempt_at` of a pending op (local clock, ms), for the scheduler.
    pub async fn next_retry_at(&self) -> Result<Option<i64>, SyncError> {
        sqlx::query_scalar("SELECT min(next_attempt_at) FROM _sync_outbox WHERE state = 'pending'")
            .fetch_one(self.inner.db.pool())
            .await
            .map_err(SyncError::db("next retry"))
    }

    // ---------------------------------------------------------------- push

    async fn push_locked(&self) -> Result<PushReport, SyncError> {
        self.inner.pushes.fetch_add(1, Ordering::SeqCst);
        let mut report = PushReport::default();
        let mut skip: Vec<(String, String)> = Vec::new();
        let mut conflict_rounds: HashMap<i64, u32> = HashMap::new();
        loop {
            let op = {
                let mut conn = self.inner.db.pool().acquire().await.map_err(SyncError::db("acquire"))?;
                outbox::next_due(&mut conn, self.now_ms(), &skip).await?
            };
            let Some(op) = op else { break };
            let adapter = match self.adapter(&op.entity) {
                Ok(a) => a,
                Err(e) => {
                    // An entity this build does not know (an older app opened a newer database): never sent.
                    let mut conn = self.inner.db.pool().acquire().await.map_err(SyncError::db("acquire"))?;
                    outbox::set_state(&mut conn, op.seq, OpState::Blocked, Some(&e.to_string())).await?;
                    continue;
                }
            };
            let req = match adapter.build_request(&op) {
                Ok(r) => r.idempotency_key(op.idem_key.clone()),
                Err(e) => {
                    let err = ApiError::InvalidRequest(e.to_string());
                    self.on_rejected(&op, adapter.as_ref(), &err).await?;
                    report.rejected += 1;
                    continue;
                }
            };
            {
                let mut conn = self.inner.db.pool().acquire().await.map_err(SyncError::db("acquire"))?;
                outbox::set_state(&mut conn, op.seq, OpState::Inflight, None).await?;
            }
            self.fail("push.inflight");
            let result = self.inner.api.send(req).await;
            self.fail("push.after_response");
            match result {
                Ok(resp) => {
                    self.on_success(&op, adapter.as_ref(), Some(&resp)).await?;
                    report.sent += 1;
                }
                Err(e) => match e.class() {
                    ErrorClass::Transient | ErrorClass::Protocol | ErrorClass::CursorExpired => {
                        self.on_transient(&op, &e).await?;
                        skip.push((op.entity.clone(), op.entity_id.clone()));
                        report.retry_later += 1;
                    }
                    ErrorClass::Unauthorized | ErrorClass::SessionExpired => {
                        let mut conn = self.inner.db.pool().acquire().await.map_err(SyncError::db("acquire"))?;
                        outbox::set_state(&mut conn, op.seq, OpState::Pending, Some("401")).await?;
                        self.inner.session_expired.store(true, Ordering::SeqCst);
                        tracing::warn!("session expired while draining the outbox: feeds paused, outbox kept");
                        return Err(SyncError::SessionExpired);
                    }
                    ErrorClass::NotFound if op.op == "delete" => {
                        // Already gone on the server: the delete is done (Android rule).
                        self.on_success(&op, adapter.as_ref(), None).await?;
                        report.sent += 1;
                    }
                    ErrorClass::NotFound => {
                        self.on_deleted_remotely(&op, adapter.as_ref()).await?;
                        report.conflicts += 1;
                    }
                    ErrorClass::Conflict => {
                        let rounds = conflict_rounds.entry(op.seq).or_insert(0);
                        *rounds += 1;
                        if *rounds > 5 {
                            // The server keeps moving: try again later rather than loop.
                            self.on_transient(&op, &e).await?;
                            skip.push((op.entity.clone(), op.entity_id.clone()));
                            report.retry_later += 1;
                        } else {
                            self.on_conflict(&op, adapter.as_ref(), &e).await?;
                            report.conflicts += 1;
                        }
                    }
                    ErrorClass::Definitive => {
                        self.on_rejected(&op, adapter.as_ref(), &e).await?;
                        report.rejected += 1;
                    }
                },
            }
        }
        Ok(report)
    }

    async fn on_transient(&self, op: &OutboxOp, e: &ApiError) -> Result<(), SyncError> {
        let delay = outbox::backoff(op.attempts, e.retry_after());
        let next = self.now_ms() + delay.as_millis() as i64;
        sqlx::query("UPDATE _sync_outbox SET state = 'pending', attempts = attempts + 1, next_attempt_at = ?, last_error = ? WHERE seq = ?")
            .bind(next)
            .bind(error_detail(e))
            .bind(op.seq)
            .execute(self.inner.db.pool())
            .await
            .map_err(SyncError::db("transient"))?;
        tracing::debug!(entity = %op.entity, id = %op.entity_id, attempts = op.attempts + 1, "op will be retried");
        Ok(())
    }

    async fn on_success(&self, op: &OutboxOp, adapter: &dyn EntityAdapter, resp: Option<&kubuno_desktop_api_client::ApiResponse>) -> Result<(), SyncError> {
        let mut tx = self.begin().await?;
        let row = resp.and_then(|r| adapter.row_from_response(op, r));
        outbox::delete_op(&mut tx, op.seq).await?;
        let mut id = op.entity_id.clone();
        // A server that mints ids (drive upload, chat message): map and rewrite the dependent ops.
        if let Some(server_id) = row.as_ref().and_then(|r| r.get("id")).and_then(|v| v.as_str()) {
            if server_id != op.entity_id && op.op == "create" {
                sqlx::query("INSERT OR REPLACE INTO _sync_id_map (entity, local_id, server_id) VALUES (?, ?, ?)")
                    .bind(&op.entity)
                    .bind(&op.entity_id)
                    .bind(server_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(SyncError::db("id map"))?;
                sqlx::query("UPDATE _sync_outbox SET entity_id = ? WHERE entity = ? AND entity_id = ?")
                    .bind(server_id)
                    .bind(&op.entity)
                    .bind(&op.entity_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(SyncError::db("rewrite ids"))?;
                adapter.delete_row(&mut tx, &op.entity_id).await?;
                id = server_id.to_string();
            }
        }
        let remaining = outbox::live_ops(&mut tx, &op.entity, &id).await?;
        if op.op == "delete" {
            if remaining.is_empty() {
                adapter.delete_row(&mut tx, &id).await?;
                outbox::drop_shadow(&mut tx, &op.entity, &id).await?;
            }
        } else if let Some(row) = row {
            let etag = adapter.etag_of(&row).or_else(|| resp.and_then(|r| r.etag()));
            if remaining.is_empty() {
                adapter.upsert_server(&mut tx, &id, &row, etag.as_deref(), None).await?;
                outbox::drop_shadow(&mut tx, &op.entity, &id).await?;
            } else {
                let seq = outbox::get_shadow(&mut tx, &op.entity, &id).await?.and_then(|s| s.change_seq);
                outbox::put_shadow(&mut tx, &op.entity, &id, &Shadow { row, etag, change_seq: seq, deleted: false }).await?;
                outbox::rebase(&mut tx, adapter, &id).await?;
            }
        } else if let Some(etag) = resp.and_then(|r| r.etag()) {
            // No body but a new ETag: keep the visible row, update its etag so the next patch matches.
            if let Some(fields) = adapter.read_row(&mut tx, &id).await? {
                let seq = adapter.row_meta(&mut tx, &id).await?.and_then(|m| m.1);
                if remaining.is_empty() {
                    adapter.upsert_server(&mut tx, &id, &Value::Object(fields), Some(&etag), seq).await?;
                }
            }
        }
        outbox::refresh_pending(&mut tx, adapter, &id).await?;
        outbox::log(&mut tx, self.now_ms(), "info", Some(&op.feed), "sent", Some(&format!("{} {} {}", op.op, op.entity, id))).await?;
        self.fail("push.before_commit");
        tx.commit().await.map_err(SyncError::db("commit success"))?;
        self.publish(SyncEvent::TablesChanged(vec![adapter.table().to_string()]));
        Ok(())
    }

    async fn on_rejected(&self, op: &OutboxOp, adapter: &dyn EntityAdapter, e: &ApiError) -> Result<(), SyncError> {
        let detail = error_detail(e);
        let mut tx = self.begin().await?;
        outbox::set_state(&mut tx, op.seq, OpState::Dead, Some(&detail)).await?;
        if op.op == "create" {
            // Everything queued on top of a rejected create is meaningless.
            sqlx::query(&format!("UPDATE _sync_outbox SET state = 'dead', last_error = 'create rejected' WHERE entity = ? AND entity_id = ? AND state IN {LIVE_STATES}"))
                .bind(&op.entity)
                .bind(&op.entity_id)
                .execute(&mut *tx)
                .await
                .map_err(SyncError::db("cascade dead"))?;
        }
        // Explicit rollback (never "wait for the next pull": a rejected write produces no change in the feed).
        let shadow = outbox::get_shadow(&mut tx, &op.entity, &op.entity_id).await?;
        match (shadow, &op.base_row) {
            (Some(_), _) => outbox::rebase(&mut tx, adapter, &op.entity_id).await?,
            (None, Some(base)) => {
                let seq = adapter.row_meta(&mut tx, &op.entity_id).await?.and_then(|m| m.1);
                outbox::put_shadow(&mut tx, &op.entity, &op.entity_id, &Shadow { row: base.clone(), etag: op.base_etag.clone(), change_seq: seq, deleted: false }).await?;
                outbox::rebase(&mut tx, adapter, &op.entity_id).await?;
            }
            (None, None) if op.op == "create" => adapter.delete_row(&mut tx, &op.entity_id).await?,
            (None, None) => {}
        }
        outbox::refresh_pending(&mut tx, adapter, &op.entity_id).await?;
        let cid = outbox::record_conflict(
            &mut tx,
            self.now_ms(),
            NewConflict {
                entity: &op.entity,
                entity_id: &op.entity_id,
                op_id: Some(&op.op_id),
                kind: conflict_kind::REJECTED,
                local: Some(&op.payload),
                server: None,
                base: op.base_row.as_ref(),
                fields: &[],
                message: Some(&detail),
                resolution: None,
            },
        )
        .await?;
        outbox::log(&mut tx, self.now_ms(), "warn", Some(&op.feed), "rejected", Some(&format!("{} {} {}: {}", op.op, op.entity, op.entity_id, e.status().unwrap_or(0)))).await?;
        tx.commit().await.map_err(SyncError::db("commit rejected"))?;
        tracing::warn!(entity = %op.entity, id = %op.entity_id, status = ?e.status(), "intent rejected by the server: rolled back");
        self.publish(SyncEvent::TablesChanged(vec![adapter.table().to_string()]));
        self.publish(SyncEvent::Rejected { entity: op.entity.clone(), entity_id: op.entity_id.clone(), message: detail });
        self.publish(SyncEvent::ConflictDetected { id: cid, entity: op.entity.clone(), entity_id: op.entity_id.clone(), kind: conflict_kind::REJECTED.into() });
        Ok(())
    }

    /// The row was deleted on the server while local intents exist. The row stays visible (so "Restaurer" has
    /// something to re-create); the ops wait in `conflict`. Runs inside the caller's transaction; the caller
    /// publishes the event after committing.
    async fn deleted_remotely_in(&self, conn: &mut SqliteConnection, op: &OutboxOp, adapter: &dyn EntityAdapter) -> Result<SyncEvent, SyncError> {
        sqlx::query(&format!("UPDATE _sync_outbox SET state = 'conflict' WHERE entity = ? AND entity_id = ? AND state IN {LIVE_STATES}"))
            .bind(&op.entity)
            .bind(&op.entity_id)
            .execute(&mut *conn)
            .await
            .map_err(SyncError::db("deleted remotely"))?;
        outbox::drop_shadow(conn, &op.entity, &op.entity_id).await?;
        let local = adapter.read_row(conn, &op.entity_id).await?.map(Value::Object);
        outbox::refresh_pending(conn, adapter, &op.entity_id).await?;
        let id = outbox::record_conflict(
            conn,
            self.now_ms(),
            NewConflict {
                entity: &op.entity,
                entity_id: &op.entity_id,
                op_id: Some(&op.op_id),
                kind: conflict_kind::DELETED_REMOTELY,
                local: local.as_ref().or(Some(&op.payload)),
                server: None,
                base: op.base_row.as_ref(),
                fields: &[],
                message: None,
                resolution: None,
            },
        )
        .await?;
        Ok(SyncEvent::ConflictDetected { id, entity: op.entity.clone(), entity_id: op.entity_id.clone(), kind: conflict_kind::DELETED_REMOTELY.into() })
    }

    async fn on_deleted_remotely(&self, op: &OutboxOp, adapter: &dyn EntityAdapter) -> Result<(), SyncError> {
        let mut t = self.begin().await?;
        let event = self.deleted_remotely_in(&mut t, op, adapter).await?;
        t.commit().await.map_err(SyncError::db("commit deleted remotely"))?;
        self.publish(SyncEvent::TablesChanged(vec![adapter.table().to_string()]));
        self.publish(event);
        Ok(())
    }

    async fn on_conflict(&self, op: &OutboxOp, adapter: &dyn EntityAdapter, e: &ApiError) -> Result<(), SyncError> {
        let server_row = match e.body().and_then(|b| adapter.row_from_conflict(b)) {
            Some(r) => Some(r),
            None => adapter.fetch_row(&self.inner.api, &op.entity_id).await?,
        };
        let Some(server_row) = server_row else {
            self.on_deleted_remotely(op, adapter).await?;
            return Ok(());
        };
        let server_etag = adapter.etag_of(&server_row);
        let now = self.now_ms();
        let mut tx = self.begin().await?;
        let mut detected: Option<(i64, &'static str)> = None;
        let seq = outbox::get_shadow(&mut tx, &op.entity, &op.entity_id).await?.and_then(|s| s.change_seq);
        let shadow = Shadow { row: server_row.clone(), etag: server_etag.clone(), change_seq: seq, deleted: false };

        if op.op == "delete" {
            // Local delete vs remote edit: never delete a newer server version silently.
            outbox::set_state(&mut tx, op.seq, OpState::Conflict, Some("412")).await?;
            adapter.upsert_server(&mut tx, &op.entity_id, &server_row, server_etag.as_deref(), seq).await?;
            outbox::refresh_pending(&mut tx, adapter, &op.entity_id).await?;
            let cid = outbox::record_conflict(
                &mut tx,
                now,
                NewConflict {
                    entity: &op.entity,
                    entity_id: &op.entity_id,
                    op_id: Some(&op.op_id),
                    kind: conflict_kind::DELETE_VS_EDIT,
                    local: None,
                    server: Some(&server_row),
                    base: op.base_row.as_ref(),
                    fields: &[],
                    message: None,
                    resolution: None,
                },
            )
            .await?;
            detected = Some((cid, conflict_kind::DELETE_VS_EDIT));
        } else {
            let local = as_map(&op.payload);
            let server = as_map(&server_row);
            let base = if op.op == "create" { None } else { op.base_row.as_ref().map(as_map) };
            let policy = adapter.conflict_policy();
            match policy {
                ConflictPolicy::FieldMerge | ConflictPolicy::Custom(_) => {
                    let merge = match &policy {
                        ConflictPolicy::Custom(c) => c.resolve(base.as_ref(), &local, &server),
                        _ => three_way_merge(base.as_ref(), &local, &server),
                    };
                    if merge.resend.is_empty() {
                        outbox::delete_op(&mut tx, op.seq).await?;
                    } else {
                        requeue(&mut tx, op.seq, merge.resend.clone(), server_etag.as_deref(), &server_row).await?;
                    }
                    outbox::put_shadow(&mut tx, &op.entity, &op.entity_id, &shadow).await?;
                    outbox::rebase(&mut tx, adapter, &op.entity_id).await?;
                    if !merge.conflicting.is_empty() {
                        let cid = outbox::record_conflict(
                            &mut tx,
                            now,
                            NewConflict {
                                entity: &op.entity,
                                entity_id: &op.entity_id,
                                op_id: Some(&op.op_id),
                                kind: conflict_kind::FIELD,
                                local: Some(&op.payload),
                                server: Some(&server_row),
                                base: op.base_row.as_ref(),
                                fields: &merge.conflicting,
                                message: None,
                                resolution: None,
                            },
                        )
                        .await?;
                        detected = Some((cid, conflict_kind::FIELD));
                    }
                }
                ConflictPolicy::LastWriterWins => {
                    requeue(&mut tx, op.seq, local.clone(), server_etag.as_deref(), &server_row).await?;
                    outbox::put_shadow(&mut tx, &op.entity, &op.entity_id, &shadow).await?;
                    outbox::rebase(&mut tx, adapter, &op.entity_id).await?;
                }
                ConflictPolicy::ServerWins => {
                    outbox::delete_op(&mut tx, op.seq).await?;
                    outbox::put_shadow(&mut tx, &op.entity, &op.entity_id, &shadow).await?;
                    outbox::rebase(&mut tx, adapter, &op.entity_id).await?;
                }
                ConflictPolicy::KeepBoth => {
                    let visible = adapter.read_row(&mut tx, &op.entity_id).await?.unwrap_or_else(|| local.clone());
                    match adapter.keep_both_copy(op, &visible) {
                        Some((copy_id, copy)) => {
                            adapter.write_local(&mut tx, &copy_id, &copy).await?;
                            outbox::enqueue(&mut tx, Some(adapter), now, Intent::new(&op.feed, &op.entity, &copy_id, "create", Value::Object(copy))).await?;
                            outbox::delete_op(&mut tx, op.seq).await?;
                            outbox::put_shadow(&mut tx, &op.entity, &op.entity_id, &shadow).await?;
                            outbox::rebase(&mut tx, adapter, &op.entity_id).await?;
                            outbox::record_conflict(
                                &mut tx,
                                now,
                                NewConflict {
                                    entity: &op.entity,
                                    entity_id: &op.entity_id,
                                    op_id: Some(&op.op_id),
                                    kind: conflict_kind::CONTENT,
                                    local: Some(&op.payload),
                                    server: Some(&server_row),
                                    base: op.base_row.as_ref(),
                                    fields: &[],
                                    message: Some(&format!("copy {copy_id}")),
                                    resolution: Some("kept_both"),
                                },
                            )
                            .await?;
                        }
                        None => {
                            outbox::set_state(&mut tx, op.seq, OpState::Conflict, Some("412")).await?;
                            let cid = outbox::record_conflict(
                                &mut tx,
                                now,
                                NewConflict {
                                    entity: &op.entity,
                                    entity_id: &op.entity_id,
                                    op_id: Some(&op.op_id),
                                    kind: conflict_kind::CONTENT,
                                    local: Some(&op.payload),
                                    server: Some(&server_row),
                                    base: op.base_row.as_ref(),
                                    fields: &[],
                                    message: None,
                                    resolution: None,
                                },
                            )
                            .await?;
                            detected = Some((cid, conflict_kind::CONTENT));
                        }
                    }
                }
            }
        }
        outbox::log(&mut tx, now, "warn", Some(&op.feed), "conflict", Some(&format!("{} {} {}", op.op, op.entity, op.entity_id))).await?;
        tx.commit().await.map_err(SyncError::db("commit conflict"))?;
        self.publish(SyncEvent::TablesChanged(vec![adapter.table().to_string()]));
        if let Some((id, kind)) = detected {
            self.publish(SyncEvent::ConflictDetected { id, entity: op.entity.clone(), entity_id: op.entity_id.clone(), kind: kind.into() });
        }
        Ok(())
    }

    // ---------------------------------------------------------------- pull

    async fn pull_locked(&self, feed: &FeedSpec) -> Result<PullReport, SyncError> {
        self.inner.pulls.fetch_add(1, Ordering::SeqCst);
        match self.pull_pages(feed).await {
            Err(SyncError::Api(e)) if e.class() == ErrorClass::CursorExpired => {
                tracing::warn!(feed = %feed.name, "cursor expired on the server: full resync");
                sqlx::query("UPDATE _sync_feeds SET cursor = '0', needs_full = 1 WHERE feed = ?")
                    .bind(&feed.name)
                    .execute(self.inner.db.pool())
                    .await
                    .map_err(SyncError::db("reset feed"))?;
                self.pull_pages(feed).await
            }
            Err(e) => {
                let msg = match &e {
                    SyncError::Api(a) => error_detail(a),
                    other => other.to_string(),
                };
                let _ = sqlx::query("UPDATE _sync_feeds SET last_pull_at = ?, last_error = ? WHERE feed = ?")
                    .bind(self.now_ms())
                    .bind(msg)
                    .bind(&feed.name)
                    .execute(self.inner.db.pool())
                    .await;
                if let SyncError::Api(a) = &e {
                    if matches!(a.class(), ErrorClass::SessionExpired | ErrorClass::Unauthorized) {
                        self.inner.session_expired.store(true, Ordering::SeqCst);
                        return Err(SyncError::SessionExpired);
                    }
                }
                Err(e)
            }
            ok => ok,
        }
    }

    async fn pull_pages(&self, feed: &FeedSpec) -> Result<PullReport, SyncError> {
        let adapter = self.adapter(&feed.entity)?;
        let mut report = PullReport::default();
        let (cursor, needs_full): (String, i64) = sqlx::query_as("SELECT cursor, needs_full FROM _sync_feeds WHERE feed = ?")
            .bind(&feed.name)
            .fetch_one(self.inner.db.pool())
            .await
            .map_err(SyncError::db("read cursor"))?;
        let mut cursor = Cursor::new(cursor);
        let needs_full = needs_full != 0;
        if needs_full && !cursor.is_zero() {
            cursor = Cursor::zero();
        }
        loop {
            let mut req = ApiRequest::get(feed.path.clone());
            for (k, v) in &feed.extra_query {
                req = req.query(k.clone(), v.clone());
            }
            let page = self.inner.api.delta_page_with(req, &cursor, feed.page_limit).await?;
            report.pages += 1;
            let mut tx = self.begin().await?;
            let mut events = Vec::new();
            for (i, change) in page.changes.iter().enumerate() {
                if i == 1 {
                    self.fail("pull.mid_page");
                }
                self.apply_change(&mut tx, feed, adapter.as_ref(), change, &mut PageCtx { needs_full, report: &mut report, events: &mut events }).await?;
            }
            let now = self.now_ms();
            let finished = !page.has_more;
            if finished && needs_full {
                // Rows the full snapshot did not contain are gone on the server (except those with local intents).
                let ids = adapter.list_ids(&mut tx).await?;
                for id in ids {
                    let seen: Option<i64> = sqlx::query_scalar("SELECT 1 FROM _sync_seen WHERE feed = ? AND entity_id = ?")
                        .bind(&feed.name)
                        .bind(&id)
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(SyncError::db("seen"))?;
                    if seen.is_none() && outbox::live_ops(&mut tx, adapter.entity(), &id).await?.is_empty() {
                        adapter.delete_row(&mut tx, &id).await?;
                        report.deleted += 1;
                    }
                }
                sqlx::query("DELETE FROM _sync_seen WHERE feed = ?").bind(&feed.name).execute(&mut *tx).await.map_err(SyncError::db("clear seen"))?;
            }
            sqlx::query("UPDATE _sync_feeds SET cursor = ?, needs_full = ?, last_pull_at = ?, last_ok_at = ?, last_error = NULL WHERE feed = ?")
                .bind(page.cursor.as_str())
                .bind(i64::from(needs_full && !finished))
                .bind(now)
                .bind(now)
                .bind(&feed.name)
                .execute(&mut *tx)
                .await
                .map_err(SyncError::db("advance cursor"))?;
            self.fail("pull.before_commit");
            tx.commit().await.map_err(SyncError::db("commit page"))?;
            for e in events {
                self.publish(e);
            }
            if !page.changes.is_empty() || (finished && needs_full) {
                self.publish(SyncEvent::TablesChanged(vec![adapter.table().to_string()]));
            }
            cursor = page.cursor;
            if finished {
                break;
            }
        }
        Ok(report)
    }

    async fn apply_change(
        &self,
        tx: &mut SqliteConnection,
        feed: &FeedSpec,
        adapter: &dyn EntityAdapter,
        change: &kubuno_desktop_api_client::Change,
        page: &mut PageCtx<'_>,
    ) -> Result<(), SyncError> {
        let PageCtx { needs_full, report, events } = page;
        let needs_full = *needs_full;
        let id = change.uuid.as_str();
        let entity = adapter.entity();
        let is_row = match &change.kind {
            ChangeKind::Modified => true,
            ChangeKind::Other(k) => feed.row_kinds.iter().any(|r| r == k),
            ChangeKind::Deleted | ChangeKind::Revoked => false,
        };
        if !is_row && !matches!(change.kind, ChangeKind::Deleted | ChangeKind::Revoked) {
            // Unknown kind: skipped and logged, never read as a delete.
            tracing::warn!(feed = %feed.name, kind = %change.kind.as_str(), "unknown change kind skipped");
            outbox::log(tx, self.now_ms(), "warn", Some(&feed.name), "unknown_kind", Some(change.kind.as_str())).await?;
            report.skipped += 1;
            return Ok(());
        }
        let ops = outbox::live_ops(tx, entity, id).await?;
        if is_row {
            if needs_full {
                sqlx::query("INSERT OR IGNORE INTO _sync_seen (feed, entity_id) VALUES (?, ?)").bind(&feed.name).bind(id).execute(&mut *tx).await.map_err(SyncError::db("seen"))?;
            }
            let row = adapter.row_from_change(change);
            let etag = adapter.etag_from_change(change);
            let seq = change.change_seq;
            if ops.is_empty() {
                // Order by the server's sequence only: an older change never overwrites a newer one.
                let current = adapter.row_meta(tx, id).await?.and_then(|m| m.1);
                if current.is_some_and(|c| c >= seq) && seq > 0 {
                    report.stale += 1;
                    return Ok(());
                }
                adapter.upsert_server(tx, id, &row, etag.as_deref(), Some(seq)).await?;
                outbox::drop_shadow(tx, entity, id).await?;
                report.applied += 1;
            } else {
                let current = outbox::get_shadow(tx, entity, id).await?.and_then(|s| s.change_seq);
                if current.is_some_and(|c| c >= seq) && seq > 0 {
                    report.stale += 1;
                    return Ok(());
                }
                outbox::put_shadow(tx, entity, id, &Shadow { row, etag, change_seq: Some(seq), deleted: false }).await?;
                outbox::rebase(tx, adapter, id).await?;
                report.shadowed += 1;
            }
            return Ok(());
        }
        // Tombstone (or revoked access).
        if ops.is_empty() {
            adapter.delete_row(tx, id).await?;
            outbox::drop_shadow(tx, entity, id).await?;
            report.deleted += 1;
        } else if ops.iter().all(|o| o.op == "delete") {
            // What we wanted happened: the pending deletes are done.
            sqlx::query(&format!("DELETE FROM _sync_outbox WHERE entity = ? AND entity_id = ? AND state IN {LIVE_STATES}"))
                .bind(entity)
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(SyncError::db("tombstone fulfils delete"))?;
            adapter.delete_row(tx, id).await?;
            outbox::drop_shadow(tx, entity, id).await?;
            report.deleted += 1;
        } else if ops.iter().all(|o| o.state == OpState::Conflict) {
            // Already reported (a 404 on push found the deletion first).
            report.skipped += 1;
        } else if let Some(first) = ops.first() {
            events.push(self.deleted_remotely_in(tx, first, adapter).await?);
            report.shadowed += 1;
        }
        Ok(())
    }

    // ---------------------------------------------------------------- conflicts

    /// Open conflicts, oldest first.
    pub async fn conflicts(&self) -> Result<Vec<ConflictRecord>, SyncError> {
        self.load_conflicts("WHERE resolved_at IS NULL").await
    }

    async fn load_conflicts(&self, filter: &str) -> Result<Vec<ConflictRecord>, SyncError> {
        type R = (i64, String, String, Option<String>, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, i64, Option<String>);
        let rows: Vec<R> = sqlx::query_as(&format!(
            "SELECT id, entity, entity_id, op_id, kind, local, server, base, fields, message, created_at, resolution FROM _sync_conflicts {filter} ORDER BY id"
        ))
        .fetch_all(self.inner.db.pool())
        .await
        .map_err(SyncError::db("conflicts"))?;
        let parse = |s: Option<String>| s.and_then(|t| serde_json::from_str::<Value>(&t).ok());
        Ok(rows
            .into_iter()
            .map(|r| ConflictRecord {
                id: r.0,
                entity: r.1,
                entity_id: r.2,
                op_id: r.3,
                kind: r.4,
                local: parse(r.5),
                server: parse(r.6),
                base: parse(r.7),
                fields: r.8.and_then(|f| serde_json::from_str(&f).ok()).unwrap_or_default(),
                message: r.9,
                created_at: r.10,
                resolution: r.11,
            })
            .collect())
    }

    /// Applies the user's choice for an open conflict.
    pub async fn resolve_conflict(&self, id: i64, choice: Resolution) -> Result<(), SyncError> {
        self.check_writable()?;
        let rec = self.load_conflicts(&format!("WHERE id = {id} AND resolved_at IS NULL")).await?.into_iter().next();
        let Some(rec) = rec else { return Ok(()) };
        let adapter = self.adapter(&rec.entity)?;
        let feed = self.feed_of(&rec.entity);
        let now = self.now_ms();
        let mut tx = self.begin().await?;
        let ops = outbox::live_ops(&mut tx, &rec.entity, &rec.entity_id).await?;
        let conflict_ops: Vec<&OutboxOp> = ops.iter().filter(|o| o.state == OpState::Conflict).collect();
        let server_etag = rec.server.as_ref().and_then(|s| adapter.etag_of(s));
        match (choice, rec.kind.as_str()) {
            (Resolution::KeepServer, conflict_kind::DELETED_REMOTELY) => {
                for o in &ops {
                    outbox::delete_op(&mut tx, o.seq).await?;
                }
                adapter.delete_row(&mut tx, &rec.entity_id).await?;
                outbox::drop_shadow(&mut tx, &rec.entity, &rec.entity_id).await?;
            }
            (Resolution::KeepServer, _) => {
                for o in &conflict_ops {
                    outbox::delete_op(&mut tx, o.seq).await?;
                }
                if let Some(server) = &rec.server {
                    let seq = outbox::get_shadow(&mut tx, &rec.entity, &rec.entity_id).await?.and_then(|s| s.change_seq);
                    outbox::put_shadow(&mut tx, &rec.entity, &rec.entity_id, &Shadow { row: server.clone(), etag: server_etag.clone(), change_seq: seq, deleted: false }).await?;
                    outbox::rebase(&mut tx, adapter.as_ref(), &rec.entity_id).await?;
                }
            }
            (Resolution::KeepMine, conflict_kind::FIELD) => {
                let local = rec.local.as_ref().map(as_map).unwrap_or_default();
                let mine: Map<String, Value> = local.into_iter().filter(|(k, _)| rec.fields.contains(k)).collect();
                if !mine.is_empty() {
                    let intent = Intent::new(&feed, &rec.entity, &rec.entity_id, "patch", Value::Object(mine)).base(server_etag.clone(), rec.server.clone());
                    outbox::enqueue(&mut tx, Some(adapter.as_ref()), now, intent).await?;
                    outbox::rebase(&mut tx, adapter.as_ref(), &rec.entity_id).await?;
                }
            }
            (Resolution::KeepMine, conflict_kind::DELETED_REMOTELY) => {
                // "Restaurer": re-create with the same client id from the visible row.
                for o in &ops {
                    outbox::delete_op(&mut tx, o.seq).await?;
                }
                let row = adapter.read_row(&mut tx, &rec.entity_id).await?.or_else(|| rec.local.as_ref().map(as_map)).unwrap_or_default();
                adapter.write_local(&mut tx, &rec.entity_id, &row).await?;
                outbox::enqueue(&mut tx, Some(adapter.as_ref()), now, Intent::new(&feed, &rec.entity, &rec.entity_id, "create", Value::Object(row))).await?;
            }
            (Resolution::KeepMine, conflict_kind::DELETE_VS_EDIT) => {
                for o in &conflict_ops {
                    outbox::delete_op(&mut tx, o.seq).await?;
                }
                adapter.delete_row(&mut tx, &rec.entity_id).await?;
                let intent = Intent::new(&feed, &rec.entity, &rec.entity_id, "delete", Value::Object(Map::new())).base(server_etag.clone(), rec.server.clone());
                outbox::enqueue(&mut tx, Some(adapter.as_ref()), now, intent).await?;
            }
            (Resolution::KeepMine, _) => {
                for o in &conflict_ops {
                    sqlx::query("UPDATE _sync_outbox SET state = 'pending', idem_key = ? WHERE seq = ?")
                        .bind(outbox::new_key())
                        .bind(o.seq)
                        .execute(&mut *tx)
                        .await
                        .map_err(SyncError::db("requeue conflict"))?;
                }
            }
        }
        outbox::refresh_pending(&mut tx, adapter.as_ref(), &rec.entity_id).await?;
        sqlx::query("UPDATE _sync_conflicts SET resolved_at = ?, resolution = ? WHERE id = ?")
            .bind(now)
            .bind(match choice {
                Resolution::KeepMine => "keep_mine",
                Resolution::KeepServer => "keep_server",
            })
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(SyncError::db("resolve conflict"))?;
        tx.commit().await.map_err(SyncError::db("commit resolve"))?;
        self.publish(SyncEvent::TablesChanged(vec![adapter.table().to_string()]));
        self.kick(Trigger::LocalWrite);
        let _ = self.refresh_status().await;
        Ok(())
    }

    // ---------------------------------------------------------------- status

    /// Recomputes and publishes the status.
    pub async fn refresh_status(&self) -> Result<SyncStatus, SyncError> {
        let pool = self.inner.db.pool();
        let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM _sync_outbox WHERE state IN ('pending','inflight','blocked')")
            .fetch_one(pool)
            .await
            .map_err(SyncError::db("status pending"))?;
        let stuck: i64 = sqlx::query_scalar("SELECT count(*) FROM _sync_outbox WHERE state = 'pending' AND attempts >= 3")
            .fetch_one(pool)
            .await
            .map_err(SyncError::db("status stuck"))?;
        let conflicts: i64 = sqlx::query_scalar("SELECT count(*) FROM _sync_conflicts WHERE resolved_at IS NULL")
            .fetch_one(pool)
            .await
            .map_err(SyncError::db("status conflicts"))?;
        type F = (String, String, i64, Option<i64>, Option<i64>, Option<String>);
        let feeds: Vec<F> = sqlx::query_as("SELECT feed, cursor, needs_full, last_pull_at, last_ok_at, last_error FROM _sync_feeds ORDER BY feed")
            .fetch_all(pool)
            .await
            .map_err(SyncError::db("status feeds"))?;
        let last_error = self.inner.last_error.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let state = if let Some(e) = self.inner.db.degraded() {
            SyncState::ReadOnly { message: e.to_string() }
        } else if self.is_session_expired() {
            SyncState::SessionExpired
        } else if self.is_offline() {
            SyncState::Offline
        } else if self.inner.syncing.load(Ordering::SeqCst) {
            SyncState::Syncing
        } else if let Some(message) = last_error {
            SyncState::Error { message }
        } else if pending > 0 {
            SyncState::Pending { count: pending }
        } else {
            SyncState::Synced
        };
        let status = SyncStatus {
            state,
            last_sync_at: *self.inner.last_sync_at.lock().unwrap_or_else(PoisonError::into_inner),
            pending_count: pending,
            stuck_count: stuck,
            conflict_count: conflicts,
            feeds: feeds
                .into_iter()
                .map(|f| FeedStatus { feed: f.0, cursor: f.1, needs_full: f.2 != 0, last_pull_at: f.3, last_ok_at: f.4, last_error: f.5 })
                .collect(),
        };
        let changed = *self.inner.status_tx.borrow() != status;
        self.inner.status_tx.send_replace(status.clone());
        if changed {
            self.publish(SyncEvent::StatusChanged(status.clone()));
        }
        Ok(status)
    }

    /// Number of live outbox ops (the sign-out dialog: "N modifications n'ont pas été envoyées").
    pub async fn unsent_count(&self) -> Result<i64, SyncError> {
        sqlx::query_scalar(&format!("SELECT count(*) FROM _sync_outbox WHERE state IN {LIVE_STATES}"))
            .fetch_one(self.inner.db.pool())
            .await
            .map_err(SyncError::db("unsent"))
    }

    /// Every op (tests, the activity panel).
    pub async fn outbox(&self) -> Result<Vec<OutboxOp>, SyncError> {
        let mut conn = self.inner.db.pool().acquire().await.map_err(SyncError::db("acquire"))?;
        outbox::all_ops(&mut conn).await
    }
}

/// A local write in progress (see [`SyncEngine::begin_local`]). Dropped without `commit`: rolled back.
pub struct LocalTx {
    tx: Option<Transaction<'static, Sqlite>>,
    engine: SyncEngine,
    tables: BTreeSet<String>,
    enqueued: bool,
}

impl LocalTx {
    fn tx(&mut self) -> Result<&mut Transaction<'static, Sqlite>, SyncError> {
        self.tx.as_mut().ok_or_else(|| SyncError::Db("the local transaction is finished".into()))
    }

    /// The connection, for the app's own statements.
    pub fn conn(&mut self) -> Result<&mut SqliteConnection, SyncError> {
        Ok(&mut **self.tx()?)
    }

    /// Queues an intent the app built itself.
    pub async fn enqueue(&mut self, intent: Intent) -> Result<EnqueueOutcome, SyncError> {
        let adapter = self.engine.adapter(&intent.entity).ok();
        if let Some(a) = &adapter {
            self.tables.insert(a.table().to_string());
        }
        let now = self.engine.now_ms();
        let out = outbox::enqueue(&mut **self.tx()?, adapter.as_deref(), now, intent).await?;
        self.enqueued = true;
        Ok(out)
    }

    /// Creates a row (client-minted id) and queues its `create`.
    pub async fn create(&mut self, entity: &str, id: &str, fields: Map<String, Value>) -> Result<EnqueueOutcome, SyncError> {
        let adapter = self.engine.adapter(entity)?;
        adapter.write_local(&mut **self.tx()?, id, &fields).await?;
        let feed = self.engine.feed_of(entity);
        self.enqueue(Intent::new(&feed, entity, id, "create", Value::Object(fields))).await
    }

    /// Changes fields of a row and queues a `patch` with the server version it was made on.
    pub async fn patch(&mut self, entity: &str, id: &str, fields: Map<String, Value>) -> Result<EnqueueOutcome, SyncError> {
        let adapter = self.engine.adapter(entity)?;
        let conn = &mut **self.tx()?;
        let visible = adapter.read_row(conn, id).await?.ok_or_else(|| SyncError::Config(format!("no {entity} {id}")))?;
        let meta = adapter.row_meta(conn, id).await?;
        let shadow = outbox::get_shadow(conn, entity, id).await?;
        let (base_row, base_etag) = match shadow {
            Some(s) if !s.deleted => (s.row, s.etag),
            _ => (Value::Object(visible), meta.and_then(|m| m.0)),
        };
        adapter.write_local(conn, id, &fields).await?;
        let feed = self.engine.feed_of(entity);
        self.enqueue(Intent::new(&feed, entity, id, "patch", Value::Object(fields)).base(base_etag, Some(base_row))).await
    }

    /// Deletes a row and queues its `delete` (with `If-Match`: never deletes a newer server version silently).
    pub async fn delete(&mut self, entity: &str, id: &str) -> Result<EnqueueOutcome, SyncError> {
        let adapter = self.engine.adapter(entity)?;
        let conn = &mut **self.tx()?;
        let visible = adapter.read_row(conn, id).await?;
        let meta = adapter.row_meta(conn, id).await?;
        let shadow = outbox::get_shadow(conn, entity, id).await?;
        let (base_row, base_etag) = match shadow {
            Some(s) if !s.deleted => (Some(s.row), s.etag),
            _ => (visible.map(Value::Object), meta.and_then(|m| m.0)),
        };
        adapter.delete_row(conn, id).await?;
        let feed = self.engine.feed_of(entity);
        self.enqueue(Intent::new(&feed, entity, id, "delete", Value::Object(Map::new())).base(base_etag, base_row)).await
    }

    /// Marks a table as changed by the app's own statements (for the change notification).
    pub fn touched(&mut self, table: &str) {
        self.tables.insert(table.to_string());
    }

    /// Commits, then notifies the views and asks the scheduler for a (debounced) push.
    pub async fn commit(mut self) -> Result<(), SyncError> {
        let tx = self.tx.take().ok_or_else(|| SyncError::Db("the local transaction is finished".into()))?;
        tx.commit().await.map_err(SyncError::db("commit local"))?;
        let tables: Vec<String> = std::mem::take(&mut self.tables).into_iter().collect();
        if !tables.is_empty() {
            self.engine.publish(SyncEvent::TablesChanged(tables));
        }
        if self.enqueued {
            self.engine.kick(Trigger::LocalWrite);
        }
        let _ = self.engine.refresh_status().await;
        Ok(())
    }
}

/// Re-queues a conflicting intent as a patch on top of the server version (new body, so a new idempotency key).
async fn requeue(conn: &mut SqliteConnection, seq: i64, payload: Map<String, Value>, etag: Option<&str>, server_row: &Value) -> Result<(), SyncError> {
    sqlx::query("UPDATE _sync_outbox SET op = 'patch', payload = ?, base_etag = ?, base_row = ?, idem_key = ?, state = 'pending' WHERE seq = ?")
        .bind(Value::Object(payload).to_string())
        .bind(etag)
        .bind(server_row.to_string())
        .bind(outbox::new_key())
        .bind(seq)
        .execute(conn)
        .await
        .map_err(SyncError::db("requeue"))?;
    Ok(())
}

/// What applying one page of a feed accumulates.
struct PageCtx<'a> {
    needs_full: bool,
    report: &'a mut PullReport,
    events: &'a mut Vec<SyncEvent>,
}
