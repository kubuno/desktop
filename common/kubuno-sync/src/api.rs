//! HTTP client against the Kubuno core.
//!
//! The file sync never holds a refresh token (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §17, "two token owners"):
//! every access token comes from the process's [`crate::tokens::TokenProvider`], which is the shell's own
//! `TokenOwner` inside the shell and the shell's token broker in any other program (the CLI, chat, documents).
//! Requests retry once after a 401 with the token the provider hands out after it (the owner refreshes at most
//! once for any number of concurrent 401s).

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// Why a token could not be obtained, so the UI can tell a real logout from a blip.
///
/// - `Genuine`: the session is over (the core rejected the refresh token, the account was signed out, or the
///   instance belongs to no signed-in account): the user must sign in again. Nothing local is deleted.
/// - `Transient`: a network error, timeout, rate-limit (429), 5xx, or the shell's broker not reachable — **no new
///   token was issued and the session is still valid**. We must NOT tell the user "session expired"; just retry.
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

/// The `User-Agent` every request from this client carries.
///
/// Without it, reqwest sends its own default and the server logged our uploads
/// against a blank UA — the sync burst that started this whole thread could
/// only be pinned to the desktop client by its IP. Every other Kubuno client
/// announces itself (`Kubuno-Maps/…`, `Kubuno-Build/…`, the mobile app's
/// okhttp), so this one does too. The OS is read at runtime rather than hard-
/// coded, so a build of this crate on the server side reports honestly.
fn user_agent() -> String {
    format!("Kubuno-Desktop-Sync/{} ({})", env!("CARGO_PKG_VERSION"), std::env::consts::OS)
}

/// Build an HTTP client with the given timeout, applying the configured outbound
/// proxy (if any) so instances reachable only through a proxy still work.
fn build_http_client_timeout(secs: u64) -> reqwest::blocking::Client {
    let mut builder = reqwest::blocking::Client::builder()
        .user_agent(user_agent())
        .timeout(std::time::Duration::from_secs(secs));
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

/// Whose tokens a client uses.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Principal {
    /// A file-sync instance (`instances/<id>`): its account is resolved through the provider.
    Instance(String),
    /// An account key (an app that has no file-sync instance, such as chat).
    Account(String),
}

pub struct Api {
    http:      reqwest::blocking::Client,
    base:      String,
    principal: Principal,
    /// The account key, once resolved.
    account:   Option<String>,
    /// The access token in use (empty until the first request).
    access:    String,
}

/// The reply of [`Api::request`], whatever its status.
#[derive(Clone, Debug, Default)]
pub struct RawResponse {
    pub status:  u16,
    /// Header names in lower case, as `reqwest` gives them.
    pub headers: Vec<(String, String)>,
    pub body:    Vec<u8>,
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
    /// Server user id (UUID).
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
    /// Storage, for the desktop header's gauge — the same two numbers the web
    /// header shows. Absent on an older server, hence the defaults.
    #[serde(default)]
    pub used_bytes:   u64,
    #[serde(default)]
    pub quota_bytes:  u64,
    /// A free-form JSON bag the server never inspects. The waffle's favourites
    /// live here (`waffle_favorites`), which is what keeps desktop and web
    /// showing the same list.
    #[serde(default)]
    pub preferences:  serde_json::Value,
}

/// What `GET /api/v1/me` says the account may do. The desktop only needs the
/// one flag that decides whether the console is reachable at all; the full
/// privilege list stays on the server, which enforces it regardless.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct Privileges {
    #[serde(default)]
    pub is_admin:     bool,
    #[serde(default)]
    pub is_superuser: bool,
}

/// One cross-module label, as `GET /api/v1/labels` returns it.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct Label {
    pub id:    String,
    pub name:  String,
    /// Hex, e.g. `#1e8e3e`.
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// How many items carry it, across every module.
    #[serde(default)]
    pub link_count:  i64,
    #[serde(default)]
    pub share_count: i64,
    #[serde(default)]
    pub is_owner:    bool,
    #[serde(default)]
    pub can_manage:  bool,
    #[serde(default)]
    pub owner_name:  Option<String>,
}

/// One directory entry, as the console's user list returns it.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct AdminUser {
    pub id:    String,
    pub email: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub username:  Option<String>,
    #[serde(default)]
    pub role:      Option<String>,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub used_bytes:  u64,
    #[serde(default)]
    pub quota_bytes: u64,
    #[serde(default)]
    pub last_login_at: Option<String>,
    #[serde(default)]
    pub org_unit_id:   Option<String>,
}

/// One organisational unit — used both to resolve a user's unit id to its name
/// and to draw the units tree (its parent and description).
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct OrgUnit {
    pub id:          String,
    #[serde(default)]
    pub name:        String,
    #[serde(default)]
    pub parent_id:   Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// One user group, as `GET /api/v1/admin/groups` returns it (with its member
/// count folded in).
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct AdminGroup {
    pub id:           String,
    #[serde(default)]
    pub name:         String,
    #[serde(default)]
    pub description:  Option<String>,
    #[serde(default)]
    pub permissions:  Vec<String>,
    #[serde(default)]
    pub is_default:   bool,
    #[serde(default)]
    pub is_system:    bool,
    #[serde(default)]
    pub member_count: i64,
    #[serde(default)]
    pub created_at:   Option<String>,
}

/// One target audience, as `GET /api/v1/admin/audiences` returns it.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct Audience {
    pub id:           String,
    #[serde(default)]
    pub name:         String,
    #[serde(default)]
    pub description:  Option<String>,
    /// The seeded « everyone » audience: no explicit members, never deletable.
    #[serde(default)]
    pub is_everyone:  bool,
    /// Entries added by hand.
    #[serde(default)]
    pub member_count: i64,
    /// Distinct active accounts those entries resolve to.
    #[serde(default)]
    pub reach:        i64,
    /// How many (unit × module) pairs offer this audience.
    #[serde(default)]
    pub applied_to:   i64,
}

/// One installed module, as `GET /api/v1/admin/modules` returns it.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct AdminModule {
    pub id:           String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description:  Option<String>,
    /// A themed-icon / Lucide name the desktop can resolve.
    #[serde(default)]
    pub icon:         Option<String>,
    #[serde(default)]
    pub is_enabled:   bool,
    #[serde(default)]
    pub version:      Option<String>,
    #[serde(default)]
    pub installed_at: Option<String>,
}

/// One instance setting. `value` stays raw JSON — a setting is a bool, a number
/// or a string depending on the key, and the console shows them all.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct AdminSetting {
    pub key:      String,
    #[serde(default)]
    pub label:    Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub value:    serde_json::Value,
    #[serde(default)]
    pub is_public: bool,
}

/// One storage category of the overview.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct StorageCategory {
    pub category:     String,
    #[serde(default)]
    pub used_bytes:   u64,
    #[serde(default)]
    pub object_count: i64,
    #[serde(default)]
    pub accounts:     i64,
    #[serde(default)]
    pub billable:     bool,
    #[serde(default)]
    pub held:         bool,
}

/// The filesystem volume the instance's data sits on — the only hard ceiling.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct StorageVolume {
    #[serde(default)]
    pub path:            String,
    #[serde(default)]
    pub total_bytes:     u64,
    #[serde(default)]
    pub available_bytes: u64,
    #[serde(default)]
    pub used_bytes:      u64,
}

/// How the accounts stand against their quotas.
#[derive(Deserialize, Serialize, Clone, Copy, Default)]
pub struct QuotaStates {
    #[serde(default)]
    pub ok:   i64,
    #[serde(default)]
    pub near: i64,
    #[serde(default)]
    pub full: i64,
}

/// One organisational unit's slice of the used space.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct UnitUsage {
    #[serde(default)]
    pub unit_id:    Option<String>,
    #[serde(default)]
    pub unit_name:  Option<String>,
    #[serde(default)]
    pub accounts:   i64,
    #[serde(default)]
    pub used_bytes: u64,
}

/// The storage overview (`GET /api/v1/admin/storage/overview`).
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct StorageOverview {
    #[serde(default)]
    pub accounts:        i64,
    /// Sum of every account's quota.
    #[serde(default)]
    pub allocated_bytes: u64,
    /// Total bytes in use across the instance (the server's own top-level
    /// figure, not a client-side sum).
    #[serde(default)]
    pub used_bytes:      u64,
    /// The physical volume, when the server reports one.
    #[serde(default)]
    pub volume:          Option<StorageVolume>,
    /// The account-state tally (ok / near / full).
    #[serde(default)]
    pub quota_states:    QuotaStates,
    /// The split by organisational unit.
    #[serde(default)]
    pub by_unit:         Vec<UnitUsage>,
    /// The fill ratio at which an account is reported « near » its limit.
    #[serde(default)]
    pub warn_percent:    i64,
    /// The per-category breakdown, lifted out of the server's `by_module`.
    #[serde(default)]
    pub categories:      Vec<StorageCategory>,
}

/// One page of the directory, with the total the pager needs.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct AdminUsers {
    #[serde(default)]
    pub users:  Vec<AdminUser>,
    #[serde(default)]
    pub total:  i64,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub limit:  i64,
}

/// Percent-encodes a query value. Only what a search box can contain needs
/// escaping here, so this stays a few lines rather than a dependency.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Deserialize)]
struct MeEnvelope {
    user: User,
    /// Absent on an older server, where nobody gets the console.
    #[serde(default)]
    privileges: Privileges,
}

impl Api {
    /// A client for file-sync instance `id` on server `base`.
    pub fn new(id: String, base: String) -> Self {
        Self { http: build_http_client(), base, principal: Principal::Instance(id), account: None, access: String::new() }
    }

    /// A client for account `account` (its key) on server `base`.
    pub fn for_account(account: String, base: String) -> Self {
        Self { http: build_http_client(), base, principal: Principal::Account(account.clone()), account: Some(account), access: String::new() }
    }

    /// The account key, resolved once through the provider.
    fn account(&mut self) -> Result<String> {
        if let Some(a) = &self.account {
            return Ok(a.clone());
        }
        let a = match &self.principal {
            Principal::Account(a) => a.clone(),
            Principal::Instance(id) => crate::tokens::provider()?.account_of_instance(id)?,
        };
        self.account = Some(a.clone());
        Ok(a)
    }

    /// The bearer token for the next request (borrowed from the provider when none is held yet).
    fn bearer(&mut self) -> Result<String> {
        if self.access.is_empty() {
            let account = self.account()?;
            self.access = crate::tokens::provider()?.access_token(&account)?;
        }
        Ok(self.access.clone())
    }

    /// The server rejected the current token: take the one the provider hands out after a 401 (the owner
    /// refreshes once for any number of callers).
    fn refresh(&mut self) -> Result<()> {
        let account = self.account()?;
        let failed = std::mem::take(&mut self.access);
        self.access = crate::tokens::provider()?.after_unauthorized(&account, &failed)?;
        Ok(())
    }

    /// A valid access token (the WebSocket URL carries one).
    pub fn access_token(&mut self) -> Result<String> {
        self.bearer()
    }

    /// GET with a single auto-refresh retry on 401.
    fn get(&mut self, path: &str) -> Result<reqwest::blocking::Response> {
        let url = format!("{}{}", self.base, path);
        let token = self.bearer()?;
        let resp = self.http.get(&url).bearer_auth(&token).send()?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            let token = self.bearer()?;
            return Ok(self.http.get(&url).bearer_auth(&token).send()?);
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

    /// The instance's activated modules (`GET /api/v1/modules`), as raw JSON —
    /// the shape (envelope or bare array) is the caller's business.
    pub fn modules(&mut self) -> Result<serde_json::Value> {
        let resp = self.get("/api/v1/modules")?;
        if !resp.status().is_success() {
            bail!("récupération des modules : HTTP {}", resp.status());
        }
        Ok(resp.json()?)
    }

    /// A generic authenticated GET returning raw JSON, for module routes the
    /// core proxies (e.g. `/api/v1/chat/*`). Same auto-refresh as [`Self::get`];
    /// the caller owns the response shape.
    pub fn get_json(&mut self, path: &str) -> Result<serde_json::Value> {
        let resp = self.get(path)?;
        if !resp.status().is_success() {
            bail!("GET {path} : HTTP {}", resp.status());
        }
        Ok(resp.json().unwrap_or(serde_json::Value::Null))
    }

    /// A generic authenticated POST of a JSON body, returning the JSON reply (or
    /// `Null` when the server answers with no body). One auto-refresh retry on
    /// 401, like the label writes.
    pub fn post_json(&mut self, path: &str, body: serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("{}{}", self.base, path);
        let attempt = |http: &reqwest::blocking::Client, token: &str| {
            http.post(&url).bearer_auth(token).json(&body).send()
        };
        let mut resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        }
        if !resp.status().is_success() {
            bail!("POST {path} : HTTP {}", resp.status());
        }
        Ok(resp.json().unwrap_or(serde_json::Value::Null))
    }

    /// A generic authenticated request that keeps what the other helpers throw away: any method, the
    /// caller's own headers (`If-Match`, `Idempotency-Key`…), and the reply's status, body and headers
    /// whatever the status — a 412 or a 413 must reach the caller as such, not as an error string.
    /// One auto-refresh retry on 401, like the other helpers; transport failures are errors.
    pub fn request(&mut self, method: &str, path: &str, headers: &[(String, String)], body: Option<Vec<u8>>) -> Result<RawResponse> {
        let url = format!("{}{}", self.base, path);
        let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| anyhow::anyhow!("méthode HTTP invalide : {e}"))?;
        let attempt = |http: &reqwest::blocking::Client, token: &str| {
            let mut req = http.request(method.clone(), &url).bearer_auth(token);
            for (k, v) in headers {
                req = req.header(k.as_str(), v.as_str());
            }
            if let Some(b) = &body {
                req = req.header(reqwest::header::CONTENT_TYPE, "application/json").body(b.clone());
            }
            req.send()
        };
        let mut resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        }
        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_string(), v.to_string())))
            .collect();
        let body = resp.bytes()?.to_vec();
        Ok(RawResponse { status, headers, body })
    }

    /// Merges keys into the user's preferences (`PATCH /api/v1/me`).
    ///
    /// The server stores `preferences` as a free JSONB column and merges with
    /// `||`, a FIRST-LEVEL merge: sending one key leaves the others untouched,
    /// so there is no need to read-modify-write the whole object.
    pub fn patch_preferences(&mut self, patch: serde_json::Value) -> Result<()> {
        let url = format!("{}/api/v1/me", self.base);
        let body = serde_json::json!({ "preferences": patch });
        let token = self.bearer()?;
        let resp = self
            .http
            .patch(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()?;
        // One refresh retry, like `get`.
        let resp = if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            let token = self.bearer()?;
            self.http
                .patch(&url)
                .bearer_auth(&token)
                .json(&body)
                .send()?
        } else {
            resp
        };
        if !resp.status().is_success() {
            bail!("enregistrement des préférences : HTTP {}", resp.status());
        }
        Ok(())
    }

    /// An authenticated GET returning raw bytes — the profile photo, whose URL
    /// the server hands out as a path rather than as data.
    pub fn get_bytes(&mut self, path: &str) -> Result<Vec<u8>> {
        let resp = self.get(path)?;
        if !resp.status().is_success() {
            bail!("téléchargement : HTTP {}", resp.status());
        }
        Ok(resp.bytes()?.to_vec())
    }

    /// Fetch the authenticated user's profile (`GET /api/v1/me`).
    pub fn me(&mut self) -> Result<(User, Privileges)> {
        let resp = self.get("/api/v1/me")?;
        if !resp.status().is_success() {
            bail!("récupération du profil : HTTP {}", resp.status());
        }
        let env: MeEnvelope = resp.json()?;
        Ok((env.user, env.privileges))
    }

    /// The account's cross-module labels (`GET /api/v1/labels`).
    pub fn labels(&mut self) -> Result<Vec<Label>> {
        #[derive(Deserialize)]
        struct Envelope {
            #[serde(default)]
            labels: Vec<Label>,
        }
        let resp = self.get("/api/v1/labels")?;
        if !resp.status().is_success() {
            bail!("étiquettes : HTTP {}", resp.status());
        }
        Ok(resp.json::<Envelope>()?.labels)
    }

    /// Creates a label (`POST /api/v1/labels`).
    pub fn create_label(&mut self, name: &str, color: &str) -> Result<()> {
        let body = serde_json::json!({ "name": name, "color": color });
        let url = format!("{}/api/v1/labels", self.base);
        let attempt = |http: &reqwest::blocking::Client, token: &str| {
            http.post(&url).bearer_auth(token).json(&body).send()
        };
        let mut resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        }
        if !resp.status().is_success() {
            bail!("création d'étiquette : HTTP {}", resp.status());
        }
        Ok(())
    }

    /// Renames or recolours a label (`PATCH /api/v1/labels/:id`). Only the
    /// fields given are sent, so a rename never clears the colour.
    pub fn update_label(
        &mut self,
        id: &str,
        name: Option<&str>,
        color: Option<&str>,
    ) -> Result<()> {
        let mut body = serde_json::Map::new();
        if let Some(n) = name {
            body.insert("name".into(), serde_json::Value::from(n));
        }
        if let Some(c) = color {
            body.insert("color".into(), serde_json::Value::from(c));
        }
        let url = format!("{}/api/v1/labels/{id}", self.base);
        let value = serde_json::Value::Object(body);
        let attempt = |http: &reqwest::blocking::Client, token: &str| {
            http.patch(&url).bearer_auth(token).json(&value).send()
        };
        let mut resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        }
        if !resp.status().is_success() {
            bail!("modification d'étiquette : HTTP {}", resp.status());
        }
        Ok(())
    }

    /// Deletes a label (`DELETE /api/v1/labels/:id`). Already gone is the goal
    /// state, so a 404 is not an error.
    pub fn delete_label(&mut self, id: &str) -> Result<()> {
        let url = format!("{}/api/v1/labels/{id}", self.base);
        let attempt =
            |http: &reqwest::blocking::Client, token: &str| http.delete(&url).bearer_auth(token).send();
        let mut resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        }
        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::NOT_FOUND {
            bail!("suppression d'étiquette : HTTP {}", resp.status());
        }
        Ok(())
    }

    /// Instance-wide aggregates for the console's dashboard
    /// (`GET /api/v1/admin/stats`). Left as raw JSON: the server keeps adding
    /// series, and a strict struct here would drop what it does not know.
    pub fn admin_stats(&mut self) -> Result<serde_json::Value> {
        let resp = self.get("/api/v1/admin/stats")?;
        if !resp.status().is_success() {
            bail!("statistiques : HTTP {}", resp.status());
        }
        Ok(resp.json()?)
    }

    /// Installed modules (`GET /api/v1/admin/modules`).
    pub fn admin_modules(&mut self) -> Result<Vec<AdminModule>> {
        #[derive(Deserialize)]
        struct Env {
            #[serde(default)]
            modules: Vec<AdminModule>,
        }
        let resp = self.get("/api/v1/admin/modules")?;
        if !resp.status().is_success() {
            bail!("modules : HTTP {}", resp.status());
        }
        Ok(resp.json::<Env>()?.modules)
    }

    /// Enables or disables a module (`PATCH /api/v1/admin/modules/:id`).
    pub fn set_module_enabled(&mut self, id: &str, enabled: bool) -> Result<()> {
        let body = serde_json::json!({ "is_enabled": enabled });
        let url = format!("{}/api/v1/admin/modules/{id}", self.base);
        let attempt = |http: &reqwest::blocking::Client, token: &str| {
            http.patch(&url).bearer_auth(token).json(&body).send()
        };
        let mut resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        }
        if !resp.status().is_success() {
            bail!("module {id} : HTTP {}", resp.status());
        }
        Ok(())
    }

    /// Instance settings (`GET /api/v1/admin/settings`).
    pub fn admin_settings(&mut self) -> Result<Vec<AdminSetting>> {
        #[derive(Deserialize)]
        struct Env {
            #[serde(default)]
            settings: Vec<AdminSetting>,
        }
        let resp = self.get("/api/v1/admin/settings")?;
        if !resp.status().is_success() {
            bail!("paramètres : HTTP {}", resp.status());
        }
        Ok(resp.json::<Env>()?.settings)
    }

    /// The path the instance opens on — the `navigation.default_module` setting,
    /// e.g. `/drive`. `None` when unset. The web reads the same key to badge the
    /// default module in the list.
    pub fn admin_default_module(&mut self) -> Result<Option<String>> {
        let settings = self.admin_settings()?;
        Ok(settings
            .into_iter()
            .find(|s| s.key == "navigation.default_module")
            .and_then(|s| s.value.as_str().map(str::to_owned))
            .filter(|v| !v.is_empty()))
    }

    /// The storage overview (`GET /api/v1/admin/storage/overview`).
    ///
    /// The server nests the per-category breakdown under `by_module.categories`
    /// and reports the used total at the top level as `used_bytes` — flattened
    /// here so the console gets one tidy struct.
    pub fn admin_storage(&mut self) -> Result<StorageOverview> {
        #[derive(Deserialize, Default)]
        struct RawByModule {
            #[serde(default)]
            categories: Vec<StorageCategory>,
        }
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            accounts: i64,
            #[serde(default)]
            allocated_bytes: u64,
            #[serde(default)]
            used_bytes: u64,
            #[serde(default)]
            volume: Option<StorageVolume>,
            #[serde(default)]
            quota_states: QuotaStates,
            #[serde(default)]
            by_unit: Vec<UnitUsage>,
            #[serde(default)]
            warn_percent: i64,
            #[serde(default)]
            by_module: RawByModule,
        }
        let resp = self.get("/api/v1/admin/storage/overview")?;
        if !resp.status().is_success() {
            bail!("stockage : HTTP {}", resp.status());
        }
        let raw: Raw = resp.json()?;
        Ok(StorageOverview {
            accounts: raw.accounts,
            allocated_bytes: raw.allocated_bytes,
            used_bytes: raw.used_bytes,
            volume: raw.volume,
            quota_states: raw.quota_states,
            by_unit: raw.by_unit,
            warn_percent: raw.warn_percent,
            categories: raw.by_module.categories,
        })
    }

    /// The user groups (`GET /api/v1/admin/groups`).
    pub fn admin_groups(&mut self) -> Result<Vec<AdminGroup>> {
        #[derive(Deserialize)]
        struct Env {
            #[serde(default)]
            groups: Vec<AdminGroup>,
        }
        let resp = self.get("/api/v1/admin/groups")?;
        if !resp.status().is_success() {
            bail!("groupes : HTTP {}", resp.status());
        }
        Ok(resp.json::<Env>()?.groups)
    }

    /// The target audiences (`GET /api/v1/admin/audiences`).
    pub fn admin_audiences(&mut self) -> Result<Vec<Audience>> {
        #[derive(Deserialize)]
        struct Env {
            #[serde(default)]
            audiences: Vec<Audience>,
        }
        let resp = self.get("/api/v1/admin/audiences")?;
        if !resp.status().is_success() {
            bail!("audiences : HTTP {}", resp.status());
        }
        Ok(resp.json::<Env>()?.audiences)
    }

    /// The organisational units (`GET /api/v1/admin/org-units`).
    pub fn admin_org_units(&mut self) -> Result<Vec<OrgUnit>> {
        #[derive(Deserialize)]
        struct Env {
            #[serde(default)]
            org_units: Vec<OrgUnit>,
        }
        let resp = self.get("/api/v1/admin/org-units")?;
        if !resp.status().is_success() {
            bail!("unités : HTTP {}", resp.status());
        }
        Ok(resp.json::<Env>()?.org_units)
    }

    /// Account counts per organisational unit (`GET /admin/users?limit=0&counts=true`).
    /// Returns each unit's OWN count; the subtree total is summed by the caller.
    pub fn admin_org_unit_counts(&mut self) -> Result<std::collections::HashMap<String, i64>> {
        #[derive(Deserialize)]
        struct Count {
            org_unit_id: String,
            #[serde(default)]
            count:       i64,
        }
        #[derive(Deserialize)]
        struct Env {
            #[serde(default)]
            org_unit_counts: Vec<Count>,
        }
        let resp = self.get("/api/v1/admin/users?limit=0&counts=true")?;
        if !resp.status().is_success() {
            bail!("effectifs : HTTP {}", resp.status());
        }
        Ok(resp
            .json::<Env>()?
            .org_unit_counts
            .into_iter()
            .map(|c| (c.org_unit_id, c.count))
            .collect())
    }

    /// One page of the directory (`GET /api/v1/admin/users`).
    pub fn admin_users(&mut self, offset: u32, limit: u32, query: &str) -> Result<AdminUsers> {
        let q = if query.trim().is_empty() {
            String::new()
        } else {
            format!("&q={}", urlencode(query.trim()))
        };
        let resp = self.get(&format!("/api/v1/admin/users?offset={offset}&limit={limit}{q}"))?;
        if !resp.status().is_success() {
            bail!("utilisateurs : HTTP {}", resp.status());
        }
        Ok(resp.json()?)
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
        let mut resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
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
        let mut resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
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
        let mut resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
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
    /// Moves a FOLDER to the server's trash. Its contents go with it, so a
    /// child already trashed answers 404 — which is the goal state, not an
    /// error, exactly as for a file.
    pub fn trash_folder(&mut self, folder_id: &str, idem: &str) -> Result<()> {
        let url = format!("{}/api/v1/drive/folders/{folder_id}/trash", self.base);
        let attempt = |http: &reqwest::blocking::Client, token: &str| {
            http.post(&url)
                .bearer_auth(token)
                .header("Idempotency-Key", idem)
                .send()
        };
        let mut resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        }
        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::NOT_FOUND {
            bail!("trash folder {folder_id} : HTTP {}", resp.status());
        }
        Ok(())
    }

    pub fn trash(&mut self, file_id: &str, idem: &str) -> Result<()> {
        let url = format!("{}/api/v1/drive/{file_id}/trash", self.base);
        let attempt = |http: &reqwest::blocking::Client, token: &str| {
            http.post(&url)
                .bearer_auth(token)
                .header("Idempotency-Key", idem)
                .send()
        };
        let mut resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.refresh()?;
            resp = { let token = self.bearer()?; attempt(&self.http, &token)? };
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
