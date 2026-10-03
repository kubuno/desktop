//! `Toolbar` button (mirrors `Files.App.Controls/Toolbar/ToolbarButton/ToolbarButton.cs`).
//!
//! The WinUI `Button` class is not portable; this file houses the port's
//! button rendering types: the flattened visual variant [`ButtonVisual`],
//! the laid-out button [`ToolbarButton`], and the chevron glyph for menu
//! buttons.

use crate::geometry::Rect;

/// Chevron glyph `▾` for dropdown menu buttons (`ChevronDown`, Segoe Fluent).
pub(crate) const GLYPH_CHEVRON_DOWN: &str = "\u{E70D}";

/// A button FLATTENED into primitives: `Hot` (the app's type) is already
/// resolved by the builder into a purely visual variant. The control only
/// knows about that.
pub enum ButtonVisual {
    /// `AppBarSeparator`: 1 DIP vertical line, centered, reduced height.
    Separator,
    /// "New ▾": `NewItem` icon + label + chevron (`CmdNew`).
    NewButton { label: String, hot: bool },
    /// Icon-only MONOCHROME filling the whole rect — left-side block.
    Icon { name: &'static str, hot: bool, enabled: bool },
    /// Multi-layer icon (fg,fg) on the left + label — Recycle Bin context.
    IconLabel { name: &'static str, label: String, hot: bool, enabled: bool },
    /// Icon filling the whole rect, accent-tinted on the bottom layer (`Filter`).
    IconAccent { name: &'static str, hot: bool },
    /// Icon on the left + chevron ▾ on the right. `accent_layer` = accent layer.
    IconChevron { name: &'static str, accent_layer: bool, hot: bool },
    /// Toggle (`InfoPane`/`Shelf`): accent-filled + white glyph if `on`.
    Toggle { name: &'static str, on: bool, hot: bool },
}

/// A laid-out button: its rectangle (hover area) and its flattened visual.
pub struct ToolbarButton {
    pub rect: Rect,
    pub visual: ButtonVisual,
}
