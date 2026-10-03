//! When the engine syncs (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §8). The OS hooks (network listeners, wake from
//! sleep, the websocket client, the settings page) call [`Scheduler::notify`]; the scheduler debounces and
//! coalesces: requests arriving during a run are merged into the next one, so nothing is lost and nothing runs twice
//! at the same time.
//!
//! | Trigger | Work | Delay |
//! |---|---|---|
//! | `Startup`, `Manual` | full cycle (push, pull every feed; `Manual` ignores the backoff) | none |
//! | `LocalWrite` | push | `local_write_debounce` (1.5 s), restarted by each new write |
//! | `Interval` (internal timer) | full cycle | `interval` (`shell.json` `sync_interval_min`) |
//! | `PushHint { feed }` (targeted websocket `<module>.changed`) | pull that feed (all feeds if `None`) | `hint_debounce` (1 s) |
//! | `NetworkChanged { online }` | `online`: full cycle; offline: network work suspended | none |
//! | `Resume` (wake from sleep) | full cycle | `resume_delay` (5 s) |
//!
//! Retries of pending ops are scheduled at their `next_attempt_at`.

use std::collections::BTreeSet;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::engine::SyncEngine;
use crate::error::SyncError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    Startup,
    LocalWrite,
    Interval,
    PushHint { feed: Option<String> },
    NetworkChanged { online: bool },
    Resume,
    Manual,
}

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub interval: Duration,
    pub local_write_debounce: Duration,
    pub hint_debounce: Duration,
    pub resume_delay: Duration,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(5 * 60),
            local_write_debounce: Duration::from_millis(1500),
            hint_debounce: Duration::from_secs(1),
            resume_delay: Duration::from_secs(5),
        }
    }
}

enum Cmd {
    Trigger(Trigger),
    SetInterval(Duration),
    Shutdown,
}

#[derive(Debug, Default, Clone)]
struct Work {
    full: bool,
    manual: bool,
    push: bool,
    pulls: BTreeSet<String>,
}

impl Work {
    fn is_empty(&self) -> bool {
        !self.full && !self.push && self.pulls.is_empty()
    }

    fn merge(&mut self, other: Work) {
        self.full |= other.full;
        self.manual |= other.manual;
        self.push |= other.push;
        self.pulls.extend(other.pulls);
    }
}

/// Handle of a running scheduler.
#[derive(Debug, Clone)]
pub struct Scheduler {
    tx: mpsc::UnboundedSender<Cmd>,
}

impl Scheduler {
    /// Starts the scheduler task of `engine` (on the current Tokio runtime). It also receives the engine's own
    /// requests (local writes, resolved conflicts, restored session).
    pub fn spawn(engine: SyncEngine, cfg: SchedulerConfig) -> (Scheduler, JoinHandle<()>) {
        let (tx, rx) = mpsc::unbounded_channel::<Cmd>();
        let (ktx, krx) = mpsc::unbounded_channel::<Trigger>();
        engine.attach_scheduler(ktx);
        let handle = tokio::spawn(run(engine, cfg, rx, krx));
        (Scheduler { tx }, handle)
    }

    pub fn notify(&self, t: Trigger) {
        let _ = self.tx.send(Cmd::Trigger(t));
    }

    /// The interval setting changed (`sync_interval_min`); also 1 min while an app is in the foreground.
    pub fn set_interval(&self, d: Duration) {
        let _ = self.tx.send(Cmd::SetInterval(d));
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(Cmd::Shutdown);
    }
}

async fn run(engine: SyncEngine, mut cfg: SchedulerConfig, mut rx: mpsc::UnboundedReceiver<Cmd>, mut krx: mpsc::UnboundedReceiver<Trigger>) {
    let far = || Instant::now() + Duration::from_secs(365 * 24 * 3600);
    let mut next_interval = Instant::now() + cfg.interval;
    let mut due_at = far();
    let mut work = Work::default();
    let mut retry_at = far();
    loop {
        let wake = due_at.min(next_interval).min(retry_at);
        let trigger = tokio::select! {
            cmd = rx.recv() => match cmd {
                None | Some(Cmd::Shutdown) => return,
                Some(Cmd::SetInterval(d)) => {
                    cfg.interval = d.max(Duration::from_secs(10));
                    next_interval = Instant::now() + cfg.interval;
                    continue;
                }
                Some(Cmd::Trigger(t)) => Some(t),
            },
            t = krx.recv() => t,
            _ = tokio::time::sleep_until(wake) => None,
        };
        let now = Instant::now();
        if let Some(t) = trigger {
            let (w, delay) = match t {
                Trigger::Startup => (Work { full: true, ..Work::default() }, Duration::ZERO),
                Trigger::Manual => (Work { full: true, manual: true, ..Work::default() }, Duration::ZERO),
                Trigger::Interval => (Work { full: true, ..Work::default() }, Duration::ZERO),
                Trigger::LocalWrite => (Work { push: true, ..Work::default() }, cfg.local_write_debounce),
                Trigger::PushHint { feed: Some(f) } => (Work { pulls: BTreeSet::from([f]), ..Work::default() }, cfg.hint_debounce),
                Trigger::PushHint { feed: None } => (Work { full: true, ..Work::default() }, cfg.hint_debounce),
                Trigger::NetworkChanged { online } => {
                    engine.set_network_available(online);
                    let _ = engine.refresh_status().await;
                    if !online {
                        continue;
                    }
                    (Work { full: true, ..Work::default() }, Duration::ZERO)
                }
                Trigger::Resume => (Work { full: true, ..Work::default() }, cfg.resume_delay),
            };
            work.merge(w);
            // Debounce: the latest request decides, except that an immediate request is never postponed.
            due_at = if delay.is_zero() { now } else { now + delay };
            continue;
        }
        if now >= next_interval {
            work.full = true;
            next_interval = now + cfg.interval;
            due_at = due_at.min(now);
        }
        if now >= retry_at {
            work.push = true;
            retry_at = far();
            due_at = due_at.min(now);
        }
        if now < due_at || work.is_empty() {
            if work.is_empty() {
                due_at = far();
            }
            continue;
        }
        let todo = std::mem::take(&mut work);
        due_at = far();
        let result = execute(&engine, &todo).await;
        match result {
            Ok(()) | Err(SyncError::Offline) | Err(SyncError::SessionExpired) | Err(SyncError::ReadOnly(_)) => {}
            Err(e) => tracing::warn!(error = %e, "sync run failed"),
        }
        // Next retry of an op in backoff (local clock, relative delay only).
        if let Ok(Some(at)) = engine.next_retry_at().await {
            let delta = (at - engine.now_ms()).max(0) as u64;
            retry_at = Instant::now() + Duration::from_millis(delta);
        }
    }
}

async fn execute(engine: &SyncEngine, w: &Work) -> Result<(), SyncError> {
    if w.full {
        if w.manual {
            engine.sync_now().await?;
        } else {
            engine.sync_once().await?;
        }
        return Ok(());
    }
    if w.push {
        engine.push().await?;
    }
    for f in &w.pulls {
        engine.pull(f).await?;
    }
    Ok(())
}
