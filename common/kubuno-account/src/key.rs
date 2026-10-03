//! Account identity: **server URL + user id** (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §9). A local id minted at
//! sign-in would change after a data wipe and orphan every database and token (the Android "session expired" bug);
//! this key is the same on every run for the same person on the same server, and different for a different user
//! on the same server, so an outbox can never be sent as someone else.

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// `hex(sha256("<normalized server url>|<user id>"))[..16]`: a stable, filesystem- and credential-safe id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccountKey(String);

/// Errors building an account key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    #[error("the server URL must start with http:// or https:// and name a host")]
    BadServerUrl,
    #[error("the user id is empty")]
    EmptyUserId,
    #[error("'{0}' is not an account key (16 lowercase hex characters)")]
    BadKey(String),
}

/// Normalizes a server URL: lowercase scheme and host, default port dropped, no trailing slash, no query or
/// fragment, path prefix kept (a server mounted under `/kubuno` is another server than the root one).
pub fn normalize_server_url(url: &str) -> Result<String, KeyError> {
    let url = url.trim();
    let (scheme, rest) = url.split_once("://").ok_or(KeyError::BadServerUrl)?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(KeyError::BadServerUrl);
    }
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    // Drop credentials if someone pasted them in the URL.
    let authority = authority.rsplit('@').next().unwrap_or(authority).to_ascii_lowercase();
    if authority.is_empty() {
        return Err(KeyError::BadServerUrl);
    }
    let default_port = if scheme == "https" { ":443" } else { ":80" };
    let authority = authority.strip_suffix(default_port).map(str::to_string).unwrap_or(authority);
    let path = path.trim_end_matches('/');
    Ok(format!("{scheme}://{authority}{path}"))
}

impl AccountKey {
    pub fn new(server_url: &str, user_id: &str) -> Result<Self, KeyError> {
        let server = normalize_server_url(server_url)?;
        let user = user_id.trim();
        if user.is_empty() {
            return Err(KeyError::EmptyUserId);
        }
        let digest = Sha256::digest(format!("{server}|{}", user.to_ascii_lowercase()).as_bytes());
        Ok(Self(hex::encode(&digest[..8])))
    }

    /// Parses a key read from disk or from the broker (validated: it becomes a directory and credential name).
    pub fn parse(s: &str) -> Result<Self, KeyError> {
        if s.len() == 16 && s.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)) {
            Ok(Self(s.to_string()))
        } else {
            Err(KeyError::BadKey(s.chars().take(40).collect()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AccountKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization() {
        assert_eq!(normalize_server_url("HTTPS://Dev.Kubuno.com:443/").as_deref(), Ok("https://dev.kubuno.com"));
        assert_eq!(normalize_server_url("http://host:80/kubuno/?x=1").as_deref(), Ok("http://host/kubuno"));
        assert_eq!(normalize_server_url("http://host:8080").as_deref(), Ok("http://host:8080"));
        assert_eq!(normalize_server_url("https://user:pw@host").as_deref(), Ok("https://host"));
        assert!(normalize_server_url("ftp://host").is_err());
        assert!(normalize_server_url("host").is_err());
    }

    #[test]
    fn key_is_stable_and_distinct() {
        let a = AccountKey::new("https://dev.kubuno.com/", "0f8c-USER").expect("key");
        let b = AccountKey::new("HTTPS://dev.kubuno.com:443", "0f8c-user").expect("key");
        assert_eq!(a, b);
        assert_eq!(a.as_str().len(), 16);
        let other_user = AccountKey::new("https://dev.kubuno.com", "another").expect("key");
        let other_server = AccountKey::new("https://other.example", "0f8c-user").expect("key");
        assert_ne!(a, other_user);
        assert_ne!(a, other_server);
        assert_eq!(AccountKey::parse(a.as_str()), Ok(a));
        assert!(AccountKey::parse("../etc").is_err());
        assert!(AccountKey::new("https://x", " ").is_err());
    }
}
