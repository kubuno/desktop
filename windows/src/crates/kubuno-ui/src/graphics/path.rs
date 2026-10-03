//! [`GraphicsPath`] (`System.Drawing.Drawing2D.GraphicsPath`): figures of lines, Bézier curves and
//! arcs, filled with a [`FillMode`], outlined with a pen, used as a clip.

use drive_app_controls::Rect;

use super::types::{Matrix, PointF, RectExt};

/// How the interior of self-intersecting or nested figures is decided (`FillMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FillMode {
    /// Even-odd (`FillMode.Alternate`, GDI+'s default).
    #[default]
    Alternate,
    /// Non-zero (`FillMode.Winding`).
    Winding,
}

/// One segment of a figure, from the previous point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    Line(PointF),
    /// A cubic Bézier: two control points, then the end.
    Bezier(PointF, PointF, PointF),
    /// An elliptical arc to `end` (Direct2D's arc segment): radii, the x-axis rotation in degrees,
    /// `large` for the arc over 180°, `clockwise` for the sweep direction on screen.
    Arc { end: PointF, radius_x: f32, radius_y: f32, rotation: f32, large: bool, clockwise: bool },
}

impl Segment {
    fn end(&self) -> PointF {
        match *self {
            Segment::Line(p) => p,
            Segment::Bezier(_, _, p) => p,
            Segment::Arc { end, .. } => end,
        }
    }
}

/// A figure: a start point, its segments, and whether it is closed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Figure {
    pub start: PointF,
    pub segments: Vec<Segment>,
    pub closed: bool,
}

/// A path: a list of figures (`GraphicsPath`). Built with the WinForms `Add…` methods, or with the
/// `move_to`/`line_to`/`bezier_to`/`arc_to` pen-plotter style.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GraphicsPath {
    pub figures: Vec<Figure>,
    pub fill_mode: FillMode,
    /// Whether the next segment starts a new figure (`StartFigure` was called, or nothing yet).
    open_new: bool,
}

impl GraphicsPath {
    pub fn new() -> Self {
        Self { figures: Vec::new(), fill_mode: FillMode::Alternate, open_new: true }
    }

    pub fn with_fill_mode(mut self, mode: FillMode) -> Self {
        self.fill_mode = mode;
        self
    }

    /// Whether the path has no figure.
    pub fn is_empty(&self) -> bool {
        self.figures.iter().all(|f| f.segments.is_empty())
    }

    /// Removes every figure (`Reset`).
    pub fn reset(&mut self) {
        self.figures.clear();
        self.open_new = true;
    }

    /// Ends the current figure open; the next segment starts a new one (`StartFigure`).
    pub fn start_figure(&mut self) {
        self.open_new = true;
    }

    /// Closes the current figure (`CloseFigure`); the next segment starts a new one.
    pub fn close_figure(&mut self) {
        if let Some(f) = self.figures.last_mut() {
            f.closed = true;
        }
        self.open_new = true;
    }

    /// Closes every open figure (`CloseAllFigures`).
    pub fn close_all_figures(&mut self) {
        for f in &mut self.figures {
            f.closed = true;
        }
        self.open_new = true;
    }

    /// The last point of the path (`GetLastPoint`).
    pub fn last_point(&self) -> Option<PointF> {
        let f = self.figures.last()?;
        Some(f.segments.last().map_or(f.start, Segment::end))
    }

    /// Starts a new figure at `p`.
    pub fn move_to(&mut self, p: PointF) {
        self.figures.push(Figure { start: p, segments: Vec::new(), closed: false });
        self.open_new = false;
    }

    /// Continues the current figure from `from`: a new figure when one must start, else a line to
    /// `from` when the path does not end there (GDI+ connects consecutive `Add…` calls in a figure).
    fn continue_from(&mut self, from: PointF) {
        if self.open_new || self.figures.is_empty() {
            self.move_to(from);
            return;
        }
        if self.last_point().is_some_and(|last| last.distance(from) > 1e-4) {
            self.push(Segment::Line(from));
        }
    }

    fn push(&mut self, s: Segment) {
        if self.figures.is_empty() || self.open_new {
            let start = self.last_point().unwrap_or_default();
            self.move_to(start);
        }
        if let Some(f) = self.figures.last_mut() {
            f.segments.push(s);
        }
    }

    pub fn line_to(&mut self, p: PointF) {
        self.push(Segment::Line(p));
    }

    pub fn bezier_to(&mut self, c1: PointF, c2: PointF, end: PointF) {
        self.push(Segment::Bezier(c1, c2, end));
    }

    /// A quadratic Bézier, raised to a cubic.
    pub fn quad_to(&mut self, c: PointF, end: PointF) {
        let start = self.last_point().unwrap_or_default();
        let c1 = PointF::new(start.x + 2.0 / 3.0 * (c.x - start.x), start.y + 2.0 / 3.0 * (c.y - start.y));
        let c2 = PointF::new(end.x + 2.0 / 3.0 * (c.x - end.x), end.y + 2.0 / 3.0 * (c.y - end.y));
        self.push(Segment::Bezier(c1, c2, end));
    }

    /// An SVG/Direct2D-style elliptical arc to `end`.
    pub fn arc_to(&mut self, end: PointF, radius_x: f32, radius_y: f32, rotation: f32, large: bool, clockwise: bool) {
        self.push(Segment::Arc { end, radius_x, radius_y, rotation, large, clockwise });
    }

    // ── WinForms `Add…` ──────────────────────────────────────────────────────────

    /// `AddLine`.
    pub fn add_line(&mut self, p1: PointF, p2: PointF) {
        self.continue_from(p1);
        self.line_to(p2);
    }

    /// `AddLines`: a polyline.
    pub fn add_lines(&mut self, points: &[PointF]) {
        let Some((first, rest)) = points.split_first() else { return };
        self.continue_from(*first);
        for p in rest {
            self.line_to(*p);
        }
    }

    /// `AddPolygon`: a closed figure of its own.
    pub fn add_polygon(&mut self, points: &[PointF]) {
        let Some((first, rest)) = points.split_first() else { return };
        self.move_to(*first);
        for p in rest {
            self.line_to(*p);
        }
        self.close_figure();
    }

    /// `AddRectangle`: a closed figure, clockwise from the top-left corner.
    pub fn add_rectangle(&mut self, r: Rect) {
        self.add_polygon(&[
            PointF::new(r.left, r.top),
            PointF::new(r.right, r.top),
            PointF::new(r.right, r.bottom),
            PointF::new(r.left, r.bottom),
        ]);
    }

    /// `AddRoundedRectangle` (.NET 9): corners of radius `radius`, clamped to half the box.
    pub fn add_rounded_rectangle(&mut self, r: Rect, radius: f32) {
        let rad = radius.max(0.0).min(r.width() / 2.0).min(r.height() / 2.0);
        if rad <= 0.0 {
            self.add_rectangle(r);
            return;
        }
        self.move_to(PointF::new(r.left + rad, r.top));
        self.line_to(PointF::new(r.right - rad, r.top));
        self.arc_to(PointF::new(r.right, r.top + rad), rad, rad, 0.0, false, true);
        self.line_to(PointF::new(r.right, r.bottom - rad));
        self.arc_to(PointF::new(r.right - rad, r.bottom), rad, rad, 0.0, false, true);
        self.line_to(PointF::new(r.left + rad, r.bottom));
        self.arc_to(PointF::new(r.left, r.bottom - rad), rad, rad, 0.0, false, true);
        self.line_to(PointF::new(r.left, r.top + rad));
        self.arc_to(PointF::new(r.left + rad, r.top), rad, rad, 0.0, false, true);
        self.close_figure();
    }

    /// `AddEllipse`: the ellipse inscribed in `r`, a closed figure.
    pub fn add_ellipse(&mut self, r: Rect) {
        let c = r.center();
        let (rx, ry) = (r.width() / 2.0, r.height() / 2.0);
        self.move_to(PointF::new(r.right, c.y));
        self.arc_to(PointF::new(r.left, c.y), rx, ry, 0.0, false, true);
        self.arc_to(PointF::new(r.right, c.y), rx, ry, 0.0, false, true);
        self.close_figure();
    }

    /// `AddArc`: the part of the ellipse inscribed in `r` from `start_angle` over `sweep_angle`
    /// (degrees, clockwise on screen, 0 = 3 o'clock), continuing the current figure.
    pub fn add_arc(&mut self, r: Rect, start_angle: f32, sweep_angle: f32) {
        let sweep = sweep_angle.clamp(-360.0, 360.0);
        let start = ellipse_point(r, start_angle);
        self.continue_from(start);
        self.arc_segments(r, start_angle, sweep);
    }

    /// The arc segments from `start_angle` over `sweep` (the start point is already current).
    fn arc_segments(&mut self, r: Rect, start_angle: f32, sweep: f32) {
        if sweep == 0.0 {
            return;
        }
        let (rx, ry) = (r.width() / 2.0, r.height() / 2.0);
        // At most 180° per segment: a full turn cannot end on its own start, and a small arc never
        // needs the "large" flag.
        let pieces = (sweep.abs() / 180.0).ceil().max(1.0) as usize;
        let step = sweep / pieces as f32;
        for i in 1..=pieces {
            let end = ellipse_point(r, start_angle + step * i as f32);
            self.arc_to(end, rx, ry, 0.0, false, step > 0.0);
        }
    }

    /// `AddPie`: the centre, the arc, closed.
    pub fn add_pie(&mut self, r: Rect, start_angle: f32, sweep_angle: f32) {
        let sweep = sweep_angle.clamp(-360.0, 360.0);
        self.move_to(r.center());
        self.line_to(ellipse_point(r, start_angle));
        self.arc_segments(r, start_angle, sweep);
        self.close_figure();
    }

    /// `AddBezier`.
    pub fn add_bezier(&mut self, p1: PointF, c1: PointF, c2: PointF, p2: PointF) {
        self.continue_from(p1);
        self.bezier_to(c1, c2, p2);
    }

    /// `AddBeziers`: a start point then groups of three (control, control, end).
    pub fn add_beziers(&mut self, points: &[PointF]) {
        let Some((first, rest)) = points.split_first() else { return };
        self.continue_from(*first);
        for [c1, c2, end] in rest.as_chunks::<3>().0 {
            self.bezier_to(*c1, *c2, *end);
        }
    }

    /// `AddCurve`: a cardinal spline through `points` (`tension` 0.5 is GDI+'s default).
    pub fn add_curve(&mut self, points: &[PointF], tension: f32) {
        if points.len() < 2 {
            return;
        }
        self.continue_from(points[0]);
        for (c1, c2, end) in cardinal_beziers(points, tension, false) {
            self.bezier_to(c1, c2, end);
        }
    }

    /// `AddClosedCurve`: a closed cardinal spline.
    pub fn add_closed_curve(&mut self, points: &[PointF], tension: f32) {
        if points.len() < 3 {
            self.add_polygon(points);
            return;
        }
        self.move_to(points[0]);
        for (c1, c2, end) in cardinal_beziers(points, tension, true) {
            self.bezier_to(c1, c2, end);
        }
        self.close_figure();
    }

    /// `AddPath`: appends `other`'s figures (the first one joined to the current figure when
    /// `connect`).
    pub fn add_path(&mut self, other: &GraphicsPath, connect: bool) {
        for (i, f) in other.figures.iter().enumerate() {
            if i == 0 && connect && !self.open_new && !self.figures.is_empty() {
                self.line_to(f.start);
                for s in &f.segments {
                    self.push(*s);
                }
                if f.closed {
                    self.close_figure();
                }
            } else {
                self.figures.push(f.clone());
                self.open_new = f.closed;
            }
        }
    }

    /// Transforms every point (`Transform`). An arc's radii are scaled by the matrix' mean scale
    /// (exact for rotations and uniform scales).
    pub fn transform(&mut self, m: &Matrix) {
        let k = m.mean_scale();
        let rot = m.m12.atan2(m.m11).to_degrees();
        let flips = m.determinant() < 0.0;
        for f in &mut self.figures {
            f.start = m.transform_point(f.start);
            for s in &mut f.segments {
                *s = match *s {
                    Segment::Line(p) => Segment::Line(m.transform_point(p)),
                    Segment::Bezier(a, b, c) => Segment::Bezier(m.transform_point(a), m.transform_point(b), m.transform_point(c)),
                    Segment::Arc { end, radius_x, radius_y, rotation, large, clockwise } => Segment::Arc {
                        end: m.transform_point(end),
                        radius_x: radius_x * k,
                        radius_y: radius_y * k,
                        rotation: rotation + rot,
                        large,
                        clockwise: clockwise != flips,
                    },
                };
            }
        }
    }

    /// The figures as polygons (`Flatten`): curves and arcs cut into segments shorter than about
    /// `tolerance` DIP of deviation. Each entry: the points and whether the figure is closed.
    pub fn flatten(&self, tolerance: f32) -> Vec<(Vec<PointF>, bool)> {
        let tol = tolerance.max(0.01);
        self.figures
            .iter()
            .filter(|f| !f.segments.is_empty())
            .map(|f| {
                let mut pts = vec![f.start];
                let mut cur = f.start;
                for s in &f.segments {
                    match *s {
                        Segment::Line(p) => pts.push(p),
                        Segment::Bezier(c1, c2, end) => {
                            let len = cur.distance(c1) + c1.distance(c2) + c2.distance(end);
                            let n = ((len / tol).sqrt().ceil() as usize).clamp(2, 256);
                            for i in 1..=n {
                                pts.push(bezier_point(cur, c1, c2, end, i as f32 / n as f32));
                            }
                        }
                        Segment::Arc { end, radius_x, radius_y, rotation, large, clockwise } => {
                            pts.extend(flatten_arc(cur, end, radius_x, radius_y, rotation, large, clockwise, tol));
                        }
                    }
                    cur = s.end();
                }
                (pts, f.closed)
            })
            .collect()
    }

    /// The bounding box of the path (`GetBounds`), from its flattened outline; an empty rectangle
    /// for an empty path.
    pub fn bounds(&self) -> Rect {
        let mut out: Option<Rect> = None;
        for (pts, _) in self.flatten(0.25) {
            for p in pts {
                let r = Rect::new(p.x, p.y, p.x, p.y);
                out = Some(match out {
                    Some(o) => o.union(&r),
                    None => r,
                });
            }
        }
        out.unwrap_or_default()
    }

    /// Whether `p` is inside the filled path (`IsVisible`), by the path's fill mode.
    pub fn is_visible(&self, p: PointF) -> bool {
        let mut winding = 0i32;
        let mut crossings = 0u32;
        for (pts, _) in self.flatten(0.25) {
            // Every figure is implicitly closed for filling.
            for i in 0..pts.len() {
                let a = pts[i];
                let b = pts[(i + 1) % pts.len()];
                if (a.y <= p.y) != (b.y <= p.y) {
                    let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                    if x > p.x {
                        crossings += 1;
                        winding += if b.y > a.y { 1 } else { -1 };
                    }
                }
            }
        }
        match self.fill_mode {
            FillMode::Alternate => crossings % 2 == 1,
            FillMode::Winding => winding != 0,
        }
    }

    /// The number of points of the path's definition (`PointCount`): starts and segment ends.
    pub fn point_count(&self) -> usize {
        self.figures.iter().map(|f| 1 + f.segments.len()).sum()
    }
}

/// The point at `angle` degrees (clockwise from 3 o'clock) on the ellipse inscribed in `r`: the
/// true angle of the point, as GDI+ measures it, not the parametric one.
pub fn ellipse_point(r: Rect, angle: f32) -> PointF {
    let c = r.center();
    let (rx, ry) = (r.width() / 2.0, r.height() / 2.0);
    let (s, co) = angle.to_radians().sin_cos();
    if rx <= 0.0 || ry <= 0.0 {
        return c;
    }
    let k = rx * ry / ((ry * co).powi(2) + (rx * s).powi(2)).sqrt();
    PointF::new(c.x + k * co, c.y + k * s)
}

fn bezier_point(p0: PointF, p1: PointF, p2: PointF, p3: PointF, t: f32) -> PointF {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    PointF::new(a * p0.x + b * p1.x + c * p2.x + d * p3.x, a * p0.y + b * p1.y + c * p2.y + d * p3.y)
}

/// The Bézier segments of a cardinal spline through `pts` (GDI+'s `tension`: 0 = straight lines).
fn cardinal_beziers(pts: &[PointF], tension: f32, closed: bool) -> Vec<(PointF, PointF, PointF)> {
    let n = pts.len();
    let k = tension / 3.0;
    let at = |i: isize| -> PointF {
        if closed {
            pts[i.rem_euclid(n as isize) as usize]
        } else {
            pts[i.clamp(0, n as isize - 1) as usize]
        }
    };
    let count = if closed { n } else { n - 1 };
    (0..count as isize)
        .map(|i| {
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            let c1 = PointF::new(p1.x + k * (p2.x - p0.x), p1.y + k * (p2.y - p0.y));
            let c2 = PointF::new(p2.x - k * (p3.x - p1.x), p2.y - k * (p3.y - p1.y));
            (c1, c2, p2)
        })
        .collect()
}

/// An SVG-style arc cut into points (the end point included), after SVG's endpoint-to-centre
/// conversion (radii grown when too small to reach).
#[allow(clippy::too_many_arguments)]
fn flatten_arc(from: PointF, to: PointF, rx: f32, ry: f32, rotation: f32, large: bool, clockwise: bool, tol: f32) -> Vec<PointF> {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-6 || ry < 1e-6 || from.distance(to) < 1e-6 {
        return vec![to];
    }
    let (sin_phi, cos_phi) = rotation.to_radians().sin_cos();
    let dx = (from.x - to.x) / 2.0;
    let dy = (from.y - to.y) / 2.0;
    let x1 = cos_phi * dx + sin_phi * dy;
    let y1 = -sin_phi * dx + cos_phi * dy;
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut coef = if den > 0.0 { (num / den).sqrt() } else { 0.0 };
    // On screen (y down), a clockwise sweep is SVG's sweep-flag = 1.
    if large == clockwise {
        coef = -coef;
    }
    let cx1 = coef * rx * y1 / ry;
    let cy1 = -coef * ry * x1 / rx;
    let cx = cos_phi * cx1 - sin_phi * cy1 + (from.x + to.x) / 2.0;
    let cy = sin_phi * cx1 + cos_phi * cy1 + (from.y + to.y) / 2.0;
    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let dot = ux * vx + uy * vy;
        let len = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
        let a = (dot / len).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            -a
        } else {
            a
        }
    };
    let theta1 = angle(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut delta = angle((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
    if clockwise && delta < 0.0 {
        delta += std::f32::consts::TAU;
    } else if !clockwise && delta > 0.0 {
        delta -= std::f32::consts::TAU;
    }
    let r = rx.max(ry);
    let step = if r > tol { 2.0 * (1.0 - tol / r).clamp(-1.0, 1.0).acos() } else { 0.5 };
    let n = ((delta.abs() / step.max(0.01)).ceil() as usize).clamp(2, 512);
    (1..=n)
        .map(|i| {
            let t = theta1 + delta * i as f32 / n as f32;
            let (st, ct) = t.sin_cos();
            PointF::new(cx + cos_phi * rx * ct - sin_phi * ry * st, cy + sin_phi * rx * ct + cos_phi * ry * st)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.2
    }

    #[test]
    fn rectangles_and_ellipses_are_closed_figures_with_the_right_bounds() {
        let mut p = GraphicsPath::new();
        p.add_rectangle(Rect::from_xywh(10.0, 10.0, 20.0, 10.0));
        p.add_ellipse(Rect::from_xywh(50.0, 0.0, 40.0, 20.0));
        assert_eq!(p.figures.len(), 2);
        assert!(p.figures.iter().all(|f| f.closed));
        let b = p.bounds();
        assert!(near(b.left, 10.0) && near(b.top, 0.0) && near(b.right, 90.0) && near(b.bottom, 20.0));
        assert!(p.is_visible(PointF::new(15.0, 15.0)));
        assert!(p.is_visible(PointF::new(70.0, 10.0)));
        assert!(!p.is_visible(PointF::new(40.0, 5.0)));
    }

    #[test]
    fn fill_modes_differ_on_nested_figures() {
        let mut p = GraphicsPath::new();
        p.add_rectangle(Rect::from_xywh(0.0, 0.0, 100.0, 100.0));
        p.add_rectangle(Rect::from_xywh(25.0, 25.0, 50.0, 50.0));
        assert!(!p.is_visible(PointF::new(50.0, 50.0)), "even-odd: the inner square is a hole");
        let w = p.clone().with_fill_mode(FillMode::Winding);
        assert!(w.is_visible(PointF::new(50.0, 50.0)), "non-zero: same direction, filled");
    }

    #[test]
    fn arcs_follow_gdiplus_angles_and_join_the_current_figure() {
        let r = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let e = ellipse_point(r, 90.0);
        assert!(near(e.x, 50.0) && near(e.y, 100.0), "90° is 6 o'clock (y down)");
        let mut p = GraphicsPath::new();
        p.add_line(PointF::new(0.0, 50.0), PointF::new(10.0, 50.0));
        p.add_arc(r, 180.0, 180.0);
        assert_eq!(p.figures.len(), 1, "an arc continues the figure");
        let flat = p.flatten(0.25);
        let pts = &flat[0].0;
        // Through the top (y = 0) and ending at 3 o'clock.
        assert!(pts.iter().any(|q| near(q.y, 0.0) && near(q.x, 50.0)));
        let last = *pts.last().expect("points");
        assert!(near(last.x, 100.0) && near(last.y, 50.0));
    }

    #[test]
    fn pies_polygons_curves_and_transforms() {
        let mut pie = GraphicsPath::new();
        pie.add_pie(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), 0.0, 90.0);
        assert!(pie.is_visible(PointF::new(70.0, 70.0)));
        assert!(!pie.is_visible(PointF::new(30.0, 30.0)));

        let mut curve = GraphicsPath::new();
        curve.add_curve(&[PointF::new(0.0, 0.0), PointF::new(10.0, 10.0), PointF::new(20.0, 0.0)], 0.5);
        assert_eq!(curve.figures[0].segments.len(), 2);
        assert!(matches!(curve.figures[0].segments[0], Segment::Bezier(..)));

        let mut sq = GraphicsPath::new();
        sq.add_rectangle(Rect::from_xywh(0.0, 0.0, 10.0, 10.0));
        sq.transform(&Matrix::translation(5.0, 5.0));
        let b = sq.bounds();
        assert!(near(b.left, 5.0) && near(b.bottom, 15.0));
        assert_eq!(sq.point_count(), 4);
        assert_eq!(sq.last_point(), Some(PointF::new(5.0, 15.0)));
    }

    #[test]
    fn rounded_rectangles_clamp_their_radius() {
        let mut p = GraphicsPath::new();
        p.add_rounded_rectangle(Rect::from_xywh(0.0, 0.0, 20.0, 10.0), 50.0);
        let b = p.bounds();
        assert!(near(b.right, 20.0) && near(b.bottom, 10.0));
        assert!(!p.is_visible(PointF::new(0.5, 0.5)), "the corner is cut");
        assert!(p.is_visible(PointF::new(10.0, 5.0)));
    }
}
