//! `GridSplitter` arithmetic core (mirror of
//! `Files.App.Controls/GridSplitter/GridSplitter.Helper.cs`).
//!
//! Only the pure clamp of `SetColumnWidth`/`SetRowHeight` (+
//! `IsValidColumnWidth`/`IsValidRowHeight`) is portable and lives here under
//! the name [`resize`]. The `GetResizeDirection`/`GetResizeBehavior` routing
//! (dependent on WinUI's `HorizontalAlignment`/`VerticalAlignment`/
//! `ActualWidth`), the index math `GetTargetIndex`/`GetSiblingIndex`, and
//! `GetTargetedColumn`/`Row` (`Grid.GetColumn`) stay in the window layer and
//! are not ported.

/// `SetColumnWidth`/`SetRowHeight`: current width + delta, clamped to
/// `[min, max]`. `grip` is the handle's own width — the original forbids the
/// column from shrinking below the handle (`newWidth > ActualWidth` of the
/// splitter); pass `0.0` if this floor doesn't apply.
pub fn resize(current: f32, delta: f32, min: f32, max: f32, grip: f32) -> f32 {
    (current + delta).clamp(min.max(grip), max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_and_grip_floor() {
        assert_eq!(resize(300.0, 50.0, 180.0, 500.0, 4.0), 350.0);
        assert_eq!(resize(300.0, 1000.0, 180.0, 500.0, 4.0), 500.0);
        assert_eq!(resize(300.0, -1000.0, 180.0, 500.0, 4.0), 180.0);
        assert_eq!(resize(10.0, -100.0, 0.0, 500.0, 4.0), 4.0);
    }
}
