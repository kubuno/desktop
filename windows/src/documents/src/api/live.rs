//! A document opened from the server, kept in step with it by a background thread.
//!
//! The [`Session`] state machine has no clock and no threads (see `session.rs`); this module gives
//! it both. One worker thread per open document owns the session and its host (the REST routes and
//! the crash journal), wakes every [`TICK_MS`] and on each command of the window, runs the save
//! jobs the session decides on — off the UI thread, one at a time by construction — and hands what
//! the window must show back through a sink (`UiDispatcher::begin_invoke` in the window, a vector
//! in the tests).
//!
//! The window never waits on the network, except when it closes: [`Live::close`] lets the worker
//! save what is unsaved, flush the journal and leave the editing session, for at most
//! [`CLOSE_WAIT`].
//!
//! What the worker saves is the latest serialisation the window published ([`Live::edited`]): the
//! document lives on the UI thread, the bytes cross over.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::session::{Close, Host, Outcome, Protection, Resolution, SaveJob, Session, State};

/// How often the worker runs the session's clock when nothing happens.
pub const TICK_MS: u64 = 250;
/// How long closing the window may wait for the last save and the leave.
pub const CLOSE_WAIT: Duration = Duration::from_secs(8);

/// What the worker tells the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The document is open: the server's content (or, offline, the journal's), and a crash journal
    /// left by a previous run that differs from it, for the window to offer.
    Opened { title: String, content: Vec<u8>, recovered: Option<Vec<u8>>, protection: Protection, offline: bool },
    /// It could not be opened at all (no account, not found, offline with no journal).
    Failed(String),
    /// Where the session stands, sent whenever it changes.
    Status(Status),
    /// The server copy moved under us: only the user can choose (never resolved automatically).
    Conflict,
    /// The user took the server's copy: the window loads these bytes.
    Reload(Vec<u8>),
    /// The session has left; the worker has ended.
    Closed,
}

/// The session's state, as the status bar shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub state: State,
    pub protection: Protection,
    pub unsaved: bool,
    pub editors: Vec<String>,
}

enum Cmd {
    Edited { structural: bool },
    SaveNow,
    Recovered(bool),
    Resolve(Resolution),
    Close(Sender<()>),
}

/// Where the worker's events go.
pub type Sink = Arc<dyn Fn(Event) + Send + Sync>;

/// The window's handle on the worker.
pub struct Live {
    tx: Sender<Cmd>,
    content: Arc<Mutex<Vec<u8>>>,
    pub id: String,
}

impl Live {
    /// Starts the worker for document `id`. `connect` runs on the worker: it builds the host and
    /// returns the document's title (a failure is reported as [`Event::Failed`]).
    pub fn open<H, F>(id: String, sink: Sink, connect: F) -> Self
    where
        H: Host + Send + 'static,
        F: FnOnce() -> Result<(H, String), String> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let content = Arc::new(Mutex::new(Vec::new()));
        let shared = content.clone();
        let doc = id.clone();
        let spawned = std::thread::Builder::new().name(format!("documents-live-{id}")).spawn(move || match connect() {
            Ok((host, title)) => run(doc, title, host, shared, rx, sink),
            Err(why) => {
                sink(Event::Failed(why));
                // Drain: a close must still be answered.
                while let Ok(cmd) = rx.recv() {
                    if let Cmd::Close(done) = cmd {
                        let _ = done.send(());
                        break;
                    }
                }
            }
        });
        if let Err(e) = spawned {
            kubuno::tracing::error!("[documents] live session thread: {e}");
        }
        Self { tx, content, id }
    }

    /// The document changed: `bytes` is its serialisation now. Structural edits (margins, page
    /// setup) save on the short debounce, like the web's `scheduleSave`.
    pub fn edited(&self, bytes: Vec<u8>, structural: bool) {
        *self.content.lock().unwrap_or_else(|p| p.into_inner()) = bytes;
        let _ = self.tx.send(Cmd::Edited { structural });
    }

    /// Ctrl+S: save now (the session still refuses from a conflict or an oversized document).
    pub fn save_now(&self, bytes: Vec<u8>) {
        *self.content.lock().unwrap_or_else(|p| p.into_inner()) = bytes;
        let _ = self.tx.send(Cmd::SaveNow);
    }

    /// The answer to the recovery offer: `true` = the journal's copy was loaded (the window then
    /// publishes it with [`Live::edited`]), `false` = it is discarded.
    pub fn recovered(&self, kept: bool) {
        let _ = self.tx.send(Cmd::Recovered(kept));
    }

    /// The user's choice in the conflict dialog.
    pub fn resolve(&self, choice: Resolution) {
        let _ = self.tx.send(Cmd::Resolve(choice));
    }

    /// Saves what can be, flushes the journal, leaves; waits at most [`CLOSE_WAIT`].
    pub fn close(&self, bytes: Option<Vec<u8>>) -> bool {
        if let Some(b) = bytes {
            *self.content.lock().unwrap_or_else(|p| p.into_inner()) = b;
        }
        let (done_tx, done_rx) = mpsc::channel();
        if self.tx.send(Cmd::Close(done_tx)).is_err() {
            return true;
        }
        done_rx.recv_timeout(CLOSE_WAIT).is_ok()
    }
}

fn run<H: Host>(id: String, title: String, mut host: H, content: Arc<Mutex<Vec<u8>>>, rx: Receiver<Cmd>, sink: Sink) {
    let start = Instant::now();
    let now = || start.elapsed().as_millis() as u64;
    let bytes = {
        let content = content.clone();
        move || content.lock().unwrap_or_else(|p| p.into_inner()).clone()
    };
    let mut session = Session::open(id.clone());
    let recovered = session.recover(&mut host);
    let protection = session.begin(now(), &mut host);
    let server = host.fetch(&id).ok().map(|s| s.content);
    let (initial, recovered, offline) = match (server, recovered) {
        (Some(s), Some(r)) if r == s => {
            session.discard_recovery(&mut host);
            (s, None, false)
        }
        (Some(s), r) => (s, r, false),
        // Offline: the journal is the only copy we have; it opens, and saves once the server answers.
        (None, Some(r)) => (r, None, true),
        (None, None) => {
            sink(Event::Failed("document injoignable (hors ligne ?) et aucune copie locale".into()));
            wait_close(&rx);
            return;
        }
    };
    *content.lock().unwrap_or_else(|p| p.into_inner()) = initial.clone();
    if offline {
        // What opened is the journal: unsaved work by definition.
        session.edited(now());
    }
    sink(Event::Opened { title, content: initial, recovered, protection, offline });

    let mut last: Option<Status> = None;
    let mut conflict_told = false;
    let report = |session: &Session, last: &mut Option<Status>, conflict_told: &mut bool| {
        let status = Status { state: session.state.clone(), protection: session.protection(), unsaved: session.has_unsaved(), editors: session.editors().to_vec() };
        if session.state == State::Conflict {
            if !*conflict_told {
                *conflict_told = true;
                sink(Event::Conflict);
            }
        } else {
            *conflict_told = false;
        }
        if last.as_ref() != Some(&status) {
            *last = Some(status.clone());
            sink(Event::Status(status));
        }
    };
    let execute = |job: SaveJob, session: &mut Session, host: &mut H| {
        let outcome: Outcome = job.run(host);
        session.finish(outcome, now(), host);
    };
    report(&session, &mut last, &mut conflict_told);

    loop {
        match rx.recv_timeout(Duration::from_millis(TICK_MS)) {
            Ok(Cmd::Edited { structural }) => {
                if structural {
                    session.structural(now());
                } else {
                    session.edited(now());
                }
            }
            Ok(Cmd::SaveNow) => {
                session.touch();
                if let Some(job) = session.save_now(&bytes) {
                    report(&session, &mut last, &mut conflict_told);
                    execute(job, &mut session, &mut host);
                }
            }
            Ok(Cmd::Recovered(kept)) => {
                if !kept {
                    session.discard_recovery(&mut host);
                }
            }
            Ok(Cmd::Resolve(choice)) => {
                if choice == Resolution::TakeTheirs {
                    match host.fetch(&id) {
                        Ok(s) => {
                            *content.lock().unwrap_or_else(|p| p.into_inner()) = s.content.clone();
                            session.resolve(choice, &mut host);
                            sink(Event::Reload(s.content));
                        }
                        // Not resolved: the conflict stays and is offered again.
                        Err(_) => conflict_told = false,
                    }
                } else {
                    session.resolve(choice, &mut host);
                }
            }
            Ok(Cmd::Close(done)) => {
                close(&mut session, &bytes, &mut host, &execute);
                report(&session, &mut last, &mut conflict_told);
                sink(Event::Closed);
                let _ = done.send(());
                return;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                close(&mut session, &bytes, &mut host, &execute);
                return;
            }
        }
        if let Some(job) = session.tick(now(), &bytes, &mut host) {
            report(&session, &mut last, &mut conflict_told);
            execute(job, &mut session, &mut host);
        }
        report(&session, &mut last, &mut conflict_told);
    }
}

fn close<H: Host>(session: &mut Session, bytes: &dyn super::session::Content, host: &mut H, execute: &dyn Fn(SaveJob, &mut Session, &mut H)) {
    // `close` asks for at most two saves, then flushes the journal and leaves; the bound only guards
    // against a session that would keep answering `Wait` (no save is ever in flight here).
    for _ in 0..8 {
        match session.close(bytes, host) {
            Close::Save(job) => execute(*job, session, host),
            Close::Wait => {}
            Close::Done => return,
        }
    }
}

fn wait_close(rx: &Receiver<Cmd>) {
    while let Ok(cmd) = rx.recv() {
        if let Cmd::Close(done) = cmd {
            let _ = done.send(());
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::session::{Digest, Error, Journal, Server, Snapshot};
    use serde_json::{Map, Value};

    /// A server and journal in memory, shared with the test through an `Arc`.
    #[derive(Default)]
    struct World {
        content: Vec<u8>,
        patches: Vec<Vec<u8>>,
        joined: u32,
        left: u32,
        journal: Option<Vec<u8>>,
        offline: bool,
        too_large: bool,
    }

    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<World>>);

    impl Fake {
        fn w(&self) -> std::sync::MutexGuard<'_, World> {
            self.0.lock().unwrap_or_else(|p| p.into_inner())
        }
    }

    impl Server for Fake {
        fn fetch(&mut self, _id: &str) -> Result<Snapshot, Error> {
            let w = self.w();
            if w.offline {
                return Err(Error::Transport("offline".into()));
            }
            Ok(Snapshot { content: w.content.clone(), etag: Some(format!("e{}", w.patches.len())), editors: vec![] })
        }
        fn patch(&mut self, job: &SaveJob) -> Result<Option<String>, Error> {
            let mut w = self.w();
            if w.offline {
                return Err(Error::Transport("offline".into()));
            }
            if w.too_large {
                return Err(Error::TooLarge);
            }
            w.content = job.content.clone();
            w.patches.push(job.content.clone());
            Ok(Some(format!("e{}", w.patches.len())))
        }
        fn join(&mut self, id: &str) -> Result<Snapshot, Error> {
            self.w().joined += 1;
            self.fetch(id)
        }
        fn ping(&mut self, _id: &str) -> Result<(), Error> {
            Ok(())
        }
        fn leave(&mut self, _id: &str) -> Result<(), Error> {
            self.w().left += 1;
            Ok(())
        }
        fn config(&mut self) -> Result<Map<String, Value>, Error> {
            Ok(Map::new())
        }
    }

    impl Journal for Fake {
        fn write(&mut self, _id: &str, bytes: &[u8]) -> Result<(), Error> {
            self.w().journal = Some(bytes.to_vec());
            Ok(())
        }
        fn read(&mut self, _id: &str) -> Option<Vec<u8>> {
            self.w().journal.clone()
        }
        fn clear(&mut self, _id: &str) {
            self.w().journal = None;
        }
    }

    fn start(world: &Fake) -> (Live, Arc<Mutex<Vec<Event>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = events.clone();
        let sink: Sink = Arc::new(move |e| sink_events.lock().unwrap_or_else(|p| p.into_inner()).push(e));
        let host = world.clone();
        let live = Live::open("d1".into(), sink, move || Ok((host, "Rapport".to_string())));
        (live, events)
    }

    fn wait_for(events: &Arc<Mutex<Vec<Event>>>, what: impl Fn(&Event) -> bool) -> bool {
        for _ in 0..200 {
            if events.lock().unwrap_or_else(|p| p.into_inner()).iter().any(&what) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn opens_saves_on_ctrl_s_and_leaves_once_on_close() {
        let world = Fake::default();
        world.w().content = br#"{"type":"doc"}"#.to_vec();
        let (live, events) = start(&world);
        assert!(wait_for(&events, |e| matches!(e, Event::Opened { recovered: None, offline: false, .. })));
        live.save_now(br#"{"type":"doc","x":1}"#.to_vec());
        assert!(wait_for(&events, |e| matches!(e, Event::Status(s) if s.state == State::Clean && !s.unsaved)) );
        for _ in 0..200 {
            if !world.w().patches.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(world.w().content, br#"{"type":"doc","x":1}"#.to_vec());
        assert!(live.close(None));
        let (joined, left) = { let w = world.w(); (w.joined, w.left) };
        assert_eq!((joined, left), (1, 1));
        assert!(world.w().journal.is_none());
    }

    #[test]
    fn a_change_on_the_server_raises_a_conflict_instead_of_overwriting() {
        let world = Fake::default();
        world.w().content = br#"{"type":"doc"}"#.to_vec();
        let (live, events) = start(&world);
        assert!(wait_for(&events, |e| matches!(e, Event::Opened { .. })));
        // A browser tab saved meanwhile.
        world.w().content = br#"{"type":"doc","web":true}"#.to_vec();
        live.save_now(br#"{"type":"doc","mine":true}"#.to_vec());
        assert!(wait_for(&events, |e| *e == Event::Conflict));
        assert_eq!(world.w().content, br#"{"type":"doc","web":true}"#.to_vec(), "never overwritten without a choice");
        live.resolve(Resolution::TakeTheirs);
        assert!(wait_for(&events, |e| *e == Event::Reload(br#"{"type":"doc","web":true}"#.to_vec())));
        assert!(live.close(None));
        assert!(world.w().patches.is_empty());
    }

    #[test]
    fn offline_the_journal_opens_and_is_kept_on_close() {
        let world = Fake::default();
        world.w().offline = true;
        world.w().journal = Some(br#"{"type":"doc","draft":1}"#.to_vec());
        let (live, events) = start(&world);
        assert!(wait_for(&events, |e| matches!(e, Event::Opened { offline: true, .. })));
        live.edited(br#"{"type":"doc","draft":2}"#.to_vec(), false);
        assert!(live.close(None));
        assert_eq!(world.w().journal.as_deref(), Some(&br#"{"type":"doc","draft":2}"#[..]), "the unsaved work stays on disk");
        assert!(world.w().patches.is_empty());
    }

    #[test]
    fn an_oversized_document_stops_saving() {
        let world = Fake::default();
        world.w().content = br#"{"type":"doc"}"#.to_vec();
        world.w().too_large = true;
        let (live, events) = start(&world);
        assert!(wait_for(&events, |e| matches!(e, Event::Opened { .. })));
        live.save_now(br#"{"type":"doc","big":1}"#.to_vec());
        assert!(wait_for(&events, |e| matches!(e, Event::Status(s) if s.state == State::TooLarge)));
        live.save_now(br#"{"type":"doc","big":2}"#.to_vec());
        assert!(live.close(None));
        assert_eq!(Digest::of(&world.w().content), Digest::of(br#"{"type":"doc"}"#));
    }

    #[test]
    fn a_failed_connection_is_reported_and_close_still_answers() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let e2 = events.clone();
        let sink: Sink = Arc::new(move |e| e2.lock().unwrap_or_else(|p| p.into_inner()).push(e));
        let live = Live::open("d".into(), sink, || Err::<(Fake, String), _>("aucun compte".into()));
        assert!(wait_for(&events, |e| *e == Event::Failed("aucun compte".into())));
        assert!(live.close(None));
    }
}
