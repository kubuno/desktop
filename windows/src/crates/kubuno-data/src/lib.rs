//! # `kubuno-data` — data access components for Kubuno desktop applications
//!
//! The runtime of `vskubuno/docs/DATA.md` (lots DATA-1 to DATA-3): ADO.NET/WinForms-style components,
//! declared in a view like any non-visual component (the designer lists them in its component tray)
//! and bound to controls through the `kubuno-views` binding engine.
//!
//! - [`DbConnection`]: provider (PostgreSQL, SQLite; MySQL/MariaDB with the `mysql` feature, SQL
//!   Server with `mssql`), connection string resolved from the secrets chain ([`secrets`]:
//!   environment, Windows Credential Manager, user secrets — never from the view), pooling, retry
//!   policy, TLS by default for remote servers, `StateChange`; [`ConnectionStringBuilder`] with
//!   redacted output.
//! - [`DbCommand`]: parameterized SQL (`@name`) or a stored procedure, execute / scalar / query.
//! - [`TableAdapter`]: `fill` (with paging) into a [`Table`] with row states, `update` through
//!   generated (or custom) parameterized DML in one transaction, optimistic concurrency.
//! - [`DbTransaction`]: several commands and saves in one transaction.
//! - [`BindingSource`]: position, current row, filter, sort, add/edit/cancel/end-edit, master/detail,
//!   paging, the WinForms events; [`ErrorProvider`]: the field errors of the current row, drawn as an
//!   error glyph next to the bound controls; [`BindingNavigator`]: the navigation tool strip.
//!
//! **In a view** (DATA-2) the view runtime owns the components: `{Binding Source=customers,
//! Path=Name, FormatString=…}` reaches them without any code in the view model, their `.kbview`
//! handlers run (synchronously for the cancelable `RowValidating`/`AddingNew`), and async handlers
//! call [`fill`], [`save`], [`save_all`]. Outside a view, a [`DataContext`] holds them.
//!
//! Every database operation runs on a private Tokio runtime ([`rt`]); the UI thread awaits a
//! [`DataTask`] from the EVT-6 executor and never blocks. Values are always parameters. Errors
//! ([`DataError`]) are logged with `tracing` before they are returned and never contain a secret.
//!
//! An application links the components' registrations (static constructors) with
//! `extern crate kubuno_data as _;` — the Kubuno templates add it.

pub mod adapter;
pub mod binding_source;
pub mod command;
pub mod conn_string;
pub mod connection;
pub mod context;
pub mod error;
pub mod error_provider;
pub mod events;
pub mod filter;
pub mod local_database;
#[cfg(feature = "mssql")]
mod mssql;
pub mod navigator;
pub mod ops;
pub mod provider;
pub mod raw;
pub mod rt;
pub mod secrets;
pub mod sql;
pub mod table;
pub mod transaction;
pub mod typed;
pub mod value;

pub use adapter::{ConflictOption, FilledPage, PagingMode, TableAdapter, UpdateOutcome, UpdatePlan};
pub use binding_source::{BindingSource, FillRequest, Relation};
pub use command::{CommandType, DbCommand};
pub use conn_string::ConnectionStringBuilder;
pub use connection::{ConnectionHandle, DbConnection, RetryPolicy};
pub use context::{fill_blocking, pump, save_all_blocking, save_blocking, DataContext, HasDataContext, PendingEvent};
pub use error::DataError;
pub use error_provider::ErrorProvider;
pub use local_database::LocalDatabase;
pub use events::{AddingNewEventArgs, ConnectionState, DataErrorEventArgs, ListChangedEventArgs, ListChangedType, RowValidatingEventArgs, StateChangeEventArgs};
pub use navigator::{BindingNavigator, NavigatorItem, NavigatorItemClickedEventArgs};
pub use ops::{fill, fill_scope, save, save_all, save_scope};
pub use provider::Provider;
pub use rt::{block_on, Canceller, DataTask, Progress};
pub use secrets::{set_user_secrets_id, AppSecretsSource, MigratingSource, SecretResolver, SecretSource, UserSecrets};
pub use table::{DataColumn, DataRow, RowState, Table};
pub use transaction::DbTransaction;
pub use typed::{FieldValue, TypedRow};
pub use value::{DbKind, DbType, DbValue};

/// The sqlx version this crate is built on (the typed code of [`data_source!`] reaches it here).
pub use sqlx;

#[cfg(feature = "macros")]
#[doc(hidden)]
pub use kubuno_data_macros::data_source as __data_source_impl;

/// Typed data source (DATA-4): `data_source!("shop.kbdata")` turns a `.kbdata` file (see
/// `kubuno_data_model::kbdata`) into typed row structs and data functions checked at compile time by
/// `sqlx` against the offline query cache `.sqlx`. Nothing is written into the project.
///
/// ```ignore
/// // src/data/mod.rs — the path is relative to this file (then to `src/`, then to the package).
/// kubuno::data::data_source!("shop.kbdata"); // or kubuno_data::data_source!
/// ```
///
/// **Generated**, for each table / view of the file (SQLite here; `sqlx::Postgres`/`MySql` alike):
///
/// ```ignore
/// #[derive(Debug, Clone, PartialEq, Default)] // PartialEq/Default only when every field type has them
/// pub struct Customer { pub id: i64, pub name: String, pub email: Option<String> }
/// impl Customer {
///     pub const TABLE: &str = "customers";
///     pub const COLUMNS: &[&str] = &["id", "name", "email"];
///     pub const KEY: &[&str] = &["id"];
///     pub async fn fetch_all<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(executor: E) -> Result<Vec<Self>, DataError>;
///     pub async fn fetch_by_key<'e, E>(executor: E, id: i64) -> Result<Option<Self>, DataError>;  // a key is declared
///     pub async fn insert<'e, E>(&self, executor: E) -> Result<Self, DataError>;     // RETURNING (MySQL: the id, u64)
///     pub async fn update<'e, E>(&self, executor: E) -> Result<u64, DataError>;      // tables with a key
///     pub async fn delete<'e, E>(&self, executor: E) -> Result<u64, DataError>;
///     pub async fn delete_by_key<'e, E>(executor: E, id: i64) -> Result<u64, DataError>;
///     // The same on the data runtime from a connection component (await it from the UI executor):
///     pub fn fetch_all_task(conn: &ConnectionHandle) -> DataTask<Vec<Self>>; // fetch_by_key_task, insert_task…
/// }
/// impl kubuno_data::TypedRow for Customer { … } // Table / BindingSource interop
/// // Each [[queries]]: a function (and its `_task`), rows of `row = "Customer"` or of its own
/// // `<Name>Row` struct (`columns`), or the affected rows when it returns none.
/// pub async fn customers_by_city<'e, E>(executor: E, city: String) -> Result<Vec<Customer>, DataError>;
/// pub fn customers_by_city_task(conn: &ConnectionHandle, city: String) -> DataTask<Vec<Customer>>;
/// ```
///
/// Executors are sqlx's: `&SqlitePool` (from [`ConnectionHandle::sqlite_pool`]), `&mut *transaction`…
/// The column types always match the file: every select forces them (`"id" AS "id!: i64"`). Errors are
/// logged (`tracing::error!`) and returned as [`DataError`]. SQL Server (no sqlx driver) gets the
/// structs, [`TypedRow`] and the `*_task` reads/queries through [`DbCommand`], checked at run time.
///
/// **The application's `Cargo.toml`** needs only `kubuno` with its `data` feature (or `kubuno-data`):
/// no `sqlx` dependency — the expansion goes through `kubuno_data::sqlx`. PostgreSQL / SQLite are the
/// defaults; `.kbdata` files for MySQL need kubuno-data's `mysql` feature.
///
/// **Offline cache.** Builds read `.sqlx/query-<sha256 of the SQL>.json` of the package (or of the
/// workspace root) when `DATABASE_URL` is unset or `SQLX_OFFLINE=true`; commit that folder. A
/// statement missing from it is a compile error naming the `.kbdata` and the statement. To
/// regenerate it without sqlx-cli, run a build online with the cache folder named:
/// `DATABASE_URL=<url> SQLX_OFFLINE=false SQLX_OFFLINE_DIR=<absolute package path>/.sqlx cargo check`
/// (the folder must exist; sqlx writes one file per statement it expands, it never deletes old ones;
/// changing `SQLX_OFFLINE`/`SQLX_OFFLINE_DIR` re-expands the macro). When the cache lacks a
/// statement of the file or is older than a migration, the build shows a warning
/// (`use of deprecated unit struct kubuno_data_stale_sqlx_cache: kubuno-data: …`).
#[cfg(feature = "macros")]
#[macro_export]
macro_rules! data_source {
    ($($input:tt)*) => {
        $crate::__data_source_impl! { krate = $crate; $($input)* }
    };
}
