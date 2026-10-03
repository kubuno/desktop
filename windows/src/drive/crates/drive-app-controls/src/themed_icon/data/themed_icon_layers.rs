//! Layer collection of a `Layered`-type `ThemedIcon`
//! (mirrors Data/ThemedIconLayers.cs).
//!
//! C# side: `sealed partial class ThemedIconLayers : List<ThemedIconLayer>`.
//! Thin mirror: a simple type alias over a `Vec` of layers. The ported
//! counterpart of `ThemedIconLayer` is the D2D renderer's `IconLayer`.

use crate::themed_icon::IconLayer;

/// A collection of layers for the `Layered` icon type.
pub type ThemedIconLayers = Vec<IconLayer>;
