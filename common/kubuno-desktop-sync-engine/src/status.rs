//! What the UI binds to (the future `SyncStatus` component and `SyncIndicator` control, lot SE-4): a state, counts,
//! per-feed details, and events (tables changed, conflict detected).

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SyncState {
    /// Nothing pending, last cycle succeeded.
    Synced,
    Syncing,
    /// Local changes not yet accepted by the server.
    Pending { count: i64 },
    /// No network, or the forced offline mode of `settings.json`.
    Offline,
    /// The refresh token was rejected: feeds paused, local data readable, outbox kept.
    SessionExpired,
    /// The last cycle failed for another reason (code + message, no payload).
    Error { message: String },
    /// The database is read-only after a failed migration.
    ReadOnly { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeedStatus {
    pub feed: String,
    pub cursor: String,
    pub needs_full: bool,
    /// Local clock, milliseconds (display only).
    pub last_pull_at: Option<i64>,
    pub last_ok_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncStatus {
    pub state: SyncState,
    /// Local clock, milliseconds, of the last successful full cycle (display only).
    pub last_sync_at: Option<i64>,
    pub pending_count: i64,
    /// Pending ops that already failed 3 times ("N modifications en attente").
    pub stuck_count: i64,
    pub conflict_count: i64,
    pub feeds: Vec<FeedStatus>,
}

impl Default for SyncStatus {
    fn default() -> Self {
        Self { state: SyncState::Synced, last_sync_at: None, pending_count: 0, stuck_count: 0, conflict_count: 0, feeds: Vec::new() }
    }
}

/// In-process notifications. Published only after the transaction that caused them committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncEvent {
    /// Rows of these tables changed (local write, pull, push answer, rollback): bound adapters re-read them.
    TablesChanged(Vec<String>),
    /// A new open conflict.
    ConflictDetected { id: i64, entity: String, entity_id: String, kind: String },
    /// A local intent was rejected definitively and rolled back.
    Rejected { entity: String, entity_id: String, message: String },
    StatusChanged(SyncStatus),
}
