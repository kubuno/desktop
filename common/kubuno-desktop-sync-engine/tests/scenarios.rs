//! Scenarios of vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §16.2 against the fake KDP server.

mod fake_server;
mod support;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use kubuno_desktop_account::AccountKey;
use kubuno_desktop_sync_engine::{
    AppSchema, ConflictPolicy, EnqueueOutcome, LocalDb, OpenOptions, Resolution, SkewedClock, SyncError, SyncEvent, SyncState,
};
use serde_json::json;
use support::{cursor, fields, row, rows, Setup, SwitchableToken, FEED};

const U1: &str = "user-1";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pull_full_then_incremental_with_tombstones() {
    let (base, fake) = fake_server::start().await;
    for i in 1..=5 {
        fake.server_create(U1, &format!("n{i}"), &format!("t{i}"), "b");
    }
    let dir = tempfile::tempdir().expect("tmp");
    let engine = Setup::new(&base, U1).open(dir.path()).await;
    let r = engine.pull(FEED).await.expect("pull");
    assert_eq!(r.applied, 5);
    assert_eq!(r.pages, 3, "page limit 2 -> 3 pages");
    assert_eq!(rows(&engine).await.len(), 5);
    assert_eq!(cursor(&engine).await, "5");

    fake.server_patch(U1, "n2", Some("t2b"), None);
    fake.server_delete(U1, "n3");
    let r = engine.pull(FEED).await.expect("pull");
    assert_eq!((r.applied, r.deleted), (1, 1));
    assert_eq!(row(&engine, "n2").await.and_then(|r| r.1).as_deref(), Some("t2b"));
    assert!(row(&engine, "n3").await.is_none());
    // Nothing new: one empty page.
    let r = engine.pull(FEED).await.expect("pull");
    assert_eq!((r.pages, r.applied), (1, 0));
    assert!(engine.db().integrity_check().await.expect("check"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_edits_are_coalesced_and_sent_once() {
    let (base, fake) = fake_server::start().await;
    fake.server_create(U1, "a", "A", "x");
    let dir = tempfile::tempdir().expect("tmp");
    let engine = Setup::new(&base, U1).open(dir.path()).await;
    engine.sync_once().await.expect("initial");

    engine.set_forced_offline(true);
    let mut tx = engine.begin_local().await.expect("tx");
    assert!(matches!(tx.create("note", "c", fields(json!({"title": "C", "body": "1"}))).await.expect("create"), EnqueueOutcome::Queued { .. }));
    assert!(matches!(tx.patch("note", "c", fields(json!({"body": "2"}))).await.expect("patch"), EnqueueOutcome::Coalesced { .. }));
    tx.patch("note", "a", fields(json!({"title": "A2"}))).await.expect("patch a");
    tx.commit().await.expect("commit");
    // Create then delete before anything was sent: nothing at all goes out.
    let mut tx = engine.begin_local().await.expect("tx");
    tx.create("note", "tmp", fields(json!({"title": "T"}))).await.expect("create tmp");
    assert_eq!(tx.delete("note", "tmp").await.expect("delete tmp"), EnqueueOutcome::Cancelled);
    tx.commit().await.expect("commit");

    assert!(matches!(engine.sync_once().await, Err(SyncError::Offline)));
    assert_eq!(fake.requests.load(Ordering::SeqCst), 1, "only the initial pull reached the server");
    let st = engine.refresh_status().await.expect("status");
    assert_eq!(st.state, SyncState::Offline);
    assert_eq!(st.pending_count, 2);
    assert_eq!(row(&engine, "c").await.map(|r| r.3), Some(1), "_pending badge");

    engine.set_forced_offline(false);
    engine.sync_once().await.expect("online");
    assert_eq!(fake.get(U1, "c").map(|n| (n.title, n.body)), Some(("C".into(), "2".into())));
    assert_eq!(fake.get(U1, "a").map(|n| n.title).as_deref(), Some("A2"));
    assert!(fake.get(U1, "tmp").is_none());
    assert_eq!(fake.max_executions(), 1, "each intent executed once");
    let st = engine.refresh_status().await.expect("status");
    assert_eq!((st.state.clone(), st.pending_count), (SyncState::Synced, 0));
    assert_eq!(row(&engine, "c").await.map(|r| r.3), Some(0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn different_fields_of_one_record_merge_without_conflict() {
    let (base, fake) = fake_server::start().await;
    fake.server_create(U1, "a", "A", "x");
    let dir = tempfile::tempdir().expect("tmp");
    let engine = Setup::new(&base, U1).open(dir.path()).await;
    engine.sync_once().await.expect("initial");
    fake.server_patch(U1, "a", None, Some("server body"));
    let mut tx = engine.begin_local().await.expect("tx");
    tx.patch("note", "a", fields(json!({"title": "local title"}))).await.expect("patch");
    tx.commit().await.expect("commit");
    let report = engine.sync_once().await.expect("sync");
    assert_eq!(report.push.conflicts, 1, "412 then merged");
    let n = fake.get(U1, "a").expect("note");
    assert_eq!((n.title.as_str(), n.body.as_str()), ("local title", "server body"));
    assert!(engine.conflicts().await.expect("conflicts").is_empty());
    assert_eq!(row(&engine, "a").await.map(|r| (r.1, r.2)), Some((Some("local title".into()), Some("server body".into()))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_field_conflict_is_recorded_then_resolved_both_ways() {
    let (base, fake) = fake_server::start().await;
    fake.server_create(U1, "a", "A", "x");
    fake.server_create(U1, "b", "B", "x");
    let dir = tempfile::tempdir().expect("tmp");
    let engine = Setup::new(&base, U1).open(dir.path()).await;
    let mut events = engine.subscribe();
    engine.sync_once().await.expect("initial");
    fake.server_patch(U1, "a", Some("web A"), None);
    fake.server_patch(U1, "b", Some("web B"), None);
    let mut tx = engine.begin_local().await.expect("tx");
    tx.patch("note", "a", fields(json!({"title": "desk A"}))).await.expect("patch");
    tx.patch("note", "b", fields(json!({"title": "desk B"}))).await.expect("patch");
    tx.commit().await.expect("commit");
    engine.sync_once().await.expect("sync");
    let conflicts = engine.conflicts().await.expect("conflicts");
    assert_eq!(conflicts.len(), 2);
    assert!(conflicts.iter().all(|c| c.kind == "field" && c.fields == vec!["title".to_string()]));
    // The server value is shown meanwhile.
    assert_eq!(row(&engine, "a").await.and_then(|r| r.1).as_deref(), Some("web A"));
    assert_eq!(engine.refresh_status().await.expect("st").conflict_count, 2);
    let mut saw = false;
    while let Ok(e) = events.try_recv() {
        saw |= matches!(e, SyncEvent::ConflictDetected { ref kind, .. } if kind == "field");
    }
    assert!(saw, "ConflictDetected published");

    let ca = conflicts.iter().find(|c| c.entity_id == "a").expect("a");
    let cb = conflicts.iter().find(|c| c.entity_id == "b").expect("b");
    engine.resolve_conflict(ca.id, Resolution::KeepMine).await.expect("keep mine");
    engine.resolve_conflict(cb.id, Resolution::KeepServer).await.expect("keep server");
    engine.sync_once().await.expect("sync");
    assert_eq!(fake.get(U1, "a").map(|n| n.title).as_deref(), Some("desk A"));
    assert_eq!(fake.get(U1, "b").map(|n| n.title).as_deref(), Some("web B"));
    assert_eq!(row(&engine, "a").await.and_then(|r| r.1).as_deref(), Some("desk A"));
    assert_eq!(row(&engine, "b").await.and_then(|r| r.1).as_deref(), Some("web B"));
    assert!(engine.conflicts().await.expect("conflicts").is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleted_on_web_edited_on_desktop_then_restored() {
    let (base, fake) = fake_server::start().await;
    fake.server_create(U1, "a", "A", "x");
    let dir = tempfile::tempdir().expect("tmp");
    let engine = Setup::new(&base, U1).open(dir.path()).await;
    engine.sync_once().await.expect("initial");
    engine.set_forced_offline(true);
    let mut tx = engine.begin_local().await.expect("tx");
    tx.patch("note", "a", fields(json!({"body": "edited offline"}))).await.expect("patch");
    tx.commit().await.expect("commit");
    fake.server_delete(U1, "a");
    engine.set_forced_offline(false);
    // Push: 404 on the patch -> deleted_remotely; the tombstone in the pull agrees.
    engine.sync_once().await.expect("sync");
    let c = engine.conflicts().await.expect("conflicts");
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].kind, "deleted_remotely");
    assert_eq!(row(&engine, "a").await.and_then(|r| r.2).as_deref(), Some("edited offline"), "kept visible");
    engine.resolve_conflict(c[0].id, Resolution::KeepMine).await.expect("restore");
    engine.sync_once().await.expect("sync");
    let n = fake.get(U1, "a").expect("re-created with the same id");
    assert_eq!(n.body, "edited offline");
    assert!(engine.conflicts().await.expect("c").is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn definitive_rejection_rolls_back_visibly() {
    let (base, fake) = fake_server::start().await;
    fake.server_create(U1, "a", "A", "x");
    let dir = tempfile::tempdir().expect("tmp");
    let engine = Setup::new(&base, U1).open(dir.path()).await;
    let mut events = engine.subscribe();
    engine.sync_once().await.expect("initial");
    let mut tx = engine.begin_local().await.expect("tx");
    tx.patch("note", "a", fields(json!({"title": "INVALID"}))).await.expect("patch");
    tx.create("note", "z", fields(json!({"title": "INVALID", "body": "zombie?"}))).await.expect("create");
    tx.commit().await.expect("commit");
    assert_eq!(row(&engine, "a").await.and_then(|r| r.1).as_deref(), Some("INVALID"));
    let report = engine.sync_once().await.expect("sync");
    assert_eq!(report.push.rejected, 2);
    assert_eq!(row(&engine, "a").await.and_then(|r| r.1).as_deref(), Some("A"), "rolled back to the server value");
    assert!(row(&engine, "z").await.is_none(), "no zombie row");
    let c = engine.conflicts().await.expect("conflicts");
    assert_eq!(c.iter().filter(|c| c.kind == "rejected").count(), 2);
    assert!(c[0].message.as_deref().is_some_and(|m| m.contains("422")));
    let mut rejected = 0;
    while let Ok(e) = events.try_recv() {
        if matches!(e, SyncEvent::Rejected { .. }) {
            rejected += 1;
        }
    }
    assert_eq!(rejected, 2);
    assert_eq!(engine.unsent_count().await.expect("unsent"), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lost_response_is_retried_with_the_same_key_without_duplicate() {
    let (base, fake) = fake_server::start().await;
    let dir = tempfile::tempdir().expect("tmp");
    let engine = Setup::new(&base, U1).open(dir.path()).await;
    let mut tx = engine.begin_local().await.expect("tx");
    tx.create("note", "c", fields(json!({"title": "C"}))).await.expect("create");
    tx.patch("note", "c", fields(json!({"body": "1"}))).await.expect("coalesced");
    tx.commit().await.expect("commit");
    let key = engine.outbox().await.expect("ops")[0].idem_key.clone();
    fake.drop_after_commit.store(1, Ordering::SeqCst);
    let r = engine.sync_once().await.expect("sync");
    assert_eq!(r.push.retry_later, 1);
    let ops = engine.outbox().await.expect("ops");
    assert_eq!((ops.len(), ops[0].attempts, ops[0].idem_key.as_str()), (1, 1, key.as_str()));
    // The same key is presented again: the server replays, nothing runs twice.
    engine.sync_now().await.expect("retry");
    assert_eq!(fake.executions_of(&key), 1);
    assert_eq!(fake.count(U1), 1);
    assert!(engine.outbox().await.expect("ops").is_empty());

    // A transient 503 keeps the op forever with a backoff (never given up).
    let mut tx = engine.begin_local().await.expect("tx");
    tx.patch("note", "c", fields(json!({"body": "2"}))).await.expect("patch");
    tx.commit().await.expect("commit");
    fake.fail_next.store(3, Ordering::SeqCst);
    for _ in 0..3 {
        engine.sync_now().await.expect("sync");
    }
    let st = engine.refresh_status().await.expect("st");
    assert_eq!((st.pending_count, st.stuck_count), (1, 1), "N modifications en attente after 3 attempts");
    assert!(engine.next_retry_at().await.expect("next").is_some());
    engine.sync_now().await.expect("sync");
    assert_eq!(fake.get(U1, "c").map(|n| n.body).as_deref(), Some("2"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_changes_are_rebased_over_pulled_rows() {
    let (base, fake) = fake_server::start().await;
    fake.server_create(U1, "a", "A", "x");
    let dir = tempfile::tempdir().expect("tmp");
    let engine = Setup::new(&base, U1).open(dir.path()).await;
    engine.sync_once().await.expect("initial");
    let mut tx = engine.begin_local().await.expect("tx");
    tx.patch("note", "a", fields(json!({"title": "mine"}))).await.expect("patch");
    tx.commit().await.expect("commit");
    fake.server_patch(U1, "a", None, Some("theirs"));
    // Pull only (the push has not happened): the server row goes to the shadow, the pending title stays on top.
    let r = engine.pull(FEED).await.expect("pull");
    assert_eq!(r.shadowed, 1);
    assert_eq!(row(&engine, "a").await, Some(("a".into(), Some("mine".into()), Some("theirs".into()), 1)));
    engine.sync_once().await.expect("sync");
    let n = fake.get(U1, "a").expect("note");
    assert_eq!((n.title.as_str(), n.body.as_str()), ("mine", "theirs"));
    assert_eq!(row(&engine, "a").await.map(|r| r.3), Some(0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_delete_against_a_newer_server_version_is_a_conflict() {
    let (base, fake) = fake_server::start().await;
    fake.server_create(U1, "a", "A", "x");
    let dir = tempfile::tempdir().expect("tmp");
    let engine = Setup::new(&base, U1).open(dir.path()).await;
    engine.sync_once().await.expect("initial");
    fake.server_patch(U1, "a", Some("newer"), None);
    let mut tx = engine.begin_local().await.expect("tx");
    tx.delete("note", "a").await.expect("delete");
    tx.commit().await.expect("commit");
    engine.sync_once().await.expect("sync");
    assert!(fake.get(U1, "a").is_some(), "never deleted silently");
    let c = engine.conflicts().await.expect("conflicts");
    assert_eq!(c[0].kind, "delete_vs_edit");
    assert_eq!(row(&engine, "a").await.and_then(|r| r.1).as_deref(), Some("newer"), "server version shown");
    engine.resolve_conflict(c[0].id, Resolution::KeepMine).await.expect("delete anyway");
    engine.sync_once().await.expect("sync");
    assert!(fake.get(U1, "a").is_none());
    assert!(row(&engine, "a").await.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keep_both_policy_creates_a_copy() {
    let (base, fake) = fake_server::start().await;
    fake.server_create(U1, "a", "Rapport", "v1");
    let dir = tempfile::tempdir().expect("tmp");
    let mut s = Setup::new(&base, U1);
    s.policy = ConflictPolicy::KeepBoth;
    let engine = s.open(dir.path()).await;
    engine.sync_once().await.expect("initial");
    fake.server_patch(U1, "a", None, Some("web body"));
    let mut tx = engine.begin_local().await.expect("tx");
    tx.patch("note", "a", fields(json!({"body": "desktop body"}))).await.expect("patch");
    tx.commit().await.expect("commit");
    engine.sync_once().await.expect("sync");
    assert_eq!(fake.get(U1, "a").map(|n| n.body).as_deref(), Some("web body"));
    assert_eq!(fake.count(U1), 2, "the local version became a copy");
    let local = rows(&engine).await;
    let copy = local.iter().find(|r| r.0 != "a").expect("copy");
    assert!(copy.1.as_deref().is_some_and(|t| t.starts_with("Rapport (conflit TESTPC ")), "{copy:?}");
    assert_eq!(copy.2.as_deref(), Some("desktop body"));
    assert!(engine.conflicts().await.expect("open").is_empty(), "keep-both is resolved automatically");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expired_cursor_triggers_a_full_resync() {
    let (base, fake) = fake_server::start().await;
    for i in 1..=3 {
        fake.server_create(U1, &format!("n{i}"), "t", "b");
    }
    let dir = tempfile::tempdir().expect("tmp");
    let engine = Setup::new(&base, U1).open(dir.path()).await;
    engine.sync_once().await.expect("initial");
    // A deletion whose tombstone is gone (retention) and a new row; the old cursor is refused.
    fake.server_purge(U1, "n2");
    fake.server_create(U1, "n4", "t", "b");
    fake.expired_below.store(100, Ordering::SeqCst);
    // A local pending change on a row that is about to disappear is kept.
    let mut tx = engine.begin_local().await.expect("tx");
    tx.create("note", "local-only", fields(json!({"title": "L"}))).await.expect("create");
    tx.commit().await.expect("commit");
    engine.set_forced_offline(false);
    let r = engine.pull(FEED).await.expect("pull");
    assert!(r.deleted >= 1);
    let ids: Vec<String> = rows(&engine).await.into_iter().map(|r| r.0).collect();
    assert_eq!(ids, vec!["local-only", "n1", "n3", "n4"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_skewed_client_clock_changes_nothing() {
    for offset_h in [-2i64, 2] {
        let (base, fake) = fake_server::start().await;
        fake.server_create(U1, "a", "A", "x");
        let dir = tempfile::tempdir().expect("tmp");
        let mut s = Setup::new(&base, U1);
        s.clock = Arc::new(SkewedClock::new(offset_h * 3600 * 1000));
        let engine = s.open(dir.path()).await;
        engine.sync_once().await.expect("initial");
        // Interleaved edits: order is decided by the server only.
        fake.server_patch(U1, "a", None, Some("server 1"));
        let mut tx = engine.begin_local().await.expect("tx");
        tx.patch("note", "a", fields(json!({"title": "desk"}))).await.expect("patch");
        tx.commit().await.expect("commit");
        fake.server_patch(U1, "a", None, Some("server 2"));
        engine.sync_once().await.expect("sync");
        let n = fake.get(U1, "a").expect("note");
        assert_eq!((n.title.as_str(), n.body.as_str()), ("desk", "server 2"), "offset {offset_h} h");
        assert_eq!(row(&engine, "a").await.map(|r| (r.1, r.2)), Some((Some("desk".into()), Some("server 2".into()))));
        // A replayed (older) change never overwrites a newer row.
        sqlx::query("UPDATE _sync_feeds SET cursor = '0'").execute(engine.db().pool()).await.expect("reset cursor");
        let r = engine.pull(FEED).await.expect("pull");
        assert_eq!(r.applied, 0);
        assert_eq!(r.stale, 1);
        // Transient retry scheduling works whatever the offset (relative delays).
        let mut tx = engine.begin_local().await.expect("tx");
        tx.patch("note", "a", fields(json!({"body": "late"}))).await.expect("patch");
        tx.commit().await.expect("commit");
        fake.fail_next.store(1, Ordering::SeqCst);
        engine.sync_once().await.expect("sync");
        let next = engine.next_retry_at().await.expect("next").expect("some");
        let delay = next - engine.now_ms();
        assert!((0..=3000).contains(&delay), "first backoff ~2 s, got {delay} ms");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accounts_are_isolated() {
    let (base, fake) = fake_server::start().await;
    let dir = tempfile::tempdir().expect("tmp");
    let e1 = Setup::new(&base, "user-1").open(dir.path()).await;
    let e2 = Setup::new(&base, "user-2").open(dir.path()).await;
    for (e, id) in [(&e1, "one"), (&e2, "two")] {
        let mut tx = e.begin_local().await.expect("tx");
        tx.create("note", id, fields(json!({"title": id}))).await.expect("create");
        tx.commit().await.expect("commit");
    }
    e1.sync_once().await.expect("sync 1");
    e2.sync_once().await.expect("sync 2");
    let writes = fake.writes.lock().expect("lock").clone();
    assert!(writes.contains(&("user-1".into(), "POST".into(), "one".into())));
    assert!(writes.contains(&("user-2".into(), "POST".into(), "two".into())));
    assert_eq!(writes.len(), 2);
    assert_eq!(rows(&e1).await.len(), 1);
    assert_eq!(rows(&e2).await.len(), 1);
    // A database never opens for another account.
    let path = e1.db().path().to_path_buf();
    e1.db().close().await;
    let other = AccountKey::new(&base, "user-2").expect("key");
    let err = LocalDb::open(OpenOptions::new(path, other).schema(AppSchema { migrator: Some(&support::MIGRATOR), resync_on: vec![] })).await.expect_err("mismatch");
    assert!(matches!(err, SyncError::AccountMismatch), "{err:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expired_session_pauses_and_keeps_the_outbox() {
    let (base, fake) = fake_server::start().await;
    let dir = tempfile::tempdir().expect("tmp");
    let tokens = SwitchableToken::new("user-1");
    let mut s = Setup::new(&base, U1);
    s.tokens = Some(tokens.clone());
    let engine = s.open(dir.path()).await;
    let mut tx = engine.begin_local().await.expect("tx");
    tx.create("note", "c", fields(json!({"title": "C"}))).await.expect("create");
    tx.commit().await.expect("commit");
    *tokens.token.lock().expect("lock") = "revoked".into();
    *tokens.revoked.lock().expect("lock") = true;
    assert!(matches!(engine.sync_once().await, Err(SyncError::SessionExpired)));
    assert_eq!(engine.refresh_status().await.expect("st").state, SyncState::SessionExpired);
    assert_eq!(engine.unsent_count().await.expect("unsent"), 1, "outbox kept");
    assert!(matches!(engine.sync_once().await, Err(SyncError::SessionExpired)), "paused");
    assert!(row(&engine, "c").await.is_some(), "local data readable");
    // Signed in again with the same account.
    *tokens.token.lock().expect("lock") = "user-1".into();
    *tokens.revoked.lock().expect("lock") = false;
    engine.session_restored();
    engine.sync_once().await.expect("resumed");
    assert!(fake.get(U1, "c").is_some());
    assert_eq!(engine.unsent_count().await.expect("unsent"), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn migrations_reset_feeds_and_a_failed_one_opens_read_only() {
    let (base, fake) = fake_server::start().await;
    fake.server_create(U1, "a", "A", "x");
    let dir = tempfile::tempdir().expect("tmp");
    let engine = Setup::new(&base, U1).open(dir.path()).await;
    engine.sync_once().await.expect("initial");
    engine.set_forced_offline(true);
    let mut tx = engine.begin_local().await.expect("tx");
    tx.patch("note", "a", fields(json!({"title": "pending"}))).await.expect("patch");
    tx.commit().await.expect("commit");
    engine.db().close().await;
    drop(engine);

    // v2 adds a column to the synced entity: the feed is re-pulled, the outbox survives.
    let mut s = Setup::new(&base, U1);
    s.schema = AppSchema { migrator: Some(&support::MIGRATOR_V2), resync_on: vec![(2, vec![FEED.to_string()])] };
    let engine = s.open(dir.path()).await;
    let (c, full): (String, i64) = sqlx::query_as("SELECT cursor, needs_full FROM _sync_feeds").fetch_one(engine.db().pool()).await.expect("feed");
    assert_eq!((c.as_str(), full), ("0", 1));
    assert_eq!(engine.unsent_count().await.expect("unsent"), 1);
    engine.db().close().await;
    drop(engine);

    // A broken migration: read-only, error kept, nothing lost.
    let path = dir.path().join("user-1-notes.db");
    let db = LocalDb::open(
        OpenOptions::new(&path, AccountKey::new(&base, U1).expect("key")).schema(AppSchema { migrator: Some(&support::MIGRATOR_BAD), resync_on: vec![] }),
    )
    .await
    .expect("opens anyway");
    assert!(db.degraded().is_some());
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM _sync_outbox").fetch_one(db.pool()).await.expect("count");
    assert_eq!(n, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_writes_notify_views_and_status() {
    let (base, _fake) = fake_server::start().await;
    let dir = tempfile::tempdir().expect("tmp");
    let engine = Setup::new(&base, U1).open(dir.path()).await;
    let mut events = engine.subscribe();
    let mut status = engine.status();
    let mut tx = engine.begin_local().await.expect("tx");
    tx.create("note", "c", fields(json!({"title": "C"}))).await.expect("create");
    tx.commit().await.expect("commit");
    let e = tokio::time::timeout(Duration::from_secs(2), events.recv()).await.expect("event").expect("recv");
    assert_eq!(e, SyncEvent::TablesChanged(vec!["notes".into()]));
    status.mark_changed();
    assert_eq!(status.borrow_and_update().pending_count, 1);
    // Dropped without commit: rolled back, nothing queued.
    let mut tx = engine.begin_local().await.expect("tx");
    tx.create("note", "x", fields(json!({"title": "X"}))).await.expect("create");
    drop(tx);
    assert!(row(&engine, "x").await.is_none());
    assert_eq!(engine.unsent_count().await.expect("unsent"), 1);
}
