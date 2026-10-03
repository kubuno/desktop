//! # The typed event system (WinForms model)
//!
//! Work package EVT-1 of `vskubuno/docs/EVENTS.md`: the purely additive core
//! the rest of the event work builds on. Nothing in the runtime raises these
//! events yet (that is EVT-2's input router); the legacy `handlers!` table and
//! [`crate::node::ViewEvent`] are untouched.
//!
//! | WinForms | Here | Module |
//! |---|---|---|
//! | `EventArgs` and its subclasses | the [`EventArgs`] trait (+ `#[derive(EventArgs)]`), [`ArgsChain`] for the ancestor chain | this module, [`args`] |
//! | `HandledEventArgs.Handled` | [`Handled`] (stops [`Event::raise`] at the first handler that sets it) | this module |
//! | `CancelEventArgs.Cancel` | [`Cancelable`] (every handler sees and may reset it) | this module |
//! | `EventHandler<T>`, `+=` / `-=` | [`Event<A>`], [`Event::subscribe`] → [`Subscription`] (drop = `-=`) | [`multicast`] |
//! | `object sender` | [`ElementRef`], [`Sender<C>`] (typed, read-only) | [`sender`] |
//! | `control.Focus()` from a handler | [`ControlQueue`], a deferred command queue | [`sender`] |
//! | `control.Invoke` / `BeginInvoke`, `InvokeRequired` | [`UiDispatcher`] (EVT-6) | [`dispatcher`] |
//! | `async void` handlers on the UI `SynchronizationContext` | `async fn` handlers, [`UiHandle`], [`spawn_local`], [`delay`] (EVT-6) | [`executor`] |
//! | `System.Windows.Forms.Timer` | [`Timer`] (EVT-6) | [`timer`] |
//!
//! Rust has no struct inheritance: "is a `CancelEventArgs`" means "implements
//! [`Cancelable`]", and the ancestor chain tooling uses for handler
//! compatibility is declared with `#[args(extends = …)]`.
//!
//! ```
//! use kubuno_views::events::{ElementRef, Event, EventArgs, Handled, KeyEventArgs, Key};
//! use kubuno_controls::host::{vk, Modifiers};
//!
//! let key_down: Event<KeyEventArgs> = Event::new();
//! let _shortcut = key_down.subscribe(|_sender, e| {
//!     if e.key == Key(vk::F5) {
//!         e.handled = true; // later handlers are not called
//!     }
//! });
//! let _never = key_down.subscribe(|_, _| unreachable!("F5 was handled"));
//!
//! let mut e = KeyEventArgs::new(Key(vk::F5), Modifiers::NONE);
//! key_down.raise(&ElementRef::detached("RefreshButton"), &mut e);
//! assert!(e.handled());
//! assert_eq!(e.type_chain(), ["KeyEventArgs", "EventArgs"]);
//! ```

use std::any::Any;

use crate::binding::Value;

pub mod args;
/// `Control.Invoke` / `BeginInvoke` (EVT-6): [`UiDispatcher`] posts closures to the UI thread.
pub mod dispatcher;
/// The UI thread's async executor (EVT-6): [`spawn_local`], [`UiHandle`], [`delay`].
pub mod executor;
pub mod multicast;
/// A WinForms-like `Timer` component ticking on the UI thread (EVT-6).
pub mod timer;
/// The input router (EVT-2): frame input → ordered WinForms event sequences.
pub mod router;
pub mod sender;
/// Typed handlers (EVT-4): `#[kubuno_views::event_handlers]`, [`EventSink`], the args and
/// sender adapters the generated code calls.
pub mod typed;

pub use args::*;
pub use multicast::{Event, Subscription, MAX_RAISE_DEPTH};
pub use sender::{AnyElement, ControlCommand, ControlQueue, ControlTarget, ElementId, ElementProps, ElementRef, ElementType, Sender};
pub use typed::{dispatch_typed, EventSink, HandlerContext, HandlerInfo, TypedViewModel};
pub use dispatcher::{AsyncResult, DispatchError, UiDispatcher};
pub use executor::{delay, spawn_local, yield_now, Cancelled, Delay, JoinHandle, UiHandle, YieldNow};
pub use timer::Timer;

/// `#[derive(EventArgs)]` — see the macro's own documentation for its
/// `#[args(handled, cancel, extends = …, legacy = …)]` options.
pub use kubuno_views_macros::EventArgs;

/// The root of every event's arguments (WinForms `System.EventArgs`).
///
/// Implement it with `#[derive(EventArgs)]` rather than by hand: the derive
/// also implements [`ArgsChain`] and, on request, [`Handled`] /
/// [`Cancelable`] together with the matching `as_*` hooks below, which is how
/// [`Event::raise`] finds them without specialization.
///
/// ```
/// use kubuno_views::events::{EventArgs, MouseEventArgs};
///
/// let mut e = MouseEventArgs::default();
/// let any: &mut dyn EventArgs = &mut e;
/// assert!(any.is_a("EventArgs") && any.is_a("MouseEventArgs"));
/// assert!(any.downcast_mut::<MouseEventArgs>().is_some());
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an event args type",
    label = "not an `EventArgs`",
    note = "a handler's args are a standard type such as `MouseEventArgs` or `KeyEventArgs`, `&dyn EventArgs` for any event, or your own struct with `#[derive(EventArgs)]`"
)]
pub trait EventArgs: Any {
    /// `self` as [`Any`], for a checked downcast (see `dyn EventArgs::downcast_ref`).
    fn as_any(&self) -> &dyn Any;
    /// `self` as mutable [`Any`].
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// The legacy [`Value`] the `handlers!` table receives for this event
    /// (`vskubuno/docs/EVENTS.md` §5.4), so an event keeps producing exactly
    /// the value it produces today. `Value::Bool(true)`, a click's, by default.
    fn legacy_value(&self) -> Value {
        Value::Bool(true)
    }
    /// The type's name followed by its ancestors' up to `"EventArgs"`, e.g.
    /// `["MouseEventArgs", "EventArgs"]` ([`ArgsChain::CHAIN`]).
    fn type_chain(&self) -> &'static [&'static str];
    /// `Some` when the args implement [`Handled`] (set by the derive's
    /// `#[args(handled)]`).
    fn as_handled(&self) -> Option<&dyn Handled> {
        None
    }
    /// Mutable [`EventArgs::as_handled`].
    fn as_handled_mut(&mut self) -> Option<&mut dyn Handled> {
        None
    }
    /// `Some` when the args implement [`Cancelable`] (set by the derive's
    /// `#[args(cancel)]`).
    fn as_cancelable(&self) -> Option<&dyn Cancelable> {
        None
    }
    /// Mutable [`EventArgs::as_cancelable`].
    fn as_cancelable_mut(&mut self) -> Option<&mut dyn Cancelable> {
        None
    }
}

impl dyn EventArgs {
    /// Whether the concrete args type is `T`.
    pub fn is<T: EventArgs>(&self) -> bool {
        self.as_any().is::<T>()
    }

    /// Checked downcast to the concrete args type.
    pub fn downcast_ref<T: EventArgs>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    /// Checked mutable downcast to the concrete args type.
    pub fn downcast_mut<T: EventArgs>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }

    /// Whether `name` is this type or one of its ancestors
    /// ([`EventArgs::type_chain`]) — the test a handler declared with args
    /// type `name` must pass to be bound to this event.
    pub fn is_a(&self, name: &str) -> bool {
        self.type_chain().contains(&name)
    }
}

/// The compile-time name and ancestor chain of an args type, generated by
/// `#[derive(EventArgs)]`. `#[args(extends = P)]` requires `P: ArgsChain`.
/// Declared in the platform-neutral `kubuno-views-model` (WV-1), beside `EventMeta::args` which reads it.
///
/// ```
/// use kubuno_views::events::{ArgsChain, CancelEventArgs, FormClosingEventArgs};
///
/// assert_eq!(FormClosingEventArgs::NAME, "FormClosingEventArgs");
/// assert_eq!(FormClosingEventArgs::CHAIN, ["FormClosingEventArgs", "CancelEventArgs", "EventArgs"]);
/// assert_eq!(CancelEventArgs::CHAIN, ["CancelEventArgs", "EventArgs"]);
/// ```
pub use kubuno_views_model::ArgsChain;

/// Args a handler cannot write back into (no `handled`, no `cancel`): the only args an `async`
/// handler may take, as a copy (EVT-6). By the time an async handler resumes after an `.await`,
/// the event is over and nobody reads `handled`/`cancel` any more, so `#[event_handlers]`
/// refuses an async handler whose args have them. `#[derive(EventArgs)]` implements it for
/// every args type without `#[args(handled)]` / `#[args(cancel)]`.
#[diagnostic::on_unimplemented(
    message = "an async handler cannot take `{Self}`: its `handled`/`cancel` would be set after the event is over",
    label = "these args are written back by the handler",
    note = "set `handled`/`cancel` in a synchronous handler (`fn f(&mut self, e: &mut {Self})`), and start the async work from it with `spawn_local`"
)]
pub trait ReadOnlyArgs: EventArgs {}

/// Args a handler can mark as handled (WinForms `HandledEventArgs`).
///
/// [`Event::raise`] stops calling handlers after the first one that leaves
/// `handled` set.
pub trait Handled {
    /// Whether a handler has handled the event.
    fn handled(&self) -> bool;
    /// Marks the event handled (or not).
    fn set_handled(&mut self, v: bool);
}

/// Args a handler can cancel (WinForms `CancelEventArgs`).
///
/// Unlike [`Handled`], `cancel` does not stop the raise: every handler sees
/// it and may reset it, and the raiser reads the final value.
pub trait Cancelable {
    /// Whether the operation should be cancelled.
    fn cancel(&self) -> bool;
    /// Requests (or withdraws) the cancellation.
    fn set_cancel(&mut self, v: bool);
}
