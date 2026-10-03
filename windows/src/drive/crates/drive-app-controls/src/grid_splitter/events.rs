//! `GridSplitter` input constants (mirror of
//! `Files.App.Controls/GridSplitter/GridSplitter.Events.cs`).
//!
//! Only the pure values are ported: the Segoe MDL2 handle glyphs and
//! `OnKeyDown`'s keyboard step. The wiring (`OnKeyDown`/`OnManipulation*`) and
//! the fixed/star algorithm of `HorizontalMove`/`VerticalMove` manipulate
//! WinUI `ColumnDefinition`/`RowDefinition`/`GridLength` and remain unported.

/// Vertical handle glyph (columns) in Segoe MDL2 Assets (`\xE784`).
pub const GRIPPER_BAR_VERTICAL: char = '\u{E784}';

/// Horizontal handle glyph (rows) in Segoe MDL2 Assets (`\xE76F`).
pub const GRIPPER_BAR_HORIZONTAL: char = '\u{E76F}';

/// Display font for the handle.
pub const GRIPPER_DISPLAY_FONT: &str = "Segoe MDL2 Assets";

/// Keyboard resize step (`OnKeyDown`, arrows).
pub const KEY_STEP: i32 = 1;

/// Keyboard resize step with Ctrl held down.
pub const KEY_STEP_CTRL: i32 = 5;
