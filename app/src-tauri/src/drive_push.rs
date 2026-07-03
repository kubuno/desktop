//! Drive offline mutations — the PUSH half of Drive local-first (v2).
//!
//! When drive-core owns a mutation route it applies the change to its local store
//! AND journals it in an `outbox`; the webview gets an optimistic response and the
//! app stays fully usable offline. This module drains that outbox back to the real
//! core once online:
//!
//!   1. peek the pending ops  — `GET  /api/v1/drive/_changes?since=<cursor>` (drive-core)
//!   2. PULL the server delta — reconciles the local store AND captures the server
//!      `change_seq` of every pending row (for the conflict guard)
//!   3. replay each op to the core backend, in `seq` order, with a stable
//!      `Idempotency-Key` (`drive-outbox-<seq>`) so a retry never double-applies
//!   4. acknowledge the drained prefix — `POST /api/v1/drive/_ack {upto:N}` (drive-core)
//!
//! Conflict guard (optimistic, server-wins): an op carries the `base_seq` — the row's
//! synced `change_seq` when it was made. If the PULL shows the server moved that row
//! past `base_seq`, a concurrent server edit raced the offline one → we DROP the local
//! op (the pull already applied the server's version) and log it. See COORDINATION_WASM.md
//! (drive-core v2, Message 25).

use std::collections::HashSet;
use std::path::PathBuf;

use serde_json::Value;

use crate::wasmoffice::{self, DRIVE};

#[derive(Default)]
pub struct PushStats {
    pub replayed:  u32,
    pub conflicts: u32,
    pub deferred:  u32,
}

/// One pending outbox entry, as journaled by drive-core.
struct Op {
    seq:      i64,
    kind:     String, // "folder" | "file"
    op:       String, // create | rename | move | star | color | trash | restore | delete
    id:       String, // target row id
    args:     Value,  // body payload for ops that carry one
    base_seq: i64,    // synced change_seq at op time (0 for a create)
}

/// Full local-first cycle for Drive: PULL (reconcile + capture) then PUSH (replay
/// the outbox). No-op when offline or when drive-core isn't installed.
pub fn cycle(instance_id: &str) -> Result<PushStats, String> {
    let mut stats = PushStats::default();
    if kubuno_sync::is_offline() || !wasmoffice::enabled_for(DRIVE) {
        return Ok(stats);
    }

    // 1. Peek the pending ops (do NOT advance the cursor here).
    let cursor = load_cursor(instance_id);
    let pending = peek_outbox(instance_id, cursor)?;

    // 2. Pull the server delta — reconciles the local store and captures the server
    //    change_seq of the rows we are about to push (conflict guard input).
    let watch: HashSet<String> = pending
        .iter()
        .filter(|o| o.base_seq > 0)
        .map(|o| o.id.clone())
        .collect();
    let (_pulled, bumped) = crate::drive_sync::pull(instance_id, &watch)?;

    if pending.is_empty() {
        return Ok(stats);
    }

    // 3. Replay each op in seq order. We can only ack a contiguous prefix, so we
    //    stop at the first transient failure and ack up to the last resolved seq.
    let mut ack_upto = cursor;
    for o in &pending {
        // Conflict guard: the server moved this row past the op's base → server-wins.
        if o.base_seq > 0 && bumped.get(&o.id).is_some_and(|s| *s > o.base_seq) {
            eprintln!(
                "[drive_push] conflit (server-wins) : {} {} {} base={} serveur={} — op abandonnée",
                o.kind, o.op, o.id, o.base_seq, bumped[&o.id]
            );
            stats.conflicts += 1;
            ack_upto = o.seq; // resolved (dropped): let the pull's version stand
            continue;
        }

        match replay(instance_id, o) {
            Replayed::Ok => {
                stats.replayed += 1;
                ack_upto = o.seq;
            }
            // Permanent rejection (4xx that a retry won't fix): drop it, the pull
            // reconciles the true state. Treated as resolved so it doesn't wedge.
            Replayed::Rejected(code, msg) => {
                let detail = snippet(&msg);
                eprintln!("[drive_push] {} {} {} rejeté HTTP {code} : {detail} — abandonné", o.kind, o.op, o.id);
                stats.conflicts += 1;
                ack_upto = o.seq;
            }
            // Transient (network / 5xx): stop; the unacked tail retries next cycle.
            Replayed::Transient(msg) => {
                eprintln!("[drive_push] {} {} {} différé : {}", o.kind, o.op, o.id, snippet(&msg));
                stats.deferred += 1;
                break;
            }
        }
    }

    // 4. Acknowledge the drained prefix so drive-core purges it.
    if ack_upto > cursor {
        ack(instance_id, ack_upto)?;
        save_cursor(instance_id, ack_upto);
    }
    Ok(stats)
}

enum Replayed {
    Ok,
    Rejected(u16, String),
    Transient(String),
}

/// Replay one outbox op to the real core backend (op → request per the v2 contract).
fn replay(instance_id: &str, o: &Op) -> Replayed {
    let folder = o.kind == "folder";
    let base = if folder {
        format!("/api/v1/drive/folders/{}", o.id)
    } else {
        format!("/api/v1/drive/{}", o.id)
    };
    let (method, path, body): (&str, String, Option<Vec<u8>>) = match o.op.as_str() {
        // create carries {id, name, parent_id} verbatim (folder only in v2).
        "create" if folder => ("POST", "/api/v1/drive/folders".into(), Some(o.args.to_string().into_bytes())),
        "rename" => ("PATCH", format!("{base}/rename"), Some(o.args.to_string().into_bytes())),
        "move" => ("PATCH", format!("{base}/move"), Some(o.args.to_string().into_bytes())),
        "color" if folder => ("PATCH", format!("{base}/color"), Some(o.args.to_string().into_bytes())),
        "star" => ("POST", format!("{base}/star"), None),
        "trash" => ("POST", format!("{base}/trash"), None),
        "restore" => ("POST", format!("{base}/restore"), None),
        "delete" => ("DELETE", base, None),
        other => {
            return Replayed::Rejected(0, format!("op inconnue « {other} » ({})", o.kind));
        }
    };

    let key = format!("drive-outbox-{}", o.seq);
    match core_send(instance_id, method, &path, body, &key) {
        Ok((status, _)) if (200..300).contains(&status) => Replayed::Ok,
        // Idempotent lifecycle: the row is already gone/trashed on the server.
        Ok((404, _)) if matches!(o.op.as_str(), "delete" | "trash" | "restore") => Replayed::Ok,
        Ok((status, body)) if (400..500).contains(&status) => Replayed::Rejected(status, body),
        Ok((status, body)) => Replayed::Transient(format!("HTTP {status} — {}", snippet(&body))),
        Err(e) => Replayed::Transient(e),
    }
}

// ── drive-core outbox access (via the WASM handle) ──────────────────────────

/// Peek the pending outbox ops (seq > cursor) without acking.
fn peek_outbox(instance_id: &str, cursor: i64) -> Result<Vec<Op>, String> {
    let path = format!("/api/v1/drive/_changes?since={cursor}");
    let (status, body) = match wasmoffice::handle_for(DRIVE, instance_id, "GET", &path, &[]) {
        Some((st, _ct, out)) => (st, out),
        None => return Err("drive-core indisponible pour _changes".into()),
    };
    if !(200..300).contains(&status) {
        return Err(format!("_changes : status {status}"));
    }
    let v: Value = serde_json::from_slice(&body).map_err(|e| format!("_changes illisible : {e}"))?;
    let mut ops = Vec::new();
    for ch in v.get("changes").and_then(|c| c.as_array()).cloned().unwrap_or_default() {
        let (Some(seq), Some(op), Some(id)) = (
            ch.get("seq").and_then(|x| x.as_i64()),
            ch.get("op").and_then(|x| x.as_str()),
            ch.get("id").and_then(|x| x.as_str()),
        ) else {
            continue;
        };
        ops.push(Op {
            seq,
            kind: ch.get("kind").and_then(|x| x.as_str()).unwrap_or("folder").to_string(),
            op: op.to_string(),
            id: id.to_string(),
            args: ch.get("args").cloned().unwrap_or(Value::Null),
            base_seq: ch.get("base_seq").and_then(|x| x.as_i64()).unwrap_or(0),
        });
    }
    // drive-core already returns them in seq order, but be defensive.
    ops.sort_by_key(|o| o.seq);
    Ok(ops)
}

/// Ack the drained prefix (purge outbox seq <= upto).
fn ack(instance_id: &str, upto: i64) -> Result<(), String> {
    let body = serde_json::json!({ "upto": upto }).to_string();
    match wasmoffice::handle_for(DRIVE, instance_id, "POST", "/api/v1/drive/_ack", body.as_bytes()) {
        Some((st, _ct, _)) if (200..300).contains(&st) => Ok(()),
        Some((st, _ct, out)) => Err(format!("_ack : status {st} — {}", snippet(&String::from_utf8_lossy(&out)))),
        None => Err("drive-core indisponible pour _ack".into()),
    }
}

// ── Core backend access (auth-injected, refresh on 401) ─────────────────────

/// Send a mutation to the real core with a stable idempotency key. Mirrors
/// `office_sync::core` but Drive-scoped (no If-Match; drive dedups by key).
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

    let token = kubuno_sync::access_token(instance_id).unwrap_or_default();
    let (status, text) = attempt(&token)?;
    if status == 401 {
        let fresh = kubuno_sync::refresh_access(instance_id).map_err(|e| e.to_string())?;
        return attempt(&fresh);
    }
    Ok((status, text))
}

// ── Outbox cursor (last acked seq) ──────────────────────────────────────────

fn cursor_path(instance_id: &str) -> Option<PathBuf> {
    kubuno_sync::db_path(instance_id)
        .ok()?
        .parent()
        .map(|p| p.join("drive_outbox"))
}

fn load_cursor(instance_id: &str) -> i64 {
    cursor_path(instance_id)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(0)
}

fn save_cursor(instance_id: &str, cursor: i64) {
    if let Some(p) = cursor_path(instance_id) {
        let _ = std::fs::write(p, cursor.to_string());
    }
}

fn snippet(s: &str) -> String {
    s.chars().take(160).collect()
}
