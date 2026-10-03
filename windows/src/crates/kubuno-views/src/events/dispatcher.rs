//! `Control.Invoke` / `BeginInvoke` (`vskubuno/docs/EVENTS.md` §6, work package EVT-6).
//!
//! The view model lives on the UI thread and is `!Send` in practice (handlers borrow it
//! mutably while a frame runs). A worker thread that wants to change it posts a closure
//! instead: [`UiDispatcher::begin_invoke`] queues it and wakes the host window
//! (`kubuno_controls::host::UiWaker`, a `PostMessage`), and the runtime runs the queue at
//! the start of its next frame, with `&mut` of the view model, in posting order.
//!
//! ```
//! use kubuno_views::events::{DispatchError, UiDispatcher};
//! # struct Vm { status: String }
//! # fn demo(dispatcher: UiDispatcher<Vm>) {
//! // On a worker thread:
//! std::thread::spawn(move || {
//!     let rows = 42; // … slow work …
//!     dispatcher.begin_invoke(move |vm: &mut Vm| vm.status = format!("{rows} rows"));
//!     // Or wait for a typed result (blocks this worker thread until the UI thread ran it):
//!     let status: Result<String, DispatchError> = dispatcher.invoke(|vm: &mut Vm| vm.status.clone());
//! });
//! # }
//! ```
//!
//! | WinForms | Here |
//! |---|---|
//! | `control.BeginInvoke(d)` → `IAsyncResult` | [`UiDispatcher::begin_invoke`] → [`AsyncResult<R>`] |
//! | `control.EndInvoke(ar)` | [`AsyncResult::wait`] (or `.await` it: it is a `Future`) |
//! | `control.Invoke(d)` | [`UiDispatcher::invoke`] |
//! | `control.InvokeRequired` | [`UiDispatcher::invoke_required`] (`!is_ui_thread()`) |
//! | `ObjectDisposedException` after the form closed | [`DispatchError::Closed`] |
//!
//! **Deviations.** `invoke` from the UI thread itself does not run inline (the view model is
//! already mutably borrowed by whoever is running there): it returns
//! [`DispatchError::OnUiThread`] at once instead of deadlocking. Calling `invoke` from a worker
//! while the UI thread blocks waiting for that worker is a deadlock, as in WinForms — prefer
//! `begin_invoke`. After the view closed, queued closures are dropped (their [`AsyncResult`]s
//! report [`DispatchError::Closed`]) and new ones are refused the same way.

use std::any::Any;
use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};
use std::thread::ThreadId;
use std::time::Duration;

/// A closure posted for the UI thread, type-erased: it receives the view model the runtime
/// paints with (`None` when the frame is untyped, see [`DispatchError::WrongViewModel`]).
pub(crate) type Job = Box<dyn FnOnce(Option<&mut dyn Any>) + Send>;

/// Locks `m`, ignoring poisoning: every critical section in this module is a single queue or
/// state operation that cannot leave the data inconsistent, and a panicking UI closure must not
/// wedge every worker thread that posts afterwards.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The cross-thread half of a view's UI-thread services: the posted closures, the async
/// executor's ready list, and how to wake the host window. Shared (`Arc`) by the runtime,
/// every [`UiDispatcher`] and every async task's waker.
pub(crate) struct UiShared {
    ui_thread: ThreadId,
    jobs: Mutex<VecDeque<Job>>,
    ready: Mutex<Vec<u64>>,
    closed: AtomicBool,
    wake: Box<dyn Fn() + Send + Sync>,
}

impl UiShared {
    /// Services for the calling thread (the UI thread), waking the host with `wake`.
    pub(crate) fn new(wake: Box<dyn Fn() + Send + Sync>) -> Arc<Self> {
        Arc::new(Self {
            ui_thread: std::thread::current().id(),
            jobs: Mutex::new(VecDeque::new()),
            ready: Mutex::new(Vec::new()),
            closed: AtomicBool::new(false),
            wake,
        })
    }

    pub(crate) fn is_ui_thread(&self) -> bool {
        std::thread::current().id() == self.ui_thread
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Asks the host for a frame.
    pub(crate) fn wake(&self) {
        (self.wake)();
    }

    /// Queues `job` and wakes the host; hands it back when the view is closed.
    fn post(&self, job: Job) -> Result<(), Job> {
        {
            let mut jobs = lock(&self.jobs);
            // Checked under the lock `close` drains with: a job is either run or dropped by
            // `close`, never stranded in the queue.
            if self.is_closed() {
                return Err(job);
            }
            jobs.push_back(job);
        }
        self.wake();
        Ok(())
    }

    /// Every posted closure, in posting order (the queue is left empty).
    pub(crate) fn take_jobs(&self) -> VecDeque<Job> {
        std::mem::take(&mut *lock(&self.jobs))
    }

    /// Marks an async task ready to be polled (its waker), and wakes the host.
    pub(crate) fn mark_ready(&self, task: u64) {
        {
            let mut ready = lock(&self.ready);
            if !ready.contains(&task) {
                ready.push(task);
            }
        }
        self.wake();
    }

    /// The tasks woken since the last call, in waking order.
    pub(crate) fn take_ready(&self) -> Vec<u64> {
        std::mem::take(&mut *lock(&self.ready))
    }

    /// Refuses new closures and drops the queued ones (their results report
    /// [`DispatchError::Closed`]).
    pub(crate) fn close(&self) {
        let dropped = {
            let mut jobs = lock(&self.jobs);
            self.closed.store(true, Ordering::Release);
            std::mem::take(&mut *jobs)
        };
        if !dropped.is_empty() {
            tracing::debug!("view closed: {} posted UI closure(s) dropped", dropped.len());
        }
        // Dropped outside the lock: a closure's captures may run arbitrary `Drop` code.
        drop(dropped);
        lock(&self.ready).clear();
    }
}

/// Why a posted closure did not produce its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchError {
    /// The view closed (or its runtime was dropped) before the closure ran: it never will.
    Closed,
    /// `invoke` was called on the UI thread, which would wait for itself (see the module doc).
    OnUiThread,
    /// The runtime painted this frame with another view model type than the dispatcher's, or
    /// untyped (`Runtime::frame`): paint with `Runtime::frame_typed` and take the dispatcher
    /// for that view model's type.
    WrongViewModel,
    /// [`AsyncResult::wait_timeout`] gave up; the closure may still run later.
    Timeout,
}

impl fmt::Display for DispatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Closed => "the view closed before the UI closure ran",
            Self::OnUiThread => "invoke called on the UI thread (it would wait for itself)",
            Self::WrongViewModel => "the UI closure expects another view model type than the one the view is painted with",
            Self::Timeout => "timed out waiting for the UI thread",
        })
    }
}

impl std::error::Error for DispatchError {}

/// A handle that posts closures to one view's UI thread, from any thread (`Send + Sync +
/// Clone`). Get it from `Runtime::dispatcher::<V>()` (or `UiHandle::dispatcher` in an async
/// handler), where `V` is the view model type the view is painted with (`Runtime::frame_typed`).
pub struct UiDispatcher<V: 'static> {
    shared: Arc<UiShared>,
    // `fn(&mut V)`: the dispatcher never holds a `V`, so it is `Send + Sync` whatever `V` is.
    _vm: PhantomData<fn(&mut V)>,
}

impl<V: 'static> Clone for UiDispatcher<V> {
    fn clone(&self) -> Self {
        Self { shared: self.shared.clone(), _vm: PhantomData }
    }
}

impl<V: 'static> fmt::Debug for UiDispatcher<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UiDispatcher").field("view_model", &std::any::type_name::<V>()).field("closed", &self.is_closed()).finish()
    }
}

impl<V: 'static> UiDispatcher<V> {
    pub(crate) fn new(shared: Arc<UiShared>) -> Self {
        Self { shared, _vm: PhantomData }
    }

    /// Queues `f` to run on the UI thread with the view model, at the start of the next frame
    /// (closures run in posting order), and wakes the window. Never blocks. The returned
    /// [`AsyncResult`] carries `f`'s result; drop it when you do not need it.
    pub fn begin_invoke<R, F>(&self, f: F) -> AsyncResult<R>
    where
        R: Send + 'static,
        F: FnOnce(&mut V) -> R + Send + 'static,
    {
        let (result, completer) = oneshot();
        let job: Job = Box::new(move |vm: Option<&mut dyn Any>| match vm.and_then(|vm| vm.downcast_mut::<V>()) {
            Some(vm) => completer.complete(Ok(f(vm))),
            None => {
                tracing::error!(
                    "a UiDispatcher<{}> closure was dropped: the view is painted with another view model type (use Runtime::frame_typed with that type)",
                    std::any::type_name::<V>()
                );
                completer.complete(Err(DispatchError::WrongViewModel));
            }
        });
        if self.shared.post(job).is_err() {
            // The job (and its completer) was dropped: the result already reports `Closed`.
            tracing::debug!("begin_invoke after the view closed: dropped");
        }
        result
    }

    /// Runs `f` on the UI thread with the view model and waits for its result (WinForms
    /// `Invoke`). Blocks the calling worker thread until the UI thread has run it; returns
    /// [`DispatchError::OnUiThread`] at once when called on the UI thread, and
    /// [`DispatchError::Closed`] if the view closes first.
    pub fn invoke<R, F>(&self, f: F) -> Result<R, DispatchError>
    where
        R: Send + 'static,
        F: FnOnce(&mut V) -> R + Send + 'static,
    {
        if self.is_ui_thread() {
            tracing::warn!("UiDispatcher::invoke called on the UI thread: use the view model you already have there");
            return Err(DispatchError::OnUiThread);
        }
        self.begin_invoke(f).wait()
    }

    /// Whether the calling thread is the view's UI thread.
    pub fn is_ui_thread(&self) -> bool {
        self.shared.is_ui_thread()
    }

    /// WinForms' `InvokeRequired`: `true` on any thread but the UI thread.
    pub fn invoke_required(&self) -> bool {
        !self.is_ui_thread()
    }

    /// Whether the view has closed: posted closures are dropped from then on.
    pub fn is_closed(&self) -> bool {
        self.shared.is_closed()
    }
}

// ── The result channel (a oneshot) ──────────────────────────────────────────

enum Slot<R> {
    Pending(Option<Waker>),
    Done(Result<R, DispatchError>),
    Taken,
}

struct Channel<R> {
    slot: Mutex<Slot<R>>,
    ready: Condvar,
}

/// The result of a [`UiDispatcher::begin_invoke`] (WinForms' `IAsyncResult` + `EndInvoke`):
/// wait for it on a worker thread ([`AsyncResult::wait`], [`AsyncResult::wait_timeout`]),
/// poll it ([`AsyncResult::try_take`]), or `.await` it from async code.
pub struct AsyncResult<R> {
    channel: Arc<Channel<R>>,
}

impl<R> fmt::Debug for AsyncResult<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncResult").field("completed", &self.is_completed()).finish()
    }
}

/// The UI-thread side of an [`AsyncResult`]: completes it, or, dropped unused (the job was
/// dropped at close), completes it with [`DispatchError::Closed`].
struct Completer<R> {
    channel: Option<Arc<Channel<R>>>,
}

fn oneshot<R>() -> (AsyncResult<R>, Completer<R>) {
    let channel = Arc::new(Channel { slot: Mutex::new(Slot::Pending(None)), ready: Condvar::new() });
    (AsyncResult { channel: channel.clone() }, Completer { channel: Some(channel) })
}

impl<R> Completer<R> {
    fn complete(mut self, value: Result<R, DispatchError>) {
        if let Some(channel) = self.channel.take() {
            fill(&channel, value);
        }
    }
}

impl<R> Drop for Completer<R> {
    fn drop(&mut self) {
        if let Some(channel) = self.channel.take() {
            fill(&channel, Err(DispatchError::Closed));
        }
    }
}

fn fill<R>(channel: &Channel<R>, value: Result<R, DispatchError>) {
    let waker = {
        let mut slot = lock(&channel.slot);
        match std::mem::replace(&mut *slot, Slot::Done(value)) {
            Slot::Pending(waker) => waker,
            // Already filled or taken: keep what was there.
            other => {
                *slot = other;
                None
            }
        }
    };
    channel.ready.notify_all();
    if let Some(w) = waker {
        w.wake();
    }
}

impl<R> AsyncResult<R> {
    /// Whether the closure has run (or will never run).
    pub fn is_completed(&self) -> bool {
        !matches!(*lock(&self.channel.slot), Slot::Pending(_))
    }

    /// The result when the closure has run, without blocking; `None` while it is pending (or
    /// after the result was already taken).
    pub fn try_take(&self) -> Option<Result<R, DispatchError>> {
        let mut slot = lock(&self.channel.slot);
        match std::mem::replace(&mut *slot, Slot::Taken) {
            Slot::Done(r) => Some(r),
            other => {
                *slot = other;
                None
            }
        }
    }

    /// Blocks until the UI thread has run the closure (WinForms `EndInvoke`). Never call it on
    /// the UI thread for a closure it has not run yet: it would wait for itself.
    pub fn wait(self) -> Result<R, DispatchError> {
        let mut slot = lock(&self.channel.slot);
        loop {
            match std::mem::replace(&mut *slot, Slot::Taken) {
                Slot::Done(r) => return r,
                Slot::Taken => return Err(DispatchError::Closed),
                pending @ Slot::Pending(_) => {
                    *slot = pending;
                    slot = self.channel.ready.wait(slot).unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            }
        }
    }

    /// [`AsyncResult::wait`] for at most `timeout`: [`DispatchError::Timeout`] when the UI thread
    /// has not run the closure by then (it may still run it later).
    pub fn wait_timeout(self, timeout: Duration) -> Result<R, DispatchError> {
        let deadline = std::time::Instant::now() + timeout;
        let mut slot = lock(&self.channel.slot);
        loop {
            match std::mem::replace(&mut *slot, Slot::Taken) {
                Slot::Done(r) => return r,
                Slot::Taken => return Err(DispatchError::Closed),
                pending @ Slot::Pending(_) => {
                    *slot = pending;
                    let now = std::time::Instant::now();
                    if now >= deadline {
                        return Err(DispatchError::Timeout);
                    }
                    slot = match self.channel.ready.wait_timeout(slot, deadline - now) {
                        Ok((guard, _)) => guard,
                        Err(poisoned) => poisoned.into_inner().0,
                    };
                }
            }
        }
    }
}

impl<R> Future for AsyncResult<R> {
    type Output = Result<R, DispatchError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut slot = lock(&self.channel.slot);
        match std::mem::replace(&mut *slot, Slot::Taken) {
            Slot::Done(r) => Poll::Ready(r),
            Slot::Taken => Poll::Ready(Err(DispatchError::Closed)),
            Slot::Pending(_) => {
                *slot = Slot::Pending(Some(cx.waker().clone()));
                Poll::Pending
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct Vm {
        log: Vec<String>,
    }

    fn shared() -> (Arc<UiShared>, Arc<AtomicUsize>) {
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = wakes.clone();
        (UiShared::new(Box::new(move || {
            w.fetch_add(1, Ordering::SeqCst);
        })), wakes)
    }

    /// What the runtime does at the start of a frame.
    fn run_jobs(shared: &UiShared, vm: &mut Vm) {
        for job in shared.take_jobs() {
            job(Some(vm));
        }
    }

    #[test]
    fn closures_posted_from_threads_run_on_the_ui_thread_in_order_with_results() {
        let (shared, wakes) = shared();
        let dispatcher: UiDispatcher<Vm> = UiDispatcher::new(shared.clone());
        assert!(dispatcher.is_ui_thread() && !dispatcher.invoke_required());
        let worker = {
            let d = dispatcher.clone();
            std::thread::spawn(move || {
                assert!(d.invoke_required());
                let results: Vec<AsyncResult<usize>> = (0..3)
                    .map(|i| {
                        d.begin_invoke(move |vm: &mut Vm| {
                            vm.log.push(format!("job {i}"));
                            vm.log.len()
                        })
                    })
                    .collect();
                results
            })
        };
        let results = worker.join().expect("worker");
        assert_eq!(wakes.load(Ordering::SeqCst), 3, "every post wakes the host");
        assert!(results.iter().all(|r| !r.is_completed()));
        let mut vm = Vm { log: Vec::new() };
        run_jobs(&shared, &mut vm);
        assert_eq!(vm.log, ["job 0", "job 1", "job 2"]);
        let values: Vec<_> = results.into_iter().map(AsyncResult::wait).collect();
        assert_eq!(values, [Ok(1), Ok(2), Ok(3)]);
    }

    #[test]
    fn invoke_blocks_the_worker_until_the_ui_thread_ran_it() {
        let (shared, _) = shared();
        let dispatcher: UiDispatcher<Vm> = UiDispatcher::new(shared.clone());
        let worker = std::thread::spawn(move || dispatcher.invoke(|vm: &mut Vm| format!("{} lines", vm.log.len())));
        let mut vm = Vm { log: vec!["a".into()] };
        // The UI thread keeps pumping frames until the worker's closure arrives.
        let mut spins = 0;
        while !worker.is_finished() && spins < 2000 {
            run_jobs(&shared, &mut vm);
            std::thread::sleep(Duration::from_millis(1));
            spins += 1;
        }
        assert_eq!(worker.join().expect("worker"), Ok("1 lines".to_string()));
    }

    #[test]
    fn invoke_on_the_ui_thread_refuses_instead_of_deadlocking() {
        let (shared, _) = shared();
        let dispatcher: UiDispatcher<Vm> = UiDispatcher::new(shared);
        assert_eq!(dispatcher.invoke(|_vm: &mut Vm| 1), Err(DispatchError::OnUiThread));
    }

    #[test]
    fn closing_drops_queued_closures_and_refuses_new_ones() {
        let (shared, _) = shared();
        let dispatcher: UiDispatcher<Vm> = UiDispatcher::new(shared.clone());
        let queued = dispatcher.begin_invoke(|vm: &mut Vm| vm.log.push("never".into()));
        let waiting = {
            let d = dispatcher.clone();
            std::thread::spawn(move || d.invoke(|_vm: &mut Vm| 7))
        };
        // Let the worker post (it then blocks); closing must release it.
        while lock(&shared.jobs).len() < 2 && !waiting.is_finished() {
            std::thread::sleep(Duration::from_millis(1));
        }
        shared.close();
        assert!(dispatcher.is_closed());
        assert_eq!(queued.wait(), Err(DispatchError::Closed));
        assert_eq!(waiting.join().expect("worker"), Err(DispatchError::Closed));
        let late = dispatcher.begin_invoke(|_vm: &mut Vm| 1);
        assert_eq!(late.try_take(), Some(Err(DispatchError::Closed)));
    }

    #[test]
    fn a_closure_for_another_view_model_type_is_reported() {
        let (shared, _) = shared();
        let dispatcher: UiDispatcher<Vm> = UiDispatcher::new(shared.clone());
        let r = dispatcher.begin_invoke(|_vm: &mut Vm| 1);
        for job in shared.take_jobs() {
            job(Some(&mut 5_u32));
        }
        assert_eq!(r.wait(), Err(DispatchError::WrongViewModel));
        let r = dispatcher.begin_invoke(|_vm: &mut Vm| 1);
        for job in shared.take_jobs() {
            job(None);
        }
        assert_eq!(r.try_take(), Some(Err(DispatchError::WrongViewModel)));
    }

    #[test]
    fn wait_timeout_gives_up_and_the_result_can_be_awaited() {
        let (shared, _) = shared();
        let dispatcher: UiDispatcher<Vm> = UiDispatcher::new(shared.clone());
        let r = dispatcher.begin_invoke(|_vm: &mut Vm| 1);
        assert_eq!(r.wait_timeout(Duration::from_millis(5)), Err(DispatchError::Timeout));

        // As a future: pending, then woken and ready once the UI thread ran the closure.
        struct Flag(AtomicBool);
        impl std::task::Wake for Flag {
            fn wake(self: Arc<Self>) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let flag = Arc::new(Flag(AtomicBool::new(false)));
        let waker = Waker::from(flag.clone());
        let mut cx = Context::from_waker(&waker);
        let mut fut = dispatcher.begin_invoke(|vm: &mut Vm| vm.log.len() + 10);
        assert!(Pin::new(&mut fut).poll(&mut cx).is_pending());
        let mut vm = Vm { log: Vec::new() };
        run_jobs(&shared, &mut vm);
        assert!(flag.0.load(Ordering::SeqCst), "completing wakes the awaiting task");
        assert_eq!(Pin::new(&mut fut).poll(&mut cx), Poll::Ready(Ok(10)));
    }
}
