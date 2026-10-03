//! An in-process fake of a module that implements the Kubuno Delta Protocol v1 correctly (§7.1), for a toy
//! entity `note {id, title, body, etag}`: commit-ordered `change_seq`, tombstones, client-minted ids with create
//! replay, `Idempotency-Key` scoped by user and body-hashed, atomic `If-Match` with 412 + current row, and knobs:
//! 503 before doing anything, "commit then drop the response", `410 CURSOR_EXPIRED`, 422 on a title "INVALID".
//! Users are identified by their bearer token (`Bearer user-1` is user `user-1`).

#![allow(dead_code)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    pub version: i64,
    pub change_seq: i64,
}

impl Note {
    fn json(&self) -> Value {
        json!({"id": self.id, "title": self.title, "body": self.body, "etag": format!("v{}", self.version)})
    }
}

/// (body hash, status, answer) of an executed idempotent request.
type IdemRecord = (String, u16, Value);

#[derive(Default)]
pub struct Fake {
    notes: Mutex<HashMap<(String, String), Note>>,
    tombstones: Mutex<HashMap<(String, String), i64>>,
    seq: AtomicI64,
    idem: Mutex<HashMap<(String, String), IdemRecord>>,
    /// How many times each idempotency key was **executed** (replays not counted).
    pub executions: Mutex<HashMap<String, u32>>,
    /// Every executed write: (user, method, id).
    pub writes: Mutex<Vec<(String, String, String)>>,
    pub fail_next: AtomicU32,
    pub drop_after_commit: AtomicU32,
    /// A cursor below this value gets 410 CURSOR_EXPIRED (0 = never).
    pub expired_below: AtomicI64,
    pub requests: AtomicU32,
}

fn user_of(h: &HeaderMap) -> Option<String> {
    let u = h.get("authorization")?.to_str().ok()?.strip_prefix("Bearer ")?.to_string();
    u.starts_with("user-").then_some(u)
}

fn err(status: StatusCode, code: &str, extra: Value) -> Response {
    let mut body = json!({"error": code, "message": code.to_lowercase()});
    if let (Value::Object(b), Value::Object(e)) = (&mut body, extra) {
        b.extend(e);
    }
    (status, Json(body)).into_response()
}

impl Fake {
    fn next_seq(&self) -> i64 {
        self.seq.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn get(&self, user: &str, id: &str) -> Option<Note> {
        self.notes.lock().expect("lock").get(&(user.to_string(), id.to_string())).cloned()
    }

    pub fn count(&self, user: &str) -> usize {
        self.notes.lock().expect("lock").keys().filter(|(u, _)| u == user).count()
    }

    pub fn server_create(&self, user: &str, id: &str, title: &str, body: &str) {
        let seq = self.next_seq();
        self.notes.lock().expect("lock").insert(
            (user.into(), id.into()),
            Note { id: id.into(), title: title.into(), body: body.into(), version: 1, change_seq: seq },
        );
    }

    pub fn server_patch(&self, user: &str, id: &str, title: Option<&str>, body: Option<&str>) {
        let seq = self.next_seq();
        let mut notes = self.notes.lock().expect("lock");
        if let Some(n) = notes.get_mut(&(user.to_string(), id.to_string())) {
            if let Some(t) = title {
                n.title = t.into();
            }
            if let Some(b) = body {
                n.body = b.into();
            }
            n.version += 1;
            n.change_seq = seq;
        }
    }

    pub fn server_delete(&self, user: &str, id: &str) {
        let seq = self.next_seq();
        self.notes.lock().expect("lock").remove(&(user.to_string(), id.to_string()));
        self.tombstones.lock().expect("lock").insert((user.to_string(), id.to_string()), seq);
    }

    /// Deletes without a tombstone (a purge older than the tombstone retention).
    pub fn server_purge(&self, user: &str, id: &str) {
        self.notes.lock().expect("lock").remove(&(user.to_string(), id.to_string()));
    }

    pub fn executions_of(&self, key: &str) -> u32 {
        self.executions.lock().expect("lock").get(key).copied().unwrap_or(0)
    }

    pub fn max_executions(&self) -> u32 {
        self.executions.lock().expect("lock").values().copied().max().unwrap_or(0)
    }
}

async fn delta(State(s): State<Arc<Fake>>, h: HeaderMap, Query(q): Query<HashMap<String, String>>) -> Response {
    s.requests.fetch_add(1, Ordering::SeqCst);
    let Some(user) = user_of(&h) else { return err(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", json!({})) };
    let cursor: i64 = q.get("cursor").and_then(|c| c.parse().ok()).unwrap_or(0);
    let limit: usize = q.get("limit").and_then(|c| c.parse().ok()).unwrap_or(100);
    let expired = s.expired_below.load(Ordering::SeqCst);
    if expired > 0 && cursor > 0 && cursor < expired {
        // One-shot: a real server must also let the client page through the full snapshot that follows.
        s.expired_below.store(0, Ordering::SeqCst);
        return err(StatusCode::GONE, "CURSOR_EXPIRED", json!({}));
    }
    let mut changes: Vec<(i64, Value)> = Vec::new();
    for ((u, _), n) in s.notes.lock().expect("lock").iter() {
        if *u == user && n.change_seq > cursor {
            changes.push((n.change_seq, json!({"uuid": n.id, "kind": "modified", "change_seq": n.change_seq, "note": n.json()})));
        }
    }
    for ((u, id), seq) in s.tombstones.lock().expect("lock").iter() {
        if *u == user && *seq > cursor {
            changes.push((*seq, json!({"uuid": id, "kind": "deleted", "change_seq": seq})));
        }
    }
    changes.sort_by_key(|c| c.0);
    let has_more = changes.len() > limit;
    changes.truncate(limit);
    let last = changes.last().map(|c| c.0).unwrap_or(cursor);
    Json(json!({"changes": changes.into_iter().map(|c| c.1).collect::<Vec<_>>(), "cursor": last, "has_more": has_more})).into_response()
}

async fn get_one(State(s): State<Arc<Fake>>, h: HeaderMap, Path(id): Path<String>) -> Response {
    let Some(user) = user_of(&h) else { return err(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", json!({})) };
    match s.get(&user, &id) {
        Some(n) => Json(json!({"note": n.json()})).into_response(),
        None => err(StatusCode::NOT_FOUND, "NOT_FOUND", json!({})),
    }
}

/// Runs a write under the idempotency rules; `op` returns (status, body).
fn idempotent(s: &Fake, user: &str, h: &HeaderMap, body: &[u8], op: impl FnOnce() -> (u16, Value)) -> Response {
    s.requests.fetch_add(1, Ordering::SeqCst);
    if s.fail_next.load(Ordering::SeqCst) > 0 {
        s.fail_next.fetch_sub(1, Ordering::SeqCst);
        return err(StatusCode::SERVICE_UNAVAILABLE, "UNAVAILABLE", json!({}));
    }
    let key = h.get("idempotency-key").and_then(|v| v.to_str().ok()).map(str::to_string);
    let hash = format!("{:x}", body.iter().fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(u64::from(*b))));
    if let Some(k) = &key {
        if let Some((h0, status, resp)) = s.idem.lock().expect("lock").get(&(user.to_string(), k.clone())).cloned() {
            if h0 != hash {
                return err(StatusCode::CONFLICT, "IDEMPOTENCY_KEY_REUSED", json!({}));
            }
            let mut r = (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(resp)).into_response();
            r.headers_mut().insert("idempotency-replayed", axum::http::HeaderValue::from_static("true"));
            return r;
        }
    }
    let (status, resp) = op();
    if let Some(k) = &key {
        *s.executions.lock().expect("lock").entry(k.clone()).or_insert(0) += 1;
        if status < 500 {
            s.idem.lock().expect("lock").insert((user.to_string(), k.clone()), (hash, status, resp.clone()));
        }
    }
    if s.drop_after_commit.load(Ordering::SeqCst) > 0 && status < 300 {
        s.drop_after_commit.fetch_sub(1, Ordering::SeqCst);
        return (StatusCode::BAD_GATEWAY, "response lost").into_response();
    }
    (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(resp)).into_response()
}

async fn create(State(s): State<Arc<Fake>>, h: HeaderMap, body: Bytes) -> Response {
    let Some(user) = user_of(&h) else { return err(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", json!({})) };
    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let s2 = s.clone();
    let u = user.clone();
    idempotent(&s, &u, &h, &body, move || {
        let id = v["id"].as_str().unwrap_or_default().to_string();
        let title = v["title"].as_str().unwrap_or_default().to_string();
        if id.is_empty() || title == "INVALID" {
            return (422, json!({"error": "VALIDATION", "message": "invalid note"}));
        }
        // Create replay with the same client id: the existing row, not a 500.
        if let Some(n) = s2.get(&user, &id) {
            return (200, json!({"note": n.json()}));
        }
        s2.writes.lock().expect("lock").push((user.clone(), "POST".into(), id.clone()));
        let seq = s2.next_seq();
        let n = Note { id: id.clone(), title, body: v["body"].as_str().unwrap_or_default().into(), version: 1, change_seq: seq };
        s2.tombstones.lock().expect("lock").remove(&(user.clone(), id.clone()));
        s2.notes.lock().expect("lock").insert((user.clone(), id), n.clone());
        (201, json!({"note": n.json()}))
    })
}

async fn write_one(State(s): State<Arc<Fake>>, method: Method, h: HeaderMap, Path(id): Path<String>, body: Bytes) -> Response {
    let Some(user) = user_of(&h) else { return err(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", json!({})) };
    let if_match = h.get("if-match").and_then(|v| v.to_str().ok()).map(str::to_string);
    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let s2 = s.clone();
    let u = user.clone();
    idempotent(&s, &u, &h, &body, move || {
        let mut notes = s2.notes.lock().expect("lock");
        let key = (user.clone(), id.clone());
        let Some(n) = notes.get_mut(&key) else { return (404, json!({"error": "NOT_FOUND", "message": "no such note"})) };
        // Atomic If-Match: checked and applied under the same lock.
        if let Some(m) = &if_match {
            if *m != format!("v{}", n.version) {
                return (412, json!({"error": "PRECONDITION_FAILED", "message": "stale", "current": n.json()}));
            }
        }
        if method == Method::DELETE {
            notes.remove(&key);
            drop(notes);
            let seq = s2.next_seq();
            s2.tombstones.lock().expect("lock").insert(key, seq);
            s2.writes.lock().expect("lock").push((user.clone(), "DELETE".into(), id.clone()));
            return (200, json!({}));
        }
        if v["title"].as_str() == Some("INVALID") {
            return (422, json!({"error": "VALIDATION", "message": "invalid title"}));
        }
        if let Some(t) = v["title"].as_str() {
            n.title = t.into();
        }
        if let Some(b) = v["body"].as_str() {
            n.body = b.into();
        }
        n.version += 1;
        n.change_seq = s2.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let out = n.json();
        drop(notes);
        s2.writes.lock().expect("lock").push((user.clone(), "PATCH".into(), id.clone()));
        (200, json!({"note": out}))
    })
}

pub async fn start() -> (String, Arc<Fake>) {
    let fake = Arc::new(Fake::default());
    let app = Router::new()
        .route("/api/v1/notes/notes/delta", get(delta))
        .route("/api/v1/notes/notes", post(create))
        .route("/api/v1/notes/notes/:id", get(get_one).patch(write_one).delete(write_one))
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), fake)
}
