//! A `ThemedIcon`'s color types (mirrors Data/ThemedIconColorType.cs).
//!
//! Sets the visual state used to pick the brushes matching the system's
//! signal colors.

/// A `ThemedIcon`'s `IconColorTypes`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemedIconColorType {
    None,
    /// `Normal` color type. Default value.
    Normal,
    /// `Critical` color type.
    Critical,
    /// `Caution` color type.
    Caution,
    /// `Success` color type.
    Success,
    /// `Neutral` color type.
    Neutral,
    /// `Accent` color type.
    Accent,
    /// `Custom` color type. Used with the `IconColor` and `Foreground`
    /// brushes.
    Custom,
}
