//! # `kubuno-desktop-data-tool` — the helper process of the Visual Studio data tooling
//!
//! `vskubuno/docs/DATA.md` §9: a tool window and wizards backed by a small Rust process speaking
//! JSON lines over stdio, so schema introspection, connection tests, queries and migrations use
//! exactly the drivers, pools and secrets chain of the `kubuno-desktop-data` runtime.
//!
//! - [`server`]: the protocol (`ping`, `cancel`, concurrency, one output writer) and the dispatch.
//! - [`targets`]: a connection target (Data Explorer entry, project connection, inline string)
//!   resolved to a `kubuno-desktop-data` connection; [`explorer`]: the Data Explorer list and its stores.
//! - [`schema`]: introspection per provider; [`query`], [`scripts`], [`kbdata`]: what is built on it.
//! - [`migrate`], [`sqlxcmd`]: sqlx migrations and the offline query cache.
//!
//! Secrets never leave the process: connection strings live in memory only, every message is
//! scrubbed of them ([`error::Redactor`]) and nothing logs them.

pub mod connstr;
pub mod ctx;
pub mod error;
pub mod explorer;
pub mod home;
pub mod kbdata;
pub mod migrate;
pub mod params;
pub mod query;
pub mod rows;
pub mod schema;
pub mod scripts;
pub mod server;
pub mod split;
pub mod sqlxcmd;
pub mod targets;
