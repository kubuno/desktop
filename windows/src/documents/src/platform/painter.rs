//! The Direct2D drawing surface for Kubuno Documents.
//!
//! It implements [`kubuno_drive_desktop_app_controls::Canvas`], the contract every shared
//! control paints through — so the window chrome (ribbon, backstage, status bar,
//! dialogs) gets the whole design system without knowing how it draws.
//!
//! The DOCUMENT BODY does not go through `Canvas`. `Canvas::text` force-centres
//! text vertically in its rect and takes one format for the whole string, which
//! is right for a button label and useless for a paragraph with three fonts in
//! it. The body is laid out and painted with raw DirectWrite against
//! `renderer.dwrite` / `renderer.d2d_context`, both of which are `pub`. Two
//! worlds, one `BeginDraw`.
//!
//! Chat has its own near-identical `Painter`; the two cannot be one type because
//! each crate hangs its own inherent `impl` blocks off it. Two deliberate
//! differences from that copy are marked below — they are fixes, not drift.

use std::cell::Cell;

use kubuno_drive_desktop_app_controls::geometry::Rect;
use kubuno_drive_desktop_app_controls::{Canvas, Renderer, TextFormats, Theme};
use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1DeviceContext, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
    D2D1_PRIMITIVE_BLEND_COPY, D2D1_PRIMITIVE_BLEND_SOURCE_OVER, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER,
};
use windows_numerics::Matrix3x2;

/// The widest line DirectWrite is ever asked to lay out when only the text's
/// natural width is wanted.
///
/// NOT `f32::MAX`: DirectWrite computes intermediate values from the layout box,
/// and at `f32::MAX` those overflow to infinity and the returned metrics come
/// back as NaN for some fonts. Drive hit this and fixed it to the same constant
/// (`drive/crates/kubuno-drive-desktop/src/ui/painter.rs:727-730`); chat's copy still
/// carries the bug, which is why this line is not a verbatim copy.
const UNBOUNDED: f32 = 65536.0;

pub struct Painter<'a> {
    pub ctx:      &'a ID2D1DeviceContext,
    pub renderer: &'a Renderer,
    pub theme:    &'a Theme,
    brush:        ID2D1SolidColorBrush,
    scale:        Cell<f32>,
    /// The transform currently installed on the device context.
    ///
    /// Direct2D has no transform stack, so we keep one. Everything that changes
    /// the transform must go through [`Painter::push_transform`] and restore
    /// this value — see [`TransformGuard`].
    transform:    Cell<Matrix3x2>,
}

/// Restores the previous Direct2D transform when it goes out of scope.
///
/// The page canvas nests transforms (viewport scroll → page origin → zoom), and
/// an early return in the middle of a page must not leave the context skewed for
/// the rest of the frame. Holding the guard makes that impossible to get wrong.
#[must_use = "the transform is restored when the guard is dropped, so it must be bound to a name"]
pub struct TransformGuard<'p, 'a> {
    painter:  &'p Painter<'a>,
    previous: Matrix3x2,
}

impl Drop for TransformGuard<'_, '_> {
    fn drop(&mut self) {
        self.painter.set_transform(self.previous);
    }
}

/// `a` applied first, then `b` — the order the page transforms compose in.
fn compose(a: Matrix3x2, b: Matrix3x2) -> Matrix3x2 {
    Matrix3x2 {
        M11: a.M11 * b.M11 + a.M12 * b.M21,
        M12: a.M11 * b.M12 + a.M12 * b.M22,
        M21: a.M21 * b.M11 + a.M22 * b.M21,
        M22: a.M21 * b.M12 + a.M22 * b.M22,
        M31: a.M31 * b.M11 + a.M32 * b.M21 + b.M31,
        M32: a.M31 * b.M12 + a.M32 * b.M22 + b.M32,
    }
}

impl<'a> Painter<'a> {
    pub fn new(renderer: &'a Renderer, theme: &'a Theme) -> Result<Self> {
        Ok(Self {
            ctx: &renderer.d2d_context,
            renderer,
            theme,
            brush: renderer.solid_brush(&theme.text_primary)?,
            scale: Cell::new(1.0),
            transform: Cell::new(Matrix3x2::identity()),
        })
    }

    pub fn set_scale(&self, scale: f32) {
        self.scale.set(scale);
    }

    /// Composes `m` onto the current transform until the guard is dropped.
    pub fn push_transform(&self, m: Matrix3x2) -> TransformGuard<'_, 'a> {
        let previous = self.transform.get();
        self.set_transform(compose(m, previous));
        TransformGuard { painter: self, previous }
    }

    fn set_transform(&self, m: Matrix3x2) {
        self.transform.set(m);
        unsafe { self.ctx.SetTransform(&m) };
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
            // Chrome text is centred in its box — a label sits in the middle of
            // its button. The document body never comes through here.
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
            self.ctx.DrawTextLayout(
                windows_numerics::Vector2 { X: r.left, Y: r.top },
                &layout,
                self.set_color(color),
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
    }

    fn vector(
        &self,
        name: &'static str,
        rect: &Rect,
        size: f32,
        pick: &dyn Fn(&kubuno_drive_desktop_app_controls::IconLayer) -> D2D1_COLOR_F,
    ) {
        use windows::core::Interface;
        let Ok(factory) = self
            .renderer
            .d2d_factory
            .cast::<windows::Win32::Graphics::Direct2D::ID2D1Factory>()
        else {
            return;
        };
        let mut icons = self.renderer.vector_icons.borrow_mut();
        let stroke_style = icons.stroke_style(&factory).cloned();
        let Some((layers, viewbox)) = icons.get_layers(&factory, name) else { return };
        let k = size / viewbox;
        let left = self.px((rect.left + rect.right) / 2.0 - size / 2.0);
        let top = self.px((rect.top + rect.bottom) / 2.0 - size / 2.0);
        // The icon's own matrix must compose ONTO whatever is installed, and the
        // context must go back to that — not to identity. Chat resets to
        // identity, which is invisible there because chat never nests a
        // transform; here an icon drawn inside a page would silently move the
        // rest of the page back to the viewport origin.
        let outer = self.transform.get();
        unsafe {
            for layer in layers {
                // The layer's own group transform composed with the icon's
                // scale and placement, which keeps the path data verbatim.
                let [a, b, cc, d, e, f2] = layer.transform;
                let local = Matrix3x2 {
                    M11: a * k,
                    M12: b * k,
                    M21: cc * k,
                    M22: d * k,
                    M31: e * k + left,
                    M32: f2 * k + top,
                };
                self.ctx.SetTransform(&compose(local, outer));
                // A fixed brand colour wins over the themed role: module logos
                // are painted in their own colours, like their web counterparts.
                let c = layer.color.unwrap_or_else(|| pick(layer));
                let brush = self.set_color(&c);
                match layer.stroke {
                    // Outlined (Lucide): the width is in design units, so the
                    // matrix scales it along with the geometry.
                    Some(width) => {
                        self.ctx.DrawGeometry(&layer.geometry, brush, width, stroke_style.as_ref())
                    }
                    None => self.ctx.FillGeometry(&layer.geometry, brush, None),
                }
            }
            self.ctx.SetTransform(&outer);
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
        let rr = D2D1_ROUNDED_RECT {
            rect: self.snap(rect).d2d(),
            radiusX: radius,
            radiusY: radius,
        };
        unsafe { self.ctx.FillRoundedRectangle(&rr, self.set_color(color)) };
    }

    fn fill_top_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        // The ribbon's tab strip is the one place this shape is wanted, and it
        // is not mounted until slice 5. A fully rounded fill is the honest
        // approximation until then.
        self.fill_rounded(rect, radius, color);
    }

    fn stroke_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
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
        self.vector(name, rect, size, &move |layer| D2D1_COLOR_F {
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
        self.vector(name, rect, size, &move |layer| {
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
            let Ok(layout) =
                self.renderer.dwrite.CreateTextLayout(&wide, format, UNBOUNDED, UNBOUNDED)
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
        unsafe {
            self.ctx.PushAxisAlignedClip(&rect.d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
    }

    fn pop_clip(&self) {
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
            let rr = D2D1_ROUNDED_RECT {
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
                maskTransform: Matrix3x2::identity(),
                opacity: 1.0,
                opacityBrush: std::mem::ManuallyDrop::new(None),
                layerOptions: windows::Win32::Graphics::Direct2D::D2D1_LAYER_OPTIONS1_NONE,
            };
            self.ctx.PushLayer(&params, None);
        }
    }

    fn pop_clip_rounded(&self) {
        unsafe { self.ctx.PopLayer() };
    }
}

impl Painter<'_> {
    /// A plain filled rectangle in the CURRENT transform, with no pixel snapping.
    ///
    /// Snapping is wrong under the page transform: it would quantise document
    /// coordinates to the viewport's pixel grid, so the same paragraph would sit
    /// on a different sub-pixel at every scroll offset and the text would crawl.
    pub fn fill_rect_raw(&self, rect: &Rect, color: &D2D1_COLOR_F) {
        unsafe { self.ctx.FillRectangle(&rect.d2d(), self.set_color(color)) };
    }

    /// A hairline rectangle outline in the CURRENT transform. `width` is in the
    /// transform's own units, so under the page transform it is document pixels
    /// and a zoomed-in page gets a proportionally thicker edge — which is what
    /// the eye expects of a sheet of paper.
    pub fn stroke_rect_raw(&self, rect: &Rect, color: &D2D1_COLOR_F, width: f32) {
        unsafe { self.ctx.DrawRectangle(&rect.d2d(), self.set_color(color), width, None) };
    }

    /// Draws one already-positioned run of text in the CURRENT transform.
    ///
    /// The document body's door. No alignment, no trimming, no wrapping — the
    /// ported breaker has already decided every one of those, and letting
    /// DirectWrite revisit any of them is how the two engines drift apart.
    pub fn draw_text_raw(
        &self,
        text: &str,
        format: &IDWriteTextFormat,
        rect: &Rect,
        color: &D2D1_COLOR_F,
    ) {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            self.ctx.DrawText(
                &wide,
                format,
                &rect.d2d(),
                self.set_color(color),
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    /// Clips to `rect` in the CURRENT transform (a page), until [`Painter::pop_clip_page`].
    pub fn push_clip_page(&self, rect: &Rect) {
        unsafe { self.ctx.PushAxisAlignedClip(&rect.d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
    }

    pub fn pop_clip_page(&self) {
        unsafe { self.ctx.PopAxisAlignedClip() };
    }

    /// A straight line in the CURRENT transform; `dash` is the web's border style (`dashed`,
    /// `dotted`) or none.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_line_raw(&self, x0: f32, y0: f32, x1: f32, y1: f32, color: &D2D1_COLOR_F, width: f32, dash: Option<&str>) {
        use windows::Win32::Graphics::Direct2D::{
            D2D1_CAP_STYLE_FLAT, D2D1_DASH_STYLE_DASH, D2D1_DASH_STYLE_DOT, D2D1_LINE_JOIN_MITER, D2D1_STROKE_STYLE_PROPERTIES,
        };
        let style = dash.and_then(|d| {
            let dash_style = match d {
                "dashed" => D2D1_DASH_STYLE_DASH,
                "dotted" => D2D1_DASH_STYLE_DOT,
                _ => return None,
            };
            let props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_FLAT,
                endCap: D2D1_CAP_STYLE_FLAT,
                dashCap: D2D1_CAP_STYLE_FLAT,
                lineJoin: D2D1_LINE_JOIN_MITER,
                miterLimit: 10.0,
                dashStyle: dash_style,
                dashOffset: 0.0,
            };
            unsafe {
                let base: windows::Win32::Graphics::Direct2D::ID2D1Factory = windows::core::Interface::cast(&self.renderer.d2d_factory).ok()?;
                base.CreateStrokeStyle(&props, None).ok()
            }
        });
        let p0 = windows_numerics::Vector2 { X: x0, Y: y0 };
        let p1 = windows_numerics::Vector2 { X: x1, Y: y1 };
        unsafe { self.ctx.DrawLine(p0, p1, self.set_color(color), width, style.as_ref()) };
    }

    /// Fills the UNION of `rects` in one pass, so the alpha of a translucent colour is laid once per
    /// pixel even where the rectangles overlap (the selection: `canvas-engine.ts` fills one combined
    /// path for the same reason — no darker joints between lines).
    pub fn fill_union_raw(&self, rects: &[Rect], color: &D2D1_COLOR_F) {
        use windows::Win32::Graphics::Direct2D::Common::D2D1_FILL_MODE_WINDING;
        use windows::Win32::Graphics::Direct2D::ID2D1Geometry;
        if rects.is_empty() {
            return;
        }
        let geoms: Vec<Option<ID2D1Geometry>> = rects
            .iter()
            .filter_map(|r| unsafe { self.renderer.d2d_factory.CreateRectangleGeometry(&r.d2d()).ok() })
            .map(|g| windows::core::Interface::cast::<ID2D1Geometry>(&g).ok())
            .collect();
        if let Ok(group) = unsafe { self.renderer.d2d_factory.CreateGeometryGroup(D2D1_FILL_MODE_WINDING, &geoms) } {
            unsafe { self.ctx.FillGeometry(&group, self.set_color(color), None) };
        }
    }

    /// A bitmap stretched into `rect` (CURRENT transform), optionally rotated about its centre.
    pub fn draw_bitmap_raw(&self, bitmap: &ID2D1Bitmap1, rect: &Rect, rotation_deg: f32) {
        let rotated = rotation_deg != 0.0;
        let guard = if rotated {
            let (cx, cy) = ((rect.left + rect.right) / 2.0, (rect.top + rect.bottom) / 2.0);
            let (s, c) = rotation_deg.to_radians().sin_cos();
            // Rotation about (cx, cy): translate(-c) · rotate · translate(c).
            Some(self.push_transform(Matrix3x2 { M11: c, M12: s, M21: -s, M22: c, M31: cx - c * cx + s * cy, M32: cy - s * cx - c * cy }))
        } else {
            None
        };
        unsafe {
            self.ctx.DrawBitmap(bitmap, Some(&rect.d2d()), 1.0, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, None, None);
        }
        drop(guard);
    }

    /// The caret: a bar `width` wide from `(x, top)` down `height`, leaning by `lean` (tan of the
    /// italic angle, bottom fixed).
    pub fn draw_caret_raw(&self, x: f32, top: f32, height: f32, width: f32, lean: f32, color: &D2D1_COLOR_F) {
        if lean == 0.0 {
            self.fill_rect_raw(&Rect::new(x, top, x + width, top + height), color);
            return;
        }
        let bottom = top + height;
        let p0 = windows_numerics::Vector2 { X: x + width / 2.0 + lean * height, Y: top };
        let p1 = windows_numerics::Vector2 { X: x + width / 2.0, Y: bottom };
        unsafe { self.ctx.DrawLine(p0, p1, self.set_color(color), width, None) };
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
            // about B/2 either side.
            let half = (layer.blur / 2.0).max(0.01);
            let inner = (layer.spread - half).max(0.0);
            let outer = layer.spread + half;
            let span = (outer - inner).max(0.01);
            // Opacity at distance `d` beyond the panel's edge, smooth at both
            // ends so neither the start nor the finish of the ramp shows.
            let profile = |d: f32| {
                let x = ((d - inner) / span).clamp(0.0, 1.0);
                layer.opacity * (1.0 - x * x * (3.0 - 2.0 * x))
            };
            // Two rings per DEVICE pixel, so a step is always finer than what a
            // pixel can show — whatever the shadow's reach or the DPI scale.
            let steps = ((outer * self.scale.get() * 2.0).ceil() as usize).clamp(16, 96);
            // Rings composite source-over, which stacks MULTIPLICATIVELY: the
            // increments are taken in optical depth, or the total comes out
            // short of the layer's opacity.
            let mut laid = 0.0f32;
            for i in 0..steps {
                // Outermost first, marching in towards the panel's own edge.
                let d = outer * (1.0 - i as f32 / (steps - 1) as f32);
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
                    radiusX: radius + d,
                    radiusY: radius + d,
                };
                let color = D2D1_COLOR_F { r: sr, g: sg, b: sb, a: 1.0 - (-step).exp() };
                unsafe { self.ctx.FillRoundedRectangle(&rr, self.set_color(&color)) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::compose;
    use windows_numerics::Matrix3x2;

    fn apply(m: Matrix3x2, x: f32, y: f32) -> (f32, f32) {
        (x * m.M11 + y * m.M21 + m.M31, x * m.M12 + y * m.M22 + m.M32)
    }

    #[test]
    fn compose_applies_the_left_matrix_first() {
        // Scale by 2, then translate by 100: the point (1,1) must land at 102,
        // not at 202 (which is what translating first would give).
        let scale = Matrix3x2 { M11: 2.0, M12: 0.0, M21: 0.0, M22: 2.0, M31: 0.0, M32: 0.0 };
        let translate = Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 100.0, M32: 100.0 };
        let (x, y) = apply(compose(scale, translate), 1.0, 1.0);
        assert_eq!((x, y), (102.0, 102.0));
    }

    #[test]
    fn identity_is_neutral_on_both_sides() {
        let m = Matrix3x2 { M11: 3.0, M12: 1.0, M21: -1.0, M22: 2.0, M31: 7.0, M32: -4.0 };
        for composed in [compose(m, Matrix3x2::identity()), compose(Matrix3x2::identity(), m)] {
            assert_eq!(apply(composed, 5.0, 6.0), apply(m, 5.0, 6.0));
        }
    }
}
