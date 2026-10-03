//! [`DbTransaction`] (`vskubuno/docs/DATA.md` lot DATA-3, ADO.NET `DbTransaction` / `TransactionScope`):
//! several commands and the saves of several adapters in **one** database transaction, committed
//! or rolled back together.
//!
//! ```ignore
//! let tx = DbTransaction::begin(&db).await?;
//! tx.execute(&archive_command).await?;
//! tx.update(&customers_plan).await?;   // a master's inserts…
//! tx.update(&orders_plan).await?;      // …then its details': the generated keys replace the temporary ones
//! tx.commit().await?;                  // dropped without a commit: rolled back
//! ```
//!
//! Everything runs on the data runtime; the transaction holds one pooled connection until it ends.
//! For the binding sources of a view, `crate::save_all` does the same in one call.

use std::sync::Arc;

use crate::adapter::{UpdateOutcome, UpdatePlan};
use crate::command::DbCommand;
use crate::connection::ConnectionHandle;
use crate::error::{logged, DataError};
use crate::provider::TxConn;
use crate::rt::{self, DataTask};
use crate::table::Table;
use crate::value::DbValue;

struct State {
    tx: Option<TxConn>,
    /// Temporary keys replaced so far (a master inserted earlier in the transaction).
    key_map: Vec<(DbValue, DbValue)>,
}

/// An open transaction (see the module doc). Cloning shares it.
#[derive(Clone)]
pub struct DbTransaction {
    conn: ConnectionHandle,
    state: Arc<tokio::sync::Mutex<State>>,
}

impl std::fmt::Debug for DbTransaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbTransaction").field("connection", &self.conn.name()).finish()
    }
}

fn finished() -> DataError {
    DataError::Validation("the transaction is already committed or rolled back".to_string())
}

impl DbTransaction {
    /// Begins a transaction on a connection of `conn`'s pool.
    pub fn begin(conn: &ConnectionHandle) -> DataTask<DbTransaction> {
        let conn = conn.clone();
        rt::spawn(async move {
            let pool = conn.pool().await?;
            let tx = pool.begin().await?;
            Ok(DbTransaction { conn, state: Arc::new(tokio::sync::Mutex::new(State { tx: Some(tx), key_map: Vec::new() })) })
        })
    }

    /// Runs `command` inside the transaction; the affected rows.
    pub fn execute(&self, command: &DbCommand) -> DataTask<u64> {
        let (sql, params) = match command.prepare_for(self.conn.provider(), false) {
            Ok(p) => p,
            Err(e) => return DataTask::failed(logged("transaction", e)),
        };
        let (state, timeout) = (self.state.clone(), self.conn.command_timeout());
        rt::spawn(async move {
            let mut s = state.lock().await;
            let tx = s.tx.as_mut().ok_or_else(finished)?;
            crate::provider::with_timeout("execute", timeout, tx.execute(&sql, &params)).await
        })
    }

    /// Runs `command` inside the transaction; its rows.
    pub fn query(&self, command: &DbCommand) -> DataTask<Table> {
        let (sql, params) = match command.prepare_for(self.conn.provider(), true) {
            Ok(p) => p,
            Err(e) => return DataTask::failed(logged("transaction", e)),
        };
        let (state, timeout) = (self.state.clone(), self.conn.command_timeout());
        rt::spawn(async move {
            let mut s = state.lock().await;
            let tx = s.tx.as_mut().ok_or_else(finished)?;
            crate::provider::with_timeout("query", timeout, tx.query_table(&sql, &params)).await
        })
    }

    /// Runs an adapter's update plan inside the transaction. The generated keys of masters saved
    /// earlier in the same transaction replace their temporary keys in the plan's foreign keys.
    /// Apply the outcome to the table (`Table::apply_update`) only after the commit.
    pub fn update(&self, plan: &UpdatePlan) -> DataTask<UpdateOutcome> {
        let (statements, state, timeout) = (plan.statements.clone(), self.state.clone(), self.conn.command_timeout());
        rt::spawn(async move {
            let mut s = state.lock().await;
            let State { tx, key_map } = &mut *s;
            let tx = tx.as_mut().ok_or_else(finished)?;
            let returned = crate::provider::with_timeout("update", timeout, tx.run(&statements, key_map)).await?;
            Ok(UpdateOutcome { returned, key_map: key_map.clone() })
        })
    }

    /// Commits the transaction.
    pub fn commit(&self) -> DataTask<()> {
        let state = self.state.clone();
        rt::spawn(async move {
            let tx = state.lock().await.tx.take().ok_or_else(finished)?;
            tx.commit().await?;
            tracing::info!(target: "kubuno_desktop_data", "transaction committed");
            Ok(())
        })
    }

    /// Rolls the transaction back (also what dropping it without a commit does).
    pub fn rollback(&self) -> DataTask<()> {
        let state = self.state.clone();
        rt::spawn(async move {
            let tx = state.lock().await.tx.take().ok_or_else(finished)?;
            tx.rollback().await
        })
    }
}
