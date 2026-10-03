//! The shell as the broker's server, in its own process (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §19.4):
//!
//! - an app whose broker is unreachable **starts the shell** (`--background`), waits for it, verifies that the
//!   process serving the endpoint is the expected shell executable, borrows a token and gets it refreshed after a
//!   401 by the shell's process;
//! - a **squatter** (another program of the same user serving the endpoint while the shell is not running) is
//!   refused by the app before any request is sent, and no shell is started instead.
//!
//! This binary has its own `main` (`harness = false`): started with `KUBUNO_FAKE_SHELL_ENDPOINT` set it *is* the
//! shell (a token owner of one account against the fake auth server, in-memory secrets, temporary data directory),
//! and it accepts the shell's `--background --no-splash` flags that libtest would reject. Nothing here touches the
//! OS credential store, the user's profile or a real server.

mod fake_auth;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use kubuno_desktop_account::app::{AppBroker, AppTokenSource, ShellLauncher};
use kubuno_desktop_account::broker::{BrokerClient, BrokerEndpoint, BrokerError, BrokerServer, ClientPolicy, ServerPolicy};
use kubuno_desktop_account::login::NativeTokens;
use kubuno_desktop_account::{AccountError, AccountEvent, AccountInfo, AccountKey, AccountStore, AccountSummary, Borrowed, BrokerBackend, OwnerConfig, TokenOwner};
use kubuno_desktop_api_client::{AccessToken, ApiClient, ApiRequest, AuthError};
use kubuno_desktop_secrets::{MemorySecretStore, Secret};

const ENV_ENDPOINT: &str = "KUBUNO_FAKE_SHELL_ENDPOINT";
const ENV_SERVER: &str = "KUBUNO_FAKE_SHELL_SERVER";
const ENV_USER: &str = "KUBUNO_FAKE_SHELL_USER";
const ENV_ACCESS: &str = "KUBUNO_FAKE_SHELL_ACCESS";
const ENV_REFRESH: &str = "KUBUNO_FAKE_SHELL_REFRESH";
const ENV_DATA: &str = "KUBUNO_FAKE_SHELL_DATA";
const ENV_LIFETIME: &str = "KUBUNO_FAKE_SHELL_LIFETIME_MS";

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"))
}

/// The child: a stand-in for `kubuno-desktop --background`.
async fn run_fake_shell() {
    let endpoint = BrokerEndpoint::from_arg(&var(ENV_ENDPOINT));
    let lifetime = std::env::var(ENV_LIFETIME).ok().and_then(|v| v.parse().ok()).unwrap_or(20_000u64);
    let owner = TokenOwner::new(Arc::new(MemorySecretStore::new()), AccountStore::new(var(ENV_DATA)), OwnerConfig::default());
    let tokens = NativeTokens { access_token: AccessToken::new(var(ENV_ACCESS)), refresh_token: Secret::from_string(var(ENV_REFRESH)) };
    let info = AccountInfo::new(&var(ENV_SERVER), &var(ENV_USER)).expect("account");
    owner.adopt(info, tokens).await.expect("adopt");
    let bound = BrokerServer::new(endpoint, owner, ClientPolicy::SameUser).bind().await.expect("bind the endpoint");
    let _ = bound.serve(tokio::time::sleep(Duration::from_millis(lifetime))).await;
}

/// An app with no shell running starts it, verifies it, and borrows through it.
async fn an_app_starts_the_shell_in_the_background_and_borrows_through_it() {
    let (base, state) = fake_auth::start().await;
    let dir = tempfile::tempdir().expect("tmp");
    let (access, refresh) = state.login("user-30");
    let endpoint = BrokerEndpoint::for_test("autostart", dir.path());
    // What the started shell reads (the launcher passes this process's environment on).
    std::env::set_var(ENV_ENDPOINT, endpoint.to_arg());
    std::env::set_var(ENV_SERVER, &base);
    std::env::set_var(ENV_USER, "user-30");
    std::env::set_var(ENV_ACCESS, &access);
    std::env::set_var(ENV_REFRESH, &refresh);
    std::env::set_var(ENV_DATA, dir.path().join("shell-data"));
    std::env::set_var(ENV_LIFETIME, "15000");

    let exe = std::env::current_exe().expect("exe");
    let client = BrokerClient::new(endpoint, "test-app").with_timeout(Duration::from_secs(5)).verify_server(ServerPolicy::Images(vec![exe.clone()]));
    let broker = Arc::new(AppBroker::new(client, Some(ShellLauncher::new(&exe))).with_start_timeout(Duration::from_secs(20)));
    let current = broker.current_account().await.expect("the shell was started and answers").expect("one account");
    // Only the launch above needed them.
    for v in [ENV_ENDPOINT, ENV_SERVER, ENV_USER, ENV_ACCESS, ENV_REFRESH, ENV_DATA, ENV_LIFETIME] {
        std::env::remove_var(v);
    }
    let key = AccountKey::new(&base, "user-30").expect("key");
    assert_eq!(current.info.key, key);

    let source = Arc::new(AppTokenSource::new(broker.clone(), key.clone()));
    let api = ApiClient::builder(&base).tokens(source).build().expect("api");
    api.send(ApiRequest::get("/api/v1/protected")).await.expect("works with the borrowed token");
    // The server forgets every access token: the app's 401 makes the SHELL's process rotate, once.
    state.expire_access();
    api.send(ApiRequest::get("/api/v1/protected")).await.expect("works after the shell refreshed");
    assert_eq!(state.rotations.load(Ordering::SeqCst), 1, "one rotation, done by the shell process");
    assert!(!state.family_revoked_for("user-30"));
    println!("ok: an app starts the shell in the background and borrows through it");
}

/// Counts what reaches a squatting server's backend.
struct CountingBackend {
    inner: Arc<TokenOwner>,
    calls: AtomicU32,
}

#[async_trait::async_trait]
impl BrokerBackend for CountingBackend {
    async fn access_token(&self, key: &AccountKey) -> Result<Borrowed, AuthError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.access_token(key).await
    }
    async fn access_after_401(&self, key: &AccountKey, failed: &str) -> Result<Borrowed, AuthError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.access_after_401(key, failed).await
    }
    async fn accounts(&self) -> Vec<AccountSummary> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.accounts().await
    }
    fn current(&self) -> Option<AccountKey> {
        self.inner.current()
    }
    fn switch(&self, key: &AccountKey) -> Result<(), AccountError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.switch(key)
    }
    async fn database_key(&self, key: &AccountKey) -> Result<Secret, AccountError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.database_key(key).await
    }
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<AccountEvent> {
        self.inner.subscribe()
    }
}

/// Another program of the same user serves the endpoint (the shell is not running): the app refuses it before
/// sending anything, and does not start a shell instead.
async fn a_squatter_serving_the_endpoint_is_refused() {
    let (base, state) = fake_auth::start().await;
    let dir = tempfile::tempdir().expect("tmp");
    let owner = TokenOwner::new(Arc::new(MemorySecretStore::new()), AccountStore::new(dir.path()), OwnerConfig::default());
    let (a, r) = state.login("user-31");
    owner.sign_in(&base, NativeTokens { access_token: AccessToken::new(a), refresh_token: Secret::from_string(r) }).await.expect("sign in");
    let backend = Arc::new(CountingBackend { inner: owner, calls: AtomicU32::new(0) });
    // This test binary is the squatter: it is not the "installed shell" the app expects.
    let endpoint = BrokerEndpoint::for_test("squat", dir.path());
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(BrokerServer::new(endpoint.clone(), backend.clone(), ClientPolicy::SameUser).serve(async move {
        let _ = stop_rx.await;
    }));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let installed_shell = dir.path().join("install").join(kubuno_desktop_account::app::SHELL_EXE);
    std::fs::create_dir_all(installed_shell.parent().expect("dir")).expect("dir");
    std::fs::write(&installed_shell, b"not started").expect("shell stand-in");
    let client = BrokerClient::new(endpoint.clone(), "test-app").with_timeout(Duration::from_secs(5)).verify_server(ServerPolicy::Images(vec![installed_shell.clone()]));
    let broker = AppBroker::new(client, Some(ShellLauncher::new(&installed_shell)));
    let err = broker.current_account().await.expect_err("the squatter must be refused");
    assert!(matches!(err, BrokerError::ServerRefused(_)), "{err:?}");
    let err = broker.access_token(&AccountKey::new(&base, "user-31").expect("key")).await.expect_err("refused again");
    assert!(matches!(err, BrokerError::ServerRefused(_)), "{err:?}");
    assert!(matches!(kubuno_desktop_account::app::auth_error(err), AuthError::Transient(_)), "never 'session expired'");
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0, "nothing reached the squatter");
    assert_eq!(state.rotations.load(Ordering::SeqCst), 0);

    // Control: the same server is accepted when it IS the expected program (this test binary).
    let exe = std::env::current_exe().expect("exe");
    let trusting = BrokerClient::new(endpoint, "test-app").verify_server(ServerPolicy::Images(vec![exe]));
    let (accounts, _) = trusting.accounts().await.expect("the expected program is accepted");
    assert_eq!(accounts.len(), 1);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    let _ = stop_tx.send(());
    let _ = server.await;
    println!("ok: a squatter serving the endpoint is refused");
}

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build().expect("runtime");
    if std::env::var_os(ENV_ENDPOINT).is_some() {
        // We are the started "shell".
        runtime.block_on(run_fake_shell());
        return;
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        // `cargo test -- --list` (IDE discovery): this harness has two tests.
        println!("an_app_starts_the_shell_in_the_background_and_borrows_through_it: test");
        println!("a_squatter_serving_the_endpoint_is_refused: test");
        return;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Diagnostics::Debug::{SetErrorMode, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX};
        // SAFETY: changes this process's (and its children's) error mode only: no error dialog box.
        unsafe { SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX) };
    }
    runtime.block_on(async {
        a_squatter_serving_the_endpoint_is_refused().await;
        an_app_starts_the_shell_in_the_background_and_borrows_through_it().await;
    });
    println!("test result: ok. 2 passed");
}
