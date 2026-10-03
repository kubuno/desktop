//! Where access tokens come from. The client never sees a refresh token: in an app the [`TokenSource`] is the token
//! broker's client (the shell owns the refresh token), in the shell it is the token owner itself, in tests a
//! [`StaticToken`].

use std::fmt;

use sha2::{Digest, Sha256};

/// A bearer access token (a 15-minute JWT). `Debug` is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct AccessToken(String);

impl AccessToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// The token text, for the `Authorization` header. Never log it.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// A short, non-reversible fingerprint (hex of the first 8 bytes of SHA-256): how a client tells the broker
    /// which token the server rejected without sending the token back, and what logs may print.
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.0)
    }

    /// The `exp` claim of a JWT, in seconds since the epoch, read **without** verifying the signature (only to
    /// schedule a refresh; the server is the judge). `None` for an opaque token.
    pub fn jwt_expiry(&self) -> Option<i64> {
        jwt_claim_i64(&self.0, "exp")
    }

    /// The `iat` claim of a JWT (seconds since the epoch, server clock), read without verification.
    pub fn jwt_issued_at(&self) -> Option<i64> {
        jwt_claim_i64(&self.0, "iat")
    }

    /// The lifetime the server gave the token (`exp - iat`, both on the server's clock, so a skewed local clock
    /// does not matter). `None` for an opaque token.
    pub fn jwt_lifetime_s(&self) -> Option<i64> {
        Some(self.jwt_expiry()? - self.jwt_issued_at()?).filter(|l| *l > 0)
    }

    /// The `sub` claim of a JWT (the user id on Kubuno), read without verification.
    pub fn jwt_subject(&self) -> Option<String> {
        jwt_claims(&self.0)?.get("sub")?.as_str().map(str::to_string)
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AccessToken(#{})", self.fingerprint())
    }
}

/// The fingerprint of a token text (see [`AccessToken::fingerprint`]).
pub fn fingerprint(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    hex::encode(&digest[..8])
}

fn b64url_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for c in input.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

fn jwt_claims(token: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let payload = token.split('.').nth(1)?;
    let bytes = b64url_decode(payload)?;
    match serde_json::from_slice::<serde_json::Value>(&bytes).ok()? {
        serde_json::Value::Object(map) => Some(map),
        _ => None,
    }
}

fn jwt_claim_i64(token: &str, claim: &str) -> Option<i64> {
    jwt_claims(token)?.get(claim)?.as_i64()
}

/// Why no access token could be obtained.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// The server rejected the refresh token (401/403 on refresh, family revoked, signed out from the web): the
    /// user must sign in again. Local data stays readable and the outbox is kept.
    #[error("the session has expired: sign in again")]
    SessionExpired,
    /// No new token right now (network down, 429, 5xx, cooldown, broker unreachable). The refresh token is still
    /// valid: retry later, never tell the user the session is over.
    #[error("no access token available right now: {0}")]
    Transient(String),
    /// The account is not known to the token owner (removed, or never signed in on this machine).
    #[error("unknown account")]
    UnknownAccount,
}

/// A provider of access tokens.
#[async_trait::async_trait]
pub trait TokenSource: Send + Sync {
    /// A currently valid access token (cached, or refreshed when close to expiry).
    async fn access_token(&self) -> Result<AccessToken, AuthError>;

    /// The server answered 401 to `failed`: return a token that is not `failed` (refreshing once if nobody else
    /// already did), or an error. Called at most once per request.
    async fn after_unauthorized(&self, failed: &AccessToken) -> Result<AccessToken, AuthError>;
}

/// A fixed token (tests, tools). `after_unauthorized` reports the session as expired.
#[derive(Debug, Clone)]
pub struct StaticToken(pub AccessToken);

#[async_trait::async_trait]
impl TokenSource for StaticToken {
    async fn access_token(&self) -> Result<AccessToken, AuthError> {
        Ok(self.0.clone())
    }

    async fn after_unauthorized(&self, _failed: &AccessToken) -> Result<AccessToken, AuthError> {
        Err(AuthError::SessionExpired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_claims_are_read_without_verification() {
        // {"alg":"HS256"} . {"sub":"u-1","exp":1700000000} . sig
        let token = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1LTEiLCJleHAiOjE3MDAwMDAwMDB9.c2ln";
        let t = AccessToken::new(token);
        assert_eq!(t.jwt_expiry(), Some(1_700_000_000));
        assert_eq!(t.jwt_subject().as_deref(), Some("u-1"));
        assert_eq!(AccessToken::new("opaque").jwt_expiry(), None);
    }

    #[test]
    fn debug_is_redacted() {
        let t = AccessToken::new("secret-token-value");
        let dbg = format!("{t:?}");
        assert!(!dbg.contains("secret"));
        assert_eq!(t.fingerprint().len(), 16);
    }
}
