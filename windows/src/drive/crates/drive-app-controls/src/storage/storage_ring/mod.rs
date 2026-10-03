//! Circular storage gauge (mirrors
//! `Files.App.Controls/Storage/StorageRing`).
//!
//! Mirror split: [`storage_ring`] carries the CALCULATIONS (← `StorageRing.cs`)
//! and [`storage_ring_properties`] the input struct + its defaults
//! (← `StorageRing.Properties.cs`). This `mod.rs` is pure glue: it
//! RE-EXPORTS the historically flat API (`storage_ring::X`) so no caller
//! breaks.

pub mod storage_ring;
pub mod storage_ring_properties;

pub use storage_ring::StorageRingLayout;
pub use storage_ring_properties::StorageRing;
