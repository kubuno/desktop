//! The worker end to end, with nothing real: a fake shell (a `BrokerBackend` behind a real `BrokerServer` on an
//! endpoint of the test's own) and a fake core (axum on 127.0.0.1), the whole profile under a temporary
//! `KUBUNO_SANDBOX_DIR`. Checks the snapshot of the current account (apps, logo downloaded and kept, favourites,
//! user, administrator, other accounts), the favourites' PATCH, an account switch through the broker (and its
//! `Switched` event), and the next start offline showing the copy kept on disk.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use kubuno_desktop_account::app::AppBroker;
use kubuno_desktop_account::broker::{BrokerClient, BrokerEndpoint, BrokerServer, ClientPolicy};
use kubuno_desktop_account::{AccountError, AccountEvent, AccountInfo, AccountKey, AccountStore, AccountSummary, Borrowed, BrokerBackend, SessionStatus};
use kubuno_desktop_api_client::{AccessToken, AuthError};
use kubuno_desktop_header_data::{FeedCommand, FeedConfig, HeaderFeed, HeaderSnapshot, Status};
use kubuno_desktop_secrets::Secret;
use serde_json::{json, Value};
use tokio::sync::broadcast;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot really a picture, but the cache only reads the signature";

/// The fake shell: two accounts, the first one current.
struct FakeShell {
    accounts: Vec<AccountSummary>,
    current: Mutex<Option<AccountKey>>,
    events: broadcast::Sender<AccountEvent>,
}

#[async_trait::async_trait]
impl BrokerBackend for FakeShell {
    async fn access_token(&self, key: &AccountKey) -> Result<Borrowed, AuthError> {
        let user = self.accounts.iter().find(|a| &a.info.key == key).map(|a| a.info.user_id.clone()).ok_or(AuthError::UnknownAccount)?;
        Ok(Borrowed { token: AccessToken::new(format!("tok-{user}")), valid_for: Duration::from_secs(600) })
    }
    async fn access_after_401(&self, key: &AccountKey, _failed: &str) -> Result<Borrowed, AuthError> {
        self.access_token(key).await
    }
    async fn accounts(&self) -> Vec<AccountSummary> {
        self.accounts.clone()
    }
    fn current(&self) -> Option<AccountKey> {
        self.current.lock().expect("lock").clone()
    }
    fn switch(&self, key: &AccountKey) -> Result<(), AccountError> {
        *self.current.lock().expect("lock") = Some(key.clone());
        let _ = self.events.send(AccountEvent::Switched { account: Some(key.clone()) });
        Ok(())
    }
    async fn database_key(&self, key: &AccountKey) -> Result<Secret, AccountError> {
        Err(AccountError::Unknown(key.clone()))
    }
    fn subscribe(&self) -> broadcast::Receiver<AccountEvent> {
        self.events.subscribe()
    }
}

/// What the fake core saw.
#[derive(Default)]
struct Core {
    patches: Mutex<Vec<Value>>,
}

fn user_of(headers: &HeaderMap) -> Option<String> {
    headers.get("authorization")?.to_str().ok()?.strip_prefix("Bearer tok-").map(str::to_string)
}

async fn modules(headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    user_of(&headers).ok_or(StatusCode::UNAUTHORIZED)?;
    Ok(Json(json!({ "modules": [
        { "module_id": "drive", "logo_url": "/drive-logo.png", "sidebar_items": [{ "id": "drive", "label": "Drive", "path": "/drive", "icon": "HardDrive" }] },
        { "module_id": "chat", "sidebar_items": [{ "id": "chat", "label": "Chat", "path": "/chat", "icon": "MessagesSquare" }] },
        { "module_id": "office", "sidebar_items": [{ "id": "office-documents", "label": "Documents", "path": "/office/documents", "icon": "FileText" }] }
    ]})))
}

async fn me(headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    let user = user_of(&headers).ok_or(StatusCode::UNAUTHORIZED)?;
    let name = if user == "camille" { "Camille Martin" } else { "Dominique Bernard" };
    Ok(Json(json!({
        "user": { "id": user, "display_name": name, "email": format!("{user}@exemple.fr"),
                  "preferences": { "waffle_favorites": ["drive", "mail-inbox"] } },
        "privileges": { "is_admin": user == "camille" }
    })))
}

async fn patch_me(State(core): State<Arc<Core>>, headers: HeaderMap, Json(body): Json<Value>) -> StatusCode {
    if user_of(&headers).is_none() {
        return StatusCode::UNAUTHORIZED;
    }
    core.patches.lock().expect("lock").push(body);
    StatusCode::NO_CONTENT
}

async fn logo(headers: HeaderMap) -> Result<Vec<u8>, StatusCode> {
    user_of(&headers).ok_or(StatusCode::UNAUTHORIZED)?;
    Ok(PNG.to_vec())
}

fn next(rx: &mpsc::Receiver<HeaderSnapshot>, want: impl Fn(&HeaderSnapshot) -> bool) -> HeaderSnapshot {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let s = rx.recv_timeout(left).expect("a snapshot in time");
        if want(&s) {
            return s;
        }
    }
}

#[test]
fn the_feed_follows_the_shell_and_the_server() {
    let sandbox = tempfile::tempdir().expect("tmp");
    // SAFETY (test process): set before any thread reads the environment; the only test of this binary.
    std::env::set_var("KUBUNO_SANDBOX_DIR", sandbox.path());

    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("runtime");
    let core = Arc::new(Core::default());
    let base = rt.block_on(async {
        let app = Router::new()
            .route("/api/v1/modules", get(modules))
            .route("/api/v1/me", get(me).patch(patch_me))
            .route("/drive-logo.png", get(logo))
            .with_state(core.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
        format!("http://{addr}")
    });

    let camille = AccountInfo { display_name: Some("Camille".into()), email: Some("camille@exemple.fr".into()), ..AccountInfo::new(&base, "camille").expect("info") };
    let dominique = AccountInfo { display_name: Some("Dominique".into()), ..AccountInfo::new(&base, "dominique").expect("info") };
    // The shell's account store (what `account.json` says offline).
    let store = AccountStore::new(kubuno_desktop_account::paths::user_data_dir().expect("data dir"));
    store.save(&camille).expect("save");
    store.save(&dominique).expect("save");
    let (events, _) = broadcast::channel(16);
    let shell = Arc::new(FakeShell {
        accounts: vec![
            AccountSummary { info: camille.clone(), status: SessionStatus::Active },
            AccountSummary { info: dominique.clone(), status: SessionStatus::Active },
        ],
        current: Mutex::new(Some(camille.key.clone())),
        events,
    });
    let endpoint = BrokerEndpoint::for_test("header-feed", sandbox.path());
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = BrokerServer::new(endpoint.clone(), shell.clone(), ClientPolicy::SameUser);
    rt.spawn(server.serve(async move {
        let _ = stop_rx.await;
    }));
    std::thread::sleep(Duration::from_millis(200));

    let (tx, rx) = mpsc::channel();
    let broker = AppBroker::new(BrokerClient::new(endpoint.clone(), "header-test"), None);
    let feed = HeaderFeed::start_with(FeedConfig::new("header-test"), Some(broker), move |s| {
        let _ = tx.send(s);
    });

    // The current account, from the server.
    let s = next(&rx, |s| s.status == Status::Online);
    assert_eq!(s.account.as_deref(), Some(camille.key.as_str()));
    assert_eq!(s.apps.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["drive", "chat", "office-documents"]);
    let logo = s.apps[0].logo_path.clone().expect("drive's logo downloaded");
    assert_eq!(std::fs::read(&logo).expect("logo file"), PNG);
    assert!(logo.starts_with(store.account_dir(&camille.key)), "kept in the account's cache: {}", logo.display());
    assert_eq!(s.favorites, ["drive", "mail-inbox"], "an id this instance does not have is carried");
    assert_eq!((s.user.name.as_str(), s.user.initials.as_str()), ("Camille Martin", "CM"));
    assert!(s.is_admin);
    assert_eq!(s.others.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), [dominique.key.as_str()]);

    // The favourites go back to the server.
    feed.send(FeedCommand::SaveFavorites(vec!["chat".into(), "drive".into()]));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while core.patches.lock().expect("lock").is_empty() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(core.patches.lock().expect("lock").first(), Some(&json!({ "preferences": { "waffle_favorites": ["chat", "drive"] } })));

    // Another account: the shell switches, and the header follows.
    feed.send(FeedCommand::SwitchAccount(dominique.key.as_str().to_string()));
    let s = next(&rx, |s| s.account.as_deref() == Some(dominique.key.as_str()));
    assert_eq!(shell.current(), Some(dominique.key.clone()));
    assert_eq!(s.user.name, "Dominique Bernard");
    assert!(!s.is_admin);

    // The shell's broker goes away; the next start shows the copy kept on disk first.
    drop(feed);
    let _ = stop_tx.send(());
    std::thread::sleep(Duration::from_millis(300));
    let (tx2, rx2) = mpsc::channel();
    let dead = AppBroker::new(BrokerClient::new(BrokerEndpoint::for_test("header-feed-gone", sandbox.path()), "header-test"), None);
    let _feed = HeaderFeed::start_with(FeedConfig::new("header-test"), Some(dead), move |s| {
        let _ = tx2.send(s);
    });
    let s = rx2.recv_timeout(Duration::from_secs(10)).expect("the cached snapshot");
    assert_eq!(s.status, Status::Cached);
    assert_eq!(s.account.as_deref(), Some(dominique.key.as_str()), "the account the last run showed");
    assert_eq!(s.user.name, "Dominique Bernard");
    assert_eq!(s.apps.len(), 3);
    assert!(s.apps[0].logo_path.as_ref().is_some_and(|p| p.is_file()), "the kept logo");
    drop(rt);
}
