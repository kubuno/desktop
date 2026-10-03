//! Sign-in against the core's native flow (`client_type = "desktop"`: tokens in the JSON body), including the TOTP
//! step the old desktop client did not support.
//!
//! - `POST /api/v1/auth/login {login, password, client_type, device_name, device_type}` ->
//!   `{access_token, refresh_token, refresh_expires_at, user}` or `{requires_totp: true, totp_session}`;
//! - `POST /api/v1/auth/totp {code | backup_code, totp_session, client_type}` -> the same tokens;
//! - `POST /api/v1/auth/refresh {refresh_token}` -> a rotated pair (`handlers/auth/refresh.rs`);
//! - `GET /api/v1/me` -> `{user: {id, display_name, email, …}}`.

use std::fmt;

use kubuno_api_client::{AccessToken, ApiClient, ApiError, ApiRequest, RetryPolicy};
use kubuno_secrets::Secret;
use serde::Deserialize;

/// A token pair as the core returns it to a native client.
pub struct NativeTokens {
    pub access_token: AccessToken,
    pub refresh_token: Secret,
}

impl fmt::Debug for NativeTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeTokens").field("access_token", &self.access_token).field("refresh_token", &"<redacted>").finish()
    }
}

#[derive(Deserialize)]
struct RawTokens {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct RawLogin {
    #[serde(default)]
    requires_totp: bool,
    #[serde(default)]
    totp_session: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
}

/// What a login answered.
#[derive(Debug)]
pub enum LoginOutcome {
    SignedIn(NativeTokens),
    /// Ask the user for a TOTP (or backup) code, then call [`login_totp`].
    TotpRequired { totp_session: String },
}

/// The second factor.
#[derive(Debug, Clone)]
pub enum TotpCode {
    Code(String),
    Backup(String),
}

fn device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "Kubuno Desktop".to_string())
}

fn tokens_from(access: Option<String>, refresh: Option<String>) -> Result<NativeTokens, ApiError> {
    match (access, refresh) {
        (Some(a), Some(r)) if !a.is_empty() && !r.is_empty() => {
            Ok(NativeTokens { access_token: AccessToken::new(a), refresh_token: Secret::from_string(r) })
        }
        _ => Err(ApiError::Decode("the server did not return a native token pair".to_string())),
    }
}

/// Step 1 of the sign-in. Not retried: a password is never replayed automatically.
pub async fn login(api: &ApiClient, login: &str, password: &str) -> Result<LoginOutcome, ApiError> {
    let req = ApiRequest::post("/api/v1/auth/login")
        .unauthenticated()
        .retry(RetryPolicy::none())
        .json(serde_json::json!({
            "login": login,
            "password": password,
            "client_type": "desktop",
            "device_name": device_name(),
            "device_type": "desktop",
        }));
    let raw: RawLogin = api.send(req).await?.json()?;
    if raw.requires_totp {
        let totp_session = raw.totp_session.ok_or_else(|| ApiError::Decode("requires_totp without totp_session".to_string()))?;
        return Ok(LoginOutcome::TotpRequired { totp_session });
    }
    Ok(LoginOutcome::SignedIn(tokens_from(raw.access_token, raw.refresh_token)?))
}

/// Step 2 of the sign-in, when the account has two-factor authentication.
pub async fn login_totp(api: &ApiClient, totp_session: &str, code: &TotpCode) -> Result<NativeTokens, ApiError> {
    let mut body = serde_json::json!({ "totp_session": totp_session, "client_type": "desktop" });
    match code {
        TotpCode::Code(c) => body["code"] = serde_json::Value::String(c.trim().to_string()),
        TotpCode::Backup(c) => body["backup_code"] = serde_json::Value::String(c.trim().to_string()),
    }
    let req = ApiRequest::post("/api/v1/auth/totp").unauthenticated().retry(RetryPolicy::none()).json(body);
    let raw: RawTokens = api.send(req).await?.json()?;
    tokens_from(Some(raw.access_token), raw.refresh_token)
}

/// The refresh call. Not retried by the client: the token owner decides (cooldown, grace window).
pub(crate) async fn refresh(api: &ApiClient, refresh_token: &Secret) -> Result<NativeTokens, ApiError> {
    let token = refresh_token.expose_str().map_err(|_| ApiError::InvalidRequest("the stored refresh token is not text".to_string()))?;
    let req = ApiRequest::post("/api/v1/auth/refresh")
        .unauthenticated()
        .retry(RetryPolicy::none())
        .json(serde_json::json!({ "refresh_token": token }));
    let raw: RawTokens = api.send(req).await?.json()?;
    // A web-style answer has no refresh token: keep the stored one (no rotation happened).
    let refresh = raw.refresh_token.unwrap_or_else(|| token.to_string());
    tokens_from(Some(raw.access_token), Some(refresh))
}

/// Who the token belongs to.
#[derive(Debug, Clone, Deserialize)]
pub struct Identity {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Deserialize)]
struct MeEnvelope {
    user: Identity,
}

/// `GET /api/v1/me` with an explicit access token (before the account exists, so before any token source).
pub async fn fetch_identity(api: &ApiClient, access: &AccessToken) -> Result<Identity, ApiError> {
    let req = ApiRequest::get("/api/v1/me").unauthenticated().header("authorization", format!("Bearer {}", access.expose()));
    let me: MeEnvelope = api.send(req).await?.json()?;
    if me.user.id.trim().is_empty() {
        return Err(ApiError::Decode("GET /me returned an empty user id".to_string()));
    }
    Ok(me.user)
}
