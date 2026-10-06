//! `GridSplitter` enums (mirror of
//! `Files.App.Controls/GridSplitter/GridSplitter.Data.cs`).
//!
//! 100% portable content: four pure enums, no WinUI dependency.

/// Indicates whether the `GridSplitter` resizes columns or rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GridResizeDirection {
    /// Determines whether to resize rows or columns based on alignment and
    /// the width/height ratio.
    #[default]
    Auto,
    /// Resizes columns by dragging the splitter.
    Columns,
    /// Resizes rows by dragging the splitter.
    Rows,
}

/// Indicates which columns or rows the `GridSplitter` resizes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GridResizeBehavior {
    /// Determines the columns/rows to resize based on alignment.
    #[default]
    BasedOnAlignment,
    /// Resizes the current and next column/row.
    CurrentAndNext,
    /// Resizes the previous and current column/row.
    PreviousAndCurrent,
    /// Resizes the previous and next column/row.
    PreviousAndNext,
}

/// Supported handle cursor types.
///
/// `Default = -1` (explicit discriminant); the following values map to
/// `InputSystemCursorShape` in `GripperHoverWrapper` (not ported).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GripperCursorType {
    /// Changes the cursor based on the splitter's direction.
    #[default]
    Default = -1,
    /// Standard arrow cursor.
    Arrow = 0,
    /// Standard cross cursor.
    Cross,
    /// Standard custom cursor.
    Custom,
    /// Standard hand cursor.
    Hand,
    /// Standard help cursor.
    Help,
    /// Standard IBeam cursor.
    IBeam,
    /// Standard SizeAll cursor.
    SizeAll,
    /// Standard SizeNortheastSouthwest cursor.
    SizeNortheastSouthwest,
    /// Standard SizeNorthSouth cursor.
    SizeNorthSouth,
    /// Standard SizeNorthwestSoutheast cursor.
    SizeNorthwestSoutheast,
    /// Standard SizeWestEast cursor.
    SizeWestEast,
    /// Standard UniversalNo cursor.
    UniversalNo,
    /// Standard UpArrow cursor.
    UpArrow,
    /// Standard Wait cursor.
    Wait,
}

/// Window cursor behavior when hovering the grid splitter.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SplitterCursorBehavior {
    /// Updates the cursor when hovering the Grid Splitter.
    #[default]
    ChangeOnSplitterHover,
    /// Updates the cursor when hovering the Grid Splitter's handle.
    ChangeOnGripperHover,
}
