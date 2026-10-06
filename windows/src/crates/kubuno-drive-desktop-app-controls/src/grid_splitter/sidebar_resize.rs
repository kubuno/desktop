//! Sidebar pane resizing — **port-only**, no C# counterpart in the
//! `GridSplitter` folder.
//!
//! These helpers do NOT come from `GridSplitter` but from `MainPage`
//! (`container - sidebar - ContentColumn.MinWidth`, floor 100) and from
//! `SidebarView.UpdateDisplayModeForPaneWidth` / `COMPACT_MAX_WIDTH`. They're
//! placed here (alongside the `resize` core) for lack of another anchor point
//! in this module; relocating them to mirrors of `MainPage`/`SidebarView`
//! would be more faithful but is out of this folder's scope.

/// The info pane's dynamic `max`: the container's free space once the content
/// column is reserved, never below `floor`. (`MainPage`:
/// `container - sidebar - ContentColumn.MinWidth`, floor 100.)
pub fn available_max(container: f32, reserved: f32, floor: f32) -> f32 {
    (container - reserved).max(floor)
}

/// Result of a sidebar resize: either a clamped expanded width, or switching
/// to the Compact rail when the target falls below the threshold
/// (`UpdateDisplayModeForPaneWidth` / `COMPACT_MAX_WIDTH`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PaneResize {
    Compact,
    Expanded(f32),
}

/// Sidebar threshold decision + clamp. Below `compact_below`, switch to
/// Compact (like the hamburger); otherwise width clamped to
/// `[compact_below, max]` — the expanded mode's lower bound is the threshold
/// itself, faithful to the port.
pub fn resolve_sidebar(target: f32, compact_below: f32, max: f32) -> PaneResize {
    if target < compact_below {
        PaneResize::Compact
    } else {
        PaneResize::Expanded(target.clamp(compact_below, max))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_pane_max_never_below_floor() {
        assert_eq!(available_max(1200.0, 380.0, 100.0), 820.0);
        assert_eq!(available_max(300.0, 380.0, 100.0), 100.0);
    }

    #[test]
    fn sidebar_threshold() {
        assert_eq!(resolve_sidebar(199.0, 200.0, 500.0), PaneResize::Compact);
        assert_eq!(resolve_sidebar(240.0, 200.0, 500.0), PaneResize::Expanded(240.0));
        assert_eq!(resolve_sidebar(9999.0, 200.0, 500.0), PaneResize::Expanded(500.0));
    }
}
