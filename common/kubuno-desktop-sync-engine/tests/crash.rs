//! Crash safety: the engine runs in a **child process** (this test binary run again on `crash_child`), stops at a
//! named failpoint, and the parent kills it hard (`TerminateProcess` on Windows, `SIGKILL` elsewhere, through
//! `tokio::process::Child::kill`) — no destructor, no flush. Then the parent reopens the database and checks:
//! integrity, cursor never ahead of the applied rows, the op present or applied exactly once (the fake server counts
//! executions per idempotency key), and that a normal run completes the work.

mod fake_server;
mod support;

use std::io::Write;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use support::{cursor, fields, rows, Setup, FEED};
use tokio::io::{AsyncBufReadExt, BufReader};

const ENV_POINT: &str = "KUBUNO_CRASH_POINT";
const ENV_DIR: &str = "KUBUNO_CRASH_DIR";
const ENV_BASE: &str = "KUBUNO_CRASH_BASE";
const U1: &str = "user-1";

/// Runs the child until it reports the failpoint, then kills it.
async fn crash_at(point: &str, dir: &std::path::Path, base: &str) {
    let mut child = tokio::process::Command::new(std::env::current_exe().expect("exe"))
        .args(["crash_child", "--exact", "--nocapture", "--test-threads=1"])
        .env(ENV_POINT, point)
        .env(ENV_DIR, dir)
        .env(ENV_BASE, base)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn");
    let stdout = child.stdout.take().expect("stdout");
    let mut lines = BufReader::new(stdout).lines();
    let reached = tokio::time::timeout(Duration::from_secs(60), async {
        while let Ok(Some(line)) = lines.next_line().await {
            if line.contains(&format!("AT:{point}")) {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    assert!(reached, "the child never reached {point}");
    child.kill().await.expect("kill -9");
    let _ = child.wait().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crash_child() {
    let Ok(point) = std::env::var(ENV_POINT) else { return };
    let dir = std::path::PathBuf::from(std::env::var(ENV_DIR).expect("dir"));
    let base = std::env::var(ENV_BASE).expect("base");
    let engine = Setup::new(&base, U1).open(&dir).await;
    let target = point.clone();
    engine.set_failpoint(Some(Arc::new(move |p: &str| {
        if p == target {
            let mut out = std::io::stdout();
            let _ = writeln!(out, "AT:{p}");
            let _ = out.flush();
            // Wait to be killed, inside the open transaction / after the request.
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    })));
    let _ = engine.sync_once().await;
    panic!("the failpoint {point} was not reached");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kill_during_page_apply_leaves_no_partial_page() {
    if std::env::var_os(ENV_POINT).is_some() {
        return;
    }
    let (base, fake) = fake_server::start().await;
    for i in 1..=5 {
        fake.server_create(U1, &format!("n{i}"), "t", "b");
    }
    let dir = tempfile::tempdir().expect("tmp");
    crash_at("pull.mid_page", dir.path(), &base).await;

    let engine = Setup::new(&base, U1).open(dir.path()).await;
    assert!(engine.db().integrity_check().await.expect("check"));
    assert_eq!(cursor(&engine).await, "0", "the cursor never moved past unapplied rows");
    assert!(rows(&engine).await.is_empty(), "the half-applied page was rolled back");
    engine.sync_once().await.expect("normal run");
    assert_eq!(rows(&engine).await.len(), 5);
    assert_eq!(cursor(&engine).await, "5");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kill_after_the_server_applied_a_write_runs_it_exactly_once() {
    if std::env::var_os(ENV_POINT).is_some() {
        return;
    }
    let (base, fake) = fake_server::start().await;
    let dir = tempfile::tempdir().expect("tmp");
    // Queue a create, then close (a first, normal process).
    {
        let engine = Setup::new(&base, U1).open(dir.path()).await;
        engine.set_forced_offline(true);
        let mut tx = engine.begin_local().await.expect("tx");
        tx.create("note", "c", fields(json!({"title": "C"}))).await.expect("create");
        tx.commit().await.expect("commit");
        engine.db().close().await;
    }
    // The child sends it; the server commits; the child dies before recording the answer.
    crash_at("push.after_response", dir.path(), &base).await;
    assert_eq!(fake.count(U1), 1, "the server applied it");

    let engine = Setup::new(&base, U1).open(dir.path()).await;
    assert!(engine.db().integrity_check().await.expect("check"));
    let ops = engine.outbox().await.expect("ops");
    assert_eq!(ops.len(), 1, "the op survived the crash");
    assert_eq!(ops[0].state, kubuno_desktop_sync_engine::OpState::Pending, "inflight is recovered as pending");
    let key = ops[0].idem_key.clone();
    engine.sync_once().await.expect("replay");
    assert_eq!(fake.executions_of(&key), 1, "replayed by the server, not run twice");
    assert_eq!(fake.count(U1), 1);
    assert!(engine.outbox().await.expect("ops").is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kill_before_the_local_commit_of_an_answer() {
    if std::env::var_os(ENV_POINT).is_some() {
        return;
    }
    let (base, fake) = fake_server::start().await;
    fake.server_create(U1, "a", "A", "x");
    let dir = tempfile::tempdir().expect("tmp");
    {
        let engine = Setup::new(&base, U1).open(dir.path()).await;
        engine.sync_once().await.expect("initial");
        let mut tx = engine.begin_local().await.expect("tx");
        tx.patch("note", "a", fields(json!({"title": "B"}))).await.expect("patch");
        tx.commit().await.expect("commit");
        engine.set_forced_offline(true);
        engine.db().close().await;
    }
    crash_at("push.before_commit", dir.path(), &base).await;
    let engine = Setup::new(&base, U1).open(dir.path()).await;
    assert!(engine.db().integrity_check().await.expect("check"));
    assert_eq!(engine.outbox().await.expect("ops").len(), 1);
    engine.sync_once().await.expect("finish");
    assert_eq!(fake.get(U1, "a").map(|n| n.title).as_deref(), Some("B"));
    assert_eq!(fake.max_executions(), 1);
    assert_eq!(support::row(&engine, "a").await.map(|r| r.3), Some(0));
    let _ = FEED;
}
