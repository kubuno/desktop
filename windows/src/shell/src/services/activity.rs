//! The activity log: what the sync loop has been doing.
//!
//! The Tauri build kept this in the web page's state, which no longer exists,
//! so the events are recorded here in a bounded log — the shell runs for weeks
//! and an unbounded list would just grow. The activity page shows it
//! (`activity_page`), the header's bell counts it.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How many events are kept. Enough to explain what happened this morning,
/// small enough to never matter.
const MAX_EVENTS: usize = 100;

#[derive(Debug, Clone)]
pub struct Event {
    /// "synced" | "conflict" | "error", as the daemon reports it.
    pub kind:  String,
    pub title: String,
    pub body:  String,
    pub at:    Instant,
}

static LOG: Mutex<Vec<Event>> = Mutex::new(Vec::new());

/// Records one event, dropping the oldest once the log is full.
pub fn record(kind: &str, title: &str, body: &str) {
    let Ok(mut log) = LOG.lock() else { return };
    log.insert(0, Event { kind: kind.to_string(), title: title.to_string(), body: body.to_string(), at: Instant::now() });
    log.truncate(MAX_EVENTS);
}

/// How many events are logged — what the bell's badge shows.
pub fn count() -> usize {
    LOG.lock().map(|l| l.len()).unwrap_or(0)
}

/// The events, newest first.
pub fn events() -> Vec<Event> {
    LOG.lock().map(|l| l.clone()).unwrap_or_default()
}

/// Renders an age the way a person reads it. Deliberately coarse: the exact
/// second of a sync helps nobody, and it avoids a date-formatting dependency.
pub fn age(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..=45 => crate::Resources::age_now().to_string(),
        46..=5400 => crate::Resources::age_minutes().replace("{0}", &((s + 30) / 60).to_string()),
        _ if s < 172_800 => crate::Resources::age_hours().replace("{0}", &((s + 1800) / 3600).to_string()),
        _ => crate::Resources::age_days().replace("{0}", &((s + 43_200) / 86_400).to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_read_like_a_person_says_them() {
        kubuno_desktop::resources::set_culture("fr");
        assert_eq!(age(Duration::from_secs(10)), "à l'instant");
        assert_eq!(age(Duration::from_secs(600)), "il y a 10 min");
        assert_eq!(age(Duration::from_secs(7200)), "il y a 2 h");
        assert_eq!(age(Duration::from_secs(3 * 86_400)), "il y a 3 j");
    }

    #[test]
    fn the_log_is_bounded_and_newest_first() {
        for i in 0..(MAX_EVENTS + 5) {
            record("synced", &format!("e{i}"), "");
        }
        let events = events();
        assert!(events.len() <= MAX_EVENTS);
        assert_eq!(events[0].title, format!("e{}", MAX_EVENTS + 4));
    }
}
