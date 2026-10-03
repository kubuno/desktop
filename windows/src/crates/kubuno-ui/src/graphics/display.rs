//! The display list: every [`super::Graphics`] call becomes an [`Op`] carrying its own state
//! (transform, clip, hints). A surface executes ops as they come; a [`DisplayList`] keeps them —
//! for tests (what was drawn, in which state) and for a control's paint buffer (replayed while the
//! control is valid, instead of painting it again).

use std::rc::Rc;

use drive_app_controls::Rect;
use windows::Win32::Graphics::DirectWrite::{IDWriteTextFormat, DWRITE_TEXT_ALIGNMENT};

use super::image::Image;
use super::paint::{Brush, Pen};
use super::path::GraphicsPath;
use super::text::{CompositingMode, Font, InterpolationMode, SmoothingMode, StringFormat, TextRenderingHint};
use super::types::{Color, Matrix, PointF, RectExt};

/// A geometric shape an op fills or outlines.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Line(PointF, PointF),
    Rect(Rect),
    RoundedRect(Rect, f32, f32),
    Ellipse(Rect),
    Path(GraphicsPath),
}

impl Shape {
    /// The shape's own bounding box (before any transform).
    pub fn bounds(&self) -> Rect {
        match self {
            Shape::Line(a, b) => Rect::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)),
            Shape::Rect(r) | Shape::RoundedRect(r, _, _) | Shape::Ellipse(r) => *r,
            Shape::Path(p) => p.bounds(),
        }
    }

    /// The shape as a path (what a surface without the primitive draws).
    pub fn to_path(&self) -> GraphicsPath {
        let mut p = GraphicsPath::new();
        match self {
            Shape::Line(a, b) => p.add_line(*a, *b),
            Shape::Rect(r) => p.add_rectangle(*r),
            Shape::RoundedRect(r, rx, _) => p.add_rounded_rectangle(*r, *rx),
            Shape::Ellipse(r) => p.add_ellipse(*r),
            Shape::Path(path) => return path.clone(),
        }
        p
    }
}

/// What a clip region is made of.
#[derive(Debug, Clone, PartialEq)]
pub enum ClipShape {
    Rect(Rect),
    Path(GraphicsPath),
}

/// One region of the clip (the clip is the intersection of all), in the transform that was in
/// force when it was set.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipItem {
    pub shape: ClipShape,
    pub transform: Matrix,
    pub antialias: bool,
}

impl ClipItem {
    /// The bounding box of the region, in surface coordinates.
    pub fn bounds(&self) -> Rect {
        let local = match &self.shape {
            ClipShape::Rect(r) => *r,
            ClipShape::Path(p) => p.bounds(),
        };
        self.transform.transform_bounds(&local)
    }
}

/// The drawing state an op was issued in.
#[derive(Debug, Clone, PartialEq)]
pub struct OpState {
    pub transform: Matrix,
    /// Shared between the ops of one state (copied on write).
    pub clips: Rc<Vec<ClipItem>>,
    pub smoothing: SmoothingMode,
    pub text_hint: TextRenderingHint,
    pub interpolation: InterpolationMode,
    pub compositing: CompositingMode,
}

impl Default for OpState {
    fn default() -> Self {
        Self {
            transform: Matrix::IDENTITY,
            clips: Rc::new(Vec::new()),
            smoothing: SmoothingMode::Default,
            text_hint: TextRenderingHint::SystemDefault,
            interpolation: InterpolationMode::Default,
            compositing: CompositingMode::SourceOver,
        }
    }
}

impl OpState {
    /// The bounding box of the clip in surface coordinates, `None` when unclipped.
    pub fn clip_bounds(&self) -> Option<Rect> {
        let mut out: Option<Rect> = None;
        for c in self.clips.iter() {
            let b = c.bounds();
            out = Some(match out {
                Some(o) => o.intersect(&b).unwrap_or_else(|| Rect::new(b.left, b.top, b.left, b.top)),
                None => b,
            });
        }
        out
    }
}

/// A primitive of the underlying [`drive_app_controls::Canvas`], called through a `Graphics`
/// (its `Canvas` implementation) — recorded so a buffered paint that mixes both APIs replays
/// whole. Executed on the surface as is: the `Graphics` transform and clip do not apply.
#[derive(Clone)]
pub enum CanvasCall {
    FillRounded(Rect, f32, Color),
    FillTopRounded(Rect, f32, Color),
    FillTriangle((f32, f32), (f32, f32), (f32, f32), Color),
    StrokeArc((f32, f32), f32, f32, f32, f32, Color),
    StrokeRounded(Rect, f32, Color),
    StrokeRoundedW(Rect, f32, Color, f32),
    Text(String, Rect, IDWriteTextFormat, Color, bool),
    TextAligned(String, Rect, IDWriteTextFormat, Color, DWRITE_TEXT_ALIGNMENT),
    TextEllipsis(String, Rect, IDWriteTextFormat, Color),
    TextEllipsisCenter(String, Rect, IDWriteTextFormat, Color),
    Image(windows::Win32::Graphics::Direct2D::ID2D1Bitmap1, Rect, f32, f32),
    VectorIcon(&'static str, Rect, f32, Color),
    VectorIconLayered(&'static str, Rect, f32, Color, Color),
    CardShadow(Rect, f32),
    Shadow(Rect, f32, Vec<drive_app_controls::themes::shape::ShadowLayer>, (f32, f32, f32)),
    EraseRounded(Rect, f32),
    PushClip(Rect),
    PushClipRounded(Rect, f32),
    PopClipRounded,
    PopClip,
    PushOffset(f32, f32),
    PopOffset,
    PushBg(Color),
    PopBg,
}

impl std::fmt::Debug for CanvasCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            CanvasCall::FillRounded(..) => "FillRounded",
            CanvasCall::FillTopRounded(..) => "FillTopRounded",
            CanvasCall::FillTriangle(..) => "FillTriangle",
            CanvasCall::StrokeArc(..) => "StrokeArc",
            CanvasCall::StrokeRounded(..) => "StrokeRounded",
            CanvasCall::StrokeRoundedW(..) => "StrokeRoundedW",
            CanvasCall::Text(t, ..) | CanvasCall::TextAligned(t, ..) | CanvasCall::TextEllipsis(t, ..) | CanvasCall::TextEllipsisCenter(t, ..) => {
                return write!(f, "Text({t:?})");
            }
            CanvasCall::Image(..) => "Image",
            CanvasCall::VectorIcon(n, ..) | CanvasCall::VectorIconLayered(n, ..) => return write!(f, "VectorIcon({n})"),
            CanvasCall::CardShadow(..) => "CardShadow",
            CanvasCall::Shadow(..) => "Shadow",
            CanvasCall::EraseRounded(..) => "EraseRounded",
            CanvasCall::PushClip(..) => "PushClip",
            CanvasCall::PushClipRounded(..) => "PushClipRounded",
            CanvasCall::PopClipRounded => "PopClipRounded",
            CanvasCall::PopClip => "PopClip",
            CanvasCall::PushOffset(..) => "PushOffset",
            CanvasCall::PopOffset => "PopOffset",
            CanvasCall::PushBg(..) => "PushBg",
            CanvasCall::PopBg => "PopBg",
        };
        f.write_str(name)
    }
}

/// One drawing operation.
#[derive(Debug, Clone)]
pub enum Op {
    Fill { shape: Shape, brush: Brush, state: OpState },
    Stroke { shape: Shape, pen: Pen, state: OpState },
    Text { text: String, font: Font, brush: Brush, layout: Rect, format: StringFormat, state: OpState },
    Image { image: Image, dest: Rect, src: Option<Rect>, opacity: f32, state: OpState },
    Icon { name: &'static str, rect: Rect, size: f32, color: Color, state: OpState },
    /// Fills the clip region (everything when unclipped) with `color`, replacing what is there.
    Clear { color: Color, state: OpState },
    Canvas(CanvasCall),
}

impl Op {
    /// The state the op was issued in (`None` for a raw canvas call).
    pub fn state(&self) -> Option<&OpState> {
        match self {
            Op::Fill { state, .. } | Op::Stroke { state, .. } | Op::Text { state, .. } | Op::Image { state, .. } | Op::Icon { state, .. } | Op::Clear { state, .. } => Some(state),
            Op::Canvas(_) => None,
        }
    }

    /// A short name for tests and traces: `FillRect`, `StrokeEllipse`, `Text("…")`…
    pub fn describe(&self) -> String {
        let shape = |s: &Shape| match s {
            Shape::Line(..) => "Line",
            Shape::Rect(..) => "Rect",
            Shape::RoundedRect(..) => "RoundedRect",
            Shape::Ellipse(..) => "Ellipse",
            Shape::Path(..) => "Path",
        };
        match self {
            Op::Fill { shape: s, .. } => format!("Fill{}", shape(s)),
            Op::Stroke { shape: s, .. } => format!("Stroke{}", shape(s)),
            Op::Text { text, .. } => format!("Text({text:?})"),
            Op::Image { .. } => "Image".to_string(),
            Op::Icon { name, .. } => format!("Icon({name})"),
            Op::Clear { .. } => "Clear".to_string(),
            Op::Canvas(c) => format!("Canvas.{c:?}"),
        }
    }

    /// Where the op draws, in surface coordinates (a conservative box), `None` when unknown (a raw
    /// clip or offset change, a clear).
    pub fn bounds(&self) -> Option<Rect> {
        let (local, state, pad) = match self {
            Op::Fill { shape, state, .. } => (shape.bounds(), state, 0.0),
            Op::Stroke { shape, pen, state } => (shape.bounds(), state, pen.width / 2.0 + pen.miter_limit.min(4.0) * pen.width / 2.0),
            Op::Text { layout, state, .. } => (*layout, state, 0.0),
            Op::Image { dest, state, .. } => (*dest, state, 0.0),
            Op::Icon { rect, state, .. } => (*rect, state, 0.0),
            Op::Clear { .. } | Op::Canvas(_) => return None,
        };
        let b = state.transform.transform_bounds(&local.inflated(pad, pad));
        match state.clip_bounds() {
            Some(c) => b.intersect(&c).or(Some(Rect::new(b.left, b.top, b.left, b.top))),
            None => Some(b),
        }
    }
}

/// A recorded sequence of ops.
#[derive(Debug, Clone, Default)]
pub struct DisplayList {
    pub ops: Vec<Op>,
}

impl DisplayList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// The ops' short names, in order (tests).
    pub fn describe(&self) -> Vec<String> {
        self.ops.iter().map(Op::describe).collect()
    }

    /// Draws the ops again, in their recorded state, on `g`'s surface (and into `g`'s own
    /// recording, if any).
    pub fn replay(&self, g: &super::Graphics<'_>) {
        for op in &self.ops {
            g.emit(op.clone());
        }
    }

    /// The union of the ops' bounds (surface coordinates).
    pub fn bounds(&self) -> Option<Rect> {
        self.ops.iter().filter_map(Op::bounds).reduce(|a, b| a.union(&b))
    }
}
