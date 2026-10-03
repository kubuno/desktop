//! Providers and their drivers: the only module that knows the differences between databases
//! (placeholders, quoting, type names, generated keys, schema queries, value decoding). Everything
//! above it sees a [`Pool`], [`DbValue`]s and [`Table`]s.
//!
//! PostgreSQL, SQLite and MySQL/MariaDB go through sqlx (features `postgres`, `sqlite`, `mysql`);
//! SQL Server through `tiberius` (feature `mssql`, `crate::mssql`), with a small pool of its own.

use std::time::Duration;

use kubuno_desktop_views::prelude::PropertyValue;

use crate::error::{log_db, logged, DataError};
use crate::rt::Progress;
use crate::table::{DataColumn, Table};
use crate::value::{DbKind, DbType, DbValue};

/// A database provider (the `Provider` property of a `DbConnection`).
#[derive(PropertyValue, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Provider {
    /// PostgreSQL (Kubuno's own stack).
    #[default]
    Postgres,
    Sqlite,
    /// MySQL / MariaDB (feature `mysql`).
    MySql,
    /// SQL Server through `tiberius` (feature `mssql`).
    SqlServer,
}

impl Provider {
    /// The placeholder of parameter `n` (1-based).
    pub fn placeholder(self, n: usize) -> String {
        match self {
            Provider::Postgres => format!("${n}"),
            Provider::Sqlite => format!("?{n}"),
            Provider::MySql => "?".to_string(),
            Provider::SqlServer => format!("@P{n}"),
        }
    }

    /// Whether a placeholder names its parameter by number (a repeated `@name` reuses it); `false`
    /// for MySQL's `?` (the value is passed again).
    pub fn numbered_placeholders(self) -> bool {
        self != Provider::MySql
    }

    /// Whether this build has the provider's driver.
    pub fn is_available(self) -> bool {
        match self {
            Provider::Postgres => cfg!(feature = "postgres"),
            Provider::Sqlite => cfg!(feature = "sqlite"),
            Provider::MySql => cfg!(feature = "mysql"),
            Provider::SqlServer => cfg!(feature = "mssql"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Postgres => "Postgres",
            Provider::Sqlite => "Sqlite",
            Provider::MySql => "MySql",
            Provider::SqlServer => "SqlServer",
        }
    }

    /// The provider's name in a `.kbdata` file (`kubuno-desktop-data-model`, DATA-4).
    pub fn model_name(self) -> kubuno_desktop_data_model::ProviderName {
        match self {
            Provider::Postgres => kubuno_desktop_data_model::ProviderName::Postgres,
            Provider::Sqlite => kubuno_desktop_data_model::ProviderName::Sqlite,
            Provider::MySql => kubuno_desktop_data_model::ProviderName::Mysql,
            Provider::SqlServer => kubuno_desktop_data_model::ProviderName::Sqlserver,
        }
    }

    /// Whether `INSERT … RETURNING` (or SQL Server's `OUTPUT INSERTED`) gives generated values.
    pub fn has_returning(self) -> bool {
        self != Provider::MySql
    }
}

/// What a statement of a transaction must produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Expect {
    /// Exactly one affected row, else a concurrency violation.
    One,
    /// Exactly one returned row; its first column is the result (a generated key, a new row version).
    Returning,
    /// MySQL: one affected row, the result is the connection's last insert id.
    LastInsertId,
}

/// One statement of a transaction (generated or custom DML).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Statement {
    pub sql: String,
    pub params: Vec<DbValue>,
    pub expect: Expect,
    /// For messages: what the statement does (`"the update of a row of customers"`).
    pub what: String,
    /// The parameters holding a parent row's key (a relation, DATA-3): when one holds a temporary
    /// key of a row inserted earlier in the transaction, the key the database gave it replaces it.
    pub fk_params: Vec<usize>,
    /// The temporary key of the row this insert creates (mapped to the generated one).
    pub temp_key: Option<DbValue>,
    /// MySQL (no `RETURNING`): a query run after the statement on the same connection, whose first
    /// column is the result (a new row version).
    pub follow_up: Option<(String, Vec<DbValue>)>,
}

impl Statement {
    pub fn new(sql: String, params: Vec<DbValue>, expect: Expect, what: String) -> Self {
        Self { sql, params, expect, what, fk_params: Vec::new(), temp_key: None, follow_up: None }
    }

    /// The parameters with the temporary keys of `key_map` replaced (only in `fk_params`).
    pub fn resolved_params(&self, key_map: &[(DbValue, DbValue)]) -> Vec<DbValue> {
        let mut params = self.params.clone();
        for &i in &self.fk_params {
            if let Some(p) = params.get_mut(i) {
                if let Some((_, real)) = key_map.iter().find(|(temp, _)| temp == p) {
                    *p = real.clone();
                }
            }
        }
        params
    }
}

/// What a transaction returned: one entry per statement (a generated key, a new version or
/// nothing), and the temporary keys it replaced.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct TxOutcome {
    pub returned: Vec<Option<Vec<DbValue>>>,
    pub key_map: Vec<(DbValue, DbValue)>,
}

/// What the schema of a table says about a column.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct ColumnSchema {
    pub name: String,
    pub native: String,
    pub nullable: bool,
    pub max_length: Option<usize>,
    pub primary_key: bool,
    pub auto_increment: bool,
    /// Written by the database only (a SQL Server `rowversion`).
    pub read_only: bool,
}

/// A connection pool of one of the compiled-in drivers.
#[derive(Clone, Debug)]
pub(crate) enum Pool {
    #[cfg(feature = "postgres")]
    Postgres(sqlx::PgPool),
    #[cfg(feature = "sqlite")]
    Sqlite(sqlx::SqlitePool),
    #[cfg(feature = "mysql")]
    MySql(sqlx::MySqlPool),
    #[cfg(feature = "mssql")]
    SqlServer(crate::mssql::MsPool),
}

/// Binds `DbValue`s to a sqlx query, in order.
macro_rules! bind_params {
    ($q:expr, $params:expr) => {{
        let mut q = $q;
        for p in $params.iter() {
            q = match p {
                DbValue::Null => q.bind(None::<String>),
                DbValue::Bool(b) => q.bind(*b),
                DbValue::Int(i) => q.bind(*i),
                DbValue::Float(f) => q.bind(*f),
                DbValue::Text(s) => q.bind(s.clone()),
                DbValue::Bytes(b) => q.bind(b.clone()),
            };
        }
        q
    }};
}

pub(crate) async fn with_timeout<T>(what: &str, timeout: Duration, f: impl std::future::Future<Output = Result<T, DataError>>) -> Result<T, DataError> {
    match tokio::time::timeout(timeout, f).await {
        Ok(r) => r,
        Err(_) => Err(logged(what, DataError::Database { message: format!("the command timed out after {} s", timeout.as_secs()), code: None })),
    }
}

/// Runs statements on one open connection (a transaction's): the results per [`Expect`], the
/// temporary keys replaced by generated ones. Any failure returns before the next statement (the
/// caller rolls back).
macro_rules! run_statements {
    ($conn:expr, $decode:path, $statements:expr, $key_map:expr) => {{
        let conn = $conn;
        let key_map: &mut Vec<(DbValue, DbValue)> = $key_map;
        let mut out: Vec<Option<Vec<DbValue>>> = Vec::with_capacity($statements.len());
        for st in $statements.iter() {
            let params = st.resolved_params(key_map);
            let q = bind_params!(sqlx::query(&st.sql), &params);
            let result = match st.expect {
                Expect::Returning => match q.fetch_optional(&mut *conn).await {
                    Ok(Some(row)) => $decode(&row).map_err(|e| log_db(&st.what, e))?,
                    Ok(None) => return Err(logged("transaction", DataError::Concurrency(format!("{} matched no row: the row was changed or deleted by someone else", st.what)))),
                    Err(e) => return Err(log_db(&st.what, e)),
                },
                Expect::One | Expect::LastInsertId => match q.execute(&mut *conn).await {
                    Ok(r) if r.rows_affected() != 1 => {
                        return Err(logged(
                            "transaction",
                            DataError::Concurrency(format!("{} affected {} rows instead of 1: the row was changed or deleted by someone else", st.what, r.rows_affected())),
                        ))
                    }
                    Ok(r) => last_insert_id(st, &r),
                    Err(e) => return Err(log_db(&st.what, e)),
                },
            };
            let result = match &st.follow_up {
                Some((sql, fparams)) => {
                    let fq = bind_params!(sqlx::query(sql), fparams);
                    match fq.fetch_optional(&mut *conn).await {
                        Ok(Some(row)) => $decode(&row).map_err(|e| log_db(&st.what, e))?,
                        Ok(None) => return Err(logged("transaction", DataError::Concurrency(format!("{}: the row is gone", st.what)))),
                        Err(e) => return Err(log_db(&st.what, e)),
                    }
                }
                None => result,
            };
            if let (Some(temp), Some(real)) = (&st.temp_key, result.first().filter(|v| !v.is_null())) {
                key_map.push((temp.clone(), real.clone()));
            }
            out.push(if st.expect == Expect::One && st.follow_up.is_none() { None } else { Some(result) });
        }
        Ok::<Vec<Option<Vec<DbValue>>>, DataError>(out)
    }};
}

/// The result of an executed statement: the last insert id for [`Expect::LastInsertId`] (MySQL),
/// else nothing.
trait LastInsertId {
    fn last_id(&self) -> Option<i64>;
}

#[cfg(feature = "postgres")]
impl LastInsertId for sqlx::postgres::PgQueryResult {
    fn last_id(&self) -> Option<i64> {
        None
    }
}

#[cfg(feature = "sqlite")]
impl LastInsertId for sqlx::sqlite::SqliteQueryResult {
    fn last_id(&self) -> Option<i64> {
        Some(self.last_insert_rowid())
    }
}

#[cfg(feature = "mysql")]
impl LastInsertId for sqlx::mysql::MySqlQueryResult {
    fn last_id(&self) -> Option<i64> {
        i64::try_from(self.last_insert_id()).ok()
    }
}

fn last_insert_id(st: &Statement, r: &impl LastInsertId) -> Vec<DbValue> {
    match (st.expect, r.last_id()) {
        (Expect::LastInsertId, Some(id)) => vec![DbValue::Int(id)],
        _ => Vec::new(),
    }
}

/// An open transaction of one of the drivers (`crate::transaction::DbTransaction`).
pub(crate) enum TxConn {
    #[cfg(feature = "postgres")]
    Postgres(sqlx::Transaction<'static, sqlx::Postgres>),
    #[cfg(feature = "sqlite")]
    Sqlite(sqlx::Transaction<'static, sqlx::Sqlite>),
    #[cfg(feature = "mysql")]
    MySql(sqlx::Transaction<'static, sqlx::MySql>),
    #[cfg(feature = "mssql")]
    SqlServer(Box<crate::mssql::MsTransaction>),
}

impl TxConn {
    /// Runs statements inside the transaction (see [`Pool::run_in_transaction`]).
    pub async fn run(&mut self, statements: &[Statement], key_map: &mut Vec<(DbValue, DbValue)>) -> Result<Vec<Option<Vec<DbValue>>>, DataError> {
        match self {
            #[cfg(feature = "postgres")]
            TxConn::Postgres(tx) => run_statements!(&mut **tx, pg::row_values, statements, key_map),
            #[cfg(feature = "sqlite")]
            TxConn::Sqlite(tx) => run_statements!(&mut **tx, lite::row_values, statements, key_map),
            #[cfg(feature = "mysql")]
            TxConn::MySql(tx) => run_statements!(&mut **tx, my::row_values, statements, key_map),
            #[cfg(feature = "mssql")]
            TxConn::SqlServer(tx) => tx.run(statements, key_map).await,
        }
    }

    /// Runs a query inside the transaction.
    pub async fn query_table(&mut self, sql: &str, params: &[DbValue]) -> Result<Table, DataError> {
        match self {
            #[cfg(feature = "postgres")]
            TxConn::Postgres(tx) => pg::read_table(bind_params!(sqlx::query(sql), params).fetch(&mut **tx), None).await,
            #[cfg(feature = "sqlite")]
            TxConn::Sqlite(tx) => lite::read_table(bind_params!(sqlx::query(sql), params).fetch(&mut **tx), None).await,
            #[cfg(feature = "mysql")]
            TxConn::MySql(tx) => my::read_table(bind_params!(sqlx::query(sql), params).fetch(&mut **tx), None).await,
            #[cfg(feature = "mssql")]
            TxConn::SqlServer(tx) => tx.query_table(sql, params).await,
        }
    }

    /// Runs a statement inside the transaction; the affected rows.
    pub async fn execute(&mut self, sql: &str, params: &[DbValue]) -> Result<u64, DataError> {
        match self {
            #[cfg(feature = "postgres")]
            TxConn::Postgres(tx) => bind_params!(sqlx::query(sql), params).execute(&mut **tx).await.map(|r| r.rows_affected()).map_err(|e| log_db("execute", e)),
            #[cfg(feature = "sqlite")]
            TxConn::Sqlite(tx) => bind_params!(sqlx::query(sql), params).execute(&mut **tx).await.map(|r| r.rows_affected()).map_err(|e| log_db("execute", e)),
            #[cfg(feature = "mysql")]
            TxConn::MySql(tx) => bind_params!(sqlx::query(sql), params).execute(&mut **tx).await.map(|r| r.rows_affected()).map_err(|e| log_db("execute", e)),
            #[cfg(feature = "mssql")]
            TxConn::SqlServer(tx) => tx.execute(sql, params).await,
        }
    }

    pub async fn commit(self) -> Result<(), DataError> {
        match self {
            #[cfg(feature = "postgres")]
            TxConn::Postgres(tx) => tx.commit().await.map_err(|e| log_db("commit", e)),
            #[cfg(feature = "sqlite")]
            TxConn::Sqlite(tx) => tx.commit().await.map_err(|e| log_db("commit", e)),
            #[cfg(feature = "mysql")]
            TxConn::MySql(tx) => tx.commit().await.map_err(|e| log_db("commit", e)),
            #[cfg(feature = "mssql")]
            TxConn::SqlServer(tx) => tx.commit().await,
        }
    }

    pub async fn rollback(self) -> Result<(), DataError> {
        match self {
            #[cfg(feature = "postgres")]
            TxConn::Postgres(tx) => tx.rollback().await.map_err(|e| log_db("rollback", e)),
            #[cfg(feature = "sqlite")]
            TxConn::Sqlite(tx) => tx.rollback().await.map_err(|e| log_db("rollback", e)),
            #[cfg(feature = "mysql")]
            TxConn::MySql(tx) => tx.rollback().await.map_err(|e| log_db("rollback", e)),
            #[cfg(feature = "mssql")]
            TxConn::SqlServer(tx) => tx.rollback().await,
        }
    }
}

impl Pool {
    pub async fn close(&self) {
        match self {
            #[cfg(feature = "postgres")]
            Pool::Postgres(p) => p.close().await,
            #[cfg(feature = "sqlite")]
            Pool::Sqlite(p) => p.close().await,
            #[cfg(feature = "mysql")]
            Pool::MySql(p) => p.close().await,
            #[cfg(feature = "mssql")]
            Pool::SqlServer(p) => p.close().await,
        }
    }

    /// Runs a query and reads its rows into a [`Table`] (columns typed even when there is no
    /// row), counting them in `progress` as they arrive.
    pub async fn query_table(&self, sql: &str, params: &[DbValue], timeout: Duration, progress: Option<&Progress>) -> Result<Table, DataError> {
        with_timeout("query", timeout, async {
            match self {
                #[cfg(feature = "postgres")]
                Pool::Postgres(pool) => {
                    let t = pg::read_table(bind_params!(sqlx::query(sql), params).fetch(pool), progress).await?;
                    pg::with_described_columns(pool, sql, t).await
                }
                #[cfg(feature = "sqlite")]
                Pool::Sqlite(pool) => {
                    let t = lite::read_table(bind_params!(sqlx::query(sql), params).fetch(pool), progress).await?;
                    lite::with_described_columns(pool, sql, t).await
                }
                #[cfg(feature = "mysql")]
                Pool::MySql(pool) => {
                    let t = my::read_table(bind_params!(sqlx::query(sql), params).fetch(pool), progress).await?;
                    my::with_described_columns(pool, sql, t).await
                }
                #[cfg(feature = "mssql")]
                Pool::SqlServer(pool) => pool.query_table(sql, params, progress).await,
            }
        })
        .await
    }

    /// Runs a statement; the number of affected rows.
    pub async fn execute(&self, sql: &str, params: &[DbValue], timeout: Duration) -> Result<u64, DataError> {
        with_timeout("execute", timeout, async {
            match self {
                #[cfg(feature = "postgres")]
                Pool::Postgres(pool) => bind_params!(sqlx::query(sql), params).execute(pool).await.map(|r| r.rows_affected()).map_err(|e| log_db("execute", e)),
                #[cfg(feature = "sqlite")]
                Pool::Sqlite(pool) => bind_params!(sqlx::query(sql), params).execute(pool).await.map(|r| r.rows_affected()).map_err(|e| log_db("execute", e)),
                #[cfg(feature = "mysql")]
                Pool::MySql(pool) => bind_params!(sqlx::query(sql), params).execute(pool).await.map(|r| r.rows_affected()).map_err(|e| log_db("execute", e)),
                #[cfg(feature = "mssql")]
                Pool::SqlServer(pool) => pool.execute(sql, params).await,
            }
        })
        .await
    }

    /// The first column of the first row (`Null` when there is none).
    pub async fn scalar(&self, sql: &str, params: &[DbValue], timeout: Duration) -> Result<DbValue, DataError> {
        let t = self.query_table(sql, params, timeout, None).await?;
        Ok(t.rows().first().and_then(|r| r.values.first().cloned()).unwrap_or(DbValue::Null))
    }

    /// Begins a transaction on a connection of the pool.
    pub async fn begin(&self) -> Result<TxConn, DataError> {
        match self {
            #[cfg(feature = "postgres")]
            Pool::Postgres(pool) => pool.begin().await.map(TxConn::Postgres).map_err(|e| log_db("begin transaction", e)),
            #[cfg(feature = "sqlite")]
            Pool::Sqlite(pool) => pool.begin().await.map(TxConn::Sqlite).map_err(|e| log_db("begin transaction", e)),
            #[cfg(feature = "mysql")]
            Pool::MySql(pool) => pool.begin().await.map(TxConn::MySql).map_err(|e| log_db("begin transaction", e)),
            #[cfg(feature = "mssql")]
            Pool::SqlServer(pool) => pool.begin().await.map(|t| TxConn::SqlServer(Box::new(t))),
        }
    }

    /// Runs `statements` in one transaction: all or nothing. Each statement's result per
    /// [`Expect`]; a statement that does not produce what it expects rolls everything back with a
    /// [`DataError::Concurrency`]. The generated keys replace the temporary keys of the rows that
    /// reference them (a master inserted with its details, DATA-3).
    pub async fn run_in_transaction(&self, statements: &[Statement], timeout: Duration) -> Result<TxOutcome, DataError> {
        let run = async {
            let mut tx = self.begin().await?;
            let mut key_map = Vec::new();
            match tx.run(statements, &mut key_map).await {
                Ok(returned) => {
                    tx.commit().await?;
                    Ok(TxOutcome { returned, key_map })
                }
                Err(e) => {
                    if let Err(rb) = tx.rollback().await {
                        tracing::warn!(target: "kubuno_desktop_data", error = %rb, "rollback failed (the connection drops the transaction)");
                    }
                    Err(e)
                }
            }
        };
        match tokio::time::timeout(timeout, run).await {
            Ok(r) => r,
            Err(_) => Err(logged("transaction", DataError::Database { message: format!("the transaction timed out after {} s and was rolled back", timeout.as_secs()), code: None })),
        }
    }

    /// The schema of `table` (`schema.table` allowed): nullability, lengths, keys, generated keys.
    pub async fn table_schema(&self, table: &str, timeout: Duration) -> Result<Vec<ColumnSchema>, DataError> {
        with_timeout("read table schema", timeout, async {
            match self {
                #[cfg(feature = "postgres")]
                Pool::Postgres(pool) => pg::table_schema(pool, table).await,
                #[cfg(feature = "sqlite")]
                Pool::Sqlite(pool) => lite::table_schema(pool, table).await,
                #[cfg(feature = "mysql")]
                Pool::MySql(pool) => my::table_schema(pool, table).await,
                #[cfg(feature = "mssql")]
                Pool::SqlServer(pool) => pool.table_schema(table).await,
            }
        })
        .await
    }
}

/// Builds a table's columns from the driver's column list.
fn columns_of<C: sqlx::Column>(cols: &[C], ty: impl Fn(&str) -> DbType) -> Vec<DataColumn> {
    use sqlx::TypeInfo;
    cols.iter().map(|c| DataColumn::new(c.name(), ty(c.type_info().name()))).collect()
}

/// Reads a row stream into a table: the columns from the first row (or, without rows, from the
/// statement's description, added by the caller), each row decoded by `decode`.
macro_rules! read_rows {
    ($stream:expr, $ty:path, $decode:expr, $progress:expr) => {{
        use futures::TryStreamExt;
        let mut stream = $stream;
        let progress: Option<&Progress> = $progress;
        let mut columns: Option<Vec<DataColumn>> = None;
        let mut rows: Vec<Vec<DbValue>> = Vec::new();
        while let Some(row) = stream.try_next().await.map_err(|e| log_db("query", e))? {
            use sqlx::Row;
            let cols = columns.get_or_insert_with(|| columns_of(row.columns(), $ty));
            let mut values = Vec::with_capacity(cols.len());
            for i in 0..cols.len() {
                values.push($decode(&row, i, cols)?);
            }
            rows.push(values);
            if let Some(p) = progress {
                p.add(1);
            }
        }
        let mut table = Table::new("", columns.unwrap_or_default());
        for values in rows {
            table.load_row(values);
        }
        Ok::<Table, DataError>(table)
    }};
}

#[cfg(feature = "postgres")]
mod pg {
    use super::*;
    use sqlx::postgres::{PgPool, PgRow};
    use sqlx::{Executor, Row, TypeInfo};

    pub(super) async fn read_table<'e>(stream: impl futures::Stream<Item = Result<PgRow, sqlx::Error>> + Send + 'e, progress: Option<&Progress>) -> Result<Table, DataError> {
        read_rows!(Box::pin(stream), DbType::postgres, decode, progress)
    }

    fn decode(row: &PgRow, i: usize, cols: &[DataColumn]) -> Result<DbValue, DataError> {
        value(row, i).map_err(|e| {
            let (name, ty) = cols.get(i).map(|c| (c.name.clone(), c.ty.native.clone())).unwrap_or_default();
            logged("read row", DataError::Validation(format!("column `{name}` of type {ty} cannot be read ({e}); cast it in the query, e.g. `{name}::text`")))
        })
    }

    /// The columns of a query without rows come from its description.
    pub(super) async fn with_described_columns(pool: &PgPool, sql: &str, table: Table) -> Result<Table, DataError> {
        if !table.columns.is_empty() {
            return Ok(table);
        }
        let stmt = pool.prepare(sql).await.map_err(|e| log_db("describe query", e))?;
        Ok(Table::new("", columns_of(sqlx::Statement::columns(&stmt), DbType::postgres)))
    }

    pub(super) fn row_values(row: &PgRow) -> Result<Vec<DbValue>, sqlx::Error> {
        (0..row.len()).map(|i| value(row, i)).collect()
    }

    fn value(row: &PgRow, i: usize) -> Result<DbValue, sqlx::Error> {
        use sqlx::Column;
        let ty = row.columns()[i].type_info().name().to_ascii_uppercase();
        Ok(match ty.as_str() {
            "BOOL" => row.try_get::<Option<bool>, _>(i)?.map_or(DbValue::Null, DbValue::Bool),
            "INT2" => row.try_get::<Option<i16>, _>(i)?.map_or(DbValue::Null, |v| DbValue::Int(i64::from(v))),
            "INT4" => row.try_get::<Option<i32>, _>(i)?.map_or(DbValue::Null, |v| DbValue::Int(i64::from(v))),
            "INT8" => row.try_get::<Option<i64>, _>(i)?.map_or(DbValue::Null, DbValue::Int),
            "FLOAT4" => row.try_get::<Option<f32>, _>(i)?.map_or(DbValue::Null, |v| DbValue::Float(f64::from(v))),
            "FLOAT8" => row.try_get::<Option<f64>, _>(i)?.map_or(DbValue::Null, DbValue::Float),
            "BYTEA" => row.try_get::<Option<Vec<u8>>, _>(i)?.map_or(DbValue::Null, DbValue::Bytes),
            "UUID" => row.try_get::<Option<uuid::Uuid>, _>(i)?.map_or(DbValue::Null, |u| DbValue::Text(u.hyphenated().to_string())),
            "TIMESTAMPTZ" => row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(i)?.map_or(DbValue::Null, |d| DbValue::Text(d.to_rfc3339())),
            "TIMESTAMP" => row.try_get::<Option<chrono::NaiveDateTime>, _>(i)?.map_or(DbValue::Null, |d| DbValue::Text(d.format("%Y-%m-%d %H:%M:%S%.f").to_string())),
            "DATE" => row.try_get::<Option<chrono::NaiveDate>, _>(i)?.map_or(DbValue::Null, |d| DbValue::Text(d.format("%Y-%m-%d").to_string())),
            "TIME" => row.try_get::<Option<chrono::NaiveTime>, _>(i)?.map_or(DbValue::Null, |d| DbValue::Text(d.format("%H:%M:%S%.f").to_string())),
            "JSON" | "JSONB" => row.try_get::<Option<serde_json::Value>, _>(i)?.map_or(DbValue::Null, |j| DbValue::Text(j.to_string())),
            _ => row.try_get::<Option<String>, _>(i)?.map_or(DbValue::Null, DbValue::Text),
        })
    }

    pub(super) async fn table_schema(pool: &PgPool, table: &str) -> Result<Vec<ColumnSchema>, DataError> {
        let (schema, name) = match table.split_once('.') {
            Some((s, n)) => (Some(s.to_string()), n.to_string()),
            None => (None, table.to_string()),
        };
        let sql = r#"
            SELECT c.column_name::text AS name,
                   c.udt_name::text AS native,
                   (c.is_nullable = 'YES') AS nullable,
                   c.character_maximum_length::int4 AS max_length,
                   (COALESCE(c.column_default, '') LIKE 'nextval(%' OR c.is_identity = 'YES') AS auto_increment,
                   EXISTS (
                       SELECT 1
                       FROM information_schema.table_constraints tc
                       JOIN information_schema.key_column_usage k
                         ON k.constraint_name = tc.constraint_name AND k.table_schema = tc.table_schema AND k.table_name = tc.table_name
                       WHERE tc.constraint_type = 'PRIMARY KEY'
                         AND tc.table_schema = c.table_schema AND tc.table_name = c.table_name
                         AND k.column_name = c.column_name
                   ) AS primary_key
            FROM information_schema.columns c
            WHERE c.table_schema = COALESCE($1, current_schema()) AND c.table_name = $2
            ORDER BY c.ordinal_position"#;
        let rows: Vec<PgRow> = sqlx::query(sql).bind(schema).bind(name).fetch_all(pool).await.map_err(|e| log_db("read table schema", e))?;
        rows.iter()
            .map(|r| {
                Ok(ColumnSchema {
                    name: r.try_get("name")?,
                    native: r.try_get("native")?,
                    nullable: r.try_get("nullable")?,
                    max_length: r.try_get::<Option<i32>, _>("max_length")?.and_then(|m| usize::try_from(m).ok()),
                    primary_key: r.try_get("primary_key")?,
                    auto_increment: r.try_get("auto_increment")?,
                    read_only: false,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(|e| log_db("read table schema", e))
    }

    /// Warns (once per pool) when the application connects as a superuser.
    pub(crate) async fn least_privilege_hint(pool: &PgPool, connection: &str) {
        match sqlx::query_scalar::<_, bool>("SELECT rolsuper FROM pg_roles WHERE rolname = current_user").fetch_optional(pool).await {
            Ok(Some(true)) => tracing::warn!(
                target: "kubuno_desktop_data",
                connection,
                "the application connects to PostgreSQL as a superuser: use a role with only the privileges it needs"
            ),
            Ok(_) => {}
            Err(e) => tracing::debug!(target: "kubuno_desktop_data", connection, error = %e, "could not check the role's privileges"),
        }
    }
}

#[cfg(feature = "postgres")]
pub(crate) use pg::least_privilege_hint;

#[cfg(feature = "sqlite")]
mod lite {
    use super::*;
    use sqlx::sqlite::{SqlitePool, SqliteRow};
    use sqlx::{Executor, Row, TypeInfo, ValueRef};

    pub(super) async fn read_table<'e>(stream: impl futures::Stream<Item = Result<SqliteRow, sqlx::Error>> + Send + 'e, progress: Option<&Progress>) -> Result<Table, DataError> {
        let mut table = read_rows!(Box::pin(stream), DbType::sqlite, decode, progress)?;
        // An expression column has no declared type: take the kind of its first value.
        for i in 0..table.columns.len() {
            if table.columns[i].ty.kind == DbKind::Other {
                if let Some(v) = table.rows().iter().map(|r| &r.values[i]).find(|v| !v.is_null()) {
                    table.columns[i].ty.kind = match v {
                        DbValue::Int(_) => DbKind::Int,
                        DbValue::Float(_) => DbKind::Float,
                        DbValue::Bytes(_) => DbKind::Bytes,
                        DbValue::Bool(_) => DbKind::Bool,
                        _ => DbKind::Text,
                    };
                }
            }
        }
        Ok(table)
    }

    fn decode(row: &SqliteRow, i: usize, cols: &[DataColumn]) -> Result<DbValue, DataError> {
        value(row, i, cols.get(i).map_or(DbKind::Other, |c| c.ty.kind)).map_err(|e| log_db("read row", e))
    }

    pub(super) async fn with_described_columns(pool: &SqlitePool, sql: &str, table: Table) -> Result<Table, DataError> {
        if !table.columns.is_empty() {
            return Ok(table);
        }
        let stmt = pool.prepare(sql).await.map_err(|e| log_db("describe query", e))?;
        Ok(Table::new("", columns_of(sqlx::Statement::columns(&stmt), DbType::sqlite)))
    }

    pub(super) fn row_values(row: &SqliteRow) -> Result<Vec<DbValue>, sqlx::Error> {
        (0..row.len()).map(|i| value(row, i, DbKind::Other)).collect()
    }

    fn value(row: &SqliteRow, i: usize, declared: DbKind) -> Result<DbValue, sqlx::Error> {
        let raw = row.try_get_raw(i)?;
        if raw.is_null() {
            return Ok(DbValue::Null);
        }
        let storage = raw.type_info().name().to_ascii_uppercase();
        let v = match storage.as_str() {
            "REAL" | "FLOAT" | "DOUBLE" => DbValue::Float(row.try_get_unchecked::<f64, _>(i)?),
            "TEXT" | "DATE" | "TIME" | "DATETIME" => DbValue::Text(row.try_get_unchecked::<String, _>(i)?),
            "BLOB" => DbValue::Bytes(row.try_get_unchecked::<Vec<u8>, _>(i)?),
            _ => DbValue::Int(row.try_get_unchecked::<i64, _>(i)?),
        };
        Ok(match (declared, v) {
            (DbKind::Bool, DbValue::Int(n)) => DbValue::Bool(n != 0),
            (DbKind::Float, DbValue::Int(n)) => DbValue::Float(n as f64),
            (_, v) => v,
        })
    }

    pub(super) async fn table_schema(pool: &SqlitePool, table: &str) -> Result<Vec<ColumnSchema>, DataError> {
        let (schema, name) = match table.split_once('.') {
            Some((s, n)) => (s.to_string(), n.to_string()),
            None => ("main".to_string(), table.to_string()),
        };
        let rows: Vec<SqliteRow> = sqlx::query(r#"SELECT name, type, "notnull", pk FROM pragma_table_info(?1, ?2) ORDER BY cid"#)
            .bind(name)
            .bind(schema)
            .fetch_all(pool)
            .await
            .map_err(|e| log_db("read table schema", e))?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let native: String = r.try_get("type").map_err(|e| log_db("read table schema", e))?;
            let pk: i64 = r.try_get("pk").map_err(|e| log_db("read table schema", e))?;
            let not_null: i64 = r.try_get("notnull").map_err(|e| log_db("read table schema", e))?;
            let max_length = native.split_once('(').and_then(|(_, rest)| rest.trim_end_matches(')').split(',').next()?.trim().parse::<usize>().ok());
            out.push(ColumnSchema {
                name: r.try_get("name").map_err(|e| log_db("read table schema", e))?,
                nullable: not_null == 0 && pk == 0,
                max_length,
                primary_key: pk > 0,
                auto_increment: false,
                read_only: false,
                native,
            });
        }
        // `INTEGER PRIMARY KEY` (the only key column) is the rowid: generated on insert.
        let keys: Vec<usize> = out.iter().enumerate().filter(|(_, c)| c.primary_key).map(|(i, _)| i).collect();
        if let [only] = keys.as_slice() {
            if out[*only].native.eq_ignore_ascii_case("INTEGER") {
                out[*only].auto_increment = true;
            }
        }
        Ok(out)
    }
}

#[cfg(feature = "mysql")]
mod my {
    use super::*;
    use sqlx::mysql::{MySqlPool, MySqlRow};
    use sqlx::{Executor, Row, TypeInfo, ValueRef};

    pub(super) async fn read_table<'e>(stream: impl futures::Stream<Item = Result<MySqlRow, sqlx::Error>> + Send + 'e, progress: Option<&Progress>) -> Result<Table, DataError> {
        read_rows!(Box::pin(stream), DbType::mysql, decode, progress)
    }

    fn decode(row: &MySqlRow, i: usize, _cols: &[DataColumn]) -> Result<DbValue, DataError> {
        value(row, i).map_err(|e| log_db("read row", e))
    }

    pub(super) async fn with_described_columns(pool: &MySqlPool, sql: &str, table: Table) -> Result<Table, DataError> {
        if !table.columns.is_empty() {
            return Ok(table);
        }
        let stmt = pool.prepare(sql).await.map_err(|e| log_db("describe query", e))?;
        Ok(Table::new("", columns_of(sqlx::Statement::columns(&stmt), DbType::mysql)))
    }

    pub(super) fn row_values(row: &MySqlRow) -> Result<Vec<DbValue>, sqlx::Error> {
        (0..row.len()).map(|i| value(row, i)).collect()
    }

    fn value(row: &MySqlRow, i: usize) -> Result<DbValue, sqlx::Error> {
        use sqlx::Column;
        if row.try_get_raw(i)?.is_null() {
            return Ok(DbValue::Null);
        }
        let ty = row.columns()[i].type_info().name().to_ascii_uppercase();
        let unsigned = ty.ends_with("UNSIGNED");
        Ok(match ty.split(' ').next().unwrap_or("") {
            "BOOLEAN" => DbValue::Bool(row.try_get::<bool, _>(i)?),
            "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "BIGINT" | "YEAR" if unsigned => {
                let v: u64 = row.try_get(i)?;
                i64::try_from(v).map_or_else(|_| DbValue::Text(v.to_string()), DbValue::Int)
            }
            "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "BIGINT" | "YEAR" => DbValue::Int(row.try_get::<i64, _>(i)?),
            "FLOAT" => DbValue::Float(f64::from(row.try_get::<f32, _>(i)?)),
            "DOUBLE" => DbValue::Float(row.try_get::<f64, _>(i)?),
            // DECIMAL travels as its exact text.
            "DECIMAL" => DbValue::Text(row.try_get_unchecked::<String, _>(i)?),
            "DATE" => DbValue::Text(row.try_get::<chrono::NaiveDate, _>(i)?.format("%Y-%m-%d").to_string()),
            "DATETIME" => DbValue::Text(row.try_get::<chrono::NaiveDateTime, _>(i)?.format("%Y-%m-%d %H:%M:%S%.f").to_string()),
            "TIMESTAMP" => DbValue::Text(row.try_get::<chrono::DateTime<chrono::Utc>, _>(i)?.to_rfc3339()),
            "TIME" => match row.try_get::<chrono::NaiveTime, _>(i) {
                Ok(t) => DbValue::Text(t.format("%H:%M:%S%.f").to_string()),
                Err(_) => DbValue::Text(row.try_get_unchecked::<String, _>(i)?),
            },
            "JSON" => DbValue::Text(row.try_get::<serde_json::Value, _>(i)?.to_string()),
            "BINARY" | "VARBINARY" | "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BIT" | "GEOMETRY" => DbValue::Bytes(row.try_get_unchecked::<Vec<u8>, _>(i)?),
            _ => DbValue::Text(row.try_get_unchecked::<String, _>(i)?),
        })
    }

    pub(super) async fn table_schema(pool: &MySqlPool, table: &str) -> Result<Vec<ColumnSchema>, DataError> {
        let (schema, name) = match table.split_once('.') {
            Some((s, n)) => (Some(s.to_string()), n.to_string()),
            None => (None, table.to_string()),
        };
        let sql = "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, CAST(CHARACTER_MAXIMUM_LENGTH AS SIGNED), COLUMN_KEY, EXTRA \
                   FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = COALESCE(?, DATABASE()) AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION";
        let rows: Vec<MySqlRow> = sqlx::query(sql).bind(schema).bind(name).fetch_all(pool).await.map_err(|e| log_db("read table schema", e))?;
        let text = |r: &MySqlRow, i: usize| -> Result<String, sqlx::Error> { Ok(r.try_get_unchecked::<Option<String>, _>(i)?.unwrap_or_default()) };
        rows.iter()
            .map(|r| {
                let extra = text(r, 5)?.to_ascii_lowercase();
                Ok(ColumnSchema {
                    name: text(r, 0)?,
                    native: text(r, 1)?,
                    nullable: text(r, 2)?.eq_ignore_ascii_case("YES"),
                    max_length: r.try_get::<Option<i64>, _>(3)?.and_then(|m| usize::try_from(m).ok()),
                    primary_key: text(r, 4)?.eq_ignore_ascii_case("PRI"),
                    auto_increment: extra.contains("auto_increment"),
                    read_only: extra.contains("virtual generated") || extra.contains("stored generated"),
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(|e| log_db("read table schema", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders() {
        assert_eq!(Provider::Postgres.placeholder(3), "$3");
        assert_eq!(Provider::Sqlite.placeholder(1), "?1");
        assert_eq!(Provider::MySql.placeholder(2), "?");
        assert_eq!(Provider::SqlServer.placeholder(2), "@P2");
        assert!(Provider::Sqlite.is_available());
        assert_eq!(Provider::SqlServer.is_available(), cfg!(feature = "mssql"));
        assert!(!Provider::MySql.numbered_placeholders() && !Provider::MySql.has_returning());
    }

    #[test]
    fn foreign_key_parameters_take_the_generated_keys() {
        let mut st = Statement::new("INSERT INTO orders (customer_id, item) VALUES (?1, ?2)".into(), vec![DbValue::Int(-1), DbValue::Int(-1)], Expect::One, "x".into());
        st.fk_params = vec![0];
        let map = vec![(DbValue::Int(-1), DbValue::Int(42))];
        assert_eq!(st.resolved_params(&map), vec![DbValue::Int(42), DbValue::Int(-1)], "only the foreign key is replaced");
    }
}
