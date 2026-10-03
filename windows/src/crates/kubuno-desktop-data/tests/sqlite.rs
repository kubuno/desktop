//! Integration tests on SQLite (in memory and a temporary file): always run. DATA-1 (fill, edit,
//! save, errors), DATA-2 (typed bindings, XML handlers run synchronously through the scope) and
//! DATA-3 (master/detail, hierarchical save, transactions across adapters, paging, optimistic
//! concurrency, custom DML, cancellation and progress).

use kubuno_desktop_data::*;
use kubuno_desktop_views::binding::{parse_binding, MapViewModel, Value, ViewModel};
use kubuno_desktop_views::events::{ElementRef, EventArgs};
use kubuno_desktop_views::format::ValueKind;

const VIEW: &str = r#"
<Panel DesignWidth="600" DesignHeight="400">
  <DbConnection x:Name="db" Provider="Sqlite" ConnectionString="sqlite::memory:"/>
  <TableAdapter x:Name="customersAdapter" Connection="db" SelectCommand="SELECT id, name, email, age, vip FROM customers ORDER BY id" UpdateTable="customers"/>
  <BindingSource x:Name="customers" DataSource="customersAdapter" OnRowValidating="customers_validating"/>
  <ErrorProvider x:Name="errors" DataSource="customers"/>
</Panel>"#;

fn setup() -> DataContext {
    let mut ctx = DataContext::from_view(VIEW).expect("view");
    let db = ctx.connection_handle("db").expect("handle");
    let ddl = "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email VARCHAR(40) UNIQUE, age INTEGER, vip BOOLEAN NOT NULL DEFAULT 0)";
    block_on(db.execute(ddl, &[])).expect("rt").expect("create");
    for (name, email, age) in [("Ada", "ada@example.org", 36), ("Linus", "linus@example.org", 28), ("Grace", "grace@example.org", 45)] {
        block_on(db.execute("INSERT INTO customers (name, email, age) VALUES (@name, @email, @age)", &[("name", name.into()), ("email", email.into()), ("age", age.into())]))
            .expect("rt")
            .expect("insert");
    }
    ctx
}

fn int(v: &DbValue) -> Option<i64> {
    match v {
        DbValue::Int(i) => Some(*i),
        _ => None,
    }
}

fn db_rows(ctx: &mut DataContext) -> Vec<(i64, String, Option<String>, Option<i64>)> {
    let db = ctx.connection_handle("db").expect("handle");
    let t = block_on(DbCommand::with_text("SELECT id, name, email, age FROM customers ORDER BY id").query(&db)).expect("rt").expect("query");
    t.rows()
        .iter()
        .map(|r| {
            let email = match &r.values[2] {
                DbValue::Text(s) => Some(s.clone()),
                _ => None,
            };
            (int(&r.values[0]).unwrap_or(-1), r.values[1].to_display(), email, int(&r.values[3]))
        })
        .collect()
}

fn scalar(ctx: &mut DataContext, sql: &str) -> DbValue {
    let db = ctx.connection_handle("db").expect("handle");
    block_on(DbCommand::with_text(sql).execute_scalar(&db)).expect("rt").expect("scalar")
}

#[test]
fn fill_edit_add_delete_and_save_round_trip() {
    let mut ctx = setup();
    assert_eq!(fill_blocking(&mut ctx, "customers").expect("fill"), 3);
    let t = ctx.binding_source("customers").expect("bs").table().clone();
    let id = &t.columns[0];
    assert!(id.primary_key && id.auto_increment, "INTEGER PRIMARY KEY is a generated key");
    assert!(!t.columns[1].nullable, "NOT NULL from the schema");
    assert_eq!(t.columns[2].max_length, Some(40));
    assert_eq!(t.columns[4].ty.kind, DbKind::Bool);
    assert_eq!(ctx.get("customers.vip"), Some(Value::Bool(false)));

    // Edit the first row through bindings, move (commits), delete the second, add one.
    assert!(ctx.set("customers.Current.name", Value::Str("Ada Lovelace".into())));
    assert!(ctx.set("customers.Position", Value::F32(1.0)));
    ctx.binding_source_mut("customers").expect("bs").remove_current().expect("remove");
    {
        let mut bs = ctx.binding_source_mut("customers").expect("bs");
        bs.add_new().expect("add");
        bs.set_path("name", &Value::Str("Barbara".into())).expect("edit");
        bs.set_path("email", &Value::Str("barbara@example.org".into())).expect("edit");
        bs.set_path("age", &Value::Str("51".into())).expect("edit");
    }
    assert_eq!(ctx.get("customers.HasChanges"), Some(Value::Bool(true)));
    assert!(matches!(ctx.get("customers.id"), Some(Value::Str(k)) if k.starts_with('-')), "a temporary key until saved");

    assert_eq!(save_blocking(&mut ctx, "customers").expect("save"), 3);
    assert_eq!(
        db_rows(&mut ctx),
        [
            (1, "Ada Lovelace".to_string(), Some("ada@example.org".to_string()), Some(36)),
            (3, "Grace".to_string(), Some("grace@example.org".to_string()), Some(45)),
            (4, "Barbara".to_string(), Some("barbara@example.org".to_string()), Some(51)),
        ]
    );
    let bs = ctx.binding_source("customers").expect("bs");
    assert!(!bs.has_changes());
    assert_eq!(bs.get_path("id"), Some(Value::Str("4".into())), "the generated key is written back to the current (new) row");
    assert!(bs.table().rows().iter().all(|r| r.state == RowState::Unchanged));
    drop(bs);
    // Nothing more to save.
    assert_eq!(save_blocking(&mut ctx, "customers").expect("save"), 0);
}

#[test]
fn a_failed_save_rolls_back_everything_and_keeps_the_changes() {
    let mut ctx = setup();
    fill_blocking(&mut ctx, "customers").expect("fill");
    {
        let mut bs = ctx.binding_source_mut("customers").expect("bs");
        bs.set_path("age", &Value::Str("99".into())).expect("edit");
        bs.move_next().expect("commit");
        // A duplicate e-mail (UNIQUE) in the same transaction.
        bs.set_path("email", &Value::Str("ada@example.org".into())).expect("edit");
    }
    let err = save_blocking(&mut ctx, "customers").expect_err("unique violation");
    assert!(matches!(err, DataError::Database { .. }), "{err:?}");
    assert_eq!(db_rows(&mut ctx)[0].3, Some(36), "the first update was rolled back");
    let bs = ctx.binding_source("customers").expect("bs");
    assert_eq!(bs.table().rows().iter().filter(|r| r.state == RowState::Modified).count(), 2, "the rows keep their changes");
    drop(bs);
    assert!(matches!(ctx.get("errors.Summary"), Some(Value::Str(s)) if s.contains("UNIQUE")));
    assert_eq!(ctx.get("errors.HasErrors"), Some(Value::Bool(true)));
}

#[test]
fn a_row_changed_by_someone_else_is_a_concurrency_violation() {
    let mut ctx = setup();
    fill_blocking(&mut ctx, "customers").expect("fill");
    let db = ctx.connection_handle("db").expect("handle");
    block_on(db.execute("DELETE FROM customers WHERE id = @id", &[("id", 2.into())])).expect("rt").expect("delete");
    {
        let mut bs = ctx.binding_source_mut("customers").expect("bs");
        bs.set_position(1).expect("move");
        bs.set_path("name", &Value::Str("Linus T.".into())).expect("edit");
    }
    let err = save_blocking(&mut ctx, "customers").expect_err("concurrency");
    assert!(matches!(err, DataError::Concurrency(_)), "{err:?}");
}

#[test]
fn validation_blocks_the_save_and_shows_in_the_error_provider() {
    let mut ctx = setup();
    fill_blocking(&mut ctx, "customers").expect("fill");
    ctx.binding_source("customers")
        .expect("bs")
        .row_validating
        .subscribe(|_, e| {
            if !e.text("email").contains('@') {
                e.add_error("email", "Enter a valid e-mail address.");
            }
        })
        .detach();
    ctx.set("customers.email", Value::Str("not-an-address".into()));
    ctx.set("customers.age", Value::Str("old".into()));
    assert_eq!(ctx.get("errors.age"), Some(Value::Str("Enter a whole number.".into())));
    assert_eq!(ctx.get("customers.age"), Some(Value::Str("old".into())));
    assert!(save_blocking(&mut ctx, "customers").is_err());
    ctx.set("customers.age", Value::Str("37".into()));
    assert!(save_blocking(&mut ctx, "customers").is_err(), "RowValidating refuses the e-mail");
    assert_eq!(ctx.get("errors.email"), Some(Value::Str("Enter a valid e-mail address.".into())));
    assert_eq!(ctx.get("errors.email.HasError"), Some(Value::Bool(true)));
    ctx.set("customers.email", Value::Str("ada@lovelace.org".into()));
    assert_eq!(save_blocking(&mut ctx, "customers").expect("save"), 1);
    assert_eq!(ctx.get("errors.HasErrors"), Some(Value::Bool(false)));
    assert_eq!(db_rows(&mut ctx)[0], (1, "Ada".to_string(), Some("ada@lovelace.org".to_string()), Some(37)));
}

#[test]
fn injection_attempts_are_stored_as_data() {
    let mut ctx = setup();
    let db = ctx.connection_handle("db").expect("handle");
    let evil = "x'); DROP TABLE customers; --";
    block_on(db.execute("INSERT INTO customers (name) VALUES (@n)", &[("n", evil.into())])).expect("rt").expect("insert");
    let mut cmd = DbCommand::with_text("SELECT count(*) FROM customers WHERE name = @n");
    cmd.param("n", evil);
    assert_eq!(block_on(cmd.execute_scalar(&db)).expect("rt").expect("scalar"), DbValue::Int(1));
    fill_blocking(&mut ctx, "customers").expect("the table is still there");
}

#[test]
fn a_file_database_persists_across_connections() {
    let dir = std::env::temp_dir().join(format!("kubuno-data-it-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("app.db");
    let view = format!(
        r#"<Panel><DbConnection x:Name="db" Provider="Sqlite" ConnectionString="Data Source={}"/>
           <TableAdapter x:Name="a" Connection="db" SelectCommand="SELECT id, title FROM notes" UpdateTable="notes"/>
           <BindingSource x:Name="notes" DataSource="a" OnListChanged="notes_changed"/></Panel>"#,
        file.display()
    );
    {
        let mut ctx = DataContext::from_view(&view).expect("view");
        let db = ctx.connection_handle("db").expect("handle");
        block_on(db.execute("CREATE TABLE notes (id INTEGER PRIMARY KEY, title TEXT NOT NULL)", &[])).expect("rt").expect("create");
        fill_blocking(&mut ctx, "notes").expect("fill (empty)");
        assert_eq!(ctx.binding_source("notes").map(|b| b.table().columns.len()), Some(2), "columns are typed without rows");
        {
            let mut bs = ctx.binding_source_mut("notes").expect("bs");
            bs.add_new().expect("add");
            bs.set_path("title", &Value::Str("Persisted".into())).expect("edit");
        }
        save_blocking(&mut ctx, "notes").expect("save");
        let events = ctx.take_events();
        assert!(events.iter().all(|e| e.handler == "notes_changed" && e.component == "notes"));
        assert!(!events.is_empty());
        let close = ctx.connection_mut("db").expect("db").close();
        block_on(close).expect("rt").expect("close");
    }
    let mut again = DataContext::from_view(&view).expect("view");
    fill_blocking(&mut again, "notes").expect("fill");
    assert_eq!(again.get("notes.title"), Some(Value::Str("Persisted".into())));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn configuration_errors_are_reported_without_io() {
    let mut ctx = DataContext::from_view(r#"<Panel><BindingSource x:Name="b" DataSource="nope"/><ErrorProvider x:Name="e" DataSource="b"/></Panel>"#).expect("view");
    assert!(matches!(fill_blocking(&mut ctx, "b"), Err(DataError::Config(_))));
    assert!(matches!(ctx.get("e.Summary"), Some(Value::Str(s)) if s.contains("not a TableAdapter") || s.contains("no TableAdapter")), "{:?}", ctx.get("e.Summary"));
}

// ── DATA-2 ──────────────────────────────────────────────────────────────────────────────────

/// A view model whose typed handlers are the `.kbview`'s: `customers_validating` refuses an age
/// over 150 (what `#[event_handlers]` would generate, written by hand).
#[derive(Default)]
struct Vm {
    log: Vec<String>,
}

impl ViewModel for Vm {
    fn get(&self, _: &str) -> Option<Value> {
        None
    }
    fn set(&mut self, _: &str, _: Value) {}
    fn dispatch_event(&mut self, handler: &str, _sender: &ElementRef<'_>, args: &mut dyn EventArgs) -> bool {
        if handler != "customers_validating" {
            return false;
        }
        if let Some(e) = args.downcast_mut::<RowValidatingEventArgs>() {
            self.log.push(format!("validating {}", e.text("age")));
            if e.value("age").and_then(|v| v.to_display().parse::<i64>().ok()).is_some_and(|a| a > 150) {
                e.add_error("age", "Nobody is that old.");
            }
        }
        true
    }
}

#[test]
fn xml_handlers_of_cancelable_events_run_synchronously_through_bindings() {
    let mut ctx = setup();
    fill_blocking(&mut ctx, "customers").expect("fill");
    let scope = ctx.scope().clone();
    let mut vm = Vm::default();
    {
        let mut scoped = scope.view_model(&mut vm, true);
        scoped.set("customers.age", Value::Str("200".into()));
        // Moving ends the edit: the XML handler runs now and refuses the row.
        scoped.set("customers.Position", Value::F32(1.0));
        assert_eq!(scoped.get("customers.Position"), Some(Value::F32(0.0)), "the handler's error kept the row");
        assert_eq!(scoped.get("errors.age"), Some(Value::Str("Nobody is that old.".into())));
    }
    assert_eq!(vm.log, ["validating 200"]);
    {
        let mut scoped = scope.view_model(&mut vm, true);
        scoped.set("customers.age", Value::Str("37".into()));
        scoped.set("customers.Position", Value::F32(1.0));
        assert_eq!(scoped.get("customers.Position"), Some(Value::F32(1.0)));
    }
    assert!(ctx.take_events().iter().all(|e| e.handler != "customers_validating"), "handled synchronously, not queued");
}

#[test]
fn typed_bindings_format_and_parse_through_the_scope() {
    let mut ctx = setup();
    fill_blocking(&mut ctx, "customers").expect("fill");
    let scope = ctx.scope().clone();
    let mut none = MapViewModel::new();
    {
        let mut scoped = scope.view_model(&mut none, false);
        let age = parse_binding("{Binding Source=customers, Path=age, FormatString=N0, Culture=fr-FR, Mode=TwoWay}").expect("binding");
        assert_eq!(scoped.get_bound(&age, ValueKind::Number), Some(Value::F32(36.0)), "a numeric property gets a number");
        scoped.set_bound(&age, Value::Str("1\u{202F}234".into()));
        assert_eq!(scoped.get_bound(&age, ValueKind::Text), Some(Value::Str("1\u{202F}234".into())));
        let email = parse_binding("{Binding Source=customers, Path=email, NullValue='(none)', Mode=TwoWay}").expect("binding");
        scoped.set_bound(&email, Value::Str("(none)".into()));
        assert_eq!(scoped.get_bound(&email, ValueKind::Text), Some(Value::Str("(none)".into())));
    }
    assert_eq!(save_blocking(&mut ctx, "customers").expect("save"), 1);
    assert_eq!(db_rows(&mut ctx)[0], (1, "Ada".to_string(), None, Some(1234)));
}

// ── DATA-3 ──────────────────────────────────────────────────────────────────────────────────

const MASTER_DETAIL: &str = r#"
<Panel>
  <DbConnection x:Name="db" Provider="Sqlite" ConnectionString="sqlite::memory:"/>
  <TableAdapter x:Name="customersAdapter" Connection="db" SelectCommand="SELECT id, name FROM customers ORDER BY id" UpdateTable="customers"/>
  <TableAdapter x:Name="ordersAdapter" Connection="db" SelectCommand="SELECT id, customer_id, item, amount FROM orders ORDER BY id" UpdateTable="orders"/>
  <BindingSource x:Name="customers" DataSource="customersAdapter"/>
  <BindingSource x:Name="orders" DataSource="customers" DataMember="customer_id = id" TableAdapter="ordersAdapter"/>
</Panel>"#;

fn master_detail(view: &str) -> DataContext {
    let mut ctx = DataContext::from_view(view).expect("view");
    let db = ctx.connection_handle("db").expect("handle");
    for sql in [
        "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
        "CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers(id), item TEXT NOT NULL, amount NUMERIC)",
        "INSERT INTO customers (name) VALUES ('Ada'), ('Linus')",
        "INSERT INTO orders (customer_id, item, amount) VALUES (1, 'Tea', 3.5), (2, 'Coffee', 2.25), (1, 'Cake', 4.0)",
    ] {
        block_on(db.execute(sql, &[])).expect("rt").expect("setup");
    }
    ctx
}

fn items(ctx: &DataContext) -> Vec<String> {
    match ctx.get("orders") {
        Some(Value::List(rows)) => rows.iter().map(|r| r.text("item")).collect(),
        _ => Vec::new(),
    }
}

#[test]
fn a_detail_list_follows_its_master_and_saves_with_it() {
    let mut ctx = master_detail(MASTER_DETAIL);
    fill_blocking(&mut ctx, "customers").expect("fill");
    fill_blocking(&mut ctx, "orders").expect("fill");
    assert_eq!(items(&ctx), ["Tea", "Cake"], "the orders of the current customer");
    ctx.set("customers.Position", Value::F32(1.0));
    assert_eq!(items(&ctx), ["Coffee"], "the detail followed the master");
    // A new customer and its first order, saved together: the order takes the generated key.
    {
        let mut bs = ctx.binding_source_mut("customers").expect("bs");
        bs.add_new().expect("add");
        bs.set_path("name", &Value::Str("Grace".into())).expect("edit");
        bs.end_edit().expect("commit");
    }
    ctx.sync();
    let temp = ctx.binding_source("customers").and_then(|b| b.current_value("id").cloned()).expect("key");
    assert!(matches!(temp, DbValue::Int(k) if k < 0), "a temporary key");
    assert_eq!(items(&ctx), Vec::<String>::new());
    {
        let mut orders = ctx.binding_source_mut("orders").expect("bs");
        orders.add_new().expect("add a detail row");
        assert_eq!(orders.current_value("customer_id"), Some(&temp), "the master's temporary key");
        orders.set_path("item", &Value::Str("Compiler".into())).expect("edit");
        orders.set_path("amount", &Value::Str("12.5".into())).expect("edit");
    }
    assert_eq!(save_all_blocking(&mut ctx, &["customers", "orders"]).expect("save"), 2);
    assert_eq!(scalar(&mut ctx, "SELECT customer_id FROM orders WHERE item = 'Compiler'"), DbValue::Int(3), "the generated customer key reached the order");
    assert_eq!(ctx.get("customers.id"), Some(Value::Str("3".into())));
    assert_eq!(ctx.get("orders.customer_id"), Some(Value::Str("3".into())), "the local detail row was rewritten");
    assert_eq!(items(&ctx), ["Compiler"]);
    assert_eq!(ctx.get("orders.HasChanges"), Some(Value::Bool(false)));
    // Saving the detail alone with a reference to an unsaved master fails as a whole.
    {
        let mut bs = ctx.binding_source_mut("customers").expect("bs");
        bs.add_new().expect("add");
        bs.set_path("name", &Value::Str("Barbara".into())).expect("edit");
        bs.end_edit().expect("commit");
    }
    ctx.sync();
    {
        let mut orders = ctx.binding_source_mut("orders").expect("bs");
        orders.add_new().expect("add");
        orders.set_path("item", &Value::Str("Book".into())).expect("edit");
        orders.end_edit().expect("commit");
    }
    assert!(save_blocking(&mut ctx, "orders").is_err(), "the foreign key refuses the temporary key");
    assert_eq!(scalar(&mut ctx, "SELECT count(*) FROM orders"), DbValue::Int(4));
}

#[test]
fn a_parameterized_detail_reads_its_master_rows_only() {
    let view = MASTER_DETAIL.replace("SELECT id, customer_id, item, amount FROM orders ORDER BY id", "SELECT id, customer_id, item, amount FROM orders WHERE customer_id = @customer_id ORDER BY id");
    let mut ctx = master_detail(&view);
    fill_blocking(&mut ctx, "customers").expect("fill");
    ctx.sync();
    assert_eq!(fill_blocking(&mut ctx, "orders").expect("fill"), 2, "the first customer's orders only");
    ctx.set("customers.Position", Value::F32(1.0));
    let request = ctx.binding_source("orders").and_then(|b| b.current_request().params.first().cloned());
    assert_eq!(request, Some(("customer_id".to_string(), DbValue::Int(2))), "a refill is asked for with the new key");
    assert_eq!(fill_blocking(&mut ctx, "orders").expect("fill"), 1);
    assert_eq!(items(&ctx), ["Coffee"]);
}

#[test]
fn a_transaction_spans_commands_and_adapters() {
    let mut ctx = master_detail(MASTER_DETAIL);
    fill_blocking(&mut ctx, "customers").expect("fill");
    let db = ctx.connection_handle("db").expect("handle");
    {
        let mut bs = ctx.binding_source_mut("customers").expect("bs");
        bs.set_path("name", &Value::Str("Ada L.".into())).expect("edit");
        bs.end_edit().expect("commit");
    }
    let plan = {
        let bs = ctx.binding_source("customers").expect("bs");
        let a = ctx.adapter("customersAdapter").expect("adapter");
        a.plan_update(Provider::Sqlite, bs.table()).expect("plan")
    };
    let mut archive = DbCommand::with_text("INSERT INTO orders (customer_id, item) VALUES (@c, @i)");
    archive.param("c", 1).param("i", "Archive");
    // Rolled back: neither the command nor the update stays.
    let tx = block_on(DbTransaction::begin(&db)).expect("rt").expect("begin");
    assert_eq!(block_on(tx.execute(&archive)).expect("rt").expect("execute"), 1);
    block_on(tx.update(&plan)).expect("rt").expect("update");
    block_on(tx.rollback()).expect("rt").expect("rollback");
    assert_eq!(scalar(&mut ctx, "SELECT count(*) FROM orders"), DbValue::Int(3));
    assert_eq!(scalar(&mut ctx, "SELECT name FROM customers WHERE id = 1"), DbValue::Text("Ada".into()));
    // Committed: both.
    let tx = block_on(DbTransaction::begin(&db)).expect("rt").expect("begin");
    block_on(tx.execute(&archive)).expect("rt").expect("execute");
    let seen = block_on(tx.query(&DbCommand::with_text("SELECT count(*) FROM orders"))).expect("rt").expect("query");
    assert_eq!(seen.rows()[0].values[0], DbValue::Int(4), "the transaction sees its own writes");
    block_on(tx.update(&plan)).expect("rt").expect("update");
    block_on(tx.commit()).expect("rt").expect("commit");
    assert!(block_on(tx.commit()).expect("rt").is_err(), "a finished transaction refuses more work");
    assert_eq!(scalar(&mut ctx, "SELECT count(*) FROM orders"), DbValue::Int(4));
    assert_eq!(scalar(&mut ctx, "SELECT name FROM customers WHERE id = 1"), DbValue::Text("Ada L.".into()));
}

fn numbers(ctx: &mut DataContext, n: i64) {
    let db = ctx.connection_handle("db").expect("handle");
    block_on(db.execute("CREATE TABLE numbers (id INTEGER PRIMARY KEY, label TEXT)", &[])).expect("rt").expect("create");
    block_on(db.execute("WITH RECURSIVE s(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM s WHERE i < @n) INSERT INTO numbers (id, label) SELECT i, 'n' || i FROM s", &[("n", n.into())]))
        .expect("rt")
        .expect("fill numbers");
}

#[test]
fn paging_by_offset_and_by_keyset() {
    for mode in ["Offset", "Keyset"] {
        let view = format!(
            r#"<Panel><DbConnection x:Name="db" Provider="Sqlite" ConnectionString="sqlite::memory:"/>
               <TableAdapter x:Name="a" Connection="db" SelectCommand="SELECT id, label FROM numbers ORDER BY id" UpdateTable="numbers" PageSize="10" PagingMode="{mode}"/>
               <BindingSource x:Name="numbers" DataSource="a"/></Panel>"#
        );
        let mut ctx = DataContext::from_view(&view).expect("view");
        numbers(&mut ctx, 25);
        assert_eq!(fill_blocking(&mut ctx, "numbers").expect("fill"), 10, "{mode}");
        assert_eq!(ctx.get("numbers.PageText"), Some(Value::Str("1 / 3".into())), "{mode}");
        assert_eq!(ctx.get("numbers.TotalCount"), Some(Value::F32(25.0)));
        ctx.set("numbers.PageIndex", Value::F32(1.0));
        assert_eq!(fill_blocking(&mut ctx, "numbers").expect("fill"), 10);
        assert_eq!(ctx.get("numbers.id"), Some(Value::Str("11".into())), "{mode}: the second page starts at 11");
        ctx.set("numbers.PageIndex", Value::F32(2.0));
        assert_eq!(fill_blocking(&mut ctx, "numbers").expect("fill"), 5);
        assert_eq!(ctx.get("numbers.CanNextPage"), Some(Value::Bool(false)));
        ctx.set("numbers.PageIndex", Value::F32(0.0));
        fill_blocking(&mut ctx, "numbers").expect("fill");
        assert_eq!(ctx.get("numbers.id"), Some(Value::Str("1".into())), "{mode}: back to the first page");
    }
}

#[test]
fn optimistic_concurrency_compares_the_original_values() {
    let view = VIEW.replace("UpdateTable=\"customers\"", "UpdateTable=\"customers\" ConflictOption=\"CompareAllSearchableValues\"");
    let mut ctx = DataContext::from_view(&view).expect("view");
    let db = ctx.connection_handle("db").expect("handle");
    block_on(db.execute("CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT, age INTEGER, vip BOOLEAN NOT NULL DEFAULT 0)", &[])).expect("rt").expect("create");
    block_on(db.execute("INSERT INTO customers (name, age) VALUES ('Ada', 36), ('Linus', 28)", &[])).expect("rt").expect("insert");
    fill_blocking(&mut ctx, "customers").expect("fill");
    // Someone else changes Ada's age after we read it.
    block_on(db.execute("UPDATE customers SET age = 37 WHERE id = 1", &[])).expect("rt").expect("update");
    ctx.set("customers.name", Value::Str("Ada L.".into()));
    let err = save_blocking(&mut ctx, "customers").expect_err("the original values no longer match");
    assert!(matches!(err, DataError::Concurrency(_)), "{err:?}");
    // Linus was not changed by anyone else: his update goes through (NULL e-mail compared as NULL).
    ctx.binding_source_mut("customers").expect("bs").table_mut().reject_changes();
    ctx.set("customers.Position", Value::F32(1.0));
    ctx.set("customers.name", Value::Str("Linus T.".into()));
    assert_eq!(save_blocking(&mut ctx, "customers").expect("save"), 1);
    assert_eq!(scalar(&mut ctx, "SELECT name FROM customers WHERE id = 2"), DbValue::Text("Linus T.".into()));
}

#[test]
fn custom_dml_commands_and_stored_procedures() {
    let view = VIEW.replace(
        "UpdateTable=\"customers\"",
        "UpdateTable=\"customers\" InsertCommand=\"INSERT INTO customers (name, email, age) VALUES (upper(@name), @email, @age) RETURNING id\" DeleteCommand=\"UPDATE customers SET vip = 1 WHERE id = @Original_id\"",
    );
    let mut ctx = DataContext::from_view(&view).expect("view");
    let db = ctx.connection_handle("db").expect("handle");
    block_on(db.execute("CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT, age INTEGER, vip BOOLEAN NOT NULL DEFAULT 0)", &[])).expect("rt").expect("create");
    block_on(db.execute("INSERT INTO customers (name) VALUES ('Ada')", &[])).expect("rt").expect("insert");
    fill_blocking(&mut ctx, "customers").expect("fill");
    {
        let mut bs = ctx.binding_source_mut("customers").expect("bs");
        bs.remove_current().expect("remove");
        bs.add_new().expect("add");
        bs.set_path("name", &Value::Str("grace".into())).expect("edit");
    }
    assert_eq!(save_blocking(&mut ctx, "customers").expect("save"), 2);
    assert_eq!(scalar(&mut ctx, "SELECT name || ':' || vip FROM customers WHERE id = 1"), DbValue::Text("Ada:1".into()), "the custom delete only flagged the row");
    assert_eq!(scalar(&mut ctx, "SELECT name FROM customers WHERE id = 2"), DbValue::Text("GRACE".into()));
    assert_eq!(ctx.get("customers.id"), Some(Value::Str("2".into())), "RETURNING gave the generated key back");
    let mut proc = DbCommand::procedure("archive_customers");
    proc.param("before", 2020);
    assert!(matches!(block_on(proc.execute_non_query(&db)).expect("rt"), Err(DataError::Validation(m)) if m.contains("SQLite")));
}

#[test]
fn a_long_fill_reports_progress_and_can_be_cancelled() {
    let view = r#"<Panel><DbConnection x:Name="db" Provider="Sqlite" ConnectionString="sqlite::memory:"/>
        <TableAdapter x:Name="a" Connection="db" SelectCommand="SELECT id, label FROM numbers ORDER BY id" UpdateTable="numbers"/>
        <BindingSource x:Name="numbers" DataSource="a"/></Panel>"#;
    let mut ctx = DataContext::from_view(view).expect("view");
    numbers(&mut ctx, 20_000);
    let db = ctx.connection_handle("db").expect("handle");
    let progress = Progress::new();
    let adapter = TableAdapter::new("db", "SELECT id, label FROM numbers ORDER BY id");
    let page = block_on(adapter.fill_page(&db, &FillRequest::default(), None, Some(progress.clone()))).expect("rt").expect("fill");
    assert_eq!(page.table.rows().len(), 20_000);
    assert_eq!(progress.rows(), 20_000, "every row was counted as it arrived");
    // A query that would run for a long time, cancelled from elsewhere.
    let slow = DbCommand::with_text("WITH RECURSIVE s(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM s WHERE i < 5000000) SELECT count(*) FROM s");
    let task = slow.execute_scalar(&db);
    let canceller = task.canceller();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        canceller.cancel();
    });
    let started = std::time::Instant::now();
    assert_eq!(block_on(task).expect("rt"), Err(DataError::Cancelled));
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
    assert_eq!(fill_blocking(&mut ctx, "numbers").expect("the connection still works"), 20_000);
    assert_eq!(ctx.get("numbers.RowsRead"), Some(Value::F32(20_000.0)));
}

// ── DataTable in-place editing (DATA-2 left-overs) ──────────────────────────────────────────

/// A real view runtime paints a `<DataTable>` bound to a `BindingSource` over a SQLite file on a
/// recording canvas: its columns are formatted (`N2`/`d` in French, `NullValue`), cells are
/// edited with the keyboard (the host's frame input), a value that does not convert keeps the
/// grid on its row with the cell's error glyph until a second Escape cancels the row, and the
/// saved file holds the parsed values.
#[test]
fn a_data_table_edits_cells_of_a_binding_source_and_the_save_reaches_the_file() {
    use kubuno_desktop_controls::host::{self, vk, Frame, InputEvent, Modifiers};
    use kubuno_desktop_ui::graphics::testing::RecordingCanvas;
    use kubuno_desktop_ui::Rect;
    use kubuno_desktop_views::runtime::Runtime;

    let dir = std::env::temp_dir().join(format!("kubuno-data-grid-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("shop.db");
    let _ = std::fs::remove_file(&file);
    let view = format!(
        r#"<Panel DesignWidth="600" DesignHeight="400">
  <DbConnection x:Name="db" Provider="Sqlite" ConnectionString="Data Source={}"/>
  <TableAdapter x:Name="ordersAdapter" Connection="db" SelectCommand="SELECT id, item, amount, ordered FROM orders ORDER BY id" UpdateTable="orders"/>
  <BindingSource x:Name="orders" DataSource="ordersAdapter"/>
  <DataTable x:Name="grid" ItemsSource="{{Binding Source=orders}}" SelectedIndex="{{Binding Source=orders, Path=Position, Mode=TwoWay}}" Culture="fr-FR" X="0" Y="0" Width="600" Height="400">
    <Column Header="Id" Binding="{{Binding id}}" ReadOnly="true" Width="60"/>
    <Column Header="Item" Binding="{{Binding item}}" Width="200"/>
    <Column Header="Amount" Binding="{{Binding amount}}" FormatString="N2" Alignment="Right" Width="150"/>
    <Column Header="Ordered" Binding="{{Binding ordered}}" FormatString="d" NullValue="-" Width="150"/>
  </DataTable>
</Panel>"#,
        file.display()
    );
    let mut rt = Runtime::new();
    assert!(rt.reload_from_text(&view), "{:?}", rt.diagnostics());
    let scope = rt.components();
    let db = rt.with_component::<DbConnection, _>("db", |c| c.handle()).expect("the connection").expect("handle");
    for sql in [
        "CREATE TABLE orders (id INTEGER PRIMARY KEY, item TEXT NOT NULL, amount NUMERIC(10,2), ordered DATE)",
        "INSERT INTO orders (item, amount, ordered) VALUES ('Tea', 3.5, '2026-09-29'), ('Engine', 1250.5, '1843-09-01'), ('CLU', 42, NULL)",
    ] {
        block_on(db.execute(sql, &[])).expect("rt").expect("setup");
    }
    assert_eq!(block_on(fill_scope(&scope, "orders")).expect("rt").expect("fill"), 3);

    let mut vm = MapViewModel::new();
    let mut frame = |events: Vec<InputEvent>, mouse: Option<(f32, f32)>, down: bool| -> RecordingCanvas {
        host::input::set_frame_events(events);
        let (x, y) = mouse.unwrap_or((host::POINTER_AWAY, host::POINTER_AWAY));
        let f = Frame {
            size: (600.0, 400.0),
            mouse: (x, y),
            mouse_down: down,
            right_down: false,
            middle_down: false,
            dismiss: false,
            scale: 1.0,
            client_origin: (0.0, 0.0),
            work_area: (0.0, 0.0, 600.0, 400.0),
            chrome_top: 0.0,
            mods: Modifiers::NONE,
            wheel: (0.0, 0.0),
            click_count: u8::from(down),
            window_focused: true,
        };
        let canvas = RecordingCanvas::new();
        rt.frame_model(&canvas, &f, &mut vm, Rect::new(0.0, 0.0, 600.0, 400.0));
        host::input::set_frame_events(Vec::new());
        canvas
    };
    let key = |k: u16| vec![InputEvent::Key { vk: k, down: true, repeat: false, mods: Modifiers::NONE }];
    let text = |s: &str| vec![InputEvent::Text(s.into())];
    let shows = |c: &RecordingCanvas, s: &str| c.calls().iter().any(|l| l.starts_with(&format!("text({s:?}")));
    // Cells: a 40 DIP header, 40 DIP rows; columns 0..60, 60..260, 260..410, 410..560.
    let amount = 335.0;
    let row = |i: usize| 60.0 + 40.0 * i as f32;
    let position = |scope: &kubuno_desktop_views::scope::ComponentScope| scope.with::<BindingSource, _>("orders", |bs| bs.position());

    let c = frame(vec![], None, false);
    assert!(shows(&c, "3,50") && shows(&c, "1\u{202F}250,50") && shows(&c, "42,00"), "N2 in French");
    assert!(shows(&c, "29/09/2026") && shows(&c, "01/09/1843") && shows(&c, "-"), "d in French, NullValue");

    // Engine's amount: typed, Enter commits (Position, then Current.amount parsed per N2 fr-FR).
    frame(vec![], Some((amount, row(1))), true);
    frame(vec![], Some((amount, row(1))), false);
    assert_eq!(position(&scope), Some(1), "the click moved the current row");
    frame(text("2000,75"), None, false);
    frame(key(vk::ENTER), None, false);
    assert_eq!(position(&scope), Some(2), "Enter moved down (ending the row edit)");
    let c = frame(vec![], None, false);
    assert!(shows(&c, "2\u{202F}000,75"), "{:?}", c.calls());
    // CLU's date: → then typed in French.
    frame(key(vk::RIGHT), None, false);
    frame(text("01/05/1952"), None, false);
    frame(key(vk::ENTER), None, false);
    let c = frame(vec![], None, false);
    assert!(shows(&c, "01/05/1952"));

    // Tea's amount: a text that is not a number stays as typed, with the cell's error glyph, and
    // the row cannot be left.
    frame(vec![], Some((amount, row(0))), true);
    frame(vec![], Some((amount, row(0))), false);
    assert_eq!(position(&scope), Some(0));
    frame(text("abc"), None, false);
    frame(key(vk::ENTER), None, false);
    let c = frame(vec![], None, false);
    assert_eq!(position(&scope), Some(0), "the conversion error keeps the grid on the row");
    assert!(shows(&c, "abc"), "the typed text is kept");
    let glyph = kubuno_desktop_ui::tables::DataTable::error_glyph_rect(Rect::new(260.0, 40.0, 410.0, 80.0));
    let expected = format!("fill_rounded({},{},{},{} r=8)", glyph.left, glyph.top, glyph.right, glyph.bottom);
    assert!(c.calls().contains(&expected), "the error glyph {expected} in the cell: {:?}", c.calls());
    // A second Escape cancels the row edit: back to 3,50, no error.
    frame(key(vk::ESCAPE), None, false);
    let c = frame(vec![], None, false);
    assert!(shows(&c, "3,50") && !shows(&c, "abc") && !c.calls().contains(&expected));

    assert_eq!(block_on(save_scope(&scope, &["orders"])).expect("rt").expect("save"), 2);
    let t = block_on(DbCommand::with_text("SELECT id, item, amount, ordered FROM orders ORDER BY id").query(&db)).expect("rt").expect("query");
    let saved: Vec<String> = t.rows().iter().map(|r| r.values.iter().map(|v| v.to_display()).collect::<Vec<_>>().join("|")).collect();
    assert_eq!(saved, ["1|Tea|3.5|2026-09-29", "2|Engine|2000.75|1843-09-01", "3|CLU|42|1952-05-01"]);
    drop(db);
    drop(rt);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `AutoFill="true"` (what the designer's drag and drop writes, DATA-6): the first live frame of
/// the view starts the fill on the view's executor, like the `Fill` call Windows Forms adds to
/// `Form_Load` — no code in the view model; a detail list does not auto-fill on its own.
#[test]
fn auto_fill_fills_the_list_when_the_view_is_shown() {
    use kubuno_desktop_controls::host::{Frame, Modifiers};
    use kubuno_desktop_ui::graphics::testing::RecordingCanvas;
    use kubuno_desktop_ui::Rect;
    use kubuno_desktop_views::runtime::Runtime;

    let dir = std::env::temp_dir().join(format!("kubuno-data-autofill-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("shop.db");
    let _ = std::fs::remove_file(&file);
    let view = format!(
        r#"<Panel DesignWidth="400" DesignHeight="300">
  <DbConnection x:Name="db" Provider="Sqlite" ConnectionString="Data Source={}"/>
  <TableAdapter x:Name="customersAdapter" Connection="db" SelectCommand="SELECT id, name FROM customers ORDER BY id" UpdateTable="customers"/>
  <BindingSource x:Name="customers" DataSource="customersAdapter" AutoFill="true"/>
  <DataTable x:Name="grid" ItemsSource="{{Binding Source=customers}}" X="0" Y="0" Width="400" Height="300">
    <Column Header="Name" Binding="{{Binding name}}" Width="200"/>
  </DataTable>
</Panel>"#,
        file.display()
    );
    let mut rt = Runtime::new();
    assert!(rt.reload_from_text(&view), "{:?}", rt.diagnostics());
    let scope = rt.components();
    let db = rt.with_component::<DbConnection, _>("db", |c| c.handle()).expect("the connection").expect("handle");
    for sql in ["CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL)", "INSERT INTO customers (name) VALUES ('Ada'), ('Grace')"] {
        block_on(db.execute(sql, &[])).expect("rt").expect("setup");
    }
    assert_eq!(scope.with::<BindingSource, _>("customers", |bs| bs.count()), Some(0), "nothing read before the view is shown");

    let mut vm = MapViewModel::new();
    let f = Frame {
        size: (400.0, 300.0),
        mouse: (kubuno_desktop_controls::host::POINTER_AWAY, kubuno_desktop_controls::host::POINTER_AWAY),
        mouse_down: false,
        right_down: false,
        middle_down: false,
        dismiss: false,
        scale: 1.0,
        client_origin: (0.0, 0.0),
        work_area: (0.0, 0.0, 400.0, 300.0),
        chrome_top: 0.0,
        mods: Modifiers::NONE,
        wheel: (0.0, 0.0),
        click_count: 0,
        window_focused: true,
    };
    let mut shown = false;
    for _ in 0..200 {
        let canvas = RecordingCanvas::new();
        rt.frame_model(&canvas, &f, &mut vm, Rect::new(0.0, 0.0, 400.0, 300.0));
        if canvas.calls().iter().any(|l| l.starts_with("text(\"Grace\"")) {
            shown = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(shown, "the grid shows the rows AutoFill read");
    assert_eq!(scope.with::<BindingSource, _>("customers", |bs| bs.count()), Some(2));
    drop(db);
    drop(rt);

    // A connection string no source resolves: the error is reported once, never retried per frame.
    let broken = r#"<Panel DesignWidth="400" DesignHeight="300">
  <DbConnection x:Name="db" Provider="Sqlite" ConnectionStringName="KubunoDataTestMissing"/>
  <TableAdapter x:Name="customersAdapter" Connection="db" SelectCommand="SELECT id, name FROM customers" UpdateTable="customers"/>
  <BindingSource x:Name="customers" DataSource="customersAdapter" AutoFill="true"/>
</Panel>"#;
    let mut rt = Runtime::new();
    assert!(rt.reload_from_text(broken), "{:?}", rt.diagnostics());
    let scope = rt.components();
    let errors = std::rc::Rc::new(std::cell::Cell::new(0));
    let counter = errors.clone();
    scope.with::<BindingSource, _>("customers", |bs| bs.data_error.subscribe(move |_, _| counter.set(counter.get() + 1)).detach());
    let mut frames_after_error = 0;
    for _ in 0..400 {
        rt.frame_model(&RecordingCanvas::new(), &f, &mut vm, Rect::new(0.0, 0.0, 400.0, 300.0));
        if errors.get() > 0 {
            frames_after_error += 1;
            if frames_after_error == 30 {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(frames_after_error, 30, "the fill failed within 4 s");
    assert_eq!(errors.get(), 1, "one DataError, no retry at every frame");
    assert_eq!(scope.with::<BindingSource, _>("customers", |bs| bs.get_path("IsBusy")), Some(Some(Value::Bool(false))));
    let _ = std::fs::remove_dir_all(&dir);
}
