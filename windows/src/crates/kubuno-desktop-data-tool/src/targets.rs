//! Connection targets (`params.target`) and the sessions opened on them.
//!
//! A target names a connection three ways — a Data Explorer entry, a project's
//! `ConnectionStrings:<name>` (resolved through the kubuno-desktop-data secrets chain with the crate's
//! `user-secrets-id`), or an inline string — and resolves to a plain connection string that only
//! ever lives in this process: it goes to kubuno-desktop-data through an in-memory secret source, is
//! registered with the request's [`Redactor`](crate::error::Redactor), and is never logged.

use std::path::Path;

use kubuno_desktop_data::secrets::{is_valid_user_secrets_id, user_secrets_id_from_manifest, EnvironmentSecrets, SecretSource};
use kubuno_desktop_data::{ConnectionHandle, DataError, DbConnection, Provider, SecretResolver};
use kubuno_desktop_data_model::ProviderName;
use serde_json::Value;

use crate::connstr::{infer_provider, ConnInfo};
use crate::ctx::Ctx;
use crate::error::{ToolError, ToolResult};
use crate::explorer::{self, Store};

/// The key the in-memory source answers to.
const SESSION_KEY: &str = "ConnectionStrings:Tool";

/// A connection target of a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Explorer(String),
    Project { manifest_dir: String, connection: String, provider: Option<ProviderName> },
    Inline { provider: Option<ProviderName>, connection_string: String },
}

fn provider_of(v: Option<&Value>) -> ToolResult<Option<ProviderName>> {
    match v.and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(name) => ProviderName::parse(name).map(Some).ok_or_else(|| ToolError::validation(format!("unknown provider `{name}`"))),
    }
}

fn string_field<'a>(v: &'a Value, name: &str) -> ToolResult<&'a str> {
    v.get(name).and_then(Value::as_str).filter(|s| !s.trim().is_empty()).ok_or_else(|| ToolError::validation(format!("`{name}` is required")))
}

impl Target {
    /// Parses the `target` parameter.
    pub fn from_json(v: &Value) -> ToolResult<Self> {
        if let Some(name) = v.get("explorer") {
            let name = name.as_str().ok_or_else(|| ToolError::validation("`target.explorer` must be a string"))?;
            return Ok(Target::Explorer(name.to_string()));
        }
        if let Some(p) = v.get("project") {
            return Ok(Target::Project {
                manifest_dir: string_field(p, "manifestDir")?.to_string(),
                connection: string_field(p, "connection")?.to_string(),
                provider: provider_of(p.get("provider"))?,
            });
        }
        if v.get("connectionString").is_some() {
            return Ok(Target::Inline { provider: provider_of(v.get("provider"))?, connection_string: string_field(v, "connectionString")?.to_string() });
        }
        Err(ToolError::validation("`target` must be {explorer}, {project} or {provider, connectionString}"))
    }
}

/// A target resolved to its connection string.
#[derive(Clone)]
pub struct Resolved {
    pub provider: ProviderName,
    /// The connection string in clear (secrets expanded). Never logged, never returned.
    text: String,
    pub info: ConnInfo,
    /// For logs and connection names: no secret.
    pub label: String,
}

impl std::fmt::Debug for Resolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolved").field("provider", &self.provider).field("label", &self.label).field("display", &self.info.display()).finish()
    }
}

impl Resolved {
    /// The connection string, for `DATABASE_URL` only.
    pub fn secret_text(&self) -> &str {
        &self.text
    }
}

/// The runtime's provider for a model provider.
pub fn to_runtime(p: ProviderName) -> Provider {
    match p {
        ProviderName::Postgres => Provider::Postgres,
        ProviderName::Sqlite => Provider::Sqlite,
        ProviderName::Mysql => Provider::MySql,
        ProviderName::Sqlserver => Provider::SqlServer,
    }
}

/// The model provider for a runtime provider.
pub fn to_model(p: Provider) -> ProviderName {
    match p {
        Provider::Postgres => ProviderName::Postgres,
        Provider::Sqlite => ProviderName::Sqlite,
        Provider::MySql => ProviderName::Mysql,
        Provider::SqlServer => ProviderName::Sqlserver,
    }
}

/// Resolves a target to its connection string, registering every secret with the redactor.
pub fn resolve(ctx: &Ctx, target: &Target) -> ToolResult<Resolved> {
    let (text, provider, label) = match target {
        Target::Explorer(name) => {
            explorer::validate_name(name)?;
            let entry = explorer::find(&ctx.home, name)?;
            let text = explorer::connection_string(&ctx.home, &entry)?;
            ctx.redactor.add(&text);
            let chain = SecretResolver::empty().with(EnvironmentSecrets);
            let expanded = chain.expand(&text)?;
            let provider = ProviderName::parse(&entry.provider).ok_or_else(|| ToolError::config(format!("the stored provider `{}` is unknown", entry.provider)))?;
            (expanded, provider, format!("explorer:{}", entry.name))
        }
        Target::Project { manifest_dir, connection, provider } => {
            let dir = Path::new(manifest_dir);
            let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).map_err(|e| ToolError::io(&format!("cannot read {}", dir.join("Cargo.toml").display()), &e))?;
            let id = user_secrets_id_from_manifest(&manifest);
            let chain = project_chain(ctx, id.as_deref())?;
            let key = format!("ConnectionStrings:{}", connection.trim());
            let text = chain.resolve(&key).map_err(|e| match id {
                Some(_) => ToolError::from(e),
                None => ToolError::secret(format!("no secret named `{key}`: the crate has no `[package.metadata.kubuno] user-secrets-id`, only the environment was searched")),
            })?;
            ctx.redactor.add(&text);
            let expanded = chain.expand(&text)?;
            let provider = match provider {
                Some(p) => *p,
                None => infer_provider(&expanded)?,
            };
            (expanded, provider, format!("project:{}", connection.trim()))
        }
        Target::Inline { provider, connection_string } => {
            ctx.redactor.add(connection_string.trim());
            let chain = SecretResolver::empty().with(EnvironmentSecrets);
            let expanded = chain.expand(connection_string.trim())?;
            let provider = match provider {
                Some(p) => *p,
                None => infer_provider(&expanded)?,
            };
            (expanded, provider, "inline".to_string())
        }
    };
    ctx.redactor.add(&text);
    let info = ConnInfo::parse(&text).map_err(|e| ToolError { kind: e.kind, message: ctx.redactor.scrub(&e.message) })?;
    if let Some(p) = &info.password {
        ctx.redactor.add(p);
    }
    Ok(Resolved { provider, text, info, label })
}

/// The chain a project's connection is resolved through: environment, Credential Manager, user secrets.
fn project_chain(ctx: &Ctx, id: Option<&str>) -> ToolResult<SecretResolver> {
    let mut chain = SecretResolver::empty().with(EnvironmentSecrets);
    if let Some(id) = id {
        if !is_valid_user_secrets_id(id) {
            return Err(ToolError::validation("the crate's user-secrets-id is not valid"));
        }
        chain = chain.with(StoreSource { home: ctx.home.clone(), store: Store::CredMan, id: id.to_string() }).with(StoreSource { home: ctx.home.clone(), store: Store::UserSecrets, id: id.to_string() });
    }
    Ok(chain)
}

/// A secret store as a [`SecretSource`] of a resolver chain.
struct StoreSource {
    home: crate::home::Home,
    store: Store,
    id: String,
}

impl SecretSource for StoreSource {
    fn name(&self) -> &'static str {
        match self.store {
            Store::CredMan => "credential manager",
            Store::UserSecrets => "user secrets",
        }
    }

    fn get(&self, key: &str) -> Result<Option<String>, DataError> {
        explorer::read_secret(&self.home, self.store, &self.id, key).map_err(|e| DataError::Secret(e.message))
    }
}

/// Answers the one key of a session: the resolved string reaches kubuno-desktop-data without a store.
struct Fixed(String);

impl SecretSource for Fixed {
    fn name(&self) -> &'static str {
        "session"
    }

    fn get(&self, key: &str) -> Result<Option<String>, DataError> {
        Ok((key == SESSION_KEY).then(|| self.0.clone()))
    }
}

/// How a session's pool behaves.
#[derive(Debug, Clone, Copy)]
pub struct OpenOptions {
    pub max_pool_size: u32,
    pub connect_timeout_secs: u32,
    pub command_timeout_secs: u32,
    /// Let SQLite create a missing database file (migrations); otherwise a missing file is an
    /// error — a mistyped path must not silently create an empty database.
    pub create_sqlite_file: bool,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self { max_pool_size: 4, connect_timeout_secs: 15, command_timeout_secs: 60, create_sqlite_file: false }
    }
}

/// An open connection: closed (in the background) when dropped, cancelled work included.
pub struct Session {
    pub handle: ConnectionHandle,
    pub resolved: Resolved,
}

impl Session {
    pub fn provider(&self) -> ProviderName {
        self.resolved.provider
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Detached on purpose: closing waits for connections still busy after a cancellation.
        drop(self.handle.close());
    }
}

/// Resolves `target` and prepares its connection (nothing is sent to the server until first use).
pub fn open(ctx: &Ctx, target: &Target, opts: OpenOptions) -> ToolResult<Session> {
    let resolved = resolve(ctx, target)?;
    let session = open_resolved(ctx, resolved, opts)?;
    Ok(session)
}

pub fn open_resolved(ctx: &Ctx, resolved: Resolved, opts: OpenOptions) -> ToolResult<Session> {
    let runtime = to_runtime(resolved.provider);
    if !runtime.is_available() {
        return Err(ToolError::config(format!("the {} provider is not available in this build", resolved.provider.as_str())));
    }
    if resolved.provider == ProviderName::Sqlite && !opts.create_sqlite_file {
        if let Some(file) = resolved.info.file.as_deref().filter(|f| f.trim() != ":memory:" && !f.contains("mode=memory")) {
            if !Path::new(file).is_file() {
                return Err(ToolError::config(format!("the SQLite database file `{file}` does not exist")));
            }
        }
    }
    let mut connection = DbConnection::new(runtime).with_connection_string_name("Tool").with_resolver(SecretResolver::empty().with(Fixed(resolved.text.clone())));
    // The component name shows in the runtime's logs; the label carries no secret.
    connection.set_name(resolved.label.clone());
    connection.max_pool_size = opts.max_pool_size.max(1);
    connection.connect_timeout = opts.connect_timeout_secs.max(1);
    connection.command_timeout = opts.command_timeout_secs.max(1);
    // A tool reports a failed connection at once instead of retrying behind the user's back.
    connection.retry_count = 0;
    let handle = connection.handle().map_err(|e| ctx.redactor.error(ToolError::from(e)))?;
    Ok(Session { handle, resolved })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::Home;
    use serde_json::json;

    fn ctx(tag: &str) -> Ctx {
        let dir = std::env::temp_dir().join(format!("kubuno-data-tool-targets-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Ctx::new(Home::at(dir))
    }

    #[test]
    fn targets_parse_in_the_three_forms() {
        assert_eq!(Target::from_json(&json!({"explorer": "Shop"})).expect("t"), Target::Explorer("Shop".into()));
        assert_eq!(
            Target::from_json(&json!({"project": {"manifestDir": "C:\\src\\app", "connection": "Shop", "provider": "sqlite"}})).expect("t"),
            Target::Project { manifest_dir: "C:\\src\\app".into(), connection: "Shop".into(), provider: Some(ProviderName::Sqlite) }
        );
        assert_eq!(
            Target::from_json(&json!({"provider": "sqlite", "connectionString": "sqlite:x.db"})).expect("t"),
            Target::Inline { provider: Some(ProviderName::Sqlite), connection_string: "sqlite:x.db".into() }
        );
        assert!(Target::from_json(&json!({})).is_err());
        assert!(Target::from_json(&json!({"provider": "oracle", "connectionString": "x"})).is_err());
        assert!(Target::from_json(&json!({"project": {"connection": "Shop"}})).is_err());
    }

    #[test]
    fn a_project_connection_resolves_through_the_user_secrets_of_its_crate() {
        let ctx = ctx("project");
        let dir = ctx.home.root().join("app");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"app\"\n[package.metadata.kubuno]\nuser-secrets-id = \"tool-test-app\"\n").expect("manifest");
        ctx.home.user_secrets("tool-test-app").expect("id").set("ConnectionStrings:Shop", "Host=h;Database=d;Username=u;Password={secret:Pw}").expect("set");
        ctx.home.user_secrets("tool-test-app").expect("id").set("Pw", "hunter22!").expect("set");
        let target = Target::Project { manifest_dir: dir.to_string_lossy().into_owned(), connection: "Shop".into(), provider: None };
        let r = resolve(&ctx, &target).expect("resolves");
        assert_eq!(r.provider, ProviderName::Postgres);
        assert_eq!(r.info.display(), "h/d (user u)");
        assert!(ctx.redactor.scrub("x hunter22! y").contains("***"), "the expanded password is registered");
        assert!(!format!("{r:?}").contains("hunter22"));
        let missing = resolve(&ctx, &Target::Project { manifest_dir: dir.to_string_lossy().into_owned(), connection: "Nope".into(), provider: None }).expect_err("missing");
        assert_eq!(missing.kind, "Secret");
        let _ = std::fs::remove_dir_all(ctx.home.root());
    }

    #[test]
    fn a_missing_sqlite_file_is_not_created() {
        let ctx = ctx("missing");
        let file = ctx.home.root().join("nope.db");
        let target = Target::Inline { provider: Some(ProviderName::Sqlite), connection_string: format!("Data Source={}", file.display()) };
        let e = open(&ctx, &target, OpenOptions::default()).err().expect("refused");
        assert_eq!(e.kind, "Config");
        assert!(!file.exists());
        assert!(open(&ctx, &target, OpenOptions { create_sqlite_file: true, ..OpenOptions::default() }).is_ok());
    }
}
