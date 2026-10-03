//! Database values ([`DbValue`]), column types ([`DbType`]) and their conversions to and from the
//! binding engine's [`Value`] (`kubuno_desktop_views::binding`).
//!
//! `DbValue` is deliberately small and exact: integers stay 64-bit, floats 64-bit; types without a
//! lossless native mapping (timestamps, dates, UUID, JSON, NUMERIC) travel as `Text` in their
//! canonical form and are cast by the server on write (`vskubuno/docs/DATA.md` §4).

use std::cmp::Ordering;

use kubuno_desktop_views::binding::{BindingFormat, Value};
use kubuno_desktop_views::format::ValueKind;

/// One value read from or written to a database.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum DbValue {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
}

impl DbValue {
    pub fn is_null(&self) -> bool {
        matches!(self, DbValue::Null)
    }

    /// The value as display text: what a text control shows (`""` for `NULL`).
    pub fn to_display(&self) -> String {
        match self {
            DbValue::Null => String::new(),
            DbValue::Bool(b) => b.to_string(),
            DbValue::Int(i) => i.to_string(),
            DbValue::Float(f) => f.to_string(),
            DbValue::Text(s) => s.clone(),
            DbValue::Bytes(b) => format!("<{} bytes>", b.len()),
        }
    }

    /// The value a binding reads: `Bool` for a boolean column (check boxes, switches), display
    /// text for everything else (the exact digits of a number are kept).
    pub fn to_view_value(&self, kind: DbKind) -> Value {
        match (self, kind) {
            (DbValue::Bool(b), _) => Value::Bool(*b),
            (DbValue::Null, DbKind::Bool) => Value::Bool(false),
            (DbValue::Int(i), DbKind::Bool) => Value::Bool(*i != 0),
            (v, _) => Value::Str(v.to_display()),
        }
    }

    /// The value as a number, when it is one (or a text that parses as one).
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            DbValue::Int(i) => Some(*i as f64),
            DbValue::Float(f) => Some(*f),
            DbValue::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            DbValue::Text(s) => s.trim().parse::<f64>().ok(),
            _ => None,
        }
    }

    /// Total order used by sorting: `NULL` first, then numbers, then text (case-insensitive),
    /// then bytes. Two numbers compare numerically whatever their variant.
    pub fn sort_cmp(&self, other: &DbValue) -> Ordering {
        fn rank(v: &DbValue) -> u8 {
            match v {
                DbValue::Null => 0,
                DbValue::Bool(_) | DbValue::Int(_) | DbValue::Float(_) => 1,
                DbValue::Text(_) => 2,
                DbValue::Bytes(_) => 3,
            }
        }
        match (self, other) {
            (DbValue::Text(a), DbValue::Text(b)) => a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)),
            (DbValue::Bytes(a), DbValue::Bytes(b)) => a.cmp(b),
            (a, b) if rank(a) == 1 && rank(b) == 1 => {
                let (x, y) = (a.as_f64().unwrap_or(0.0), b.as_f64().unwrap_or(0.0));
                x.partial_cmp(&y).unwrap_or(Ordering::Equal)
            }
            (a, b) => rank(a).cmp(&rank(b)),
        }
    }
}

impl From<&str> for DbValue {
    fn from(s: &str) -> Self {
        DbValue::Text(s.to_string())
    }
}
impl From<String> for DbValue {
    fn from(s: String) -> Self {
        DbValue::Text(s)
    }
}
impl From<i64> for DbValue {
    fn from(i: i64) -> Self {
        DbValue::Int(i)
    }
}
impl From<i32> for DbValue {
    fn from(i: i32) -> Self {
        DbValue::Int(i64::from(i))
    }
}
impl From<f64> for DbValue {
    fn from(f: f64) -> Self {
        DbValue::Float(f)
    }
}
impl From<bool> for DbValue {
    fn from(b: bool) -> Self {
        DbValue::Bool(b)
    }
}
impl From<Vec<u8>> for DbValue {
    fn from(b: Vec<u8>) -> Self {
        DbValue::Bytes(b)
    }
}
impl<T: Into<DbValue>> From<Option<T>> for DbValue {
    fn from(v: Option<T>) -> Self {
        v.map_or(DbValue::Null, Into::into)
    }
}

/// The logical type of a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DbKind {
    Bool,
    Int,
    Float,
    #[default]
    Text,
    Bytes,
    /// A timestamp, carried as ISO text.
    DateTime,
    /// A date, carried as `YYYY-MM-DD`.
    Date,
    /// A time of day, carried as `HH:MM:SS`.
    Time,
    /// A UUID, carried as its hyphenated text.
    Uuid,
    /// JSON, carried as its text.
    Json,
    /// An exact decimal, carried as text.
    Decimal,
    /// Anything else the driver can read as text.
    Other,
}

/// A column's type: its logical kind and the provider's native name (`"int4"`, `"INTEGER"`), which
/// the adapter uses to cast parameters on PostgreSQL.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DbType {
    pub kind: DbKind,
    pub native: String,
}

impl DbType {
    pub fn new(kind: DbKind, native: impl Into<String>) -> Self {
        Self { kind, native: native.into() }
    }

    /// The kind of a PostgreSQL type name (`udt_name` or the driver's type name, any case).
    pub fn postgres(native: &str) -> Self {
        let kind = match native.to_ascii_lowercase().as_str() {
            "bool" | "boolean" => DbKind::Bool,
            "int2" | "int4" | "int8" | "smallint" | "integer" | "bigint" | "oid" => DbKind::Int,
            "float4" | "float8" | "real" | "double precision" => DbKind::Float,
            "text" | "varchar" | "bpchar" | "char" | "name" | "citext" | "character varying" | "character" | "unknown" => DbKind::Text,
            "bytea" => DbKind::Bytes,
            "timestamp" | "timestamptz" | "timestamp without time zone" | "timestamp with time zone" => DbKind::DateTime,
            "date" => DbKind::Date,
            "time" | "timetz" | "time without time zone" => DbKind::Time,
            "uuid" => DbKind::Uuid,
            "json" | "jsonb" => DbKind::Json,
            "numeric" | "decimal" | "money" => DbKind::Decimal,
            _ => DbKind::Other,
        };
        Self::new(kind, native.to_ascii_lowercase())
    }

    /// The kind of a SQLite declared type, by SQLite's affinity rules (plus the usual `BOOLEAN`,
    /// `DATE`, `DATETIME` names).
    pub fn sqlite(declared: &str) -> Self {
        let up = declared.to_ascii_uppercase();
        let kind = if up.starts_with("BOOL") {
            DbKind::Bool
        } else if up.contains("INT") {
            DbKind::Int
        } else if up.contains("CHAR") || up.contains("CLOB") || up.contains("TEXT") {
            DbKind::Text
        } else if up.contains("BLOB") {
            DbKind::Bytes
        } else if up.contains("REAL") || up.contains("FLOA") || up.contains("DOUB") {
            DbKind::Float
        } else if up == "DATE" {
            DbKind::Date
        } else if up.contains("DATETIME") || up.contains("TIMESTAMP") {
            DbKind::DateTime
        } else if up == "TIME" {
            DbKind::Time
        } else if up.contains("NUMERIC") || up.contains("DECIMAL") {
            DbKind::Decimal
        } else if up.is_empty() || up == "NULL" {
            DbKind::Other
        } else {
            DbKind::Text
        };
        Self::new(kind, declared)
    }
}

impl DbType {
    /// The kind of a MySQL / MariaDB type (the driver's name, `DATA_TYPE` or `COLUMN_TYPE`).
    pub fn mysql(native: &str) -> Self {
        let lower = native.to_ascii_lowercase();
        let base = lower.split(['(', ' ']).next().unwrap_or("");
        let kind = match base {
            "boolean" | "bool" => DbKind::Bool,
            "tinyint" if lower.starts_with("tinyint(1)") => DbKind::Bool,
            "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint" | "year" => DbKind::Int,
            "float" | "double" | "real" => DbKind::Float,
            "decimal" | "numeric" => DbKind::Decimal,
            "char" | "varchar" | "text" | "tinytext" | "mediumtext" | "longtext" | "enum" | "set" => DbKind::Text,
            "binary" | "varbinary" | "blob" | "tinyblob" | "mediumblob" | "longblob" | "bit" => DbKind::Bytes,
            "date" => DbKind::Date,
            "datetime" | "timestamp" => DbKind::DateTime,
            "time" => DbKind::Time,
            "json" => DbKind::Json,
            _ => DbKind::Other,
        };
        Self::new(kind, lower)
    }

    /// The kind of a SQL Server type (`DATA_TYPE`, or the driver's column type).
    pub fn mssql(native: &str) -> Self {
        let lower = native.to_ascii_lowercase();
        let base = lower.split('(').next().unwrap_or("").trim();
        let kind = match base {
            "bit" => DbKind::Bool,
            "tinyint" | "smallint" | "int" | "bigint" => DbKind::Int,
            "real" | "float" => DbKind::Float,
            "decimal" | "numeric" | "money" | "smallmoney" => DbKind::Decimal,
            "char" | "varchar" | "nchar" | "nvarchar" | "text" | "ntext" | "xml" | "sysname" => DbKind::Text,
            "date" => DbKind::Date,
            "datetime" | "datetime2" | "smalldatetime" | "datetimeoffset" => DbKind::DateTime,
            "time" => DbKind::Time,
            "uniqueidentifier" => DbKind::Uuid,
            "binary" | "varbinary" | "image" | "timestamp" | "rowversion" => DbKind::Bytes,
            _ => DbKind::Other,
        };
        Self::new(kind, lower)
    }
}

/// What a typed binding reads from a column's value (DATA-2, `kubuno_desktop_views::format`): a number for a
/// numeric property, a boolean for a check box, formatted text for a text property (`FormatString`,
/// `NullValue`, culture). Integers are formatted from their exact value, never through `f32`.
pub fn to_bound(v: &DbValue, kind: DbKind, want: ValueKind, format: &BindingFormat) -> Option<Value> {
    use kubuno_desktop_views::format as f;
    let text_of = |n: f64, exact: String| -> String {
        match format.format_string.as_deref().filter(|s| !s.trim().is_empty()) {
            Some(fs) if f::is_numeric_format(fs) => f::format_number(n, fs, &f::culture_of(format)).unwrap_or(exact),
            _ if format.culture.is_some() && kind == DbKind::Float => exact.replace('.', &f::culture_of(format).decimal.to_string()),
            _ => exact,
        }
    };
    match (v, want) {
        (DbValue::Null, ValueKind::Number) => None,
        (DbValue::Null, ValueKind::Bool) => Some(Value::Bool(false)),
        (DbValue::Null, _) if kind == DbKind::Bool && want == ValueKind::Any => Some(Value::Bool(false)),
        (DbValue::Null, _) => Some(Value::Str(format.null_value.clone().unwrap_or_default())),
        (DbValue::Bool(b), ValueKind::Text) => Some(Value::Str(b.to_string())),
        (DbValue::Bool(b), ValueKind::Number) => Some(Value::F32(if *b { 1.0 } else { 0.0 })),
        (DbValue::Bool(b), _) => Some(Value::Bool(*b)),
        (DbValue::Int(i), ValueKind::Bool) => Some(Value::Bool(*i != 0)),
        (DbValue::Int(i), ValueKind::Number) => Some(Value::F32(*i as f32)),
        (DbValue::Int(i), _) if kind == DbKind::Bool => Some(Value::Bool(*i != 0)),
        (DbValue::Int(i), _) => Some(Value::Str(text_of(*i as f64, i.to_string()))),
        (DbValue::Float(x), ValueKind::Number) => Some(Value::F32(*x as f32)),
        (DbValue::Float(x), ValueKind::Bool) => Some(Value::Bool(*x != 0.0)),
        (DbValue::Float(x), _) => Some(Value::Str(text_of(*x, x.to_string()))),
        (DbValue::Text(s), ValueKind::Number) => s.trim().parse::<f64>().ok().map(|n| Value::F32(n as f32)),
        (DbValue::Text(s), ValueKind::Bool) => f::to_target(Value::Str(s.clone()), ValueKind::Bool, format),
        (DbValue::Text(s), _) => Some(Value::Str(f::format_text(s, format))),
        (DbValue::Bytes(b), _) => Some(Value::Str(format!("<{} bytes>", b.len()))),
    }
}

/// Converts what a typed binding wrote to a value of a column of `kind`, per the binding's format
/// and culture (`1 234,50` → `1234.5` in French, `10/12/1815` → `1815-12-10` with `FormatString=d`),
/// else as [`from_view_value`] does. `Err` carries the message shown to the user.
pub fn from_bound(value: &Value, kind: DbKind, format: &BindingFormat) -> Result<DbValue, String> {
    use kubuno_desktop_views::format as f;
    if format.is_empty() {
        return from_view_value(value, kind);
    }
    let Value::Str(text) = value else { return from_view_value(value, kind) };
    if format.null_value.as_deref().is_some_and(|n| n == text) {
        return Ok(DbValue::Null);
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return from_view_value(value, kind);
    }
    let culture = f::culture_of(format);
    let pattern = format.format_string.as_deref().filter(|s| !s.trim().is_empty());
    match kind {
        DbKind::Int | DbKind::Float | DbKind::Decimal => {
            let Some(n) = f::parse_number(trimmed, &culture) else {
                return Err(if kind == DbKind::Int { "Enter a whole number." } else { "Enter a number." }.to_string());
            };
            match kind {
                DbKind::Int if n.fract() == 0.0 && n.abs() < 9.2e18 => Ok(DbValue::Int(n as i64)),
                DbKind::Int => Err("Enter a whole number.".to_string()),
                DbKind::Float => Ok(DbValue::Float(n)),
                _ => Ok(DbValue::Text(format!("{n}"))),
            }
        }
        DbKind::Date | DbKind::DateTime | DbKind::Time => {
            let parsed = pattern.and_then(|p| f::parse_date(trimmed, p, &culture)).or_else(|| f::parse_iso(trimmed));
            match parsed {
                Some(p) if kind == DbKind::Date && p.has_date => Ok(DbValue::Text(f::DateParts { has_time: false, ..p }.to_iso())),
                Some(p) if kind == DbKind::Time && p.has_time => Ok(DbValue::Text(f::DateParts { has_date: false, ..p }.to_iso())),
                Some(p) if kind == DbKind::DateTime && p.has_date => Ok(DbValue::Text(f::DateParts { has_time: true, ..p }.to_iso())),
                _ => Err(match pattern {
                    Some(p) => format!("Enter a date as {}.", f::date_pattern(p, &culture)),
                    None => "Enter a date as YYYY-MM-DD.".to_string(),
                }),
            }
        }
        DbKind::Bool => match f::to_target(Value::Str(text.clone()), ValueKind::Bool, format) {
            Some(Value::Bool(b)) => Ok(DbValue::Bool(b)),
            _ => Err("Enter true or false.".to_string()),
        },
        _ => from_view_value(value, kind),
    }
}

/// Converts what a binding wrote (`Value`) to a value of a column of `kind`. `Err` carries the
/// message shown to the user (the proposed text is kept by the binding source).
pub fn from_view_value(value: &Value, kind: DbKind) -> Result<DbValue, String> {
    let text = match value {
        Value::Bool(b) => {
            return Ok(match kind {
                DbKind::Bool => DbValue::Bool(*b),
                DbKind::Int => DbValue::Int(i64::from(*b)),
                _ => DbValue::Text(b.to_string()),
            })
        }
        Value::F32(f) => {
            return match kind {
                DbKind::Int if f.fract() == 0.0 => Ok(DbValue::Int(*f as i64)),
                DbKind::Int => Err("Enter a whole number.".to_string()),
                DbKind::Float | DbKind::Decimal => Ok(DbValue::Float(f64::from(*f))),
                DbKind::Bool => Ok(DbValue::Bool(*f != 0.0)),
                _ => Ok(DbValue::Text(f.to_string())),
            }
        }
        Value::List(_) | Value::Object(_) => return Err("A list cannot be stored in a field.".to_string()),
        Value::Str(s) => s,
    };
    let trimmed = text.trim();
    if trimmed.is_empty() && kind != DbKind::Text {
        // An empty non-text field is NULL; NOT NULL is checked when the edit ends.
        return Ok(DbValue::Null);
    }
    match kind {
        DbKind::Text | DbKind::Other | DbKind::Json | DbKind::DateTime | DbKind::Time => Ok(DbValue::Text(text.clone())),
        DbKind::Int => trimmed.trim_start_matches('+').parse::<i64>().map(DbValue::Int).map_err(|_| "Enter a whole number.".to_string()),
        DbKind::Float => parse_decimal(trimmed).map(DbValue::Float).ok_or_else(|| "Enter a number.".to_string()),
        DbKind::Decimal => parse_decimal(trimmed).map(|_| DbValue::Text(trimmed.replace(',', "."))).ok_or_else(|| "Enter a number.".to_string()),
        DbKind::Bool => match trimmed.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "oui" | "vrai" => Ok(DbValue::Bool(true)),
            "false" | "0" | "no" | "non" | "faux" => Ok(DbValue::Bool(false)),
            _ => Err("Enter true or false.".to_string()),
        },
        DbKind::Bytes => Err("Binary data cannot be typed.".to_string()),
        DbKind::Date => chrono::NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
            .map(|d| DbValue::Text(d.format("%Y-%m-%d").to_string()))
            .map_err(|_| "Enter a date as YYYY-MM-DD.".to_string()),
        DbKind::Uuid => uuid::Uuid::parse_str(trimmed).map(|u| DbValue::Text(u.hyphenated().to_string())).map_err(|_| "Enter a valid identifier (UUID).".to_string()),
    }
}

/// A decimal number with `.` or `,` as its separator.
fn parse_decimal(s: &str) -> Option<f64> {
    let normalized = s.replace(',', ".");
    normalized.parse::<f64>().ok().filter(|f| f.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_values() {
        assert_eq!(DbValue::Int(12345678901).to_view_value(DbKind::Int), Value::Str("12345678901".into()));
        assert_eq!(DbValue::Bool(true).to_view_value(DbKind::Bool), Value::Bool(true));
        assert_eq!(DbValue::Int(1).to_view_value(DbKind::Bool), Value::Bool(true));
        assert_eq!(DbValue::Null.to_view_value(DbKind::Text), Value::Str(String::new()));
    }

    #[test]
    fn conversions_from_bindings() {
        assert_eq!(from_view_value(&Value::Str(" 42 ".into()), DbKind::Int), Ok(DbValue::Int(42)));
        assert!(from_view_value(&Value::Str("abc".into()), DbKind::Int).is_err());
        assert_eq!(from_view_value(&Value::Str("3,5".into()), DbKind::Float), Ok(DbValue::Float(3.5)));
        assert_eq!(from_view_value(&Value::Str("".into()), DbKind::Int), Ok(DbValue::Null));
        assert_eq!(from_view_value(&Value::Str("".into()), DbKind::Text), Ok(DbValue::Text(String::new())));
        assert_eq!(from_view_value(&Value::Bool(true), DbKind::Bool), Ok(DbValue::Bool(true)));
        assert_eq!(from_view_value(&Value::F32(3.0), DbKind::Int), Ok(DbValue::Int(3)));
        assert!(from_view_value(&Value::Str("2026-13-01".into()), DbKind::Date).is_err());
        assert!(from_view_value(&Value::Str("not-a-uuid".into()), DbKind::Uuid).is_err());
        assert_eq!(from_view_value(&Value::Str("1,25".into()), DbKind::Decimal), Ok(DbValue::Text("1.25".into())));
    }

    #[test]
    fn type_names() {
        assert_eq!(DbType::postgres("INT4").kind, DbKind::Int);
        assert_eq!(DbType::postgres("timestamptz").kind, DbKind::DateTime);
        assert_eq!(DbType::sqlite("BOOLEAN").kind, DbKind::Bool);
        assert_eq!(DbType::sqlite("VARCHAR(80)").kind, DbKind::Text);
        assert_eq!(DbType::sqlite("INTEGER").kind, DbKind::Int);
        assert_eq!(DbType::sqlite("DATE").kind, DbKind::Date);
    }

    #[test]
    fn typed_bindings() {
        let fr = |fs: &str| BindingFormat { format_string: Some(fs.into()), null_value: Some("—".into()), culture: Some("fr-FR".into()) };
        assert_eq!(to_bound(&DbValue::Int(12345678901), DbKind::Int, ValueKind::Text, &BindingFormat::default()), Some(Value::Str("12345678901".into())));
        assert_eq!(to_bound(&DbValue::Int(1234), DbKind::Int, ValueKind::Text, &fr("N0")), Some(Value::Str("1\u{202F}234".into())));
        assert_eq!(to_bound(&DbValue::Int(36), DbKind::Int, ValueKind::Number, &BindingFormat::default()), Some(Value::F32(36.0)));
        assert_eq!(to_bound(&DbValue::Text("1234.5".into()), DbKind::Decimal, ValueKind::Text, &fr("N2")), Some(Value::Str("1\u{202F}234,50".into())));
        assert_eq!(to_bound(&DbValue::Text("1815-12-10".into()), DbKind::Date, ValueKind::Text, &fr("d")), Some(Value::Str("10/12/1815".into())));
        assert_eq!(to_bound(&DbValue::Null, DbKind::Date, ValueKind::Text, &fr("d")), Some(Value::Str("—".into())));
        assert_eq!(to_bound(&DbValue::Null, DbKind::Int, ValueKind::Number, &fr("N0")), None);
        assert_eq!(from_bound(&Value::Str("1 234,50".into()), DbKind::Decimal, &fr("N2")), Ok(DbValue::Text("1234.5".into())));
        assert_eq!(from_bound(&Value::Str("1\u{202F}234".into()), DbKind::Int, &fr("N0")), Ok(DbValue::Int(1234)));
        assert!(from_bound(&Value::Str("12,5".into()), DbKind::Int, &fr("N0")).is_err());
        assert_eq!(from_bound(&Value::Str("11/12/1815".into()), DbKind::Date, &fr("d")), Ok(DbValue::Text("1815-12-11".into())));
        assert_eq!(from_bound(&Value::Str("31/02/2026".into()), DbKind::Date, &fr("d")), Err("Enter a date as dd/MM/yyyy.".into()));
        assert_eq!(from_bound(&Value::Str("—".into()), DbKind::Date, &fr("d")), Ok(DbValue::Null));
        assert_eq!(from_bound(&Value::F32(3.0), DbKind::Int, &fr("N0")), Ok(DbValue::Int(3)));
    }

    #[test]
    fn other_providers_types() {
        assert_eq!(DbType::mysql("tinyint(1)").kind, DbKind::Bool);
        assert_eq!(DbType::mysql("BIGINT UNSIGNED").kind, DbKind::Int);
        assert_eq!(DbType::mysql("decimal(10,2)").kind, DbKind::Decimal);
        assert_eq!(DbType::mysql("DATETIME").kind, DbKind::DateTime);
        assert_eq!(DbType::mssql("nvarchar").kind, DbKind::Text);
        assert_eq!(DbType::mssql("datetime2").kind, DbKind::DateTime);
        assert_eq!(DbType::mssql("uniqueidentifier").kind, DbKind::Uuid);
        assert_eq!(DbType::mssql("rowversion").kind, DbKind::Bytes);
    }

    #[test]
    fn sort_order() {
        let mut v = [DbValue::Text("b".into()), DbValue::Null, DbValue::Int(2), DbValue::Float(1.5), DbValue::Text("A".into())];
        v.sort_by(|a, b| a.sort_cmp(b));
        assert_eq!(v, [DbValue::Null, DbValue::Float(1.5), DbValue::Int(2), DbValue::Text("A".into()), DbValue::Text("b".into())]);
    }
}
