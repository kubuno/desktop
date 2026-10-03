//! SQL text helpers shared by the runtime (`kubuno_data::sql`) and the `data_source!` macro — never
//! for values: values are always parameters.
//!
//! - [`rewrite_named`]: provider-neutral named parameters `@name` → the provider's placeholders
//!   (`$1` on PostgreSQL, `?1` on SQLite, `?` on MySQL, `@P1` on SQL Server), skipping string
//!   literals, quoted identifiers, comments and PostgreSQL dollar-quoted strings; a repeated name
//!   reuses its number (MySQL's `?` takes the value again).
//! - [`quote_ident`]: a table or column name quoted for a provider (the quote character doubled
//!   inside), for names that come from a `.kbdata` file.

use crate::types::ProviderName;

/// A command text with its named parameters replaced by placeholders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedSql {
    pub sql: String,
    /// The parameter names in placeholder order (`names[0]` is `$1`); with MySQL's positional `?`,
    /// a repeated name appears once per occurrence.
    pub names: Vec<String>,
}

impl ProviderName {
    /// The placeholder of parameter `n` (1-based).
    pub fn placeholder(self, n: usize) -> String {
        match self {
            ProviderName::Postgres => format!("${n}"),
            ProviderName::Sqlite => format!("?{n}"),
            ProviderName::Mysql => "?".to_string(),
            ProviderName::Sqlserver => format!("@P{n}"),
        }
    }

    /// Whether a placeholder names its parameter by number (a repeated `@name` reuses it); `false`
    /// for MySQL's `?` (the value is passed again).
    pub fn numbered_placeholders(self) -> bool {
        self != ProviderName::Mysql
    }

    /// The identifier quotes: `"` (PostgreSQL, SQLite), `` ` `` (MySQL), `[`/`]` (SQL Server).
    pub fn quotes(self) -> (char, char) {
        match self {
            ProviderName::Mysql => ('`', '`'),
            ProviderName::Sqlserver => ('[', ']'),
            _ => ('"', '"'),
        }
    }
}

/// Rewrites the `@name` parameters of `sql` for `provider` (see the module doc).
pub fn rewrite_named(sql: &str, provider: ProviderName) -> NamedSql {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut names: Vec<String> = Vec::new();
    let mut i = 0;
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' | '"' | '`' => {
                // A literal or a quoted identifier: copied up to its closing quote (doubled quotes
                // are escapes and simply re-enter the loop).
                out.push(c);
                i += 1;
                while i < chars.len() {
                    out.push(chars[i]);
                    if chars[i] == c {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            '[' if provider == ProviderName::Sqlserver => {
                while i < chars.len() {
                    out.push(chars[i]);
                    i += 1;
                    if chars[i - 1] == ']' {
                        break;
                    }
                }
            }
            '-' if chars.get(i + 1) == Some(&'-') => {
                while i < chars.len() && chars[i] != '\n' {
                    out.push(chars[i]);
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                out.push_str("/*");
                i += 2;
                while i < chars.len() {
                    if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                        out.push_str("*/");
                        i += 2;
                        break;
                    }
                    out.push(chars[i]);
                    i += 1;
                }
            }
            '$' if provider == ProviderName::Postgres && !(i > 0 && is_ident(chars[i - 1])) => {
                // A dollar-quoted string `$tag$ … $tag$` (not a `$1` placeholder).
                let mut j = i + 1;
                while j < chars.len() && (chars[j].is_ascii_alphabetic() || chars[j] == '_') {
                    j += 1;
                }
                if chars.get(j) == Some(&'$') {
                    let tag: String = chars[i..=j].iter().collect();
                    out.push_str(&tag);
                    i = j + 1;
                    let tag_chars: Vec<char> = tag.chars().collect();
                    while i < chars.len() {
                        if chars[i..].starts_with(&tag_chars) {
                            out.push_str(&tag);
                            i += tag_chars.len();
                            break;
                        }
                        out.push(chars[i]);
                        i += 1;
                    }
                } else {
                    out.push(c);
                    i += 1;
                }
            }
            '@' if chars.get(i + 1).is_some_and(|n| n.is_ascii_alphabetic() || *n == '_') && !(i > 0 && (is_ident(chars[i - 1]) || chars[i - 1] == '@')) => {
                let mut j = i + 1;
                while j < chars.len() && is_ident(chars[j]) {
                    j += 1;
                }
                let name: String = chars[i + 1..j].iter().collect();
                // Numbered placeholders reuse a repeated name's number; `?` (MySQL) takes it again.
                let n = match names.iter().position(|x| x.eq_ignore_ascii_case(&name)) {
                    Some(p) if provider.numbered_placeholders() => p + 1,
                    _ => {
                        names.push(name);
                        names.len()
                    }
                };
                out.push_str(&provider.placeholder(n));
                i = j;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    NamedSql { sql: out, names }
}

/// `name` quoted for `provider`, with a `schema.` prefix when `schema` is not empty. The closing
/// quote character is doubled inside the name, so any name is safe; an empty name or one with a
/// control character is refused.
pub fn quote_ident(provider: ProviderName, schema: &str, name: &str) -> Result<String, String> {
    let one = |part: &str| -> Result<String, String> {
        if part.is_empty() || part.chars().any(char::is_control) {
            return Err(format!("`{}` is not a valid name", part.escape_debug()));
        }
        let (open, close) = provider.quotes();
        let mut out = String::with_capacity(part.len() + 2);
        out.push(open);
        for c in part.chars() {
            out.push(c);
            if c == close {
                out.push(close);
            }
        }
        out.push(close);
        Ok(out)
    };
    if schema.is_empty() {
        one(name)
    } else {
        Ok(format!("{}.{}", one(schema)?, one(name)?))
    }
}

/// Checks a native type name used in a `CAST(… AS type)`: letters, digits, `_`, spaces, and a
/// length/precision suffix `(n[,m])`.
pub fn is_valid_type_name(name: &str) -> bool {
    let base = name.split('(').next().unwrap_or("");
    let suffix = &name[base.len()..];
    let base_ok = !base.trim().is_empty() && base.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ' ');
    let suffix_ok = suffix.is_empty() || (suffix.starts_with('(') && suffix.ends_with(')') && suffix[1..suffix.len() - 1].chars().all(|c| c.is_ascii_digit() || c == ',' || c == ' '));
    base_ok && suffix_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_per_provider() {
        let sql = "SELECT * FROM t WHERE a = @a AND b > @B OR a = @a";
        assert_eq!(rewrite_named(sql, ProviderName::Postgres).sql, "SELECT * FROM t WHERE a = $1 AND b > $2 OR a = $1");
        assert_eq!(rewrite_named(sql, ProviderName::Sqlite).sql, "SELECT * FROM t WHERE a = ?1 AND b > ?2 OR a = ?1");
        let my = rewrite_named(sql, ProviderName::Mysql);
        assert_eq!(my.sql, "SELECT * FROM t WHERE a = ? AND b > ? OR a = ?");
        assert_eq!(my.names, ["a", "B", "a"]);
        assert_eq!(rewrite_named(sql, ProviderName::Sqlserver).sql, "SELECT * FROM t WHERE a = @P1 AND b > @P2 OR a = @P1");
    }

    #[test]
    fn literals_comments_and_operators_are_left_alone() {
        let sql = "SELECT '@not', \"@col\", $$ @x $$, $tag$ @y $tag$ -- @z\n, /* @w */ @@version, e.mail@host, j @> '{}', @real";
        let p = rewrite_named(sql, ProviderName::Postgres);
        assert_eq!(p.names, ["real"]);
        assert!(p.sql.ends_with("$1"));
        assert_eq!(rewrite_named("SELECT $1", ProviderName::Postgres).sql, "SELECT $1");
        assert_eq!(rewrite_named("SELECT [@x] FROM t WHERE a = @a", ProviderName::Sqlserver).names, ["a"]);
    }

    #[test]
    fn quoting() {
        assert_eq!(quote_ident(ProviderName::Postgres, "shop", "customers").as_deref(), Ok("\"shop\".\"customers\""));
        assert_eq!(quote_ident(ProviderName::Sqlite, "", "a\"b").as_deref(), Ok("\"a\"\"b\""));
        assert_eq!(quote_ident(ProviderName::Mysql, "", "order lines").as_deref(), Ok("`order lines`"));
        assert_eq!(quote_ident(ProviderName::Sqlserver, "dbo", "x]y").as_deref(), Ok("[dbo].[x]]y]"));
        assert!(quote_ident(ProviderName::Sqlite, "", "").is_err());
        assert!(quote_ident(ProviderName::Sqlite, "", "a\nb").is_err());
        assert!(is_valid_type_name("numeric(10,2)"));
        assert!(is_valid_type_name("character varying"));
        assert!(!is_valid_type_name("int4); drop table x; --"));
    }
}
