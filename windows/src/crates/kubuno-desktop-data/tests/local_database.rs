//! `<LocalDatabase>` and `Data Source=app:<name>` (vskubuno docs/STORAGE-COMPONENTS.md §3.5), for real on SQLite in
//! a sandboxed profile (one test in its own binary: the sandbox variable is process-wide).

use kubuno_desktop_data::{block_on, DataContext, DbCommand, DbValue};

#[test]
fn local_databases_live_in_the_apps_data_folder() {
    let dir = std::env::temp_dir().join(format!("kubuno-localdb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("sandbox");
    std::env::set_var("KUBUNO_SANDBOX_DIR", &dir);
    kubuno_desktop_app_storage::set_default_app_id(kubuno_desktop_app_storage::AppId::new("localdb-test").expect("id"));

    let view = r#"<Panel DesignWidth="400" DesignHeight="300">
        <LocalDatabase x:Name="db" DatabaseName="notes"/>
        <DbConnection x:Name="plain" Provider="Sqlite" ConnectionString="Data Source=app:other"/>
        <TableAdapter x:Name="adapter" Connection="db" SelectCommand="SELECT id, text FROM notes" UpdateTable="notes"/>
    </Panel>"#;
    let diagnostics = kubuno_desktop_views::validate::validate_with_default_registry(&kubuno_desktop_views::syntax::parse(view));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let mut ctx = DataContext::from_view(view).expect("view");
    let db = ctx.connection_handle("db").expect("a LocalDatabase is a DbConnection");
    block_on(db.execute("CREATE TABLE notes (id INTEGER PRIMARY KEY, text TEXT)", &[])).expect("rt").expect("create");
    block_on(db.execute("INSERT INTO notes (text) VALUES (@t)", &[("t", "hello".into())])).expect("rt").expect("insert");
    let n = block_on(DbCommand::with_text("SELECT COUNT(*) FROM notes").execute_scalar(&db)).expect("rt").expect("count");
    assert!(matches!(n, DbValue::Int(1)), "{n:?}");
    let file = dir.join("data").join("localdb-test").join("databases").join("notes.db");
    assert!(file.is_file(), "{file:?}");

    let plain = ctx.connection_handle("plain").expect("handle");
    block_on(plain.execute("CREATE TABLE t (x INTEGER)", &[])).expect("rt").expect("create");
    assert!(dir.join("data").join("localdb-test").join("databases").join("other.db").is_file());

    let mut named = kubuno_desktop_data::LocalDatabase::named("cache");
    assert_eq!(named.spec(), "app:cache");
    assert_eq!(named.connection().connection_string, "Data Source=app:cache");
    drop(ctx);
    let _ = std::fs::remove_dir_all(&dir);
}
