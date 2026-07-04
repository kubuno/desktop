//! Generic entity local-first sync — one engine for EVERY claimed prefix that
//! follows the documents-core v2.2 contract (office sub-modules today; the
//! notes/tasks/keestore/… waves tomorrow, with ZERO desktop change per module).
//! The entity list is DERIVED from the persisted component manifest (claims);
//! only documents (`office_sync`, legacy journal contract) and drive
//! (`drive_sync`/`drive_push`, op-table contract) keep their dedicated loops.
//!
//!   PULL  — `GET {core}<prefix>/delta?cursor&limit(&include=content)`
//!           → `POST {wasm}<prefix>/_ingest` (page verbatim; the wasm applies
//!           transactionally, idempotent by change_seq, and deposits the server
//!           change_seq for the push conflict guard). After the first COMPLETE
//!           pull, the prefix is marked primed → the proxy starts routing it
//!           (components::route_for).
//!   PUSH  — the wasm journals local mutations as REPLAYABLE REQUESTS
//!           (`{seq, method, path, target_id, body, base_seq}`):
//!           `GET {wasm}<prefix>/_changes?since=cursor` → replay verbatim to the
//!           core with a stable `Idempotency-Key` → `POST _ack`. Optimistic
//!           conflict guard: the pull captures the server change_seq of pending
//!           target_ids; base_seq behind → server-wins (op dropped).
//!
//! Mirrors drive_sync/drive_push, which proved the shape. A wasm without
//! `_ingest`/`_changes` (older artifact) → entity skipped (passthrough).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde_json::Value;

use crate::wasmoffice::{self, Spec};

/// Prefixes synced by DEDICATED loops — never by this engine.
const DEDICATED: [&str; 2] = ["/api/v1/office/documents", "/api/v1/drive"];

/// Prefixes whose delta carries no content payload (e.g. whiteboard boards:
/// the drawing is Yjs, outside the delta).
const NO_CONTENT: [&str; 1] = ["/api/v1/office/whiteboard/boards"];

/// One synced entity (a claimed prefix of an installed component).
struct Entity {
    /// The component's wasm spec (documents-core, notes-core, …).
    spec:    Spec,
    /// Filesystem/idempotency-safe key derived from the prefix.
    key:     String,
    /// API prefix (core + wasm share it), also the delta/_changes/_ack base.
    prefix:  String,
    /// Whether to ask the delta for inline content.
    content: bool,
}

/// The entities to sync: every claim of every INSTALLED component, minus the
/// prefixes handled by dedicated loops. Manifest-driven — a new module's wasm
/// starts syncing with no desktop change.
fn entities() -> Vec<Entity> {
    let mut out = Vec::new();
    for c in crate::components::all() {
        let spec = wasmoffice::spec_for(&c.name, &c.module);
        if !wasmoffice::enabled_for(spec) {
            continue;
        }
        for prefix in c.claims {
            if DEDICATED.contains(&prefix.as_str()) {
                continue;
            }
            let key: String = prefix
                .trim_start_matches("/api/v1/")
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
                .collect();
            out.push(Entity {
                spec,
                key,
                content: !NO_CONTENT.contains(&prefix.as_str()),
                prefix,
            });
        }
    }
    out
}

#[derive(Default)]
pub struct Stats {
    pub pulled:    u32,
    pub replayed:  u32,
    pub conflicts: u32,
    pub deferred:  u32,
}

/// One full cycle for every entity: push (replay local outbox) interleaved with
/// pull (reconcile + capture). No-op when offline.
pub fn cycle(instance_id: &str) -> Result<Stats, String> {
    let mut stats = Stats::default();
    if kubuno_sync::is_offline() {
        return Ok(stats);
    }
    for e in entities() {
        match cycle_entity(instance_id, &e) {
            Ok((pulled, replayed, conflicts, deferred)) => {
                stats.pulled += pulled;
                stats.replayed += replayed;
                stats.conflicts += conflicts;
                stats.deferred += deferred;
            }
            Err(err) => eprintln!("[entity_sync] {} : {err}", e.key),
        }
    }
    Ok(stats)
}

fn cycle_entity(instance_id: &str, e: &Entity) -> Result<(u32, u32, u32, u32), String> {
    // 1. Peek pending local mutations (no ack yet).
    let cursor = load_i64(instance_id, &format!("outbox_{}", e.key));
    let pending = peek_outbox(instance_id, e, cursor)?;

    // 2. PULL: reconcile the local store; capture server change_seq of the rows
    //    we are about to push (conflict-guard input).
    let watch: HashSet<String> = pending
        .iter()
        .filter(|o| o.base_seq > 0)
        .map(|o| o.target_id.clone())
        .collect();
    let (pulled, bumped) = pull_entity(instance_id, e, &watch)?;

    if pending.is_empty() {
        return Ok((pulled, 0, 0, 0));
    }

    // 3. Replay in seq order; ack only the resolved contiguous prefix.
    let (mut replayed, mut conflicts, mut deferred) = (0u32, 0u32, 0u32);
    let mut ack_upto = cursor;
    for o in &pending {
        // Server moved this row past the op's base → server-wins, drop the op.
        if o.base_seq > 0 && bumped.get(&o.target_id).is_some_and(|s| *s > o.base_seq) {
            eprintln!(
                "[entity_sync] conflit (server-wins) : {} {} {} base={} serveur={}",
                e.key, o.method, o.target_id, o.base_seq, bumped[&o.target_id]
            );
            conflicts += 1;
            ack_upto = o.seq;
            continue;
        }
        // Stable per-op idempotency key (key = sanitized prefix, e.g.
        // office_spreadsheets / notes) — a network retry never double-applies.
        let key = format!("{}-outbox-{}", e.key, o.seq);
        match core_send(instance_id, &o.method, &o.path, o.body.clone(), &key) {
            Ok((status, _)) if (200..300).contains(&status) => {
                replayed += 1;
                ack_upto = o.seq;
            }
            // Row already gone server-side on a lifecycle op → resolved.
            Ok((404, _)) if o.method == "DELETE" || o.path.ends_with("/trash") || o.path.ends_with("/restore") => {
                replayed += 1;
                ack_upto = o.seq;
            }
            // Permanent rejection: drop it, the pull reconciles the true state.
            Ok((status, body)) if (400..500).contains(&status) => {
                let detail = snippet(&body);
                eprintln!("[entity_sync] {} {} {} rejeté HTTP {status} : {detail}", e.key, o.method, o.path);
                conflicts += 1;
                ack_upto = o.seq;
            }
            // Transient: stop, the unacked tail retries next cycle.
            Ok((status, body)) => {
                eprintln!("[entity_sync] {} {} différé : HTTP {status} — {}", e.key, o.path, snippet(&body));
                deferred += 1;
                break;
            }
            Err(err) => {
                eprintln!("[entity_sync] {} {} différé : {err}", e.key, o.path);
                deferred += 1;
                break;
            }
        }
    }

    // 4. Ack the drained prefix so the wasm prunes its outbox.
    if ack_upto > cursor {
        let body = serde_json::json!({ "upto": ack_upto }).to_string();
        match wasm(e.spec, instance_id, "POST", &format!("{}/_ack", e.prefix), body.as_bytes()) {
            Some((st, _)) if (200..300).contains(&st) => save_i64(instance_id, &format!("outbox_{}", e.key), ack_upto),
            other => eprintln!("[entity_sync] {} _ack : {:?}", e.key, other.map(|(s, _)| s)),
        }
    }
    Ok((pulled, replayed, conflicts, deferred))
}

/// Pull the entity delta from the stored cursor and feed each page verbatim to
/// the wasm `_ingest`. Marks the prefix primed after the first complete pull.
fn pull_entity(
    instance_id: &str,
    e: &Entity,
    watch: &HashSet<String>,
) -> Result<(u32, HashMap<String, i64>), String> {
    let mut bumped: HashMap<String, i64> = HashMap::new();
    let mut total = 0u32;
    let cursor_key = format!("pull_{}", e.key);
    for _ in 0..1000 {
        let cursor = load_i64(instance_id, &cursor_key);
        let content = if e.content { "&include=content" } else { "" };
        let path = format!("{}/delta?cursor={cursor}&limit=200{content}", e.prefix);
        let (status, body) = core_get(instance_id, &path)?;
        if status == 404 {
            return Err("delta indisponible (core sans sync sous-modules ?)".into());
        }
        if status != 200 {
            return Err(format!("delta : HTTP {status} — {}", snippet(&body)));
        }
        let v: Value = serde_json::from_str(&body).map_err(|err| format!("delta illisible : {err}"))?;
        let changes = v.get("changes").and_then(|c| c.as_array());
        let n = changes.map(|a| a.len()).unwrap_or(0);
        let new_cursor = v.get("cursor").and_then(|x| x.as_i64()).unwrap_or(cursor);
        let has_more = v.get("has_more").and_then(|x| x.as_bool()).unwrap_or(false);

        // Conflict-guard capture: server change_seq of watched (pending) rows.
        if let Some(arr) = changes {
            if !watch.is_empty() {
                for ch in arr {
                    let Some(id) = ch.get("id").and_then(|x| x.as_str()) else { continue };
                    if !watch.contains(id) {
                        continue;
                    }
                    if let Some(seq) = ch.get("change_seq").and_then(|x| x.as_i64()) {
                        let s = bumped.entry(id.to_string()).or_insert(seq);
                        *s = (*s).max(seq);
                    }
                }
            }
        }

        if n > 0 {
            // Page verbatim → transactional, idempotent apply (+ implicit _seq).
            match wasm(e.spec, instance_id, "POST", &format!("{}/_ingest", e.prefix), body.as_bytes()) {
                Some((st, _)) if (200..300).contains(&st) => {}
                Some((0, _)) => return Err("wasm sans _ingest (documents-core < 2.2)".into()),
                Some((st, out)) => {
                    return Err(format!("_ingest : status {st} — {}", snippet(&String::from_utf8_lossy(&out))));
                }
                None => return Err("backend office indisponible".into()),
            }
            total += n as u32;
        }

        save_i64(instance_id, &cursor_key, new_cursor);
        if !has_more || new_cursor == cursor {
            // First COMPLETE pull done → the proxy may start serving this prefix
            // from the local store (components::route_for checks this marker).
            crate::components::mark_primed(instance_id, e.spec, &e.prefix);
            break;
        }
    }
    Ok((total, bumped))
}

// ── Local outbox access (wasm, contract v2.1) ───────────────────────────────

struct Op {
    seq:       i64,
    method:    String,
    path:      String,
    target_id: String,
    body:      Option<Vec<u8>>,
    base_seq:  i64,
}

fn peek_outbox(instance_id: &str, e: &Entity, cursor: i64) -> Result<Vec<Op>, String> {
    let path = format!("{}/_changes?since={cursor}", e.prefix);
    let (status, body) = match wasm(e.spec, instance_id, "GET", &path, &[]) {
        Some(r) => r,
        None => return Err("backend office indisponible".into()),
    };
    if status == 0 {
        return Ok(Vec::new()); // wasm without the granular outbox → nothing to push
    }
    if !(200..300).contains(&status) {
        return Err(format!("_changes : status {status}"));
    }
    let v: Value = serde_json::from_slice(&body).map_err(|err| format!("_changes illisible : {err}"))?;
    let mut ops = Vec::new();
    for ch in v.get("changes").and_then(|c| c.as_array()).cloned().unwrap_or_default() {
        let (Some(seq), Some(method), Some(path)) = (
            ch.get("seq").and_then(|x| x.as_i64()),
            ch.get("method").and_then(|x| x.as_str()),
            ch.get("path").and_then(|x| x.as_str()),
        ) else {
            continue;
        };
        ops.push(Op {
            seq,
            method: method.to_string(),
            path: path.to_string(),
            target_id: ch.get("target_id").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
            body: ch.get("body").filter(|b| !b.is_null()).map(|b| b.to_string().into_bytes()),
            base_seq: ch.get("base_seq").and_then(|x| x.as_i64()).unwrap_or(0),
        });
    }
    ops.sort_by_key(|o| o.seq);
    Ok(ops)
}

fn wasm(spec: Spec, instance_id: &str, method: &str, path: &str, body: &[u8]) -> Option<(u16, Vec<u8>)> {
    wasmoffice::handle_for(spec, instance_id, method, path, body).map(|(s, _ct, b)| (s, b))
}

// ── Core access (Bearer + refresh on 401) ───────────────────────────────────

fn core_get(instance_id: &str, path: &str) -> Result<(u16, String), String> {
    let server = kubuno_sync::server_url(instance_id).ok_or("instance inconnue")?;
    let url = format!("{}{}", server.trim_end_matches('/'), path);
    let attempt = |token: &str| -> Result<(u16, String), String> {
        match ureq::get(&url).set("Authorization", &format!("Bearer {token}")).call() {
            Ok(r) => Ok((r.status(), r.into_string().unwrap_or_default())),
            Err(ureq::Error::Status(code, r)) => Ok((code, r.into_string().unwrap_or_default())),
            Err(e) => Err(e.to_string()),
        }
    };
    let (status, text) = attempt(&kubuno_sync::access_token(instance_id).unwrap_or_default())?;
    if status == 401 {
        let fresh = kubuno_sync::refresh_access(instance_id).map_err(|e| e.to_string())?;
        return attempt(&fresh);
    }
    Ok((status, text))
}

fn core_send(
    instance_id: &str,
    method: &str,
    path: &str,
    body: Option<Vec<u8>>,
    idempotency_key: &str,
) -> Result<(u16, String), String> {
    let server = kubuno_sync::server_url(instance_id).ok_or("instance inconnue")?;
    let url = format!("{}{}", server.trim_end_matches('/'), path);
    let attempt = |token: &str| -> Result<(u16, String), String> {
        let req = ureq::request(method, &url)
            .set("Authorization", &format!("Bearer {token}"))
            .set("Idempotency-Key", idempotency_key);
        let resp = match &body {
            Some(b) => req.set("Content-Type", "application/json").send_bytes(b),
            None => req.call(),
        };
        match resp {
            Ok(r) => Ok((r.status(), r.into_string().unwrap_or_default())),
            Err(ureq::Error::Status(code, r)) => Ok((code, r.into_string().unwrap_or_default())),
            Err(e) => Err(e.to_string()),
        }
    };
    let (status, text) = attempt(&kubuno_sync::access_token(instance_id).unwrap_or_default())?;
    if status == 401 {
        let fresh = kubuno_sync::refresh_access(instance_id).map_err(|e| e.to_string())?;
        return attempt(&fresh);
    }
    Ok((status, text))
}

// ── Cursors (files under <instance>/office/) ────────────────────────────────

fn cursor_path(instance_id: &str, name: &str) -> Option<PathBuf> {
    let dir = kubuno_sync::db_path(instance_id).ok()?.parent()?.join("office");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join(format!("cursor_{name}")))
}

fn load_i64(instance_id: &str, name: &str) -> i64 {
    cursor_path(instance_id, name)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn save_i64(instance_id: &str, name: &str, v: i64) {
    if let Some(p) = cursor_path(instance_id, name) {
        let _ = std::fs::write(p, v.to_string());
    }
}

fn snippet(s: &str) -> String {
    s.chars().take(160).collect()
}
