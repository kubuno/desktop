//! The local database of one app for one account (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §6).
//!
//! - sqlx (SQLite) — the stack of `kubuno-desktop-data`, so the views and the engine can share one pool per database;
//! - SQLCipher when a key is given (`PRAGMA key` is the first statement of every connection — sqlx puts it first —
//!   and statement logging is disabled so the key never reaches a log); opening with a key on a build without
//!   SQLCipher **fails** instead of silently writing a clear database;
//! - pragmas: WAL, `synchronous=NORMAL`, foreign keys, `busy_timeout=10000`, a 32 MB page cache (decrypted pages
//!   stay cached: the SE-0 spike measured full scans +300 % with the default cache, +70 % with this one);
//! - the engine's own tables (`_sync_*`) through versioned migrations kept in this crate (not in
//!   `_sqlx_migrations`, so they never collide with the app's numbering);
//! - the app's migrations through a `sqlx::migrate::Migrator` — the files of `kubuno-desktop-data`'s *Migrations* node,
//!   embedded with `sqlx::migrate!` — forward only; a failure opens the database **read-only** with the error kept
//!   (`LocalDb::degraded`), the outbox untouched, never a destructive fallback;
//! - `_sync_meta.account_key` checked at every open: a database never syncs for another account.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use kubuno_desktop_account::AccountKey;
use kubuno_desktop_secrets::Secret;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{ConnectOptions, Row, SqlitePool};

use crate::error::SyncError;

/// The engine schema, one entry per version (applied in order, each in its own transaction).
const ENGINE_MIGRATIONS: &[&str] = &[
    // 1: the tables of §6.2.
    r#"
    CREATE TABLE IF NOT EXISTS _sync_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
    CREATE TABLE _sync_feeds (
        feed          TEXT PRIMARY KEY,
        cursor        TEXT NOT NULL DEFAULT '0',
        needs_full    INTEGER NOT NULL DEFAULT 1,
        last_pull_at  INTEGER,
        last_ok_at    INTEGER,
        last_error    TEXT
    );
    CREATE TABLE _sync_outbox (
        seq             INTEGER PRIMARY KEY AUTOINCREMENT,
        op_id           TEXT NOT NULL UNIQUE,
        idem_key        TEXT NOT NULL,
        feed            TEXT NOT NULL,
        entity          TEXT NOT NULL,
        entity_id       TEXT NOT NULL,
        op              TEXT NOT NULL,
        payload         TEXT NOT NULL,
        base_etag       TEXT,
        base_row        TEXT,
        depends_on      INTEGER REFERENCES _sync_outbox(seq) ON DELETE SET NULL,
        state           TEXT NOT NULL DEFAULT 'pending',
        attempts        INTEGER NOT NULL DEFAULT 0,
        next_attempt_at INTEGER,
        last_error      TEXT,
        created_at      INTEGER NOT NULL
    );
    CREATE INDEX _sync_outbox_entity ON _sync_outbox(entity, entity_id, state);
    CREATE TABLE _sync_shadow (
        entity TEXT NOT NULL, entity_id TEXT NOT NULL, server_row TEXT NOT NULL, etag TEXT, change_seq INTEGER,
        deleted INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (entity, entity_id));
    CREATE TABLE _sync_id_map (
        entity TEXT NOT NULL, local_id TEXT NOT NULL, server_id TEXT NOT NULL, PRIMARY KEY (entity, local_id));
    CREATE TABLE _sync_conflicts (
        id INTEGER PRIMARY KEY, entity TEXT NOT NULL, entity_id TEXT NOT NULL, op_id TEXT, kind TEXT NOT NULL,
        local TEXT, server TEXT, base TEXT, fields TEXT, message TEXT,
        created_at INTEGER NOT NULL, resolved_at INTEGER, resolution TEXT);
    CREATE INDEX _sync_conflicts_open ON _sync_conflicts(resolved_at);
    CREATE TABLE _sync_seen (feed TEXT NOT NULL, entity_id TEXT NOT NULL, PRIMARY KEY (feed, entity_id));
    CREATE TABLE _sync_log (id INTEGER PRIMARY KEY, at INTEGER NOT NULL, level TEXT NOT NULL, feed TEXT,
        event TEXT NOT NULL, detail TEXT);
    "#,
];

/// The engine schema version this build writes.
pub const ENGINE_SCHEMA_VERSION: usize = ENGINE_MIGRATIONS.len();

/// The app's schema: its migrations and the feeds a migration must re-pull (a migration that adds a column to a
/// synced entity resets that feed so existing rows get the new field, §6.1).
#[derive(Clone, Default)]
pub struct AppSchema {
    pub migrator: Option<&'static sqlx::migrate::Migrator>,
    /// `(migration version, feeds to reset when that version is applied)`.
    pub resync_on: Vec<(i64, Vec<String>)>,
}

impl std::fmt::Debug for AppSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppSchema").field("migrations", &self.migrator.map(|m| m.iter().count())).field("resync_on", &self.resync_on).finish()
    }
}

/// How to open a database.
#[derive(Debug, Clone)]
pub struct OpenOptions {
    pub path: PathBuf,
    pub account: AccountKey,
    /// The SQLCipher key (64 hex characters, from the OS store). `None` = clear database (tests, tools).
    pub key: Option<Secret>,
    pub schema: AppSchema,
    /// Pool size (readers in WAL mode; one writer at a time anyway).
    pub max_connections: u32,
}

impl OpenOptions {
    pub fn new(path: impl Into<PathBuf>, account: AccountKey) -> Self {
        Self { path: path.into(), account, key: None, schema: AppSchema::default(), max_connections: 4 }
    }

    pub fn key(mut self, key: Secret) -> Self {
        self.key = Some(key);
        self
    }

    pub fn schema(mut self, schema: AppSchema) -> Self {
        self.schema = schema;
        self
    }
}

/// An open local database.
#[derive(Debug, Clone)]
pub struct LocalDb {
    pool: SqlitePool,
    path: PathBuf,
    account: AccountKey,
    encrypted: bool,
    degraded: Option<String>,
}

fn connect_options(path: &Path, key: Option<&Secret>) -> Result<SqliteConnectOptions, SyncError> {
    let mut opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10))
        .pragma("cache_size", "-32768")
        .disable_statement_logging();
    if let Some(k) = key {
        let hex = k.expose_str().map_err(|_| SyncError::Config("the database key is not text".into()))?;
        if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(SyncError::Config("the database key must be 64 hex characters".into()));
        }
        // Raw key syntax: no key derivation (the key is already 256 random bits).
        opts = opts.pragma("key", format!("\"x'{hex}'\""));
    }
    Ok(opts)
}

impl LocalDb {
    /// Opens (creating if needed) and migrates the database. See the module doc.
    pub async fn open(o: OpenOptions) -> Result<Self, SyncError> {
        if let Some(dir) = o.path.parent() {
            crate::paths::create_private_dir(dir).map_err(|e| SyncError::Io(format!("cannot create {}: {e}", dir.display())))?;
        }
        let opts = connect_options(&o.path, o.key.as_ref())?;
        let pool = SqlitePoolOptions::new()
            .max_connections(o.max_connections.max(1))
            .connect_with(opts)
            .await
            .map_err(|e| classify_open_error(&o.path, e))?;
        // The first real read: a wrong key or a clear/encrypted mismatch shows up here.
        sqlx::query("SELECT count(*) FROM sqlite_master").fetch_one(&pool).await.map_err(|e| classify_open_error(&o.path, e))?;
        let encrypted = o.key.is_some();
        if encrypted {
            let cipher: Option<String> = sqlx::query_scalar("PRAGMA cipher_version").fetch_optional(&pool).await.map_err(SyncError::db("cipher_version"))?;
            if cipher.is_none() {
                pool.close().await;
                return Err(SyncError::EncryptionUnavailable);
            }
        }
        let db = Self { pool, path: o.path.clone(), account: o.account.clone(), encrypted, degraded: None };
        db.migrate_engine().await?;
        db.check_account().await?;
        // Crash recovery: an op left in flight was maybe sent; it is retried with the same idempotency key.
        sqlx::query("UPDATE _sync_outbox SET state = 'pending' WHERE state = 'inflight'")
            .execute(&db.pool)
            .await
            .map_err(SyncError::db("recover inflight"))?;
        let mut db = db;
        if let Some(m) = o.schema.migrator {
            if let Err(e) = db.migrate_app(m, &o.schema.resync_on).await {
                tracing::error!(db = %db.path.display(), error = %e, "app migration failed: database opened read-only");
                db.degraded = Some(e.to_string());
            }
        }
        Ok(db)
    }

    async fn migrate_engine(&self) -> Result<(), SyncError> {
        sqlx::query("CREATE TABLE IF NOT EXISTS _sync_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .execute(&self.pool)
            .await
            .map_err(SyncError::db("create _sync_meta"))?;
        let current: Option<String> = sqlx::query_scalar("SELECT value FROM _sync_meta WHERE key = 'engine_schema'")
            .fetch_optional(&self.pool)
            .await
            .map_err(SyncError::db("read engine_schema"))?;
        let current: usize = current.and_then(|v| v.parse().ok()).unwrap_or(0);
        if current > ENGINE_SCHEMA_VERSION {
            return Err(SyncError::Config(format!(
                "the database was written by a newer engine (schema {current} > {ENGINE_SCHEMA_VERSION}); update the application"
            )));
        }
        for (i, sql) in ENGINE_MIGRATIONS.iter().enumerate().skip(current) {
            let version = i + 1;
            let mut tx = self.pool.begin().await.map_err(SyncError::db("begin engine migration"))?;
            sqlx::raw_sql(sql).execute(&mut *tx).await.map_err(SyncError::db("engine migration"))?;
            sqlx::query("INSERT INTO _sync_meta(key, value) VALUES ('engine_schema', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
                .bind(version.to_string())
                .execute(&mut *tx)
                .await
                .map_err(SyncError::db("engine migration version"))?;
            tx.commit().await.map_err(SyncError::db("commit engine migration"))?;
            tracing::info!(db = %self.path.display(), version, "engine schema migrated");
        }
        Ok(())
    }

    async fn check_account(&self) -> Result<(), SyncError> {
        let stored: Option<String> = sqlx::query_scalar("SELECT value FROM _sync_meta WHERE key = 'account_key'")
            .fetch_optional(&self.pool)
            .await
            .map_err(SyncError::db("read account_key"))?;
        match stored {
            Some(s) if s == self.account.as_str() => Ok(()),
            Some(_) => Err(SyncError::AccountMismatch),
            None => {
                sqlx::query("INSERT INTO _sync_meta(key, value) VALUES ('account_key', ?), ('engine_version', ?)")
                    .bind(self.account.as_str())
                    .bind(env!("CARGO_PKG_VERSION"))
                    .execute(&self.pool)
                    .await
                    .map_err(SyncError::db("write account_key"))?;
                Ok(())
            }
        }
    }

    async fn applied_versions(&self) -> Result<Vec<i64>, SyncError> {
        let exists: Option<String> = sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'")
            .fetch_optional(&self.pool)
            .await
            .map_err(SyncError::db("read migrations"))?;
        if exists.is_none() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query("SELECT version FROM _sqlx_migrations WHERE success = 1")
            .fetch_all(&self.pool)
            .await
            .map_err(SyncError::db("read migrations"))?;
        Ok(rows.iter().map(|r| r.get::<i64, _>(0)).collect())
    }

    async fn migrate_app(&self, migrator: &'static sqlx::migrate::Migrator, resync_on: &[(i64, Vec<String>)]) -> Result<(), SyncError> {
        let before = self.applied_versions().await?;
        migrator.run(&self.pool).await.map_err(|e| SyncError::Migration(e.to_string()))?;
        let after = self.applied_versions().await?;
        let fresh_db = before.is_empty();
        for (version, feeds) in resync_on {
            if after.contains(version) && !before.contains(version) && !fresh_db {
                for feed in feeds {
                    tracing::info!(feed = %feed, version, "migration adds data to a synced entity: the feed is re-pulled");
                    sqlx::query(
                        "INSERT INTO _sync_feeds(feed, cursor, needs_full) VALUES (?, '0', 1)
                         ON CONFLICT(feed) DO UPDATE SET cursor = '0', needs_full = 1",
                    )
                    .bind(feed)
                    .execute(&self.pool)
                    .await
                    .map_err(SyncError::db("reset feed"))?;
                }
            }
        }
        Ok(())
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn account(&self) -> &AccountKey {
        &self.account
    }

    pub fn is_encrypted(&self) -> bool {
        self.encrypted
    }

    /// The app migration error, when the database was opened read-only because of it.
    pub fn degraded(&self) -> Option<&str> {
        self.degraded.as_deref()
    }

    /// `PRAGMA integrity_check` (tests, diagnostics).
    pub async fn integrity_check(&self) -> Result<bool, SyncError> {
        let r: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(&self.pool).await.map_err(SyncError::db("integrity_check"))?;
        Ok(r == "ok")
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// Deletes the database files (`.db`, `-wal`, `-shm`, `.sync.lock`) after closing the pool: the sign-out wipe.
    pub async fn wipe(self) -> Result<(), SyncError> {
        self.pool.close().await;
        for suffix in ["", "-wal", "-shm", ".sync.lock"] {
            let mut p = self.path.clone().into_os_string();
            p.push(suffix);
            match std::fs::remove_file(PathBuf::from(p)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(SyncError::Io(format!("cannot delete the database: {e}"))),
            }
        }
        Ok(())
    }
}

fn classify_open_error(path: &Path, e: sqlx::Error) -> SyncError {
    let msg = e.to_string();
    // SQLITE_NOTADB (26): wrong key, or a clear database opened with a key / the reverse.
    if msg.contains("file is not a database") || msg.contains("(code: 26)") {
        return SyncError::WrongKey;
    }
    tracing::error!(db = %path.display(), error = %msg, "cannot open the local database");
    SyncError::Db(format!("open {}: {msg}", path.display()))
}

/// Parses an `<app>.db` URL-free path (helper for tools).
pub fn options_from_path(path: &str) -> Result<SqliteConnectOptions, SyncError> {
    SqliteConnectOptions::from_str(path).map_err(|e| SyncError::Config(e.to_string()))
}

/// Single writer per database (§5.2): an exclusive OS lock on `<db>.sync.lock` elects the process that drains the
/// outbox and pulls. Another process opening the same database only reads and writes local rows and outbox entries
/// (WAL makes that safe) and asks the owner to sync. Released when dropped (or when the process dies).
#[derive(Debug)]
pub struct SyncLock {
    _file: std::fs::File,
    path: PathBuf,
}

impl SyncLock {
    /// `Ok(None)` when another process holds it.
    pub fn try_acquire(db_path: &Path) -> Result<Option<Self>, SyncError> {
        let mut p = db_path.as_os_str().to_owned();
        p.push(".sync.lock");
        let path = PathBuf::from(p);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| SyncError::Io(format!("cannot open {}: {e}", path.display())))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file, path })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(e)) => Err(SyncError::Io(format!("cannot lock {}: {e}", path.display()))),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
