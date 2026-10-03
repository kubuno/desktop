//! Executes ops with Direct2D on the canvas' own device context, in the same `BeginDraw` as the
//! widgets. Each op sets up its clip, transform and hints, draws, and puts the device context back
//! exactly as it found it, so it interleaves safely with the canvas' primitives.

use std::cell::RefCell;
use std::collections::HashMap;

use kubuno_drive_desktop_app_controls::{Canvas, Rect, Renderer};
use windows::core::{Interface, HSTRING};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BEZIER_SEGMENT, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_ALTERNATE, D2D1_FILL_MODE_WINDING, D2D1_GRADIENT_STOP,
    D2D_RECT_F, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, ID2D1DeviceContext, ID2D1Geometry, ID2D1SolidColorBrush, ID2D1StrokeStyle, D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT,
    D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL, D2D1_BUFFER_PRECISION_8BPC_UNORM, D2D1_CAP_STYLE, D2D1_CAP_STYLE_FLAT, D2D1_CAP_STYLE_ROUND, D2D1_CAP_STYLE_SQUARE,
    D2D1_CAP_STYLE_TRIANGLE, D2D1_COLOR_INTERPOLATION_MODE_STRAIGHT, D2D1_COLOR_SPACE_SRGB, D2D1_DASH_STYLE_CUSTOM, D2D1_DASH_STYLE_SOLID,
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_ELLIPSE, D2D1_EXTEND_MODE, D2D1_EXTEND_MODE_CLAMP, D2D1_EXTEND_MODE_MIRROR,
    D2D1_EXTEND_MODE_WRAP, D2D1_INTERPOLATION_MODE, D2D1_INTERPOLATION_MODE_CUBIC, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, D2D1_INTERPOLATION_MODE_LINEAR,
    D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_LINE_JOIN,
    D2D1_LINE_JOIN_BEVEL, D2D1_LINE_JOIN_MITER, D2D1_LINE_JOIN_MITER_OR_BEVEL, D2D1_LINE_JOIN_ROUND, D2D1_PRIMITIVE_BLEND_COPY,
    D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES1, D2D1_STROKE_TRANSFORM_TYPE_NORMAL, D2D1_SWEEP_DIRECTION_CLOCKWISE,
    D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE, D2D1_TEXT_ANTIALIAS_MODE_ALIASED, D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, IDWriteTextLayout, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_FAR, DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_READING_DIRECTION_RIGHT_TO_LEFT,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_RANGE, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
    DWRITE_TRIMMING_GRANULARITY_NONE, DWRITE_TRIMMING_GRANULARITY_WORD, DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP,
};

use super::display::{CanvasCall, ClipItem, ClipShape, Op, OpState, Shape};
use super::paint::{Brush, GradientStop, LineCap, LineJoin, Pen, PenAlignment, WrapMode};
use super::path::{FillMode, GraphicsPath, Segment};
use super::text::{CompositingMode, Font, FontStyle, InterpolationMode, StringAlignment, StringFormat, StringFormatFlags, StringTrimming, TextRenderingHint};
use super::types::{Matrix, PointF, RectExt};

/// The layout box of an unbounded text (large, finite: DirectWrite's metrics stay exact).
const UNBOUNDED: f32 = 65_536.0;

/// Per-`Graphics` Direct2D resources.
#[derive(Default)]
pub struct Cache {
    solid: Option<ID2D1SolidColorBrush>,
}

thread_local! {
    /// Text formats built for explicit fonts, by (family, size, bold, italic): device-independent.
    static FORMATS: RefCell<HashMap<(String, u32, bool, bool), IDWriteTextFormat>> = RefCell::new(HashMap::new());
}

fn pt(p: PointF) -> windows_numerics::Vector2 {
    windows_numerics::Vector2 { X: p.x, Y: p.y }
}

fn rect_f(r: &Rect) -> D2D_RECT_F {
    D2D_RECT_F { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
}

/// What the device context looked like before an op.
struct Saved {
    transform: windows_numerics::Matrix3x2,
    antialias: windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE,
    text: windows::Win32::Graphics::Direct2D::D2D1_TEXT_ANTIALIAS_MODE,
    blend: windows::Win32::Graphics::Direct2D::D2D1_PRIMITIVE_BLEND,
}

enum Pushed {
    Axis,
    Layer,
}

pub fn execute(renderer: &Renderer, canvas: &dyn Canvas, cache: &RefCell<Cache>, op: &Op) {
    let ctx = &renderer.d2d_context;
    let Some(state) = op.state() else {
        if let Op::Canvas(call) = op {
            canvas_call(canvas, call);
        }
        return;
    };
    // SAFETY: plain Direct2D calls on the live device context, inside the host's BeginDraw; every
    // state change is undone below and every clip pushed here is popped here.
    unsafe {
        let mut base_raw = windows_numerics::Matrix3x2::default();
        ctx.GetTransform(&mut base_raw);
        let saved = Saved { transform: base_raw, antialias: ctx.GetAntialiasMode(), text: ctx.GetTextAntialiasMode(), blend: ctx.GetPrimitiveBlend() };
        let base = Matrix::from_d2d(&base_raw);
        let pushed = push_clips(renderer, ctx, &state.clips, &base);
        let world = state.transform.then(&base);
        ctx.SetTransform(&world.to_d2d());
        ctx.SetAntialiasMode(if state.smoothing.antialiased() { D2D1_ANTIALIAS_MODE_PER_PRIMITIVE } else { D2D1_ANTIALIAS_MODE_ALIASED });
        if state.compositing == CompositingMode::SourceCopy {
            ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
        }
        draw(renderer, canvas, cache, ctx, op, state, &base);
        ctx.SetPrimitiveBlend(saved.blend);
        ctx.SetTextAntialiasMode(saved.text);
        ctx.SetAntialiasMode(saved.antialias);
        ctx.SetTransform(&saved.transform);
        for p in pushed.iter().rev() {
            match p {
                Pushed::Axis => ctx.PopAxisAlignedClip(),
                Pushed::Layer => ctx.PopLayer(),
            }
        }
        ctx.SetTransform(&saved.transform);
    }
    if let Some(b) = op.bounds() {
        canvas.note_drawn(&b);
    }
}

unsafe fn push_clips(renderer: &Renderer, ctx: &ID2D1DeviceContext, clips: &[ClipItem], base: &Matrix) -> Vec<Pushed> {
    let mut pushed = Vec::with_capacity(clips.len());
    for c in clips {
        let world = c.transform.then(base);
        let aa = if c.antialias { D2D1_ANTIALIAS_MODE_PER_PRIMITIVE } else { D2D1_ANTIALIAS_MODE_ALIASED };
        match &c.shape {
            ClipShape::Rect(r) if world.is_axis_aligned() => {
                ctx.SetTransform(&world.to_d2d());
                ctx.PushAxisAlignedClip(&rect_f(r), aa);
                pushed.push(Pushed::Axis);
            }
            shape => {
                let geometry: Option<ID2D1Geometry> = match shape {
                    ClipShape::Rect(r) => renderer.d2d_factory.CreateRectangleGeometry(&rect_f(r)).ok().and_then(|g| g.cast().ok()),
                    ClipShape::Path(p) => path_geometry(renderer, p).and_then(|g| g.cast().ok()),
                };
                let Some(geometry) = geometry else {
                    // A clip that cannot be built clips everything out rather than nothing.
                    ctx.SetTransform(&base.to_d2d());
                    ctx.PushAxisAlignedClip(&D2D_RECT_F::default(), aa);
                    pushed.push(Pushed::Axis);
                    continue;
                };
                ctx.SetTransform(&base.to_d2d());
                let params = D2D1_LAYER_PARAMETERS1 {
                    contentBounds: D2D_RECT_F { left: f32::NEG_INFINITY, top: f32::NEG_INFINITY, right: f32::INFINITY, bottom: f32::INFINITY },
                    geometricMask: std::mem::ManuallyDrop::new(Some(geometry)),
                    maskAntialiasMode: aa,
                    maskTransform: c.transform.to_d2d(),
                    opacity: 1.0,
                    opacityBrush: std::mem::ManuallyDrop::new(None),
                    layerOptions: D2D1_LAYER_OPTIONS1_NONE,
                };
                ctx.PushLayer(&params, None);
                // The parameters' mask was moved into ManuallyDrop: release our reference.
                drop(std::mem::ManuallyDrop::into_inner(params.geometricMask));
                pushed.push(Pushed::Layer);
            }
        }
    }
    pushed
}

unsafe fn draw(renderer: &Renderer, canvas: &dyn Canvas, cache: &RefCell<Cache>, ctx: &ID2D1DeviceContext, op: &Op, state: &OpState, base: &Matrix) {
    match op {
        Op::Fill { shape, brush, .. } => {
            let Some(b) = make_brush(ctx, cache, brush) else { return };
            match shape {
                Shape::Line(..) => {}
                Shape::Rect(r) => ctx.FillRectangle(&rect_f(r), &b),
                Shape::RoundedRect(r, rx, ry) => ctx.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect_f(r), radiusX: *rx, radiusY: *ry }, &b),
                Shape::Ellipse(r) => ctx.FillEllipse(&ellipse(r), &b),
                Shape::Path(p) => {
                    if let Some(g) = path_geometry(renderer, p) {
                        ctx.FillGeometry(&g, &b, None);
                    }
                }
            }
        }
        Op::Stroke { shape, pen, .. } => {
            let Some(b) = make_brush(ctx, cache, &pen.brush) else { return };
            let style = stroke_style(renderer, pen);
            let w = pen.width;
            let inset = if pen.alignment == PenAlignment::Inset { w / 2.0 } else { 0.0 };
            match shape {
                Shape::Line(a, c) => ctx.DrawLine(pt(*a), pt(*c), &b, w, style.as_ref()),
                Shape::Rect(r) => ctx.DrawRectangle(&rect_f(&r.inflated(-inset, -inset)), &b, w, style.as_ref()),
                Shape::RoundedRect(r, rx, ry) => ctx.DrawRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect: rect_f(&r.inflated(-inset, -inset)), radiusX: (rx - inset).max(0.0), radiusY: (ry - inset).max(0.0) },
                    &b,
                    w,
                    style.as_ref(),
                ),
                Shape::Ellipse(r) => ctx.DrawEllipse(&ellipse(&r.inflated(-inset, -inset)), &b, w, style.as_ref()),
                Shape::Path(p) => {
                    if let Some(g) = path_geometry(renderer, p) {
                        ctx.DrawGeometry(&g, &b, w, style.as_ref());
                    }
                }
            }
        }
        Op::Text { text, font, brush, layout, format, .. } => {
            let Some(b) = make_brush(ctx, cache, brush) else { return };
            let Some(tl) = text_layout(renderer, canvas, text, font, layout.width().max(0.0), layout.height().max(0.0), format) else { return };
            if let Some(mode) = text_mode(state.text_hint) {
                ctx.SetTextAntialiasMode(mode);
            }
            let mut options = D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT;
            if !format.flags.contains(StringFormatFlags::NO_CLIP) {
                options |= D2D1_DRAW_TEXT_OPTIONS_CLIP;
            }
            ctx.DrawTextLayout(pt(PointF::new(layout.left, layout.top)), &tl, &b, options);
        }
        Op::Image { image, dest, src, opacity, .. } => {
            let Some(bitmap) = image.bitmap(renderer) else { return };
            let s = src.map(|r| rect_f(&r));
            ctx.DrawBitmap(&bitmap, Some(&rect_f(dest)), *opacity, interpolation(state.interpolation), s.as_ref().map(|r| r as *const _), None);
        }
        Op::Icon { name, rect, size, color, .. } => {
            // The canvas draws the icon with its own transform (its origin): give it the rectangle
            // already transformed (translation and scale), under the clip pushed above.
            ctx.SetTransform(&base.to_d2d());
            let r = state.transform.transform_bounds(rect);
            let k = state.transform.mean_scale();
            canvas.vector_icon(name, &r, size * k, &color.to_d2d());
        }
        Op::Clear { color, .. } => {
            ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
            ctx.SetTransform(&base.to_d2d());
            let area = state.clip_bounds().unwrap_or(Rect::new(-1.0e6, -1.0e6, 1.0e6, 1.0e6));
            if let Some(b) = make_brush(ctx, cache, &Brush::Solid(*color)) {
                ctx.FillRectangle(&rect_f(&area), &b);
            }
        }
        Op::Canvas(_) => {}
    }
}

/// A raw canvas primitive, recorded through the `Graphics`' `Canvas` implementation.
pub fn canvas_call(c: &dyn Canvas, call: &CanvasCall) {
    match call {
        CanvasCall::FillRounded(r, rad, col) => c.fill_rounded(r, *rad, &col.to_d2d()),
        CanvasCall::FillTopRounded(r, rad, col) => c.fill_top_rounded(r, *rad, &col.to_d2d()),
        CanvasCall::FillTriangle(a, b, cc, col) => c.fill_triangle(*a, *b, *cc, &col.to_d2d()),
        CanvasCall::StrokeArc(centre, radius, start, sweep, width, col) => c.stroke_arc(*centre, *radius, *start, *sweep, *width, &col.to_d2d()),
        CanvasCall::StrokeRounded(r, rad, col) => c.stroke_rounded(r, *rad, &col.to_d2d()),
        CanvasCall::StrokeRoundedW(r, rad, col, w) => c.stroke_rounded_w(r, *rad, &col.to_d2d(), *w),
        CanvasCall::Text(t, r, f, col, centered) => c.text(t, r, f, &col.to_d2d(), *centered),
        CanvasCall::TextAligned(t, r, f, col, a) => c.text_aligned(t, r, f, &col.to_d2d(), *a),
        CanvasCall::TextEllipsis(t, r, f, col) => c.text_ellipsis(t, r, f, &col.to_d2d()),
        CanvasCall::TextEllipsisCenter(t, r, f, col) => c.text_ellipsis_center(t, r, f, &col.to_d2d()),
        CanvasCall::Image(b, r, size, alpha) => c.image_alpha(b, r, *size, *alpha),
        CanvasCall::VectorIcon(n, r, size, col) => c.vector_icon(n, r, *size, &col.to_d2d()),
        CanvasCall::VectorIconLayered(n, r, size, fg, acc) => c.vector_icon_layered(n, r, *size, &fg.to_d2d(), &acc.to_d2d()),
        CanvasCall::CardShadow(r, rad) => c.draw_card_shadow(r, *rad),
        CanvasCall::Shadow(r, rad, layers, colour) => c.draw_shadow(r, *rad, layers, *colour),
        CanvasCall::EraseRounded(r, rad) => c.erase_rounded(r, *rad),
        CanvasCall::PushClip(r) => c.push_clip(r),
        CanvasCall::PushClipRounded(r, rad) => c.push_clip_rounded(r, *rad),
        CanvasCall::PopClipRounded => c.pop_clip_rounded(),
        CanvasCall::PopClip => c.pop_clip(),
        CanvasCall::PushOffset(dx, dy) => c.push_offset(*dx, *dy),
        CanvasCall::PopOffset => c.pop_offset(),
        CanvasCall::PushBg(col) => c.push_bg(col.to_d2d()),
        CanvasCall::PopBg => c.pop_bg(),
    }
}

fn ellipse(r: &Rect) -> D2D1_ELLIPSE {
    let c = r.center();
    D2D1_ELLIPSE { point: pt(c), radiusX: r.width() / 2.0, radiusY: r.height() / 2.0 }
}

fn extend(w: WrapMode) -> D2D1_EXTEND_MODE {
    match w {
        WrapMode::Clamp => D2D1_EXTEND_MODE_CLAMP,
        WrapMode::Tile => D2D1_EXTEND_MODE_WRAP,
        WrapMode::TileFlip => D2D1_EXTEND_MODE_MIRROR,
    }
}

unsafe fn stops(ctx: &ID2D1DeviceContext, stops: &[GradientStop], wrap: WrapMode) -> Option<windows::Win32::Graphics::Direct2D::ID2D1GradientStopCollection1> {
    let list: Vec<D2D1_GRADIENT_STOP> = stops.iter().map(|s| D2D1_GRADIENT_STOP { position: s.position, color: s.color.to_d2d() }).collect();
    ctx.CreateGradientStopCollection(
        &list,
        D2D1_COLOR_SPACE_SRGB,
        D2D1_COLOR_SPACE_SRGB,
        D2D1_BUFFER_PRECISION_8BPC_UNORM,
        extend(wrap),
        D2D1_COLOR_INTERPOLATION_MODE_STRAIGHT,
    )
    .ok()
}

unsafe fn make_brush(ctx: &ID2D1DeviceContext, cache: &RefCell<Cache>, brush: &Brush) -> Option<ID2D1Brush> {
    match brush {
        Brush::Solid(c) => {
            let mut cache = cache.borrow_mut();
            if cache.solid.is_none() {
                cache.solid = ctx.CreateSolidColorBrush(&c.to_d2d(), None).ok();
            }
            let b = cache.solid.as_ref()?;
            b.SetColor(&c.to_d2d());
            b.cast().ok()
        }
        Brush::Linear(l) => {
            let collection = stops(ctx, &l.stops, l.wrap)?;
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: pt(l.start), endPoint: pt(l.end) };
            ctx.CreateLinearGradientBrush(&props, None, &collection).ok().and_then(|b| b.cast().ok())
        }
        Brush::Radial(r) => {
            let collection = stops(ctx, &r.stops, r.wrap)?;
            let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center: pt(r.center),
                gradientOriginOffset: pt(r.origin_offset),
                radiusX: r.radius_x.max(0.0),
                radiusY: r.radius_y.max(0.0),
            };
            ctx.CreateRadialGradientBrush(&props, None, &collection).ok().and_then(|b| b.cast().ok())
        }
    }
}

fn cap(c: LineCap) -> D2D1_CAP_STYLE {
    match c {
        LineCap::Flat => D2D1_CAP_STYLE_FLAT,
        LineCap::Square => D2D1_CAP_STYLE_SQUARE,
        LineCap::Round => D2D1_CAP_STYLE_ROUND,
        LineCap::Triangle => D2D1_CAP_STYLE_TRIANGLE,
    }
}

fn join(j: LineJoin) -> D2D1_LINE_JOIN {
    match j {
        LineJoin::Miter => D2D1_LINE_JOIN_MITER,
        LineJoin::Bevel => D2D1_LINE_JOIN_BEVEL,
        LineJoin::Round => D2D1_LINE_JOIN_ROUND,
        LineJoin::MiterClipped => D2D1_LINE_JOIN_MITER_OR_BEVEL,
    }
}

/// The stroke style of `pen`, `None` for a plain solid flat-capped mitred pen (Direct2D's default).
unsafe fn stroke_style(renderer: &Renderer, pen: &Pen) -> Option<ID2D1StrokeStyle> {
    let pattern = pen.effective_pattern();
    let plain = pattern.is_empty() && pen.start_cap == LineCap::Flat && pen.end_cap == LineCap::Flat && pen.line_join == LineJoin::Miter && pen.miter_limit == 10.0;
    if plain {
        return None;
    }
    let props = D2D1_STROKE_STYLE_PROPERTIES1 {
        startCap: cap(pen.start_cap),
        endCap: cap(pen.end_cap),
        dashCap: cap(pen.dash_cap),
        lineJoin: join(pen.line_join),
        miterLimit: pen.miter_limit.max(1.0),
        dashStyle: if pattern.is_empty() { D2D1_DASH_STYLE_SOLID } else { D2D1_DASH_STYLE_CUSTOM },
        dashOffset: pen.dash_offset,
        transformType: D2D1_STROKE_TRANSFORM_TYPE_NORMAL,
    };
    let dashes = if pattern.is_empty() { None } else { Some(pattern) };
    renderer.d2d_factory.CreateStrokeStyle(&props, dashes).ok().and_then(|s| s.cast().ok())
}

/// The Direct2D geometry of `path`.
pub(super) unsafe fn path_geometry(renderer: &Renderer, path: &GraphicsPath) -> Option<windows::Win32::Graphics::Direct2D::ID2D1PathGeometry1> {
    let geometry = renderer.d2d_factory.CreatePathGeometry().ok()?;
    let sink = geometry.Open().ok()?;
    sink.SetFillMode(match path.fill_mode {
        FillMode::Alternate => D2D1_FILL_MODE_ALTERNATE,
        FillMode::Winding => D2D1_FILL_MODE_WINDING,
    });
    for f in &path.figures {
        if f.segments.is_empty() {
            continue;
        }
        sink.BeginFigure(pt(f.start), D2D1_FIGURE_BEGIN_FILLED);
        for s in &f.segments {
            match *s {
                Segment::Line(p) => sink.AddLine(pt(p)),
                Segment::Bezier(a, b, c) => sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: pt(a), point2: pt(b), point3: pt(c) }),
                Segment::Arc { end, radius_x, radius_y, rotation, large, clockwise } => sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: pt(end),
                    size: D2D_SIZE_F { width: radius_x.max(0.0), height: radius_y.max(0.0) },
                    rotationAngle: rotation,
                    sweepDirection: if clockwise { D2D1_SWEEP_DIRECTION_CLOCKWISE } else { D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE },
                    arcSize: if large { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
                }),
            }
        }
        sink.EndFigure(if f.closed { D2D1_FIGURE_END_CLOSED } else { D2D1_FIGURE_END_OPEN });
    }
    sink.Close().ok()?;
    Some(geometry)
}

fn interpolation(m: InterpolationMode) -> D2D1_INTERPOLATION_MODE {
    match m {
        InterpolationMode::NearestNeighbor => D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
        InterpolationMode::Low | InterpolationMode::Bilinear | InterpolationMode::HighQualityBilinear => D2D1_INTERPOLATION_MODE_LINEAR,
        InterpolationMode::Bicubic => D2D1_INTERPOLATION_MODE_CUBIC,
        InterpolationMode::Default | InterpolationMode::High | InterpolationMode::HighQualityBicubic => D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
    }
}

fn text_mode(h: TextRenderingHint) -> Option<windows::Win32::Graphics::Direct2D::D2D1_TEXT_ANTIALIAS_MODE> {
    match h {
        TextRenderingHint::SystemDefault => None,
        TextRenderingHint::SingleBitPerPixel | TextRenderingHint::SingleBitPerPixelGridFit => Some(D2D1_TEXT_ANTIALIAS_MODE_ALIASED),
        TextRenderingHint::AntiAlias | TextRenderingHint::AntiAliasGridFit => Some(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE),
        TextRenderingHint::ClearTypeGridFit => Some(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE),
    }
}

/// The DirectWrite format of `font`: the theme's own format for a plain role, else one built (and
/// kept) for its family, size and weight.
fn text_format(renderer: &Renderer, canvas: &dyn Canvas, font: &Font) -> Option<IDWriteTextFormat> {
    let role = super::role_format(canvas.formats(), font.role);
    if font.is_plain_role() {
        return Some(role.clone());
    }
    // SAFETY: plain COM getters on a live format.
    let (role_family, role_size, role_weight) = unsafe {
        let len = role.GetFontFamilyNameLength() as usize;
        let mut buf = vec![0u16; len + 1];
        let name = if role.GetFontFamilyName(&mut buf).is_ok() { String::from_utf16_lossy(&buf[..len]) } else { "Segoe UI".to_string() };
        (name, role.GetFontSize(), role.GetFontWeight())
    };
    let family = font.family.clone().unwrap_or(role_family);
    let size = font.size.unwrap_or(role_size);
    let bold = font.style.contains(FontStyle::BOLD) || role_weight.0 >= DWRITE_FONT_WEIGHT_BOLD.0 && font.family.is_none();
    let italic = font.style.contains(FontStyle::ITALIC);
    let key = (family.clone(), size.to_bits(), bold, italic);
    if let Some(found) = FORMATS.with(|m| m.borrow().get(&key).cloned()) {
        return Some(found);
    }
    let weight = if bold { DWRITE_FONT_WEIGHT_BOLD } else if font.family.is_none() { role_weight } else { DWRITE_FONT_WEIGHT(400) };
    // SAFETY: creating a text format from owned strings.
    let format = unsafe {
        renderer
            .dwrite
            .CreateTextFormat(
                &HSTRING::from(family.as_str()),
                None,
                weight,
                if italic { DWRITE_FONT_STYLE_ITALIC } else { DWRITE_FONT_STYLE_NORMAL },
                DWRITE_FONT_STRETCH_NORMAL,
                size.max(0.1),
                &HSTRING::from(""),
            )
            .ok()?
    };
    FORMATS.with(|m| m.borrow_mut().insert(key, format.clone()));
    Some(format)
}

/// A laid-out text: alignment, wrapping, trimming, direction and decorations of `format`/`font`.
fn text_layout(renderer: &Renderer, canvas: &dyn Canvas, text: &str, font: &Font, width: f32, height: f32, format: &StringFormat) -> Option<IDWriteTextLayout> {
    let tf = text_format(renderer, canvas, font)?;
    let wide: Vec<u16> = text.encode_utf16().collect();
    // SAFETY: DirectWrite calls on objects created here.
    unsafe {
        let layout = renderer.dwrite.CreateTextLayout(&wide, &tf, width.min(UNBOUNDED), height.min(UNBOUNDED)).ok()?;
        let rtl = format.flags.contains(StringFormatFlags::DIRECTION_RIGHT_TO_LEFT);
        if rtl {
            let _ = layout.SetReadingDirection(DWRITE_READING_DIRECTION_RIGHT_TO_LEFT);
        }
        let _ = layout.SetTextAlignment(match format.alignment {
            StringAlignment::Near => DWRITE_TEXT_ALIGNMENT_LEADING,
            StringAlignment::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
            StringAlignment::Far => DWRITE_TEXT_ALIGNMENT_TRAILING,
        });
        let _ = layout.SetParagraphAlignment(match format.line_alignment {
            StringAlignment::Near => DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            StringAlignment::Center => DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            StringAlignment::Far => DWRITE_PARAGRAPH_ALIGNMENT_FAR,
        });
        let _ = layout.SetWordWrapping(if format.wraps() { DWRITE_WORD_WRAPPING_WRAP } else { DWRITE_WORD_WRAPPING_NO_WRAP });
        let (granularity, ellipsis, delimiter) = match format.trimming {
            StringTrimming::None => (DWRITE_TRIMMING_GRANULARITY_NONE, false, 0),
            StringTrimming::Character => (DWRITE_TRIMMING_GRANULARITY_CHARACTER, false, 0),
            StringTrimming::Word => (DWRITE_TRIMMING_GRANULARITY_WORD, false, 0),
            StringTrimming::EllipsisCharacter => (DWRITE_TRIMMING_GRANULARITY_CHARACTER, true, 0),
            StringTrimming::EllipsisWord => (DWRITE_TRIMMING_GRANULARITY_WORD, true, 0),
            StringTrimming::EllipsisPath => (DWRITE_TRIMMING_GRANULARITY_CHARACTER, true, u32::from('\\')),
        };
        let trimming = DWRITE_TRIMMING { granularity, delimiter, delimiterCount: u32::from(delimiter != 0) };
        let sign = if ellipsis { renderer.dwrite.CreateEllipsisTrimmingSign(&tf).ok() } else { None };
        let _ = layout.SetTrimming(&trimming, sign.as_ref());
        let range = DWRITE_TEXT_RANGE { startPosition: 0, length: wide.len() as u32 };
        if font.style.contains(FontStyle::UNDERLINE) {
            let _ = layout.SetUnderline(true, range);
        }
        if font.style.contains(FontStyle::STRIKEOUT) {
            let _ = layout.SetStrikethrough(true, range);
        }
        Some(layout)
    }
}

/// The size of `text` laid out for `format`, wrapped at `max_width`.
pub fn measure(renderer: &Renderer, canvas: &dyn Canvas, text: &str, font: &Font, max_width: Option<f32>, format: &StringFormat) -> (f32, f32) {
    let width = max_width.filter(|w| *w > 0.0 && format.wraps()).unwrap_or(UNBOUNDED);
    let fmt = if max_width.is_some() { *format } else { format.with_flags(StringFormatFlags::NO_WRAP) };
    let Some(layout) = text_layout(renderer, canvas, text, font, width, UNBOUNDED, &fmt.with_alignment(StringAlignment::Near).with_line_alignment(StringAlignment::Near))
    else {
        return (0.0, 0.0);
    };
    let mut m = Default::default();
    // SAFETY: a getter on the layout created above.
    if unsafe { layout.GetMetrics(&mut m) }.is_err() {
        return (0.0, 0.0);
    }
    let w = if format.flags.contains(StringFormatFlags::MEASURE_TRAILING_SPACES) { m.widthIncludingTrailingWhitespace } else { m.width };
    (w, m.height)
}
