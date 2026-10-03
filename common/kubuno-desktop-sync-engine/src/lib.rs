//! Offline-first data sync of the Kubuno desktop (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md`, lots SE-0..SE-3).
//!
//! - [`paths`]: per-user locations (Windows known folders, macOS `~/Library`, XDG), aligned with `kubuno-paths`;
//! - [`db`]: the per-account, per-app SQLite database (SQLCipher when keyed), engine schema, app migrations,
//!   account check, single-writer lock;
//! - [`outbox`]: intents with idempotency keys, coalescing, shadow rows, rebase, conflict records, activity log;
//! - [`engine`]: local writes ([`engine::LocalTx`]), push with the classification table of §7.3 and explicit
//!   rollback, pull of KDP feeds (page + cursor in one transaction, ordering by `change_seq`, tombstones, full
//!   resync on `410 CURSOR_EXPIRED`), conflicts and their resolution, status;
//! - [`conflict`]: policies (field merge, keep both, server wins, last writer wins, custom);
//! - [`adapter`]: how an entity table is read/written and how intents become requests;
//! - [`scheduler`]: triggers (startup, local write, interval, websocket hint, network change, resume, manual);
//! - [`status`]: what the UI binds to.
//!
//! No UI dependency: usable by the shell, the apps, tests and the future macOS/Linux front-ends.

pub mod adapter;
pub mod clock;
pub mod conflict;
pub mod db;
pub mod engine;
pub mod error;
pub mod outbox;
pub mod paths;
pub mod scheduler;
pub mod status;

pub use adapter::{EntityAdapter, JsonTableAdapter};
pub use clock::{Clock, SkewedClock, SystemClock};
pub use conflict::{three_way_merge, ConflictPolicy, CustomPolicy, MergeResult};
pub use db::{AppSchema, LocalDb, OpenOptions, SyncLock};
pub use engine::{ConflictRecord, CycleReport, EngineBuilder, FeedSpec, LocalTx, PullReport, PushReport, Resolution, SyncEngine};
pub use error::SyncError;
pub use outbox::{EnqueueOutcome, Intent, OpState, OutboxOp};
pub use scheduler::{Scheduler, SchedulerConfig, Trigger};
pub use status::{FeedStatus, SyncEvent, SyncState, SyncStatus};

/// Whether this build encrypts databases (the `sqlcipher` feature).
pub const SQLCIPHER: bool = cfg!(feature = "sqlcipher");
