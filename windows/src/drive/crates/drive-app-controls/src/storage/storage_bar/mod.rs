//! Storage bar (mirrors `Files.App.Controls/Storage/StorageBar`).
//!
//! Mirror split: [`storage_bar`] carries the CALCULATIONS (← `StorageBar.cs`)
//! and [`storage_bar_properties`] the `DependencyProperty` defaults
//! (← `StorageBar.Properties.cs`). This `mod.rs` is pure glue: it
//! RE-EXPORTS the historically flat API (`storage_bar::X`) so no caller
//! breaks.

pub mod storage_bar;
pub mod storage_bar_properties;

pub use storage_bar::{corner_radius, severity, severity_default, value_bar_width};
pub use storage_bar_properties::{
    PERCENT_CAUTION, PERCENT_CRITICAL, TRACK_BAR_HEIGHT, VALUE_BAR_HEIGHT,
};
