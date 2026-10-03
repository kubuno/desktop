//! A fake of the core's native auth (`handlers/auth/refresh.rs`): rotation on every refresh, family revocation on
//! reuse, and the rotation grace (an old token whose successor was never presented gets a fresh pair).

#![allow(dead_code)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

#[derive(Debug, Clone)]
struct Rt {
    family: u64,
    user: String,
    presented: bool,
    revoked: bool,
    successor: Option<String>,
}

#[derive(Default)]
pub struct AuthState {
    tokens: Mutex<HashMap<String, Rt>>,
    access: Mutex<HashMap<String, String>>, // access token -> user
    revoked_families: Mutex<Vec<u64>>,
    next: AtomicU64,
    /// Refresh calls that rotated (successful or response dropped).
    pub rotations: AtomicU64,
    /// Refresh calls received.
    pub refresh_calls: AtomicU64,
    /// Answer 503 *after* rotating, for the next N refreshes (a lost response).
    pub drop_after_rotation: AtomicU32,
    /// Answer 503 without rotating, for the next N refreshes.
    pub fail_before_rotation: AtomicU32,
}

fn b64url(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let chars = [(n >> 18) & 63, (n >> 12) & 63, (n >> 6) & 63, n & 63];
        for (i, c) in chars.iter().enumerate() {
            if i <= chunk.len() {
                out.push(A[*c as usize] as char);
            }
        }
    }
    out
}

impl AuthState {
    fn mint_access(&self, user: &str) -> String {
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        let payload = serde_json::json!({"sub": user, "iat": 1_000, "exp": 1_900, "n": n}).to_string();
        let token = format!("eyJhbGciOiJub25lIn0.{}.sig{n}", b64url(payload.as_bytes()));
        self.access.lock().expect("lock").insert(token.clone(), user.to_string());
        token
    }

    fn mint_refresh(&self, family: u64, user: &str) -> String {
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        let t = format!("rt-{family}-{n}");
        self.tokens.lock().expect("lock").insert(t.clone(), Rt { family, user: user.to_string(), presented: false, revoked: false, successor: None });
        t
    }

    /// A signed-in session: a new family. Returns (access, refresh).
    pub fn login(&self, user: &str) -> (String, String) {
        let family = self.next.fetch_add(1, Ordering::SeqCst);
        (self.mint_access(user), self.mint_refresh(family, user))
    }

    /// Revokes every token of `user` (sign-out from the web's device list).
    pub fn revoke_user(&self, user: &str) {
        let mut toks = self.tokens.lock().expect("lock");
        for rt in toks.values_mut() {
            if rt.user == user {
                rt.revoked = true;
                self.revoked_families.lock().expect("lock").push(rt.family);
            }
        }
        self.access.lock().expect("lock").retain(|_, u| u != user);
    }

    pub fn family_revoked_for(&self, user: &str) -> bool {
        let toks = self.tokens.lock().expect("lock");
        let fams = self.revoked_families.lock().expect("lock");
        toks.values().any(|rt| rt.user == user && fams.contains(&rt.family))
    }

    pub fn access_valid(&self, token: &str) -> bool {
        self.access.lock().expect("lock").contains_key(token)
    }

    /// Expires every access token (forces a refresh on next use).
    pub fn expire_access(&self) {
        self.access.lock().expect("lock").clear();
    }
}

async fn refresh(State(s): State<Arc<AuthState>>, Json(body): Json<serde_json::Value>) -> Response {
    s.refresh_calls.fetch_add(1, Ordering::SeqCst);
    if s.fail_before_rotation.load(Ordering::SeqCst) > 0 {
        s.fail_before_rotation.fetch_sub(1, Ordering::SeqCst);
        return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": "UNAVAILABLE"}))).into_response();
    }
    let presented = body["refresh_token"].as_str().unwrap_or_default().to_string();
    let rt = s.tokens.lock().expect("lock").get(&presented).cloned();
    let Some(rt) = rt else { return StatusCode::UNAUTHORIZED.into_response() };
    if s.revoked_families.lock().expect("lock").contains(&rt.family) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if rt.revoked {
        // Grace: the successor was never presented -> supersede it with a fresh token.
        let succ_virgin = rt
            .successor
            .as_ref()
            .and_then(|succ| s.tokens.lock().expect("lock").get(succ).cloned())
            .is_some_and(|succ| !succ.presented && !succ.revoked);
        if !succ_virgin {
            s.revoked_families.lock().expect("lock").push(rt.family);
            return StatusCode::UNAUTHORIZED.into_response();
        }
        if let Some(succ) = &rt.successor {
            if let Some(x) = s.tokens.lock().expect("lock").get_mut(succ) {
                x.revoked = true;
            }
        }
    }
    s.rotations.fetch_add(1, Ordering::SeqCst);
    let new_refresh = s.mint_refresh(rt.family, &rt.user);
    {
        let mut toks = s.tokens.lock().expect("lock");
        if let Some(x) = toks.get_mut(&presented) {
            x.presented = true;
            x.revoked = true;
            x.successor = Some(new_refresh.clone());
        }
    }
    let access = s.mint_access(&rt.user);
    if s.drop_after_rotation.load(Ordering::SeqCst) > 0 {
        s.drop_after_rotation.fetch_sub(1, Ordering::SeqCst);
        return (StatusCode::BAD_GATEWAY, "lost").into_response();
    }
    Json(serde_json::json!({"access_token": access, "refresh_token": new_refresh, "refresh_expires_at": "2099-01-01T00:00:00Z"})).into_response()
}

fn bearer(h: &HeaderMap) -> Option<String> {
    h.get("authorization")?.to_str().ok()?.strip_prefix("Bearer ").map(str::to_string)
}

async fn me(State(s): State<Arc<AuthState>>, h: HeaderMap) -> Response {
    let Some(t) = bearer(&h) else { return StatusCode::UNAUTHORIZED.into_response() };
    let user = s.access.lock().expect("lock").get(&t).cloned();
    match user {
        Some(u) => Json(serde_json::json!({"user": {"id": u, "display_name": "Test", "email": "t@example.test"}})).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

async fn protected(State(s): State<Arc<AuthState>>, h: HeaderMap) -> Response {
    match bearer(&h) {
        Some(t) if s.access_valid(&t) => Json(serde_json::json!({"ok": true})).into_response(),
        _ => StatusCode::UNAUTHORIZED.into_response(),
    }
}

async fn logout() -> StatusCode {
    StatusCode::OK
}

/// Starts the fake on 127.0.0.1:0. Returns its base URL and state.
pub async fn start() -> (String, Arc<AuthState>) {
    let state = Arc::new(AuthState::default());
    let app = Router::new()
        .route("/api/v1/auth/refresh", post(refresh))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/me", get(me))
        .route("/api/v1/protected", get(protected))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), state)
}
