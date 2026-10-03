//! The token owner: the **only** holder of refresh tokens, run by the shell (vskubuno
//! `docs/DESKTOP-OFFLINE-SYNC.md` §5.2, §9). Apps borrow access tokens from it through the broker.
//!
//! The refresh state machine is the one `kubuno-sync/src/api.rs` learnt the hard way, made per account and async:
//!
//! - **single flight**: one refresh at a time per account (an async mutex); a caller that waited adopts the pair
//!   the previous one obtained instead of rotating again (two rotations of one token revoke its family);
//! - **adopt a fresh pair**: a token obtained less than `fresh_ttl` ago is handed out again after a 401 on another
//!   token, which collapses refresh storms into one rotation;
//! - **persist before use**: the rotated refresh token is written to the OS store, and read back, before the new
//!   access token is handed out. If that fails the new pair is dropped: the next refresh presents the previous
//!   token again, which the server's **rotation grace** accepts as long as its successor was never used
//!   (`core/.../handlers/auth/refresh.rs`, `try_rotation_grace`). The same grace covers a lost response;
//! - **genuine vs transient**: only 401/403 on refresh ends the session (`SessionExpired`, the outbox is kept, the
//!   feeds pause); network errors, 429 and 5xx start a `cooldown` during which callers fail fast with `Transient`
//!   instead of hammering `/auth/refresh` (its rate limit is 10/min);
//! - **expiry from the server's clock**: an access token is refreshed `refresh_margin` before `exp - iat` has
//!   elapsed on the local monotonic clock, so a skewed wall clock never causes a refresh loop.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use kubuno_api_client::{AccessToken, ApiClient, AuthError, ErrorClass};
use kubuno_secrets::{Secret, SecretName, SecretStore};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::key::AccountKey;
use crate::login::{self, NativeTokens};
use crate::store::{AccountError, AccountInfo, AccountStore};

/// Tunables of the owner.
#[derive(Debug, Clone)]
pub struct OwnerConfig {
    /// How long a newly rotated access token is handed out again instead of rotating (5 min).
    pub fresh_ttl: Duration,
    /// After a transient refresh failure, fail fast for this long (45 s, under the server's 60 s rate window).
    pub cooldown: Duration,
    /// Refresh this long before the access token expires.
    pub refresh_margin: Duration,
    /// Lifetime assumed for an access token whose claims cannot be read (the server's default, 900 s).
    pub default_lifetime: Duration,
    /// Timeout of auth requests.
    pub timeout: Duration,
    /// Outbound proxy for every account.
    pub proxy: Option<String>,
}

impl Default for OwnerConfig {
    fn default() -> Self {
        Self {
            fresh_ttl: Duration::from_secs(300),
            cooldown: Duration::from_secs(45),
            refresh_margin: Duration::from_secs(60),
            default_lifetime: Duration::from_secs(900),
            timeout: Duration::from_secs(30),
            proxy: None,
        }
    }
}

/// The session state of an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    /// The server rejected the refresh token: sign in again (same account key resumes everything).
    SessionExpired,
    /// No refresh token stored (signed out, or never signed in on this machine).
    SignedOut,
}

/// What subscribers (the broker's clients, the shell's UI) are told. Never carries a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AccountEvent {
    AccountAdded { account: AccountKey },
    AccountRemoved { account: AccountKey },
    SessionExpired { account: AccountKey },
    SessionRestored { account: AccountKey },
    Switched { account: Option<AccountKey> },
}

/// An account as listed to apps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountSummary {
    pub info: AccountInfo,
    pub status: SessionStatus,
}

/// A borrowed access token and how long it is still good for (local monotonic estimate).
#[derive(Debug, Clone)]
pub struct Borrowed {
    pub token: AccessToken,
    pub valid_for: Duration,
}

struct Cached {
    token: AccessToken,
    obtained: Instant,
    lifetime: Duration,
}

impl Cached {
    fn remaining(&self) -> Duration {
        self.lifetime.saturating_sub(self.obtained.elapsed())
    }
}

struct SlotState {
    access: Option<Cached>,
    status: SessionStatus,
    cooldown_until: Option<Instant>,
}

struct Slot {
    info: AccountInfo,
    api: ApiClient,
    state: tokio::sync::Mutex<SlotState>,
}

/// The token owner. One per shell process; shared as `Arc<TokenOwner>`.
pub struct TokenOwner {
    secrets: Arc<dyn SecretStore>,
    accounts: AccountStore,
    cfg: OwnerConfig,
    slots: Mutex<HashMap<AccountKey, Arc<Slot>>>,
    current: Mutex<Option<AccountKey>>,
    events: broadcast::Sender<AccountEvent>,
    refreshes: std::sync::atomic::AtomicU64,
}

impl std::fmt::Debug for TokenOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenOwner").field("secrets", &self.secrets.backend()).field("accounts", &self.accounts.root()).finish()
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, AccountError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| AccountError::Io(format!("secret store task failed: {e}")))
}

impl TokenOwner {
    pub fn new(secrets: Arc<dyn SecretStore>, accounts: AccountStore, cfg: OwnerConfig) -> Arc<Self> {
        let (events, _) = broadcast::channel(64);
        Arc::new(Self {
            secrets,
            accounts,
            cfg,
            slots: Mutex::new(HashMap::new()),
            current: Mutex::new(None),
            events,
            refreshes: std::sync::atomic::AtomicU64::new(0),
        })
    }

    pub fn account_store(&self) -> &AccountStore {
        &self.accounts
    }

    /// Network refreshes done so far (tests, diagnostics).
    pub fn refresh_count(&self) -> u64 {
        self.refreshes.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AccountEvent> {
        self.events.subscribe()
    }

    fn emit(&self, e: AccountEvent) {
        // No subscriber is fine.
        let _ = self.events.send(e);
    }

    fn api_for(&self, server_url: &str) -> Result<ApiClient, AccountError> {
        Ok(ApiClient::builder(server_url).timeout(self.cfg.timeout).proxy(self.cfg.proxy.clone()).build()?)
    }

    fn slot(&self, key: &AccountKey) -> Option<Arc<Slot>> {
        self.slots.lock().unwrap_or_else(PoisonError::into_inner).get(key).cloned()
    }

    /// Loads every account of the store. Status is `Active` when a refresh token is stored, else `SignedOut`.
    pub async fn load(&self) -> Result<(), AccountError> {
        let infos = self.accounts.list()?;
        for info in infos {
            let secrets = self.secrets.clone();
            let name = SecretName::refresh_token(info.key.as_str())?;
            let has = blocking(move || secrets.get(&name)).await??.is_some();
            let status = if has { SessionStatus::Active } else { SessionStatus::SignedOut };
            let slot = Arc::new(Slot {
                api: self.api_for(&info.server_url)?,
                info: info.clone(),
                state: tokio::sync::Mutex::new(SlotState { access: None, status, cooldown_until: None }),
            });
            self.slots.lock().unwrap_or_else(PoisonError::into_inner).insert(info.key.clone(), slot);
        }
        let mut cur = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        if cur.is_none() {
            *cur = self.slots.lock().unwrap_or_else(PoisonError::into_inner).keys().min().cloned();
        }
        Ok(())
    }

    /// Completes a sign-in: identifies the user (`GET /me`), persists the refresh token **first**, writes
    /// `account.json`, then makes the account active (and current when there was none). Signing in again into
    /// an account whose session expired restores it under the same key.
    pub async fn sign_in(&self, server_url: &str, tokens: NativeTokens) -> Result<AccountInfo, AccountError> {
        let api = self.api_for(server_url)?;
        let who = login::fetch_identity(&api, &tokens.access_token).await?;
        let mut info = AccountInfo::new(server_url, &who.id)?;
        if let Some(existing) = self.accounts.load(&info.key)? {
            info.linked_instances = existing.linked_instances;
        }
        info.display_name = who.display_name;
        info.email = who.email;
        self.adopt(info, tokens).await
    }

    /// Registers an account whose identity is already known (sign-in above, legacy migration, tests).
    pub async fn adopt(&self, info: AccountInfo, tokens: NativeTokens) -> Result<AccountInfo, AccountError> {
        let secrets = self.secrets.clone();
        let name = SecretName::refresh_token(info.key.as_str())?;
        let refresh = tokens.refresh_token;
        blocking(move || secrets.set(&name, &refresh)).await??;
        self.accounts.save(&info)?;
        let lifetime = self.lifetime_of(&tokens.access_token);
        let cached = Cached { token: tokens.access_token, obtained: Instant::now(), lifetime };
        let existed = self.slot(&info.key);
        let restored = match &existed {
            Some(slot) => {
                let mut st = slot.state.lock().await;
                let was = st.status;
                st.status = SessionStatus::Active;
                st.access = Some(cached);
                st.cooldown_until = None;
                was != SessionStatus::Active
            }
            None => {
                let slot = Arc::new(Slot {
                    api: self.api_for(&info.server_url)?,
                    info: info.clone(),
                    state: tokio::sync::Mutex::new(SlotState { access: Some(cached), status: SessionStatus::Active, cooldown_until: None }),
                });
                self.slots.lock().unwrap_or_else(PoisonError::into_inner).insert(info.key.clone(), slot);
                false
            }
        };
        if existed.is_none() {
            self.emit(AccountEvent::AccountAdded { account: info.key.clone() });
        } else if restored {
            self.emit(AccountEvent::SessionRestored { account: info.key.clone() });
        }
        let became_current = {
            let mut cur = self.current.lock().unwrap_or_else(PoisonError::into_inner);
            if cur.is_none() {
                *cur = Some(info.key.clone());
                true
            } else {
                false
            }
        };
        if became_current {
            self.emit(AccountEvent::Switched { account: Some(info.key.clone()) });
        }
        Ok(info)
    }

    fn lifetime_of(&self, token: &AccessToken) -> Duration {
        token
            .jwt_lifetime_s()
            .and_then(|s| u64::try_from(s).ok())
            .map(Duration::from_secs)
            .unwrap_or(self.cfg.default_lifetime)
            .saturating_sub(self.cfg.refresh_margin)
    }

    pub async fn status(&self, key: &AccountKey) -> Option<SessionStatus> {
        let slot = self.slot(key)?;
        let st = slot.state.lock().await;
        Some(st.status)
    }

    pub async fn accounts(&self) -> Vec<AccountSummary> {
        let slots: Vec<Arc<Slot>> = self.slots.lock().unwrap_or_else(PoisonError::into_inner).values().cloned().collect();
        let mut out = Vec::with_capacity(slots.len());
        for s in slots {
            let status = s.state.lock().await.status;
            out.push(AccountSummary { info: s.info.clone(), status });
        }
        out.sort_by(|a, b| a.info.key.cmp(&b.info.key));
        out
    }

    pub fn current(&self) -> Option<AccountKey> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Makes `key` the current account (the views rebind to its databases; background sync keeps running for
    /// all accounts).
    pub fn switch(&self, key: &AccountKey) -> Result<(), AccountError> {
        if self.slot(key).is_none() {
            return Err(AccountError::Unknown(key.clone()));
        }
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) = Some(key.clone());
        self.emit(AccountEvent::Switched { account: Some(key.clone()) });
        Ok(())
    }

    /// A valid access token for `key`, refreshing when it is (nearly) expired.
    pub async fn access_token(&self, key: &AccountKey) -> Result<Borrowed, AuthError> {
        let slot = self.slot(key).ok_or(AuthError::UnknownAccount)?;
        let mut st = slot.state.lock().await;
        if let Some(c) = &st.access {
            if c.remaining() > Duration::ZERO && st.status == SessionStatus::Active {
                return Ok(Borrowed { token: c.token.clone(), valid_for: c.remaining() });
            }
        }
        self.refresh_locked(&slot, &mut st).await
    }

    /// The server answered 401 to the token whose fingerprint is `failed`: hand out a newer token when another
    /// caller already refreshed, else refresh once.
    pub async fn access_after_401(&self, key: &AccountKey, failed: &str) -> Result<Borrowed, AuthError> {
        let slot = self.slot(key).ok_or(AuthError::UnknownAccount)?;
        let mut st = slot.state.lock().await;
        if let Some(c) = &st.access {
            if c.token.fingerprint() != failed && c.remaining() > Duration::ZERO && st.status == SessionStatus::Active {
                return Ok(Borrowed { token: c.token.clone(), valid_for: c.remaining() });
            }
        }
        // The rejected token must not be handed out again.
        if st.access.as_ref().is_some_and(|c| c.token.fingerprint() == failed) {
            st.access = None;
        }
        self.refresh_locked(&slot, &mut st).await
    }

    async fn refresh_locked(&self, slot: &Slot, st: &mut SlotState) -> Result<Borrowed, AuthError> {
        match st.status {
            SessionStatus::Active => {}
            SessionStatus::SessionExpired | SessionStatus::SignedOut => return Err(AuthError::SessionExpired),
        }
        // Adopt a pair rotated moments ago (another caller refreshed while we waited for the lock).
        if let Some(c) = &st.access {
            if c.obtained.elapsed() < self.cfg.fresh_ttl && c.remaining() > Duration::ZERO {
                return Ok(Borrowed { token: c.token.clone(), valid_for: c.remaining() });
            }
        }
        if let Some(until) = st.cooldown_until {
            if Instant::now() < until {
                return Err(AuthError::Transient("refresh cooling down after a failure".to_string()));
            }
        }
        let key = slot.info.key.clone();
        let secrets = self.secrets.clone();
        let name = SecretName::refresh_token(key.as_str()).map_err(|e| AuthError::Transient(e.to_string()))?;
        let read_name = name.clone();
        let stored = blocking(move || secrets.get(&read_name))
            .await
            .map_err(|e| AuthError::Transient(e.to_string()))?
            .map_err(|e| AuthError::Transient(e.to_string()))?;
        let Some(refresh_token) = stored else {
            st.status = SessionStatus::SignedOut;
            self.emit(AccountEvent::SessionExpired { account: key });
            return Err(AuthError::SessionExpired);
        };
        self.refreshes.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        match login::refresh(&slot.api, &refresh_token).await {
            Ok(pair) => {
                // Persist before use, and check the store kept it.
                let secrets = self.secrets.clone();
                let new_refresh = pair.refresh_token;
                let written = blocking(move || -> Result<bool, kubuno_secrets::SecretError> {
                    secrets.set(&name, &new_refresh)?;
                    Ok(secrets.get(&name)?.as_ref() == Some(&new_refresh))
                })
                .await;
                match written {
                    Ok(Ok(true)) => {}
                    other => {
                        let why = match other {
                            Ok(Ok(_)) => "the store did not keep it".to_string(),
                            Ok(Err(e)) => e.to_string(),
                            Err(e) => e.to_string(),
                        };
                        tracing::error!(account = %key, error = %why, "could not persist the rotated refresh token; dropping the new pair (the server's rotation grace heals it)");
                        st.cooldown_until = Some(Instant::now() + self.cfg.cooldown);
                        return Err(AuthError::Transient("could not persist the rotated refresh token".to_string()));
                    }
                }
                let lifetime = self.lifetime_of(&pair.access_token);
                let cached = Cached { token: pair.access_token, obtained: Instant::now(), lifetime };
                let out = Borrowed { token: cached.token.clone(), valid_for: cached.remaining() };
                st.access = Some(cached);
                st.cooldown_until = None;
                tracing::debug!(account = %key, token = %out.token.fingerprint(), "access token refreshed");
                Ok(out)
            }
            Err(e) => {
                let genuine = matches!(e.status(), Some(401) | Some(403));
                if genuine {
                    tracing::warn!(account = %key, "refresh token rejected by the server: session expired");
                    st.status = SessionStatus::SessionExpired;
                    st.access = None;
                    self.emit(AccountEvent::SessionExpired { account: key });
                    return Err(AuthError::SessionExpired);
                }
                tracing::warn!(account = %key, error = %e, transient = (e.class() == ErrorClass::Transient), "refresh failed, cooling down");
                st.cooldown_until = Some(Instant::now() + self.cfg.cooldown);
                Err(AuthError::Transient(e.to_string()))
            }
        }
    }

    /// Signs out locally: tells the server (best effort), deletes the account's secrets (refresh token and database
    /// key) and forgets the account in memory. The caller asks the user first when the outbox is not empty and
    /// deletes the account directory with [`AccountStore::remove_dir`] when wiping.
    pub async fn sign_out(&self, key: &AccountKey) -> Result<(), AccountError> {
        let slot = self.slot(key).ok_or_else(|| AccountError::Unknown(key.clone()))?;
        let secrets = self.secrets.clone();
        let name = SecretName::refresh_token(key.as_str())?;
        if let Ok(Ok(Some(rt))) = blocking(move || secrets.get(&name)).await {
            if let Ok(token) = rt.expose_str() {
                let req = kubuno_api_client::ApiRequest::post("/api/v1/auth/logout")
                    .unauthenticated()
                    .retry(kubuno_api_client::RetryPolicy::none())
                    .json(serde_json::json!({ "refresh_token": token }));
                if let Err(e) = slot.api.send(req).await {
                    tracing::warn!(account = %key, error = %e, "server-side logout failed; the local sign-out continues");
                }
            }
        }
        let secrets = self.secrets.clone();
        let scope = key.as_str().to_string();
        blocking(move || secrets.delete_scope(&scope)).await??;
        self.slots.lock().unwrap_or_else(PoisonError::into_inner).remove(key);
        let switched = {
            let mut cur = self.current.lock().unwrap_or_else(PoisonError::into_inner);
            if cur.as_ref() == Some(key) {
                *cur = self.slots.lock().unwrap_or_else(PoisonError::into_inner).keys().min().cloned();
                Some(cur.clone())
            } else {
                None
            }
        };
        self.emit(AccountEvent::AccountRemoved { account: key.clone() });
        if let Some(account) = switched {
            self.emit(AccountEvent::Switched { account });
        }
        Ok(())
    }

    /// The database key of an account, created at the first call (random 256 bits, kept in the OS store).
    pub async fn database_key(&self, key: &AccountKey) -> Result<Secret, AccountError> {
        let secrets = self.secrets.clone();
        let name = SecretName::db_key(key.as_str())?;
        Ok(blocking(move || kubuno_secrets::get_or_create(secrets.as_ref(), &name, Secret::random_key_hex)).await??)
    }
}

/// What the broker server needs from the owner; implemented by [`TokenOwner`] (a trait so the shell can wrap it
/// and tests can fake it).
#[async_trait::async_trait]
pub trait BrokerBackend: Send + Sync {
    async fn access_token(&self, key: &AccountKey) -> Result<Borrowed, AuthError>;
    async fn access_after_401(&self, key: &AccountKey, failed_fingerprint: &str) -> Result<Borrowed, AuthError>;
    async fn accounts(&self) -> Vec<AccountSummary>;
    fn current(&self) -> Option<AccountKey>;
    fn switch(&self, key: &AccountKey) -> Result<(), AccountError>;
    /// The database key of the account (apps open their own databases with it).
    async fn database_key(&self, key: &AccountKey) -> Result<Secret, AccountError>;
    fn subscribe(&self) -> broadcast::Receiver<AccountEvent>;
}

#[async_trait::async_trait]
impl BrokerBackend for TokenOwner {
    async fn access_token(&self, key: &AccountKey) -> Result<Borrowed, AuthError> {
        TokenOwner::access_token(self, key).await
    }

    async fn access_after_401(&self, key: &AccountKey, failed_fingerprint: &str) -> Result<Borrowed, AuthError> {
        TokenOwner::access_after_401(self, key, failed_fingerprint).await
    }

    async fn accounts(&self) -> Vec<AccountSummary> {
        TokenOwner::accounts(self).await
    }

    fn current(&self) -> Option<AccountKey> {
        TokenOwner::current(self)
    }

    fn switch(&self, key: &AccountKey) -> Result<(), AccountError> {
        TokenOwner::switch(self, key)
    }

    async fn database_key(&self, key: &AccountKey) -> Result<Secret, AccountError> {
        TokenOwner::database_key(self, key).await
    }

    fn subscribe(&self) -> broadcast::Receiver<AccountEvent> {
        TokenOwner::subscribe(self)
    }
}

/// The owner as the [`kubuno_api_client::TokenSource`] of one account (the shell's own API calls).
#[derive(Debug, Clone)]
pub struct OwnerTokenSource {
    pub owner: Arc<TokenOwner>,
    pub account: AccountKey,
}

#[async_trait::async_trait]
impl kubuno_api_client::TokenSource for OwnerTokenSource {
    async fn access_token(&self) -> Result<AccessToken, AuthError> {
        Ok(self.owner.access_token(&self.account).await?.token)
    }

    async fn after_unauthorized(&self, failed: &AccessToken) -> Result<AccessToken, AuthError> {
        Ok(self.owner.access_after_401(&self.account, &failed.fingerprint()).await?.token)
    }
}
