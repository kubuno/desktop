//! The control library's own Direct2D drawing surface.
//!
//! It implements [`kubuno_drive_desktop_app_controls::Canvas`], the contract every control in
//! this crate paints through — so the demo host (and, in time, every Kubuno
//! desktop app that mounts these controls) gets the whole design system
//! (palette, shape tokens, DirectWrite formats, Material Symbols geometries)
//! without knowing anything about how a control draws itself.
//!
//! This is a faithful copy of the shell's `Painter` (`shell/src/painter.rs`):
//! the SAME primitives, the SAME pixel-snapping and DPI behaviour. It has to
//! be — the reference sheets are painted by the real toolkit, and any drift in
//! how a border snaps or a shadow ramps would make every side-by-side
//! comparison lie. The shell keeps its own copy only because Rust forbids
//! implementing a foreign trait on a foreign type across crates; the two are
//! meant to stay identical.
//!
//! The ONE deliberate deviation from the shell is [`Painter::fill_top_rounded`],
//! documented at its definition: the shell approximated a top-only rounded
//! shape with a fully rounded fill because it mounts no tab strip, whereas this
//! library mounts `TabControl`, so the honest geometry is drawn here.

use crate::control::ControlCanvas;
use crate::system::{Border3DSide, Border3DStyle, Visuals};
use crate::theme::{ThemeClass, ThemeRenderer};
use kubuno_drive_desktop_app_controls::geometry::Rect;
use kubuno_drive_desktop_app_controls::{Canvas, Renderer, TextFormats, Theme};
use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1DeviceContext, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
    D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_PRIMITIVE_BLEND_COPY,
    D2D1_PRIMITIVE_BLEND_SOURCE_OVER, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER,
};

/// The width and height of the layout box [`Canvas::measure`] lays text out
/// in: large enough for any single line, finite so DirectWrite's metrics stay
/// exact (the same bound kubuno-drive-desktop's painter and renderer use).
const MEASURE_BOUND: f32 = 65_536.0;

pub struct Painter<'a> {
    pub ctx:      &'a ID2D1DeviceContext,
    pub renderer: &'a Renderer,
    pub theme:    &'a Theme,
    /// The system's colours, metrics and UI font at this window's DPI.
    ///
    /// BORROWED, not owned: they are read from Windows once per DPI and the
    /// host keeps them alive across frames, whereas a `Painter` lives for one
    /// paint. Rebuilding three DirectWrite text formats sixty times a second
    /// would be the kind of cost that never shows up in a profile as one line.
    pub visuals:  &'a Visuals,
    /// The window's themed-part cache — `uxtheme.dll`'s own rendering of a
    /// control's chrome, kept as Direct2D bitmaps.
    ///
    /// BORROWED for exactly the reason `visuals` is, one step further: it holds
    /// theme handles and GPU bitmaps, and a `Painter` lives for one frame. An
    /// owned cache would be a cache that is empty on every frame, i.e. a full
    /// GDI round trip per control per frame — the cost the cache exists to
    /// remove. The host owns it, so its lifetime is the window's.
    pub parts:    &'a ThemeRenderer,
    brush:        ID2D1SolidColorBrush,
    scale:        std::cell::Cell<f32>,
    /// Background-colour stack, published to widgets through `Canvas::current_bg`
    /// / `push_bg` / `pop_bg`. Empty = the outermost surface, whose value is
    /// [`Painter::ground`].
    bg_stack:     std::cell::RefCell<Vec<D2D1_COLOR_F>>,
    /// What `current_bg` answers while nothing has been pushed: the colour the
    /// host really cleared the surface to (see [`Painter::set_ground`]).
    ground:       std::cell::Cell<D2D1_COLOR_F>,
    /// The scroll offsets in force (see `Canvas::push_offset`), each with the
    /// transform and origin it replaced so `pop_offset` restores them exactly.
    offsets:      std::cell::RefCell<Vec<(windows_numerics::Matrix3x2, (f32, f32))>>,
    /// Sum of the pushed offsets: content coordinates + `origin` = surface
    /// coordinates.
    origin:       std::cell::Cell<(f32, f32)>,
    /// The clips in force, in SURFACE coordinates, axis-aligned and rounded
    /// alike (a rounded clip counts as its bounding box).
    clips:        std::cell::RefCell<Vec<Rect>>,
    /// The open `begin_extent` scopes, innermost last.
    extents:      std::cell::RefCell<Vec<ExtentScope>>,
}

/// One `Canvas::begin_extent` scope: where content coordinates started, how
/// many clips were already in force (those do not limit it), and the furthest
/// right/bottom edge drawn so far, in surface coordinates.
struct ExtentScope {
    origin: (f32, f32),
    clip_depth: usize,
    reach: Option<(f32, f32)>,
}

impl<'a> Painter<'a> {
    pub fn new(
        renderer: &'a Renderer,
        theme: &'a Theme,
        visuals: &'a Visuals,
        parts: &'a ThemeRenderer,
    ) -> Result<Self> {
        Ok(Self {
            ctx: &renderer.d2d_context,
            renderer,
            theme,
            visuals,
            parts,
            brush: renderer.solid_brush(&theme.text_primary)?,
            scale: std::cell::Cell::new(1.0),
            bg_stack: std::cell::RefCell::new(Vec::new()),
            // The historical answer, kept as the default so a caller that
            // builds its own `Painter` and never calls `set_ground` sees no
            // change.
            ground: std::cell::Cell::new(theme.window_background),
            offsets: std::cell::RefCell::new(Vec::new()),
            origin: std::cell::Cell::new((0.0, 0.0)),
            clips: std::cell::RefCell::new(Vec::new()),
            extents: std::cell::RefCell::new(Vec::new()),
        })
    }

    pub fn set_scale(&self, scale: f32) {
        self.scale.set(scale);
    }

    /// Tells the painter what the surface was cleared to before the paint
    /// started — the answer `Canvas::current_bg` gives while no container has
    /// pushed a background.
    ///
    /// The widgets' "opaque ground" preamble fills their bounds with
    /// `current_bg()` before painting. On the web nothing is painted there, so
    /// the only faithful value for an unpushed stack is one that paints
    /// NOTHING visible: the host passes the exact colour it cleared the main
    /// window to (the preamble then repaints the same pixels — invisible), and
    /// a fully transparent colour for a floating surface, whose window is
    /// cleared to transparent (a source-over fill with alpha 0 is a no-op, so
    /// no square block appears around a rounded panel or its shadow).
    ///
    /// Defaults to the theme's window background, the historical value.
    pub fn set_ground(&self, colour: D2D1_COLOR_F) {
        self.ground.set(colour);
    }

    /// The colour `current_bg` answers while nothing has been pushed (see
    /// [`Painter::set_ground`]).
    pub fn ground(&self) -> D2D1_COLOR_F {
        self.ground.get()
    }

    fn set_color(&self, color: &D2D1_COLOR_F) -> &ID2D1SolidColorBrush {
        unsafe { self.brush.SetColor(color) };
        &self.brush
    }

    /// Snaps a DIP value onto the physical pixel grid — without this, a 1 px
    /// border lands between two device pixels and renders as a soft 2 px halo
    /// at fractional DPI scales.
    fn px(&self, v: f32) -> f32 {
        let s = self.scale.get().max(0.01);
        (v * s).round() / s
    }

    /// Snaps onto pixel CENTRES, which is where a 1 px stroke must sit.
    fn px_center(&self, v: f32) -> f32 {
        let s = self.scale.get().max(0.01);
        ((v * s).round() + 0.5) / s
    }

    fn snap(&self, rect: &Rect) -> Rect {
        Rect::new(self.px(rect.left), self.px(rect.top), self.px(rect.right), self.px(rect.bottom))
    }

    /// The transform the scroll offsets put in force: identity, or a pure
    /// translation by [`Painter::origin`].
    fn base_transform(&self) -> windows_numerics::Matrix3x2 {
        let (x, y) = self.origin.get();
        windows_numerics::Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: x, M32: y }
    }

    /// Records that `rect` (in the current, possibly offset, coordinates) was
    /// drawn, for every open `begin_extent` scope — cut by the clips that scope
    /// did not already have in force.
    fn touch(&self, rect: &Rect) {
        let mut scopes = self.extents.borrow_mut();
        if scopes.is_empty() {
            return;
        }
        let (ox, oy) = self.origin.get();
        let r = Rect::new(rect.left + ox, rect.top + oy, rect.right + ox, rect.bottom + oy);
        let clips = self.clips.borrow();
        for scope in scopes.iter_mut() {
            let mut v = r;
            for c in clips.iter().skip(scope.clip_depth) {
                v = Rect::new(v.left.max(c.left), v.top.max(c.top), v.right.min(c.right), v.bottom.min(c.bottom));
            }
            if v.right <= v.left || v.bottom <= v.top {
                continue;
            }
            scope.reach = Some(match scope.reach {
                Some((x, y)) => (x.max(v.right), y.max(v.bottom)),
                None => (v.right, v.bottom),
            });
        }
    }

    /// Records a clip (in the current coordinates) for the extent scopes.
    fn note_clip(&self, rect: &Rect) {
        let (ox, oy) = self.origin.get();
        self.clips
            .borrow_mut()
            .push(Rect::new(rect.left + ox, rect.top + oy, rect.right + ox, rect.bottom + oy));
    }

    /// Text through an explicit layout rather than by mutating the shared
    /// format: alignment and trimming set on a format would leak into every
    /// other caller using it.
    fn draw_layout(
        &self,
        text: &str,
        rect: &Rect,
        format: &IDWriteTextFormat,
        color: &D2D1_COLOR_F,
        alignment: DWRITE_TEXT_ALIGNMENT,
        ellipsis: bool,
    ) {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let r = self.snap(rect);
        unsafe {
            let Ok(layout) = self.renderer.dwrite.CreateTextLayout(
                &wide,
                format,
                (r.right - r.left).max(0.0),
                (r.bottom - r.top).max(0.0),
            ) else {
                return;
            };
            let _ = layout.SetTextAlignment(alignment);
            let _ = layout.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            if ellipsis {
                if let Ok(sign) = self.renderer.dwrite.CreateEllipsisTrimmingSign(format) {
                    let trimming = DWRITE_TRIMMING {
                        granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                        delimiter: 0,
                        delimiterCount: 0,
                    };
                    let _ = layout.SetTrimming(&trimming, &sign);
                }
            }
            // What the text really covers — it may spill past a box too narrow
            // for it (no wrapping), and that spill is what a scrolled area
            // must reach.
            let mut m = Default::default();
            if !text.is_empty() && layout.GetMetrics(&mut m).is_ok() {
                let (x, y) = (r.left + m.left, r.top + m.top);
                self.touch(&Rect::new(x, y, x + m.width, y + m.height));
            }
            self.ctx.DrawTextLayout(
                windows_numerics::Vector2 { X: r.left, Y: r.top },
                &layout,
                self.set_color(color),
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
    }

    /// Draws icon `name` `size` DIP square centred in `rect`: a glyph of the vector set (each layer
    /// in the colour `pick` gives it), or what `kubuno_drive_desktop_app_controls::icon_source` describes — an
    /// image file (`crate::icon_image`, rendered at the pixel size it covers, `base` being an SVG's
    /// `currentColor`), and the drawing options of either (a tint, a size of its own, a mirror).
    fn vector(
        &self,
        name: &'static str,
        rect: &Rect,
        size: f32,
        base: D2D1_COLOR_F,
        pick: &dyn Fn(&kubuno_drive_desktop_app_controls::IconLayer) -> D2D1_COLOR_F,
    ) {
        use windows::core::Interface;
        let spec = kubuno_drive_desktop_app_controls::icon_source::parse(name);
        if spec.is_image() {
            let drawn = crate::icon_image::draw(self.ctx, name, (rect.left, rect.top, rect.right, rect.bottom), size, self.scale.get(), base, self.theme);
            if let Some((l, t, r, b)) = drawn {
                self.touch(&Rect::new(l, t, r, b));
            }
            return;
        }
        let Ok(factory) = self
            .renderer
            .d2d_factory
            .cast::<windows::Win32::Graphics::Direct2D::ID2D1Factory>()
        else {
            return;
        };
        // A glyph with options: `spec.source` is a part of `name`, itself `'static`.
        let (name, size) = (spec.source, spec.size.map_or(size, |(w, h)| w.min(h)));
        let tint = spec.tint.and_then(|t| crate::icon_image::tint_color(t, Some(self.theme)));
        let mut icons = self.renderer.vector_icons.borrow_mut();
        let stroke_style = icons.stroke_style(&factory).cloned();
        let Some((layers, viewbox)) = icons.get_layers(&factory, name) else { return };
        let k = size / viewbox;
        let left = self.px((rect.left + rect.right) / 2.0 - size / 2.0);
        let top = self.px((rect.top + rect.bottom) / 2.0 - size / 2.0);
        self.touch(&Rect::new(left, top, left + size, top + size));
        // Mirrored (a right-to-left layout): x' = size - x.
        let (flip, shift) = if spec.mirror { (-1.0, size) } else { (1.0, 0.0) };
        // Composed onto the scroll offset in force, and restored to it after.
        let (ox, oy) = self.origin.get();
        unsafe {
            for layer in layers {
                // The layer's own group transform composed with the icon's
                // scale and placement, which keeps the path data verbatim.
                let [a, b, cc, d, e, f2] = layer.transform;
                let transform = windows_numerics::Matrix3x2 {
                    M11: flip * a * k,
                    M12: b * k,
                    M21: flip * cc * k,
                    M22: d * k,
                    M31: flip * e * k + shift + left + ox,
                    M32: f2 * k + top + oy,
                };
                self.ctx.SetTransform(&transform);
                // An explicit tint wins over everything; then a fixed brand colour wins over the
                // themed role: module logos are painted in their own colours, like their web
                // counterparts.
                let c = match tint {
                    Some(t) => D2D1_COLOR_F { a: t.a * layer.opacity, ..t },
                    None => layer.color.unwrap_or_else(|| pick(layer)),
                };
                let brush = self.set_color(&c);
                match layer.stroke {
                    // Outlined (Lucide): the width is in design units, so the
                    // matrix scales it along with the geometry.
                    Some(width) => self.ctx.DrawGeometry(
                        &layer.geometry,
                        brush,
                        width,
                        stroke_style.as_ref(),
                    ),
                    None => self.ctx.FillGeometry(&layer.geometry, brush, None),
                }
            }
            self.ctx.SetTransform(&self.base_transform());
        }
    }
}

impl Canvas for Painter<'_> {
    fn theme(&self) -> &Theme {
        self.theme
    }

    fn formats(&self) -> &TextFormats {
        &self.renderer.formats
    }

    fn scale(&self) -> f32 {
        self.scale.get().max(0.01)
    }

    fn fill_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        self.touch(rect);
        let rr = D2D1_ROUNDED_RECT {
            rect: self.snap(rect).d2d(),
            radiusX: radius,
            radiusY: radius,
        };
        unsafe { self.ctx.FillRoundedRectangle(&rr, self.set_color(color)) };
    }

    /// A genuine top-only rounded rectangle — rounded top corners, square
    /// bottom ones — built as a hand-made path.
    ///
    /// DELIBERATE DEVIATION FROM THE SHELL: the shell's `fill_top_rounded`
    /// approximates the shape with a fully rounded fill, and says so — it
    /// "has no tab strip, so the top-only shape is not worth a hand built
    /// geometry". This library DOES mount `TabControl` (the `layout_panels`
    /// family), whose `TabContainer` is exactly this shape, so approximating it
    /// would make the tab reference comparison lie. The honest geometry is
    /// drawn here instead.
    fn fill_top_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        use windows::core::Interface;
        self.touch(rect);
        let r = self.snap(rect);
        // Clamp to what the box can hold; below a pixel there is no visible arc,
        // so a plain square-cornered fill is both correct and safe against a
        // degenerate sink.
        let radius = radius.min((r.right - r.left) / 2.0).min((r.bottom - r.top) / 2.0);
        if radius <= 0.5 {
            self.fill_rounded(rect, 0.0, color);
            return;
        }
        let Ok(factory) = self
            .renderer
            .d2d_factory
            .cast::<windows::Win32::Graphics::Direct2D::ID2D1Factory>()
        else {
            // Without a factory to build the path, a fully rounded fill is the
            // same honest approximation the shell settles for.
            self.fill_rounded(rect, radius, color);
            return;
        };
        // Sink points are `Vector2` in this `windows` version (the D2D point
        // types collapsed onto the numerics vector).
        let pt = |x: f32, y: f32| windows_numerics::Vector2 { X: x, Y: y };
        // Each corner is a small clockwise arc, traversed as the outline is
        // walked clockwise from the top edge.
        let arc = |x: f32, y: f32| windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
            point: pt(x, y),
            size: windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_F {
                width: radius,
                height: radius,
            },
            rotationAngle: 0.0,
            sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: windows::Win32::Graphics::Direct2D::D2D1_ARC_SIZE_SMALL,
        };
        unsafe {
            let Ok(path) = factory.CreatePathGeometry() else {
                self.fill_rounded(rect, radius, color);
                return;
            };
            let Ok(sink) = path.Open() else {
                self.fill_rounded(rect, radius, color);
                return;
            };
            sink.BeginFigure(
                pt(r.left + radius, r.top),
                windows::Win32::Graphics::Direct2D::Common::D2D1_FIGURE_BEGIN_FILLED,
            );
            sink.AddLine(pt(r.right - radius, r.top));
            sink.AddArc(&arc(r.right, r.top + radius));
            sink.AddLine(pt(r.right, r.bottom));
            sink.AddLine(pt(r.left, r.bottom));
            sink.AddLine(pt(r.left, r.top + radius));
            sink.AddArc(&arc(r.left + radius, r.top));
            sink.EndFigure(windows::Win32::Graphics::Direct2D::Common::D2D1_FIGURE_END_CLOSED);
            let _ = sink.Close();
            self.ctx.FillGeometry(&path, self.set_color(color), None);
        }
    }

    fn fill_triangle(&self, a: (f32, f32), b: (f32, f32), c: (f32, f32), color: &D2D1_COLOR_F) {
        use windows::core::Interface;
        let Ok(factory) = self
            .renderer
            .d2d_factory
            .cast::<windows::Win32::Graphics::Direct2D::ID2D1Factory>()
        else {
            // Without a factory to build the path, the bubble alone still reads;
            // the arrow is a cosmetic pointer, not information.
            return;
        };
        self.touch(&Rect::new(
            a.0.min(b.0).min(c.0),
            a.1.min(b.1).min(c.1),
            a.0.max(b.0).max(c.0),
            a.1.max(b.1).max(c.1),
        ));
        // Points snapped to the pixel grid, like every other fill here, so the
        // base sits flush on the bubble's own snapped edge.
        let pt = |p: (f32, f32)| windows_numerics::Vector2 { X: self.px(p.0), Y: self.px(p.1) };
        unsafe {
            let Ok(path) = factory.CreatePathGeometry() else {
                return;
            };
            let Ok(sink) = path.Open() else {
                return;
            };
            sink.BeginFigure(
                pt(a),
                windows::Win32::Graphics::Direct2D::Common::D2D1_FIGURE_BEGIN_FILLED,
            );
            sink.AddLine(pt(b));
            sink.AddLine(pt(c));
            sink.EndFigure(windows::Win32::Graphics::Direct2D::Common::D2D1_FIGURE_END_CLOSED);
            let _ = sink.Close();
            self.ctx.FillGeometry(&path, self.set_color(color), None);
        }
    }

    /// One anti-aliased arc path with round caps — smooth at every angle, where
    /// the default's row of dots reads as a beaded, shimmering edge once it
    /// turns. NOT snapped to the pixel grid: a curve that moves every frame
    /// would jitter from snap to snap.
    fn stroke_arc(
        &self,
        centre: (f32, f32),
        radius: f32,
        start: f32,
        sweep: f32,
        width: f32,
        color: &D2D1_COLOR_F,
    ) {
        use windows::core::Interface;
        use windows::Win32::Graphics::Direct2D::Common::{
            D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN, D2D_SIZE_F,
        };
        use windows::Win32::Graphics::Direct2D::{
            D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL, D2D1_CAP_STYLE_ROUND,
            D2D1_LINE_JOIN_ROUND, D2D1_STROKE_STYLE_PROPERTIES, D2D1_SWEEP_DIRECTION_CLOCKWISE,
            D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
        };
        if radius <= 0.0 || width <= 0.0 || sweep == 0.0 {
            return;
        }
        let pad = radius + width / 2.0;
        self.touch(&Rect::new(centre.0 - pad, centre.1 - pad, centre.0 + pad, centre.1 + pad));
        let Ok(factory) = self
            .renderer
            .d2d_factory
            .cast::<windows::Win32::Graphics::Direct2D::ID2D1Factory>()
        else {
            return;
        };
        let at = |a: f32| windows_numerics::Vector2 {
            X: centre.0 + radius * a.cos(),
            Y: centre.1 + radius * a.sin(),
        };
        // An arc segment cannot close on its own start, and one wider than half
        // a turn needs the "large" flag: splitting anything past π into two
        // halves avoids both, so every segment here is a small arc.
        let sweep = sweep.clamp(-std::f32::consts::TAU, std::f32::consts::TAU);
        let ends: &[f32] = if sweep.abs() > std::f32::consts::PI { &[0.5, 1.0] } else { &[1.0] };
        let direction = if sweep > 0.0 {
            D2D1_SWEEP_DIRECTION_CLOCKWISE
        } else {
            D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE
        };
        unsafe {
            let Ok(path) = factory.CreatePathGeometry() else { return };
            let Ok(sink) = path.Open() else { return };
            sink.BeginFigure(at(start), D2D1_FIGURE_BEGIN_HOLLOW);
            for &to in ends {
                sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: at(start + sweep * to),
                    size: D2D_SIZE_F { width: radius, height: radius },
                    rotationAngle: 0.0,
                    sweepDirection: direction,
                    arcSize: D2D1_ARC_SIZE_SMALL,
                });
            }
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            let _ = sink.Close();
            let props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                ..Default::default()
            };
            let style = factory.CreateStrokeStyle(&props, None).ok();
            self.ctx.DrawGeometry(&path, self.set_color(color), width, style.as_ref());
        }
    }

    fn stroke_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        self.touch(rect);
        let s = self.scale.get().max(0.01);
        let snapped = Rect::new(
            self.px_center(rect.left),
            self.px_center(rect.top),
            self.px_center(rect.right - 1.0 / s),
            self.px_center(rect.bottom - 1.0 / s),
        );
        let rr = D2D1_ROUNDED_RECT { rect: snapped.d2d(), radiusX: radius, radiusY: radius };
        unsafe { self.ctx.DrawRoundedRectangle(&rr, self.set_color(color), 1.0 / s, None) };
    }

    fn stroke_rounded_w(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F, width: f32) {
        self.touch(rect);
        // Drawn INWARD: the stroke straddles the path, so the rect is inset by
        // half the width to keep the border inside the shape.
        let half = width / 2.0;
        let inner = Rect::new(
            rect.left + half,
            rect.top + half,
            rect.right - half,
            rect.bottom - half,
        );
        let rr = D2D1_ROUNDED_RECT {
            rect: inner.d2d(),
            radiusX: (radius - half).max(0.0),
            radiusY: (radius - half).max(0.0),
        };
        unsafe { self.ctx.DrawRoundedRectangle(&rr, self.set_color(color), width, None) };
    }

    fn text(
        &self,
        text: &str,
        rect: &Rect,
        format: &IDWriteTextFormat,
        color: &D2D1_COLOR_F,
        centered: bool,
    ) {
        let alignment =
            if centered { DWRITE_TEXT_ALIGNMENT_CENTER } else { DWRITE_TEXT_ALIGNMENT_LEADING };
        self.draw_layout(text, rect, format, color, alignment, false);
    }

    fn text_aligned(
        &self,
        text: &str,
        rect: &Rect,
        format: &IDWriteTextFormat,
        color: &D2D1_COLOR_F,
        alignment: DWRITE_TEXT_ALIGNMENT,
    ) {
        self.draw_layout(text, rect, format, color, alignment, false);
    }

    fn text_ellipsis(
        &self,
        text: &str,
        rect: &Rect,
        format: &IDWriteTextFormat,
        color: &D2D1_COLOR_F,
    ) {
        self.draw_layout(text, rect, format, color, DWRITE_TEXT_ALIGNMENT_LEADING, true);
    }

    fn text_ellipsis_center(
        &self,
        text: &str,
        rect: &Rect,
        format: &IDWriteTextFormat,
        color: &D2D1_COLOR_F,
    ) {
        self.draw_layout(text, rect, format, color, DWRITE_TEXT_ALIGNMENT_CENTER, true);
    }

    fn image(&self, bitmap: &ID2D1Bitmap1, rect: &Rect, size: f32) {
        self.image_alpha(bitmap, rect, size, 1.0);
    }

    fn image_alpha(&self, bitmap: &ID2D1Bitmap1, rect: &Rect, size: f32, alpha: f32) {
        let s = self.scale.get().max(0.01);
        let size_px = (size * s).round();
        let cx = (rect.left + rect.right) / 2.0;
        let cy = (rect.top + rect.bottom) / 2.0;
        let left = ((cx * s) - size_px / 2.0).round() / s;
        let top = ((cy * s) - size_px / 2.0).round() / s;
        let dest = Rect::new(left, top, left + size_px / s, top + size_px / s);
        self.touch(&dest);
        unsafe {
            self.ctx.DrawBitmap(
                bitmap,
                Some(&dest.d2d()),
                alpha,
                D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
                None,
                None,
            );
        }
    }

    fn vector_icon(&self, name: &'static str, rect: &Rect, size: f32, color: &D2D1_COLOR_F) {
        let color = *color;
        self.vector(name, rect, size, color, &move |layer| D2D1_COLOR_F {
            a: color.a * layer.opacity,
            ..color
        });
    }

    fn vector_icon_layered(
        &self,
        name: &'static str,
        rect: &Rect,
        size: f32,
        fg: &D2D1_COLOR_F,
        accent: &D2D1_COLOR_F,
    ) {
        use kubuno_drive_desktop_app_controls::LayerRole;
        let (fg, accent) = (*fg, *accent);
        let contrast = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        self.vector(name, rect, size, fg, &move |layer| {
            let base = match layer.role {
                LayerRole::Accent => accent,
                LayerRole::AccentContrast => contrast,
                _ => fg,
            };
            D2D1_COLOR_F { a: base.a * layer.opacity, ..base }
        });
    }

    fn measure(&self, text: &str, format: &IDWriteTextFormat) -> f32 {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            // A finite layout box, like kubuno-drive-desktop's `Renderer::measure_width`:
            // `f32::MAX` can overflow DirectWrite's internal arithmetic and
            // yield wrong metrics, and nothing measured here comes close to
            // 65536 DIP.
            let Ok(layout) =
                self.renderer.dwrite.CreateTextLayout(&wide, format, MEASURE_BOUND, MEASURE_BOUND)
            else {
                return 0.0;
            };
            let mut metrics = Default::default();
            if layout.GetMetrics(&mut metrics).is_err() {
                return 0.0;
            }
            metrics.widthIncludingTrailingWhitespace
        }
    }

    fn draw_card_shadow(&self, rect: &Rect, radius: f32) {
        self.draw_layered_shadow(
            rect,
            radius,
            &kubuno_drive_desktop_app_controls::themes::shape::SHADOW_MENU,
            kubuno_drive_desktop_app_controls::themes::shape::SHADOW_GREY,
        );
    }

    fn draw_shadow(
        &self,
        rect: &Rect,
        radius: f32,
        layers: &[kubuno_drive_desktop_app_controls::themes::shape::ShadowLayer],
        colour: (f32, f32, f32),
    ) {
        self.draw_layered_shadow(rect, radius, layers, colour);
    }

    fn erase_rounded(&self, rect: &Rect, radius: f32) {
        let rr = D2D1_ROUNDED_RECT {
            rect: self.snap(rect).d2d(),
            radiusX: radius,
            radiusY: radius,
        };
        let clear = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
        unsafe {
            // COPY *replaces* the destination pixels instead of blending into
            // them, which is the only way a fill can remove what is already
            // there — SourceOver with a transparent colour is a no-op.
            self.ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
            self.ctx.FillRoundedRectangle(&rr, self.set_color(&clear));
            self.ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
        }
    }

    fn push_clip(&self, rect: &Rect) {
        self.note_clip(rect);
        unsafe {
            self.ctx.PushAxisAlignedClip(&rect.d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
    }

    fn pop_clip(&self) {
        let _ = self.clips.borrow_mut().pop();
        unsafe { self.ctx.PopAxisAlignedClip() };
    }

    fn push_clip_rounded(&self, rect: &Rect, radius: f32) {
        use windows::core::Interface;
        unsafe {
            let Ok(factory) = self
                .renderer
                .d2d_factory
                .cast::<windows::Win32::Graphics::Direct2D::ID2D1Factory>()
            else {
                // Without the mask, clipping to the rect at least keeps the
                // drawing inside its box.
                self.push_clip(rect);
                return;
            };
            let rr = windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                rect: self.snap(rect).d2d(),
                radiusX: radius,
                radiusY: radius,
            };
            let Ok(mask) = factory.CreateRoundedRectangleGeometry(&rr) else {
                self.push_clip(rect);
                return;
            };
            let params = windows::Win32::Graphics::Direct2D::D2D1_LAYER_PARAMETERS1 {
                contentBounds: windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F {
                    left: f32::NEG_INFINITY,
                    top: f32::NEG_INFINITY,
                    right: f32::INFINITY,
                    bottom: f32::INFINITY,
                },
                geometricMask: std::mem::ManuallyDrop::new(Some(mask.into())),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: windows_numerics::Matrix3x2::identity(),
                opacity: 1.0,
                opacityBrush: std::mem::ManuallyDrop::new(None),
                layerOptions: windows::Win32::Graphics::Direct2D::D2D1_LAYER_OPTIONS1_NONE,
            };
            self.note_clip(rect);
            self.ctx.PushLayer(&params, None);
        }
    }

    fn pop_clip_rounded(&self) {
        let _ = self.clips.borrow_mut().pop();
        unsafe { self.ctx.PopLayer() };
    }

    // ── Background-colour stack ─────────────────────────────────────────────
    // Real implementation of the trait's three default methods, backed by
    // a `RefCell<Vec<_>>`. A paint pass is single-threaded, so a cell suffices;
    // `push_bg`/`pop_bg` mirror the clip stack right above.

    fn current_bg(&self) -> D2D1_COLOR_F {
        // With nothing pushed, the outermost surface is what the host cleared
        // to before the paint started (see `set_ground`).
        self.bg_stack.borrow().last().copied().unwrap_or(self.ground.get())
    }

    fn push_bg(&self, colour: D2D1_COLOR_F) {
        self.bg_stack.borrow_mut().push(colour);
    }

    fn pop_bg(&self) {
        // A mismatched pop from an ill-behaved container is worth noticing —
        // eating it silently would let two containers' pushes stay stacked
        // past their scope, but panicking on it in release would be worse.
        let _ = self.bg_stack.borrow_mut().pop();
    }

    // ── Offset & extent ─────────────────────────────────────────────────────
    // A pure translation on the device context, rounded to whole device
    // pixels so the pixel snapping of every primitive stays exact.

    fn push_offset(&self, dx: f32, dy: f32) {
        let (dx, dy) = (self.px(dx), self.px(dy));
        let prev = self.origin.get();
        self.offsets.borrow_mut().push((self.base_transform(), prev));
        self.origin.set((prev.0 + dx, prev.1 + dy));
        unsafe { self.ctx.SetTransform(&self.base_transform()) };
        super::shift_content_offset(dx, dy);
    }

    fn pop_offset(&self) {
        let Some((transform, origin)) = self.offsets.borrow_mut().pop() else { return };
        let now = self.origin.get();
        self.origin.set(origin);
        unsafe { self.ctx.SetTransform(&transform) };
        super::shift_content_offset(origin.0 - now.0, origin.1 - now.1);
    }

    fn begin_extent(&self) {
        let clip_depth = self.clips.borrow().len();
        self.extents.borrow_mut().push(ExtentScope { origin: self.origin.get(), clip_depth, reach: None });
    }

    fn end_extent(&self) -> Option<(f32, f32)> {
        let scope = self.extents.borrow_mut().pop()?;
        scope.reach.map(|(x, y)| (x - scope.origin.0, y - scope.origin.1))
    }

    // ── Device access (the `Graphics` layer of `kubuno_desktop_ui::graphics`) ───────

    fn graphics_renderer(&self) -> Option<&Renderer> {
        Some(self.renderer)
    }

    fn note_drawn(&self, rect: &Rect) {
        self.touch(rect);
    }
}

/// What makes this surface usable by a *control* rather than by any Kubuno
/// drawing: it can answer for the system's own visuals.
///
/// Everything else the trait offers — square fills, square strokes, the 3-D
/// edge — is a provided method built from `Canvas` primitives, so there is one
/// implementation of a WinForms bevel in the library and not one per host.
impl ControlCanvas for Painter<'_> {
    fn visuals(&self) -> &Visuals {
        self.visuals
    }

    fn renderer(&self) -> Option<&Renderer> {
        Some(self.renderer)
    }

    fn draw_bitmap(&self, bitmap: &ID2D1Bitmap1, dest: &Rect, alpha: f32) {
        self.touch(dest);
        unsafe {
            self.ctx.DrawBitmap(bitmap, Some(&dest.d2d()), alpha.clamp(0.0, 1.0), D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, None, None);
        }
    }

    /// The one override: this is the only surface holding both a Direct2D
    /// device (to upload a part) and the window's part cache (so it is uploaded
    /// once, not once per frame).
    ///
    /// The blit is `NEAREST_NEIGHBOR` and the destination comes from the cache
    /// rather than from `rect`. Both are deliberate: [`ThemeRenderer::draw_part`]
    /// rasterises the part at the exact device-pixel size of the snapped
    /// rectangle, so the only correct blit is 1:1. Interpolating it — or letting
    /// the caller's unsnapped rectangle decide the destination — resamples a
    /// one-pixel theme border into a two-pixel grey smear, which is precisely
    /// the "close but not identical" this whole path exists to eliminate.
    fn draw_theme_part(
        &self,
        class: &str,
        part: i32,
        state: i32,
        rect: Rect,
        background: D2D1_COLOR_F,
    ) -> bool {
        let Some(class) = ThemeClass::from_name(class) else { return false };
        let Some(cached) = self.parts.draw_part(self.ctx, class, part, state, rect, background)
        else {
            return false;
        };
        unsafe {
            self.ctx.DrawBitmap(
                &cached.bitmap,
                Some(&cached.dest.d2d()),
                1.0,
                D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                None,
                None,
            );
        }
        true
    }
}

impl Painter<'_> {
    // The three WinForms primitives, also reachable WITHOUT a trait object —
    // the host's own chrome and the demo binaries hold a concrete `Painter`.
    // They forward to the `ControlCanvas` defaults rather than reimplementing
    // them, so a bevel drawn by the host and a bevel drawn by a control are the
    // same pixels by construction.

    /// A square filled rectangle — see [`ControlCanvas::fill_rect`].
    pub fn fill_rect(&self, rect: &Rect, color: &D2D1_COLOR_F) {
        ControlCanvas::fill_rect(self, rect, color);
    }

    /// A square one-pixel outline — see [`ControlCanvas::stroke_rect`].
    pub fn stroke_rect(&self, rect: &Rect, color: &D2D1_COLOR_F) {
        ControlCanvas::stroke_rect(self, rect, color);
    }

    /// A `DrawEdge` bevel, returning the interior — see
    /// [`ControlCanvas::draw_edge`].
    pub fn draw_edge(&self, rect: &Rect, style: Border3DStyle, sides: Border3DSide) -> Rect {
        ControlCanvas::draw_edge(self, rect, style, sides)
    }

    /// The web's layered `box-shadow`, rebuilt as concentric rounded rects
    /// whose alphas add up (Direct2D has no `box-shadow`). Same recipe as
    /// Drive's, including the two values that are easy to get wrong: the CSS
    /// SPREAD inflates the shape before the blur, and a blur radius reaches
    /// about three quarters of its value, not all of it.
    pub fn draw_layered_shadow(
        &self,
        rect: &Rect,
        radius: f32,
        layers: &[kubuno_drive_desktop_app_controls::themes::shape::ShadowLayer],
        colour: (f32, f32, f32),
    ) {
        let (sr, sg, sb) = colour;
        for layer in layers {
            // A CSS blur of radius B is a gaussian centred ON the spread
            // boundary: the shadow is already half faded there, and reaches
            // about B/2 either side. Holding full opacity all the way out to
            // `spread` and only then ramping — as this used to — leaves a hard
            // step where the solid collar ends, seen as a darker band with an
            // edge rather than an even gradient.
            let half = (layer.blur / 2.0).max(0.01);
            // A NEGATIVE spread shrinks the shape before the blur — every
            // Tailwind `shadow-md/lg/xl` layer has one (`0 20px 25px -5px`). The
            // ramp must then start INSIDE the panel's edge, or the full-size
            // shape offset by `dy` shows below the panel as a hard grey slab.
            // A spread ≥ 0 keeps the ramp it always had, starting at the edge.
            let inner = if layer.spread < 0.0 {
                layer.spread - half
            } else {
                (layer.spread - half).max(0.0)
            };
            let outer = layer.spread + half;
            let span = (outer - inner).max(0.01);
            // How far in the rings march: to the edge, or inside it when the
            // spread is negative — never past the panel's own half-size, where a
            // ring would turn inside out.
            let half_size = ((rect.right - rect.left).min(rect.bottom - rect.top) / 2.0).max(0.0);
            let lo = inner.min(0.0).max(-half_size);
            if outer <= lo {
                continue;
            }
            // Opacity at distance `d` beyond the panel's edge, smooth at both
            // ends so neither the start nor the finish of the ramp shows.
            let profile = |d: f32| {
                let x = ((d - inner) / span).clamp(0.0, 1.0);
                layer.opacity * (1.0 - x * x * (3.0 - 2.0 * x))
            };
            // Two rings per DEVICE pixel, so a step is always finer than what a
            // pixel can show — whatever the shadow's reach or the DPI scale.
            let steps = (((outer - lo) * self.scale.get() * 2.0).ceil() as usize).clamp(16, 96);
            // Rings composite source-over, which stacks MULTIPLICATIVELY: the
            // increments are taken in optical depth, or the total comes out
            // short of the layer's opacity.
            let mut laid = 0.0f32;
            for i in 0..steps {
                // Outermost first, marching in towards the panel's own edge.
                let d = outer - (outer - lo) * (i as f32 / (steps - 1) as f32);
                let depth = -(1.0 - profile(d).min(0.999)).ln();
                let step = depth - laid;
                if step <= 0.0005 {
                    continue;
                }
                laid = depth;
                let ring = Rect::new(
                    rect.left - d,
                    rect.top - d + layer.dy,
                    rect.right + d,
                    rect.bottom + d + layer.dy,
                );
                // Deliberately NOT snapped to the pixel grid. Rings this close
                // together snap onto one another, piling several alphas into a
                // single pixel ring — which is itself a dark band, the artefact
                // all of this exists to avoid.
                let rr = D2D1_ROUNDED_RECT {
                    rect: ring.d2d(),
                    radiusX: (radius + d).max(0.0),
                    radiusY: (radius + d).max(0.0),
                };
                let color =
                    D2D1_COLOR_F { r: sr, g: sg, b: sb, a: 1.0 - (-step).exp() };
                unsafe { self.ctx.FillRoundedRectangle(&rr, self.set_color(&color)) };
            }
        }
    }
}
