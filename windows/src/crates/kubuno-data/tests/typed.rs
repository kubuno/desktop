//! Typed data sources (DATA-4): the code `data_source!` generates from `tests/typed/shop.kbdata`,
//! compiled offline against the committed cache `kubuno-data/.sqlx`, run against a temporary SQLite
//! file created from `tests/typed/shop.sql`.
//!
//! **Regenerating the fixture cache** (after a change of `shop.kbdata` or of the macro's SQL), from
//! `desktop/windows`: `cargo test -p kubuno-data --test typed_fixture -- --ignored --nocapture`
//! creates the database from `shop.sql` and prints the command that rebuilds this test online:
//!
//! ```text
//! DATABASE_URL=sqlite:<abs>/shop.db SQLX_OFFLINE=false SQLX_OFFLINE_DIR=<abs>/src/crates/kubuno-data/.sqlx \
//!     cargo test -p kubuno-data --test typed --no-run
//! ```
//!
//! (`typed_fixture_cache_is_complete` names the statements that have no cache file.)

kubuno_data::data_source!("typed/shop.kbdata");

use kubuno_data::sqlx;
use kubuno_data::{block_on, ConnectionHandle, DbConnection, Provider, RowState, TableAdapter, TypedRow};
use kubuno_views::binding::Value;

struct TempDb {
    dir: std::path::PathBuf,
    file: std::path::PathBuf,
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A SQLite file with the fixture schema.
fn temp_db(name: &str) -> TempDb {
    let dir = std::env::temp_dir().join(format!("kubuno-data-typed-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("shop.db");
    let url = format!("sqlite:{}?mode=rwc", file.display());
    block_on(async {
        use sqlx::Connection;
        let mut conn = sqlx::SqliteConnection::connect(&url).await.expect("create the database");
        sqlx::raw_sql(include_str!("typed/shop.sql")).execute(&mut conn).await.expect("schema");
        conn.close().await.expect("close");
    })
    .expect("runtime");
    TempDb { dir, file }
}

fn handle(db: &TempDb) -> (DbConnection, ConnectionHandle) {
    let mut conn = DbConnection::new(Provider::Sqlite).with_connection_string(format!("Data Source={}", db.file.display()));
    let handle = conn.handle().expect("handle");
    (conn, handle)
}

fn ada() -> Customer {
    Customer { id: 0, name: "Ada".into(), email: Some("ada@example.org".into()), age: Some(36), vip: false, birth_date: chrono_date(1815, 12, 10), balance: Some(12.5) }
}

fn chrono_date(y: i32, m: u32, d: u32) -> Option<sqlx::types::chrono::NaiveDate> {
    sqlx::types::chrono::NaiveDate::from_ymd_opt(y, m, d)
}

#[test]
fn generated_items_describe_the_kbdata() {
    assert_eq!(Customer::TABLE, "customers");
    assert_eq!(Customer::COLUMNS, ["id", "name", "email", "age", "vip", "birth_date", "balance"]);
    assert_eq!(Customer::KEY, ["id"]);
    assert_eq!(VipCustomer::KEY, [] as [&str; 0]);
    assert_eq!(<Customer as TypedRow>::table_name(), "customers");
    let cols = Customer::columns();
    assert!(cols[0].primary_key && cols[0].auto_increment && !cols[0].nullable);
    assert_eq!(cols[1].max_length, Some(80));
    assert_eq!(cols[5].ty.kind, kubuno_data::DbKind::Date);
    // Default and PartialEq are derived (every field type has them).
    assert_eq!(Customer::default(), Customer::default());
    let back = Customer::from_values(&ada().to_values()).expect("round trip");
    assert_eq!(back, ada());
    assert_eq!(OrderTotalsRow::TABLE, "order_totals");
}

#[test]
fn typed_functions_run_against_sqlite() {
    let db = temp_db("crud");
    let (_conn, conn) = handle(&db);
    let pool = block_on(conn.sqlite_pool()).expect("rt").expect("pool");
    block_on(async {
        // insert → the stored row with its generated key.
        let a = ada().insert(&pool).await.expect("insert ada");
        assert!(a.id > 0);
        assert_eq!(Customer { id: a.id, ..ada() }, a);
        let l = Customer { name: "Linus".into(), email: None, age: Some(28), birth_date: None, balance: None, vip: true, ..ada() }.insert(&pool).await.expect("insert linus");
        let g = Customer { name: "Grace".into(), email: Some("grace@example.org".into()), age: Some(45), ..ada() }.insert(&pool).await.expect("insert grace");

        // fetch_all (ordered by the key), fetch_by_key.
        let all = Customer::fetch_all(&pool).await.expect("fetch all");
        assert_eq!(all.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Ada", "Linus", "Grace"]);
        assert_eq!(all[1].email, None);
        assert_eq!(Customer::fetch_by_key(&pool, l.id).await.expect("by key"), Some(l.clone()));
        assert_eq!(Customer::fetch_by_key(&pool, 999).await.expect("by key"), None);

        // update, the view, named queries.
        let mut a2 = a.clone();
        a2.name = "Ada Lovelace".into();
        a2.vip = true;
        assert_eq!(a2.update(&pool).await.expect("update"), 1);
        assert_eq!(Customer::fetch_by_key(&pool, a.id).await.expect("by key").map(|c| c.name), Some("Ada Lovelace".to_string()));
        let vips = VipCustomer::fetch_all(&pool).await.expect("view");
        assert_eq!(vips.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["Ada Lovelace", "Linus"]);
        let older = customers_older_than(&pool, 30).await.expect("query");
        assert_eq!(older.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Ada Lovelace", "Grace"]);
        assert_eq!(set_vip(&pool, true, g.id).await.expect("execute"), 1);
        assert_eq!(VipCustomer::fetch_all(&pool).await.expect("view").len(), 3);

        // Orders and a query with a row type of its own.
        for (c, label, amount) in [(a.id, "Engine", 100.0), (a.id, "Notes", 50.5), (g.id, "Compiler", 1234.5)] {
            Order { id: 0, customer_id: c, label: label.into(), amount }.insert(&pool).await.expect("order");
        }
        let totals = order_totals(&pool).await.expect("totals");
        assert_eq!(totals, vec![OrderTotalsRow { customer_id: a.id, total: 150.5, orders: 2 }, OrderTotalsRow { customer_id: g.id, total: 1234.5, orders: 1 }]);

        // delete (and in a transaction: the executor may be a transaction).
        let mut tx = pool.begin().await.expect("begin");
        assert_eq!(l.delete(&mut *tx).await.expect("delete"), 1);
        tx.rollback().await.expect("rollback");
        assert!(Customer::fetch_by_key(&pool, l.id).await.expect("by key").is_some(), "rolled back");
        assert_eq!(Customer::delete_by_key(&pool, l.id).await.expect("delete"), 1);
        assert_eq!(Customer::delete_by_key(&pool, l.id).await.expect("delete"), 0);

        // A database error is a DataError (logged), never a panic.
        let dup = Order { id: 0, customer_id: 4242, label: "x".into(), amount: 1.0 };
        assert!(dup.insert(&pool).await.is_err(), "the foreign key refuses an unknown customer");
    })
    .expect("runtime");
}

#[test]
fn tasks_run_on_the_data_runtime_from_a_connection_handle() {
    let db = temp_db("tasks");
    let (_conn, conn) = handle(&db);
    // Awaited from a thread that is not a Tokio worker, like the UI thread's executor.
    let inserted = std::thread::spawn({
        let conn = conn.clone();
        move || block_on(ada().insert_task(&conn))
    })
    .join()
    .expect("thread")
    .expect("rt")
    .expect("insert task");
    assert_eq!(block_on(Customer::fetch_all_task(&conn)).expect("rt").expect("fetch"), vec![inserted.clone()]);
    assert_eq!(block_on(Customer::fetch_by_key_task(&conn, inserted.id)).expect("rt").expect("by key").map(|c| c.id), Some(inserted.id));
    let mut changed = inserted.clone();
    changed.age = None;
    assert_eq!(block_on(changed.update_task(&conn)).expect("rt").expect("update"), 1);
    assert_eq!(block_on(customers_older_than_task(&conn, 0)).expect("rt").expect("query"), vec![], "age is NULL now");
    assert_eq!(block_on(set_vip_task(&conn, true, inserted.id)).expect("rt").expect("exec"), 1);
    assert_eq!(block_on(changed.delete_task(&conn)).expect("rt").expect("delete"), 1);
    // A PostgreSQL pool from a SQLite connection is a configuration error, not a panic.
    #[cfg(feature = "postgres")]
    assert!(matches!(block_on(conn.pg_pool()).expect("rt"), Err(kubuno_data::DataError::Config(_))));
}

#[test]
fn binding_source_round_trip_through_typed_rows() {
    let db = temp_db("binding");
    let (_conn, conn) = handle(&db);
    let pool = block_on(conn.sqlite_pool()).expect("rt").expect("pool");
    let rows = block_on(async {
        ada().insert(&pool).await.expect("ada");
        Customer { name: "Linus".into(), ..ada() }.insert(&pool).await.expect("linus");
        Customer::fetch_all(&pool).await.expect("fetch")
    })
    .expect("rt");

    // Typed rows feed the same BindingSource the designer's grids and bindings use.
    let mut bs = kubuno_data::BindingSource::new();
    bs.load_typed(&rows);
    assert_eq!(bs.count(), 2);
    assert_eq!(bs.get_path("name"), Some(Value::Str("Ada".into())));
    assert_eq!(bs.get_path("birth_date"), Some(Value::Str("1815-12-10".into())));
    bs.set_path("age", &Value::Str("37".into())).expect("edit");
    bs.set_path("vip", &Value::Bool(true)).expect("edit");
    bs.move_next().expect("move commits the edit");
    bs.remove_current().expect("delete Linus");
    bs.add_typed(&Customer { name: "Grace".into(), email: None, ..ada() }).expect("add typed");
    bs.add_new().expect("add through the binding source");
    bs.set_path("name", &Value::Str("Barbara".into())).expect("edit");
    bs.end_edit().expect("end edit");
    assert_eq!(bs.current_typed::<Customer>().expect("typed").map(|c| c.name), Some("Barbara".into()));

    // Save the typed way: the changes, typed, through the generated functions, in one transaction.
    let changes = bs.typed_changes::<Customer>().expect("typed changes");
    assert_eq!(changes.iter().map(|(s, _)| *s).collect::<Vec<_>>(), [RowState::Modified, RowState::Deleted, RowState::Added, RowState::Added]);
    assert_eq!(changes[0].1.age, Some(37));
    assert!(changes[0].1.vip);
    block_on(async {
        let mut tx = pool.begin().await.expect("begin");
        for (state, row) in &changes {
            match state {
                RowState::Added => {
                    row.insert(&mut *tx).await.expect("insert");
                }
                RowState::Modified => assert_eq!(row.update(&mut *tx).await.expect("update"), 1),
                RowState::Deleted => assert_eq!(row.delete(&mut *tx).await.expect("delete"), 1),
                _ => {}
            }
        }
        tx.commit().await.expect("commit");
    })
    .expect("rt");
    bs.table_mut().accept_changes();
    assert!(!bs.has_changes());
    let stored = block_on(Customer::fetch_all(&pool)).expect("rt").expect("fetch");
    assert_eq!(stored.iter().map(|c| (c.name.as_str(), c.age)).collect::<Vec<_>>(), [("Ada", Some(37)), ("Grace", Some(36)), ("Barbara", None)]);
    assert!(stored[0].vip);

    // …or the dynamic way: the typed rows' table saved by a TableAdapter (its columns carry the key).
    let mut bs = kubuno_data::BindingSource::new();
    bs.load_typed(&stored);
    bs.set_path("email", &Value::Str("ada@lovelace.org".into())).expect("edit");
    bs.end_edit().expect("end edit");
    let adapter = TableAdapter::new("db", "SELECT * FROM customers").with_update_table("customers");
    let plan = adapter.plan_update(Provider::Sqlite, bs.table()).expect("plan");
    let outcome = block_on(adapter.update(&conn, &plan)).expect("rt").expect("update");
    bs.table_mut().apply_update(&plan, &outcome);
    assert!(!bs.has_changes());
    let ada_now = block_on(Customer::fetch_by_key(&pool, stored[0].id)).expect("rt").expect("by key");
    assert_eq!(ada_now.and_then(|c| c.email), Some("ada@lovelace.org".into()));
}

/// Every statement of the fixture has its file in the committed cache (else the build above would
/// already have failed offline — this names the missing ones for whoever regenerates it).
#[test]
fn typed_fixture_cache_is_complete() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = kubuno_data_model::DataSource::parse(include_str!("typed/shop.kbdata")).expect("kbdata");
    let plan = kubuno_data_model::plan(&source).expect("plan");
    let missing = kubuno_data_model::cache::missing_queries(&plan, &[manifest.join(".sqlx")]);
    assert!(missing.is_empty(), "missing cache files: {missing:?}");
    assert_eq!(plan.statements().len(), 5 + 5 + 1 + 3);
}
