//! `connection.test`, `query.execute` and `data.top`.

use std::time::{Duration, Instant};

use kubuno_desktop_data_model::ProviderName;
use serde_json::{json, Value};

use crate::ctx::Ctx;
use crate::error::{ToolError, ToolResult};
use crate::params::{opt_int, req_str, str_or_empty, target};
use crate::rows::{cell, raw_execute, raw_query, result_set, text};
use crate::schema::{self, TableNode};
use crate::scripts::{qualified, quote_ident};
use crate::split::{returns_rows, split_statements};
use crate::targets::{open, OpenOptions};

/// The largest query text accepted (a query window, not a data import).
const MAX_SQL_BYTES: usize = 4 * 1024 * 1024;

/// The statement returning the server's version.
fn version_query(provider: ProviderName) -> &'static str {
    match provider {
        ProviderName::Sqlite => "SELECT sqlite_version()",
        ProviderName::Postgres => "SELECT current_setting('server_version')::text",
        ProviderName::Mysql => "SELECT CAST(VERSION() AS CHAR)",
        ProviderName::Sqlserver => "SELECT CAST(SERVERPROPERTY('ProductVersion') AS nvarchar(128))",
    }
}

/// `connection.test {target, timeoutSeconds?}` → `{serverVersion, elapsedMs}`.
pub async fn connection_test(ctx: &Ctx, params: &Value) -> ToolResult<Value> {
    let timeout = opt_int(params, "timeoutSeconds", 15, 1, 300)?;
    let session = open(ctx, &target(params)?, OpenOptions { max_pool_size: 1, connect_timeout_secs: timeout as u32, command_timeout_secs: timeout as u32, create_sqlite_file: false })?;
    let started = Instant::now();
    let table = raw_query(&session.handle, version_query(session.provider()), Some(Duration::from_secs(timeout))).await?;
    let version = table.rows().first().map(|r| text(cell(&r.values, 0))).unwrap_or_default();
    Ok(json!({"serverVersion": version, "elapsedMs": started.elapsed().as_millis() as u64}))
}

/// `query.execute {target, sql, maxRows, timeoutSeconds}`.
pub async fn execute(ctx: &Ctx, params: &Value) -> ToolResult<Value> {
    let sql = req_str(params, "sql")?;
    if sql.len() > MAX_SQL_BYTES {
        return Err(ToolError::validation("the query text is larger than 4 MiB"));
    }
    let max_rows = opt_int(params, "maxRows", 1000, 1, 1_000_000)? as usize;
    let timeout = opt_int(params, "timeoutSeconds", 30, 1, 3600)?;
    // One connection for the whole text: `SET`, `USE`, temporary tables and variables carry over
    // from one statement to the next. No implicit transaction.
    let session = open(ctx, &target(params)?, OpenOptions { max_pool_size: 1, connect_timeout_secs: timeout.min(15) as u32, command_timeout_secs: timeout as u32, create_sqlite_file: false })?;
    let provider = session.provider();
    let statements = split_statements(sql, provider);
    if statements.is_empty() {
        return Err(ToolError::validation("the text holds no SQL statement"));
    }
    let started = Instant::now();
    let limit = Some(Duration::from_secs(timeout));
    let (mut sets, mut messages) = (Vec::new(), Vec::new());
    let total = statements.len();
    for (n, statement) in statements.iter().enumerate() {
        let outcome = if returns_rows(statement, provider) {
            raw_query(&session.handle, statement, limit).await.map(|table| {
                if table.columns.is_empty() {
                    messages.push("Command completed.".to_string());
                } else {
                    sets.push(result_set(&table, max_rows));
                }
            })
        } else {
            raw_execute(&session.handle, statement, limit).await.map(|affected| messages.push(format!("{affected} row(s) affected")))
        };
        if let Err(e) = outcome {
            if e.kind == "Cancelled" {
                return Err(e);
            }
            let mut message = if total > 1 { format!("statement {} of {total}: {}", n + 1, e.message) } else { e.message };
            if n > 0 {
                message.push_str(&format!(" (the {n} statement(s) before it already ran and were not rolled back)"));
            }
            return Err(ToolError { kind: e.kind, message });
        }
    }
    Ok(json!({"resultSets": sets, "messages": messages, "elapsedMs": started.elapsed().as_millis() as u64}))
}

/// PostgreSQL types the runtime decodes natively; the others are read as text.
const PG_NATIVE: &[&str] = &["bool", "int2", "int4", "int8", "float4", "float8", "bytea", "uuid", "timestamptz", "timestamp", "date", "time", "json", "jsonb", "text", "varchar", "bpchar", "name"];

/// `SELECT … FROM <table>` limited to `limit` rows, for the grid of the Data Explorer.
pub fn top_query(provider: ProviderName, schema: &str, table: &TableNode, limit: u64) -> ToolResult<String> {
    let from = qualified(provider, schema, &table.name)?;
    let list = match provider {
        // A PostgreSQL column of a type the runtime cannot decode (numeric, interval, arrays…) is read as text.
        ProviderName::Postgres => {
            let mut parts = Vec::with_capacity(table.columns.len());
            for c in &table.columns {
                let q = quote_ident(provider, &c.name)?;
                parts.push(if PG_NATIVE.contains(&c.db_type.as_str()) { q } else { format!("{q}::text AS {q}") });
            }
            if parts.is_empty() { "*".to_string() } else { parts.join(", ") }
        }
        _ => "*".to_string(),
    };
    Ok(match provider {
        ProviderName::Sqlserver => format!("SELECT TOP ({limit}) {list} FROM {from}"),
        _ => format!("SELECT {list} FROM {from} LIMIT {limit}"),
    })
}

/// `data.top {target, schema, table, limit}` → a result set.
pub async fn data_top(ctx: &Ctx, params: &Value) -> ToolResult<Value> {
    let table_name = req_str(params, "table")?;
    let limit = opt_int(params, "limit", 100, 1, 100_000)?;
    let session = open(ctx, &target(params)?, OpenOptions::default())?;
    let provider = session.provider();
    // The table must exist in the catalog: nothing the caller typed reaches the SQL text unchecked.
    let (schema_name, table) = schema::load_table(&session.handle, provider, str_or_empty(params, "schema"), table_name).await?;
    let sql = top_query(provider, &schema_name, &table, limit)?;
    let data = raw_query(&session.handle, &sql, None).await?;
    serde_json::to_value(result_set(&data, limit as usize)).map_err(|e| ToolError::new("Io", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::column;

    fn table() -> TableNode {
        TableNode {
            name: "Order Details".into(),
            kind: "table",
            columns: vec![column(ProviderName::Postgres, "id".into(), "int4".into(), "integer".into(), false), column(ProviderName::Postgres, "price".into(), "numeric".into(), "numeric(10,2)".into(), true)],
            primary_key: vec!["id".into()],
            foreign_keys: vec![],
            indexes: vec![],
        }
    }

    #[test]
    fn top_queries_are_quoted_per_provider() {
        let t = table();
        assert_eq!(top_query(ProviderName::Sqlite, "main", &t, 5).expect("q"), "SELECT * FROM \"main\".\"Order Details\" LIMIT 5");
        assert_eq!(top_query(ProviderName::Mysql, "shop", &t, 5).expect("q"), "SELECT * FROM `shop`.`Order Details` LIMIT 5");
        assert_eq!(top_query(ProviderName::Sqlserver, "dbo", &t, 5).expect("q"), "SELECT TOP (5) * FROM [dbo].[Order Details]");
        assert_eq!(
            top_query(ProviderName::Postgres, "public", &t, 5).expect("q"),
            "SELECT \"id\", \"price\"::text AS \"price\" FROM \"public\".\"Order Details\" LIMIT 5"
        );
        let mut evil = table();
        evil.name = "t\"; DROP TABLE x; --".into();
        assert_eq!(top_query(ProviderName::Sqlite, "main", &evil, 1).expect("q"), "SELECT * FROM \"main\".\"t\"\"; DROP TABLE x; --\" LIMIT 1");
    }
}
