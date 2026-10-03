//! DATA-1 integration tests on PostgreSQL. They run only when `KUBUNO_TEST_PG_URL` names a
//! disposable database the tests may create a schema in (e.g.
//! `postgres://kubuno_test@localhost/kubuno_test`); without it they pass without doing anything.
//! Never point it at a shared or production database: each test creates, then drops, its own
//! schema `kubuno_data_test_<pid>_<n>`.

use kubuno_data::*;
use kubuno_views::binding::Value;

fn url() -> Option<String> {
    std::env::var("KUBUNO_TEST_PG_URL").ok().filter(|u| !u.trim().is_empty())
}

/// A context on a fresh schema, and that schema's name.
fn setup(tag: &str) -> Option<(DataContext, String)> {
    let url = url()?;
    let schema = format!("kubuno_data_test_{}_{tag}", std::process::id());
    // The URL comes from the environment: resolve it like an application would.
    std::env::set_var("ConnectionStrings__KubunoDataTest", &url);
    let view = format!(
        r#"<Panel>
             <DbConnection x:Name="db" Provider="Postgres" ConnectionStringName="KubunoDataTest" Schema="{schema}"/>
             <TableAdapter x:Name="a" Connection="db" SelectCommand="SELECT id, name, email, born, score FROM customers ORDER BY id" UpdateTable="customers"/>
             <BindingSource x:Name="customers" DataSource="a"/>
           </Panel>"#
    );
    let mut ctx = DataContext::from_view(&view).expect("view");
    let db = ctx.connection_handle("db").expect("handle");
    for sql in [
        format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"),
        format!("CREATE SCHEMA \"{schema}\""),
        "CREATE TABLE customers (id serial PRIMARY KEY, name varchar(50) NOT NULL, email text UNIQUE, born date, score float8)".to_string(),
        "INSERT INTO customers (name, email, born, score) VALUES ('Ada', 'ada@example.org', '1815-12-10', 9.5), ('Linus', 'linus@example.org', NULL, NULL)".to_string(),
    ] {
        block_on(db.execute(&sql, &[])).expect("rt").expect("setup");
    }
    Some((ctx, schema))
}

fn teardown(ctx: &mut DataContext, schema: &str) {
    if let Ok(db) = ctx.connection_handle("db") {
        let _ = block_on(db.execute(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"), &[]));
    }
}

#[test]
fn postgres_fill_edit_and_save() {
    let Some((mut ctx, schema)) = setup("crud") else {
        eprintln!("KUBUNO_TEST_PG_URL is not set: PostgreSQL tests skipped");
        return;
    };
    assert_eq!(ctx.get("db.State"), Some(Value::Str("Open".into())), "setup opened the pool");
    assert_eq!(fill_blocking(&mut ctx, "customers").expect("fill"), 2);
    let t = ctx.binding_source("customers").expect("bs").table().clone();
    assert!(t.columns[0].primary_key && t.columns[0].auto_increment, "serial");
    assert_eq!(t.columns[1].max_length, Some(50));
    assert_eq!(t.columns[3].ty.kind, DbKind::Date);
    assert_eq!(ctx.get("customers.born"), Some(Value::Str("1815-12-10".into())));
    ctx.set("customers.born", Value::Str("1815-12-11".into()));
    ctx.set("customers.score", Value::Str("9,75".into()));
    ctx.set("customers.Position", Value::F32(1.0));
    {
        let mut bs = ctx.binding_source_mut("customers").expect("bs");
        bs.add_new().expect("add");
        bs.set_path("name", &Value::Str("Grace".into())).expect("edit");
    }
    assert_eq!(save_blocking(&mut ctx, "customers").expect("save"), 2);
    assert_eq!(ctx.get("customers.id"), Some(Value::Str("3".into())));
    let db = ctx.connection_handle("db").expect("handle");
    let mut check = DbCommand::with_text("SELECT born::text || ' ' || score::text FROM customers WHERE id = @id");
    check.param("id", 1);
    assert_eq!(block_on(check.execute_scalar(&db)).expect("rt").expect("scalar"), DbValue::Text("1815-12-11 9.75".into()));
    // A unique violation rolls the whole save back.
    ctx.set("customers.email", Value::Str("ada@example.org".into()));
    assert!(matches!(save_blocking(&mut ctx, "customers"), Err(DataError::Database { code: Some(c), .. }) if c == "23505"));
    teardown(&mut ctx, &schema);
}

#[test]
fn postgres_row_versions_master_detail_and_paging() {
    let Some((mut ctx, schema)) = setup("depth") else {
        eprintln!("KUBUNO_TEST_PG_URL is not set: PostgreSQL tests skipped");
        return;
    };
    let db = ctx.connection_handle("db").expect("handle");
    for sql in [
        "CREATE TABLE orders (id serial PRIMARY KEY, customer_id int4 NOT NULL REFERENCES customers(id), item text NOT NULL)",
        "INSERT INTO orders (customer_id, item) VALUES (1, 'Tea'), (2, 'Coffee')",
    ] {
        block_on(db.execute(sql, &[])).expect("rt").expect("setup");
    }
    // `xmin` as the row version: an update checks it and reads the new one back.
    let mut adapter = TableAdapter::new("db", "SELECT id, name, xmin::text AS xmin FROM customers ORDER BY id").with_update_table("customers");
    adapter.conflict_option = ConflictOption::CompareRowVersion;
    adapter.row_version_column = "xmin".into();
    let mut t = block_on(adapter.fill(&db)).expect("rt").expect("fill");
    let id = t.rows()[0].id;
    let first_version = t.rows()[0].values[2].clone();
    {
        let r = t.row_by_id_mut(id).expect("row");
        r.values[1] = "Ada L.".into();
        r.state = RowState::Modified;
    }
    let plan = adapter.plan_update(Provider::Postgres, &t).expect("plan");
    let outcome = block_on(adapter.update(&db, &plan)).expect("rt").expect("update");
    t.apply_update(&plan, &outcome);
    assert_ne!(t.rows()[0].values[2], first_version, "the new version was read back");
    // A second edit with the new version passes; a stale version is a concurrency violation.
    {
        let r = t.row_by_id_mut(id).expect("row");
        r.values[1] = "Ada Lovelace".into();
        r.state = RowState::Modified;
    }
    let mut stale = t.clone();
    let plan = adapter.plan_update(Provider::Postgres, &t).expect("plan");
    block_on(adapter.update(&db, &plan)).expect("rt").expect("update with the current version");
    let plan = adapter.plan_update(Provider::Postgres, &stale).expect("plan");
    assert!(matches!(block_on(adapter.update(&db, &plan)).expect("rt"), Err(DataError::Concurrency(_))));
    stale.reject_changes();
    // A new customer and its order in one transaction.
    let mut orders = TableAdapter::new("db", "SELECT id, customer_id, item FROM orders ORDER BY id").with_update_table("orders");
    orders.command_timeout = 30;
    let mut ot = block_on(orders.fill(&db)).expect("rt").expect("fill");
    let mut ct = block_on(TableAdapter::new("db", "SELECT id, name, email, born, score FROM customers ORDER BY id").with_update_table("customers").fill(&db)).expect("rt").expect("fill");
    ct.add_row(vec![DbValue::Int(-900), "Grace".into(), DbValue::Null, DbValue::Null, DbValue::Null]);
    ot.add_row(vec![DbValue::Null, DbValue::Int(-900), "Compiler".into()]);
    let cplan = TableAdapter::new("db", "x").with_update_table("customers").plan_update(Provider::Postgres, &ct).expect("plan");
    let oplan = orders.plan_update_with(Provider::Postgres, &ot, Some(1)).expect("plan");
    let tx = block_on(DbTransaction::begin(&db)).expect("rt").expect("begin");
    let c_out = block_on(tx.update(&cplan)).expect("rt").expect("customers");
    block_on(tx.update(&oplan)).expect("rt").expect("orders");
    block_on(tx.commit()).expect("rt").expect("commit");
    let new_id = c_out.returned.iter().flatten().next().and_then(|r| r.first().cloned()).expect("generated key");
    let mut check = DbCommand::with_text("SELECT customer_id FROM orders WHERE item = 'Compiler'");
    assert_eq!(block_on(check.clear_params().execute_scalar(&db)).expect("rt").expect("scalar"), new_id);
    // Paging.
    let mut paged = TableAdapter::new("db", "SELECT id, item FROM orders ORDER BY id");
    paged.page_size = 2;
    let page = block_on(paged.fill_page(&db, &FillRequest { page: 1, params: Vec::new() }, None, None)).expect("rt").expect("page");
    assert_eq!((page.table.rows().len(), page.total), (1, Some(3)));
    teardown(&mut ctx, &schema);
}
