//! The open-document session: when to save, and when to refuse to.
//!
//! # The rule this module exists for
//!
//! **`If-Match` does not protect the document.** The server has a second write
//! path — `save_editing`, `leave_editing`, `save_editing_internal` — which
//! copies the draft file over the content file. A browser tab that joined an
//! editing session, typed, and closed changes the bytes while our token may
//! still look usable: a save that trusts the precondition alone can pass, get a
//! 200, and overwrite that work with no 412 and no dialog.
//!
//! Therefore the precondition is **a digest of the content**, re-read
//! immediately before every save, and `If-Match` is kept only because it is
//! free. If the digest differs from what we opened or last saved, the session
//! goes to [`State::Conflict`] *even though `If-Match` would have succeeded*.
//!
//! Corollary to keep in mind: `content_etag` is **not** a content fingerprint.
//! It moves when someone PATCHes with `content_json`; it is not derived from
//! the bytes, so equal etags never prove equal content.
//!
//! # The other rule
//!
//! A save must not be able to strand a draft. `PATCH` with content writes
//! through `draft_or_main_file_id`, which **creates** a draft if none exists,
//! and the draft is cleaned up only when the last editing session leaves. So
//! the session joins on open, pings inside the server's 120 s window, and
//! leaves exactly once on close — ordered **after** any in-flight save, because
//! `leave` promotes and then deletes the draft and a leave racing a save loses
//! the save.
//!
//! # What this module is, mechanically
//!
//! A state machine with **no clock, no threads and no sockets**, so every rule
//! above is testable as pure logic:
//!
//! * time arrives as a monotonic `now_ms` the caller passes in;
//! * the server arrives as the [`Server`] trait and the crash journal as
//!   [`Journal`] (together, [`Host`]);
//! * a save is *described* on the UI thread ([`SaveJob`]) and *executed*
//!   wherever the caller likes ([`SaveJob::run`]), with the result handed back
//!   through [`Session::finish`]. "One save in flight" is then a property of
//!   the type rather than of a comment.
//!
//! # Facts this is built on (verified against the sources, 2026-09-19)
//!
//! * The autosave interval is `office.autosave_interval_s`, and it is read from
//!   the **core's** `GET /api/v1/config` — not from an office route. The office
//!   module declares the key `public` in `module.toml:239-246` (default 30) and
//!   `frontend/src/useOfficeInstance.ts:38-59` reads it through the SDK `api`
//!   whose `baseURL` is `/api/v1` (`core/frontend/src/core/api/client.ts:6`).
//!   There is no `/api/v1/office/config` handler in `office/src`.
//! * **A value of `0` disables autosave.** `DocumentEditorPage.tsx:5070` is
//!   `autosaveIntervalS > 0 ? autosaveIntervalS * 1000 : 0`, and `:7079` only
//!   arms the timer when the value is positive. A non-numeric or absent value
//!   falls back to 30, never to a permissive value (`useOfficeInstance.ts:50-53`).
//! * The web's autosave is a **debounce, not a cadence**: every edit clears the
//!   timer and re-arms it (`DocumentEditorPage.tsx:7078-7081`), so continuous
//!   typing defers the save indefinitely. We match that — parity is the product
//!   requirement — and close the resulting exposure with the journal below
//!   rather than by inventing a different cadence.
//! * The **structural** debounce is 700 ms (`scheduleSave`,
//!   `DocumentEditorPage.tsx:14342-14345`): margins, orientation, header and
//!   footer save almost immediately, typing does not.
//! * The editing-session window is `last_ping_at > NOW() - INTERVAL '2 minutes'`
//!   (`office/src/handlers/documents.rs:1085-1088`), so we ping at 60 s and
//!   treat 120 s of failed pings as "the server has forgotten us, re-join".
//! * Those four handlers filter `WHERE id = $1 AND owner_id = $2` — **owner
//!   only** (`:866-871`, `:974-995`). On a document we do not own, join fails;
//!   the session then runs in [`Protection::DigestOnly`] and says so.
//!
//! # Divergences from the spec, deliberately
//!
//! * `31-CORRECTIONS.md` C1 states that the draft-promotion path rotates no
//!   etag. That is **no longer true of the current server**: both
//!   `save_editing` (`documents.rs:955-962`) and `save_editing_internal`
//!   (`:1057-1061`) now `UPDATE … etag = $3, content_etag = $4`. The digest
//!   guard stays anyway — it is the only check that is a function of the bytes,
//!   `leave_editing` discards the result of its promotion (`let _ =`, `:974`)
//!   so a failed rotation is silent, and a guard that costs one GET per save
//!   is not worth removing on the strength of a comment.
//! * After a successful PATCH we do **not** assume the server now holds our
//!   bytes. The office server re-serialises `content_json` through
//!   `serde_json::Value`, so the stored bytes can differ from the sent bytes;
//!   assuming otherwise would manufacture a conflict on the very next save.
//!   The post-save baseline is therefore re-read from the server.
//! * A save is never issued **without a baseline digest**. If the bootstrap
//!   read failed (offline at open), the document stays dirty, the journal keeps
//!   the work, and the save happens when a baseline can be established. Not
//!   saving is recoverable; overwriting blind is not.

use serde_json::{Map, Value};

// ── Constants, all sourced rather than chosen ────────────────────────────────

/// The core route that carries every module's `public` settings.
pub const CONFIG_PATH: &str = "/api/v1/config";
/// The key inside it. Module keys are namespaced `<module>.<key>`.
pub const AUTOSAVE_KEY: &str = "office.autosave_interval_s";
/// `office/module.toml:245`.
pub const DEFAULT_AUTOSAVE_S: u64 = 30;
/// `DocumentEditorPage.tsx:14344`.
pub const STRUCTURAL_DEBOUNCE_MS: u64 = 700;
/// Half the server's window, so one lost ping is not a dropped session.
pub const PING_INTERVAL_MS: u64 = 60_000;
/// `INTERVAL '2 minutes'` in `get_editing_sessions`.
pub const SESSION_WINDOW_MS: u64 = 120_000;
/// First backoff after a failed save. Doubles up to [`RETRY_MAX_MS`].
pub const RETRY_BASE_MS: u64 = 5_000;
/// A failed save must not become a spin loop, and must not become a save that
/// never happens again either.
pub const RETRY_MAX_MS: u64 = 60_000;

// ── The outside world, as two traits ─────────────────────────────────────────

/// What went wrong, reduced to the four cases the state machine reacts to
/// differently. Everything else is [`Error::Transport`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// HTTP 412. The etag moved: treated exactly like a digest mismatch.
    Precondition,
    /// HTTP 413. Terminal — see [`State::TooLarge`].
    TooLarge,
    /// The editing-session handlers are owner-only, so 403/404 from them means
    /// "not ours", not "gone".
    NotOwner,
    /// Offline, 5xx, a closed socket — anything worth retrying.
    Transport(String),
}

/// One read of the document as the server holds it now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub content: Vec<u8>,
    pub etag:    Option<String>,
    /// Display names from the `editors[]` array `join` returns, refreshed by
    /// each ping. Drives the "another editor is in this document" banner.
    pub editors: Vec<String>,
}

/// The office routes this module calls, and nothing else.
///
/// Deliberately not async and deliberately not `Client`: the point is that the
/// rules can be driven by a test with no network at all.
pub trait Server {
    /// `GET /api/v1/office/documents/:id` — the content the server holds now.
    fn fetch(&mut self, id: &str) -> Result<Snapshot, Error>;
    /// `PATCH /api/v1/office/documents/:id` with `content_json`. Returns the
    /// new `content_etag` when the server sends one.
    fn patch(&mut self, job: &SaveJob) -> Result<Option<String>, Error>;
    /// `POST /:id/editing/join`.
    fn join(&mut self, id: &str) -> Result<Snapshot, Error>;
    /// `POST /:id/editing/ping`.
    fn ping(&mut self, id: &str) -> Result<(), Error>;
    /// `DELETE /:id/editing/leave`.
    fn leave(&mut self, id: &str) -> Result<(), Error>;
    /// [`CONFIG_PATH`], returning the `config` object verbatim.
    fn config(&mut self) -> Result<Map<String, Value>, Error>;
}

/// The local crash journal.
///
/// Separate from [`Server`] because it must work when [`Server`] does not —
/// that is the entire point of it.
pub trait Journal {
    fn write(&mut self, id: &str, bytes: &[u8]) -> Result<(), Error>;
    fn read(&mut self, id: &str) -> Option<Vec<u8>>;
    fn clear(&mut self, id: &str);
}

/// The session's whole outside world.
pub trait Host: Server + Journal {}
impl<T: Server + Journal> Host for T {}

/// The document, as the session sees it: whatever a save would send.
///
/// Taken as a callback so the session never owns or copies the model, and so
/// the bytes are produced only when they are actually about to be used.
pub trait Content {
    fn bytes(&self) -> Vec<u8>;
}

impl<F: Fn() -> Vec<u8>> Content for F {
    fn bytes(&self) -> Vec<u8> {
        self()
    }
}

// ── The digest ───────────────────────────────────────────────────────────────

/// SHA-256 of the content bytes, hex-encoded.
///
/// Implemented here rather than with the `sha2` crate because `sha2` is not in
/// `kubuno-documents`' dependency graph and this module owns exactly one file.
/// The implementation is checked against the FIPS 180-4 vectors in the tests;
/// swapping it for `sha2::Sha256` is a two-line change once the manifest gains
/// the dependency.
pub struct Digest;

impl Digest {
    /// The digest of one document's stored bytes.
    pub fn of(bytes: &[u8]) -> String {
        hex(&sha256(bytes))
    }
}

const HEX: &[u8; 16] = b"0123456789abcdef";

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[rustfmt::skip]
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // Padding: 0x80, zeroes to 56 mod 64, then the length in bits, big-endian.
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = Vec::with_capacity(data.len() + 72);
    msg.extend_from_slice(data);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for block in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (slot, quad) in w.iter_mut().zip(block.as_chunks::<4>().0) {
            *slot = u32::from_be_bytes(*quad);
        }
        for i in 16..64 {
            let x = w[i - 15];
            let y = w[i - 2];
            let s0 = x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3);
            let s1 = y.rotate_right(17) ^ y.rotate_right(19) ^ (y >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut s] = h;
        for (kv, wv) in K.iter().zip(w.iter()) {
            let sig1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = s
                .wrapping_add(sig1)
                .wrapping_add(ch)
                .wrapping_add(*kv)
                .wrapping_add(*wv);
            let sig0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = sig0.wrapping_add(maj);
            s = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, add) in h.iter_mut().zip([a, b, c, d, e, f, g, s]) {
            *slot = slot.wrapping_add(add);
        }
    }

    let mut out = [0u8; 32];
    for (quad, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(h.iter()) {
        *quad = word.to_be_bytes();
    }
    out
}

/// A UUID-shaped idempotency key derived from `(nonce, seq, payload digest)`.
///
/// `31-CORRECTIONS.md` C9 asks for a fresh v4 UUID **per payload**, reused only
/// for a byte-identical retry of a request whose outcome is unknown — because
/// the server replays on `(user_id, key)` **without comparing the request**
/// (`handlers/documents.rs:421-427`), so a key reused across two different
/// snapshots silently discards the second.
///
/// Deriving the key gives exactly those semantics and is deterministic enough
/// to test. It is not random, which is why it is not a *real* v4 UUID; it is
/// shaped like one because that is what every other client sends. `uuid` is not
/// in this crate's dependency graph.
fn idempotency_key(nonce: u64, seq: u64, payload_digest: &str) -> String {
    let mut seed = Vec::with_capacity(32 + payload_digest.len());
    seed.extend_from_slice(&nonce.to_be_bytes());
    seed.extend_from_slice(&seq.to_be_bytes());
    seed.extend_from_slice(payload_digest.as_bytes());
    let d = sha256(&seed);

    let mut b = [0u8; 16];
    b.copy_from_slice(&d[..16]);
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // RFC 4122 variant

    let h = hex(&b);
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

// ── State ────────────────────────────────────────────────────────────────────

/// Where an open document stands with the server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Clean,
    Dirty,
    Saving,
    /// The server copy moved under us. The only way out is a choice the user
    /// clicked — never an automatic resolution.
    Conflict,
    /// The document is over the server's body limit. Terminal, not retryable:
    /// retrying forever while the user keeps typing is the worst outcome.
    TooLarge,
    Failed,
}

impl State {
    /// States no automatic save may ever leave. Typing does not clear them and
    /// no timer fires out of them; only the user does.
    pub fn is_blocked(&self) -> bool {
        matches!(self, State::TooLarge | State::Conflict)
    }

    /// [`State::TooLarge`] alone: no retry path exists at all, not even a
    /// user-driven one, until the document gets smaller.
    pub fn is_terminal(&self) -> bool {
        matches!(self, State::TooLarge)
    }
}

/// How much protection this session actually has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Protection {
    /// We joined the editing session: presence is published, the draft's
    /// lifecycle is ours to close, and the digest guard runs on top.
    #[default]
    Session,
    /// `join` was refused — the handler is owner-only and this document is
    /// shared with us. The digest guard is the **only** protection, and the
    /// PATCH still creates a draft we cannot clean up. The status bar says so.
    DigestOnly,
}

/// What the user chose in the conflict dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Keep our text: the next save adopts the server's current digest as its
    /// baseline and overwrites. Only ever reachable from a click.
    KeepMine,
    /// Take the server's: the caller reloads the document, so the session is
    /// clean again at the server's digest.
    TakeTheirs,
}

/// How long to wait before each kind of save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cadence {
    /// Debounce after the last edit of any kind. `0` = autosave disabled.
    pub autosave_ms:   u64,
    /// Debounce after a structural change (margins, orientation, header).
    pub structural_ms: u64,
}

impl Default for Cadence {
    fn default() -> Self {
        Self {
            autosave_ms:   DEFAULT_AUTOSAVE_S * 1_000,
            structural_ms: STRUCTURAL_DEBOUNCE_MS,
        }
    }
}

impl Cadence {
    /// Reads [`AUTOSAVE_KEY`] out of the core's config object, exactly the way
    /// `useOfficeInstance.ts:50-53` does: a finite number or the compiled
    /// default, never a permissive fallback.
    pub fn from_config(cfg: &Map<String, Value>) -> Self {
        let secs = match cfg.get(AUTOSAVE_KEY).and_then(Value::as_f64) {
            Some(v) if v.is_finite() && v >= 0.0 => v as u64,
            _ => DEFAULT_AUTOSAVE_S,
        };
        Self {
            autosave_ms: secs.saturating_mul(1_000),
            ..Self::default()
        }
    }

    /// `autosaveIntervalS > 0` — zero means the administrator turned it off.
    pub fn autosave_enabled(&self) -> bool {
        self.autosave_ms > 0
    }
}

// ── One save attempt ─────────────────────────────────────────────────────────

/// Everything one save attempt must do, decided on the UI thread so the digest
/// guard and the idempotency key are fixed before any thread hop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveJob {
    pub id:              String,
    pub content:         Vec<u8>,
    /// Digest of `content`, so a caller can log it without re-hashing.
    pub content_digest:  String,
    /// The digest we believe the server holds. `None` never reaches here —
    /// [`Session`] refuses to issue a job without a baseline.
    pub expect:          Option<String>,
    /// Kept because it is free and it does catch the REST write path.
    pub if_match:        Option<String>,
    pub idempotency_key: String,
}

/// The result of one attempt, in the four shapes the state machine cares about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Saved {
        etag:   Option<String>,
        /// The digest the server holds **after** the save, re-read rather than
        /// assumed: the server re-serialises `content_json`, so our bytes and
        /// its bytes need not be equal.
        digest: String,
    },
    /// The content moved under us. Carries what the server holds now, so
    /// [`Resolution::KeepMine`] has a baseline to overwrite from.
    Conflict {
        server: String,
    },
    /// 413. Terminal.
    TooLarge,
    Failed(Error),
}

impl SaveJob {
    /// Runs the guard and the save. Safe to call off the UI thread.
    ///
    /// Order matters and is the whole rule: **read, compare, only then write.**
    pub fn run(&self, srv: &mut dyn Server) -> Outcome {
        let before = match srv.fetch(&self.id) {
            Ok(s) => Digest::of(&s.content),
            Err(e) => return Outcome::Failed(e),
        };
        if let Some(expected) = self.expect.as_deref() {
            if expected != before {
                // The etag may well still be valid. It does not matter: the
                // bytes moved, and that is the only fact we trust.
                return Outcome::Conflict { server: before };
            }
        } else {
            // A job without a baseline should not exist; refuse rather than
            // overwrite blind.
            return Outcome::Conflict { server: before };
        }

        let etag = match srv.patch(self) {
            Ok(e) => e,
            Err(Error::TooLarge) => return Outcome::TooLarge,
            Err(Error::Precondition) => return Outcome::Conflict { server: before },
            Err(e) => return Outcome::Failed(e),
        };

        // Re-read the baseline. If this read fails we fall back to the digest
        // of what we sent: the next save may then see a mismatch and raise a
        // conflict dialog for no reason, which is the safe direction to be
        // wrong in.
        let digest = match srv.fetch(&self.id) {
            Ok(s) => Digest::of(&s.content),
            Err(_) => self.content_digest.clone(),
        };
        Outcome::Saved { etag, digest }
    }
}

/// What [`Session::close`] wants the caller to do next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Close {
    /// Run this, report it with [`Session::finish`], then call `close` again.
    Save(Box<SaveJob>),
    /// A save is already in flight. Report it, then call `close` again. The
    /// leave will not be sent before then — a leave racing a save loses it.
    Wait,
    /// The session has left. Nothing more to do.
    Done,
}

// ── The session ──────────────────────────────────────────────────────────────

/// An open document's relationship with the server.
#[derive(Debug)]
pub struct Session {
    pub state:  State,
    pub id:     String,
    /// Digest of the content as we last knew the server to hold it. `None`
    /// means "not established" — and then no save is issued at all.
    pub digest: Option<String>,

    /// Resolved from the core config on [`Session::begin`].
    pub cadence: Cadence,

    protection:  Protection,
    etag:        Option<String>,
    editors:     Vec<String>,
    /// What the server held when the conflict was raised. Kept apart from
    /// `digest` so that resolving is an explicit adoption, never a drift.
    conflict_server_digest: Option<String>,

    /// Unsaved changes exist. Tracked apart from `state` because `state` is
    /// also what the status bar displays, and `Saving` must not erase it.
    dirty:       bool,
    /// Edits arrived while a save was in flight: exactly one more save follows.
    pending:     bool,
    /// The journal is behind the model.
    journal_due: bool,

    joined:      bool,
    left:        bool,
    /// We have written at least once, so a draft may exist even if we never
    /// joined. Then the leave is still worth attempting.
    wrote:       bool,
    closing:     bool,
    close_saves: u8,

    last_edit_ms:       u64,
    structural_edit_ms: Option<u64>,
    last_ping_ms:       u64,
    /// The last ping the *server* accepted. Silence past the 120 s window means
    /// it has forgotten us.
    last_ping_ok_ms:    u64,
    retry_at_ms:        Option<u64>,
    retry_backoff_ms:   u64,

    nonce:        u64,
    seq:          u64,
    /// Digest of the payload of the last job issued, so a byte-identical retry
    /// reuses its idempotency key and a different payload gets a fresh one.
    last_payload: Option<String>,
}

impl Session {
    pub fn open(id: String) -> Self {
        // The nonce must be distinct per OPEN, not merely per document: it is
        // mixed into the idempotency key, and `seq` restarts at 0 on every open,
        // so a constant nonce would make reopening the same document replay the
        // previous session's keys. Within the server's 24 h replay window a
        // byte-identical re-save would then be answered from cache without
        // writing — the retyped content silently lost.
        //
        // The earlier nonce was derived from a stack ADDRESS, which is constant
        // across calls (and therefore no entropy at all). It now comes from
        // `fresh_uuid_v4`, the same per-payload key source the REST client uses,
        // which mixes real entropy (RandomState + clock + pid).
        let nonce = {
            let d = sha256(crate::api::client::fresh_uuid_v4().as_bytes());
            u64::from_be_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]])
        };
        Self {
            state: State::Clean,
            id,
            digest: None,
            cadence: Cadence::default(),
            protection: Protection::Session,
            etag: None,
            editors: Vec::new(),
            conflict_server_digest: None,
            dirty: false,
            pending: false,
            journal_due: false,
            joined: false,
            left: false,
            wrote: false,
            closing: false,
            close_saves: 0,
            last_edit_ms: 0,
            structural_edit_ms: None,
            last_ping_ms: 0,
            last_ping_ok_ms: 0,
            retry_at_ms: None,
            retry_backoff_ms: RETRY_BASE_MS,
            nonce,
            seq: 0,
            last_payload: None,
        }
    }

    // ── Opening ──────────────────────────────────────────────────────────────

    /// Whatever the previous run left in the journal, if it crashed.
    ///
    /// Called **before** [`Session::begin`], because the recovered bytes are
    /// what the user is offered, not what the server holds.
    pub fn recover(&mut self, host: &mut dyn Host) -> Option<Vec<u8>> {
        host.read(&self.id)
    }

    /// Discards a recovered journal the user declined.
    pub fn discard_recovery(&mut self, host: &mut dyn Host) {
        host.clear(&self.id);
    }

    /// Reads the config, joins the editing session and establishes the digest
    /// baseline. Never fails: a refused join downgrades to
    /// [`Protection::DigestOnly`], and a failed read leaves the baseline unset
    /// so that no save is issued until it can be established.
    pub fn begin(&mut self, now_ms: u64, host: &mut dyn Host) -> Protection {
        if let Ok(cfg) = host.config() {
            self.cadence = Cadence::from_config(&cfg);
        }
        self.last_edit_ms = now_ms;
        self.last_ping_ms = now_ms;
        self.last_ping_ok_ms = now_ms;

        match host.join(&self.id) {
            Ok(snap) => {
                self.joined = true;
                self.protection = Protection::Session;
                self.digest = Some(Digest::of(&snap.content));
                self.etag = snap.etag;
                self.editors = snap.editors;
            }
            Err(Error::NotOwner) => {
                self.protection = Protection::DigestOnly;
                self.bootstrap(host);
            }
            Err(_) => {
                // Offline, or the server is unwell. We are not in a session, so
                // we must not behave as if we were; the baseline attempt is
                // retried by `tick`.
                self.protection = Protection::DigestOnly;
                self.bootstrap(host);
            }
        }
        self.protection
    }

    fn bootstrap(&mut self, host: &mut dyn Host) {
        if let Ok(snap) = host.fetch(&self.id) {
            self.digest = Some(Digest::of(&snap.content));
            self.etag = snap.etag;
        }
    }

    // ── Editing ──────────────────────────────────────────────────────────────

    /// Marks the document changed. Debounced by the caller.
    pub fn touch(&mut self) {
        self.dirty = true;
        self.journal_due = true;
        match self.state {
            // The user owns these. Typing does not clear a conflict and does
            // not un-break an oversized document.
            State::TooLarge | State::Conflict => self.pending = true,
            State::Saving => self.pending = true,
            _ => self.state = State::Dirty,
        }
    }

    /// A content edit at `now_ms`: arms the autosave debounce and the journal.
    pub fn edited(&mut self, now_ms: u64) {
        self.touch();
        self.last_edit_ms = now_ms;
    }

    /// A structural edit — margins, orientation, paper size, header or footer.
    /// Saves on the short debounce, the way `scheduleSave` does in the browser.
    pub fn structural(&mut self, now_ms: u64) {
        self.edited(now_ms);
        if self.structural_edit_ms.is_none() {
            self.structural_edit_ms = Some(now_ms);
        }
    }

    // ── The clock tick ───────────────────────────────────────────────────────

    /// One pass of everything time-driven: presence, the journal, and the
    /// decision to save. Returns the job to run, at most one, ever.
    pub fn tick(
        &mut self,
        now_ms: u64,
        content: &dyn Content,
        host: &mut dyn Host,
    ) -> Option<SaveJob> {
        self.pump_session(now_ms, host);
        self.pump_journal(now_ms, content, host);
        self.pump_save(now_ms, content)
    }

    fn pump_session(&mut self, now_ms: u64, host: &mut dyn Host) {
        // No baseline yet: keep trying, because nothing can be saved until
        // there is one and the work is piling up in the journal meanwhile.
        if !self.left && self.digest.is_none() {
            self.bootstrap(host);
        }
        if self.left || !self.joined {
            return;
        }
        if now_ms.saturating_sub(self.last_ping_ms) < PING_INTERVAL_MS {
            return;
        }
        self.last_ping_ms = now_ms;
        match host.ping(&self.id) {
            Ok(()) => self.last_ping_ok_ms = now_ms,
            Err(Error::NotOwner) => {
                // The session is gone and cannot be re-created: stop pretending.
                self.joined = false;
                self.protection = Protection::DigestOnly;
            }
            Err(_) => {
                if now_ms.saturating_sub(self.last_ping_ok_ms) >= SESSION_WINDOW_MS {
                    // Past the server's window the row is filtered out, so a
                    // ping can never revive it. Re-join — but do **not** adopt
                    // the returned content as the baseline: that would erase
                    // the very change the guard exists to catch.
                    if let Ok(snap) = host.join(&self.id) {
                        self.last_ping_ok_ms = now_ms;
                        self.editors = snap.editors;
                    }
                }
            }
        }
    }

    fn pump_journal(&mut self, now_ms: u64, content: &dyn Content, host: &mut dyn Host) {
        if !self.journal_due {
            return;
        }
        if now_ms.saturating_sub(self.last_edit_ms) < STRUCTURAL_DEBOUNCE_MS {
            return;
        }
        // Regardless of connectivity, and regardless of whether a save is in
        // flight or even possible. Losing 29 s of typing to a crash is the
        // commoner loss and this is what closes it.
        if host.write(&self.id, &content.bytes()).is_ok() {
            self.journal_due = false;
        }
    }

    fn pump_save(&mut self, now_ms: u64, content: &dyn Content) -> Option<SaveJob> {
        if !self.can_start_save() {
            return None;
        }
        if let Some(at) = self.retry_at_ms {
            if now_ms < at {
                return None;
            }
        }
        let structural_due = self
            .structural_edit_ms
            .is_some_and(|t| now_ms.saturating_sub(t) >= self.cadence.structural_ms);
        let autosave_due = self.cadence.autosave_enabled()
            && now_ms.saturating_sub(self.last_edit_ms) >= self.cadence.autosave_ms;
        if !structural_due && !autosave_due {
            return None;
        }
        self.start_save(content)
    }

    /// `Ctrl+S`: saves now, skipping both debounces. Still refuses from a
    /// blocked state — a conflict has to be answered first, and an oversized
    /// document cannot be saved by asking harder.
    pub fn save_now(&mut self, content: &dyn Content) -> Option<SaveJob> {
        if !self.can_start_save() {
            return None;
        }
        self.start_save(content)
    }

    fn can_start_save(&self) -> bool {
        !self.left
            && self.dirty
            && self.state != State::Saving
            && !self.state.is_blocked()
            && self.digest.is_some()
    }

    fn start_save(&mut self, content: &dyn Content) -> Option<SaveJob> {
        let expect = self.digest.clone()?;
        let bytes = content.bytes();
        let content_digest = Digest::of(&bytes);

        // A fresh key per payload; the same key only for a byte-identical
        // retry, whose outcome we do not know.
        if self.last_payload.as_deref() != Some(content_digest.as_str()) {
            self.seq = self.seq.wrapping_add(1);
            self.last_payload = Some(content_digest.clone());
        }

        self.state = State::Saving;
        self.pending = false;
        self.structural_edit_ms = None;
        Some(SaveJob {
            id: self.id.clone(),
            content: bytes,
            idempotency_key: idempotency_key(self.nonce, self.seq, &content_digest),
            content_digest,
            expect: Some(expect),
            if_match: self.etag.clone(),
        })
    }

    /// Reports the result of the one in-flight save.
    pub fn finish(&mut self, outcome: Outcome, now_ms: u64, host: &mut dyn Host) {
        match outcome {
            Outcome::Saved { etag, digest } => {
                self.wrote = true;
                self.digest = Some(digest);
                if etag.is_some() {
                    self.etag = etag;
                }
                self.retry_at_ms = None;
                self.retry_backoff_ms = RETRY_BASE_MS;
                self.dirty = self.pending;
                if self.pending {
                    // Coalescing: the edits made during the save are not lost
                    // and did not queue a second save of their own — they
                    // trigger exactly one more, from here.
                    self.pending = false;
                    self.state = State::Dirty;
                } else {
                    self.state = State::Clean;
                    // The journal only ever holds work the server does not.
                    host.clear(&self.id);
                    self.journal_due = false;
                }
            }
            Outcome::Conflict { server } => {
                // Deliberately keep `digest` pointing at what we opened: the
                // conflict dialog needs to know both, and adopting the server's
                // digest here would let the next automatic save overwrite it.
                self.state = State::Conflict;
                self.dirty = true;
                self.pending = false;
                self.conflict_server_digest = Some(server);
            }
            Outcome::TooLarge => {
                // Terminal. No backoff, no retry, no timer: `can_start_save`
                // refuses from here forever.
                self.state = State::TooLarge;
                self.dirty = true;
                self.retry_at_ms = None;
            }
            Outcome::Failed(_) => {
                self.state = State::Failed;
                self.dirty = true;
                self.pending = false;
                self.retry_at_ms = Some(now_ms.saturating_add(self.retry_backoff_ms));
                self.retry_backoff_ms = (self.retry_backoff_ms * 2).min(RETRY_MAX_MS);
            }
        }
        // A close that was waiting on this save does not leave here: the caller
        // drives it by calling `close` again, so the ordering is visible in the
        // call sequence rather than hidden in a flag.
    }

    // ── Conflict ─────────────────────────────────────────────────────────────

    /// What the server held when the conflict was raised.
    pub fn conflict_digest(&self) -> Option<&str> {
        self.conflict_server_digest.as_deref()
    }

    /// Applies the user's choice. Never called by a timer.
    pub fn resolve(&mut self, choice: Resolution, host: &mut dyn Host) {
        if self.state != State::Conflict {
            return;
        }
        match choice {
            Resolution::KeepMine => {
                // Adopt the server's digest as the baseline so the next save
                // passes the guard, and let it run.
                if let Some(d) = self.conflict_server_digest.take() {
                    self.digest = Some(d);
                }
                self.dirty = true;
                self.state = State::Dirty;
            }
            Resolution::TakeTheirs => {
                if let Some(d) = self.conflict_server_digest.take() {
                    self.digest = Some(d);
                }
                self.dirty = false;
                self.pending = false;
                self.journal_due = false;
                self.state = State::Clean;
                host.clear(&self.id);
            }
        }
    }

    // ── Closing ──────────────────────────────────────────────────────────────

    /// Closes the document. Call until it returns [`Close::Done`].
    ///
    /// The leave is sent **after** any in-flight save, because `leave_editing`
    /// promotes the draft and then deletes it: a leave that overtakes a save
    /// deletes the draft the save was about to write into.
    pub fn close(&mut self, content: &dyn Content, host: &mut dyn Host) -> Close {
        self.closing = true;
        if self.left {
            return Close::Done;
        }
        if self.state == State::Saving {
            return Close::Wait;
        }
        if self.dirty && !self.state.is_blocked() && self.close_saves < 2 {
            if let Some(job) = self.save_now(content) {
                self.close_saves += 1;
                return Close::Save(Box::new(job));
            }
        }
        // Everything that could be saved has been tried. Flush the crash
        // journal FIRST: `pump_journal` only writes on the structural debounce,
        // so a close in a blocked or offline state — with edits still inside
        // that window — would otherwise leave them on neither the server nor the
        // journal. The next open reads the journal, so this is the last chance
        // to make it hold what it claims to.
        if self.journal_due && host.write(&self.id, &content.bytes()).is_ok() {
            self.journal_due = false;
        }
        self.do_leave(host);
        Close::Done
    }

    fn do_leave(&mut self, host: &mut dyn Host) {
        if self.left {
            return;
        }
        self.left = true;
        // Attempted whenever a draft could exist: we joined, or we wrote (a
        // PATCH creates a draft even on a document we could not join).
        if self.joined || self.wrote {
            let _ = host.leave(&self.id);
        }
    }

    // ── What the UI reads ────────────────────────────────────────────────────

    pub fn protection(&self) -> Protection {
        self.protection
    }

    pub fn editors(&self) -> &[String] {
        &self.editors
    }

    pub fn etag(&self) -> Option<&str> {
        self.etag.as_deref()
    }

    pub fn has_unsaved(&self) -> bool {
        self.dirty || self.pending
    }

    pub fn is_saving(&self) -> bool {
        self.state == State::Saving
    }

    pub fn has_left(&self) -> bool {
        self.left
    }

    /// The one line the status bar owes the user when this session is not as
    /// protected as it looks.
    pub fn status_note(&self) -> Option<&'static str> {
        match (self.protection, &self.state) {
            (_, State::TooLarge) => Some(
                "This document is too large for the server. Export it to a local file; \
                 saving cannot succeed until it is smaller.",
            ),
            (_, State::Conflict) => {
                Some("Someone else changed this document on the server. Choose which copy to keep.")
            }
            (Protection::DigestOnly, _) => Some(
                "Shared document: no editing session. Changes are checked against the server \
                 copy before each save, but other editors are not shown.",
            ),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    // ── A server that lies, fails and normalises on demand ───────────────────

    #[derive(Default)]
    struct Fake {
        content:      Vec<u8>,
        etag:         Option<String>,
        cfg:          Map<String, Value>,
        calls:        Vec<String>,
        journal:      BTreeMap<String, Vec<u8>>,
        join_err:     Option<Error>,
        patch_err:    Option<Error>,
        fetch_err:    Option<Error>,
        ping_err:     Option<Error>,
        /// The server re-serialises what it stores, the way the office module
        /// does through `serde_json::Value`.
        normalise:    bool,
        /// Journal writes fail (a full disk).
        journal_err:  bool,
        last_key:     Option<String>,
        keys:         Vec<String>,
    }

    impl Fake {
        fn with_content(bytes: &[u8]) -> Self {
            Self {
                content: bytes.to_vec(),
                etag: Some("e0".into()),
                ..Self::default()
            }
        }
        fn count(&self, what: &str) -> usize {
            self.calls.iter().filter(|c| c.as_str() == what).count()
        }
    }

    impl Server for Fake {
        fn fetch(&mut self, _id: &str) -> Result<Snapshot, Error> {
            self.calls.push("fetch".into());
            if let Some(e) = self.fetch_err.clone() {
                return Err(e);
            }
            Ok(Snapshot {
                content: self.content.clone(),
                etag:    self.etag.clone(),
                editors: Vec::new(),
            })
        }
        fn patch(&mut self, job: &SaveJob) -> Result<Option<String>, Error> {
            self.calls.push("patch".into());
            self.keys.push(job.idempotency_key.clone());
            self.last_key = Some(job.idempotency_key.clone());
            if let Some(e) = self.patch_err.clone() {
                return Err(e);
            }
            self.content = job.content.clone();
            if self.normalise {
                self.content.push(b' ');
            }
            self.etag = Some(format!("e{}", self.count("patch")));
            Ok(self.etag.clone())
        }
        fn join(&mut self, _id: &str) -> Result<Snapshot, Error> {
            self.calls.push("join".into());
            if let Some(e) = self.join_err.clone() {
                return Err(e);
            }
            Ok(Snapshot {
                content: self.content.clone(),
                etag:    self.etag.clone(),
                editors: vec!["someone".into()],
            })
        }
        fn ping(&mut self, _id: &str) -> Result<(), Error> {
            self.calls.push("ping".into());
            match self.ping_err.clone() {
                Some(e) => Err(e),
                None => Ok(()),
            }
        }
        fn leave(&mut self, _id: &str) -> Result<(), Error> {
            self.calls.push("leave".into());
            Ok(())
        }
        fn config(&mut self) -> Result<Map<String, Value>, Error> {
            self.calls.push("config".into());
            Ok(self.cfg.clone())
        }
    }

    impl Journal for Fake {
        fn write(&mut self, id: &str, bytes: &[u8]) -> Result<(), Error> {
            self.calls.push("journal".into());
            if self.journal_err {
                return Err(Error::Transport("disk full".into()));
            }
            self.journal.insert(id.to_string(), bytes.to_vec());
            Ok(())
        }
        fn read(&mut self, id: &str) -> Option<Vec<u8>> {
            self.journal.get(id).cloned()
        }
        fn clear(&mut self, id: &str) {
            self.journal.remove(id);
        }
    }

    fn body(s: &'static str) -> impl Content {
        move || s.as_bytes().to_vec()
    }

    // ── The digest itself ────────────────────────────────────────────────────

    #[test]
    fn sha256_matches_the_fips_180_4_vectors() {
        assert_eq!(
            Digest::of(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            Digest::of(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Two blocks: the padding path that a one-block test cannot reach.
        assert_eq!(
            Digest::of(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn equal_bytes_digest_equal_and_one_changed_byte_does_not() {
        assert_eq!(Digest::of(b"hello"), Digest::of(b"hello"));
        assert_ne!(Digest::of(b"hello"), Digest::of(b"hellp"));
    }

    // ── C1: the digest guard ─────────────────────────────────────────────────

    #[test]
    fn content_changed_without_the_etag_moving_is_still_a_conflict() {
        // Exactly the scenario the module exists for: a browser tab joined the
        // editing session, typed, and left. The bytes moved; the etag did not.
        let mut h = Fake::with_content(b"original");
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);

        h.content = b"the other tab's work".to_vec();
        // The etag is deliberately left untouched — `If-Match` would pass.
        assert_eq!(h.etag.as_deref(), Some("e0"));

        let job = s.save_now(&body("ours")).expect("a save was due");
        assert_eq!(job.if_match.as_deref(), Some("e0"), "the token still looks valid");

        let outcome = job.run(&mut h);
        assert!(matches!(outcome, Outcome::Conflict { .. }), "{outcome:?}");
        assert_eq!(h.count("patch"), 0, "nothing was written over the other tab");

        s.finish(outcome, 0, &mut h);
        assert_eq!(s.state, State::Conflict);
        assert_eq!(h.content, b"the other tab's work", "their work survived");
    }

    #[test]
    fn an_unchanged_server_copy_saves() {
        let mut h = Fake::with_content(b"original");
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);

        let job = s.save_now(&body("ours")).expect("a save was due");
        let out = job.run(&mut h);
        assert!(matches!(out, Outcome::Saved { .. }), "{out:?}");
        s.finish(out, 0, &mut h);
        assert_eq!(s.state, State::Clean);
        assert_eq!(h.content, b"ours");
    }

    #[test]
    fn a_server_that_reserialises_does_not_manufacture_a_conflict() {
        // The office server re-serialises `content_json` through
        // `serde_json::Value`, so the stored bytes need not equal the sent
        // bytes. Assuming they do would flag a conflict on the next save.
        let mut h = Fake::with_content(b"original");
        h.normalise = true;
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);

        s.edited(0);
        let out = s.save_now(&body("first")).expect("job").run(&mut h);
        s.finish(out, 0, &mut h);
        assert_eq!(s.state, State::Clean);

        s.edited(1);
        let out = s.save_now(&body("second")).expect("job").run(&mut h);
        assert!(matches!(out, Outcome::Saved { .. }), "phantom conflict: {out:?}");
    }

    #[test]
    fn a_412_is_a_conflict_and_not_a_retry() {
        let mut h = Fake::with_content(b"original");
        h.patch_err = Some(Error::Precondition);
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);
        let out = s.save_now(&body("ours")).expect("job").run(&mut h);
        assert!(matches!(out, Outcome::Conflict { .. }), "{out:?}");
        s.finish(out, 0, &mut h);
        assert_eq!(s.state, State::Conflict);
    }

    #[test]
    fn no_save_is_issued_without_a_baseline_digest() {
        // Offline at open: no baseline. Saving blind would overwrite whatever
        // the server holds.
        let mut h = Fake::with_content(b"original");
        h.join_err = Some(Error::Transport("offline".into()));
        h.fetch_err = Some(Error::Transport("offline".into()));
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        assert!(s.digest.is_none());

        s.edited(0);
        assert!(s.save_now(&body("ours")).is_none());
        assert!(s.tick(60_000, &body("ours"), &mut h).is_none());
        assert!(s.has_unsaved());

        // The network returns: the same tick re-establishes the baseline and
        // the work that was held back goes out.
        h.fetch_err = None;
        let job = s.tick(61_000, &body("ours"), &mut h).expect("the held-back save");
        assert_eq!(job.expect.as_deref(), Some(Digest::of(b"original").as_str()));
        let out = job.run(&mut h);
        s.finish(out, 61_000, &mut h);
        assert_eq!(s.state, State::Clean);
        assert_eq!(h.content, b"ours");
    }

    #[test]
    fn a_conflict_is_only_left_by_a_user_choice() {
        let mut h = Fake::with_content(b"original");
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);
        h.content = b"theirs".to_vec();
        let out = s.save_now(&body("ours")).expect("job").run(&mut h);
        s.finish(out, 0, &mut h);

        // No amount of typing or ticking gets out of it.
        for t in 1..200u64 {
            s.edited(t * 1_000);
            assert!(s.tick(t * 1_000, &body("ours"), &mut h).is_none());
        }
        assert_eq!(s.state, State::Conflict);

        s.resolve(Resolution::KeepMine, &mut h);
        assert_eq!(s.state, State::Dirty);
        let out = s.save_now(&body("ours")).expect("job").run(&mut h);
        assert!(matches!(out, Outcome::Saved { .. }), "{out:?}");
    }

    // ── Coalescing ───────────────────────────────────────────────────────────

    #[test]
    fn edits_during_a_save_trigger_exactly_one_more_save() {
        let mut h = Fake::with_content(b"original");
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);

        let job = s.save_now(&body("v1")).expect("first job");
        assert!(s.is_saving());

        // Five edits land while the save is in flight.
        for t in 1..=5u64 {
            s.edited(t);
            assert!(s.save_now(&body("v2")).is_none(), "never two in flight");
        }

        let out = job.run(&mut h);
        s.finish(out, 10, &mut h);
        assert_eq!(s.state, State::Dirty, "the edits were not lost");

        let second = s.save_now(&body("v2")).expect("exactly one more");
        let out = second.run(&mut h);
        s.finish(out, 20, &mut h);
        assert_eq!(s.state, State::Clean);
        assert!(s.save_now(&body("v2")).is_none(), "and no third");
        assert_eq!(h.count("patch"), 2);
    }

    #[test]
    fn a_save_with_no_edits_is_not_issued() {
        let mut h = Fake::with_content(b"original");
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        assert!(s.save_now(&body("same")).is_none());
        assert!(s.tick(10_000_000, &body("same"), &mut h).is_none());
    }

    // ── Cadence ──────────────────────────────────────────────────────────────

    #[test]
    fn autosave_interval_comes_from_the_core_config() {
        let mut cfg = Map::new();
        cfg.insert(AUTOSAVE_KEY.into(), Value::from(5));
        assert_eq!(Cadence::from_config(&cfg).autosave_ms, 5_000);
    }

    #[test]
    fn a_missing_or_non_numeric_interval_falls_back_to_thirty_seconds() {
        assert_eq!(Cadence::from_config(&Map::new()).autosave_ms, 30_000);

        let mut cfg = Map::new();
        cfg.insert(AUTOSAVE_KEY.into(), Value::from("30"));
        assert_eq!(Cadence::from_config(&cfg).autosave_ms, 30_000, "a string is not a number");

        // The console cannot produce a negative, but a hand-edited row can.
        let mut cfg = Map::new();
        cfg.insert(AUTOSAVE_KEY.into(), Value::from(-5));
        assert_eq!(Cadence::from_config(&cfg).autosave_ms, 30_000);
    }

    #[test]
    fn an_interval_of_zero_disables_autosave() {
        // `autosaveIntervalS > 0 ? … : 0` — the administrator turned it off.
        let mut cfg = Map::new();
        cfg.insert(AUTOSAVE_KEY.into(), Value::from(0));
        let c = Cadence::from_config(&cfg);
        assert!(!c.autosave_enabled());

        let mut h = Fake::with_content(b"original");
        h.cfg = cfg;
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);
        assert!(s.tick(10_000_000, &body("ours"), &mut h).is_none(), "no autosave");
        // Ctrl+S still works: the setting governs the timer, not the user.
        assert!(s.save_now(&body("ours")).is_some());
    }

    #[test]
    fn typing_defers_autosave_and_stopping_fires_it() {
        let mut h = Fake::with_content(b"original");
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);

        // A keystroke every 10 s for five minutes: the browser's debounce never
        // fires, and neither does ours.
        let mut t = 0u64;
        for _ in 0..30 {
            s.edited(t);
            t += 10_000;
            assert!(s.tick(t, &body("ours"), &mut h).is_none());
        }
        // The user stops. 30 s later the save goes.
        assert!(s.tick(t + 30_000, &body("ours"), &mut h).is_some());
    }

    #[test]
    fn a_structural_change_saves_on_the_short_debounce() {
        let mut h = Fake::with_content(b"original");
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.structural(0);
        assert!(s.tick(699, &body("ours"), &mut h).is_none());
        assert!(s.tick(700, &body("ours"), &mut h).is_some(), "700 ms, not 30 s");
    }

    // ── 413 ──────────────────────────────────────────────────────────────────

    #[test]
    fn too_large_is_terminal_and_no_retry_loop_can_be_entered() {
        let mut h = Fake::with_content(b"original");
        h.patch_err = Some(Error::TooLarge);
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);

        let out = s.save_now(&body("huge")).expect("job").run(&mut h);
        assert_eq!(out, Outcome::TooLarge);
        s.finish(out, 0, &mut h);
        assert_eq!(s.state, State::TooLarge);

        // An hour of ticking and typing: not one further attempt.
        for i in 1..=3_600u64 {
            s.edited(i * 1_000);
            s.structural(i * 1_000);
            assert!(s.tick(i * 1_000, &body("huge"), &mut h).is_none(), "retried at {i}s");
            assert!(s.save_now(&body("huge")).is_none(), "Ctrl+S retried at {i}s");
        }
        assert_eq!(h.count("patch"), 1, "exactly the one attempt that got the 413");
        assert_eq!(s.state, State::TooLarge);
        assert!(s.status_note().is_some(), "and the user is told");
    }

    #[test]
    fn closing_an_oversized_document_leaves_without_retrying() {
        let mut h = Fake::with_content(b"original");
        h.patch_err = Some(Error::TooLarge);
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);
        let out = s.save_now(&body("huge")).expect("job").run(&mut h);
        s.finish(out, 0, &mut h);

        assert_eq!(s.close(&body("huge"), &mut h), Close::Done);
        assert_eq!(h.count("patch"), 1);
        assert_eq!(h.count("leave"), 1);
    }

    #[test]
    fn a_failing_save_backs_off_instead_of_spinning() {
        let mut h = Fake::with_content(b"original");
        h.patch_err = Some(Error::Transport("502".into()));
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);

        let out = s.save_now(&body("ours")).expect("job").run(&mut h);
        assert!(matches!(out, Outcome::Failed(_)));
        s.finish(out, 30_000, &mut h);
        assert_eq!(s.state, State::Failed);

        // One tick per second for the next five seconds: nothing.
        for t in 31..=34u64 {
            assert!(s.tick(t * 1_000, &body("ours"), &mut h).is_none(), "spun at {t}s");
        }
        assert!(s.tick(35_000, &body("ours"), &mut h).is_some(), "and then retries");
    }

    // ── The editing session ──────────────────────────────────────────────────

    #[test]
    fn the_session_joins_on_open_and_pings_inside_the_server_window() {
        let mut h = Fake::with_content(b"original");
        let mut s = Session::open("doc".into());
        assert_eq!(s.begin(0, &mut h), Protection::Session);
        assert_eq!(h.count("join"), 1);
        assert_eq!(s.editors(), ["someone"]);

        assert!(s.tick(59_000, &body("x"), &mut h).is_none());
        assert_eq!(h.count("ping"), 0);
        s.tick(60_000, &body("x"), &mut h);
        assert_eq!(h.count("ping"), 1, "inside the 120 s window");
        s.tick(120_000, &body("x"), &mut h);
        assert_eq!(h.count("ping"), 2);
    }

    #[test]
    fn a_refused_join_downgrades_the_session_instead_of_breaking_it() {
        // The four editing handlers filter on `owner_id`: on a shared document
        // join fails, and the digest guard is all we have.
        let mut h = Fake::with_content(b"original");
        h.join_err = Some(Error::NotOwner);
        let mut s = Session::open("doc".into());
        assert_eq!(s.begin(0, &mut h), Protection::DigestOnly);
        assert!(s.status_note().is_some(), "the status bar says so");

        // Everything else still works, digest guard included.
        s.edited(0);
        let out = s.save_now(&body("ours")).expect("job").run(&mut h);
        assert!(matches!(out, Outcome::Saved { .. }), "{out:?}");
        s.finish(out, 0, &mut h);
        assert_eq!(s.state, State::Clean);
        assert_eq!(h.count("ping"), 0, "we never joined, so we never ping");
    }

    #[test]
    fn a_shared_document_we_wrote_to_still_leaves_so_the_draft_is_cleaned() {
        // PATCH creates a draft whether or not we could join. Not leaving would
        // strand it in the owner's Drive for good.
        let mut h = Fake::with_content(b"original");
        h.join_err = Some(Error::NotOwner);
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);
        let out = s.save_now(&body("ours")).expect("job").run(&mut h);
        s.finish(out, 0, &mut h);

        assert_eq!(s.close(&body("ours"), &mut h), Close::Done);
        assert_eq!(h.count("leave"), 1);
    }

    #[test]
    fn a_document_we_never_wrote_and_never_joined_sends_no_leave() {
        let mut h = Fake::with_content(b"original");
        h.join_err = Some(Error::NotOwner);
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        assert_eq!(s.close(&body("ours"), &mut h), Close::Done);
        assert_eq!(h.count("leave"), 0, "there is no draft and no session to close");
    }

    #[test]
    fn a_rejoin_after_a_lost_ping_does_not_move_the_digest_baseline() {
        let mut h = Fake::with_content(b"original");
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        let opened = s.digest.clone();

        // The pings fail for longer than the server's window, and meanwhile
        // someone else changes the content.
        h.ping_err = Some(Error::Transport("offline".into()));
        h.content = b"theirs".to_vec();
        s.tick(60_000, &body("x"), &mut h);
        s.tick(180_000, &body("x"), &mut h);
        assert!(h.count("join") >= 2, "it re-joined");
        assert_eq!(s.digest, opened, "but the baseline is what we opened");

        // So the next save still catches them.
        s.edited(180_000);
        let out = s.save_now(&body("ours")).expect("job").run(&mut h);
        assert!(matches!(out, Outcome::Conflict { .. }), "{out:?}");
    }

    // ── Closing ──────────────────────────────────────────────────────────────

    #[test]
    fn the_leave_is_ordered_after_an_in_flight_save() {
        // `leave_editing` promotes the draft and then deletes it, so a leave
        // that overtakes a save deletes the file the save was writing into.
        let mut h = Fake::with_content(b"original");
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);

        let job = s.save_now(&body("ours")).expect("job");
        assert_eq!(s.close(&body("ours"), &mut h), Close::Wait);
        assert_eq!(h.count("leave"), 0, "the leave must not overtake the save");

        let out = job.run(&mut h);
        s.finish(out, 0, &mut h);
        assert_eq!(h.count("leave"), 0, "not on finish either");

        assert_eq!(s.close(&body("ours"), &mut h), Close::Done);
        assert_eq!(h.count("leave"), 1);
        assert_eq!(h.content, b"ours", "and the save survived the close");
    }

    #[test]
    fn closing_a_dirty_document_saves_first_then_leaves_exactly_once() {
        let mut h = Fake::with_content(b"original");
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);

        let job = match s.close(&body("final"), &mut h) {
            Close::Save(j) => j,
            other => panic!("expected a save, got {other:?}"),
        };
        let out = job.run(&mut h);
        s.finish(out, 0, &mut h);
        assert_eq!(s.close(&body("final"), &mut h), Close::Done);

        // And however many times the window asks again.
        for _ in 0..5 {
            assert_eq!(s.close(&body("final"), &mut h), Close::Done);
        }
        assert_eq!(h.count("leave"), 1);
        assert_eq!(h.content, b"final");
    }

    #[test]
    fn a_close_whose_saves_keep_failing_still_leaves() {
        let mut h = Fake::with_content(b"original");
        h.patch_err = Some(Error::Transport("offline".into()));
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);

        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 10, "close never terminated");
            match s.close(&body("final"), &mut h) {
                Close::Save(j) => {
                    let out = j.run(&mut h);
                    s.finish(out, 0, &mut h);
                }
                Close::Wait => panic!("nothing is in flight"),
                Close::Done => break,
            }
        }
        assert_eq!(h.count("leave"), 1);
        // The work is not lost — and this now proves it rather than trusting a
        // flag: the journal actually holds the final content after close, so the
        // next open recovers it. Before the close-flush fix, `has_unsaved()` was
        // true while the journal was empty, and the content was gone.
        assert!(s.has_unsaved());
        assert_eq!(h.read("doc").as_deref(), Some(b"final".as_ref()));
    }

    #[test]
    fn closing_from_a_blocked_state_still_flushes_the_journal() {
        // A conflict blocks saving entirely. A close in that state must still
        // write the crash journal, or the edits since the last debounce vanish.
        let mut h = Fake::with_content(b"original");
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);
        // A conflict blocks the save path entirely (`is_blocked`).
        s.state = State::Conflict;
        loop {
            match s.close(&body("typed in conflict"), &mut h) {
                Close::Save(j) => {
                    let out = j.run(&mut h);
                    s.finish(out, 0, &mut h);
                }
                Close::Wait => panic!("nothing is in flight"),
                Close::Done => break,
            }
        }
        assert_eq!(h.read("doc").as_deref(), Some(b"typed in conflict".as_ref()));
    }

    #[test]
    fn reopening_the_same_document_does_not_reuse_the_previous_sessions_keys() {
        // The nonce must carry real entropy: with a stack-address nonce, two
        // opens produced identical keys, and the server replayed the second
        // save from cache without writing — silent data loss.
        let key = |id: &str| {
            let mut h = Fake::with_content(b"original");
            let mut s = Session::open(id.into());
            s.begin(0, &mut h);
            s.edited(0);
            s.save_now(&body("same content")).expect("a job").idempotency_key
        };
        // Same document id, same first-save payload — the keys must still differ,
        // because each open has its own nonce.
        assert_ne!(key("doc"), key("doc"), "two opens must not share an idempotency key");
    }

    // ── Crash recovery ───────────────────────────────────────────────────────

    #[test]
    fn the_journal_is_written_on_the_structural_debounce_even_when_offline() {
        // "Regardless of connectivity" is the whole point: it closes the 29 s
        // of typing that the 30 s autosave debounce leaves in RAM.
        let mut h = Fake::with_content(b"original");
        h.join_err = Some(Error::Transport("offline".into()));
        h.fetch_err = Some(Error::Transport("offline".into()));
        h.patch_err = Some(Error::Transport("offline".into()));

        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);
        assert!(h.journal.is_empty());
        s.tick(699, &body("typed"), &mut h);
        assert!(h.journal.is_empty(), "not before the debounce");
        s.tick(700, &body("typed"), &mut h);
        assert_eq!(h.journal.get("doc").map(Vec::as_slice), Some(&b"typed"[..]));
    }

    #[test]
    fn the_journal_is_written_while_fully_online_too() {
        let mut h = Fake::with_content(b"original");
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);
        s.tick(700, &body("typed"), &mut h);
        assert_eq!(h.journal.get("doc").map(Vec::as_slice), Some(&b"typed"[..]));
    }

    #[test]
    fn a_successful_save_clears_the_journal_and_a_new_edit_rewrites_it() {
        let mut h = Fake::with_content(b"original");
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);
        s.tick(700, &body("typed"), &mut h);
        assert!(h.journal.contains_key("doc"));

        let out = s.save_now(&body("typed")).expect("job").run(&mut h);
        s.finish(out, 1_000, &mut h);
        assert!(!h.journal.contains_key("doc"), "the server has it now");

        s.edited(2_000);
        s.tick(2_700, &body("more"), &mut h);
        assert_eq!(h.journal.get("doc").map(Vec::as_slice), Some(&b"more"[..]));
    }

    #[test]
    fn a_failed_journal_write_is_retried_on_the_next_tick() {
        let mut h = Fake::with_content(b"original");
        h.journal_err = true;
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);
        s.tick(700, &body("typed"), &mut h);
        assert!(h.journal.is_empty());

        h.journal_err = false;
        s.tick(800, &body("typed"), &mut h);
        assert_eq!(h.journal.get("doc").map(Vec::as_slice), Some(&b"typed"[..]));
    }

    #[test]
    fn the_next_open_recovers_what_the_crash_left_behind() {
        let mut h = Fake::with_content(b"original");
        {
            let mut s = Session::open("doc".into());
            s.begin(0, &mut h);
            s.edited(0);
            s.tick(700, &body("unsaved work"), &mut h);
            // …and the process dies here: no finish, no close, no leave.
        }
        let mut s = Session::open("doc".into());
        assert_eq!(s.recover(&mut h), Some(b"unsaved work".to_vec()));
        s.discard_recovery(&mut h);
        assert_eq!(s.recover(&mut h), None);
    }

    // ── Idempotency ──────────────────────────────────────────────────────────

    #[test]
    fn the_idempotency_key_is_fresh_per_payload_and_stable_for_a_retry() {
        // The server replays on `(user_id, key)` WITHOUT comparing the body, so
        // a key reused across two snapshots silently discards the second.
        let mut h = Fake::with_content(b"original");
        h.patch_err = Some(Error::Transport("timeout, outcome unknown".into()));
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        s.edited(0);

        let first = s.save_now(&body("v1")).expect("job");
        let out = first.run(&mut h);
        s.finish(out, 0, &mut h);

        // Same bytes, unknown outcome: the same key, so the server can replay.
        let retry = s.save_now(&body("v1")).expect("job");
        assert_eq!(retry.idempotency_key, first.idempotency_key);

        // Different bytes: a new key, or the server would discard them.
        h.patch_err = None;
        let out = retry.run(&mut h);
        s.finish(out, 0, &mut h);
        s.edited(1_000);
        let next = s.save_now(&body("v2")).expect("job");
        assert_ne!(next.idempotency_key, first.idempotency_key);
    }

    #[test]
    fn the_idempotency_key_is_uuid_shaped() {
        let k = idempotency_key(1, 2, "abc");
        let parts: Vec<&str> = k.split('-').collect();
        assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), [8, 4, 4, 4, 12]);
        assert_eq!(parts[2].as_bytes()[0], b'4', "version nibble");
        assert!(matches!(parts[3].as_bytes()[0], b'8' | b'9' | b'a' | b'b'), "variant");
        assert!(k.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'));
    }

    // ── State hygiene ────────────────────────────────────────────────────────

    #[test]
    fn touch_never_clears_a_blocked_state() {
        let mut s = Session::open("doc".into());
        s.state = State::TooLarge;
        s.touch();
        assert_eq!(s.state, State::TooLarge);
        s.state = State::Conflict;
        s.touch();
        assert_eq!(s.state, State::Conflict);
        s.state = State::Failed;
        s.touch();
        assert_eq!(s.state, State::Dirty, "a failure is retryable, though");
    }

    #[test]
    fn nothing_is_saved_after_the_session_has_left() {
        let mut h = Fake::with_content(b"original");
        let mut s = Session::open("doc".into());
        s.begin(0, &mut h);
        assert_eq!(s.close(&body("x"), &mut h), Close::Done);
        s.edited(1_000);
        assert!(s.save_now(&body("x")).is_none());
        assert!(s.tick(100_000, &body("x"), &mut h).is_none());
    }
}
