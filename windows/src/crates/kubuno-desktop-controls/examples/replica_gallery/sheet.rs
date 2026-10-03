//! The sheet: how one family page is measured, packed and painted.
//!
//! It reproduces the geometry of the reference generator
//! (`tools/winforms-ref/gallery/Families.cs`), because the two images are meant
//! to be put side by side: an outer `FlowLayoutPanel` wrapping left-to-right,
//! each family group inside a captioned `GroupBox`, and inside that a top-down
//! flow of the controls in the states that change their painting.
//!
//! Nothing here draws a control. Every pixel comes from the library: the frame
//! and caption from [`GroupBox::paint`], the packing from
//! [`kubuno_desktop_controls::layout_panels::flow_layout`], the sizes from each
//! control's own `preferred_size`. That is the whole point of the demo — a
//! hand-drawn stand-in would prove nothing about the port.

use kubuno_desktop_controls::containers::GroupBox;
use kubuno_desktop_controls::layout_panels::{flow_layout, FlowChild, FlowDirection};
use kubuno_desktop_controls::{Canvas, Control, ControlCanvas, Padding, Rect, Size};

// ── The reference sheet's own metrics, in DIP ────────────────────────────────
// Taken from `Families.cs`: the outer flow's Padding, the GroupBox Margin and
// Padding, the inner top-down flow's Padding, and the Margin every child is
// given. They are DIP, not pixels: the canvas is DPI-aware, so the same numbers
// hold at every scale.

/// `FlowLayoutPanel.Padding = new Padding(12)` on the sheet.
const SHEET_PADDING: Padding = Padding::all(12.0);
/// `GroupBox.Margin = new Padding(8)`.
const GROUP_MARGIN: Padding = Padding::all(8.0);
/// `GroupBox.Padding = new Padding(4, 6, 4, 4)`.
const GROUP_PADDING: Padding = Padding::new(4.0, 6.0, 4.0, 4.0);
/// The inner `FlowLayoutPanel.Padding = new Padding(6, 4, 6, 6)`.
const INNER_PADDING: Padding = Padding::new(6.0, 4.0, 6.0, 6.0);
/// `k.Margin = new Padding(4)` on every grouped control.
const KID_MARGIN: Padding = Padding::all(4.0);

/// The measuring line a top-down flow is laid out against. `flow_layout` still
/// compares each cell against the display box even when `wrap` is off, so the
/// measuring box must be big enough that nothing can break — the group's real
/// height is whatever the column comes out to.
const UNBOUNDED: f32 = 100_000.0;

/// Extra painting a grouped control cannot do through its own `paint`.
///
/// A handful of container controls in the library paint their frame but leave
/// their content to the host: `TabControl` paints the strip and the page frame
/// (it owns no children), `SplitContainer` paints two panels and the splitter,
/// `TableLayoutPanel` paints the outer border and publishes its track sizes.
/// For those the page hands the sheet a closure that is called with the final
/// rectangle of the control it decorates, and paints the content **with library
/// controls** placed against the geometry the library itself resolved.
pub type Decor = Box<dyn Fn(&dyn ControlCanvas, Rect)>;

/// One control in a group, with the size the reference gave it.
pub struct Kid {
    ctrl:      Box<dyn Control>,
    width:     Option<f32>,
    height:    Option<f32>,
    /// Mirrors the reference's `k.AutoSize = true`: the designed size is a
    /// FLOOR, never a ceiling (`AutoSizeMode.GrowOnly`), so a caption that does
    /// not fit grows its control instead of being clipped.
    auto_size: bool,
    decor:     Option<Decor>,
}

/// Adds a control to a group with its natural size.
pub fn kid(ctrl: impl Control + 'static) -> Kid {
    Kid { ctrl: Box::new(ctrl), width: None, height: None, auto_size: true, decor: None }
}

impl Kid {
    /// `Width = w` in the reference.
    pub fn w(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    /// `Height = h` in the reference.
    pub fn h(mut self, h: f32) -> Self {
        self.height = Some(h);
        self
    }

    /// The controls the reference notes ignore `AutoSize` — `ScrollBar`,
    /// `ListBox`, `ListView`, `TreeView`, and anything given an explicit box.
    /// They keep exactly the size they were designed at.
    pub fn fixed(mut self) -> Self {
        self.auto_size = false;
        self
    }

    /// Both axes at once, fixed — the shape of every `Width = …, Height = …` in
    /// the reference.
    pub fn size(self, w: f32, h: f32) -> Self {
        self.w(w).h(h).fixed()
    }

    pub fn decor(mut self, f: impl Fn(&dyn ControlCanvas, Rect) + 'static) -> Self {
        self.decor = Some(Box::new(f));
        self
    }

    /// Resolves the control's box: its own `preferred_size`, floored by the
    /// designed width/height when the reference set one.
    fn measure(&mut self, c: &dyn Canvas) -> Size {
        let natural = self.ctrl.preferred_size(c);
        let pick = |explicit: Option<f32>, natural: f32| match explicit {
            Some(v) if self.auto_size => v.max(natural),
            Some(v) => v,
            None => natural,
        };
        let size = Size::new(pick(self.width, natural.width), pick(self.height, natural.height));
        // Several controls measure and paint from their own bounds (a scroll
        // bar's long axis, a list's integral height, a tab control's page box),
        // so the resolved size has to be written back before anything else asks.
        self.ctrl.control_mut().set_size(size);
        self.ctrl.control().size()
    }
}

/// A captioned group of controls — the reference's `Box(caption, w, h, …)`.
pub struct Group {
    caption:   String,
    min_width: f32,
    kids:      Vec<Kid>,
}

/// `min_width` is the reference's `MinimumSize = new Size(w, 0)`: a floor only,
/// so a group still grows for a caption or a control that needs more room.
pub fn group(caption: &str, min_width: f32, kids: Vec<Kid>) -> Group {
    Group { caption: caption.to_string(), min_width, kids }
}

/// One family page: the groups, in the reference's order.
pub struct Sheet {
    groups: Vec<Group>,
}

impl Sheet {
    pub fn new(groups: Vec<Group>) -> Self {
        Self { groups }
    }

    /// Measures every group, packs them left-to-right with wrapping, and paints
    /// them inside `area`.
    pub fn paint(self, c: &dyn ControlCanvas, area: Rect) {
        let measured: Vec<Measured> = self.groups.into_iter().map(|g| measure(g, c)).collect();
        let cells: Vec<FlowChild> = measured
            .iter()
            .map(|m| FlowChild { size: m.size, margin: GROUP_MARGIN, flow_break: false })
            .collect();
        let display = Rect::new(
            area.left + SHEET_PADDING.left,
            area.top + SHEET_PADDING.top,
            area.right - SHEET_PADDING.right,
            area.bottom - SHEET_PADDING.bottom,
        );
        let rects = flow_layout(display, FlowDirection::LeftToRight, true, &cells);

        for (mut m, rect) in measured.into_iter().zip(rects) {
            // The children's bounds are parent-relative, so placing the group is
            // the whole move: painting it translates its contents for free.
            m.group.control_mut().set_bounds(rect);
            m.group.paint(c, rect);

            // A decoration is the one thing that genuinely needs canvas space, so
            // it asks the group where its local origin landed instead of
            // re-deriving the frame, caption band and padding for itself.
            let origin = m.group.content_origin(rect);
            for (i, decor) in m.decors.iter().enumerate() {
                if let (Some(d), Some(child)) = (decor, m.group.children.get(i)) {
                    let b = child.control().bounds;
                    d(
                        c,
                        Rect::new(
                            b.left + origin.x,
                            b.top + origin.y,
                            b.right + origin.x,
                            b.bottom + origin.y,
                        ),
                    );
                }
            }
        }
    }
}

/// A group whose children are placed and whose outer size is known. The children
/// are positioned in the group's own local space and stay there — the sheet only
/// ever moves the group itself.
struct Measured {
    group:  GroupBox,
    size:   Size,
    decors: Vec<Option<Decor>>,
}

fn measure(g: Group, c: &dyn Canvas) -> Measured {
    let mut gb = GroupBox::new();
    gb.text = g.caption;
    gb.padding = GROUP_PADDING;

    // The grouped controls are laid out in the group box's CLIENT space: their
    // bounds are parent-relative, and `local_display_rect` already carries the
    // frame and the caption band in its origin — so the measuring box is that
    // origin plus the inner flow's own padding. Neither metric is restated here.
    let d = gb.local_display_rect();
    let display = Rect::new(
        d.left + INNER_PADDING.left,
        d.top + INNER_PADDING.top,
        d.left + UNBOUNDED,
        d.top + UNBOUNDED,
    );

    let mut kids = g.kids;
    let cells: Vec<FlowChild> = kids
        .iter_mut()
        .map(|k| FlowChild { size: k.measure(c), margin: KID_MARGIN, flow_break: false })
        .collect();
    let rects = flow_layout(display, FlowDirection::TopDown, false, &cells);

    let mut decors = Vec::with_capacity(kids.len());
    for (k, r) in kids.into_iter().zip(rects) {
        let Kid { mut ctrl, decor, .. } = k;
        ctrl.control_mut().set_bounds(r);
        gb.children.push(ctrl);
        decors.push(decor);
    }

    // The group measures its own frame, caption and padding around the children
    // it now owns. What it cannot know is the trailing space the inner flow adds
    // after the last child — that padding belongs to the sheet, not to the box.
    let p = gb.preferred_size(c);
    let size = Size::new(
        (p.width + INNER_PADDING.right + KID_MARGIN.right).max(g.min_width),
        p.height + INNER_PADDING.bottom + KID_MARGIN.bottom,
    );
    Measured { group: gb, size, decors }
}
