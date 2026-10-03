//! `RingShape` properties (mirrors
//! `Files.App.Controls/Storage/RingShape/RingShape.Properties.cs`).
//!
//! On the C# side these are `[GeneratedDependencyProperty]`; the only
//! portable content is the default values. They're ported here as the input
//! fields of the [`RingShape`] struct + its `impl Default` (the
//! `OnXChanged` / DP wiring are WinUI machinery, not ported). The
//! CALCULATIONS live in the [`super::ring_shape`] module.

use super::ring_shape::SweepDirection;

/// `RingShape` inputs (its `DependencyProperty`s + the control's size).
/// Default values mirror `RingShape.Properties.cs`.
#[derive(Clone, Copy, Debug)]
pub struct RingShape {
    pub width: f64,
    pub height: f64,
    pub stroke_thickness: f64,
    pub start_angle: f64,
    pub end_angle: f64,
    pub min_angle: f64,
    pub max_angle: f64,
    pub radius_width: f64,
    pub radius_height: f64,
    pub is_circle: bool,
    pub sweep_direction: SweepDirection,
}

impl Default for RingShape {
    fn default() -> Self {
        Self {
            width: 0.0,
            height: 0.0,
            stroke_thickness: 0.0,
            start_angle: 0.0,
            end_angle: 90.0,
            min_angle: 0.0,
            max_angle: 360.0,
            radius_width: 0.0,
            radius_height: 0.0,
            is_circle: false,
            sweep_direction: SweepDirection::Clockwise,
        }
    }
}
