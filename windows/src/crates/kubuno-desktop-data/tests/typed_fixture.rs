//! The database the offline cache fixture (`kubuno-desktop-data/.sqlx`, used by `tests/typed.rs` and the
//! trybuild tests) is prepared against. Kept apart from `tests/typed.rs`, which cannot compile
//! offline without that cache.

use kubuno_desktop_data::{block_on, sqlx};

/// Creates `shop.db` from `tests/typed/shop.sql` (in the temp folder, or `KUBUNO_TYPED_FIXTURE_DB`)
/// and prints the command that regenerates the cache:
/// `cargo test -p kubuno-desktop-data --test typed_fixture -- --ignored --nocapture`.
#[test]
#[ignore]
fn create_fixture_database() {
    let file = match std::env::var_os("KUBUNO_TYPED_FIXTURE_DB") {
        Some(p) => std::path::PathBuf::from(p),
        None => std::env::temp_dir().join("kubuno-data-typed-fixture").join("shop.db"),
    };
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).expect("dir");
    }
    let _ = std::fs::remove_file(&file);
    let url = format!("sqlite:{}?mode=rwc", file.display());
    block_on(async {
        use sqlx::Connection;
        let mut conn = sqlx::SqliteConnection::connect(&url).await.expect("create");
        sqlx::raw_sql(include_str!("typed/shop.sql")).execute(&mut conn).await.expect("schema");
        conn.close().await.expect("close");
    })
    .expect("runtime");
    let cache = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".sqlx");
    println!("DATABASE_URL=sqlite:{} SQLX_OFFLINE=false SQLX_OFFLINE_DIR={} cargo test -p kubuno-desktop-data --test typed --no-run", file.display(), cache.display());
}
