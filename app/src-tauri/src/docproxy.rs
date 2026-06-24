//! Local document proxy — gives the embedded web app a STABLE origin
//! (`http://127.0.0.1:<fixed port>`) online and offline, which is what makes
//! offline reload work: IndexedDB (where `y-indexeddb` persists the document) is
//! keyed by origin, so the window must never switch between the remote core and
//! a local URL.
//!
//! The proxy:
//!   • serves the web shell + module bundle + assets, caching every successful
//!     GET to disk so a reload while offline is served from cache;
//!   • passes `/api/v1/*` through to the core, injecting the native session
//!     (`Authorization: Bearer`) so the web app is authenticated WITHOUT a web
//!     login — and caches a few read endpoints (`/me`, `/modules`, the document)
//!     so the editor can still mount offline;
//!   • intercepts `POST /api/v1/auth/refresh` and answers it from the native
//!     kubuno-sync session (rotating on disk), so the frontend's bootstrap
//!     (`initialize()` → refresh → `/me`) succeeds with a real core token that it
//!     then reuses for every request and for the collab WebSocket token;
//!   • bridges the collab WebSocket (`/collab/:room/sync`) to the core, failing
//!     cleanly when offline (the frontend falls back to `y-indexeddb`).
//!
//! Auth design: a SINGLE interception point (`/auth/refresh`) hands the frontend
//! the native access token. After that the frontend carries a valid token in
//! `Authorization` and in the WS `?token=` itself — no per-request rewriting and
//! no frontend changes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path as AxPath, RawQuery, Request, State,
    },
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use sha2::{Digest, Sha256};
use tokio_tungstenite::tungstenite::Message as TMsg;

const MAX_BODY: usize = 32 * 1024 * 1024;

#[derive(Clone)]
struct ProxyState {
    id:        String,
    upstream:  String, // e.g. "https://dev.kubuno.com" (no trailing slash)
    http:      reqwest_async::Client,
    cache_dir: PathBuf,
}

/// instance id → loopback port of its running proxy.
fn registry() -> &'static Mutex<HashMap<String, u16>> {
    static R: OnceLock<Mutex<HashMap<String, u16>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Deterministic, stable loopback port for an instance (FNV-1a over the id).
/// Stability across restarts is what keeps the web origin — and therefore the
/// offline IndexedDB store — identical between sessions.
fn port_for(id: &str) -> u16 {
    let mut h: u32 = 0x811c_9dc5;
    for b in id.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    8700 + (h % 80) as u16
}

/// Ensure the proxy for `id` is running and return its loopback port. Idempotent:
/// a second call for the same instance returns the already-bound port.
pub fn ensure_started(id: &str) -> Result<u16, String> {
    {
        let reg = registry().lock().unwrap_or_else(|p| p.into_inner());
        if let Some(p) = reg.get(id) {
            return Ok(*p);
        }
    }
    let upstream = kubuno_sync::server_url(id)
        .ok_or_else(|| format!("instance inconnue : {id}"))?
        .trim_end_matches('/')
        .to_string();
    let cache_dir = kubuno_sync::db_path(id)
        .map_err(|e| e.to_string())?
        .parent()
        .map(|p| p.join("webcache"))
        .ok_or_else(|| "chemin de cache introuvable".to_string())?;
    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;

    let port = port_for(id);
    // Bind synchronously so we fail early (and keep ownership of the port) before
    // the window is told to load it.
    let std_listener =
        std::net::TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("bind {port} : {e}"))?;
    std_listener.set_nonblocking(true).map_err(|e| e.to_string())?;

    let state = ProxyState {
        id: id.to_string(),
        upstream,
        http: reqwest_async::Client::builder()
            .build()
            .map_err(|e| e.to_string())?,
        cache_dir,
    };

    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(std_listener) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[docproxy] from_std : {e}");
                return;
            }
        };
        let app = Router::new()
            .route("/api/v1/auth/refresh", post(auth_refresh))
            .route("/collab/:room/sync", get(collab_ws))
            .fallback(proxy_all)
            .with_state(state);
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[docproxy] serve : {e}");
        }
    });

    registry()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(id.to_string(), port);
    Ok(port)
}

// ── Auth interception ───────────────────────────────────────────────────────

/// How long a freshly minted access token is reused before we rotate again. The
/// core's access token lives ~15 min; rotating at most every 10 min keeps the
/// frontend authenticated without churning the server's limited session slots
/// (which, done on every bootstrap/retry, would evict the active token and log
/// the user out).
const REFRESH_TTL: Duration = Duration::from_secs(600);

/// Per-instance cache of the last access token handed out and when. Shared by all
/// document/app windows so a flurry of `/auth/refresh` calls maps to one rotation.
fn token_cache() -> &'static Mutex<HashMap<String, (String, Instant)>> {
    static C: OnceLock<Mutex<HashMap<String, (String, Instant)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `POST /api/v1/auth/refresh` → answer from the native session WITHOUT rotating
/// every time. Returns the cached token while it's fresh; otherwise forces one
/// kubuno-sync refresh (rotating on disk) and caches it. Offline: returns the
/// last good / stored token so the web app can still bootstrap from IndexedDB.
async fn auth_refresh(State(st): State<ProxyState>) -> Response {
    // Reuse the still-fresh token (no rotation) — this is the hot path.
    {
        let cache = token_cache().lock().unwrap_or_else(|p| p.into_inner());
        if let Some((tok, at)) = cache.get(&st.id) {
            if at.elapsed() < REFRESH_TTL {
                return token_json(tok.clone());
            }
        }
    }
    // Forced offline → never touch the network; serve the last good token so the
    // embedded app still bootstraps from local data.
    if kubuno_sync::is_offline() {
        if let Ok(tok) = std::fs::read_to_string(st.cache_dir.join("last_token")) {
            if !tok.is_empty() {
                return token_json(tok);
            }
        }
        if let Some(tok) = kubuno_sync::access_token(&st.id) {
            return token_json(tok);
        }
        return (StatusCode::SERVICE_UNAVAILABLE, "hors-ligne").into_response();
    }
    let id = st.id.clone();
    let refreshed = tokio::task::spawn_blocking(move || kubuno_sync::refresh_access(&id)).await;
    match refreshed {
        Ok(Ok(tok)) => {
            token_cache()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(st.id.clone(), (tok.clone(), Instant::now()));
            let _ = std::fs::write(st.cache_dir.join("last_token"), &tok);
            token_json(tok)
        }
        _ => {
            if let Ok(tok) = std::fs::read_to_string(st.cache_dir.join("last_token")) {
                if !tok.is_empty() {
                    return token_json(tok);
                }
            }
            if let Some(tok) = kubuno_sync::access_token(&st.id) {
                return token_json(tok);
            }
            (StatusCode::SERVICE_UNAVAILABLE, "hors-ligne, aucune session en cache").into_response()
        }
    }
}

fn token_json(token: String) -> Response {
    let body = serde_json::json!({ "access_token": token }).to_string();
    ([(header::CONTENT_TYPE, "application/json")], body).into_response()
}

// ── Generic caching reverse proxy ───────────────────────────────────────────

async fn proxy_all(State(st): State<ProxyState>, req: Request) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path().to_string();
    let pq = uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| path.clone());
    let req_headers = req.headers().clone();
    let body_bytes = axum::body::to_bytes(req.into_body(), MAX_BODY)
        .await
        .unwrap_or_default();

    // Local-first: the office DOCUMENTS sub-namespace is offered to the embedded
    // WASM backend (offline-capable) when its artifact is present. The module
    // returns the reserved `status == 0` for requests it doesn't own yet (e.g. a
    // document that only exists on the server) → we fall through to the core
    // proxy below. Everything else under /api/v1/office (fonts, recipients, other
    // sub-modules) goes straight to the core. Keeps the daemon route-agnostic.
    let office_doc = path.starts_with("/api/v1/office/documents");
    if crate::wasmoffice::enabled() && office_doc {
        let id = st.id.clone();
        let m = method.as_str().to_string();
        let p = pq.clone();
        let b = body_bytes.to_vec();
        let res = tokio::task::spawn_blocking(move || crate::wasmoffice::handle(&id, &m, &p, &b)).await;
        if let Ok(Some((status, ctype, out))) = res {
            if status != 0 {
                let mut headers = HeaderMap::new();
                if let Ok(v) = HeaderValue::from_str(&ctype) {
                    headers.insert(header::CONTENT_TYPE, v);
                }
                let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                return build_response(code, headers, out);
            }
            // status == 0 → passthrough: fall through to the core proxy.
        }
    }

    let is_get = method == Method::GET;
    let navigation = is_get && wants_html(&req_headers) && !is_asset_or_api(&path);
    let cacheable = is_get && (is_cacheable_asset(&path) || is_cacheable_api(&path));

    // Forced offline → don't reach the core; serve the shell/assets from cache.
    // (Office document routes were already handled by the WASM backend above.)
    if kubuno_sync::is_offline() {
        return from_cache(&st.cache_dir, &path, navigation);
    }

    let url = format!("{}{}", st.upstream, pq);
    let orig_ae = req_headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let mut rb = st.http.request(method.clone(), &url);
    for (k, v) in req_headers.iter() {
        if is_hop_by_hop(k.as_str()) || k == header::HOST || k == header::ACCEPT_ENCODING {
            continue;
        }
        rb = rb.header(k.clone(), v.clone());
    }
    // We rewrite the shell HTML (standalone CSS) but this reqwest has no
    // auto-decompression → ask the upstream for an uncompressed response on
    // navigations so the bytes are plain HTML. Assets keep their compression.
    let accept_encoding = if navigation { "identity" } else { orig_ae.as_deref().unwrap_or("identity") };
    rb = rb.header(header::ACCEPT_ENCODING, accept_encoding);
    // Inject the native identity for API calls that arrive before the frontend
    // has its token (e.g. the un-awaited `fetchModules()` during bootstrap).
    if path.starts_with("/api/") && !req_headers.contains_key(header::AUTHORIZATION) {
        if let Some(tok) = kubuno_sync::access_token(&st.id) {
            if let Ok(v) = HeaderValue::from_str(&format!("Bearer {tok}")) {
                rb = rb.header(header::AUTHORIZATION, v);
            }
        }
    }
    if !body_bytes.is_empty() {
        rb = rb.body(body_bytes.to_vec());
    }

    match rb.send().await {
        Ok(resp) => {
            let status = resp.status();
            let ct = resp
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();
            let mut out = HeaderMap::new();
            for (k, v) in resp.headers().iter() {
                if is_hop_by_hop(k.as_str()) {
                    continue;
                }
                out.insert(k.clone(), v.clone());
            }
            let bytes = resp.bytes().await.unwrap_or_default();
            if status.is_success() {
                if cacheable {
                    cache_write(&st.cache_dir, &path, &ct, &bytes);
                }
                if navigation && ct.contains("text/html") {
                    cache_write(&st.cache_dir, "__shell__", &ct, &bytes);
                }
            }
            // Don't let WebView2 cache the shell HTML (it would bypass the proxy
            // and skip the standalone-CSS injection on later loads).
            if ct.contains("text/html") {
                out.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            }
            // Cache stays pristine; only the served HTML gets the standalone CSS.
            build_response(status, out, inject_standalone_css(&ct, bytes.to_vec()))
        }
        // Network error → serve from cache so the shell/app/editor can still load.
        Err(_) => from_cache(&st.cache_dir, &path, navigation),
    }
}

/// Serve a request from the disk cache (offline / upstream unreachable): the
/// exact path, else the cached shell for SPA navigations, else 502.
fn from_cache(cache_dir: &std::path::Path, path: &str, navigation: bool) -> Response {
    if let Some((ct, bytes)) = cache_read(cache_dir, path) {
        return cached(ct, bytes);
    }
    if navigation {
        if let Some((ct, bytes)) = cache_read(cache_dir, "__shell__") {
            return cached(ct, bytes);
        }
    }
    (StatusCode::BAD_GATEWAY, "hors-ligne et non mis en cache").into_response()
}

// ── Collab WebSocket bridge ─────────────────────────────────────────────────

async fn collab_ws(
    State(st): State<ProxyState>,
    AxPath(room): AxPath<String>,
    RawQuery(q): RawQuery,
    ws: WebSocketUpgrade,
) -> Response {
    // Forced offline → no server collab: accept the upgrade then close so the
    // editor falls back to its local (y-indexeddb) offline mode.
    if kubuno_sync::is_offline() {
        return ws.on_upgrade(|mut client| async move {
            let _ = client.send(Message::Close(None)).await;
        });
    }
    // The frontend already puts its (native) token in `?token=`; keep it, but
    // fall back to the stored token if somehow absent.
    let query = q.unwrap_or_default();
    let has_token = query.split('&').any(|p| p.starts_with("token="));
    let token = if has_token {
        String::new()
    } else {
        kubuno_sync::access_token(&st.id).unwrap_or_default()
    };
    let scheme = if st.upstream.starts_with("https") { "wss" } else { "ws" };
    let host = st
        .upstream
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    // Local-first: a doc born offline has a local id (doc-N). Once pushed and
    // mapped to its core uuid, rewrite the collab room `office-document:doc-N` →
    // `office-document:<uuid>` so the native editor joins the SAME server room as
    // web clients. Unmapped (not yet synced) → left as-is (no server collab).
    let room = rewrite_collab_room(&st.id, room);
    // Axum decodes the path param; the core expects the ':' percent-encoded.
    let room_enc = room.replace(':', "%3A");
    let mut up = format!("{scheme}://{host}/collab/{room_enc}/sync");
    if !query.is_empty() {
        up.push('?');
        up.push_str(&query);
    } else if !token.is_empty() {
        up.push_str("?token=");
        up.push_str(&token);
    }
    ws.on_upgrade(move |client| bridge(client, up))
}

/// Map an `office-document:<doc-N>` collab room to `office-document:<uuid>` when
/// the local doc is mapped to a core uuid. Other rooms pass through unchanged.
fn rewrite_collab_room(instance_id: &str, room: String) -> String {
    if !crate::wasmoffice::enabled() {
        return room;
    }
    if let Some(doc_id) = room.strip_prefix("office-document:") {
        if let Some(uuid) = crate::office_sync::mapped_uuid(instance_id, doc_id) {
            eprintln!("[docproxy] room collab réécrite : {doc_id} → {uuid}");
            return format!("office-document:{uuid}");
        }
    }
    room
}

async fn bridge(client: WebSocket, upstream_url: String) {
    let upstream = match tokio_tungstenite::connect_async(&upstream_url).await {
        Ok((s, _)) => s,
        Err(_) => {
            // Offline / refused: close the client socket so the frontend treats
            // collab as disconnected and stays in offline (y-indexeddb) mode.
            let mut c = client;
            let _ = c.send(Message::Close(None)).await;
            return;
        }
    };
    let (mut up_tx, mut up_rx) = upstream.split();
    let (mut cl_tx, mut cl_rx) = client.split();

    let client_to_up = async {
        while let Some(Ok(msg)) = cl_rx.next().await {
            let fwd = match msg {
                Message::Binary(b) => TMsg::Binary(b),
                Message::Text(t) => TMsg::Text(t),
                Message::Ping(p) => TMsg::Ping(p),
                Message::Pong(p) => TMsg::Pong(p),
                Message::Close(_) => break,
            };
            if up_tx.send(fwd).await.is_err() {
                break;
            }
        }
        let _ = up_tx.close().await;
    };
    let up_to_client = async {
        while let Some(Ok(msg)) = up_rx.next().await {
            let fwd = match msg {
                TMsg::Binary(b) => Message::Binary(b),
                TMsg::Text(t) => Message::Text(t),
                TMsg::Ping(p) => Message::Ping(p),
                TMsg::Pong(p) => Message::Pong(p),
                TMsg::Close(_) => break,
                TMsg::Frame(_) => continue,
            };
            if cl_tx.send(fwd).await.is_err() {
                break;
            }
        }
        let _ = cl_tx.close().await;
    };
    tokio::select! {
        _ = client_to_up => {},
        _ = up_to_client => {},
    }
}

// ── Disk cache ──────────────────────────────────────────────────────────────

fn cache_key(path: &str) -> String {
    let mut h = Sha256::new();
    h.update(path.as_bytes());
    hex(&h.finalize())
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn cache_write(dir: &std::path::Path, path: &str, content_type: &str, body: &[u8]) {
    let key = cache_key(path);
    let _ = std::fs::write(dir.join(&key), body);
    let _ = std::fs::write(dir.join(format!("{key}.ct")), content_type);
}

fn cache_read(dir: &std::path::Path, path: &str) -> Option<(String, Vec<u8>)> {
    let key = cache_key(path);
    let body = std::fs::read(dir.join(&key)).ok()?;
    let ct = std::fs::read_to_string(dir.join(format!("{key}.ct")))
        .unwrap_or_else(|_| "application/octet-stream".into());
    Some((ct, body))
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn cached(content_type: String, body: Vec<u8>) -> Response {
    let mut headers = HeaderMap::new();
    if let Ok(v) = HeaderValue::from_str(&content_type) {
        headers.insert(header::CONTENT_TYPE, v);
    }
    if content_type.contains("text/html") {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    let body = inject_standalone_css(&content_type, body);
    build_response(StatusCode::OK, headers, body)
}

/// Native app/document windows should look like standalone apps: the host shell
/// tags its sidebar / left+right rails / right panel with `data-app-chrome`. We
/// hide them by injecting a `<style>` into the served shell HTML (the CSS applies
/// to the elements React creates at runtime). The top header is kept.
fn inject_standalone_css(content_type: &str, body: Vec<u8>) -> Vec<u8> {
    if !content_type.contains("text/html") {
        return body;
    }
    const HEAD: &[u8] = b"</head>";
    const STYLE: &[u8] = b"<style>[data-app-chrome]{display:none !important}</style></head>";
    match body.windows(HEAD.len()).position(|w| w == HEAD) {
        Some(pos) => {
            let mut out = Vec::with_capacity(body.len() + STYLE.len());
            out.extend_from_slice(&body[..pos]);
            out.extend_from_slice(STYLE);
            out.extend_from_slice(&body[pos + HEAD.len()..]);
            out
        }
        None => body,
    }
}

fn build_response(status: StatusCode, headers: HeaderMap, body: Vec<u8>) -> Response {
    let mut resp = Response::builder().status(status);
    if let Some(h) = resp.headers_mut() {
        *h = headers;
    }
    resp.body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn wants_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|a| a.contains("text/html"))
        .unwrap_or(false)
}

/// True for paths that are static assets or API/collab (i.e. NOT SPA navigations).
fn is_asset_or_api(path: &str) -> bool {
    path.starts_with("/assets/")
        || path.starts_with("/shared/")
        || path.starts_with("/modules/")
        || path.starts_with("/api/")
        || path.starts_with("/collab/")
        || path == "/favicon.svg"
        || path == "/office-logo.svg"
}

fn is_cacheable_asset(path: &str) -> bool {
    path == "/"
        || path.starts_with("/assets/")
        || path.starts_with("/shared/")
        || path.starts_with("/modules/")
        || path == "/favicon.svg"
        || path == "/office-logo.svg"
}

/// Read-only API endpoints worth caching so the editor can mount offline.
fn is_cacheable_api(path: &str) -> bool {
    path == "/api/v1/modules" || path == "/api/v1/me" || path.starts_with("/api/v1/office")
}

fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
    )
}
