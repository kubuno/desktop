//! Ring geometry primitive (mirrors
//! `Files.App.Controls/Storage/RingShape/`, namespace
//! `Files.App.Controls.Primitives`).
//!
//! Mirror split: [`ring_shape`] carries the CALCULATIONS (← `RingShape.cs`)
//! and [`ring_shape_properties`] the input struct + its defaults
//! (← `RingShape.Properties.cs`). This `mod.rs` is pure glue: it
//! RE-EXPORTS the historically flat API (`ring_shape::X`) so no caller
//! breaks.

pub mod ring_shape;
pub mod ring_shape_properties;

pub use ring_shape::{Point, RingArc, SweepDirection};
pub use ring_shape_properties::RingShape;
