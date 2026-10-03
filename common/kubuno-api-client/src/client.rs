//! The HTTP client.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE, ETAG, RETRY_AFTER};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;

use crate::delta::{check_progress, Change, Cursor, DeltaPage};
use crate::error::{ApiError, ErrorClass};
use crate::token::{AccessToken, AuthError, TokenSource};

/// Header carrying the idempotency key of a write (KDP §7.1 rule 7).
pub const IDEMPOTENCY_KEY: &str = "idempotency-key";
/// Header the server sets on a replayed idempotent answer.
pub const IDEMPOTENCY_REPLAYED: &str = "idempotency-replayed";
/// Header identifying the device (`X-Kubuno-Device-Key`).
pub const DEVICE_KEY: &str = "x-kubuno-device-key";

/// Retries done by the client itself, within one call. Long-term retries (minutes, hours) belong to the outbox.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Attempts in total (1 = no retry).
    pub max_attempts: u32,
    /// First delay; doubled at each attempt.
    pub base_delay: Duration,
    /// Upper bound of a delay (also caps `Retry-After`).
    pub max_delay: Duration,
    /// Random spread, as a fraction of the delay (0.2 = ±20 %).
    pub jitter: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self { max_attempts: 4, base_delay: Duration::from_millis(500), max_delay: Duration::from_secs(30), jitter: 0.2 }
    }
}

impl RetryPolicy {
    /// No retry at all.
    pub fn none() -> Self {
        Self { max_attempts: 1, ..Self::default() }
    }

    /// The delay before attempt `attempt + 1` (`attempt` starting at 1), honouring `Retry-After`.
    pub fn delay(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        if let Some(ra) = retry_after {
            return ra.min(self.max_delay);
        }
        let exp = self.base_delay.saturating_mul(2u32.saturating_pow(attempt.saturating_sub(1)));
        let capped = exp.min(self.max_delay);
        jittered(capped, self.jitter)
    }
}

/// `d` ± `fraction`, from the OS random source (no clock involved).
pub fn jittered(d: Duration, fraction: f64) -> Duration {
    if fraction <= 0.0 {
        return d;
    }
    let mut b = [0u8; 4];
    if getrandom::getrandom(&mut b).is_err() {
        return d;
    }
    let unit = f64::from(u32::from_le_bytes(b)) / f64::from(u32::MAX); // 0..=1
    let factor = 1.0 + fraction * (unit * 2.0 - 1.0);
    d.mul_f64(factor.max(0.0))
}

/// The body of a request.
#[derive(Debug, Clone, Default)]
pub enum Body {
    #[default]
    None,
    Json(serde_json::Value),
    Bytes { data: Vec<u8>, content_type: String },
}

/// One request. Build it with the constructors and `with_*` methods, send it with [`ApiClient::send`].
#[derive(Clone)]
pub struct ApiRequest {
    pub method: Method,
    /// Server-relative path (`/api/v1/notes/notes/delta`).
    pub path: String,
    pub query: Vec<(String, String)>,
    pub body: Body,
    /// `If-Match: <etag>` (the server answers 412 with the current row when it moved).
    pub if_match: Option<String>,
    /// `Idempotency-Key`. A request with a key is retried even if it is not idempotent by method.
    pub idempotency_key: Option<String>,
    pub headers: Vec<(String, String)>,
    /// Whether to send `Authorization` (false for login, refresh, health).
    pub authenticated: bool,
    /// Overrides the client's retry policy for this request.
    pub retry: Option<RetryPolicy>,
    pub timeout: Option<Duration>,
}

impl fmt::Debug for ApiRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // No body, no header values: they may hold user data or secrets.
        f.debug_struct("ApiRequest")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("if_match", &self.if_match.is_some())
            .field("idempotency_key", &self.idempotency_key)
            .field("authenticated", &self.authenticated)
            .finish()
    }
}

impl ApiRequest {
    pub fn new(method: Method, path: impl Into<String>) -> Self {
        Self {
            method,
            path: path.into(),
            query: Vec::new(),
            body: Body::None,
            if_match: None,
            idempotency_key: None,
            headers: Vec::new(),
            authenticated: true,
            retry: None,
            timeout: None,
        }
    }

    pub fn get(path: impl Into<String>) -> Self {
        Self::new(Method::GET, path)
    }

    pub fn post(path: impl Into<String>) -> Self {
        Self::new(Method::POST, path)
    }

    pub fn put(path: impl Into<String>) -> Self {
        Self::new(Method::PUT, path)
    }

    pub fn patch(path: impl Into<String>) -> Self {
        Self::new(Method::PATCH, path)
    }

    pub fn delete(path: impl Into<String>) -> Self {
        Self::new(Method::DELETE, path)
    }

    pub fn query(mut self, key: impl Into<String>, value: impl ToString) -> Self {
        self.query.push((key.into(), value.to_string()));
        self
    }

    pub fn json(mut self, body: serde_json::Value) -> Self {
        self.body = Body::Json(body);
        self
    }

    pub fn bytes(mut self, data: Vec<u8>, content_type: impl Into<String>) -> Self {
        self.body = Body::Bytes { data, content_type: content_type.into() };
        self
    }

    pub fn if_match(mut self, etag: impl Into<String>) -> Self {
        self.if_match = Some(etag.into());
        self
    }

    pub fn idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn unauthenticated(mut self) -> Self {
        self.authenticated = false;
        self
    }

    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.retry = Some(policy);
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Whether replaying the request is harmless: safe/idempotent method, or an idempotency key.
    pub fn is_replayable(&self) -> bool {
        self.idempotency_key.is_some()
            || matches!(self.method, Method::GET | Method::HEAD | Method::OPTIONS | Method::PUT | Method::DELETE)
    }
}

/// A successful answer.
#[derive(Debug, Clone)]
pub struct ApiResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl ApiResponse {
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, ApiError> {
        if self.body.is_empty() {
            return serde_json::from_str("null").map_err(|e| ApiError::Decode(e.to_string()));
        }
        serde_json::from_slice(&self.body).map_err(|e| ApiError::Decode(format!("line {} column {}: {}", e.line(), e.column(), e.classify_name())))
    }

    /// The JSON body, `Null` when empty or not JSON.
    pub fn json_value(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }

    pub fn etag(&self) -> Option<String> {
        self.headers.get(ETAG).and_then(|v| v.to_str().ok()).map(|s| s.trim_matches('"').to_string())
    }

    /// Whether the server replayed a stored idempotent answer.
    pub fn idempotency_replayed(&self) -> bool {
        self.headers.get(IDEMPOTENCY_REPLAYED).and_then(|v| v.to_str().ok()).is_some_and(|v| v.eq_ignore_ascii_case("true"))
    }
}

trait ClassifyName {
    fn classify_name(&self) -> &'static str;
}

impl ClassifyName for serde_json::Error {
    fn classify_name(&self) -> &'static str {
        match self.classify() {
            serde_json::error::Category::Io => "io",
            serde_json::error::Category::Syntax => "syntax",
            serde_json::error::Category::Data => "data does not match the expected type",
            serde_json::error::Category::Eof => "truncated",
        }
    }
}

/// Builder of an [`ApiClient`].
pub struct ApiClientBuilder {
    base_url: String,
    tokens: Option<Arc<dyn TokenSource>>,
    retry: RetryPolicy,
    timeout: Duration,
    connect_timeout: Duration,
    user_agent: String,
    device_key: Option<String>,
    proxy: Option<String>,
}

impl ApiClientBuilder {
    pub fn tokens(mut self, tokens: Arc<dyn TokenSource>) -> Self {
        self.tokens = Some(tokens);
        self
    }

    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = ua.into();
        self
    }

    pub fn device_key(mut self, key: impl Into<String>) -> Self {
        self.device_key = Some(key.into());
        self
    }

    /// Outbound proxy (`http://host:port`), the `settings.json` `proxy` of the desktop.
    pub fn proxy(mut self, url: Option<String>) -> Self {
        self.proxy = url.filter(|u| !u.trim().is_empty());
        self
    }

    pub fn build(self) -> Result<ApiClient, ApiError> {
        let base = self.base_url.trim_end_matches('/').to_string();
        if !(base.starts_with("http://") || base.starts_with("https://")) {
            return Err(ApiError::InvalidRequest("the server URL must start with http:// or https://".to_string()));
        }
        let mut builder = reqwest::Client::builder()
            .user_agent(self.user_agent)
            .timeout(self.timeout)
            .connect_timeout(self.connect_timeout);
        if let Some(p) = self.proxy {
            let proxy = reqwest::Proxy::all(&p).map_err(|e| ApiError::InvalidRequest(format!("proxy: {e}")))?;
            builder = builder.proxy(proxy);
        }
        let http = builder.build().map_err(|e| ApiError::InvalidRequest(format!("HTTP client: {e}")))?;
        Ok(ApiClient { inner: Arc::new(Inner { http, base, tokens: self.tokens, retry: self.retry, device_key: self.device_key }) })
    }
}

struct Inner {
    http: reqwest::Client,
    base: String,
    tokens: Option<Arc<dyn TokenSource>>,
    retry: RetryPolicy,
    device_key: Option<String>,
}

/// The client of one server. Cheap to clone.
#[derive(Clone)]
pub struct ApiClient {
    inner: Arc<Inner>,
}

impl fmt::Debug for ApiClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiClient")
            .field("base", &self.inner.base)
            .field("authenticated", &self.inner.tokens.is_some())
            .field("device_key", &self.inner.device_key.as_ref().map(|_| "<set>"))
            .finish()
    }
}

/// The `User-Agent` of desktop requests.
pub fn default_user_agent() -> String {
    format!("Kubuno-Desktop/{} ({})", env!("CARGO_PKG_VERSION"), std::env::consts::OS)
}

enum Attempt {
    Done(ApiResponse),
    Failed(ApiError),
}

impl ApiClient {
    pub fn builder(base_url: impl Into<String>) -> ApiClientBuilder {
        ApiClientBuilder {
            base_url: base_url.into(),
            tokens: None,
            retry: RetryPolicy::default(),
            timeout: Duration::from_secs(60),
            connect_timeout: Duration::from_secs(10),
            user_agent: default_user_agent(),
            device_key: None,
            proxy: None,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.inner.base
    }

    /// Sends `req`: access token from the token source, one refresh-and-retry on 401, retries with backoff on
    /// transient failures when the request is replayable (safe method or idempotency key).
    pub async fn send(&self, req: ApiRequest) -> Result<ApiResponse, ApiError> {
        let policy = req.retry.clone().unwrap_or_else(|| self.inner.retry.clone());
        let max_attempts = if req.is_replayable() { policy.max_attempts.max(1) } else { 1 };
        let mut token = if req.authenticated { Some(self.token().await?) } else { None };
        let mut refreshed = false;
        let mut attempt = 1u32;
        loop {
            let outcome = self.attempt(&req, token.as_ref()).await;
            let err = match outcome {
                Attempt::Done(resp) => return Ok(resp),
                Attempt::Failed(e) => e,
            };
            // 401: one refresh through the token source, then the same request again (not counted as an attempt).
            if err.status() == Some(401) && req.authenticated && !refreshed {
                refreshed = true;
                if let (Some(tokens), Some(failed)) = (self.inner.tokens.as_ref(), token.as_ref()) {
                    tracing::debug!(path = %req.path, token = %failed.fingerprint(), "401: asking the token source for a fresh token");
                    token = Some(tokens.after_unauthorized(failed).await?);
                    continue;
                }
                return Err(ApiError::Auth(AuthError::SessionExpired));
            }
            if err.class() != ErrorClass::Transient || attempt >= max_attempts {
                return Err(err);
            }
            let delay = policy.delay(attempt, err.retry_after());
            tracing::debug!(path = %req.path, attempt, delay_ms = delay.as_millis() as u64, error = %err, "transient failure, retrying");
            tokio::time::sleep(delay).await;
            attempt += 1;
        }
    }

    async fn token(&self) -> Result<AccessToken, ApiError> {
        match self.inner.tokens.as_ref() {
            Some(t) => Ok(t.access_token().await?),
            None => Err(ApiError::Auth(AuthError::UnknownAccount)),
        }
    }

    async fn attempt(&self, req: &ApiRequest, token: Option<&AccessToken>) -> Attempt {
        let url = format!("{}{}", self.inner.base, req.path);
        let mut rb = self.inner.http.request(req.method.clone(), &url);
        if !req.query.is_empty() {
            rb = rb.query(&req.query);
        }
        if let Some(t) = token {
            let value = match HeaderValue::from_str(&format!("Bearer {}", t.expose())) {
                Ok(mut v) => {
                    v.set_sensitive(true);
                    v
                }
                Err(_) => return Attempt::Failed(ApiError::InvalidRequest("the access token is not a valid header value".to_string())),
            };
            rb = rb.header(AUTHORIZATION, value);
        }
        if let Some(etag) = &req.if_match {
            rb = rb.header(reqwest::header::IF_MATCH, etag.as_str());
        }
        if let Some(key) = &req.idempotency_key {
            rb = rb.header(IDEMPOTENCY_KEY, key.as_str());
        }
        if let Some(dk) = &self.inner.device_key {
            rb = rb.header(DEVICE_KEY, dk.as_str());
        }
        for (name, value) in &req.headers {
            match (HeaderName::from_bytes(name.as_bytes()), HeaderValue::from_str(value)) {
                (Ok(n), Ok(v)) => rb = rb.header(n, v),
                _ => return Attempt::Failed(ApiError::InvalidRequest(format!("invalid header {name}"))),
            }
        }
        match &req.body {
            Body::None => {}
            Body::Json(v) => rb = rb.json(v),
            Body::Bytes { data, content_type } => {
                rb = rb.header(CONTENT_TYPE, content_type.as_str()).body(data.clone());
            }
        }
        if let Some(t) = req.timeout {
            rb = rb.timeout(t);
        }
        let resp = match rb.send().await {
            Ok(r) => r,
            Err(e) => return Attempt::Failed(network_error(&e)),
        };
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = match resp.bytes().await {
            Ok(b) => b.to_vec(),
            // The status line arrived: the server did process the request.
            Err(e) => return Attempt::Failed(ApiError::Network { message: describe(&e), maybe_sent: true }),
        };
        tracing::debug!(method = %req.method, path = %req.path, status = status.as_u16(), "kubuno api");
        if status.is_success() {
            return Attempt::Done(ApiResponse { status: status.as_u16(), headers, body });
        }
        Attempt::Failed(http_error(status, &headers, &body))
    }

    /// `GET path` decoded as `T`.
    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        self.send(ApiRequest::get(path)).await?.json()
    }

    /// One page of a KDP feed.
    pub async fn delta_page(&self, path: &str, cursor: &Cursor, limit: u32) -> Result<DeltaPage<Change>, ApiError> {
        self.delta_page_with(ApiRequest::get(path), cursor, limit).await
    }

    /// One page of a KDP feed, from a prepared request (extra query parameters such as drive's `full=true`).
    pub async fn delta_page_with(&self, req: ApiRequest, cursor: &Cursor, limit: u32) -> Result<DeltaPage<Change>, ApiError> {
        let page: DeltaPage<Change> = self.send(req.query("cursor", cursor.as_str()).query("limit", limit)).await?.json()?;
        check_progress(cursor, &page.cursor, page.has_more).map_err(ApiError::Protocol)?;
        Ok(page)
    }

    /// Every page from `cursor` to the end of the feed, with the progress guard (for tools and tests; the sync
    /// engine applies each page in its own transaction instead of collecting).
    pub async fn delta_all(&self, path: &str, mut cursor: Cursor, limit: u32, max_pages: u32) -> Result<(Vec<Change>, Cursor), ApiError> {
        let mut out = Vec::new();
        for _ in 0..max_pages.max(1) {
            let page = self.delta_page(path, &cursor, limit).await?;
            out.extend(page.changes);
            cursor = page.cursor;
            if !page.has_more {
                return Ok((out, cursor));
            }
        }
        Err(ApiError::Protocol(format!("the feed {path} did not end within {max_pages} pages")))
    }
}

fn describe(e: &reqwest::Error) -> String {
    // reqwest's Display includes the URL (no secret: tokens never go in URLs here) and the cause.
    let mut s = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(inner) = src {
        s.push_str(": ");
        s.push_str(&inner.to_string());
        src = inner.source();
    }
    s
}

fn network_error(e: &reqwest::Error) -> ApiError {
    // A connection that never opened cannot have delivered the request; anything else (timeout, reset while
    // waiting for the answer) may have.
    let maybe_sent = !e.is_connect() && !e.is_builder();
    if e.is_builder() {
        return ApiError::InvalidRequest(describe(e));
    }
    ApiError::Network { message: describe(e), maybe_sent }
}

fn http_error(status: StatusCode, headers: &HeaderMap, body: &[u8]) -> ApiError {
    let json: Option<serde_json::Value> = serde_json::from_slice(body).ok();
    let field = |k: &str| json.as_ref().and_then(|v| v.get(k)).and_then(|v| v.as_str()).map(str::to_string);
    let code = field("error").or_else(|| field("code"));
    let message = field("message");
    let retry_after = headers
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(Duration::from_secs);
    ApiError::Http { status: status.as_u16(), code, message, retry_after, body: json }
}

/// A new random idempotency key (UUID v4: never derived from a clock, so a replay of an old key cannot happen).
pub fn new_idempotency_key() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        let p = RetryPolicy { max_attempts: 10, base_delay: Duration::from_millis(100), max_delay: Duration::from_secs(1), jitter: 0.0 };
        assert_eq!(p.delay(1, None), Duration::from_millis(100));
        assert_eq!(p.delay(2, None), Duration::from_millis(200));
        assert_eq!(p.delay(3, None), Duration::from_millis(400));
        assert_eq!(p.delay(8, None), Duration::from_secs(1));
        assert_eq!(p.delay(1, Some(Duration::from_secs(5))), Duration::from_secs(1));
        assert_eq!(p.delay(1, Some(Duration::from_millis(50))), Duration::from_millis(50));
    }

    #[test]
    fn jitter_stays_in_range() {
        for _ in 0..100 {
            let d = jittered(Duration::from_millis(1000), 0.2);
            assert!(d >= Duration::from_millis(799) && d <= Duration::from_millis(1201), "{d:?}");
        }
    }

    #[test]
    fn replayable_requests() {
        assert!(ApiRequest::get("/x").is_replayable());
        assert!(!ApiRequest::post("/x").is_replayable());
        assert!(ApiRequest::post("/x").idempotency_key("k").is_replayable());
        assert!(!ApiRequest::patch("/x").is_replayable());
    }

    #[test]
    fn request_debug_hides_body() {
        let r = ApiRequest::post("/x").json(serde_json::json!({"password": "hunter2"}));
        assert!(!format!("{r:?}").contains("hunter2"));
    }
}
