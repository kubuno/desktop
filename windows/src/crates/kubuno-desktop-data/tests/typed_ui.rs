//! Compile-pass / compile-fail tests of `data_source!` (DATA-4), through trybuild.
//!
//! trybuild builds the cases in its own package (`<target>/tests/trybuild/kubuno-desktop-data`), where sqlx
//! looks for the offline cache (`CARGO_MANIFEST_DIR/.sqlx`, then the workspace root's — sqlx 0.8
//! reads `SQLX_OFFLINE_DIR` only from a `.env` file, not from the environment): the committed
//! fixture cache `kubuno-desktop-data/.sqlx` is copied there first. The cases need `--cfg trybuild`, passed
//! through `CARGO_ENCODED_RUSTFLAGS`, which cargo ranks above any configured rustflags.
//!
//! Two phases, one after the other (they set process-wide environment variables): offline (the
//! pass case and the errors found without a database), then online against a temporary SQLite
//! database with the fixture schema (an unknown column, reported by SQLite itself).

use std::path::{Path, PathBuf};

use kubuno_desktop_data::{block_on, sqlx};

/// `cargo metadata`'s target directory (what trybuild uses).
fn target_dir() -> PathBuf {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = std::process::Command::new(cargo).args(["metadata", "--format-version=1", "--no-deps"]).current_dir(env!("CARGO_MANIFEST_DIR")).output().expect("cargo metadata");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("metadata json");
    PathBuf::from(json["target_directory"].as_str().expect("target_directory"))
}

/// Copies the fixture cache into trybuild's package.
fn install_cache() {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join(".sqlx");
    let to = target_dir().join("tests").join("trybuild").join(env!("CARGO_PKG_NAME")).join(".sqlx");
    let _ = std::fs::remove_dir_all(&to);
    std::fs::create_dir_all(&to).expect("cache dir");
    for entry in std::fs::read_dir(&from).expect("the fixture cache kubuno-desktop-data/.sqlx").flatten() {
        std::fs::copy(entry.path(), to.join(entry.file_name())).expect("copy a cache file");
    }
}

fn set_rustflags() {
    // Flags of the compiled cases only (the workspace configures no rustflags).
    std::env::set_var("CARGO_ENCODED_RUSTFLAGS", ["--cfg", "trybuild", "-A", "dead_code"].join("\u{1f}"));
}

#[test]
fn data_source_compile_tests() {
    set_rustflags();
    install_cache();

    // Offline: the committed cache only.
    std::env::remove_var("DATABASE_URL");
    std::env::remove_var("SQLX_OFFLINE_DIR");
    std::env::set_var("SQLX_OFFLINE", "true");
    {
        let t = trybuild::TestCases::new();
        t.pass("tests/ui/pass_shop.rs");
        t.compile_fail("tests/ui/fail_malformed.rs");
        t.compile_fail("tests/ui/fail_unknown_provider.rs");
        t.compile_fail("tests/ui/fail_undeclared_param.rs");
        t.compile_fail("tests/ui/fail_not_cached.rs");
        if !cfg!(feature = "mysql") {
            t.compile_fail("tests/ui/fail_provider_feature.rs");
        }
    }

    // Online: a SQLite database with the fixture schema reports the unknown column.
    let dir = std::env::temp_dir().join(format!("kubuno-data-typed-ui-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("shop.db");
    let url = format!("sqlite:{}", file.display().to_string().replace('\\', "/"));
    block_on(async {
        use sqlx::Connection;
        let mut conn = sqlx::SqliteConnection::connect(&format!("{url}?mode=rwc")).await.expect("create");
        sqlx::raw_sql(include_str!("typed/shop.sql")).execute(&mut conn).await.expect("schema");
        conn.close().await.expect("close");
    })
    .expect("runtime");
    std::env::set_var("DATABASE_URL", &url);
    std::env::set_var("SQLX_OFFLINE", "false");
    {
        let t = trybuild::TestCases::new();
        t.compile_fail("tests/ui/fail_unknown_column.rs");
    }
    std::env::remove_var("DATABASE_URL");
    let _ = std::fs::remove_dir_all(&dir);
}
