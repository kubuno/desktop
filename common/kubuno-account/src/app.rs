//! The app side of the token broker (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §5.2, §9, §19.4): what chat,
//! documents, drive… use to borrow access tokens from the shell.
//!
//! - [`AppBroker::for_app`] connects to the current user's broker and **verifies the server**: the process
//!   serving the pipe/socket must run as the current user and be the installed shell
//!   (`<directory of this program>/kubuno-desktop[.exe]`). Another program squatting the endpoint while the shell
//!   is not running is refused before anything is sent ([`ServerPolicy::Images`]).
//! - When the broker is unreachable the shell is started in background mode (`kubuno-desktop --background`,
//!   detached, no window: it lives in the notification area) and the call is retried once it answers; the app
//!   never becomes a second token owner. A failure to start it is a *transient* error, never "session expired".
//! - [`AppTokenSource`] is the `TokenSource` of one account for `kubuno_api_client::ApiClient`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use kubuno_api_client::{AccessToken, AuthError, TokenSource};
use kubuno_secrets::Secret;

use crate::broker::{BrokerClient, BrokerEndpoint, BrokerError, ServerPolicy};
use crate::key::AccountKey;
use crate::owner::{AccountSummary, Borrowed};

/// The shell's executable name.
#[cfg(windows)]
pub const SHELL_EXE: &str = "kubuno-desktop.exe";
#[cfg(not(windows))]
pub const SHELL_EXE: &str = "kubuno-desktop";

/// The shell's flag for a start without a window (the logon start, and a start by an app).
pub const BACKGROUND_FLAG: &str = "--background";

/// Starts the shell in background mode.
#[derive(Debug, Clone)]
pub struct ShellLauncher {
    exe: PathBuf,
}

impl ShellLauncher {
    pub fn new(exe: impl Into<PathBuf>) -> Self {
        Self { exe: exe.into() }
    }

    /// The shell installed next to the running program (every Kubuno desktop program ships in one directory).
    pub fn beside_current_exe() -> io::Result<Self> {
        let me = std::env::current_exe()?;
        let dir = me.parent().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "the program has no directory"))?;
        Ok(Self::new(dir.join(SHELL_EXE)))
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }

    /// Starts `<exe> --background`, detached from this process (it outlives the app, has no console and inherits
    /// no standard handle). The environment is inherited, so a sandboxed app starts a sandboxed shell.
    pub fn launch(&self) -> io::Result<()> {
        if !self.exe.is_file() {
            return Err(io::Error::new(io::ErrorKind::NotFound, format!("{} does not exist", self.exe.display())));
        }
        let mut cmd = std::process::Command::new(&self.exe);
        cmd.arg(BACKGROUND_FLAG).arg("--no-splash").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        if let Some(dir) = self.exe.parent() {
            cmd.current_dir(dir);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // A session of its own: closing the app's terminal does not take the shell down.
            cmd.process_group(0);
        }
        let child = cmd.spawn()?;
        tracing::info!(pid = child.id(), exe = %self.exe.display(), "started the Kubuno shell in the background for the token broker");
        Ok(())
    }
}

/// The broker as an app sees it: verified server, shell started on demand.
#[derive(Debug)]
pub struct AppBroker {
    client: BrokerClient,
    launcher: Option<ShellLauncher>,
    start_timeout: Duration,
    last_launch: Mutex<Option<Instant>>,
}

impl AppBroker {
    /// The broker of the current user, served by the shell installed next to this program; `app` names the
    /// caller in the shell's logs (`kubuno-chat`, `kubuno-documents`...).
    pub fn for_app(app: &str) -> io::Result<Self> {
        let launcher = ShellLauncher::beside_current_exe()?;
        let endpoint = BrokerEndpoint::for_current_user(&crate::paths::user_runtime_dir()?)?;
        let client = BrokerClient::new(endpoint, app).verify_server(ServerPolicy::Images(vec![launcher.exe().to_path_buf()]));
        Ok(Self::new(client, Some(launcher)))
    }

    /// A broker with an explicit client (its server policy included) and, optionally, a launcher.
    pub fn new(client: BrokerClient, launcher: Option<ShellLauncher>) -> Self {
        Self { client, launcher, start_timeout: Duration::from_secs(20), last_launch: Mutex::new(None) }
    }

    /// How long to wait for a shell started by [`Self::ensure_running`] (20 s).
    pub fn with_start_timeout(mut self, timeout: Duration) -> Self {
        self.start_timeout = timeout;
        self
    }

    pub fn client(&self) -> &BrokerClient {
        &self.client
    }

    /// Makes sure a verified broker answers: when it is unreachable, starts the shell in background mode (at most
    /// once per `start_timeout`) and waits for it.
    pub async fn ensure_running(&self) -> Result<(), BrokerError> {
        match self.client.accounts().await {
            Ok(_) => return Ok(()),
            Err(BrokerError::Unreachable(_)) => {}
            Err(e) => return Err(e),
        }
        let Some(launcher) = &self.launcher else {
            return Err(BrokerError::Unreachable("the Kubuno shell is not running".to_string()));
        };
        let launch = {
            let mut last = self.last_launch.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let due = last.is_none_or(|at| at.elapsed() >= self.start_timeout);
            if due {
                *last = Some(Instant::now());
            }
            due
        };
        if launch {
            launcher.launch().map_err(|e| BrokerError::Unreachable(format!("the Kubuno shell could not be started: {e}")))?;
        }
        let deadline = Instant::now() + self.start_timeout;
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            match self.client.accounts().await {
                Ok(_) => return Ok(()),
                Err(BrokerError::Unreachable(e)) if Instant::now() >= deadline => {
                    return Err(BrokerError::Unreachable(format!("the Kubuno shell did not start its broker in time: {e}")));
                }
                Err(BrokerError::Unreachable(_)) => {}
                Err(e) => return Err(e),
            }
        }
    }

    /// Runs `call`; when the broker is unreachable, starts the shell and runs it once more.
    async fn with_shell<T, F, Fut>(&self, call: F) -> Result<T, BrokerError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, BrokerError>>,
    {
        match call().await {
            Err(BrokerError::Unreachable(_)) => {
                self.ensure_running().await?;
                call().await
            }
            other => other,
        }
    }

    /// The accounts the shell owns and the current one.
    pub async fn accounts(&self) -> Result<(Vec<AccountSummary>, Option<AccountKey>), BrokerError> {
        self.with_shell(|| self.client.accounts()).await
    }

    /// The account the apps should show: the shell's current account, else the first active one.
    pub async fn current_account(&self) -> Result<Option<AccountSummary>, BrokerError> {
        let (accounts, current) = self.accounts().await?;
        Ok(pick_current(accounts, current.as_ref()))
    }

    pub async fn access_token(&self, account: &AccountKey) -> Result<Borrowed, BrokerError> {
        self.with_shell(|| self.client.access_token(account)).await
    }

    pub async fn access_after_401(&self, account: &AccountKey, failed: &AccessToken) -> Result<Borrowed, BrokerError> {
        self.with_shell(|| self.client.access_after_401(account, failed)).await
    }

    /// The account's database key (to open the app's SQLCipher databases).
    pub async fn database_key(&self, account: &AccountKey) -> Result<Secret, BrokerError> {
        self.with_shell(|| self.client.database_key(account)).await
    }
}

/// The current account, else the first active one, else the first one.
pub fn pick_current(accounts: Vec<AccountSummary>, current: Option<&AccountKey>) -> Option<AccountSummary> {
    let mut accounts = accounts;
    if let Some(cur) = current {
        if let Some(i) = accounts.iter().position(|a| &a.info.key == cur) {
            return Some(accounts.swap_remove(i));
        }
    }
    if let Some(i) = accounts.iter().position(|a| a.status == crate::owner::SessionStatus::Active) {
        return Some(accounts.swap_remove(i));
    }
    accounts.into_iter().next()
}

/// Maps a broker error to the auth error of the API client: only the shell's own verdict ends a session.
pub fn auth_error(e: BrokerError) -> AuthError {
    match e {
        BrokerError::Remote { code: crate::broker::proto::ErrorCode::SessionExpired, .. } => AuthError::SessionExpired,
        BrokerError::Remote { code: crate::broker::proto::ErrorCode::UnknownAccount, .. } => AuthError::UnknownAccount,
        other => AuthError::Transient(other.to_string()),
    }
}

/// The [`TokenSource`] of one account through an [`AppBroker`], cached until shortly before expiry.
pub struct AppTokenSource {
    broker: std::sync::Arc<AppBroker>,
    account: AccountKey,
    cache: Mutex<Option<(AccessToken, Instant)>>,
    margin: Duration,
}

impl std::fmt::Debug for AppTokenSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppTokenSource").field("account", &self.account).finish()
    }
}

impl AppTokenSource {
    pub fn new(broker: std::sync::Arc<AppBroker>, account: AccountKey) -> Self {
        Self { broker, account, cache: Mutex::new(None), margin: Duration::from_secs(30) }
    }

    fn remember(&self, b: &Borrowed) {
        let until = Instant::now() + b.valid_for.saturating_sub(self.margin);
        *self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((b.token.clone(), until));
    }
}

#[async_trait::async_trait]
impl TokenSource for AppTokenSource {
    async fn access_token(&self) -> Result<AccessToken, AuthError> {
        if let Some((t, until)) = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
            if Instant::now() < *until {
                return Ok(t.clone());
            }
        }
        let b = self.broker.access_token(&self.account).await.map_err(auth_error)?;
        self.remember(&b);
        Ok(b.token)
    }

    async fn after_unauthorized(&self, failed: &AccessToken) -> Result<AccessToken, AuthError> {
        *self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        let b = self.broker.access_after_401(&self.account, failed).await.map_err(auth_error)?;
        self.remember(&b);
        Ok(b.token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::owner::SessionStatus;
    use crate::store::AccountInfo;

    fn summary(server: &str, user: &str, status: SessionStatus) -> AccountSummary {
        AccountSummary { info: AccountInfo::new(server, user).expect("info"), status }
    }

    #[test]
    fn the_current_account_wins_then_an_active_one() {
        let a = summary("https://a.example", "u1", SessionStatus::SessionExpired);
        let b = summary("https://b.example", "u2", SessionStatus::Active);
        let all = vec![a.clone(), b.clone()];
        assert_eq!(pick_current(all.clone(), Some(&a.info.key)).map(|s| s.info.key), Some(a.info.key.clone()));
        assert_eq!(pick_current(all.clone(), None).map(|s| s.info.key), Some(b.info.key.clone()));
        assert!(pick_current(Vec::new(), None).is_none());
    }

    #[tokio::test]
    async fn an_unreachable_broker_without_launcher_is_transient() {
        let dir = tempfile::tempdir().expect("tmp");
        let client = BrokerClient::new(BrokerEndpoint::for_test("nobody", dir.path()), "test").with_timeout(Duration::from_secs(2));
        let broker = AppBroker::new(client, None);
        let err = broker.accounts().await.expect_err("nobody listens");
        assert!(matches!(err, BrokerError::Unreachable(_)), "{err:?}");
        assert!(matches!(auth_error(err), AuthError::Transient(_)));
    }

    #[tokio::test]
    async fn a_missing_shell_executable_is_reported_not_spawned() {
        let dir = tempfile::tempdir().expect("tmp");
        let client = BrokerClient::new(BrokerEndpoint::for_test("noshell", dir.path()), "test").with_timeout(Duration::from_secs(2));
        let broker = AppBroker::new(client, Some(ShellLauncher::new(dir.path().join(SHELL_EXE))));
        let err = broker.ensure_running().await.expect_err("no shell to start");
        assert!(err.to_string().contains("could not be started"), "{err}");
    }
}
