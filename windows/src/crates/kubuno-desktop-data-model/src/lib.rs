//! # `kubuno-desktop-data-model` — the `.kbdata` data-source description
//!
//! A `.kbdata` file (TOML) is the developer-owned description of a *typed data source*
//! (`vskubuno/docs/DATA.md`, DATA-4): the connection it uses (by name, never a connection string),
//! the provider, the schema, the tables and views with their columns and keys, and named queries.
//! Visual Studio's "Ajouter une source de données…" wizard writes it; `kubuno_desktop_data::data_source!`
//! turns it into typed row structs and `sqlx::query_as!` calls at compile time; nothing generated
//! is ever written into the project.
//!
//! This crate holds the format ([`DataSource`]), the provider names ([`ProviderName`]), the names
//! derived from database names ([`naming`]) and the mapping from a column's native type to the Rust
//! type `sqlx` decodes it into ([`rust_type_for`]).

pub mod cache;
pub mod kbdata;
pub mod naming;
pub mod sql;
pub mod typed;
pub mod types;

pub use kbdata::{Column, DataSource, KbdataError, ObjectKind, Param, Query, TableSource, FORMAT_VERSION};
pub use typed::{plan, FieldPlan, QueryPlan, QueryResult, RowPlan, Statement, TypedPlan};
pub use types::{rust_type_for, ProviderName, RustType};
