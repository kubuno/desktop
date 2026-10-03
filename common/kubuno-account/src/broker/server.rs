//! The broker server, run by the shell next to its [`TokenOwner`](crate::TokenOwner).

use std::future::Future;
use std::io;
use std::sync::Arc;

use kubuno_api_client::AuthError;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use super::proto::{encode, ErrorCode, Request, Response, MAX_LINE, PROTOCOL_VERSION};
use super::transport::{BrokerEndpoint, ClientPolicy};
use crate::owner::BrokerBackend;

/// The broker server. `serve` runs until `shutdown` completes.
pub struct BrokerServer {
    endpoint: BrokerEndpoint,
    backend: Arc<dyn BrokerBackend>,
    policy: ClientPolicy,
}

fn auth_error(e: &AuthError) -> Response {
    let code = match e {
        AuthError::SessionExpired => ErrorCode::SessionExpired,
        AuthError::Transient(_) => ErrorCode::Transient,
        AuthError::UnknownAccount => ErrorCode::UnknownAccount,
    };
    Response::Error { code, message: e.to_string() }
}

async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R, buf: &mut Vec<u8>) -> io::Result<Option<()>> {
    buf.clear();
    let n = (&mut *reader).take(MAX_LINE as u64 + 1).read_until(b'\n', buf).await?;
    if n == 0 {
        return Ok(None);
    }
    if buf.len() > MAX_LINE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "broker frame too long"));
    }
    Ok(Some(()))
}

async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, resp: &Response) -> io::Result<()> {
    let bytes = encode(resp).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    w.write_all(&bytes).await?;
    w.flush().await
}

/// Serves one connection (generic over the stream so tests can use an in-memory duplex).
pub(crate) async fn handle_connection<S>(stream: S, backend: Arc<dyn BrokerBackend>) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (r, mut w) = tokio::io::split(stream);
    let mut reader = BufReader::new(r);
    let mut buf = Vec::new();
    let mut greeted = false;
    while read_frame(&mut reader, &mut buf).await?.is_some() {
        let req: Request = match serde_json::from_slice(&buf) {
            Ok(r) => r,
            Err(e) => {
                write_frame(&mut w, &Response::Error { code: ErrorCode::BadRequest, message: format!("bad request: {e}") }).await?;
                continue;
            }
        };
        if !greeted {
            match req {
                Request::Hello { version, app } if version == PROTOCOL_VERSION => {
                    tracing::debug!(app = %app, "broker client connected");
                    greeted = true;
                    write_frame(&mut w, &Response::Hello { version: PROTOCOL_VERSION }).await?;
                }
                Request::Hello { version, .. } => {
                    write_frame(&mut w, &Response::Error { code: ErrorCode::Version, message: format!("protocol {version} not supported, use {PROTOCOL_VERSION}") }).await?;
                    return Ok(());
                }
                _ => {
                    write_frame(&mut w, &Response::Error { code: ErrorCode::BadRequest, message: "hello expected first".into() }).await?;
                    return Ok(());
                }
            }
            continue;
        }
        let resp = match req {
            Request::Hello { .. } => Response::Error { code: ErrorCode::BadRequest, message: "already greeted".into() },
            Request::AccessToken { account } => match backend.access_token(&account).await {
                Ok(b) => Response::Token { token: b.token.expose().to_string(), valid_for_s: b.valid_for.as_secs() },
                Err(e) => auth_error(&e),
            },
            Request::AccessAfter401 { account, failed } => match backend.access_after_401(&account, &failed).await {
                Ok(b) => Response::Token { token: b.token.expose().to_string(), valid_for_s: b.valid_for.as_secs() },
                Err(e) => auth_error(&e),
            },
            Request::Accounts => Response::Accounts { accounts: backend.accounts().await, current: backend.current() },
            Request::SwitchAccount { account } => match backend.switch(&account) {
                Ok(()) => Response::Ok,
                Err(_) => Response::Error { code: ErrorCode::UnknownAccount, message: "unknown account".into() },
            },
            Request::DatabaseKey { account } => match backend.database_key(&account).await {
                Ok(secret) => match secret.expose_str() {
                    Ok(v) => Response::Secret { value: v.to_string() },
                    Err(_) => Response::Error { code: ErrorCode::Transient, message: "the database key is corrupted".into() },
                },
                Err(e) => Response::Error { code: ErrorCode::Transient, message: e.to_string() },
            },
            Request::Subscribe => {
                let mut rx = backend.subscribe();
                write_frame(&mut w, &Response::Ok).await?;
                loop {
                    match rx.recv().await {
                        Ok(event) => write_frame(&mut w, &Response::Event { event }).await?,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(missed = n, "broker subscriber lagged");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
                    }
                }
            }
        };
        write_frame(&mut w, &resp).await?;
    }
    Ok(())
}

impl BrokerServer {
    pub fn new(endpoint: BrokerEndpoint, backend: Arc<dyn BrokerBackend>, policy: ClientPolicy) -> Self {
        Self { endpoint, backend, policy }
    }

    pub fn endpoint(&self) -> &BrokerEndpoint {
        &self.endpoint
    }

    /// Listens until `shutdown` completes. Fails at once if the endpoint is taken (another shell is running).
    pub async fn serve(self, shutdown: impl Future<Output = ()>) -> io::Result<()> {
        self.bind().await?.serve(shutdown).await
    }

    /// Takes the endpoint now (so the caller learns at once whether another shell owns it) without serving yet.
    pub async fn bind(self) -> io::Result<BoundBroker> {
        match self.endpoint.clone() {
            #[cfg(windows)]
            BrokerEndpoint::Pipe(name) => {
                use super::transport::sys;
                let security = sys::UserOnlySecurity::new()?;
                let first = create_pipe(&name, &security, true)?;
                Ok(BoundBroker { backend: self.backend, policy: self.policy, name, security, first })
            }
            #[cfg(unix)]
            BrokerEndpoint::Socket(path) => {
                use std::os::unix::fs::PermissionsExt;
                use tokio::net::{UnixListener, UnixStream};

                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
                }
                if path.exists() {
                    // A live server answers: refuse to steal its socket. A dead one left a stale file: replace it.
                    if UnixStream::connect(&path).await.is_ok() {
                        return Err(io::Error::new(io::ErrorKind::AddrInUse, "another broker is already listening"));
                    }
                    std::fs::remove_file(&path)?;
                }
                let listener = UnixListener::bind(&path)?;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
                Ok(BoundBroker { backend: self.backend, policy: self.policy, path, listener })
            }
            #[allow(unreachable_patterns)]
            other => Err(io::Error::new(io::ErrorKind::Unsupported, format!("endpoint {other:?} is not available on this OS"))),
        }
    }
}

#[cfg(windows)]
fn create_pipe(
    name: &str,
    security: &super::transport::sys::UserOnlySecurity,
    first: bool,
) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    let mut opts = tokio::net::windows::named_pipe::ServerOptions::new();
    opts.first_pipe_instance(first).reject_remote_clients(true);
    // SAFETY: `security.as_ptr()` points to a valid SECURITY_ATTRIBUTES whose descriptor lives as long as
    // `security`, which the caller keeps alive for every pipe instance it creates (the call copies what it needs).
    unsafe { opts.create_with_security_attributes_raw(name, security.as_ptr()) }
}

/// A broker that owns its endpoint ([`BrokerServer::bind`]) and serves it with [`BoundBroker::serve`].
pub struct BoundBroker {
    backend: Arc<dyn BrokerBackend>,
    policy: ClientPolicy,
    #[cfg(windows)]
    name: String,
    #[cfg(windows)]
    security: super::transport::sys::UserOnlySecurity,
    #[cfg(windows)]
    first: tokio::net::windows::named_pipe::NamedPipeServer,
    #[cfg(unix)]
    path: std::path::PathBuf,
    #[cfg(unix)]
    listener: tokio::net::UnixListener,
}

impl BoundBroker {
    /// Serves until `shutdown` completes.
    #[cfg(windows)]
    pub async fn serve(self, shutdown: impl Future<Output = ()>) -> io::Result<()> {
        use std::os::windows::io::AsRawHandle;

        use super::transport::sys;

        let BoundBroker { backend, policy, name, security, first } = self;
        let mut server = first;
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                r = server.connect() => r?,
                _ = &mut shutdown => return Ok(()),
            }
            let connected = server;
            server = create_pipe(&name, &security, false)?;
            let pid = sys::pipe_client_pid(connected.as_raw_handle() as _);
            let allowed = match &pid {
                Ok(pid) => policy.allows_image(sys::process_image(*pid).as_deref()),
                Err(_) => matches!(policy, ClientPolicy::SameUser),
            };
            if !allowed {
                tracing::warn!(pid = ?pid.ok(), "broker: client refused by policy");
                drop(connected);
                continue;
            }
            let backend = backend.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_connection(connected, backend).await {
                    tracing::debug!(error = %e, "broker connection ended");
                }
            });
        }
    }

    /// Serves until `shutdown` completes, then removes the socket file.
    #[cfg(unix)]
    pub async fn serve(self, shutdown: impl Future<Output = ()>) -> io::Result<()> {
        use super::transport::sys;

        let BoundBroker { backend, policy, path, listener } = self;
        let me = sys::effective_uid();
        tokio::pin!(shutdown);
        let result = loop {
            let (stream, _) = tokio::select! {
                r = listener.accept() => match r { Ok(v) => v, Err(e) => break Err(e) },
                _ = &mut shutdown => break Ok(()),
            };
            let cred = match stream.peer_cred() {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(error = %e, "broker: cannot read the peer credentials");
                    continue;
                }
            };
            if cred.uid() != me {
                tracing::warn!(uid = cred.uid(), "broker: client of another user refused");
                continue;
            }
            let image = cred.pid().and_then(sys::process_image);
            if !policy.allows_image(image.as_deref()) {
                tracing::warn!(pid = ?cred.pid(), "broker: client refused by policy");
                continue;
            }
            let backend = backend.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, backend).await {
                    tracing::debug!(error = %e, "broker connection ended");
                }
            });
        };
        let _ = std::fs::remove_file(&path);
        result
    }
}
