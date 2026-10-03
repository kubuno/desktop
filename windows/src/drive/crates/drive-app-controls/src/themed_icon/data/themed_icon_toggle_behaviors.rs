//! A `ThemedIcon`'s toggle behaviors
//! (mirrors Data/ThemedIconToggleBehaviors.cs).

/// A `ThemedIcon`'s toggle behaviors.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToggleBehaviors {
    /// `Auto`: the `ThemedIcon` listens to the owning control's states.
    Auto,
    /// `On`: always uses the `ThemedIcon`'s toggle state.
    On,
    /// `Off`: does not use the `ThemedIcon`'s toggle state.
    Off,
}
