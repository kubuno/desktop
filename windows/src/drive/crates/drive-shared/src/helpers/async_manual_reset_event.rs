//! Port of `Files.Shared/Helpers/AsyncManualResetEvent.cs`.

use std::time::Duration;
use tokio::sync::watch;

/// An async manual-reset event: `wait` completes once `set` has been called,
/// until `reset` re-arms it.
pub struct AsyncManualResetEvent {
    tx: watch::Sender<bool>,
}

impl AsyncManualResetEvent {
    pub fn new() -> Self {
        let (tx, _rx) = watch::channel(false);
        Self { tx }
    }

    /// Waits until the event is set.
    pub async fn wait(&self) {
        let mut rx = self.tx.subscribe();
        // wait_for returns immediately if already true.
        let _ = rx.wait_for(|set| *set).await;
    }

    /// Waits with a timeout; returns `true` if the event was set in time.
    pub async fn wait_timeout(&self, timeout: Duration) -> bool {
        tokio::time::timeout(timeout, self.wait()).await.is_ok()
    }

    pub fn set(&self) {
        // send_replace updates the value even with no live receivers.
        self.tx.send_replace(true);
    }

    pub fn reset(&self) {
        self.tx.send_replace(false);
    }

    pub fn is_set(&self) -> bool {
        *self.tx.borrow()
    }
}

impl Default for AsyncManualResetEvent {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn set_releases_waiters_and_reset_rearms() {
        let event = AsyncManualResetEvent::new();
        assert!(!event.wait_timeout(Duration::from_millis(10)).await);

        event.set();
        assert!(event.wait_timeout(Duration::from_millis(10)).await);
        // Already-set event completes immediately.
        event.wait().await;

        event.reset();
        assert!(!event.wait_timeout(Duration::from_millis(10)).await);
    }
}
