//! Helpers shared by the engine tests.

#![allow(dead_code)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use kubuno_desktop_account::AccountKey;
use kubuno_desktop_api_client::{AccessToken, ApiClient, AuthError, RetryPolicy, TokenSource};
use kubuno_desktop_sync_engine::{AppSchema, Clock, ConflictPolicy, FeedSpec, JsonTableAdapter, LocalDb, OpenOptions, SyncEngine, SystemClock};
use serde_json::{Map, Value};

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("tests/migrations");
pub static MIGRATOR_V2: sqlx::migrate::Migrator = sqlx::migrate!("tests/migrations_v2");
pub static MIGRATOR_BAD: sqlx::migrate::Migrator = sqlx::migrate!("tests/migrations_bad");

pub const FEED: &str = "notes.notes";

/// A token source whose token can be revoked and replaced (session expiry tests).
#[derive(Debug)]
pub struct SwitchableToken {
    pub token: Mutex<String>,
    pub revoked: Mutex<bool>,
}

impl SwitchableToken {
    pub fn new(t: &str) -> Arc<Self> {
        Arc::new(Self { token: Mutex::new(t.into()), revoked: Mutex::new(false) })
    }
}

#[async_trait::async_trait]
impl TokenSource for SwitchableToken {
    async fn access_token(&self) -> Result<AccessToken, AuthError> {
        Ok(AccessToken::new(self.token.lock().expect("lock").clone()))
    }
    async fn after_unauthorized(&self, _failed: &AccessToken) -> Result<AccessToken, AuthError> {
        if *self.revoked.lock().expect("lock") {
            return Err(AuthError::SessionExpired);
        }
        Ok(AccessToken::new(self.token.lock().expect("lock").clone()))
    }
}

pub fn adapter(policy: ConflictPolicy) -> JsonTableAdapter {
    JsonTableAdapter::new("note", "notes", &["title", "body"], "/api/v1/notes/notes")
        .expect("adapter")
        .title_field("title")
        .policy(policy)
        .machine_name("TESTPC")
}

pub struct Setup {
    pub base: String,
    pub user: String,
    pub policy: ConflictPolicy,
    pub clock: Arc<dyn Clock>,
    pub tokens: Option<Arc<dyn TokenSource>>,
    pub schema: AppSchema,
    pub key: Option<kubuno_desktop_secrets::Secret>,
    pub page_limit: u32,
}

impl Setup {
    pub fn new(base: &str, user: &str) -> Self {
        Self {
            base: base.into(),
            user: user.into(),
            policy: ConflictPolicy::FieldMerge,
            clock: Arc::new(SystemClock),
            tokens: None,
            schema: AppSchema { migrator: Some(&MIGRATOR), resync_on: Vec::new() },
            key: None,
            page_limit: 2,
        }
    }

    pub async fn open(self, dir: &Path) -> SyncEngine {
        let account = AccountKey::new(&self.base, &self.user).expect("key");
        let mut o = OpenOptions::new(dir.join(format!("{}-notes.db", self.user)), account).schema(self.schema);
        if let Some(k) = self.key {
            o = o.key(k);
        }
        let db = LocalDb::open(o).await.expect("open db");
        let tokens: Arc<dyn TokenSource> =
            self.tokens.unwrap_or_else(|| Arc::new(kubuno_desktop_api_client::StaticToken(AccessToken::new(self.user.clone()))));
        let api = ApiClient::builder(&self.base).tokens(tokens).retry(RetryPolicy::none()).build().expect("api");
        SyncEngine::builder(db, api)
            .adapter(Arc::new(adapter(self.policy)))
            .feed(FeedSpec::new(FEED, "/api/v1/notes/notes/delta", "note").page_limit(self.page_limit))
            .clock(self.clock)
            .build()
            .await
            .expect("engine")
    }
}

pub fn fields(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

/// `(id, title, body, _pending)` of every local row, sorted.
pub async fn rows(engine: &SyncEngine) -> Vec<(String, Option<String>, Option<String>, i64)> {
    sqlx::query_as("SELECT id, title, body, _pending FROM notes ORDER BY id").fetch_all(engine.db().pool()).await.expect("rows")
}

pub async fn row(engine: &SyncEngine, id: &str) -> Option<(String, Option<String>, Option<String>, i64)> {
    rows(engine).await.into_iter().find(|r| r.0 == id)
}

pub async fn cursor(engine: &SyncEngine) -> String {
    sqlx::query_scalar("SELECT cursor FROM _sync_feeds WHERE feed = ?").bind(FEED).fetch_one(engine.db().pool()).await.expect("cursor")
}
