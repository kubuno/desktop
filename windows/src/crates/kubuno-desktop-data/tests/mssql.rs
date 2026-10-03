//! DATA-3 integration tests on SQL Server (feature `mssql`, `tiberius`). They run only when
//! `KUBUNO_TEST_MSSQL_URL` is an ADO.NET connection string of a disposable database the tests may
//! create tables in (e.g. `Server=tcp:localhost,1433;Database=kubuno_test;Integrated
//! Security=true;TrustServerCertificate=true`); without it they pass without doing anything. Never
//! point it at a shared or production database: the tests create, then drop, their own tables
//! `kdt_<pid>_…`.
#![cfg(feature = "mssql")]

use kubuno_desktop_data::*;
use kubuno_desktop_views::binding::Value;

fn setup(tag: &str) -> Option<(DataContext, String)> {
    let url = std::env::var("KUBUNO_TEST_MSSQL_URL").ok().filter(|u| !u.trim().is_empty())?;
    let prefix = format!("kdt_{}_{tag}", std::process::id());
    std::env::set_var("ConnectionStrings__KubunoDataMssqlTest", &url);
    let view = format!(
        r#"<Panel>
             <DbConnection x:Name="db" Provider="SqlServer" ConnectionStringName="KubunoDataMssqlTest"/>
             <TableAdapter x:Name="ca" Connection="db" SelectCommand="SELECT id, name, email, born, amount, rv FROM dbo.{prefix}_customers ORDER BY id" UpdateTable="dbo.{prefix}_customers" ConflictOption="CompareRowVersion" RowVersionColumn="rv"/>
             <TableAdapter x:Name="oa" Connection="db" SelectCommand="SELECT id, customer_id, item FROM dbo.{prefix}_orders ORDER BY id" UpdateTable="dbo.{prefix}_orders"/>
             <BindingSource x:Name="customers" DataSource="ca"/>
             <BindingSource x:Name="orders" DataSource="customers" DataMember="customer_id = id" TableAdapter="oa"/>
           </Panel>"#
    );
    let mut ctx = DataContext::from_view(&view).expect("view");
    let db = ctx.connection_handle("db").expect("handle");
    for sql in [
        format!("IF OBJECT_ID('dbo.{prefix}_orders') IS NOT NULL DROP TABLE dbo.{prefix}_orders"),
        format!("IF OBJECT_ID('dbo.{prefix}_customers') IS NOT NULL DROP TABLE dbo.{prefix}_customers"),
        format!("CREATE TABLE dbo.{prefix}_customers (id INT IDENTITY PRIMARY KEY, name NVARCHAR(50) NOT NULL, email NVARCHAR(80) NULL UNIQUE, born DATE NULL, amount DECIMAL(10,2) NULL, rv ROWVERSION)"),
        format!("CREATE TABLE dbo.{prefix}_orders (id INT IDENTITY PRIMARY KEY, customer_id INT NOT NULL REFERENCES dbo.{prefix}_customers(id), item NVARCHAR(50) NOT NULL)"),
        format!("INSERT INTO dbo.{prefix}_customers (name, email, born, amount) VALUES (N'Ada', N'ada@example.org', '1815-12-10', 1234.50), (N'Linus', N'linus@example.org', NULL, NULL)"),
        format!("INSERT INTO dbo.{prefix}_orders (customer_id, item) VALUES (1, N'Tea'), (2, N'Coffee')"),
    ] {
        block_on(db.execute(&sql, &[])).expect("rt").expect("setup");
    }
    Some((ctx, prefix))
}

fn teardown(ctx: &mut DataContext, prefix: &str) {
    if let Ok(db) = ctx.connection_handle("db") {
        let _ = block_on(db.execute(&format!("IF OBJECT_ID('dbo.{prefix}_orders') IS NOT NULL DROP TABLE dbo.{prefix}_orders"), &[]));
        let _ = block_on(db.execute(&format!("IF OBJECT_ID('dbo.{prefix}_customers') IS NOT NULL DROP TABLE dbo.{prefix}_customers"), &[]));
    }
}

#[test]
fn sql_server_fill_edit_row_versions_master_detail_and_save() {
    let Some((mut ctx, prefix)) = setup("crud") else {
        eprintln!("KUBUNO_TEST_MSSQL_URL is not set: SQL Server tests skipped");
        return;
    };
    assert_eq!(fill_blocking(&mut ctx, "customers").expect("fill"), 2);
    assert_eq!(fill_blocking(&mut ctx, "orders").expect("fill"), 1);
    let t = ctx.binding_source("customers").expect("bs").table().clone();
    assert!(t.columns[0].primary_key && t.columns[0].auto_increment, "IDENTITY");
    assert!(t.columns[5].read_only, "the rowversion is the database's");
    assert_eq!(ctx.get("customers.amount"), Some(Value::Str("1234.50".into())));
    ctx.set("customers.name", Value::Str("Ada L.".into()));
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
    assert_eq!(ctx.get("orders.customer_id"), Some(Value::Str("3".into())), "OUTPUT INSERTED gave the key");
    // The row version was read back: a second edit of Ada passes.
    ctx.set("customers.Position", Value::F32(0.0));
    ctx.set("customers.name", Value::Str("Ada Lovelace".into()));
    assert_eq!(save_blocking(&mut ctx, "customers").expect("save"), 1);
    // Someone else changes Ada: the stale version is a concurrency violation.
    let db = ctx.connection_handle("db").expect("handle");
    block_on(db.execute(&format!("UPDATE dbo.{prefix}_customers SET name = N'Other' WHERE id = 1"), &[])).expect("rt").expect("update");
    ctx.set("customers.name", Value::Str("Ada again".into()));
    assert!(matches!(save_blocking(&mut ctx, "customers"), Err(DataError::Concurrency(_))));
    teardown(&mut ctx, &prefix);
}
