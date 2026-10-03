//! (mirror of BladeMode.cs) — enum for the `BladeView`'s display mode.

/// The blade mode.
///
/// Port of `Files.App.Controls/BladeView/BladeMode.cs`. The port currently
/// only handles `Normal`; `Fullscreen` is transcribed for fidelity to the C#.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BladeMode {
    /// Default mode : each blade will take the specified Width and Height
    #[default]
    Normal,

    /// Fullscreen mode : each blade will take the entire Width and Height of the
    /// UI control container (cf `BladeView`)
    Fullscreen,
}
