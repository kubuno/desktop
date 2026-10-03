//! SQL Server through `tiberius` (feature `mssql`, `vskubuno/docs/DATA.md` lot DATA-3): pure Rust TDS,
//! TLS through SChannel, Windows integrated authentication (`Integrated Security=true`). tiberius has
//! no pool, so this module keeps one: idle clients reused, at most `MaxPoolSize` at a time, a client
//! whose operation failed or was cancelled is dropped (its connection closed, an open transaction
//! rolled back by the server) rather than returned.
//!
//! Differences from the other providers are confined here and in `provider.rs`: `@P1` placeholders,
//! `[bracketed]` names, generated keys and new row versions through `OUTPUT INSERTED.[column]`,
//! `BEGIN TRANSACTION` / `COMMIT` on one client.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::TryStreamExt;
use tiberius::{ColumnType, Config, Query, Row};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use crate::error::{logged, DataError};
use crate::provider::{ColumnSchema, Expect, Statement};
use crate::rt::Progress;
use crate::table::{DataColumn, Table};
use crate::value::{DbType, DbValue};

type Client = tiberius::Client<Compat<tokio::net::TcpStream>>;

/// Logs a tiberius error with its context and converts it (the server's message and number).
fn ms_err(context: &str, e: tiberius::error::Error) -> DataError {
    let err = match &e {
        tiberius::error::Error::Io { kind, message } => DataError::Database { message: format!("I/O error: {kind:?} {message}"), code: None },
        tiberius::error::Error::Tls(m) => DataError::Database { message: format!("TLS error: {m}"), code: None },
        other => DataError::Database { message: other.to_string(), code: other.code().map(|c| c.to_string()) },
    };
    logged(context, err)
}

struct Inner {
    config: Config,
    idle: Mutex<Vec<Client>>,
    permits: Arc<Semaphore>,
    connect_timeout: Duration,
    closed: AtomicBool,
}

/// A pool of SQL Server clients (see the module doc).
#[derive(Clone)]
pub(crate) struct MsPool {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for MsPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The configuration holds the credentials: only the address is shown.
        f.debug_struct("MsPool").field("server", &self.inner.config.get_addr()).finish()
    }
}

/// A client taken from the pool; returned by [`Lease::release`], dropped (closed) otherwise.
struct Lease {
    client: Client,
    _permit: OwnedSemaphorePermit,
    pool: MsPool,
}

impl Lease {
    async fn release(self) {
        if !self.pool.inner.closed.load(Ordering::Acquire) {
            self.pool.inner.idle.lock().await.push(self.client);
        }
    }
}

impl MsPool {
    /// A pool for `config`, opening one connection now (the retry policy retries this).
    pub async fn connect(config: Config, max: u32, connect_timeout: Duration) -> Result<Self, DataError> {
        let pool = Self {
            inner: Arc::new(Inner { config, idle: Mutex::new(Vec::new()), permits: Arc::new(Semaphore::new(max.max(1) as usize)), connect_timeout, closed: AtomicBool::new(false) }),
        };
        let lease = pool.acquire().await?;
        lease.release().await;
        Ok(pool)
    }

    async fn open(&self) -> Result<Client, DataError> {
        let config = self.inner.config.clone();
        let connect = async move {
            let tcp = tokio::net::TcpStream::connect(config.get_addr()).await.map_err(|e| logged("connect", DataError::Database { message: format!("I/O error: {e}"), code: None }))?;
            tcp.set_nodelay(true).map_err(|e| logged("connect", DataError::Database { message: format!("I/O error: {e}"), code: None }))?;
            tiberius::Client::connect(config, tcp.compat_write()).await.map_err(|e| ms_err("connect", e))
        };
        match tokio::time::timeout(self.inner.connect_timeout, connect).await {
            Ok(r) => r,
            Err(_) => Err(logged("connect", DataError::Database { message: "timed out waiting for a connection from the pool".to_string(), code: None })),
        }
    }

    async fn acquire(&self) -> Result<Lease, DataError> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(logged("connect", DataError::Database { message: "the connection pool is closed".to_string(), code: None }));
        }
        let permit = match tokio::time::timeout(self.inner.connect_timeout, self.inner.permits.clone().acquire_owned()).await {
            Ok(Ok(p)) => p,
            _ => return Err(logged("connect", DataError::Database { message: "timed out waiting for a connection from the pool".to_string(), code: None })),
        };
        let idle = self.inner.idle.lock().await.pop();
        let client = match idle {
            Some(c) => c,
            None => self.open().await?,
        };
        Ok(Lease { client, _permit: permit, pool: self.clone() })
    }

    pub async fn close(&self) {
        self.inner.closed.store(true, Ordering::Release);
        let clients = std::mem::take(&mut *self.inner.idle.lock().await);
        for c in clients {
            let _ = c.close().await;
        }
    }

    pub async fn query_table(&self, sql: &str, params: &[DbValue], progress: Option<&Progress>) -> Result<Table, DataError> {
        let mut lease = self.acquire().await?;
        let table = read(&mut lease.client, sql, params, progress).await?;
        lease.release().await;
        Ok(table)
    }

    pub async fn execute(&self, sql: &str, params: &[DbValue]) -> Result<u64, DataError> {
        let mut lease = self.acquire().await?;
        let n = execute(&mut lease.client, sql, params).await?;
        lease.release().await;
        Ok(n)
    }

    pub async fn begin(&self) -> Result<MsTransaction, DataError> {
        let mut lease = self.acquire().await?;
        lease.client.simple_query("BEGIN TRANSACTION").await.map_err(|e| ms_err("begin transaction", e))?.into_results().await.map_err(|e| ms_err("begin transaction", e))?;
        Ok(MsTransaction { lease: Some(lease) })
    }

    pub async fn table_schema(&self, table: &str) -> Result<Vec<ColumnSchema>, DataError> {
        let (schema, name) = match table.split_once('.') {
            Some((s, n)) => (Some(s.to_string()), n.to_string()),
            None => (None, table.to_string()),
        };
        let sql = "SELECT c.COLUMN_NAME, c.DATA_TYPE, c.IS_NULLABLE, c.CHARACTER_MAXIMUM_LENGTH, \
                   COLUMNPROPERTY(OBJECT_ID(QUOTENAME(c.TABLE_SCHEMA) + '.' + QUOTENAME(c.TABLE_NAME)), c.COLUMN_NAME, 'IsIdentity') AS is_identity, \
                   COLUMNPROPERTY(OBJECT_ID(QUOTENAME(c.TABLE_SCHEMA) + '.' + QUOTENAME(c.TABLE_NAME)), c.COLUMN_NAME, 'IsComputed') AS is_computed, \
                   CASE WHEN EXISTS (SELECT 1 FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc JOIN INFORMATION_SCHEMA.KEY_COLUMN_USAGE k \
                        ON k.CONSTRAINT_NAME = tc.CONSTRAINT_NAME AND k.TABLE_SCHEMA = tc.TABLE_SCHEMA AND k.TABLE_NAME = tc.TABLE_NAME \
                        WHERE tc.CONSTRAINT_TYPE = 'PRIMARY KEY' AND tc.TABLE_SCHEMA = c.TABLE_SCHEMA AND tc.TABLE_NAME = c.TABLE_NAME AND k.COLUMN_NAME = c.COLUMN_NAME) \
                   THEN 1 ELSE 0 END AS is_pk \
                   FROM INFORMATION_SCHEMA.COLUMNS c WHERE c.TABLE_SCHEMA = COALESCE(@P1, SCHEMA_NAME()) AND c.TABLE_NAME = @P2 ORDER BY c.ORDINAL_POSITION";
        let params = [schema.map_or(DbValue::Null, DbValue::Text), DbValue::Text(name)];
        let t = self.query_table(sql, &params, None).await?;
        let int = |v: &DbValue| match v {
            DbValue::Int(i) => *i,
            _ => 0,
        };
        Ok(t.rows()
            .iter()
            .map(|r| {
                let native = r.values[1].to_display();
                let read_only = int(&r.values[5]) == 1 || matches!(native.to_ascii_lowercase().as_str(), "timestamp" | "rowversion");
                ColumnSchema {
                    name: r.values[0].to_display(),
                    nullable: r.values[2].to_display().eq_ignore_ascii_case("YES"),
                    max_length: usize::try_from(int(&r.values[3])).ok().filter(|m| *m > 0),
                    auto_increment: int(&r.values[4]) == 1,
                    primary_key: int(&r.values[6]) == 1,
                    read_only,
                    native,
                }
            })
            .collect())
    }
}

/// A transaction on one client (`BEGIN TRANSACTION` … `COMMIT`). Dropped without a commit, its
/// client is closed and the server rolls the transaction back.
pub(crate) struct MsTransaction {
    lease: Option<Lease>,
}

impl MsTransaction {
    fn client(&mut self) -> Result<&mut Client, DataError> {
        self.lease.as_mut().map(|l| &mut l.client).ok_or_else(|| DataError::Database { message: "the transaction is finished".to_string(), code: None })
    }

    pub async fn run(&mut self, statements: &[Statement], key_map: &mut Vec<(DbValue, DbValue)>) -> Result<Vec<Option<Vec<DbValue>>>, DataError> {
        let client = self.client()?;
        let mut out = Vec::with_capacity(statements.len());
        for st in statements {
            let params = st.resolved_params(key_map);
            let result = match st.expect {
                Expect::Returning => {
                    let t = read(client, &st.sql, &params, None).await?;
                    match t.rows().first().map(|r| r.values.clone()) {
                        Some(v) => Some(v),
                        None => return Err(logged("transaction", DataError::Concurrency(format!("{} matched no row: the row was changed or deleted by someone else", st.what)))),
                    }
                }
                Expect::One | Expect::LastInsertId => {
                    let n = execute(client, &st.sql, &params).await?;
                    if n != 1 {
                        return Err(logged("transaction", DataError::Concurrency(format!("{} affected {n} rows instead of 1: the row was changed or deleted by someone else", st.what))));
                    }
                    None
                }
            };
            if let (Some(temp), Some(real)) = (&st.temp_key, result.as_ref().and_then(|r| r.first())) {
                key_map.push((temp.clone(), real.clone()));
            }
            out.push(result);
        }
        Ok(out)
    }

    pub async fn query_table(&mut self, sql: &str, params: &[DbValue]) -> Result<Table, DataError> {
        read(self.client()?, sql, params, None).await
    }

    pub async fn execute(&mut self, sql: &str, params: &[DbValue]) -> Result<u64, DataError> {
        execute(self.client()?, sql, params).await
    }

    async fn finish(mut self, sql: &str) -> Result<(), DataError> {
        let Some(mut lease) = self.lease.take() else { return Ok(()) };
        let r = lease.client.simple_query(sql).await.map_err(|e| ms_err(sql, e))?.into_results().await.map_err(|e| ms_err(sql, e));
        if r.is_ok() {
            lease.release().await;
        }
        r.map(|_| ())
    }

    pub async fn commit(self) -> Result<(), DataError> {
        self.finish("COMMIT TRANSACTION").await
    }

    pub async fn rollback(self) -> Result<(), DataError> {
        self.finish("ROLLBACK TRANSACTION").await
    }
}

/// A tiberius query with `params` bound in order.
fn query_of<'a>(sql: &'a str, params: &'a [DbValue]) -> Query<'a> {
    let mut q = Query::new(sql);
    for p in params {
        match p {
            DbValue::Null => q.bind(Option::<String>::None),
            DbValue::Bool(b) => q.bind(*b),
            DbValue::Int(i) => q.bind(*i),
            DbValue::Float(f) => q.bind(*f),
            DbValue::Text(s) => q.bind(s.as_str()),
            DbValue::Bytes(b) => q.bind(b.as_slice()),
        }
    }
    q
}

async fn execute(client: &mut Client, sql: &str, params: &[DbValue]) -> Result<u64, DataError> {
    let r = query_of(sql, params).execute(client).await.map_err(|e| ms_err("execute", e))?;
    Ok(r.total())
}

/// The type name of a column, for [`DbType::mssql`].
fn type_name(t: ColumnType) -> &'static str {
    match t {
        ColumnType::Bit | ColumnType::Bitn => "bit",
        ColumnType::Int1 => "tinyint",
        ColumnType::Int2 => "smallint",
        ColumnType::Int4 | ColumnType::Intn => "int",
        ColumnType::Int8 => "bigint",
        ColumnType::Float4 => "real",
        ColumnType::Float8 | ColumnType::Floatn => "float",
        ColumnType::Money | ColumnType::Money4 => "money",
        ColumnType::Decimaln | ColumnType::Numericn => "decimal",
        ColumnType::Guid => "uniqueidentifier",
        ColumnType::Daten => "date",
        ColumnType::Timen => "time",
        ColumnType::Datetime | ColumnType::Datetime4 | ColumnType::Datetimen => "datetime",
        ColumnType::Datetime2 => "datetime2",
        ColumnType::DatetimeOffsetn => "datetimeoffset",
        ColumnType::BigVarBin | ColumnType::BigBinary | ColumnType::Image => "varbinary",
        ColumnType::Xml => "xml",
        ColumnType::NVarchar | ColumnType::NChar | ColumnType::NText => "nvarchar",
        ColumnType::BigVarChar | ColumnType::BigChar | ColumnType::Text => "varchar",
        _ => "sql_variant",
    }
}

async fn read(client: &mut Client, sql: &str, params: &[DbValue], progress: Option<&Progress>) -> Result<Table, DataError> {
    let mut stream = query_of(sql, params).query(client).await.map_err(|e| ms_err("query", e))?;
    let columns: Vec<(String, ColumnType)> = match stream.columns().await.map_err(|e| ms_err("query", e))? {
        Some(cols) => cols.iter().map(|c| (c.name().to_string(), c.column_type())).collect(),
        None => Vec::new(),
    };
    let mut table = Table::new("", columns.iter().map(|(n, t)| DataColumn::new(n.clone(), DbType::mssql(type_name(*t)))).collect());
    let mut rows = stream.into_row_stream();
    while let Some(row) = rows.try_next().await.map_err(|e| ms_err("query", e))? {
        let mut values = Vec::with_capacity(columns.len());
        for (i, (_, t)) in columns.iter().enumerate() {
            values.push(value(&row, i, *t).map_err(|e| ms_err("read row", e))?);
        }
        table.load_row(values);
        if let Some(p) = progress {
            p.add(1);
        }
    }
    Ok(table)
}

fn value(row: &Row, i: usize, t: ColumnType) -> Result<DbValue, tiberius::error::Error> {
    fn int<'a, T: tiberius::FromSql<'a> + Into<i64>>(row: &'a Row, i: usize) -> Result<Option<i64>, tiberius::error::Error> {
        Ok(row.try_get::<T, usize>(i)?.map(Into::into))
    }
    let text = |s: Option<String>| s.map_or(DbValue::Null, DbValue::Text);
    Ok(match t {
        ColumnType::Bit | ColumnType::Bitn => row.try_get::<bool, usize>(i)?.map_or(DbValue::Null, DbValue::Bool),
        ColumnType::Int1 | ColumnType::Int2 | ColumnType::Int4 | ColumnType::Int8 | ColumnType::Intn => {
            let v = int::<i64>(row, i).or_else(|_| int::<i32>(row, i)).or_else(|_| int::<i16>(row, i)).or_else(|_| int::<u8>(row, i))?;
            v.map_or(DbValue::Null, DbValue::Int)
        }
        ColumnType::Float4 | ColumnType::Float8 | ColumnType::Floatn | ColumnType::Money | ColumnType::Money4 => {
            let v = row.try_get::<f64, usize>(i).or_else(|_| row.try_get::<f32, usize>(i).map(|f| f.map(f64::from)))?;
            v.map_or(DbValue::Null, DbValue::Float)
        }
        ColumnType::Decimaln | ColumnType::Numericn => text(row.try_get::<tiberius::numeric::Numeric, usize>(i)?.map(|n| n.to_string())),
        ColumnType::Guid => text(row.try_get::<tiberius::Uuid, usize>(i)?.map(|u| u.hyphenated().to_string())),
        ColumnType::Daten => text(row.try_get::<chrono::NaiveDate, usize>(i)?.map(|d| d.format("%Y-%m-%d").to_string())),
        ColumnType::Timen => text(row.try_get::<chrono::NaiveTime, usize>(i)?.map(|d| d.format("%H:%M:%S%.f").to_string())),
        ColumnType::Datetime | ColumnType::Datetime4 | ColumnType::Datetimen | ColumnType::Datetime2 => {
            text(row.try_get::<chrono::NaiveDateTime, usize>(i)?.map(|d| d.format("%Y-%m-%d %H:%M:%S%.f").to_string()))
        }
        ColumnType::DatetimeOffsetn => text(row.try_get::<chrono::DateTime<chrono::FixedOffset>, usize>(i)?.map(|d| d.to_rfc3339())),
        ColumnType::BigVarBin | ColumnType::BigBinary | ColumnType::Image => row.try_get::<&[u8], usize>(i)?.map_or(DbValue::Null, |b| DbValue::Bytes(b.to_vec())),
        ColumnType::Xml => text(row.try_get::<&tiberius::xml::XmlData, usize>(i)?.map(|x| x.to_string())),
        ColumnType::Null => DbValue::Null,
        _ => text(row.try_get::<&str, usize>(i)?.map(str::to_string)),
    })
}

/// Whether a SQL Server address is this machine (`localhost`, `.`, `(local)`, a loopback address;
/// `tcp:` prefixes, ports and instance names ignored).
pub(crate) fn is_local_server(server: &str) -> bool {
    let s = server.trim().trim_start_matches("tcp:").trim_start_matches("np:");
    let host = s.split([',', '\\', ':']).next().unwrap_or("").trim().to_ascii_lowercase();
    matches!(host.as_str(), "" | "." | "(local)" | "localhost" | "::1" | "(localdb)") || host.starts_with("127.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_servers() {
        assert!(is_local_server("tcp:localhost,1433"));
        assert!(is_local_server(".\\SQLEXPRESS"));
        assert!(is_local_server("(local)"));
        assert!(is_local_server("127.0.0.1"));
        assert!(!is_local_server("db.example.org,1433"));
        assert_eq!(type_name(ColumnType::NVarchar), "nvarchar");
        assert_eq!(DbType::mssql(type_name(ColumnType::Datetime2)).kind, crate::value::DbKind::DateTime);
    }
}
