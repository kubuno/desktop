//! Storage control types (mirrors `Files.App.Controls/Storage/Data`).
//!
//! The C# `Data/` folder has one file per type; the same split is reproduced
//! here: [`bar_shapes`] (← `BarShapes.cs`), [`thickness_check`]
//! (← `ThicknessCheck.cs`). `StorageSeverity` below has NO source file —
//! Files decides the state via `VisualStateManager.GoToState(...)` with
//! strings ("Safe"/"Caution"/"Critical"/"Disabled"); it is reified here as an
//! enum for immediate rendering, so it lives in this `mod.rs` (glue), not in
//! a mirror file.

pub mod bar_shapes;
pub mod thickness_check;

pub use bar_shapes::BarShapes;
pub use thickness_check::ThicknessCheck;

/// Visual state of a storage bar/gauge based on fill level
/// (`StorageBar`/`StorageRing` `UpdateVisualState`). Reification by the port
/// — not a C# type (see the module note).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StorageSeverity {
    Safe,
    Caution,
    Critical,
}
