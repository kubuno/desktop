//! Push half of the sync loop: local changes → server.
//!
//! Runs BEFORE the pull so local edits are sent (with `If-Match`) before the
//! pull could overwrite them. Detected changes are recorded in the SQLite outbox
//! and then drained to the server; an op that fails (offline) stays queued and
//! is replayed on the next sync — the offline-first guarantee.
//!
//! Conflict handling (Nextcloud-style): if the server changed a file since we
//! last saw it, `put_content` returns 412; we rename the local edit to
//! `… (conflit <host> <ts>)` and let the pull restore the server version. The
//! renamed copy is then uploaded as a new file on the next push.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use anyhow::Result;
use sha2::{Digest, Sha256};

use crate::{
    api::{Api, PutResult},
    config::Config,
    store::{OutboxOp, Store},
};

#[derive(Default)]
pub struct PushStats {
    pub uploaded:  u32,
    pub modified:  u32,
    pub deleted:   u32,
    pub conflicts: u32,
    pub pending:   u32,
}

pub fn push(api: &mut Api, store: &Store, cfg: &Config) -> Result<PushStats> {
    detect(store, cfg)?;
    drain(api, store, cfg)
}

/// Records local create/modify/delete into the outbox (skipping ops already queued).
fn detect(store: &Store, cfg: &Config) -> Result<()> {
    let ob = store.outbox()?;
    let queued_files: HashSet<String> = ob.iter().filter_map(|o| o.file_id.clone()).collect();
    let queued_paths: HashSet<String> = ob.iter().filter_map(|o| o.local_path.clone()).collect();

    // Tracked files → modify or delete.
    for (id, _folder_id, _name, etag, local_path) in store.all_files()? {
        if queued_files.contains(&id) {
            continue;
        }
        let p = Path::new(&local_path);
        if !p.exists() {
            store.enqueue(&OutboxOp {
                key: new_key(), op: "delete".into(),
                file_id: Some(id), folder_id: None, name: None, local_path: None, base_etag: None,
            })?;
        } else if is_online_only(p) {
            // Online-only ("virtual") placeholder: its content isn't on disk, so
            // it can't have local edits. Reading it to hash would needlessly
            // hydrate (download) it — skip.
        } else if Some(&hash_file(p)?) != etag.as_ref() {
            store.enqueue(&OutboxOp {
                key: new_key(), op: "modify".into(),
                file_id: Some(id), folder_id: None, name: None,
                local_path: Some(local_path), base_etag: etag,
            })?;
        }
    }

    // Brand-new local files → create. The parent folder hierarchy (if new) is
    // created on the server at drain time via ensure_folder.
    let known = store.known_local_paths()?;
    for entry in walk(&cfg.sync_root) {
        let lp = entry.to_string_lossy().to_string();
        if known.contains(&lp) || queued_paths.contains(&lp) {
            continue;
        }
        let name = entry
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        store.enqueue(&OutboxOp {
            key: new_key(), op: "create".into(),
            file_id: None, folder_id: None, name: Some(name), local_path: Some(lp), base_etag: None,
        })?;
    }
    Ok(())
}

/// Returns the server folder id for a materialized path ('' = root → None),
/// creating any missing folders (and their parents) on the server.
fn ensure_folder(api: &mut Api, store: &Store, rel_path: &str) -> Result<Option<String>> {
    if rel_path.is_empty() {
        return Ok(None);
    }
    if let Some(id) = store.folder_id_by_path(rel_path)? {
        return Ok(Some(id));
    }
    let (parent_path, name) = split_last(rel_path);
    let parent_id = ensure_folder(api, store, &parent_path)?;
    // Stable key per path → a retry replays instead of hitting the unique constraint.
    let (id, path) = api.create_folder(parent_id.as_deref(), &name, &format!("mkdir:{rel_path}"))?;
    store.upsert_folder(&id, &path)?;
    Ok(Some(id))
}

/// Splits a materialized path into (parent, last): "/a/b" → ("/a", "b").
fn split_last(rel: &str) -> (String, String) {
    match rel.rfind('/') {
        Some(i) => (rel[..i].to_string(), rel[i + 1..].to_string()),
        None => (String::new(), rel.to_string()),
    }
}

/// Replays queued ops to the server. Failures stay queued (offline-first).
fn drain(api: &mut Api, store: &Store, cfg: &Config) -> Result<PushStats> {
    let mut s = PushStats::default();
    for op in store.outbox()? {
        match op.op.as_str() {
            "modify" => {
                let id = op.file_id.clone().unwrap_or_default();
                let path = op.local_path.clone().unwrap_or_default();
                let Ok(data) = std::fs::read(&path) else {
                    store.dequeue(&op.key)?; // file vanished before push
                    continue;
                };
                match api.put_content(&id, data, op.base_etag.as_deref(), &op.key) {
                    Ok(PutResult::Updated(etag)) => {
                        store.update_file_etag(&id, etag.as_deref())?;
                        store.dequeue(&op.key)?;
                        s.modified += 1;
                    }
                    Ok(PutResult::Conflict) => {
                        make_conflict_copy(Path::new(&path))?;
                        store.dequeue(&op.key)?;
                        s.conflicts += 1;
                    }
                    Err(e) => {
                        eprintln!("  modification différée ({id}) : {e}");
                        s.pending += 1;
                    }
                }
            }
            "delete" => {
                let id = op.file_id.clone().unwrap_or_default();
                match api.trash(&id, &op.key) {
                    Ok(()) => {
                        store.remove_file(&id)?;
                        store.dequeue(&op.key)?;
                        s.deleted += 1;
                    }
                    Err(e) => {
                        eprintln!("  suppression différée ({id}) : {e}");
                        s.pending += 1;
                    }
                }
            }
            "create" => {
                let path = op.local_path.clone().unwrap_or_default();
                let name = op.name.clone().unwrap_or_default();
                let Ok(data) = std::fs::read(&path) else {
                    store.dequeue(&op.key)?;
                    continue;
                };
                // Create the (possibly new) parent folder hierarchy first.
                let rel_parent = relative_parent(&cfg.sync_root, Path::new(&path));
                let folder_id = match ensure_folder(api, store, &rel_parent) {
                    Ok(f) => f,
                    Err(e) => {
                        eprintln!("  dossier différé ('{rel_parent}') : {e}");
                        s.pending += 1;
                        continue;
                    }
                };
                match api.upload(folder_id.as_deref(), &name, data, &op.key) {
                    Ok((id, etag)) => {
                        store.upsert_file(&id, folder_id.as_deref(), &name, etag.as_deref(), &path)?;
                        store.dequeue(&op.key)?;
                        s.uploaded += 1;
                    }
                    Err(e) => {
                        eprintln!("  envoi différé ('{name}') : {e}");
                        s.pending += 1;
                    }
                }
            }
            _ => {
                store.dequeue(&op.key)?;
            }
        }
    }
    Ok(s)
}

/// True if `path` is a cloud placeholder whose content is not on disk (online-
/// only). Reads only attributes, so it never triggers hydration.
#[cfg(windows)]
pub(crate) fn is_online_only(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
    std::fs::metadata(path)
        .map(|m| m.file_attributes() & FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS != 0)
        .unwrap_or(false)
}
#[cfg(not(windows))]
pub(crate) fn is_online_only(_path: &Path) -> bool {
    false
}

fn hash_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(hex::encode(h.finalize()))
}

fn make_conflict_copy(path: &Path) -> Result<()> {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "desktop".into());
    let target = path.with_file_name(format!("{stem} (conflit {host} {ts}){ext}"));
    std::fs::rename(path, target)?;
    Ok(())
}

fn new_key() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Recursively lists regular files under `root`.
fn walk(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.is_file() {
                out.push(p);
            }
        }
    }
    out
}

/// Materialized server path of `entry`'s parent ('/photos' style; '' = root).
fn relative_parent(root: &Path, entry: &Path) -> String {
    let rel = entry.strip_prefix(root).unwrap_or(entry);
    let Some(parent) = rel.parent() else { return String::new() };
    let mut s = String::new();
    for c in parent.components() {
        if let Component::Normal(n) = c {
            s.push('/');
            s.push_str(&n.to_string_lossy());
        }
    }
    s
}
