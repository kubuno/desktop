//! Blob-sync driver — for components whose local data is ONE opaque binary blob
//! (keestore: the client-encrypted KeePass `.kdbx`), not CRUD entities. Contract
//! (COORDINATION_WASM.md Msg 42): the wasm tracks `sync_version`/`synced_version`
//! /`dirty`/`pending_op` in its `/status`; the daemon reconciles:
//!
//!   PUSH — local `dirty` + `pending_op=='put'` → `GET wasm /kdbx` (bytes) →
//!          `PUT core /kdbx` with `X-Sync-Version: synced_version` (server-side
//!          conflict detection) → on 200 `{sync_version}` → `POST wasm /_synced`.
//!          `pending_op=='delete'` → `DELETE core /kdbx` → `_synced {deleted}`.
//!          On 409 (the core moved while we were dirty): server-wins for vault
//!          integrity — drop the pending op via the pull below (logged loudly).
//!   PULL — `core.sync_version > local.synced_version` and NOT dirty →
//!          `GET core /kdbx` (bytes) → `PUT wasm /kdbx?origin=sync&sync_version=
//!          N&file_hash=H`. Core vault deleted → `DELETE wasm /kdbx?origin=sync`.
//!
//! Selected by `sync_mode == "blob"` in the component manifest (fallback: a
//! claim ending in `/kdbx`). Both claims are marked primed after a successful
//! cycle so the proxy serves the vault locally (offline unlock).

use serde_json::Value;

use crate::wasmoffice::{self, Spec};

/// The blob components (today: keestore). Manifest-driven.
fn blob_components() -> Vec<(crate::components::Component, Spec)> {
    crate::components::all()
        .into_iter()
        .filter(|c| {
            c.sync_mode.as_deref() == Some("blob")
                || c.claims.iter().any(|p| p.ends_with("/kdbx"))
        })
        .filter_map(|c| {
            let spec = wasmoffice::spec_for(&c.name, &c.module);
            wasmoffice::enabled_for(spec).then_some((c, spec))
        })
        .collect()
}

/// One reconciliation cycle for every installed blob component.
pub fn cycle(instance_id: &str) -> Result<(), String> {
    if kubuno_sync::is_offline() {
        return Ok(());
    }
    for (c, spec) in blob_components() {
        let Some(base) = c.claims.iter().find(|p| p.ends_with("/kdbx")).cloned() else {
            continue;
        };
        let root = base.trim_end_matches("/kdbx").to_string(); // e.g. /api/v1/keestore
        if let Err(e) = cycle_one(instance_id, spec, &root) {
            eprintln!("[blob_sync] {} : {e}", c.module);
            continue;
        }
        // Vault reconciled → serve it (and its /status) locally from now on.
        for claim in &c.claims {
            crate::components::mark_primed(instance_id, spec, claim);
        }
    }
    Ok(())
}

fn cycle_one(instance_id: &str, spec: Spec, root: &str) -> Result<(), String> {
    let local = status_json(&wasm(spec, instance_id, "GET", &format!("{root}/status"), &[])
        .ok_or("backend indisponible")?)?;

    // ── PUSH: local edits first (they carry the user's latest intent). ──────
    let dirty = local["dirty"].as_bool().unwrap_or(false)
        || local["dirty"].as_i64().unwrap_or(0) != 0;
    if dirty {
        match local["pending_op"].as_str().unwrap_or("") {
            "put" => {
                let (st, blob) = wasm(spec, instance_id, "GET", &format!("{root}/kdbx"), &[])
                    .ok_or("backend indisponible")?;
                if st == 200 {
                    let synced = local["synced_version"].as_i64().unwrap_or(0);
                    let (cst, cbody) =
                        core_put_bytes(instance_id, &format!("{root}/kdbx"), &blob, synced)?;
                    if (200..300).contains(&cst) {
                        let v: Value = serde_json::from_str(&cbody).unwrap_or(Value::Null);
                        let ack = serde_json::json!({
                            "sync_version": v["sync_version"].as_i64().unwrap_or(synced + 1),
                            "file_hash": v["file_hash"].as_str().unwrap_or_default(),
                        });
                        let _ = wasm(spec, instance_id, "POST", &format!("{root}/_synced"),
                                     ack.to_string().as_bytes());
                    } else if cst == 409 {
                        // Concurrent server edit: server-wins (vault integrity) —
                        // the pull below adopts the core version; the local edit
                        // is superseded. Loud log: this loses a local change.
                        eprintln!(
                            "[blob_sync] ⚠ conflit coffre {root} : le serveur a une version plus récente — version serveur adoptée"
                        );
                        let _ = wasm(spec, instance_id, "POST", &format!("{root}/_synced"),
                                     br#"{"sync_version":0,"file_hash":""}"#);
                    } else {
                        return Err(format!("push coffre : HTTP {cst}"));
                    }
                }
            }
            "delete" => {
                let (cst, _) = core_call(instance_id, "DELETE", &format!("{root}/kdbx"), None)?;
                if (200..300).contains(&cst) || cst == 404 {
                    let _ = wasm(spec, instance_id, "POST", &format!("{root}/_synced"),
                                 br#"{"deleted":true}"#);
                } else {
                    return Err(format!("delete coffre : HTTP {cst}"));
                }
            }
            _ => {}
        }
    }

    // ── PULL: adopt a newer core version (only when clean locally). ─────────
    let local = status_json(&wasm(spec, instance_id, "GET", &format!("{root}/status"), &[])
        .ok_or("backend indisponible")?)?;
    let still_dirty = local["dirty"].as_bool().unwrap_or(false)
        || local["dirty"].as_i64().unwrap_or(0) != 0;
    let (cst, cbody) = core_call(instance_id, "GET", &format!("{root}/status"), None)?;
    if cst != 200 {
        return Err(format!("status core : HTTP {cst}"));
    }
    let core: Value = serde_json::from_str(&String::from_utf8_lossy(&cbody))
        .map_err(|e| format!("status core illisible : {e}"))?;
    let core_exists = core["exists"].as_bool().unwrap_or(false);
    let core_ver = core["sync_version"].as_i64().unwrap_or(0);
    let local_synced = local["synced_version"].as_i64().unwrap_or(0);
    let local_exists = local["exists"].as_bool().unwrap_or(false);

    if core_exists && core_ver > local_synced && !still_dirty {
        let (bst, blob) = core_call(instance_id, "GET", &format!("{root}/kdbx"), None)?;
        if bst != 200 {
            return Err(format!("pull coffre : HTTP {bst}"));
        }
        let hash = core["file_hash"].as_str().unwrap_or_default();
        let path = format!("{root}/kdbx?origin=sync&sync_version={core_ver}&file_hash={hash}");
        match wasm(spec, instance_id, "PUT", &path, &blob) {
            Some((st, _)) if (200..300).contains(&st) => {}
            other => return Err(format!("écriture coffre locale : {:?}", other.map(|(s, _)| s))),
        }
    } else if !core_exists && local_exists && !still_dirty {
        let _ = wasm(spec, instance_id, "DELETE", &format!("{root}/kdbx?origin=sync"), &[]);
    }
    Ok(())
}

fn status_json((st, body): &(u16, Vec<u8>)) -> Result<Value, String> {
    if !(200..300).contains(st) {
        return Err(format!("status local : {st}"));
    }
    serde_json::from_slice(body).map_err(|e| format!("status local illisible : {e}"))
}

fn wasm(spec: Spec, instance_id: &str, method: &str, path: &str, body: &[u8]) -> Option<(u16, Vec<u8>)> {
    wasmoffice::handle_for(spec, instance_id, method, path, body).map(|(s, _ct, b)| (s, b))
}

// ── Core access (binary-safe, Bearer + refresh on 401) ──────────────────────

fn core_call(
    instance_id: &str,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> Result<(u16, Vec<u8>), String> {
    use std::io::Read;
    let server = kubuno_sync::server_url(instance_id).ok_or("instance inconnue")?;
    let url = format!("{}{}", server.trim_end_matches('/'), path);
    let attempt = |token: &str| -> Result<(u16, Vec<u8>), String> {
        let req = ureq::request(method, &url).set("Authorization", &format!("Bearer {token}"));
        let resp = match body {
            Some(b) => req.set("Content-Type", "application/octet-stream").send_bytes(b),
            None => req.call(),
        };
        let r = match resp {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let mut buf = Vec::new();
                let _ = r.into_reader().read_to_end(&mut buf);
                return Ok((code, buf));
            }
            Err(e) => return Err(e.to_string()),
        };
        let status = r.status();
        let mut buf = Vec::new();
        r.into_reader().read_to_end(&mut buf).map_err(|e| e.to_string())?;
        Ok((status, buf))
    };
    let (status, bytes) = attempt(&kubuno_sync::access_token(instance_id).unwrap_or_default())?;
    if status == 401 {
        let fresh = kubuno_sync::refresh_access(instance_id).map_err(|e| e.to_string())?;
        return attempt(&fresh);
    }
    Ok((status, bytes))
}

/// PUT the vault to the core with the optimistic-concurrency header.
fn core_put_bytes(
    instance_id: &str,
    path: &str,
    blob: &[u8],
    synced_version: i64,
) -> Result<(u16, String), String> {
    let server = kubuno_sync::server_url(instance_id).ok_or("instance inconnue")?;
    let url = format!("{}{}", server.trim_end_matches('/'), path);
    let attempt = |token: &str| -> Result<(u16, String), String> {
        let resp = ureq::request("PUT", &url)
            .set("Authorization", &format!("Bearer {token}"))
            .set("Content-Type", "application/octet-stream")
            .set("X-Sync-Version", &synced_version.to_string())
            .send_bytes(blob);
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
