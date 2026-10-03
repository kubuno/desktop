//! What the tool needs to know about a connection string beyond kubuno-desktop-data's builder: the
//! provider a string implies, a redacted one-line description for the Data Explorer list, and the
//! URL form (`DATABASE_URL`) that sqlx's macros and CLI understand.

use kubuno_desktop_data::ConnectionStringBuilder;
use kubuno_desktop_data_model::ProviderName;

use crate::error::{ToolError, ToolResult};

/// The parts of a connection string. The password is kept apart: it never reaches a description.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnInfo {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    /// SQLite: the database file (or `:memory:`).
    pub file: Option<String>,
    pub ssl_mode: Option<String>,
}

impl ConnInfo {
    pub fn parse(text: &str) -> ToolResult<Self> {
        let text = text.trim();
        // The builder validates both forms (unclosed quotes, missing values…) with its own messages.
        let builder = ConnectionStringBuilder::parse(text)?;
        if let Some(rest) = strip_prefix_ci(text, "sqlite:") {
            let path = rest.strip_prefix("//").unwrap_or(rest);
            let path = path.split('?').next().unwrap_or("");
            return Ok(Self { file: Some(percent_decode(path)), ..Self::default() });
        }
        if builder.is_url() {
            return Ok(parse_url(text));
        }
        let (host, port_in_host) = split_host_port(builder.get("host"));
        Ok(Self {
            host,
            port: builder.port().or(port_in_host),
            database: builder.database().map(str::to_string),
            user: builder.username().map(str::to_string),
            password: builder.get("password").map(str::to_string),
            file: builder.data_source().filter(|_| builder.get("host").is_none()).map(str::to_string),
            ssl_mode: builder.ssl_mode(),
        })
    }

    /// The database's name as a person calls it: the file name for SQLite.
    pub fn database_name(&self) -> String {
        if let Some(file) = &self.file {
            return file.rsplit(['\\', '/']).next().unwrap_or(file).to_string();
        }
        self.database.clone().unwrap_or_default()
    }

    /// `localhost:5432/shop (user kubuno)`, `C:\db\shop.db` — never the password.
    pub fn display(&self) -> String {
        if let Some(file) = &self.file {
            return file.clone();
        }
        let mut out = self.host.clone().unwrap_or_else(|| "localhost".to_string());
        if let Some(p) = self.port {
            out.push_str(&format!(":{p}"));
        }
        if let Some(d) = self.database.as_deref().filter(|d| !d.is_empty()) {
            out.push('/');
            out.push_str(d);
        }
        if let Some(u) = self.user.as_deref().filter(|u| !u.is_empty()) {
            out.push_str(&format!(" (user {u})"));
        }
        out
    }
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| &text[prefix.len()..])
}

/// `tcp:host,1433` / `host,1433` / `host` → (host, port).
fn split_host_port(host: Option<&str>) -> (Option<String>, Option<u16>) {
    let Some(host) = host else { return (None, None) };
    let host = strip_prefix_ci(host, "tcp:").unwrap_or(host);
    match host.rsplit_once(',') {
        Some((h, p)) => (Some(h.trim().to_string()), p.trim().parse().ok()),
        None => (Some(host.trim().to_string()), None),
    }
}

fn parse_url(url: &str) -> ConnInfo {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let (before_query, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (authority, path) = before_query.split_once('/').unwrap_or((before_query, ""));
    let (userinfo, hostport) = match authority.rfind('@') {
        Some(at) => (Some(&authority[..at]), &authority[at + 1..]),
        None => (None, authority),
    };
    let (user, password) = match userinfo {
        Some(u) => match u.split_once(':') {
            Some((n, p)) => (Some(percent_decode(n)), Some(percent_decode(p))),
            None => (Some(percent_decode(u)), None),
        },
        None => (None, None),
    };
    let (host, port) = if let Some(inner) = hostport.strip_prefix('[') {
        let (h, after) = inner.split_once(']').unwrap_or((inner, ""));
        (h.to_string(), after.strip_prefix(':').and_then(|p| p.parse().ok()))
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().ok()),
            None => (hostport.to_string(), None),
        }
    };
    let ssl_mode = query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        k.eq_ignore_ascii_case("sslmode").then(|| v.to_string())
    });
    ConnInfo {
        host: Some(host).filter(|h| !h.is_empty()),
        port,
        database: Some(percent_decode(path)).filter(|d| !d.is_empty()),
        user: user.filter(|u| !u.is_empty()),
        password: password.filter(|p| !p.is_empty()),
        file: None,
        ssl_mode,
    }
}

/// The provider a connection string implies, for callers that did not name one.
pub fn infer_provider(text: &str) -> ToolResult<ProviderName> {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("sqlite:") {
        return Ok(ProviderName::Sqlite);
    }
    if lower.starts_with("postgres://") || lower.starts_with("postgresql://") {
        return Ok(ProviderName::Postgres);
    }
    if lower.starts_with("mysql://") || lower.starts_with("mariadb://") {
        return Ok(ProviderName::Mysql);
    }
    if lower.starts_with("mssql://") || lower.starts_with("sqlserver://") {
        return Ok(ProviderName::Sqlserver);
    }
    let keys: Vec<String> = t.split(';').filter_map(|p| p.split_once('=')).map(|(k, _)| k.trim().to_ascii_lowercase().replace([' ', '_'], "")).collect();
    let has = |names: &[&str]| keys.iter().any(|k| names.contains(&k.as_str()));
    if has(&["server", "initialcatalog", "integratedsecurity", "trustedconnection", "trustservercertificate", "encrypt"]) {
        return Ok(ProviderName::Sqlserver);
    }
    if has(&["host"]) {
        return Ok(ProviderName::Postgres);
    }
    if has(&["datasource", "filename"]) {
        return Ok(ProviderName::Sqlite);
    }
    Err(ToolError::validation("cannot tell the provider from the connection string: name it (sqlite, postgres, mysql, sqlserver)"))
}

/// The `DATABASE_URL` for sqlx's macros and CLI. A secret: only for a child process's environment.
pub fn database_url(provider: ProviderName, text: &str) -> ToolResult<String> {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    match provider {
        ProviderName::Sqlserver => Err(ToolError::config("sqlx has no SQL Server driver: the offline query cache is for PostgreSQL, SQLite and MySQL")),
        ProviderName::Sqlite => {
            if lower.starts_with("sqlite:") {
                return Ok(text.to_string());
            }
            let info = ConnInfo::parse(text)?;
            let file = info.file.ok_or_else(|| ToolError::config("a SQLite connection string needs `Data Source=<file>` or a `sqlite:` URL"))?;
            Ok(format!("sqlite:{}", file.replace('\\', "/")))
        }
        ProviderName::Postgres | ProviderName::Mysql => {
            let scheme = if provider == ProviderName::Postgres { "postgres" } else { "mysql" };
            if text.contains("://") {
                return Ok(text.to_string());
            }
            let info = ConnInfo::parse(text)?;
            let mut url = format!("{scheme}://");
            if let Some(u) = &info.user {
                url.push_str(&percent_encode(u));
                if let Some(p) = &info.password {
                    url.push(':');
                    url.push_str(&percent_encode(p));
                }
                url.push('@');
            }
            url.push_str(info.host.as_deref().unwrap_or("localhost"));
            if let Some(p) = info.port {
                url.push_str(&format!(":{p}"));
            }
            if let Some(d) = &info.database {
                url.push('/');
                url.push_str(&percent_encode(d));
            }
            if let Some(m) = &info.ssl_mode {
                url.push_str(&format!("?sslmode={}", percent_encode(m)));
            }
            Ok(url)
        }
    }
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(b) = hex {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_never_show_the_password() {
        let i = ConnInfo::parse("postgres://kubuno:S3cr3t!@localhost:5432/shop?sslmode=require").expect("url");
        assert_eq!(i.display(), "localhost:5432/shop (user kubuno)");
        assert_eq!(i.password.as_deref(), Some("S3cr3t!"));
        let kv = ConnInfo::parse("Host=db;Port=5433;Database=app;Username=svc;Password='a;b'").expect("kv");
        assert_eq!(kv.display(), "db:5433/app (user svc)");
        assert!(!kv.display().contains("a;b"));
        let lite = ConnInfo::parse("Data Source=C:\\db\\shop.db").expect("sqlite kv");
        assert_eq!((lite.display().as_str(), lite.database_name().as_str()), ("C:\\db\\shop.db", "shop.db"));
        let url = ConnInfo::parse("sqlite:C:\\db\\shop.db?mode=rwc").expect("sqlite url");
        assert_eq!(url.file.as_deref(), Some("C:\\db\\shop.db"));
        let ms = ConnInfo::parse("Server=tcp:sql.example,1444;Database=erp;User Id=sa;Password=x1y2z3").expect("mssql");
        assert_eq!(ms.display(), "sql.example:1444/erp (user sa)");
    }

    #[test]
    fn providers_are_inferred() {
        assert_eq!(infer_provider("sqlite::memory:").expect("p"), ProviderName::Sqlite);
        assert_eq!(infer_provider("postgresql://u@h/d").expect("p"), ProviderName::Postgres);
        assert_eq!(infer_provider("Host=h;Database=d").expect("p"), ProviderName::Postgres);
        assert_eq!(infer_provider("Server=h;Database=d;User Id=sa").expect("p"), ProviderName::Sqlserver);
        assert_eq!(infer_provider("Data Source=x.db").expect("p"), ProviderName::Sqlite);
        assert_eq!(infer_provider("mysql://u@h/d").expect("p"), ProviderName::Mysql);
        assert!(infer_provider("nonsense").is_err());
    }

    #[test]
    fn database_urls() {
        assert_eq!(database_url(ProviderName::Sqlite, "Data Source=C:\\db\\shop.db").expect("u"), "sqlite:C:/db/shop.db");
        assert_eq!(database_url(ProviderName::Sqlite, "sqlite:x.db").expect("u"), "sqlite:x.db");
        assert_eq!(
            database_url(ProviderName::Postgres, "Host=db;Port=5433;Database=app;Username=svc;Password='p@ss:w/rd'").expect("u"),
            "postgres://svc:p%40ss%3Aw%2Frd@db:5433/app"
        );
        assert_eq!(database_url(ProviderName::Mysql, "Server=h;Database=d;User=u;SslMode=required").expect("u"), "mysql://u@h/d?sslmode=required");
        assert!(database_url(ProviderName::Sqlserver, "Server=h").is_err());
        assert_eq!(percent_decode("a%40b%2"), "a@b%2");
    }
}
