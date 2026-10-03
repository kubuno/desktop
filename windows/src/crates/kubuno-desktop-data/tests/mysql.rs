//! DATA-3 integration tests on MySQL / MariaDB (feature `mysql`). They run only when
//! `KUBUNO_TEST_MYSQL_URL` names a disposable database the tests may create tables in (e.g.
//! `mysql://kubuno_test@localhost/kubuno_test`); without it they pass without doing anything.
//! Never point it at a shared or production database: the tests create, then drop, their own
//! tables `kdt_<pid>_…`.
#![cfg(feature = "mysql")]

use kubuno_desktop_data::*;
use kubuno_desktop_views::binding::Value;

fn setup(tag: &str) -> Option<(DataContext, String)> {
    let url = std::env::var("KUBUNO_TEST_MYSQL_URL").ok().filter(|u| !u.trim().is_empty())?;
    let prefix = format!("kdt_{}_{tag}", std::process::id());
    std::env::set_var("ConnectionStrings__KubunoDataMySqlTest", &url);
    let view = format!(
        r#"<Panel>
             <DbConnection x:Name="db" Provider="MySql" ConnectionStringName="KubunoDataMySqlTest"/>
             <TableAdapter x:Name="ca" Connection="db" SelectCommand="SELECT id, name, email, born, amount, vip FROM {prefix}_customers ORDER BY id" UpdateTable="{prefix}_customers"/>
             <TableAdapter x:Name="oa" Connection="db" SelectCommand="SELECT id, customer_id, item FROM {prefix}_orders ORDER BY id" UpdateTable="{prefix}_orders"/>
             <BindingSource x:Name="customers" DataSource="ca"/>
             <BindingSource x:Name="orders" DataSource="customers" DataMember="customer_id = id" TableAdapter="oa"/>
           </Panel>"#
    );
    let mut ctx = DataContext::from_view(&view).expect("view");
    let db = ctx.connection_handle("db").expect("handle");
    for sql in [
        format!("DROP TABLE IF EXISTS {prefix}_orders"),
        format!("DROP TABLE IF EXISTS {prefix}_customers"),
        format!("CREATE TABLE {prefix}_customers (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(50) NOT NULL, email VARCHAR(80) UNIQUE, born DATE, amount DECIMAL(10,2), vip BOOLEAN NOT NULL DEFAULT 0)"),
        format!("CREATE TABLE {prefix}_orders (id INT AUTO_INCREMENT PRIMARY KEY, customer_id INT NOT NULL, item VARCHAR(50) NOT NULL, FOREIGN KEY (customer_id) REFERENCES {prefix}_customers(id))"),
        format!("INSERT INTO {prefix}_customers (name, email, born, amount) VALUES ('Ada', 'ada@example.org', '1815-12-10', 1234.50), ('Linus', 'linus@example.org', NULL, NULL)"),
        format!("INSERT INTO {prefix}_orders (customer_id, item) VALUES (1, 'Tea'), (2, 'Coffee')"),
    ] {
        block_on(db.execute(&sql, &[])).expect("rt").expect("setup");
    }
    Some((ctx, prefix))
}

fn teardown(ctx: &mut DataContext, prefix: &str) {
    if let Ok(db) = ctx.connection_handle("db") {
        let _ = block_on(db.execute(&format!("DROP TABLE IF EXISTS {prefix}_orders"), &[]));
        let _ = block_on(db.execute(&format!("DROP TABLE IF EXISTS {prefix}_customers"), &[]));
    }
}

#[test]
fn mysql_fill_edit_master_detail_and_save() {
    let Some((mut ctx, prefix)) = setup("crud") else {
        eprintln!("KUBUNO_TEST_MYSQL_URL is not set: MySQL tests skipped");
        return;
    };
    assert_eq!(fill_blocking(&mut ctx, "customers").expect("fill"), 2);
    assert_eq!(fill_blocking(&mut ctx, "orders").expect("fill"), 1, "the first customer's orders");
    let t = ctx.binding_source("customers").expect("bs").table().clone();
    assert!(t.columns[0].primary_key && t.columns[0].auto_increment, "AUTO_INCREMENT");
    assert_eq!(t.columns[3].ty.kind, DbKind::Date);
    assert_eq!(t.columns[5].ty.kind, DbKind::Bool);
    assert_eq!(ctx.get("customers.amount"), Some(Value::Str("1234.50".into())), "DECIMAL keeps its digits");
    assert_eq!(ctx.get("orders.item"), Some(Value::Str("Tea".into())));
    ctx.set("customers.born", Value::Str("1815-12-11".into()));
    {
        let mut bs = ctx.binding_source_mut("customers").expect("bs");
        bs.add_new().expect("add");
        bs.set_path("name", &Value::Str("Grace".into())).expect("edit");
        bs.end_edit().expect("commit");
    }
    ctx.sync();
    {
        let mut orders = ctx.binding_source_mut("orders").expect("bs");
        orders.add_new().expect("add");
        orders.set_path("item", &Value::Str("Compiler".into())).expect("edit");
    }
    assert_eq!(save_all_blocking(&mut ctx, &["customers", "orders"]).expect("save"), 3);
    assert_eq!(ctx.get("customers.id"), Some(Value::Str("3".into())), "LAST_INSERT_ID written back");
    assert_eq!(ctx.get("orders.customer_id"), Some(Value::Str("3".into())));
    let db = ctx.connection_handle("db").expect("handle");
    let check = DbCommand::with_text(format!("SELECT customer_id FROM {prefix}_orders WHERE item = 'Compiler'"));
    assert_eq!(block_on(check.execute_scalar(&db)).expect("rt").expect("scalar"), DbValue::Int(3));
    ctx.set("customers.email", Value::Str("ada@example.org".into()));
    assert!(matches!(save_blocking(&mut ctx, "customers"), Err(DataError::Database { .. })), "UNIQUE rolls back");
    teardown(&mut ctx, &prefix);
}
