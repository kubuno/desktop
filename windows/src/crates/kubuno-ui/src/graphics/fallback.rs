//! Executes ops with the canvas primitives, for a canvas that does not lend its renderer: shapes as
//! rounded fills/strokes or flattened outlines, gradients as their middle colour, text in the theme
//! formats, axis-aligned rectangle clips only. Good enough to see what is drawn; the host's own
//! surface always lends its renderer.

use drive_app_controls::{Canvas, Rect};
use windows::Win32::Graphics::DirectWrite::{DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING};

use super::display::{ClipShape, Op, Shape};
use super::paint::PenAlignment;
use super::text::{StringAlignment, StringTrimming};
use super::types::{Color, Matrix, PointF, RectExt};

pub fn execute(c: &dyn Canvas, op: &Op) {
    let Some(state) = op.state() else {
        if let Op::Canvas(call) = op {
            super::d2d::canvas_call(c, call);
        }
        return;
    };
    let mut pushed = 0;
    for clip in state.clips.iter() {
        if let ClipShape::Rect(r) = &clip.shape {
            if clip.transform.is_axis_aligned() {
                c.push_clip(&clip.transform.transform_bounds(r));
                pushed += 1;
            }
        }
    }
    let m = state.transform;
    match op {
        Op::Fill { shape, brush, .. } => fill(c, shape, brush.representative(), &m),
        Op::Stroke { shape, pen, .. } => {
            let k = m.mean_scale();
            let inset = if pen.alignment == PenAlignment::Inset { pen.width / 2.0 } else { 0.0 };
            stroke(c, shape, pen.brush.representative(), pen.width * k, inset, &m);
        }
        Op::Text { text, font, brush, layout, format, .. } => {
            let r = m.transform_bounds(layout);
            let f = super::role_format(c.formats(), font.role);
            let color = brush.representative().to_d2d();
            let ellipsis = matches!(format.trimming, StringTrimming::EllipsisCharacter | StringTrimming::EllipsisWord | StringTrimming::EllipsisPath);
            match (format.alignment, ellipsis) {
                (StringAlignment::Center, true) => c.text_ellipsis_center(text, &r, f, &color),
                (StringAlignment::Near, true) => c.text_ellipsis(text, &r, f, &color),
                (a, _) => c.text_aligned(
                    text,
                    &r,
                    f,
                    &color,
                    match a {
                        StringAlignment::Near => DWRITE_TEXT_ALIGNMENT_LEADING,
                        StringAlignment::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                        StringAlignment::Far => DWRITE_TEXT_ALIGNMENT_TRAILING,
                    },
                ),
            }
        }
        Op::Icon { name, rect, size, color, .. } => c.vector_icon(name, &m.transform_bounds(rect), size * m.mean_scale(), &color.to_d2d()),
        Op::Image { .. } | Op::Clear { .. } | Op::Canvas(_) => {}
    }
    for _ in 0..pushed {
        c.pop_clip();
    }
}

fn fill(c: &dyn Canvas, shape: &Shape, color: Color, m: &Matrix) {
    let col = color.to_d2d();
    if m.is_axis_aligned() {
        match shape {
            Shape::Line(..) => return,
            Shape::Rect(r) => return c.fill_rounded(&m.transform_bounds(r), 0.0, &col),
            Shape::RoundedRect(r, rx, _) => return c.fill_rounded(&m.transform_bounds(r), rx * m.mean_scale(), &col),
            Shape::Ellipse(r) => {
                let t = m.transform_bounds(r);
                if (t.width() - t.height()).abs() < 0.5 {
                    return c.fill_rounded(&t, t.width() / 2.0, &col);
                }
            }
            Shape::Path(_) => {}
        }
    }
    let mut path = shape.to_path();
    path.transform(m);
    for (pts, _) in path.flatten(0.5) {
        // A triangle fan: exact for the convex shapes a paint mostly fills.
        if pts.len() < 3 {
            continue;
        }
        let o = pts[0];
        for w in pts[1..].windows(2) {
            c.fill_triangle((o.x, o.y), (w[0].x, w[0].y), (w[1].x, w[1].y), &col);
        }
    }
}

fn stroke(c: &dyn Canvas, shape: &Shape, color: Color, width: f32, inset: f32, m: &Matrix) {
    let col = color.to_d2d();
    if m.is_axis_aligned() {
        match shape {
            Shape::Rect(r) => return c.stroke_rounded_w(&m.transform_bounds(&r.inflated(-inset, -inset)), 0.0, &col, width),
            Shape::RoundedRect(r, rx, _) => return c.stroke_rounded_w(&m.transform_bounds(&r.inflated(-inset, -inset)), rx * m.mean_scale(), &col, width),
            Shape::Ellipse(r) => {
                let t = m.transform_bounds(&r.inflated(-inset, -inset));
                if (t.width() - t.height()).abs() < 0.5 {
                    let ctr = t.center();
                    return c.stroke_arc((ctr.x, ctr.y), t.width() / 2.0, 0.0, std::f32::consts::TAU, width, &col);
                }
            }
            _ => {}
        }
    }
    let mut path = shape.to_path();
    path.transform(m);
    for (pts, closed) in path.flatten(0.5) {
        for w in pts.windows(2) {
            line(c, w[0], w[1], width, &col);
        }
        if closed && pts.len() > 2 {
            line(c, pts[pts.len() - 1], pts[0], width, &col);
        }
    }
}

/// A line as fills: a rectangle when axis-aligned, else a row of small squares.
fn line(c: &dyn Canvas, a: PointF, b: PointF, width: f32, col: &windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F) {
    let h = (width / 2.0).max(0.5);
    if (a.y - b.y).abs() < 1e-3 {
        return c.fill_rounded(&Rect::new(a.x.min(b.x), a.y - h, a.x.max(b.x), a.y + h), 0.0, col);
    }
    if (a.x - b.x).abs() < 1e-3 {
        return c.fill_rounded(&Rect::new(a.x - h, a.y.min(b.y), a.x + h, a.y.max(b.y)), 0.0, col);
    }
    let len = a.distance(b);
    let steps = ((len / h).ceil() as usize).clamp(1, 2048);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let (x, y) = (a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
        c.fill_rounded(&Rect::new(x - h, y - h, x + h, y + h), 0.0, col);
    }
}
