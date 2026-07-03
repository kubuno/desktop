//! HTTP client against the Kubuno core.
//!
//! Uses the native auth flow (F1): logs in with `client_type=desktop` to obtain
//! the refresh token in the JSON body, and rotates it on every refresh. Read
//! requests auto-refresh once on a 401.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Creds;

/// Why a token refresh failed, so the UI can tell a real logout from a blip.
///
/// - `Genuine`: the core rejected the refresh token itself (401/403) — the
///   session is truly over and the user must reconnect.
/// - `Transient`: a network error, timeout, rate-limit (429) or 5xx — **no new
///   token was issued, so the existing refresh token is still valid**. We must
///   NOT tell the user "session expired"; just retry later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFailure {
    Genuine,
    Transient,
}

impl std::fmt::Display for AuthFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthFailure::Genuine => write!(f, "session rejetée — reconnexion nécessaire"),
            AuthFailure::Transient => write!(f, "rafraîchissement temporairement indisponible"),
        }
    }
}

impl std::error::Error for AuthFailure {}

/// Per-instance lock serializing token refreshes. Several `Api` values can exist
/// for the same instance (the background sync thread plus on-demand calls like
/// the account popup); without this they would refresh concurrently, each
/// rotating the refresh token and invalidating the other — eventually bricking
/// the session. The lock funnels refreshes so only one hits the network.
fn refresh_lock(id: &str) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    let map = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut m = map.lock().unwrap_or_else(|p| p.into_inner());
    m.entry(id.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
}

/// How long a successfully rotated access token is shared before rotating again.
/// The access token lives ~15 min server-side, so reusing it for 5 min is safe —
/// and it collapses a burst of independent refreshers (doc proxy, sync daemon,
/// connection probe, WASM update check…) into ONE network rotation. Without
/// this, every caller rotated its own token: ~6 rotations per app start, which
/// trips the core's `/auth/refresh` rate-limit (10/min) after a few restarts.
const FRESH_TTL: std::time::Duration = std::time::Duration::from_secs(300);

/// Per-instance cache of the last successfully rotated access token.
fn fresh_cache() -> &'static Mutex<HashMap<String, (String, std::time::Instant)>> {
    static C: OnceLock<Mutex<HashMap<String, (String, std::time::Instant)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// After a TRANSIENT refresh failure (rate-limit, 5xx, network), don't hit the
/// network again for this long — fail fast with `Transient` instead. Without it,
/// a sync cycle with many pending ops retries the refresh once per op, keeping
/// the core's 60 s rate-limit window permanently saturated (a self-sustaining
/// 429 loop). 45 s < the server window, so at most ~1 probe per window.
const REFRESH_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(45);

/// Per-instance timestamp of the last transient refresh failure.
fn refresh_cooldown() -> &'static Mutex<HashMap<String, std::time::Instant>> {
    static C: OnceLock<Mutex<HashMap<String, std::time::Instant>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Build an HTTP client with the given timeout, applying the configured outbound
/// proxy (if any) so instances reachable only through a proxy still work.
fn build_http_client_timeout(secs: u64) -> reqwest::blocking::Client {
    let mut builder =
        reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(secs));
    if let Some(url) = crate::config::proxy_url() {
        if let Ok(proxy) = reqwest::Proxy::all(&url) {
            builder = builder.proxy(proxy);
        }
    }
    builder.build().unwrap_or_default()
}

fn build_http_client() -> reqwest::blocking::Client {
    build_http_client_timeout(60)
}

/// Quick reachability check against the server's `/healthz` (proxy-aware,
/// short timeout). Used to show the connection state on the home page.
pub fn ping(base: &str) -> bool {
    let url = format!("{}/healthz", base.trim_end_matches('/'));
    build_http_client_timeout(5)
        .get(&url)
        .send()
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

pub struct Api {
    http:  reqwest::blocking::Client,
    base:  String,
    creds: Creds,
    /// Instance id — used to persist the rotated refresh token to the right
    /// per-instance `creds.json`.
    id:    String,
}

#[derive(Deserialize)]
struct NativeTokens {
    access_token:  String,
    refresh_token: String,
}

/// Delta response from `GET /api/v1/drive/sync/delta`.
#[derive(Deserialize)]
pub struct Delta {
    pub changes:  Vec<serde_json::Value>,
    pub cursor:   i64,
    pub has_more: bool,
}

/// The authenticated user's public profile, as returned by `GET /api/v1/me`
/// (wrapped in a `{ "user": { … } }` envelope). Only the fields the desktop UI
/// needs to identify the account are kept.
#[derive(Deserialize, Serialize, Clone)]
pub struct User {
    /// Server user id (UUID). Used to scope the local office WASM backend's data.
    #[serde(default)]
    pub id:           Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    pub email:        String,
    #[serde(default)]
    pub username:     Option<String>,
    /// Server-relative avatar path (e.g. `/api/v1/users/<id>/avatar`).
    #[serde(default)]
    pub avatar_url:   Option<String>,
}

#[derive(Deserialize)]
struct MeEnvelope {
    user: User,
}

impl Api {
    pub fn new(id: String, base: String, creds: Creds) -> Self {
        Self { http: build_http_client(), base, creds, id }
    }

    /// Authenticate and return the native token pair (refresh token in body).
    pub fn login(base: &str, login: &str, password: &str) -> Result<Creds> {
        let http = build_http_client();
        let resp = http
            .post(format!("{base}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "login": login,
                "password": password,
                "client_type": "desktop",
                "device_name": hostname(),
                "device_type": "desktop",
            }))
            .send()?;
        if !resp.status().is_success() {
            bail!("connexion échouée : HTTP {}", resp.status());
        }
        let t: NativeTokens = resp
            .json()
            .context("réponse de connexion inattendue (la 2FA n'est pas encore gérée par le client desktop)")?;
        Ok(Creds {
            access_token:  t.access_token,
            refresh_token: t.refresh_token,
        })
    }

    fn refresh(&mut self) -> Result<()> {
        // Serialize refreshes for this instance (see `refresh_lock`).
        let lock = refresh_lock(&self.id);
        let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());

        // A rotation from the last few minutes is still perfectly valid — adopt
        // it instead of rotating again (see `FRESH_TTL`). Collapses refresh storms.
        {
            let cache = fresh_cache().lock().unwrap_or_else(|p| p.into_inner());
            if let Some((token, at)) = cache.get(&self.id) {
                if at.elapsed() < FRESH_TTL && self.creds.access_token != *token {
                    self.creds.access_token = token.clone();
                    if let Ok(disk) = Creds::load(&self.id) {
                        self.creds.refresh_token = disk.refresh_token;
                    }
                    return Ok(());
                }
            }
        }

        // While we waited for the lock, another Api may have already refreshed
        // and saved a fresh token. Adopt it instead of rotating again (which
        // would invalidate the one just saved).
        if let Ok(disk) = Creds::load(&self.id) {
            if !disk.refresh_token.is_empty() && disk.refresh_token != self.creds.refresh_token {
                self.creds = disk;
                return Ok(());
            }
        }

        // Cooling down after a recent transient failure → fail fast, no network.
        {
            let cd = refresh_cooldown().lock().unwrap_or_else(|p| p.into_inner());
            if let Some(at) = cd.get(&self.id) {
                if at.elapsed() < REFRESH_COOLDOWN {
                    return Err(AuthFailure::Transient.into());
                }
            }
        }
        let note_transient = |id: &str| {
            refresh_cooldown()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(id.to_string(), std::time::Instant::now());
        };

        // A network error/timeout is transient — the session isn't rejected.
        let resp = match self
            .http
            .post(format!("{}/api/v1/auth/refresh", self.base))
            .json(&serde_json::json!({ "refresh_token": self.creds.refresh_token }))
            .send()
        {
            Ok(r) => r,
            Err(_e) => {
                note_transient(&self.id);
                return Err(AuthFailure::Transient.into());
            }
        };
        let status = resp.status();
        if status.is_success() {
            let t: NativeTokens = resp.json()?;
            self.creds.access_token = t.access_token;
            self.creds.refresh_token = t.refresh_token; // rotation
            self.creds.save(&self.id)?;
            // Share this rotation with every other caller for FRESH_TTL,
            // and clear any failure cooldown.
            fresh_cache()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(self.id.clone(), (self.creds.access_token.clone(), std::time::Instant::now()));
            refresh_cooldown()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&self.id);
            return Ok(());
        }
        // Only a rejected token (401/403) means the session is really over. A 429
        // (rate-limit) or 5xx issues no token → the refresh token stays valid, so
        // keep the session and retry later instead of forcing a reconnect.
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AuthFailure::Genuine.into());
        }
        note_transient(&self.id);
        Err(AuthFailure::Transient.into())
    }

    /// Force a token refresh and return the fresh access token. Used by the
    /// desktop document proxy to hand the web frontend a valid native token
    /// (rotating the refresh token on disk) without requiring a web login.
    pub fn refresh_access(&mut self) -> Result<String> {
        self.refresh()?;
        Ok(self.creds.access_token.clone())
    }

    /// GET with a single auto-refresh retry on 401.
    fn get(&mut self, path: &str) -> Result<reqwest::blocking::Response> {
        let url = format!("{}{}", self.base, path);
        let resp = self.http.get(&url).bearer_auth(&self.creds.access_token).send()?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            return Ok(self.http.get(&url).bearer_auth(&self.creds.access_token).send()?);
        }
        Ok(resp)
    }

    pub fn delta(&mut self, cursor: i64, limit: i64) -> Result<Delta> {
        let resp = self.get(&format!("/api/v1/drive/sync/delta?cursor={cursor}&limit={limit}"))?;
        if !resp.status().is_success() {
            bail!("récupération du delta : HTTP {}", resp.status());
        }
        Ok(resp.json()?)
    }

    /// Fetch the authenticated user's profile (`GET /api/v1/me`).
    pub fn me(&mut self) -> Result<User> {
        let resp = self.get("/api/v1/me")?;
        if !resp.status().is_success() {
            bail!("récupération du profil : HTTP {}", resp.status());
        }
        let env: MeEnvelope = resp.json()?;
        Ok(env.user)
    }

    pub fn download(&mut self, file_id: &str) -> Result<Vec<u8>> {
        let resp = self.get(&format!("/api/v1/drive/{file_id}/download"))?;
        if !resp.status().is_success() {
            bail!("téléchargement {file_id} : HTTP {}", resp.status());
        }
        Ok(resp.bytes()?.to_vec())
    }

    // ── Push (local → serveur) ─────────────────────────────────────────────────

    /// Replace a file's content. `if_match` enables conflict-safe push.
    pub fn put_content(
        &mut self,
        file_id: &str,
        data: Vec<u8>,
        if_match: Option<&str>,
        idem: &str,
    ) -> Result<PutResult> {
        let url = format!("{}/api/v1/drive/sync/file/{file_id}/content", self.base);
        let attempt = |http: &reqwest::blocking::Client, token: &str| {
            let mut req = http
                .put(&url)
                .bearer_auth(token)
                .header("Idempotency-Key", idem)
                .body(data.clone());
            if let Some(m) = if_match {
                req = req.header("If-Match", m);
            }
            req.send()
        };
        let mut resp = attempt(&self.http, &self.creds.access_token)?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            resp = attempt(&self.http, &self.creds.access_token)?;
        }
        if resp.status() == reqwest::StatusCode::PRECONDITION_FAILED {
            return Ok(PutResult::Conflict);
        }
        if !resp.status().is_success() {
            bail!("put_content {file_id} : HTTP {}", resp.status());
        }
        let v: serde_json::Value = resp.json()?;
        Ok(PutResult::Updated(v["etag"].as_str().map(|s| s.to_string())))
    }

    /// Upload a new file (multipart). Returns (server id, etag).
    pub fn upload(
        &mut self,
        folder_id: Option<&str>,
        name: &str,
        data: Vec<u8>,
        idem: &str,
    ) -> Result<(String, Option<String>)> {
        let url = format!("{}/api/v1/drive/upload", self.base);
        let attempt = |http: &reqwest::blocking::Client, token: &str| {
            let part = reqwest::blocking::multipart::Part::bytes(data.clone()).file_name(name.to_string());
            let mut form = reqwest::blocking::multipart::Form::new().part("file", part);
            if let Some(fid) = folder_id {
                form = form.text("folder_id", fid.to_string());
            }
            http.post(&url)
                .bearer_auth(token)
                .header("Idempotency-Key", idem)
                .multipart(form)
                .send()
        };
        let mut resp = attempt(&self.http, &self.creds.access_token)?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            resp = attempt(&self.http, &self.creds.access_token)?;
        }
        if !resp.status().is_success() {
            bail!("upload '{name}' : HTTP {}", resp.status());
        }
        let v: serde_json::Value = resp.json()?;
        let f = &v["file"];
        let id = f["id"].as_str().unwrap_or_default().to_string();
        let etag = f["content_hash"].as_str().map(|s| s.to_string());
        Ok((id, etag))
    }

    /// Create a folder. Returns (server id, materialized path). The idempotency
    /// key should be stable per target path so a retry dedups (drive 412/replay).
    pub fn create_folder(
        &mut self,
        parent_id: Option<&str>,
        name: &str,
        idem: &str,
    ) -> Result<(String, String)> {
        let url = format!("{}/api/v1/drive/folders", self.base);
        let attempt = |http: &reqwest::blocking::Client, token: &str| {
            let body = serde_json::json!({ "parent_id": parent_id, "name": name });
            http.post(&url)
                .bearer_auth(token)
                .header("Idempotency-Key", idem)
                .json(&body)
                .send()
        };
        let mut resp = attempt(&self.http, &self.creds.access_token)?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            resp = attempt(&self.http, &self.creds.access_token)?;
        }
        if !resp.status().is_success() {
            bail!("création dossier '{name}' : HTTP {}", resp.status());
        }
        let v: serde_json::Value = resp.json()?;
        let f = &v["folder"];
        Ok((
            f["id"].as_str().unwrap_or_default().to_string(),
            f["path"].as_str().unwrap_or_default().to_string(),
        ))
    }

    /// Move a file to the server trash (used for local deletions).
    pub fn trash(&mut self, file_id: &str, idem: &str) -> Result<()> {
        let url = format!("{}/api/v1/drive/{file_id}/trash", self.base);
        let attempt = |http: &reqwest::blocking::Client, token: &str| {
            http.post(&url)
                .bearer_auth(token)
                .header("Idempotency-Key", idem)
                .send()
        };
        let mut resp = attempt(&self.http, &self.creds.access_token)?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            resp = attempt(&self.http, &self.creds.access_token)?;
        }
        // A file already gone server-side (404) is fine — the goal state is reached.
        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::NOT_FOUND {
            bail!("trash {file_id} : HTTP {}", resp.status());
        }
        Ok(())
    }
}

/// Outcome of a conflict-safe content push.
pub enum PutResult {
    Updated(Option<String>), // new etag
    Conflict,                // server changed since base etag (HTTP 412)
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "kubuno-desktop".into())
}
