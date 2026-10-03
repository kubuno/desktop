//! Token owner against the fake auth server: single flight, rotation grace, revoked session, persist failure,
//! account switch and events, and the broker over the real local transport (in-process).

mod fake_auth;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use kubuno_account::broker::{BrokerClient, BrokerEndpoint, BrokerServer, ClientPolicy};
use kubuno_account::login::NativeTokens;
use kubuno_account::{AccountEvent, AccountInfo, AccountKey, AccountStore, OwnerConfig, SessionStatus, TokenOwner};
use kubuno_api_client::{AccessToken, ApiClient, ApiRequest, AuthError, TokenSource};
use kubuno_secrets::{MemorySecretStore, Secret, SecretError, SecretName, SecretStore};

fn cfg() -> OwnerConfig {
    OwnerConfig { cooldown: Duration::from_millis(0), fresh_ttl: Duration::from_millis(300), ..OwnerConfig::default() }
}

fn tokens(access: String, refresh: String) -> NativeTokens {
    NativeTokens { access_token: AccessToken::new(access), refresh_token: Secret::from_string(refresh) }
}

async fn setup(user: &str) -> (Arc<TokenOwner>, Arc<fake_auth::AuthState>, AccountKey, String, tempfile::TempDir, Arc<MemorySecretStore>) {
    let (base, state) = fake_auth::start().await;
    let dir = tempfile::tempdir().expect("tmp");
    let secrets = Arc::new(MemorySecretStore::new());
    let owner = TokenOwner::new(secrets.clone(), AccountStore::new(dir.path()), cfg());
    let (a, r) = state.login(user);
    let info = owner.sign_in(&base, tokens(a, r)).await.expect("sign in");
    (owner, state, info.key, base, dir, secrets)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sign_in_identifies_the_account_and_stores_the_refresh_token() {
    let (owner, _state, key, base, _dir, secrets) = setup("user-1").await;
    assert_eq!(key, AccountKey::new(&base, "user-1").expect("key"));
    let name = SecretName::refresh_token(key.as_str()).expect("name");
    assert!(secrets.get(&name).expect("get").is_some());
    let info = owner.account_store().load(&key).expect("load").expect("some");
    assert_eq!(info.display_name.as_deref(), Some("Test"));
    assert_eq!(owner.current(), Some(key.clone()));
    assert_eq!(owner.status(&key).await, Some(SessionStatus::Active));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_401s_rotate_once() {
    let (owner, state, key, _base, _dir, _s) = setup("user-1").await;
    let first = owner.access_token(&key).await.expect("token").token;
    state.expire_access();
    let mut tasks = Vec::new();
    for _ in 0..20 {
        let owner = owner.clone();
        let key = key.clone();
        let fp = first.fingerprint();
        tasks.push(tokio::spawn(async move { owner.access_after_401(&key, &fp).await }));
    }
    let mut seen = std::collections::HashSet::new();
    for t in tasks {
        let b = t.await.expect("join").expect("token");
        seen.insert(b.token.expose().to_string());
    }
    assert_eq!(state.rotations.load(Ordering::SeqCst), 1, "a single network rotation");
    assert_eq!(seen.len(), 1, "everybody got the same new token");
    let t = seen.into_iter().next().expect("one");
    assert!(state.access_valid(&t));
    assert!(!state.family_revoked_for("user-1"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lost_refresh_response_is_healed_by_the_rotation_grace() {
    let (owner, state, key, _base, _dir, _s) = setup("user-1").await;
    let first = owner.access_token(&key).await.expect("token").token;
    state.drop_after_rotation.store(1, Ordering::SeqCst);
    // The server rotated but the answer was lost: transient, the old refresh token stays stored.
    let err = owner.access_after_401(&key, &first.fingerprint()).await.expect_err("lost answer");
    assert!(matches!(err, AuthError::Transient(_)), "{err:?}");
    assert_eq!(owner.status(&key).await, Some(SessionStatus::Active));
    // Next attempt presents the old token again: grace serves a fresh pair, the family survives.
    let b = owner.access_after_401(&key, &first.fingerprint()).await.expect("healed");
    assert!(state.access_valid(b.token.expose()));
    assert!(!state.family_revoked_for("user-1"));
    // And the healed token keeps rotating normally afterwards.
    state.expire_access();
    let c = owner.access_after_401(&key, &b.token.fingerprint()).await.expect("next rotation");
    assert!(state.access_valid(c.token.expose()));
    assert!(!state.family_revoked_for("user-1"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn transient_failures_cool_down_and_keep_the_session() {
    let (base, state) = fake_auth::start().await;
    let dir = tempfile::tempdir().expect("tmp");
    let owner = TokenOwner::new(
        Arc::new(MemorySecretStore::new()),
        AccountStore::new(dir.path()),
        OwnerConfig { cooldown: Duration::from_millis(400), fresh_ttl: Duration::from_millis(0), ..OwnerConfig::default() },
    );
    let (a, r) = state.login("user-1");
    let key = owner.sign_in(&base, tokens(a.clone(), r)).await.expect("sign in").key;
    state.fail_before_rotation.store(1, Ordering::SeqCst);
    let fp = AccessToken::new(a).fingerprint();
    assert!(matches!(owner.access_after_401(&key, &fp).await, Err(AuthError::Transient(_))));
    // Inside the cooldown: no network call at all.
    let calls = state.refresh_calls.load(Ordering::SeqCst);
    assert!(matches!(owner.access_after_401(&key, &fp).await, Err(AuthError::Transient(_))));
    assert_eq!(state.refresh_calls.load(Ordering::SeqCst), calls);
    tokio::time::sleep(Duration::from_millis(450)).await;
    owner.access_after_401(&key, &fp).await.expect("after the cooldown");
    assert_eq!(owner.status(&key).await, Some(SessionStatus::Active));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revoked_session_expires_then_sign_in_restores_the_same_account() {
    let (owner, state, key, base, _dir, _s) = setup("user-1").await;
    let mut events = owner.subscribe();
    let first = owner.access_token(&key).await.expect("token").token;
    state.revoke_user("user-1");
    let err = owner.access_after_401(&key, &first.fingerprint()).await.expect_err("revoked");
    assert_eq!(err, AuthError::SessionExpired);
    assert_eq!(owner.status(&key).await, Some(SessionStatus::SessionExpired));
    assert_eq!(events.recv().await.expect("event"), AccountEvent::SessionExpired { account: key.clone() });
    // Further calls fail fast, without the network.
    let calls = state.refresh_calls.load(Ordering::SeqCst);
    assert_eq!(owner.access_token(&key).await.expect_err("still expired"), AuthError::SessionExpired);
    assert_eq!(state.refresh_calls.load(Ordering::SeqCst), calls);
    // Signing in again as the same user on the same server resumes the same account key.
    let (a, r) = state.login("user-1");
    let info = owner.sign_in(&base, tokens(a, r)).await.expect("sign in again");
    assert_eq!(info.key, key);
    assert_eq!(owner.status(&key).await, Some(SessionStatus::Active));
    assert_eq!(events.recv().await.expect("event"), AccountEvent::SessionRestored { account: key.clone() });
}

/// A store whose writes can be made to fail.
#[derive(Debug, Default)]
struct FlakyStore {
    inner: MemorySecretStore,
    fail_writes: AtomicBool,
}

impl SecretStore for FlakyStore {
    fn backend(&self) -> &'static str {
        "flaky"
    }
    fn get(&self, n: &SecretName) -> Result<Option<Secret>, SecretError> {
        self.inner.get(n)
    }
    fn set(&self, n: &SecretName, v: &Secret) -> Result<(), SecretError> {
        if self.fail_writes.load(Ordering::SeqCst) {
            return Err(SecretError::Unavailable("locked".into()));
        }
        self.inner.set(n, v)
    }
    fn delete(&self, n: &SecretName) -> Result<bool, SecretError> {
        self.inner.delete(n)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rotated_token_that_cannot_be_persisted_is_never_used() {
    let (base, state) = fake_auth::start().await;
    let dir = tempfile::tempdir().expect("tmp");
    let store = Arc::new(FlakyStore::default());
    let owner = TokenOwner::new(store.clone(), AccountStore::new(dir.path()), cfg());
    let (a, r) = state.login("user-1");
    let key = owner.sign_in(&base, tokens(a.clone(), r.clone())).await.expect("sign in").key;
    store.fail_writes.store(true, Ordering::SeqCst);
    let fp = AccessToken::new(a).fingerprint();
    let err = owner.access_after_401(&key, &fp).await.expect_err("not persisted");
    assert!(matches!(err, AuthError::Transient(_)));
    // The stored token is still the old one.
    let stored = store.get(&SecretName::refresh_token(key.as_str()).expect("n")).expect("get").expect("some");
    assert_eq!(stored.expose(), r.as_bytes());
    // Store back: the old token is presented again and the grace heals it.
    store.fail_writes.store(false, Ordering::SeqCst);
    let b = owner.access_after_401(&key, &fp).await.expect("healed");
    assert!(state.access_valid(b.token.expose()));
    assert!(!state.family_revoked_for("user-1"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn accounts_switch_and_isolation() {
    let (owner, state, k1, base, _dir, _s) = setup("user-1").await;
    let (a, r) = state.login("user-2");
    let k2 = owner.sign_in(&base, tokens(a, r)).await.expect("second").key;
    assert_ne!(k1, k2);
    assert_eq!(owner.accounts().await.len(), 2);
    assert_eq!(owner.current(), Some(k1.clone()), "the first account stays current");
    let mut events = owner.subscribe();
    owner.switch(&k2).expect("switch");
    assert_eq!(events.recv().await.expect("event"), AccountEvent::Switched { account: Some(k2.clone()) });
    // Each account gets its own user's tokens.
    let t1 = owner.access_token(&k1).await.expect("t1").token;
    let t2 = owner.access_token(&k2).await.expect("t2").token;
    assert_eq!(t1.jwt_subject().as_deref(), Some("user-1"));
    assert_eq!(t2.jwt_subject().as_deref(), Some("user-2"));
    // Database keys are per account and stable.
    let d1 = owner.database_key(&k1).await.expect("dbkey");
    assert_eq!(owner.database_key(&k1).await.expect("dbkey"), d1);
    assert_ne!(owner.database_key(&k2).await.expect("dbkey"), d1);
    // Sign-out removes the account's secrets and moves "current" away from it.
    owner.sign_out(&k2).await.expect("sign out");
    assert_eq!(owner.current(), Some(k1.clone()));
    assert_eq!(owner.access_token(&k2).await.expect_err("gone"), AuthError::UnknownAccount);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn load_restores_accounts_after_a_restart() {
    let (base, state) = fake_auth::start().await;
    let dir = tempfile::tempdir().expect("tmp");
    let secrets = Arc::new(MemorySecretStore::new());
    let (a, r) = state.login("user-1");
    let key = {
        let owner = TokenOwner::new(secrets.clone(), AccountStore::new(dir.path()), cfg());
        owner.sign_in(&base, tokens(a, r)).await.expect("sign in").key
    };
    // A new process: no access token in memory, the stored refresh token gets one.
    let owner = TokenOwner::new(secrets, AccountStore::new(dir.path()), cfg());
    owner.load().await.expect("load");
    assert_eq!(owner.status(&key).await, Some(SessionStatus::Active));
    let t = owner.access_token(&key).await.expect("token").token;
    assert!(state.access_valid(t.expose()));
    assert_eq!(state.rotations.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn broker_in_process_over_the_real_transport() {
    let (owner, state, key, base, dir, _s) = setup("user-1").await;
    let endpoint = BrokerEndpoint::for_test("inproc", dir.path());
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = BrokerServer::new(endpoint.clone(), owner.clone(), ClientPolicy::SameUser);
    let handle = tokio::spawn(server.serve(async move {
        let _ = stop_rx.await;
    }));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = BrokerClient::new(endpoint.clone(), "test-app");
    let (accounts, current) = client.accounts().await.expect("accounts");
    assert_eq!(accounts.len(), 1);
    assert_eq!(current, Some(key.clone()));

    // An ApiClient whose tokens come from the broker: first call ok, then a 401 is healed through the broker.
    let source = Arc::new(client.token_source(key.clone()));
    let api = ApiClient::builder(&base).tokens(source.clone()).build().expect("api");
    api.send(ApiRequest::get("/api/v1/protected")).await.expect("first call");
    state.expire_access();
    api.send(ApiRequest::get("/api/v1/protected")).await.expect("after a 401, through the broker");
    assert_eq!(state.rotations.load(Ordering::SeqCst), 1);

    // Events reach subscribers; revocation is reported as SessionExpired to the app.
    let mut events = client.subscribe().await.expect("subscribe");
    state.revoke_user("user-1");
    let failed = source.access_token().await.expect("cached");
    assert_eq!(source.after_unauthorized(&failed).await.expect_err("revoked"), AuthError::SessionExpired);
    assert_eq!(events.next().await, Some(AccountEvent::SessionExpired { account: key.clone() }));

    // The database key travels to the app (same value as the owner's).
    assert_eq!(client.database_key(&key).await.expect("key"), owner.database_key(&key).await.expect("key"));
    // Unknown account.
    let other = AccountKey::parse("ffffffffffffffff").expect("key");
    assert!(client.access_token(&other).await.is_err());

    // A second server on the same endpoint is refused while the first runs.
    let dup = BrokerServer::new(endpoint.clone(), owner.clone(), ClientPolicy::SameUser);
    let dup_result = tokio::time::timeout(Duration::from_secs(2), dup.serve(std::future::pending())).await;
    assert!(matches!(dup_result, Ok(Err(_))), "a second broker must not share the endpoint");

    let _ = stop_tx.send(());
    let _ = handle.await;
    // The shell is gone: the app gets a transient error, not "session expired".
    let gone = BrokerClient::new(endpoint, "test-app").with_timeout(Duration::from_secs(2));
    let src = gone.token_source(key);
    assert!(matches!(src.access_token().await, Err(AuthError::Transient(_))));
}

#[test]
fn account_info_rejects_bad_input() {
    assert!(AccountInfo::new("not a url", "u").is_err());
    assert!(AccountInfo::new("https://x.example", "").is_err());
}
