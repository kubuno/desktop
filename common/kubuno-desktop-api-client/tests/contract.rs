//! Contract tests of the client against an in-process fake server: headers, retries, refresh on 401, error
//! mapping, KDP paging.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use kubuno_desktop_api_client::{
    AccessToken, ApiClient, ApiError, ApiRequest, AuthError, ChangeKind, Cursor, ErrorClass, RetryPolicy, StaticToken, TokenSource,
};

#[derive(Default)]
struct Fake {
    seen_headers: Mutex<Vec<HeaderMap>>,
    flaky_left: AtomicU32,
    flaky_calls: AtomicU32,
    valid_token: Mutex<String>,
}

async fn echo(State(s): State<Arc<Fake>>, h: HeaderMap) -> Json<serde_json::Value> {
    s.seen_headers.lock().expect("lock").push(h);
    Json(serde_json::json!({"ok": true}))
}

async fn flaky(State(s): State<Arc<Fake>>, h: HeaderMap) -> Response {
    s.flaky_calls.fetch_add(1, Ordering::SeqCst);
    s.seen_headers.lock().expect("lock").push(h);
    if s.flaky_left.load(Ordering::SeqCst) > 0 {
        s.flaky_left.fetch_sub(1, Ordering::SeqCst);
        return (StatusCode::SERVICE_UNAVAILABLE, [("retry-after", "0")], Json(serde_json::json!({"error": "UNAVAILABLE", "message": "try later"}))).into_response();
    }
    Json(serde_json::json!({"done": true})).into_response()
}

async fn guarded(State(s): State<Arc<Fake>>, h: HeaderMap) -> Response {
    let valid = s.valid_token.lock().expect("lock").clone();
    let auth = h.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    if auth == format!("Bearer {valid}") {
        Json(serde_json::json!({"ok": true})).into_response()
    } else {
        (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "UNAUTHORIZED", "message": "token expired"}))).into_response()
    }
}

async fn precondition(h: HeaderMap) -> Response {
    match h.get("if-match").and_then(|v| v.to_str().ok()) {
        Some("e2") => (StatusCode::OK, [("etag", "\"e3\"")], Json(serde_json::json!({"id": "n1", "etag": "e3"}))).into_response(),
        _ => (StatusCode::PRECONDITION_FAILED, Json(serde_json::json!({"error": "PRECONDITION_FAILED", "message": "stale", "current": {"id": "n1", "etag": "e2", "title": "server"}}))).into_response(),
    }
}

async fn status_of(Query(q): Query<std::collections::HashMap<String, String>>) -> Response {
    let code: u16 = q.get("code").and_then(|c| c.parse().ok()).unwrap_or(500);
    let err = q.get("error").cloned().unwrap_or_else(|| "ERR".into());
    (StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), Json(serde_json::json!({"error": err, "message": "nope"}))).into_response()
}

/// A KDP feed of 5 changes, served `limit` at a time.
async fn delta(Query(q): Query<std::collections::HashMap<String, String>>) -> Json<serde_json::Value> {
    let cursor: i64 = q.get("cursor").and_then(|c| c.parse().ok()).unwrap_or(0);
    let limit: i64 = q.get("limit").and_then(|c| c.parse().ok()).unwrap_or(2);
    let all: Vec<serde_json::Value> = (1..=5)
        .map(|seq| {
            if seq == 4 {
                serde_json::json!({"uuid": "n2", "kind": "deleted", "change_seq": seq})
            } else {
                serde_json::json!({"uuid": format!("n{seq}"), "kind": "modified", "change_seq": seq, "note": {"title": format!("t{seq}"), "etag": format!("e{seq}")}})
            }
        })
        .collect();
    let page: Vec<_> = all.iter().filter(|c| c["change_seq"].as_i64().unwrap_or(0) > cursor).take(limit as usize).cloned().collect();
    let last = page.last().and_then(|c| c["change_seq"].as_i64()).unwrap_or(cursor);
    Json(serde_json::json!({"changes": page, "cursor": last, "has_more": last < 5}))
}

async fn stuck() -> Json<serde_json::Value> {
    Json(serde_json::json!({"changes": [], "cursor": 0, "has_more": true}))
}

async fn start() -> (String, Arc<Fake>) {
    let fake = Arc::new(Fake::default());
    *fake.valid_token.lock().expect("lock") = "t-2".into();
    let app = Router::new()
        .route("/echo", post(echo).get(echo))
        .route("/flaky", post(flaky).get(flaky))
        .route("/guarded", get(guarded))
        .route("/notes/n1", patch(precondition))
        .route("/status", get(status_of))
        .route("/notes/delta", get(delta))
        .route("/stuck/delta", get(stuck))
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), fake)
}

fn fast_retry() -> RetryPolicy {
    RetryPolicy { max_attempts: 4, base_delay: Duration::from_millis(10), max_delay: Duration::from_millis(50), jitter: 0.2 }
}

fn client(base: &str, token: &str) -> ApiClient {
    ApiClient::builder(base)
        .tokens(Arc::new(StaticToken(AccessToken::new(token))))
        .retry(fast_retry())
        .device_key("dev-123")
        .build()
        .expect("client")
}

#[tokio::test]
async fn sends_auth_and_per_request_headers() {
    let (base, fake) = start().await;
    let api = client(&base, "tok-1");
    api.send(ApiRequest::post("/echo").json(serde_json::json!({"a": 1})).if_match("e7").idempotency_key("k-1")).await.expect("ok");
    let h = fake.seen_headers.lock().expect("lock")[0].clone();
    assert_eq!(h.get("authorization").and_then(|v| v.to_str().ok()), Some("Bearer tok-1"));
    assert_eq!(h.get("if-match").and_then(|v| v.to_str().ok()), Some("e7"));
    assert_eq!(h.get("idempotency-key").and_then(|v| v.to_str().ok()), Some("k-1"));
    assert_eq!(h.get("x-kubuno-device-key").and_then(|v| v.to_str().ok()), Some("dev-123"));
    assert!(h.get("user-agent").and_then(|v| v.to_str().ok()).is_some_and(|ua| ua.starts_with("Kubuno-Desktop/")));
}

#[tokio::test]
async fn retries_transient_failures_with_the_same_idempotency_key() {
    let (base, fake) = start().await;
    let api = client(&base, "tok-1");
    fake.flaky_left.store(2, Ordering::SeqCst);
    let resp = api.send(ApiRequest::post("/flaky").json(serde_json::json!({})).idempotency_key("k-42")).await.expect("eventually ok");
    assert_eq!(resp.json_value()["done"], true);
    assert_eq!(fake.flaky_calls.load(Ordering::SeqCst), 3);
    let keys: Vec<String> = fake.seen_headers.lock().expect("lock").iter().filter_map(|h| h.get("idempotency-key").and_then(|v| v.to_str().ok()).map(str::to_string)).collect();
    assert_eq!(keys, vec!["k-42"; 3]);
}

#[tokio::test]
async fn does_not_retry_a_post_without_key_and_reports_transient() {
    let (base, fake) = start().await;
    let api = client(&base, "tok-1");
    fake.flaky_left.store(5, Ordering::SeqCst);
    let err = api.send(ApiRequest::post("/flaky").json(serde_json::json!({}))).await.expect_err("503");
    assert_eq!(fake.flaky_calls.load(Ordering::SeqCst), 1);
    assert_eq!(err.class(), ErrorClass::Transient);
    assert_eq!(err.status(), Some(503));
    assert_eq!(err.code(), Some("UNAVAILABLE"));
    assert_eq!(err.retry_after(), Some(Duration::from_secs(0)));
}

#[tokio::test]
async fn gives_up_after_max_attempts() {
    let (base, fake) = start().await;
    let api = client(&base, "tok-1");
    fake.flaky_left.store(10, Ordering::SeqCst);
    let started = Instant::now();
    let err = api.send(ApiRequest::get("/flaky")).await.expect_err("still failing");
    assert_eq!(fake.flaky_calls.load(Ordering::SeqCst), 4);
    assert_eq!(err.class(), ErrorClass::Transient);
    assert!(started.elapsed() < Duration::from_secs(5));
}

/// A token source that hands out t-1, then t-2 after a 401 (counting refreshes).
struct Rotating {
    refreshes: AtomicU32,
    fail_refresh: bool,
}

#[async_trait::async_trait]
impl TokenSource for Rotating {
    async fn access_token(&self) -> Result<AccessToken, AuthError> {
        Ok(AccessToken::new("t-1"))
    }
    async fn after_unauthorized(&self, failed: &AccessToken) -> Result<AccessToken, AuthError> {
        assert_eq!(failed.expose(), "t-1");
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        if self.fail_refresh {
            return Err(AuthError::SessionExpired);
        }
        Ok(AccessToken::new("t-2"))
    }
}

#[tokio::test]
async fn refreshes_once_on_401() {
    let (base, fake) = start().await;
    let src = Arc::new(Rotating { refreshes: AtomicU32::new(0), fail_refresh: false });
    let api = ApiClient::builder(&base).tokens(src.clone()).retry(fast_retry()).build().expect("client");
    api.send(ApiRequest::get("/guarded")).await.expect("ok after refresh");
    assert_eq!(src.refreshes.load(Ordering::SeqCst), 1);

    // A token that stays invalid after the refresh: one refresh only, then Unauthorized.
    *fake.valid_token.lock().expect("lock") = "nobody".into();
    let src2 = Arc::new(Rotating { refreshes: AtomicU32::new(0), fail_refresh: false });
    let api2 = ApiClient::builder(&base).tokens(src2.clone()).build().expect("client");
    let err = api2.send(ApiRequest::get("/guarded")).await.expect_err("401");
    assert_eq!(src2.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(err.class(), ErrorClass::Unauthorized);

    // A rejected refresh token: SessionExpired.
    let src3 = Arc::new(Rotating { refreshes: AtomicU32::new(0), fail_refresh: true });
    let api3 = ApiClient::builder(&base).tokens(src3).build().expect("client");
    let err = api3.send(ApiRequest::get("/guarded")).await.expect_err("expired");
    assert_eq!(err.class(), ErrorClass::SessionExpired);
}

#[tokio::test]
async fn maps_412_with_the_current_row() {
    let (base, _fake) = start().await;
    let api = client(&base, "tok");
    let err = api.send(ApiRequest::patch("/notes/n1").if_match("e1").json(serde_json::json!({"title": "mine"}))).await.expect_err("412");
    assert_eq!(err.class(), ErrorClass::Conflict);
    assert_eq!(err.body().expect("body")["current"]["title"], "server");
    let ok = api.send(ApiRequest::patch("/notes/n1").if_match("e2").json(serde_json::json!({"title": "mine"}))).await.expect("ok");
    assert_eq!(ok.etag().as_deref(), Some("e3"));
}

#[tokio::test]
async fn classification_of_status_codes() {
    let (base, _fake) = start().await;
    let api = ApiClient::builder(&base).tokens(Arc::new(StaticToken(AccessToken::new("t")))).retry(RetryPolicy::none()).build().expect("client");
    let cases = [
        (400, "BAD", ErrorClass::Definitive),
        (403, "FORBIDDEN", ErrorClass::Definitive),
        (404, "NOT_FOUND", ErrorClass::NotFound),
        (409, "CONFLICT", ErrorClass::Conflict),
        (410, "CURSOR_EXPIRED", ErrorClass::CursorExpired),
        (413, "TOO_LARGE", ErrorClass::Definitive),
        (422, "VALIDATION", ErrorClass::Definitive),
        (429, "RATE_LIMITED", ErrorClass::Transient),
        (500, "DATABASE_ERROR", ErrorClass::Transient),
    ];
    for (code, error, class) in cases {
        let err = api.send(ApiRequest::get("/status").query("code", code).query("error", error)).await.expect_err("error");
        assert_eq!((err.status(), err.class()), (Some(code), class), "{code} {error}");
        assert_eq!(err.code(), Some(error));
    }
}

#[tokio::test]
async fn network_errors_are_transient() {
    // Nothing listens on this port.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(listener);
    let api = ApiClient::builder(format!("http://{addr}")).tokens(Arc::new(StaticToken(AccessToken::new("t")))).retry(RetryPolicy::none()).build().expect("client");
    let err = api.send(ApiRequest::get("/x")).await.expect_err("refused");
    assert!(matches!(err, ApiError::Network { maybe_sent: false, .. }), "{err:?}");
    assert_eq!(err.class(), ErrorClass::Transient);
}

#[tokio::test]
async fn kdp_paging_and_guard() {
    let (base, _fake) = start().await;
    let api = client(&base, "tok");
    let p1 = api.delta_page("/notes/delta", &Cursor::zero(), 2).await.expect("page 1");
    assert_eq!(p1.changes.len(), 2);
    assert_eq!(p1.cursor.as_str(), "2");
    assert!(p1.has_more);
    let (all, end) = api.delta_all("/notes/delta", Cursor::zero(), 2, 10).await.expect("all");
    assert_eq!(all.len(), 5);
    assert_eq!(end.as_str(), "5");
    assert_eq!(all[3].kind, ChangeKind::Deleted);
    assert_eq!(all[0].row("note")["title"], "t1");
    let seqs: Vec<i64> = all.iter().map(|c| c.change_seq).collect();
    assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
    let err = api.delta_page("/stuck/delta", &Cursor::zero(), 2).await.expect_err("guard");
    assert_eq!(err.class(), ErrorClass::Protocol);
}
