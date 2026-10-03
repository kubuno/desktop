//! [`Event<A>`], a multicast event, and [`Subscription`], its RAII token
//! (`vskubuno/docs/EVENTS.md` §2).
//!
//! Dispatch semantics, every one covered by this module's tests:
//!
//! - **Order.** Handlers run in subscription order, synchronously, on the
//!   raising (UI) thread. `Event<A>` is `!Send`.
//! - **`-=` is `Drop`.** Dropping the [`Subscription`] unsubscribes;
//!   [`Subscription::detach`] keeps the handler for the event's lifetime.
//! - **Added during a raise:** not called in that raise (snapshot, like .NET);
//!   it runs from the next raise.
//! - **Removed during a raise:** *not* called any more in that raise (a
//!   deliberate deviation from .NET: dropping a subscription usually goes with
//!   dropping the state its closure talks to).
//! - **Re-entrancy.** A handler may raise the same event again. A handler that
//!   is already running is skipped in the nested raise (with a
//!   `tracing::warn!`) rather than panicking — which is also what ends a
//!   ping-pong between two events. The nesting of raises on a thread (all
//!   events together) is capped at [`MAX_RAISE_DEPTH`], which catches longer
//!   cascades between controls: a raise beyond it calls nothing and logs a
//!   warning.
//! - **[`Handled`](super::Handled)** args stop the raise after the first
//!   handler that leaves `handled` set. **[`Cancelable`](super::Cancelable)**
//!   args do not: every handler sees and may reset `cancel`.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};

use super::{ElementRef, EventArgs};

/// The deepest nesting of [`Event::raise`] calls allowed on one thread (all
/// events together, so a ping-pong between two events is caught too). A raise
/// beyond it calls no handler.
pub const MAX_RAISE_DEPTH: usize = 32;

thread_local! {
    /// How many raises are running on this thread, nested in one another.
    static RAISE_DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// Decrements [`RAISE_DEPTH`] on every exit of [`Event::raise`], a panicking
/// handler included.
struct DepthGuard;

impl DepthGuard {
    /// `None` when the thread is already [`MAX_RAISE_DEPTH`] raises deep.
    fn enter() -> Option<Self> {
        RAISE_DEPTH.with(|d| {
            if d.get() >= MAX_RAISE_DEPTH {
                None
            } else {
                d.set(d.get() + 1);
                Some(DepthGuard)
            }
        })
    }
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        RAISE_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

type HandlerFn<A> = dyn FnMut(&ElementRef<'_>, &mut A);

/// One subscribed handler. `removed` is the tombstone a raise in progress
/// checks before calling it.
struct Slot<A> {
    id: u64,
    removed: Cell<bool>,
    handler: RefCell<Box<HandlerFn<A>>>,
}

struct Slots<A> {
    next_id: u64,
    list: Vec<Rc<Slot<A>>>,
}

/// The type-erased side of an event a [`Subscription`] points back to.
trait SlotOwner {
    fn remove(&self, id: u64);
    fn contains(&self, id: u64) -> bool;
}

impl<A: 'static> SlotOwner for RefCell<Slots<A>> {
    fn remove(&self, id: u64) {
        // Never borrowed across a handler call (raise snapshots first), so
        // this cannot conflict; `try_` anyway, a failure only leaks the slot.
        if let Ok(mut slots) = self.try_borrow_mut() {
            if let Some(pos) = slots.list.iter().position(|s| s.id == id) {
                let slot = slots.list.remove(pos);
                slot.removed.set(true);
            }
        }
    }

    fn contains(&self, id: u64) -> bool {
        self.try_borrow().is_ok_and(|s| s.list.iter().any(|slot| slot.id == id))
    }
}

/// A multicast event: the Rust `event EventHandler<A>`.
///
/// The building block for Rust code (custom controls, view models,
/// services). Cloning an `Event` gives another handle to the *same* handler
/// list. UI thread only (`!Send`).
///
/// ```
/// use std::cell::RefCell;
/// use std::rc::Rc;
/// use kubuno_desktop_views::events::{ElementRef, Event, MouseEventArgs};
///
/// let click: Event<MouseEventArgs> = Event::new();
/// let log = Rc::new(RefCell::new(Vec::new()));
///
/// let l = log.clone();
/// let first = click.subscribe(move |sender, e| l.borrow_mut().push(format!("{:?} {}", sender.name, e.clicks)));
/// let l = log.clone();
/// click.subscribe(move |_, _| l.borrow_mut().push("second".into())).detach();
///
/// let mut e = MouseEventArgs { clicks: 1, ..Default::default() };
/// click.raise(&ElementRef::detached("Ok"), &mut e);
/// drop(first); // unsubscribes
/// click.raise(&ElementRef::detached("Ok"), &mut e);
///
/// assert_eq!(*log.borrow(), ["Some(\"Ok\") 1", "second", "second"]);
/// ```
pub struct Event<A: EventArgs> {
    slots: Rc<RefCell<Slots<A>>>,
}

impl<A: EventArgs> Event<A> {
    /// An event with no subscriber.
    pub fn new() -> Self {
        Self { slots: Rc::new(RefCell::new(Slots { next_id: 0, list: Vec::new() })) }
    }

    /// Adds `handler` after the existing ones (WinForms `+=`). It stays
    /// subscribed as long as the returned [`Subscription`] lives.
    ///
    /// Called during a raise of this event, the new handler is not called by
    /// that raise.
    pub fn subscribe(&self, handler: impl FnMut(&ElementRef<'_>, &mut A) + 'static) -> Subscription {
        let id = {
            let mut slots = self.slots.borrow_mut();
            let id = slots.next_id;
            slots.next_id += 1;
            slots.list.push(Rc::new(Slot { id, removed: Cell::new(false), handler: RefCell::new(Box::new(handler)) }));
            id
        };
        let owner: Rc<dyn SlotOwner> = self.slots.clone();
        Subscription { owner: Some(Rc::downgrade(&owner)), id }
    }

    /// Calls every handler in subscription order with `sender` and `args`
    /// (see the module doc for the exact semantics). The raiser reads the
    /// handlers' outputs (`handled`, `cancel`, `effect`…) from `args` after.
    pub fn raise(&self, sender: &ElementRef<'_>, args: &mut A) {
        let Some(_depth) = DepthGuard::enter() else {
            tracing::warn!(
                args = std::any::type_name::<A>(),
                sender = sender.name.unwrap_or(sender.element),
                "event raise skipped: more than {MAX_RAISE_DEPTH} nested raises (event ping-pong?)"
            );
            return;
        };
        // Snapshot: handlers added from now on are not part of this raise.
        let snapshot: Vec<Rc<Slot<A>>> = match self.slots.try_borrow() {
            Ok(slots) => slots.list.clone(),
            Err(_) => return,
        };
        for slot in snapshot {
            if slot.removed.get() {
                continue;
            }
            let Ok(mut handler) = slot.handler.try_borrow_mut() else {
                tracing::warn!(
                    args = std::any::type_name::<A>(),
                    sender = sender.name.unwrap_or(sender.element),
                    "re-entrant event raise: skipping a handler that is already running"
                );
                continue;
            };
            handler(sender, args);
            drop(handler);
            if args.as_handled().is_some_and(|h| h.handled()) {
                break;
            }
        }
    }

    /// Whether at least one handler is subscribed — lets a raiser skip
    /// building costly args.
    pub fn has_subscribers(&self) -> bool {
        self.subscriber_count() > 0
    }

    /// How many handlers are subscribed.
    pub fn subscriber_count(&self) -> usize {
        self.slots.try_borrow().map_or(0, |s| s.list.len())
    }

    /// Unsubscribes every handler (their [`Subscription`]s become no-ops).
    pub fn clear(&self) {
        if let Ok(mut slots) = self.slots.try_borrow_mut() {
            for slot in slots.list.drain(..) {
                slot.removed.set(true);
            }
        }
    }
}

impl<A: EventArgs> Default for Event<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: EventArgs> Clone for Event<A> {
    /// Another handle to the same handler list.
    fn clone(&self) -> Self {
        Self { slots: self.slots.clone() }
    }
}

impl<A: EventArgs> fmt::Debug for Event<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Event")
            .field("args", &std::any::type_name::<A>())
            .field("subscribers", &self.subscriber_count())
            .finish()
    }
}

/// The token [`Event::subscribe`] returns: dropping it unsubscribes the
/// handler (WinForms `-=`).
///
/// Holds only a weak reference: the event may be dropped first, the token
/// then does nothing.
#[must_use = "dropping a Subscription unsubscribes"]
pub struct Subscription {
    owner: Option<Weak<dyn SlotOwner>>,
    id: u64,
}

impl Subscription {
    /// Keeps the handler subscribed for the event's whole lifetime (the
    /// frequent "subscribe once, never remove" case).
    pub fn detach(mut self) {
        self.owner = None;
    }

    /// Unsubscribes now (same as dropping the token).
    pub fn unsubscribe(self) {
        drop(self);
    }

    /// Whether the handler is still subscribed: `false` once the event is
    /// gone or was [`Event::clear`]ed.
    pub fn is_active(&self) -> bool {
        self.owner.as_ref().and_then(Weak::upgrade).is_some_and(|o| o.contains(self.id))
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take().and_then(|w| w.upgrade()) {
            owner.remove(self.id);
        }
    }
}

impl fmt::Debug for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subscription").field("id", &self.id).field("active", &self.is_active()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{CancelEventArgs, EmptyEventArgs, EventArgs, HandledEventArgs, KeyEventArgs};

    type Log = Rc<RefCell<Vec<&'static str>>>;

    fn log() -> Log {
        Rc::new(RefCell::new(Vec::new()))
    }

    fn push(log: &Log, s: &'static str) -> impl FnMut(&ElementRef<'_>, &mut EmptyEventArgs) + 'static {
        let log = log.clone();
        move |_, _| log.borrow_mut().push(s)
    }

    fn sender() -> ElementRef<'static> {
        ElementRef::detached("Test")
    }

    fn raise(ev: &Event<EmptyEventArgs>) {
        ev.raise(&sender(), &mut EmptyEventArgs);
    }

    #[test]
    fn subscription_order_is_call_order() {
        let ev = Event::new();
        let l = log();
        let _a = ev.subscribe(push(&l, "a"));
        let _b = ev.subscribe(push(&l, "b"));
        let _c = ev.subscribe(push(&l, "c"));
        raise(&ev);
        raise(&ev);
        assert_eq!(*l.borrow(), ["a", "b", "c", "a", "b", "c"]);
    }

    #[test]
    fn raise_without_subscribers_is_a_no_op() {
        let ev: Event<EmptyEventArgs> = Event::default();
        assert!(!ev.has_subscribers());
        raise(&ev);
    }

    #[test]
    fn dropping_the_subscription_unsubscribes() {
        let ev = Event::new();
        let l = log();
        let a = ev.subscribe(push(&l, "a"));
        let _b = ev.subscribe(push(&l, "b"));
        assert_eq!(ev.subscriber_count(), 2);
        assert!(a.is_active());
        drop(a);
        assert_eq!(ev.subscriber_count(), 1);
        raise(&ev);
        assert_eq!(*l.borrow(), ["b"]);
    }

    #[test]
    fn unsubscribe_middle_keeps_order_of_the_rest() {
        let ev = Event::new();
        let l = log();
        let _a = ev.subscribe(push(&l, "a"));
        let b = ev.subscribe(push(&l, "b"));
        let _c = ev.subscribe(push(&l, "c"));
        b.unsubscribe();
        let _d = ev.subscribe(push(&l, "d"));
        raise(&ev);
        assert_eq!(*l.borrow(), ["a", "c", "d"]);
    }

    #[test]
    fn unused_subscription_result_unsubscribes_immediately() {
        let ev = Event::new();
        let l = log();
        // Bound to `_`: dropped at the end of the statement.
        let _ = ev.subscribe(push(&l, "gone"));
        raise(&ev);
        assert!(l.borrow().is_empty());
        assert!(!ev.has_subscribers());
    }

    #[test]
    fn detach_keeps_the_handler() {
        let ev = Event::new();
        let l = log();
        ev.subscribe(push(&l, "a")).detach();
        raise(&ev);
        assert_eq!(*l.borrow(), ["a"]);
        assert_eq!(ev.subscriber_count(), 1);
    }

    #[test]
    fn subscription_outliving_the_event_is_harmless() {
        let l = log();
        let sub = {
            let ev = Event::new();
            ev.subscribe(push(&l, "a"))
        };
        assert!(!sub.is_active());
        drop(sub);
    }

    #[test]
    fn clones_share_the_handler_list_and_clear_empties_it() {
        let ev = Event::new();
        let other = ev.clone();
        let l = log();
        let sub = other.subscribe(push(&l, "a"));
        raise(&ev);
        assert_eq!(*l.borrow(), ["a"]);
        ev.clear();
        assert!(!sub.is_active());
        raise(&other);
        assert_eq!(*l.borrow(), ["a"]);
        drop(sub);
        assert!(format!("{ev:?}").contains("subscribers: 0"));
    }

    #[test]
    fn handler_added_during_raise_runs_from_the_next_raise() {
        let ev: Event<EmptyEventArgs> = Event::new();
        let l = log();
        let keep: Rc<RefCell<Vec<Subscription>>> = Rc::default();
        let (ev2, l2, keep2) = (ev.clone(), l.clone(), keep.clone());
        let _a = ev.subscribe(move |_, _| {
            l2.borrow_mut().push("a");
            if keep2.borrow().is_empty() {
                let sub = ev2.subscribe(push(&l2, "late"));
                keep2.borrow_mut().push(sub);
            }
        });
        let _b = ev.subscribe(push(&l, "b"));
        raise(&ev);
        assert_eq!(*l.borrow(), ["a", "b"]);
        raise(&ev);
        assert_eq!(*l.borrow(), ["a", "b", "a", "b", "late"]);
        keep.borrow_mut().clear();
    }

    #[test]
    fn handler_removed_during_raise_is_not_called() {
        let ev = Event::new();
        let l = log();
        let victim: Rc<RefCell<Option<Subscription>>> = Rc::default();
        let (l2, v2) = (l.clone(), victim.clone());
        let _a = ev.subscribe(move |_, _| {
            l2.borrow_mut().push("a");
            v2.borrow_mut().take(); // drops b's subscription
        });
        *victim.borrow_mut() = Some(ev.subscribe(push(&l, "b")));
        let _c = ev.subscribe(push(&l, "c"));
        raise(&ev);
        assert_eq!(*l.borrow(), ["a", "c"]);
        assert_eq!(ev.subscriber_count(), 2);
    }

    #[test]
    fn handler_can_unsubscribe_itself() {
        let ev = Event::new();
        let l = log();
        let me: Rc<RefCell<Option<Subscription>>> = Rc::default();
        let (l2, me2) = (l.clone(), me.clone());
        *me.borrow_mut() = Some(ev.subscribe(move |_, _| {
            l2.borrow_mut().push("once");
            me2.borrow_mut().take();
        }));
        let _b = ev.subscribe(push(&l, "b"));
        raise(&ev);
        raise(&ev);
        assert_eq!(*l.borrow(), ["once", "b", "b"]);
    }

    #[test]
    fn clear_during_raise_stops_the_remaining_handlers() {
        let ev = Event::new();
        let l = log();
        let (ev2, l2) = (ev.clone(), l.clone());
        let _a = ev.subscribe(move |_, _| {
            l2.borrow_mut().push("a");
            ev2.clear();
        });
        let _b = ev.subscribe(push(&l, "b"));
        raise(&ev);
        assert_eq!(*l.borrow(), ["a"]);
    }

    #[test]
    fn reentrant_raise_skips_the_running_handler() {
        let ev: Event<EmptyEventArgs> = Event::new();
        let l = log();
        let (ev2, l2) = (ev.clone(), l.clone());
        let depth = Rc::new(Cell::new(0));
        let d2 = depth.clone();
        let _a = ev.subscribe(move |_, _| {
            l2.borrow_mut().push("a");
            d2.set(d2.get() + 1);
            ev2.raise(&ElementRef::detached("Nested"), &mut EmptyEventArgs);
        });
        let _b = ev.subscribe(push(&l, "b"));
        raise(&ev);
        // Outer: a (nested raise: a skipped, b), then b.
        assert_eq!(*l.borrow(), ["a", "b", "b"]);
        assert_eq!(depth.get(), 1);
    }

    #[test]
    fn ping_pong_between_two_events_ends_at_the_running_handler() {
        // Two events whose handlers raise each other: the second raise of
        // `ping` finds its handler still running and skips it.
        let ping: Event<EmptyEventArgs> = Event::new();
        let pong: Event<EmptyEventArgs> = Event::new();
        let calls = Rc::new(Cell::new(0_usize));
        let (pong2, c2) = (pong.clone(), calls.clone());
        let _p = ping.subscribe(move |s, e| {
            c2.set(c2.get() + 1);
            pong2.raise(s, e);
        });
        let (ping2, c3) = (ping.clone(), calls.clone());
        let _q = pong.subscribe(move |s, e| {
            c3.set(c3.get() + 1);
            ping2.raise(s, e);
        });
        raise(&ping);
        assert_eq!(calls.get(), 2);
    }

    /// A chain of `n` distinct events, each handler raising the next one.
    fn chain(n: usize, calls: &Rc<Cell<usize>>) -> (Vec<Event<EmptyEventArgs>>, Vec<Subscription>) {
        let events: Vec<Event<EmptyEventArgs>> = (0..n).map(|_| Event::new()).collect();
        let subs = (0..n)
            .map(|i| {
                let next = events.get(i + 1).cloned();
                let c = calls.clone();
                events[i].subscribe(move |s, e| {
                    c.set(c.get() + 1);
                    if let Some(next) = &next {
                        next.raise(s, e);
                    }
                })
            })
            .collect();
        (events, subs)
    }

    #[test]
    fn nesting_is_capped_at_max_depth() {
        let calls = Rc::new(Cell::new(0_usize));
        let (events, _subs) = chain(MAX_RAISE_DEPTH + 8, &calls);
        raise(&events[0]);
        assert_eq!(calls.get(), MAX_RAISE_DEPTH);
        // The depth counter is back to zero: a fresh raise gets the full budget.
        calls.set(0);
        raise(&events[0]);
        assert_eq!(calls.get(), MAX_RAISE_DEPTH);
        RAISE_DEPTH.with(|d| assert_eq!(d.get(), 0));
    }

    #[test]
    fn nesting_up_to_the_cap_is_allowed() {
        let calls = Rc::new(Cell::new(0_usize));
        let (events, _subs) = chain(MAX_RAISE_DEPTH, &calls);
        raise(&events[0]);
        assert_eq!(calls.get(), MAX_RAISE_DEPTH);
    }

    #[test]
    fn depth_counter_survives_a_panicking_handler() {
        let ev: Event<EmptyEventArgs> = Event::new();
        let _a = ev.subscribe(|_, _| panic!("boom"));
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| raise(&ev)));
        assert!(caught.is_err());
        RAISE_DEPTH.with(|d| assert_eq!(d.get(), 0));
    }

    #[test]
    fn handled_short_circuits() {
        let ev: Event<KeyEventArgs> = Event::new();
        let l = log();
        let l1 = l.clone();
        let _a = ev.subscribe(move |_, _| l1.borrow_mut().push("a"));
        let l2 = l.clone();
        let _b = ev.subscribe(move |_, e| {
            l2.borrow_mut().push("b");
            e.handled = true;
        });
        let l3 = l.clone();
        let _c = ev.subscribe(move |_, _| l3.borrow_mut().push("c"));
        let mut e = KeyEventArgs::default();
        ev.raise(&sender(), &mut e);
        assert!(e.handled);
        assert_eq!(*l.borrow(), ["a", "b"]);
    }

    #[test]
    fn handled_reset_by_the_same_handler_does_not_stop() {
        let ev: Event<HandledEventArgs> = Event::new();
        let count = Rc::new(Cell::new(0));
        let c1 = count.clone();
        let _a = ev.subscribe(move |_, e| {
            e.handled = true;
            e.handled = false;
            c1.set(c1.get() + 1);
        });
        let c2 = count.clone();
        let _b = ev.subscribe(move |_, _| c2.set(c2.get() + 1));
        let mut e = HandledEventArgs::default();
        ev.raise(&sender(), &mut e);
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn cancel_is_seen_and_resettable_by_every_handler() {
        let ev: Event<CancelEventArgs> = Event::new();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s1 = seen.clone();
        let _a = ev.subscribe(move |_, e| {
            s1.borrow_mut().push(e.cancel);
            e.cancel = true;
        });
        let s2 = seen.clone();
        let _b = ev.subscribe(move |_, e| {
            s2.borrow_mut().push(e.cancel);
            e.cancel = false; // a later handler overrides the veto
        });
        let s3 = seen.clone();
        let _c = ev.subscribe(move |_, e| s3.borrow_mut().push(e.cancel));
        let mut e = CancelEventArgs::default();
        ev.raise(&sender(), &mut e);
        assert_eq!(*seen.borrow(), [false, true, false]);
        assert!(!e.cancel);
        assert!(e.as_handled().is_none());
    }

    #[test]
    fn sender_reaches_the_handler() {
        let ev: Event<EmptyEventArgs> = Event::new();
        let got = Rc::new(RefCell::new(String::new()));
        let g = got.clone();
        let _a = ev.subscribe(move |s, _| *g.borrow_mut() = format!("{}:{}", s.element, s.id));
        let s = ElementRef { name: Some("Ok"), element: "Button", id: "0.2", bounds: Default::default(), focus_id: None, attributes: &[] };
        ev.raise(&s, &mut EmptyEventArgs);
        assert_eq!(*got.borrow(), "Button:0.2");
    }
}
