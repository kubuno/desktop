//! Where the file sync (and every `kubuno_desktop_sync` call) gets its access tokens.
//!
//! There is exactly **one** owner of each refresh token on a machine: the shell's `TokenOwner`
//! (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §5.2, §9, §17). This crate never reads or writes a refresh token; it
//! asks the [`TokenProvider`] installed in the process:
//!
//! - inside the shell, [`OwnerProvider`]: the shell's `TokenOwner`, in process;
//! - in any other program (the `kubuno-sync` CLI, chat, documents), [`BrokerProvider`]: the shell's token broker,
//!   whose server is verified to be the installed shell, and which starts the shell in background mode when it
//!   is not running.
//!
//! The provider also maps a file-sync instance to its account (`account.json`'s `linked_instances`): an instance
//! that belongs to no signed-in account has no session ([`AuthFailure::Genuine`]: sign in again).

use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::Duration;

use kubuno_desktop_account::app::AppBroker;
use kubuno_desktop_account::broker::BrokerError;
use kubuno_desktop_account::{AccountKey, AccountSummary, SessionStatus, TokenOwner};
use kubuno_desktop_api_client::{AccessToken, AuthError};

use crate::api::AuthFailure;

/// An account as the callers of this crate see it (no secret).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRef {
    /// The account key (`kubuno_desktop_account::AccountKey`, 16 hex characters).
    pub key: String,
    pub server_url: String,
    pub user_id: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    /// The session is usable (not expired, not signed out).
    pub active: bool,
    /// The file-sync instances of the account.
    pub instances: Vec<String>,
}

impl AccountRef {
    fn from_summary(s: &AccountSummary) -> Self {
        Self {
            key: s.info.key.as_str().to_string(),
            server_url: s.info.server_url.clone(),
            user_id: s.info.user_id.clone(),
            display_name: s.info.display_name.clone(),
            email: s.info.email.clone(),
            active: s.status == SessionStatus::Active,
            instances: s.info.linked_instances.clone(),
        }
    }
}

/// The source of access tokens of this process (see the module doc). Blocking: it is called from the sync threads.
pub trait TokenProvider: Send + Sync {
    /// The account file-sync instance `instance` belongs to.
    fn account_of_instance(&self, instance: &str) -> Result<String, AuthFailure>;
    /// A valid access token of `account`.
    fn access_token(&self, account: &str) -> Result<String, AuthFailure>;
    /// The server answered 401 to `failed`: a newer token (refreshed at most once by the owner).
    fn after_unauthorized(&self, account: &str, failed: &str) -> Result<String, AuthFailure>;
    /// Every account of the shell.
    fn accounts(&self) -> Result<Vec<AccountRef>, AuthFailure>;
    /// The account the apps show (the shell's current one, else the first active one).
    fn current_account(&self) -> Result<Option<AccountRef>, AuthFailure>;
}

static PROVIDER: RwLock<Option<Arc<dyn TokenProvider>>> = RwLock::new(None);

/// Installs the provider of this process (once at start-up; a later call replaces it).
pub fn install(provider: Arc<dyn TokenProvider>) {
    *PROVIDER.write().unwrap_or_else(PoisonError::into_inner) = Some(provider);
}

/// Removes the provider (tests).
pub fn uninstall() {
    *PROVIDER.write().unwrap_or_else(PoisonError::into_inner) = None;
}

/// The provider of this process. None installed is a transient failure (a program forgot to install one, or is
/// still starting): never "session expired".
pub fn provider() -> Result<Arc<dyn TokenProvider>, AuthFailure> {
    match PROVIDER.read().unwrap_or_else(PoisonError::into_inner).as_ref() {
        Some(p) => Ok(p.clone()),
        None => {
            tracing::warn!("no token provider is installed in this process: the request cannot be authenticated");
            Err(AuthFailure::Transient)
        }
    }
}

/// The account the apps show, through the installed provider.
pub fn current_account() -> Result<Option<AccountRef>, AuthFailure> {
    provider()?.current_account()
}

/// Every account, through the installed provider.
pub fn accounts() -> Result<Vec<AccountRef>, AuthFailure> {
    provider()?.accounts()
}

fn map_auth(e: AuthError) -> AuthFailure {
    match e {
        AuthError::SessionExpired | AuthError::UnknownAccount => AuthFailure::Genuine,
        AuthError::Transient(_) => AuthFailure::Transient,
    }
}

fn parse_key(account: &str) -> Result<AccountKey, AuthFailure> {
    AccountKey::parse(account).map_err(|_| AuthFailure::Genuine)
}

/// Runs a future on `handle` from a plain (non-runtime) thread and waits for it.
fn run<T: Send + 'static>(handle: &tokio::runtime::Handle, timeout: Duration, fut: impl Future<Output = T> + Send + 'static) -> Result<T, AuthFailure> {
    let (tx, rx) = std::sync::mpsc::channel();
    handle.spawn(async move {
        let _ = tx.send(fut.await);
    });
    rx.recv_timeout(timeout).map_err(|_| {
        tracing::warn!("token request timed out");
        AuthFailure::Transient
    })
}

/// The shell's provider: its own `TokenOwner`, in process.
pub struct OwnerProvider {
    owner: Arc<TokenOwner>,
    handle: tokio::runtime::Handle,
    timeout: Duration,
}

impl OwnerProvider {
    /// `handle`: the shell's runtime (the owner's futures run there).
    pub fn new(owner: Arc<TokenOwner>, handle: tokio::runtime::Handle) -> Self {
        Self { owner, handle, timeout: Duration::from_secs(60) }
    }
}

impl TokenProvider for OwnerProvider {
    fn account_of_instance(&self, instance: &str) -> Result<String, AuthFailure> {
        match self.owner.account_store().account_of_instance(instance) {
            Ok(Some(info)) => Ok(info.key.as_str().to_string()),
            Ok(None) => {
                tracing::warn!(instance, "this sync folder belongs to no signed-in account: sign in again");
                Err(AuthFailure::Genuine)
            }
            Err(e) => {
                tracing::warn!(instance, error = %e, "the accounts cannot be read");
                Err(AuthFailure::Transient)
            }
        }
    }

    fn access_token(&self, account: &str) -> Result<String, AuthFailure> {
        let key = parse_key(account)?;
        let owner = self.owner.clone();
        let b = run(&self.handle, self.timeout, async move { owner.access_token(&key).await })?.map_err(map_auth)?;
        Ok(b.token.expose().to_string())
    }

    fn after_unauthorized(&self, account: &str, failed: &str) -> Result<String, AuthFailure> {
        let key = parse_key(account)?;
        let owner = self.owner.clone();
        let fp = AccessToken::new(failed.to_string()).fingerprint();
        let b = run(&self.handle, self.timeout, async move { owner.access_after_401(&key, &fp).await })?.map_err(map_auth)?;
        Ok(b.token.expose().to_string())
    }

    fn accounts(&self) -> Result<Vec<AccountRef>, AuthFailure> {
        let owner = self.owner.clone();
        let all = run(&self.handle, self.timeout, async move { owner.accounts().await })?;
        Ok(all.iter().map(AccountRef::from_summary).collect())
    }

    fn current_account(&self) -> Result<Option<AccountRef>, AuthFailure> {
        let owner = self.owner.clone();
        let current = self.owner.current();
        let all = run(&self.handle, self.timeout, async move { owner.accounts().await })?;
        Ok(kubuno_desktop_account::app::pick_current(all, current.as_ref()).as_ref().map(AccountRef::from_summary))
    }
}

/// An app's provider: the shell's token broker (server verified, shell started when needed).
pub struct BrokerProvider {
    broker: Arc<AppBroker>,
    runtime: tokio::runtime::Runtime,
    timeout: Duration,
    /// Instance -> account, from the last account list.
    instances: Mutex<Vec<(String, String)>>,
}

fn map_broker(e: BrokerError) -> AuthFailure {
    if let BrokerError::ServerRefused(why) = &e {
        tracing::error!(reason = %why, "the token broker endpoint is not served by the Kubuno shell");
    }
    map_auth(kubuno_desktop_account::app::auth_error(e))
}

impl BrokerProvider {
    /// The broker of the current user, served by the shell installed next to this program.
    pub fn for_app(app: &str) -> std::io::Result<Self> {
        Self::new(AppBroker::for_app(app)?)
    }

    pub fn new(broker: AppBroker) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).thread_name("kubuno-broker-client").enable_all().build()?;
        Ok(Self { broker: Arc::new(broker), runtime, timeout: Duration::from_secs(60), instances: Mutex::new(Vec::new()) })
    }

    pub fn broker(&self) -> &Arc<AppBroker> {
        &self.broker
    }

    fn summaries(&self) -> Result<(Vec<AccountSummary>, Option<AccountKey>), AuthFailure> {
        let broker = self.broker.clone();
        let (all, current) = run(self.runtime.handle(), self.timeout, async move { broker.accounts().await })?.map_err(map_broker)?;
        let map = all.iter().flat_map(|a| a.info.linked_instances.iter().map(|i| (i.clone(), a.info.key.as_str().to_string()))).collect();
        *self.instances.lock().unwrap_or_else(PoisonError::into_inner) = map;
        Ok((all, current))
    }
}

impl TokenProvider for BrokerProvider {
    fn account_of_instance(&self, instance: &str) -> Result<String, AuthFailure> {
        let cached = self.instances.lock().unwrap_or_else(PoisonError::into_inner).iter().find(|(i, _)| i == instance).map(|(_, a)| a.clone());
        if let Some(a) = cached {
            return Ok(a);
        }
        self.summaries()?;
        self.instances
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(i, _)| i == instance)
            .map(|(_, a)| a.clone())
            .ok_or(AuthFailure::Genuine)
    }

    fn access_token(&self, account: &str) -> Result<String, AuthFailure> {
        let key = parse_key(account)?;
        let broker = self.broker.clone();
        let b = run(self.runtime.handle(), self.timeout, async move { broker.access_token(&key).await })?.map_err(map_broker)?;
        Ok(b.token.expose().to_string())
    }

    fn after_unauthorized(&self, account: &str, failed: &str) -> Result<String, AuthFailure> {
        let key = parse_key(account)?;
        let broker = self.broker.clone();
        let failed = AccessToken::new(failed.to_string());
        let b = run(self.runtime.handle(), self.timeout, async move { broker.access_after_401(&key, &failed).await })?.map_err(map_broker)?;
        Ok(b.token.expose().to_string())
    }

    fn accounts(&self) -> Result<Vec<AccountRef>, AuthFailure> {
        Ok(self.summaries()?.0.iter().map(AccountRef::from_summary).collect())
    }

    fn current_account(&self) -> Result<Option<AccountRef>, AuthFailure> {
        let (all, current) = self.summaries()?;
        Ok(kubuno_desktop_account::app::pick_current(all, current.as_ref()).as_ref().map(AccountRef::from_summary))
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! A provider for this crate's tests: fixed tokens, counts the refreshes.

    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    pub(crate) struct FixedProvider {
        pub(crate) refreshes: AtomicU32,
        pub(crate) failure: Option<AuthFailure>,
    }

    impl TokenProvider for FixedProvider {
        fn account_of_instance(&self, instance: &str) -> Result<String, AuthFailure> {
            if instance == "orphan" {
                return Err(AuthFailure::Genuine);
            }
            Ok("0123456789abcdef".to_string())
        }
        fn access_token(&self, _: &str) -> Result<String, AuthFailure> {
            self.failure.map_or(Ok("t0".to_string()), Err)
        }
        fn after_unauthorized(&self, _: &str, _: &str) -> Result<String, AuthFailure> {
            let n = self.refreshes.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(format!("t{n}"))
        }
        fn accounts(&self) -> Result<Vec<AccountRef>, AuthFailure> {
            Ok(Vec::new())
        }
        fn current_account(&self) -> Result<Option<AccountRef>, AuthFailure> {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::test_support::FixedProvider;
    use super::*;
    use crate::api::Api;

    /// A one-route HTTP server: 401 to any token but `t1`, `{"ok":true}` to `t1`. Serves `n` connections.
    fn server(n: usize) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            for stream in listener.incoming().take(n) {
                let Ok(mut s) = stream else { continue };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(k) => buf.extend_from_slice(&chunk[..k]),
                    }
                }
                let head = String::from_utf8_lossy(&buf).to_ascii_lowercase();
                let reply = if head.contains("authorization: bearer t1") {
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}"
                } else {
                    "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                };
                let _ = s.write_all(reply.as_bytes());
            }
        });
        format!("http://{addr}")
    }

    // One test owns the process-wide provider (tests run in parallel threads).
    #[test]
    fn the_engine_takes_its_tokens_from_the_installed_provider_only() {
        uninstall();
        let err = Api::new("inst".into(), "http://127.0.0.1:9".into()).access_token().expect_err("no provider");
        assert_eq!(err.downcast_ref::<AuthFailure>(), Some(&AuthFailure::Transient), "no provider is never 'session expired'");

        let provider = Arc::new(FixedProvider { refreshes: AtomicU32::new(0), failure: None });
        install(provider.clone());
        assert_eq!(Api::new("inst".into(), "http://127.0.0.1:9".into()).access_token().expect("token"), "t0");
        let err = Api::new("orphan".into(), "http://127.0.0.1:9".into()).access_token().expect_err("orphan");
        assert_eq!(err.downcast_ref::<AuthFailure>(), Some(&AuthFailure::Genuine), "an instance of no account has no session");
        assert!(crate::daemon::is_session_over(&err));

        // A 401 takes the provider's next token (t1) and retries once; no refresh token anywhere.
        let base = server(2);
        let v = Api::new("inst".into(), base).get_json("/api/v1/x").expect("retried with the refreshed token");
        assert_eq!(v["ok"], serde_json::Value::Bool(true));
        assert_eq!(provider.refreshes.load(Ordering::SeqCst), 1);

        // A revoked session: Genuine, which the daemon reads as "pause", never as a deletion trigger.
        install(Arc::new(FixedProvider { refreshes: AtomicU32::new(0), failure: Some(AuthFailure::Genuine) }));
        let err = Api::new("inst".into(), "http://127.0.0.1:9".into()).access_token().expect_err("revoked");
        assert!(crate::daemon::is_session_over(&err));
        uninstall();
    }
}
