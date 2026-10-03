//! The value types of the drawing API: [`Color`], [`PointF`], [`SizeF`], [`Matrix`] and the
//! [`RectExt`] helpers on the shared [`Rect`].

use kubuno_drive_desktop_app_controls::Rect;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

/// A colour with straight (not premultiplied) alpha, each channel in `0..=1` — WinForms'
/// `System.Drawing.Color`, built from bytes (`Color::rgb(0x33, 0x66, 0xFF)`) or read from the
/// theme (`Color::from(theme.accent)`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    pub const BLACK: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Self = Self { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const RED: Self = Self { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const GREEN: Self = Self { r: 0.0, g: 0.5, b: 0.0, a: 1.0 };
    pub const BLUE: Self = Self { r: 0.0, g: 0.0, b: 1.0, a: 1.0 };
    pub const GRAY: Self = Self { r: 0.5, g: 0.5, b: 0.5, a: 1.0 };

    /// An opaque colour from bytes (`Color.FromArgb(r, g, b)`).
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
    }

    /// A colour from bytes with alpha first (`Color.FromArgb(a, r, g, b)`).
    pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> Self {
        Self { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: a as f32 / 255.0 }
    }

    /// A colour from `0..=1` channels.
    pub const fn rgba_f(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// `#RGB`, `#RRGGBB` or `#RRGGBBAA` (the `#` is optional); `None` for anything else.
    pub fn from_hex(text: &str) -> Option<Self> {
        let hex = text.trim().trim_start_matches('#');
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        match hex.len() {
            3 => {
                let n = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok().map(|v| v * 17);
                Some(Self::rgb(n(0)?, n(1)?, n(2)?))
            }
            6 => Some(Self::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Self::argb(byte(6)?, byte(0)?, byte(2)?, byte(4)?)),
            _ => None,
        }
    }

    /// The same colour with alpha `a` (`0..=1`).
    pub fn with_alpha(self, a: f32) -> Self {
        Self { a: a.clamp(0.0, 1.0), ..self }
    }

    /// The colour `t` of the way from `self` to `other` (straight-alpha interpolation).
    pub fn lerp(self, other: Color, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let m = |a: f32, b: f32| a + (b - a) * t;
        Self { r: m(self.r, other.r), g: m(self.g, other.g), b: m(self.b, other.b), a: m(self.a, other.a) }
    }

    /// The Direct2D colour.
    pub fn to_d2d(self) -> D2D1_COLOR_F {
        D2D1_COLOR_F { r: self.r, g: self.g, b: self.b, a: self.a }
    }

    /// Whether nothing would be painted with it.
    pub fn is_transparent(&self) -> bool {
        self.a <= 0.0
    }
}

impl From<D2D1_COLOR_F> for Color {
    fn from(c: D2D1_COLOR_F) -> Self {
        Self { r: c.r, g: c.g, b: c.b, a: c.a }
    }
}

impl From<&D2D1_COLOR_F> for Color {
    fn from(c: &D2D1_COLOR_F) -> Self {
        Self::from(*c)
    }
}

impl From<Color> for D2D1_COLOR_F {
    fn from(c: Color) -> Self {
        c.to_d2d()
    }
}

/// A point, in DIP (`PointF`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PointF {
    pub x: f32,
    pub y: f32,
}

impl PointF {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// The point moved by `(dx, dy)`.
    pub fn offset(self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy)
    }

    /// The distance to `other`.
    pub fn distance(self, other: PointF) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
}

impl From<(f32, f32)> for PointF {
    fn from((x, y): (f32, f32)) -> Self {
        Self::new(x, y)
    }
}

/// A size, in DIP (`SizeF`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SizeF {
    pub width: f32,
    pub height: f32,
}

impl SizeF {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// Helpers WinForms' `RectangleF` has, on the shared [`Rect`] (left/top/right/bottom).
pub trait RectExt: Sized {
    /// A rectangle from its origin and size (`new RectangleF(x, y, w, h)`).
    fn from_xywh(x: f32, y: f32, width: f32, height: f32) -> Rect;
    fn width(&self) -> f32;
    fn height(&self) -> f32;
    fn center(&self) -> PointF;
    fn location(&self) -> PointF;
    fn size(&self) -> SizeF;
    /// Grown by `dx` on the left and right and `dy` at the top and bottom (negative shrinks).
    fn inflated(&self, dx: f32, dy: f32) -> Rect;
    /// Moved by `(dx, dy)`.
    fn offset(&self, dx: f32, dy: f32) -> Rect;
    /// The overlap of two rectangles, `None` when they do not overlap.
    fn intersect(&self, other: &Rect) -> Option<Rect>;
    /// The smallest rectangle holding both.
    fn union(&self, other: &Rect) -> Rect;
    fn is_empty(&self) -> bool;
    fn contains_point(&self, p: PointF) -> bool;
}

impl RectExt for Rect {
    fn from_xywh(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect::new(x, y, x + width, y + height)
    }
    fn width(&self) -> f32 {
        self.right - self.left
    }
    fn height(&self) -> f32 {
        self.bottom - self.top
    }
    fn center(&self) -> PointF {
        PointF::new((self.left + self.right) / 2.0, (self.top + self.bottom) / 2.0)
    }
    fn location(&self) -> PointF {
        PointF::new(self.left, self.top)
    }
    fn size(&self) -> SizeF {
        SizeF::new(self.width(), self.height())
    }
    fn inflated(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.left - dx, self.top - dy, self.right + dx, self.bottom + dy)
    }
    fn offset(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.left + dx, self.top + dy, self.right + dx, self.bottom + dy)
    }
    fn intersect(&self, other: &Rect) -> Option<Rect> {
        let r = Rect::new(self.left.max(other.left), self.top.max(other.top), self.right.min(other.right), self.bottom.min(other.bottom));
        (r.right > r.left && r.bottom > r.top).then_some(r)
    }
    fn union(&self, other: &Rect) -> Rect {
        Rect::new(self.left.min(other.left), self.top.min(other.top), self.right.max(other.right), self.bottom.max(other.bottom))
    }
    fn is_empty(&self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }
    fn contains_point(&self, p: PointF) -> bool {
        self.contains(p.x, p.y)
    }
}

/// Whether a transform change is combined before or after the current one (`MatrixOrder`).
/// `Prepend` (WinForms' default) applies the new operation first, in the current local space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatrixOrder {
    #[default]
    Prepend,
    Append,
}

/// A 3×2 affine transform (`System.Drawing.Drawing2D.Matrix`), row-vector convention like GDI+ and
/// Direct2D: `x' = x·m11 + y·m21 + dx`, `y' = x·m12 + y·m22 + dy`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix {
    pub m11: f32,
    pub m12: f32,
    pub m21: f32,
    pub m22: f32,
    pub dx: f32,
    pub dy: f32,
}

impl Default for Matrix {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Matrix {
    pub const IDENTITY: Self = Self { m11: 1.0, m12: 0.0, m21: 0.0, m22: 1.0, dx: 0.0, dy: 0.0 };

    pub const fn new(m11: f32, m12: f32, m21: f32, m22: f32, dx: f32, dy: f32) -> Self {
        Self { m11, m12, m21, m22, dx, dy }
    }

    pub const fn translation(dx: f32, dy: f32) -> Self {
        Self { m11: 1.0, m12: 0.0, m21: 0.0, m22: 1.0, dx, dy }
    }

    pub const fn scaling(sx: f32, sy: f32) -> Self {
        Self { m11: sx, m12: 0.0, m21: 0.0, m22: sy, dx: 0.0, dy: 0.0 }
    }

    /// A rotation by `degrees`, clockwise on screen (y points down), around the origin.
    pub fn rotation(degrees: f32) -> Self {
        let (s, c) = degrees.to_radians().sin_cos();
        Self { m11: c, m12: s, m21: -s, m22: c, dx: 0.0, dy: 0.0 }
    }

    /// A rotation by `degrees` around `center` (`Matrix.RotateAt`).
    pub fn rotation_at(degrees: f32, center: PointF) -> Self {
        Self::translation(-center.x, -center.y).then(&Self::rotation(degrees)).then(&Self::translation(center.x, center.y))
    }

    /// A shear (`Matrix.Shear`).
    pub const fn shearing(shx: f32, shy: f32) -> Self {
        Self { m11: 1.0, m12: shy, m21: shx, m22: 1.0, dx: 0.0, dy: 0.0 }
    }

    /// `self` applied first, then `next`.
    pub fn then(&self, next: &Matrix) -> Matrix {
        let a = self;
        let b = next;
        Matrix {
            m11: a.m11 * b.m11 + a.m12 * b.m21,
            m12: a.m11 * b.m12 + a.m12 * b.m22,
            m21: a.m21 * b.m11 + a.m22 * b.m21,
            m22: a.m21 * b.m12 + a.m22 * b.m22,
            dx: a.dx * b.m11 + a.dy * b.m21 + b.dx,
            dy: a.dx * b.m12 + a.dy * b.m22 + b.dy,
        }
    }

    /// Combines `other` with this matrix in `order` (`Matrix.Multiply`).
    pub fn multiply(&mut self, other: &Matrix, order: MatrixOrder) {
        *self = match order {
            MatrixOrder::Prepend => other.then(self),
            MatrixOrder::Append => self.then(other),
        };
    }

    pub fn translate(&mut self, dx: f32, dy: f32, order: MatrixOrder) {
        self.multiply(&Self::translation(dx, dy), order);
    }

    pub fn scale(&mut self, sx: f32, sy: f32, order: MatrixOrder) {
        self.multiply(&Self::scaling(sx, sy), order);
    }

    pub fn rotate(&mut self, degrees: f32, order: MatrixOrder) {
        self.multiply(&Self::rotation(degrees), order);
    }

    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }

    /// Whether it keeps rectangles axis-aligned (translation and scaling only).
    pub fn is_axis_aligned(&self) -> bool {
        self.m12 == 0.0 && self.m21 == 0.0
    }

    pub fn determinant(&self) -> f32 {
        self.m11 * self.m22 - self.m12 * self.m21
    }

    /// The inverse, `None` when the matrix is singular (`Matrix.Invert`).
    pub fn inverted(&self) -> Option<Matrix> {
        let det = self.determinant();
        if det.abs() < 1e-12 {
            return None;
        }
        let inv = 1.0 / det;
        let m11 = self.m22 * inv;
        let m12 = -self.m12 * inv;
        let m21 = -self.m21 * inv;
        let m22 = self.m11 * inv;
        let dx = -(self.dx * m11 + self.dy * m21);
        let dy = -(self.dx * m12 + self.dy * m22);
        Some(Matrix { m11, m12, m21, m22, dx, dy })
    }

    /// Where `p` goes (`Matrix.TransformPoints`).
    pub fn transform_point(&self, p: PointF) -> PointF {
        PointF::new(p.x * self.m11 + p.y * self.m21 + self.dx, p.x * self.m12 + p.y * self.m22 + self.dy)
    }

    /// A vector (no translation) (`Matrix.TransformVectors`).
    pub fn transform_vector(&self, p: PointF) -> PointF {
        PointF::new(p.x * self.m11 + p.y * self.m21, p.x * self.m12 + p.y * self.m22)
    }

    /// The bounding box of `r` once transformed.
    pub fn transform_bounds(&self, r: &Rect) -> Rect {
        let pts = [
            self.transform_point(PointF::new(r.left, r.top)),
            self.transform_point(PointF::new(r.right, r.top)),
            self.transform_point(PointF::new(r.right, r.bottom)),
            self.transform_point(PointF::new(r.left, r.bottom)),
        ];
        let (mut l, mut t, mut rr, mut b) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in pts {
            l = l.min(p.x);
            t = t.min(p.y);
            rr = rr.max(p.x);
            b = b.max(p.y);
        }
        Rect::new(l, t, rr, b)
    }

    /// The average linear scale (what a stroke width or a font size is multiplied by).
    pub fn mean_scale(&self) -> f32 {
        self.determinant().abs().sqrt()
    }

    /// The Direct2D matrix.
    pub fn to_d2d(&self) -> windows_numerics::Matrix3x2 {
        windows_numerics::Matrix3x2 { M11: self.m11, M12: self.m12, M21: self.m21, M22: self.m22, M31: self.dx, M32: self.dy }
    }

    pub fn from_d2d(m: &windows_numerics::Matrix3x2) -> Self {
        Self { m11: m.M11, m12: m.M12, m21: m.M21, m22: m.M22, dx: m.M31, dy: m.M32 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: PointF, b: PointF) -> bool {
        (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4
    }

    #[test]
    fn prepend_applies_the_new_operation_first_like_gdiplus() {
        // GDI+: TranslateTransform(10, 0) then ScaleTransform(2, 2), both Prepend: a point is scaled,
        // then translated.
        let mut m = Matrix::IDENTITY;
        m.translate(10.0, 0.0, MatrixOrder::Prepend);
        m.scale(2.0, 2.0, MatrixOrder::Prepend);
        assert!(close(m.transform_point(PointF::new(1.0, 1.0)), PointF::new(12.0, 2.0)));
        let mut a = Matrix::IDENTITY;
        a.translate(10.0, 0.0, MatrixOrder::Append);
        a.scale(2.0, 2.0, MatrixOrder::Append);
        assert!(close(a.transform_point(PointF::new(1.0, 1.0)), PointF::new(22.0, 2.0)));
    }

    #[test]
    fn rotation_is_clockwise_on_screen_and_invertible() {
        let r = Matrix::rotation(90.0);
        assert!(close(r.transform_point(PointF::new(1.0, 0.0)), PointF::new(0.0, 1.0)));
        let at = Matrix::rotation_at(180.0, PointF::new(5.0, 5.0));
        assert!(close(at.transform_point(PointF::new(0.0, 0.0)), PointF::new(10.0, 10.0)));
        let inv = at.inverted().expect("invertible");
        assert!(close(inv.transform_point(PointF::new(10.0, 10.0)), PointF::new(0.0, 0.0)));
        assert!(Matrix::scaling(0.0, 1.0).inverted().is_none());
    }

    #[test]
    fn colours_parse_from_hex_and_convert() {
        assert_eq!(Color::from_hex("#336699"), Some(Color::rgb(0x33, 0x66, 0x99)));
        assert_eq!(Color::from_hex("f00"), Some(Color::rgb(255, 0, 0)));
        assert_eq!(Color::from_hex("#00000080").map(|c| (c.a * 255.0).round()), Some(128.0));
        assert!(Color::from_hex("#12").is_none());
        let d: D2D1_COLOR_F = Color::WHITE.into();
        assert_eq!(Color::from(d), Color::WHITE);
        assert_eq!(Color::BLACK.lerp(Color::WHITE, 0.5).r, 0.5);
    }

    #[test]
    fn rect_helpers() {
        let r = Rect::from_xywh(10.0, 20.0, 30.0, 40.0);
        assert_eq!((r.width(), r.height()), (30.0, 40.0));
        assert_eq!(r.center(), PointF::new(25.0, 40.0));
        assert!(r.intersect(&Rect::from_xywh(100.0, 0.0, 5.0, 5.0)).is_none());
        let u = r.union(&Rect::from_xywh(0.0, 0.0, 1.0, 1.0));
        assert_eq!((u.left, u.top, u.right, u.bottom), (0.0, 0.0, 40.0, 60.0));
        let b = Matrix::rotation(90.0).transform_bounds(&Rect::from_xywh(0.0, 0.0, 10.0, 20.0));
        assert!((b.width() - 20.0).abs() < 1e-4 && (b.height() - 10.0).abs() < 1e-4);
    }
}
