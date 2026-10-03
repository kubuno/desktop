//! The worker behind the header's data: a thread of its own (a single-threaded Tokio runtime) that borrows
//! access tokens from the shell's broker, asks the server, keeps the answers on disk and hands each
//! [`HeaderSnapshot`] to a sink (the app posts it to its UI thread).
//!
//! 1. At start, the copy kept on disk for the account the last run showed (`Status::Cached`): the header is
//!    filled before the broker or the server answer (offline-first).
//! 2. Then the broker's accounts ([`AppBroker::accounts`]: it starts the shell in the background when it is
//!    not running) and the current one ([`kubuno_desktop_account::app::pick_current`], the rule every app follows);
//!    `GET /api/v1/modules` and `GET /api/v1/me` as that account, through `kubuno-desktop-api-client` with an
//!    [`AppTokenSource`] (the app never holds a refresh token). Each answer is kept under
//!    `<data>/accounts/<key>/blobs/header/` (`modules.json`, `me.json`, the pictures and their index); when
//!    the server cannot be reached the kept copies are shown instead (`Status::Offline` / `Status::Expired`).
//! 3. Then it waits: a command of the UI ([`FeedCommand`]: refresh, save the favourites, switch account), an
//!    event of the broker (account switched, added, removed, session expired or restored: the shell's
//!    subscription, `BrokerClient::subscribe`) or the periodic refresh.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use kubuno_desktop_account::app::{pick_current, AppBroker, AppTokenSource};
use kubuno_desktop_account::broker::{BrokerClient, BrokerError};
use kubuno_desktop_account::{AccountInfo, AccountKey, AccountStore, AccountSummary, SessionStatus};
use kubuno_desktop_api_client::{ApiClient, ApiError, ApiRequest, AuthError, RetryPolicy};
use tokio::sync::mpsc;

use crate::logos::{self, PictureCache};
use crate::snapshot::{self, HeaderSnapshot, Status};

/// What the UI asks the worker for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedCommand {
    /// Ask the broker and the server again now.
    Refresh,
    /// `preferences.waffle_favorites` := this list (`PATCH /api/v1/me`), as the shell and the web save it.
    SaveFavorites(Vec<String>),
    /// Make account `key` the shell's current account (the broker's `switch_account`); every app follows.
    SwitchAccount(String),
}

/// How the worker runs.
#[derive(Debug, Clone)]
pub struct FeedConfig {
    /// The program, as the broker's logs name it (`kubuno-documents`, `kubuno-chat`).
    pub app: String,
    /// The HTTP proxy the server is reached through (the shell's setting), `None` for a direct connection.
    pub proxy: Option<String>,
    /// How often the data are fetched again without an event (15 minutes).
    pub refresh_every: Duration,
}

impl FeedConfig {
    pub fn new(app: impl Into<String>) -> Self {
        Self { app: app.into(), proxy: None, refresh_every: Duration::from_secs(15 * 60) }
    }
}

/// The worker's handle: commands go through it; dropping it ends the worker.
#[derive(Debug, Clone, Default)]
pub struct HeaderFeed {
    tx: Option<mpsc::UnboundedSender<FeedCommand>>,
}

impl HeaderFeed {
    /// A feed that asks nobody (the offline sample): commands are dropped.
    pub fn none() -> Self {
        Self { tx: None }
    }

    /// Starts the worker thread (see the module doc) with the current user's broker, served by the shell
    /// installed next to this program; `sink` hears every snapshot, on that thread.
    pub fn start(config: FeedConfig, sink: impl Fn(HeaderSnapshot) + Send + 'static) -> Self {
        Self::start_with(config, None, sink)
    }

    /// [`Self::start`] with an explicit broker (tests: a fake shell on an endpoint of their own).
    pub fn start_with(config: FeedConfig, broker: Option<AppBroker>, sink: impl Fn(HeaderSnapshot) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let spawned = std::thread::Builder::new().name("kubuno-header".into()).spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("[header] no runtime for the header's data: {e}");
                    return;
                }
            };
            runtime.block_on(run(config, broker, rx, Box::new(sink)));
        });
        if let Err(e) = spawned {
            tracing::error!("[header] the header's worker could not start: {e}");
        }
        Self { tx: Some(tx) }
    }

    /// Hands `command` to the worker (nothing for the sample, or once the worker ended).
    pub fn send(&self, command: FeedCommand) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(command);
        }
    }
}

type Sink = Box<dyn Fn(HeaderSnapshot) + Send>;

// ── Where things are kept ────────────────────────────────────────────────────────────────────────

/// `<data>/accounts`, the shell's account store (read only here: `account.json` names an account offline).
fn account_store() -> Option<AccountStore> {
    Some(AccountStore::new(kubuno_desktop_account::paths::user_data_dir().ok()?))
}

/// The header's cache of account `key`: `<data>/accounts/<key>/blobs/header/` (vskubuno
/// `docs/DESKTOP-OFFLINE-SYNC.md` §6.1, the account's content caches), shared by every app of the account.
pub fn cache_of(key: &AccountKey) -> Option<PictureCache> {
    Some(PictureCache::new(account_store()?.account_dir(key).join("blobs").join("header")))
}

/// The account the last run showed (`<cache>/header/last-account`): what is shown before the broker answers.
fn last_account_file() -> Option<PathBuf> {
    Some(kubuno_desktop_account::paths::user_cache_dir().ok()?.join("header").join("last-account"))
}

fn read_last_account() -> Option<AccountKey> {
    let text = std::fs::read_to_string(last_account_file()?).ok()?;
    AccountKey::parse(text.trim()).ok()
}

fn write_last_account(key: Option<&AccountKey>) {
    let Some(file) = last_account_file() else { return };
    match key {
        Some(k) => {
            if read_last_account().as_ref() != Some(k) {
                let _ = logos::write_atomic(&file, k.as_str().as_bytes());
            }
        }
        None => {
            let _ = std::fs::remove_file(file);
        }
    }
}

/// The snapshot kept on disk for account `key` (its `account.json`, `modules.json`, `me.json` and pictures),
/// with `status`; `None` when nothing was ever kept.
fn from_disk(key: &AccountKey, status: Status) -> Option<HeaderSnapshot> {
    let store = account_store()?;
    let info = store.load(key).ok()??;
    let cache = cache_of(key)?;
    let (modules, me) = (cache.load_json("modules.json"), cache.load_json("me.json"));
    if modules.is_none() && me.is_none() {
        return None;
    }
    let current = AccountSummary { info, status: SessionStatus::Active };
    // The other accounts as the store lists them; their session states arrive with the broker's answer.
    let others: Vec<AccountSummary> = store.list().unwrap_or_default().into_iter().map(|info| AccountSummary { info, status: SessionStatus::Active }).collect();
    let mut s = snapshot::build(status, &current, &others, modules.as_ref(), me.as_ref());
    resolve_pictures_offline(&cache, &mut s, me.as_ref());
    Some(s)
}

// ── The worker ───────────────────────────────────────────────────────────────────────────────────

async fn run(config: FeedConfig, broker: Option<AppBroker>, mut rx: mpsc::UnboundedReceiver<FeedCommand>, sink: Sink) {
    let mut shown = false;
    if let Some(s) = read_last_account().and_then(|k| from_disk(&k, Status::Cached)) {
        sink(s);
        shown = true;
    }
    let broker = match broker.map_or_else(|| AppBroker::for_app(&config.app), Ok) {
        Ok(b) => Arc::new(b),
        Err(e) => {
            tracing::warn!("[header] no token broker: {e}");
            if !shown {
                sink(HeaderSnapshot::empty(Status::NoShell));
            }
            // Nothing to ask: wait for the window to close.
            while rx.recv().await.is_some() {}
            return;
        }
    };
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    tokio::spawn(watch_events(broker.client().clone(), ev_tx));
    let mut worker = Worker { config, broker, sink, current: None, shown };
    worker.refresh().await;
    loop {
        tokio::select! {
            command = rx.recv() => match command {
                None => break,
                Some(FeedCommand::Refresh) => worker.refresh().await,
                Some(FeedCommand::SaveFavorites(list)) => worker.save_favorites(list).await,
                Some(FeedCommand::SwitchAccount(id)) => worker.switch(&id).await,
            },
            Some(()) = ev_rx.recv() => {
                // Several events in a row (a switch, then its session check) make one refresh.
                while ev_rx.try_recv().is_ok() {}
                worker.refresh().await;
            }
            () = tokio::time::sleep(worker.config.refresh_every) => worker.refresh().await,
        }
    }
}

/// Follows the broker's events (in a task of its own, so that a half-read line is never lost to the
/// worker's `select!`); reconnects every 30 s while the broker is away.
async fn watch_events(client: BrokerClient, tx: mpsc::UnboundedSender<()>) {
    loop {
        if let Ok(mut events) = client.subscribe().await {
            while let Some(event) = events.next().await {
                tracing::debug!("[header] broker event {event:?}");
                if tx.send(()).is_err() {
                    return;
                }
            }
        }
        if tx.is_closed() {
            return;
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

struct Worker {
    config: FeedConfig,
    broker: Arc<AppBroker>,
    sink: Sink,
    /// The account shown and its client.
    current: Option<(AccountInfo, ApiClient)>,
    /// A snapshot was handed out already.
    shown: bool,
}

impl Worker {
    fn emit(&mut self, s: HeaderSnapshot) {
        self.shown = true;
        (self.sink)(s);
    }

    /// A client of `info`'s server with `info`'s tokens (kept while the account stays the same).
    fn client_for(&mut self, info: &AccountInfo) -> Option<ApiClient> {
        if let Some((held, client)) = &self.current {
            if held.key == info.key && held.server_url == info.server_url {
                return Some(client.clone());
            }
        }
        let tokens = Arc::new(AppTokenSource::new(self.broker.clone(), info.key.clone()));
        let client = ApiClient::builder(info.server_url.clone())
            .tokens(tokens)
            .proxy(self.config.proxy.clone())
            // The header is not worth a long wait: what is kept on disk shows meanwhile.
            .retry(RetryPolicy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build();
        match client {
            Ok(c) => {
                self.current = Some((info.clone(), c.clone()));
                Some(c)
            }
            Err(e) => {
                tracing::warn!("[header] no client for {}: {e}", info.server_url);
                None
            }
        }
    }

    async fn refresh(&mut self) {
        let (accounts, current_key) = match self.broker.accounts().await {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!("[header] the shell's broker did not answer: {e}");
                if !self.shown {
                    let status = if matches!(e, BrokerError::Unreachable(_) | BrokerError::ServerRefused(_)) { Status::NoShell } else { Status::Offline };
                    self.emit(HeaderSnapshot::empty(status));
                }
                return;
            }
        };
        let Some(current) = pick_current(accounts.clone(), current_key.as_ref()) else {
            write_last_account(None);
            self.current = None;
            self.emit(HeaderSnapshot::empty(Status::SignedOut));
            return;
        };
        write_last_account(Some(&current.info.key));
        let cache = cache_of(&current.info.key);
        let Some(client) = self.client_for(&current.info) else { return };

        let modules = client.send(ApiRequest::get("/api/v1/modules")).await.map(|r| r.json_value());
        let me = client.send(ApiRequest::get("/api/v1/me")).await.map(|r| r.json_value());
        let status = match (&modules, &me) {
            (Ok(_), Ok(_)) => Status::Online,
            (Err(e), _) | (_, Err(e)) if is_expired(e) || current.status == SessionStatus::SessionExpired => Status::Expired,
            (Err(e), _) | (_, Err(e)) => {
                tracing::info!("[header] {} unreachable, showing the copy kept on disk: {e}", current.info.server_url);
                Status::Offline
            }
        };
        let kept = |name: &str, answer: Result<serde_json::Value, ApiError>| match answer {
            Ok(v) => {
                if let Some(c) = &cache {
                    c.save_json(name, &v);
                }
                Some(v)
            }
            Err(_) => cache.as_ref().and_then(|c| c.load_json(name)),
        };
        let (modules, me) = (kept("modules.json", modules), kept("me.json", me));
        let mut s = snapshot::build(status, &current, &accounts, modules.as_ref(), me.as_ref());
        if let Some(cache) = &cache {
            if status == Status::Online {
                resolve_pictures_online(&client, cache, &mut s, me.as_ref()).await;
            } else {
                resolve_pictures_offline(cache, &mut s, me.as_ref());
            }
        }
        self.emit(s);
    }

    async fn save_favorites(&mut self, list: Vec<String>) {
        let Some((info, client)) = self.current.clone() else {
            tracing::warn!("[header] favourites not saved: no account");
            return;
        };
        let body = serde_json::json!({ "preferences": { "waffle_favorites": list } });
        match client.send(ApiRequest::patch("/api/v1/me").json(body)).await {
            Ok(_) => {
                // The copy kept on disk follows, so a start offline shows the list just saved.
                if let Some(cache) = cache_of(&info.key) {
                    if let Some(mut me) = cache.load_json("me.json") {
                        let user = if me.get("user").is_some_and(|u| u.is_object()) { me.get_mut("user") } else { Some(&mut me) };
                        if let Some(prefs) = user.and_then(|u| u.as_object_mut()).map(|u| u.entry("preferences").or_insert_with(|| serde_json::json!({}))) {
                            if let Some(p) = prefs.as_object_mut() {
                                p.insert("waffle_favorites".into(), serde_json::json!(list));
                            }
                        }
                        cache.save_json("me.json", &me);
                    }
                }
            }
            Err(e) => tracing::warn!("[header] favourites not saved: {e}"),
        }
    }

    async fn switch(&mut self, id: &str) {
        let Ok(key) = AccountKey::parse(id) else {
            tracing::warn!("[header] « {id} » is not an account of the shell");
            return;
        };
        match self.broker.client().switch_account(&key).await {
            // The broker's `Switched` event refreshes too; this one does not wait for it.
            Ok(()) => self.refresh().await,
            Err(e) => tracing::warn!("[header] the shell did not switch accounts: {e}"),
        }
    }
}

fn is_expired(e: &ApiError) -> bool {
    matches!(e, ApiError::Auth(AuthError::SessionExpired) | ApiError::Auth(AuthError::UnknownAccount)) || e.status() == Some(401)
}

// ── Pictures ─────────────────────────────────────────────────────────────────────────────────────

/// A picture of the instance (`/drive-logo.png`, `/api/v1/users/1/avatar`), downloaded as the account. A
/// foreign URL is never fetched: only the instance's own channel is trusted with its token.
async fn download(client: &ApiClient, url: &str) -> Option<Vec<u8>> {
    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("//") {
        return None;
    }
    let path = if url.starts_with('/') { url.to_string() } else { format!("/{url}") };
    match client.send(ApiRequest::get(path.clone())).await {
        Ok(r) => Some(r.body),
        Err(e) => {
            tracing::debug!("[header] {path}: {e}");
            None
        }
    }
}

/// Each app's logo and the user's photo, downloaded (the server is the source of truth), else the copies
/// kept, else the web's files embedded at build time.
async fn resolve_pictures_online(client: &ApiClient, cache: &PictureCache, s: &mut HeaderSnapshot, me: Option<&serde_json::Value>) {
    for app in &mut s.apps {
        let mut path = None;
        if let Some(url) = app.logo_url.clone() {
            if let Some(bytes) = download(client, &url).await {
                path = cache.store(&app.id, &url, &bytes);
            }
        }
        app.logo_path = path.or_else(|| fallback_logo(cache, &app.id, app.logo_url.as_deref()));
    }
    let avatar_url = me.map(snapshot::me_user).and_then(snapshot::me_avatar_url);
    s.user.avatar = match avatar_url {
        Some(url) => {
            let fresh = match download(client, &url).await {
                Some(bytes) => cache.store("avatar", &url, &bytes),
                None => None,
            };
            fresh.or_else(|| cache.lookup("avatar", &url)).map(|p| p.to_string_lossy().into_owned())
        }
        None => {
            cache.forget("avatar");
            None
        }
    };
}

/// The same pictures without the network: the copies kept, else the embedded web files.
fn resolve_pictures_offline(cache: &PictureCache, s: &mut HeaderSnapshot, me: Option<&serde_json::Value>) {
    for app in &mut s.apps {
        app.logo_path = fallback_logo(cache, &app.id, app.logo_url.as_deref());
    }
    s.user.avatar = me
        .map(snapshot::me_user)
        .and_then(snapshot::me_avatar_url)
        .and_then(|url| cache.lookup("avatar", &url))
        .map(|p| p.to_string_lossy().into_owned());
}

fn fallback_logo(cache: &PictureCache, id: &str, url: Option<&str>) -> Option<PathBuf> {
    let builtin = logos::default_builtin_dir();
    url.and_then(|u| cache.lookup(id, u))
        .or_else(|| url.zip(builtin.as_deref()).and_then(|(u, dir)| logos::builtin_for_url(dir, u)))
        .or_else(|| builtin.as_deref().and_then(|dir| logos::builtin_for_id(dir, id)))
        .filter(|p| logos::tile_grid_draws(p))
}
