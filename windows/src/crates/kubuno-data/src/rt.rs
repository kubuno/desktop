//! The data runtime: a small private Tokio runtime on which every database operation runs, so the
//! UI thread never waits on I/O (`vskubuno/docs/DATA.md` §1). The UI side gets a [`DataTask`], a
//! future it awaits from the EVT-6 executor (an async handler, `spawn_local`): its waker is
//! executor-agnostic, so the task resumes on the UI thread when the operation completes.

use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;
use std::task::{Context, Poll};

use crate::error::DataError;

static RUNTIME: OnceLock<Result<tokio::runtime::Runtime, String>> = OnceLock::new();

/// The shared data runtime (created on first use: two worker threads named `kubuno-data`).
pub fn runtime() -> Result<&'static tokio::runtime::Runtime, DataError> {
    let rt = RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("kubuno-data")
            .enable_all()
            .build()
            .map_err(|e| e.to_string())
    });
    match rt {
        Ok(rt) => Ok(rt),
        Err(e) => Err(crate::error::logged("data runtime", DataError::Config(format!("cannot start the data runtime: {e}")))),
    }
}

/// Runs `future` on the data runtime and returns the handle the UI awaits.
pub fn spawn<T, F>(future: F) -> DataTask<T>
where
    T: Send + 'static,
    F: Future<Output = Result<T, DataError>> + Send + 'static,
{
    match runtime() {
        Ok(rt) => DataTask { inner: TaskInner::Running(rt.spawn(future)) },
        Err(e) => DataTask::failed(e),
    }
}

/// Blocks the current thread until `future` completes, driving it on the data runtime. For tests,
/// tools and `main` before the window opens — never from the UI thread while it paints, and never
/// from inside a data operation.
pub fn block_on<F: Future>(future: F) -> Result<F::Output, DataError> {
    Ok(runtime()?.block_on(future))
}

enum TaskInner<T> {
    Running(tokio::task::JoinHandle<Result<T, DataError>>),
    Failed(Option<DataError>),
}

/// A data operation running on the data runtime. Awaiting it gives its result; [`DataTask::cancel`]
/// aborts it at its next `.await` (a transaction that did not commit is rolled back when its
/// connection returns to the pool). Dropping it detaches the operation (it still completes).
#[must_use = "a DataTask does nothing visible unless it is awaited"]
pub struct DataTask<T> {
    inner: TaskInner<T>,
}

impl<T> DataTask<T> {
    /// A task that already failed (a configuration error found before any I/O).
    pub fn failed(error: DataError) -> Self {
        Self { inner: TaskInner::Failed(Some(error)) }
    }

    /// Aborts the operation; awaiting it then gives [`DataError::Cancelled`].
    pub fn cancel(&self) {
        if let TaskInner::Running(h) = &self.inner {
            h.abort();
        }
    }

    /// Whether the operation has completed (successfully or not).
    pub fn is_finished(&self) -> bool {
        match &self.inner {
            TaskInner::Running(h) => h.is_finished(),
            TaskInner::Failed(_) => true,
        }
    }

    /// A cancellation token of the operation (`Send + Sync`, cloneable): a Cancel button's handler
    /// or another thread cancels it while the UI awaits the task (DATA-3).
    pub fn canceller(&self) -> Canceller {
        match &self.inner {
            TaskInner::Running(h) => Canceller(Some(h.abort_handle())),
            TaskInner::Failed(_) => Canceller(None),
        }
    }
}

/// Cancels a running data operation (see [`DataTask::canceller`]). A transaction that did not
/// commit is rolled back.
#[derive(Clone, Debug, Default)]
pub struct Canceller(Option<tokio::task::AbortHandle>);

impl Canceller {
    /// Cancels the operation: awaiting its task gives [`DataError::Cancelled`].
    pub fn cancel(&self) {
        if let Some(h) = &self.0 {
            h.abort();
        }
    }

    /// Whether the operation ended (completed, failed or cancelled).
    pub fn is_finished(&self) -> bool {
        self.0.as_ref().is_none_or(|h| h.is_finished())
    }
}

/// How far a long operation got — the rows a fill has read so far — shared between the data
/// runtime (which counts) and the UI (which shows it; `customers.RowsRead`).
#[derive(Clone, Debug, Default)]
pub struct Progress(std::sync::Arc<std::sync::atomic::AtomicU64>);

impl Progress {
    pub fn new() -> Self {
        Self::default()
    }

    /// The rows read so far.
    pub fn rows(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn add(&self, n: u64) {
        self.0.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn reset(&self) {
        self.0.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

impl<T> Future for DataTask<T> {
    type Output = Result<T, DataError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut self.inner {
            TaskInner::Failed(e) => Poll::Ready(Err(e.take().unwrap_or(DataError::Cancelled))),
            TaskInner::Running(h) => match Pin::new(h).poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(result)) => Poll::Ready(result),
                Poll::Ready(Err(join)) if join.is_cancelled() => Poll::Ready(Err(DataError::Cancelled)),
                Poll::Ready(Err(join)) => {
                    let err = DataError::Database { message: format!("the data operation panicked: {join}"), code: None };
                    Poll::Ready(Err(crate::error::logged("data task", err)))
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_completes_and_can_be_awaited_from_another_executor() {
        let task = spawn(async { Ok::<_, DataError>(21 * 2) });
        // Awaited from a thread that is not a Tokio worker (like the UI thread's executor).
        let out = std::thread::spawn(move || block_on(task)).join().expect("thread");
        assert_eq!(out, Ok(Ok(42)));
    }

    #[test]
    fn cancel_gives_cancelled() {
        let task = spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            Ok::<_, DataError>(())
        });
        task.cancel();
        assert_eq!(block_on(task), Ok(Err(DataError::Cancelled)));
        assert_eq!(block_on(DataTask::<()>::failed(DataError::Closed)), Ok(Err(DataError::Closed)));
    }
}
