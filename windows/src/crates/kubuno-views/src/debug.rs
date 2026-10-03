//! Debugger helpers for application code (`vskubuno/docs/DEBUGGING.md`).
//!
//! A Rust `Result` error is a value, not an exception: no debugger can break "where an `Err` was
//! returned" generically (`?` is an ordinary early return, and nothing marks the value that becomes the
//! error). These helpers are the manual equivalent of .NET's `Debugger.Break()` for the places you care
//! about - harmless in shipped code, since they do nothing without a debugger attached.
//!
//! ```no_run
//! use kubuno_views::debug::BreakOnError;
//!
//! fn load(path: &str) -> std::io::Result<String> {
//!     // Stops in the debugger on this line when the read fails, then returns the error as usual.
//!     let text = std::fs::read_to_string(path).break_on_err()?;
//!     Ok(text)
//! }
//! ```

/// Stops in the debugger when one is attached (`DebugBreak`), like `Debugger.Break()`; does nothing
/// otherwise.
pub fn debug_break() {
    kubuno_ui::diagnostics::debug_break();
}

/// Whether a debugger is attached to the process right now, like `Debugger.IsAttached`.
pub fn is_debugger_attached() -> bool {
    kubuno_ui::diagnostics::is_debugger_attached()
}

/// Returns `result` unchanged, after breaking into the debugger (when one is attached) if it is an
/// `Err`: `debug_break_on_error(parse(text))?`.
#[inline(never)]
pub fn debug_break_on_error<T, E>(result: Result<T, E>) -> Result<T, E> {
    if result.is_err() {
        debug_break();
    }
    result
}

/// [`debug_break_on_error`] as a method, for `?` chains: `parse(text).break_on_err()?`. Also on
/// `Option` (breaks on `None`).
pub trait BreakOnError: Sized {
    /// Breaks into the attached debugger when `self` is an error (or `None`), then returns `self`.
    fn break_on_err(self) -> Self;
}

impl<T, E> BreakOnError for Result<T, E> {
    #[inline(never)]
    fn break_on_err(self) -> Self {
        debug_break_on_error(self)
    }
}

impl<T> BreakOnError for Option<T> {
    #[inline(never)]
    fn break_on_err(self) -> Self {
        if self.is_none() {
            debug_break();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_pass_through_unchanged() {
        // No debugger in a test run: nothing breaks, and the values are returned as they were.
        assert_eq!(debug_break_on_error::<i32, &str>(Ok(3)), Ok(3));
        assert_eq!(debug_break_on_error::<i32, &str>(Err("no")), Err("no"));
        assert_eq!(Some(1).break_on_err(), Some(1));
        assert_eq!(None::<i32>.break_on_err(), None);
        assert_eq!("7".parse::<i32>().break_on_err(), Ok(7));
    }
}
