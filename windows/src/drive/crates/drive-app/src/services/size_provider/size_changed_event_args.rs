//! SizeChangedEventArgs (mirror of `SizeChangedEventArgs.cs`).
//!
//! Reduced here to a triple: the `None` state of `SizeChangedValueState` is skipped
//! (the port only distinguishes "intermediate" / "final").

/// (path, size, final) — `SizeChangedEventArgs` without the `None` state.
pub(crate) type SizeResult = (String, u64, bool);
