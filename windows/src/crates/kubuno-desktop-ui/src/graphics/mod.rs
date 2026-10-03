//! # `Graphics` — a WinForms-`Graphics`-like drawing API over the Kubuno [`Canvas`]
//!
//! The [`Canvas`] a widget paints through offers the primitives the design system needs (rounded
//! fills, strokes, text in a box, icons). A custom control or an owner-drawn list needs more: lines
//! and polygons, ellipses, arcs and pies, Bézier curves and paths, linear and radial gradients,
//! dashed pens with caps and joins, images, text laid out, measured, aligned, wrapped and trimmed,
//! clipping to any shape, transforms, saved states and rendering hints. [`Graphics`] is that
//! surface, shaped after `System.Drawing.Graphics` so a WinForms `OnPaint` reads the same:
//!
//! ```ignore
//! let g = &e.graphics;
//! g.set_smoothing_mode(SmoothingMode::AntiAlias);
//! let pen = Pen::new(Color::rgb(0x33, 0x66, 0xFF), 3.0).with_dash(DashStyle::Dash);
//! g.draw_ellipse(&pen, r);
//! g.fill_path(LinearGradientBrush::from_rect(r, c1, c2, LinearGradientMode::Vertical), &path);
//! g.draw_string("42 %", &Font::new("Segoe UI", 14.0, FontStyle::BOLD), Color::BLACK, r, &StringFormat::centered());
//! ```
//!
//! ## How it draws
//!
//! A `Graphics` is a thin, borrowed view over a canvas: it owns no device. When the canvas lends its
//! Direct2D renderer ([`Canvas::graphics_renderer`], the host's painter does) every call is drawn
//! with Direct2D directly, in the same `BeginDraw` as the widgets, with the transform, clip and
//! hints of the `Graphics` applied around that one call and the device context restored after it —
//! so interleaving `Graphics` calls with the canvas' own primitives is safe. Without a renderer (a
//! canvas of another app, a test double) the calls fall back to the canvas primitives, as close as
//! they allow (gradients → their middle colour, no rotation of images…).
//!
//! Every call is an [`display::Op`]: a `Graphics` can also **record** them into a
//! [`display::DisplayList`] ([`Graphics::recording`], [`Graphics::recorder`]) — what the unit tests
//! assert on, and what a control's paint buffer keeps and replays while the control is valid.
//!
//! ## Coordinates and state
//!
//! Coordinates are the canvas' own DIP (the same space as the bounds a control is painted in), not
//! a control's client coordinates: `g.translate_transform(bounds.left, bounds.top)` draws in local
//! coordinates. The transform, clip and hints apply to the `Graphics` calls only; the canvas
//! primitives called through the `Graphics` ([`Canvas`] is implemented on it, so existing paint code
//! keeps working) draw in the surface's own state. All methods take `&self`: a `Graphics` is lent,
//! never replaced — which is also what lets an event's args carry one ([`GraphicsSlot`]).

pub mod display;
mod d2d;
mod fallback;
pub mod image;
pub mod owner_draw;
pub mod paint;
pub mod path;
mod scoped;
pub mod testing;
pub mod text;
pub mod types;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use kubuno_drive_desktop_app_controls::{Canvas, Rect, Renderer, TextFormats, Theme};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap1;
use windows::Win32::Graphics::DirectWrite::{IDWriteTextFormat, DWRITE_TEXT_ALIGNMENT};

pub use display::{CanvasCall, ClipItem, ClipShape, DisplayList, Op, OpState, Shape};
pub use image::Image;
pub use owner_draw::{DrawItemEventArgs, DrawItemState, DrawMode, MeasureItemEventArgs, OwnerDrawHandler};
pub use paint::{Brush, DashStyle, GradientStop, LineCap, LineJoin, LinearGradientBrush, LinearGradientMode, Pen, PenAlignment, RadialGradientBrush, WrapMode};
pub use path::{FillMode, GraphicsPath};
pub use scoped::GraphicsSlot;
pub use text::{
    CompositingMode, Font, FontStyle, InterpolationMode, SmoothingMode, StringAlignment, StringFormat, StringFormatFlags, StringTrimming, TextRenderingHint,
};
pub use types::{Color, Matrix, MatrixOrder, PointF, RectExt, SizeF};
pub use kubuno_desktop_controls::control::FontRole;

/// A saved drawing state (`GraphicsState`), from [`Graphics::save`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphicsState(u32);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Direct2D through the canvas' renderer.
    Direct,
    /// The canvas primitives.
    Fallback,
    /// Nothing is drawn (a recorder, a null graphics).
    None,
}

/// The WinForms-like drawing surface (see the module doc).
pub struct Graphics<'a> {
    canvas: Option<&'a dyn Canvas>,
    renderer: Option<&'a Renderer>,
    mode: Mode,
    state: RefCell<OpState>,
    saved: RefCell<Vec<(u32, OpState)>>,
    next_id: Cell<u32>,
    recording: RefCell<Option<DisplayList>>,
    /// Something reached the canvas without going through the recording (see
    /// [`Graphics::raw_canvas`]): a recorded display list is then incomplete.
    unrecorded: Cell<bool>,
    cache: RefCell<d2d::Cache>,
}

impl<'a> Graphics<'a> {
    /// A `Graphics` over `canvas` (`Graphics.FromHdc`): Direct2D when the canvas lends its renderer,
    /// else its primitives.
    pub fn new(canvas: &'a dyn Canvas) -> Self {
        let renderer = canvas.graphics_renderer();
        let mode = if renderer.is_some() { Mode::Direct } else { Mode::Fallback };
        Self::build(Some(canvas), renderer, mode)
    }

    /// Records while drawing ([`Graphics::take_recording`]).
    pub fn recording(self) -> Self {
        *self.recording.borrow_mut() = Some(DisplayList::new());
        self
    }

    fn build(canvas: Option<&'a dyn Canvas>, renderer: Option<&'a Renderer>, mode: Mode) -> Self {
        Self {
            canvas,
            renderer,
            mode,
            state: RefCell::new(OpState::default()),
            saved: RefCell::new(Vec::new()),
            next_id: Cell::new(1),
            recording: RefCell::new(None),
            unrecorded: Cell::new(false),
            cache: RefCell::new(d2d::Cache::default()),
        }
    }
}

impl Graphics<'static> {
    /// A `Graphics` that draws nothing and records every call — for tests, and for measuring what
    /// a paint would draw.
    pub fn recorder() -> Self {
        Self::build(None, None, Mode::None).recording()
    }

    /// A `Graphics` that draws and records nothing (the one an event's args answer outside the
    /// paint that lent a real one).
    pub fn null() -> Self {
        Self::build(None, None, Mode::None)
    }
}

impl<'a> Graphics<'a> {
    // ── Surface ────────────────────────────────────────────────────────────────────────────────

    /// Whether calls are drawn with Direct2D (the full API), rather than approximated with the
    /// canvas primitives or not drawn at all.
    pub fn has_device(&self) -> bool {
        self.mode == Mode::Direct
    }

    /// Whether it draws anywhere.
    pub fn is_null(&self) -> bool {
        self.mode == Mode::None && self.recording.borrow().is_none()
    }

    /// The canvas underneath, for drawing that must bypass the `Graphics` (and its recording):
    /// what is drawn this way is not in [`Graphics::take_recording`], and a paint buffer built from
    /// this `Graphics` is not kept. Prefer the `Canvas` methods on the `Graphics` itself, which are
    /// recorded.
    pub fn raw_canvas(&self) -> Option<&'a dyn Canvas> {
        self.unrecorded.set(true);
        self.canvas
    }

    /// The renderer (Direct2D device context, DirectWrite factory), when the surface lends it —
    /// for code that draws with Direct2D itself. Like [`Graphics::raw_canvas`], not recorded.
    pub fn renderer(&self) -> Option<&'a Renderer> {
        self.unrecorded.set(true);
        self.renderer
    }

    /// Whether something was drawn bypassing the recording since it started.
    pub fn has_unrecorded_drawing(&self) -> bool {
        self.unrecorded.get()
    }

    /// Stops recording and returns what was recorded (`None` when not recording).
    pub fn take_recording(&self) -> Option<DisplayList> {
        self.recording.borrow_mut().take()
    }

    /// The ops recorded so far (a copy), without stopping.
    pub fn recorded(&self) -> Option<DisplayList> {
        self.recording.borrow().clone()
    }

    /// The DPI scale of the surface (`DpiX / 96`).
    pub fn dpi_scale(&self) -> f32 {
        self.canvas.map_or(1.0, |c| c.scale())
    }

    /// Draws `op` (in its own state) and records it.
    pub fn emit(&self, op: Op) {
        match self.mode {
            Mode::Direct => {
                if let (Some(renderer), Some(canvas)) = (self.renderer, self.canvas) {
                    d2d::execute(renderer, canvas, &self.cache, &op);
                }
            }
            Mode::Fallback => {
                if let Some(canvas) = self.canvas {
                    fallback::execute(canvas, &op);
                }
            }
            Mode::None => {}
        }
        if let Some(list) = self.recording.borrow_mut().as_mut() {
            list.ops.push(op);
        }
    }

    fn snapshot(&self) -> OpState {
        self.state.borrow().clone()
    }

    // ── Transform ──────────────────────────────────────────────────────────────────────────────

    /// The world transform (`Graphics.Transform`).
    pub fn transform(&self) -> Matrix {
        self.state.borrow().transform
    }

    pub fn set_transform(&self, m: Matrix) {
        self.state.borrow_mut().transform = m;
    }

    pub fn reset_transform(&self) {
        self.set_transform(Matrix::IDENTITY);
    }

    /// `MultiplyTransform(m, order)`.
    pub fn multiply_transform(&self, m: &Matrix, order: MatrixOrder) {
        self.state.borrow_mut().transform.multiply(m, order);
    }

    /// `TranslateTransform(dx, dy)` (prepended, like WinForms).
    pub fn translate_transform(&self, dx: f32, dy: f32) {
        self.multiply_transform(&Matrix::translation(dx, dy), MatrixOrder::Prepend);
    }

    /// `ScaleTransform(sx, sy)` (prepended).
    pub fn scale_transform(&self, sx: f32, sy: f32) {
        self.multiply_transform(&Matrix::scaling(sx, sy), MatrixOrder::Prepend);
    }

    /// `RotateTransform(degrees)` (prepended; clockwise on screen, around the current origin).
    pub fn rotate_transform(&self, degrees: f32) {
        self.multiply_transform(&Matrix::rotation(degrees), MatrixOrder::Prepend);
    }

    /// Rotates by `degrees` around `center` (in the current coordinates).
    pub fn rotate_transform_at(&self, degrees: f32, center: PointF) {
        self.multiply_transform(&Matrix::rotation_at(degrees, center), MatrixOrder::Prepend);
    }

    // ── Hints ──────────────────────────────────────────────────────────────────────────────────

    pub fn smoothing_mode(&self) -> SmoothingMode {
        self.state.borrow().smoothing
    }

    /// `SmoothingMode`: antialiased edges (the default) or aliased ones.
    pub fn set_smoothing_mode(&self, mode: SmoothingMode) {
        self.state.borrow_mut().smoothing = mode;
    }

    pub fn text_rendering_hint(&self) -> TextRenderingHint {
        self.state.borrow().text_hint
    }

    pub fn set_text_rendering_hint(&self, hint: TextRenderingHint) {
        self.state.borrow_mut().text_hint = hint;
    }

    pub fn interpolation_mode(&self) -> InterpolationMode {
        self.state.borrow().interpolation
    }

    pub fn set_interpolation_mode(&self, mode: InterpolationMode) {
        self.state.borrow_mut().interpolation = mode;
    }

    pub fn compositing_mode(&self) -> CompositingMode {
        self.state.borrow().compositing
    }

    pub fn set_compositing_mode(&self, mode: CompositingMode) {
        self.state.borrow_mut().compositing = mode;
    }

    // ── Clip ───────────────────────────────────────────────────────────────────────────────────

    fn push_clip_item(&self, shape: ClipShape, replace: bool) {
        let mut st = self.state.borrow_mut();
        let item = ClipItem { shape, transform: st.transform, antialias: st.smoothing.antialiased() };
        let clips = Rc::make_mut(&mut st.clips);
        if replace {
            clips.clear();
        }
        clips.push(item);
    }

    /// `SetClip(rect)`: the clip becomes `rect` (in the current coordinates).
    pub fn set_clip(&self, rect: Rect) {
        self.push_clip_item(ClipShape::Rect(rect), true);
    }

    /// `SetClip(path)`.
    pub fn set_clip_path(&self, path: &GraphicsPath) {
        self.push_clip_item(ClipShape::Path(path.clone()), true);
    }

    /// `IntersectClip(rect)`: the clip shrinks to its overlap with `rect`.
    pub fn intersect_clip(&self, rect: Rect) {
        self.push_clip_item(ClipShape::Rect(rect), false);
    }

    /// `IntersectClip(region)` with a path.
    pub fn intersect_clip_path(&self, path: &GraphicsPath) {
        self.push_clip_item(ClipShape::Path(path.clone()), false);
    }

    /// `ResetClip()`: no clip.
    pub fn reset_clip(&self) {
        let mut st = self.state.borrow_mut();
        Rc::make_mut(&mut st.clips).clear();
    }

    /// Whether a clip is set (`!Clip.IsInfinite`).
    pub fn is_clipped(&self) -> bool {
        !self.state.borrow().clips.is_empty()
    }

    /// The bounding box of the clip in the current coordinates (`ClipBounds`), `None` when there is
    /// no clip.
    pub fn clip_bounds(&self) -> Option<Rect> {
        let st = self.state.borrow();
        let b = st.clip_bounds()?;
        Some(st.transform.inverted().map_or(b, |inv| inv.transform_bounds(&b)))
    }

    /// Whether `rect` (current coordinates) can show through the clip (`IsVisible(rect)`).
    pub fn is_visible(&self, rect: Rect) -> bool {
        let st = self.state.borrow();
        let world = st.transform.transform_bounds(&rect);
        st.clip_bounds().is_none_or(|c| c.intersect(&world).is_some())
    }

    // ── Save / restore ─────────────────────────────────────────────────────────────────────────

    /// `Save()`: remembers the transform, clip and hints.
    pub fn save(&self) -> GraphicsState {
        let id = self.next_id.get();
        self.next_id.set(id.wrapping_add(1));
        self.saved.borrow_mut().push((id, self.snapshot()));
        GraphicsState(id)
    }

    /// `Restore(state)`: back to what `state` saved; the states saved after it are discarded. An
    /// unknown (already restored) state does nothing, like WinForms.
    pub fn restore(&self, state: GraphicsState) {
        let mut saved = self.saved.borrow_mut();
        let Some(pos) = saved.iter().rposition(|(id, _)| *id == state.0) else { return };
        let (_, st) = saved.remove(pos);
        saved.truncate(pos);
        *self.state.borrow_mut() = st;
    }

    /// Runs `f` and restores the state it found (`Save` … `Restore`, even on an early return).
    pub fn with_saved<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        let s = self.save();
        let r = f(self);
        self.restore(s);
        r
    }

    // ── Filling and outlining ──────────────────────────────────────────────────────────────────

    fn fill(&self, shape: Shape, brush: Brush) {
        if brush.is_invisible() {
            return;
        }
        self.emit(Op::Fill { shape, brush, state: self.snapshot() });
    }

    fn stroke(&self, shape: Shape, pen: &Pen) {
        if pen.width <= 0.0 || pen.brush.is_invisible() {
            return;
        }
        self.emit(Op::Stroke { shape, pen: pen.clone(), state: self.snapshot() });
    }

    /// `DrawLine`.
    pub fn draw_line(&self, pen: &Pen, p1: PointF, p2: PointF) {
        self.stroke(Shape::Line(p1, p2), pen);
    }

    /// `DrawLines`: a polyline.
    pub fn draw_lines(&self, pen: &Pen, points: &[PointF]) {
        if points.len() < 2 {
            return;
        }
        let mut p = GraphicsPath::new();
        p.add_lines(points);
        self.stroke(Shape::Path(p), pen);
    }

    /// `DrawRectangle`.
    pub fn draw_rectangle(&self, pen: &Pen, rect: Rect) {
        self.stroke(Shape::Rect(rect), pen);
    }

    /// `DrawRectangles`.
    pub fn draw_rectangles(&self, pen: &Pen, rects: &[Rect]) {
        for r in rects {
            self.draw_rectangle(pen, *r);
        }
    }

    /// `FillRectangle`.
    pub fn fill_rectangle(&self, brush: impl Into<Brush>, rect: Rect) {
        self.fill(Shape::Rect(rect), brush.into());
    }

    /// `FillRectangles`.
    pub fn fill_rectangles(&self, brush: impl Into<Brush>, rects: &[Rect]) {
        let brush = brush.into();
        for r in rects {
            self.fill(Shape::Rect(*r), brush.clone());
        }
    }

    /// `DrawRoundedRectangle` (.NET 9): corners of `radius`.
    pub fn draw_rounded_rectangle(&self, pen: &Pen, rect: Rect, radius: f32) {
        self.stroke(Shape::RoundedRect(rect, radius.max(0.0), radius.max(0.0)), pen);
    }

    /// `FillRoundedRectangle` (.NET 9).
    pub fn fill_rounded_rectangle(&self, brush: impl Into<Brush>, rect: Rect, radius: f32) {
        self.fill(Shape::RoundedRect(rect, radius.max(0.0), radius.max(0.0)), brush.into());
    }

    /// `DrawEllipse`: the ellipse inscribed in `rect`.
    pub fn draw_ellipse(&self, pen: &Pen, rect: Rect) {
        self.stroke(Shape::Ellipse(rect), pen);
    }

    /// `FillEllipse`.
    pub fn fill_ellipse(&self, brush: impl Into<Brush>, rect: Rect) {
        self.fill(Shape::Ellipse(rect), brush.into());
    }

    /// `DrawArc`: part of the ellipse inscribed in `rect`, from `start_angle` over `sweep_angle`
    /// degrees (clockwise, 0 = 3 o'clock).
    pub fn draw_arc(&self, pen: &Pen, rect: Rect, start_angle: f32, sweep_angle: f32) {
        let mut p = GraphicsPath::new();
        p.add_arc(rect, start_angle, sweep_angle);
        self.stroke(Shape::Path(p), pen);
    }

    /// `DrawPie`.
    pub fn draw_pie(&self, pen: &Pen, rect: Rect, start_angle: f32, sweep_angle: f32) {
        let mut p = GraphicsPath::new();
        p.add_pie(rect, start_angle, sweep_angle);
        self.stroke(Shape::Path(p), pen);
    }

    /// `FillPie`.
    pub fn fill_pie(&self, brush: impl Into<Brush>, rect: Rect, start_angle: f32, sweep_angle: f32) {
        let mut p = GraphicsPath::new();
        p.add_pie(rect, start_angle, sweep_angle);
        self.fill(Shape::Path(p), brush.into());
    }

    /// `DrawBezier`.
    pub fn draw_bezier(&self, pen: &Pen, p1: PointF, c1: PointF, c2: PointF, p2: PointF) {
        let mut p = GraphicsPath::new();
        p.add_bezier(p1, c1, c2, p2);
        self.stroke(Shape::Path(p), pen);
    }

    /// `DrawBeziers`: a start point then groups of three.
    pub fn draw_beziers(&self, pen: &Pen, points: &[PointF]) {
        let mut p = GraphicsPath::new();
        p.add_beziers(points);
        self.stroke(Shape::Path(p), pen);
    }

    /// `DrawCurve`: a cardinal spline through `points` (tension 0.5 is WinForms' default).
    pub fn draw_curve(&self, pen: &Pen, points: &[PointF], tension: f32) {
        let mut p = GraphicsPath::new();
        p.add_curve(points, tension);
        self.stroke(Shape::Path(p), pen);
    }

    /// `DrawClosedCurve`.
    pub fn draw_closed_curve(&self, pen: &Pen, points: &[PointF], tension: f32) {
        let mut p = GraphicsPath::new();
        p.add_closed_curve(points, tension);
        self.stroke(Shape::Path(p), pen);
    }

    /// `FillClosedCurve`.
    pub fn fill_closed_curve(&self, brush: impl Into<Brush>, points: &[PointF], tension: f32) {
        let mut p = GraphicsPath::new();
        p.add_closed_curve(points, tension);
        self.fill(Shape::Path(p), brush.into());
    }

    /// `DrawPolygon`.
    pub fn draw_polygon(&self, pen: &Pen, points: &[PointF]) {
        let mut p = GraphicsPath::new();
        p.add_polygon(points);
        self.stroke(Shape::Path(p), pen);
    }

    /// `FillPolygon` (even-odd).
    pub fn fill_polygon(&self, brush: impl Into<Brush>, points: &[PointF]) {
        let mut p = GraphicsPath::new();
        p.add_polygon(points);
        self.fill(Shape::Path(p), brush.into());
    }

    /// `DrawPath`.
    pub fn draw_path(&self, pen: &Pen, path: &GraphicsPath) {
        if !path.is_empty() {
            self.stroke(Shape::Path(path.clone()), pen);
        }
    }

    /// `FillPath` (by the path's fill mode).
    pub fn fill_path(&self, brush: impl Into<Brush>, path: &GraphicsPath) {
        if !path.is_empty() {
            self.fill(Shape::Path(path.clone()), brush.into());
        }
    }

    /// `Clear(color)`: the clip region (everything when unclipped) becomes `color`, replacing what
    /// is there — transparent included.
    pub fn clear(&self, color: impl Into<Color>) {
        self.emit(Op::Clear { color: color.into(), state: self.snapshot() });
    }

    // ── Text ───────────────────────────────────────────────────────────────────────────────────

    /// `DrawString(text, font, brush, layoutRectangle, format)`: laid out in `rect` — wrapped,
    /// aligned both ways and trimmed as `format` says, clipped to `rect` unless `NO_CLIP`.
    pub fn draw_string(&self, text: &str, font: &Font, brush: impl Into<Brush>, rect: Rect, format: &StringFormat) {
        let brush = brush.into();
        if text.is_empty() || brush.is_invisible() {
            return;
        }
        self.emit(Op::Text { text: text.to_string(), font: font.clone(), brush, layout: rect, format: *format, state: self.snapshot() });
    }

    /// `DrawString(text, font, brush, point)`: one unwrapped line per paragraph from `point` (its
    /// top-left), never clipped.
    pub fn draw_string_at(&self, text: &str, font: &Font, brush: impl Into<Brush>, point: PointF) {
        let (w, h) = self.measure_raw(text, font, None, &StringFormat::generic_default().with_flags(StringFormatFlags::NO_WRAP));
        let rect = Rect::new(point.x, point.y, point.x + w.max(1.0) + 1.0, point.y + h.max(1.0));
        let format = StringFormat::generic_default().with_flags(StringFormatFlags::NO_WRAP | StringFormatFlags::NO_CLIP).with_trimming(StringTrimming::None);
        self.draw_string(text, font, brush, rect, &format);
    }

    /// `MeasureString(text, font, width, format)`: the size the text takes, wrapped at `max_width`
    /// when given (and the format wraps).
    pub fn measure_string(&self, text: &str, font: &Font, max_width: Option<f32>, format: &StringFormat) -> SizeF {
        let (w, h) = self.measure_raw(text, font, max_width, format);
        SizeF::new(w, h)
    }

    fn measure_raw(&self, text: &str, font: &Font, max_width: Option<f32>, format: &StringFormat) -> (f32, f32) {
        match (self.mode, self.renderer, self.canvas) {
            (Mode::Direct, Some(renderer), Some(canvas)) => d2d::measure(renderer, canvas, text, font, max_width, format),
            (_, _, Some(canvas)) if font.is_plain_role() && !text.contains('\n') && max_width.is_none() => {
                let f = role_format(canvas.formats(), font.role);
                (canvas.measure(text, f), font.height())
            }
            _ => text::approximate_measure(text, font, max_width.filter(|_| format.wraps())),
        }
    }

    // ── Images and icons ───────────────────────────────────────────────────────────────────────

    /// `DrawImage(image, rect)`: stretched into `dest`.
    pub fn draw_image(&self, image: &Image, dest: Rect) {
        self.draw_image_with(image, dest, None, 1.0);
    }

    /// `DrawImage(image, destRect, srcRect, …)`: the `src` part of the image (in its DIP) into
    /// `dest`, `opacity` opaque (`ImageAttributes`' matrix alpha).
    pub fn draw_image_with(&self, image: &Image, dest: Rect, src: Option<Rect>, opacity: f32) {
        if opacity <= 0.0 {
            return;
        }
        self.emit(Op::Image { image: image.clone(), dest, src, opacity: opacity.min(1.0), state: self.snapshot() });
    }

    /// `DrawImageUnscaled`: at its own size from `point`.
    pub fn draw_image_unscaled(&self, image: &Image, point: PointF) {
        let size = self.image_size(image).unwrap_or_default();
        self.draw_image(image, Rect::new(point.x, point.y, point.x + size.width, point.y + size.height));
    }

    /// The size of `image` in DIP, once decoded for this surface (`None` without a device, or for a
    /// file that cannot be read).
    pub fn image_size(&self, image: &Image) -> Option<SizeF> {
        image.size(self.renderer?)
    }

    /// Draws the Kubuno vector icon `name` (the icon set every control uses: `"folder"`,
    /// `"check"`…) `size` DIP square, centred in `rect`, in `color`. Follows the translation and
    /// scale of the transform (not rotation).
    pub fn draw_icon(&self, name: &'static str, rect: Rect, size: f32, color: impl Into<Color>) {
        self.emit(Op::Icon { name, rect, size, color: color.into(), state: self.snapshot() });
    }

    // ── Theme ──────────────────────────────────────────────────────────────────────────────────

    /// The surface's theme (the light theme without a surface).
    pub fn theme_colors(&self) -> Theme {
        self.canvas.map_or_else(Theme::light, |c| c.theme().clone())
    }

    fn canvas_call(&self, call: CanvasCall) {
        self.emit(Op::Canvas(call));
    }
}

/// The shared text format of a role.
pub fn role_format(f: &TextFormats, role: FontRole) -> &IDWriteTextFormat {
    match role {
        FontRole::Caption => &f.caption,
        FontRole::CaptionStrong => &f.caption_strong,
        FontRole::Body => &f.body,
        FontRole::BodyStrong => &f.body_strong,
        FontRole::Heading => &f.heading,
        FontRole::Title => &f.title,
    }
}

/// The canvas primitives on a `Graphics` — so paint code written for a [`Canvas`] runs on one
/// unchanged. Each call is recorded (and drawn in the surface's own state: the `Graphics`
/// transform and clip do not apply to them). Without a surface, the theme and text formats are
/// those of a headless stand-in and nothing is drawn.
impl Canvas for Graphics<'_> {
    fn theme(&self) -> &Theme {
        match self.canvas {
            Some(c) => c.theme(),
            None => testing::headless_theme(),
        }
    }

    fn formats(&self) -> &TextFormats {
        match self.canvas {
            Some(c) => c.formats(),
            None => testing::headless_formats(),
        }
    }

    fn scale(&self) -> f32 {
        self.dpi_scale()
    }

    fn fill_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        self.canvas_call(CanvasCall::FillRounded(*rect, radius, color.into()));
    }

    fn fill_top_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        self.canvas_call(CanvasCall::FillTopRounded(*rect, radius, color.into()));
    }

    fn fill_triangle(&self, a: (f32, f32), b: (f32, f32), c: (f32, f32), color: &D2D1_COLOR_F) {
        self.canvas_call(CanvasCall::FillTriangle(a, b, c, color.into()));
    }

    fn stroke_arc(&self, centre: (f32, f32), radius: f32, start: f32, sweep: f32, width: f32, color: &D2D1_COLOR_F) {
        self.canvas_call(CanvasCall::StrokeArc(centre, radius, start, sweep, width, color.into()));
    }

    fn stroke_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        self.canvas_call(CanvasCall::StrokeRounded(*rect, radius, color.into()));
    }

    fn stroke_rounded_w(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F, width: f32) {
        self.canvas_call(CanvasCall::StrokeRoundedW(*rect, radius, color.into(), width));
    }

    fn text(&self, text: &str, rect: &Rect, format: &IDWriteTextFormat, color: &D2D1_COLOR_F, centered: bool) {
        self.canvas_call(CanvasCall::Text(text.to_string(), *rect, format.clone(), color.into(), centered));
    }

    fn text_aligned(&self, text: &str, rect: &Rect, format: &IDWriteTextFormat, color: &D2D1_COLOR_F, alignment: DWRITE_TEXT_ALIGNMENT) {
        self.canvas_call(CanvasCall::TextAligned(text.to_string(), *rect, format.clone(), color.into(), alignment));
    }

    fn text_ellipsis(&self, text: &str, rect: &Rect, format: &IDWriteTextFormat, color: &D2D1_COLOR_F) {
        self.canvas_call(CanvasCall::TextEllipsis(text.to_string(), *rect, format.clone(), color.into()));
    }

    fn text_ellipsis_center(&self, text: &str, rect: &Rect, format: &IDWriteTextFormat, color: &D2D1_COLOR_F) {
        self.canvas_call(CanvasCall::TextEllipsisCenter(text.to_string(), *rect, format.clone(), color.into()));
    }

    fn image(&self, bitmap: &ID2D1Bitmap1, rect: &Rect, size: f32) {
        self.canvas_call(CanvasCall::Image(bitmap.clone(), *rect, size, 1.0));
    }

    fn image_alpha(&self, bitmap: &ID2D1Bitmap1, rect: &Rect, size: f32, alpha: f32) {
        self.canvas_call(CanvasCall::Image(bitmap.clone(), *rect, size, alpha));
    }

    fn vector_icon(&self, name: &'static str, rect: &Rect, size: f32, color: &D2D1_COLOR_F) {
        self.canvas_call(CanvasCall::VectorIcon(name, *rect, size, color.into()));
    }

    fn vector_icon_layered(&self, name: &'static str, rect: &Rect, size: f32, fg: &D2D1_COLOR_F, accent: &D2D1_COLOR_F) {
        self.canvas_call(CanvasCall::VectorIconLayered(name, *rect, size, fg.into(), accent.into()));
    }

    fn measure(&self, text: &str, format: &IDWriteTextFormat) -> f32 {
        match self.canvas {
            Some(c) => c.measure(text, format),
            None => {
                // SAFETY: a plain COM getter on a live text format.
                let size = unsafe { format.GetFontSize() };
                text.chars().count() as f32 * size * 0.55
            }
        }
    }

    fn draw_card_shadow(&self, rect: &Rect, radius: f32) {
        self.canvas_call(CanvasCall::CardShadow(*rect, radius));
    }

    fn draw_shadow(&self, rect: &Rect, radius: f32, layers: &[kubuno_drive_desktop_app_controls::themes::shape::ShadowLayer], colour: (f32, f32, f32)) {
        self.canvas_call(CanvasCall::Shadow(*rect, radius, layers.to_vec(), colour));
    }

    fn erase_rounded(&self, rect: &Rect, radius: f32) {
        self.canvas_call(CanvasCall::EraseRounded(*rect, radius));
    }

    fn push_clip(&self, rect: &Rect) {
        self.canvas_call(CanvasCall::PushClip(*rect));
    }

    fn push_clip_rounded(&self, rect: &Rect, radius: f32) {
        self.canvas_call(CanvasCall::PushClipRounded(*rect, radius));
    }

    fn pop_clip_rounded(&self) {
        self.canvas_call(CanvasCall::PopClipRounded);
    }

    fn pop_clip(&self) {
        self.canvas_call(CanvasCall::PopClip);
    }

    fn push_offset(&self, dx: f32, dy: f32) {
        self.canvas_call(CanvasCall::PushOffset(dx, dy));
    }

    fn pop_offset(&self) {
        self.canvas_call(CanvasCall::PopOffset);
    }

    fn begin_extent(&self) {
        self.unrecorded.set(true);
        if let Some(c) = self.canvas {
            c.begin_extent();
        }
    }

    fn end_extent(&self) -> Option<(f32, f32)> {
        self.unrecorded.set(true);
        self.canvas.and_then(|c| c.end_extent())
    }

    fn current_bg(&self) -> D2D1_COLOR_F {
        self.canvas.map_or_else(|| self.theme().window_background, |c| c.current_bg())
    }

    fn push_bg(&self, colour: D2D1_COLOR_F) {
        self.canvas_call(CanvasCall::PushBg(colour.into()));
    }

    fn pop_bg(&self) {
        self.canvas_call(CanvasCall::PopBg);
    }

    fn graphics_renderer(&self) -> Option<&Renderer> {
        self.unrecorded.set(true);
        self.renderer
    }

    fn note_drawn(&self, rect: &Rect) {
        if let Some(c) = self.canvas {
            c.note_drawn(rect);
        }
    }
}

#[cfg(test)]
mod tests;
