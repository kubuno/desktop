//! Typed asynchronous client of the Kubuno web API for the desktop (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §5.1,
//! `kubuno-api`).
//!
//! - any method (`GET`, `POST`, `PUT`, `PATCH`, `DELETE`...), per-request headers: `If-Match`, `Idempotency-Key`,
//!   `X-Kubuno-Device-Key`, JSON or byte bodies;
//! - access tokens from a [`TokenSource`] (the shell's token broker in apps): one refresh-and-retry on 401;
//! - retries with exponential backoff and jitter for replayable requests (safe method or idempotency key),
//!   honouring `Retry-After`;
//! - errors mapped from `{"error": CODE, "message": …}` and classified ([`ErrorClass`]) the way the outbox needs;
//! - the Kubuno Delta Protocol v1 ([`DeltaPage`], [`Change`], [`Cursor`]) with the "cursor must move" guard.
//!
//! Nothing here logs a token, a body or a header value: `Debug` of requests and tokens is redacted, logs carry the
//! method, the path, the status and a token fingerprint at most.

mod client;
mod delta;
mod error;
mod token;

pub use client::{
    default_user_agent, jittered, new_idempotency_key, ApiClient, ApiClientBuilder, ApiRequest, ApiResponse, Body,
    RetryPolicy, DEVICE_KEY, IDEMPOTENCY_KEY, IDEMPOTENCY_REPLAYED,
};
pub use delta::{check_progress, Change, ChangeKind, Cursor, DeltaPage};
pub use error::{classify_status, ApiError, ErrorClass};
pub use reqwest::Method;
pub use token::{fingerprint, AccessToken, AuthError, StaticToken, TokenSource};
