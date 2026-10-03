//! Shared maths for storage controls (mirrors
//! `Files.App.Controls/Storage/StorageControlsHelpers.cs`).
//!
//! Static class of PURE maths (`System.Math` only), shared by the bar and
//! the ring: interpolation, percentage, angle. DRAWING stays in `drive-app`
//! (immediate Direct2D); the CALCULATIONS live here, transcribed identically
//! from the C# for fidelity.
//!
//! 1:1 transcription of the static methods. Order and formulas match the
//! C#; don't "simplify" anything (fidelity trumps elegance).

/// `CalculateModulus`: modulo always positive or zero, regardless of sign.
pub fn calculate_modulus(number: f64, divider: f64) -> f64 {
    let result = number % divider;
    if result < 0.0 {
        result + divider
    } else {
        result
    }
}

/// `GetThicknessTransition`: thickness interpolated between two bounds, with
/// an optional easing (`EaseOutCubic`).
pub fn get_thickness_transition(
    start_value: f64,
    value: f64,
    end_value: f64,
    start_thickness: f64,
    end_thickness: f64,
    use_easing: bool,
) -> f64 {
    let value = start_value.max(end_value.min(value));
    let t = (value - start_value) / (end_value - start_value);
    if use_easing {
        let eased_t = ease_out_cubic(t);
        start_thickness + eased_t * (end_thickness - start_thickness)
    } else {
        start_thickness + t * (end_thickness - start_thickness)
    }
}

/// `GetAdjustedAngle`: angle interpolated between two bounds, with optional easing.
#[allow(clippy::too_many_arguments)]
pub fn get_adjusted_angle(
    start_value: f64,
    value: f64,
    end_value: f64,
    start_angle: f64,
    end_angle: f64,
    _value_angle: f64,
    use_easing: bool,
) -> f64 {
    let value = start_value.max(end_value.min(value));
    let t = (value - start_value) / (end_value - start_value);
    if use_easing {
        let eased_t = ease_out_cubic(t);
        start_angle + eased_t * (end_angle - start_angle)
    } else {
        start_angle + t * (end_angle - start_angle)
    }
}

/// `DoubleToPercentage`: `value∈[min, max]` as a `[0, 100]` percentage,
/// rounded to 2 decimals (banker's rounding, like
/// `Math.Round(..., MidpointRounding.ToEven)`).
pub fn double_to_percentage(value: f64, min_value: f64, max_value: f64) -> f64 {
    if value < min_value {
        0.0
    } else if value > max_value {
        100.0
    } else {
        let normalized_value = (value - min_value) / (max_value - min_value);
        let percentage = normalized_value * 100.0;
        round_to_even(percentage, 2)
    }
}

/// `PercentageToValue`: a percentage converted to a value, clamped to
/// `[min, max]`. Faithful to the C#: `percentage * (max - min) / 100`
/// (does NOT add `min`).
pub fn percentage_to_value(percentage: f64, min_value: f64, max_value: f64) -> f64 {
    let converted_value = percentage * (max_value - min_value) / 100.0;
    if converted_value < min_value {
        min_value
    } else if converted_value > max_value {
        max_value
    } else {
        converted_value
    }
}

/// `GapThicknessToAngle`: total angle to fit a `thickness` gap around a
/// circle of radius `radius`.
pub fn gap_thickness_to_angle(radius: f64, thickness: f64) -> f64 {
    if radius > 0.0 && thickness > 0.0 {
        let n = std::f64::consts::PI * (radius / thickness);
        360.0 / n
    } else {
        0.0
    }
}

/// `GetInterpolatedAngle`: lerp (as-is from the C#, including its unusual
/// formula `(startAngle + valueAngle) * (endAngle - startAngle)`).
pub fn get_interpolated_angle(start_angle: f64, end_angle: f64, value_angle: f64) -> f64 {
    (start_angle + value_angle) * (end_angle - start_angle)
}

/// `IsFullCircle`: true if the amplitude `|max - min|` equals 360° (within ε).
pub fn is_full_circle(min_angle: f64, max_angle: f64) -> bool {
    let angle_difference = (max_angle - min_angle).abs();
    (angle_difference - 360.0).abs() < f64::EPSILON
}

/// `CalculateInterpolatedValue`: output interpolated between two bounds, optional easing.
pub fn calculate_interpolated_value(
    start_value: f64,
    value: f64,
    end_value: f64,
    start_output: f64,
    end_output: f64,
    use_easing: bool,
) -> f64 {
    let value = start_value.max(end_value.min(value));
    let t = (value - start_value) / (end_value - start_value);
    if use_easing {
        let eased_t = ease_out_cubic(t);
        start_output + eased_t * (end_output - start_output)
    } else {
        start_output + t * (end_output - start_output)
    }
}

/// `EasingInOutFunction`: quadratic ease-in-out (C# example).
pub fn easing_in_out_function(t: f64) -> f64 {
    if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
    }
}

/// `EaseOutCubic`: cubic deceleration.
pub fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powf(3.0)
}

/// "Banker's" rounding (half to even) to `digits` decimals — counterpart of
/// `Math.Round(value, digits, MidpointRounding.ToEven)`.
fn round_to_even(value: f64, digits: i32) -> f64 {
    let factor = 10f64.powi(digits);
    let scaled = value * factor;
    let floor = scaled.floor();
    let diff = scaled - floor;
    let rounded = if (diff - 0.5).abs() < f64::EPSILON {
        // Midpoint: round to the even integer.
        if (floor as i64) % 2 == 0 {
            floor
        } else {
            floor + 1.0
        }
    } else {
        scaled.round()
    };
    rounded / factor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentage_roundtrip() {
        assert_eq!(double_to_percentage(50.0, 0.0, 100.0), 50.0);
        assert_eq!(double_to_percentage(150.0, 0.0, 100.0), 100.0);
        assert_eq!(double_to_percentage(-5.0, 0.0, 100.0), 0.0);
        // Faithful to the C#: no min added.
        assert_eq!(percentage_to_value(50.0, 0.0, 200.0), 100.0);
        assert_eq!(percentage_to_value(50.0, 10.0, 210.0), 100.0);
    }

    #[test]
    fn modulus_is_positive() {
        assert_eq!(calculate_modulus(-90.0, 360.0), 270.0);
        assert_eq!(calculate_modulus(450.0, 360.0), 90.0);
        assert_eq!(calculate_modulus(0.0, 360.0), 0.0);
    }

    #[test]
    fn full_circle_detection() {
        assert!(is_full_circle(0.0, 360.0));
        assert!(!is_full_circle(0.0, 270.0));
    }

    #[test]
    fn gap_angle() {
        assert_eq!(gap_thickness_to_angle(0.0, 4.0), 0.0);
        assert_eq!(gap_thickness_to_angle(10.0, 0.0), 0.0);
        // n = π·(r/t); angle = 360/n
        let a = gap_thickness_to_angle(20.0, 5.0);
        assert!((a - (360.0 / (std::f64::consts::PI * 4.0))).abs() < 1e-9);
    }

    #[test]
    fn banker_rounding() {
        // 12.345 → 12.34 (4 is even), 12.355 → 12.36 (6 is even)
        assert_eq!(round_to_even(12.345, 2), 12.34);
        assert_eq!(round_to_even(12.355, 2), 12.36);
    }

    #[test]
    fn easing_bounds() {
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert_eq!(ease_out_cubic(1.0), 1.0);
        assert_eq!(easing_in_out_function(0.0), 0.0);
        assert_eq!(easing_in_out_function(1.0), 1.0);
    }
}
