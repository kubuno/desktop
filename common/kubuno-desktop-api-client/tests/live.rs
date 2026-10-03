//! Optional live checks against a real Kubuno server. Skipped unless `KUBUNO_TEST_SERVER_URL` is set (for example
//! `https://dev.kubuno.com`). Without credentials they only exercise what an anonymous client may do; when
//! `KUBUNO_TEST_ACCESS_TOKEN` is also set (an access token the developer exported **in the environment**; this
//! test never reads a configuration file or a credential store), they also read one page of the notes and drive
//! feeds and check the KDP shape. Nothing is written.

use std::sync::Arc;

use kubuno_desktop_api_client::{AccessToken, ApiClient, ApiRequest, Cursor, ErrorClass, RetryPolicy, StaticToken};

fn server() -> Option<String> {
    std::env::var("KUBUNO_TEST_SERVER_URL").ok().filter(|s| !s.trim().is_empty())
}

#[tokio::test]
async fn live_health_and_anonymous_rejection() {
    let Some(base) = server() else {
        eprintln!("KUBUNO_TEST_SERVER_URL not set: live test skipped");
        return;
    };
    let anon = ApiClient::builder(&base).retry(RetryPolicy::none()).build().expect("client");
    let health = anon.send(ApiRequest::get("/healthz").unauthenticated()).await;
    assert!(health.is_ok(), "GET /healthz: {health:?}");
    // A protected route without a token: 401, classified Unauthorized.
    let err = anon.send(ApiRequest::get("/api/v1/me").unauthenticated()).await.expect_err("401 expected");
    assert_eq!(err.class(), ErrorClass::Unauthorized, "{err}");
}

#[tokio::test]
async fn live_feeds_have_the_kdp_shape() {
    let (Some(base), Ok(token)) = (server(), std::env::var("KUBUNO_TEST_ACCESS_TOKEN")) else {
        eprintln!("KUBUNO_TEST_SERVER_URL / KUBUNO_TEST_ACCESS_TOKEN not set: live feed test skipped");
        return;
    };
    let api = ApiClient::builder(&base).tokens(Arc::new(StaticToken(AccessToken::new(token)))).build().expect("client");
    for path in ["/api/v1/notes/notes/delta", "/api/v1/drive/sync/delta"] {
        match api.delta_page(path, &Cursor::zero(), 5).await {
            Ok(page) => {
                for c in &page.changes {
                    assert!(!c.uuid.is_empty(), "{path}: change without id");
                }
                eprintln!("{path}: {} changes, cursor {}, has_more {}", page.changes.len(), page.cursor, page.has_more);
            }
            // A module that is not installed on that server.
            Err(e) if e.class() == ErrorClass::NotFound => eprintln!("{path}: not installed ({e})"),
            Err(e) => panic!("{path}: {e}"),
        }
    }
}
