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

/// Env-gated one-line trace of every raw change the delta hands us, for
/// debugging what the server actually sends versus how the client classifies
/// it. Off unless `KB_SYNC_TRACE` is set; when on, prints kind / trashed /
/// folder_id / name|path to stderr, which is how the PaintSharp-never-synced
/// gap was pinned down.
fn trace(ch: &serde_json::Value) {
    if std::env::var_os("KB_SYNC_TRACE").is_some() {
        eprintln!(
            "TRACE kind={} trashed={} target={} folder_id={} name={} path={}",
            ch["kind"], ch["trashed"], ch["target"], ch["folder_id"], ch["name"], ch["path"]
        );
    }
}

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

        // Directories left behind by a server-side folder MOVE. Collected in
        // pass 1 and cleaned up after the file pass has emptied them (see the
        // block before `set_cursor`). This mirrors the file reconciliation
        // further down; without it, a renamed folder's old tree is left
        // orphaned on disk and the next push re-uploads it as " (2)" — the same
        // cascade the file fix already closes, one level up.
        let mut moved_from: Vec<PathBuf> = Vec::new();

        // Pass 1 — folders (so file parents exist).
        for ch in &delta.changes {
            trace(ch);
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
                // A move/rename lands this id at a NEW materialized path while
                // the store still holds its OLD one. Record the old directory so
                // the cleanup after the file pass can drop it once its children
                // have moved out.
                if let Some(prev_path) = store.folder_path(id)? {
                    let prev_local = join_rel(root, &prev_path);
                    if prev_local != local && prev_local.exists() {
                        moved_from.push(prev_local);
                    }
                }
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

            // Where this id lived locally before this change. A server-side move
            // or rename lands the file at a NEW `local`; the old path must be
            // dropped, otherwise it is left orphaned on disk AND untracked (the
            // store below now points the id at the new path). The next push would
            // then walk that stale file, see a path the store doesn't know, treat
            // it as brand-new and re-upload it — the server appends " (2)" on the
            // name collision, that copy is pulled back, re-uploaded as
            // " (2) (2)"… an unbounded duplication cascade, and every server-side
            // move gets silently undone. Captured before the write so it survives
            // the up-to-date short-circuit too.
            let prev_local = store.file_local_path(id)?;

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
            // Move/rename reconciliation: drop the stale copy at the previous path
            // once the file exists at its new one. Same id, different path → a
            // move, never a delete.
            if let Some(prev) = prev_local {
                if prev != local.to_string_lossy() {
                    // Don't delete a file another tracked entry still points to:
                    // when two folders collided onto one local path, both ids
                    // shared this file, and removing it for the mover would wipe
                    // the survivor's bytes on disk — which then read as a delete
                    // and got trashed on the server. The store still holds this
                    // id at `prev` (its upsert is below), so "another id here"
                    // means a genuine second owner.
                    if store.other_files_at_path(&prev, id)? == 0 {
                        let _ = std::fs::remove_file(&prev);
                    }
                }
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

        // The file pass (2) has moved every child to its new home and the
        // tombstone pass (3) applied every hard delete, so any directory a
        // folder move emptied can now be dropped. `prune_emptied_dirs` is
        // empty-only, so a child that was not reconciled is never taken with it.
        prune_emptied_dirs(&mut moved_from);

        store.set_cursor(delta.cursor)?;
        if !delta.has_more {
            break;
        }
    }

    Ok(stats)
}

/// Removes the directories a server-side folder move left behind — deepest
/// first, and only when actually empty.
///
/// `remove_dir` (never `remove_dir_all`) is the entire safety guarantee: a
/// directory that still holds a file the file pass has not reconciled is left
/// standing for a later round rather than deleted with its contents. Deepest
/// first so a moved parent collapses once its moved children are gone.
fn prune_emptied_dirs(dirs: &mut [PathBuf]) {
    dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for dir in dirs.iter() {
        // Ignored on purpose: `NotEmpty` means "keep it", `NotFound` means
        // "already gone" — neither is an error worth failing the sync over.
        let _ = std::fs::remove_dir(dir);
    }
}

/// Joins a server-provided relative path under `root`, dropping any `..` or
/// absolute components so a crafted path can never escape the sync folder.
///
/// Shared with the push side, which has to ask the same question in reverse:
/// where would this server folder be on disk, so it can tell whether the user
/// deleted it. Both directions MUST agree, or a folder would look missing the
/// moment its name needed sanitizing.
pub(crate) fn join_rel(root: &Path, rel: &str) -> PathBuf {
    let mut out = root.to_path_buf();
    for part in rel.split('/') {
        let part = part.trim();
        if part.is_empty() || part == "." || part == ".." {
            continue;
        }
        // Folder names come from the server too, so they need the same
        // treatment as file names — a folder called "11:51" is just as
        // impossible to create here.
        out.push(sanitize(part));
    }
    out
}

/// Characters Windows refuses in a name. `/` and `\` are separators and are
/// dealt with by the component split above, but they are listed for the case
/// where a single component contains one.
#[cfg(windows)]
const ILLEGAL: &[char] = &['<', '>', ':', '"', '|', '?', '*', '/', '\\'];
#[cfg(not(windows))]
const ILLEGAL: &[char] = &['/'];

/// Device names Windows reserves whatever the extension.
#[cfg(windows)]
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];
#[cfg(not(windows))]
const RESERVED: &[&str] = &[];

/// Turns one server-side name into a name the local filesystem accepts.
///
/// Path separators and traversal are stripped, then the characters the platform
/// refuses are replaced. The mapping is DETERMINISTIC and never empty: the
/// store keys a file by its server id and remembers the local path, so the same
/// server name must always yield the same local name — otherwise a second sync
/// would download the file again under a different name.
///
/// A name that cannot be written blocks the whole pull (the write error aborts
/// the cycle), so this is not cosmetic: one file named `Test 11:51.kbsld` was
/// enough to stop every sync on Windows.
fn sanitize(name: &str) -> String {
    let base = Path::new(name)
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .next_back()
        .unwrap_or_default();

    // Illegal and control characters become '-', which reads as a separator
    // where a ':' or '|' used to be.
    let mut out: String = base
        .chars()
        .map(|c| if ILLEGAL.contains(&c) || c.is_control() { '-' } else { c })
        .collect();

    // Windows drops trailing dots and spaces silently, which would make the
    // name we store differ from the name on disk.
    let trimmed = out.trim_end_matches([' ', '.']);
    if trimmed.len() != out.len() {
        out = trimmed.to_string();
    }

    if out.is_empty() {
        return "fichier".into();
    }

    // A reserved device name is refused with or without an extension.
    let stem = out.split('.').next().unwrap_or(&out).to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        out.insert(0, '_');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{prune_emptied_dirs, sanitize};
    use std::path::PathBuf;

    /// The reconciliation of a server-side folder move: the old directory, once
    /// its children have moved out, is removed — nested first, so a moved parent
    /// collapses with its moved child. A directory that still holds an
    /// un-reconciled file is KEPT, never deleted with its contents.
    #[test]
    fn a_moved_folder_is_pruned_only_when_empty() {
        let base = std::env::temp_dir().join(format!("kbsync-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let parent = base.join("videos");
        let child = parent.join("films");
        let keep = base.join("archive");
        std::fs::create_dir_all(&child).unwrap();      // videos/films, both empty
        std::fs::create_dir_all(&keep).unwrap();
        std::fs::write(keep.join("still-here.mkv"), b"x").unwrap(); // archive is NOT empty

        // Passed in shallow-first ON PURPOSE, to prove the sort makes the depth
        // order right regardless of caller order.
        let mut dirs: Vec<PathBuf> = vec![parent.clone(), child.clone(), keep.clone()];
        prune_emptied_dirs(&mut dirs);

        assert!(!child.exists(), "the moved child directory is gone");
        assert!(!parent.exists(), "its now-empty parent went too, deepest-first");
        assert!(keep.exists(), "a directory with an un-reconciled file is untouched");
        assert!(keep.join("still-here.mkv").exists(), "and its file survives");
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// The case that stopped every sync: a colon in a name coming from mobile.
    #[test]
    fn a_colon_no_longer_blocks_the_sync() {
        assert_eq!(sanitize("Test mobile 11:51:22.kbsld"), "Test mobile 11-51-22.kbsld");
    }

    /// Every character Windows refuses is replaced, and the extension survives.
    #[test]
    fn illegal_characters_are_replaced() {
        let out = sanitize(r#"a<b>c:d"e|f?g*h.txt"#);
        assert!(!out.contains(['<', '>', ':', '"', '|', '?', '*']));
        assert!(out.ends_with(".txt"));
    }

    /// Path separators and traversal never escape the sync folder.
    #[test]
    fn separators_and_traversal_are_stripped() {
        assert_eq!(sanitize("../../etc/passwd"), "passwd");
        assert_eq!(sanitize(r"..\..\windows\system32"), "system32");
        assert!(!sanitize("a/b").contains('/'));
    }

    /// The mapping is stable: the same server name always gives the same local
    /// name, or the next sync would download the file all over again.
    #[test]
    fn the_mapping_is_deterministic() {
        for name in ["Test 11:51.kbsld", "rapport|2026.pdf", "note.txt"] {
            assert_eq!(sanitize(name), sanitize(name));
        }
    }

    /// A name that is nothing but illegal characters still yields something.
    #[test]
    fn a_name_is_never_empty() {
        assert!(!sanitize("").is_empty());
        assert!(!sanitize("...").is_empty());
        assert!(!sanitize("   ").is_empty());
    }

    /// Reserved device names are escaped, extension or not.
    #[test]
    #[cfg(windows)]
    fn reserved_device_names_are_escaped() {
        assert_eq!(sanitize("CON"), "_CON");
        assert_eq!(sanitize("nul.txt"), "_nul.txt");
        assert_eq!(sanitize("console.txt"), "console.txt", "only exact device names");
    }

    /// Windows silently drops trailing dots and spaces; we do it ourselves so
    /// the stored path matches what lands on disk.
    #[test]
    #[cfg(windows)]
    fn trailing_dots_and_spaces_are_trimmed() {
        assert_eq!(sanitize("dossier ."), "dossier");
        assert_eq!(sanitize("fichier.txt "), "fichier.txt");
    }
}
