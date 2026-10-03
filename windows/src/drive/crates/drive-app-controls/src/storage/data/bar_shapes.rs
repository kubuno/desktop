//! `BarShapes` (mirrors `Files.App.Controls/Storage/Data/BarShapes.cs`).

/// Shape of a [`crate::storage::storage_bar`]'s ends. Determines the corner
/// radius in `UpdateContainerHeightsAndCorners`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BarShapes {
    /// Round bars (default state) — corners = height / 2.
    #[default]
    Round,
    /// Soft bars — corners = height / 4.
    Soft,
    /// Flat bars — corners = 0.
    Flat,
}
