//! A `ThemedIcon`'s icon types (mirrors Data/ThemedIconTypes.cs).

/// A `ThemedIcon`'s `IconTypes`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemedIconTypes {
    /// The icon is of type `Outline`.
    Outline,
    /// The icon is of type `Layered`.
    Layered,
}
