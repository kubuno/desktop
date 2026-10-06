//! Ring geometry primitive — calculations (mirrors
//! `Files.App.Controls/Storage/RingShape/RingShape.cs`, namespace
//! `Files.App.Controls.Primitives`).
//!
//! The C# is a `Path` whose `Data` property is recomputed as either an
//! `EllipseGeometry` (full circle/ellipse) or a `PathGeometry` with an
//! `ArcSegment` (partial arc). Having no XAML, only the pure CALCULATIONS
//! are ported: normalized angles, radius adjustment, start point and arc
//! segment. The [`RingArc`] result describes exactly what a Direct2D render
//! must draw (`ID2D1PathGeometry` + `D2D1_ARC_SEGMENT`, or an ellipse).
//!
//! The input struct [`RingShape`] (its `DependencyProperty`s + defaults)
//! lives in [`super::ring_shape_properties`]; this file carries its
//! calculation `impl`.

use super::ring_shape_properties::RingShape;

/// `Microsoft.UI.Xaml.Media.SweepDirection`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SweepDirection {
    Counterclockwise,
    Clockwise,
}

/// `Windows.Foundation.Point`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Result of a `RingShape` calculation — either a full ellipse (closed,
/// complete ring), or an arc (`DrawArc`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RingArc {
    /// `DrawEllipse`: complete closed ring.
    Ellipse {
        center: Point,
        radius_x: f64,
        radius_y: f64,
    },
    /// `DrawArc`: partial arc, from the start point to the end point.
    Arc {
        start: Point,
        end: Point,
        radius_width: f64,
        radius_height: f64,
        is_large_arc: bool,
        sweep: SweepDirection,
    },
}

const DEGREES_TO_RADIANS: f64 = std::f64::consts::PI / 180.0;

/// Fields computed by `UpdateSizeAndStroke` actually consumed by
/// `UpdatePath` (the C#'s intermediate `_private` fields are inlined).
struct Computed {
    equal_radius: f64,
    center: Point,
    radius_width: f64,
    radius_height: f64,
    valid_start_angle: f64,
    valid_end_angle: f64,
}

impl RingShape {
    /// `UpdatePath`: returns the geometry to draw, or `None` in degenerate
    /// cases (zero size/radius), like the C#'s early return.
    pub fn compute(&self) -> Option<RingArc> {
        let c = self.update_size_and_stroke();

        // Degenerate cases: ActualWidth/Height <= 0 or adjusted radii <= 0.
        if self.width <= 0.0 || self.height <= 0.0 || c.radius_width <= 0.0 || c.radius_height <= 0.0
        {
            return None;
        }

        let start_angle = c.valid_start_angle;
        let end_angle = c.valid_end_angle;

        if end_angle >= start_angle + 360.0 {
            Some(draw_ellipse(
                self.is_circle,
                c.center,
                c.equal_radius,
                c.radius_width,
                c.radius_height,
            ))
        } else {
            Some(draw_arc(
                self.sweep_direction,
                self.is_circle,
                c.center,
                start_angle,
                end_angle,
                c.equal_radius,
                c.radius_width,
                c.radius_height,
            ))
        }
    }

    fn update_size_and_stroke(&self) -> Computed {
        let radius_width = adjust_radius(self.radius_width, self.width, self.stroke_thickness);
        let radius_height = adjust_radius(self.radius_height, self.height, self.stroke_thickness);

        let equal_size_width = calculate_equal_size(self.width, self.height, self.stroke_thickness);
        let equal_radius = calculate_equal_radius(
            self.radius_width,
            self.radius_height,
            self.stroke_thickness,
            equal_size_width,
        );

        let center = Point::new(self.width / 2.0, self.height / 2.0);

        let (normalized_min_angle, normalized_max_angle) =
            calculate_normalized_angles(self.min_angle, self.max_angle);

        let valid_start_angle = validate_angle(self.start_angle, normalized_min_angle, normalized_max_angle);
        let valid_end_angle = validate_angle(self.end_angle, normalized_min_angle, normalized_max_angle);

        Computed {
            equal_radius,
            center,
            radius_width,
            radius_height,
            valid_start_angle,
            valid_end_angle,
        }
    }
}

/// `DrawEllipse`.
fn draw_ellipse(
    is_circle: bool,
    center: Point,
    equal_radius: f64,
    radius_width: f64,
    radius_height: f64,
) -> RingArc {
    if is_circle {
        RingArc::Ellipse {
            center,
            radius_x: equal_radius,
            radius_y: equal_radius,
        }
    } else {
        RingArc::Ellipse {
            center,
            radius_x: radius_width,
            radius_y: radius_height,
        }
    }
}

/// `DrawArc`.
#[allow(clippy::too_many_arguments)]
fn draw_arc(
    sweep: SweepDirection,
    is_circle: bool,
    center: Point,
    start_angle: f64,
    end_angle: f64,
    equal_radius: f64,
    radius_width: f64,
    radius_height: f64,
) -> RingArc {
    let (rw, rh) = if is_circle {
        (equal_radius, equal_radius)
    } else {
        (radius_width, radius_height)
    };

    let start = arc_start_point(sweep, center, start_angle, rw, rh);
    let (end, is_large_arc, seg_sweep) = create_arc_segment(sweep, center, start_angle, end_angle, rw, rh);

    RingArc::Arc {
        start,
        end,
        radius_width: rw,
        radius_height: rh,
        is_large_arc,
        sweep: seg_sweep,
    }
}

/// `ArcStartPoint`.
fn arc_start_point(
    sweep: SweepDirection,
    center: Point,
    start_angle: f64,
    radius_width: f64,
    radius_height: f64,
) -> Point {
    if sweep == SweepDirection::Counterclockwise {
        Point::new(
            center.x - (start_angle * DEGREES_TO_RADIANS).sin() * radius_width,
            center.y - (start_angle * DEGREES_TO_RADIANS).cos() * radius_height,
        )
    } else {
        Point::new(
            center.x + (start_angle * DEGREES_TO_RADIANS).sin() * radius_width,
            center.y - (start_angle * DEGREES_TO_RADIANS).cos() * radius_height,
        )
    }
}

/// `CreateArcSegment` → (end point, IsLargeArc, segment's SweepDirection).
fn create_arc_segment(
    sweep: SweepDirection,
    center: Point,
    start_angle: f64,
    end_angle: f64,
    radius_width: f64,
    radius_height: f64,
) -> (Point, bool, SweepDirection) {
    if sweep == SweepDirection::Counterclockwise {
        let point = Point::new(
            center.x - (end_angle * DEGREES_TO_RADIANS).sin() * radius_width,
            center.y - (end_angle * DEGREES_TO_RADIANS).cos() * radius_height,
        );
        if end_angle < start_angle {
            (point, (end_angle - start_angle) <= -180.0, SweepDirection::Clockwise)
        } else {
            (point, (end_angle - start_angle) >= 180.0, SweepDirection::Counterclockwise)
        }
    } else {
        let point = Point::new(
            center.x + (end_angle * DEGREES_TO_RADIANS).sin() * radius_width,
            center.y - (end_angle * DEGREES_TO_RADIANS).cos() * radius_height,
        );
        if end_angle < start_angle {
            (point, (end_angle - start_angle) <= -180.0, SweepDirection::Counterclockwise)
        } else {
            (point, (end_angle - start_angle) >= 180.0, SweepDirection::Clockwise)
        }
    }
}

/// `CalculateEqualSize` → the common (square) dimension.
fn calculate_equal_size(width: f64, height: f64, stroke_thickness: f64) -> f64 {
    let smaller = width.min(height);
    if smaller > stroke_thickness * 2.0 {
        smaller
    } else {
        stroke_thickness * 2.0
    }
}

/// `CalculateEqualRadius`.
fn calculate_equal_radius(
    radius_width: f64,
    radius_height: f64,
    stroke_thickness: f64,
    equal_size_width: f64,
) -> f64 {
    let smaller = radius_width.min(radius_height);
    if smaller <= stroke_thickness {
        stroke_thickness
    } else if smaller >= (equal_size_width / 2.0) - (stroke_thickness / 2.0) {
        (equal_size_width / 2.0) - (stroke_thickness / 2.0)
    } else {
        smaller
    }
}

/// `CalculateAndSetNormalizedAngles` → (`_normalizedMinAngle`, `_normalizedMaxAngle`).
fn calculate_normalized_angles(min_angle: f64, max_angle: f64) -> (f64, f64) {
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
    let normalized_max = result;

    (normalized_min, normalized_max)
}

/// `ValidateAngle`: clamps the angle within `[normalizedMin, normalizedMax]`.
fn validate_angle(angle: f64, normalized_min: f64, normalized_max: f64) -> f64 {
    if angle >= normalized_max {
        normalized_max
    } else if angle <= normalized_min {
        normalized_min
    } else {
        angle
    }
}

/// `AdjustRadiusWidth` / `AdjustRadiusHeight` (same logic on each axis).
fn adjust_radius(radius: f64, dimension: f64, stroke_thickness: f64) -> f64 {
    let max_value = (dimension / 2.0) - (stroke_thickness / 2.0);
    let threshold = stroke_thickness;
    if radius >= max_value {
        max_value
    } else if radius <= max_value && radius >= threshold {
        radius
    } else {
        threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degenerate_returns_none() {
        let r = RingShape::default();
        assert_eq!(r.compute(), None); // width/height = 0
    }

    #[test]
    fn full_circle_is_ellipse() {
        let r = RingShape {
            width: 100.0,
            height: 100.0,
            stroke_thickness: 4.0,
            start_angle: 0.0,
            end_angle: 360.0,
            radius_width: 40.0,
            radius_height: 40.0,
            is_circle: true,
            ..Default::default()
        };
        // end_angle(360) >= start(0)+360 → ellipse
        match r.compute() {
            Some(RingArc::Ellipse { radius_x, .. }) => assert!(radius_x > 0.0),
            other => panic!("attendu Ellipse, obtenu {other:?}"),
        }
    }

    #[test]
    fn partial_is_arc() {
        let r = RingShape {
            width: 100.0,
            height: 100.0,
            stroke_thickness: 4.0,
            start_angle: 0.0,
            end_angle: 90.0,
            radius_width: 40.0,
            radius_height: 40.0,
            is_circle: true,
            ..Default::default()
        };
        match r.compute() {
            Some(RingArc::Arc { is_large_arc, .. }) => assert!(!is_large_arc),
            other => panic!("attendu Arc, obtenu {other:?}"),
        }
    }

    #[test]
    fn arc_start_point_clockwise_top() {
        // Angle 0° horaire : sin0=0, cos0=1 → point = (cx, cy - r)
        let p = arc_start_point(SweepDirection::Clockwise, Point::new(50.0, 50.0), 0.0, 40.0, 40.0);
        assert!((p.x - 50.0).abs() < 1e-9);
        assert!((p.y - 10.0).abs() < 1e-9);
    }
}
