//! Local time. **Never used for ordering** (§7.4): order comes from the server's `change_seq` and the outbox's
//! `seq`. The local clock only stamps display fields (`last_pull_at`, "il y a 2 min") and schedules retries, so a
//! client clock skewed by hours changes nothing but those texts.

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub trait Clock: Send + Sync + std::fmt::Debug {
    /// Milliseconds since the Unix epoch, local clock.
    fn now_ms(&self) -> i64;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
    }
}

/// The system clock shifted by a fixed offset (tests of clock skew).
#[derive(Debug, Default)]
pub struct SkewedClock {
    offset_ms: AtomicI64,
}

impl SkewedClock {
    pub fn new(offset_ms: i64) -> Self {
        Self { offset_ms: AtomicI64::new(offset_ms) }
    }

    pub fn set_offset_ms(&self, offset_ms: i64) {
        self.offset_ms.store(offset_ms, Ordering::SeqCst);
    }
}

impl Clock for SkewedClock {
    fn now_ms(&self) -> i64 {
        SystemClock.now_ms() + self.offset_ms.load(Ordering::SeqCst)
    }
}
