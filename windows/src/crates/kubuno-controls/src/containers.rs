//! The container family: `ScrollableControl → ContainerControl → Form,
//! UserControl`, plus `Panel` (from `ScrollableControl`) and `GroupBox` (from
//! `Control` directly).
//!
//! ## Why this file exists, and what it is the base of
//!
//! A container is the one thing `ControlBase` cannot be on its own: it *owns
//! children* and *lays them out*. WinForms puts that behaviour on
//! `ScrollableControl` (docking, anchoring, auto-scroll) and refines it up the
//! chain. The port mirrors the chain with composition + `Deref`, exactly as
//! `control.rs` describes, so each level owns only what its .NET counterpart
//! **declares** in the reflection catalogue:
//!
//! ```ignore
//! ScrollableControl { control: ControlBase, +6 }   // AutoScroll & friends
//!   ContainerControl { scrollable, +6 }            // AutoScale, ActiveControl…
//!     Form           { container,  +47 }           // the windowed host
//!     UserControl    { container,  +5  }
//! Panel   { scrollable: ScrollableControl, +5 }     // BorderStyle, AutoSize…
//! GroupBox { control: ControlBase, +6 }             // NOT scrollable
//! ```
//!
//! The counts here are the catalogue's `declaredProperties` for each type. The
//! hierarchy map's headline numbers (`+8`, `+7`, `+55`, `+8`) are larger because
//! they also count read-only/host-only members that reflection attributes to the
//! level; every one of them is represented below, honoured or documented.
//!
//! ## The child-collection model
//!
//! `ControlBase` does not carry a `Controls` collection (it is shared and this
//! file may not edit it), so the collection lives on the first container level
//! that needs it — [`ScrollableControl`] — and on [`GroupBox`], which descends
//! from `Control` directly. Both delegate the actual placement to
//! [`crate::layout::layout`]: this file never re-implements Dock or Anchor.
//!
//! ## Coordinate spaces — the rule that keeps containers movable
//!
//! `ControlBase::bounds` is **parent-relative**, as WinForms defines it. A
//! container therefore lays its children out in its own **local space**: the
//! display rectangle handed to the layout engine has its top-left at the origin,
//! so a child's `bounds` mean « this far inside my parent's client area », never
//! « this spot on the canvas ».
//!
//! Canvas space is entered exactly once, at paint time. [`Control::paint`]
//! receives the box the container occupies, and the container translates its
//! children by that box's origin, plus its padding, its border and the
//! auto-scroll offset. Moving a container therefore moves its whole subtree for
//! free — nothing ever shifts a descendant by hand.
//!
//! The operations that cross the boundary are named for it: `local_display_rect`
//! produces the layout input, while [`Children::paint`] and
//! [`Children::child_at`] take the canvas-space origin (respectively a point the
//! caller has already converted into local space).
//!
//! ## What a windowing host owns, not the library
//!
//! `Form` declares 47 properties, but a control library that paints into a
//! `Canvas` does not own a top-level window. Properties like `Opacity`,
//! `TopMost`, `WindowState`, `Icon`, `ShowInTaskbar` or the MDI surface are
//! stored faithfully (so a host can read them) and documented on the field as
//! *applied by the host*. They are never silently dropped, and never faked.
//!
//! ## How this family paints
//!
//! Like the real toolkit, and only like it. Every colour, every metric and the
//! caption font come from [`crate::system::Visuals`] — `GetSysColor`,
//! `GetSystemMetricsForDpi`, `lfMessageFont` — reached through
//! [`ControlCanvas::visuals`]. Nothing here reads the Kubuno palette or the
//! design system's text formats, and nothing here rounds a corner: a WinForms
//! container is a square box, and its borders are the two-tone one-pixel rings
//! `DrawEdge` paints, not strokes.
//!
//! Concretely:
//!
//! * a container's ground is its own `BackColor`, else `SystemColors.Control`
//!   (`Control.DefaultBackColor`) — the ambient value a real control would
//!   inherit from its parent, which this library models as `back_color = None`;
//! * `BorderStyle::FixedSingle` is one square line in `SystemColors.WindowFrame`
//!   — measured on the reference sheet as `#646464`, the `COLOR_WINDOWFRAME` of
//!   this machine, not a shadow grey;
//! * `BorderStyle::Fixed3D` is [`Border3DStyle::Sunken`] through
//!   [`ControlCanvas::draw_edge`]: two rings, dark outside and darker inside on
//!   the top/left, light on the bottom/right — and it stays that way **with
//!   visual styles on**, which is the measurement [`paint_border`] carries;
//! * a [`GroupBox`] frames itself with the theme's own `BP_GROUPBOX` — a single
//!   flat `#DCDCDC` line on a default Windows 11 — falling back to
//!   [`Border3DStyle::Etched`], the classic engraved groove, when visual styles
//!   are off. Either way it notches its caption into the top of that frame, in
//!   `SystemColors.ControlText` and the system message font.
//!
//! ## Themed and classic, in this family
//!
//! One control, two correct renderings — see [`crate::theme`] for why, and for
//! the rule this migration was done under: **a part is chosen by rendering it
//! and sampling it, never by reading its name.** Both decisions below came out
//! of a measurement, and one of them contradicts the obvious guess:
//!
//! | asked | sampled — on `04-containers.png`, on `DrawToBitmap`, and on the live control | adopted |
//! |---|---|---|
//! | `GroupBox` frame | one flat `#DCDCDC` line, no groove, in every `FlatStyle` | `BUTTON` / `BP_GROUPBOX` |
//! | `Panel` / `UserControl` `Fixed3D` | the classic `#A0A0A0`→`#696969` over `#FFFFFF`→`#E3E3E3` two-ring well | **no theme part** |
//!
//! The second is the one worth reading [`paint_border`] for: a `Panel` and a
//! `TextBox` both call their frame `Fixed3D`, and they are not the same frame.

use drive_app_controls::{Canvas, Rect, TextFormats};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

use crate::control::{Control, ControlBase, ControlCanvas, FontRole};
use crate::enums::{AutoSizeMode, BorderStyle, FlatStyle, Size};
use crate::layout::{self, Item};
use crate::system::{edge_interior, Border3DSide, Border3DStyle};
use crate::theme;

// ─────────────────────────────────────────────────────────────────────────────
// Enumerations declared by this family (kept here, not in the shared `enums.rs`,
// because only the container types use them). Members and defaults come from the
// catalogue, never invented.
// ─────────────────────────────────────────────────────────────────────────────

/// How a `ContainerControl` auto-scales to the DPI/font it was designed at
/// (`AutoScaleMode`). The default is `Inherit`, which takes the parent's mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AutoScaleMode {
    None,
    Font,
    Dpi,
    #[default]
    Inherit,
}

/// Whether a container validates its children when focus leaves them
/// (`AutoValidate`). `Inherit` is the base default; `Form` overrides to
/// `EnablePreventFocusChange`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AutoValidate {
    Disable,
    EnablePreventFocusChange,
    EnableAllowFocusChange,
    #[default]
    Inherit,
}

/// A `Form`'s border and its resize behaviour (`FormBorderStyle`). The default
/// is `Sizable`. Which chrome this produces is the host's business; the value is
/// stored so the host can read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormBorderStyle {
    None,
    FixedSingle,
    Fixed3D,
    FixedDialog,
    #[default]
    Sizable,
    FixedToolWindow,
    SizableToolWindow,
}

/// A `Form`'s minimised/maximised state (`FormWindowState`). Applied by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormWindowState {
    #[default]
    Normal,
    Minimized,
    Maximized,
}

/// Where a `Form` first appears (`FormStartPosition`). Resolved by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormStartPosition {
    Manual,
    CenterScreen,
    #[default]
    WindowsDefaultLocation,
    WindowsDefaultBounds,
    CenterParent,
}

/// Whether a `Form` shows a sizing grip (`SizeGripStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SizeGripStyle {
    #[default]
    Auto,
    Show,
    Hide,
}

/// The result a modal dialog reports (`DialogResult`). `None` until the dialog
/// is closed with a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DialogResult {
    #[default]
    None,
    Ok,
    Cancel,
    Abort,
    Retry,
    Ignore,
    Yes,
    No,
    TryAgain,
    Continue,
}

/// The rounded-corner preference of a `Form` on modern Windows
/// (`FormCornerPreference`). Purely a hint to the host compositor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormCornerPreference {
    #[default]
    Default,
    DoNotRound,
    Round,
    RoundSmall,
}

/// A two-dimensional integer-ish point (`Point`). WinForms carries an integer
/// `Point`; the port stays in `f32` to match `Rect`, which is the only unit the
/// drawing layer speaks. `enums.rs` owns `Size` but no `Point`, so it lives here
/// where `AutoScrollPosition` needs it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ORIGIN: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// One scrollbar's designer-visible state — the port of `HScrollProperties` /
/// `VScrollProperties`. The two toolkit types are identical apart from the axis
/// they drive, so one struct models both; a `ScrollableControl` holds an `h` and
/// a `v`.
///
/// Not yet painted: this wave models the scroll STATE honestly (so a host can
/// drive it and tests can assert the content extent), but does not draw a
/// scrollbar track/thumb. See [`ScrollableControl::horizontal_scroll`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollProperties {
    pub visible:      bool,
    pub enabled:      bool,
    pub value:        f32,
    pub minimum:      f32,
    pub maximum:      f32,
    pub small_change: f32,
    pub large_change: f32,
}

impl Default for ScrollProperties {
    /// The toolkit's defaults: `Maximum = 100`, `LargeChange = 10`,
    /// `SmallChange = 1`, enabled, and hidden until content overflows.
    fn default() -> Self {
        Self {
            visible: false,
            enabled: true,
            value: 0.0,
            minimum: 0.0,
            maximum: 100.0,
            small_change: 1.0,
            large_change: 10.0,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The child collection — the model of `Control.Controls` plus the layout pass.
// ─────────────────────────────────────────────────────────────────────────────

/// A container's children and the state its layout needs.
///
/// This is the `Control.Controls` collection the toolkit gives every control,
/// carried here on the container levels that use it (rather than on the shared
/// `ControlBase`, which this file may not touch). It owns the one piece of
/// layout state that must survive between passes — the previous display rect —
/// because anchoring is defined against the *change* in size, not the size.
// `Clone` works because `Box<dyn Control>` is cloneable through the
// `ControlClone` supertrait — cloning a container deep-copies its children,
// which is what a control value must do.
#[derive(Clone, Default)]
pub struct Children {
    items: Vec<Box<dyn Control>>,
    /// The display rect used by the last [`Self::perform_layout`]. `None` before
    /// the first pass, so that pass legitimately moves nothing (the anchor delta
    /// is zero), matching `layout::layout`'s `previous == display` contract.
    previous: Option<Rect>,
}

impl Children {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a child, last in z-order — and therefore, for docking, the child
    /// placed against the *outermost* container edge (see the reverse-z-order
    /// rule in `layout.rs`).
    pub fn push(&mut self, child: Box<dyn Control>) {
        self.items.push(child);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&dyn Control> {
        self.items.get(i).map(|b| b.as_ref())
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn Control> {
        self.items.iter().map(|b| b.as_ref())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Box<dyn Control>> {
        self.items.iter_mut()
    }

    /// Resolves `Dock` then `Anchor` for every child inside `display`, and
    /// writes each child's new bounds back onto it. Reuses the shared engine —
    /// there is deliberately no docking or anchoring logic in this file.
    ///
    /// `display` must be the container's client rectangle in its **own local
    /// space**: top-left at the origin, size already deflated by padding and any
    /// border. That is what makes the resulting child `bounds` parent-relative,
    /// as `ControlBase::bounds` is documented to be. Passing a canvas-absolute
    /// rectangle compiles and paints correctly *once*, then breaks the moment the
    /// container moves — see the coordinate-space note in the module docs.
    pub fn perform_layout(&mut self, display: Rect) {
        let items: Vec<Item> = self.items.iter().map(|c| Item::from(c.control())).collect();
        let previous = self.previous.unwrap_or(display);
        let rects = layout::layout(display, previous, &items);
        for (child, r) in self.items.iter_mut().zip(rects) {
            child.control_mut().bounds = r;
        }
        self.previous = Some(display);
    }

    /// The furthest right and bottom edge the children reach, in **client**
    /// coordinates — so a container with padding sees an extent that already
    /// includes that padding on the left and top. Hidden children contribute
    /// nothing.
    ///
    /// This is the raw extent, before `AutoScrollMargin`/`AutoScrollMinSize` are
    /// applied — [`ScrollableControl::scroll_content_size`] does that.
    pub fn content_extent(&self) -> Size {
        let mut w = 0.0_f32;
        let mut h = 0.0_f32;
        for child in self.iter() {
            if !child.control().visible {
                continue;
            }
            let b = child.control().bounds;
            w = w.max(b.right);
            h = h.max(b.bottom);
        }
        Size::new(w.max(0.0), h.max(0.0))
    }

    /// Paints every visible child, translating its local `bounds` into canvas
    /// space by `origin` — the canvas position of this collection's local origin,
    /// which the container computes from the box it was asked to paint into (plus
    /// padding, border and the auto-scroll offset). The container is responsible
    /// for clipping.
    pub fn paint(&self, c: &dyn ControlCanvas, origin: Point) {
        for child in self.iter() {
            if !child.control().visible {
                continue;
            }
            let b = child.control().bounds;
            let shifted = Rect::new(b.left + origin.x, b.top + origin.y, b.right + origin.x, b.bottom + origin.y);
            child.paint(c, shifted);
        }
    }

    /// The topmost visible child containing `local` — a point already converted
    /// into this collection's local space, since that is the space the children's
    /// `bounds` live in. Walks in reverse z-order, so the child painted last (and
    /// therefore on top) is the one that answers.
    pub fn child_at(&self, local: Point) -> Option<usize> {
        self.items.iter().enumerate().rev().find_map(|(i, child)| {
            let c = child.control();
            (c.visible && c.bounds.contains(local.x, local.y)).then_some(i)
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ScrollableControl — the base of the container chain.
// ─────────────────────────────────────────────────────────────────────────────

/// `System.Windows.Forms.ScrollableControl`: a `Control` that owns children,
/// lays them out, and can scroll a content area larger than its client rect.
///
/// Declares the six auto-scroll properties; the `children` collection is the
/// model of the inherited `Controls`, added here because a scrollable control is
/// the first level that must place children.
#[derive(Clone)]
pub struct ScrollableControl {
    control: ControlBase,

    // ── The six declared properties ──────────────────────────────────────
    /// `AutoScroll` — when the content overflows, scroll instead of clip.
    pub auto_scroll: bool,
    /// `AutoScrollMargin` — padding added around the content extent when the
    /// scrollable area is computed.
    pub auto_scroll_margin: Size,
    /// `AutoScrollMinSize` — a floor on the virtual content size, so the area
    /// scrolls even when the children are smaller.
    pub auto_scroll_min_size: Size,
    /// `AutoScrollPosition` — the current scroll offset. WinForms *returns* this
    /// as `<= 0` on each axis (the amount the content is shifted up/left); the
    /// port keeps that convention, so painting adds it directly.
    pub auto_scroll_position: Point,
    /// `HorizontalScroll` — the horizontal scrollbar's state. Modelled, not yet
    /// painted (this wave draws no track/thumb); a host can drive and read it.
    pub horizontal_scroll: ScrollProperties,
    /// `VerticalScroll` — the vertical scrollbar's state. Same note as above.
    pub vertical_scroll: ScrollProperties,

    /// The child controls (`Controls`), plus their layout state.
    pub children: Children,
}

impl Default for ScrollableControl {
    fn default() -> Self {
        Self {
            control: ControlBase::new(),
            auto_scroll: false,
            auto_scroll_margin: Size::EMPTY,
            auto_scroll_min_size: Size::EMPTY,
            auto_scroll_position: Point::ORIGIN,
            horizontal_scroll: ScrollProperties::default(),
            vertical_scroll: ScrollProperties::default(),
            children: Children::new(),
        }
    }
}

impl ScrollableControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// The children's `DisplayRectangle`, in client coordinates — the box
    /// deflated by padding, its origin carrying that inset. A `ScrollableControl`
    /// has no border of its own, hence the zero inset.
    pub fn local_display_rect(&self) -> Rect {
        local_display_rect(&self.control, 0.0)
    }

    /// Where this control's client origin lands on the canvas, given the box
    /// `paint` was asked to draw into. Add a child's `bounds` to it to get that
    /// child's on-screen rectangle.
    pub fn content_origin(&self, bounds: Rect) -> Point {
        content_origin(bounds, 0.0, self.auto_scroll_position)
    }

    /// Lays out the children inside [`Self::local_display_rect`], so their bounds
    /// come out parent-relative.
    pub fn perform_layout(&mut self) {
        let display = self.local_display_rect();
        self.children.perform_layout(display);
    }

    /// The virtual content size measured from `display`'s origin, grown by
    /// `AutoScrollMargin` and floored by `AutoScrollMinSize`.
    ///
    /// Takes the display rect rather than reading its own, because a bordered
    /// subclass (`Panel`, `UserControl`) has a different one — and subtracting the
    /// wrong origin would fold the border into the scrollable area.
    pub fn scroll_content_size_within(&self, display: Rect) -> Size {
        let raw = self.children.content_extent();
        let w = (raw.width - display.left).max(0.0) + self.auto_scroll_margin.width;
        let h = (raw.height - display.top).max(0.0) + self.auto_scroll_margin.height;
        Size::new(w.max(self.auto_scroll_min_size.width), h.max(self.auto_scroll_min_size.height))
    }

    /// The virtual content size: the children's extent, grown by
    /// `AutoScrollMargin`, then floored by `AutoScrollMinSize` — exactly the
    /// rectangle the scrollbars would range over.
    pub fn scroll_content_size(&self) -> Size {
        self.scroll_content_size_within(self.local_display_rect())
    }
}

impl std::ops::Deref for ScrollableControl {
    type Target = ControlBase;
    fn deref(&self) -> &ControlBase {
        &self.control
    }
}

impl std::ops::DerefMut for ScrollableControl {
    fn deref_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }
}

impl Control for ScrollableControl {
    fn control(&self) -> &ControlBase {
        &self.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }

    /// The size that just contains the children plus the padding — the base
    /// container measurement the concrete types build on.
    fn preferred_size(&self, _c: &dyn Canvas) -> Size {
        let content = self.scroll_content_size();
        Size::new(
            content.width + self.control.padding.horizontal(),
            content.height + self.control.padding.vertical(),
        )
    }

    /// Fills the background, then paints the children — translated from local
    /// space into `bounds` and clipped to the client rectangle.
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        c.fill_rect(&bounds, &background(&self.control, c));
        let clip = client_rect_on_canvas(bounds, &self.control, 0.0);
        c.push_clip(&clip);
        self.children.paint(c, self.content_origin(bounds));
        c.pop_clip();
    }

    fn type_name(&self) -> &'static str {
        "ScrollableControl"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ContainerControl — adds focus management and DPI auto-scaling.
// ─────────────────────────────────────────────────────────────────────────────

/// `System.Windows.Forms.ContainerControl`: a `ScrollableControl` that manages a
/// focused child and can auto-scale to the DPI/font it was designed at.
#[derive(Clone)]
pub struct ContainerControl {
    scrollable: ScrollableControl,

    /// `ActiveControl` — the focused child. WinForms holds a `Control`
    /// reference; the port holds the child's index in [`Children`], since that
    /// is what can be resolved without a boxed identity. `None` means no active
    /// child.
    pub active_control: Option<usize>,
    /// `AutoScaleDimensions` — the DPI/font the layout was designed at.
    pub auto_scale_dimensions: Size,
    /// `AutoScaleMode` — how to scale from design dimensions to runtime.
    pub auto_scale_mode: AutoScaleMode,
    /// `AutoValidate` — whether leaving a child validates it.
    pub auto_validate: AutoValidate,
    /// `CurrentAutoScaleDimensions` — the *current* DPI/font dimensions. A
    /// read-only computed value in the toolkit; here it is stored so a host that
    /// performs the scaling can publish it. Not derived by the library, which
    /// owns no device from which to measure the current font.
    pub current_auto_scale_dimensions: Size,
    /// `ParentForm` — the `Form` this container sits on. A back-reference the
    /// library does not own (there is no parent graph here); always `None`, set
    /// and read by the host. Kept so the property is never silently missing.
    pub parent_form: Option<()>,
}

impl Default for ContainerControl {
    fn default() -> Self {
        Self {
            scrollable: ScrollableControl::new(),
            active_control: None,
            auto_scale_dimensions: Size::EMPTY,
            auto_scale_mode: AutoScaleMode::default(),
            auto_validate: AutoValidate::default(),
            current_auto_scale_dimensions: Size::EMPTY,
            parent_form: None,
        }
    }
}

impl ContainerControl {
    pub fn new() -> Self {
        Self::default()
    }
}

impl std::ops::Deref for ContainerControl {
    type Target = ScrollableControl;
    fn deref(&self) -> &ScrollableControl {
        &self.scrollable
    }
}

impl std::ops::DerefMut for ContainerControl {
    fn deref_mut(&mut self) -> &mut ScrollableControl {
        &mut self.scrollable
    }
}

impl Control for ContainerControl {
    fn control(&self) -> &ControlBase {
        self.scrollable.control()
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        self.scrollable.control_mut()
    }
    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        self.scrollable.preferred_size(c)
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        self.scrollable.paint(c, bounds);
    }
    fn type_name(&self) -> &'static str {
        "ContainerControl"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Form — the windowed host. 47 declared properties.
// ─────────────────────────────────────────────────────────────────────────────

/// `System.Windows.Forms.Form`: a `ContainerControl` that is a top-level window.
///
/// The 47 declared properties fall into four groups:
///
/// 1. **Redeclarations of a base property** (`AutoScroll`, `AutoSize`,
///    `AutoSizeMode`, `AutoValidate`, `BackColor`, `Location`, `MaximumSize`,
///    `MinimumSize`, `Size`, `TabStop`, `Text`). The toolkit re-declares these
///    on `Form` only to re-attribute them (a different browsable default, mostly);
///    the storage is the *same* field, reached through `Deref`. Duplicating them
///    would create two sources of truth, so they are **not** re-added here —
///    `Form::default` sets the base to `Form`'s documented defaults instead.
/// 2. **Content geometry the form itself owns** (`ClientSize`, `DesktopBounds`,
///    `DesktopLocation`).
/// 3. **Dialog / focus behaviour** (`AcceptButton`, `CancelButton`,
///    `DialogResult`, `KeyPreview`, `Modal`, `MainMenuStrip`).
/// 4. **Windowing chrome the host owns, not this library** (`FormBorderStyle`,
///    `WindowState`, `Opacity`, `TopMost`, `Icon`, `ShowInTaskbar`, the MDI
///    surface, …). Each is stored and documented as host-applied; none is faked.
#[derive(Clone)]
pub struct Form {
    container: ContainerControl,

    // ── Group 2 — content geometry ───────────────────────────────────────
    /// `ClientSize` — the drawable area inside the window chrome. The library
    /// owns no chrome, so this equals the control size; kept as a field the host
    /// can override once it knows its border/caption thickness.
    pub client_size: Size,
    /// `DesktopLocation` — the window's top-left in screen space. Host space; the
    /// library never positions a window.
    pub desktop_location: Point,

    // ── Group 3 — dialog / focus behaviour ───────────────────────────────
    /// `AcceptButton` — the default button (Enter). Referenced by child name;
    /// the host resolves the actual `IButtonControl`.
    pub accept_button: Option<String>,
    /// `CancelButton` — the cancel button (Esc). Referenced by child name.
    pub cancel_button: Option<String>,
    /// `DialogResult` — the decision a modal form reports on close.
    pub dialog_result: DialogResult,
    /// `KeyPreview` — whether the form sees key events before its children.
    pub key_preview: bool,
    /// `Modal` — read-only in the toolkit: true only while shown with `ShowDialog`.
    /// Host-owned; there is no message loop here.
    pub modal: bool,
    /// `MainMenuStrip` — the form's primary menu, referenced by name. The menu
    /// itself belongs to the toolstrip family, not this file.
    pub main_menu_strip: Option<String>,

    // ── Group 4 — windowing chrome (host-applied) ────────────────────────
    /// `FormBorderStyle` — border/resize style. Host draws the frame.
    pub form_border_style: FormBorderStyle,
    /// `WindowState` — normal/minimised/maximised. Host applies it.
    pub window_state: FormWindowState,
    /// `StartPosition` — initial placement. Host resolves it.
    pub start_position: FormStartPosition,
    /// `SizeGripStyle` — whether the resize grip shows. Host draws it.
    pub size_grip_style: SizeGripStyle,
    /// `FormCornerPreference` — rounded-corner hint to the compositor.
    pub form_corner_preference: FormCornerPreference,
    /// `Opacity` — whole-window opacity (0.0–1.0). Applied by the host layer.
    pub opacity: f64,
    /// `TopMost` — keep above other windows. Host-applied.
    pub top_most: bool,
    /// `ControlBox` — show the system menu / close box. Host-applied.
    pub control_box: bool,
    /// `HelpButton` — show the `?` caption button. Host-applied.
    pub help_button: bool,
    /// `MaximizeBox` — show the maximise button. Host-applied.
    pub maximize_box: bool,
    /// `MinimizeBox` — show the minimise button. Host-applied.
    pub minimize_box: bool,
    /// `ShowIcon` — show the caption icon. Host-applied.
    pub show_icon: bool,
    /// `ShowInTaskbar` — appear in the taskbar. Host-applied.
    pub show_in_taskbar: bool,
    /// `KeyPreview`'s cousin for RTL mirroring — `RightToLeftLayout`. Host-applied.
    pub right_to_left_layout: bool,
    /// `Icon` — the caption/taskbar icon, referenced by name; the host loads it.
    pub icon: Option<String>,
    /// `TransparencyKey` — the colour painted as transparent. Host compositing.
    pub transparency_key: Option<D2D1_COLOR_F>,
    /// `FormBorderColor` — custom border colour (modern Windows). Host-applied.
    pub form_border_color: Option<D2D1_COLOR_F>,
    /// `FormCaptionBackColor` — custom caption background. Host-applied.
    pub form_caption_back_color: Option<D2D1_COLOR_F>,
    /// `FormCaptionTextColor` — custom caption text colour. Host-applied.
    pub form_caption_text_color: Option<D2D1_COLOR_F>,

    // ── Group 4 (cont.) — MDI surface (host-applied) ─────────────────────
    /// `IsMdiContainer` — hosts MDI children. Host-managed.
    pub is_mdi_container: bool,
    /// `MdiChildrenMinimizedAnchorBottom` — anchor minimised MDI children to the
    /// bottom. Host-managed.
    pub mdi_children_minimized_anchor_bottom: bool,
    /// `MdiParent` — the MDI parent form, referenced by name. Host graph.
    pub mdi_parent: Option<String>,
    /// `MdiChildren` — read-only list of MDI children (by name). Host graph.
    pub mdi_children: Vec<String>,
    /// `Owner` — the owning form, referenced by name. Host graph.
    pub owner: Option<String>,
    /// `OwnedForms` — read-only list of owned forms (by name). Host graph.
    pub owned_forms: Vec<String>,

    // ── Obsolete ─────────────────────────────────────────────────────────
    /// `AutoScale` — obsolete since .NET 2.0 (superseded by `AutoScaleMode`).
    /// Kept because the catalogue still declares it; defaults to `true` as the
    /// toolkit does.
    pub auto_scale: bool,
}

impl Default for Form {
    /// The toolkit's `Form` defaults. Base properties re-declared by `Form` are
    /// set on the base here rather than duplicated (see the struct docs).
    fn default() -> Self {
        let mut container = ContainerControl::new();
        // `Form` overrides its inherited `AutoValidate` default. `BackColor`
        // stays ambient (`None`) and resolves to the theme's window background at
        // paint time, so it is not forced here.
        container.auto_validate = AutoValidate::EnablePreventFocusChange;

        Self {
            container,
            client_size: Size::EMPTY,
            desktop_location: Point::ORIGIN,
            accept_button: None,
            cancel_button: None,
            dialog_result: DialogResult::default(),
            key_preview: false,
            modal: false,
            main_menu_strip: None,
            form_border_style: FormBorderStyle::default(),
            window_state: FormWindowState::default(),
            start_position: FormStartPosition::default(),
            size_grip_style: SizeGripStyle::default(),
            form_corner_preference: FormCornerPreference::default(),
            opacity: 1.0,
            top_most: false,
            control_box: true,
            help_button: false,
            maximize_box: true,
            minimize_box: true,
            show_icon: true,
            show_in_taskbar: true,
            right_to_left_layout: false,
            icon: None,
            transparency_key: None,
            form_border_color: None,
            form_caption_back_color: None,
            form_caption_text_color: None,
            is_mdi_container: false,
            mdi_children_minimized_anchor_bottom: true,
            mdi_parent: None,
            mdi_children: Vec::new(),
            owner: None,
            owned_forms: Vec::new(),
            auto_scale: true,
        }
    }
}

impl Form {
    pub fn new() -> Self {
        Self::default()
    }

    /// `IsMdiChild` — read-only in the toolkit: a form is an MDI child iff it has
    /// an MDI parent. Derived, never stored, so the two can never disagree.
    pub fn is_mdi_child(&self) -> bool {
        self.mdi_parent.is_some()
    }

    /// Lays the form's children out in its own local space.
    pub fn perform_layout(&mut self) {
        self.container.perform_layout();
    }
}

impl std::ops::Deref for Form {
    type Target = ContainerControl;
    fn deref(&self) -> &ContainerControl {
        &self.container
    }
}

impl std::ops::DerefMut for Form {
    fn deref_mut(&mut self) -> &mut ContainerControl {
        &mut self.container
    }
}

impl Control for Form {
    fn control(&self) -> &ControlBase {
        self.container.control()
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        self.container.control_mut()
    }

    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        self.container.preferred_size(c)
    }

    /// Paints the form's client area, then its children. Window chrome (caption,
    /// border, buttons) is the host's, so only the client fill is drawn here —
    /// `SystemColors.Control` unless a `BackColor` was set, which is what
    /// `Form.DefaultBackColor` resolves to and what the reference sheets show
    /// behind every control (`#F0F0F0` on this machine).
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        c.fill_rect(&bounds, &background(self.control(), c));
        let control = self.control();
        let clip = client_rect_on_canvas(bounds, control, 0.0);
        c.push_clip(&clip);
        self.container.children.paint(c, self.container.content_origin(bounds));
        c.pop_clip();
    }

    fn type_name(&self) -> &'static str {
        "Form"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// UserControl — a reusable composite. 5 declared properties.
// ─────────────────────────────────────────────────────────────────────────────

/// `System.Windows.Forms.UserControl`: a `ContainerControl` packaged as a single
/// reusable control. Declares five properties, all re-exposing behaviour with a
/// designer default of its own.
#[derive(Clone)]
pub struct UserControl {
    container: ContainerControl,

    /// `AutoSize` — grow to fit the children. Re-declared from `Control` because
    /// `UserControl` makes it browsable; stored here as the type's own field.
    pub auto_size: bool,
    /// `AutoSizeMode` — grow-only vs grow-and-shrink.
    pub auto_size_mode: AutoSizeMode,
    /// `AutoValidate` — re-declared; forwarded to the container base at
    /// construction so there is one source of truth at paint time.
    pub auto_validate: AutoValidate,
    /// `BorderStyle` — the frame the user control paints. Default `None`.
    pub border_style: BorderStyle,
}

impl Default for UserControl {
    fn default() -> Self {
        Self {
            container: ContainerControl::new(),
            auto_size: false,
            auto_size_mode: AutoSizeMode::GrowOnly,
            auto_validate: AutoValidate::default(),
            border_style: BorderStyle::None,
        }
    }
}

impl UserControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// The children's `DisplayRectangle`, in client coordinates: the box deflated
    /// by padding and by the border, its origin carrying that inset.
    pub fn local_display_rect(&self) -> Rect {
        local_display_rect(self.control(), border_thickness(self.border_style))
    }

    /// Where this control's client origin lands on the canvas — its position plus
    /// the border, since client coordinates start just inside that border. The
    /// padding is not added: [`Self::local_display_rect`] carries it.
    pub fn content_origin(&self, bounds: Rect) -> Point {
        content_origin(
            bounds,
            border_thickness(self.border_style),
            self.container.auto_scroll_position,
        )
    }

    /// Lays out children inside [`Self::local_display_rect`].
    pub fn perform_layout(&mut self) {
        let display = self.local_display_rect();
        self.container.children.perform_layout(display);
    }
}

impl std::ops::Deref for UserControl {
    type Target = ContainerControl;
    fn deref(&self) -> &ContainerControl {
        &self.container
    }
}

impl std::ops::DerefMut for UserControl {
    fn deref_mut(&mut self) -> &mut ContainerControl {
        &mut self.container
    }
}

impl Control for UserControl {
    fn control(&self) -> &ControlBase {
        self.container.control()
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        self.container.control_mut()
    }

    /// The children's extent measured from this control's own display rect, plus
    /// the padding and the two border lines that surround it.
    fn preferred_size(&self, _c: &dyn Canvas) -> Size {
        let content = self.container.scroll_content_size_within(self.local_display_rect());
        let p = self.control().padding;
        let b = 2.0 * border_thickness(self.border_style);
        Size::new(content.width + p.horizontal() + b, content.height + p.vertical() + b)
    }

    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        c.fill_rect(&bounds, &background(self.control(), c));
        paint_border(c, bounds, self.border_style);
        let control = self.control();
        let inset = border_thickness(self.border_style);
        let clip = client_rect_on_canvas(bounds, control, inset);
        c.push_clip(&clip);
        self.container.children.paint(c, self.content_origin(bounds));
        c.pop_clip();
    }

    fn type_name(&self) -> &'static str {
        "UserControl"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Panel — a scrollable box with a border. 5 declared properties.
// ─────────────────────────────────────────────────────────────────────────────

/// `System.Windows.Forms.Panel`: a `ScrollableControl` with a `BorderStyle`.
/// The workhorse container; the layout-panel family composes it.
#[derive(Clone)]
pub struct Panel {
    scrollable: ScrollableControl,

    /// `AutoSize` — re-declared browsable on `Panel`; the panel grows to its
    /// content when set.
    pub auto_size: bool,
    /// `AutoSizeMode` — grow-only vs grow-and-shrink.
    pub auto_size_mode: AutoSizeMode,
    /// `BorderStyle` — `None` (default), `FixedSingle`, or `Fixed3D`.
    pub border_style: BorderStyle,
    /// `TabStop` — `Panel` re-declares this with a default of **false** (a panel
    /// is not a tab stop), where `Control`'s default is `true`. Set on the base
    /// at construction; this field mirrors it for the designer surface.
    pub tab_stop: bool,
    /// `Text` — a panel has no visible caption, but the property exists (used by
    /// tooling); the string lives on the base and is mirrored here.
    pub text: String,
}

impl Default for Panel {
    /// `Panel`'s notable override: `BorderStyle = None` (not `Fixed3D`, which is
    /// the `BorderStyle` enum's own default) and `TabStop = false`.
    fn default() -> Self {
        let mut scrollable = ScrollableControl::new();
        scrollable.control_mut().tab_stop = false;
        Self {
            scrollable,
            auto_size: false,
            auto_size_mode: AutoSizeMode::GrowOnly,
            border_style: BorderStyle::None,
            tab_stop: false,
            text: String::new(),
        }
    }
}

impl Panel {
    pub fn new() -> Self {
        Self::default()
    }

    /// The children's `DisplayRectangle`, in client coordinates: the panel's box
    /// deflated by padding and by the border, its origin carrying that inset.
    pub fn local_display_rect(&self) -> Rect {
        local_display_rect(self.control(), border_thickness(self.border_style))
    }

    /// Where this panel's client origin lands on the canvas — its position plus
    /// the border, since client coordinates start just inside that border. The
    /// padding is not added: [`Self::local_display_rect`] carries it.
    pub fn content_origin(&self, bounds: Rect) -> Point {
        content_origin(
            bounds,
            border_thickness(self.border_style),
            self.scrollable.auto_scroll_position,
        )
    }

    /// Lays out the children inside [`Self::local_display_rect`].
    pub fn perform_layout(&mut self) {
        let display = self.local_display_rect();
        self.scrollable.children.perform_layout(display);
    }
}

impl std::ops::Deref for Panel {
    type Target = ScrollableControl;
    fn deref(&self) -> &ScrollableControl {
        &self.scrollable
    }
}

impl std::ops::DerefMut for Panel {
    fn deref_mut(&mut self) -> &mut ScrollableControl {
        &mut self.scrollable
    }
}

impl Control for Panel {
    fn control(&self) -> &ControlBase {
        self.scrollable.control()
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        self.scrollable.control_mut()
    }

    /// The children's extent measured from this panel's own display rect, plus the
    /// padding and the two border lines that surround it.
    fn preferred_size(&self, _c: &dyn Canvas) -> Size {
        let content = self.scrollable.scroll_content_size_within(self.local_display_rect());
        let p = self.control().padding;
        let b = 2.0 * border_thickness(self.border_style);
        Size::new(content.width + p.horizontal() + b, content.height + p.vertical() + b)
    }

    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        c.fill_rect(&bounds, &background(self.control(), c));
        paint_border(c, bounds, self.border_style);
        let control = self.control();
        let inset = border_thickness(self.border_style);
        let clip = client_rect_on_canvas(bounds, control, inset);
        c.push_clip(&clip);
        self.scrollable.children.paint(c, self.content_origin(bounds));
        c.pop_clip();
    }

    fn type_name(&self) -> &'static str {
        "Panel"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GroupBox — a captioned frame. Derives from Control DIRECTLY (not scrollable).
// 6 declared properties.
// ─────────────────────────────────────────────────────────────────────────────

/// Height of the caption band a `GroupBox` reserves at the top for its label, in
/// unscaled DIP. The toolkit derives it from the font at paint time; layout is
/// canvas-free, so it uses this nominal band (the default font's line height).
const GROUPBOX_CAPTION_BAND: f32 = 16.0;

/// `BUTTON` / `BP_GROUPBOX` — the themed group-box frame.
///
/// Taken from the SDK's own constant rather than written as `4`, for the reason
/// [`crate::theme::part`] gives: a part id copied wrong does not fail, it draws
/// something plausible. It is spelled here rather than added to
/// [`crate::theme::part`] because that module is another file's, and one `const`
/// in the family that uses it is cheaper than a cross-file edit — the SDK
/// remains the single source of the number either way.
const BP_GROUPBOX: i32 = windows::Win32::UI::Controls::BP_GROUPBOX.0;
/// `BP_GROUPBOX` — a live group box.
const GBS_NORMAL: i32 = windows::Win32::UI::Controls::GBS_NORMAL.0;
/// `BP_GROUPBOX` — a group box whose control is disabled.
const GBS_DISABLED: i32 = windows::Win32::UI::Controls::GBS_DISABLED.0;

/// `System.Windows.Forms.GroupBox`: a captioned, square etched frame that groups
/// controls. Unlike `Panel`, it descends from `Control` directly — it is **not**
/// scrollable — so it carries its own [`Children`] and its own `ControlBase`.
#[derive(Clone)]
pub struct GroupBox {
    control: ControlBase,

    /// `Text` — the caption, notched into the top border. `GroupBox` re-declares
    /// it; kept here as the authoritative caption while the base `text` mirrors
    /// it, because the caption is this type's defining feature.
    pub text: String,
    /// `AllowDrop` — re-declared browsable on `GroupBox`; mirrors the base.
    pub allow_drop: bool,
    /// `AutoSize` — grow to fit the grouped controls.
    pub auto_size: bool,
    /// `AutoSizeMode` — grow-only vs grow-and-shrink.
    pub auto_size_mode: AutoSizeMode,
    /// `FlatStyle` — how the frame is drawn (`Standard`/`Flat`/`Popup`/`System`).
    /// Only `Standard`/`Flat` differ visibly here; `System`/`Popup` fall back to
    /// the standard frame, documented rather than faked.
    pub flat_style: FlatStyle,
    /// `UseCompatibleTextRendering` — GDI vs GDI+ text metrics in the toolkit.
    /// Irrelevant to DirectWrite; stored for fidelity, not honoured.
    pub use_compatible_text_rendering: bool,

    /// The grouped children (`Controls`) and their layout state.
    pub children: Children,
}

impl Default for GroupBox {
    fn default() -> Self {
        Self {
            control: ControlBase::new(),
            text: String::new(),
            allow_drop: false,
            auto_size: false,
            auto_size_mode: AutoSizeMode::GrowOnly,
            flat_style: FlatStyle::Standard,
            use_compatible_text_rendering: false,
            children: Children::new(),
        }
    }
}

impl GroupBox {
    pub fn new() -> Self {
        Self::default()
    }

    /// `GroupBox.DisplayRectangle`, in client coordinates — its origin carrying
    /// the inset, like every other container.
    ///
    /// The inset is asymmetric, which is why this does not use the shared
    /// [`local_display_rect`] helper: 1 DIP of frame on the left, right and
    /// bottom, but the whole caption band on top.
    pub fn local_display_rect(&self) -> Rect {
        let s = self.control.size();
        let p = self.control.padding;
        let l = p.left + 1.0;
        let t = p.top + GROUPBOX_CAPTION_BAND;
        Rect::new(l, t, (s.width - p.right - 1.0).max(l), (s.height - p.bottom - 1.0).max(t))
    }

    /// Where this group box's client origin lands on the canvas, given the box
    /// `paint` was asked to draw into. Add a grouped child's `bounds` to it to get
    /// that child's on-screen rectangle — which is what a caller wanting to
    /// decorate a grouped control needs, and what it must not re-derive by hand.
    ///
    /// The frame and caption band are already in the children's coordinates, and
    /// a `GroupBox` does not scroll, so this is simply where the box sits.
    pub fn content_origin(&self, bounds: Rect) -> Point {
        Point::new(bounds.left, bounds.top)
    }

    /// Lays out the grouped children inside [`Self::local_display_rect`].
    pub fn perform_layout(&mut self) {
        let display = self.local_display_rect();
        self.children.perform_layout(display);
    }
}

impl std::ops::Deref for GroupBox {
    type Target = ControlBase;
    fn deref(&self) -> &ControlBase {
        &self.control
    }
}

impl std::ops::DerefMut for GroupBox {
    fn deref_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }
}

impl Control for GroupBox {
    fn control(&self) -> &ControlBase {
        &self.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }

    /// Wide enough for the children and the caption, tall enough for the caption
    /// band plus the children.
    ///
    /// The caption is *painted* in the system message font but *measured* here
    /// with the host's own format — see [`caption_format`] for why, and for what
    /// it would take to close the gap. The arithmetic is untouched by the
    /// repaint: this family's geometry is pinned by the layout parity harness.
    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        // The extent is in client coordinates, so measure it from the display
        // rect's origin — otherwise the frame and caption band would be counted
        // twice, once in the children's bounds and once in the sum below.
        let d = self.local_display_rect();
        let raw = self.children.content_extent();
        let content = Size::new((raw.width - d.left).max(0.0), (raw.height - d.top).max(0.0));
        let caption_w = c.measure(&self.text, caption_format(&self.control, c.formats())) + 12.0;
        Size::new(
            content.width.max(caption_w) + self.control.padding.horizontal() + 2.0,
            content.height + self.control.padding.vertical() + GROUPBOX_CAPTION_BAND + 1.0,
        )
    }

    /// Draws the frame with the caption notched into its top, then the grouped
    /// children.
    ///
    /// **Themed** (visual styles on, which is how the reference sheets were
    /// made): `BUTTON` / `BP_GROUPBOX`, which on a default Windows 11 is a
    /// single flat `#DCDCDC` line — sampled straight off `04-containers.png`,
    /// where the sheet's own group boxes are that one line and *not* the
    /// engraved groove. The state is the control's real one; measured, the
    /// theme draws the same line for `GBS_NORMAL` and `GBS_DISABLED`, so it
    /// changes no pixel today — passing a lie because it happens not to show
    /// would be a fact waiting to be wrong.
    ///
    /// `FlatStyle` does **not** reach the themed branch: `Standard`, `Flat`,
    /// `Popup` and `System` group boxes were rendered side by side on this
    /// machine and all four came out as the same `#DCDCDC` frame. The style
    /// still decides the classic edge, which is the only place it is visible.
    ///
    /// **Classic** (visual styles off): `ControlPaint.DrawBorder3D` with
    /// [`groupbox_edge`] — normally `Border3DStyle::Etched`, a sunken outer ring
    /// over a raised inner one, so the frame reads as two one-pixel lines
    /// (`ControlDark` with `ControlLightLight` under it) rather than as a stroke.
    ///
    /// Either way the frame's top runs along the caption band's centre line,
    /// which is why it starts half a band below the control's own top edge —
    /// 8 DIP, which is exactly where the reference sheet puts it.
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        let colors = c.visuals().colors;
        let frame = Rect::new(
            bounds.left,
            bounds.top + GROUPBOX_CAPTION_BAND * 0.5,
            bounds.right,
            bounds.bottom,
        );
        // The ground the part is composited onto is this group box's own: the
        // theme leaves the frame's interior alone (`BP_GROUPBOX` is partially
        // transparent), so what shows through it is the pre-fill — which must be
        // the colour the container would have put there itself.
        if !c.draw_theme_part(
            theme::class::BUTTON,
            BP_GROUPBOX,
            groupbox_state(self.control.enabled),
            frame,
            background(&self.control, c),
        ) {
            c.draw_edge(&frame, groupbox_edge(self.flat_style), Border3DSide::ALL);
        }

        // Notch the caption into the top of the frame: measure the label in the
        // system message font, erase the segment of the frame behind it with the
        // container's own ground, then draw the text over the gap. The band is
        // taller than the frame is thick — one themed line, or the two classic
        // rings — so the erase clears it whole either way.
        //
        // The toolkit does the same thing the other way round (`GroupBoxRenderer`
        // draws the part in pieces around the caption). Both land on the same
        // pixels here, and erasing keeps ONE frame rectangle rather than three,
        // which is what stops the themed and the classic branch from drifting.
        let font = &c.visuals().fonts.message;
        let text_w = c.measure(&self.text, font);
        if text_w > 0.0 {
            let pad = 4.0;
            let notch = Rect::new(
                bounds.left + 8.0 - pad,
                bounds.top,
                bounds.left + 8.0 + text_w + pad,
                bounds.top + GROUPBOX_CAPTION_BAND,
            );
            c.fill_rect(&notch, &background(&self.control, c));
            let caption_rect = Rect::new(
                bounds.left + 8.0,
                bounds.top,
                bounds.left + 8.0 + text_w,
                bounds.top + GROUPBOX_CAPTION_BAND,
            );
            // `ControlText` is what a caption is painted in; an explicit
            // `ForeColor` still wins, as the property means.
            let fg = self.control.fore_color.unwrap_or(colors.control_text);
            c.text(&self.text, &caption_rect, font, &fg, false);
        }

        // The grouped children carry the frame and caption band in their own
        // coordinates, so the translation is just where the box sits; the clip is
        // the display rect moved the same way.
        let origin = self.content_origin(bounds);
        let d = self.local_display_rect();
        let clip = Rect::new(
            origin.x + d.left,
            origin.y + d.top,
            origin.x + d.right,
            origin.y + d.bottom,
        );
        c.push_clip(&clip);
        self.children.paint(c, origin);
        c.pop_clip();
    }

    fn type_name(&self) -> &'static str {
        "GroupBox"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared painting helpers.
// ─────────────────────────────────────────────────────────────────────────────

/// **Returns CLIENT space.** A container's `DisplayRectangle`: its client box
/// deflated by padding and by `inset` (the border it paints), with its origin at
/// `(padding.left + inset, padding.top + inset)` — *not* at zero.
///
/// That origin is the correction the parity harness forced. `ControlBase::bounds`
/// is parent-**client**-relative, so a display rect normalised to `(0, 0)` puts
/// docked children (placed against the display rect) and anchored children (which
/// keep their client-authored coordinates) in two different spaces. Carrying the
/// inset in the origin keeps both in one, and makes the rectangle mean what
/// WinForms' `DisplayRectangle` means: measured against real WinForms, a 200×100
/// panel with `Padding = 10` reports `10,10,190,90`.
///
/// Deliberately derived from the control's SIZE, never from its `bounds` position
/// — `bounds` is itself parent-relative, so reading its origin here would mix two
/// spaces and reintroduce the bug this convention removes.
///
/// Containers with a symmetric inset call this through their own
/// `local_display_rect()`; `GroupBox` computes its own, because its caption band
/// makes the top inset differ from the other three.
pub fn local_display_rect(control: &ControlBase, inset: f32) -> Rect {
    let s = control.size();
    let p = control.padding;
    // The ORIGIN comes from the padding alone. Client coordinates already start
    // inside the border, so the border must not offset them a second time — the
    // panels harness proved it: `table-border-single` (padding 0, border 1)
    // reports `0,0,301,98` in WinForms, not `1,1,…`. The border still costs its
    // two lines of SIZE, which is why the far edges lose `2 * inset`.
    let l = p.left;
    let t = p.top;
    Rect::new(
        l,
        t,
        (s.width - p.right - 2.0 * inset).max(l),
        (s.height - p.bottom - 2.0 * inset).max(t),
    )
}

/// **Takes CANVAS space, returns CANVAS space.** Where a container's client
/// origin — the `(0, 0)` its children's `bounds` are measured from — lands on the
/// canvas, given the box `paint` was asked to draw into.
///
/// It adds **no** padding: [`local_display_rect`] already carries that in its
/// origin, and adding it again would apply it twice. It *does* add `inset`, the
/// border — because client coordinates start inside the border, so client `(0,0)`
/// is the point just within it. Between them the two functions account for the
/// border exactly once: as size in the display rect, as offset here.
///
/// This is the single point at which client space becomes canvas space. Anything
/// that needs a child's on-screen position (a host hit-test, a decoration drawn
/// over a control) must add this to the child's `bounds` rather than re-deriving
/// an offset for itself — two copies of that sum are exactly how a container's
/// geometry and its painting drift apart.
///
/// Prefer the inherent `content_origin(&self, bounds)` each container exposes: it
/// supplies its own inset.
pub fn content_origin(bounds: Rect, inset: f32, scroll: Point) -> Point {
    Point::new(bounds.left + inset + scroll.x, bounds.top + inset + scroll.y)
}

/// **Takes CANVAS space, returns CANVAS space.** The client rectangle children are
/// clipped to — the same deflation as [`local_display_rect`], but left where it is
/// instead of moved to the origin.
///
/// The pair is deliberate: [`local_display_rect`] answers « how big is the area,
/// measured from zero » (what the layout engine needs), this one answers « where
/// is that area on screen » (what clipping needs). Using one where the other
/// belongs is the mistake this module is shaped to prevent.
pub fn client_rect_on_canvas(bounds: Rect, control: &ControlBase, inset: f32) -> Rect {
    let p = control.padding;
    let l = bounds.left + p.left + inset;
    let t = bounds.top + p.top + inset;
    Rect::new(
        l,
        t,
        (bounds.right - p.right - inset).max(l),
        (bounds.bottom - p.bottom - inset).max(t),
    )
}

/// The border thickness a `BorderStyle` occupies, in DIP: none, a single hairline,
/// or the two-line 3-D bevel.
///
/// Public because `SplitContainer` *declares* its own `BorderStyle` (the
/// catalogue says so) and therefore cannot borrow `Panel`'s inset — without this
/// it would copy the mapping, and two copies of a three-line table are exactly
/// how the geometry of a border and the painting of it drift apart.
pub fn border_thickness(style: BorderStyle) -> f32 {
    match style {
        BorderStyle::None => 0.0,
        BorderStyle::FixedSingle => 1.0,
        BorderStyle::Fixed3D => 2.0,
    }
}

/// The ground a container fills its box with: its own `BackColor` when one was
/// set, else `SystemColors.Control`.
///
/// `back_color = None` is the port's model of an *ambient* colour — the value a
/// real control inherits from its parent. There is no parent graph here, so the
/// resolution is `Control.DefaultBackColor`, which is `SystemColors.Control`
/// (`#F0F0F0` on a default Windows 11) and is what every reference sheet shows
/// behind and inside every container.
fn background(control: &ControlBase, c: &dyn ControlCanvas) -> D2D1_COLOR_F {
    control.back_color.unwrap_or(c.visuals().colors.control)
}

/// The `BP_GROUPBOX` state a group box is in — `GBS_DISABLED` when its control
/// is disabled, `GBS_NORMAL` otherwise. Those are the only two the part has.
///
/// A free function for the same reason [`groupbox_edge`] is one: the mapping is
/// asserted without a window. It is deliberately **not** folded into the call
/// site, because "the frame looks the same in both states" is a *measurement*
/// about today's theme, not a licence to stop asking which state a control is
/// in — the moment a theme greys the frame, this is already right.
fn groupbox_state(enabled: bool) -> i32 {
    if enabled {
        GBS_NORMAL
    } else {
        GBS_DISABLED
    }
}

/// The 3-D edge a `GroupBox` frames itself with **when visual styles are off**,
/// for a given `FlatStyle`.
///
/// With them on the frame comes from `BP_GROUPBOX` and this is not consulted —
/// measured, all four flat styles render the same themed line (see
/// [`GroupBox::paint`]). What follows is therefore the classic rendering only.
///
/// `Standard` is the engraved groove — a sunken ring with a raised one inside,
/// which is what `ControlPaint.DrawBorder3D(Border3DStyle.Etched)` paints.
/// `Flat` is the toolkit's one visible alternative: a single flat ring in
/// `ControlDark`, no bevel. `System` and `Popup` are painted as `Standard`, as
/// [`GroupBox::flat_style`] declares — they differ only under a theme this
/// library does not read.
///
/// A free function so the mapping can be asserted without a window, the way this
/// crate keeps every decision testable.
fn groupbox_edge(style: FlatStyle) -> Border3DStyle {
    match style {
        FlatStyle::Flat => Border3DStyle::Flat,
        _ => Border3DStyle::Etched,
    }
}

/// Paints a `BorderStyle`, exactly as the toolkit draws it, and returns the
/// **interior** the content goes in.
///
/// * `None` — nothing; the interior is the box itself.
/// * `FixedSingle` — one **square** line in `SystemColors.WindowFrame`. That is
///   the colour the reference sheet actually shows (`#646464` here), because a
///   `FixedSingle` control is a `WS_BORDER` window and Windows paints that frame
///   with `COLOR_WINDOWFRAME`, not with a shadow grey.
/// * `Fixed3D` — a real `DrawEdge` bevel, [`Border3DStyle::Sunken`]: the two
///   concentric one-pixel rings of a well, dark on the top/left and light on the
///   bottom/right. The old two-tone stroke approximation is gone.
///
/// ## Why `Fixed3D` takes **no** theme part — a measurement, not an omission
///
/// Every other family in this wave replaced its `Fixed3D` frame with a themed
/// one, and the obvious move here was the same: a `TextBox`'s `Fixed3D` is the
/// theme's `EDIT` / `EP_EDITTEXT` line, `#ABADB3`, so surely a `Panel`'s is too.
/// It is not, and the two are not even close.
///
/// A `Panel` (and a `UserControl`) asks for `Fixed3D` by putting
/// `WS_EX_CLIENTEDGE` on a **plain window class**, and `uxtheme` does not theme
/// that client edge — `DefWindowProc` still paints it, from `GetSysColor`. A
/// `TextBox`, a `ListView` and a `TreeView` are common controls that draw their
/// own themed border instead, which is why the same property name produces two
/// different frames.
///
/// Sampled on this machine, with visual styles **on**, a `Panel` with
/// `BorderStyle.Fixed3D` — identically through `Form.DrawToBitmap` (how the
/// reference sheets are made) and off the live screen:
///
/// | ring | top + left | bottom + right |
/// |---|---|---|
/// | outer | `#A0A0A0` (`ControlDark`) | `#FFFFFF` (`ControlLightLight`) |
/// | inner | `#696969` (`ControlDarkDark`) | `#E3E3E3` (`ControlLight`) |
///
/// which is exactly [`Border3DStyle::Sunken`] out of `edge_colors` — so the
/// classic path below is already pixel-identical and a themed branch would only
/// make it wrong. `the_container_fixed3d_frame_is_the_classic_well` pins those
/// four hexes so a future migration cannot "finish the job" by mistake.
///
/// **This is a statement about containers, not about the function.** `ListView`
/// and `TreeView` reach `Fixed3D` through this very function and *do* wear the
/// themed `#ABADB3` line; a `Label`'s `Fixed3D` is a single `SS_SUNKEN` ring
/// (`#A0A0A0` over `#FFFFFF`). Three families, three frames, one shared name —
/// so a family that needs its own must branch around this call rather than
/// change what it draws for everyone.
///
/// **Public because `BorderStyle` is not this family's alone.** `TextBoxBase`,
/// `ListBox`, `TreeView`, `ListView` and `SplitContainer` all declare it, and a
/// second implementation of the same three lines is how a field and a list end
/// up reading as different depths — this crate has already paid for duplicated
/// definitions (`HorizontalAlignment` in four copies, two `LeftRightAlignment`s
/// whose defaults had drifted). One bevel, one caller-visible interior.
///
/// The interior is inset by the rings actually **painted**, i.e. one *device*
/// pixel each, while [`border_thickness`] reports what the **layout** reserves,
/// in DIP. At 100 % they are the same number; above it the painted frame is the
/// thinner of the two, which is what keeps a bevel hairline at every scale.
/// Containers deflate their display rect with `border_thickness` and clip with
/// [`client_rect_on_canvas`], so they discard this return value — it exists for
/// the families that lay their content out *against the frame they just drew*.
///
/// **The interior contract is unchanged by the themed migration**: nothing in
/// this function became conditional on visual styles, so `views.rs` still gets
/// `bounds` for `None`, one device pixel off for `FixedSingle` and two for
/// `Fixed3D`, on a themed machine exactly as on a classic one.
pub fn paint_border(c: &dyn ControlCanvas, bounds: Rect, style: BorderStyle) -> Rect {
    match style {
        BorderStyle::None => bounds,
        BorderStyle::FixedSingle => {
            c.stroke_rect(&bounds, &c.visuals().colors.window_frame);
            edge_interior(&bounds, 1, c.scale())
        }
        BorderStyle::Fixed3D => c.draw_edge(&bounds, Border3DStyle::Sunken, Border3DSide::ALL),
    }
}

/// The format a caption is **measured** with when only a bare [`Canvas`] is at
/// hand — the one thing this repaint cannot fix from inside this file.
///
/// A `GroupBox` *paints* its caption in the system message font
/// (`lfMessageFont`, see [`GroupBox::paint`]), which is reachable only through
/// [`ControlCanvas::visuals`]. But `Control::preferred_size` receives a
/// `&dyn Canvas`, which has no way to answer for the system font, so the
/// measurement falls back to the host's own text format. The two agree on where
/// the caption sits; they can disagree on how wide a long caption makes the box.
///
/// Closing the gap means widening `Control::preferred_size` to take a
/// `&dyn ControlCanvas` (or teaching `Canvas` to hand out the system font) —
/// `control.rs`'s call, not this file's. Measuring with a *wrong* font is the
/// lesser error of the two available: dropping the caption term instead would
/// silently shrink every group box whose caption is longer than its content.
fn caption_format<'a>(control: &ControlBase, f: &'a TextFormats) -> &'a IDWriteTextFormat {
    match control.font.unwrap_or(FontRole::Body) {
        FontRole::Caption => &f.caption,
        FontRole::CaptionStrong => &f.caption_strong,
        FontRole::Body => &f.body,
        FontRole::BodyStrong => &f.body_strong,
        FontRole::Heading => &f.heading,
        FontRole::Title => &f.title,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{AnchorStyles, DockStyle};

    // A minimal child used to exercise the layout pass without pulling in another
    // family's control. A `Panel` is the natural choice: it is a `Control` and it
    // is in this file.
    fn child(bounds: Rect, dock: DockStyle, anchor: AnchorStyles) -> Box<dyn Control> {
        let mut p = Panel::new();
        p.control_mut().bounds = bounds;
        p.control_mut().dock = dock;
        p.control_mut().anchor = anchor;
        Box::new(p)
    }

    // ── Declared defaults, asserted against the catalogue ────────────────

    #[test]
    fn scrollable_control_defaults_match_the_catalogue() {
        let s = ScrollableControl::new();
        assert!(!s.auto_scroll);
        assert!(s.auto_scroll_margin.is_empty());
        assert!(s.auto_scroll_min_size.is_empty());
        assert_eq!(s.auto_scroll_position, Point::ORIGIN);
        // HScrollProperties/VScrollProperties documented defaults.
        assert_eq!(s.horizontal_scroll.maximum, 100.0);
        assert_eq!(s.vertical_scroll.large_change, 10.0);
        assert!(!s.horizontal_scroll.visible && s.horizontal_scroll.enabled);
    }

    #[test]
    fn container_control_defaults_match_the_catalogue() {
        let c = ContainerControl::new();
        assert_eq!(c.auto_scale_mode, AutoScaleMode::Inherit);
        assert_eq!(c.auto_validate, AutoValidate::Inherit);
        assert!(c.active_control.is_none());
        assert!(c.parent_form.is_none());
    }

    #[test]
    fn form_defaults_match_the_catalogue() {
        let f = Form::new();
        assert_eq!(f.form_border_style, FormBorderStyle::Sizable);
        assert_eq!(f.window_state, FormWindowState::Normal);
        assert_eq!(f.start_position, FormStartPosition::WindowsDefaultLocation);
        assert_eq!(f.size_grip_style, SizeGripStyle::Auto);
        assert_eq!(f.form_corner_preference, FormCornerPreference::Default);
        assert_eq!(f.opacity, 1.0);
        assert!(f.control_box && f.maximize_box && f.minimize_box);
        assert!(f.show_icon && f.show_in_taskbar);
        assert!(!f.top_most && !f.help_button && !f.is_mdi_container);
        assert!(f.mdi_children_minimized_anchor_bottom);
        assert!(f.auto_scale, "the obsolete AutoScale still defaults to true");
        // Form's two inherited overrides.
        assert_eq!(f.container.auto_validate, AutoValidate::EnablePreventFocusChange);
        // TabStop stays the Control default (true) for a Form.
        assert!(f.control().tab_stop);
    }

    #[test]
    fn form_is_mdi_child_is_derived_not_stored() {
        let mut f = Form::new();
        assert!(!f.is_mdi_child());
        f.mdi_parent = Some("main".into());
        assert!(f.is_mdi_child(), "an MDI parent makes it an MDI child");
    }

    #[test]
    fn user_control_and_panel_and_groupbox_defaults() {
        let u = UserControl::new();
        assert_eq!(u.border_style, BorderStyle::None);
        assert!(!u.auto_size);

        let p = Panel::new();
        assert_eq!(p.border_style, BorderStyle::None, "Panel overrides BorderStyle to None");
        assert!(!p.tab_stop, "Panel overrides TabStop to false");
        assert!(!p.control().tab_stop, "and the base agrees");

        let g = GroupBox::new();
        assert_eq!(g.flat_style, FlatStyle::Standard);
        assert!(!g.auto_size && !g.allow_drop && !g.use_compatible_text_rendering);
    }

    // ── The repaint: system edges, and the inset they must agree with ────

    /// The border a container PAINTS and the border its layout RESERVES are two
    /// statements of one fact, and nothing checks them against each other at
    /// run time. `Fixed3D` is `DrawEdge(Sunken)`, which is two one-pixel rings,
    /// so the inset must be 2 — a single-ring style with a 2 DIP inset would
    /// leave a one-pixel gap between the frame and the first child, on every
    /// panel in the library.
    #[test]
    fn the_border_inset_is_exactly_the_rings_the_edge_paints() {
        let colors = crate::system::SystemColors::read();
        let sunken = crate::system::edge_colors(&colors, Border3DStyle::Sunken);
        assert_eq!(sunken.rings() as f32, border_thickness(BorderStyle::Fixed3D));
        // `FixedSingle` is one square line, not an edge, and reserves one DIP.
        assert_eq!(border_thickness(BorderStyle::FixedSingle), 1.0);
        assert_eq!(border_thickness(BorderStyle::None), 0.0);
    }

    /// With visual styles OFF, a group box frames itself with the engraved
    /// groove — two rings — unless `FlatStyle::Flat` asks for the toolkit's
    /// single flat ring. If `Standard` ever stopped being `Etched`, every
    /// classic group box would read as embossed.
    #[test]
    fn a_groupbox_frames_itself_with_the_etched_groove() {
        assert_eq!(groupbox_edge(FlatStyle::Standard), Border3DStyle::Etched);
        assert_eq!(groupbox_edge(FlatStyle::System), Border3DStyle::Etched);
        assert_eq!(groupbox_edge(FlatStyle::Popup), Border3DStyle::Etched);
        assert_eq!(groupbox_edge(FlatStyle::Flat), Border3DStyle::Flat);
        let colors = crate::system::SystemColors::read();
        assert_eq!(crate::system::edge_colors(&colors, Border3DStyle::Etched).rings(), 2);
        assert_eq!(crate::system::edge_colors(&colors, Border3DStyle::Flat).rings(), 1);
    }

    // ── The themed frame: the pixels, pinned ─────────────────────────────

    /// `#RRGGBB` for a BGRA word, so a failure prints a colour that can be
    /// compared with the reference sheet by eye rather than a decimal.
    fn hex(px: u32) -> String {
        format!("#{:02X}{:02X}{:02X}", (px >> 16) & 0xFF, (px >> 8) & 0xFF, px & 0xFF)
    }

    /// The same for a Direct2D colour, so a system colour and a themed pixel
    /// can be compared in one alphabet.
    fn hex_of(c: D2D1_COLOR_F) -> String {
        let ch = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
        format!("#{:02X}{:02X}{:02X}", ch(c.r), ch(c.g), ch(c.b))
    }

    /// Renders one `BUTTON` part through `uxtheme` into a top-down 32-bit DIB
    /// and hands back its pixels, so a hex can be asserted with no window, no
    /// device and no swap chain — the same GDI round trip [`crate::theme`]
    /// performs, stopping short of the Direct2D upload.
    ///
    /// The DIB is pre-filled with `COLOR_BTNFACE` (`#F0F0F0`), which is the
    /// ground the reference sheet puts behind every group box, so a sampled
    /// pixel is directly comparable with the sheet.
    ///
    /// `None` is a **skip**, not a failure: on a themed-off machine the classic
    /// path is the correct rendering and there is no themed pixel to assert.
    fn sample_button_part(part: i32, state: i32, w: i32, h: i32) -> Option<Vec<u32>> {
        use windows::core::w;
        use windows::Win32::Foundation::RECT;
        use windows::Win32::Graphics::Gdi::{
            CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, SelectObject,
            BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        };
        use windows::Win32::UI::Controls::{
            CloseThemeData, DrawThemeBackground, IsAppThemed, IsThemeActive,
        };
        use windows::Win32::UI::HiDpi::OpenThemeDataForDpi;

        if std::env::var_os(theme::CLASSIC_ENV).is_some() {
            return None;
        }
        unsafe {
            if !IsThemeActive().as_bool() || !IsAppThemed().as_bool() {
                return None;
            }
            let theme = OpenThemeDataForDpi(None, w!("BUTTON"), 96);
            if theme.is_invalid() {
                return None;
            }
            let dc = CreateCompatibleDC(None);
            // `biHeight` NEGATIVE: a top-down DIB, so row 0 is the top row and
            // an index computed as `y * w + x` means what it reads as.
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
                .expect("the test's own DIB section");
            let previous = SelectObject(dc, bitmap.into());
            let pixels = std::slice::from_raw_parts_mut(bits.cast::<u32>(), (w * h) as usize);
            pixels.fill(0xFFF0_F0F0);
            let rect = RECT { left: 0, top: 0, right: w, bottom: h };
            let drawn = DrawThemeBackground(theme, dc, part, state, &rect, None).is_ok();
            // GDI batches per thread: reading the bits without flushing can read
            // them BEFORE the theme has drawn, intermittently and under load.
            let _ = GdiFlush();
            let out = drawn.then(|| pixels.to_vec());

            SelectObject(dc, previous);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(dc);
            let _ = CloseThemeData(theme);
            out
        }
    }

    /// **The finding this migration rests on.** The group boxes on
    /// `04-containers.png` are a single flat `#DCDCDC` line — not the two-ring
    /// etched groove the classic path paints — and `BP_GROUPBOX` is what
    /// produces it, byte for byte.
    ///
    /// Sampled clear of the corners: the left edge at mid-height, and the row
    /// just inside it, which the part leaves at the pre-fill because a group box
    /// draws a frame and no fill.
    #[test]
    fn the_themed_groupbox_frame_is_the_reference_line() {
        let (w, h) = (200usize, 90usize);
        let Some(px) = sample_button_part(BP_GROUPBOX, GBS_NORMAL, w as i32, h as i32) else {
            eprintln!("[containers] visual styles unavailable — themed frame not asserted");
            return;
        };
        let mid = h / 2;
        assert_eq!(hex(px[mid * w]), "#DCDCDC", "the left edge of BP_GROUPBOX");
        assert_eq!(hex(px[mid * w + 1]), "#F0F0F0", "and it is ONE line, not a groove");
        assert_eq!(hex(px[w / 2]), "#DCDCDC", "the top edge");
        assert_eq!(hex(px[mid * w + w - 1]), "#DCDCDC", "the right edge");
        // The part keeps the last row for itself: the bottom line sits one pixel
        // inside the rectangle, exactly as the live control does on screen.
        assert_eq!(hex(px[(h - 2) * w + w / 2]), "#DCDCDC", "the bottom edge, one row in");
        assert_eq!(hex(px[mid * w + w / 2]), "#F0F0F0", "the interior is the caller's ground");
    }

    /// The frame is the SAME line in both states — measured, not assumed, which
    /// is what lets [`GroupBox::paint`] pass the control's real state without
    /// the frame moving. The live disabled group box on this machine agrees; it
    /// is its CAPTION that greys, not its frame.
    #[test]
    fn the_themed_groupbox_frame_is_the_same_line_when_disabled() {
        let (w, h) = (200usize, 90usize);
        let (Some(normal), Some(disabled)) = (
            sample_button_part(BP_GROUPBOX, GBS_NORMAL, w as i32, h as i32),
            sample_button_part(BP_GROUPBOX, GBS_DISABLED, w as i32, h as i32),
        ) else {
            eprintln!("[containers] visual styles unavailable — themed states not asserted");
            return;
        };
        let mid = h / 2;
        assert_eq!(hex(disabled[mid * w]), "#DCDCDC", "GBS_DISABLED frame");
        assert_eq!(hex(normal[mid * w]), hex(disabled[mid * w]), "the frame is state-invariant");
    }

    /// The state a group box asks the theme for. `GBS_*` are the part's only two.
    #[test]
    fn a_disabled_groupbox_asks_the_theme_for_its_own_state() {
        assert_eq!(groupbox_state(true), GBS_NORMAL);
        assert_eq!(groupbox_state(false), GBS_DISABLED);
        assert_ne!(GBS_NORMAL, GBS_DISABLED, "two states, or the call says nothing");
    }

    /// **The other finding: a container's `Fixed3D` is NOT the themed `EDIT`
    /// frame**, however much it shares the property name with a `TextBox`.
    ///
    /// A `Panel` asks for it with `WS_EX_CLIENTEDGE` on a plain window class,
    /// which `uxtheme` leaves to `DefWindowProc` — so with visual styles ON the
    /// live control still shows the classic two-ring well. Sampled off both a
    /// `Form.DrawToBitmap` (how the reference sheets are made) and the real
    /// screen, a `Panel` with `BorderStyle.Fixed3D` gives `#A0A0A0` over
    /// `#696969` on the top/left and `#FFFFFF` over `#E3E3E3` on the
    /// bottom/right — which is precisely what [`paint_border`] already paints.
    ///
    /// The assertion is on the COLOURS rather than on the absence of a call,
    /// because "no themed branch" is only right for as long as those four hexes
    /// are what the toolkit shows.
    #[test]
    fn the_container_fixed3d_frame_is_the_classic_well() {
        let colors = crate::system::SystemColors::read();
        let sunken = crate::system::edge_colors(&colors, Border3DStyle::Sunken);
        let (Some(ol), Some(od), Some(il), Some(id)) =
            (sunken.outer_light, sunken.outer_dark, sunken.inner_light, sunken.inner_dark)
        else {
            panic!("a sunken well is two full rings");
        };
        assert_eq!(hex_of(ol), "#A0A0A0", "outer top/left — sampled on a live Panel");
        assert_eq!(hex_of(od), "#FFFFFF", "outer bottom/right");
        assert_eq!(hex_of(il), "#696969", "inner top/left");
        assert_eq!(hex_of(id), "#E3E3E3", "inner bottom/right");
        // And it is emphatically not the themed EDIT line a TextBox wears.
        assert_ne!(hex_of(ol), "#ABADB3", "a Panel is not a TextBox");
    }

    // ── Display-rect / padding arithmetic, in CLIENT space ───────────────

    #[test]
    fn panel_local_display_rect_is_deflated_by_padding_then_border() {
        let mut p = Panel::new();
        p.control_mut().set_bounds(Rect::new(0.0, 0.0, 100.0, 80.0));
        p.control_mut().padding = crate::enums::Padding::all(5.0);
        p.border_style = BorderStyle::Fixed3D; // 2 DIP
        let d = p.local_display_rect();
        // The origin carries the PADDING only — client coordinates already start
        // inside the border. The border costs two lines of size instead:
        // 100 - 5 - 2*2 = 91, 80 - 5 - 2*2 = 71.
        assert_eq!((d.left, d.top, d.right, d.bottom), (5.0, 5.0, 91.0, 71.0));
    }

    /// Measured against real WinForms by the panels harness: `table-border-single`
    /// (padding 0, `FixedSingle`) reports `0,0,301,98` on a 303×100 box. A border
    /// that offsets the origin put every bordered container's children 1 DIP out.
    #[test]
    fn a_border_costs_size_but_never_offsets_the_display_origin() {
        let mut p = Panel::new();
        p.control_mut().set_bounds(Rect::new(0.0, 0.0, 303.0, 100.0));
        p.border_style = BorderStyle::FixedSingle;
        let d = p.local_display_rect();
        assert_eq!((d.left, d.top, d.right, d.bottom), (0.0, 0.0, 301.0, 98.0));

        // The border reappears exactly once, when client space becomes canvas.
        let o = p.content_origin(Rect::new(40.0, 20.0, 343.0, 120.0));
        assert_eq!((o.x, o.y), (41.0, 21.0));
    }

    /// Measured against real WinForms by the parity harness: a 200×100 container
    /// with `Padding = 10` reports `DisplayRectangle = 10,10,190,90`. A display
    /// rect normalised to `(0,0)` was the port's last two parity failures.
    #[test]
    fn display_rect_matches_winforms_for_a_padded_container() {
        let mut s = ScrollableControl::new();
        s.control_mut().set_bounds(Rect::new(0.0, 0.0, 200.0, 100.0));
        s.control_mut().padding = crate::enums::Padding::all(10.0);
        let d = s.local_display_rect();
        assert_eq!((d.left, d.top, d.right, d.bottom), (10.0, 10.0, 190.0, 90.0));
    }

    /// The local display rect must come from the control's SIZE, never from the
    /// position of its own (parent-relative) `bounds`.
    #[test]
    fn local_display_rect_ignores_the_containers_own_position() {
        let mut a = Panel::new();
        a.control_mut().set_bounds(Rect::new(0.0, 0.0, 100.0, 80.0));
        let mut b = Panel::new();
        b.control_mut().set_bounds(Rect::new(640.0, 480.0, 740.0, 560.0));
        let (ra, rb) = (a.local_display_rect(), b.local_display_rect());
        assert_eq!((ra.left, ra.top, ra.right, ra.bottom), (rb.left, rb.top, rb.right, rb.bottom));
    }

    /// The `D23` shape from the harness: a container placed away from the origin,
    /// with padding, must report a display rect starting at the padding — and a
    /// Top-docked child must then land there too, in the same space.
    #[test]
    fn a_padded_container_docks_its_child_at_the_padding_not_at_zero() {
        let mut s = ScrollableControl::new();
        // Deliberately NOT at the origin: the display rect must not follow it.
        s.control_mut().set_bounds(Rect::new(400.0, 300.0, 600.0, 400.0));
        s.control_mut().padding = crate::enums::Padding::all(10.0);
        assert_eq!(
            {
                let d = s.local_display_rect();
                (d.left, d.top, d.right, d.bottom)
            },
            (10.0, 10.0, 190.0, 90.0),
            "the display rect is client-relative, independent of where the container sits"
        );

        s.children.push(child(Rect::new(0.0, 0.0, 0.0, 24.0), DockStyle::Top, AnchorStyles::default()));
        s.perform_layout();
        let b = s.children.get(0).unwrap().control().bounds;
        assert_eq!(
            (b.left, b.top, b.right, b.bottom),
            (10.0, 10.0, 190.0, 34.0),
            "a docked child starts at the padding, in the same space anchored children use"
        );

        // And the crossing into canvas space adds only the container's position:
        // the padding is already in the child's bounds.
        let o = s.content_origin(s.control().bounds);
        assert_eq!((o.x, o.y), (400.0, 300.0), "content_origin must not re-add the padding");
    }

    #[test]
    fn groupbox_local_display_rect_reserves_the_caption_band_on_top() {
        let mut g = GroupBox::new();
        g.control_mut().set_bounds(Rect::new(0.0, 0.0, 200.0, 120.0));
        let d = g.local_display_rect();
        // 1 DIP of frame each side, carried in the origin; the caption band on top.
        assert_eq!((d.left, d.top), (1.0, GROUPBOX_CAPTION_BAND));
        assert_eq!(d.right, 199.0);
        assert_eq!(d.bottom, 119.0);
    }

    // ── Child layout through the shared engine ───────────────────────────

    #[test]
    fn children_dock_resolves_in_reverse_z_order() {
        // Two top-docked children: the one added LAST must sit outermost (top).
        let mut s = ScrollableControl::new();
        s.control_mut().set_bounds(Rect::new(0.0, 0.0, 200.0, 100.0));
        s.children.push(child(Rect::new(0.0, 0.0, 0.0, 30.0), DockStyle::Top, AnchorStyles::default()));
        s.children.push(child(Rect::new(0.0, 0.0, 0.0, 20.0), DockStyle::Top, AnchorStyles::default()));
        s.perform_layout();
        let first = s.children.get(0).unwrap().control().bounds;
        let last = s.children.get(1).unwrap().control().bounds;
        assert_eq!((last.top, last.bottom), (0.0, 20.0), "last-added is outermost");
        assert_eq!((first.top, first.bottom), (20.0, 50.0), "first-added stacks below");
    }

    #[test]
    fn a_fill_child_takes_the_panels_client_rect() {
        let mut p = Panel::new();
        p.control_mut().set_bounds(Rect::new(0.0, 0.0, 100.0, 100.0));
        p.border_style = BorderStyle::FixedSingle; // 1 DIP inset
        p.children.push(child(Rect::new(0.0, 0.0, 0.0, 0.0), DockStyle::Fill, AnchorStyles::default()));
        p.perform_layout();
        let f = p.children.get(0).unwrap().control().bounds;
        // Client space starts inside the border, so the child starts at zero and
        // is 2 DIP shorter in each axis (one border line on each side).
        assert_eq!((f.left, f.top, f.right, f.bottom), (0.0, 0.0, 98.0, 98.0));
    }

    #[test]
    fn a_bottom_right_anchored_child_follows_a_growing_container() {
        let mut s = ScrollableControl::new();
        s.control_mut().set_bounds(Rect::new(0.0, 0.0, 200.0, 100.0));
        s.children.push(child(
            Rect::new(150.0, 70.0, 190.0, 90.0),
            DockStyle::None,
            AnchorStyles::RIGHT.union(AnchorStyles::BOTTOM),
        ));
        // First pass: previous == display, nothing moves.
        s.perform_layout();
        let a = s.children.get(0).unwrap().control().bounds;
        assert_eq!((a.left, a.top), (150.0, 70.0), "first pass moves nothing");
        // Grow the container by (+100, +50): a right/bottom anchor tracks it.
        s.control_mut().set_bounds(Rect::new(0.0, 0.0, 300.0, 150.0));
        s.perform_layout();
        let b = s.children.get(0).unwrap().control().bounds;
        assert_eq!((b.left, b.top, b.right, b.bottom), (250.0, 120.0, 290.0, 140.0));
    }

    // ── Coordinate spaces ────────────────────────────────────────────────

    /// The fix this pins: a container placed away from the origin keeps its
    /// children's `bounds` **parent-relative**, while the position it paints them
    /// at follows the container. Before this, `perform_layout` fed the engine a
    /// canvas-absolute rect, so moving a container meant shifting every
    /// descendant by hand.
    #[test]
    fn a_moved_container_keeps_child_bounds_local_and_moves_what_it_paints() {
        let mut p = Panel::new();
        // A 200x100 panel sitting at (300, 200) in its parent.
        p.control_mut().set_bounds(Rect::new(300.0, 200.0, 500.0, 300.0));
        p.children.push(child(Rect::new(0.0, 0.0, 0.0, 24.0), DockStyle::Top, AnchorStyles::default()));
        p.perform_layout();

        let before = p.children.get(0).unwrap().control().bounds;
        assert_eq!(
            (before.left, before.top, before.right, before.bottom),
            (0.0, 0.0, 200.0, 24.0),
            "a top-docked child sits at the CLIENT origin, not at (300, 200)"
        );

        // `paint` translates by the box it is given; a top-level container is
        // painted at its own rectangle, so that is the stand-in here.
        let origin = p.content_origin(p.control().bounds);
        assert_eq!((origin.x, origin.y), (300.0, 200.0));

        // Move the panel: no re-layout, no manual shifting of descendants.
        p.control_mut().set_location(340.0, 260.0);
        let after = p.children.get(0).unwrap().control().bounds;
        assert_eq!(
            (after.left, after.top, after.right, after.bottom),
            (before.left, before.top, before.right, before.bottom),
            "moving the parent must not touch a child's bounds"
        );
        let moved = p.content_origin(p.control().bounds);
        assert_eq!((moved.x, moved.y), (340.0, 260.0), "but what it paints follows the parent");
    }

    /// The padding belongs to the display rect and the border to the crossing
    /// into canvas space — each accounted exactly once. Carrying the padding here
    /// as well was the double-count the layout harness caught.
    #[test]
    fn the_paint_origin_carries_position_border_and_scroll_but_not_the_padding() {
        let mut p = Panel::new();
        p.control_mut().set_bounds(Rect::new(100.0, 50.0, 300.0, 200.0));
        p.control_mut().padding = crate::enums::Padding::all(4.0);
        p.border_style = BorderStyle::FixedSingle; // 1 DIP
        p.auto_scroll_position = Point::new(-10.0, -20.0); // scrolled down/right
        let origin = p.content_origin(p.control().bounds);
        // 100 + border 1 - scroll 10 = 91; 50 + 1 - 20 = 31. The PADDING is not
        // here — it is already inside the children's bounds, via the display rect.
        assert_eq!((origin.x, origin.y), (91.0, 31.0));

        // The clip is the fully inset box on canvas, and does not scroll.
        let clip = client_rect_on_canvas(
            p.control().bounds,
            p.control(),
            border_thickness(p.border_style),
        );
        assert_eq!((clip.left, clip.top), (105.0, 55.0), "100 + padding 4 + border 1");
    }

    /// Hit-testing crosses the same boundary: the caller converts a canvas point
    /// into local space, then compares against the children's local bounds.
    #[test]
    fn child_at_takes_a_local_point_and_answers_in_reverse_z_order() {
        let mut p = Panel::new();
        p.control_mut().set_bounds(Rect::new(300.0, 200.0, 500.0, 300.0));
        // Two overlapping children; the one added last paints on top.
        p.children.push(child(Rect::new(0.0, 0.0, 100.0, 50.0), DockStyle::None, AnchorStyles::default()));
        p.children.push(child(Rect::new(20.0, 10.0, 120.0, 60.0), DockStyle::None, AnchorStyles::default()));

        let origin = p.content_origin(p.control().bounds);
        // A canvas point inside the overlap, converted into client space.
        let canvas = Point::new(330.0, 220.0);
        let local = Point::new(canvas.x - origin.x, canvas.y - origin.y);
        assert_eq!(p.children.child_at(local), Some(1), "the topmost child wins");

        // A point only the first child covers.
        let local = Point::new(5.0, 5.0);
        assert_eq!(p.children.child_at(local), Some(0));
        // And one outside both.
        assert_eq!(p.children.child_at(Point::new(190.0, 90.0)), None);
    }

    // ── AutoScroll content extent ────────────────────────────────────────

    #[test]
    fn scroll_content_size_unions_children_and_applies_margin_and_min() {
        let mut s = ScrollableControl::new();
        s.control_mut().set_bounds(Rect::new(0.0, 0.0, 100.0, 100.0));
        // A child that overflows the client rect on both axes.
        let mut c = Panel::new();
        c.control_mut().bounds = Rect::new(10.0, 10.0, 180.0, 140.0);
        s.children.push(Box::new(c));
        // Raw extent from the display origin (0,0): 180 x 140.
        assert_eq!(s.scroll_content_size(), Size::new(180.0, 140.0));
        // Margin grows it.
        s.auto_scroll_margin = Size::new(20.0, 5.0);
        assert_eq!(s.scroll_content_size(), Size::new(200.0, 145.0));
        // A larger MinSize floors it.
        s.auto_scroll_min_size = Size::new(400.0, 100.0);
        assert_eq!(s.scroll_content_size(), Size::new(400.0, 145.0));
    }

    #[test]
    fn a_hidden_child_contributes_no_content_extent() {
        let mut s = ScrollableControl::new();
        s.control_mut().set_bounds(Rect::new(0.0, 0.0, 100.0, 100.0));
        let mut c = Panel::new();
        c.control_mut().bounds = Rect::new(0.0, 0.0, 500.0, 500.0);
        c.control_mut().visible = false;
        s.children.push(Box::new(c));
        assert_eq!(s.scroll_content_size(), Size::EMPTY, "a hidden child does not scroll");
    }

    // ── Trap found: the first layout pass must not treat an unset `previous`
    // as a zero rect, or every anchored child would jump by the full display
    // size on the very first layout. `Children::previous` is `Option` for
    // exactly this reason. ─────────────────────────────────────────────────
    #[test]
    fn trap_first_pass_does_not_move_an_anchored_child() {
        let mut s = ScrollableControl::new();
        s.control_mut().set_bounds(Rect::new(0.0, 0.0, 200.0, 100.0));
        s.children.push(child(
            Rect::new(20.0, 20.0, 60.0, 40.0),
            DockStyle::None,
            AnchorStyles::TOP.union(AnchorStyles::LEFT),
        ));
        s.perform_layout();
        let a = s.children.get(0).unwrap().control().bounds;
        assert_eq!((a.left, a.top, a.right, a.bottom), (20.0, 20.0, 60.0, 40.0));
    }
}
