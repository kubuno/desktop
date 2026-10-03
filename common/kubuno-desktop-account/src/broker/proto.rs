//! The broker's wire protocol, version 1: one JSON object per line (UTF-8, `\n`-terminated, at most
//! [`MAX_LINE`] bytes) over a local stream (a named pipe on Windows, a Unix domain socket elsewhere).
//!
//! A connection starts with `{"op":"hello","version":1,"app":"<name>"}` answered by `{"type":"hello","version":1}`.
//! Then each request line gets exactly one response line, in order. After `{"op":"subscribe"}` (answered by
//! `{"type":"ok"}`) the connection only carries `{"type":"event",…}` lines from the server until either side
//! closes it.
//!
//! Requests: `access_token {account}`, `access_after_401 {account, failed}` (`failed` = fingerprint of the
//! rejected token, never the token), `accounts`, `switch_account {account}`, `database_key {account}`,
//! `subscribe`. Errors: `{"type":"error","code":"session_expired"|"transient"|"unknown_account"|"bad_request"|
//! "forbidden"|"version","message":…}`. Refresh tokens never cross the broker.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::key::AccountKey;
use crate::owner::{AccountEvent, AccountSummary};

pub const PROTOCOL_VERSION: u32 = 1;
/// Longest accepted line (a request or a response). Larger lines close the connection.
pub const MAX_LINE: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Hello { version: u32, app: String },
    AccessToken { account: AccountKey },
    #[serde(rename = "access_after_401")]
    AccessAfter401 { account: AccountKey, failed: String },
    Accounts,
    SwitchAccount { account: AccountKey },
    DatabaseKey { account: AccountKey },
    Subscribe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    SessionExpired,
    Transient,
    UnknownAccount,
    BadRequest,
    Forbidden,
    Version,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Hello { version: u32 },
    Token { token: String, valid_for_s: u64 },
    Accounts { accounts: Vec<AccountSummary>, current: Option<AccountKey> },
    Secret { value: String },
    Ok,
    Event { event: AccountEvent },
    Error { code: ErrorCode, message: String },
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Response::Hello { version } => write!(f, "Hello(v{version})"),
            Response::Token { valid_for_s, .. } => write!(f, "Token(<redacted>, {valid_for_s}s)"),
            Response::Accounts { accounts, current } => write!(f, "Accounts({} accounts, current {current:?})", accounts.len()),
            Response::Secret { .. } => f.write_str("Secret(<redacted>)"),
            Response::Ok => f.write_str("Ok"),
            Response::Event { event } => write!(f, "Event({event:?})"),
            Response::Error { code, message } => write!(f, "Error({code:?}: {message})"),
        }
    }
}

/// Encodes one frame (JSON + `\n`).
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut v = serde_json::to_vec(value)?;
    v.push(b'\n');
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_shapes() {
        let key = AccountKey::parse("0123456789abcdef").expect("key");
        let req = Request::AccessAfter401 { account: key, failed: "abcd".into() };
        let text = String::from_utf8(encode(&req).expect("encode")).expect("utf8");
        assert_eq!(text, "{\"op\":\"access_after_401\",\"account\":\"0123456789abcdef\",\"failed\":\"abcd\"}\n");
        let resp: Response = serde_json::from_str(r#"{"type":"error","code":"session_expired","message":"x"}"#).expect("parse");
        assert_eq!(resp, Response::Error { code: ErrorCode::SessionExpired, message: "x".into() });
        let tok = Response::Token { token: "secret".into(), valid_for_s: 3 };
        assert!(!format!("{tok:?}").contains("secret"));
    }
}
