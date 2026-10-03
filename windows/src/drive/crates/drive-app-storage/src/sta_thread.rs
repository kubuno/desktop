//! Port of `Windows/Managers/STATask.cs`.
//!
//! Many shell APIs (context menus, drag & drop, thumbnails, `IFileOperation`
//! with UI, …) must be called from a single-threaded apartment. These helpers
//! schedule a closure on a dedicated background thread initialized with
//! `OleInitialize` (which implies `CoInitializeEx(COINIT_APARTMENTTHREADED)`),
//! mirroring `STATask.Run`.

use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};

/// Runs `f` on a new background STA thread and resolves once it completes.
///
/// Mirrors `STATask.Run<T>(Func<T>, ILogger?)`: a panic inside `f` is caught,
/// logged as a warning and surfaced as `None` (the C# version swallows the
/// exception and completes the task with `default`).
pub async fn run<T, F>(f: F) -> Option<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();

    std::thread::Builder::new()
        .name("files-sta-worker".into())
        .spawn(move || {
            // SAFETY: paired with `OleUninitialize` below; this thread is brand
            // new, so it cannot already be in an MTA.
            let init = unsafe { OleInitialize(None) };

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            match result {
                Ok(value) => {
                    let _ = tx.send(Some(value));
                }
                Err(_) => {
                    tracing::warn!("a panic occurred during the execution within the STA thread");
                    let _ = tx.send(None);
                }
            }

            if init.is_ok() {
                // SAFETY: balances the successful `OleInitialize` above.
                unsafe { OleUninitialize() };
            }
        })
        .expect("failed to spawn STA thread");

    rx.await.ok().flatten()
}

/// Blocking variant of [`run`], for callers that are not on an async runtime.
pub fn run_blocking<T, F>(f: F) -> Option<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let handle = std::thread::Builder::new()
        .name("files-sta-worker".into())
        .spawn(move || {
            // SAFETY: see `run`.
            let init = unsafe { OleInitialize(None) };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok();
            if init.is_ok() {
                // SAFETY: balances the successful `OleInitialize` above.
                unsafe { OleUninitialize() };
            }
            result
        })
        .expect("failed to spawn STA thread");

    match handle.join() {
        Ok(value) => value,
        Err(_) => {
            tracing::warn!("a panic occurred during the execution within the STA thread");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_blocking_returns_value() {
        assert_eq!(run_blocking(|| 21 * 2), Some(42));
    }

    #[test]
    fn run_blocking_swallows_panics() {
        assert_eq!(run_blocking::<i32, _>(|| panic!("boom")), None);
    }

    #[tokio::test]
    async fn run_returns_value() {
        assert_eq!(run(|| "hello".to_string()).await.as_deref(), Some("hello"));
    }
}
