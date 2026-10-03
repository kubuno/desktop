//! Encryption at rest (SE-0): a keyed database is unreadable without its key, sqlx and the engine work on it, and
//! a build without SQLCipher refuses a key instead of writing a clear file.

mod fake_server;
mod support;

use kubuno_desktop_account::AccountKey;
use kubuno_desktop_secrets::Secret;
use kubuno_desktop_sync_engine::{AppSchema, LocalDb, OpenOptions, SyncError};
#[cfg(feature = "sqlcipher")]
use serde_json::json;
#[cfg(feature = "sqlcipher")]
use support::{fields, rows, Setup};
use support::MIGRATOR;

fn opts(path: &std::path::Path, key: Option<Secret>) -> OpenOptions {
    let mut o = OpenOptions::new(path, AccountKey::parse("0123456789abcdef").expect("key")).schema(AppSchema { migrator: Some(&MIGRATOR), resync_on: vec![] });
    if let Some(k) = key {
        o = o.key(k);
    }
    o
}

#[cfg(feature = "sqlcipher")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn encrypted_database_round_trip() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("enc.db");
    let key = Secret::random_key_hex().expect("key");
    {
        let db = LocalDb::open(opts(&path, Some(key.clone()))).await.expect("open");
        assert!(db.is_encrypted());
        sqlx::query("INSERT INTO notes (id, title) VALUES ('a', 'secret title')").execute(db.pool()).await.expect("insert");
        db.close().await;
    }
    let bytes = std::fs::read(&path).expect("read");
    assert!(!bytes.starts_with(b"SQLite format 3"), "the header must be encrypted");
    assert!(!bytes.windows(12).any(|w| w == b"secret title"), "no clear text on disk");

    let wrong = Secret::random_key_hex().expect("key");
    assert!(matches!(LocalDb::open(opts(&path, Some(wrong))).await, Err(SyncError::WrongKey)));
    assert!(matches!(LocalDb::open(opts(&path, None)).await, Err(SyncError::WrongKey)));

    let db = LocalDb::open(opts(&path, Some(key))).await.expect("reopen");
    let t: String = sqlx::query_scalar("SELECT title FROM notes WHERE id = 'a'").fetch_one(db.pool()).await.expect("read");
    assert_eq!(t, "secret title");
    assert!(db.integrity_check().await.expect("check"));
}

#[cfg(feature = "sqlcipher")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_engine_syncs_an_encrypted_database() {
    let (base, fake) = fake_server::start().await;
    fake.server_create("user-1", "a", "A", "x");
    let dir = tempfile::tempdir().expect("tmp");
    let mut s = Setup::new(&base, "user-1");
    s.key = Some(Secret::random_key_hex().expect("key"));
    let engine = s.open(dir.path()).await;
    engine.sync_once().await.expect("pull");
    let mut tx = engine.begin_local().await.expect("tx");
    tx.create("note", "b", fields(json!({"title": "B"}))).await.expect("create");
    tx.commit().await.expect("commit");
    engine.sync_once().await.expect("push");
    assert_eq!(rows(&engine).await.len(), 2);
    assert!(fake.get("user-1", "b").is_some());
}

#[cfg(not(feature = "sqlcipher"))]
#[tokio::test]
async fn a_key_without_sqlcipher_is_refused() {
    let dir = tempfile::tempdir().expect("tmp");
    let r = LocalDb::open(opts(&dir.path().join("x.db"), Some(Secret::random_key_hex().expect("key")))).await;
    assert!(matches!(r, Err(SyncError::EncryptionUnavailable)), "{r:?}");
}
