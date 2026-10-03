//! `ThicknessCheck` (mirrors `Files.App.Controls/Storage/Data/ThicknessCheck.cs`).

/// Compares the thickness of the two rings ([`crate::storage::storage_ring`])
/// or bars ([`crate::storage::storage_bar`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThicknessCheck {
    /// The "Value" thickness is greater.
    Value,
    /// The "Track" thickness is greater.
    Track,
    /// Both thicknesses are equal.
    Equal,
}
