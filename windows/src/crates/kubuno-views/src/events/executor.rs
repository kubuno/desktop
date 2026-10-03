//! The UI thread's async executor (`vskubuno/docs/EVENTS.md` §6, work package EVT-6) — the
//! role WinForms' `WindowsFormsSynchronizationContext` plays for `async void` handlers.
//!
//! Each view runtime owns a single-threaded executor polled from the message loop: the
//! runtime polls the tasks that were woken at the start of each frame (and once more after the
//! frame's own handlers ran). A waker may fire from any thread (an I/O thread, a
//! [`crate::events::UiDispatcher`] result): it marks the task ready and wakes the host window,
//! so the task continues on the UI thread, never elsewhere.
//!
//! - [`spawn_local`] starts a future on the current view's executor (inside a handler or a
//!   task); `Runtime::spawn_local` does it from outside. Tasks are `!Send` futures: they may
//!   hold `Rc`s.
//! - [`UiHandle<V>`] is what an `async` handler gets instead of `&mut self`: a borrow of the
//!   view model cannot live across `.await`, so state is changed through
//!   [`UiHandle::update`], which lends `&mut V` for the duration of a closure.
//! - [`delay`] is a timer future on the view's clock (the frame time), [`yield_now`] gives
//!   the frame back once.
//! - Closing the view cancels every task: their futures are dropped at their current `.await`
//!   (WinForms has no equivalent — an `async void` handler there outlives its form and throws on
//!   the next UI access); [`JoinHandle`] then reports [`Cancelled`] and [`UiHandle::update`]
//!   returns `None`.
//!
//! ```
//! # use kubuno_views::prelude::*;
//! # use std::time::Duration;
//! # #[derive(Default)] struct MainViewModel { status: String }
//! # impl ViewModel for MainViewModel {
//! #     fn get(&self, _: &str) -> Option<Value> { None }
//! #     fn set(&mut self, _: &str, _: Value) {}
//! # }
//! #[kubuno_views::event_handlers]
//! impl MainViewModel {
//!     // `OnClick="refresh_click"`: runs on the UI thread, awaits without blocking it.
//!     async fn refresh_click(ui: UiHandle<Self>, e: MouseEventArgs) {
//!         ui.update(|vm| vm.status = "Refreshing…".into());
//!         delay(Duration::from_millis(500)).await;
//!         ui.update(|vm| vm.status = format!("Refreshed ({} clicks)", e.clicks));
//!     }
//! }
//! ```

use std::any::TypeId;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use super::dispatcher::{UiDispatcher, UiShared};

/// How many times one pump re-polls tasks woken while it runs (a task that yields in a loop
/// gets the next frame, instead of freezing this one).
const MAX_PASSES: usize = 8;

type LocalTask = Pin<Box<dyn Future<Output = ()>>>;

/// One view's UI-thread services: the async executor and its timers, and the cross-thread
/// half ([`UiShared`]: posted closures, ready tasks, host wake-up). Owned by the runtime (`Rc`);
/// [`UiHandle`]s and [`JoinHandle`]s hold it weakly.
pub(crate) struct UiContext {
    id: usize,
    pub(crate) shared: Arc<UiShared>,
    tasks: RefCell<HashMap<u64, (String, LocalTask)>>,
    next_task: Cell<u64>,
    /// `(deadline, waker)` of the pending [`delay`]s.
    timers: RefCell<Vec<(u64, Waker)>>,
    /// The frame time, in ms of the runtime's clock (what [`delay`] measures against).
    now: Cell<u64>,
    closed: Cell<bool>,
    /// The view's named components (DATA-2, `crate::scope`): what `scope::current` finds.
    pub(crate) scope: crate::scope::ComponentScope,
}

impl fmt::Debug for UiContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UiContext").field("id", &self.id).field("tasks", &self.tasks.borrow().len()).field("closed", &self.closed.get()).finish()
    }
}

static NEXT_CONTEXT: AtomicUsize = AtomicUsize::new(1);

impl UiContext {
    pub(crate) fn new(shared: Arc<UiShared>) -> Rc<Self> {
        Rc::new(Self {
            id: NEXT_CONTEXT.fetch_add(1, Ordering::Relaxed),
            shared,
            tasks: RefCell::new(HashMap::new()),
            next_task: Cell::new(1),
            timers: RefCell::new(Vec::new()),
            now: Cell::new(0),
            closed: Cell::new(false),
            scope: crate::scope::ComponentScope::new(),
        })
    }

    pub(crate) fn now(&self) -> u64 {
        self.now.get()
    }

    /// Sets the frame time (monotonic: a clock going back is ignored).
    pub(crate) fn set_now(&self, now: u64) {
        if now > self.now.get() {
            self.now.set(now);
        }
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.get()
    }

    /// How many tasks are alive (spawned, not finished, not cancelled).
    #[cfg(test)]
    pub(crate) fn task_count(&self) -> usize {
        self.tasks.borrow().len()
    }

    fn spawn<T: 'static>(self: &Rc<Self>, name: &str, future: impl Future<Output = T> + 'static) -> JoinHandle<T> {
        let state = Rc::new(RefCell::new(JoinState { result: None, cancelled: false, waiter: None }));
        let id = self.next_task.get();
        let handle = JoinHandle { state: state.clone(), task: Some((Rc::downgrade(self), id)) };
        if self.is_closed() {
            tracing::warn!("task `{name}` spawned after its view closed: never started");
            state.borrow_mut().cancelled = true;
            return handle;
        }
        self.next_task.set(id + 1);
        let task = TaskFuture { future: Box::pin(future), state: Some(state) };
        self.tasks.borrow_mut().insert(id, (name.to_string(), Box::pin(task)));
        // First poll at the next pump (never inside the handler that spawned it: the view model
        // is borrowed there).
        self.shared.mark_ready(id);
        handle
    }

    fn cancel(&self, id: u64) {
        let task = self.tasks.borrow_mut().remove(&id);
        // Dropped outside the borrow: the future's locals may run any `Drop` code.
        drop(task);
    }

    /// Wakes every [`delay`] whose deadline has passed.
    pub(crate) fn fire_timers(&self) {
        let now = self.now();
        let due: Vec<Waker> = {
            let mut timers = self.timers.borrow_mut();
            let (due, pending): (Vec<_>, Vec<_>) = timers.drain(..).partition(|(deadline, _)| *deadline <= now);
            *timers = pending;
            due.into_iter().map(|(_, w)| w).collect()
        };
        for w in due {
            w.wake();
        }
    }

    /// The earliest pending [`delay`] deadline.
    pub(crate) fn next_deadline(&self) -> Option<u64> {
        self.timers.borrow().iter().map(|(d, _)| *d).min()
    }

    fn register_timer(&self, deadline: u64, waker: &Waker) {
        let mut timers = self.timers.borrow_mut();
        if !timers.iter().any(|(d, w)| *d == deadline && w.will_wake(waker)) {
            timers.push((deadline, waker.clone()));
        }
    }

    /// Polls the tasks woken since the last pump (repeatedly, up to [`MAX_PASSES`], for the
    /// ones woken meanwhile). Returns how many polls ran.
    pub(crate) fn run_ready(&self) -> usize {
        let mut polls = 0;
        for _ in 0..MAX_PASSES {
            let ready = self.shared.take_ready();
            if ready.is_empty() || self.is_closed() {
                break;
            }
            for id in ready {
                // Taken out of the map while it runs, so it may spawn or cancel tasks.
                let Some((name, mut future)) = self.tasks.borrow_mut().remove(&id) else { continue };
                let waker = Waker::from(Arc::new(TaskWaker { task: id, shared: Arc::downgrade(&self.shared) }));
                let mut cx = Context::from_waker(&waker);
                polls += 1;
                let started = std::time::Instant::now();
                let done = future.as_mut().poll(&mut cx).is_ready();
                let took = started.elapsed();
                if took > Duration::from_millis(50) {
                    tracing::warn!("async task `{name}` blocked the UI thread for {} ms between two awaits", took.as_millis());
                }
                if !done && !self.is_closed() {
                    self.tasks.borrow_mut().insert(id, (name, future));
                }
            }
        }
        polls
    }

    /// Cancels every task and timer, refuses new ones, closes the dispatcher.
    pub(crate) fn shutdown(&self) {
        if self.closed.replace(true) {
            return;
        }
        self.shared.close();
        let tasks = std::mem::take(&mut *self.tasks.borrow_mut());
        if !tasks.is_empty() {
            tracing::debug!("view closed: {} async task(s) cancelled", tasks.len());
        }
        drop(tasks);
        self.timers.borrow_mut().clear();
    }
}

/// A task's waker: marks it ready and wakes the host window, from any thread.
struct TaskWaker {
    task: u64,
    shared: std::sync::Weak<UiShared>,
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if let Some(shared) = self.shared.upgrade() {
            if !shared.is_closed() {
                shared.mark_ready(self.task);
            }
        }
    }
}

// ── Current view (the "synchronization context") ────────────────────────────

thread_local! {
    /// The views whose frame is running on this thread, innermost last.
    static CURRENT: RefCell<Vec<Rc<UiContext>>> = const { RefCell::new(Vec::new()) };
    /// The view model lent to [`UiHandle::update`] while tasks are polled: `(context id,
    /// type, pointer)`. Taken (set to `None`) while an update runs, so two `&mut` never coexist.
    static SCOPED_VM: Cell<Option<(usize, TypeId, *mut ())>> = const { Cell::new(None) };
}

/// Makes `ctx` the current view until the guard drops (what [`spawn_local`] and
/// [`UiHandle::current`] find).
pub(crate) fn enter(ctx: &Rc<UiContext>) -> CurrentGuard {
    CURRENT.with(|c| c.borrow_mut().push(ctx.clone()));
    CurrentGuard(())
}

pub(crate) struct CurrentGuard(());

impl Drop for CurrentGuard {
    fn drop(&mut self) {
        CURRENT.with(|c| {
            c.borrow_mut().pop();
        });
    }
}

fn current() -> Option<Rc<UiContext>> {
    CURRENT.with(|c| c.borrow().last().cloned())
}

/// The components of the view whose frame is running (`crate::scope::current`).
pub(crate) fn current_scope() -> Option<crate::scope::ComponentScope> {
    current().map(|ctx| ctx.scope.clone())
}

/// Runs `f` with `vm` lent to the [`UiHandle`]s of `ctx` (see [`UiHandle::update`]).
pub(crate) fn with_vm_scope<V: 'static, R>(ctx: &UiContext, vm: &mut V, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<(usize, TypeId, *mut ())>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SCOPED_VM.with(|s| s.set(self.0));
        }
    }
    let ptr = (vm as *mut V).cast::<()>();
    let _restore = Restore(SCOPED_VM.with(|s| s.replace(Some((ctx.id, TypeId::of::<V>(), ptr)))));
    f()
}

/// Starts `future` on the current view's UI-thread executor (inside a handler, a task, a
/// timer tick — anywhere a view's frame is running). Its first poll happens right after the
/// current handler returns, within the same frame or the next one; it then runs on the UI
/// thread only, until it finishes or the view closes (which cancels it).
///
/// Outside a view's frame there is no current view: the future is dropped, a warning logged,
/// and the handle reports [`Cancelled`] — use `Runtime::spawn_local` there.
pub fn spawn_local<T: 'static>(future: impl Future<Output = T> + 'static) -> JoinHandle<T> {
    spawn_named("spawn_local", future)
}

/// [`spawn_local`] with a name for the diagnostics (the handler name for an async handler).
#[doc(hidden)]
pub fn spawn_named<T: 'static>(name: &str, future: impl Future<Output = T> + 'static) -> JoinHandle<T> {
    match current() {
        Some(ctx) => ctx.spawn(name, future),
        None => {
            tracing::warn!("task `{name}` spawned outside a view's frame: dropped (use Runtime::spawn_local)");
            JoinHandle { state: Rc::new(RefCell::new(JoinState { result: None, cancelled: true, waiter: None })), task: None }
        }
    }
}

/// What `#[event_handlers]` generates for an `async fn` handler: spawns it as a task named
/// after the handler.
#[doc(hidden)]
pub fn spawn_handler(handler: &str, future: impl Future<Output = ()> + 'static) {
    // Detached (dropping a handle does not cancel its task).
    drop(spawn_named(handler, future));
}

/// What the generated code of an async handler calls when it runs outside a view's frame
/// (a direct `dispatch_typed` call): there is no view to give it a `UiHandle` of.
#[doc(hidden)]
pub fn no_view_for(handler: &str) {
    tracing::warn!("async handler `{handler}` raised outside a view's frame: not started (paint with Runtime::frame_typed)");
}

pub(crate) fn spawn_on<T: 'static>(ctx: &Rc<UiContext>, future: impl Future<Output = T> + 'static) -> JoinHandle<T> {
    ctx.spawn("Runtime::spawn_local", future)
}

// ── Tasks and their handles ──────────────────────────────────────────────────

struct JoinState<T> {
    result: Option<T>,
    cancelled: bool,
    waiter: Option<Waker>,
}

/// Wraps a spawned future: stores its output in the [`JoinHandle`]'s state, and marks the
/// task cancelled when dropped unfinished (closing the view, [`JoinHandle::cancel`]).
struct TaskFuture<F: Future> {
    future: Pin<Box<F>>,
    state: Option<Rc<RefCell<JoinState<F::Output>>>>,
}

impl<F: Future> Future for TaskFuture<F> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = &mut *self;
        match this.future.as_mut().poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(value) => {
                if let Some(state) = this.state.take() {
                    let waiter = {
                        let mut s = state.borrow_mut();
                        s.result = Some(value);
                        s.waiter.take()
                    };
                    if let Some(w) = waiter {
                        w.wake();
                    }
                }
                Poll::Ready(())
            }
        }
    }
}

impl<F: Future> Drop for TaskFuture<F> {
    fn drop(&mut self) {
        if let Some(state) = self.state.take() {
            let waiter = match state.try_borrow_mut() {
                Ok(mut s) => {
                    s.cancelled = true;
                    s.waiter.take()
                }
                Err(_) => None,
            };
            if let Some(w) = waiter {
                w.wake();
            }
        }
    }
}

/// The task was cancelled (its view closed, or [`JoinHandle::cancel`]) before it finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the UI task was cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// A handle to a task started with [`spawn_local`]: await it for its output, or cancel it.
/// Dropping it does NOT cancel the task (it detaches it, like WinForms' fire-and-forget).
pub struct JoinHandle<T> {
    state: Rc<RefCell<JoinState<T>>>,
    task: Option<(Weak<UiContext>, u64)>,
}

impl<T> fmt::Debug for JoinHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JoinHandle").field("finished", &self.is_finished()).field("cancelled", &self.is_cancelled()).finish()
    }
}

impl<T> JoinHandle<T> {
    /// Whether the task produced its output (still to be taken or awaited, or already taken).
    pub fn is_finished(&self) -> bool {
        let s = self.state.borrow();
        s.result.is_some() || (!s.cancelled && self.task.as_ref().is_some_and(|(ctx, id)| ctx.upgrade().is_some_and(|c| !c.tasks.borrow().contains_key(id))))
    }

    /// Whether the task was cancelled before it finished.
    pub fn is_cancelled(&self) -> bool {
        let s = self.state.borrow();
        s.cancelled && s.result.is_none()
    }

    /// Stops the task at its current `.await` (its future is dropped). No-op once it finished.
    pub fn cancel(&self) {
        if let Some((ctx, id)) = &self.task {
            if let Some(ctx) = ctx.upgrade() {
                ctx.cancel(*id);
            }
        }
    }

    /// The task's output, if it has finished and it was not taken yet.
    pub fn try_take(&self) -> Option<T> {
        self.state.borrow_mut().result.take()
    }
}

impl<T> Future for JoinHandle<T> {
    type Output = Result<T, Cancelled>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut s = self.state.borrow_mut();
        if let Some(v) = s.result.take() {
            return Poll::Ready(Ok(v));
        }
        if s.cancelled {
            return Poll::Ready(Err(Cancelled));
        }
        s.waiter = Some(cx.waker().clone());
        Poll::Pending
    }
}

// ── UiHandle ────────────────────────────────────────────────────────────────

/// What an `async` handler gets instead of `&mut self` (`async fn refresh_click(ui:
/// UiHandle<Self>, e: MouseEventArgs)`): a handle to the view model of the view that raised
/// the event, usable across `.await`s. Not `Send`: it stays on the UI thread with its task (give
/// a worker thread a [`UiHandle::dispatcher`] instead).
pub struct UiHandle<V: 'static> {
    ctx: Weak<UiContext>,
    _vm: PhantomData<fn(&mut V)>,
}

impl<V: 'static> Clone for UiHandle<V> {
    fn clone(&self) -> Self {
        Self { ctx: self.ctx.clone(), _vm: PhantomData }
    }
}

impl<V: 'static> fmt::Debug for UiHandle<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UiHandle").field("view_model", &std::any::type_name::<V>()).field("closed", &self.is_closed()).finish()
    }
}

impl<V: 'static> UiHandle<V> {
    /// The handle of the view whose frame is running (what the generated code of an async
    /// handler calls). `None` outside a view's frame.
    pub fn current() -> Option<Self> {
        current().map(|ctx| Self { ctx: Rc::downgrade(&ctx), _vm: PhantomData })
    }

    pub(crate) fn of(ctx: &Rc<UiContext>) -> Self {
        Self { ctx: Rc::downgrade(ctx), _vm: PhantomData }
    }

    /// Runs `f` with `&mut` of the view model, now, and returns its result — the view repaints
    /// with the change at the end of the current frame. `None` when it cannot: the view closed
    /// (the task is being cancelled), the view is painted with another view model type or
    /// untyped (`Runtime::frame`), or `update` is called from inside another `update`'s closure,
    /// or from a context the runtime is not polling (a handle moved into a closure that runs
    /// elsewhere).
    pub fn update<R>(&self, f: impl FnOnce(&mut V) -> R) -> Option<R> {
        let ctx = self.ctx.upgrade().filter(|c| !c.is_closed())?;
        let Some((id, ty, ptr)) = SCOPED_VM.with(|s| s.take()) else {
            tracing::warn!("UiHandle::update outside a task polled by its view (or nested in another update): not run");
            return None;
        };
        // Given back even if `f` panics.
        struct Restore(Option<(usize, TypeId, *mut ())>);
        impl Drop for Restore {
            fn drop(&mut self) {
                SCOPED_VM.with(|s| s.set(self.0));
            }
        }
        let _restore = Restore(Some((id, ty, ptr)));
        if id != ctx.id || ty != TypeId::of::<V>() {
            tracing::warn!("UiHandle<{}>::update: the view is painted with another view model type (use Runtime::frame_typed)", std::any::type_name::<V>());
            return None;
        }
        // SAFETY: `ptr` was made by `with_vm_scope` from a `&mut V` (the type is checked just
        // above) that is exclusively borrowed for the whole scope, during which nothing but this
        // function touches the view model. The pointer is taken out of `SCOPED_VM` while `f`
        // runs, so a nested `update` finds `None` instead of making a second `&mut`, and the
        // scope ends (restoring the previous value) only after this borrow is gone.
        let vm = unsafe { &mut *ptr.cast::<V>() };
        Some(f(vm))
    }

    /// A [`UiDispatcher`] of the same view, to hand to a worker thread. `None` once it closed.
    pub fn dispatcher(&self) -> Option<UiDispatcher<V>> {
        let ctx = self.ctx.upgrade().filter(|c| !c.is_closed())?;
        Some(UiDispatcher::new(ctx.shared.clone()))
    }

    /// The named components of the view (DATA-2, `crate::scope`): `None` once it closed.
    pub fn components(&self) -> Option<crate::scope::ComponentScope> {
        let ctx = self.ctx.upgrade().filter(|c| !c.is_closed())?;
        Some(ctx.scope.clone())
    }

    /// Whether the view closed: `update` does nothing any more.
    pub fn is_closed(&self) -> bool {
        self.ctx.upgrade().is_none_or(|c| c.is_closed())
    }
}

// ── Futures ─────────────────────────────────────────────────────────────────

/// Waits `duration` without blocking the UI thread (measured on the view's frame clock). Outside
/// a view's task it falls back to a helper thread's sleep, so it works on any executor.
pub fn delay(duration: Duration) -> Delay {
    Delay { ms: u64::try_from(duration.as_millis()).unwrap_or(u64::MAX), state: DelayState::Idle }
}

enum DelayState {
    Idle,
    /// On a view's clock.
    View { ctx: Weak<UiContext>, deadline: u64 },
    /// On a helper thread (no current view).
    Thread(Arc<AtomicBool>),
}

/// The future of [`delay`].
pub struct Delay {
    ms: u64,
    state: DelayState,
}

impl fmt::Debug for Delay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Delay").field("ms", &self.ms).finish()
    }
}

impl Future for Delay {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if matches!(self.state, DelayState::Idle) {
            self.state = match current() {
                Some(ctx) => DelayState::View { deadline: ctx.now().saturating_add(self.ms), ctx: Rc::downgrade(&ctx) },
                None => {
                    let done = Arc::new(AtomicBool::new(false));
                    let (flag, waker, ms) = (done.clone(), cx.waker().clone(), self.ms);
                    // Named for the debugger's Threads window.
                    let spawned = std::thread::Builder::new().name("kubuno-delay".into()).spawn(move || {
                        std::thread::sleep(Duration::from_millis(ms));
                        flag.store(true, Ordering::Release);
                        waker.wake();
                    });
                    if spawned.is_err() {
                        // No thread to wait with: the delay ends at once rather than never.
                        done.store(true, Ordering::Release);
                    }
                    DelayState::Thread(done)
                }
            };
        }
        match &self.state {
            DelayState::View { ctx, deadline } => {
                let Some(ctx) = ctx.upgrade() else { return Poll::Pending };
                if ctx.now() >= *deadline {
                    return Poll::Ready(());
                }
                ctx.register_timer(*deadline, cx.waker());
                Poll::Pending
            }
            DelayState::Thread(done) => {
                if done.load(Ordering::Acquire) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }
            DelayState::Idle => Poll::Pending,
        }
    }
}

/// Gives the frame back once: the task continues at the next pump.
pub fn yield_now() -> YieldNow {
    YieldNow(false)
}

/// The future of [`yield_now`].
#[derive(Debug)]
pub struct YieldNow(bool);

impl Future for YieldNow {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            return Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

