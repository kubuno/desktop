//! Pull sync engine: applies the drive delta into the local folder.
//!
//! Each round fetches `GET /sync/delta?cursor=` and applies the changes in three
//! passes (folders, then files, then tombstones) so a file's parent folder is
//! always known before the file is written. Files are downloaded only when their
//! etag (content hash) differs from what we already have. The cursor is
//! persisted after every page so an interrupted sync resumes cleanly.
//!
//! This is the pull half of the offline-first loop; the push half (local
//! changes → server, with the outbox + If-Match conflict handling) is the next
//! increment.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use crate::{api::Api, config::Config, store::Store};

#[derive(Default)]
pub struct Stats {
    pub downloaded: u32,
    pub folders:    u32,
    pub up_to_date: u32,
    pub deleted:    u32,
}

pub fn sync(api: &mut Api, store: &Store, cfg: &Config) -> Result<Stats> {
    let mut stats = Stats::default();
    let root = &cfg.sync_root;
    std::fs::create_dir_all(root)?;

    loop {
        let cursor = store.cursor()?;
        let delta = api.delta(cursor, 500)?;
        if delta.changes.is_empty() {
            break;
        }

        // Pass 1 — folders (so file parents exist).
        for ch in &delta.changes {
            if ch["kind"] != "folder" {
                continue;
            }
            let id = ch["id"].as_str().unwrap_or_default();
            let path = ch["path"].as_str().unwrap_or_default();
            let trashed = ch["trashed"].as_bool().unwrap_or(false);
            let local = join_rel(root, path);
            if trashed {
                let _ = std::fs::remove_dir_all(&local);
                store.remove_folder(id)?;
            } else {
                std::fs::create_dir_all(&local)?;
                store.upsert_folder(id, path)?;
                stats.folders += 1;
            }
        }

        // Pass 2 — files.
        for ch in &delta.changes {
            if ch["kind"] != "file" {
                continue;
            }
            let id = ch["id"].as_str().unwrap_or_default();
            let name = ch["name"].as_str().unwrap_or_default();
            let etag = ch["etag"].as_str();
            let trashed = ch["trashed"].as_bool().unwrap_or(false);
            let folder_id = ch["folder_id"].as_str();

            if trashed {
                if let Some(lp) = store.file_local_path(id)? {
                    let _ = std::fs::remove_file(lp);
                }
                store.remove_file(id)?;
                stats.deleted += 1;
                continue;
            }

            let folder_path = match folder_id {
                Some(fid) => store.folder_path(fid)?.unwrap_or_default(),
                None => String::new(),
            };
            let local = join_rel(root, &folder_path).join(sanitize(name));

            let prev_etag = store.file_etag(id)?;
            if prev_etag.as_deref() == etag && local.exists() {
                stats.up_to_date += 1;
            } else {
                use anyhow::Context;
                if let Some(parent) = local.parent() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("mkdir {}", parent.display()))?;
                }
                let bytes = api.download(id)?;
                // If the target is an online-only ("virtual") cloud placeholder,
                // writing over it fails with a Cloud Files error. Remove it first
                // so we materialize a fresh, normal file; the desktop app will
                // re-dehydrate it after the sync.
                if crate::push::is_online_only(&local) {
                    let _ = std::fs::remove_file(&local);
                }
                std::fs::write(&local, &bytes)
                    .with_context(|| format!("write {} ({} octets)", local.display(), bytes.len()))?;
                stats.downloaded += 1;
            }
            store.upsert_file(id, folder_id, name, etag, &local.to_string_lossy())?;
        }

        // Pass 3 — tombstones (hard deletes).
        for ch in &delta.changes {
            if ch["kind"] != "deleted" {
                continue;
            }
            let id = ch["id"].as_str().unwrap_or_default();
            if ch["target"].as_str() == Some("file") {
                if let Some(lp) = store.file_local_path(id)? {
                    let _ = std::fs::remove_file(lp);
                }
                store.remove_file(id)?;
            } else {
                store.remove_folder(id)?;
            }
            stats.deleted += 1;
        }

        store.set_cursor(delta.cursor)?;
        if !delta.has_more {
            break;
        }
    }

    Ok(stats)
}

/// Joins a server-provided relative path under `root`, dropping any `..` or
/// absolute components so a crafted path can never escape the sync folder.
fn join_rel(root: &Path, rel: &str) -> PathBuf {
    let mut out = root.to_path_buf();
    for part in rel.split('/') {
        let part = part.trim();
        if part.is_empty() || part == "." || part == ".." {
            continue;
        }
        out.push(part);
    }
    out
}

/// Strips path separators / traversal from a single file name component.
fn sanitize(name: &str) -> String {
    Path::new(name)
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .next_back()
        .unwrap_or_else(|| "fichier".into())
}
