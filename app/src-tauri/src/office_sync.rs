//! Office documents sync — Phase 1: PUSH (local → core).
//!
//! The daemon is a reconciliation engine between two API surfaces: the LOCAL
//! office backend (the WASM module, reached via `wasmoffice::handle`) and the
//! remote CORE office API (HTTP). It keeps its own state in
//! `<instance>/office/sync.db` (rusqlite) — separate from the WASM's `docs.db`,
//! which it never touches directly.
//!
//! Detection uses the WASM's local change journal
//! `GET /api/v1/office/documents/_changes?since=<seq>` (kinds: created/modified/trashed/
//! restored/deleted, global monotone `seq`). Each change becomes an outbox op,
//! replayed to the core with an `Idempotency-Key` (+ `If-Match` once we hold an
//! etag). The `local_id ↔ core_uuid` mapping lets the daemon translate ids at
//! the boundary; the webview only ever sees local ids. See COORDINATION_WASM.md.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use rusqlite::Connection;
use serde_json::Value;

#[derive(Default)]
pub struct PushStats {
    pub created:  u32,
    pub modified: u32,
    pub trashed:  u32,
    pub restored: u32,
    pub deleted:  u32,
    pub pending:  u32,
}

impl PushStats {
    fn summary(&self) -> String {
        format!(
            "↑ office : {} créé(s), {} modifié(s), {} corbeille, {} restauré(s), {} supprimé(s), {} en attente",
            self.created, self.modified, self.trashed, self.restored, self.deleted, self.pending
        )
    }
}

#[derive(Default)]
pub struct PullStats {
    pub downloaded: u32, // new server docs materialized locally
    pub updated:    u32, // existing docs updated from the server
    pub trashed:    u32,
    pub deleted:    u32,
}

impl PullStats {
    fn summary(&self) -> String {
        format!(
            "↓ office : {} nouveau(x), {} mis à jour, {} corbeille, {} supprimé(s)",
            self.downloaded, self.updated, self.trashed, self.deleted
        )
    }
}

/// One full sync cycle: push local changes up, then pull server changes down.
/// No-op if the local WASM backend is absent.
pub fn sync(instance_id: &str) -> Result<(PushStats, PullStats)> {
    if !crate::wasmoffice::enabled() {
        return Ok((PushStats::default(), PullStats::default()));
    }
    let db = open_db(instance_id)?;
    detect(instance_id, &db)?;
    let push = drain(instance_id, &db)?;
    let pull = pull_changes(instance_id, &db)?;
    // Echo guard: applying pulled changes via the local API journals them in
    // `_changes`; advance `last_seq` past them so the next push doesn't replay
    // server-originated changes back to the core. (Concurrent local edits during
    // the pull window are a Phase 3 refinement.)
    consume_local_echoes(instance_id, &db)?;
    Ok((push, pull))
}

/// Public summary helper for the command layer.
pub fn sync_summary(instance_id: &str) -> Result<String> {
    let (push, pull) = sync(instance_id)?;
    Ok(format!("{}\n{}", push.summary(), pull.summary()))
}

// ── State (sync.db) ─────────────────────────────────────────────────────────

fn sync_db_path(instance_id: &str) -> Result<PathBuf> {
    let dir = kubuno_sync::db_path(instance_id)?
        .parent()
        .map(|p| p.join("office"))
        .ok_or_else(|| anyhow!("chemin office introuvable"))?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("sync.db"))
}

fn open_db(instance_id: &str) -> Result<Connection> {
    let conn = Connection::open(sync_db_path(instance_id)?)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mapping (
            local_id   TEXT PRIMARY KEY,
            core_uuid  TEXT NOT NULL,
            core_etag  TEXT,
            updated_at TEXT
         );
         CREATE TABLE IF NOT EXISTS meta (
            key   TEXT PRIMARY KEY,
            value TEXT
         );
         CREATE TABLE IF NOT EXISTS outbox (
            key        TEXT PRIMARY KEY,
            op         TEXT NOT NULL,
            local_id   TEXT NOT NULL,
            base_etag  TEXT,
            created_at TEXT
         );",
    )?;
    // Pull needs the core content etag to know when to refetch the .kbdoc.
    // ALTER is idempotent-ish: ignore the error if the column already exists.
    let _ = conn.execute("ALTER TABLE mapping ADD COLUMN core_content_etag TEXT", []);
    Ok(conn)
}

fn get_meta(db: &Connection, key: &str) -> Result<Option<String>> {
    let v = db
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get::<_, String>(0))
        .ok();
    Ok(v)
}

fn set_meta(db: &Connection, key: &str, value: &str) -> Result<()> {
    db.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
        [key, value],
    )?;
    Ok(())
}

fn core_uuid(db: &Connection, local_id: &str) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT core_uuid FROM mapping WHERE local_id = ?1",
            [local_id],
            |r| r.get::<_, String>(0),
        )
        .ok())
}

fn core_etag(db: &Connection, local_id: &str) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT core_etag FROM mapping WHERE local_id = ?1",
            [local_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten())
}

fn put_mapping(db: &Connection, local_id: &str, uuid: &str, etag: Option<&str>) -> Result<()> {
    db.execute(
        "INSERT OR REPLACE INTO mapping (local_id, core_uuid, core_etag, updated_at)
         VALUES (?1, ?2, ?3, datetime('now'))",
        rusqlite::params![local_id, uuid, etag],
    )?;
    Ok(())
}

/// Reverse lookup (pull direction): the local id mapped to a core uuid, if any.
fn local_id_by_uuid(db: &Connection, uuid: &str) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT local_id FROM mapping WHERE core_uuid = ?1",
            [uuid],
            |r| r.get::<_, String>(0),
        )
        .ok())
}

fn content_etag(db: &Connection, local_id: &str) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT core_content_etag FROM mapping WHERE local_id = ?1",
            [local_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten())
}

/// Insert/replace a full mapping including both etags (pull direction).
fn put_mapping_full(
    db: &Connection,
    local_id: &str,
    uuid: &str,
    etag: Option<&str>,
    content_etag: Option<&str>,
) -> Result<()> {
    db.execute(
        "INSERT OR REPLACE INTO mapping (local_id, core_uuid, core_etag, core_content_etag, updated_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))",
        rusqlite::params![local_id, uuid, etag, content_etag],
    )?;
    Ok(())
}

/// Update both etags on an existing mapping (pull applied an update).
fn set_etags(db: &Connection, local_id: &str, etag: Option<&str>, content_etag: Option<&str>) -> Result<()> {
    db.execute(
        "UPDATE mapping SET core_etag = ?2, core_content_etag = ?3, updated_at = datetime('now')
         WHERE local_id = ?1",
        rusqlite::params![local_id, etag, content_etag],
    )?;
    Ok(())
}

fn update_etag(db: &Connection, local_id: &str, etag: Option<&str>) -> Result<()> {
    db.execute(
        "UPDATE mapping SET core_etag = ?2, updated_at = datetime('now') WHERE local_id = ?1",
        rusqlite::params![local_id, etag],
    )?;
    Ok(())
}

fn drop_mapping(db: &Connection, local_id: &str) -> Result<()> {
    db.execute("DELETE FROM mapping WHERE local_id = ?1", [local_id])?;
    Ok(())
}

/// Enqueue one pending op per (op, local_id) — the row's `key` is its stable
/// idempotency key (persisted, so a retry replays the same key).
fn enqueue(db: &Connection, op: &str, local_id: &str) -> Result<()> {
    let exists: i64 = db.query_row(
        "SELECT COUNT(*) FROM outbox WHERE op = ?1 AND local_id = ?2",
        [op, local_id],
        |r| r.get(0),
    )?;
    if exists > 0 {
        return Ok(());
    }
    db.execute(
        "INSERT INTO outbox (key, op, local_id, base_etag, created_at)
         VALUES (?1, ?2, ?3, NULL, datetime('now'))",
        rusqlite::params![new_key(), op, local_id],
    )?;
    Ok(())
}

/// Remove a still-pending `create` for a doc deleted before it ever reached the
/// core (it never existed there — nothing to push).
fn drop_pending_create(db: &Connection, local_id: &str) -> Result<()> {
    db.execute(
        "DELETE FROM outbox WHERE op = 'create' AND local_id = ?1",
        [local_id],
    )?;
    Ok(())
}

fn outbox(db: &Connection) -> Result<Vec<(String, String, String)>> {
    let mut stmt = db.prepare("SELECT key, op, local_id FROM outbox ORDER BY created_at")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn dequeue(db: &Connection, key: &str) -> Result<()> {
    db.execute("DELETE FROM outbox WHERE key = ?1", [key])?;
    Ok(())
}

fn new_key() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("oc-{t:x}-{n:x}")
}

// ── Detection (via the local `_changes` journal) ────────────────────────────

fn detect(instance_id: &str, db: &Connection) -> Result<()> {
    let last_seq: i64 = get_meta(db, "last_seq")?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let (status, body) = local_call(
        instance_id,
        "GET",
        &format!("/api/v1/office/documents/_changes?since={last_seq}"),
        &[],
    )?;
    if status == 404 {
        // Older WASM without the journal → fall back to a full list diff (creates).
        return detect_creates_fulldiff(instance_id, db);
    }
    if status != 200 {
        return Err(anyhow!("journal local _changes : HTTP {status}"));
    }
    let v: Value = serde_json::from_slice(&body).context("journal _changes illisible")?;
    let changes = v.get("changes").and_then(|c| c.as_array()).cloned().unwrap_or_default();
    let mut max_seq = last_seq;
    for ch in changes {
        let Some(local_id) = ch.get("local_id").and_then(|x| x.as_str()) else { continue };
        let kind = ch.get("kind").and_then(|x| x.as_str()).unwrap_or("");
        if let Some(seq) = ch.get("seq").and_then(|x| x.as_i64()) {
            max_seq = max_seq.max(seq);
        }
        let mapped = core_uuid(db, local_id)?.is_some();
        match kind {
            // Not yet on the core → push a create (it carries the latest content).
            "created" | "modified" | "restored" if !mapped => enqueue(db, "create", local_id)?,
            "modified" => enqueue(db, "modify", local_id)?,
            "trashed" if mapped => enqueue(db, "trash", local_id)?,
            "restored" => enqueue(db, "restore", local_id)?,
            "deleted" => {
                if mapped {
                    enqueue(db, "delete", local_id)?;
                } else {
                    drop_pending_create(db, local_id)?; // never reached the core
                }
            }
            _ => {}
        }
    }
    set_meta(db, "last_seq", &max_seq.to_string())?;
    Ok(())
}

/// Fallback for a WASM without `_changes`: enqueue a create for every local doc
/// not yet mapped.
fn detect_creates_fulldiff(instance_id: &str, db: &Connection) -> Result<()> {
    let (status, body) = local_call(instance_id, "GET", "/api/v1/office/documents", &[])?;
    if status != 200 {
        return Err(anyhow!("liste locale : HTTP {status}"));
    }
    let v: Value = serde_json::from_slice(&body)?;
    for d in v.get("documents").and_then(|d| d.as_array()).cloned().unwrap_or_default() {
        if d.get("is_trashed").and_then(|x| x.as_bool()).unwrap_or(false) {
            continue;
        }
        let Some(local_id) = d.get("id").and_then(|x| x.as_str()) else { continue };
        if core_uuid(db, local_id)?.is_none() {
            enqueue(db, "create", local_id)?;
        }
    }
    Ok(())
}

// ── Drain (replay outbox to the core) ───────────────────────────────────────

fn drain(instance_id: &str, db: &Connection) -> Result<PushStats> {
    let mut stats = PushStats::default();
    for (key, op, local_id) in outbox(db)? {
        let r = match op.as_str() {
            "create" => push_create(instance_id, db, &key, &local_id),
            "modify" => push_modify(instance_id, db, &key, &local_id),
            "trash" => push_lifecycle(instance_id, db, &local_id, "trash"),
            "restore" => push_lifecycle(instance_id, db, &local_id, "restore"),
            "delete" => push_delete(instance_id, db, &local_id),
            _ => Ok(()), // unknown op → drop
        };
        match r {
            Ok(()) => {
                match op.as_str() {
                    "create" => stats.created += 1,
                    "modify" => stats.modified += 1,
                    "trash" => stats.trashed += 1,
                    "restore" => stats.restored += 1,
                    "delete" => stats.deleted += 1,
                    _ => {}
                }
                dequeue(db, &key)?;
            }
            Err(e) => {
                eprintln!("[office_sync] {op} {local_id} différé : {e}");
                stats.pending += 1;
            }
        }
    }
    Ok(stats)
}

/// Create a locally-born document on the core, push its content, store mapping.
fn push_create(instance_id: &str, db: &Connection, key: &str, local_id: &str) -> Result<()> {
    if core_uuid(db, local_id)?.is_some() {
        return Ok(()); // already mapped (retry that previously succeeded)
    }
    let Some(doc) = read_local(instance_id, local_id)? else {
        return Ok(()); // vanished locally before push
    };
    let title = doc_title(&doc);
    let (cst, cbody) = core(
        instance_id,
        "POST",
        "/api/v1/office/documents",
        Some(serde_json::json!({ "title": title }).to_string().into_bytes()),
        Some(key),
        None,
    )?;
    if !(cst == 200 || cst == 201) {
        return Err(anyhow!("création core : HTTP {cst} — {}", snippet(&cbody)));
    }
    let created: Value = serde_json::from_str(&cbody).context("réponse création core illisible")?;
    let uuid = created
        .pointer("/document/id")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("uuid absent de la réponse de création"))?
        .to_string();
    let mut etag = created.pointer("/document/etag").and_then(|x| x.as_str()).map(String::from);

    // POST doesn't carry content → PATCH the initial content_json (same key + etag).
    if let Some(content) = doc.get("content_json").filter(|c| !c.is_null()) {
        let (pst, pbody) = core(
            instance_id,
            "PATCH",
            &format!("/api/v1/office/documents/{uuid}"),
            Some(serde_json::json!({ "content_json": content }).to_string().into_bytes()),
            Some(key),
            etag.as_deref(),
        )?;
        if pst == 200 {
            etag = serde_json::from_str::<Value>(&pbody)
                .ok()
                .and_then(|v| v.pointer("/document/etag").and_then(|x| x.as_str()).map(String::from))
                .or(etag);
        }
    }
    put_mapping(db, local_id, &uuid, etag.as_deref())?;
    Ok(())
}

/// Push a metadata/content change of an already-mapped document.
fn push_modify(instance_id: &str, db: &Connection, key: &str, local_id: &str) -> Result<()> {
    let Some(uuid) = core_uuid(db, local_id)? else {
        return Ok(()); // not mapped yet → the pending create will carry it
    };
    let Some(doc) = read_local(instance_id, local_id)? else {
        return Ok(());
    };
    let mut payload = serde_json::Map::new();
    payload.insert("title".into(), Value::String(doc_title(&doc)));
    if let Some(content) = doc.get("content_json").filter(|c| !c.is_null()) {
        payload.insert("content_json".into(), content.clone());
    }
    let (st, body) = core(
        instance_id,
        "PATCH",
        &format!("/api/v1/office/documents/{uuid}"),
        Some(Value::Object(payload).to_string().into_bytes()),
        Some(key),
        core_etag(db, local_id)?.as_deref(),
    )?;
    if st != 200 {
        return Err(anyhow!("modif core : HTTP {st} — {}", snippet(&body)));
    }
    let new_etag = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| v.pointer("/document/etag").and_then(|x| x.as_str()).map(String::from));
    update_etag(db, local_id, new_etag.as_deref())?;
    Ok(())
}

/// trash / restore — `POST /api/v1/office/documents/:uuid/{trash,restore}`.
fn push_lifecycle(instance_id: &str, db: &Connection, local_id: &str, action: &str) -> Result<()> {
    let Some(uuid) = core_uuid(db, local_id)? else { return Ok(()) };
    let (st, body) = core(
        instance_id,
        "POST",
        &format!("/api/v1/office/documents/{uuid}/{action}"),
        Some(b"{}".to_vec()),
        None,
        None,
    )?;
    if !(st == 200 || st == 204) {
        return Err(anyhow!("{action} core : HTTP {st} — {}", snippet(&body)));
    }
    Ok(())
}

/// Hard delete — `DELETE /api/v1/office/documents/:uuid/delete` (→ 204) — then drop mapping.
fn push_delete(instance_id: &str, db: &Connection, local_id: &str) -> Result<()> {
    let Some(uuid) = core_uuid(db, local_id)? else { return Ok(()) };
    let (st, body) = core(
        instance_id,
        "DELETE",
        &format!("/api/v1/office/documents/{uuid}/delete"),
        None,
        None,
        core_etag(db, local_id)?.as_deref(),
    )?;
    if !(st == 200 || st == 204 || st == 404) {
        return Err(anyhow!("suppression core : HTTP {st} — {}", snippet(&body)));
    }
    drop_mapping(db, local_id)?;
    Ok(())
}

// ── Pull (core delta → local) ───────────────────────────────────────────────

fn pull_changes(instance_id: &str, db: &Connection) -> Result<PullStats> {
    let mut stats = PullStats::default();
    // Page through the delta; bound the loop as a runaway backstop.
    for _ in 0..1000 {
        let cursor: i64 = get_meta(db, "pull_cursor")?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let (st, body) = core(
            instance_id,
            "GET",
            &format!("/api/v1/office/documents/delta?cursor={cursor}&limit=200"),
            None,
            None,
            None,
        )?;
        if st != 200 {
            return Err(anyhow!("delta core : HTTP {st} — {}", snippet(&body)));
        }
        let v: Value = serde_json::from_str(&body).context("delta illisible")?;
        for ch in v.get("changes").and_then(|c| c.as_array()).cloned().unwrap_or_default() {
            if let Err(e) = apply_pull_change(instance_id, db, &ch, &mut stats) {
                eprintln!("[office_sync] pull change différé : {e}");
            }
        }
        let new_cursor = v.get("cursor").and_then(|x| x.as_i64()).unwrap_or(cursor);
        set_meta(db, "pull_cursor", &new_cursor.to_string())?;
        let has_more = v.get("has_more").and_then(|x| x.as_bool()).unwrap_or(false);
        if !has_more || new_cursor == cursor {
            break;
        }
    }
    Ok(stats)
}

fn apply_pull_change(
    instance_id: &str,
    db: &Connection,
    ch: &Value,
    stats: &mut PullStats,
) -> Result<()> {
    let uuid = ch.get("uuid").and_then(|x| x.as_str()).ok_or_else(|| anyhow!("uuid absent"))?;
    let kind = ch.get("kind").and_then(|x| x.as_str()).unwrap_or("");
    let local = local_id_by_uuid(db, uuid)?;
    let etag = ch.pointer("/etag").and_then(|x| x.as_str());
    let cet = ch.get("content_etag").and_then(|x| x.as_str());
    let document = ch.get("document");
    let title = document
        .and_then(|d| d.get("title"))
        .and_then(|x| x.as_str())
        .unwrap_or("Sans titre");
    let is_starred = document.and_then(|d| d.get("is_starred")).and_then(|x| x.as_bool());

    match kind {
        "deleted" => {
            if let Some(lid) = local {
                let _ = local_call(instance_id, "DELETE", &format!("/api/v1/office/documents/{lid}/delete"), &[]);
                drop_mapping(db, &lid)?;
                stats.deleted += 1;
            }
        }
        "trashed" => {
            if let Some(lid) = local {
                let _ = local_call(instance_id, "POST", &format!("/api/v1/office/documents/{lid}/trash"), b"{}");
                stats.trashed += 1;
            }
        }
        "modified" => {
            // Already at this version (e.g. our own push reflected back in the
            // delta)? Skip — avoids a redundant local PATCH and a phantom update.
            if let Some(lid) = &local {
                if etag.is_some() && core_etag(db, lid)?.as_deref() == etag {
                    return Ok(());
                }
            }
            // Build the metadata patch; refetch content only if it changed.
            let mut payload = serde_json::Map::new();
            payload.insert("title".into(), Value::String(title.to_string()));
            if let Some(s) = is_starred {
                payload.insert("is_starred".into(), Value::Bool(s));
            }
            match local {
                Some(lid) => {
                    if cet != content_etag(db, &lid)?.as_deref() {
                        if let Some(content) = fetch_core_content(instance_id, uuid)? {
                            payload.insert("content_json".into(), content);
                        }
                    }
                    local_call(instance_id, "PATCH", &format!("/api/v1/office/documents/{lid}"),
                        Value::Object(payload).to_string().as_bytes())?;
                    set_etags(db, &lid, etag, cet)?;
                    stats.updated += 1;
                }
                None => {
                    // New server document → materialize it locally.
                    let (cst, cbody) = local_call(
                        instance_id, "POST", "/api/v1/office/documents",
                        serde_json::json!({ "title": title }).to_string().as_bytes(),
                    )?;
                    if !(cst == 200 || cst == 201) {
                        return Err(anyhow!("création locale : HTTP {cst}"));
                    }
                    let created: Value = serde_json::from_slice(&cbody)?;
                    let new_lid = created
                        .pointer("/document/id")
                        .and_then(|x| x.as_str())
                        .ok_or_else(|| anyhow!("id local absent"))?
                        .to_string();
                    if let Some(content) = fetch_core_content(instance_id, uuid)? {
                        payload.insert("content_json".into(), content);
                    }
                    let _ = local_call(instance_id, "PATCH", &format!("/api/v1/office/documents/{new_lid}"),
                        Value::Object(payload).to_string().as_bytes());
                    put_mapping_full(db, &new_lid, uuid, etag, cet)?;
                    stats.downloaded += 1;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Fetch a document's `content_json` from the core (for refetch on content change).
fn fetch_core_content(instance_id: &str, uuid: &str) -> Result<Option<Value>> {
    let (st, body) = core(instance_id, "GET", &format!("/api/v1/office/documents/{uuid}"), None, None, None)?;
    if st != 200 {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(&body)?;
    Ok(v.get("content_json").filter(|c| !c.is_null()).cloned())
}

/// After applying pulled changes (which journal local `_changes` echoes), fast-
/// forward `last_seq` past them so push doesn't replay server-originated changes.
fn consume_local_echoes(instance_id: &str, db: &Connection) -> Result<()> {
    let last: i64 = get_meta(db, "last_seq")?.and_then(|s| s.parse().ok()).unwrap_or(0);
    let (st, body) = local_call(instance_id, "GET", &format!("/api/v1/office/documents/_changes?since={last}"), &[])?;
    if st == 200 {
        if let Ok(v) = serde_json::from_slice::<Value>(&body) {
            if let Some(max) = v.get("seq").and_then(|x| x.as_i64()) {
                if max > last {
                    set_meta(db, "last_seq", &max.to_string())?;
                }
            }
        }
    }
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Read a local document `{document, content_json}` from the WASM backend.
fn read_local(instance_id: &str, local_id: &str) -> Result<Option<Value>> {
    let (st, body) = local_call(instance_id, "GET", &format!("/api/v1/office/documents/{local_id}"), &[])?;
    if st == 404 {
        return Ok(None);
    }
    if st != 200 {
        return Err(anyhow!("lecture locale {local_id} : HTTP {st}"));
    }
    Ok(Some(serde_json::from_slice(&body)?))
}

fn doc_title(doc: &Value) -> String {
    doc.pointer("/document/title")
        .and_then(|t| t.as_str())
        .unwrap_or("Sans titre")
        .to_string()
}

fn snippet(s: &str) -> String {
    s.chars().take(160).collect()
}

fn local_call(instance_id: &str, method: &str, path: &str, body: &[u8]) -> Result<(u16, Vec<u8>)> {
    crate::wasmoffice::handle(instance_id, method, path, body)
        .map(|(status, _ctype, body)| (status, body))
        .ok_or_else(|| anyhow!("backend office local indisponible"))
}

// ── Core API access (auth-injected, refresh on 401) ─────────────────────────

fn core(
    instance_id: &str,
    method: &str,
    path: &str,
    body: Option<Vec<u8>>,
    idempotency_key: Option<&str>,
    if_match: Option<&str>,
) -> Result<(u16, String)> {
    let server = kubuno_sync::server_url(instance_id).ok_or_else(|| anyhow!("instance inconnue"))?;
    let url = format!("{}{}", server.trim_end_matches('/'), path);

    let attempt = |token: &str| -> Result<(u16, String)> {
        let mut req = ureq::request(method, &url).set("Authorization", &format!("Bearer {token}"));
        if let Some(k) = idempotency_key {
            req = req.set("Idempotency-Key", k);
        }
        if let Some(m) = if_match {
            req = req.set("If-Match", m);
        }
        let resp = match &body {
            Some(b) => req.set("Content-Type", "application/json").send_bytes(b),
            None => req.call(),
        };
        match resp {
            Ok(r) => Ok((r.status(), r.into_string().unwrap_or_default())),
            Err(ureq::Error::Status(code, r)) => Ok((code, r.into_string().unwrap_or_default())),
            Err(e) => Err(anyhow!(e)),
        }
    };

    let token = kubuno_sync::access_token(instance_id).unwrap_or_default();
    let (status, text) = attempt(&token)?;
    if status == 401 {
        let fresh = kubuno_sync::refresh_access(instance_id)?;
        return attempt(&fresh);
    }
    Ok((status, text))
}
