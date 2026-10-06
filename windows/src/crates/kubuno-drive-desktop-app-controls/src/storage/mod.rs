//! Storage controls (mirrors `Files.App.Controls/Storage`).
//!
//! Mirror submodules: [`data`] (enums, ← `Data/`), [`storage_bar`]
//! (← `StorageBar/`), [`ring_shape`] (arc geometry primitive, ← `RingShape/`),
//! [`storage_ring`] (circular gauge, ← `StorageRing/`), and
//! [`storage_controls_helpers`] (← `StorageControlsHelpers.cs`): the PURE
//! interpolation/percentage/angle maths (`System.Math` only) shared by the
//! bar and the ring.
//!
//! DRAWING stays in `kubuno-drive-desktop` (immediate Direct2D); the CALCULATIONS live
//! here, transcribed identically from the C# for fidelity.
//!
//! This `mod.rs` is pure glue: it declares the submodules and RE-EXPORTS the
//! historically flat API (`storage::X`) so no caller breaks.

pub mod data;
pub mod ring_shape;
pub mod storage_bar;
pub mod storage_controls_helpers;
pub mod storage_ring;

pub use data::{BarShapes, StorageSeverity, ThicknessCheck};
pub use ring_shape::{RingArc, SweepDirection};
pub use storage_bar::{severity, severity_default, PERCENT_CAUTION, PERCENT_CRITICAL};
pub use storage_controls_helpers::{
    calculate_interpolated_value, calculate_modulus, double_to_percentage, ease_out_cubic,
    easing_in_out_function, gap_thickness_to_angle, get_adjusted_angle, get_interpolated_angle,
    get_thickness_transition, is_full_circle, percentage_to_value,
};
