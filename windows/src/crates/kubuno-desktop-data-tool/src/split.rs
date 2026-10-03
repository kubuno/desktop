//! Splitting a query window's text into statements, and telling which ones return rows.
//!
//! The lexer skips what can hide a `;`: string literals (with `''`; MySQL's backslash escapes),
//! quoted identifiers (`"…"`, `` `…` ``, SQL Server's `[…]`), line and block comments (nested on
//! PostgreSQL, `#` on MySQL) and PostgreSQL dollar quotes. Compound statements (SQLite triggers,
//! T-SQL procedures) keep their inner `;` by counting `BEGIN`/`CASE` … `END`; on SQL Server a line
//! holding only `GO` also ends a batch.

use kubuno_desktop_data_model::ProviderName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Semicolon,
    Space,
    Comment,
    /// A literal or quoted identifier.
    Quoted,
    Punct,
}

#[derive(Debug, Clone, Copy)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '$' | '#' | '@')
}

fn tokenize(sql: &str, provider: ProviderName) -> Vec<Token> {
    let bytes = sql.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    let pg = provider == ProviderName::Postgres;
    let mysql = provider == ProviderName::Mysql;
    let mssql = provider == ProviderName::Sqlserver;
    while i < bytes.len() {
        let start = i;
        let c = bytes[i];
        let kind = match c {
            b';' => {
                i += 1;
                Kind::Semicolon
            }
            b' ' | b'\t' | b'\r' | b'\n' | 0x0c => {
                while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n' | 0x0c) {
                    i += 1;
                }
                Kind::Space
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                Kind::Comment
            }
            b'#' if mysql => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                Kind::Comment
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let mut depth = 1;
                i += 2;
                while i < bytes.len() && depth > 0 {
                    if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        depth -= 1;
                        i += 2;
                    } else if pg && bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                        depth += 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                Kind::Comment
            }
            b'\'' | b'"' | b'`' => {
                let quote = c;
                i += 1;
                while i < bytes.len() {
                    if mysql && bytes[i] == b'\\' && quote != b'`' {
                        i += 2;
                    } else if bytes[i] == quote {
                        if bytes.get(i + 1) == Some(&quote) {
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                Kind::Quoted
            }
            b'[' if mssql => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b']' {
                        if bytes.get(i + 1) == Some(&b']') {
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                Kind::Quoted
            }
            b'$' if pg && dollar_tag_end(sql, i).is_some() => {
                // `$tag$ … $tag$`; an unterminated one runs to the end of the text.
                let tag_end = dollar_tag_end(sql, i).unwrap_or(i + 1);
                let tag = &sql[i..tag_end];
                i = match sql[tag_end..].find(tag) {
                    Some(p) => tag_end + p + tag.len(),
                    None => bytes.len(),
                };
                Kind::Quoted
            }
            _ => {
                let ch = sql[i..].chars().next().unwrap_or(' ');
                if is_word_char(ch) && !(ch == '$' && pg) {
                    while i < bytes.len() {
                        let cc = sql[i..].chars().next().unwrap_or(' ');
                        if is_word_char(cc) {
                            i += cc.len_utf8();
                        } else {
                            break;
                        }
                    }
                    Kind::Word
                } else {
                    i += ch.len_utf8();
                    Kind::Punct
                }
            }
        };
        i = i.min(bytes.len());
        out.push(Token { kind, start, end: i });
    }
    out
}

/// If a PostgreSQL dollar-quote tag (`$$`, `$tag$`) opens at `at`, the index just after it. A `$`
/// glued to an identifier (`a$b`) or followed by a digit (`$1`) does not open one.
fn dollar_tag_end(sql: &str, at: usize) -> Option<usize> {
    let bytes = sql.as_bytes();
    if at > 0 && (bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_') {
        return None;
    }
    let mut j = at + 1;
    if bytes.get(j).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
        j += 1;
    }
    (bytes.get(j) == Some(&b'$')).then_some(j + 1)
}

/// The statements of `sql`, trimmed, without the empty ones (and without those that are only comments).
pub fn split_statements(sql: &str, provider: ProviderName) -> Vec<String> {
    let tokens = tokenize(sql, provider);
    let mssql = provider == ProviderName::Sqlserver;
    let block_tracking = provider != ProviderName::Postgres;
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut has_code = false;
    let mut depth = 0i32;
    let mut first_word: Option<String> = None;
    let mut line_start = true;
    let flush = |from: usize, to: usize, has_code: &mut bool, out: &mut Vec<String>| {
        let text = sql[from..to].trim();
        if *has_code && !text.is_empty() {
            out.push(text.to_string());
        }
        *has_code = false;
    };
    for (n, t) in tokens.iter().enumerate() {
        let text = &sql[t.start..t.end];
        match t.kind {
            Kind::Semicolon => {
                if depth <= 0 {
                    flush(start, t.start, &mut has_code, &mut out);
                    start = t.end;
                    depth = 0;
                    first_word = None;
                }
            }
            Kind::Space => {
                if text.contains('\n') {
                    line_start = true;
                }
                continue;
            }
            Kind::Comment => continue,
            Kind::Word => {
                let word = text.to_ascii_lowercase();
                if mssql && line_start && word == "go" && only_go_on_line(sql, t.end) {
                    flush(start, t.start, &mut has_code, &mut out);
                    start = skip_line(sql, t.end);
                    depth = 0;
                    first_word = None;
                    line_start = false;
                    continue;
                }
                has_code = true;
                if block_tracking {
                    match word.as_str() {
                        "begin" if first_word.is_some() && !next_word_is(&tokens, sql, n, &["transaction", "tran", "distributed"]) => depth += 1,
                        "case" => depth += 1,
                        "end" if depth > 0 => depth -= 1,
                        _ => {}
                    }
                }
                if first_word.is_none() {
                    first_word = Some(word.clone());
                }
            }
            Kind::Quoted | Kind::Punct => {
                has_code = true;
            }
        }
        line_start = false;
    }
    flush(start, sql.len(), &mut has_code, &mut out);
    out
}

/// Whether the rest of the line after `from` is blank (a `GO` batch separator, optionally with a count).
fn only_go_on_line(sql: &str, from: usize) -> bool {
    let rest = sql[from..].split('\n').next().unwrap_or("").trim();
    rest.is_empty() || rest.chars().all(|c| c.is_ascii_digit())
}

fn skip_line(sql: &str, from: usize) -> usize {
    match sql[from..].find('\n') {
        Some(p) => from + p + 1,
        None => sql.len(),
    }
}

fn next_word_is(tokens: &[Token], sql: &str, at: usize, words: &[&str]) -> bool {
    tokens[at + 1..]
        .iter()
        .find(|t| !matches!(t.kind, Kind::Space | Kind::Comment))
        .is_some_and(|t| t.kind == Kind::Word && words.contains(&sql[t.start..t.end].to_ascii_lowercase().as_str()))
}

/// Whether a statement is expected to return rows (a result set), by its first word — or, for a
/// DML statement, a top-level `RETURNING` / `OUTPUT`. Unknown statements are run as queries: one
/// that returns no column simply gives no result set.
pub fn returns_rows(statement: &str, provider: ProviderName) -> bool {
    let tokens = tokenize(statement, provider);
    let words: Vec<String> = tokens.iter().filter(|t| t.kind == Kind::Word).map(|t| statement[t.start..t.end].to_ascii_lowercase()).collect();
    let Some(first) = words.first() else { return false };
    match first.as_str() {
        "select" | "with" | "values" | "pragma" | "show" | "explain" | "describe" | "desc" | "table" | "exec" | "execute" | "call" | "list" | "help" | "check" | "analyze" => true,
        "insert" | "update" | "delete" | "merge" | "replace" => words.iter().any(|w| w == "returning" || w == "output"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQLITE: ProviderName = ProviderName::Sqlite;

    #[test]
    fn splits_on_top_level_semicolons_only() {
        let sql = "UPDATE t SET name = 'a;b' WHERE id = 1; -- trailing ; comment\nSELECT \"x;y\", `z;` FROM t; /* ; */ SELECT 1;;";
        let s = split_statements(sql, SQLITE);
        assert_eq!(s.len(), 3, "{s:?}");
        assert!(s[0].contains("'a;b'") && s[1].contains("\"x;y\""));
        assert_eq!(s[2], "/* ; */ SELECT 1");
    }

    #[test]
    fn comment_only_and_empty_statements_are_dropped() {
        assert!(split_statements("  ; -- nothing\n ;/* x */", SQLITE).is_empty());
        assert_eq!(split_statements("SELECT 1", SQLITE), ["SELECT 1"]);
    }

    #[test]
    fn a_doubled_quote_does_not_end_a_string() {
        let s = split_statements("SELECT 'it''s; fine'; SELECT 2", SQLITE);
        assert_eq!(s, ["SELECT 'it''s; fine'", "SELECT 2"]);
    }

    #[test]
    fn postgres_dollar_quotes_and_nested_comments() {
        let sql = "CREATE FUNCTION f() RETURNS int AS $body$ BEGIN RETURN 1; END; $body$ LANGUAGE plpgsql; SELECT $1, $$a;b$$; /* a /* nested ; */ ; */ SELECT 3";
        let s = split_statements(sql, ProviderName::Postgres);
        assert_eq!(s.len(), 3, "{s:?}");
        assert!(s[0].ends_with("plpgsql") && s[0].contains("RETURN 1; END;"));
        assert_eq!(s[1], "SELECT $1, $$a;b$$");
        // BEGIN as a statement is a transaction, not a block, on PostgreSQL.
        assert_eq!(split_statements("BEGIN; SELECT 1; COMMIT", ProviderName::Postgres), ["BEGIN", "SELECT 1", "COMMIT"]);
    }

    #[test]
    fn sqlite_trigger_bodies_and_case_blocks_stay_whole() {
        let sql = "CREATE TRIGGER tr AFTER INSERT ON t BEGIN UPDATE u SET n = n + 1; INSERT INTO log VALUES (CASE WHEN 1 THEN 'a' ELSE 'b' END); END; SELECT 1;";
        let s = split_statements(sql, SQLITE);
        assert_eq!(s.len(), 2, "{s:?}");
        assert!(s[0].ends_with("END"));
        assert_eq!(split_statements("BEGIN TRANSACTION; SELECT 1; COMMIT;", SQLITE), ["BEGIN TRANSACTION", "SELECT 1", "COMMIT"]);
    }

    #[test]
    fn mysql_backslash_escapes_and_hash_comments() {
        let s = split_statements("SELECT 'a\\'b;c'; # ; comment\nSELECT 2", ProviderName::Mysql);
        assert_eq!(s.len(), 2, "{s:?}");
        assert!(s[0].contains("a\\'b;c"));
    }

    #[test]
    fn sql_server_brackets_procedures_and_go() {
        let sql = "SELECT [a;b] FROM t;\nCREATE PROCEDURE p AS BEGIN SELECT 1; SELECT 2; END\nGO\nSELECT 3";
        let s = split_statements(sql, ProviderName::Sqlserver);
        assert_eq!(s.len(), 3, "{s:?}");
        assert!(s[1].starts_with("CREATE PROCEDURE") && s[1].ends_with("END"));
        assert_eq!(s[2], "SELECT 3");
        assert_eq!(split_statements("DECLARE @x int; SET @x = 1; SELECT @x", ProviderName::Sqlserver).len(), 3);
    }

    #[test]
    fn statements_that_return_rows() {
        for q in ["SELECT 1", "  -- c\n (SELECT 1)", "WITH x AS (SELECT 1) SELECT * FROM x", "PRAGMA table_info(t)", "INSERT INTO t VALUES (1) RETURNING id", "EXEC p"] {
            assert!(returns_rows(q, SQLITE) || returns_rows(q, ProviderName::Sqlserver), "{q}");
        }
        for q in ["UPDATE t SET a = 'returning'", "INSERT INTO t VALUES (1)", "CREATE TABLE x (a int)", "DELETE FROM t", ""] {
            assert!(!returns_rows(q, SQLITE), "{q}");
        }
    }
}
