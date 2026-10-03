//! The splash screen's artwork: procedural, vector, drawn with Direct2D — nothing but code.
//!
//! One artwork per application, in the colour family of its web logo
//! (`core/frontend/public/<module>-logo.png`), and one composition for all of them so they read
//! as a family:
//!
//! * a deep diagonal ground in the module's colours, lit by soft blooms;
//! * a motif of the module, in translucent layers: the Kubuno aperture rings and floating cubes
//!   (the shell), stacked storage slabs and data streams (Drive), ripples and speech bubbles
//!   (Chat), fanned pages and a sweeping ribbon (Documents);
//! * light beams and out-of-focus specks;
//! * the hero: the module's mark, drawn large as vectors after the web logo, with its glow, a
//!   gloss, a rim light and its shadow on the ground;
//! * the type on the left — the Kubuno mark and eyebrow, the product name set large, its tagline,
//!   version, and at the foot the live status line, a thin progress bar, the legal and credits
//!   lines.
//!
//! Everything is laid out in a fixed 800 × 500 design space ([`Layout`]) and scaled, so the same
//! painter draws the splash window (at its monitor's DPI) and the Visual Studio designer's
//! preview of a `<SplashArtwork>` (at any size). Desktop type is Segoe UI Variable (Plus Jakarta
//! Sans stays on the web).

use windows::core::{Interface, Result, HSTRING};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_WINDING, D2D1_GRADIENT_STOP,
    D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, ID2D1Factory, ID2D1Geometry, ID2D1GradientStopCollection, ID2D1PathGeometry, ID2D1RenderTarget, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_COMBINE_MODE, D2D1_COMBINE_MODE_EXCLUDE, D2D1_COMBINE_MODE_INTERSECT, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2,
    D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_QUADRATIC_BEZIER_SEGMENT, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES,
    D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextLayout1, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_LIGHT, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_RANGE, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows_numerics::{Matrix3x2, Vector2};

use super::SplashContent;
use crate::Rect;

/// The design width of the card, in DIP.
pub const WIDTH: f32 = 800.0;
/// The design height of the card, in DIP.
pub const HEIGHT: f32 = 500.0;
/// The card's corner radius (Windows 11's large-window radius, a little rounder).
pub const RADIUS: f32 = 12.0;

/// The type: Segoe UI Variable's optical sizes (the desktop's face; DirectWrite falls back to
/// Segoe UI where they are missing).
const DISPLAY: &str = "Segoe UI Variable Display";
const TEXT: &str = "Segoe UI Variable Text";

/// Which application's artwork.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Artwork {
    /// The shell (`kubuno-desktop`): the Kubuno mark — aperture rings and floating cubes.
    Kubuno,
    /// Drive: stacked storage slabs and data streams, amber status lights.
    Drive,
    /// Chat: ripples and speech bubbles in sky and teal.
    Chat,
    /// Documents (Office): fanned pages and a sweeping ribbon in azure.
    Documents,
}

impl Artwork {
    pub const ALL: [Artwork; 4] = [Artwork::Kubuno, Artwork::Drive, Artwork::Chat, Artwork::Documents];

    /// Its name in a `.kbview` (`Artwork="Drive"`).
    pub fn name(self) -> &'static str {
        match self {
            Artwork::Kubuno => "Kubuno",
            Artwork::Drive => "Drive",
            Artwork::Chat => "Chat",
            Artwork::Documents => "Documents",
        }
    }

    /// The artwork named `name` (case-insensitive; « Desktop » and « Shell » are the Kubuno one,
    /// « Office » the Documents one).
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "kubuno" | "desktop" | "shell" => Some(Artwork::Kubuno),
            "drive" | "files" => Some(Artwork::Drive),
            "chat" => Some(Artwork::Chat),
            "documents" | "office" | "docs" => Some(Artwork::Documents),
            _ => None,
        }
    }

    /// The product it stands for.
    pub fn product(self) -> &'static str {
        match self {
            Artwork::Kubuno => "Kubuno Desktop",
            Artwork::Drive => "Kubuno Drive",
            Artwork::Chat => "Kubuno Chat",
            Artwork::Documents => "Kubuno Documents",
        }
    }

    /// Its default tagline.
    pub fn tagline(self) -> &'static str {
        match self {
            Artwork::Kubuno => "Votre cloud souverain, à portée de bureau.",
            Artwork::Drive => "Vos fichiers, partout, synchronisés chez vous.",
            Artwork::Chat => "Messages et réunions, sans intermédiaire.",
            Artwork::Documents => "Écrire, mettre en page, partager.",
        }
    }

    /// Its colours.
    pub fn palette(self) -> Palette {
        match self {
            // The Kubuno logo's blue (#2563EB) and its cube's lavender, with a violet spark.
            Artwork::Kubuno => Palette { ink: 0x050A1F, deep: 0x0B1645, mid: 0x1D3DB8, accent: 0x2563EB, light: 0xA5B8FB, spark: 0x8B5CF6 },
            // The Drive logo: cobalt slabs, sky line, amber lights.
            Artwork::Drive => Palette { ink: 0x030B20, deep: 0x07194A, mid: 0x1547C9, accent: 0x2563EB, light: 0x7DD3FC, spark: 0xFACC15 },
            // The Chat logo's sky hexagon, with cyan and teal.
            Artwork::Chat => Palette { ink: 0x021420, deep: 0x05283E, mid: 0x0369A1, accent: 0x0EA5E9, light: 0x67E8F9, spark: 0x2DD4BF },
            // The Documents logo: azure hexagon, light-blue page, navy badge.
            Artwork::Documents => Palette { ink: 0x020D22, deep: 0x062049, mid: 0x0A4F9E, accent: 0x0078D4, light: 0x78B0F0, spark: 0xD6E6FB },
        }
    }
}

/// The colours of an artwork, as `0xRRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// The darkest ground (behind the type).
    pub ink: u32,
    /// The ground.
    pub deep: u32,
    /// The lit ground (toward the hero).
    pub mid: u32,
    /// The module's accent (its logo's main colour).
    pub accent: u32,
    /// The light tint (lines, glows).
    pub light: u32,
    /// The second accent (sparks, packets).
    pub spark: u32,
}

/// Where everything goes in the 800 × 500 design space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub card: Rect,
    /// The small Kubuno mark, top left.
    pub mark: Rect,
    /// « KUBUNO », beside it.
    pub eyebrow: Rect,
    /// « Kubuno », light, above the product name.
    pub lead: Rect,
    /// The product name, large.
    pub title: Rect,
    pub tagline: Rect,
    pub version: Rect,
    pub status: Rect,
    /// The progress bar's track.
    pub progress: Rect,
    pub legal: Rect,
    pub credits: Rect,
    /// The hero mark's centre and size.
    pub hero: (f32, f32),
    pub hero_size: f32,
}

impl Layout {
    /// The one layout of the splash.
    pub fn standard() -> Self {
        Self {
            card: Rect::new(0.0, 0.0, WIDTH, HEIGHT),
            mark: Rect::new(48.0, 42.0, 78.0, 72.0),
            eyebrow: Rect::new(88.0, 42.0, 380.0, 72.0),
            lead: Rect::new(47.0, 138.0, 468.0, 176.0),
            title: Rect::new(44.0, 166.0, 470.0, 256.0),
            tagline: Rect::new(48.0, 262.0, 470.0, 286.0),
            version: Rect::new(48.0, 294.0, 470.0, 314.0),
            status: Rect::new(48.0, 382.0, 460.0, 402.0),
            progress: Rect::new(48.0, 412.0, 460.0, 414.5),
            legal: Rect::new(48.0, 434.0, 620.0, 450.0),
            credits: Rect::new(48.0, 452.0, 700.0, 468.0),
            hero: (606.0, 232.0),
            hero_size: 250.0,
        }
    }

    /// The hero's square.
    pub fn hero_rect(&self) -> Rect {
        let h = self.hero_size / 2.0;
        Rect::new(self.hero.0 - h, self.hero.1 - h, self.hero.0 + h, self.hero.1 + h)
    }
}

/// The colour `0xRRGGBB` at `alpha`.
pub(crate) fn rgba(hex: u32, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((hex >> 16) & 0xFF) as f32 / 255.0,
        g: ((hex >> 8) & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a: alpha.clamp(0.0, 1.0),
    }
}

const WHITE: u32 = 0xFFFFFF;
const BLACK: u32 = 0x000000;

fn v(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn rect_f(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
}

// ── Geometry, pure ───────────────────────────────────────────────────────────

/// One segment of a figure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Seg {
    Line(f32, f32),
    Quad((f32, f32), (f32, f32)),
    Cubic((f32, f32), (f32, f32), (f32, f32)),
}

/// A figure of a path.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Figure {
    pub start: (f32, f32),
    pub segs: Vec<Seg>,
    pub closed: bool,
}

/// The six corners of a hexagon of radius `r` (pointy top, as the Kubuno marks).
pub(crate) fn hexagon(cx: f32, cy: f32, r: f32) -> Vec<(f32, f32)> {
    (0..6)
        .map(|k| {
            let a = (-90.0 + 60.0 * k as f32).to_radians();
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

/// A closed polygon with its corners rounded by `radius` (quadratic corners, each limited to half
/// of its shorter edge).
pub(crate) fn rounded_polygon(points: &[(f32, f32)], radius: f32) -> Figure {
    let n = points.len();
    if n < 3 || radius <= 0.0 {
        let start = points.first().copied().unwrap_or((0.0, 0.0));
        return Figure { start, segs: points.iter().skip(1).map(|&(x, y)| Seg::Line(x, y)).collect(), closed: true };
    }
    let corner = |i: usize| {
        let p = points[i];
        let prev = points[(i + n - 1) % n];
        let next = points[(i + 1) % n];
        let toward = |q: (f32, f32)| {
            let (dx, dy) = (q.0 - p.0, q.1 - p.1);
            let len = (dx * dx + dy * dy).sqrt().max(1e-6);
            let r = radius.min(len / 2.0);
            (p.0 + dx / len * r, p.1 + dy / len * r)
        };
        (toward(prev), p, toward(next))
    };
    let (_, _, first_out) = corner(0);
    let mut segs = Vec::with_capacity(2 * n);
    for i in 1..=n {
        let (into, at, out) = corner(i % n);
        segs.push(Seg::Line(into.0, into.1));
        segs.push(Seg::Quad(at, out));
    }
    Figure { start: first_out, segs, closed: true }
}

/// A rounded rectangle as a figure.
pub(crate) fn rounded_rect(r: Rect, radius: f32) -> Figure {
    rounded_polygon(&[(r.left, r.top), (r.right, r.top), (r.right, r.bottom), (r.left, r.bottom)], radius)
}

/// The quadrilateral of a segment `width` thick, from `a` to `b`.
pub(crate) fn thick_segment(a: (f32, f32), b: (f32, f32), width: f32) -> Vec<(f32, f32)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    let (nx, ny) = (-dy / len * width / 2.0, dx / len * width / 2.0);
    vec![(a.0 + nx, a.1 + ny), (b.0 + nx, b.1 + ny), (b.0 - nx, b.1 - ny), (a.0 - nx, a.1 - ny)]
}

/// The point at `t` of the cubic Bézier `p0 p1 p2 p3`.
pub(crate) fn bezier_point(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32), t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    (a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1)
}

/// A small deterministic random sequence (the specks must not move between two frames).
pub(crate) struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Self(seed.wrapping_mul(2_654_435_761).wrapping_add(1))
    }
    /// The next value in `0..1`.
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
}

// ── The Direct2D pen ─────────────────────────────────────────────────────────

/// Draws on a render target in the design space: a root transform (design → target) on top of
/// whatever transform the target already had.
pub(crate) struct Pen<'a> {
    rt: &'a ID2D1RenderTarget,
    factory: ID2D1Factory,
    dwrite: IDWriteFactory,
    /// The target's own transform when the pen was made (restored by `Drop`).
    base: Matrix3x2,
    root: std::cell::Cell<Matrix3x2>,
}

impl<'a> Pen<'a> {
    /// A pen drawing the design space at `scale`, its origin at `origin` of the target's space.
    pub fn new(rt: &'a ID2D1RenderTarget, origin: (f32, f32), scale: f32) -> Result<Self> {
        // SAFETY: plain queries of a live render target and the shared DirectWrite factory.
        unsafe {
            let factory = rt.GetFactory()?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let mut base = Matrix3x2::identity();
            rt.GetTransform(&mut base);
            let root = Matrix3x2::scale(scale, scale) * Matrix3x2::translation(origin.0, origin.1) * base;
            rt.SetTransform(&root);
            Ok(Self { rt, factory, dwrite, base, root: std::cell::Cell::new(root) })
        }
    }

    /// Draws in a local space `m` (applied before the root).
    fn local(&self, m: Matrix3x2) {
        // SAFETY: a transform change on the live target.
        unsafe { self.rt.SetTransform(&(m * self.root.get())) }
    }

    fn reset(&self) {
        // SAFETY: as above.
        unsafe { self.rt.SetTransform(&self.root.get()) }
    }

    fn stops(&self, stops: &[(f32, D2D1_COLOR_F)]) -> Result<ID2D1GradientStopCollection> {
        let stops: Vec<D2D1_GRADIENT_STOP> = stops.iter().map(|&(position, color)| D2D1_GRADIENT_STOP { position, color }).collect();
        // SAFETY: a resource of the live target.
        unsafe { self.rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) }
    }

    fn solid(&self, color: D2D1_COLOR_F) -> Result<ID2D1Brush> {
        // SAFETY: as above.
        unsafe { self.rt.CreateSolidColorBrush(&color, None)?.cast() }
    }

    fn linear(&self, from: (f32, f32), to: (f32, f32), stops: &[(f32, D2D1_COLOR_F)]) -> Result<ID2D1Brush> {
        let collection = self.stops(stops)?;
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: v(from.0, from.1), endPoint: v(to.0, to.1) };
        // SAFETY: as above.
        unsafe { self.rt.CreateLinearGradientBrush(&props, None, &collection)?.cast() }
    }

    fn radial(&self, center: (f32, f32), rx: f32, ry: f32, stops: &[(f32, D2D1_COLOR_F)]) -> Result<ID2D1Brush> {
        let collection = self.stops(stops)?;
        let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: v(center.0, center.1), gradientOriginOffset: v(0.0, 0.0), radiusX: rx, radiusY: ry };
        // SAFETY: as above.
        unsafe { self.rt.CreateRadialGradientBrush(&props, None, &collection)?.cast() }
    }

    fn path(&self, figures: &[Figure]) -> Result<ID2D1Geometry> {
        // SAFETY: building a path geometry with the target's own factory.
        unsafe {
            let path: ID2D1PathGeometry = self.factory.CreatePathGeometry()?;
            let sink = path.Open()?;
            sink.SetFillMode(D2D1_FILL_MODE_WINDING);
            for f in figures {
                sink.BeginFigure(v(f.start.0, f.start.1), D2D1_FIGURE_BEGIN_FILLED);
                for s in &f.segs {
                    match *s {
                        Seg::Line(x, y) => sink.AddLine(v(x, y)),
                        Seg::Quad(c, e) => sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT { point1: v(c.0, c.1), point2: v(e.0, e.1) }),
                        Seg::Cubic(a, b, e) => sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: v(a.0, a.1), point2: v(b.0, b.1), point3: v(e.0, e.1) }),
                    }
                }
                sink.EndFigure(if f.closed { D2D1_FIGURE_END_CLOSED } else { D2D1_FIGURE_END_OPEN });
            }
            sink.Close()?;
            path.cast()
        }
    }

    fn polygon(&self, points: &[(f32, f32)], radius: f32) -> Result<ID2D1Geometry> {
        self.path(&[rounded_polygon(points, radius)])
    }

    /// `a` combined with `b` (exclude, intersect…).
    fn combine(&self, a: &ID2D1Geometry, b: &ID2D1Geometry, mode: D2D1_COMBINE_MODE) -> Result<ID2D1Geometry> {
        // SAFETY: as above; the flattening tolerance is fine enough for a mark drawn 300 px tall.
        unsafe {
            let path: ID2D1PathGeometry = self.factory.CreatePathGeometry()?;
            let sink = path.Open()?;
            a.CombineWithGeometry(b, mode, None, 0.01, &sink)?;
            sink.Close()?;
            path.cast()
        }
    }

    fn fill(&self, geometry: &ID2D1Geometry, brush: &ID2D1Brush) {
        // SAFETY: drawing on the live target, inside its BeginDraw.
        unsafe { self.rt.FillGeometry(geometry, brush, None) }
    }

    fn stroke(&self, geometry: &ID2D1Geometry, brush: &ID2D1Brush, width: f32) {
        // SAFETY: as above.
        unsafe { self.rt.DrawGeometry(geometry, brush, width, None) }
    }

    fn fill_rect(&self, r: Rect, brush: &ID2D1Brush) {
        // SAFETY: as above.
        unsafe { self.rt.FillRectangle(&rect_f(r), brush) }
    }

    fn fill_rounded(&self, r: Rect, radius: f32, brush: &ID2D1Brush) {
        let rr = D2D1_ROUNDED_RECT { rect: rect_f(r), radiusX: radius, radiusY: radius };
        // SAFETY: as above.
        unsafe { self.rt.FillRoundedRectangle(&rr, brush) }
    }

    fn fill_ellipse(&self, c: (f32, f32), rx: f32, ry: f32, brush: &ID2D1Brush) {
        let e = D2D1_ELLIPSE { point: v(c.0, c.1), radiusX: rx, radiusY: ry };
        // SAFETY: as above.
        unsafe { self.rt.FillEllipse(&e, brush) }
    }

    fn stroke_ellipse(&self, c: (f32, f32), rx: f32, ry: f32, brush: &ID2D1Brush, width: f32) {
        let e = D2D1_ELLIPSE { point: v(c.0, c.1), radiusX: rx, radiusY: ry };
        // SAFETY: as above.
        unsafe { self.rt.DrawEllipse(&e, brush, width, None) }
    }

    /// A soft round glow: a radial gradient from `color` at `alpha` to nothing.
    fn glow(&self, c: (f32, f32), rx: f32, ry: f32, color: u32, alpha: f32) -> Result<()> {
        let b = self.radial(c, rx, ry, &[(0.0, rgba(color, alpha)), (0.45, rgba(color, alpha * 0.42)), (1.0, rgba(color, 0.0))])?;
        self.fill_ellipse(c, rx, ry, &b);
        Ok(())
    }

    /// Clips what follows to `geometry` until [`pop`](Self::pop).
    fn push_clip(&self, geometry: &ID2D1Geometry) {
        let params = D2D1_LAYER_PARAMETERS {
            contentBounds: D2D_RECT_F { left: f32::NEG_INFINITY, top: f32::NEG_INFINITY, right: f32::INFINITY, bottom: f32::INFINITY },
            geometricMask: std::mem::ManuallyDrop::new(Some(geometry.clone())),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: std::mem::ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS_NONE,
        };
        // SAFETY: as above; the layer is popped by `pop`.
        unsafe { self.rt.PushLayer(&params, None) };
        let D2D1_LAYER_PARAMETERS { geometricMask, .. } = params;
        drop(std::mem::ManuallyDrop::into_inner(geometricMask));
    }

    fn pop(&self) {
        // SAFETY: pops the layer `push_clip` pushed.
        unsafe { self.rt.PopLayer() }
    }

    /// One line (or a few) of text in `r`.
    fn text(&self, s: &str, r: Rect, style: &TextStyle) -> Result<()> {
        if s.is_empty() {
            return Ok(());
        }
        // SAFETY: DirectWrite objects made and used right here, drawn on the live target.
        unsafe {
            let format = self.dwrite.CreateTextFormat(
                &HSTRING::from(style.family),
                None,
                style.weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                style.size,
                &HSTRING::from("fr-fr"),
            )?;
            format.SetTextAlignment(if style.centered { DWRITE_TEXT_ALIGNMENT_CENTER } else { DWRITE_TEXT_ALIGNMENT_LEADING })?;
            format.SetParagraphAlignment(if style.middle { DWRITE_PARAGRAPH_ALIGNMENT_CENTER } else { DWRITE_PARAGRAPH_ALIGNMENT_NEAR })?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            if style.ellipsis {
                let sign = self.dwrite.CreateEllipsisTrimmingSign(&format)?;
                let trimming = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
                format.SetTrimming(&trimming, &sign)?;
            }
            let wide: Vec<u16> = s.encode_utf16().collect();
            let layout = self.dwrite.CreateTextLayout(&wide, &format, (r.right - r.left).max(1.0), (r.bottom - r.top).max(1.0))?;
            if style.tracking != 0.0 {
                if let Ok(l1) = layout.cast::<IDWriteTextLayout1>() {
                    let _ = l1.SetCharacterSpacing(0.0, style.tracking, 0.0, DWRITE_TEXT_RANGE { startPosition: 0, length: wide.len() as u32 });
                }
            }
            let brush = self.solid(style.color)?;
            self.rt.DrawTextLayout(v(r.left, r.top), &layout, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
        }
        Ok(())
    }
}

impl Drop for Pen<'_> {
    fn drop(&mut self) {
        // SAFETY: gives the target back the transform it had.
        unsafe { self.rt.SetTransform(&self.base) }
    }
}

struct TextStyle {
    family: &'static str,
    size: f32,
    weight: DWRITE_FONT_WEIGHT,
    color: D2D1_COLOR_F,
    /// Extra space after each character, DIP.
    tracking: f32,
    centered: bool,
    middle: bool,
    ellipsis: bool,
}

impl TextStyle {
    fn new(family: &'static str, size: f32, weight: DWRITE_FONT_WEIGHT, color: D2D1_COLOR_F) -> Self {
        Self { family, size, weight, color, tracking: 0.0, centered: false, middle: false, ellipsis: false }
    }
}

// ── The painting ─────────────────────────────────────────────────────────────

/// Which parts of the splash to paint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Parts {
    /// The card: ground, motif, hero, type — all but the status line and the progress bar.
    Static,
    /// The status line and the progress bar (`progress` as displayed; `phase` drives the
    /// indeterminate shimmer, `0..1`).
    Dynamic { progress: Option<f32>, phase: f32 },
    /// Both: a still of the whole splash (the designer's preview).
    All { progress: Option<f32>, phase: f32 },
}

/// Paints `content` on `rt`, the design space placed at `origin` and scaled by `scale` (in the
/// target's current coordinates). The card is clipped to its rounded corners; nothing is drawn
/// outside it.
pub fn paint(rt: &ID2D1RenderTarget, origin: (f32, f32), scale: f32, content: &SplashContent, parts: Parts) -> Result<()> {
    let pen = Pen::new(rt, origin, scale)?;
    let layout = Layout::standard();
    let palette = content.artwork.palette();
    if matches!(parts, Parts::Static | Parts::All { .. }) {
        let card = pen.path(&[rounded_rect(layout.card, RADIUS)])?;
        pen.push_clip(&card);
        let drawn = paint_card(&pen, &layout, palette, content);
        pen.pop();
        drawn?;
        // The crisp edge: a hairline of light inside the card's rim, brighter along the top.
        let edge = pen.linear((0.0, 0.0), (0.0, HEIGHT), &[(0.0, rgba(WHITE, 0.26)), (0.25, rgba(WHITE, 0.10)), (1.0, rgba(WHITE, 0.06))])?;
        let inner = pen.path(&[rounded_rect(Rect::new(0.5, 0.5, WIDTH - 0.5, HEIGHT - 0.5), RADIUS - 0.5)])?;
        pen.stroke(&inner, &edge, 1.0);
    }
    if let Parts::Dynamic { progress, phase } | Parts::All { progress, phase } = parts {
        paint_status(&pen, &layout, palette, content, progress, phase)?;
    }
    Ok(())
}

/// The soft shadow and dark rim around a card placed at `origin` (the window paints it on its
/// transparent margin, under the card).
pub fn paint_shadow(rt: &ID2D1RenderTarget, origin: (f32, f32), scale: f32) -> Result<()> {
    let pen = Pen::new(rt, origin, scale)?;
    // An ambient shadow: rounded rectangles growing outward, each a little fainter (a Gaussian
    // falloff, without an effect graph), set lower than the card as if lit from above.
    let steps = 22;
    let brush = pen.solid(rgba(BLACK, 0.0))?;
    for i in (0..steps).rev() {
        let t = i as f32 / steps as f32;
        let grow = 1.0 + i as f32 * 1.25;
        let alpha = 0.030 * (1.0 - t).powf(1.7);
        let r = Rect::new(-grow, -grow + 9.0 + t * 4.0, WIDTH + grow, HEIGHT + grow + 9.0 + t * 4.0);
        // SAFETY: a brush of the live target.
        unsafe {
            if let Ok(b) = brush.cast::<windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush>() {
                b.SetColor(&rgba(BLACK, alpha));
            }
        }
        pen.fill_rounded(r, RADIUS + grow, &brush);
    }
    // The contact shadow, tight under the card.
    let contact = pen.solid(rgba(BLACK, 0.22))?;
    pen.fill_rounded(Rect::new(-1.0, 0.0, WIDTH + 1.0, HEIGHT + 2.5), RADIUS + 1.0, &contact);
    // A dark hairline outside the card: its edge stays crisp on a light desktop.
    let rim = pen.solid(rgba(BLACK, 0.38))?;
    let outer = pen.path(&[rounded_rect(Rect::new(-0.5, -0.5, WIDTH + 0.5, HEIGHT + 0.5), RADIUS + 0.5)])?;
    pen.stroke(&outer, &rim, 1.0);
    Ok(())
}

fn paint_card(pen: &Pen<'_>, l: &Layout, p: Palette, content: &SplashContent) -> Result<()> {
    background(pen, l, p)?;
    match content.artwork {
        Artwork::Kubuno => motif_kubuno(pen, l, p)?,
        Artwork::Drive => motif_drive(pen, l, p)?,
        Artwork::Chat => motif_chat(pen, l, p)?,
        Artwork::Documents => motif_documents(pen, l, p)?,
    }
    light(pen, l, p, content.artwork)?;
    hero(pen, l, p, content.artwork)?;
    scrims(pen, p)?;
    type_block(pen, l, p, content)?;
    Ok(())
}

/// The ground: a diagonal gradient, lit by three blooms.
fn background(pen: &Pen<'_>, l: &Layout, p: Palette) -> Result<()> {
    let ground = pen.linear((0.0, 0.0), (WIDTH, HEIGHT), &[(0.0, rgba(p.ink, 1.0)), (0.42, rgba(p.deep, 1.0)), (1.0, rgba(p.mid, 1.0))])?;
    pen.fill_rect(l.card, &ground);
    pen.glow(l.hero, 420.0, 380.0, p.accent, 0.80)?;
    pen.glow((WIDTH - 30.0, 10.0), 320.0, 260.0, p.light, 0.30)?;
    pen.glow((110.0, HEIGHT + 40.0), 400.0, 260.0, p.spark, 0.12)?;
    Ok(())
}

/// Light beams through the scene, and specks of light out of focus.
fn light(pen: &Pen<'_>, l: &Layout, p: Palette, art: Artwork) -> Result<()> {
    for (i, (x, width, alpha)) in [(560.0, 46.0, 0.10), (660.0, 22.0, 0.08), (742.0, 64.0, 0.06)].into_iter().enumerate() {
        pen.local(Matrix3x2::rotation_around(28.0 + i as f32 * 3.0, v(x, 0.0)));
        let beam = pen.linear((x, -40.0), (x, HEIGHT * 0.95), &[(0.0, rgba(p.light, alpha)), (0.55, rgba(p.light, alpha * 0.35)), (1.0, rgba(p.light, 0.0))])?;
        pen.fill_rect(Rect::new(x - width / 2.0, -60.0, x + width / 2.0, HEIGHT + 60.0), &beam);
    }
    pen.reset();
    let mut rng = Lcg::new(art as u32 + 7);
    for _ in 0..22 {
        let x = 400.0 + rng.next() * 400.0;
        let y = rng.next() * HEIGHT;
        let r = 1.2 + rng.next().powi(2) * 5.5;
        let a = 0.10 + rng.next() * 0.38;
        let color = if rng.next() > 0.75 { p.spark } else { p.light };
        // Keep the hero clear.
        let (dx, dy) = (x - l.hero.0, y - l.hero.1);
        if (dx * dx + dy * dy).sqrt() < l.hero_size * 0.62 {
            continue;
        }
        pen.glow((x, y), r * 2.2, r * 2.2, color, a)?;
    }
    Ok(())
}

/// Darkens the left, under the type, and the foot, under the status and legal lines.
fn scrims(pen: &Pen<'_>, p: Palette) -> Result<()> {
    let left = pen.linear((0.0, 0.0), (560.0, 0.0), &[(0.0, rgba(p.ink, 0.86)), (0.5, rgba(p.ink, 0.52)), (1.0, rgba(p.ink, 0.0))])?;
    pen.fill_rect(Rect::new(0.0, 0.0, 560.0, HEIGHT), &left);
    let foot = pen.linear((0.0, HEIGHT), (0.0, HEIGHT - 150.0), &[(0.0, rgba(p.ink, 0.62)), (1.0, rgba(p.ink, 0.0))])?;
    pen.fill_rect(Rect::new(0.0, HEIGHT - 150.0, WIDTH, HEIGHT), &foot);
    Ok(())
}

/// The fixed type: the Kubuno mark and eyebrow, the product name, the tagline, the version, the
/// legal and credits lines.
fn type_block(pen: &Pen<'_>, l: &Layout, p: Palette, content: &SplashContent) -> Result<()> {
    let m = l.mark;
    mark(pen, Artwork::Kubuno, ((m.left + m.right) / 2.0, (m.top + m.bottom) / 2.0), m.bottom - m.top, false)?;
    let mut eyebrow = TextStyle::new(TEXT, 12.5, DWRITE_FONT_WEIGHT_SEMI_BOLD, rgba(WHITE, 0.92));
    eyebrow.tracking = 2.6;
    eyebrow.middle = true;
    pen.text("KUBUNO", l.eyebrow, &eyebrow)?;

    let (family, name) = content.title_parts();
    if !family.is_empty() {
        pen.text(&family, l.lead, &TextStyle::new(DISPLAY, 30.0, DWRITE_FONT_WEIGHT_LIGHT, rgba(p.light, 0.95)))?;
    }
    // The name shrinks to fit the column (a long product name stays on one line).
    let width = l.title.right - l.title.left;
    let size = (66.0 * (width / (name.chars().count().max(1) as f32 * 66.0 * 0.56))).clamp(34.0, 66.0);
    let mut title = TextStyle::new(DISPLAY, size, DWRITE_FONT_WEIGHT_SEMI_BOLD, rgba(WHITE, 1.0));
    title.tracking = -0.6;
    title.ellipsis = true;
    pen.text(&name, l.title, &title)?;

    let mut tagline = TextStyle::new(TEXT, 16.0, DWRITE_FONT_WEIGHT_NORMAL, rgba(WHITE, 0.84));
    tagline.ellipsis = true;
    pen.text(&content.tagline, l.tagline, &tagline)?;
    pen.text(&content.version, l.version, &TextStyle::new(TEXT, 12.5, DWRITE_FONT_WEIGHT_NORMAL, rgba(p.light, 0.78)))?;

    pen.text(&content.legal, l.legal, &TextStyle::new(TEXT, 11.5, DWRITE_FONT_WEIGHT_NORMAL, rgba(WHITE, 0.66)))?;
    pen.text(&content.credits, l.credits, &TextStyle::new(TEXT, 11.0, DWRITE_FONT_WEIGHT_NORMAL, rgba(WHITE, 0.46)))?;
    Ok(())
}

/// The live part: the status line and the thin progress bar (or its shimmer while unknown).
fn paint_status(pen: &Pen<'_>, l: &Layout, p: Palette, content: &SplashContent, progress: Option<f32>, phase: f32) -> Result<()> {
    let mut status = TextStyle::new(TEXT, 13.0, DWRITE_FONT_WEIGHT_NORMAL, rgba(WHITE, 0.90));
    status.ellipsis = true;
    pen.text(&content.status, l.status, &status)?;

    let track = l.progress;
    let h = track.bottom - track.top;
    let width = track.right - track.left;
    pen.fill_rounded(track, h / 2.0, &pen.solid(rgba(WHITE, 0.14))?);
    match progress {
        Some(value) => {
            let value = value.clamp(0.0, 1.0);
            if value > 0.0 {
                let end = track.left + width * value;
                let fill = pen.linear((track.left, 0.0), (track.right, 0.0), &[(0.0, rgba(p.light, 0.85)), (1.0, rgba(WHITE, 1.0))])?;
                pen.fill_rounded(Rect::new(track.left, track.top, end.max(track.left + h), track.bottom), h / 2.0, &fill);
                // A small spark at the head of the bar.
                pen.glow((end, (track.top + track.bottom) / 2.0), 10.0, 5.0, p.light, 0.55)?;
            }
        }
        None => {
            // A light that runs along the track.
            let span = width * 0.32;
            let x = track.left - span + (width + span) * phase.fract();
            let shimmer = pen.linear(
                (x, 0.0),
                (x + span, 0.0),
                &[(0.0, rgba(p.light, 0.0)), (0.5, rgba(WHITE, 0.95)), (1.0, rgba(p.light, 0.0))],
            )?;
            let seg = Rect::new(x.max(track.left), track.top, (x + span).min(track.right), track.bottom);
            if seg.right > seg.left {
                pen.fill_rounded(seg, h / 2.0, &shimmer);
            }
        }
    }
    Ok(())
}

// ── Motifs ───────────────────────────────────────────────────────────────────

/// The shell: hexagonal rings around the hero (the aperture of the Kubuno mark), the light that
/// comes through it, and cubes floating around.
fn motif_kubuno(pen: &Pen<'_>, l: &Layout, p: Palette) -> Result<()> {
    let (cx, cy) = l.hero;
    for (i, r) in [150.0, 202.0, 258.0, 320.0, 388.0, 462.0].into_iter().enumerate() {
        let ring = pen.polygon(&hexagon(cx, cy, r), 22.0)?;
        pen.stroke(&ring, &pen.solid(rgba(p.light, 0.20 - i as f32 * 0.028))?, 1.2);
    }
    // The aperture's six blades, continued outward as shafts of light.
    for k in 0..6 {
        let a = (-90.0 + 60.0 * k as f32 + 60.0).to_radians();
        let start = (cx + 132.0 * a.cos(), cy + 132.0 * a.sin());
        let end = (cx + 560.0 * a.cos(), cy + 560.0 * a.sin());
        let shaft = pen.linear(start, end, &[(0.0, rgba(p.light, 0.12)), (1.0, rgba(p.light, 0.0))])?;
        pen.fill(&pen.polygon(&thick_segment(start, end, 30.0), 0.0)?, &shaft);
    }
    for (x, y, size, alpha) in [(470.0, 96.0, 24.0, 0.55), (724.0, 404.0, 32.0, 0.5), (754.0, 92.0, 17.0, 0.45), (456.0, 404.0, 13.0, 0.32), (684.0, 54.0, 11.0, 0.32), (548.0, 446.0, 9.0, 0.26)] {
        cube(pen, (x, y), size, p, alpha)?;
    }
    Ok(())
}

/// An isometric cube, its three faces shaded from the palette.
fn cube(pen: &Pen<'_>, c: (f32, f32), size: f32, p: Palette, alpha: f32) -> Result<()> {
    let (w, h) = (size * 0.87, size * 0.5);
    let (x, y) = c;
    let top = [(x, y - size), (x + w, y - size + h), (x, y), (x - w, y - size + h)];
    let left = [(x - w, y - size + h), (x, y), (x, y + size), (x - w, y + h)];
    let right = [(x, y), (x + w, y - size + h), (x + w, y + h), (x, y + size)];
    let round = size * 0.08;
    pen.fill(&pen.polygon(&top, round)?, &pen.solid(rgba(p.light, alpha))?);
    pen.fill(&pen.polygon(&left, round)?, &pen.solid(rgba(p.accent, alpha))?);
    pen.fill(&pen.polygon(&right, round)?, &pen.solid(rgba(p.mid, alpha))?);
    Ok(())
}

/// Drive: large ghosted slabs (the logo's storage layers) and streams of data flowing in, with
/// amber packets on them.
fn motif_drive(pen: &Pen<'_>, l: &Layout, p: Palette) -> Result<()> {
    let (cx, cy) = l.hero;
    for (i, dy) in [-150.0, -40.0, 70.0, 180.0].into_iter().enumerate() {
        let (hw, hh) = (300.0, 157.0);
        let y = cy + dy;
        let slab = pen.polygon(&[(cx, y - hh), (cx + hw, y), (cx, y + hh), (cx - hw, y)], 26.0)?;
        pen.fill(&slab, &pen.solid(rgba(p.accent, 0.05))?);
        pen.stroke(&slab, &pen.solid(rgba(p.light, 0.13 - i as f32 * 0.02))?, 1.1);
    }
    // Data streams: from the left edge into the hero.
    for (i, y0) in [250.0, 300.0, 350.0, 410.0, 470.0, 540.0].into_iter().enumerate() {
        let f = i as f32;
        let p0 = (-30.0, y0);
        let p1 = (190.0, y0 - 30.0 - f * 6.0);
        let p2 = (330.0, cy + 40.0 + f * 14.0);
        let p3 = (cx - 104.0, cy + 30.0 + f * 9.0);
        let figure = Figure { start: p0, segs: vec![Seg::Cubic(p1, p2, p3)], closed: false };
        let stream = pen.linear((0.0, 0.0), (p3.0, 0.0), &[(0.0, rgba(p.light, 0.0)), (0.55, rgba(p.light, 0.16)), (1.0, rgba(p.light, 0.50))])?;
        pen.stroke(&pen.path(&[figure])?, &stream, 1.3);
        for t in [0.62 + f * 0.04, 0.86 - f * 0.03] {
            let (x, y) = bezier_point(p0, p1, p2, p3, t.clamp(0.0, 1.0));
            pen.glow((x, y), 9.0, 9.0, p.spark, 0.45)?;
            pen.fill_rounded(Rect::new(x - 3.6, y - 1.6, x + 3.6, y + 1.6), 1.6, &pen.solid(rgba(p.spark, 0.95))?);
        }
    }
    // A field of dots, bottom right.
    let dot = pen.solid(rgba(p.light, 0.16))?;
    let mut y = 318.0;
    while y < HEIGHT {
        let mut x = 652.0 + ((y - 318.0) / 22.0 % 2.0) * 11.0;
        while x < WIDTH {
            pen.fill_ellipse((x, y), 1.1, 1.1, &dot);
            x += 22.0;
        }
        y += 22.0;
    }
    Ok(())
}

/// Chat: ripples from the hero (a voice), and speech bubbles drifting around it.
fn motif_chat(pen: &Pen<'_>, l: &Layout, p: Palette) -> Result<()> {
    let (cx, cy) = l.hero;
    for i in 0..7 {
        let r = 148.0 + i as f32 * 56.0;
        pen.stroke_ellipse((cx, cy), r, r, &pen.solid(rgba(p.light, 0.20 - i as f32 * 0.026))?, if i % 2 == 0 { 1.4 } else { 0.8 });
    }
    for (x, y, w, h, alpha, tail_right, dots) in [
        (420.0, 70.0, 86.0, 50.0, 0.12, false, true),
        (690.0, 386.0, 100.0, 58.0, 0.14, true, false),
        (726.0, 74.0, 54.0, 34.0, 0.09, true, false),
        (452.0, 352.0, 60.0, 36.0, 0.08, false, false),
    ] {
        bubble(pen, Rect::new(x, y, x + w, y + h), alpha, tail_right, p)?;
        if dots {
            for k in 0..3 {
                pen.fill_ellipse((x + w / 2.0 - 14.0 + k as f32 * 14.0, y + h / 2.0), 4.0, 4.0, &pen.solid(rgba(WHITE, 0.55))?);
            }
        }
    }
    for (x, y, s) in [(512.0, 160.0, 7.0), (760.0, 250.0, 9.0), (640.0, 452.0, 6.0), (430.0, 250.0, 5.0)] {
        sparkle(pen, (x, y), s, p.spark)?;
    }
    Ok(())
}

/// A speech bubble: a rounded body and its tail.
fn bubble(pen: &Pen<'_>, r: Rect, alpha: f32, tail_right: bool, p: Palette) -> Result<()> {
    let h = r.bottom - r.top;
    let tail = if tail_right {
        vec![(r.right - h * 0.55, r.bottom - 2.0), (r.right - h * 0.15, r.bottom - 2.0), (r.right - h * 0.1, r.bottom + h * 0.32)]
    } else {
        vec![(r.left + h * 0.15, r.bottom - 2.0), (r.left + h * 0.55, r.bottom - 2.0), (r.left + h * 0.1, r.bottom + h * 0.32)]
    };
    let body = pen.path(&[rounded_rect(r, h * 0.3), rounded_polygon(&tail, 2.0)])?;
    pen.fill(&body, &pen.solid(rgba(WHITE, alpha))?);
    pen.stroke(&body, &pen.solid(rgba(p.light, 0.26))?, 1.0);
    Ok(())
}

/// A four-pointed star of light.
fn sparkle(pen: &Pen<'_>, c: (f32, f32), s: f32, color: u32) -> Result<()> {
    let (x, y) = c;
    let k = s * 0.22;
    let star = pen.path(&[Figure {
        start: (x, y - s),
        segs: vec![
            Seg::Quad((x + k, y - k), (x + s, y)),
            Seg::Quad((x + k, y + k), (x, y + s)),
            Seg::Quad((x - k, y + k), (x - s, y)),
            Seg::Quad((x - k, y - k), (x, y - s)),
        ],
        closed: true,
    }])?;
    pen.glow(c, s * 2.4, s * 2.4, color, 0.30)?;
    pen.fill(&star, &pen.solid(rgba(color, 0.9))?);
    Ok(())
}

/// Documents: ruled lines, fanned pages behind the hero, and a ribbon sweeping across.
fn motif_documents(pen: &Pen<'_>, l: &Layout, p: Palette) -> Result<()> {
    let (cx, cy) = l.hero;
    let rule = pen.solid(rgba(WHITE, 0.045))?;
    let mut y = 26.0;
    while y < HEIGHT {
        pen.fill_rect(Rect::new(440.0, y, WIDTH, y + 1.0), &rule);
        y += 24.0;
    }
    // The ribbon, under the pages.
    let ribbon = pen.path(&[Figure {
        start: (360.0, HEIGHT + 30.0),
        segs: vec![
            Seg::Cubic((520.0, 360.0), (640.0, 170.0), (WIDTH + 40.0, 40.0)),
            Seg::Line(WIDTH + 40.0, 104.0),
            Seg::Cubic((680.0, 230.0), (560.0, 420.0), (440.0, HEIGHT + 30.0)),
        ],
        closed: true,
    }])?;
    let sweep = pen.linear((360.0, HEIGHT), (WIDTH, 40.0), &[(0.0, rgba(p.light, 0.02)), (0.5, rgba(p.light, 0.20)), (1.0, rgba(p.spark, 0.04))])?;
    pen.fill(&ribbon, &sweep);
    for (angle, dx, dy, alpha) in [(-17.0, 52.0, -6.0, 0.06), (-7.0, 26.0, 2.0, 0.08), (6.0, -4.0, 10.0, 0.10)] {
        pen.local(Matrix3x2::rotation_around(angle, v(cx + dx, cy + dy)));
        let page = Rect::new(cx + dx - 92.0, cy + dy - 122.0, cx + dx + 92.0, cy + dy + 122.0);
        let sheet = pen.path(&[rounded_rect(page, 8.0)])?;
        pen.fill(&sheet, &pen.solid(rgba(WHITE, alpha))?);
        pen.stroke(&sheet, &pen.solid(rgba(WHITE, 0.16))?, 1.0);
        let bar = pen.solid(rgba(WHITE, alpha + 0.04))?;
        for k in 0..7 {
            let y = page.top + 34.0 + k as f32 * 22.0;
            let w = if k % 3 == 2 { 90.0 } else { 136.0 };
            pen.fill_rounded(Rect::new(page.left + 24.0, y, page.left + 24.0 + w, y + 5.0), 2.5, &bar);
        }
        pen.reset();
    }
    Ok(())
}

// ── The hero and the marks ───────────────────────────────────────────────────

/// The module's mark, large: its glow, its shadow on the ground, the mark itself, a gloss and a
/// rim of light.
fn hero(pen: &Pen<'_>, l: &Layout, p: Palette, art: Artwork) -> Result<()> {
    let (cx, cy) = l.hero;
    let size = l.hero_size;
    pen.glow((cx, cy + size * 0.56), size * 0.46, size * 0.075, BLACK, 0.55)?;
    pen.glow((cx, cy), size * 0.95, size * 0.95, p.light, 0.30)?;
    mark(pen, art, (cx, cy), size, true)
}

/// Draws `art`'s mark centred on `c`, `size` DIP tall (the marks are drawn in a ±50 unit box
/// after the web logos), with the hero's gloss and rim light when `gloss`.
fn mark(pen: &Pen<'_>, art: Artwork, c: (f32, f32), size: f32, gloss: bool) -> Result<()> {
    let unit = size / 100.0;
    pen.local(Matrix3x2::scale(unit, unit) * Matrix3x2::translation(c.0, c.1));
    let drawn = match art {
        Artwork::Kubuno => mark_kubuno(pen),
        Artwork::Drive => mark_drive(pen),
        Artwork::Chat => mark_chat(pen),
        Artwork::Documents => mark_documents(pen),
    };
    let result = drawn.and_then(|silhouettes| {
        if gloss {
            let shine = pen.linear((0.0, -50.0), (0.0, 8.0), &[(0.0, rgba(WHITE, 0.30)), (1.0, rgba(WHITE, 0.0))])?;
            let rim = pen.linear((0.0, -50.0), (0.0, 50.0), &[(0.0, rgba(WHITE, 0.75)), (0.5, rgba(WHITE, 0.12)), (1.0, rgba(WHITE, 0.0))])?;
            for s in &silhouettes {
                pen.fill(s, &shine);
                pen.stroke(s, &rim, 1.2 / unit);
            }
        }
        Ok(())
    });
    pen.reset();
    result
}

/// The Kubuno mark (`kubuno-logo.png`): a hexagon cut into six aperture blades around a smaller
/// hexagon holding a cube.
fn mark_kubuno(pen: &Pen<'_>) -> Result<Vec<ID2D1Geometry>> {
    const BLUE: u32 = 0x2563EB;
    let mut blades = pen.polygon(&hexagon(0.0, 0.0, 50.0), 7.0)?;
    for k in 0..6 {
        let a = (-90.0 + 60.0 * k as f32).to_radians();
        let d = a + 60f32.to_radians();
        let corner = (24.0 * a.cos(), 24.0 * a.sin());
        let from = (corner.0 - d.cos() * 2.0, corner.1 - d.sin() * 2.0);
        let to = (corner.0 + d.cos() * 80.0, corner.1 + d.sin() * 80.0);
        let cut = pen.polygon(&thick_segment(from, to, 4.6), 0.0)?;
        blades = pen.combine(&blades, &cut, D2D1_COMBINE_MODE_EXCLUDE)?;
    }
    let gap = pen.polygon(&hexagon(0.0, 0.0, 31.0), 5.0)?;
    blades = pen.combine(&blades, &gap, D2D1_COMBINE_MODE_EXCLUDE)?;
    let blue = pen.solid(rgba(BLUE, 1.0))?;
    pen.fill(&blades, &blue);
    let centre = pen.polygon(&hexagon(0.0, 0.0, 25.5), 4.5)?;
    pen.fill(&centre, &blue);
    let (w, h) = (13.0, 7.5);
    pen.fill(&pen.polygon(&[(0.0, -15.0), (w, -h), (0.0, 0.0), (-w, -h)], 1.6)?, &pen.solid(rgba(0xA5B8FB, 1.0))?);
    pen.fill(&pen.polygon(&[(-w, -h), (0.0, 0.0), (0.0, 15.0), (-w, h)], 1.6)?, &pen.solid(rgba(0x6E93F7, 1.0))?);
    pen.fill(&pen.polygon(&[(0.0, 0.0), (w, -h), (w, h), (0.0, 15.0)], 1.6)?, &pen.solid(rgba(0x4C7BF0, 1.0))?);
    Ok(vec![blades, centre])
}

/// The Drive mark (`drive-logo.png`): two stacked storage slabs, a white top, a sky-blue seam and
/// amber status lights.
fn mark_drive(pen: &Pen<'_>) -> Result<Vec<ID2D1Geometry>> {
    let body = pen.polygon(&[(0.0, -46.0), (44.0, -23.0), (44.0, 25.0), (0.0, 48.0), (-44.0, 25.0), (-44.0, -23.0)], 8.0)?;
    pen.fill(&body, &pen.solid(rgba(0x2563EB, 1.0))?);
    // The right faces in shade.
    let right = pen.polygon(&[(0.0, 0.0), (60.0, -31.0), (60.0, 60.0), (0.0, 60.0)], 0.0)?;
    let shade = pen.combine(&body, &right, D2D1_COMBINE_MODE_INTERSECT)?;
    pen.fill(&shade, &pen.solid(rgba(0x0B2A8A, 0.28))?);
    // The top face.
    let s = 0.78;
    let top = pen.polygon(&[(0.0, -23.0 - 23.0 * s), (44.0 * s, -23.0), (0.0, -23.0 + 23.0 * s), (-44.0 * s, -23.0)], 4.0)?;
    pen.fill(&top, &pen.solid(rgba(0xF8FAFF, 1.0))?);
    // The seam between the slabs.
    let seam = pen.path(&[Figure { start: (-41.0, -5.5), segs: vec![Seg::Line(0.0, 16.0), Seg::Line(41.0, -5.5)], closed: false }])?;
    pen.stroke(&seam, &pen.solid(rgba(0x7DD3FC, 1.0))?, 3.4);
    // The status lights, along the slope of the right faces.
    let amber = pen.solid(rgba(0xFACC15, 1.0))?;
    let slope = (-23.0f32).atan2(44.0);
    for (x, y) in [(27.0, -5.0), (27.0, 23.0)] {
        let (dx, dy) = (slope.cos() * 5.2, slope.sin() * 5.2);
        pen.fill(&pen.polygon(&thick_segment((x - dx, y - dy), (x + dx, y + dy), 4.4), 2.1)?, &amber);
    }
    Ok(vec![body])
}

/// The Chat mark (`chat-logo.png`): a sky hexagon and a white speech bubble.
fn mark_chat(pen: &Pen<'_>) -> Result<Vec<ID2D1Geometry>> {
    let hex = pen.polygon(&hexagon(0.0, 0.0, 50.0), 9.0)?;
    pen.fill(&hex, &pen.solid(rgba(0x0EA5E9, 1.0))?);
    let bubble = pen.path(&[rounded_rect(Rect::new(-25.0, -19.0, 25.0, 14.0), 7.0), rounded_polygon(&[(-15.0, 12.0), (-4.0, 12.0), (-17.0, 26.0)], 1.5)])?;
    pen.fill(&bubble, &pen.solid(rgba(0xFFFFFF, 1.0))?);
    Ok(vec![hex])
}

/// The Documents mark (`office-documents-logo.png`): an azure hexagon, a light-blue page with its
/// folded corner and lines, and the navy « D » badge.
fn mark_documents(pen: &Pen<'_>) -> Result<Vec<ID2D1Geometry>> {
    let hex = pen.polygon(&hexagon(4.0, 0.0, 48.0), 9.0)?;
    pen.fill(&hex, &pen.solid(rgba(0x0078D4, 1.0))?);
    let page = pen.polygon(&[(-12.0, -30.0), (13.0, -30.0), (26.0, -17.0), (26.0, 30.0), (-12.0, 30.0)], 3.0)?;
    pen.fill(&page, &pen.solid(rgba(0x8FBDF3, 1.0))?);
    pen.fill(&pen.polygon(&[(13.0, -30.0), (13.0, -17.0), (26.0, -17.0)], 1.0)?, &pen.solid(rgba(0xDCEBFC, 1.0))?);
    let line = pen.solid(rgba(0x2F6FCB, 1.0))?;
    for y in [-3.0, 5.0, 13.0] {
        pen.fill_rounded(Rect::new(3.0, y - 1.7, 19.0, y + 1.7), 1.7, &line);
    }
    // The badge, with a soft shadow on the page.
    pen.fill_rounded(Rect::new(-36.0, -14.0, -3.0, 19.0), 7.0, &pen.solid(rgba(0x00285A, 0.35))?);
    let badge = pen.path(&[rounded_rect(Rect::new(-38.0, -16.0, -5.0, 17.0), 6.5)])?;
    pen.fill(&badge, &pen.solid(rgba(0x0050A8, 1.0))?);
    let mut d = TextStyle::new(DISPLAY, 24.0, DWRITE_FONT_WEIGHT_BOLD, rgba(0xFFFFFF, 1.0));
    d.centered = true;
    d.middle = true;
    pen.text("D", Rect::new(-38.0, -17.5, -5.0, 15.5), &d)?;
    Ok(vec![hex, badge])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rounded_polygon_is_closed_and_has_a_corner_per_vertex() {
        let f = rounded_polygon(&hexagon(0.0, 0.0, 50.0), 6.0);
        assert!(f.closed);
        assert_eq!(f.segs.len(), 12, "a line and a corner per vertex");
        // It starts just after the first corner, on the first edge.
        let (x, y) = f.start;
        assert!((x * x + y * y).sqrt() < 50.0 && y < -40.0, "{:?}", f.start);
    }

    #[test]
    fn corners_never_take_more_than_half_an_edge() {
        let f = rounded_polygon(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)], 10.0);
        assert_eq!(f.start, (2.0, 0.0));
    }

    #[test]
    fn hexagons_are_pointy_topped() {
        let h = hexagon(10.0, 20.0, 50.0);
        assert!((h[0].0 - 10.0).abs() < 1e-4 && (h[0].1 + 30.0).abs() < 1e-4);
        assert!((h[3].1 - 70.0).abs() < 1e-4);
    }

    #[test]
    fn a_thick_segment_is_as_wide_as_asked() {
        let q = thick_segment((0.0, 0.0), (10.0, 0.0), 4.0);
        assert_eq!(q, vec![(0.0, 2.0), (10.0, 2.0), (10.0, -2.0), (0.0, -2.0)]);
    }

    #[test]
    fn the_layout_keeps_the_type_off_the_hero_and_inside_the_card() {
        let l = Layout::standard();
        let inner = Rect::new(l.card.left + 32.0, l.card.top + 32.0, l.card.right - 32.0, l.card.bottom - 24.0);
        let hero = l.hero_rect();
        for (name, r) in [("mark", l.mark), ("eyebrow", l.eyebrow), ("lead", l.lead), ("title", l.title), ("tagline", l.tagline), ("version", l.version), ("status", l.status), ("progress", l.progress), ("legal", l.legal), ("credits", l.credits)] {
            assert!(r.left >= inner.left && r.top >= inner.top && r.right <= inner.right && r.bottom <= inner.bottom, "{name} leaves the card's margins: {r:?}");
            let over_hero = r.left < hero.right && r.right > hero.left && r.top < hero.bottom && r.bottom > hero.top;
            assert!(!over_hero, "{name} runs under the hero: {r:?} / {hero:?}");
        }
        assert!(hero.left >= l.card.left && hero.right <= l.card.right && hero.top >= l.card.top && hero.bottom <= l.card.bottom);
        // Top to bottom, without overlaps.
        let column = [l.eyebrow, l.lead, l.title, l.tagline, l.version, l.status, l.progress, l.legal, l.credits];
        for pair in column.windows(2) {
            assert!(pair[0].top < pair[1].top, "{:?} then {:?}", pair[0], pair[1]);
        }
        for pair in [l.tagline, l.version, l.status, l.progress, l.legal, l.credits].windows(2) {
            assert!(pair[0].bottom <= pair[1].top, "{:?} overlaps {:?}", pair[0], pair[1]);
        }
    }

    #[test]
    fn the_card_is_in_the_size_range_of_a_large_splash() {
        assert!((700.0..=900.0).contains(&WIDTH) && (450.0..=560.0).contains(&HEIGHT));
    }

    #[test]
    fn artworks_round_trip_by_name_and_differ() {
        for a in Artwork::ALL {
            assert_eq!(Artwork::from_name(a.name()), Some(a));
            assert!(a.product().starts_with("Kubuno "));
        }
        assert_eq!(Artwork::from_name("office"), Some(Artwork::Documents));
        assert_eq!(Artwork::from_name(" Desktop "), Some(Artwork::Kubuno));
        assert_eq!(Artwork::from_name("calendar"), None);
        let palettes: std::collections::HashSet<_> = Artwork::ALL.iter().map(|a| (a.palette().accent, a.palette().spark)).collect();
        assert_eq!(palettes.len(), 4, "one colour family per application");
    }

    #[test]
    fn the_specks_are_the_same_every_frame() {
        let a: Vec<f32> = { let mut r = Lcg::new(3); (0..5).map(|_| r.next()).collect() };
        let b: Vec<f32> = { let mut r = Lcg::new(3); (0..5).map(|_| r.next()).collect() };
        assert_eq!(a, b);
        assert!(a.iter().all(|x| (0.0..1.0).contains(x)));
    }
}
