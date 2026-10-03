//! The layout-container family: `FlowLayoutPanel`, `TableLayoutPanel`,
//! `SplitContainer` (+ `SplitterPanel`), `Splitter`, and `TabControl`
//! (+ `TabPage`).
//!
//! ## What is special about this family
//!
//! Every other control paints *itself*. These place *other controls*, so the
//! interesting part is not the paint but the **arithmetic**: how a flow panel
//! wraps, how a table resolves mixed Absolute/Percent/AutoSize tracks, where a
//! split container puts its splitter, which rectangle a tab page receives.
//!
//! Following `layout::layout`, each of those rules is a **pure function** of
//! primitives (`Rect`, `Size`, styles) — no canvas, no children borrowed off the
//! control — so it can be unit-tested without a window. The concrete controls
//! own only their declared properties (the catalogue is the authority) and reuse
//! their real base by composition:
//!
//! * `FlowLayoutPanel`, `TableLayoutPanel`, `SplitterPanel`, `TabPage` derive
//!   from `Panel` (`crate::containers::Panel`);
//! * `SplitContainer` derives from `ContainerControl`;
//! * `Splitter` and `TabControl` derive directly from `Control` (`ControlBase`).
//!
//! ### Coupling note
//!
//! `crate::containers::{Panel, ContainerControl}` are owned by a sibling module.
//! The only facts this module relies on are the ones the shared design
//! guarantees for every base: it implements `Default`, and it `Deref`s (and
//! `DerefMut`s) down to `ControlBase`. `&self.base` therefore coerces to
//! `&ControlBase`, and inherited fields are reached through the deref chain.

use std::collections::HashSet;

use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

use crate::containers::{border_thickness, ContainerControl, Panel};
use crate::control::{Control, ControlBase, ControlCanvas, FontRole};
use crate::enums::{AnchorStyles, AutoSizeMode, BorderStyle, Padding, Size};
use crate::{Canvas, Rect};

// ═════════════════════════════════════════════════════════════════════════════
// Enumerations this family owns
//
// These are extracted from the same `System.Windows.Forms` surface as
// `enums.rs`, but they are only ever used by this family, so they live with it
// rather than in the shared module. Members and defaults are the toolkit's.
// ═════════════════════════════════════════════════════════════════════════════

/// The direction a `FlowLayoutPanel` lays its children out (`FlowDirection`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlowDirection {
    #[default]
    LeftToRight,
    TopDown,
    RightToLeft,
    BottomUp,
}

impl FlowDirection {
    /// Flow that advances along the X axis (rows that wrap downward).
    const fn is_horizontal(self) -> bool {
        matches!(self, Self::LeftToRight | Self::RightToLeft)
    }
}

/// Horizontal or vertical orientation (`Orientation`) — shared by
/// `SplitContainer`. Its default there is `Vertical`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    Horizontal,
    #[default]
    Vertical,
}

/// Which panel of a `SplitContainer` keeps its size when the container is
/// resized (`FixedPanel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FixedPanel {
    #[default]
    None,
    Panel1,
    Panel2,
}

/// How a `TableLayoutPanel` track (column or row) is sized (`SizeType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SizeType {
    /// Sized to its content — the toolkit's `AutoSize`.
    #[default]
    AutoSize,
    /// A fixed number of pixels.
    Absolute,
    /// A share of the space the fixed and auto tracks leave, weighted by its
    /// value relative to the other percent tracks.
    Percent,
}

/// How a `TableLayoutPanel` paints the lines around and between cells
/// (`TableLayoutPanelCellBorderStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableLayoutPanelCellBorderStyle {
    #[default]
    None,
    Single,
    Inset,
    InsetDouble,
    Outset,
    OutsetDouble,
    OutsetPartial,
}

impl TableLayoutPanelCellBorderStyle {
    /// The pixel thickness a border line consumes between two cells (and around
    /// the grid). Every value here is measured against the toolkit.
    ///
    /// `OutsetPartial` is the trap: it *paints* a partial line, so it reads like
    /// a light style, but it reserves **3** DIP — the same as the doubles, not
    /// the 1 its appearance suggests.
    pub const fn thickness(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Single => 1.0,
            Self::Inset | Self::Outset => 2.0,
            Self::InsetDouble | Self::OutsetDouble | Self::OutsetPartial => 3.0,
        }
    }
}

/// Where a `TableLayoutPanel` adds space once its declared grid is full
/// (`TableLayoutPanelGrowStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableLayoutPanelGrowStyle {
    /// The grid never grows; controls past the last cell are not placed.
    FixedSize,
    /// New rows are added (the default) — cells fill left→right, then wrap down.
    #[default]
    AddRows,
    /// New columns are added — cells fill top→bottom, then wrap right.
    AddColumns,
}

/// Which edge a `TabControl` shows its tabs on (`TabAlignment`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabAlignment {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

impl TabAlignment {
    /// Tabs on the left/right edges run the strip down the side.
    const fn is_vertical(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// How a `TabControl` paints its tabs (`TabAppearance`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabAppearance {
    #[default]
    Normal,
    Buttons,
    FlatButtons,
}

/// How a `TabControl` sizes its tabs (`TabSizeMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabSizeMode {
    #[default]
    Normal,
    /// Every tab is stretched so the row fills the strip.
    FillToRight,
    /// Every tab takes `ItemSize.Width`.
    Fixed,
}

/// Whether a `TabControl` paints its tabs itself or raises an owner-draw event
/// (`TabDrawMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabDrawMode {
    #[default]
    Normal,
    OwnerDrawFixed,
}

/// Resolves a control's `FontRole` to one of the shared DirectWrite formats.
/// Every control paints text through this — never a family or point size of its
/// own, so the library stays themed and DPI-aware.
fn format_for(c: &dyn Canvas, role: FontRole) -> &IDWriteTextFormat {
    let f = c.formats();
    match role {
        FontRole::Caption => &f.caption,
        FontRole::CaptionStrong => &f.caption_strong,
        FontRole::Body => &f.body,
        FontRole::BodyStrong => &f.body_strong,
        FontRole::Heading => &f.heading,
        FontRole::Title => &f.title,
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// FlowLayoutPanel
// ═════════════════════════════════════════════════════════════════════════════

/// One child as the flow engine sees it: its own size, the `Margin` that spaces
/// it from its neighbours, and whether a `FlowBreak` was set on it.
#[derive(Debug, Clone, Copy)]
pub struct FlowChild {
    pub size: Size,
    pub margin: Padding,
    /// `FlowLayoutPanel.SetFlowBreak(child, true)` — after this child, the next
    /// one starts a fresh line (or column), even if room remained.
    ///
    /// It is **subordinate to `WrapContents`**: measured against the toolkit, a
    /// panel with `WrapContents = false` ignores flow breaks entirely and runs
    /// every child onto one line. That is not obvious from the docs — a break
    /// reads like an explicit instruction — but a non-wrapping panel has exactly
    /// one line by definition, and nothing may start a second.
    pub flow_break: bool,
}

impl FlowChild {
    pub fn new(size: Size) -> Self {
        Self { size, margin: Padding::all(3.0), flow_break: false }
    }
}

/// Lays `children` out inside `display` following `direction`.
///
/// With `wrap` off (`WrapContents = false`) the run never wraps on space — it
/// only breaks where a child carries an explicit `FlowBreak` — and the overflow
/// is what the panel would scroll. The four directions are the same algorithm
/// on a mirrored/transposed axis, kept as one match so the wrap rule cannot
/// drift between them.
pub fn flow_layout(
    display: Rect,
    direction: FlowDirection,
    wrap: bool,
    children: &[FlowChild],
) -> Vec<Rect> {
    let mut out = Vec::with_capacity(children.len());

    // `cross` is the size of the current line across the flow axis; it is what a
    // wrap advances by. `first` guards the rule that the first child of a line
    // never wraps (a child wider than the whole line still gets placed).
    let mut cross = 0.0f32;
    let mut first = true;

    // The running origin of the current cell, in the flow's own terms.
    let (mut main, mut lane) = match direction {
        FlowDirection::LeftToRight => (display.left, display.top),
        FlowDirection::RightToLeft => (display.right, display.top),
        FlowDirection::TopDown => (display.top, display.left),
        FlowDirection::BottomUp => (display.bottom, display.left),
    };

    for (idx, ch) in children.iter().enumerate() {
        let (cell_main, cell_cross) = if direction.is_horizontal() {
            (ch.margin.horizontal() + ch.size.width, ch.margin.vertical() + ch.size.height)
        } else {
            (ch.margin.vertical() + ch.size.height, ch.margin.horizontal() + ch.size.width)
        };

        // Would this cell overrun the line? Compare in the flow's direction.
        let overruns = match direction {
            FlowDirection::LeftToRight => main + cell_main > display.right,
            FlowDirection::RightToLeft => main - cell_main < display.left,
            FlowDirection::TopDown => main + cell_main > display.bottom,
            FlowDirection::BottomUp => main - cell_main < display.top,
        };
        // A `FlowBreak` on the PREVIOUS child forces a fresh line here, even if
        // room remained — handled at the top so the break and the space-wrap
        // share one advance. `first` still guards a line's opening cell, which
        // never wraps however wide it is.
        //
        // Both reasons to break are gated on `wrap`: a `WrapContents = false`
        // panel has a single line, so a `FlowBreak` inside it is inert. That is
        // measured behaviour, not an inference from the docs.
        let broke_before = idx > 0 && children[idx - 1].flow_break;
        if !first && wrap && (broke_before || overruns) {
            main = wrap_main(direction, display);
            lane += cross;
            cross = 0.0;
        }

        out.push(place_flow_cell(direction, main, lane, ch));

        main = advance_main(direction, main, cell_main);
        cross = cross.max(cell_cross);
        first = false;
    }
    out
}

/// The main-axis origin a fresh line starts from.
fn wrap_main(direction: FlowDirection, display: Rect) -> f32 {
    match direction {
        FlowDirection::LeftToRight => display.left,
        FlowDirection::RightToLeft => display.right,
        FlowDirection::TopDown => display.top,
        FlowDirection::BottomUp => display.bottom,
    }
}

/// Advances the main-axis origin past a placed cell. The lane (cross axis)
/// always grows in the positive direction when a line wraps — `lane += cross` at
/// the wrap site — because only the *main* axis reverses between `LeftToRight`/
/// `RightToLeft` and `TopDown`/`BottomUp`.
fn advance_main(direction: FlowDirection, main: f32, cell_main: f32) -> f32 {
    match direction {
        FlowDirection::LeftToRight | FlowDirection::TopDown => main + cell_main,
        FlowDirection::RightToLeft | FlowDirection::BottomUp => main - cell_main,
    }
}

fn place_flow_cell(direction: FlowDirection, main: f32, lane: f32, ch: &FlowChild) -> Rect {
    let (w, h) = (ch.size.width, ch.size.height);
    match direction {
        FlowDirection::LeftToRight => {
            let x = main + ch.margin.left;
            let y = lane + ch.margin.top;
            Rect::new(x, y, x + w, y + h)
        }
        FlowDirection::RightToLeft => {
            let right = main - ch.margin.right;
            let y = lane + ch.margin.top;
            Rect::new(right - w, y, right, y + h)
        }
        FlowDirection::TopDown => {
            let x = lane + ch.margin.left;
            let y = main + ch.margin.top;
            Rect::new(x, y, x + w, y + h)
        }
        FlowDirection::BottomUp => {
            let x = lane + ch.margin.left;
            let bottom = main - ch.margin.bottom;
            Rect::new(x, bottom - h, x + w, bottom)
        }
    }
}

/// A line long enough that no real content reaches its end — the stand-in for
/// the *unconstrained* proposed size `GetPreferredSize` measures against. A
/// finite value (rather than `f32::INFINITY`) keeps every comparison and sum in
/// ordinary arithmetic, so no `NaN` can leak into a rectangle.
const UNCONSTRAINED: f32 = 1.0e9;

/// The extent `children` occupy when the flow is given all the room it wants.
///
/// This is what `GetPreferredSize` reports: with no width constraint a wrapping
/// panel does **not** wrap, so an auto-sizing `FlowLayoutPanel` grows to fit its
/// content on one line rather than wrapping inside its current box.
///
/// `wrap` is the panel's real `WrapContents`, and it must be passed through
/// rather than hard-coded to `false`: the space-wrap is already suppressed by
/// the unbounded line, but `FlowBreak` is gated on the same flag, so a panel
/// that wraps still breaks where it was told to while a non-wrapping one does
/// not. Inferring the break rule from the `wrap` argument used for measuring
/// would silently drop the panel's own setting.
///
/// The measurement runs in the *forward* equivalent of the direction
/// (`LeftToRight` for the two horizontal flows, `TopDown` for the two vertical
/// ones): a mirrored flow is the same layout reflected, so its extent is
/// identical, and measuring forward keeps the arithmetic in small positive
/// coordinates instead of counting down from a 10⁹ edge.
pub fn flow_content_size(direction: FlowDirection, wrap: bool, children: &[FlowChild]) -> Size {
    let forward = if direction.is_horizontal() {
        FlowDirection::LeftToRight
    } else {
        FlowDirection::TopDown
    };
    let area = Rect::new(0.0, 0.0, UNCONSTRAINED, UNCONSTRAINED);
    let rects = flow_layout(area, forward, wrap, children);

    let (mut w, mut h) = (0.0f32, 0.0f32);
    for (r, ch) in rects.iter().zip(children) {
        // The trailing margin is part of the extent: it is space the panel must
        // own, exactly as the leading margin already shifted the cell.
        w = w.max(r.right + ch.margin.right);
        h = h.max(r.bottom + ch.margin.bottom);
    }
    Size::new(w.max(0.0), h.max(0.0))
}

/// `FlowLayoutPanel` — declares `FlowDirection` and `WrapContents`; everything
/// else is `Panel`'s.
#[derive(Clone)]
pub struct FlowLayoutPanel {
    pub base: Panel,
    pub flow_direction: FlowDirection,
    pub wrap_contents: bool,
    /// The port of `SetFlowBreak(child, true)`, indexed by child. WinForms keeps
    /// this in an extender-property table rather than on the child, because the
    /// break belongs to *this* panel's arrangement, not to the control; a short
    /// list simply means « no break » for the children past its end.
    pub flow_breaks: Vec<bool>,
}

impl Default for FlowLayoutPanel {
    /// Catalogue: `FlowDirection = LeftToRight`, `WrapContents = true`.
    fn default() -> Self {
        Self {
            base: Panel::default(),
            flow_direction: FlowDirection::default(),
            wrap_contents: true,
            flow_breaks: Vec::new(),
        }
    }
}

impl FlowLayoutPanel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Places `children` inside the panel's client area.
    pub fn arrange(&self, children: &[FlowChild]) -> Vec<Rect> {
        flow_layout(self.base.local_display_rect(), self.flow_direction, self.wrap_contents, children)
    }

    /// The real children, measured and turned into flow cells. A child that
    /// auto-sizes is measured; any other keeps the size it was given.
    pub fn flow_children(&self, c: &dyn Canvas) -> Vec<FlowChild> {
        self.base
            .children
            .iter()
            .enumerate()
            .filter(|(_, ch)| ch.control().visible)
            .map(|(i, ch)| {
                let cb = ch.control();
                let size = if cb.auto_size { ch.preferred_size(c) } else { cb.size() };
                FlowChild {
                    size,
                    margin: cb.margin,
                    flow_break: self.flow_breaks.get(i).copied().unwrap_or(false),
                }
            })
            .collect()
    }

    /// Measures the children, flows them across the panel's client area and
    /// writes each one's rectangle back — the flow panel's replacement for the
    /// inherited Dock/Anchor pass.
    pub fn perform_layout(&mut self, c: &dyn Canvas) {
        let cells = self.flow_children(c);
        let rects = self.arrange(&cells);
        let mut visible = self
            .base
            .children
            .iter_mut()
            .filter(|ch| ch.control().visible);
        for r in rects {
            match visible.next() {
                Some(child) => child.control_mut().bounds = r,
                None => break,
            }
        }
    }

    /// How much bigger the control's box is than the area its children get —
    /// the padding plus whatever the `BorderStyle` eats. Read off `Panel` rather
    /// than recomputed, so a change to the border metric cannot drift from it.
    fn chrome(&self) -> Size {
        let outer = self.control().bounds;
        let inner = self.base.local_display_rect();
        Size::new(
            (outer.right - outer.left) - (inner.right - inner.left),
            (outer.bottom - outer.top) - (inner.bottom - inner.top),
        )
    }
}

impl std::ops::Deref for FlowLayoutPanel {
    type Target = Panel;
    fn deref(&self) -> &Panel {
        &self.base
    }
}

impl std::ops::DerefMut for FlowLayoutPanel {
    fn deref_mut(&mut self) -> &mut Panel {
        &mut self.base
    }
}

impl Control for FlowLayoutPanel {
    fn control(&self) -> &ControlBase {
        &self.base
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
    /// `GetPreferredSize`: the content's own extent plus the panel's chrome —
    /// never the current box, which would make an auto-sizing panel a fixpoint
    /// at whatever size it happens to have.
    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        let content =
            flow_content_size(self.flow_direction, self.wrap_contents, &self.flow_children(c));
        let chrome = self.chrome();
        let mut s = Size::new(content.width + chrome.width, content.height + chrome.height);
        // `GrowOnly` is the default and it means what it says: the preferred
        // size may exceed the current one but never fall below it, so an
        // auto-sizing panel does not snap smaller when a child is removed.
        if self.base.auto_size_mode == AutoSizeMode::GrowOnly {
            let cur = self.control().size();
            s = Size::new(s.width.max(cur.width), s.height.max(cur.height));
        }
        self.control().clamp(s)
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        // Straight through `Panel`: the background, the `BorderStyle` and the
        // children are its job, and duplicating any of it here would let the
        // two drift.
        self.base.paint(c, bounds);
    }
    fn type_name(&self) -> &'static str {
        "FlowLayoutPanel"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// TableLayoutPanel
// ═════════════════════════════════════════════════════════════════════════════

/// A column or row style — `TableLayoutColumnStyle` / `TableLayoutRowStyle`.
/// `size` is a pixel width/height for `Absolute`, a weight for `Percent`, and
/// ignored for `AutoSize`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackStyle {
    pub size_type: SizeType,
    pub size: f32,
}

impl Default for TrackStyle {
    fn default() -> Self {
        Self { size_type: SizeType::AutoSize, size: 0.0 }
    }
}

impl TrackStyle {
    pub const fn absolute(px: f32) -> Self {
        Self { size_type: SizeType::Absolute, size: px }
    }
    pub const fn percent(weight: f32) -> Self {
        Self { size_type: SizeType::Percent, size: weight }
    }
    pub const fn auto() -> Self {
        Self { size_type: SizeType::AutoSize, size: 0.0 }
    }
}

/// Resolves one axis of a table into concrete track sizes.
///
/// `available` is the space left for the tracks (the client extent already less
/// the cell-border spacing). `auto_content[i]` is the measured content extent of
/// track `i`, used only when its style is `AutoSize`.
///
/// The order is the toolkit's: `Absolute` tracks take their fixed size,
/// `AutoSize` tracks take their content, and the `Percent` tracks divide what is
/// left **weighted by their values relative to each other** — so `50/50` and
/// `30/30` behave identically, and percents that do not sum to 100 still consume
/// the whole remainder.
///
/// Two rules here were measured against the toolkit rather than reasoned out,
/// and both are surprising:
///
/// * **Nothing is left unused.** If no track is `Percent`, the space the fixed
///   and auto tracks did not take goes to the **last** track, whatever its
///   style. A three-column table of `Absolute(100)` in a 300-wide panel really
///   does come out `[100, 100, 100]`, but at 500 wide it is `[100, 100, 300]` —
///   the last column absorbs the slack instead of the table under-filling.
/// * **The percent remainder is not spread.** Each percent share is floored and
///   the entire rounding remainder goes to the **last** percent track. At 302
///   across three equal columns the toolkit gives `[100, 100, 102]`, not the
///   `[101, 101, 100]` a largest-remainder distribution would produce.
pub fn resolve_tracks(available: f32, styles: &[TrackStyle], auto_content: &[f32]) -> Vec<f32> {
    let mut sizes = vec![0.0f32; styles.len()];
    if styles.is_empty() {
        return sizes;
    }
    let mut used = 0.0f32;
    for (i, s) in styles.iter().enumerate() {
        match s.size_type {
            SizeType::Absolute => sizes[i] = s.size.max(0.0),
            SizeType::AutoSize => sizes[i] = auto_content.get(i).copied().unwrap_or(0.0).max(0.0),
            SizeType::Percent => continue,
        }
        used += sizes[i];
    }

    let remaining = (available - used).max(0.0);
    let sum_pct: f32 = styles.iter().filter(|s| s.size_type == SizeType::Percent).map(|s| s.size).sum();

    if sum_pct <= 0.0 {
        // No percent track to soak up the slack, so the last track takes it.
        let last = sizes.len() - 1;
        sizes[last] += remaining;
        return sizes;
    }

    let target = remaining.floor();
    let mut floors_sum = 0.0f32;
    let mut last_pct = 0usize;
    for (i, s) in styles.iter().enumerate() {
        if s.size_type == SizeType::Percent {
            let floor = (target * (s.size / sum_pct)).floor();
            sizes[i] = floor;
            floors_sum += floor;
            last_pct = i;
        }
    }
    sizes[last_pct] += target - floors_sum;
    sizes
}

/// A child's grid placement request. `col`/`row` are `-1` when the designer left
/// the cell unset (`Column = -1, Row = -1`), which means « first empty cell ».
#[derive(Debug, Clone, Copy)]
pub struct CellSpec {
    pub col: i32,
    pub row: i32,
    pub col_span: u32,
    pub row_span: u32,
}

impl Default for CellSpec {
    fn default() -> Self {
        Self { col: -1, row: -1, col_span: 1, row_span: 1 }
    }
}

/// The number of columns the fill actually wraps at.
///
/// `GrowStyle` does **not** change the scan order (see [`place_cells`]); it only
/// decides which dimension may be invented. Under `AddColumns` the row count is
/// the fixed one, so the column count is whatever is needed to fit every child
/// in that many rows — five children in two rows means three columns.
fn fill_columns(col_count: u32, row_count: u32, grow: TableLayoutPanelGrowStyle, n: usize) -> u32 {
    match grow {
        TableLayoutPanelGrowStyle::AddColumns => {
            let rows = row_count.max(1);
            let needed = (n as u32).div_ceil(rows);
            col_count.max(needed).max(1)
        }
        _ => col_count.max(1),
    }
}

/// Assigns a concrete `(column, row)` to every child.
///
/// Explicitly-placed children are honoured first and reserve their spanned
/// cells. The rest fill the first empty cell scanning **left→right, then
/// top→bottom** — and that is true under *every* `GrowStyle`.
///
/// That last point is the trap this function used to get wrong. `AddColumns`
/// reads like « fill downwards and add columns as you go », but measured against
/// the toolkit the scan stays row-major: `AddColumns` only means the *column*
/// count is the one free to grow, so the grid is first widened to
/// [`fill_columns`] and then filled left→right like any other. Transposing the
/// scan puts every child in the wrong cell as soon as there is more than one
/// row.
///
/// `FixedSize` never invents a track past the declared grid. WinForms *throws*
/// when a child does not fit; this library has no exceptions, so the child is
/// parked at the cell it was tried against — see `TableLayoutPanel::cells` for
/// the deviation note.
pub fn place_cells(
    col_count: u32,
    row_count: u32,
    grow: TableLayoutPanelGrowStyle,
    cells: &[CellSpec],
) -> Vec<(u32, u32)> {
    let columns = fill_columns(col_count, row_count, grow, cells.len());

    // Free helpers rather than closures: a closure that captured `occupied`
    // immutably (for the fit test) would forbid the mutable capture the marking
    // needs, so both take the set explicitly.
    fn mark(occ: &mut HashSet<(u32, u32)>, c: u32, r: u32, spec: &CellSpec) {
        for dc in 0..spec.col_span.max(1) {
            for dr in 0..spec.row_span.max(1) {
                occ.insert((c + dc, r + dr));
            }
        }
    }
    fn fits(occ: &HashSet<(u32, u32)>, c: u32, r: u32, spec: &CellSpec, columns: u32) -> bool {
        // A span may not run off the right edge of the grid.
        if c + spec.col_span.max(1) > columns {
            return false;
        }
        for dc in 0..spec.col_span.max(1) {
            for dr in 0..spec.row_span.max(1) {
                if occ.contains(&(c + dc, r + dr)) {
                    return false;
                }
            }
        }
        true
    }

    let mut occupied: HashSet<(u32, u32)> = HashSet::new();
    let mut result = vec![(0u32, 0u32); cells.len()];

    for (i, spec) in cells.iter().enumerate() {
        if spec.col >= 0 && spec.row >= 0 {
            let (c, r) = (spec.col as u32, spec.row as u32);
            result[i] = (c, r);
            mark(&mut occupied, c, r, spec);
        }
    }

    let mut cursor = 0u32;
    for (i, spec) in cells.iter().enumerate() {
        if spec.col >= 0 && spec.row >= 0 {
            continue;
        }
        loop {
            let (c, r) = (cursor % columns, cursor / columns);

            if grow == TableLayoutPanelGrowStyle::FixedSize && r >= row_count.max(1) {
                // Out of the fixed grid: park it and stop searching.
                result[i] = (c, r);
                break;
            }
            if fits(&occupied, c, r, spec, columns) {
                result[i] = (c, r);
                mark(&mut occupied, c, r, spec);
                cursor += 1;
                break;
            }
            cursor += 1;
        }
    }
    result
}

/// `TableLayoutPanel` — declares its counts, styles, cell-border style and grow
/// style. The styles are the port's `TableLayout{Column,Row}StyleCollection`.
#[derive(Clone)]
pub struct TableLayoutPanel {
    pub base: Panel,
    pub column_count: u32,
    pub row_count: u32,
    pub column_styles: Vec<TrackStyle>,
    pub row_styles: Vec<TrackStyle>,
    /// `CellBorderStyle`. `None` and `Single` are painted exactly. The five
    /// carved styles — `Inset`, `InsetDouble`, `Outset`, `OutsetDouble`,
    /// `OutsetPartial` — reserve the right thickness (so every cell rectangle is
    /// correct) but are **approximated in paint** as flat lines of that
    /// thickness: their light/dark bevel is a Win32 3-D edge, and this library
    /// paints flat themed surfaces with no engraved-edge vocabulary.
    pub cell_border_style: TableLayoutPanelCellBorderStyle,
    pub grow_style: TableLayoutPanelGrowStyle,
    /// The port of `SetCellPosition`/`SetColumnSpan`/`SetRowSpan`, indexed by
    /// child. WinForms keeps these in an extender-property table for the same
    /// reason: a cell address belongs to *this* table's arrangement, not to the
    /// control sitting in it. A short list leaves the remaining children
    /// unplaced (`-1, -1`), which is the « first empty cell » request.
    ///
    /// **Deliberate deviation.** When `GrowStyle = FixedSize` and a child does
    /// not fit the declared grid, WinForms raises an `ArgumentException` and lays
    /// nothing out. This library is exception-free — a control is a value that
    /// measures and paints itself — so the overflowing child is instead assigned
    /// the cell just past the grid and can be recognised (and dropped) by the
    /// caller. The parity harness records this as `overflowThrew = 0` where the
    /// toolkit reports `1`; that difference is intended, not a defect.
    pub cells: Vec<CellSpec>,
}

impl Default for TableLayoutPanel {
    /// Catalogue: `ColumnCount = 0`, `RowCount = 0`, `CellBorderStyle = None`,
    /// `GrowStyle = AddRows`. The style collections start empty.
    fn default() -> Self {
        Self {
            base: Panel::default(),
            column_count: 0,
            row_count: 0,
            column_styles: Vec::new(),
            row_styles: Vec::new(),
            cell_border_style: TableLayoutPanelCellBorderStyle::default(),
            grow_style: TableLayoutPanelGrowStyle::default(),
            cells: Vec::new(),
        }
    }
}

impl TableLayoutPanel {
    pub fn new() -> Self {
        Self::default()
    }

    /// `GetColumnWidths()` — the width of each column **including the cell
    /// border line that precedes it**, which is the unit the toolkit's accessor
    /// reports and the unit [`cell_rect_in`] consumes.
    ///
    /// The distinction matters as soon as `CellBorderStyle` is not `None`: a
    /// three-column table 301 DIP wide with `Single` borders reports
    /// `[100, 100, 100]`, and the *drawable* cell inside each is 99. Reporting
    /// the drawable 99 would be a defensible unit, but it is not the toolkit's,
    /// and the two are easy to confuse — hence this note.
    ///
    /// The list is as long as the grid actually grew to, not as long as
    /// `ColumnCount` was declared: a table left at `ColumnCount = 0` still
    /// reports the columns its children created. Missing styles default to
    /// `AutoSize`, as the toolkit does when there are fewer styles than columns.
    pub fn column_widths(&self, auto_content: &[f32]) -> Vec<f32> {
        let (n, _) = self.effective_counts(&self.placements());
        let styles = padded_styles(&self.column_styles, n);
        let d = self.base.local_display_rect();
        let raw = resolve_tracks((d.right - d.left) - self.spacing(n), &styles, auto_content);
        self.with_border(raw)
    }

    /// `GetRowHeights()`, mirroring [`Self::column_widths`] — including the
    /// border line above each row.
    pub fn row_heights(&self, auto_content: &[f32]) -> Vec<f32> {
        let (_, n) = self.effective_counts(&self.placements());
        let styles = padded_styles(&self.row_styles, n);
        let d = self.base.local_display_rect();
        let raw = resolve_tracks((d.bottom - d.top) - self.spacing(n), &styles, auto_content);
        self.with_border(raw)
    }

    /// Converts drawable track sizes into the accessor's unit by folding the
    /// preceding border line into each.
    fn with_border(&self, raw: Vec<f32>) -> Vec<f32> {
        let b = self.cell_border_style.thickness();
        raw.into_iter().map(|v| v + b).collect()
    }

    /// The space `count` tracks give up to cell borders: one line between every
    /// pair, plus one on each outer edge.
    fn spacing(&self, count: usize) -> f32 {
        self.cell_border_style.thickness() * (count as f32 + 1.0)
    }

    /// The cell address requested for child `i` — unset (`-1, -1`, span 1×1)
    /// when the table was given no spec for it.
    pub fn cell_spec(&self, i: usize) -> CellSpec {
        self.cells.get(i).copied().unwrap_or_default()
    }

    /// The resolved `(column, row)` of every child, in child order.
    pub fn placements(&self) -> Vec<(u32, u32)> {
        let specs: Vec<CellSpec> = (0..self.base.children.len()).map(|i| self.cell_spec(i)).collect();
        place_cells(self.column_count, self.row_count, self.grow_style, &specs)
    }

    /// The content extent each column and row must accommodate, measured from
    /// the real children — what an `AutoSize` track is sized to.
    ///
    /// A child that spans several tracks contributes to **none** of them: the
    /// toolkit resolves spanned content only after the single-cell tracks are
    /// known, and a table whose auto column is sized by a spanning child is
    /// pathological anyway. Documented rather than silently mis-attributed.
    pub fn measure_auto_tracks(&self, c: &dyn Canvas) -> (Vec<f32>, Vec<f32>) {
        let placements = self.placements();
        let (n_cols, n_rows) = self.effective_counts(&placements);
        let mut cols = vec![0.0f32; n_cols];
        let mut rows = vec![0.0f32; n_rows];

        for (i, child) in self.base.children.iter().enumerate() {
            let cb = child.control();
            if !cb.visible {
                continue;
            }
            let spec = self.cell_spec(i);
            if spec.col_span.max(1) != 1 || spec.row_span.max(1) != 1 {
                continue;
            }
            let (col, row) = placements[i];
            let size = if cb.auto_size { child.preferred_size(c) } else { cb.size() };
            if let Some(w) = cols.get_mut(col as usize) {
                *w = w.max(size.width + cb.margin.horizontal());
            }
            if let Some(h) = rows.get_mut(row as usize) {
                *h = h.max(size.height + cb.margin.vertical());
            }
        }
        (cols, rows)
    }

    /// The declared counts, widened to hold whatever the placement actually
    /// used — the grid grows under `AddRows`/`AddColumns`.
    ///
    /// Under `FixedSize` it does **not** grow: the grid is exactly what was
    /// declared, and a child that did not fit (see [`Self::cells`]) must not
    /// conjure a track for itself. WinForms throws instead of placing that
    /// child, so counting its parked cell here would invent a row the toolkit
    /// never has.
    fn effective_counts(&self, placements: &[(u32, u32)]) -> (usize, usize) {
        if self.grow_style == TableLayoutPanelGrowStyle::FixedSize {
            return (self.column_count.max(1) as usize, self.row_count.max(1) as usize);
        }
        let mut cols = self.column_count as usize;
        let mut rows = self.row_count as usize;
        for &(c, r) in placements {
            cols = cols.max(c as usize + 1);
            rows = rows.max(r as usize + 1);
        }
        (cols, rows)
    }

    /// The fully resolved track sizes for the panel's current box, measuring the
    /// `AutoSize` tracks against the real children.
    pub fn tracks(&self, c: &dyn Canvas) -> (Vec<f32>, Vec<f32>) {
        let (auto_cols, auto_rows) = self.measure_auto_tracks(c);
        (self.column_widths(&auto_cols), self.row_heights(&auto_rows))
    }

    /// The rectangle of one cell, resolved against the real children — the
    /// geometry a host would otherwise have to re-derive from
    /// [`Self::column_widths`] and [`Self::row_heights`].
    pub fn cell_rect(&self, c: &dyn Canvas, col: u32, row: u32) -> Rect {
        let (widths, heights) = self.tracks(c);
        cell_rect_in(
            self.base.local_display_rect(),
            &widths,
            &heights,
            self.cell_border_style.thickness(),
            CellSpec { col: col as i32, row: row as i32, col_span: 1, row_span: 1 },
        )
    }

    /// The rectangle each child occupies, in child order.
    ///
    /// The cell is deflated by the child's `Margin`, and the child is then placed
    /// **inside** that box according to its own `Dock`/`Anchor` — a table cell is
    /// a miniature container, not a box that swallows whatever is put in it.
    /// `Dock = Fill`, or anchoring to both edges of an axis, stretches along that
    /// axis; otherwise the child keeps its own size and is pinned to the edges it
    /// is anchored to (`Top | Left`, the default, puts it at the cell's
    /// top-left). A child with no anchor on an axis is centred on it.
    pub fn child_rects(&self, c: &dyn Canvas) -> Vec<Rect> {
        let (widths, heights) = self.tracks(c);
        let origin = self.base.local_display_rect();
        let border = self.cell_border_style.thickness();
        let placements = self.placements();
        self.base
            .children
            .iter()
            .enumerate()
            .map(|(i, child)| {
                let spec = self.cell_spec(i);
                let (col, row) = placements[i];
                let placed = CellSpec {
                    col: col as i32,
                    row: row as i32,
                    col_span: spec.col_span,
                    row_span: spec.row_span,
                };
                let cell = cell_rect_in(origin, &widths, &heights, border, placed);
                let cb = child.control();
                let m = cb.margin;
                let inner = Rect::new(
                    cell.left + m.left,
                    cell.top + m.top,
                    (cell.right - m.right).max(cell.left + m.left),
                    (cell.bottom - m.bottom).max(cell.top + m.top),
                );
                let size = if cb.auto_size { child.preferred_size(c) } else { cb.size() };
                place_in_cell(inner, cb, size)
            })
            .collect()
    }

    /// Assigns every child the rectangle of its cell.
    pub fn perform_layout(&mut self, c: &dyn Canvas) {
        let rects = self.child_rects(c);
        for (child, r) in self.base.children.iter_mut().zip(rects) {
            child.control_mut().bounds = r;
        }
    }
}

/// Places one child inside the cell box it was given, honouring its `Dock` and
/// `Anchor` — the miniature layout a `TableLayoutPanel` runs per cell.
///
/// Anchoring to both edges of an axis (or docking `Fill`) stretches along it;
/// one edge pins to that edge at the child's own size; neither centres. This is
/// the same vocabulary `layout::layout` uses for a free-floating child, applied
/// to a cell instead of a client rectangle.
///
/// Public because it is the whole per-cell rule in one pure function: a caller
/// that has already resolved its tracks (the parity harness, a host doing its
/// own measuring) can place a child correctly without a `Canvas` — and without
/// transcribing the rule, which is how the two drift apart.
pub fn place_in_cell(cell: Rect, cb: &ControlBase, size: Size) -> Rect {
    let (cw, ch) = (cell.right - cell.left, cell.bottom - cell.top);
    if cb.dock == crate::enums::DockStyle::Fill {
        return cell;
    }
    let axis = |lo_a: bool, hi_a: bool, lo: f32, extent: f32, want: f32| -> (f32, f32) {
        match (lo_a, hi_a) {
            // Both edges held: stretch to the cell.
            (true, true) => (lo, extent),
            // One edge only: keep the size, pin to that edge.
            (true, false) => (lo, want.min(extent)),
            (false, true) => (lo + extent - want.min(extent), want.min(extent)),
            // Free on this axis: centred, halving to whole DIP as the toolkit does.
            (false, false) => (lo + ((extent - want.min(extent)) / 2.0).floor(), want.min(extent)),
        }
    };
    let (x, w) = axis(
        cb.anchor.contains(AnchorStyles::LEFT),
        cb.anchor.contains(AnchorStyles::RIGHT),
        cell.left,
        cw,
        size.width,
    );
    let (y, h) = axis(
        cb.anchor.contains(AnchorStyles::TOP),
        cb.anchor.contains(AnchorStyles::BOTTOM),
        cell.top,
        ch,
        size.height,
    );
    Rect::new(x, y, x + w, y + h)
}

/// The rectangle of the cell `spec` addresses, inside a grid whose tracks are
/// `widths`/`heights` and whose lines are `border` thick.
///
/// `widths`/`heights` are in the **accessor's unit** — what
/// [`TableLayoutPanel::column_widths`] returns, i.e. each track *including* the
/// border line that precedes it. So the cell starts one border past the running
/// sum, and gives that border back out of its own extent. Passing drawable track
/// sizes here instead would shift every cell left by one line.
pub fn cell_rect_in(origin: Rect, widths: &[f32], heights: &[f32], border: f32, spec: CellSpec) -> Rect {
    let col = spec.col.max(0) as usize;
    let row = spec.row.max(0) as usize;
    let col_span = spec.col_span.max(1) as usize;
    let row_span = spec.row_span.max(1) as usize;

    let x = origin.left + border + widths.iter().take(col).sum::<f32>();
    let y = origin.top + border + heights.iter().take(row).sum::<f32>();
    let w = (widths.iter().skip(col).take(col_span).sum::<f32>() - border).max(0.0);
    let h = (heights.iter().skip(row).take(row_span).sum::<f32>() - border).max(0.0);
    Rect::new(x, y, x + w, y + h)
}

/// Paints the grid of cell-border lines over `origin`, as thin filled bands.
///
/// Lines are drawn as rectangles rather than strokes because the border can be
/// 2 or 3 pixels thick (`Inset`/`Outset` and their doubles) and a stroke is one
/// physical pixel by contract.
///
/// `widths`/`heights` are in the accessor's unit (each track carries its leading
/// border), so a line sits at every running sum, plus one closing the grid.
fn paint_cell_grid(c: &dyn Canvas, origin: Rect, widths: &[f32], heights: &[f32], border: f32) {
    if border <= 0.0 {
        return;
    }
    let colour = c.theme().divider;
    let grid_w: f32 = widths.iter().sum::<f32>() + border;
    let grid_h: f32 = heights.iter().sum::<f32>() + border;
    let (right, bottom) = (origin.left + grid_w, origin.top + grid_h);

    let mut x = origin.left;
    for i in 0..=widths.len() {
        c.fill_rounded(&Rect::new(x, origin.top, x + border, bottom), 0.0, &colour);
        if let Some(w) = widths.get(i) {
            x += w;
        }
    }
    let mut y = origin.top;
    for i in 0..=heights.len() {
        c.fill_rounded(&Rect::new(origin.left, y, right, y + border), 0.0, &colour);
        if let Some(h) = heights.get(i) {
            y += h;
        }
    }
}

/// Extends a style list to `count`, defaulting missing tracks to `AutoSize`.
/// `count` is the grid's EFFECTIVE track count, not the declared one — a table
/// left at `ColumnCount = 0` still has the columns its children created.
fn padded_styles(styles: &[TrackStyle], count: usize) -> Vec<TrackStyle> {
    let mut out = styles.to_vec();
    out.resize(count, TrackStyle::auto());
    out
}

impl Control for TableLayoutPanel {
    fn control(&self) -> &ControlBase {
        &self.base
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
    /// The grid's own extent: every track at its resolved size, plus the border
    /// lines and the panel's chrome.
    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        let (auto_cols, auto_rows) = self.measure_auto_tracks(c);
        let border = self.cell_border_style.thickness();
        let content = Size::new(
            auto_cols.iter().sum::<f32>() + border * (auto_cols.len() as f32 + 1.0),
            auto_rows.iter().sum::<f32>() + border * (auto_rows.len() as f32 + 1.0),
        );
        let outer = self.control().bounds;
        let inner = self.base.local_display_rect();
        let s = Size::new(
            content.width + (outer.right - outer.left) - (inner.right - inner.left),
            content.height + (outer.bottom - outer.top) - (inner.bottom - inner.top),
        );
        self.control().clamp(s)
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        // `Panel` owns the background, the `BorderStyle` and the children.
        self.base.paint(c, bounds);
        // The cell grid goes on top: `Panel::paint` draws the children in the
        // same call, and the lines run *between* cells, where a child's margin
        // has already kept it clear.
        if self.cell_border_style != TableLayoutPanelCellBorderStyle::None {
            let (widths, heights) = self.tracks(c);
            // The panel's own `BorderStyle` is the inset the crossing adds back;
            // `cell_border_style` is a different thing entirely and belongs to
            // the grid, not to the client box.
            let origin = local_to_canvas(
                self.base.local_display_rect(),
                bounds,
                border_thickness(self.base.border_style),
            );
            paint_cell_grid(c, origin, &widths, &heights, self.cell_border_style.thickness());
        }
    }
    fn type_name(&self) -> &'static str {
        "TableLayoutPanel"
    }
}

impl std::ops::Deref for TableLayoutPanel {
    type Target = Panel;
    fn deref(&self) -> &Panel {
        &self.base
    }
}

impl std::ops::DerefMut for TableLayoutPanel {
    fn deref_mut(&mut self) -> &mut Panel {
        &mut self.base
    }
}

/// Maps a container's **client** rectangle onto the canvas box it is being
/// painted into.
///
/// This is the single place this family crosses between the two spaces. Child
/// `bounds` are parent-relative — what WinForms means by `Bounds` — so a
/// container that moves carries its descendants with it and nothing has to be
/// shifted by hand. They become canvas coordinates only here, at paint time.
///
/// The two insets are split, and the split is the whole subtlety:
///
/// * **Padding is already in `local`.** `local_display_rect` takes its origin
///   from the padding alone, so a 200×100 box padded 10 reports `10,10,…`.
///   Re-adding it here would indent every child twice.
/// * **The border is not, and must be added here.** Client coordinates start
///   *inside* the border, so the border costs `2 × inset` of client SIZE but
///   never offsets the client origin — which is why `table-border-single`
///   (padding 0, border 1) reports `0,0,301,98`. Crossing to canvas is where
///   that inset comes back; omit it and every bordered container paints its
///   children one DIP high and left.
///
/// The check that catches a mistake in either half: the result must equal
/// `containers::client_rect_on_canvas`, i.e. `bounds.left + padding.left +
/// inset`, which is algebraically the same under either way of splitting the
/// two insets.
fn local_to_canvas(local: Rect, bounds: Rect, inset: f32) -> Rect {
    translate(local, bounds.left + inset, bounds.top + inset)
}

// ═════════════════════════════════════════════════════════════════════════════
// SplitContainer, SplitterPanel, Splitter
// ═════════════════════════════════════════════════════════════════════════════

/// The three rectangles a split produces, in container-local coordinates.
// No `Debug`: `Rect` comes from the drawing layer and does not implement it.
#[derive(Clone, Copy)]
pub struct SplitRects {
    pub panel1: Rect,
    pub splitter: Rect,
    pub panel2: Rect,
}

/// The inputs the split arithmetic needs, packed so the function stays a pure
/// one-argument transform of `(bounds, params)`.
#[derive(Debug, Clone, Copy)]
pub struct SplitParams {
    pub orientation: Orientation,
    pub splitter_distance: f32,
    pub splitter_width: f32,
    pub panel1_min: f32,
    pub panel2_min: f32,
    pub panel1_collapsed: bool,
    pub panel2_collapsed: bool,
}

/// Clamps a splitter distance so both panels keep at least their minimum and the
/// splitter itself fits. `total` is the extent along the split axis.
pub fn clamp_distance(total: f32, distance: f32, width: f32, min1: f32, min2: f32) -> f32 {
    let hi = (total - width - min2).max(min1.min(total));
    distance.max(min1).min(hi).max(0.0)
}

/// Splits `bounds` into panel1 / splitter / panel2.
///
/// The naming is the toolkit's, and its trap: **`Orientation::Vertical`** (the
/// default) is a *vertical splitter*, so the panels sit **side by side** and
/// `SplitterDistance` is Panel1's **width**. `Orientation::Horizontal` stacks
/// them, and the distance is Panel1's **height**. A collapsed panel yields the
/// whole area to its sibling and hides the splitter.
pub fn split_layout(bounds: Rect, p: SplitParams) -> SplitRects {
    let vertical_split = p.orientation == Orientation::Vertical;
    let total = if vertical_split { bounds.right - bounds.left } else { bounds.bottom - bounds.top };

    let d = clamp_distance(total, p.splitter_distance, p.splitter_width, p.panel1_min, p.panel2_min);
    let mut rects = if vertical_split {
        let x1 = bounds.left + d;
        let x2 = x1 + p.splitter_width;
        SplitRects {
            panel1: Rect::new(bounds.left, bounds.top, x1, bounds.bottom),
            splitter: Rect::new(x1, bounds.top, x2, bounds.bottom),
            panel2: Rect::new(x2, bounds.top, bounds.right, bounds.bottom),
        }
    } else {
        let y1 = bounds.top + d;
        let y2 = y1 + p.splitter_width;
        SplitRects {
            panel1: Rect::new(bounds.left, bounds.top, bounds.right, y1),
            splitter: Rect::new(bounds.left, y1, bounds.right, y2),
            panel2: Rect::new(bounds.left, y2, bounds.right, bounds.bottom),
        }
    };

    // Collapsing does NOT zero the hidden panel's rectangle. Measured, the
    // toolkit gives the whole area to the surviving panel and leaves the
    // collapsed one — and the splitter — reporting the geometry they had; they
    // are merely not shown. `SplitterRectangle` keeps answering from that stale
    // position, which is why collapsing and re-expanding restores the split.
    if p.panel1_collapsed {
        rects.panel2 = bounds;
    } else if p.panel2_collapsed {
        rects.panel1 = bounds;
    }
    rects
}

/// New splitter distance after the container is resized along the split axis.
/// `FixedPanel` decides who absorbs the change: nobody (distance scales with the
/// container), Panel1 (distance unchanged), or Panel2 (distance shifts by the
/// whole delta so Panel2 keeps its size).
///
/// `None` scales by the **whole** extents, `new / old` — not by the extents less
/// the splitter. Excluding the splitter width looks more principled (it is the
/// space the panels actually share) but it is not what the toolkit does, and the
/// two disagree by a few DIP on any real resize.
pub fn adjusted_distance(
    old_total: f32,
    new_total: f32,
    distance: f32,
    fixed: FixedPanel,
    // Kept in the signature although the measured rule does not use it: the
    // splitter width is what a caller naturally has to hand, and dropping the
    // parameter would silently change every call site's meaning.
    _splitter_width: f32,
) -> f32 {
    let moved = match fixed {
        FixedPanel::Panel1 => distance,
        FixedPanel::Panel2 => distance + (new_total - old_total),
        FixedPanel::None => {
            if old_total <= 0.0 {
                distance
            } else {
                distance * (new_total / old_total)
            }
        }
    };
    // `SplitterDistance` is an `Int32`, so the scaled value is truncated, not
    // rounded: 70 × 500/300 = 116.67 settles at 116, never 117.
    moved.trunc()
}

/// Same rectangle, moved — the local→canvas step for an already-arranged box.
fn translate(r: Rect, dx: f32, dy: f32) -> Rect {
    Rect::new(r.left + dx, r.top + dy, r.right + dx, r.bottom + dy)
}

/// `SplitterPanel` — the fixed pair of panels a `SplitContainer` hosts. It adds
/// nothing over `Panel` in this port (its `Height`/`Width`/`AutoSizeMode` are
/// `Panel`'s), but it is its own type because `Panel1`/`Panel2` are typed
/// `SplitterPanel` and cannot be reparented or removed.
#[derive(Clone, Default)]
pub struct SplitterPanel {
    pub base: Panel,
}

impl Control for SplitterPanel {
    fn control(&self) -> &ControlBase {
        &self.base
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        self.base.preferred_size(c)
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        // `Panel` paints the background, the `BorderStyle` and the children.
        self.base.paint(c, bounds);
    }
    fn type_name(&self) -> &'static str {
        "SplitterPanel"
    }
}

impl std::ops::Deref for SplitterPanel {
    type Target = Panel;
    fn deref(&self) -> &Panel {
        &self.base
    }
}

impl std::ops::DerefMut for SplitterPanel {
    fn deref_mut(&mut self) -> &mut Panel {
        &mut self.base
    }
}

/// `SplitContainer` — declares the split geometry and its two `SplitterPanel`s;
/// derives from `ContainerControl`.
///
/// ## Re-declared only to be hidden
///
/// The catalogue lists `AutoScroll` and `AutoScrollPosition` as declared here,
/// but the toolkit re-declares them for one reason: to take them off the
/// designer surface. A split container does not scroll — **its panels do**, and
/// each `SplitterPanel` is a `Panel` with its own auto-scroll state. So neither
/// gets a field on this type: the state lives once, on the composed
/// `ContainerControl`, and is reached through it like any other inherited
/// property. Adding fields here would create a second copy that no code reads
/// and that would quietly disagree with the base.
#[derive(Clone)]
pub struct SplitContainer {
    pub base: ContainerControl,
    pub panel1: SplitterPanel,
    pub panel2: SplitterPanel,
    pub orientation: Orientation,
    pub fixed_panel: FixedPanel,
    pub is_splitter_fixed: bool,
    pub border_style: BorderStyle,
    pub splitter_distance: f32,
    pub splitter_increment: f32,
    pub splitter_width: f32,
    pub panel1_min_size: f32,
    pub panel2_min_size: f32,
    pub panel1_collapsed: bool,
    pub panel2_collapsed: bool,
}

impl Default for SplitContainer {
    /// Catalogue: `Orientation = Vertical`, `FixedPanel = None`,
    /// `IsSplitterFixed = false`, `BorderStyle = None`, `SplitterDistance = 50`,
    /// `SplitterIncrement = 1`, `SplitterWidth = 4`, `Panel{1,2}MinSize = 25`,
    /// panels not collapsed.
    fn default() -> Self {
        Self {
            base: ContainerControl::default(),
            panel1: SplitterPanel::default(),
            panel2: SplitterPanel::default(),
            orientation: Orientation::default(),
            fixed_panel: FixedPanel::default(),
            is_splitter_fixed: false,
            border_style: BorderStyle::None,
            splitter_distance: 50.0,
            splitter_increment: 1.0,
            splitter_width: 4.0,
            panel1_min_size: 25.0,
            panel2_min_size: 25.0,
            panel1_collapsed: false,
            panel2_collapsed: false,
        }
    }
}

impl SplitContainer {
    pub fn new() -> Self {
        Self::default()
    }

    fn params(&self) -> SplitParams {
        SplitParams {
            orientation: self.orientation,
            splitter_distance: self.splitter_distance,
            splitter_width: self.splitter_width,
            panel1_min: self.panel1_min_size,
            panel2_min: self.panel2_min_size,
            panel1_collapsed: self.panel1_collapsed,
            panel2_collapsed: self.panel2_collapsed,
        }
    }

    /// The container's client area, in **client coordinates** — the space the
    /// rectangles it hands its two panels are measured in. Inherited unchanged
    /// from `ContainerControl`, so it carries the padding inset in its origin.
    ///
    /// `BorderStyle` deliberately does **not** deflate this. On a
    /// `SplitContainer` the border belongs to the two `SplitterPanel`s, not to
    /// the split, so the geometry of a `Fixed3D` container is identical to a
    /// borderless one — measured, the two agree exactly. Insetting here would
    /// shrink both panels for a border neither of them draws.
    pub fn local_display_rect(&self) -> Rect {
        self.base.local_display_rect()
    }

    /// The three rectangles for the container's client area, in local space.
    pub fn arrange(&self) -> SplitRects {
        split_layout(self.local_display_rect(), self.params())
    }

    /// Gives each `SplitterPanel` the bounds the split resolved for it, so the
    /// panels (and their own children) lay out against real geometry.
    pub fn perform_layout(&mut self) {
        let rects = self.arrange();
        self.panel1.control_mut().bounds = rects.panel1;
        self.panel2.control_mut().bounds = rects.panel2;
        self.panel1.perform_layout();
        self.panel2.perform_layout();
    }
}

impl std::ops::Deref for SplitContainer {
    type Target = ContainerControl;
    fn deref(&self) -> &ContainerControl {
        &self.base
    }
}

impl std::ops::DerefMut for SplitContainer {
    fn deref_mut(&mut self) -> &mut ContainerControl {
        &mut self.base
    }
}

impl Control for SplitContainer {
    fn control(&self) -> &ControlBase {
        &self.base
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
    fn preferred_size(&self, _c: &dyn Canvas) -> Size {
        self.control().clamp(self.control().size())
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        // Arrange in client space, then cross into canvas space once — the same
        // rectangles the panels carry as their parent-relative `bounds`.
        //
        // The crossing adds NO border inset here, unlike every `Panel`-derived
        // container: a `SplitContainer`'s own `BorderStyle` belongs to its two
        // `SplitterPanel`s, not to the split, so it neither shrinks the client
        // box nor shifts it. Measured, a `Fixed3D` container and a borderless
        // one lay out identically.
        let local = self.arrange();
        let (dx, dy) = (bounds.left, bounds.top);
        let rects = SplitRects {
            panel1: translate(local.panel1, dx, dy),
            splitter: translate(local.splitter, dx, dy),
            panel2: translate(local.panel2, dx, dy),
        };
        // An opaque ground first, so the panels' children never sit on whatever
        // was behind the container…
        c.fill_rounded(&rects.panel1, 0.0, &c.theme().layer_background);
        c.fill_rounded(&rects.panel2, 0.0, &c.theme().layer_background);
        // …then each panel paints itself: its own `BackColor`, its `BorderStyle`
        // and its children are `Panel`'s business, not the container's.
        if !self.panel1_collapsed {
            self.panel1.paint(c, rects.panel1);
        }
        if !self.panel2_collapsed {
            self.panel2.paint(c, rects.panel2);
        }
        // The splitter bar reads as a subtle divider band.
        if !self.panel1_collapsed && !self.panel2_collapsed {
            c.fill_rounded(&rects.splitter, 0.0, &c.theme().divider);
        }
        if self.border_style != BorderStyle::None {
            c.stroke_rounded(&bounds, 0.0, &c.theme().card_stroke);
        }
    }
    fn type_name(&self) -> &'static str {
        "SplitContainer"
    }
}

/// `Splitter` — the older, free-standing splitter bar (docks to an edge and
/// resizes the sibling it faces). Derives directly from `Control`, so it owns
/// only its four new properties; its layout-changing defaults (`Dock = Left`,
/// `Anchor = None`) are applied to the inherited `ControlBase`.
#[derive(Clone)]
pub struct Splitter {
    pub base: ControlBase,
    pub border_style: BorderStyle,
    /// Minimum size of the pane on the far side of the splitter (`MinExtra`).
    pub min_extra: f32,
    /// Minimum size of the pane the splitter is docked against (`MinSize`).
    pub min_size: f32,
    /// Live drag position (`SplitPosition`); `-1` until the bar is dragged.
    pub split_position: f32,
}

impl Default for Splitter {
    /// Catalogue: `BorderStyle = None`, `MinExtra = 25`, `MinSize = 25`,
    /// `Dock = Left`, `Anchor = None`.
    fn default() -> Self {
        let mut base = ControlBase::new();
        base.dock = crate::enums::DockStyle::Left;
        base.anchor = crate::enums::AnchorStyles::NONE;
        Self { base, border_style: BorderStyle::None, min_extra: 25.0, min_size: 25.0, split_position: -1.0 }
    }
}

impl Splitter {
    pub fn new() -> Self {
        Self::default()
    }
}

impl std::ops::Deref for Splitter {
    type Target = ControlBase;
    fn deref(&self) -> &ControlBase {
        &self.base
    }
}

impl std::ops::DerefMut for Splitter {
    fn deref_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
}

impl Control for Splitter {
    fn control(&self) -> &ControlBase {
        &self.base
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
    fn preferred_size(&self, _c: &dyn Canvas) -> Size {
        self.control().clamp(self.control().size())
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        c.fill_rounded(&bounds, 0.0, &c.theme().divider);
    }
    fn type_name(&self) -> &'static str {
        "Splitter"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// TabControl, TabPage
// ═════════════════════════════════════════════════════════════════════════════

/// A default tab row thickness when `ItemSize.Height` is left auto and no canvas
/// has measured the font. The toolkit's own default is 20 DIP; a font-derived
/// value replaces this whenever one is available.
const TAB_DEFAULT_ROW: f32 = 20.0;

/// The nominal height of one caption line inside a tab, used when `ItemSize` is
/// auto. `Canvas` measures text *width* only, so a row's height is this plus
/// `Padding.Y` top and bottom — which lands on the toolkit's 20 DIP row at its
/// default `Padding.Y = 3`.
const TAB_CAPTION_LINE: f32 = 14.0;

/// How far the tab strip is inset from the control's edge, and how far the page
/// then starts after the strip.
///
/// Measured, the toolkit uses **2 DIP on each side** of the strip, which is why
/// the page area of a top-aligned control begins at `2 + ItemSize.Height + 2`
/// and not at the strip height itself.
const TAB_STRIP_INSET: f32 = 2.0;

/// The raised frame the page sits inside, on the three edges away from the strip
/// — **4** DIP, not the 2 the strip inset alone would suggest. A 300×200 control
/// with a 20 DIP tab row gives its page `[4, 24, 292, 172]`.
const TAB_PAGE_BORDER: f32 = 2.0 * TAB_STRIP_INSET;

/// The whole strip band: every row, plus the 2 DIP inset above the first and
/// below the last. This is the amount [`tab_display_rect`] takes off the aligned
/// edge — it is NOT a single tab's height, which is `item_height` alone.
pub fn tab_strip_thickness(item_height: f32, rows: u32) -> f32 {
    let one = if item_height > 0.0 { item_height } else { TAB_DEFAULT_ROW };
    one * rows.max(1) as f32 + 2.0 * TAB_STRIP_INSET
}

/// The rectangle a `TabPage` receives — the client area less the strip band on
/// the aligned edge and the raised page frame on the other three.
pub fn tab_display_rect(bounds: Rect, alignment: TabAlignment, strip: f32) -> Rect {
    let b = TAB_PAGE_BORDER;
    match alignment {
        TabAlignment::Top => Rect::new(bounds.left + b, bounds.top + strip, bounds.right - b, bounds.bottom - b),
        TabAlignment::Bottom => Rect::new(bounds.left + b, bounds.top + b, bounds.right - b, bounds.bottom - strip),
        TabAlignment::Left => Rect::new(bounds.left + strip, bounds.top + b, bounds.right - b, bounds.bottom - b),
        TabAlignment::Right => Rect::new(bounds.left + b, bounds.top + b, bounds.right - strip, bounds.bottom - b),
    }
}

/// The `(offset, extent)` of every tab **as if the strip were one long row** —
/// the raw along-axis sizes, before any wrapping.
///
/// `labels` are the measured caption extents; `pad` is `TabControl.Padding` on
/// the along axis. `Fixed` forces `ItemSize`; `Normal` and `FillToRight` fit each
/// tab to its caption.
///
/// The running offsets are only meaningful for a single-row strip. A `Multiline`
/// strip must feed the extents to [`tab_rows`] and then to [`tab_rects`], which
/// restarts the offset on each row — using these offsets directly is what puts
/// the fourth tab at x = 480 on a 300-wide control.
///
/// `FillToRight` deliberately does **not** stretch here: `TCS_RIGHTJUSTIFY`
/// justifies only rows that actually wrapped, so a strip whose tabs all fit on
/// one row is laid out exactly like `Normal`. The justification therefore lives
/// in [`tab_rects`], which is the first place the row count is known.
pub fn tab_strip(
    labels: &[f32],
    pad: f32,
    item_extent: f32,
    mode: TabSizeMode,
    _strip_length: f32,
) -> Vec<(f32, f32)> {
    let widths: Vec<f32> = labels
        .iter()
        .map(|&l| match mode {
            TabSizeMode::Fixed if item_extent > 0.0 => item_extent,
            _ => (l + 2.0 * pad).max(item_extent),
        })
        .collect();

    let mut out = Vec::with_capacity(widths.len());
    let mut x = 0.0;
    for w in widths {
        out.push((x, w));
        x += w;
    }
    out
}

/// The rectangle of every tab, wrapped into rows and placed against `bounds`.
///
/// `extents` are the along-axis sizes from [`tab_strip`], `rows` the row index of
/// each tab from [`tab_rows`], and `row_thickness` one row's across-axis size
/// (`ItemSize.Height`).
///
/// Two behaviours here are measured, not inferred:
///
/// * **Rows are rotated so the selected tab's row sits against the page.** A
///   five-tab, three-row strip with the first tab selected paints rows in the
///   order 1, 2, 0 — the selected row ends up adjacent to the page area so the
///   active tab can join it. This is why tab 0 of such a strip is at the
///   *bottom* of a top-aligned strip rather than the top.
/// * **`FillToRight` justifies only wrapped rows**, growing each tab in a row
///   equally until the row fills the strip.
pub fn tab_rects(
    bounds: Rect,
    alignment: TabAlignment,
    extents: &[f32],
    row_thickness: f32,
    rows: &[u32],
    mode: TabSizeMode,
    selected: i32,
) -> Vec<Rect> {
    if extents.is_empty() {
        return Vec::new();
    }
    let n_rows = rows.iter().copied().max().unwrap_or(0) + 1;
    let vertical = alignment.is_vertical();
    let along = if vertical { bounds.bottom - bounds.top } else { bounds.right - bounds.left };
    let usable = (along - 2.0 * TAB_STRIP_INSET).max(0.0);

    // `FillToRight` only justifies rows that actually wrapped.
    let mut sized: Vec<f32> = extents.to_vec();
    if mode == TabSizeMode::FillToRight && n_rows > 1 {
        for r in 0..n_rows {
            let members: Vec<usize> =
                (0..sized.len()).filter(|&i| rows.get(i).copied().unwrap_or(0) == r).collect();
            let total: f32 = members.iter().map(|&i| sized[i]).sum();
            if total < usable && !members.is_empty() {
                let extra = (usable - total) / members.len() as f32;
                for &i in &members {
                    sized[i] += extra;
                }
            }
        }
    }

    // The row the selected tab is on becomes the one nearest the page.
    let selected_row = usize::try_from(selected)
        .ok()
        .and_then(|i| rows.get(i).copied())
        .unwrap_or(0);
    let display_row =
        |r: u32| -> u32 { (r + n_rows - selected_row - 1) % n_rows };

    let mut out = Vec::with_capacity(sized.len());
    let mut run = vec![0.0f32; n_rows as usize];
    for (i, &extent) in sized.iter().enumerate() {
        let r = rows.get(i).copied().unwrap_or(0);
        let offset = TAB_STRIP_INSET + run[r as usize];
        run[r as usize] += extent;
        let dr = display_row(r) as f32;

        out.push(match alignment {
            TabAlignment::Top => {
                let y = bounds.top + TAB_STRIP_INSET + dr * row_thickness;
                Rect::new(bounds.left + offset, y, bounds.left + offset + extent, y + row_thickness)
            }
            TabAlignment::Bottom => {
                let y = bounds.bottom - TAB_STRIP_INSET - (dr + 1.0) * row_thickness;
                Rect::new(bounds.left + offset, y, bounds.left + offset + extent, y + row_thickness)
            }
            TabAlignment::Left => {
                let x = bounds.left + TAB_STRIP_INSET + dr * row_thickness;
                Rect::new(x, bounds.top + offset, x + row_thickness, bounds.top + offset + extent)
            }
            TabAlignment::Right => {
                let x = bounds.right - TAB_STRIP_INSET - (dr + 1.0) * row_thickness;
                Rect::new(x, bounds.top + offset, x + row_thickness, bounds.top + offset + extent)
            }
        });
    }
    out
}

/// Row index of each tab once a `Multiline` strip wraps.
///
/// `strip_length` is the control's full extent along the strip; the tabs get it
/// less the 2 DIP inset at each end, which is where they actually start and stop.
/// The largest index it returns, plus one, is the row count that thickens the
/// strip.
pub fn tab_rows(widths: &[f32], strip_length: f32) -> Vec<u32> {
    let usable = (strip_length - 2.0 * TAB_STRIP_INSET).max(0.0);
    let mut rows = Vec::with_capacity(widths.len());
    let mut x = 0.0f32;
    let mut row = 0u32;
    for &w in widths {
        if x > 0.0 && x + w > usable {
            row += 1;
            x = 0.0;
        }
        rows.push(row);
        x += w;
    }
    rows
}

/// `TabPage` — a `Panel` that additionally carries the image and tooltip a tab
/// shows, and whether it paints the visual-style background.
#[derive(Clone)]
pub struct TabPage {
    pub base: Panel,
    pub image_index: i32,
    pub image_key: String,
    pub tool_tip_text: String,
    pub use_visual_style_back_color: bool,
}

impl Default for TabPage {
    /// Catalogue: `ImageIndex = -1`, `ImageKey = ""`, `ToolTipText = ""`,
    /// `UseVisualStyleBackColor = false`.
    fn default() -> Self {
        Self {
            base: Panel::default(),
            image_index: -1,
            image_key: String::new(),
            tool_tip_text: String::new(),
            use_visual_style_back_color: false,
        }
    }
}

impl TabPage {
    pub fn new(text: impl Into<String>) -> Self {
        let mut p = Self::default();
        p.control_mut().text = text.into();
        p
    }
}

impl std::ops::Deref for TabPage {
    type Target = Panel;
    fn deref(&self) -> &Panel {
        &self.base
    }
}

impl std::ops::DerefMut for TabPage {
    fn deref_mut(&mut self) -> &mut Panel {
        &mut self.base
    }
}

impl Control for TabPage {
    fn control(&self) -> &ControlBase {
        &self.base
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        self.base.preferred_size(c)
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        // A tab page is an opaque content surface: it grounds the page area even
        // when no `BackColor` was set, which is what `UseVisualStyleBackColor`
        // asks the toolkit for. `Panel` then adds any explicit `BackColor`, the
        // `BorderStyle` and the children.
        c.fill_rounded(&bounds, 0.0, &c.theme().layer_background);
        self.base.paint(c, bounds);
    }
    fn type_name(&self) -> &'static str {
        "TabPage"
    }
}

/// `TabControl` — declares the strip's presentation and owns its `TabPage`s.
/// Derives directly from `Control`.
///
/// ## Not modelled
///
/// `ImageList` is absent, and deliberately so: the port has no `ImageList` type.
/// A control here never resolves its own bitmaps — the host resolves them and
/// passes them in already decoded (see the `shell_icon` contract on `Canvas`) —
/// so there is nothing for a tab strip to hold an image *list* in. This is the
/// same reason `TabPage::image_index` and `TabPage::image_key` are stored but
/// never painted: the indices are kept so a host can carry them, and the lookup
/// they imply belongs to the host. Reinstating tab icons means giving the
/// library an image-resolution story first, not adding a field here.
#[derive(Clone)]
pub struct TabControl {
    pub base: ControlBase,
    pub tab_pages: Vec<TabPage>,
    pub alignment: TabAlignment,
    pub appearance: TabAppearance,
    pub draw_mode: TabDrawMode,
    pub size_mode: TabSizeMode,
    pub hot_track: bool,
    pub multiline: bool,
    pub right_to_left_layout: bool,
    pub show_tool_tips: bool,
    /// `ItemSize` — an explicit tab extent; `Size::EMPTY` means auto.
    pub item_size: Size,
    /// `Padding` — a `Point(x, y)` of horizontal/vertical space added to each
    /// tab. Modelled as a `Size` so the two axes stay named.
    pub padding: Size,
    /// `SelectedIndex` — `-1` when the control has no pages selected.
    pub selected_index: i32,
}

impl Default for TabControl {
    /// Catalogue: `Alignment = Top`, `Appearance = Normal`, `DrawMode = Normal`,
    /// `SizeMode = Normal`, `HotTrack = false`, `Multiline = false`,
    /// `RightToLeftLayout = false`, `ShowToolTips = false`, `ItemSize = 0,0`,
    /// `Padding = 6,3`, `SelectedIndex = -1`.
    fn default() -> Self {
        Self {
            base: ControlBase::new(),
            tab_pages: Vec::new(),
            alignment: TabAlignment::default(),
            appearance: TabAppearance::default(),
            draw_mode: TabDrawMode::default(),
            size_mode: TabSizeMode::default(),
            hot_track: false,
            multiline: false,
            right_to_left_layout: false,
            show_tool_tips: false,
            item_size: Size::EMPTY,
            padding: Size::new(6.0, 3.0),
            selected_index: -1,
        }
    }
}

impl TabControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// `TabCount` — a read-only view of the page collection.
    pub fn tab_count(&self) -> usize {
        self.tab_pages.len()
    }

    /// `SelectedTab` — the currently selected page, if any.
    pub fn selected_tab(&self) -> Option<&TabPage> {
        usize::try_from(self.selected_index).ok().and_then(|i| self.tab_pages.get(i))
    }

    /// Adds a page and selects the first one, mirroring `TabPages.Add` (the
    /// control selects index 0 as soon as it has a page).
    pub fn add_page(&mut self, page: TabPage) {
        self.tab_pages.push(page);
        if self.selected_index < 0 {
            self.selected_index = 0;
        }
    }

    /// The control's client box, in **client coordinates** — the padding inset
    /// carried in the origin, matching what every container in the library now
    /// reports. `TabControl` derives straight from `Control`, so it has no
    /// `ScrollableControl` to inherit this from and computes it here.
    pub fn client_box(&self) -> Rect {
        let s = self.control().size();
        let p = self.control().padding;
        Rect::new(
            p.left,
            p.top,
            (s.width - p.right).max(p.left),
            (s.height - p.bottom).max(p.top),
        )
    }

    /// `DisplayRectangle` — the rectangle the selected page is given, in the tab
    /// control's client space. That is the space a `TabPage`'s parent-relative
    /// `bounds` live in; painting translates it.
    pub fn display_rect(&self, c: &dyn Canvas) -> Rect {
        tab_display_rect(self.client_box(), self.alignment, self.strip_thickness(c))
    }

    /// The control's extent along the strip — its width for a top/bottom strip,
    /// its height for one running down a side.
    fn along_extent(&self) -> f32 {
        let b = self.client_box();
        if self.alignment.is_vertical() { b.bottom - b.top } else { b.right - b.left }
    }

    /// The along-axis extent of every tab, before wrapping.
    fn tab_extents(&self, c: &dyn Canvas) -> Vec<f32> {
        let labels = self.measure_labels(c);
        tab_strip(
            &labels,
            self.padding.width,
            self.item_size.width,
            self.size_mode,
            self.along_extent(),
        )
        .iter()
        .map(|t| t.1)
        .collect()
    }

    /// The row index of each tab.
    fn tab_row_map(&self, c: &dyn Canvas) -> Vec<u32> {
        let extents = self.tab_extents(c);
        if self.multiline {
            tab_rows(&extents, self.along_extent())
        } else {
            vec![0; extents.len()]
        }
    }

    /// How many rows the strip wrapped onto.
    pub fn row_count(&self, c: &dyn Canvas) -> u32 {
        self.tab_row_map(c).iter().copied().max().map(|r| r + 1).unwrap_or(1)
    }

    /// The thickness of the whole strip band, including the extra rows a
    /// `Multiline` strip wrapped onto and the 2 DIP inset at each end.
    pub fn strip_thickness(&self, c: &dyn Canvas) -> f32 {
        tab_strip_thickness(self.row_thickness(c), self.row_count(c))
    }

    /// The across-strip thickness of ONE tab row.
    ///
    /// This is `ItemSize.Height` for **every** alignment. The axes are not
    /// swapped for a side strip: with `ItemSize = 80 × 20` a left-aligned control
    /// makes each tab 20 wide and 80 tall, so `Width` stays the along-axis extent
    /// and `Height` stays the thickness. Reading `Width` as the thickness of a
    /// vertical strip — the natural guess — comes out transposed.
    pub fn row_thickness(&self, _c: &dyn Canvas) -> f32 {
        if self.item_size.height > 0.0 {
            return self.item_size.height;
        }
        // `Normal` mode: one caption line plus `Padding.Y` above and below. The
        // line is a NOMINAL constant because `Canvas` exposes a text *width*
        // metric and no height — with the toolkit's default `Padding.Y = 3` this
        // reproduces its 20 DIP row exactly. A real font height would be better;
        // it needs a `Canvas` method that does not exist yet.
        TAB_CAPTION_LINE + 2.0 * self.padding.height
    }

    /// `ItemSize` as the toolkit **reports** it, not as it was set.
    ///
    /// In `Fixed` mode that is the value assigned. In `Normal` the property
    /// resolves to a real measurement once the control has tabs — returning the
    /// unset `0 × 0` is simply wrong. The width it reports is the FIRST tab's,
    /// which is what the toolkit does for a variable-width strip; the exact
    /// number is font-derived and will differ from GDI+'s by a DIP or two.
    pub fn resolved_item_size(&self, c: &dyn Canvas) -> Size {
        if self.item_size.width > 0.0 && self.item_size.height > 0.0 {
            return self.item_size;
        }
        let extents = self.tab_extents(c);
        Size::new(extents.first().copied().unwrap_or(0.0), self.row_thickness(c))
    }

    /// Every tab's rectangle, in the control's client space — wrapped into rows,
    /// with the selected tab's row against the page.
    pub fn tab_rectangles(&self, c: &dyn Canvas) -> Vec<Rect> {
        tab_rects(
            self.client_box(),
            self.alignment,
            &self.tab_extents(c),
            self.row_thickness(c),
            &self.tab_row_map(c),
            self.size_mode,
            self.selected_index,
        )
    }

    /// Gives the selected page the display rectangle, and lets it lay its own
    /// children out inside it.
    pub fn perform_layout(&mut self, c: &dyn Canvas) {
        let page = self.display_rect(c);
        if let Ok(i) = usize::try_from(self.selected_index) {
            if let Some(p) = self.tab_pages.get_mut(i) {
                p.control_mut().bounds = page;
                p.base.perform_layout();
            }
        }
    }

    /// The along-strip label extent of each page's caption.
    fn measure_labels(&self, c: &dyn Canvas) -> Vec<f32> {
        let fmt = format_for(c, self.control().font.unwrap_or(FontRole::Body));
        self.tab_pages.iter().map(|p| c.measure(&p.control().text, fmt)).collect()
    }

}

impl std::ops::Deref for TabControl {
    type Target = ControlBase;
    fn deref(&self) -> &ControlBase {
        &self.base
    }
}

impl std::ops::DerefMut for TabControl {
    fn deref_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
}

impl Control for TabControl {
    fn control(&self) -> &ControlBase {
        &self.base
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
    fn preferred_size(&self, _c: &dyn Canvas) -> Size {
        self.control().clamp(self.control().size())
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        // The page frame: the client box less the strip, bordered like a card.
        // Measured the same way `display_rect` does, so what is painted and what
        // the page was given cannot disagree.
        let strip = self.strip_thickness(c);
        let page = tab_display_rect(bounds, self.alignment, strip);
        c.fill_rounded(&page, 0.0, &c.theme().layer_background);
        c.stroke_rounded(&page, 0.0, &c.theme().tab_border);
        // The selected page fills that frame and paints its own children.
        if let Some(p) = self.selected_tab() {
            p.paint(c, page);
        }

        // The tabs themselves. All four alignments are placed by `tab_rects`,
        // which wraps them into rows and rotates those rows so the selected
        // tab's row meets the page — the same geometry `tab_rectangles` reports,
        // so what is painted is what a hit-test would find.
        let fmt = format_for(c, self.control().font.unwrap_or(FontRole::Body));
        let fore = self.control().resolved_fore(c);
        let origin = tab_rects(
            bounds,
            self.alignment,
            &self.tab_extents(c),
            self.row_thickness(c),
            &self.tab_row_map(c),
            self.size_mode,
            self.selected_index,
        );
        for (i, r) in origin.iter().enumerate() {
            let active = i as i32 == self.selected_index;
            let bg = if active { c.theme().tab_active_background } else { c.theme().tab_hover_background };
            c.fill_top_rounded(r, 4.0, &bg);
            if let Some(p) = self.tab_pages.get(i) {
                c.text(&p.control().text, r, fmt, &fore, true);
            }
        }
    }
    fn type_name(&self) -> &'static str {
        "TabControl"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests — the arithmetic, without a canvas
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{AnchorStyles, DockStyle};

    const D: Rect = Rect { left: 0.0, top: 0.0, right: 100.0, bottom: 100.0 };

    fn child(w: f32, h: f32) -> FlowChild {
        FlowChild { size: Size::new(w, h), margin: Padding::ZERO, flow_break: false }
    }

    // ── Defaults ─────────────────────────────────────────────────────────

    #[test]
    fn defaults_match_the_catalogue() {
        let f = FlowLayoutPanel::default();
        assert_eq!(f.flow_direction, FlowDirection::LeftToRight);
        assert!(f.wrap_contents);

        let t = TableLayoutPanel::default();
        assert_eq!((t.column_count, t.row_count), (0, 0));
        assert_eq!(t.cell_border_style, TableLayoutPanelCellBorderStyle::None);
        assert_eq!(t.grow_style, TableLayoutPanelGrowStyle::AddRows);

        let s = SplitContainer::default();
        assert_eq!(s.orientation, Orientation::Vertical);
        assert_eq!(s.fixed_panel, FixedPanel::None);
        assert!(!s.is_splitter_fixed);
        assert_eq!(s.splitter_distance, 50.0);
        assert_eq!(s.splitter_width, 4.0);
        assert_eq!((s.panel1_min_size, s.panel2_min_size), (25.0, 25.0));
        assert!(!s.panel1_collapsed && !s.panel2_collapsed);

        let sp = Splitter::default();
        assert_eq!(sp.control().dock, DockStyle::Left, "Splitter defaults to Dock=Left");
        assert_eq!(sp.control().anchor, AnchorStyles::NONE, "Splitter defaults to Anchor=None");
        assert_eq!((sp.min_size, sp.min_extra), (25.0, 25.0));

        let tc = TabControl::default();
        assert_eq!(tc.alignment, TabAlignment::Top);
        assert_eq!(tc.appearance, TabAppearance::Normal);
        assert_eq!(tc.size_mode, TabSizeMode::Normal);
        assert!(!tc.multiline);
        assert_eq!(tc.selected_index, -1);
        assert_eq!(tc.padding, Size::new(6.0, 3.0));
        assert!(tc.item_size.is_empty());

        let tp = TabPage::default();
        assert_eq!(tp.image_index, -1);
        assert!(!tp.use_visual_style_back_color);
    }

    // ── Flow wrapping arithmetic ─────────────────────────────────────────

    #[test]
    fn flow_wraps_when_a_row_is_full_and_stacks_by_row_height() {
        // Three 40-wide cells in a 100-wide area: two fit on row 0, the third
        // wraps to row 1 under the tallest cell of row 0 (height 30).
        let cs = [child(40.0, 20.0), child(40.0, 30.0), child(40.0, 20.0)];
        let out = flow_layout(D, FlowDirection::LeftToRight, true, &cs);
        assert_eq!((out[0].left, out[0].top), (0.0, 0.0));
        assert_eq!((out[1].left, out[1].top), (40.0, 0.0));
        assert_eq!((out[2].left, out[2].top), (0.0, 30.0), "wrapped under the row's tallest");
    }

    #[test]
    fn flow_without_wrap_overflows_in_one_line() {
        let cs = [child(40.0, 20.0), child(40.0, 20.0), child(40.0, 20.0)];
        let out = flow_layout(D, FlowDirection::LeftToRight, false, &cs);
        assert_eq!(out[2].left, 80.0, "no wrap: third cell runs past the edge");
    }

    #[test]
    fn a_flow_break_forces_a_new_line_even_with_room() {
        let mut cs = [child(20.0, 20.0), child(20.0, 20.0)];
        cs[0].flow_break = true;
        let out = flow_layout(D, FlowDirection::LeftToRight, true, &cs);
        assert_eq!((out[1].left, out[1].top), (0.0, 20.0), "break moved the second cell down");
    }

    #[test]
    fn right_to_left_flow_places_from_the_right_edge() {
        let cs = [child(30.0, 20.0)];
        let out = flow_layout(D, FlowDirection::RightToLeft, true, &cs);
        assert_eq!((out[0].left, out[0].right), (70.0, 100.0));
    }

    #[test]
    fn top_down_flow_wraps_into_a_new_column() {
        // Two 60-tall cells in a 100-tall area: the second wraps to column 1.
        let cs = [child(20.0, 60.0), child(20.0, 60.0)];
        let out = flow_layout(D, FlowDirection::TopDown, true, &cs);
        assert_eq!((out[0].left, out[0].top), (0.0, 0.0));
        assert_eq!((out[1].left, out[1].top), (20.0, 0.0), "second cell starts the next column");
    }

    #[test]
    fn bottom_up_flow_places_from_the_bottom_edge() {
        let cs = [child(20.0, 30.0)];
        let out = flow_layout(D, FlowDirection::BottomUp, true, &cs);
        assert_eq!((out[0].top, out[0].bottom), (70.0, 100.0));
    }

    // ── Flow wrapping: the boundary ──────────────────────────────────────
    //
    // The comfortable cases above leave the interesting question open: what
    // happens at exactly the line's edge? These pin it, because an off-by-one
    // here is invisible until one item lands a single DIP over.

    /// A cell whose trailing edge lands EXACTLY on the line's end still fits.
    /// This is why the test is `>` and not `>=`: `>=` would push a
    /// perfectly-fitting item onto the next row and leave a ragged gap.
    #[test]
    fn an_item_that_exactly_fills_the_line_does_not_wrap() {
        // 60 + 40 = 100, precisely the display width.
        let cs = [child(60.0, 20.0), child(40.0, 20.0)];
        let out = flow_layout(D, FlowDirection::LeftToRight, true, &cs);
        assert_eq!(out[1].top, 0.0, "an exact fit stays on the first row");
        assert_eq!((out[1].left, out[1].right), (60.0, 100.0));
    }

    /// One DIP more than fits must wrap — and must still be PLACED. A wrap that
    /// dropped the item would show up as a control vanishing from the page.
    #[test]
    fn an_item_one_dip_too_wide_wraps_and_is_still_placed() {
        let cs = [child(60.0, 20.0), child(41.0, 20.0)];
        let out = flow_layout(D, FlowDirection::LeftToRight, true, &cs);
        assert_eq!(out.len(), 2, "nothing is ever dropped");
        assert_eq!((out[1].left, out[1].top), (0.0, 20.0), "wrapped to the second row");
    }

    /// The candidate's own MARGIN counts toward the break: a cell that fits by
    /// its size alone but not once its margin is added must wrap. Testing the
    /// bare size here would overrun the line by exactly the margin.
    #[test]
    fn the_break_counts_the_candidates_margin() {
        let mut second = child(40.0, 20.0);
        second.margin = Padding::new(1.0, 0.0, 0.0, 0.0);
        // 60 + (1 + 40) = 101 > 100, so it must not stay on row 1.
        let cs = [child(60.0, 20.0), second];
        let out = flow_layout(D, FlowDirection::LeftToRight, true, &cs);
        assert_eq!(out[1].top, 0.0 + 20.0, "the margin pushed it over the edge");
    }

    /// An item wider than the whole line cannot fit anywhere, so it takes a row
    /// of its own and overflows it — rather than disappearing or stacking.
    #[test]
    fn an_item_wider_than_the_line_takes_its_own_row() {
        let cs = [child(30.0, 20.0), child(150.0, 20.0), child(30.0, 20.0)];
        let out = flow_layout(D, FlowDirection::LeftToRight, true, &cs);
        assert_eq!(out[0].top, 0.0);
        assert_eq!((out[1].left, out[1].top), (0.0, 20.0), "own row, from the left edge");
        assert_eq!(out[1].right, 150.0, "it overflows rather than being shrunk");
        assert_eq!((out[2].left, out[2].top), (0.0, 40.0), "the next item starts a third row");
    }

    /// Whatever the sizes, the flow returns exactly one rectangle per child, in
    /// order. A control that "vanishes" from a page therefore cannot have been
    /// dropped here.
    #[test]
    fn every_child_gets_exactly_one_rectangle() {
        let widths = [10.0, 250.0, 30.0, 99.0, 100.0, 101.0, 1.0];
        let cs: Vec<FlowChild> = widths.iter().map(|&w| child(w, 20.0)).collect();
        let out = flow_layout(D, FlowDirection::LeftToRight, true, &cs);
        assert_eq!(out.len(), cs.len());
        // Rows never go backwards, and no cell starts left of the display.
        let mut last_top = 0.0f32;
        for r in &out {
            assert!(r.top >= last_top, "lanes advance monotonically");
            assert!(r.left >= D.left, "no cell is placed left of the display");
            last_top = r.top;
        }
    }

    /// The sheet's own packing shape: several groups across a wide line, where
    /// the one that will not fit is the one that must start the next row.
    #[test]
    fn a_row_of_groups_wraps_at_the_first_one_that_does_not_fit() {
        let line = Rect::new(0.0, 0.0, 1500.0, 2000.0);
        let mut cs: Vec<FlowChild> = [420.0, 380.0, 400.0, 500.0, 300.0, 300.0]
            .iter()
            .map(|&w| child(w, 200.0))
            .collect();
        for k in &mut cs {
            k.margin = Padding::all(8.0);
        }
        let out = flow_layout(line, FlowDirection::LeftToRight, true, &cs);
        // 436 + 396 + 416 = 1248 fit; the fourth needs 516 more (1764 > 1500).
        assert_eq!((out[0].top, out[1].top, out[2].top), (8.0, 8.0, 8.0), "three on row 1");
        // Row 1 is 200 tall plus its 8+8 margins, so the next lane starts at
        // 216 and the cell's own top margin puts its box at 224.
        assert_eq!(out[3].top, 224.0, "the fourth starts row 2 — it is not left behind");
        assert_eq!(out[3].left, 8.0, "at the left edge, not off to the right");
        assert_eq!((out[4].top, out[5].top), (224.0, 224.0), "and the rest follow it");
    }

    /// The gallery's containers sheet, to scale: six groups with a 300 DIP
    /// minimum width and an 8 DIP margin — 316 per cell — inside a page inset by
    /// 12 DIP. On a 1500 **physical** pixel window at 150 % scale the line is
    /// 1000 DIP, i.e. 976 usable, so THREE cells fit (948) and the fourth (1264)
    /// cannot. Three-on-row-one is therefore the correct answer, and the fourth
    /// group starts row two at the left edge — it is not left behind on row one.
    #[test]
    fn the_gallery_sheet_wraps_where_the_dip_width_says_it_should() {
        let page = Rect::new(12.0, 12.0, 1000.0 - 12.0, 800.0);
        let mut cs: Vec<FlowChild> = (0..6).map(|_| child(300.0, 150.0)).collect();
        for k in &mut cs {
            k.margin = Padding::all(8.0);
        }
        let out = flow_layout(page, FlowDirection::LeftToRight, true, &cs);
        let rows: Vec<f32> = out.iter().map(|r| r.top).collect();
        assert_eq!(rows[0], rows[1], "groups 1 and 2 share row 1");
        assert_eq!(rows[1], rows[2], "and group 3 too");
        assert!(rows[3] > rows[2], "group 4 must drop to row 2");
        assert_eq!(out[3].left, 20.0, "at the page's left edge, NOT off to the right");
        assert_eq!((rows[4], rows[5]), (rows[3], rows[3]), "groups 5 and 6 follow it on row 2");
    }

    // ── Table track resolution ───────────────────────────────────────────

    #[test]
    fn table_mixes_absolute_percent_and_autosize() {
        // 200 wide: one 50px absolute, one autosize measured at 30, the rest to
        // two percent columns weighted 1:3 → remaining 120 split 30 / 90.
        let styles = [
            TrackStyle::absolute(50.0),
            TrackStyle::auto(),
            TrackStyle::percent(25.0),
            TrackStyle::percent(75.0),
        ];
        let content = [0.0, 30.0, 0.0, 0.0];
        let w = resolve_tracks(200.0, &styles, &content);
        assert_eq!(w[0], 50.0);
        assert_eq!(w[1], 30.0);
        assert_eq!(w[2], 30.0);
        assert_eq!(w[3], 90.0);
        assert_eq!(w.iter().sum::<f32>(), 200.0, "tracks fill the width exactly");
    }

    #[test]
    fn percents_need_not_sum_to_100_and_still_fill() {
        // Two equal weights that sum to 60, not 100 — they still split the whole
        // remainder in half.
        let styles = [TrackStyle::percent(30.0), TrackStyle::percent(30.0)];
        let w = resolve_tracks(100.0, &styles, &[0.0, 0.0]);
        assert_eq!(w, vec![50.0, 50.0]);
    }

    /// The whole rounding remainder goes to the **last** percent track, not
    /// spread by largest-remainder. At 302 across three equal columns the
    /// toolkit gives `[100, 100, 102]` — a spread would give `[101, 101, 100]`.
    #[test]
    fn the_percent_remainder_all_lands_on_the_last_percent_track() {
        let styles = [TrackStyle::percent(1.0), TrackStyle::percent(1.0), TrackStyle::percent(1.0)];
        let w = resolve_tracks(302.0, &styles, &[0.0, 0.0, 0.0]);
        assert_eq!(w, vec![100.0, 100.0, 102.0]);
        assert_eq!(w.iter().sum::<f32>(), 302.0, "and the tracks still fill exactly");
    }

    /// With no percent track to soak it up, the space the fixed and auto tracks
    /// left over goes to the LAST track — the table never under-fills.
    #[test]
    fn leftover_space_goes_to_the_last_track() {
        let styles = [TrackStyle::absolute(10.0), TrackStyle::absolute(10.0), TrackStyle::auto()];
        let w = resolve_tracks(300.0, &styles, &[0.0, 0.0, 0.0]);
        assert_eq!(w, vec![10.0, 10.0, 280.0]);

        // Even when every track is Absolute: the last one absorbs the slack.
        let fixed = [TrackStyle::absolute(100.0), TrackStyle::absolute(100.0)];
        assert_eq!(resolve_tracks(500.0, &fixed, &[0.0, 0.0]), vec![100.0, 400.0]);
    }

    /// `GetColumnWidths()` reports each track **with** its leading border line,
    /// which is the toolkit's unit. The drawable cell is one line narrower.
    #[test]
    fn the_accessor_reports_tracks_including_their_border_line() {
        let mut t = TableLayoutPanel::new();
        t.column_count = 2;
        t.column_styles = vec![TrackStyle::percent(1.0), TrackStyle::percent(1.0)];
        t.cell_border_style = TableLayoutPanelCellBorderStyle::Single;
        t.control_mut().set_bounds(Rect::new(0.0, 0.0, 103.0, 50.0));
        // 103 wide, three 1 DIP lines → 100 drawable, shared 50/50; each track
        // then reports 51 because it carries the line before it.
        assert_eq!(t.column_widths(&[0.0, 0.0]), vec![51.0, 51.0]);
    }

    /// `OutsetPartial` paints a partial line but reserves the full 3 DIP — the
    /// one cell-border value that cannot be guessed from its appearance.
    #[test]
    fn outset_partial_reserves_three_dip() {
        use TableLayoutPanelCellBorderStyle as B;
        assert_eq!(B::None.thickness(), 0.0);
        assert_eq!(B::Single.thickness(), 1.0);
        assert_eq!(B::Inset.thickness(), 2.0);
        assert_eq!(B::Outset.thickness(), 2.0);
        assert_eq!(B::InsetDouble.thickness(), 3.0);
        assert_eq!(B::OutsetDouble.thickness(), 3.0);
        assert_eq!(B::OutsetPartial.thickness(), 3.0, "not 1, despite painting light");
    }

    // ── Table placement (GrowStyle, spans, unset cells) ──────────────────

    #[test]
    fn unset_cells_fill_left_to_right_then_wrap_down() {
        let cells = [CellSpec::default(), CellSpec::default(), CellSpec::default()];
        let out = place_cells(2, 0, TableLayoutPanelGrowStyle::AddRows, &cells);
        assert_eq!(out, vec![(0, 0), (1, 0), (0, 1)]);
    }

    /// `AddColumns` does **not** transpose the scan. Measured against the
    /// toolkit, the fill stays left→right then top→bottom under every
    /// `GrowStyle`; `AddColumns` only means the column count is the one free to
    /// grow, so the grid is first widened to hold everything in `RowCount` rows.
    ///
    /// Five children in two rows therefore need three columns and come out
    /// `(0,0) (1,0) (2,0) (0,1) (1,1)` — not the column-major order the style's
    /// name suggests.
    #[test]
    fn add_columns_widens_the_grid_but_still_fills_row_major() {
        let cells: Vec<CellSpec> = (0..5).map(|_| CellSpec::default()).collect();
        let out = place_cells(0, 2, TableLayoutPanelGrowStyle::AddColumns, &cells);
        assert_eq!(out, vec![(0, 0), (1, 0), (2, 0), (0, 1), (1, 1)]);
    }

    #[test]
    fn an_explicit_cell_is_reserved_and_autos_flow_around_it() {
        let explicit = CellSpec { col: 1, row: 0, col_span: 1, row_span: 1 };
        let cells = [CellSpec::default(), explicit, CellSpec::default()];
        let out = place_cells(2, 0, TableLayoutPanelGrowStyle::AddRows, &cells);
        assert_eq!(out[1], (1, 0), "explicit stays put");
        assert_eq!(out[0], (0, 0));
        assert_eq!(out[2], (0, 1), "the reserved (1,0) is skipped");
    }

    /// A child anchored `Top | Left` keeps its OWN size inside the cell; only
    /// `Dock = Fill` (or anchoring both edges) stretches it. The toolkit's
    /// numbers: a 40×20 child with margin `5,4,3,2` in a 150×120 cell comes out
    /// `[5, 4, 40, 20]`, not filling the cell.
    #[test]
    fn an_anchored_child_keeps_its_size_inside_the_cell() {
        let mut cb = ControlBase::new();
        cb.margin = Padding::new(5.0, 4.0, 3.0, 2.0);
        // Cell (0,0) of a 150-wide, 120-tall grid, already deflated by margin.
        let inner = Rect::new(5.0, 4.0, 147.0, 118.0);
        let r = place_in_cell(inner, &cb, Size::new(40.0, 20.0));
        assert_eq!((r.left, r.top), (5.0, 4.0));
        assert_eq!((r.right - r.left, r.bottom - r.top), (40.0, 20.0), "not stretched to the cell");

        // Docked Fill: the whole cell.
        cb.dock = DockStyle::Fill;
        let f = place_in_cell(inner, &cb, Size::new(40.0, 20.0));
        assert_eq!((f.right - f.left, f.bottom - f.top), (142.0, 114.0));

        // Anchored on both horizontal edges: stretched across, natural height.
        cb.dock = DockStyle::None;
        cb.anchor = AnchorStyles::LEFT.union(AnchorStyles::RIGHT).union(AnchorStyles::TOP);
        let s = place_in_cell(inner, &cb, Size::new(40.0, 20.0));
        assert_eq!((s.right - s.left, s.bottom - s.top), (142.0, 20.0));
    }

    /// `FixedSize` never grows the grid, so the child that did not fit must not
    /// invent a row for itself in the reported track list.
    #[test]
    fn a_fixed_size_grid_reports_only_its_declared_tracks() {
        let mut t = TableLayoutPanel::new();
        t.column_count = 2;
        t.row_count = 2;
        t.grow_style = TableLayoutPanelGrowStyle::FixedSize;
        t.column_styles = vec![TrackStyle::absolute(100.0); 2];
        t.row_styles = vec![TrackStyle::absolute(60.0); 2];
        t.control_mut().set_bounds(Rect::new(0.0, 0.0, 200.0, 120.0));
        t.cells = (0..5).map(|_| CellSpec::default()).collect();
        assert_eq!(t.row_heights(&[0.0, 0.0]).len(), 2, "the fifth child adds no row");
    }

    #[test]
    fn a_span_reserves_its_extra_cells() {
        let wide = CellSpec { col: -1, row: -1, col_span: 2, row_span: 1 };
        let cells = [wide, CellSpec::default()];
        let out = place_cells(2, 0, TableLayoutPanelGrowStyle::AddRows, &cells);
        assert_eq!(out[0], (0, 0));
        assert_eq!(out[1], (0, 1), "the 2-wide span filled row 0, pushing the next down");
    }

    // ── Splitter distance clamping and collapse ──────────────────────────

    fn sp(distance: f32, c1: bool, c2: bool) -> SplitParams {
        SplitParams {
            orientation: Orientation::Vertical,
            splitter_distance: distance,
            splitter_width: 4.0,
            panel1_min: 25.0,
            panel2_min: 25.0,
            panel1_collapsed: c1,
            panel2_collapsed: c2,
        }
    }

    #[test]
    fn a_vertical_split_puts_panels_side_by_side() {
        let r = split_layout(Rect::new(0.0, 0.0, 200.0, 100.0), sp(60.0, false, false));
        assert_eq!((r.panel1.left, r.panel1.right), (0.0, 60.0));
        assert_eq!((r.splitter.left, r.splitter.right), (60.0, 64.0));
        assert_eq!((r.panel2.left, r.panel2.right), (64.0, 200.0));
        assert_eq!(r.panel1.bottom, 100.0, "vertical split runs full height");
    }

    #[test]
    fn a_horizontal_split_stacks_the_panels() {
        let mut p = sp(30.0, false, false);
        p.orientation = Orientation::Horizontal;
        let r = split_layout(Rect::new(0.0, 0.0, 200.0, 100.0), p);
        assert_eq!((r.panel1.top, r.panel1.bottom), (0.0, 30.0));
        assert_eq!((r.panel2.top, r.panel2.bottom), (34.0, 100.0));
    }

    #[test]
    fn the_distance_is_clamped_against_both_min_sizes() {
        // Too small → clamped up to Panel1MinSize.
        let r = split_layout(Rect::new(0.0, 0.0, 200.0, 100.0), sp(5.0, false, false));
        assert_eq!(r.panel1.right, 25.0);
        // Too large → clamped so Panel2 keeps its 25 and the 4px splitter fits.
        let r = split_layout(Rect::new(0.0, 0.0, 200.0, 100.0), sp(500.0, false, false));
        assert_eq!(r.panel1.right, 200.0 - 4.0 - 25.0);
    }

    /// Collapsing gives the whole area to the surviving panel — but the hidden
    /// panel and the splitter KEEP the rectangles they had rather than
    /// collapsing to zero. `SplitterRectangle` still answers from that stale
    /// position, which is how expanding again restores the same split.
    #[test]
    fn a_collapsed_panel_yields_everything_but_keeps_its_own_rect() {
        let full = Rect::new(0.0, 0.0, 300.0, 200.0);
        let r1 = split_layout(full, sp(60.0, true, false));
        assert_eq!((r1.panel2.left, r1.panel2.right), (0.0, 300.0), "panel2 takes it all");
        assert_eq!((r1.panel1.left, r1.panel1.right), (0.0, 60.0), "panel1 keeps its rect");
        assert_eq!((r1.splitter.left, r1.splitter.right), (60.0, 64.0), "so does the splitter");

        let r2 = split_layout(full, sp(60.0, false, true));
        assert_eq!((r2.panel1.left, r2.panel1.right), (0.0, 300.0), "panel1 takes it all");
        assert_eq!((r2.panel2.left, r2.panel2.right), (64.0, 300.0), "panel2 keeps its rect");
        assert_eq!((r2.splitter.left, r2.splitter.right), (60.0, 64.0));
    }

    /// `SplitterDistance` is an `Int32`, so a scaled distance truncates.
    #[test]
    fn a_scaled_splitter_distance_truncates_to_a_whole_dip() {
        // 70 × 500/300 = 116.67 → 116, never 117.
        assert_eq!(adjusted_distance(300.0, 500.0, 70.0, FixedPanel::None, 20.0), 116.0);
    }

    /// `None` scales by the WHOLE extents, `new / old`. Discounting the splitter
    /// width first (`(new - w) / (old - w)`) is the more principled-looking rule
    /// and it is not the toolkit's: at 200 → 300 with a 4 DIP splitter the two
    /// give 90 and 90.6, and the gap widens with the splitter.
    #[test]
    fn fixed_panel_decides_who_absorbs_a_resize() {
        // Panel1 fixed: the distance does not move.
        assert_eq!(adjusted_distance(200.0, 300.0, 60.0, FixedPanel::Panel1, 4.0), 60.0);
        // Panel2 fixed: the distance shifts by the whole +100 delta.
        assert_eq!(adjusted_distance(200.0, 300.0, 60.0, FixedPanel::Panel2, 4.0), 160.0);
        // None: 60 × 300/200 = 90, NOT 60 × 296/196 ≈ 90.6.
        assert_eq!(adjusted_distance(200.0, 300.0, 60.0, FixedPanel::None, 4.0), 90.0);
    }

    // ── Tab strip measurement and page display rect ──────────────────────

    #[test]
    fn normal_tabs_fit_their_labels_plus_padding() {
        // Labels 40 and 60, padding 6 per side → 52 and 72.
        let tabs = tab_strip(&[40.0, 60.0], 6.0, 0.0, TabSizeMode::Normal, 300.0);
        assert_eq!(tabs[0], (0.0, 52.0));
        assert_eq!(tabs[1], (52.0, 72.0));
    }

    #[test]
    fn fixed_tabs_all_take_item_size() {
        let tabs = tab_strip(&[40.0, 90.0], 6.0, 80.0, TabSizeMode::Fixed, 300.0);
        assert_eq!(tabs[0].1, 80.0);
        assert_eq!(tabs[1].1, 80.0);
    }

    /// `TCS_RIGHTJUSTIFY` justifies only rows that actually **wrapped**. A strip
    /// whose tabs all fit on one row is laid out exactly like `Normal` — the
    /// natural reading, that every tab stretches to fill the control, is wrong.
    #[test]
    fn fill_to_right_leaves_a_single_row_alone() {
        let extents: Vec<f32> =
            tab_strip(&[40.0, 60.0], 6.0, 0.0, TabSizeMode::FillToRight, 200.0)
                .iter()
                .map(|t| t.1)
                .collect();
        assert_eq!(extents, vec![52.0, 72.0], "sized to their captions, not stretched");

        let bounds = Rect::new(0.0, 0.0, 200.0, 100.0);
        let rects = tab_rects(bounds, TabAlignment::Top, &extents, 20.0, &[0, 0], TabSizeMode::FillToRight, 0);
        assert_eq!(rects[0].right - rects[0].left, 52.0, "one row: untouched");
    }

    /// Two rows, so justification applies: each row grows to the usable strip.
    #[test]
    fn fill_to_right_justifies_rows_that_wrapped() {
        let extents = [100.0, 100.0, 60.0];
        let bounds = Rect::new(0.0, 0.0, 200.0, 100.0);
        let rects =
            tab_rects(bounds, TabAlignment::Top, &extents, 20.0, &[0, 0, 1], TabSizeMode::FillToRight, 0);
        // Row 0 already fills the 196 usable strip to within 4; row 1's lone tab
        // grows from 60 to the full 196.
        assert_eq!(rects[2].right - rects[2].left, 196.0);
    }

    #[test]
    fn a_multiline_strip_wraps_tabs_onto_new_rows() {
        // Three 80-wide tabs on a 200 strip: the tabs get 196 of it (2 DIP inset
        // at each end), so two fit on row 0 and the third wraps.
        let rows = tab_rows(&[80.0, 80.0, 80.0], 200.0);
        assert_eq!(rows, vec![0, 0, 1]);
    }

    /// The page frame is **4** DIP, and the strip band is `2 + ItemSize.Height +
    /// 2` — so a 300×200 control with a 20 DIP tab row gives its page
    /// `[4, 24, 292, 172]`, the toolkit's own numbers.
    #[test]
    fn the_page_display_rect_drops_the_strip_band_and_a_four_dip_frame() {
        let b = Rect::new(0.0, 0.0, 300.0, 200.0);
        let strip = tab_strip_thickness(20.0, 1);
        assert_eq!(strip, 24.0, "2 + 20 + 2");
        let d = tab_display_rect(b, TabAlignment::Top, strip);
        assert_eq!((d.left, d.top), (4.0, 24.0));
        assert_eq!((d.right - d.left, d.bottom - d.top), (292.0, 172.0));
    }

    #[test]
    fn bottom_alignment_keeps_the_strip_at_the_foot() {
        let b = Rect::new(0.0, 0.0, 300.0, 200.0);
        let d = tab_display_rect(b, TabAlignment::Bottom, 24.0);
        assert_eq!((d.left, d.top), (4.0, 4.0));
        assert_eq!((d.right - d.left, d.bottom - d.top), (292.0, 172.0));
    }

    /// A side strip does **not** swap `ItemSize`'s axes: with `80 × 20` each tab
    /// is 20 wide and 80 tall, so `Width` stays the along-axis extent.
    #[test]
    fn a_side_strip_keeps_item_size_axes() {
        let b = Rect::new(0.0, 0.0, 300.0, 200.0);
        // Three 80-tall tabs down a 200-tall control: two per row, so two rows.
        let rows = tab_rows(&[80.0, 80.0, 80.0], 200.0);
        assert_eq!(rows, vec![0, 0, 1]);
        let rects =
            tab_rects(b, TabAlignment::Left, &[80.0, 80.0, 80.0], 20.0, &rows, TabSizeMode::Fixed, 0);
        assert_eq!((rects[0].right - rects[0].left, rects[0].bottom - rects[0].top), (20.0, 80.0));
        // Two rows of 20 → the page starts at 2 + 40 + 2.
        let strip = tab_strip_thickness(20.0, 2);
        assert_eq!(strip, 44.0);
        let d = tab_display_rect(b, TabAlignment::Left, strip);
        assert_eq!((d.left, d.top), (44.0, 4.0));
    }

    /// Rows are rotated so the SELECTED tab's row sits against the page. Five
    /// 120-wide tabs on a 300-wide control wrap 2/2/1; with tab 0 selected the
    /// toolkit paints rows in the order 1, 2, 0 — putting tab 0 at the BOTTOM of
    /// a top-aligned strip, next to the page it belongs to.
    #[test]
    fn the_selected_tabs_row_is_moved_against_the_page() {
        let b = Rect::new(0.0, 0.0, 300.0, 200.0);
        let extents = [120.0; 5];
        let rows = tab_rows(&extents, 300.0);
        assert_eq!(rows, vec![0, 0, 1, 1, 2], "two per row on the 296 usable");
        let r = tab_rects(b, TabAlignment::Top, &extents, 20.0, &rows, TabSizeMode::Fixed, 0);
        assert_eq!((r[0].left, r[0].top), (2.0, 42.0), "selected row last");
        assert_eq!((r[1].left, r[1].top), (122.0, 42.0));
        assert_eq!((r[2].left, r[2].top), (2.0, 2.0), "row 1 painted first");
        assert_eq!((r[3].left, r[3].top), (122.0, 2.0));
        assert_eq!((r[4].left, r[4].top), (2.0, 22.0), "row 2 in the middle");
    }

    // ── Preferred size measures content, not the current box ─────────────

    /// The reference case: six 40-wide buttons in a 260-wide panel. Measured
    /// unconstrained they belong on ONE row (240 + margins), which is what an
    /// auto-sizing FlowLayoutPanel grows to — the panel must not report its own
    /// current size, which would make it a fixpoint at whatever it already is.
    #[test]
    fn flow_content_is_measured_unconstrained_so_it_does_not_wrap() {
        let cs: Vec<FlowChild> = (0..6).map(|_| child(40.0, 20.0)).collect();
        let s = flow_content_size(FlowDirection::LeftToRight, true, &cs);
        assert_eq!(s, Size::new(240.0, 20.0), "one row, no wrap at measure time");
    }

    #[test]
    fn flow_content_includes_both_margins() {
        let mut c0 = child(40.0, 20.0);
        c0.margin = Padding::all(3.0);
        let s = flow_content_size(FlowDirection::LeftToRight, true, &[c0]);
        assert_eq!(s, Size::new(46.0, 26.0), "3 on each side of a 40x20 cell");
    }

    #[test]
    fn a_flow_break_still_breaks_when_measuring() {
        let mut cs = [child(40.0, 20.0), child(40.0, 20.0)];
        cs[0].flow_break = true;
        let s = flow_content_size(FlowDirection::LeftToRight, true, &cs);
        assert_eq!(s, Size::new(40.0, 40.0), "the break forces two rows even unconstrained");
    }

    /// `FlowBreak` is subordinate to `WrapContents`: measured against the
    /// toolkit, a non-wrapping panel ignores breaks entirely and runs everything
    /// onto one line. A break reads like an explicit instruction, which is what
    /// makes this worth pinning.
    #[test]
    fn a_flow_break_is_inert_when_wrap_contents_is_off() {
        let mut cs = [child(40.0, 20.0), child(40.0, 20.0)];
        cs[0].flow_break = true;
        let out = flow_layout(D, FlowDirection::LeftToRight, false, &cs);
        assert_eq!(out[1].top, 0.0, "no wrap means no second line, break or not");
        assert_eq!(out[1].left, 40.0, "the second cell continues the row");

        // And the same when measuring, which shares the rule.
        let s = flow_content_size(FlowDirection::LeftToRight, false, &cs);
        assert_eq!(s, Size::new(80.0, 20.0), "one line, both cells");
    }

    /// A mirrored flow is the same layout reflected, so it measures the same.
    #[test]
    fn mirrored_directions_measure_the_same_extent() {
        let cs = [child(40.0, 20.0), child(30.0, 25.0)];
        assert_eq!(
            flow_content_size(FlowDirection::LeftToRight, true, &cs),
            flow_content_size(FlowDirection::RightToLeft, true, &cs)
        );
        assert_eq!(
            flow_content_size(FlowDirection::TopDown, true, &cs),
            flow_content_size(FlowDirection::BottomUp, true, &cs)
        );
    }

    // ── Cell rectangles ──────────────────────────────────────────────────

    /// Tracks arrive in the accessor's unit — each carrying its leading border
    /// line — so a cell starts one line into its track and gives that line back
    /// out of its own width. The real toolkit numbers: three 100-wide reported
    /// columns with `Single` borders put cell 1 at x = 101 and make it 99 wide.
    #[test]
    fn a_cell_rect_sits_one_border_into_its_track() {
        let origin = Rect::new(0.0, 0.0, 301.0, 98.0);
        let r = cell_rect_in(
            origin,
            &[100.0, 100.0, 100.0],
            &[97.0],
            1.0,
            CellSpec { col: 1, row: 0, col_span: 1, row_span: 1 },
        );
        assert_eq!((r.left, r.top), (101.0, 1.0));
        assert_eq!((r.right - r.left, r.bottom - r.top), (99.0, 96.0));
    }

    #[test]
    fn a_span_swallows_the_lines_it_crosses() {
        let origin = Rect::new(0.0, 0.0, 301.0, 98.0);
        let r = cell_rect_in(
            origin,
            &[100.0, 100.0, 100.0],
            &[97.0],
            1.0,
            CellSpec { col: 0, row: 0, col_span: 2, row_span: 1 },
        );
        // Two reported columns (200) less the one line the cell itself starts
        // after — the line *between* them is swallowed.
        assert_eq!(r.right - r.left, 199.0);
    }

    #[test]
    fn with_no_cell_border_the_grid_is_flush() {
        let origin = Rect::new(10.0, 5.0, 210.0, 105.0);
        let r = cell_rect_in(
            origin,
            &[50.0, 50.0],
            &[20.0],
            0.0,
            CellSpec { col: 1, row: 0, col_span: 1, row_span: 1 },
        );
        assert_eq!((r.left, r.top), (60.0, 5.0));
    }

    /// The cell rects must tile the same width the tracks were resolved against,
    /// borders included — otherwise the grid and its lines disagree.
    #[test]
    fn cells_tile_the_width_the_tracks_were_resolved_for() {
        let mut t = TableLayoutPanel::new();
        t.column_count = 3;
        t.column_styles = vec![TrackStyle::percent(1.0); 3];
        t.row_count = 1;
        t.row_styles = vec![TrackStyle::absolute(20.0)];
        t.cell_border_style = TableLayoutPanelCellBorderStyle::Single;
        t.control_mut().set_bounds(Rect::new(0.0, 0.0, 104.0, 40.0));

        let widths = t.column_widths(&[0.0, 0.0, 0.0]);
        let heights = t.row_heights(&[0.0]);
        let origin = t.base.local_display_rect();
        let last = cell_rect_in(
            origin,
            &widths,
            &heights,
            1.0,
            CellSpec { col: 2, row: 0, col_span: 1, row_span: 1 },
        );
        // 104 wide less four 1px lines = 100 shared three ways; the last cell
        // must end exactly one line short of the client edge.
        assert_eq!(last.right, origin.right - 1.0);
    }

    // ── Coordinate space: bounds are PARENT-RELATIVE ─────────────────────

    /// The convention that stops a container from having to shift its
    /// descendants by hand: a child's `bounds` are relative to its parent's
    /// CLIENT area, so moving the parent moves the child for free. The layout
    /// runs in client coordinates and canvas coordinates appear only at paint
    /// time.
    #[test]
    fn a_moved_panel_keeps_its_children_parent_relative() {
        let mut f = FlowLayoutPanel::new();
        f.control_mut().set_bounds(Rect::new(0.0, 0.0, 100.0, 60.0));
        let at_origin = f.arrange(&[child(30.0, 20.0), child(30.0, 20.0)]);

        // Move the panel far away; its children's rectangles must not change.
        f.control_mut().set_location(400.0, 250.0);
        let moved = f.arrange(&[child(30.0, 20.0), child(30.0, 20.0)]);

        for (a, b) in at_origin.iter().zip(&moved) {
            assert_eq!((a.left, a.top, a.right, a.bottom), (b.left, b.top, b.right, b.bottom));
        }
        assert_eq!((moved[0].left, moved[0].top), (0.0, 0.0), "no padding, so the client box starts at 0");
        assert_eq!(moved[1].left, 30.0);
    }

    /// With padding, the client box's ORIGIN carries the inset — matching
    /// `DisplayRectangle` — so the first child starts at the padding rather than
    /// at zero. This is the convention the whole library now shares.
    #[test]
    fn the_client_box_origin_carries_the_padding() {
        let mut f = FlowLayoutPanel::new();
        f.control_mut().set_bounds(Rect::new(0.0, 0.0, 200.0, 100.0));
        f.control_mut().padding = Padding::all(10.0);
        let d = f.base.local_display_rect();
        assert_eq!((d.left, d.top, d.right, d.bottom), (10.0, 10.0, 190.0, 90.0));
        let out = f.arrange(&[child(30.0, 20.0)]);
        assert_eq!((out[0].left, out[0].top), (10.0, 10.0), "the child starts at the padding");
    }

    /// The same rule for the split container: local rectangles, and the canvas
    /// translation applied once when painting.
    #[test]
    fn a_split_arranges_locally_and_translates_once() {
        let mut s = SplitContainer::new();
        s.control_mut().set_bounds(Rect::new(500.0, 300.0, 700.0, 400.0));
        let r = s.arrange();
        assert_eq!(r.panel1.left, 0.0, "local, not the container's 500");
        assert_eq!(r.panel1.right, 50.0, "the default SplitterDistance");
        assert_eq!(r.panel2.right, 200.0, "local width, not 700");

        // Painted at an arbitrary canvas box, the same split lands under it.
        let painted = translate(r.panel2, 500.0, 300.0);
        assert_eq!((painted.left, painted.right), (554.0, 700.0));
    }

    /// A tab page is given a rectangle in the tab control's local space, so it
    /// too travels with its parent.
    #[test]
    fn a_tab_page_rect_is_local_to_its_tab_control() {
        let b = Rect::new(600.0, 400.0, 800.0, 550.0);
        // The measurement is the same whether the control sits at the origin or
        // far away: only the size feeds it.
        let local = tab_display_rect(Rect::new(0.0, 0.0, 200.0, 150.0), TabAlignment::Top, 24.0);
        assert_eq!((local.left, local.top), (4.0, 24.0));
        let on_canvas = tab_display_rect(b, TabAlignment::Top, 24.0);
        assert_eq!((on_canvas.left, on_canvas.top), (604.0, 424.0), "same rect, translated");
    }

    /// The client→canvas crossing carries the PADDING (already in the client
    /// rect) but must ADD the border, because client coordinates start inside
    /// it. Getting either half wrong is invisible until a bordered container
    /// paints, which is why the identity below is the real assertion.
    #[test]
    fn the_crossing_adds_the_border_but_not_the_padding() {
        let mut base = ControlBase::new();
        base.set_bounds(Rect::new(0.0, 0.0, 100.0, 50.0));
        base.padding = Padding::all(5.0);
        let inset = 1.0;

        // What `local_display_rect` reports: origin from padding alone, far
        // edges short by 2 × border.
        let local = Rect::new(5.0, 5.0, 100.0 - 5.0 - 2.0, 50.0 - 5.0 - 2.0);
        let bounds = Rect::new(200.0, 100.0, 300.0, 150.0);
        let r = local_to_canvas(local, bounds, inset);

        // The independent check: identical to `client_rect_on_canvas`, i.e.
        // `bounds.left + padding.left + inset` — true under either way of
        // splitting the two insets, so it catches a mistake in either half.
        assert_eq!((r.left, r.top), (206.0, 106.0));
        assert_eq!(r.left, bounds.left + base.padding.left + inset);
        assert_eq!(r.top, bounds.top + base.padding.top + inset);
        // Crossing never resizes.
        assert_eq!(
            (r.right - r.left, r.bottom - r.top),
            (local.right - local.left, local.bottom - local.top)
        );
    }

    /// A borderless container crosses by the paint box's origin alone.
    #[test]
    fn the_crossing_of_a_borderless_container_is_a_plain_translation() {
        let local = Rect::new(10.0, 10.0, 190.0, 90.0);
        let r = local_to_canvas(local, Rect::new(400.0, 250.0, 600.0, 350.0), 0.0);
        assert_eq!((r.left, r.top), (410.0, 260.0));
    }

    // ── Deref reaches the inherited properties ───────────────────────────

    /// Every other family lets a caller write `.dock`; these must too, through
    /// their real base rather than a second copy of the property.
    #[test]
    fn the_panels_deref_to_their_base_properties() {
        let mut f = FlowLayoutPanel::new();
        f.dock = DockStyle::Fill;
        assert_eq!(f.control().dock, DockStyle::Fill);

        let mut t = TableLayoutPanel::new();
        t.border_style = BorderStyle::FixedSingle;
        assert_eq!(t.base.border_style, BorderStyle::FixedSingle, "reaches Panel's own property");

        let mut s = SplitContainer::new();
        s.dock = DockStyle::Fill;
        assert_eq!(s.control().dock, DockStyle::Fill);

        let mut tc = TabControl::new();
        tc.dock = DockStyle::Fill;
        assert_eq!(tc.control().dock, DockStyle::Fill);
        assert_eq!(tc.padding, Size::new(6.0, 3.0), "TabControl's own Point padding still wins");
    }
}
