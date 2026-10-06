//! `ToolbarItemTypes` (mirrors
//! `Files.App.Controls/Toolbar/ToolbarItem/ToolbarItemTypes.cs`).

/// `Toolbar` item type. Port convention: C# plural → Rust singular
/// (`ToolbarItemTypes` → `ToolbarItemType`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolbarItemType {
    Button,
    /// Default type.
    Content,
    FlyoutButton,
    /// Possible Radio Button support via `GroupName`.
    RadioButton,
    Separator,
    SplitButton,
    ToggleButton,
}
