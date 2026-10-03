//! A WinForms `System.Windows.Forms.Timer`-like non-visual component (`vskubuno/docs/
//! EVENTS.md` §3 "Timer → Tick", work package EVT-6): raises `Tick` on the UI thread every
//! `interval` milliseconds while it is enabled.
//!
//! ```
//! use kubuno_desktop_views::events::Timer;
//!
//! let clock = Timer::new("clock").with_interval(1000).with_handler("clock_tick");
//! clock.start(); // ticks once the runtime it is added to (`Runtime::add_timer`) paints
//! assert!(clock.enabled() && clock.interval() == 1000);
//! ```
//!
//! Like WinForms' timer (and unlike a thread timer), a tick runs **between** frames' input on
//! the UI thread, so its handler has `&mut self` of the view model: `with_handler("clock_tick")`
//! names a handler method exactly as `OnTick="clock_tick"` would (a typed `#[event_handlers]`
//! method or a `handlers!` entry; the sender is the timer, element `"Timer"`). Rust code can also
//! subscribe to [`Timer::tick`]. Ticks that fall due while the UI thread is busy are coalesced
//! into one (as `WM_TIMER` does), and the next tick is due `interval` after the one that ran. The
//! runtime asks the host to wake it for the next due tick, minimised windows included.
//!
//! Deviation: WinForms' timer resolution is the system tick (~15.6 ms); here it is the frame
//! clock, scheduled with `host::request_wake_after` (a Win32 `SetTimer`, same resolution).

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use kubuno_desktop_ui::Rect;

use super::router::{raise, Dispatch, SlotEvents};
use super::{EmptyEventArgs, Event};

/// WinForms' default `Timer.Interval`.
pub const DEFAULT_INTERVAL_MS: u32 = 100;

struct TimerInner {
    name: String,
    interval: Cell<u32>,
    enabled: Cell<bool>,
    /// The sender of its ticks and the handler they run (rebuilt when the handler changes).
    slot: RefCell<Rc<SlotEvents>>,
    tick: Event<EmptyEventArgs>,
    /// When the next tick is due, in ms of the runtime's clock; `None` = restart counting at the
    /// next pump (just started, interval changed).
    due: Cell<Option<u64>>,
}

/// See the module documentation. `Clone` is another handle to the same timer (`Rc`); `!Send`:
/// it lives on the UI thread.
#[derive(Clone)]
pub struct Timer(Rc<TimerInner>);

impl fmt::Debug for Timer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Timer")
            .field("name", &self.0.name)
            .field("interval", &self.interval())
            .field("enabled", &self.enabled())
            .field("handler", &self.handler())
            .finish()
    }
}

fn slot_for(name: &str, handler: Option<&str>) -> Rc<SlotEvents> {
    let mut slot = SlotEvents::new(format!("timer:{name}"), "Timer");
    slot.name = Some(name.to_string());
    if let Some(h) = handler.filter(|h| !h.is_empty()) {
        slot = slot.with_handler("OnTick", h);
    }
    Rc::new(slot)
}

impl Timer {
    /// A disabled timer named `name` (its sender's `x:Name`) with the default 100 ms interval.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let slot = slot_for(&name, None);
        Self(Rc::new(TimerInner {
            name,
            interval: Cell::new(DEFAULT_INTERVAL_MS),
            enabled: Cell::new(false),
            slot: RefCell::new(slot),
            tick: Event::new(),
            due: Cell::new(None),
        }))
    }

    /// Builder form of [`Timer::set_interval`].
    pub fn with_interval(self, ms: u32) -> Self {
        self.set_interval(ms);
        self
    }

    /// Builder form of [`Timer::set_handler`].
    pub fn with_handler(self, handler: &str) -> Self {
        self.set_handler(Some(handler));
        self
    }

    pub fn name(&self) -> &str {
        &self.0.name
    }

    /// Milliseconds between ticks (at least 1).
    pub fn interval(&self) -> u32 {
        self.0.interval.get()
    }

    /// Changes the interval; a running timer starts counting again from now (WinForms).
    pub fn set_interval(&self, ms: u32) {
        self.0.interval.set(ms.max(1));
        self.0.due.set(None);
    }

    pub fn enabled(&self) -> bool {
        self.0.enabled.get()
    }

    /// Starts (`true`) or stops the timer (WinForms `Enabled`). Starting counts from now.
    pub fn set_enabled(&self, enabled: bool) {
        if enabled != self.0.enabled.replace(enabled) {
            self.0.due.set(None);
        }
    }

    pub fn start(&self) {
        self.set_enabled(true);
    }

    pub fn stop(&self) {
        self.set_enabled(false);
    }

    /// The view-model handler each tick runs (as `OnTick="…"` would name it).
    pub fn handler(&self) -> Option<String> {
        self.0.slot.borrow().handler("OnTick").map(str::to_string)
    }

    /// Names the handler each tick runs, or none.
    pub fn set_handler(&self, handler: Option<&str>) {
        *self.0.slot.borrow_mut() = slot_for(&self.0.name, handler);
    }

    /// The `Tick` event, for Rust subscribers (raised after the named handler).
    pub fn tick(&self) -> &Event<EmptyEventArgs> {
        &self.0.tick
    }

    pub(crate) fn same(&self, other: &Timer) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    /// Raises the tick if it is due at `now`, and returns when the next one is due (`None` when
    /// disabled).
    pub(crate) fn pump(&self, now: u64, d: &mut Dispatch<'_>) -> Option<u64> {
        if !self.enabled() {
            return None;
        }
        let interval = u64::from(self.interval());
        let Some(due) = self.0.due.get() else {
            let due = now + interval;
            self.0.due.set(Some(due));
            return Some(due);
        };
        if now < due {
            return Some(due);
        }
        // Rescheduled before the handler runs, so a handler that stops, restarts or changes the
        // interval has the last word.
        self.0.due.set(Some(now + interval));
        let slot = self.0.slot.borrow().clone();
        raise(d, &slot, Rect::default(), "OnTick", EmptyEventArgs);
        if self.0.tick.has_subscribers() {
            self.0.tick.raise(&slot.sender(Rect::default()), &mut EmptyEventArgs);
        }
        if !self.enabled() {
            return None;
        }
        match self.0.due.get() {
            Some(d) => Some(d),
            None => {
                let d = now + u64::from(self.interval());
                self.0.due.set(Some(d));
                Some(d)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binding::{HandlerTable, MapViewModel, Value};
    use crate::node::ViewEvent;

    /// A fake clock driving `pump` like the runtime does at each frame; the log is every
    /// tick handler call with the clock time.
    struct Bench {
        vm: MapViewModel,
        handlers: HandlerTable,
        log: Rc<RefCell<Vec<u64>>>,
        now: Rc<Cell<u64>>,
    }

    impl Bench {
        fn new() -> Self {
            let log = Rc::new(RefCell::new(Vec::new()));
            let now = Rc::new(Cell::new(0));
            let (l, n) = (log.clone(), now.clone());
            let mut handlers = HandlerTable::new();
            handlers.insert("clock_tick", Box::new(move |_vm, v| {
                assert_eq!(v, Value::Bool(true));
                l.borrow_mut().push(n.get());
            }));
            Self { vm: MapViewModel::new(), handlers, log, now }
        }

        fn frame(&mut self, t: &Timer, now: u64) -> (Option<u64>, Vec<ViewEvent>) {
            self.now.set(now);
            let mut events = Vec::new();
            let next = t.pump(now, &mut Dispatch { vm: &mut self.vm, handlers: &mut self.handlers, events: &mut events });
            (next, events)
        }
    }

    #[test]
    fn ticks_every_interval_on_a_fake_clock() {
        let mut b = Bench::new();
        let t = Timer::new("clock").with_interval(1000).with_handler("clock_tick");
        assert_eq!(b.frame(&t, 0).0, None, "disabled: no tick, no wake-up");
        t.start();
        assert_eq!(b.frame(&t, 50).0, Some(1050), "counting starts at the first pump after start");
        assert_eq!(b.frame(&t, 900).0, Some(1050));
        let (next, events) = b.frame(&t, 1050);
        assert_eq!(next, Some(2050));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].handler.as_deref(), Some("clock_tick"));
        assert_eq!(b.frame(&t, 2100).0, Some(3100));
        assert_eq!(*b.log.borrow(), [1050, 2100]);
    }

    #[test]
    fn late_ticks_are_coalesced_and_stop_or_interval_changes_restart_counting() {
        let mut b = Bench::new();
        let t = Timer::new("clock").with_interval(100).with_handler("clock_tick");
        t.start();
        b.frame(&t, 0);
        // The UI thread was busy for 1 s: one tick, not ten.
        assert_eq!(b.frame(&t, 1000).0, Some(1100));
        assert_eq!(b.log.borrow().len(), 1);
        t.set_interval(500);
        assert_eq!(b.frame(&t, 1050).0, Some(1550), "a new interval counts from now");
        t.stop();
        assert_eq!(b.frame(&t, 1600).0, None);
        assert_eq!(b.log.borrow().len(), 1);
        t.start();
        assert_eq!(b.frame(&t, 5000).0, Some(5500), "restarting does not tick for the time it was stopped");
    }

    #[test]
    fn rust_subscribers_see_the_timer_as_sender_and_a_handler_can_stop_it() {
        let mut b = Bench::new();
        let t = Timer::new("once").with_interval(10);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let (s, stopper) = (seen.clone(), t.clone());
        t.tick()
            .subscribe(move |sender, _e| {
                s.borrow_mut().push(format!("{}:{}", sender.element, sender.display_name()));
                stopper.stop();
            })
            .detach();
        t.start();
        b.frame(&t, 0);
        assert_eq!(b.frame(&t, 10).0, None, "stopped from its own tick");
        assert_eq!(b.frame(&t, 100).0, None);
        assert_eq!(*seen.borrow(), ["Timer:once"]);
        assert!(b.log.borrow().is_empty(), "no handler named: only the Rust subscriber ran");
    }
}
