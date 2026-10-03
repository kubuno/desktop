//! A `ThemedIcon`'s layer type (mirrors Data/ThemedIconLayerType.cs).
//!
//! C# names this enum `ThemedIconLayerType { Base, Alt, Accent,
//! AccentContrast }`. The port exposes it under the historical name
//! `LayerRole`; the name is kept as-is to avoid breaking the public API
//! (`kubuno_drive_desktop_app_controls::LayerRole`).

/// The color role of a `ThemedIconLayer` (the `Files.App.Controls`
/// multi-layer system). `Base` takes the foreground; `Alt` takes the
/// INVERSE color at 40% (the cutouts/notches — `ThemedIconAltColor`);
/// `Accent` takes the accent color; `AccentContrast` the color that
/// contrasts with the accent (white on the blue badge, e.g.).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LayerRole {
    Base,
    Alt,
    Accent,
    AccentContrast,
}
