//! `OverflowBehaviors` (mirrors `Files.App.Controls/Toolbar/Primitives/OverflowBehaviors.cs`).

/// Per-item overflow rule for the `Toolbar`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverflowBehavior {
    /// Goes to the menu if space is lacking.
    Auto,
    /// Always in the overflow menu.
    Always,
    /// Never overflows.
    Never,
}
