//! Drive delta sync: pull the core's drive change-feed (`?full=true`, which embeds
//! the full folder/file model in every change) and feed each page verbatim to the
//! local `drive-core` WASM store via `POST /_ingest`, so Drive folder/file listings
//! are served locally and keep working offline.
//!
//! Pull-only (v1): mutations still go to the core and come back on the next delta.
//! The cursor is owned HERE (persisted per instance, separate from kubuno-sync's
//! own drive cursor); drive-core is stateless about it and applies pages
//! idempotently (conditional upsert by `change_seq`).

use std::path::PathBuf;

use serde_json::Value;

use crate::wasmoffice::{self, DRIVE};

/// Pull the drive delta from the stored cursor and ingest every page into
/// drive-core. No-op when offline or when drive-core isn't installed. Returns the
/// number of changes applied across all pages.
pub fn sync(instance_id: &str) -> Result<u32, String> {
    if kubuno_sync::is_offline() || !wasmoffice::enabled_for(DRIVE) {
        return Ok(0);
    }
    let mut total = 0u32;
    // Page through the delta; bound the loop as a runaway backstop.
    for _ in 0..1000 {
        let cursor = load_cursor(instance_id);
        let path = format!("/api/v1/drive/sync/delta?cursor={cursor}&limit=500&full=true");
        let (status, body) = core_get(instance_id, &path)?;
        if status != 200 {
            return Err(format!("delta drive : HTTP {status} — {}", snippet(&body)));
        }
        let v: Value = serde_json::from_str(&body).map_err(|e| format!("delta illisible : {e}"))?;
        let n = v.get("changes").and_then(|c| c.as_array()).map(|a| a.len()).unwrap_or(0);
        let new_cursor = v.get("cursor").and_then(|x| x.as_i64()).unwrap_or(cursor);
        let has_more = v.get("has_more").and_then(|x| x.as_bool()).unwrap_or(false);

        if n > 0 {
            // Feed the page verbatim ({changes, cursor, …}); drive-core applies all
            // changes in one transaction, idempotent by change_seq.
            match wasmoffice::handle_for(DRIVE, instance_id, "POST", "/api/v1/drive/_ingest", body.as_bytes()) {
                Some((st, _ct, _out)) if (200..300).contains(&st) => {}
                Some((st, _ct, out)) => {
                    return Err(format!("ingest drive : status {st} — {}", snippet(&String::from_utf8_lossy(&out))));
                }
                None => return Err("drive-core indisponible pendant l'ingest".into()),
            }
            total += n as u32;
        }

        save_cursor(instance_id, new_cursor);
        if !has_more || new_cursor == cursor {
            break;
        }
    }
    Ok(total)
}

/// GET a core path with the native session (Bearer + refresh on 401). GET-only
/// mirror of `office_sync::core`.
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

    let token = kubuno_sync::access_token(instance_id).unwrap_or_default();
    let (status, text) = attempt(&token)?;
    if status == 401 {
        let fresh = kubuno_sync::refresh_access(instance_id).map_err(|e| e.to_string())?;
        return attempt(&fresh);
    }
    Ok((status, text))
}

/// File holding the drive delta cursor for an instance (next pull position).
fn cursor_path(instance_id: &str) -> Option<PathBuf> {
    kubuno_sync::db_path(instance_id)
        .ok()?
        .parent()
        .map(|p| p.join("drive_cursor"))
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
