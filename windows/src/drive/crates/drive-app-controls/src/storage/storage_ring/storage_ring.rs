//! Circular storage gauge — calculations (mirrors
//! `Files.App.Controls/Storage/StorageRing/StorageRing.cs`).
//!
//! The C# is a templated `RangeBase` with two [`RingShape`]s ("value" and
//! "track") whose size, angles and stroke thickness it continuously
//! recomputes based on `Value`, then picks a visual state
//! (Safe/Caution/Critical/Disabled). The pure CALCULATIONS are ported here:
//! `compute` reproduces the `UpdateRings` cascade and returns
//! [`StorageRingLayout`] — the two `RingShape`s ready to be drawn + the
//! visual state. The input struct [`StorageRing`] (its `DependencyProperty`s
//! + defaults) lives in [`super::storage_ring_properties`].

use super::storage_ring_properties::StorageRing;
use crate::storage::data::{StorageSeverity, ThicknessCheck};
use crate::storage::ring_shape::{RingShape, SweepDirection};

/// Minimum container size (C#'s `minSize`).
const MIN_SIZE: f64 = 8.0;

/// Result of `UpdateRings`: the two rings ready to draw + the state.
#[derive(Clone, Copy, Debug)]
pub struct StorageRingLayout {
    pub adjusted_size: f64,
    pub center: f64,
    pub percent: f64,
    pub value_angle: f64,
    /// `None` = "Disabled" state (`IsEnabled == false`).
    pub severity: Option<StorageSeverity>,
    pub value_ring: RingShape,
    pub track_ring: RingShape,
}

impl StorageRing {
    /// `UpdateRings`: full calculation cascade, returns the geometry of the
    /// two rings and the visual state.
    pub fn compute(&self) -> StorageRingLayout {
        // UpdateContainerCenterAndSizes
        let border_width = self.border_left + self.border_right;
        let border_height = self.border_top + self.border_bottom;
        let corrected_width =
            self.width - (border_width * 2.0) - (self.padding_left + self.padding_right);
        let corrected_height =
            self.height - (border_height * 2.0) - (self.padding_top + self.padding_bottom);
        let check = corrected_width.min(corrected_height);
        let adjusted_size = if check < MIN_SIZE { MIN_SIZE } else { check };
        let center = adjusted_size / 2.0;

        // UpdateRingThickness (computed upfront: it's a logical dependency
        // of the following steps, reapplied at the end of the cycle without
        // changing state).
        let value_ring_thickness = self.value_ring_thickness;
        let track_ring_thickness = self.track_ring_thickness;
        let (thickness_check, larger_thickness) = if value_ring_thickness > track_ring_thickness {
            (ThicknessCheck::Value, value_ring_thickness)
        } else if value_ring_thickness < track_ring_thickness {
            (ThicknessCheck::Track, track_ring_thickness)
        } else {
            (ThicknessCheck::Equal, value_ring_thickness)
        };

        // UpdateNormalizedAngles
        let (normalized_min_angle, normalized_max_angle) =
            normalized_angles(self.min_angle, self.max_angle);

        // UpdateRadii: _sharedRadius (the `newRadius` parameter only
        // influences unused local variables in the C# — reproduced as-is).
        let radii_check =
            (adjusted_size / 2.0) - if thickness_check == ThicknessCheck::Equal { 0.0 } else { larger_thickness / 2.0 };
        let shared_radius = if radii_check <= 4.0 { 4.0 } else { radii_check };

        // UpdateGapAngle
        let gap_angle = crate::storage::gap_thickness_to_angle(shared_radius, larger_thickness * 0.75);

        // UpdateValues: ValueAngle + Percent (from Value).
        let value_angle = self.double_to_angle(
            self.value,
            self.minimum,
            self.maximum,
            normalized_min_angle,
            normalized_max_angle,
        );
        let percent = crate::storage::double_to_percentage(self.value, self.minimum, self.maximum);

        // UpdateRingAngles
        let (value_start, value_end, track_start, track_end) = self.ring_angles(
            percent,
            value_angle,
            gap_angle,
            normalized_min_angle,
            normalized_max_angle,
        );

        // UpdateRingStrokes
        let (value_stroke, track_stroke) = self.ring_strokes(
            percent,
            value_angle,
            gap_angle,
            normalized_min_angle,
            normalized_max_angle,
        );

        // UpdateVisualState
        let severity = if self.is_enabled {
            Some(if percent >= self.percent_critical {
                StorageSeverity::Critical
            } else if percent >= self.percent_caution {
                StorageSeverity::Caution
            } else {
                StorageSeverity::Safe
            })
        } else {
            None
        };

        // UpdateRingLayouts: Radius = _sharedRadius, Width/Height = AdjustedSize.
        let make_ring = |start: f64, end: f64, stroke: f64| RingShape {
            width: adjusted_size,
            height: adjusted_size,
            stroke_thickness: stroke,
            start_angle: start,
            end_angle: end,
            min_angle: self.min_angle,
            max_angle: self.max_angle,
            radius_width: shared_radius,
            radius_height: shared_radius,
            is_circle: false,
            sweep_direction: SweepDirection::Clockwise,
        };

        StorageRingLayout {
            adjusted_size,
            center,
            percent,
            value_angle,
            severity,
            value_ring: make_ring(value_start, value_end, value_stroke),
            track_ring: make_ring(track_start, track_end, track_stroke),
        }
    }

    /// `DoubleToAngle`: `value` (∈[min,max]) → angle (∈[MinAngle,MaxAngle]).
    /// Uses NORMALIZED angles for clamping, RAW angles for amplitude and
    /// offset — exactly like the C#.
    fn double_to_angle(
        &self,
        value: f64,
        min_value: f64,
        max_value: f64,
        min_angle_norm: f64,
        max_angle_norm: f64,
    ) -> f64 {
        if value < min_value {
            return min_angle_norm;
        }
        if value > max_value {
            return max_angle_norm;
        }
        let normalized_value = (value - min_value) / (max_value - min_value);
        let angle_range = self.max_angle - self.min_angle;
        self.min_angle + (normalized_value * angle_range)
    }

    /// `UpdateRingAngles` → (valueStart, valueEnd, trackStart, trackEnd).
    fn ring_angles(
        &self,
        percent: f64,
        value_angle: f64,
        gap_angle: f64,
        normalized_min_angle: f64,
        normalized_max_angle: f64,
    ) -> (f64, f64, f64, f64) {
        let value_start_angle = normalized_min_angle;
        let value_end_angle;
        let mut track_start_angle;
        let track_end_angle;

        let min_percent = crate::storage::double_to_percentage(self.minimum, self.minimum, self.maximum);
        let max_percent = crate::storage::double_to_percentage(self.maximum, self.minimum, self.maximum);

        if percent <= min_percent {
            value_end_angle = normalized_min_angle;
            track_start_angle = normalized_max_angle - 0.01;
            track_end_angle = normalized_min_angle;
        } else if percent > min_percent && percent < min_percent + 2.0 {
            value_end_angle = value_angle;

            let interpolated_start_to = crate::storage::get_adjusted_angle(
                min_percent,
                percent,
                min_percent + 2.0,
                normalized_min_angle,
                normalized_min_angle + gap_angle,
                value_angle,
                true,
            );

            let interpolated_end_to = if crate::storage::is_full_circle(normalized_min_angle, normalized_max_angle) {
                crate::storage::get_adjusted_angle(
                    min_percent,
                    percent,
                    min_percent + 2.0,
                    normalized_max_angle,
                    normalized_max_angle - (gap_angle + value_angle),
                    value_angle,
                    true,
                )
            } else {
                normalized_max_angle
            };

            track_start_angle = interpolated_end_to;
            track_end_angle = interpolated_start_to;
        } else if percent >= max_percent {
            value_end_angle = normalized_max_angle;
            track_start_angle = normalized_max_angle;
            track_end_angle = normalized_min_angle;
        } else {
            value_end_angle = value_angle;

            if crate::storage::is_full_circle(self.min_angle, self.max_angle) {
                track_start_angle = normalized_max_angle - gap_angle;
                if value_angle > (normalized_max_angle - (gap_angle * 2.0)) {
                    track_end_angle = normalized_max_angle - (gap_angle - 0.0001);
                } else {
                    track_end_angle =
                        (normalized_min_angle + gap_angle) - (normalized_min_angle - value_angle);
                }
            } else {
                track_start_angle = normalized_max_angle;
                if value_angle > (normalized_max_angle - (gap_angle / 20.0)) {
                    track_end_angle = normalized_max_angle - 0.0001;
                } else {
                    track_end_angle =
                        normalized_min_angle + (gap_angle - (normalized_min_angle - value_angle));
                }
            }
        }

        // Silences the "value never read" warning from the initial default case.
        let _ = &mut track_start_angle;

        (value_start_angle, value_end_angle, track_start_angle, track_end_angle)
    }

    /// `UpdateRingStrokes` → (valueStrokeThickness, trackStrokeThickness).
    fn ring_strokes(
        &self,
        percent: f64,
        value_angle: f64,
        gap_angle: f64,
        normalized_min_angle: f64,
        normalized_max_angle: f64,
    ) -> (f64, f64) {
        let min_percent = crate::storage::double_to_percentage(self.minimum, self.minimum, self.maximum);
        let max_percent = crate::storage::double_to_percentage(self.maximum, self.minimum, self.maximum);

        if percent <= min_percent {
            (0.0, self.track_ring_thickness)
        } else if percent > min_percent && percent < min_percent + 2.0 {
            let value_stroke = crate::storage::get_thickness_transition(
                min_percent,
                percent,
                min_percent + 2.0,
                0.0,
                self.value_ring_thickness,
                true,
            );
            (value_stroke, self.track_ring_thickness)
        } else if percent >= max_percent {
            (self.value_ring_thickness, 0.0)
        } else if crate::storage::is_full_circle(normalized_min_angle, normalized_max_angle) {
            if value_angle > (normalized_max_angle + 1.0) - (gap_angle * 2.0) {
                let track_stroke = crate::storage::get_thickness_transition(
                    (normalized_max_angle + 0.1) - (gap_angle * 2.0),
                    value_angle,
                    normalized_max_angle - gap_angle,
                    self.track_ring_thickness,
                    0.0,
                    true,
                );
                (self.value_ring_thickness, track_stroke)
            } else {
                (self.value_ring_thickness, self.track_ring_thickness)
            }
        } else if value_angle > (normalized_max_angle - gap_angle) {
            let track_stroke = crate::storage::get_thickness_transition(
                (normalized_max_angle + 0.1) - (gap_angle / 2.0),
                value_angle,
                normalized_max_angle - (gap_angle / 2.0),
                self.track_ring_thickness,
                0.0,
                true,
            );
            (self.value_ring_thickness, track_stroke)
        } else {
            (self.value_ring_thickness, self.track_ring_thickness)
        }
    }
}

/// `UpdateNormalizedAngles` (identical to `RingShape`'s, via the helpers).
fn normalized_angles(min_angle: f64, max_angle: f64) -> (f64, f64) {
    let mut result = crate::storage::calculate_modulus(min_angle, 360.0);
    if result >= 180.0 {
        result -= 360.0;
    }
    let normalized_min = result;

    let mut result = crate::storage::calculate_modulus(max_angle, 360.0);
    if result < 180.0 {
        result += 360.0;
    }
    if result > normalized_min + 360.0 {
        result -= 360.0;
    }
    (normalized_min, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(value: f64) -> StorageRing {
        StorageRing {
            value,
            minimum: 0.0,
            maximum: 100.0,
            value_ring_thickness: 4.0,
            track_ring_thickness: 2.0,
            width: 100.0,
            height: 100.0,
            ..Default::default()
        }
    }

    #[test]
    fn empty_ring_has_no_value_stroke() {
        let l = ring(0.0).compute();
        assert_eq!(l.percent, 0.0);
        assert_eq!(l.value_ring.stroke_thickness, 0.0);
        assert_eq!(l.track_ring.stroke_thickness, 2.0);
    }

    #[test]
    fn full_ring_has_no_track_stroke() {
        let l = ring(100.0).compute();
        assert_eq!(l.percent, 100.0);
        assert_eq!(l.value_ring.stroke_thickness, 4.0);
        assert_eq!(l.track_ring.stroke_thickness, 0.0);
    }

    #[test]
    fn severity_thresholds() {
        assert_eq!(ring(50.0).compute().severity, Some(StorageSeverity::Safe));
        assert_eq!(ring(80.0).compute().severity, Some(StorageSeverity::Caution));
        assert_eq!(ring(95.0).compute().severity, Some(StorageSeverity::Critical));
    }

    #[test]
    fn disabled_has_no_severity() {
        let mut r = ring(50.0);
        r.is_enabled = false;
        assert_eq!(r.compute().severity, None);
    }

    #[test]
    fn adjusted_size_floor() {
        let mut r = ring(50.0);
        r.width = 4.0;
        r.height = 4.0;
        assert_eq!(r.compute().adjusted_size, MIN_SIZE);
    }
}
