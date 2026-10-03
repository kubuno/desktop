//! The scheduler: debounce, coalescing, hints, network changes, interval.

mod fake_server;
mod support;

use std::time::Duration;

use kubuno_desktop_sync_engine::{Scheduler, SchedulerConfig, Trigger};
use serde_json::json;
use support::{fields, Setup, FEED};

const U1: &str = "user-1";

fn cfg() -> SchedulerConfig {
    SchedulerConfig {
        interval: Duration::from_secs(3600),
        local_write_debounce: Duration::from_millis(150),
        hint_debounce: Duration::from_millis(50),
        resume_delay: Duration::from_millis(50),
    }
}

async fn wait_until(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..100 {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_writes_are_debounced_into_one_push() {
    let (base, fake) = fake_server::start().await;
    let dir = tempfile::tempdir().expect("tmp");
    let engine = Setup::new(&base, U1).open(dir.path()).await;
    let (sched, handle) = Scheduler::spawn(engine.clone(), cfg());
    for i in 0..10 {
        let mut tx = engine.begin_local().await.expect("tx");
        tx.create("note", &format!("n{i}"), fields(json!({"title": i.to_string()}))).await.expect("create");
        tx.commit().await.expect("commit");
    }
    assert!(wait_until(|| fake.count(U1) == 10).await, "all sent");
    let (pushes, pulls) = engine.run_counts();
    assert!(pushes <= 2, "coalesced: {pushes} push runs for 10 writes");
    assert_eq!(pulls, 0, "a local write pushes only");
    sched.shutdown();
    let _ = handle.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hints_network_and_startup() {
    let (base, fake) = fake_server::start().await;
    let dir = tempfile::tempdir().expect("tmp");
    let engine = Setup::new(&base, U1).open(dir.path()).await;
    let (sched, handle) = Scheduler::spawn(engine.clone(), cfg());
    sched.notify(Trigger::Startup);
    assert!(wait_until(|| engine.run_counts().1 >= 1).await, "startup = full cycle");

    // A websocket hint pulls that feed.
    fake.server_create(U1, "w", "from web", "");
    sched.notify(Trigger::PushHint { feed: Some(FEED.into()) });
    let e = engine.clone();
    assert!(
        wait_until(move || {
            let pool = e.db().pool().clone();
            futures::executor::block_on(async move {
                sqlx::query_scalar::<_, i64>("SELECT count(*) FROM notes").fetch_one(&pool).await.unwrap_or(0) == 1
            })
        })
        .await
    );

    // Network down: no request at all, local writes still work.
    sched.notify(Trigger::NetworkChanged { online: false });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let before = fake.requests.load(std::sync::atomic::Ordering::SeqCst);
    let mut tx = engine.begin_local().await.expect("tx");
    tx.create("note", "offline", fields(json!({"title": "o"}))).await.expect("create");
    tx.commit().await.expect("commit");
    sched.notify(Trigger::Manual);
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(fake.requests.load(std::sync::atomic::Ordering::SeqCst), before);
    assert!(engine.is_offline());
    // Back online: a full cycle sends it.
    sched.notify(Trigger::NetworkChanged { online: true });
    assert!(wait_until(|| fake.get(U1, "offline").is_some()).await);

    // The interval: shortened at run time.
    let pulls = engine.run_counts().1;
    sched.set_interval(Duration::from_secs(10));
    sched.notify(Trigger::Resume);
    assert!(wait_until(|| engine.run_counts().1 > pulls).await, "resume = full cycle");
    sched.shutdown();
    let _ = handle.await;
}
