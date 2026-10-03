//! What shapes are filled and outlined with: [`Brush`] (solid, linear and radial gradients) and
//! [`Pen`] (width, dash style, caps, joins).

use drive_app_controls::Rect;

use super::types::{Color, PointF, RectExt};

/// One colour of a gradient at `position` (`0..=1` along it) — GDI+'s `ColorBlend` entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    pub position: f32,
    pub color: Color,
}

impl GradientStop {
    pub const fn new(position: f32, color: Color) -> Self {
        Self { position, color }
    }
}

/// What a gradient does past its ends (`WrapMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WrapMode {
    /// The end colours extend (`WrapMode.Clamp`, Direct2D `CLAMP`).
    #[default]
    Clamp,
    /// The gradient repeats (`WrapMode.Tile`).
    Tile,
    /// The gradient repeats mirrored (`WrapMode.TileFlipX`/`TileFlipXY`).
    TileFlip,
}

/// The direction of a [`LinearGradientBrush`] built from a rectangle (`LinearGradientMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinearGradientMode {
    #[default]
    Horizontal,
    Vertical,
    /// Top-left to bottom-right.
    ForwardDiagonal,
    /// Top-right to bottom-left.
    BackwardDiagonal,
}

/// A gradient along the line `start → end` (`LinearGradientBrush`).
#[derive(Debug, Clone, PartialEq)]
pub struct LinearGradientBrush {
    pub start: PointF,
    pub end: PointF,
    /// Sorted by position; at least one.
    pub stops: Vec<GradientStop>,
    pub wrap: WrapMode,
}

impl LinearGradientBrush {
    /// From `c1` at `start` to `c2` at `end`.
    pub fn new(start: PointF, end: PointF, c1: Color, c2: Color) -> Self {
        Self { start, end, stops: vec![GradientStop::new(0.0, c1), GradientStop::new(1.0, c2)], wrap: WrapMode::Clamp }
    }

    /// Across `rect` in `mode` (`new LinearGradientBrush(rect, c1, c2, mode)`).
    pub fn from_rect(rect: Rect, c1: Color, c2: Color, mode: LinearGradientMode) -> Self {
        let (start, end) = match mode {
            LinearGradientMode::Horizontal => (PointF::new(rect.left, rect.top), PointF::new(rect.right, rect.top)),
            LinearGradientMode::Vertical => (PointF::new(rect.left, rect.top), PointF::new(rect.left, rect.bottom)),
            LinearGradientMode::ForwardDiagonal => (PointF::new(rect.left, rect.top), PointF::new(rect.right, rect.bottom)),
            LinearGradientMode::BackwardDiagonal => (PointF::new(rect.right, rect.top), PointF::new(rect.left, rect.bottom)),
        };
        Self::new(start, end, c1, c2)
    }

    /// Across `rect` at `angle` degrees clockwise from the x-axis (`new LinearGradientBrush(rect,
    /// c1, c2, angle)`): the line runs through the centre and spans the whole rectangle.
    pub fn with_angle(rect: Rect, c1: Color, c2: Color, angle: f32) -> Self {
        let (s, c) = angle.to_radians().sin_cos();
        let center = rect.center();
        // Half the rectangle's extent projected on the direction, so both corners are covered.
        let half = (rect.width() * c.abs() + rect.height() * s.abs()) / 2.0;
        Self::new(PointF::new(center.x - c * half, center.y - s * half), PointF::new(center.x + c * half, center.y + s * half), c1, c2)
    }

    /// Replaces the colours with `stops` (`InterpolationColors`); sorted, never empty.
    pub fn with_stops(mut self, stops: &[GradientStop]) -> Self {
        if !stops.is_empty() {
            self.stops = sorted(stops);
        }
        self
    }

    pub fn with_wrap(mut self, wrap: WrapMode) -> Self {
        self.wrap = wrap;
        self
    }
}

/// A gradient radiating from `center` over an ellipse of radii `radius_x`/`radius_y` — GDI+'s
/// elliptical `PathGradientBrush` (`CenterColor` → `SurroundColors`), Direct2D's radial brush.
/// `origin_offset` moves the focal point off the centre (`CenterPoint`).
#[derive(Debug, Clone, PartialEq)]
pub struct RadialGradientBrush {
    pub center: PointF,
    pub origin_offset: PointF,
    pub radius_x: f32,
    pub radius_y: f32,
    /// From the centre (0) to the rim (1).
    pub stops: Vec<GradientStop>,
    pub wrap: WrapMode,
}

impl RadialGradientBrush {
    pub fn new(center: PointF, radius_x: f32, radius_y: f32, center_color: Color, surround_color: Color) -> Self {
        Self {
            center,
            origin_offset: PointF::default(),
            radius_x,
            radius_y,
            stops: vec![GradientStop::new(0.0, center_color), GradientStop::new(1.0, surround_color)],
            wrap: WrapMode::Clamp,
        }
    }

    /// The ellipse inscribed in `rect` (`PathGradientBrush` over `AddEllipse(rect)`).
    pub fn from_rect(rect: Rect, center_color: Color, surround_color: Color) -> Self {
        Self::new(rect.center(), rect.width() / 2.0, rect.height() / 2.0, center_color, surround_color)
    }

    pub fn with_stops(mut self, stops: &[GradientStop]) -> Self {
        if !stops.is_empty() {
            self.stops = sorted(stops);
        }
        self
    }

    pub fn with_origin_offset(mut self, offset: PointF) -> Self {
        self.origin_offset = offset;
        self
    }
}

fn sorted(stops: &[GradientStop]) -> Vec<GradientStop> {
    let mut v = stops.to_vec();
    v.sort_by(|a, b| a.position.total_cmp(&b.position));
    v
}

/// What a shape is filled with (`System.Drawing.Brush`).
#[derive(Debug, Clone, PartialEq)]
pub enum Brush {
    Solid(Color),
    Linear(LinearGradientBrush),
    Radial(RadialGradientBrush),
}

impl Brush {
    /// A solid brush (`new SolidBrush(color)`).
    pub fn solid(color: impl Into<Color>) -> Self {
        Brush::Solid(color.into())
    }

    /// The colour at `t` (`0..=1`) along the gradient, or the solid colour.
    pub fn color_at(&self, t: f32) -> Color {
        let stops = match self {
            Brush::Solid(c) => return *c,
            Brush::Linear(l) => &l.stops,
            Brush::Radial(r) => &r.stops,
        };
        stop_color(stops, t)
    }

    /// One colour standing for the brush where a surface cannot draw gradients: its middle.
    pub fn representative(&self) -> Color {
        self.color_at(0.5)
    }

    /// Whether it paints nothing at all.
    pub fn is_invisible(&self) -> bool {
        match self {
            Brush::Solid(c) => c.is_transparent(),
            Brush::Linear(l) => l.stops.iter().all(|s| s.color.is_transparent()),
            Brush::Radial(r) => r.stops.iter().all(|s| s.color.is_transparent()),
        }
    }
}

/// The colour of a sorted stop list at `t`.
pub fn stop_color(stops: &[GradientStop], t: f32) -> Color {
    let Some(first) = stops.first() else { return Color::TRANSPARENT };
    if t <= first.position {
        return first.color;
    }
    for pair in stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.position {
            let span = (b.position - a.position).max(f32::EPSILON);
            return a.color.lerp(b.color, (t - a.position) / span);
        }
    }
    stops.last().map_or(Color::TRANSPARENT, |s| s.color)
}

impl From<Color> for Brush {
    fn from(c: Color) -> Self {
        Brush::Solid(c)
    }
}

impl From<LinearGradientBrush> for Brush {
    fn from(b: LinearGradientBrush) -> Self {
        Brush::Linear(b)
    }
}

impl From<RadialGradientBrush> for Brush {
    fn from(b: RadialGradientBrush) -> Self {
        Brush::Radial(b)
    }
}

/// The pattern of a pen's dashes (`DashStyle`). Lengths are in multiples of the pen's width, as in
/// GDI+.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DashStyle {
    #[default]
    Solid,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
    /// The pen's [`Pen::dash_pattern`].
    Custom,
}

impl DashStyle {
    /// The dash/gap lengths of a named style, in pen widths (GDI+'s values).
    pub fn pattern(self) -> &'static [f32] {
        match self {
            DashStyle::Solid | DashStyle::Custom => &[],
            DashStyle::Dash => &[3.0, 1.0],
            DashStyle::Dot => &[1.0, 1.0],
            DashStyle::DashDot => &[3.0, 1.0, 1.0, 1.0],
            DashStyle::DashDotDot => &[3.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        }
    }
}

/// How an open line ends (`LineCap`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineCap {
    #[default]
    Flat,
    Square,
    Round,
    Triangle,
}

/// How two segments meet (`LineJoin`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineJoin {
    #[default]
    Miter,
    Bevel,
    Round,
    MiterClipped,
}

/// Where the stroke sits on a closed shape's outline (`PenAlignment`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PenAlignment {
    /// Centred on the outline (GDI+'s default).
    #[default]
    Center,
    /// Wholly inside the shape — a border that never leaves its box.
    Inset,
}

/// What lines and outlines are drawn with (`System.Drawing.Pen`).
#[derive(Debug, Clone, PartialEq)]
pub struct Pen {
    pub brush: Brush,
    /// In DIP (scaled by the transform, like GDI+).
    pub width: f32,
    pub dash_style: DashStyle,
    /// The dash/gap lengths for [`DashStyle::Custom`], in pen widths.
    pub dash_pattern: Vec<f32>,
    /// Where the pattern starts, in pen widths.
    pub dash_offset: f32,
    pub start_cap: LineCap,
    pub end_cap: LineCap,
    pub dash_cap: LineCap,
    pub line_join: LineJoin,
    pub miter_limit: f32,
    pub alignment: PenAlignment,
}

impl Pen {
    /// A solid pen (`new Pen(color, width)`).
    pub fn new(color: impl Into<Color>, width: f32) -> Self {
        Self::from_brush(Brush::Solid(color.into()), width)
    }

    /// A pen stroking with `brush` (`new Pen(brush, width)`) — a gradient outline.
    pub fn from_brush(brush: impl Into<Brush>, width: f32) -> Self {
        Self {
            brush: brush.into(),
            width: width.max(0.0),
            dash_style: DashStyle::Solid,
            dash_pattern: Vec::new(),
            dash_offset: 0.0,
            start_cap: LineCap::Flat,
            end_cap: LineCap::Flat,
            dash_cap: LineCap::Flat,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            alignment: PenAlignment::Center,
        }
    }

    pub fn with_dash(mut self, style: DashStyle) -> Self {
        self.dash_style = style;
        self
    }

    /// A custom pattern (sets [`DashStyle::Custom`]).
    pub fn with_dash_pattern(mut self, pattern: &[f32]) -> Self {
        self.dash_style = DashStyle::Custom;
        self.dash_pattern = pattern.iter().map(|v| v.max(0.0)).collect();
        self
    }

    /// The same cap at both ends and on the dashes (`SetLineCap`).
    pub fn with_caps(mut self, cap: LineCap) -> Self {
        self.start_cap = cap;
        self.end_cap = cap;
        self.dash_cap = cap;
        self
    }

    pub fn with_join(mut self, join: LineJoin) -> Self {
        self.line_join = join;
        self
    }

    pub fn with_alignment(mut self, alignment: PenAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// The dash/gap lengths in force, in pen widths (empty = solid).
    pub fn effective_pattern(&self) -> &[f32] {
        match self.dash_style {
            DashStyle::Custom => &self.dash_pattern,
            other => other.pattern(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_colours_interpolate_between_sorted_stops() {
        let b = Brush::from(LinearGradientBrush::new(PointF::new(0.0, 0.0), PointF::new(10.0, 0.0), Color::BLACK, Color::WHITE).with_stops(&[
            GradientStop::new(1.0, Color::WHITE),
            GradientStop::new(0.0, Color::BLACK),
            GradientStop::new(0.5, Color::RED),
        ]));
        assert_eq!(b.color_at(0.5), Color::RED);
        assert_eq!(b.color_at(-1.0), Color::BLACK);
        assert_eq!(b.color_at(2.0), Color::WHITE);
        assert!((b.color_at(0.25).r - 0.5).abs() < 1e-5);
        assert!(Brush::solid(Color::TRANSPARENT).is_invisible());
    }

    #[test]
    fn gradient_geometry_from_rect_and_angle() {
        let r = Rect::from_xywh(0.0, 0.0, 100.0, 50.0);
        let v = LinearGradientBrush::from_rect(r, Color::BLACK, Color::WHITE, LinearGradientMode::Vertical);
        assert_eq!((v.start, v.end), (PointF::new(0.0, 0.0), PointF::new(0.0, 50.0)));
        let h = LinearGradientBrush::with_angle(r, Color::BLACK, Color::WHITE, 0.0);
        assert!((h.start.x - 0.0).abs() < 1e-4 && (h.end.x - 100.0).abs() < 1e-4);
        let radial = RadialGradientBrush::from_rect(r, Color::WHITE, Color::BLACK);
        assert_eq!((radial.center, radial.radius_x, radial.radius_y), (PointF::new(50.0, 25.0), 50.0, 25.0));
    }

    #[test]
    fn dash_patterns_are_gdiplus_ones() {
        assert_eq!(Pen::new(Color::BLACK, 1.0).with_dash(DashStyle::Dash).effective_pattern(), &[3.0, 1.0]);
        assert!(Pen::new(Color::BLACK, 1.0).effective_pattern().is_empty());
        assert_eq!(Pen::new(Color::BLACK, 2.0).with_dash_pattern(&[2.0, -1.0]).effective_pattern(), &[2.0, 0.0]);
    }
}
