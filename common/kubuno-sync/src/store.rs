//! Local SQLite mirror of the synced drive: the sync cursor, the folder tree
//! (id → materialized path) and the file index (id, etag, local path). This is
//! the source of truth for what the daemon has already pulled, so reconnections
//! resume from the cursor and unchanged files are skipped.

use std::path::Path;

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

/// A pending local change to push to the server.
pub struct OutboxOp {
    pub key:        String, // idempotency key (uuid)
    pub op:         String, // 'create' | 'modify' | 'delete' | 'delete_folder'
    pub file_id:    Option<String>,
    pub folder_id:  Option<String>,
    pub name:       Option<String>,
    pub local_path: Option<String>,
    pub base_etag:  Option<String>,
}

/// One row of the file index: (id, folder_id, name, etag, local_path).
pub type FileRow = (String, Option<String>, String, Option<String>, String);

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        // The GUI app may open the store from both the watch thread and a manual
        // sync; wait instead of failing on a concurrent writer.
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS meta    (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS folders (id TEXT PRIMARY KEY, path TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS files   (
                id TEXT PRIMARY KEY, folder_id TEXT, name TEXT NOT NULL,
                etag TEXT, local_path TEXT NOT NULL
            );
            -- Outbox: local changes pending push to the server. Survives restarts,
            -- so changes made offline are replayed on the next sync (offline-first).
            CREATE TABLE IF NOT EXISTS outbox (
                key        TEXT PRIMARY KEY,   -- idempotency key (uuid)
                op         TEXT NOT NULL,      -- 'create' | 'modify' | 'delete' | 'delete_folder'
                file_id    TEXT,               -- server id (modify/delete)
                folder_id  TEXT,               -- target folder (create), or the one being trashed
                name       TEXT,               -- file name (create)
                local_path TEXT,               -- source path (create/modify)
                base_etag  TEXT                -- expected server etag (modify, for If-Match)
            );
            "#,
        )?;
        Ok(Self { conn })
    }

    /// All known files (id, folder_id, name, etag, local_path).
    pub fn all_files(&self) -> Result<Vec<FileRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, folder_id, name, etag, local_path FROM files")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// All known folders (id, materialized server path). Deepest first, so a
    /// child is dealt with before the parent that contains it.
    pub fn all_folders(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, path FROM folders ORDER BY length(path) DESC")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Set of local paths the store already tracks (to detect brand-new files).
    pub fn known_local_paths(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT local_path FROM files")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<std::collections::HashSet<_>, _>>()?;
        Ok(rows)
    }

    pub fn update_file_etag(&self, id: &str, etag: Option<&str>) -> Result<()> {
        self.conn
            .execute("UPDATE files SET etag=?2 WHERE id=?1", params![id, etag])?;
        Ok(())
    }

    /// Reverse lookup: folder id for a materialized path ('' = root → None).
    pub fn folder_id_by_path(&self, path: &str) -> Result<Option<String>> {
        if path.is_empty() {
            return Ok(None);
        }
        Ok(self
            .conn
            .query_row("SELECT id FROM folders WHERE path=?1", params![path], |r| r.get(0))
            .ok())
    }

    // ── Outbox ────────────────────────────────────────────────────────────────

    pub fn enqueue(&self, op: &OutboxOp) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO outbox(key,op,file_id,folder_id,name,local_path,base_etag)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![op.key, op.op, op.file_id, op.folder_id, op.name, op.local_path, op.base_etag],
        )?;
        Ok(())
    }

    pub fn outbox(&self) -> Result<Vec<OutboxOp>> {
        let mut stmt = self.conn.prepare(
            "SELECT key,op,file_id,folder_id,name,local_path,base_etag FROM outbox",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(OutboxOp {
                    key:        r.get(0)?,
                    op:         r.get(1)?,
                    file_id:    r.get(2)?,
                    folder_id:  r.get(3)?,
                    name:       r.get(4)?,
                    local_path: r.get(5)?,
                    base_etag:  r.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn dequeue(&self, key: &str) -> Result<()> {
        self.conn.execute("DELETE FROM outbox WHERE key=?1", params![key])?;
        Ok(())
    }

    pub fn cursor(&self) -> Result<i64> {
        let v: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key='cursor'", [], |r| r.get(0))
            .ok();
        Ok(v.and_then(|s| s.parse().ok()).unwrap_or(0))
    }

    pub fn set_cursor(&self, c: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key,value) VALUES('cursor',?1)
             ON CONFLICT(key) DO UPDATE SET value=?1",
            params![c.to_string()],
        )?;
        Ok(())
    }

    pub fn upsert_folder(&self, id: &str, path: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO folders(id,path) VALUES(?1,?2)
             ON CONFLICT(id) DO UPDATE SET path=?2",
            params![id, path],
        )?;
        Ok(())
    }

    pub fn folder_path(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT path FROM folders WHERE id=?1", params![id], |r| r.get(0))
            .ok())
    }

    /// Removes a folder from the index, together with every folder and file
    /// beneath it.
    ///
    /// The server trashes a folder by cascade — its whole subtree goes with it —
    /// but the index used to drop only the one row. The descendants were then
    /// orphans: present in `folders`/`files`, absent from disk and from the
    /// server, and — because their own parent was now gone and untracked — no
    /// longer detectable as deletions. That is the state that made « delete a
    /// folder, sync, nothing happens » reproducible. Cascading here keeps the
    /// index honest.
    pub fn remove_folder(&self, id: &str) -> Result<()> {
        // The subtree is identified by path prefix: the folder's own path, then
        // anything under `<path>/`. Ids alone will not do — `folders` has no
        // parent column.
        let path: Option<String> = self
            .conn
            .query_row("SELECT path FROM folders WHERE id=?1", params![id], |r| r.get(0))
            .optional()?;
        let Some(path) = path else {
            // Already gone (a retry, or the row a cascade removed): nothing to do.
            return Ok(());
        };
        let under = format!("{}/%", path.trim_end_matches('/'));
        // Files first, then folders, so a foreign-key-less schema still ends up
        // consistent even if interrupted.
        self.conn.execute(
            "DELETE FROM files WHERE local_path IN (\
                 SELECT f.local_path FROM files f JOIN folders d ON f.folder_id = d.id \
                 WHERE d.path = ?1 OR d.path LIKE ?2)",
            params![path, under],
        )?;
        self.conn
            .execute("DELETE FROM folders WHERE path = ?1 OR path LIKE ?2", params![path, under])?;
        Ok(())
    }


    pub fn file_etag(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT etag FROM files WHERE id=?1", params![id], |r| {
                r.get::<_, Option<String>>(0)
            })
            .ok()
            .flatten())
    }

    pub fn file_local_path(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT local_path FROM files WHERE id=?1", params![id], |r| r.get(0))
            .ok())
    }

    /// How many OTHER files (any id but `exclude_id`) the index maps to
    /// `local_path`. Two ids on one local path is a folder-collision artifact;
    /// the mover checks this before deleting a file's old copy, so it never
    /// removes bytes another entry still owns.
    pub fn other_files_at_path(&self, local_path: &str, exclude_id: &str) -> Result<u32> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM files WHERE local_path=?1 AND id<>?2",
            params![local_path, exclude_id],
            |r| r.get(0),
        )?)
    }

    pub fn upsert_file(
        &self,
        id: &str,
        folder_id: Option<&str>,
        name: &str,
        etag: Option<&str>,
        local_path: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO files(id,folder_id,name,etag,local_path) VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(id) DO UPDATE SET folder_id=?2, name=?3, etag=?4, local_path=?5",
            params![id, folder_id, name, etag, local_path],
        )?;
        Ok(())
    }

    pub fn remove_file(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM files WHERE id=?1", params![id])?;
        Ok(())
    }

    /// After the sync folder is moved, rewrite every stored absolute path so its
    /// `old_root` prefix becomes `new_root` (in both the file index and the
    /// outbox). `old_root`/`new_root` must be given without a trailing separator.
    pub fn rebase_paths(&self, old_root: &str, new_root: &str) -> Result<()> {
        let like = format!("{old_root}%");
        // SQLite substr is 1-indexed: keep everything after the old prefix.
        let keep_from = (old_root.len() + 1) as i64;
        self.conn.execute(
            "UPDATE files SET local_path = ?2 || substr(local_path, ?3) WHERE local_path LIKE ?1",
            params![like, new_root, keep_from],
        )?;
        self.conn.execute(
            "UPDATE outbox SET local_path = ?2 || substr(local_path, ?3) WHERE local_path LIKE ?1",
            params![like, new_root, keep_from],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem(tag: &str) -> Store {
        // A store on a throwaway temp file, unique per test — a shared path would
        // let one test see another's rows. `Store::open` runs the schema.
        let path = std::env::temp_dir().join(format!(
            "kbstore-{}-{}.db",
            std::process::id(),
            tag.replace("::", "_").replace("!", "")
        ));
        let _ = std::fs::remove_file(&path);
        Store::open(&path).unwrap()
    }

    /// Trashing a folder must clear its whole subtree from the index, not just
    /// its own row — otherwise the descendants become orphans that no later
    /// scan can detect as deleted (the reported « delete does nothing » bug).
    #[test]
    fn remove_folder_cascades_over_the_subtree() {
        let s = mem(concat!(module_path!(), "::", line!()));
        s.upsert_folder("f1", "/videos").unwrap();
        s.upsert_folder("f2", "/videos/films").unwrap();
        s.upsert_folder("f3", "/music").unwrap();
        s.upsert_file("x1", Some("f2"), "a.mkv", Some("e1"), "C:/root/videos/films/a.mkv").unwrap();
        s.upsert_file("x2", Some("f3"), "b.mp3", Some("e2"), "C:/root/music/b.mp3").unwrap();

        s.remove_folder("f1").unwrap();

        let folders: Vec<String> = s.all_folders().unwrap().into_iter().map(|(id, _)| id).collect();
        assert_eq!(folders, vec!["f3".to_string()], "only the untouched folder remains");
        let files: Vec<String> = s.all_files().unwrap().into_iter().map(|(id, ..)| id).collect();
        assert_eq!(files, vec!["x2".to_string()], "the subtree's file went with it, the sibling stayed");
    }

    /// Removing an id that is not there (a retry, or a row a cascade already
    /// took) is a no-op, not an error.
    #[test]
    fn remove_folder_is_idempotent() {
        let s = mem(concat!(module_path!(), "::", line!()));
        s.upsert_folder("f1", "/a").unwrap();
        s.remove_folder("f1").unwrap();
        s.remove_folder("f1").unwrap();
        assert!(s.all_folders().unwrap().is_empty());
    }

    /// A path prefix must match on a SEGMENT boundary: trashing `/vid` must not
    /// take `/video` with it.
    #[test]
    fn remove_folder_does_not_match_a_name_prefix() {
        let s = mem(concat!(module_path!(), "::", line!()));
        s.upsert_folder("a", "/vid").unwrap();
        s.upsert_folder("b", "/video").unwrap();
        s.remove_folder("a").unwrap();
        let ids: Vec<String> = s.all_folders().unwrap().into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, vec!["b".to_string()], "/video is not under /vid");
    }
}
