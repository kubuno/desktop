//! The connection component (`vskubuno/docs/DATA.md` §3): `<DbConnection x:Name="db" Provider="Postgres"
//! ConnectionStringName="Northwind" Schema="crm"/>`. It resolves its connection string through the
//! secrets chain (never from the view), owns a pool that is created on the data runtime on first use
//! (with a retry policy), and raises `StateChange` on the UI thread.

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use kubuno_desktop_views::prelude::*;

use crate::conn_string::ConnectionStringBuilder;
use crate::error::{log_db, logged, DataError};
use crate::events::{emit, ConnectionState, StateChangeEventArgs};
use crate::provider::{Pool, Provider};
use crate::rt::{self, DataTask};
use crate::secrets::SecretResolver;

/// How transient failures are retried: `count` more attempts, waiting `delay`, then twice as long
/// each time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub count: u32,
    pub delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self { count: 3, delay: Duration::from_millis(200) }
    }
}

impl RetryPolicy {
    /// Runs `op` until it succeeds, fails with a non-transient error, or the attempts run out.
    pub async fn run<T, F, Fut>(&self, what: &str, mut op: F) -> Result<T, DataError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, DataError>>,
    {
        let mut attempt = 0u32;
        loop {
            match op().await {
                Ok(v) => return Ok(v),
                Err(e) if e.is_transient() && attempt < self.count => {
                    let wait = self.delay.saturating_mul(1u32 << attempt.min(10));
                    tracing::warn!(target: "kubuno_desktop_data", what, attempt = attempt + 1, wait_ms = wait.as_millis() as u64, error = %e, "transient failure, retrying");
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// What a resolved connection needs on the data runtime.
struct Settings {
    name: String,
    provider: Provider,
    /// The resolved connection string (secrets expanded): never logged, never shown.
    target: ConnectionStringBuilder,
    schema: Option<String>,
    max_pool_size: u32,
    connect_timeout: Duration,
    command_timeout: Duration,
    retry: RetryPolicy,
}

struct HandleInner {
    settings: Settings,
    pool: tokio::sync::Mutex<Option<Pool>>,
    state: AtomicU8,
    hinted: AtomicBool,
}

/// A resolved connection, shared by the commands and adapters that use it (`Send + Sync`; cloning
/// shares the pool).
#[derive(Clone)]
pub struct ConnectionHandle {
    inner: Arc<HandleInner>,
}

impl std::fmt::Debug for ConnectionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The resolved connection string is a secret: only the redacted form is shown.
        f.debug_struct("ConnectionHandle")
            .field("name", &self.inner.settings.name)
            .field("provider", &self.inner.settings.provider)
            .field("target", &self.inner.settings.target)
            .field("state", &self.state())
            .finish()
    }
}

impl ConnectionHandle {
    pub fn name(&self) -> &str {
        &self.inner.settings.name
    }

    pub fn provider(&self) -> Provider {
        self.inner.settings.provider
    }

    pub fn state(&self) -> ConnectionState {
        ConnectionState::from_u8(self.inner.state.load(Ordering::Acquire))
    }

    pub(crate) fn set_state(&self, s: ConnectionState) {
        self.inner.state.store(s.to_u8(), Ordering::Release);
    }

    pub fn command_timeout(&self) -> Duration {
        self.inner.settings.command_timeout
    }

    pub fn retry(&self) -> RetryPolicy {
        self.inner.settings.retry
    }

    /// The pool, created on first use (on the data runtime) with the retry policy.
    pub(crate) async fn pool(&self) -> Result<Pool, DataError> {
        let mut slot = self.inner.pool.lock().await;
        if let Some(p) = slot.as_ref() {
            return Ok(p.clone());
        }
        self.set_state(ConnectionState::Connecting);
        let settings = &self.inner.settings;
        let result = settings.retry.run("connect", || self.connect()).await;
        match result {
            Ok(pool) => {
                self.set_state(ConnectionState::Open);
                tracing::info!(target: "kubuno_desktop_data", connection = %settings.name, provider = settings.provider.as_str(), "connected");
                #[cfg(feature = "postgres")]
                if let Pool::Postgres(pg) = &pool {
                    if !self.inner.hinted.swap(true, Ordering::AcqRel) {
                        crate::provider::least_privilege_hint(pg, &settings.name).await;
                    }
                }
                *slot = Some(pool.clone());
                Ok(pool)
            }
            Err(e) => {
                self.set_state(ConnectionState::Broken);
                Err(e)
            }
        }
    }

    async fn connect(&self) -> Result<Pool, DataError> {
        let s = &self.inner.settings;
        match s.provider {
            #[cfg(feature = "postgres")]
            Provider::Postgres => {
                use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
                let mut options = match s.target.url() {
                    Some(url) => PgConnectOptions::from_str(url).map_err(|e| log_db("connection string", e))?,
                    None => {
                        let mut o = PgConnectOptions::new_without_pgpass();
                        if let Some(h) = s.target.host() {
                            o = o.host(&h);
                        }
                        if let Some(p) = s.target.port() {
                            o = o.port(p);
                        }
                        if let Some(d) = s.target.database() {
                            o = o.database(d);
                        }
                        if let Some(u) = s.target.username() {
                            o = o.username(u);
                        }
                        if let Some(p) = s.target.password() {
                            o = o.password(p);
                        }
                        if let Some(m) = s.target.ssl_mode() {
                            o = o.ssl_mode(PgSslMode::from_str(&m).map_err(|_| DataError::Config(format!("unknown SSL mode `{m}`")))?);
                        }
                        if let Some(app) = s.target.get("applicationname") {
                            o = o.application_name(app);
                        }
                        o
                    }
                };
                // TLS by default for remote servers (DATA.md "Security rules").
                if s.target.ssl_mode().is_none() && !s.target.is_local() {
                    options = options.ssl_mode(PgSslMode::Require);
                }
                let search_path = match &s.schema {
                    Some(schema) => Some(format!("SET search_path TO {}", crate::sql::quote_identifier(schema, false)?)),
                    None => None,
                };
                let pool = PgPoolOptions::new()
                    .max_connections(s.max_pool_size.max(1))
                    .acquire_timeout(s.connect_timeout)
                    .after_connect(move |conn, _meta| {
                        let search_path = search_path.clone();
                        Box::pin(async move {
                            if let Some(sql) = search_path {
                                sqlx::Executor::execute(conn, sql.as_str()).await?;
                            }
                            Ok(())
                        })
                    })
                    .connect_with(options)
                    .await
                    .map_err(|e| log_db("connect", e))?;
                Ok(Pool::Postgres(pool))
            }
            #[cfg(feature = "sqlite")]
            Provider::Sqlite => {
                use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
                // `Data Source=app:<name>` / `account:<name>` (and `sqlite:app:<name>`): a local database of the app
                // (vskubuno docs/STORAGE-COMPONENTS.md §3.5, `LocalDatabase`), never a path written in the view.
                let local = s.target.url().and_then(|u| u.strip_prefix("sqlite:")).or(s.target.data_source()).and_then(kubuno_desktop_app_storage::files::local_database_path);
                let (options, memory) = match (s.target.url(), s.target.data_source()) {
                    _ if local.is_some() => match local {
                        Some(Ok(path)) => (SqliteConnectOptions::new().filename(path), false),
                        Some(Err(e)) => return Err(logged("connect", DataError::Config(format!("the local database: {e}")))),
                        None => return Err(logged("connect", DataError::Config("the local database".to_string()))),
                    },
                    (Some(url), _) => {
                        let memory = url.contains(":memory:") || url.contains("mode=memory");
                        (SqliteConnectOptions::from_str(url).map_err(|e| log_db("connection string", e))?, memory)
                    }
                    (None, Some(path)) if path.trim() == ":memory:" => (SqliteConnectOptions::from_str("sqlite::memory:").map_err(|e| log_db("connection string", e))?, true),
                    (None, Some(path)) => (SqliteConnectOptions::new().filename(path.trim()), false),
                    (None, None) => return Err(logged("connect", DataError::Config("a SQLite connection string needs `Data Source=<file>` or a `sqlite:` URL".to_string()))),
                };
                let options = options.create_if_missing(true).foreign_keys(true).busy_timeout(s.connect_timeout);
                let mut pool = SqlitePoolOptions::new().acquire_timeout(s.connect_timeout);
                pool = if memory {
                    // Every connection to `:memory:` is a new database: keep exactly one, forever.
                    pool.max_connections(1).min_connections(1).idle_timeout(None).max_lifetime(None)
                } else {
                    pool.max_connections(s.max_pool_size.max(1))
                };
                let pool = pool.connect_with(options).await.map_err(|e| log_db("connect", e))?;
                Ok(Pool::Sqlite(pool))
            }
            #[cfg(feature = "mysql")]
            Provider::MySql => {
                use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode};
                let mut options = match s.target.url() {
                    Some(url) => MySqlConnectOptions::from_str(url).map_err(|e| log_db("connection string", e))?,
                    None => {
                        let mut o = MySqlConnectOptions::new();
                        if let Some(h) = s.target.host() {
                            o = o.host(&h);
                        }
                        if let Some(p) = s.target.port() {
                            o = o.port(p);
                        }
                        if let Some(d) = s.target.database() {
                            o = o.database(d);
                        }
                        if let Some(u) = s.target.username() {
                            o = o.username(u);
                        }
                        if let Some(p) = s.target.password() {
                            o = o.password(p);
                        }
                        if let Some(m) = s.target.ssl_mode() {
                            o = o.ssl_mode(MySqlSslMode::from_str(&m).map_err(|_| DataError::Config(format!("unknown SSL mode `{m}`")))?);
                        }
                        o
                    }
                };
                // TLS by default for remote servers (DATA.md "Security rules").
                if s.target.ssl_mode().is_none() && !s.target.is_local() {
                    options = options.ssl_mode(MySqlSslMode::Required);
                }
                // MySQL's schema is its database: `Schema` selects it on every pooled connection.
                let use_schema = match &s.schema {
                    Some(schema) => Some(format!("USE {}", crate::sql::quote_for(Provider::MySql, schema, false)?)),
                    None => None,
                };
                let pool = MySqlPoolOptions::new()
                    .max_connections(s.max_pool_size.max(1))
                    .acquire_timeout(s.connect_timeout)
                    .after_connect(move |conn, _meta| {
                        let use_schema = use_schema.clone();
                        Box::pin(async move {
                            if let Some(sql) = use_schema {
                                sqlx::Executor::execute(conn, sql.as_str()).await?;
                            }
                            Ok(())
                        })
                    })
                    .connect_with(options)
                    .await
                    .map_err(|e| log_db("connect", e))?;
                Ok(Pool::MySql(pool))
            }
            #[cfg(feature = "mssql")]
            Provider::SqlServer => {
                if s.target.is_url() {
                    return Err(logged("connect", DataError::Config("a SQL Server connection string uses the `Server=…;Database=…;` form".to_string())));
                }
                let mut config = tiberius::Config::from_ado_string(&s.target.expose()).map_err(|_| logged("connect", DataError::Config("the connection string is not valid for SQL Server".to_string())))?;
                let server = s.target.get("host").or_else(|| s.target.get("datasource")).unwrap_or_default().to_string();
                // TLS by default for remote servers: `Encrypt` unset means required.
                if s.target.get("encrypt").is_none() && !crate::mssql::is_local_server(&server) {
                    config.encryption(tiberius::EncryptionLevel::Required);
                }
                if s.schema.is_some() {
                    tracing::warn!(target: "kubuno_desktop_data", connection = %s.name, "SQL Server has no search path: `Schema` is ignored, qualify the names (schema.table)");
                }
                let pool = crate::mssql::MsPool::connect(config, s.max_pool_size, s.connect_timeout).await?;
                Ok(Pool::SqlServer(pool))
            }
            #[allow(unreachable_patterns)] // Every provider compiled in.
            other => Err(logged("connect", DataError::Config(format!("the {} provider is not available in this build", other.as_str())))),
        }
    }

    /// Closes the pool (its connections are closed on the data runtime).
    pub fn close(&self) -> DataTask<()> {
        let me = self.clone();
        rt::spawn(async move {
            let pool = me.inner.pool.lock().await.take();
            if let Some(p) = pool {
                p.close().await;
            }
            me.set_state(ConnectionState::Closed);
            Ok(())
        })
    }

    /// Runs `sql` (with `@name` or native positional parameters) and returns the affected rows.
    pub fn execute(&self, sql: &str, params: &[(&str, crate::DbValue)]) -> DataTask<u64> {
        let mut cmd = crate::command::DbCommand::with_text(sql);
        for (n, v) in params {
            cmd.param(n, v.clone());
        }
        cmd.execute_non_query(self)
    }
}

/// `<DbConnection>`: a database connection (see the module doc).
#[derive(Component)]
#[kubuno(extends = Component)]
#[toolbox(icon = "database", category = "Data")]
#[default_event("StateChange")]
#[default_property("ConnectionStringName")]
pub struct DbConnection {
    base: ComponentCore,
    /// The database: Postgres, Sqlite, MySql (MySQL / MariaDB, feature mysql), SqlServer (feature mssql).
    #[property]
    #[category("Data")]
    #[default_value("Postgres")]
    pub provider: Provider,
    /// The name of the connection string, looked up in the environment (ConnectionStrings__Name), the Windows Credential Manager and the user secrets. Credentials never go in the view.
    #[property]
    #[category("Data")]
    pub connection_string_name: String,
    /// A connection string without credentials (for example Data Source=app.db for SQLite). Use ConnectionStringName for anything that needs a password.
    #[property]
    #[category("Data")]
    pub connection_string: String,
    /// PostgreSQL: the schema used for unqualified names (search_path). A Kubuno module uses its own schema.
    #[property]
    #[category("Data")]
    pub schema: String,
    /// The maximum number of pooled connections.
    #[property]
    #[category("Behavior")]
    #[default_value(10)]
    pub max_pool_size: u32,
    /// How long to wait for a connection, in seconds.
    #[property]
    #[category("Behavior")]
    #[default_value(15)]
    pub connect_timeout: u32,
    /// How long a command may run, in seconds.
    #[property]
    #[category("Behavior")]
    #[default_value(30)]
    pub command_timeout: u32,
    /// How many times a failed connection or read is retried when the failure is temporary.
    #[property]
    #[category("Behavior")]
    #[default_value(3)]
    pub retry_count: u32,
    /// The first wait before a retry, in milliseconds (doubled at each retry).
    #[property]
    #[category("Behavior")]
    #[default_value(200)]
    pub retry_delay: u32,
    /// Occurs when the connection opens, closes or breaks.
    #[event]
    #[category("Behavior")]
    pub state_change: Event<StateChangeEventArgs>,
    handle: Option<ConnectionHandle>,
    raised_state: ConnectionState,
    resolver: Option<Arc<SecretResolver>>,
}

impl Default for DbConnection {
    fn default() -> Self {
        Self {
            base: ComponentCore::default(),
            provider: Provider::Postgres,
            connection_string_name: String::new(),
            connection_string: String::new(),
            schema: String::new(),
            max_pool_size: 10,
            connect_timeout: 15,
            command_timeout: 30,
            retry_count: 3,
            retry_delay: 200,
            state_change: Event::default(),
            handle: None,
            raised_state: ConnectionState::Closed,
            resolver: None,
        }
    }
}

impl DbConnection {
    pub fn new(provider: Provider) -> Self {
        Self { provider, ..Self::default() }
    }

    /// The component's name (its `x:Name`), used in messages and as the events' sender.
    pub fn name(&self) -> &str {
        crate::events::name_of(&self.base)
    }

    pub fn set_name(&mut self, name: impl Into<String>) -> &mut Self {
        crate::events::set_name_of(&mut self.base, name);
        self
    }

    pub fn with_connection_string_name(mut self, name: impl Into<String>) -> Self {
        self.connection_string_name = name.into();
        self
    }

    pub fn with_connection_string(mut self, text: impl Into<String>) -> Self {
        self.connection_string = text.into();
        self
    }

    pub fn with_schema(mut self, schema: impl Into<String>) -> Self {
        self.schema = schema.into();
        self
    }

    /// Resolves secrets with `resolver` instead of the default chain (tests, tools).
    pub fn with_resolver(mut self, resolver: SecretResolver) -> Self {
        self.resolver = Some(Arc::new(resolver));
        self
    }

    /// The resolved connection (resolving it on first call): validates the configuration and
    /// resolves the secrets, without any I/O to the database.
    pub fn handle(&mut self) -> Result<ConnectionHandle, DataError> {
        if let Some(h) = &self.handle {
            return Ok(h.clone());
        }
        let name = if self.name().is_empty() { "DbConnection".to_string() } else { self.name().to_string() };
        let fail = |e: DataError| logged(&format!("connection `{name}`"), e);
        if !self.provider.is_available() {
            return Err(fail(DataError::Config(format!("the {} provider is not available in this build", self.provider.as_str()))));
        }
        let default_resolver;
        let resolver: &SecretResolver = match &self.resolver {
            Some(r) => r,
            None => {
                default_resolver = SecretResolver::default_chain();
                &default_resolver
            }
        };
        let text = match (self.connection_string_name.trim(), self.connection_string.trim()) {
            ("", "") => return Err(fail(DataError::Config("set ConnectionStringName (or, without credentials, ConnectionString)".to_string()))),
            (key, "") => resolver.resolve(&format!("ConnectionStrings:{key}")).map_err(fail)?,
            ("", literal) => {
                let parsed = ConnectionStringBuilder::parse(literal).map_err(fail)?;
                if parsed.has_literal_password() {
                    return Err(fail(DataError::Secret(
                        "ConnectionString contains a password: store the connection string in the user secrets or the Credential Manager and set ConnectionStringName, or use a {secret:Key} placeholder".to_string(),
                    )));
                }
                literal.to_string()
            }
            _ => return Err(fail(DataError::Config("set either ConnectionStringName or ConnectionString, not both".to_string()))),
        };
        let expanded = resolver.expand(&text).map_err(fail)?;
        let target = ConnectionStringBuilder::parse(&expanded).map_err(fail)?;
        let schema = match self.schema.trim() {
            "" => None,
            s => {
                crate::sql::validate_identifier(s, false).map_err(fail)?;
                Some(s.to_string())
            }
        };
        let handle = ConnectionHandle {
            inner: Arc::new(HandleInner {
                settings: Settings {
                    name,
                    provider: self.provider,
                    target,
                    schema,
                    max_pool_size: self.max_pool_size,
                    connect_timeout: Duration::from_secs(u64::from(self.connect_timeout.max(1))),
                    command_timeout: Duration::from_secs(u64::from(self.command_timeout.max(1))),
                    retry: RetryPolicy { count: self.retry_count, delay: Duration::from_millis(u64::from(self.retry_delay)) },
                },
                pool: tokio::sync::Mutex::new(None),
                state: AtomicU8::new(ConnectionState::Closed.to_u8()),
                hinted: AtomicBool::new(false),
            }),
        };
        self.handle = Some(handle.clone());
        Ok(handle)
    }

    /// The current state (`Closed` before the first use).
    pub fn state(&self) -> ConnectionState {
        self.handle.as_ref().map_or(ConnectionState::Closed, ConnectionHandle::state)
    }

    /// Opens the connection now (it otherwise opens on first use). Await the task, then call
    /// [`Self::sync_state`] on the UI thread (the `DataContext` helpers do both).
    pub fn open(&mut self) -> DataTask<()> {
        let handle = match self.handle() {
            Ok(h) => h,
            Err(e) => return DataTask::failed(e),
        };
        handle.set_state(ConnectionState::Connecting);
        self.sync_state();
        rt::spawn(async move { handle.pool().await.map(|_| ()) })
    }

    /// Closes the pool. A later use opens it again.
    pub fn close(&mut self) -> DataTask<()> {
        match &self.handle {
            Some(h) => h.close(),
            None => DataTask::failed(DataError::Config("the connection was never opened".to_string())),
        }
    }

    /// Raises `StateChange` when the state changed since it was last raised. Returns whether it did.
    pub fn sync_state(&mut self) -> bool {
        let now = self.state();
        if now == self.raised_state {
            return false;
        }
        let args = StateChangeEventArgs { original_state: self.raised_state, current_state: now };
        self.raised_state = now;
        let event = self.state_change.clone();
        emit(&mut self.base, "DbConnection", &event, "OnStateChange", args);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::UserSecrets;

    fn store(tag: &str) -> UserSecrets {
        UserSecrets::at(std::env::temp_dir().join(format!("kubuno-data-conn-{tag}-{}", std::process::id())).join("secrets.json"))
    }

    #[test]
    fn literal_passwords_are_refused() {
        let mut c = DbConnection::new(Provider::Postgres).with_connection_string("Host=db;Password=hunter2").with_resolver(SecretResolver::empty());
        let e = c.handle().expect_err("refused");
        assert!(matches!(e, DataError::Secret(_)));
        assert!(!e.to_string().contains("hunter2"));
    }

    #[test]
    fn names_resolve_through_the_secrets_and_placeholders_expand() {
        let s = store("resolve");
        s.set("ConnectionStrings:Main", "Host=localhost;Database=app;Username=u;Password={secret:Pw}").expect("set");
        s.set("Pw", "p w").expect("set");
        let mut c = DbConnection::new(Provider::Postgres).with_connection_string_name("Main").with_resolver(SecretResolver::empty().with(s.clone()));
        let h = c.handle().expect("handle");
        let shown = format!("{h:?}");
        assert!(shown.contains("Password=***") && !shown.contains("p w"), "{shown}");
        assert_eq!(c.state(), ConnectionState::Closed);
        let mut missing = DbConnection::new(Provider::Postgres).with_connection_string_name("Other").with_resolver(SecretResolver::empty().with(s.clone()));
        assert!(matches!(missing.handle(), Err(DataError::Secret(m)) if m.contains("ConnectionStrings:Other")));
        let _ = std::fs::remove_dir_all(s.path().parent().expect("dir"));
    }

    #[test]
    fn configuration_is_validated() {
        let mut none = DbConnection::new(Provider::Sqlite);
        assert!(matches!(none.handle(), Err(DataError::Config(_))));
        let mut both = DbConnection::new(Provider::Sqlite).with_connection_string("Data Source=x.db").with_connection_string_name("X");
        assert!(matches!(both.handle(), Err(DataError::Config(_))));
        let mut bad_schema = DbConnection::new(Provider::Sqlite).with_connection_string("Data Source=x.db").with_schema("a;b");
        assert!(matches!(bad_schema.handle(), Err(DataError::Validation(_))));
        if !Provider::SqlServer.is_available() {
            let mut unavailable = DbConnection::new(Provider::SqlServer).with_connection_string("Server=x");
            assert!(matches!(unavailable.handle(), Err(DataError::Config(_))));
        }
    }

    #[test]
    fn state_changes_are_raised_once_each() {
        let mut c = DbConnection::new(Provider::Sqlite).with_connection_string("sqlite::memory:");
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let s = seen.clone();
        c.state_change.subscribe(move |_, e| s.borrow_mut().push((e.original_state, e.current_state))).detach();
        let task = c.open();
        assert_eq!(rt::block_on(task), Ok(Ok(())));
        assert!(c.sync_state());
        assert!(!c.sync_state());
        assert_eq!(*seen.borrow(), [(ConnectionState::Closed, ConnectionState::Connecting), (ConnectionState::Connecting, ConnectionState::Open)]);
    }

    #[test]
    fn retry_policy_retries_transient_errors_only() {
        let policy = RetryPolicy { count: 2, delay: Duration::from_millis(1) };
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let c = calls.clone();
        let out = rt::block_on(policy.run("test", move || {
            let c = c.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst);
                if n < 2 {
                    Err(DataError::Database { message: "I/O error: reset".into(), code: None })
                } else {
                    Ok(n)
                }
            }
        }));
        assert_eq!(out, Ok(Ok(2)));
        let c2 = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let c3 = c2.clone();
        let out = rt::block_on(policy.run("test", move || {
            let c = c3.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(DataError::Validation("no".into()))
            }
        }));
        assert!(matches!(out, Ok(Err(DataError::Validation(_)))));
        assert_eq!(c2.load(Ordering::SeqCst), 1);
    }
}
