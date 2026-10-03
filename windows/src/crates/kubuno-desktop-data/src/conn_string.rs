//! [`ConnectionStringBuilder`]: parses and writes connection strings in both usual forms —
//! `Host=db.example;Database=app;Username=app;Password={secret:DbPassword}` (ADO.NET style, keys
//! case-insensitive, with the common aliases) and URLs (`postgres://app@db.example/app?sslmode=require`,
//! `sqlite:app.db`) — and never shows a password: `Display` and `Debug` write `***` instead.

use std::fmt;

use crate::error::DataError;

/// The canonical key of each alias (lower case, spaces removed).
fn canonical(key: &str) -> String {
    let k: String = key.chars().filter(|c| !c.is_whitespace() && *c != '_').collect::<String>().to_ascii_lowercase();
    match k.as_str() {
        "server" | "host" | "address" | "addr" => "host".to_string(),
        "datasource" | "filename" | "file" => "datasource".to_string(),
        "database" | "initialcatalog" | "dbname" => "database".to_string(),
        "userid" | "username" | "user" | "uid" => "username".to_string(),
        "password" | "pwd" => "password".to_string(),
        "sslmode" | "ssl" => "sslmode".to_string(),
        "applicationname" => "applicationname".to_string(),
        other => other.to_string(),
    }
}

/// A parsed connection string (see the module doc).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ConnectionStringBuilder {
    /// Key/value form: `(key as written, value)`.
    entries: Vec<(String, String)>,
    /// URL form, kept verbatim.
    url: Option<String>,
}

impl ConnectionStringBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses either form. Values may be quoted (`Password='a;b'`, `"…"` with doubled quotes).
    pub fn parse(text: &str) -> Result<Self, DataError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(DataError::Config("the connection string is empty".to_string()));
        }
        if text.contains("://") || text.starts_with("sqlite:") {
            return Ok(Self { entries: Vec::new(), url: Some(text.to_string()) });
        }
        let mut entries = Vec::new();
        let mut chars = text.chars().peekable();
        loop {
            // Key.
            let mut key = String::new();
            while let Some(&c) = chars.peek() {
                if c == '=' || c == ';' {
                    break;
                }
                key.push(c);
                chars.next();
            }
            let key = key.trim().to_string();
            match chars.next() {
                None if key.is_empty() => break,
                Some(';') if key.is_empty() => continue,
                Some('=') => {}
                _ => return Err(DataError::Config(format!("the connection string entry `{key}` has no value"))),
            }
            if key.is_empty() {
                return Err(DataError::Config("the connection string has a value without a key".to_string()));
            }
            // Value.
            while chars.peek().is_some_and(|c| *c == ' ') {
                chars.next();
            }
            let mut value = String::new();
            match chars.peek().copied() {
                Some(q @ ('\'' | '"')) => {
                    chars.next();
                    let mut closed = false;
                    while let Some(c) = chars.next() {
                        if c == q {
                            if chars.peek() == Some(&q) {
                                value.push(q);
                                chars.next();
                            } else {
                                closed = true;
                                break;
                            }
                        } else {
                            value.push(c);
                        }
                    }
                    if !closed {
                        return Err(DataError::Config(format!("the value of `{key}` has an unclosed quote")));
                    }
                    while chars.peek().is_some_and(|c| *c != ';') {
                        if !chars.next().is_some_and(char::is_whitespace) {
                            return Err(DataError::Config(format!("unexpected text after the quoted value of `{key}`")));
                        }
                    }
                }
                _ => {
                    while let Some(&c) = chars.peek() {
                        if c == ';' {
                            break;
                        }
                        value.push(c);
                        chars.next();
                    }
                    value = value.trim().to_string();
                }
            }
            entries.push((key, value));
            if chars.next().is_none() {
                break;
            }
        }
        Ok(Self { entries, url: None })
    }

    /// Whether this is the URL form.
    pub fn is_url(&self) -> bool {
        self.url.is_some()
    }

    /// The URL, for the URL form.
    pub(crate) fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// The value of `key` (any alias, any case) in the key/value form.
    pub fn get(&self, key: &str) -> Option<&str> {
        let want = canonical(key);
        self.entries.iter().rev().find(|(k, _)| canonical(k) == want).map(|(_, v)| v.as_str())
    }

    /// Sets `key` (replacing any alias of it) in the key/value form.
    pub fn set(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
        let want = canonical(key);
        self.entries.retain(|(k, _)| canonical(k) != want);
        self.entries.push((key.to_string(), value.into()));
        self
    }

    pub fn host(&self) -> Option<String> {
        match &self.url {
            Some(url) => url_host(url),
            None => self.get("host").map(str::to_string),
        }
    }
    pub fn port(&self) -> Option<u16> {
        self.get("port").and_then(|p| p.trim().parse().ok())
    }
    pub fn database(&self) -> Option<&str> {
        self.get("database")
    }
    pub fn username(&self) -> Option<&str> {
        self.get("username")
    }
    pub fn data_source(&self) -> Option<&str> {
        self.get("datasource")
    }
    pub fn ssl_mode(&self) -> Option<String> {
        match &self.url {
            Some(url) => url_query(url, "sslmode"),
            None => self.get("sslmode").map(|s| s.to_ascii_lowercase()),
        }
    }
    /// The password as written (possibly a `{secret:…}` placeholder). Crate-private: nothing outside
    /// the connection code needs it.
    pub(crate) fn password(&self) -> Option<&str> {
        self.get("password")
    }

    /// Whether the string carries a password in clear (a `{secret:…}` placeholder does not count).
    pub fn has_literal_password(&self) -> bool {
        let literal = |p: &str| !p.is_empty() && !p.trim_start().starts_with("{secret:");
        match &self.url {
            Some(url) => url_password(url).is_some_and(|p| literal(&p)) || url_query(url, "password").is_some_and(|p| literal(&p)),
            None => self.password().is_some_and(literal),
        }
    }

    /// Whether the server is on this machine (no TLS needed by default): `localhost`, a loopback
    /// address, a socket path, or no host at all.
    pub fn is_local(&self) -> bool {
        match self.host() {
            None => true,
            Some(h) => {
                let h = h.trim().trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
                h.is_empty() || h == "localhost" || h == "::1" || h.starts_with("127.") || h.starts_with('/') || h == "."
            }
        }
    }

    /// The string with every secret in clear (tests: the round trip).
    #[cfg(test)]
    pub(crate) fn to_secret_string(&self) -> String {
        self.render(false)
    }

    /// The string with its secrets, for a driver that parses it itself (SQL Server). Never logged.
    #[cfg_attr(not(feature = "mssql"), allow(dead_code))]
    pub(crate) fn expose(&self) -> String {
        self.render(false)
    }

    fn render(&self, redact: bool) -> String {
        if let Some(url) = &self.url {
            return if redact { redact_url(url) } else { url.clone() };
        }
        let mut out = String::new();
        for (k, v) in &self.entries {
            let value = if redact && canonical(k) == "password" && !v.trim_start().starts_with("{secret:") { "***".to_string() } else { quote_value(v) };
            out.push_str(k);
            out.push('=');
            out.push_str(&value);
            out.push(';');
        }
        out
    }
}

/// Quotes a value when it needs it (`;`, a quote, leading/trailing spaces).
fn quote_value(v: &str) -> String {
    if v.contains(';') || v.contains('\'') || v.contains('"') || v.trim() != v {
        format!("'{}'", v.replace('\'', "''"))
    } else {
        v.to_string()
    }
}

/// The part of a URL between `://` and the path, and the userinfo inside it.
fn authority(url: &str) -> Option<(&str, Option<&str>)> {
    let rest = url.split_once("://")?.1;
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let auth = &rest[..end];
    match auth.rfind('@') {
        Some(at) => Some((&auth[at + 1..], Some(&auth[..at]))),
        None => Some((auth, None)),
    }
}

fn url_host(url: &str) -> Option<String> {
    let (hostport, _) = authority(url)?;
    let host = if let Some(stripped) = hostport.strip_prefix('[') { stripped.split(']').next().unwrap_or("") } else { hostport.split(':').next().unwrap_or("") };
    let host = if host.is_empty() { url_query(url, "host").unwrap_or_default() } else { host.to_string() };
    Some(host)
}

fn url_password(url: &str) -> Option<String> {
    let (_, user) = authority(url)?;
    user?.split_once(':').map(|(_, p)| p.to_string())
}

fn url_query(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        k.eq_ignore_ascii_case(name).then(|| v.to_string())
    })
}

fn redact_url(url: &str) -> String {
    let mut out = url.to_string();
    if let Some((_, Some(user))) = authority(url) {
        if let Some((name, _)) = user.split_once(':') {
            out = out.replacen(user, &format!("{name}:***"), 1);
        }
    }
    if let Some((base, query)) = out.clone().split_once('?') {
        let q: Vec<String> = query
            .split('&')
            .map(|pair| match pair.split_once('=') {
                Some((k, _)) if k.eq_ignore_ascii_case("password") => format!("{k}=***"),
                _ => pair.to_string(),
            })
            .collect();
        out = format!("{base}?{}", q.join("&"));
    }
    out
}

impl fmt::Display for ConnectionStringBuilder {
    /// The string with its password replaced by `***`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(true))
    }
}

impl fmt::Debug for ConnectionStringBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ConnectionStringBuilder").field(&self.render(true)).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_value_form_with_aliases_and_quotes() {
        let b = ConnectionStringBuilder::parse("Server=db.example; Port=5433;Initial Catalog=app;User Id=svc;Password='a;b''c'").expect("parse");
        assert_eq!(b.host().as_deref(), Some("db.example"));
        assert_eq!(b.port(), Some(5433));
        assert_eq!(b.database(), Some("app"));
        assert_eq!(b.username(), Some("svc"));
        assert_eq!(b.password(), Some("a;b'c"));
        assert!(b.has_literal_password());
        assert!(!b.is_local());
        let shown = b.to_string();
        assert!(shown.contains("Password=***") && !shown.contains("a;b"), "{shown}");
        assert!(!format!("{b:?}").contains("a;b"));
        let round = ConnectionStringBuilder::parse(&b.to_secret_string()).expect("round trip");
        assert_eq!(round.password(), Some("a;b'c"));
    }

    #[test]
    fn placeholders_are_not_literal_passwords() {
        let b = ConnectionStringBuilder::parse("Host=localhost;Password={secret:DbPassword}").expect("parse");
        assert!(!b.has_literal_password());
        assert!(b.is_local());
        assert!(b.to_string().contains("{secret:DbPassword}"));
    }

    #[test]
    fn url_form() {
        let b = ConnectionStringBuilder::parse("postgres://app:s3cret@db.example:5432/app?sslmode=verify-full").expect("parse");
        assert!(b.is_url());
        assert_eq!(b.host().as_deref(), Some("db.example"));
        assert_eq!(b.ssl_mode().as_deref(), Some("verify-full"));
        assert!(b.has_literal_password());
        let shown = b.to_string();
        assert_eq!(shown, "postgres://app:***@db.example:5432/app?sslmode=verify-full");
        let local = ConnectionStringBuilder::parse("postgres://app@localhost/app").expect("parse");
        assert!(local.is_local() && !local.has_literal_password());
        assert!(ConnectionStringBuilder::parse("sqlite::memory:").expect("parse").is_local());
    }

    #[test]
    fn malformed() {
        assert!(ConnectionStringBuilder::parse("").is_err());
        assert!(ConnectionStringBuilder::parse("Host").is_err());
        assert!(ConnectionStringBuilder::parse("Password='abc").is_err());
        assert!(ConnectionStringBuilder::parse("=x").is_err());
        let mut b = ConnectionStringBuilder::new();
        b.set("Data Source", "C:\\data\\app.db").set("filename", "D:\\other.db");
        assert_eq!(b.data_source(), Some("D:\\other.db"));
    }
}
