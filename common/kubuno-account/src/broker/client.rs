//! The broker client, used by the apps (chat, documents, drive...) to borrow access tokens from the shell.
//!
//! One short connection per request (local IPC is cheap, and a restarted shell is picked up transparently);
//! [`BrokerClient::subscribe`] keeps one long connection for events. When the shell is not running the calls fail
//! with [`BrokerError::Unreachable`], which the token source maps to a *transient* auth error: the app starts the
//! shell in background mode (`kubuno-desktop --background`) and retries, it never becomes a second token owner.

use std::io;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use kubuno_api_client::{AccessToken, AuthError, TokenSource};
use kubuno_secrets::Secret;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use super::proto::{encode, ErrorCode, Request, Response, MAX_LINE, PROTOCOL_VERSION};
use super::transport::{BrokerEndpoint, ServerPolicy};
use crate::key::AccountKey;
use crate::owner::{AccountEvent, AccountSummary, Borrowed};

/// Errors of the broker client.
#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    /// No broker listens (the shell is not running) or the connection failed.
    #[error("the Kubuno shell's token broker is unreachable: {0}")]
    Unreachable(String),
    /// The broker answered something unexpected.
    #[error("broker protocol error: {0}")]
    Protocol(String),
    /// The broker refused the request.
    #[error("broker error {code:?}: {message}")]
    Remote { code: ErrorCode, message: String },
    /// The process serving the endpoint is not the Kubuno shell (squatting, or another user): nothing was sent.
    #[error("the process serving the broker endpoint is not the Kubuno shell: {0}")]
    ServerRefused(String),
}

impl BrokerError {
    fn into_auth(self) -> AuthError {
        match self {
            BrokerError::Remote { code: ErrorCode::SessionExpired, .. } => AuthError::SessionExpired,
            BrokerError::Remote { code: ErrorCode::UnknownAccount, .. } => AuthError::UnknownAccount,
            other => AuthError::Transient(other.to_string()),
        }
    }
}

trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

/// A connection to the broker.
#[derive(Debug, Clone)]
pub struct BrokerClient {
    endpoint: BrokerEndpoint,
    app: String,
    timeout: Duration,
    server: ServerPolicy,
}

/// The event stream of [`BrokerClient::subscribe`].
pub struct EventStream {
    reader: BufReader<tokio::io::ReadHalf<Box<dyn Stream>>>,
    _writer: tokio::io::WriteHalf<Box<dyn Stream>>,
    buf: Vec<u8>,
}

impl EventStream {
    /// The next event; `None` when the broker went away (the shell exited: reconnect later).
    pub async fn next(&mut self) -> Option<AccountEvent> {
        loop {
            self.buf.clear();
            let n = (&mut self.reader).take(MAX_LINE as u64 + 1).read_until(b'\n', &mut self.buf).await.ok()?;
            if n == 0 || self.buf.len() > MAX_LINE {
                return None;
            }
            match serde_json::from_slice::<Response>(&self.buf) {
                Ok(Response::Event { event }) => return Some(event),
                Ok(_) => continue,
                Err(_) => return None,
            }
        }
    }
}

impl BrokerClient {
    /// `app` names the calling application in the broker's logs.
    pub fn new(endpoint: BrokerEndpoint, app: impl Into<String>) -> Self {
        Self { endpoint, app: app.into(), timeout: Duration::from_secs(30), server: ServerPolicy::SameUser }
    }

    /// What the process serving the endpoint must be, checked on every connection before anything is sent (an
    /// app passes [`ServerPolicy::Images`] with the installed shell executable). The default,
    /// [`ServerPolicy::SameUser`], only relies on the transport's own access control: tests and tools.
    pub fn verify_server(mut self, policy: ServerPolicy) -> Self {
        self.server = policy;
        self
    }

    pub fn server_policy(&self) -> &ServerPolicy {
        &self.server
    }

    /// Windows: the server end must belong to a process of the current user whose image the policy accepts.
    #[cfg(windows)]
    fn check_pipe_server(&self, pipe: &tokio::net::windows::named_pipe::NamedPipeClient) -> Result<(), BrokerError> {
        use std::os::windows::io::AsRawHandle;

        use super::transport::sys;

        let pid = sys::pipe_server_pid(pipe.as_raw_handle() as _)
            .map_err(|e| BrokerError::ServerRefused(format!("the server process cannot be identified ({e})")))?;
        let info = sys::process_info(pid);
        let me = sys::current_user_sid().map_err(|e| BrokerError::ServerRefused(format!("the current user cannot be identified ({e})")))?;
        if info.user_sid.as_deref() != Some(me.as_str()) {
            tracing::error!(pid, "broker: the pipe is served by a process of another user (or one that cannot be queried); refused");
            return Err(BrokerError::ServerRefused(format!("process {pid} does not run as the current user")));
        }
        if !self.server.allows_image(info.image.as_deref()) {
            tracing::error!(pid, image = ?info.image, "broker: the pipe is served by a program that is not the Kubuno shell; refused");
            return Err(BrokerError::ServerRefused(format!("process {pid} is not the installed shell")));
        }
        Ok(())
    }

    /// Unix: the peer must run as the effective user, with an image the policy accepts.
    #[cfg(unix)]
    fn check_socket_server(&self, s: &tokio::net::UnixStream) -> Result<(), BrokerError> {
        use super::transport::sys;

        let cred = s.peer_cred().map_err(|e| BrokerError::ServerRefused(format!("the server cannot be identified ({e})")))?;
        if cred.uid() != sys::effective_uid() {
            tracing::error!(uid = cred.uid(), "broker: the socket is served by another user; refused");
            return Err(BrokerError::ServerRefused("the server runs as another user".to_string()));
        }
        if matches!(self.server, ServerPolicy::SameUser) {
            return Ok(());
        }
        let image = cred.pid().and_then(sys::process_image);
        if !self.server.allows_image(image.as_deref()) {
            tracing::error!(pid = ?cred.pid(), image = ?image, "broker: the socket is served by a program that is not the Kubuno shell; refused");
            return Err(BrokerError::ServerRefused("the server is not the installed shell".to_string()));
        }
        Ok(())
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn endpoint(&self) -> &BrokerEndpoint {
        &self.endpoint
    }

    async fn open(&self) -> Result<Box<dyn Stream>, BrokerError> {
        match &self.endpoint {
            #[cfg(windows)]
            BrokerEndpoint::Pipe(name) => {
                use tokio::net::windows::named_pipe::ClientOptions;
                // ERROR_PIPE_BUSY: every instance is taken for a moment; wait a little and retry.
                const ERROR_PIPE_BUSY: i32 = 231;
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    match ClientOptions::new().open(name) {
                        Ok(c) => {
                            self.check_pipe_server(&c)?;
                            return Ok(Box::new(c));
                        }
                        Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && Instant::now() < deadline => {
                            tokio::time::sleep(Duration::from_millis(20)).await;
                        }
                        Err(e) => return Err(BrokerError::Unreachable(e.to_string())),
                    }
                }
            }
            #[cfg(unix)]
            BrokerEndpoint::Socket(path) => {
                let s = tokio::net::UnixStream::connect(path).await.map_err(|e| BrokerError::Unreachable(e.to_string()))?;
                self.check_socket_server(&s)?;
                Ok(Box::new(s))
            }
            #[allow(unreachable_patterns)]
            other => Err(BrokerError::Unreachable(format!("endpoint {other:?} is not available on this OS"))),
        }
    }

    async fn exchange(
        reader: &mut BufReader<tokio::io::ReadHalf<Box<dyn Stream>>>,
        writer: &mut tokio::io::WriteHalf<Box<dyn Stream>>,
        req: &Request,
    ) -> Result<Response, BrokerError> {
        let io_err = |e: io::Error| BrokerError::Unreachable(e.to_string());
        let bytes = encode(req).map_err(|e| BrokerError::Protocol(e.to_string()))?;
        writer.write_all(&bytes).await.map_err(io_err)?;
        writer.flush().await.map_err(io_err)?;
        let mut buf = Vec::new();
        let n = reader.take(MAX_LINE as u64 + 1).read_until(b'\n', &mut buf).await.map_err(io_err)?;
        if n == 0 {
            return Err(BrokerError::Unreachable("the broker closed the connection".into()));
        }
        if buf.len() > MAX_LINE {
            return Err(BrokerError::Protocol("response too long".into()));
        }
        let resp: Response = serde_json::from_slice(&buf).map_err(|e| BrokerError::Protocol(e.to_string()))?;
        if let Response::Error { code, message } = resp {
            return Err(BrokerError::Remote { code, message });
        }
        Ok(resp)
    }

    async fn connect_greeted(
        &self,
    ) -> Result<(BufReader<tokio::io::ReadHalf<Box<dyn Stream>>>, tokio::io::WriteHalf<Box<dyn Stream>>), BrokerError> {
        let stream = self.open().await?;
        let (r, mut w) = tokio::io::split(stream);
        let mut reader = BufReader::new(r);
        match Self::exchange(&mut reader, &mut w, &Request::Hello { version: PROTOCOL_VERSION, app: self.app.clone() }).await? {
            Response::Hello { version } if version == PROTOCOL_VERSION => Ok((reader, w)),
            other => Err(BrokerError::Protocol(format!("unexpected greeting {other:?}"))),
        }
    }

    async fn call(&self, req: Request) -> Result<Response, BrokerError> {
        let fut = async {
            let (mut reader, mut w) = self.connect_greeted().await?;
            Self::exchange(&mut reader, &mut w, &req).await
        };
        tokio::time::timeout(self.timeout, fut).await.map_err(|_| BrokerError::Unreachable("timed out".into()))?
    }

    fn token(resp: Response) -> Result<Borrowed, BrokerError> {
        match resp {
            Response::Token { token, valid_for_s } => Ok(Borrowed { token: AccessToken::new(token), valid_for: Duration::from_secs(valid_for_s) }),
            other => Err(BrokerError::Protocol(format!("expected a token, got {other:?}"))),
        }
    }

    pub async fn access_token(&self, account: &AccountKey) -> Result<Borrowed, BrokerError> {
        Self::token(self.call(Request::AccessToken { account: account.clone() }).await?)
    }

    pub async fn access_after_401(&self, account: &AccountKey, failed: &AccessToken) -> Result<Borrowed, BrokerError> {
        Self::token(self.call(Request::AccessAfter401 { account: account.clone(), failed: failed.fingerprint() }).await?)
    }

    pub async fn accounts(&self) -> Result<(Vec<AccountSummary>, Option<AccountKey>), BrokerError> {
        match self.call(Request::Accounts).await? {
            Response::Accounts { accounts, current } => Ok((accounts, current)),
            other => Err(BrokerError::Protocol(format!("expected accounts, got {other:?}"))),
        }
    }

    pub async fn switch_account(&self, account: &AccountKey) -> Result<(), BrokerError> {
        match self.call(Request::SwitchAccount { account: account.clone() }).await? {
            Response::Ok => Ok(()),
            other => Err(BrokerError::Protocol(format!("expected ok, got {other:?}"))),
        }
    }

    /// The account's database key (to open the app's SQLCipher databases).
    pub async fn database_key(&self, account: &AccountKey) -> Result<Secret, BrokerError> {
        match self.call(Request::DatabaseKey { account: account.clone() }).await? {
            Response::Secret { value } => Ok(Secret::from_string(value)),
            other => Err(BrokerError::Protocol(format!("expected a secret, got {other:?}"))),
        }
    }

    /// Opens the event stream (session expired, account added/removed, switched).
    pub async fn subscribe(&self) -> Result<EventStream, BrokerError> {
        let (mut reader, mut w) = self.connect_greeted().await?;
        match Self::exchange(&mut reader, &mut w, &Request::Subscribe).await? {
            Response::Ok => Ok(EventStream { reader, _writer: w, buf: Vec::new() }),
            other => Err(BrokerError::Protocol(format!("expected ok, got {other:?}"))),
        }
    }

    /// The [`TokenSource`] of one account, for `kubuno_api_client::ApiClient`.
    pub fn token_source(&self, account: AccountKey) -> BrokerTokenSource {
        BrokerTokenSource { client: self.clone(), account, cache: Mutex::new(None), margin: Duration::from_secs(30) }
    }
}

/// Access tokens of one account borrowed from the broker, cached in the app until shortly before they expire.
pub struct BrokerTokenSource {
    client: BrokerClient,
    account: AccountKey,
    cache: Mutex<Option<(AccessToken, Instant)>>,
    margin: Duration,
}

impl std::fmt::Debug for BrokerTokenSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrokerTokenSource").field("account", &self.account).finish()
    }
}

impl BrokerTokenSource {
    pub fn account(&self) -> &AccountKey {
        &self.account
    }

    fn remember(&self, b: &Borrowed) {
        let until = Instant::now() + b.valid_for.saturating_sub(self.margin);
        *self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((b.token.clone(), until));
    }
}

#[async_trait::async_trait]
impl TokenSource for BrokerTokenSource {
    async fn access_token(&self) -> Result<AccessToken, AuthError> {
        if let Some((t, until)) = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
            if Instant::now() < *until {
                return Ok(t.clone());
            }
        }
        let b = self.client.access_token(&self.account).await.map_err(BrokerError::into_auth)?;
        self.remember(&b);
        Ok(b.token)
    }

    async fn after_unauthorized(&self, failed: &AccessToken) -> Result<AccessToken, AuthError> {
        *self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        let b = self.client.access_after_401(&self.account, failed).await.map_err(BrokerError::into_auth)?;
        self.remember(&b);
        Ok(b.token)
    }
}
