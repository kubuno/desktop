//! Providers and the native-type → Rust-type mapping of typed rows.
//!
//! The Rust type is the one `sqlx` decodes the column into, so the generated
//! `sqlx::query_as!` compiles against the offline cache. Types `sqlx` cannot decode without an
//! optional crate the application may not have (NUMERIC/DECIMAL, MONEY, intervals…) travel as text:
//! [`RustType::text_cast`] says the select must cast them to text and the DML cast the parameter
//! back to the native type.

use serde::{Deserialize, Serialize};

/// A database provider, as written in a `.kbdata` file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderName {
    #[default]
    Postgres,
    Sqlite,
    Mysql,
    Sqlserver,
}

impl ProviderName {
    /// `postgres`, `sqlite`, `mysql`, `sqlserver`.
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderName::Postgres => "postgres",
            ProviderName::Sqlite => "sqlite",
            ProviderName::Mysql => "mysql",
            ProviderName::Sqlserver => "sqlserver",
        }
    }

    /// Parses a provider name (case-insensitive; `postgresql`/`pg`, `mariadb`, `mssql` accepted).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "postgres" | "postgresql" | "pg" => Some(ProviderName::Postgres),
            "sqlite" => Some(ProviderName::Sqlite),
            "mysql" | "mariadb" => Some(ProviderName::Mysql),
            "sqlserver" | "mssql" => Some(ProviderName::Sqlserver),
            _ => None,
        }
    }

    /// The `sqlx` database type (`sqlx::Sqlite`…); `None` for SQL Server (no `sqlx` driver).
    pub fn sqlx_database(self) -> Option<&'static str> {
        match self {
            ProviderName::Postgres => Some("Postgres"),
            ProviderName::Sqlite => Some("Sqlite"),
            ProviderName::Mysql => Some("MySql"),
            ProviderName::Sqlserver => None,
        }
    }
}

impl std::fmt::Display for ProviderName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The Rust side of a column type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustType {
    /// The field type, without `Option` (`i64`, `String`, `chrono::NaiveDate`…).
    pub ty: String,
    /// The value travels as text: select `CAST(col AS TEXT)`, write `CAST(@p AS <native>)`.
    pub text_cast: bool,
    /// The `sqlx` feature the application needs for this type (`chrono`, `uuid`, `json`).
    pub sqlx_feature: Option<&'static str>,
}

impl RustType {
    fn plain(ty: &str) -> Self {
        RustType { ty: ty.to_string(), text_cast: false, sqlx_feature: None }
    }
    fn feature(ty: &str, feature: &'static str) -> Self {
        RustType { ty: ty.to_string(), text_cast: false, sqlx_feature: Some(feature) }
    }
    fn text() -> Self {
        RustType { ty: "String".to_string(), text_cast: true, sqlx_feature: None }
    }
}

/// The Rust type of a column of native type `native` (as the database's catalog names it).
pub fn rust_type_for(provider: ProviderName, native: &str) -> RustType {
    let lower = native.trim().to_ascii_lowercase();
    // `varchar(20)`, `numeric(10,2)`, `int unsigned`: the base name decides.
    let base: String = lower.split(['(', ' ']).next().unwrap_or("").to_string();
    let unsigned = lower.contains("unsigned");
    match provider {
        ProviderName::Sqlite => sqlite_type(&base),
        ProviderName::Postgres => postgres_type(&base),
        ProviderName::Mysql => mysql_type(&base, &lower, unsigned),
        ProviderName::Sqlserver => sqlserver_type(&base),
    }
}

/// SQLite: type affinity rules (https://sqlite.org/datatype3.html §3.1).
fn sqlite_type(base: &str) -> RustType {
    match base {
        "boolean" | "bool" => RustType::plain("bool"),
        "date" => RustType::feature("chrono::NaiveDate", "chrono"),
        "datetime" | "timestamp" => RustType::feature("chrono::NaiveDateTime", "chrono"),
        "time" => RustType::feature("chrono::NaiveTime", "chrono"),
        _ if base.contains("int") => RustType::plain("i64"),
        _ if base.contains("char") || base.contains("clob") || base.contains("text") => RustType::plain("String"),
        _ if base.contains("blob") || base.is_empty() => RustType::plain("Vec<u8>"),
        _ if base.contains("real") || base.contains("floa") || base.contains("doub") => RustType::plain("f64"),
        // NUMERIC affinity (numeric, decimal…): sqlx reads it as a float.
        _ => RustType::plain("f64"),
    }
}

fn postgres_type(base: &str) -> RustType {
    match base {
        "bool" | "boolean" => RustType::plain("bool"),
        "int2" | "smallint" | "smallserial" => RustType::plain("i16"),
        "int4" | "integer" | "int" | "serial" => RustType::plain("i32"),
        "int8" | "bigint" | "bigserial" => RustType::plain("i64"),
        "float4" | "real" => RustType::plain("f32"),
        "float8" | "double" => RustType::plain("f64"),
        "text" | "varchar" | "character" | "char" | "bpchar" | "name" | "citext" => RustType::plain("String"),
        "bytea" => RustType::plain("Vec<u8>"),
        "uuid" => RustType::feature("sqlx::types::Uuid", "uuid"),
        "json" | "jsonb" => RustType::feature("sqlx::types::JsonValue", "json"),
        "date" => RustType::feature("chrono::NaiveDate", "chrono"),
        "time" => RustType::feature("chrono::NaiveTime", "chrono"),
        "timestamp" => RustType::feature("chrono::NaiveDateTime", "chrono"),
        "timestamptz" => RustType::feature("chrono::DateTime<chrono::Utc>", "chrono"),
        // numeric, money, interval, inet, arrays, enums…: exact text.
        _ => RustType::text(),
    }
}

fn mysql_type(base: &str, full: &str, unsigned: bool) -> RustType {
    let int = |signed: &str, uns: &str| RustType::plain(if unsigned { uns } else { signed });
    match base {
        "tinyint" if full.starts_with("tinyint(1)") => RustType::plain("bool"),
        "bool" | "boolean" => RustType::plain("bool"),
        "tinyint" => int("i8", "u8"),
        "smallint" => int("i16", "u16"),
        "mediumint" | "int" | "integer" => int("i32", "u32"),
        "bigint" => int("i64", "u64"),
        "float" => RustType::plain("f32"),
        "double" | "real" => RustType::plain("f64"),
        "char" | "varchar" | "text" | "tinytext" | "mediumtext" | "longtext" | "enum" | "set" => RustType::plain("String"),
        "binary" | "varbinary" | "blob" | "tinyblob" | "mediumblob" | "longblob" => RustType::plain("Vec<u8>"),
        "json" => RustType::feature("sqlx::types::JsonValue", "json"),
        "date" => RustType::feature("chrono::NaiveDate", "chrono"),
        "time" => RustType::feature("chrono::NaiveTime", "chrono"),
        "datetime" => RustType::feature("chrono::NaiveDateTime", "chrono"),
        "timestamp" => RustType::feature("chrono::DateTime<chrono::Utc>", "chrono"),
        _ => RustType::text(),
    }
}

/// SQL Server has no `sqlx` driver: typed rows are read through `kubuno-data`'s own values, so the
/// mapping only chooses the field types.
fn sqlserver_type(base: &str) -> RustType {
    match base {
        "bit" => RustType::plain("bool"),
        "tinyint" => RustType::plain("u8"),
        "smallint" => RustType::plain("i16"),
        "int" => RustType::plain("i32"),
        "bigint" => RustType::plain("i64"),
        "real" => RustType::plain("f32"),
        "float" => RustType::plain("f64"),
        "binary" | "varbinary" | "image" | "timestamp" | "rowversion" => RustType::plain("Vec<u8>"),
        "char" | "varchar" | "nchar" | "nvarchar" | "text" | "ntext" | "xml" | "uniqueidentifier" => RustType::plain("String"),
        _ => RustType::text(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers() {
        assert_eq!(ProviderName::parse("PostgreSQL"), Some(ProviderName::Postgres));
        assert_eq!(ProviderName::parse("mariadb"), Some(ProviderName::Mysql));
        assert_eq!(ProviderName::parse("mssql"), Some(ProviderName::Sqlserver));
        assert_eq!(ProviderName::parse("oracle"), None);
        assert_eq!(ProviderName::Sqlite.sqlx_database(), Some("Sqlite"));
        assert_eq!(ProviderName::Sqlserver.sqlx_database(), None);
    }

    #[test]
    fn sqlite_affinities() {
        assert_eq!(rust_type_for(ProviderName::Sqlite, "INTEGER").ty, "i64");
        assert_eq!(rust_type_for(ProviderName::Sqlite, "VARCHAR(40)").ty, "String");
        assert_eq!(rust_type_for(ProviderName::Sqlite, "REAL").ty, "f64");
        assert_eq!(rust_type_for(ProviderName::Sqlite, "BLOB").ty, "Vec<u8>");
        assert_eq!(rust_type_for(ProviderName::Sqlite, "BOOLEAN").ty, "bool");
        assert_eq!(rust_type_for(ProviderName::Sqlite, "DATE").sqlx_feature, Some("chrono"));
    }

    #[test]
    fn postgres_and_mysql() {
        assert_eq!(rust_type_for(ProviderName::Postgres, "int4").ty, "i32");
        assert_eq!(rust_type_for(ProviderName::Postgres, "timestamptz").ty, "chrono::DateTime<chrono::Utc>");
        let numeric = rust_type_for(ProviderName::Postgres, "numeric(10,2)");
        assert!(numeric.text_cast);
        assert_eq!(numeric.ty, "String");
        assert_eq!(rust_type_for(ProviderName::Mysql, "tinyint(1)").ty, "bool");
        assert_eq!(rust_type_for(ProviderName::Mysql, "int unsigned").ty, "u32");
        assert!(rust_type_for(ProviderName::Mysql, "decimal(10,2)").text_cast);
    }
}
