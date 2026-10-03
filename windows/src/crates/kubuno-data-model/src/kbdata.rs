//! The `.kbdata` format (see the crate doc).
//!
//! ```toml
//! version = 1
//! name = "Shop"                # the data source
//! connection = "Shop"          # ConnectionStrings:Shop, resolved through the secrets chain
//! provider = "sqlite"          # sqlite | postgres | mysql | sqlserver
//! schema = "shop"              # optional: PostgreSQL schema / MySQL database (a Kubuno module's own)
//!
//! [[tables]]
//! name = "customers"
//! kind = "table"               # table | view
//! row = "Customer"             # optional: the row struct (default: singular PascalCase of name)
//! key = ["id"]
//!
//! [[tables.columns]]
//! name = "id"
//! db_type = "INTEGER"
//! rust_type = "i64"
//! nullable = false
//! auto_increment = true
//!
//! [[queries]]
//! name = "customers_by_city"   # the generated function
//! sql = "SELECT id, name FROM customers WHERE city = @city"
//! row = "Customer"             # optional: an existing row struct of this source
//!
//! [[queries.params]]
//! name = "city"
//! rust_type = "String"
//! ```

use serde::{Deserialize, Serialize};

use crate::types::ProviderName;

/// The version this crate reads and writes.
pub const FORMAT_VERSION: u32 = 1;

/// Why a `.kbdata` text could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KbdataError {
    pub message: String,
}

impl std::fmt::Display for KbdataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for KbdataError {}

/// A typed data source (one `.kbdata` file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataSource {
    #[serde(default = "default_version")]
    pub version: u32,
    /// The data source's name.
    pub name: String,
    /// The connection string's *name*: `ConnectionStrings:<connection>` in the secrets chain.
    pub connection: String,
    pub provider: ProviderName,
    /// PostgreSQL schema / MySQL database; empty = the connection's default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub schema: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<TableSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queries: Vec<Query>,
}

fn default_version() -> u32 {
    FORMAT_VERSION
}

/// Table or view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectKind {
    #[default]
    Table,
    View,
}

/// A table or a view of the data source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableSource {
    pub name: String,
    #[serde(default)]
    pub kind: ObjectKind,
    /// The row struct's name; empty = [`crate::naming::row_struct_name`] of `name`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub row: String,
    /// Primary-key columns (empty for a view or a table without a key: no update/delete functions).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key: Vec<String>,
    #[serde(default)]
    pub columns: Vec<Column>,
}

impl TableSource {
    /// The row struct's name (explicit `row`, else derived from the table name).
    pub fn row_name(&self) -> String {
        if self.row.is_empty() {
            crate::naming::row_struct_name(&self.name)
        } else {
            self.row.clone()
        }
    }
}

/// A column of a table or view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Column {
    pub name: String,
    /// The native type as the database names it (`INTEGER`, `int4`, `varchar`…).
    pub db_type: String,
    /// The Rust type of the field, without `Option` (nullability is `nullable`).
    pub rust_type: String,
    #[serde(default)]
    pub nullable: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto_increment: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub read_only: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_length: u32,
}

/// A named query of the data source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    /// The generated function's name (snake_case).
    pub name: String,
    /// The query text, with `@name` parameters (never values).
    pub sql: String,
    /// An existing row struct of this source the query returns; empty = a row type of its own
    /// (`<Name>Row`, from `columns`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub row: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<Param>,
    /// The result columns when `row` is empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<Column>,
}

/// A parameter of a named query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Param {
    pub name: String,
    pub rust_type: String,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl DataSource {
    /// Parses a `.kbdata` text.
    pub fn parse(text: &str) -> Result<Self, KbdataError> {
        let source: DataSource = toml::from_str(text).map_err(|e| KbdataError { message: e.to_string() })?;
        if source.version > FORMAT_VERSION {
            return Err(KbdataError {
                message: format!("unsupported .kbdata version {} (this build reads version {FORMAT_VERSION})", source.version),
            });
        }
        Ok(source)
    }

    /// Writes the data source as a `.kbdata` text, with a header comment.
    pub fn to_toml(&self) -> Result<String, KbdataError> {
        let body = toml::to_string(self).map_err(|e| KbdataError { message: e.to_string() })?;
        Ok(format!(
            "# Typed data source \"{}\" (vskubuno docs/DATA.md, DATA-4). This file is yours: edit it freely.\n\
             # `data_source!` turns it into typed rows at compile time; the connection string is never\n\
             # here: `connection` names it in the user secrets / Windows Credential Manager.\n\n{body}",
            self.name
        ))
    }

    /// Finds a table or view by name.
    pub fn table(&self, name: &str) -> Option<&TableSource> {
        self.tables.iter().find(|t| t.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DataSource {
        DataSource {
            version: 1,
            name: "Shop".into(),
            connection: "Shop".into(),
            provider: ProviderName::Sqlite,
            schema: String::new(),
            tables: vec![TableSource {
                name: "customers".into(),
                kind: ObjectKind::Table,
                row: String::new(),
                key: vec!["id".into()],
                columns: vec![
                    Column { name: "id".into(), db_type: "INTEGER".into(), rust_type: "i64".into(), nullable: false, auto_increment: true, read_only: false, max_length: 0 },
                    Column { name: "email".into(), db_type: "TEXT".into(), rust_type: "String".into(), nullable: true, auto_increment: false, read_only: false, max_length: 0 },
                ],
            }],
            queries: vec![Query {
                name: "by_email".into(),
                sql: "SELECT * FROM customers WHERE email = @email".into(),
                row: "Customer".into(),
                params: vec![Param { name: "email".into(), rust_type: "String".into() }],
                columns: vec![],
            }],
        }
    }

    #[test]
    fn round_trips() {
        let text = sample().to_toml().expect("serialises");
        assert!(text.starts_with("# Typed data source \"Shop\""));
        let back = DataSource::parse(&text).expect("parses");
        assert_eq!(back, sample());
        assert_eq!(back.tables[0].row_name(), "Customer");
    }

    #[test]
    fn rejects_unknown_fields_and_future_versions() {
        assert!(DataSource::parse("name='a'\nconnection='a'\nprovider='sqlite'\npassword='x'").is_err());
        assert!(DataSource::parse("version=9\nname='a'\nconnection='a'\nprovider='sqlite'").is_err());
        let min = DataSource::parse("name='a'\nconnection='a'\nprovider='postgres'").expect("minimal");
        assert_eq!(min.version, 1);
        assert!(min.tables.is_empty());
    }
}
