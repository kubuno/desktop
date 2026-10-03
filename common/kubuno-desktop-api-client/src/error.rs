//! Errors of the API client and their classification (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §7.3): what the
//! outbox does with a failed request depends only on [`ApiError::class`].

use std::time::Duration;

use crate::token::AuthError;

/// What a failure means for the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// Network, timeout, 429, 5xx: retry later with the **same** idempotency key; never give up on an intent.
    Transient,
    /// 400, 403, 413, 422 and other 4xx: the server will never accept this request; roll the local change back.
    Definitive,
    /// 409 (other than an idempotency mismatch) or 412: the server state moved; run the conflict policy.
    Conflict,
    /// 404: the target does not exist (a delete is then done, a patch is a `deleted_remotely` conflict).
    NotFound,
    /// 401 even after one refresh: treat as an expired session for this request.
    Unauthorized,
    /// The refresh token was rejected: pause every feed of the account, keep the outbox.
    SessionExpired,
    /// `410 CURSOR_EXPIRED`: the feed must be reset (full pull).
    CursorExpired,
    /// The answer could not be understood (bad JSON, protocol guard): a server or client bug; logged, retried
    /// later like a transient error but surfaced as an error state.
    Protocol,
}

/// An error of the API client. Never carries a token; a server message is the server's text.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ApiError {
    /// The server answered with a non-success status.
    #[error("HTTP {status}{}{}", code.as_deref().map(|c| format!(" {c}")).unwrap_or_default(), message.as_deref().map(|m| format!(": {m}")).unwrap_or_default())]
    Http {
        status: u16,
        /// `error` of the `{"error": CODE, "message": …}` body, when there is one.
        code: Option<String>,
        message: Option<String>,
        /// `Retry-After`, when present and in seconds.
        retry_after: Option<Duration>,
        /// The JSON body (a 412 carries the current row; KDP §7.1 rule 7).
        body: Option<serde_json::Value>,
    },
    /// The request could not complete. `maybe_sent` = the request may have reached the server (timeout after
    /// sending, connection reset while reading): retry with the same idempotency key.
    #[error("network error{}: {message}", if *maybe_sent { " (the request may have been applied)" } else { "" })]
    Network { message: String, maybe_sent: bool },
    /// No access token (see [`AuthError`]).
    #[error(transparent)]
    Auth(#[from] AuthError),
    /// The body did not match the expected type.
    #[error("unexpected response: {0}")]
    Decode(String),
    /// A protocol guard tripped (cursor that does not move while `has_more`...).
    #[error("protocol error: {0}")]
    Protocol(String),
    /// The request itself is invalid (bad URL, body not serializable).
    #[error("invalid request: {0}")]
    InvalidRequest(String),
}

impl ApiError {
    pub fn status(&self) -> Option<u16> {
        match self {
            ApiError::Http { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn code(&self) -> Option<&str> {
        match self {
            ApiError::Http { code, .. } => code.as_deref(),
            _ => None,
        }
    }

    pub fn body(&self) -> Option<&serde_json::Value> {
        match self {
            ApiError::Http { body, .. } => body.as_ref(),
            _ => None,
        }
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            ApiError::Http { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// The classification table of §7.3.
    pub fn class(&self) -> ErrorClass {
        match self {
            ApiError::Http { status, code, .. } => classify_status(*status, code.as_deref()),
            ApiError::Network { .. } => ErrorClass::Transient,
            ApiError::Auth(AuthError::SessionExpired) | ApiError::Auth(AuthError::UnknownAccount) => {
                ErrorClass::SessionExpired
            }
            ApiError::Auth(AuthError::Transient(_)) => ErrorClass::Transient,
            ApiError::Decode(_) | ApiError::Protocol(_) => ErrorClass::Protocol,
            ApiError::InvalidRequest(_) => ErrorClass::Definitive,
        }
    }
}

/// Classifies a status code (and the error code of its body).
pub fn classify_status(status: u16, code: Option<&str>) -> ErrorClass {
    match status {
        401 => ErrorClass::Unauthorized,
        404 => ErrorClass::NotFound,
        410 if code == Some("CURSOR_EXPIRED") => ErrorClass::CursorExpired,
        410 => ErrorClass::NotFound,
        // 409 with an idempotency mismatch means the same key was reused with another body: a client bug,
        // never fixed by retrying.
        409 if matches!(code, Some("IDEMPOTENCY_KEY_REUSED") | Some("IDEMPOTENCY_MISMATCH")) => ErrorClass::Definitive,
        // 409 IN_PROGRESS: the same key is being processed (single-flight on the server): retry later.
        409 if code == Some("IN_PROGRESS") => ErrorClass::Transient,
        409 | 412 => ErrorClass::Conflict,
        408 | 425 | 429 => ErrorClass::Transient,
        500..=599 => ErrorClass::Transient,
        400..=499 => ErrorClass::Definitive,
        _ => ErrorClass::Protocol,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_table() {
        assert_eq!(classify_status(400, None), ErrorClass::Definitive);
        assert_eq!(classify_status(403, None), ErrorClass::Definitive);
        assert_eq!(classify_status(413, None), ErrorClass::Definitive);
        assert_eq!(classify_status(422, None), ErrorClass::Definitive);
        assert_eq!(classify_status(404, None), ErrorClass::NotFound);
        assert_eq!(classify_status(409, Some("CONFLICT")), ErrorClass::Conflict);
        assert_eq!(classify_status(409, Some("IN_PROGRESS")), ErrorClass::Transient);
        assert_eq!(classify_status(409, Some("IDEMPOTENCY_KEY_REUSED")), ErrorClass::Definitive);
        assert_eq!(classify_status(412, None), ErrorClass::Conflict);
        assert_eq!(classify_status(410, Some("CURSOR_EXPIRED")), ErrorClass::CursorExpired);
        assert_eq!(classify_status(429, None), ErrorClass::Transient);
        assert_eq!(classify_status(503, None), ErrorClass::Transient);
        assert_eq!(classify_status(401, None), ErrorClass::Unauthorized);
    }
}
