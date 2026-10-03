//! The `Filter` and `Sort` languages of a binding source (ADO.NET `RowFilter` / `Sort`, a subset),
//! evaluated in memory:
//!
//! ```text
//! Name LIKE 'A%' AND (Age >= 18 OR Vip = true) AND Email IS NOT NULL AND Country IN ('FR', 'BE')
//! Name ASC, Age DESC
//! ```
//!
//! Comparisons `= <> != < <= > >=`, `LIKE` (`%` any run, `_` one character, case-insensitive),
//! `IS [NOT] NULL`, `IN (…)`, `NOT`, `AND`, `OR`, parentheses; literals `'text'` (`''` escapes a
//! quote), numbers, `true`/`false`, `NULL`; column names bare or `[bracketed]`. Text comparisons are
//! case-insensitive. A malformed expression is an error when it is set, never while painting.

use std::cmp::Ordering;

use crate::error::DataError;
use crate::value::DbValue;

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Num(f64),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
}

fn lex(text: &str) -> Result<Vec<Tok>, DataError> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '\'' => {
                let mut s = String::new();
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return Err(DataError::Validation("a text in the filter is not closed".to_string())),
                        Some('\'') if chars.get(i + 1) == Some(&'\'') => {
                            s.push('\'');
                            i += 2;
                        }
                        Some('\'') => {
                            i += 1;
                            break;
                        }
                        Some(&ch) => {
                            s.push(ch);
                            i += 1;
                        }
                    }
                }
                out.push(Tok::Str(s));
            }
            '[' => {
                let end = chars[i..].iter().position(|&ch| ch == ']').ok_or_else(|| DataError::Validation("a [column] in the filter is not closed".to_string()))?;
                out.push(Tok::Ident(chars[i + 1..i + end].iter().collect()));
                i += end + 1;
            }
            '<' | '>' | '=' | '!' => {
                let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
                let op = match two.as_str() {
                    "<=" => Some("<="),
                    ">=" => Some(">="),
                    "<>" | "!=" => Some("<>"),
                    _ => None,
                };
                if let Some(op) = op {
                    out.push(Tok::Op(op));
                    i += 2;
                } else {
                    out.push(Tok::Op(match c {
                        '<' => "<",
                        '>' => ">",
                        '=' => "=",
                        _ => return Err(DataError::Validation("unexpected '!' in the filter".to_string())),
                    }));
                    i += 1;
                }
            }
            c if c.is_ascii_digit() || (c == '-' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())) || c == '.' => {
                let start = i;
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                out.push(Tok::Num(s.parse().map_err(|_| DataError::Validation(format!("`{s}` is not a number")))?));
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                out.push(Tok::Ident(chars[start..i].iter().collect()));
            }
            other => return Err(DataError::Validation(format!("unexpected `{other}` in the filter"))),
        }
    }
    Ok(out)
}

/// A comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// A parsed filter.
#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    And(Box<Filter>, Box<Filter>),
    Or(Box<Filter>, Box<Filter>),
    Not(Box<Filter>),
    Compare { column: String, op: CmpOp, value: DbValue },
    Like { column: String, pattern: String },
    IsNull { column: String, negated: bool },
    In { column: String, values: Vec<DbValue> },
}

/// The public face of [`Cmp`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CmpOp(Cmp);

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn keyword(&self, kw: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(s)) if s.eq_ignore_ascii_case(kw))
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn or(&mut self) -> Result<Filter, DataError> {
        let mut left = self.and()?;
        while self.keyword("OR") {
            self.pos += 1;
            left = Filter::Or(Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Filter, DataError> {
        let mut left = self.unary()?;
        while self.keyword("AND") {
            self.pos += 1;
            left = Filter::And(Box::new(left), Box::new(self.unary()?));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Filter, DataError> {
        if self.keyword("NOT") {
            self.pos += 1;
            return Ok(Filter::Not(Box::new(self.unary()?)));
        }
        if self.peek() == Some(&Tok::LParen) {
            self.pos += 1;
            let inner = self.or()?;
            if self.next() != Some(Tok::RParen) {
                return Err(DataError::Validation("a parenthesis in the filter is not closed".to_string()));
            }
            return Ok(inner);
        }
        self.comparison()
    }

    fn literal(&mut self) -> Result<DbValue, DataError> {
        match self.next() {
            Some(Tok::Str(s)) => Ok(DbValue::Text(s)),
            Some(Tok::Num(n)) => Ok(if n.fract() == 0.0 && n.abs() < 9.0e15 { DbValue::Int(n as i64) } else { DbValue::Float(n) }),
            Some(Tok::Ident(s)) if s.eq_ignore_ascii_case("true") => Ok(DbValue::Bool(true)),
            Some(Tok::Ident(s)) if s.eq_ignore_ascii_case("false") => Ok(DbValue::Bool(false)),
            Some(Tok::Ident(s)) if s.eq_ignore_ascii_case("null") => Ok(DbValue::Null),
            _ => Err(DataError::Validation("the filter expects a value ('text', a number, true, false or NULL)".to_string())),
        }
    }

    fn comparison(&mut self) -> Result<Filter, DataError> {
        let column = match self.next() {
            Some(Tok::Ident(c)) => c,
            _ => return Err(DataError::Validation("the filter expects a column name".to_string())),
        };
        if self.keyword("LIKE") {
            self.pos += 1;
            return match self.next() {
                Some(Tok::Str(pattern)) => Ok(Filter::Like { column, pattern }),
                _ => Err(DataError::Validation("LIKE expects a 'pattern'".to_string())),
            };
        }
        if self.keyword("IS") {
            self.pos += 1;
            let negated = self.keyword("NOT");
            if negated {
                self.pos += 1;
            }
            if !self.keyword("NULL") {
                return Err(DataError::Validation("IS expects NULL or NOT NULL".to_string()));
            }
            self.pos += 1;
            return Ok(Filter::IsNull { column, negated });
        }
        if self.keyword("IN") {
            self.pos += 1;
            if self.next() != Some(Tok::LParen) {
                return Err(DataError::Validation("IN expects a list in parentheses".to_string()));
            }
            let mut values = vec![self.literal()?];
            loop {
                match self.next() {
                    Some(Tok::Comma) => values.push(self.literal()?),
                    Some(Tok::RParen) => break,
                    _ => return Err(DataError::Validation("the IN list is not closed".to_string())),
                }
            }
            return Ok(Filter::In { column, values });
        }
        let op = match self.next() {
            Some(Tok::Op("=")) => Cmp::Eq,
            Some(Tok::Op("<>")) => Cmp::Ne,
            Some(Tok::Op("<")) => Cmp::Lt,
            Some(Tok::Op("<=")) => Cmp::Le,
            Some(Tok::Op(">")) => Cmp::Gt,
            Some(Tok::Op(">=")) => Cmp::Ge,
            _ => return Err(DataError::Validation(format!("the filter expects a comparison after `{column}`"))),
        };
        Ok(Filter::Compare { column, op: CmpOp(op), value: self.literal()? })
    }
}

/// Compares for filtering: numbers numerically, text case-insensitively, a number with a text that
/// parses as one numerically. `None` when either side is NULL or they cannot be compared.
fn compare(a: &DbValue, b: &DbValue) -> Option<Ordering> {
    match (a, b) {
        (DbValue::Null, _) | (_, DbValue::Null) => None,
        (DbValue::Text(x), DbValue::Text(y)) => Some(x.to_lowercase().cmp(&y.to_lowercase())),
        (DbValue::Bool(x), DbValue::Bool(y)) => Some(x.cmp(y)),
        (x, y) => match (x.as_f64(), y.as_f64()) {
            (Some(p), Some(q)) => p.partial_cmp(&q),
            _ => Some(x.to_display().to_lowercase().cmp(&y.to_display().to_lowercase())),
        },
    }
}

fn like(text: &str, pattern: &str) -> bool {
    fn go(t: &[char], p: &[char]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some(('%', rest)) => (0..=t.len()).any(|i| go(&t[i..], rest)),
            Some(('_', rest)) => !t.is_empty() && go(&t[1..], rest),
            Some((c, rest)) => t.first().is_some_and(|x| x == c) && go(&t[1..], rest),
        }
    }
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    go(&t, &p)
}

impl Filter {
    /// Parses a filter expression (an empty text is no filter: `Ok(None)`).
    pub fn parse(text: &str) -> Result<Option<Filter>, DataError> {
        let toks = lex(text)?;
        if toks.is_empty() {
            return Ok(None);
        }
        let mut p = Parser { toks, pos: 0 };
        let f = p.or()?;
        if p.pos < p.toks.len() {
            return Err(DataError::Validation("unexpected text at the end of the filter".to_string()));
        }
        Ok(Some(f))
    }

    /// The column names the filter uses.
    pub fn columns(&self) -> Vec<&str> {
        match self {
            Filter::And(a, b) | Filter::Or(a, b) => {
                let mut v = a.columns();
                v.extend(b.columns());
                v
            }
            Filter::Not(a) => a.columns(),
            Filter::Compare { column, .. } | Filter::Like { column, .. } | Filter::IsNull { column, .. } | Filter::In { column, .. } => vec![column.as_str()],
        }
    }

    /// Whether a row passes; `get` reads a column's value by name.
    pub fn matches<'a>(&self, get: &dyn Fn(&str) -> Option<&'a DbValue>) -> bool {
        let value = |c: &str| get(c).cloned().unwrap_or(DbValue::Null);
        match self {
            Filter::And(a, b) => a.matches(get) && b.matches(get),
            Filter::Or(a, b) => a.matches(get) || b.matches(get),
            Filter::Not(a) => !a.matches(get),
            Filter::IsNull { column, negated } => value(column).is_null() != *negated,
            Filter::Like { column, pattern } => {
                let v = value(column);
                !v.is_null() && like(&v.to_display(), pattern)
            }
            Filter::In { column, values } => {
                let v = value(column);
                values.iter().any(|x| compare(&v, x) == Some(Ordering::Equal))
            }
            Filter::Compare { column, op, value: lit } => {
                let Some(ord) = compare(&value(column), lit) else { return false };
                match op.0 {
                    Cmp::Eq => ord == Ordering::Equal,
                    Cmp::Ne => ord != Ordering::Equal,
                    Cmp::Lt => ord == Ordering::Less,
                    Cmp::Le => ord != Ordering::Greater,
                    Cmp::Gt => ord == Ordering::Greater,
                    Cmp::Ge => ord != Ordering::Less,
                }
            }
        }
    }
}

/// A parsed sort: `(column, descending)` keys, applied in order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SortSpec(pub Vec<(String, bool)>);

impl SortSpec {
    /// Parses `Name ASC, Age DESC` (an empty text is no sort).
    pub fn parse(text: &str) -> Result<SortSpec, DataError> {
        let mut keys = Vec::new();
        for part in text.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let mut words = part.split_whitespace();
            let column = words.next().unwrap_or("").trim_start_matches('[').trim_end_matches(']').to_string();
            let desc = match words.next().map(str::to_ascii_uppercase).as_deref() {
                None | Some("ASC") => false,
                Some("DESC") => true,
                Some(other) => return Err(DataError::Validation(format!("`{other}` is not ASC or DESC"))),
            };
            if words.next().is_some() || column.is_empty() {
                return Err(DataError::Validation(format!("`{part}` is not a sort key (Column [ASC|DESC])")));
            }
            keys.push((column, desc));
        }
        Ok(SortSpec(keys))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row<'a>(pairs: &'a [(&'a str, DbValue)]) -> impl Fn(&str) -> Option<&'a DbValue> {
        move |c| pairs.iter().find(|(n, _)| n.eq_ignore_ascii_case(c)).map(|(_, v)| v)
    }

    #[test]
    fn filters_evaluate() {
        let data = [("Name", DbValue::Text("Ada Lovelace".into())), ("Age", DbValue::Int(36)), ("Email", DbValue::Null), ("Country", DbValue::Text("fr".into()))];
        let get = row(&data);
        let check = |f: &str| Filter::parse(f).expect("parse").expect("some").matches(&get);
        assert!(check("Name LIKE 'ada%'"));
        assert!(check("Name LIKE '%love_ace'"));
        assert!(!check("Name LIKE 'Grace%'"));
        assert!(check("Age >= 18 AND Age < 40"));
        assert!(check("Age = '36'"));
        assert!(check("Email IS NULL AND NOT (Age > 50)"));
        assert!(!check("Email IS NOT NULL"));
        assert!(check("Country IN ('FR', 'BE')"));
        assert!(check("[Name] <> 'x' OR Age = 1"));
        assert!(!check("Email = 'a'"), "NULL never compares equal");
        assert_eq!(Filter::parse("  ").expect("empty"), None);
    }

    #[test]
    fn malformed_filters_are_errors() {
        for bad in ["Name LIKE", "Age >", "(Age > 1", "Name = 'x", "Age > 1 extra", "= 3", "Age IN (1, 2", "Age ! 3"] {
            assert!(Filter::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn sort_specs() {
        assert_eq!(SortSpec::parse("Name, Age DESC").expect("ok").0, vec![("Name".to_string(), false), ("Age".to_string(), true)]);
        assert!(SortSpec::parse("Name UP").is_err());
        assert!(SortSpec::parse("").expect("ok").is_empty());
    }
}
