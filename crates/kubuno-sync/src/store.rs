//! Local SQLite mirror of the synced drive: the sync cursor, the folder tree
//! (id → materialized path) and the file index (id, etag, local path). This is
//! the source of truth for what the daemon has already pulled, so reconnections
//! resume from the cursor and unchanged files are skipped.

use std::path::Path;

use anyhow::Result;
use rusqlite::{params, Connection};

/// A pending local change to push to the server.
pub struct OutboxOp {
    pub key:        String, // idempotency key (uuid)
    pub op:         String, // 'create' | 'modify' | 'delete'
    pub file_id:    Option<String>,
    pub folder_id:  Option<String>,
    pub name:       Option<String>,
    pub local_path: Option<String>,
    pub base_etag:  Option<String>,
}

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
                op         TEXT NOT NULL,      -- 'create' | 'modify' | 'delete'
                file_id    TEXT,               -- server id (modify/delete)
                folder_id  TEXT,               -- target folder (create)
                name       TEXT,               -- file name (create)
                local_path TEXT,               -- source path (create/modify)
                base_etag  TEXT                -- expected server etag (modify, for If-Match)
            );
            "#,
        )?;
        Ok(Self { conn })
    }

    /// All known files (id, folder_id, name, etag, local_path).
    pub fn all_files(&self) -> Result<Vec<(String, Option<String>, String, Option<String>, String)>> {
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

    pub fn remove_folder(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM folders WHERE id=?1", params![id])?;
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
}
