//! Two processes: this test starts a fake server, a token owner and the broker, then runs **this same test
//! binary** again as a child (an "app") which borrows a token through the named pipe / Unix socket, has it
//! rejected, gets a refreshed one, and checks both against the fake server. The child part is the
//! `child_app_process` test, a no-op unless the parent set `KUBUNO_BROKER_CHILD_ENDPOINT`.

mod fake_auth;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use kubuno_desktop_account::broker::{BrokerClient, BrokerEndpoint, BrokerServer, ClientPolicy};
use kubuno_desktop_account::login::NativeTokens;
use kubuno_desktop_account::{AccountKey, AccountStore, OwnerConfig, TokenOwner};
use kubuno_desktop_api_client::{AccessToken, ApiClient, ApiRequest, TokenSource};
use kubuno_desktop_secrets::{MemorySecretStore, Secret};

const ENV_ENDPOINT: &str = "KUBUNO_BROKER_CHILD_ENDPOINT";
const ENV_ACCOUNT: &str = "KUBUNO_BROKER_CHILD_ACCOUNT";
const ENV_SERVER: &str = "KUBUNO_BROKER_CHILD_SERVER";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_app_process_borrows_tokens_from_the_shell_process() {
    if std::env::var_os(ENV_ENDPOINT).is_some() {
        return; // We are the child: only `child_app_process` runs.
    }
    let (base, state) = fake_auth::start().await;
    let dir = tempfile::tempdir().expect("tmp");
    let owner = TokenOwner::new(Arc::new(MemorySecretStore::new()), AccountStore::new(dir.path()), OwnerConfig::default());
    let (a, r) = state.login("user-7");
    let key = owner
        .sign_in(&base, NativeTokens { access_token: AccessToken::new(a), refresh_token: Secret::from_string(r) })
        .await
        .expect("sign in")
        .key;

    let endpoint = BrokerEndpoint::for_test("ipc", dir.path());
    // Only programs from the directory of this test binary may connect (the child is the same binary).
    let exe = std::env::current_exe().expect("exe");
    let exe_dir = exe.parent().expect("dir").to_path_buf();
    let policy = ClientPolicy::ImagesUnder(vec![std::fs::canonicalize(&exe_dir).unwrap_or(exe_dir)]);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(BrokerServer::new(endpoint.clone(), owner.clone(), policy).serve(async move {
        let _ = stop_rx.await;
    }));
    tokio::time::sleep(Duration::from_millis(100)).await;

    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Diagnostics::Debug::{SetErrorMode, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX};
        // SAFETY: changes this process's (and its children's) error mode only: no error dialog box.
        unsafe { SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX) };
    }
    let out = tokio::process::Command::new(&exe)
        .args(["child_app_process", "--exact", "--nocapture", "--test-threads=1"])
        .env(ENV_ENDPOINT, endpoint.to_arg())
        .env(ENV_ACCOUNT, key.as_str())
        .env(ENV_SERVER, &base)
        .output()
        .await
        .expect("spawn child");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "child failed:\n{stdout}\n{stderr}");
    assert!(stdout.contains("CHILD-OK"), "child did not report:\n{stdout}\n{stderr}");
    // The child's 401 caused exactly one rotation, done by this (the owner's) process.
    assert_eq!(state.rotations.load(Ordering::SeqCst), 1);
    assert_eq!(owner.refresh_count(), 1);
    assert!(!state.family_revoked_for("user-7"));
    let _ = stop_tx.send(());
    let _ = server.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn child_app_process() {
    let Some(endpoint) = std::env::var_os(ENV_ENDPOINT) else { return };
    let endpoint = BrokerEndpoint::from_arg(&endpoint.to_string_lossy());
    let account = AccountKey::parse(&std::env::var(ENV_ACCOUNT).expect("account")).expect("key");
    let server = std::env::var(ENV_SERVER).expect("server");

    let client = BrokerClient::new(endpoint, "child-app");
    let (accounts, current) = client.accounts().await.expect("accounts");
    assert_eq!(accounts.len(), 1);
    assert_eq!(current.as_ref(), Some(&account));

    let source = Arc::new(client.token_source(account));
    let first = source.access_token().await.expect("borrowed");
    let api = ApiClient::builder(&server).tokens(source.clone()).build().expect("api");
    api.send(ApiRequest::get("/api/v1/protected")).await.expect("works with the borrowed token");

    // Pretend the server rejected it: the broker refreshes once in the owner's process.
    let second = source.after_unauthorized(&first).await.expect("refreshed through the broker");
    assert_ne!(first, second);
    api.send(ApiRequest::get("/api/v1/protected")).await.expect("works with the refreshed token");
    println!("CHILD-OK {}", second.fingerprint());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_program_outside_the_allowed_directories_is_refused() {
    if std::env::var_os(ENV_ENDPOINT).is_some() {
        return;
    }
    let (base, state) = fake_auth::start().await;
    let dir = tempfile::tempdir().expect("tmp");
    let owner = TokenOwner::new(Arc::new(MemorySecretStore::new()), AccountStore::new(dir.path()), OwnerConfig::default());
    let (a, r) = state.login("user-8");
    let key = owner
        .sign_in(&base, NativeTokens { access_token: AccessToken::new(a), refresh_token: Secret::from_string(r) })
        .await
        .expect("sign in")
        .key;
    let endpoint = BrokerEndpoint::for_test("refuse", dir.path());
    // Only the (empty) temp directory is allowed: the test binary is not under it.
    let policy = ClientPolicy::ImagesUnder(vec![dir.path().join("install")]);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(BrokerServer::new(endpoint.clone(), owner, policy).serve(async move {
        let _ = stop_rx.await;
    }));
    tokio::time::sleep(Duration::from_millis(100)).await;
    let out = tokio::process::Command::new(std::env::current_exe().expect("exe"))
        .args(["child_app_process", "--exact", "--nocapture", "--test-threads=1"])
        .env(ENV_ENDPOINT, endpoint.to_arg())
        .env(ENV_ACCOUNT, key.as_str())
        .env(ENV_SERVER, &base)
        .output()
        .await
        .expect("spawn child");
    assert!(!out.status.success(), "the child must be refused");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("CHILD-OK"));
    assert_eq!(state.rotations.load(Ordering::SeqCst), 0);
    let _ = stop_tx.send(());
    let _ = server.await;
}
