//! SQL text helpers — never for values: values are always parameters.
//!
//! - [`rewrite_named`]: provider-neutral named parameters `@name` → the provider's placeholders
//!   (`$1` on PostgreSQL, `?1` on SQLite), skipping string literals, quoted identifiers, comments
//!   and PostgreSQL dollar-quoted strings; a repeated name reuses its number.
//! - [`validate_identifier`] / [`quote_identifier`]: the table and column names the library writes
//!   itself (generated DML) are validated, then double-quoted.

use crate::error::DataError;
use crate::provider::Provider;

/// A command text with its named parameters replaced by placeholders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedSql {
    pub sql: String,
    /// The parameter names in placeholder order (`names[0]` is `$1`).
    pub names: Vec<String>,
}

/// Rewrites the `@name` parameters of `sql` for `provider` (see the module doc).
pub fn rewrite_named(sql: &str, provider: Provider) -> Result<PreparedSql, DataError> {
    // The lexer lives in `kubuno-desktop-data-model`, shared with the `data_source!` macro (DATA-4).
    let named = kubuno_desktop_data_model::sql::rewrite_named(sql, provider.model_name());
    Ok(PreparedSql { sql: named.sql, names: named.names })
}

/// Checks a table or column name the library will write into SQL: `[A-Za-z_][A-Za-z0-9_$]*`, at
/// most 63 characters (PostgreSQL's limit), optionally `schema.name` when `allow_schema`.
pub fn validate_identifier(name: &str, allow_schema: bool) -> Result<(), DataError> {
    let parts: Vec<&str> = name.split('.').collect();
    if parts.len() > if allow_schema { 2 } else { 1 } {
        return Err(DataError::Validation(format!("`{name}` is not a valid name")));
    }
    for part in parts {
        let mut chars = part.chars();
        let ok = part.len() <= 63 && chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
        if !ok {
            return Err(DataError::Validation(format!("`{name}` is not a valid name (letters, digits and '_' only)")));
        }
    }
    Ok(())
}

/// A validated name, double-quoted per part (`crm.customers` → `"crm"."customers"`).
pub fn quote_identifier(name: &str, allow_schema: bool) -> Result<String, DataError> {
    validate_identifier(name, allow_schema)?;
    Ok(name.split('.').map(|p| format!("\"{p}\"")).collect::<Vec<_>>().join("."))
}

/// A validated name quoted for `provider`: `"x"` (PostgreSQL, SQLite), `` `x` `` (MySQL), `[x]` (SQL Server).
pub fn quote_for(provider: Provider, name: &str, allow_schema: bool) -> Result<String, DataError> {
    validate_identifier(name, allow_schema)?;
    let (open, close) = match provider {
        Provider::MySql => ("`", "`"),
        Provider::SqlServer => ("[", "]"),
        _ => ("\"", "\""),
    };
    Ok(name.split('.').map(|p| format!("{open}{p}{close}")).collect::<Vec<_>>().join("."))
}

/// Checks a native type name used in a `CAST(… AS type)` (PostgreSQL): letters, digits, `_`,
/// spaces, and a length/precision suffix `(n[,m])`.
pub fn validate_type_name(name: &str) -> Result<(), DataError> {
    let base = name.split('(').next().unwrap_or("");
    let suffix = &name[base.len()..];
    let base_ok = !base.trim().is_empty() && base.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ' ');
    let suffix_ok = suffix.is_empty() || (suffix.starts_with('(') && suffix.ends_with(')') && suffix[1..suffix.len() - 1].chars().all(|c| c.is_ascii_digit() || c == ',' || c == ' '));
    if base_ok && suffix_ok {
        Ok(())
    } else {
        Err(DataError::Validation(format!("`{name}` is not a valid type name")))
    }
}

/// The byte index of the last `ORDER BY` at the top level of `sql` (outside parentheses, literals,
/// quoted identifiers and comments).
pub(crate) fn top_level_order_by(sql: &str) -> Option<usize> {
    let bytes = sql.as_bytes();
    let lower = sql.to_ascii_lowercase();
    let lb = lower.as_bytes();
    let (mut i, mut depth, mut found) = (0usize, 0i32, None);
    while i < bytes.len() {
        match bytes[i] {
            q @ (b'\'' | b'"' | b'`') => {
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    i += 1;
                }
            }
            b'[' => {
                while i < bytes.len() && bytes[i] != b']' {
                    i += 1;
                }
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 1;
            }
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'o' | b'O' if depth == 0 && lb[i..].starts_with(b"order") && (i == 0 || !lb[i - 1].is_ascii_alphanumeric()) => {
                let mut j = i + 5;
                let ws = j;
                while j < lb.len() && lb[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j > ws && lb[j..].starts_with(b"by") && lb.get(j + 2).is_none_or(|c| !c.is_ascii_alphanumeric()) {
                    found = Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    found
}

/// `sql` without a trailing `;` and white space.
pub(crate) fn trim_statement(sql: &str) -> &str {
    sql.trim_end().trim_end_matches(';').trim_end()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_repeats_positional_parameters_and_quotes_with_backticks() {
        let p = rewrite_named("SELECT * FROM t WHERE a = @a OR b = @a", Provider::MySql).expect("rewrite");
        assert_eq!(p.sql, "SELECT * FROM t WHERE a = ? OR b = ?");
        assert_eq!(p.names, ["a", "a"]);
        let m = rewrite_named("SELECT * FROM t WHERE a = @a OR b = @a", Provider::SqlServer).expect("rewrite");
        assert_eq!(m.sql, "SELECT * FROM t WHERE a = @P1 OR b = @P1");
        assert_eq!(quote_for(Provider::MySql, "crm.orders", true).expect("ok"), "`crm`.`orders`");
        assert_eq!(quote_for(Provider::SqlServer, "dbo.orders", true).expect("ok"), "[dbo].[orders]");
        assert!(quote_for(Provider::SqlServer, "x]; drop", false).is_err());
    }

    #[test]
    fn order_by_at_the_top_level() {
        let sql = "SELECT id, (SELECT max(x) FROM y ORDER BY x) AS m FROM t WHERE n = 'order by' ORDER BY id";
        assert_eq!(top_level_order_by(sql), sql.rfind("ORDER BY"));
        assert_eq!(top_level_order_by("SELECT (SELECT 1 ORDER BY 1)"), None);
        assert_eq!(top_level_order_by("SELECT border_by FROM t"), None);
        assert_eq!(trim_statement("SELECT 1 ;  "), "SELECT 1");
    }

    #[test]
    fn named_parameters_become_placeholders() {
        let p = rewrite_named("SELECT * FROM t WHERE a = @a AND b > @B OR a = @a", Provider::Postgres).expect("rewrite");
        assert_eq!(p.sql, "SELECT * FROM t WHERE a = $1 AND b > $2 OR a = $1");
        assert_eq!(p.names, ["a", "B"]);
        let s = rewrite_named("UPDATE t SET x = @x WHERE id = @id", Provider::Sqlite).expect("rewrite");
        assert_eq!(s.sql, "UPDATE t SET x = ?1 WHERE id = ?2");
    }

    #[test]
    fn literals_comments_and_operators_are_left_alone() {
        let sql = "SELECT '@not', \"@col\", $$ @x $$, $tag$ @y $tag$ -- @z\n, /* @w */ @@version, e.mail@host, j @> '{}', @real";
        let p = rewrite_named(sql, Provider::Postgres).expect("rewrite");
        assert_eq!(p.names, ["real"]);
        assert!(p.sql.ends_with("$1"));
        assert!(p.sql.contains("'@not'") && p.sql.contains("\"@col\"") && p.sql.contains("$$ @x $$") && p.sql.contains("/* @w */"));
        // `$1` stays a positional placeholder.
        assert_eq!(rewrite_named("SELECT $1", Provider::Postgres).expect("rewrite").sql, "SELECT $1");
    }

    #[test]
    fn identifiers() {
        assert_eq!(quote_identifier("crm.customers", true).expect("ok"), "\"crm\".\"customers\"");
        assert!(quote_identifier("crm.customers", false).is_err());
        for bad in ["", "1abc", "a b", "a\"b", "a;drop", "x.y.z"] {
            assert!(validate_identifier(bad, true).is_err(), "{bad}");
        }
        assert!(validate_type_name("character varying(80)").is_ok());
        assert!(validate_type_name("int4); drop table x; --").is_err());
    }
}
