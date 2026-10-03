//! The typed client for `/api/v1/office/*`.
//!
//! # Facts this is built on, verified against the server
//!
//! * The content is **not** in the database. `office.documents` is metadata;
//!   the body is a gzipped `.kbdoc` JSON file in Drive.
//! * `GET /office/documents/:id` returns **no etag**. Only create, update and
//!   delta go through `doc_response()`.
//! * `GET /office/documents/delta` currently returns **500** — its `SELECT`
//!   omits `source_format` while the row type requires it. Reported to the
//!   office module owner. It is the only read-only source of etags, so until it
//!   is fixed the etag has to come from somewhere else.
//! * `PATCH` is capped at axum's **2 MiB** default (`DefaultBodyLimit` appears
//!   nowhere in `office/src`), and images live as base64 inside that same JSON.
//!   A 413 is therefore terminal, not retryable.
//! * `Idempotency-Key` replay is keyed on `(user_id, key)` and the request body
//!   is **never compared**. A key reused across two different snapshots returns
//!   200 with the first one's response and silently discards the second. The
//!   key is a UUID per **payload**, reused only for a byte-identical retry.
//!
//! # Re-verification, 2026-09-19 — two of those facts have moved
//!
//! The server was read again while implementing this module, and `office/src`
//! has since been fixed on both counts. Both corrections are encoded here as
//! *tolerance*, never as a new assumption:
//!
//! * `documents::get` now reads `etag, content_etag` and answers through
//!   `doc_response` (`office/src/handlers/documents.rs`, the `SELECT etag,
//!   content_etag` right after the access check). `join_editing` does the same.
//!   [`Fetched::etag`] therefore stays an `Option`: a Kubuno instance running
//!   the older build still answers without one, and the caller must cope either
//!   way. What must **not** happen is the client requiring an etag to save.
//! * `documents::delta`'s `SELECT` now lists `source_format`, with a comment
//!   naming the 500 it caused. [`Client::delta`] still refuses to fold a failure
//!   into an empty change set: a delta that cannot be read comes back as
//!   [`Error::DeltaUnavailable`], because "no changes" and "I could not ask" are
//!   opposite instructions to the caller.
//! * **C1's premise no longer holds either.** Both draft-promotion paths now
//!   rotate both tokens: `save_editing` does it and hands them back
//!   ([`SavedDraft`]), and `save_editing_internal` — the one `leave_editing`
//!   calls, i.e. the browser tab that joined, typed and closed — does the same,
//!   with a comment naming the silent overwrite it prevents. So on a current
//!   server `If-Match` *does* catch that scenario, with a 412.
//!
//!   [`Fetched::digest`] — SHA-256 over the content bytes as they arrived —
//!   stays all the same, for two reasons that are not hypothetical: an instance
//!   running the older build rotates nothing on promotion, and `content_etag`
//!   tracks *writes*, not *bytes*, so it cannot tell a real change from a
//!   re-save of identical content. The digest can, and it is the only check
//!   that answers the question the user cares about — did the document change?


//!
//! # What this module does and does not decide
//!
//! It knows the routes, the headers and the error mapping. It does not know
//! *when* to call anything: no retry loop, no autosave cadence, no conflict
//! resolution. Those live in [`super::session`], which is where a wrong decision
//! costs the user a paragraph rather than a round trip.
//!
//! # The transport
//!
//! Auth, instance discovery, token rotation, the process-wide refresh lock and
//! the shared-token cache all belong to `kubuno_desktop_sync` (the access token is
//! borrowed from the shell's broker) and are not reimplemented here. The routing
//! logic is written against [`Transport`]; the shipped [`SyncTransport`] goes
//! through `kubuno_desktop_sync::request`, which carries any method and every header
//! (`If-Match`, `Idempotency-Key` — neither may ever be dropped) and returns the
//! status whatever it is, so a 412 or a 413 is told from a 500.

use std::sync::Mutex;

use serde_json::Value;

// ── Route constants ──────────────────────────────────────────────────────────

/// Client-visible prefix. The core nests the module router under `/api/v1`,
/// strips `/api/v1`, then the module proxy strips `/office`, so a route written
/// `/documents/:id` in `office/src/router/mod.rs` is reached at
/// `/api/v1/office/documents/<id>`.
pub const OFFICE_PREFIX: &str = "/api/v1/office";

/// The instance configuration the editor reads is a **core** route, not an
/// office one: the web editor calls `/config` through the SDK's `/api/v1` base
/// (`office/frontend/src/useOfficeInstance.ts`), and the keys are namespaced
/// `office.<key>`. `/api/v1/office/config` does not exist.
pub const CORE_CONFIG_PATH: &str = "/api/v1/config";

/// Instance setting holding the editor's autosave cadence, in seconds. `0`
/// disables autosave. Declared in `office/module.toml`; default 30.
pub const AUTOSAVE_KEY: &str = "office.autosave_interval_s";

/// Fallback cadence when the instance does not publish [`AUTOSAVE_KEY`], the
/// same constant the web editor compiles in.
pub const AUTOSAVE_DEFAULT_S: u64 = 30;

/// The server's presence window: an editing session is live while its
/// `last_ping_at` is under two minutes old. Ping well inside it.
pub const PRESENCE_WINDOW_S: u64 = 120;

const HEADER_USER_AGENT: &str = "User-Agent";
const HEADER_IF_MATCH: &str = "If-Match";
const HEADER_IDEMPOTENCY: &str = "Idempotency-Key";

// ── Errors ───────────────────────────────────────────────────────────────────

/// Everything a call to the office API can answer that is not a document.
///
/// The variants exist to be *told apart*, which is the whole point: the caller
/// treats [`Error::TooLarge`] as terminal, [`Error::Precondition`] as a
/// conflict to show the user, and [`Error::Transport`] as worth another try.
/// Folding them into one string would make all three retry forever.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// 401 — the session is gone. `kubuno-sync` already retried once after
    /// refreshing, so reaching here means the refresh itself failed.
    Unauthorized,
    /// 403 `FORBIDDEN` — a collaborator without `edit`, or a route the server
    /// restricts to the owner (all four `editing/*` routes are owner-only).
    Forbidden,
    /// 403 `POLICY_DISABLED` — the instance switched the feature off.
    PolicyDisabled(String),
    /// 404 — no such document, or not ours. `delete` also answers 404 when the
    /// document is not trashed yet.
    NotFound,
    /// 409 `CONFLICT`.
    Conflict(String),
    /// 412 — `If-Match` did not match `documents.etag`, and **nothing was
    /// written**. Terminal until the user resolves it.
    Precondition,
    /// 413 — over the body limit (axum's 2 MiB default on the module's
    /// `Json<UpdateDocumentDto>` extractor). **Terminal.** Retrying is the
    /// worst possible response: the document can never fit, and the user keeps
    /// typing into something that will never save.
    TooLarge,
    /// 422 — `VALIDATION` or `CONVERSION_ERROR`. Note the server answers 422,
    /// never 400, for bad input.
    Validation(String),
    /// The delta feed could not be read. Kept apart from every other failure on
    /// purpose: an empty change set means "nothing moved", and this means "I
    /// could not ask" — a caller that confuses them stops syncing silently.
    DeltaUnavailable { status: u16, message: String },
    /// Any other non-2xx.
    Server { status: u16, code: String, message: String },
    /// Network, timeout, offline, token refresh failure.
    Transport(String),
    /// A 2xx whose body was not the shape the route documents.
    Malformed(String),
    /// The transport cannot make this call at all (see [`SyncTransport`]).
    Unsupported(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unauthorized => write!(f, "session expirée"),
            Error::Forbidden => write!(f, "accès refusé"),
            Error::PolicyDisabled(m) => write!(f, "désactivé par l'administrateur : {m}"),
            Error::NotFound => write!(f, "document introuvable"),
            Error::Conflict(m) => write!(f, "conflit : {m}"),
            Error::Precondition => write!(f, "le document a changé sur le serveur (412)"),
            Error::TooLarge => write!(f, "document trop volumineux pour être enregistré (413)"),
            Error::Validation(m) => write!(f, "requête invalide : {m}"),
            Error::DeltaUnavailable { status, message } => {
                write!(f, "flux delta indisponible (HTTP {status}) : {message}")
            }
            Error::Server { status, code, message } => {
                write!(f, "erreur serveur HTTP {status} ({code}) : {message}")
            }
            Error::Transport(m) => write!(f, "réseau : {m}"),
            Error::Malformed(m) => write!(f, "réponse inattendue : {m}"),
            Error::Unsupported(m) => write!(f, "appel impossible : {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    /// Whether trying the same request again can plausibly succeed.
    ///
    /// The two that must answer `false` even though they look like ordinary
    /// failures are [`Error::TooLarge`] (the payload will never fit) and
    /// [`Error::Precondition`] (the server state moved; resending the same
    /// bytes either fails again or overwrites someone's work).
    pub fn retryable(&self) -> bool {
        match self {
            Error::Transport(_) => true,
            Error::Server { status, .. } => *status >= 500,
            Error::DeltaUnavailable { status, .. } => *status >= 500,
            Error::TooLarge
            | Error::Precondition
            | Error::Unauthorized
            | Error::Forbidden
            | Error::PolicyDisabled(_)
            | Error::NotFound
            | Error::Conflict(_)
            | Error::Validation(_)
            | Error::Malformed(_)
            | Error::Unsupported(_) => false,
        }
    }

    /// Whether the user's work is now at risk and only a human choice can
    /// resolve it.
    pub fn needs_user_decision(&self) -> bool {
        matches!(self, Error::Precondition | Error::TooLarge | Error::Conflict(_))
    }

    /// Maps one HTTP status plus the module's error envelope
    /// (`{"error":CODE,"message":…}`) onto a variant.
    ///
    /// The status decides; the envelope only refines (403 splits on
    /// `POLICY_DISABLED`) and carries the human text. A body that is not JSON —
    /// axum's own 413, for instance, which never reaches the module's error
    /// type — must still map correctly, so the parse is best-effort.
    pub fn from_response(status: u16, body: &[u8]) -> Self {
        let parsed: Option<Value> = serde_json::from_slice(body).ok();
        let code = parsed
            .as_ref()
            .and_then(|v| v.get("error"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let message = parsed
            .as_ref()
            .and_then(|v| v.get("message"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        match status {
            401 => Error::Unauthorized,
            403 if code == "POLICY_DISABLED" => Error::PolicyDisabled(message),
            403 => Error::Forbidden,
            404 => Error::NotFound,
            409 => Error::Conflict(message),
            412 => Error::Precondition,
            413 => Error::TooLarge,
            422 => Error::Validation(message),
            _ => Error::Server { status, code, message },
        }
    }

    /// Recovers a status from the text `kubuno-sync` produces on a non-2xx
    /// (`bail!("GET {path} : HTTP {}", resp.status())`).
    ///
    /// Its generic helpers throw the response away and keep only that line, so
    /// until they return a status this is the only way a 412 or a 413 can be
    /// told from a 500. Anything unrecognised stays a transport failure, which
    /// is the safe reading: a request that never reached the server is exactly
    /// what a retry is for.
    pub fn from_sync_message(message: &str) -> Self {
        match status_in(message) {
            Some(status) => Error::from_response(status, b""),
            None => Error::Transport(message.to_string()),
        }
    }
}

/// Finds `HTTP <ddd>` in a message and returns the code.
///
/// `reqwest`'s `Display` for a status is `412 Precondition Failed`, so the
/// digits are followed by a space and a reason phrase — parsing must stop at
/// the first non-digit rather than read the rest of the line.
fn status_in(message: &str) -> Option<u16> {
    let at = message.find("HTTP ")?;
    let digits: String = message[at + 5..].chars().take_while(char::is_ascii_digit).collect();
    if digits.len() != 3 {
        return None;
    }
    digits.parse().ok()
}

/// Shorthand for this module.
pub type ApiResult<T> = Result<T, Error>;

// ── Transport ────────────────────────────────────────────────────────────────

/// The HTTP verbs the office document routes use.
///
/// `PATCH` is the save path, and `DELETE` is both the hard delete
/// (`/documents/:id/delete` — not `/documents/:id`) and `editing/leave`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Patch,
    Delete,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

/// One fully-built request: the exact path, the exact headers, the exact body.
///
/// Built by pure functions so the interesting part — URL escaping, the
/// idempotency key, the `If-Match` — is testable without a network.
#[derive(Clone, Debug)]
pub struct Request {
    pub method: Method,
    /// Server-relative, already percent-encoded, starting with `/api/v1/`.
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Value>,
}

impl Request {
    pub fn new(method: Method, path: impl Into<String>) -> Self {
        Self {
            method,
            path: path.into(),
            // Every Kubuno client announces itself; this one is
            // `Kubuno-Documents/<version>`, per the convention in
            // `kubuno-sync/src/api.rs`.
            headers: vec![(HEADER_USER_AGENT.to_string(), user_agent())],
            body: None,
        }
    }

    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_string(), value.into()));
        self
    }

    pub fn json(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    /// Case-insensitive lookup, because HTTP header names are.
    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The body as it goes on the wire.
    ///
    /// `serde_json`'s map is a `BTreeMap` here (`preserve_order` is off), so
    /// the same logical body always serialises to the same bytes — which is
    /// what makes the idempotency fingerprint mean anything.
    pub fn body_bytes(&self) -> Vec<u8> {
        match &self.body {
            Some(v) => serde_json::to_vec(v).unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// Headers whose loss changes what the server does, and which a transport
    /// must therefore refuse rather than drop.
    ///
    /// `If-Match` is the overwrite guard; `Idempotency-Key` is what keeps a
    /// retried create from making a second document. The `User-Agent` is not on
    /// this list: it is diagnostic, and failing every call over it would be
    /// worse than logging under the sync client's name.
    pub fn undroppable_headers(&self) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(k, _)| {
                k.eq_ignore_ascii_case(HEADER_IF_MATCH) || k.eq_ignore_ascii_case(HEADER_IDEMPOTENCY)
            })
            .map(|(k, _)| k.as_str())
            .collect()
    }
}

/// What came back. Kept as bytes so exports (DOCX/ODT) use the same path as
/// JSON routes.
#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(status: u16, body: &Value) -> Self {
        Self { status, body: serde_json::to_vec(body).unwrap_or_default() }
    }

    /// The body as JSON, or [`Error::Malformed`].
    pub fn parse(&self) -> ApiResult<Value> {
        if self.body.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&self.body)
            .map_err(|e| Error::Malformed(format!("JSON illisible : {e}")))
    }

    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// How a [`Client`] reaches the server. One method, so a test double is three
/// lines and every route's request can be inspected byte for byte.
pub trait Transport {
    fn send(&self, request: &Request) -> ApiResult<Response>;
}

/// The shipping transport: `kubuno_desktop_sync::request`, which owns the bearer token
/// (borrowed from the shell's broker), the refresh lock and the rotation.
pub struct SyncTransport {
    instance: String,
}

impl SyncTransport {
    pub fn new(instance: String) -> Self {
        Self { instance }
    }

    /// The request's own `User-Agent` (`Kubuno-Documents/<version>`) replaces the
    /// sync client's default one.
    pub const OVERRIDES_USER_AGENT: bool = true;
}

impl Transport for SyncTransport {
    fn send(&self, request: &Request) -> ApiResult<Response> {
        // `kubuno_desktop_sync::request` carries every header (`If-Match`, `Idempotency-Key`) and every method, and
        // hands back the status whatever it is: a 412 or a 413 reaches `Error::from_response` as such.
        // A POST without a body still sends `{}`: the office handlers that take no body run through axum,
        // and `create` has a `Json<CreateDocumentDto>` extractor that refuses `null`.
        let body = match (&request.body, request.method) {
            (Some(_), _) => Some(request.body_bytes()),
            (None, Method::Post) => Some(b"{}".to_vec()),
            (None, _) => None,
        };
        match kubuno_desktop_sync::request(&self.instance, request.method.as_str(), &request.path, &request.headers, body) {
            Ok(r) => Ok(Response { status: r.status, body: r.body }),
            Err(e) => Err(Error::Transport(e.to_string())),
        }
    }
}

/// The `User-Agent` every request from this app carries.
pub fn user_agent() -> String {
    format!("Kubuno-Documents/{}", env!("CARGO_PKG_VERSION"))
}

// ── Percent-encoding and path building ───────────────────────────────────────

/// Percent-encodes everything outside the unreserved set.
///
/// Used for path segments as well as query values: a document id arrives as a
/// `String` from the model layer, and an id containing `/`, `?` or `..` must
/// not be able to address a different route.
pub fn urlencode(s: &str) -> String {
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

/// `/api/v1/office/documents/<id><suffix>`, with the id escaped.
pub fn document_path(id: &str, suffix: &str) -> String {
    format!("{OFFICE_PREFIX}/documents/{}{suffix}", urlencode(id))
}

// ── Digest and idempotency ───────────────────────────────────────────────────

/// Lowercase hex SHA-256 of `bytes`.
///
/// Implemented here rather than pulled in: the crate's dependency list is
/// fixed, and the one thing this is used for — deciding whether two save
/// payloads are the same document — must not collide. A 64-bit hash would be
/// cheaper and would, on a collision, reuse an `Idempotency-Key` across two
/// different snapshots, which the server answers by discarding the second
/// silently. That is the exact failure this module exists to prevent.
pub fn digest_hex(bytes: &[u8]) -> String {
    hex(&sha256(bytes))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[rustfmt::skip]
const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 (FIPS 180-4), byte input only.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut message = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    let (blocks, _) = message.as_chunks::<64>();
    for block in blocks {
        let mut w = [0u32; 64];
        let (words, _) = block.as_chunks::<4>();
        for (i, word) in words.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
        let (mut e, mut f, mut g, mut hh) = (h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(v);
        }
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// A v4-shaped UUID, unique by construction.
///
/// There is no CSPRNG in this crate's dependency set, and an `Idempotency-Key`
/// does not need one: the server keys replay on `(user_id, key)`, so a guessed
/// key can only replay the guesser's own request. What it *does* need is never
/// to repeat — a repeat across two payloads makes the server answer the second
/// with the first's response and drop it. So the value is a SHA-256 over a
/// monotonic counter, the clock, the process id and a per-process hash seed,
/// stamped with the version and variant nibbles.
pub fn fresh_uuid_v4() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(n);
    hasher.write_u64(nanos);
    let seed = hasher.finish();

    let mut material = Vec::with_capacity(32);
    material.extend_from_slice(&n.to_le_bytes());
    material.extend_from_slice(&nanos.to_le_bytes());
    material.extend_from_slice(&seed.to_le_bytes());
    material.extend_from_slice(&(std::process::id() as u64).to_le_bytes());
    let d = sha256(&material);

    let mut b = [0u8; 16];
    b.copy_from_slice(&d[..16]);
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // RFC 4122 variant
    format!(
        "{}-{}-{}-{}-{}",
        hex(&b[0..4]),
        hex(&b[4..6]),
        hex(&b[6..8]),
        hex(&b[8..10]),
        hex(&b[10..16])
    )
}

/// Enforces the one rule about `Idempotency-Key` that the server does not.
///
/// The server keys replay on `(user_id, key)` and **never compares the body**:
/// the same key sent with two different snapshots returns 200 with the first
/// one's response and silently discards the second. So a key belongs to a
/// *payload*, not to a document or a session, and may be reused only for a
/// byte-identical retry of a request whose outcome is unknown.
///
/// Going back to an earlier payload mints a **third** key rather than reusing
/// the first: the first key's stored response is the server's answer to a write
/// that has since been superseded, and replaying it would return a stale etag
/// while writing nothing.
#[derive(Debug, Default)]
pub struct IdemGuard {
    current: Option<(String, String)>, // (payload digest, key)
}

impl IdemGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// The key to send for `payload`: the same one for identical bytes, a fresh
    /// one otherwise.
    pub fn key_for(&mut self, payload: &[u8]) -> String {
        let fingerprint = digest_hex(payload);
        if let Some((seen, key)) = &self.current {
            if *seen == fingerprint {
                return key.clone();
            }
        }
        let key = fresh_uuid_v4();
        self.current = Some((fingerprint, key.clone()));
        key
    }

    /// Forgets the in-flight payload, so the next call mints a fresh key even
    /// for identical bytes. Call it once a write is known to have landed: the
    /// next save of the same bytes is a *new* write, not a retry.
    pub fn settled(&mut self) {
        self.current = None;
    }
}

// ── Response shapes ──────────────────────────────────────────────────────────

/// One document's metadata, as the list returns it.
///
/// The list `SELECT` carries no etag on any branch — only `doc_response`
/// (create / fetch / update / join) and the delta feed do — so [`Summary::etag`]
/// is `None` out of [`Client::list`] and populated out of [`Client::delta`].
#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub updated: String,
    pub etag: Option<String>,
    pub icon: Option<String>,
    pub word_count: i64,
    pub starred: bool,
    pub trashed: bool,
    pub parent_id: Option<String>,
    /// Drive file holding the generated `.kbdoc`.
    pub file_id: Option<String>,
    /// The foreign file this document was imported from, if any. `None` means
    /// "save back to source" has nothing to write to.
    pub source_file_id: Option<String>,
    /// `docx` / `odt` / `doc` when imported, `None` for a document created here.
    pub source_format: Option<String>,
}

impl Summary {
    fn from_json(v: &Value) -> Self {
        Self {
            id: str_at(v, "id"),
            title: str_at(v, "title"),
            updated: str_at(v, "updated_at"),
            etag: opt_str(v, "etag"),
            icon: opt_str(v, "icon"),
            word_count: v.get("word_count").and_then(Value::as_i64).unwrap_or(0),
            starred: v.get("is_starred").and_then(Value::as_bool).unwrap_or(false),
            trashed: v.get("is_trashed").and_then(Value::as_bool).unwrap_or(false),
            parent_id: opt_str(v, "parent_id"),
            file_id: opt_str(v, "file_id"),
            source_file_id: opt_str(v, "source_file_id"),
            source_format: opt_str(v, "source_format"),
        }
    }
}

/// A document's content plus the tokens needed to write it back.
#[derive(Clone, Debug, Default)]
pub struct Fetched {
    /// The `content_json` value, serialised. Either a bare ProseMirror doc or
    /// the `{"_type":"multi-page", …}` envelope — the model layer decides.
    pub content: Vec<u8>,
    /// `documents.etag`, the **row** token `If-Match` compares against.
    ///
    /// `None` on an instance whose `documents::get` predates the fix that made
    /// it answer through `doc_response`. A `None` here is not an error and must
    /// not block a save: it means the first PATCH goes without `If-Match`.
    pub etag: Option<String>,
    /// `documents.content_etag`. **Not a content fingerprint**: it counts
    /// writes, not bytes. A re-save of identical content rotates it, and an
    /// instance predating the draft-promotion fix rewrites the bytes without
    /// rotating it at all. Compare [`Fetched::digest`] instead.
    pub content_etag: Option<String>,
    /// SHA-256 of [`Fetched::content`] as it arrived — the only value that
    /// actually tracks the bytes.
    pub digest: String,
    /// The whole `document` object, so nothing the server sends is lost to a
    /// struct that predates it.
    pub document: Value,
    pub title: String,
}

impl Fetched {
    fn from_envelope(v: &Value) -> ApiResult<Self> {
        let document = v
            .get("document")
            .cloned()
            .ok_or_else(|| Error::Malformed("`document` absent de la réponse".into()))?;
        let content_value = v
            .get("content_json")
            .cloned()
            .ok_or_else(|| Error::Malformed("`content_json` absent de la réponse".into()))?;
        let content = serde_json::to_vec(&content_value)
            .map_err(|e| Error::Malformed(format!("content_json non sérialisable : {e}")))?;
        Ok(Self {
            digest: digest_hex(&content),
            content,
            etag: opt_str(&document, "etag"),
            content_etag: opt_str(&document, "content_etag"),
            title: str_at(&document, "title"),
            document,
        })
    }

    pub fn id(&self) -> String {
        str_at(&self.document, "id")
    }
}

/// One live editor, as `editing/join` reports them. Liveness is a two-minute
/// window on `last_ping_at` ([`PRESENCE_WINDOW_S`]).
#[derive(Clone, Debug, Default)]
pub struct Editor {
    pub user_id: String,
    pub display_name: Option<String>,
    pub color: Option<String>,
    pub last_ping_at: String,
}

/// What `editing/join` returns: the document, plus who else is in it.
#[derive(Clone, Debug, Default)]
pub struct Joined {
    pub document: Fetched,
    pub editors: Vec<Editor>,
}

impl Joined {
    /// Editors other than us. The caller knows its own user id; the server does
    /// not filter.
    pub fn others(&self, me: &str) -> Vec<&Editor> {
        self.editors.iter().filter(|e| e.user_id != me).collect()
    }
}

/// What `editing/save` returns. Both tokens rotate there, so a client that
/// promoted its draft keeps a usable `If-Match` instead of one its own write
/// invalidated. Older instances answer `{"ok":true}` with no tokens, hence the
/// `Option`s.
#[derive(Clone, Debug, Default)]
pub struct SavedDraft {
    pub etag: Option<String>,
    pub content_etag: Option<String>,
    /// `true` when the server reported there was no draft to promote.
    pub no_draft: bool,
}

/// One entry of the delta feed.
#[derive(Clone, Debug)]
pub struct Change {
    pub id: String,
    /// `modified` | `trashed` | `deleted`.
    pub kind: String,
    pub etag: Option<String>,
    pub content_etag: Option<String>,
    pub change_seq: i64,
    /// Absent for a `deleted` change: a tombstone carries no document.
    pub summary: Option<Summary>,
    /// Present only when the call asked for `include=content`.
    pub content: Option<Vec<u8>>,
}

/// A page of the delta feed.
#[derive(Clone, Debug, Default)]
pub struct Delta {
    pub changes: Vec<Change>,
    pub cursor: i64,
    pub has_more: bool,
}

/// Which branch of `GET /documents` the server will take.
///
/// The branches are **exclusive**, tested in this order in
/// `documents::list`: `search` > `starred` > `shared` > `recent` > default. Only
/// `search` combines with `trashed`. Asking for `recent` *and* `starred` gets
/// starred, not "recently starred" — the reason this is a type rather than a
/// handful of booleans nobody reads the precedence of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListBranch {
    Search,
    Starred,
    Shared,
    /// `is_trashed = FALSE ORDER BY updated_at DESC`, and the server clamps the
    /// limit to 20 whatever was asked.
    Recent,
    /// Children of `parent_id` (root when absent), ordered by `position`.
    Folder,
}

/// The query `GET /api/v1/office/documents` accepts.
#[derive(Clone, Debug, Default)]
pub struct ListQuery {
    pub search: Option<String>,
    pub starred: bool,
    pub shared: bool,
    pub recent: bool,
    pub trashed: bool,
    pub parent_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl ListQuery {
    /// The most recently updated documents. The server caps this at 20 rows.
    pub fn recent() -> Self {
        Self { recent: true, ..Self::default() }
    }

    pub fn search(text: impl Into<String>) -> Self {
        Self { search: Some(text.into()), ..Self::default() }
    }

    /// Which branch the server will actually run.
    pub fn branch(&self) -> ListBranch {
        if self.search.is_some() {
            ListBranch::Search
        } else if self.starred {
            ListBranch::Starred
        } else if self.shared {
            ListBranch::Shared
        } else if self.recent {
            ListBranch::Recent
        } else {
            ListBranch::Folder
        }
    }

    /// The query string, without the leading `?`. Parameters are emitted in a
    /// fixed order so a request is reproducible, and every value is escaped:
    /// a search for `100 % coton` must not split the query.
    pub fn query_string(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(s) = &self.search {
            parts.push(format!("search={}", urlencode(s)));
        }
        if self.starred {
            parts.push("starred=true".into());
        }
        if self.shared {
            parts.push("shared=true".into());
        }
        if self.recent {
            parts.push("recent=true".into());
        }
        if self.trashed {
            parts.push("trashed=true".into());
        }
        if let Some(p) = &self.parent_id {
            parts.push(format!("parent_id={}", urlencode(p)));
        }
        if let Some(l) = self.limit {
            parts.push(format!("limit={l}"));
        }
        if let Some(o) = self.offset {
            parts.push(format!("offset={o}"));
        }
        parts.join("&")
    }

    pub fn path(&self) -> String {
        let q = self.query_string();
        if q.is_empty() {
            format!("{OFFICE_PREFIX}/documents")
        } else {
            format!("{OFFICE_PREFIX}/documents?{q}")
        }
    }
}

/// What a new document may be given at creation.
///
/// Everything is optional. The server defaults the title to
/// `Nouveau document.odt` and de-duplicates it per (owner, parent) —
/// `rapport.odt` becomes `rapport (2).odt` — so the title asked for is not
/// necessarily the title returned. Read it back from the response.
#[derive(Clone, Debug, Default)]
pub struct NewDocument {
    pub title: Option<String>,
    pub icon: Option<String>,
    pub parent_id: Option<String>,
    pub template_id: Option<String>,
}

impl NewDocument {
    pub fn titled(title: impl Into<String>) -> Self {
        Self { title: Some(title.into()), ..Self::default() }
    }

    fn body(&self) -> Value {
        let mut m = serde_json::Map::new();
        if let Some(t) = &self.title {
            m.insert("title".into(), Value::from(t.clone()));
        }
        if let Some(i) = &self.icon {
            m.insert("icon".into(), Value::from(i.clone()));
        }
        if let Some(p) = &self.parent_id {
            m.insert("parent_id".into(), Value::from(p.clone()));
        }
        if let Some(t) = &self.template_id {
            m.insert("template_id".into(), Value::from(t.clone()));
        }
        Value::Object(m)
    }
}

/// Export formats the server produces.
///
/// `odt` drops layout and headers/footers (`export_as_odt` ignores `_layout`
/// and `_hf`); `docx` keeps them, plus the envelope's comment threads. There is
/// no PDF route at all — the web renders PDF client-side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Docx,
    Odt,
}

impl ExportFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            ExportFormat::Docx => "docx",
            ExportFormat::Odt => "odt",
        }
    }
}

// ── The client ───────────────────────────────────────────────────────────────

/// Everything a request needs to identify itself.
pub struct Client {
    /// The Kubuno instance id `kubuno-sync` stores credentials under.
    pub instance: String,
    /// `Send + Sync`, so the client can be shared with the autosave thread
    /// instead of forcing every save onto the window's message loop.
    transport: Box<dyn Transport + Send + Sync>,
    /// One guard per client: a document is edited in one window at a time, and
    /// the guard's whole job is to notice when the payload changed.
    idem: Mutex<IdemGuard>,
}

impl Client {
    pub fn new(instance: String) -> Self {
        let transport = Box::new(SyncTransport::new(instance.clone()));
        Self { instance, transport, idem: Mutex::new(IdemGuard::new()) }
    }

    /// Same client over another transport — a test double, or the generic
    /// `PATCH` helper once `kubuno-sync` grows one.
    pub fn with_transport(instance: String, transport: Box<dyn Transport + Send + Sync>) -> Self {
        Self { instance, transport, idem: Mutex::new(IdemGuard::new()) }
    }

    /// The idempotency guard. A poisoned lock is recovered rather than
    /// panicked on: one thread dying mid-save must not make every later save
    /// impossible.
    fn idem(&self) -> std::sync::MutexGuard<'_, IdemGuard> {
        self.idem.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Sends and maps the status. Every route goes through here, so the error
    /// mapping is written once.
    fn call(&self, request: Request) -> ApiResult<Response> {
        let response = self.transport.send(&request)?;
        if response.ok() {
            Ok(response)
        } else {
            Err(Error::from_response(response.status, &response.body))
        }
    }

    fn call_json(&self, request: Request) -> ApiResult<Value> {
        self.call(request)?.parse()
    }

    // ── Listing ──────────────────────────────────────────────────────────────

    /// `GET /api/v1/office/documents` with an explicit query.
    pub fn list_query(&self, query: &ListQuery) -> ApiResult<Vec<Summary>> {
        let v = self.call_json(Request::new(Method::Get, query.path()))?;
        let rows = v
            .get("documents")
            .and_then(Value::as_array)
            // A response without the key is not an empty library: it is a
            // response we did not understand, and answering `vec![]` would show
            // the user an empty document list as if it were the truth.
            .ok_or_else(|| Error::Malformed("`documents` absent de la réponse".into()))?;
        Ok(rows.iter().map(Summary::from_json).collect())
    }

    /// The default listing (root folder, not trashed).
    pub fn list(&self) -> Result<Vec<Summary>, String> {
        self.list_query(&ListQuery::default()).map_err(|e| e.to_string())
    }

    /// The `recent=true` branch: latest first, capped at 20 rows by the server
    /// whatever `limit` asks for.
    pub fn recent(&self) -> ApiResult<Vec<Summary>> {
        self.list_query(&ListQuery::recent())
    }

    // ── Reading ──────────────────────────────────────────────────────────────

    /// `GET /api/v1/office/documents/:id`.
    ///
    /// Owner or collaborator. Side effect worth knowing: when the content is
    /// read from the main file, the server re-syncs the title from the Drive
    /// file name, so a rename made in the file explorer wins here.
    pub fn fetch_doc(&self, id: &str) -> ApiResult<Fetched> {
        let v = self.call_json(Request::new(Method::Get, document_path(id, "")))?;
        Fetched::from_envelope(&v)
    }

    pub fn fetch(&self, id: &str) -> Result<Fetched, String> {
        self.fetch_doc(id).map_err(|e| e.to_string())
    }

    // ── Writing ──────────────────────────────────────────────────────────────

    /// Builds the save request without sending it — so the headers can be
    /// tested, and so the caller can see the exact bytes that will be weighed
    /// against the 2 MiB limit.
    ///
    /// `if_match` is sent when known. It is free and it catches the REST path,
    /// but it is **not** the real precondition: `editing/leave` promotes a
    /// draft over the content file through `save_editing_internal` without the
    /// client ever calling `editing/save`, so bytes can move under a token that
    /// stays valid. The digest comparison in [`super::session`] is what
    /// protects the document.
    pub fn save_request(&self, id: &str, content: &[u8], if_match: Option<&str>) -> ApiResult<Request> {
        let content_json: Value = serde_json::from_slice(content)
            .map_err(|e| Error::Malformed(format!("contenu non JSON : {e}")))?;
        let mut body = serde_json::Map::new();
        body.insert("content_json".into(), content_json);
        let mut request = Request::new(Method::Patch, document_path(id, "")).json(Value::Object(body));
        if let Some(etag) = if_match {
            request = request.header(HEADER_IF_MATCH, etag);
        }
        let key = self.idem().key_for(&request.body_bytes());
        Ok(request.header(HEADER_IDEMPOTENCY, key))
    }

    /// `PATCH /api/v1/office/documents/:id` with `content_json` — the only save
    /// path for content.
    ///
    /// A 413 comes back as [`Error::TooLarge`] and is **terminal**: the payload
    /// will not shrink by being sent again.
    pub fn save_content(
        &self,
        id: &str,
        content: &[u8],
        if_match: Option<&str>,
    ) -> ApiResult<Fetched> {
        let request = self.save_request(id, content, if_match)?;
        let result = self.call_json(request).and_then(|v| Fetched::from_envelope(&v));
        if result.is_ok() {
            // The write landed: the next save of these same bytes is a new
            // write, not a retry, and must not replay this key.
            self.idem().settled();
        }
        result
    }

    /// Writes the content back. `if_match` is the etag we believe is current.
    pub fn save(&self, id: &str, content: &[u8], if_match: Option<&str>) -> Result<String, String> {
        let fetched = self.save_content(id, content, if_match).map_err(|e| e.to_string())?;
        fetched
            .etag
            .ok_or_else(|| "réponse de sauvegarde sans etag".to_string())
    }

    /// Renames the document. There is no rename route: a rename is a `PATCH`
    /// with `title`, and a non-empty title also renames the visible `.kbdoc` in
    /// Drive (best effort, server-side).
    ///
    /// Every `UpdateDocumentDto` field is `COALESCE`d, so sending only `title`
    /// leaves the content alone — and, crucially, does **not** rotate
    /// `content_etag`.
    pub fn rename(&self, id: &str, title: &str) -> ApiResult<Fetched> {
        let body = serde_json::json!({ "title": title });
        let request = Request::new(Method::Patch, document_path(id, "")).json(body);
        let key = self.idem().key_for(&request.body_bytes());
        let v = self.call_json(request.header(HEADER_IDEMPOTENCY, key))?;
        self.idem().settled();
        Fetched::from_envelope(&v)
    }

    /// Stars or unstars. Same `PATCH`, one field.
    pub fn set_starred(&self, id: &str, starred: bool) -> ApiResult<Fetched> {
        let body = serde_json::json!({ "is_starred": starred });
        let request = Request::new(Method::Patch, document_path(id, "")).json(body);
        let key = self.idem().key_for(&request.body_bytes());
        let v = self.call_json(request.header(HEADER_IDEMPOTENCY, key))?;
        self.idem().settled();
        Fetched::from_envelope(&v)
    }

    /// `POST /api/v1/office/documents`.
    ///
    /// Carries an `Idempotency-Key` so a retry after an unclear network failure
    /// returns the first document instead of creating a second one.
    pub fn create(&self, spec: &NewDocument) -> ApiResult<Fetched> {
        let request = Request::new(Method::Post, format!("{OFFICE_PREFIX}/documents")).json(spec.body());
        let key = self.idem().key_for(&request.body_bytes());
        let v = self.call_json(request.header(HEADER_IDEMPOTENCY, key))?;
        self.idem().settled();
        Fetched::from_envelope(&v)
    }

    /// `POST /documents/:id/duplicate`. Owner only. Copies the **main** file,
    /// not the draft, so unsaved session work is not in the copy. The response
    /// does not go through `doc_response`, so it carries no etag.
    pub fn duplicate(&self, id: &str) -> ApiResult<Fetched> {
        let v = self.call_json(Request::new(Method::Post, document_path(id, "/duplicate")))?;
        Fetched::from_envelope(&v)
    }

    /// `POST /documents/open-by-file` — opens (or imports) the document backing
    /// a Drive file. Readable formats are `docx`/`dotx`, `odt`/`ott` and `doc`;
    /// anything else answers [`Error::Validation`].
    pub fn open_by_file(&self, file_id: &str) -> ApiResult<Fetched> {
        let body = serde_json::json!({ "file_id": file_id });
        let v = self.call_json(
            Request::new(Method::Post, format!("{OFFICE_PREFIX}/documents/open-by-file")).json(body),
        )?;
        Fetched::from_envelope(&v)
    }

    /// `POST /documents/:id/trash`. Owner only; a document already trashed
    /// answers 404.
    pub fn trash(&self, id: &str) -> ApiResult<()> {
        self.call(Request::new(Method::Post, document_path(id, "/trash")))?;
        Ok(())
    }

    /// `POST /documents/:id/restore`.
    pub fn restore(&self, id: &str) -> ApiResult<()> {
        self.call(Request::new(Method::Post, document_path(id, "/restore")))?;
        Ok(())
    }

    /// `DELETE /documents/:id/delete` — note the path: there is no
    /// `DELETE /documents/:id`. Requires the document to be trashed first,
    /// otherwise 404. Deletes the content and draft files from Drive; the
    /// imported source file is deliberately kept.
    pub fn delete_forever(&self, id: &str) -> ApiResult<()> {
        self.call(Request::new(Method::Delete, document_path(id, "/delete")))?;
        Ok(())
    }

    // ── Editing session ──────────────────────────────────────────────────────

    /// `POST /documents/:id/editing/join`.
    ///
    /// Joining is not optional politeness: a `PATCH` with content writes
    /// through `draft_or_main_file_id`, which creates a draft if none exists,
    /// and the draft is deleted only when the last session leaves. A client
    /// that never joins never leaves, and strands a draft that every later
    /// reader reads instead of the `.kbdoc`.
    ///
    /// All four `editing/*` routes are **owner-only** (`WHERE id = $1 AND
    /// owner_id = $2`): on a document shared with us they answer
    /// [`Error::NotFound`], and the digest guard is then the only protection.
    pub fn join_editing(&self, id: &str) -> ApiResult<Joined> {
        let v = self.call_json(Request::new(Method::Post, document_path(id, "/editing/join")))?;
        let document = Fetched::from_envelope(&v)?;
        let editors = v
            .get("editors")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .map(|r| Editor {
                        user_id: str_at(r, "user_id"),
                        display_name: opt_str(r, "display_name"),
                        color: opt_str(r, "color"),
                        last_ping_at: str_at(r, "last_ping_at"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Joined { document, editors })
    }

    /// `POST /documents/:id/editing/ping` — keepalive. The server's liveness
    /// window is [`PRESENCE_WINDOW_S`]; ping at half that.
    pub fn ping_editing(&self, id: &str) -> ApiResult<()> {
        self.call(Request::new(Method::Post, document_path(id, "/editing/ping")))?;
        Ok(())
    }

    /// `POST /documents/:id/editing/save` — promotes the draft onto the main
    /// content file and rotates both etags, which it returns.
    pub fn save_editing(&self, id: &str) -> ApiResult<SavedDraft> {
        let v = self.call_json(Request::new(Method::Post, document_path(id, "/editing/save")))?;
        Ok(SavedDraft {
            etag: opt_str(&v, "etag"),
            content_etag: opt_str(&v, "content_etag"),
            no_draft: v.get("note").and_then(Value::as_str).is_some(),
        })
    }

    /// `DELETE /documents/:id/editing/leave` — exactly once, on close, and
    /// **after** any in-flight save: leave promotes the draft and then deletes
    /// it, so a leave racing a save loses the save.
    pub fn leave_editing(&self, id: &str) -> ApiResult<()> {
        self.call(Request::new(Method::Delete, document_path(id, "/editing/leave")))?;
        Ok(())
    }

    // ── Delta ────────────────────────────────────────────────────────────────

    /// `GET /documents/delta?cursor=…&limit=…[&include=content]`.
    ///
    /// The only read-only source of etags. A failure comes back as
    /// [`Error::DeltaUnavailable`] rather than as an empty page: this route
    /// answered a flat 500 for as long as its `SELECT` omitted `source_format`,
    /// and a caller that reads that as "nothing changed" stops syncing without
    /// a symptom.
    pub fn delta(&self, cursor: i64, limit: i64, include_content: bool) -> ApiResult<Delta> {
        let mut path = format!(
            "{OFFICE_PREFIX}/documents/delta?cursor={cursor}&limit={}",
            limit.clamp(1, 500)
        );
        if include_content {
            path.push_str("&include=content");
        }
        let v = match self.call_json(Request::new(Method::Get, path)) {
            Ok(v) => v,
            Err(Error::Server { status, code, message }) => {
                return Err(Error::DeltaUnavailable {
                    status,
                    message: if message.is_empty() { code } else { message },
                })
            }
            Err(e) => return Err(e),
        };
        let rows = v.get("changes").and_then(Value::as_array).ok_or_else(|| {
            Error::DeltaUnavailable { status: 200, message: "`changes` absent de la réponse".into() }
        })?;
        let changes = rows
            .iter()
            .map(|c| Change {
                id: str_at(c, "uuid"),
                kind: str_at(c, "kind"),
                etag: opt_str(c, "etag"),
                content_etag: opt_str(c, "content_etag"),
                change_seq: c.get("change_seq").and_then(Value::as_i64).unwrap_or(0),
                summary: c.get("document").map(|d| {
                    let mut s = Summary::from_json(d);
                    // The delta carries the etags beside the document, not
                    // inside it — the one place a listing ever has one.
                    s.etag = opt_str(c, "etag");
                    s
                }),
                content: c
                    .get("content_json")
                    .and_then(|x| serde_json::to_vec(x).ok()),
            })
            .collect();
        Ok(Delta {
            changes,
            cursor: v.get("cursor").and_then(Value::as_i64).unwrap_or(cursor),
            has_more: v.get("has_more").and_then(Value::as_bool).unwrap_or(false),
        })
    }

    // ── Export and instance config ───────────────────────────────────────────

    /// `GET /documents/:id/export/{docx,odt}` — the raw file bytes.
    pub fn export(&self, id: &str, format: ExportFormat) -> ApiResult<Vec<u8>> {
        let path = document_path(id, &format!("/export/{}", format.as_str()));
        Ok(self.call(Request::new(Method::Get, path))?.body)
    }

    /// `POST /documents/:id/save-source` — re-exports into the file the
    /// document was imported from. Owner only; answers
    /// [`Error::Validation`] when there is no source, or when the source format
    /// is readable but not writable (`doc` is: the message asks the client to
    /// offer "save as docx").
    pub fn save_to_source(&self, id: &str) -> ApiResult<String> {
        let v = self.call_json(Request::new(Method::Post, document_path(id, "/save-source")))?;
        Ok(str_at(&v, "format"))
    }

    /// The **core**'s public configuration object (`GET /api/v1/config`, its `config` member), verbatim.
    pub fn core_config(&self) -> ApiResult<serde_json::Map<String, Value>> {
        let v = self.call_json(Request::new(Method::Get, CORE_CONFIG_PATH))?;
        Ok(v.get("config").and_then(Value::as_object).cloned().unwrap_or_default())
    }

    /// The editor's autosave cadence, in seconds, from the **core** config
    /// route. `Some(0)` means the administrator disabled autosave — which is
    /// not the same as the key being absent, hence the `Option`.
    pub fn autosave_interval_s(&self) -> ApiResult<Option<u64>> {
        let v = self.call_json(Request::new(Method::Get, CORE_CONFIG_PATH))?;
        Ok(v.get("config")
            .and_then(|c| c.get(AUTOSAVE_KEY))
            .and_then(Value::as_u64))
    }
}

// ── Small JSON helpers ───────────────────────────────────────────────────────

fn str_at(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// A key that is absent **or** JSON `null` is `None`. The server sends explicit
/// nulls for `icon`, `parent_id`, `source_format` and friends, and a `Some("")`
/// there would read as "the document has an icon named nothing".
fn opt_str(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Records what was sent and replays canned answers.
    struct Fake {
        sent: Mutex<Vec<Request>>,
        answers: Mutex<Vec<ApiResult<Response>>>,
    }

    impl Fake {
        fn new(answers: Vec<ApiResult<Response>>) -> Self {
            Self { sent: Mutex::new(Vec::new()), answers: Mutex::new(answers) }
        }

        fn sent(&self) -> Vec<Request> {
            self.sent.lock().expect("sent").clone()
        }
    }

    impl Transport for Arc<Fake> {
        fn send(&self, request: &Request) -> ApiResult<Response> {
            self.sent.lock().expect("sent").push(request.clone());
            let mut answers = self.answers.lock().expect("answers");
            if answers.is_empty() {
                return Ok(Response::json(200, &serde_json::json!({})));
            }
            answers.remove(0)
        }
    }

    fn client_with(answers: Vec<ApiResult<Response>>) -> (Client, Arc<Fake>) {
        let fake = Arc::new(Fake::new(answers));
        let client = Client::with_transport("inst".into(), Box::new(fake.clone()));
        (client, fake)
    }

    /// The autosave thread will hold this client; a `Client` that stopped being
    /// `Send + Sync` would only show up as a confusing error in someone else's
    /// module.
    #[test]
    fn the_client_can_cross_a_thread() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Client>();
    }

    fn ok(body: Value) -> ApiResult<Response> {
        Ok(Response::json(200, &body))
    }

    fn envelope() -> Value {
        serde_json::json!({
            "document": {
                "id": "doc-1",
                "title": "Rapport",
                "etag": "e1",
                "content_etag": "c1",
                "updated_at": "2026-09-19T10:00:00Z",
            },
            "content_json": { "type": "doc", "content": [] },
        })
    }

    // ── Digest ───────────────────────────────────────────────────────────────

    #[test]
    fn sha256_matches_the_published_vectors() {
        assert_eq!(
            digest_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            digest_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn digest_covers_input_longer_than_one_block() {
        // Two-block input exercises the padding and the length field, which is
        // where a hand-written SHA-256 goes wrong.
        let long = vec![b'k'; 200];
        assert_ne!(digest_hex(&long), digest_hex(&long[..199]));
        assert_eq!(digest_hex(&long).len(), 64);
    }

    // ── Idempotency: the rule the server does not enforce ────────────────────

    #[test]
    fn the_same_payload_reuses_its_key_because_that_is_a_retry() {
        let mut guard = IdemGuard::new();
        let payload = br#"{"content_json":{"type":"doc"}}"#;
        assert_eq!(guard.key_for(payload), guard.key_for(payload));
    }

    #[test]
    fn a_different_payload_never_reuses_a_key() {
        // Reusing it would make the server answer 200 with the FIRST payload's
        // stored response and discard this one — silent loss of a save.
        let mut guard = IdemGuard::new();
        let first = guard.key_for(b"{\"a\":1}");
        let second = guard.key_for(b"{\"a\":2}");
        assert_ne!(first, second);
    }

    #[test]
    fn returning_to_an_earlier_payload_mints_a_third_key() {
        let mut guard = IdemGuard::new();
        let first = guard.key_for(b"{\"a\":1}");
        guard.key_for(b"{\"a\":2}");
        let again = guard.key_for(b"{\"a\":1}");
        assert_ne!(first, again, "replaying the old key would return a stale, superseded response");
    }

    #[test]
    fn a_settled_write_does_not_replay_its_key() {
        let mut guard = IdemGuard::new();
        let first = guard.key_for(b"{\"a\":1}");
        guard.settled();
        assert_ne!(first, guard.key_for(b"{\"a\":1}"));
    }

    #[test]
    fn generated_keys_are_v4_shaped_and_distinct() {
        let a = fresh_uuid_v4();
        let b = fresh_uuid_v4();
        assert_ne!(a, b);
        let parts: Vec<&str> = a.split('-').collect();
        assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), vec![8, 4, 4, 4, 12]);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() || c == '-'), "{a}");
        assert_eq!(parts[2].as_bytes()[0], b'4', "version nibble");
        assert!(matches!(parts[3].as_bytes()[0], b'8' | b'9' | b'a' | b'b'), "variant nibble");
    }

    // ── URL building ─────────────────────────────────────────────────────────

    #[test]
    fn a_document_id_cannot_escape_its_route() {
        // The id reaches us as a String from the model layer. Interpolating it
        // raw would let `../../admin` address another route entirely.
        let path = document_path("../../admin", "/trash");
        assert_eq!(path, "/api/v1/office/documents/..%2F..%2Fadmin/trash");
        assert!(!path.contains("/../"));
    }

    #[test]
    fn a_search_value_is_escaped_so_it_cannot_split_the_query() {
        let q = ListQuery::search("100 % coton&limit=1");
        assert_eq!(q.query_string(), "search=100%20%25%20coton%26limit%3D1");
    }

    #[test]
    fn list_paths_are_the_routes_the_server_registers() {
        assert_eq!(ListQuery::default().path(), "/api/v1/office/documents");
        assert_eq!(ListQuery::recent().path(), "/api/v1/office/documents?recent=true");
        let q = ListQuery { trashed: true, limit: Some(10), offset: Some(20), ..Default::default() };
        assert_eq!(q.path(), "/api/v1/office/documents?trashed=true&limit=10&offset=20");
    }

    #[test]
    fn hard_delete_is_not_delete_on_the_document_path() {
        // `DELETE /documents/:id` does not exist; the route is `/…/:id/delete`.
        let (client, fake) = client_with(vec![ok(serde_json::json!({"ok": true}))]);
        client.delete_forever("doc-1").expect("delete");
        let sent = fake.sent();
        assert_eq!(sent[0].method, Method::Delete);
        assert_eq!(sent[0].path, "/api/v1/office/documents/doc-1/delete");
    }

    #[test]
    fn editing_routes_use_the_verbs_the_router_registers() {
        let (client, fake) = client_with(vec![
            ok(serde_json::json!({"ok": true})),
            ok(serde_json::json!({"ok": true})),
        ]);
        client.ping_editing("d").expect("ping");
        client.leave_editing("d").expect("leave");
        let sent = fake.sent();
        assert_eq!((sent[0].method, sent[0].path.as_str()), (Method::Post, "/api/v1/office/documents/d/editing/ping"));
        // `leave` is a DELETE — a POST there is a 405, not a leave.
        assert_eq!((sent[1].method, sent[1].path.as_str()), (Method::Delete, "/api/v1/office/documents/d/editing/leave"));
    }

    #[test]
    fn the_delta_limit_is_clamped_to_what_the_server_accepts() {
        let (client, fake) = client_with(vec![ok(serde_json::json!({
            "changes": [], "cursor": 7, "has_more": false
        }))]);
        client.delta(3, 100_000, true).expect("delta");
        assert_eq!(
            fake.sent()[0].path,
            "/api/v1/office/documents/delta?cursor=3&limit=500&include=content"
        );
    }

    // ── Headers ──────────────────────────────────────────────────────────────

    #[test]
    fn every_request_announces_this_app_not_the_sync_client() {
        let request = Request::new(Method::Get, "/x");
        let ua = request.header_value("user-agent").expect("User-Agent");
        assert!(ua.starts_with("Kubuno-Documents/"), "{ua}");
    }

    #[test]
    fn a_save_carries_if_match_and_a_key_scoped_to_its_payload() {
        let (client, _) = client_with(vec![]);
        let request = client.save_request("doc-1", br#"{"type":"doc"}"#, Some("e1")).expect("built");
        assert_eq!(request.method, Method::Patch);
        assert_eq!(request.path, "/api/v1/office/documents/doc-1");
        assert_eq!(request.header_value("If-Match"), Some("e1"));
        assert!(request.header_value("Idempotency-Key").is_some());
        // The content travels under `content_json`; anything else is ignored by
        // `UpdateDocumentDto` and would save nothing.
        assert!(request.body.as_ref().and_then(|b| b.get("content_json")).is_some());
    }

    #[test]
    fn a_save_without_a_known_etag_still_goes_out() {
        // `GET` on an older instance returns no etag. Refusing to save without
        // one would make the document unsaveable; last-writer-wins plus the
        // digest guard is the documented fallback.
        let (client, _) = client_with(vec![]);
        let request = client.save_request("doc-1", br#"{"type":"doc"}"#, None).expect("built");
        assert_eq!(request.header_value("If-Match"), None);
    }

    #[test]
    fn retrying_the_identical_save_reuses_the_key() {
        let (client, _) = client_with(vec![]);
        let a = client.save_request("d", br#"{"type":"doc"}"#, Some("e1")).expect("a");
        let b = client.save_request("d", br#"{"type":"doc"}"#, Some("e1")).expect("b");
        assert_eq!(a.header_value("Idempotency-Key"), b.header_value("Idempotency-Key"));
    }

    #[test]
    fn a_changed_document_gets_a_new_key() {
        let (client, _) = client_with(vec![]);
        let a = client.save_request("d", br#"{"type":"doc","content":[]}"#, Some("e1")).expect("a");
        let b = client.save_request("d", br#"{"type":"doc","content":[1]}"#, Some("e1")).expect("b");
        assert_ne!(a.header_value("Idempotency-Key"), b.header_value("Idempotency-Key"));
    }

    #[test]
    fn critical_headers_are_named_so_a_transport_cannot_drop_them_quietly() {
        let request = Request::new(Method::Patch, "/x")
            .header(HEADER_IF_MATCH, "e1")
            .header(HEADER_IDEMPOTENCY, "k1");
        assert_eq!(request.undroppable_headers(), vec!["If-Match", "Idempotency-Key"]);
        // The User-Agent is diagnostic, not load-bearing.
        assert!(Request::new(Method::Get, "/x").undroppable_headers().is_empty());
    }

    // ── Error mapping ────────────────────────────────────────────────────────

    #[test]
    fn a_413_is_its_own_error_and_is_never_retried() {
        // axum's body-limit rejection is not the module's JSON error envelope,
        // so the mapping must not depend on parsing one.
        let e = Error::from_response(413, b"length limit exceeded");
        assert_eq!(e, Error::TooLarge);
        assert!(!e.retryable(), "retrying a payload that cannot fit never ends");
        assert!(e.needs_user_decision());
    }

    #[test]
    fn a_412_is_a_conflict_to_show_not_a_failure_to_retry() {
        let body = br#"{"error":"PRECONDITION_FAILED","message":"Precondition echouee (etag)"}"#;
        let e = Error::from_response(412, body);
        assert_eq!(e, Error::Precondition);
        assert!(!e.retryable());
    }

    #[test]
    fn bad_input_is_422_not_400() {
        let e = Error::from_response(422, br#"{"error":"VALIDATION","message":"format non pris en charge"}"#);
        assert_eq!(e, Error::Validation("format non pris en charge".into()));
        // A client that only special-cases 400 mis-handles every validation
        // failure this server produces.
        assert!(!matches!(Error::from_response(400, b""), Error::Validation(_)));
    }

    #[test]
    fn a_disabled_policy_is_told_apart_from_a_plain_refusal() {
        let disabled = Error::from_response(403, br#"{"error":"POLICY_DISABLED","message":"liens publics desactives"}"#);
        assert!(matches!(disabled, Error::PolicyDisabled(_)));
        assert_eq!(Error::from_response(403, br#"{"error":"FORBIDDEN"}"#), Error::Forbidden);
    }

    #[test]
    fn a_server_error_is_retryable_and_a_client_error_is_not() {
        assert!(Error::from_response(500, br#"{"error":"DATABASE_ERROR"}"#).retryable());
        assert!(!Error::from_response(404, b"").retryable());
        assert!(Error::Transport("timeout".into()).retryable());
    }

    #[test]
    fn a_status_is_recovered_from_the_sync_crates_error_text() {
        // `kubuno-sync` keeps only this line; without parsing it a 412 would be
        // indistinguishable from a network blip and would be retried.
        assert_eq!(
            Error::from_sync_message("GET /api/v1/office/documents : HTTP 412 Precondition Failed"),
            Error::Precondition
        );
        assert_eq!(Error::from_sync_message("POST /x : HTTP 413 Payload Too Large"), Error::TooLarge);
        // Anything that is not a status stays transport — i.e. retryable.
        assert!(Error::from_sync_message("error sending request").retryable());
    }

    // ── Response parsing ─────────────────────────────────────────────────────

    #[test]
    fn a_fetch_without_an_etag_is_not_an_error() {
        // The older server answers `get` without going through `doc_response`.
        let body = serde_json::json!({
            "document": { "id": "d", "title": "Sans etag" },
            "content_json": { "type": "doc" },
        });
        let (client, _) = client_with(vec![ok(body)]);
        let fetched = client.fetch_doc("d").expect("fetch");
        assert_eq!(fetched.etag, None);
        assert_eq!(fetched.content_etag, None);
        assert!(!fetched.digest.is_empty(), "the digest is what actually tracks the bytes");
    }

    #[test]
    fn a_fetch_reports_the_etag_when_the_server_sends_one() {
        let (client, _) = client_with(vec![ok(envelope())]);
        let fetched = client.fetch_doc("doc-1").expect("fetch");
        assert_eq!(fetched.etag.as_deref(), Some("e1"));
        assert_eq!(fetched.content_etag.as_deref(), Some("c1"));
        assert_eq!(fetched.title, "Rapport");
    }

    #[test]
    fn a_response_missing_content_json_is_malformed_not_an_empty_document() {
        let (client, _) = client_with(vec![ok(serde_json::json!({ "document": { "id": "d" } }))]);
        assert!(matches!(client.fetch_doc("d"), Err(Error::Malformed(_))));
    }

    #[test]
    fn a_list_without_its_key_is_malformed_not_an_empty_library() {
        let (client, _) = client_with(vec![ok(serde_json::json!({ "total": 0 }))]);
        assert!(matches!(client.list_query(&ListQuery::default()), Err(Error::Malformed(_))));
    }

    #[test]
    fn a_listing_carries_no_etag_because_the_select_has_none() {
        let body = serde_json::json!({
            "documents": [{
                "id": "d1", "title": "A", "updated_at": "2026-09-19T10:00:00Z",
                "icon": null, "parent_id": null, "source_format": null,
                "word_count": 12, "is_starred": true, "is_trashed": false,
            }],
            "total": 1,
        });
        let (client, _) = client_with(vec![ok(body)]);
        let rows = client.list_query(&ListQuery::default()).expect("list");
        assert_eq!(rows[0].etag, None);
        assert_eq!(rows[0].word_count, 12);
        assert!(rows[0].starred);
        // An explicit JSON null is absence, not an empty string.
        assert_eq!(rows[0].icon, None);
        assert_eq!(rows[0].source_format, None);
    }

    #[test]
    fn a_broken_delta_is_never_read_as_no_changes() {
        let (client, _) = client_with(vec![Err(Error::Server {
            status: 500,
            code: "DATABASE_ERROR".into(),
            message: "Erreur base de données".into(),
        })]);
        match client.delta(0, 200, false) {
            Err(Error::DeltaUnavailable { status, .. }) => assert_eq!(status, 500),
            other => panic!("a 500 must surface as DeltaUnavailable, got {other:?}"),
        }
    }

    #[test]
    fn a_delta_answer_without_changes_is_also_unavailable() {
        let (client, _) = client_with(vec![ok(serde_json::json!({ "cursor": 4 }))]);
        assert!(matches!(client.delta(0, 200, false), Err(Error::DeltaUnavailable { .. })));
    }

    #[test]
    fn delta_carries_the_etags_a_listing_cannot() {
        let body = serde_json::json!({
            "changes": [
                { "uuid": "d1", "kind": "modified", "etag": "e9", "content_etag": "c9",
                  "change_seq": 12, "document": { "id": "d1", "title": "A" } },
                { "uuid": "d2", "kind": "deleted", "change_seq": 13 },
            ],
            "cursor": 13, "has_more": true,
        });
        let (client, _) = client_with(vec![ok(body)]);
        let delta = client.delta(0, 200, false).expect("delta");
        assert_eq!(delta.cursor, 13);
        assert!(delta.has_more);
        assert_eq!(delta.changes[0].summary.as_ref().and_then(|s| s.etag.clone()), Some("e9".into()));
        // A tombstone carries no document.
        assert_eq!(delta.changes[1].kind, "deleted");
        assert!(delta.changes[1].summary.is_none());
    }

    #[test]
    fn join_reports_the_other_editors() {
        let mut body = envelope();
        body["editors"] = serde_json::json!([
            { "user_id": "me", "display_name": "Moi", "color": "#123456", "last_ping_at": "t" },
            { "user_id": "other", "display_name": null, "color": "#654321", "last_ping_at": "t" },
        ]);
        let (client, _) = client_with(vec![ok(body)]);
        let joined = client.join_editing("doc-1").expect("join");
        assert_eq!(joined.editors.len(), 2);
        assert_eq!(joined.others("me").len(), 1);
        assert_eq!(joined.editors[1].display_name, None);
    }

    #[test]
    fn a_drafts_promotion_hands_back_fresh_tokens() {
        let (client, _) = client_with(vec![ok(serde_json::json!({
            "ok": true, "etag": "e2", "content_etag": "c2"
        }))]);
        let saved = client.save_editing("d").expect("save");
        assert_eq!(saved.etag.as_deref(), Some("e2"));
        assert!(!saved.no_draft);
    }

    #[test]
    fn a_missing_draft_is_reported_rather_than_faked_as_a_save() {
        let (client, _) = client_with(vec![ok(serde_json::json!({
            "ok": true, "note": "no draft to save"
        }))]);
        let saved = client.save_editing("d").expect("save");
        assert!(saved.no_draft);
        assert_eq!(saved.etag, None);
    }

    #[test]
    fn an_editing_route_on_a_shared_document_answers_not_found() {
        // The four editing routes filter on `owner_id`, so a document shared
        // with us is simply not there — it is not a bug to report as a crash.
        let (client, _) = client_with(vec![Err(Error::NotFound)]);
        assert_eq!(client.join_editing("d").unwrap_err(), Error::NotFound);
    }

    // ── Branch precedence ────────────────────────────────────────────────────

    #[test]
    fn list_branches_are_exclusive_in_the_servers_order() {
        let q = ListQuery { recent: true, starred: true, ..Default::default() };
        // `starred` is tested first server-side: this is NOT "recently starred".
        assert_eq!(q.branch(), ListBranch::Starred);
        assert_eq!(
            ListQuery { search: Some("x".into()), starred: true, ..Default::default() }.branch(),
            ListBranch::Search
        );
        assert_eq!(
            ListQuery { shared: true, recent: true, ..Default::default() }.branch(),
            ListBranch::Shared
        );
        assert_eq!(ListQuery::default().branch(), ListBranch::Folder);
    }

    // ── Create / rename ──────────────────────────────────────────────────────

    #[test]
    fn a_create_sends_only_the_fields_that_were_asked_for() {
        let (client, fake) = client_with(vec![ok(envelope())]);
        client.create(&NewDocument::titled("Rapport")).expect("create");
        let sent = fake.sent();
        let body = sent[0].body.clone().expect("body");
        assert_eq!(body.get("title").and_then(Value::as_str), Some("Rapport"));
        // Sending `icon: null` would be COALESCE'd away anyway, but an absent
        // key is what the DTO documents.
        assert!(body.get("icon").is_none());
        assert!(sent[0].header_value("Idempotency-Key").is_some());
    }

    #[test]
    fn a_rename_is_a_patch_with_a_title_and_touches_no_content() {
        let (client, fake) = client_with(vec![ok(envelope())]);
        client.rename("doc-1", "Nouveau titre").expect("rename");
        let sent = fake.sent();
        assert_eq!(sent[0].method, Method::Patch);
        let body = sent[0].body.clone().expect("body");
        assert_eq!(body.get("title").and_then(Value::as_str), Some("Nouveau titre"));
        assert!(body.get("content_json").is_none(), "a rename must not rotate content_etag");
    }

    #[test]
    fn a_save_that_landed_does_not_replay_its_key_on_the_next_save() {
        let (client, fake) = client_with(vec![ok(envelope()), ok(envelope())]);
        client.save_content("doc-1", br#"{"type":"doc"}"#, Some("e1")).expect("first");
        client.save_content("doc-1", br#"{"type":"doc"}"#, Some("e1")).expect("second");
        let sent = fake.sent();
        assert_ne!(
            sent[0].header_value("Idempotency-Key"),
            sent[1].header_value("Idempotency-Key"),
            "the second save is a new write, not a retry of the first"
        );
    }

    #[test]
    fn a_failed_save_keeps_its_key_so_the_retry_is_deduplicated() {
        let (client, fake) = client_with(vec![Err(Error::Transport("timeout".into())), ok(envelope())]);
        let payload = br#"{"type":"doc"}"#;
        assert!(client.save_content("doc-1", payload, Some("e1")).is_err());
        client.save_content("doc-1", payload, Some("e1")).expect("retry");
        let sent = fake.sent();
        assert_eq!(
            sent[0].header_value("Idempotency-Key"),
            sent[1].header_value("Idempotency-Key"),
            "an unknown outcome must be retried under the SAME key, or the write lands twice"
        );
    }

    #[test]
    fn non_json_content_is_refused_before_it_reaches_the_network() {
        let (client, fake) = client_with(vec![]);
        assert!(matches!(client.save_request("d", b"not json", None), Err(Error::Malformed(_))));
        assert!(fake.sent().is_empty());
    }

    #[test]
    fn the_autosave_cadence_is_read_from_the_core_config_route() {
        let (client, fake) = client_with(vec![ok(serde_json::json!({
            "config": { "office.autosave_interval_s": 0 }
        }))]);
        // `Some(0)` means the administrator switched autosave off — which an
        // `unwrap_or(30)` on a plain u64 would turn back on.
        assert_eq!(client.autosave_interval_s().expect("config"), Some(0));
        assert_eq!(fake.sent()[0].path, "/api/v1/config");
    }

    #[test]
    fn an_absent_cadence_key_is_absent_not_zero() {
        let (client, _) = client_with(vec![ok(serde_json::json!({ "config": {} }))]);
        assert_eq!(client.autosave_interval_s().expect("config"), None);
    }

    #[test]
    fn an_export_returns_the_raw_bytes_of_the_file() {
        let (client, fake) = client_with(vec![Ok(Response { status: 200, body: b"PK\x03\x04".to_vec() })]);
        let bytes = client.export("doc-1", ExportFormat::Docx).expect("export");
        assert_eq!(bytes, b"PK\x03\x04");
        assert_eq!(fake.sent()[0].path, "/api/v1/office/documents/doc-1/export/docx");
    }
}
