//! Raw access for tools (the Visual Studio helper `kubuno-data-tool`, `vskubuno/docs/DATA.md` §9):
//! SQL text run exactly as written — no `@name` rewriting, so a SQL Server `DECLARE @id INT` or a
//! user's query window text reaches the server untouched — and the native `sqlx` pool behind a
//! connection, for what only `sqlx` does (its migrator).
//!
//! Applications use [`crate::DbCommand`]; nothing here builds SQL from values.

use std::time::Duration;

use crate::connection::ConnectionHandle;
#[cfg(feature = "mssql")]
use crate::error::DataError;
use crate::provider::Pool;
use crate::rt::{self, DataTask};
use crate::table::Table;

/// The `sqlx` pool of a connection (SQL Server has none: it goes through `tiberius`).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum SqlxPool {
    #[cfg(feature = "postgres")]
    Postgres(sqlx::PgPool),
    #[cfg(feature = "sqlite")]
    Sqlite(sqlx::SqlitePool),
    #[cfg(feature = "mysql")]
    MySql(sqlx::MySqlPool),
}

impl ConnectionHandle {
    /// Runs `sql` verbatim (no parameters, no rewriting) and reads its rows; `timeout` defaults to
    /// the connection's command timeout. A statement that returns no columns gives a table
    /// without columns.
    pub fn query_raw(&self, sql: &str, timeout: Option<Duration>) -> DataTask<Table> {
        let (conn, sql) = (self.clone(), sql.to_string());
        let timeout = timeout.unwrap_or_else(|| self.command_timeout());
        rt::spawn(async move {
            let pool = conn.pool().await?;
            pool.query_table(&sql, &[], timeout, None).await
        })
    }

    /// Runs `sql` verbatim and returns the affected rows (see [`Self::query_raw`]).
    pub fn execute_raw(&self, sql: &str, timeout: Option<Duration>) -> DataTask<u64> {
        let (conn, sql) = (self.clone(), sql.to_string());
        let timeout = timeout.unwrap_or_else(|| self.command_timeout());
        rt::spawn(async move {
            let pool = conn.pool().await?;
            pool.execute(&sql, &[], timeout).await
        })
    }

    /// The native `sqlx` pool (opening the connection if needed). SQL Server: a `Config` error.
    pub fn native_pool(&self) -> DataTask<SqlxPool> {
        let conn = self.clone();
        rt::spawn(async move {
            match conn.pool().await? {
                #[cfg(feature = "postgres")]
                Pool::Postgres(p) => Ok(SqlxPool::Postgres(p)),
                #[cfg(feature = "sqlite")]
                Pool::Sqlite(p) => Ok(SqlxPool::Sqlite(p)),
                #[cfg(feature = "mysql")]
                Pool::MySql(p) => Ok(SqlxPool::MySql(p)),
                #[cfg(feature = "mssql")]
                Pool::SqlServer(_) => Err(DataError::Config("this connection has no sqlx pool (SQL Server uses tiberius)".to_string())),
            }
        })
    }
}
