//! Kubuno primitives — **containers**: the family that places rectangles so no
//! application has to compute them.
//!
//! ## Why this file exists
//!
//! The shell currently writes **314 `Rect::new(…)` calls by hand**. Every one of
//! them is a small re-derivation of « the header band is 56 tall, so the body
//! starts at `top + 56` », and every one of them has to be found and edited
//! again when a metric moves. The replica layer already ships the engine that
//! answers those questions — [`kubuno_controls::layout::layout`], checked at
//! 52/52 cases against the real toolkit, including the
//! `⌊cur/2⌋ − ⌊prev/2⌋` re-centring nobody guesses right — and it was simply not
//! reachable from a Kubuno surface. This family is that reach.
//!
//! ```ignore
//! let rects = Panel::new()
//!     .with_padding(Padding::all(space::LG))
//!     .top(HEADER_H)        // the fixed header
//!     .left(SIDEBAR_W)      // the nav column
//!     .fill()               // whatever is left
//!     .layout_children(body);
//! ```
//!
//! ## What each type owns
//!
//! | type | replica underneath | what Kubuno adds |
//! |---|---|---|
//! | [`Panel`] | [`kubuno_controls::containers::Panel`] | a child list, [`Surface`], canvas-space placement |
//! | [`Card`] | *(a `Panel`)* | the titled card surface of the web design system |
//! | [`GroupBox`] | [`kubuno_controls::containers::GroupBox`] | the Kubuno frame + caption |
//! | [`Splitter`] | [`kubuno_controls::layout_panels::SplitContainer`] | the grab band, the drag arithmetic, the divider |
//! | [`ScrollView`] | *(a `Panel` with `auto_scroll`)* | the ranges a scroll bar needs |
//! | [`Stack`] | [`kubuno_controls::layout_panels::flow_layout`] | a run that stretches across the cross axis |
//!
//! Not one line of Dock, Anchor, Flow or Split arithmetic is written here: the
//! four engines are called, never copied. What this file adds is the *space
//! crossing* (local → canvas), the Kubuno pixels, and an API shaped for the
//! call sites that exist.
//!
//! ## The two coordinate spaces, and the one rule
//!
//! The replica layer is strict about this and so is this family:
//!
//! * a child's `bounds` are **parent-relative**, in the container's own local
//!   space — that is what makes moving a container move its whole subtree;
//! * a container crosses into **canvas** space exactly once, and only through
//!   [`kubuno_controls::containers::content_origin`].
//!
//! Every method here says which space it speaks in its name or its first line.
//! `*_local` returns local space; everything else returns canvas space.
//!
//! ## The anchoring reference — the one place this differs from WinForms
//!
//! WinForms anchors against the change **since the last pass**, because it
//! writes each child's new rectangle back onto the child. A Kubuno container
//! keeps the caller's *design* rectangles untouched (they are the caller's
//! data, not ours to mutate), so the reference the anchor pass measures against
//! must be the display rect those rectangles were authored in — recorded on the
//! first layout, or stated up front with [`Panel::with_design_size`].
//!
//! That is what makes `layout_children` a **pure function of its argument**:
//! calling it twice at the same size answers the same thing twice, which the
//! incremental rule would not (the second call would move an anchored child a
//! second time). See `layout_children_is_idempotent` in the tests below.

use std::cell::Cell;
use std::ops::{Deref, DerefMut};

use drive_app_controls::{Canvas, Rect};
use kubuno_controls::containers::{
    border_thickness, client_rect_on_canvas, content_origin, Children, Point, ScrollProperties,
};
use kubuno_controls::enums::{AnchorStyles, BorderStyle, DockStyle, Padding, Size};
use kubuno_controls::layout::{layout, Item};
use kubuno_controls::layout_panels::{
    adjusted_distance, clamp_distance, flow_layout, FixedPanel, FlowChild, FlowDirection,
    Orientation, SplitRects,
};
use kubuno_controls::host::{self, vk, Cursor, Modifiers};
use kubuno_controls::{containers as kc, layout_panels as kl, Control};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::metrics::{control, pill, radius, space, ShadowLayer, SHADOW_GREY, SHADOW_MENU};
use crate::range::{ScrollBar, ScrollPart};
use crate::widget::{Widget, WidgetState};

// ─────────────────────────────────────────────────────────────────────────────
// Family-local metrics — composition numbers the web writes as Tailwind classes
// (read off the `@ui` sources named on each), kept here rather than as literals
// in a paint body.
// ─────────────────────────────────────────────────────────────────────────────

/// `@ui/Card.tsx` composition.
pub mod card_metrics {
    use crate::metrics::space;

    /// Header / footer padding: `px-4 py-3`.
    pub const PAD_X: f32 = space::LG;
    pub const PAD_Y: f32 = space::MD;
    /// `dense`: `px-3 py-2.5`.
    pub const DENSE_PAD_X: f32 = space::MD;
    pub const DENSE_PAD_Y: f32 = 10.0;
    /// Body padding: `p-4`, `dense` → `p-3`, `flush` → none.
    pub const BODY_PAD: f32 = space::LG;
    pub const DENSE_BODY_PAD: f32 = space::MD;
    /// The header row is `flex items-start gap-3`: icon · title block · actions.
    pub const HEADER_GAP: f32 = space::MD;
    /// The actions cluster: `flex shrink-0 items-center gap-1.5`.
    pub const ACTIONS_GAP: f32 = 6.0;
    /// The leading glyph: a lucide icon at 16, pushed down by `mt-0.5`.
    pub const ICON: f32 = 16.0;
    pub const ICON_TOP: f32 = space::XXS;
    /// `border` / `border-b` / `border-t`: one DIP (one CSS px).
    pub const RULE: f32 = 1.0;
    /// The dense title is set at `--kb-text-body` (the 12-DIP desktop body)
    /// over a 16-DIP line.
    pub const DENSE_TITLE_LINE: f32 = 16.0;
}

/// `@ui/ResizeHandle.tsx` — the web's pane divider, which [`SplitterStyle::Handle`]
/// paints.
pub mod handle_metrics {
    /// The visible line: `w-[5px] rounded-full`.
    pub const LINE: f32 = 5.0;
    /// The grip pill: `h-9 w-3.5 rounded-full`.
    pub const PILL_W: f32 = 14.0;
    pub const PILL_H: f32 = 36.0;
    /// `GripVertical size={13}`: lucide draws two columns of three dots at
    /// x = 9 / 15 and y = 5 / 12 / 19 in a 24 box, r = 1 with a 2 stroke —
    /// scaled to 13 that is ±1.6 across, ±3.8 down, and a 1.1 radius.
    pub const DOT_DX: f32 = 1.6;
    pub const DOT_DY: f32 = 3.8;
    pub const DOT_R: f32 = 1.1;
    /// `group-hover:bg-primary/40`, `group-hover:border-primary/40`.
    pub const HOVER_ALPHA: f32 = 0.4;
    /// `opacity-80` at rest.
    pub const REST_OPACITY: f32 = 0.8;
}

/// Tailwind's `shadow-sm` (`0 1px 2px 0 rgb(0 0 0 / 0.05)`), worn by the
/// ResizeHandle's grip pill.
const SHADOW_SM: [ShadowLayer; 1] = [ShadowLayer { dy: 1.0, blur: 2.0, spread: 0.0, opacity: 0.05 }];

/// Keyboard steps. The browser's own numbers (Chromium `ScrollableArea`):
/// an arrow scrolls 40 px, a page is 87.5 % of the viewport.
pub mod scroll_metrics {
    pub const LINE_STEP: f32 = 40.0;
    pub const PAGE_FRACTION: f32 = 0.875;
    /// The focus ring a focused scroller / splitter draws: `ring-2`.
    pub const FOCUS_RING: f32 = 2.0;
    /// A splitter moved with the arrows (WAI-ARIA « window splitter »): a small
    /// step, and a large one with Shift.
    pub const SPLIT_STEP: f32 = 10.0;
    pub const SPLIT_STEP_LARGE: f32 = 50.0;
}

// ─────────────────────────────────────────────────────────────────────────────
// Background and clip scopes.
//
// The opaque-ground rule of the other families reads `Canvas::current_bg()`,
// which is only right while a container has PUSHED its surface. A caller that
// paints a card and then paints controls into `card.body_rect()` used to see the
// window ground there (square blocks of the wrong colour inside the rounded
// card in the dark theme). These scopes make the push a one-liner that cannot be
// left unbalanced: they pop on drop.
// ─────────────────────────────────────────────────────────────────────────────

/// A pushed background (and optionally a pushed clip), popped when dropped.
///
/// ```ignore
/// card.paint(c, r, WidgetState::REST);
/// let _bg = card.push_surface(c);          // controls below read the card's ground
/// field.paint(c, card.body_rect(r), st);
/// ```
#[must_use = "the scope pops as soon as it is dropped"]
pub struct Scope<'a> {
    canvas: &'a dyn Canvas,
    bg: bool,
    clip: ClipKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClipKind {
    None,
    Axis,
    Rounded,
    /// A rounded clip with an axis-aligned band inside it (pushed in that
    /// order, popped in reverse).
    RoundedAxis,
}

impl<'a> Scope<'a> {
    fn new(canvas: &'a dyn Canvas, bg: Option<D2D1_COLOR_F>, clip: ClipKind) -> Self {
        if let Some(colour) = bg {
            canvas.push_bg(colour);
        }
        Self { canvas, bg: bg.is_some(), clip }
    }

    /// A scope that only announces `colour` as the parent surface.
    pub fn bg(canvas: &'a dyn Canvas, colour: D2D1_COLOR_F) -> Self {
        Self::new(canvas, Some(colour), ClipKind::None)
    }
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        match self.clip {
            ClipKind::None => {}
            ClipKind::Axis => self.canvas.pop_clip(),
            ClipKind::Rounded => self.canvas.pop_clip_rounded(),
            ClipKind::RoundedAxis => {
                self.canvas.pop_clip();
                self.canvas.pop_clip_rounded();
            }
        }
        if self.bg {
            self.canvas.pop_bg();
        }
    }
}

/// Runs `f` with `colour` announced as the parent surface — the closure form of
/// [`Scope::bg`].
pub fn with_bg<R>(canvas: &dyn Canvas, colour: D2D1_COLOR_F, f: impl FnOnce() -> R) -> R {
    let _scope = Scope::bg(canvas, colour);
    f()
}

/// The DIP a container's frame really takes on the inside of its box: the
/// replica's `BorderStyle` thickness, but never less than the one-DIP hairline
/// a stroked surface draws even with `BorderStyle::None`. Content clipped to the
/// box deflated by this never paints over the frame.
pub fn frame_inset(stroked: bool, border: f32) -> f32 {
    if stroked {
        border.max(card_metrics::RULE)
    } else {
        border
    }
}

/// `r` deflated by `d` on every side (never inverted).
fn deflate(r: Rect, d: f32) -> Rect {
    let l = r.left + d;
    let t = r.top + d;
    Rect::new(l, t, (r.right - d).max(l), (r.bottom - d).max(t))
}

/// The radius of a rounded rectangle inset by `d` inside one of radius `r` —
/// what keeps an inner clip concentric with the outer curve.
fn inner_radius(r: f32, d: f32) -> f32 {
    (r - d).max(0.0)
}

/// `c` at `alpha` × its own opacity — a token tinted the way Tailwind's `/40`
/// suffix does.
fn with_alpha(c: D2D1_COLOR_F, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: c.a * alpha, ..c }
}

// ─────────────────────────────────────────────────────────────────────────────
// Metrics this family needs and the shared table does not carry.
//
// `crate::metrics` re-exports the design system's tokens and adds the control
// metrics the web never had to describe. The three below are card *composition*
// numbers — where the header band ends, how tall a line of type is — which the
// web expresses as Tailwind classes rather than as tokens. They are stated here,
// with their provenance, rather than as literals in a paint body.
// ─────────────────────────────────────────────────────────────────────────────

/// A card header's title line. READ off `@ui/Card.tsx`, which sets the title at
/// `var(--kb-text-heading)` (16) with Tailwind's matching `leading-normal` — 20
/// DIP. Not measured live: the desktop has no browser to measure in.
const CARD_TITLE_LINE: f32 = 20.0;

/// A card header's subtitle line: `var(--kb-text-meta)` (12) at the same ratio,
/// preceded by the `mt-0.5` gap the component writes. Same source.
const CARD_SUBTITLE_LINE: f32 = 16.0;
const CARD_SUBTITLE_GAP: f32 = space::XXS;

// ─────────────────────────────────────────────────────────────────────────────
// Surface — what a Kubuno container paints itself as.
// ─────────────────────────────────────────────────────────────────────────────

/// The ground a container draws under its children.
///
/// This is the one concept .NET has no name for. WinForms answers « what colour
/// is my box » with `BackColor` + `BorderStyle`, which is a colour and a bevel;
/// Kubuno answers with a *surface* — a token fill, a token stroke and a radius
/// that belong together. `BorderStyle` still decides the border's **thickness**
/// (and therefore the geometry, through
/// [`kubuno_controls::containers::border_thickness`]); the surface decides its
/// **pixels**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Surface {
    /// Paints nothing. The default, and the common case: a panel that only
    /// *places* rectangles has no business putting a colour under them.
    #[default]
    None,
    /// The module ground: `layer_background`, `card_stroke`, `radius::TILE`.
    ///
    /// This is what the console's cards paint **today**
    /// (`shell/src/admin_storage.rs::card_frame`), kept as its own variant so a
    /// page can move onto [`Card`] without a single pixel changing.
    Layer,
    /// The Kubuno card: `rounded-xl border border-border bg-surface-0` of
    /// `@ui/Card.tsx` (and of `@ui/Accordion.tsx`) — `layer_background`
    /// (surface-0, #fff / #202124), `card_stroke`, `radius::XL`. A card is the
    /// WHITE block on the surface-1 page, not a grey one: `card_background` is
    /// surface-1, the page's own ground and the card footer's.
    Card,
    /// A [`Surface::Card`] lifted off the page with [`SHADOW_MENU`] — the web's
    /// elevation 2. `SHADOW_FLOAT` is deliberately not used: its outer layers
    /// reach 40 DIP and read as a grey cloud under a card-sized box.
    Raised,
    /// A recessed well — `surface_2`, no stroke, `radius::XL`. What a read-only
    /// block sunk into a card sits on.
    Well,
}

impl Surface {
    /// The corner radius this surface is drawn with.
    pub fn radius(self) -> f32 {
        match self {
            Surface::None => 0.0,
            Surface::Layer => radius::TILE,
            Surface::Card | Surface::Raised | Surface::Well => radius::XL,
        }
    }

    pub(crate) fn fill(self, c: &dyn Canvas) -> Option<D2D1_COLOR_F> {
        let t = c.theme();
        match self {
            Surface::None => None,
            Surface::Layer | Surface::Card | Surface::Raised => Some(t.layer_background),
            Surface::Well => Some(t.surface_2),
        }
    }

    /// The colour this surface fills with — what a control landing on it must
    /// read as `current_bg()`. `None` for [`Surface::None`], which is
    /// transparent (the web's default).
    pub fn ground(self, c: &dyn Canvas) -> Option<D2D1_COLOR_F> {
        self.fill(c)
    }

    /// Whether this surface draws a hairline frame.
    pub fn is_stroked(self) -> bool {
        !matches!(self, Surface::None | Surface::Well)
    }

    fn stroke(self, c: &dyn Canvas) -> Option<D2D1_COLOR_F> {
        self.is_stroked().then(|| c.theme().card_stroke)
    }

    /// Shadow then fill — everything but the frame, so a container can paint
    /// its content and THEN the frame on top of it.
    fn paint_ground(self, c: &dyn Canvas, bounds: Rect) {
        let r = self.radius();
        if self == Surface::Raised {
            c.draw_shadow(&bounds, r, &SHADOW_MENU, SHADOW_GREY);
        }
        if let Some(fill) = self.fill(c) {
            c.fill_rounded(&bounds, r, &fill);
        }
    }

    /// The hairline frame alone.
    fn paint_frame(self, c: &dyn Canvas, bounds: Rect, border: f32) {
        let r = self.radius();
        if let Some(stroke) = self.stroke(c) {
            if border > 1.0 {
                c.stroke_rounded_w(&bounds, r, &stroke, border);
            } else {
                c.stroke_rounded(&bounds, r, &stroke);
            }
        }
    }

    /// Paints the surface into `bounds` — shadow, fill, then stroke.
    ///
    /// `border` is the thickness [`border_thickness`] resolved from the
    /// replica's `BorderStyle`: a Kubuno border is a stroke, not the toolkit's
    /// two-tone bevel, but it costs exactly the DIP the replica's geometry
    /// reserved for it, so a `Fixed3D` panel's frame really is the 2 DIP its
    /// display rectangle gave up.
    fn paint(self, c: &dyn Canvas, bounds: Rect, border: f32) {
        // A container with no surface paints NOTHING, whatever its
        // `BorderStyle` says: the style is a geometry statement here (how much
        // room the frame takes), and the surface is the only thing that decides
        // whether a frame is drawn at all.
        if self == Surface::None {
            return;
        }
        self.paint_ground(c, bounds);
        self.paint_frame(c, bounds, border);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Child — one placed rectangle.
// ─────────────────────────────────────────────────────────────────────────────

/// One child of a container: what the layout engine needs, plus (optionally)
/// the primitive that paints into the rectangle it resolves.
///
/// `item` is [`kubuno_controls::layout::Item`] itself — the engine's own input
/// type, not a copy of it. `bounds`, `dock`, `anchor`, `visible`, `min` and
/// `max` therefore mean exactly what they mean to the toolkit, and a caller who
/// already holds an `Item` can hand it straight over.
pub struct Child {
    /// The engine's input: the child's design rectangle and its layout rules.
    pub item: Item,
    /// The primitive painted into the resolved rectangle, when the caller gave
    /// one. `None` means the container only **places** the rectangle and the
    /// caller paints it — which is the shape every shell page wants today.
    pub widget: Option<Box<dyn Widget>>,
}

impl Child {
    pub fn new(item: Item) -> Self {
        Self { item, widget: None }
    }
}

/// An [`Item`] with the engine's own defaults: visible, unconstrained, and
/// anchored top-left (`AnchorStyles::default()`, which is what WinForms gives a
/// freshly dropped control).
pub fn item(bounds: Rect, dock: DockStyle, anchor: AnchorStyles) -> Item {
    Item { bounds, dock, anchor, visible: true, min: Size::EMPTY, max: Size::EMPTY }
}

/// A docked band `thickness` DIP thick.
///
/// Only the docking axis is read by the engine — a `Top` band takes its own
/// height and the container's width — so the other extent is left at zero
/// rather than guessed. `Fill` reads neither.
pub fn band(dock: DockStyle, thickness: f32) -> Item {
    let bounds = match dock {
        DockStyle::Left | DockStyle::Right => Rect::new(0.0, 0.0, thickness, 0.0),
        _ => Rect::new(0.0, 0.0, 0.0, thickness),
    };
    item(bounds, dock, AnchorStyles::default())
}

// ─────────────────────────────────────────────────────────────────────────────
// Slots — the child list and its anchoring reference, shared by the containers
// that own children. Private: `Panel` and `GroupBox` expose it through their
// own API, so there is one implementation of « add a child, place the children »
// rather than two that can drift.
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct Slots {
    children: Vec<Child>,
    /// The display rect the children's `bounds` were authored against. See the
    /// module docs: this is the anchor pass' reference, and it is *not* « the
    /// last pass », which is what makes placement idempotent.
    design: Cell<Option<Rect>>,
}

impl Slots {
    fn push(&mut self, child: Child) -> usize {
        self.children.push(child);
        self.children.len() - 1
    }

    /// Places the children inside `display` (LOCAL space), delegating to the
    /// engine. Records the design reference on the first call.
    fn arrange(&self, display: Rect) -> Vec<Rect> {
        let design = match self.design.get() {
            Some(d) => d,
            None => {
                self.design.set(Some(display));
                display
            }
        };
        let items: Vec<Item> = self.children.iter().map(|c| c.item).collect();
        layout(display, design, &items)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Panel — the container that was missing.
// ─────────────────────────────────────────────────────────────────────────────

/// **The** Kubuno container: a box that holds children and places them.
///
/// It owns [`kubuno_controls::containers::Panel`], so `auto_scroll`,
/// `border_style`, `padding`, `dock`, `anchor`, `minimum_size` and the rest of
/// the .NET surface are the replica's fields, reached through [`Deref`] — this
/// type declares none of them. What it adds is the child list, the Kubuno
/// [`Surface`], and the two methods an application actually calls:
/// [`Panel::layout_children`] and [`Panel::child_rect`].
///
/// ## Replacing a page of hand-computed rectangles
///
/// ```ignore
/// // shell/src/admin_groups.rs, before: eight `Rect::new` and a running `y`.
/// let rects = Panel::new()
///     .with_padding(Padding::all(space::LG))
///     .top(HEADER_H)                       // title + intro, fixed
///     .top(TOOLBAR_H)                      // search field and actions
///     .bottom(STATUS_H)                    // the count line
///     .fill()                              // the list
///     .layout_children(body);
/// let (header, toolbar, status, list) = (rects[0], rects[1], rects[2], rects[3]);
/// ```
///
/// The bands stack in **reverse z-order**, exactly as the toolkit docks them:
/// the child added *last* takes the outermost edge. That is the engine's rule,
/// not a choice made here, and it is why a `Fill` child belongs at the back of
/// the list — see [`kubuno_controls::layout`].
#[derive(Default)]
pub struct Panel {
    inner: kc::Panel,
    /// How the box paints itself. The one concept the replica has no word for.
    pub surface: Surface,
    slots: Slots,
}

impl Panel {
    pub fn new() -> Self {
        Self::default()
    }

    /// A panel that paints `surface`.
    pub fn with_surface(mut self, surface: Surface) -> Self {
        self.surface = surface;
        self
    }

    /// Sets the replica's `Padding` — the inset every child is placed inside.
    pub fn with_padding(mut self, padding: Padding) -> Self {
        self.inner.padding = padding;
        self
    }

    /// Sets the replica's `BorderStyle`. It costs its thickness in *geometry*
    /// (`FixedSingle` 1 DIP, `Fixed3D` 2) whatever the [`Surface`] then paints.
    pub fn with_border(mut self, style: BorderStyle) -> Self {
        self.inner.border_style = style;
        self
    }

    /// Sets the replica's `AutoScroll`. See [`ScrollView`], which is this plus
    /// the ranges a scroll bar needs.
    pub fn with_auto_scroll(mut self, on: bool) -> Self {
        self.inner.auto_scroll = on;
        self
    }

    /// States the size the children's rectangles were authored at, so anchoring
    /// has a reference before the first layout.
    ///
    /// Without it the reference is whatever size the panel is first laid out at,
    /// which is right for a panel built at runtime and wrong for one whose
    /// children carry design-time coordinates.
    pub fn with_design_size(mut self, size: Size) -> Self {
        self.set_design_size(size);
        self
    }

    /// See [`Panel::with_design_size`].
    pub fn set_design_size(&mut self, size: Size) {
        let at = self.display_rect(Rect::new(0.0, 0.0, size.width, size.height));
        self.slots.design.set(Some(at));
    }

    // ── Adding children ──────────────────────────────────────────────────

    /// Adds a child and returns its index — the index its rectangle has in
    /// [`Panel::layout_children`].
    pub fn push(&mut self, item: Item) -> usize {
        self.slots.push(Child::new(item))
    }

    /// Adds a child that carries the primitive painting it.
    pub fn push_widget(&mut self, item: Item, widget: Box<dyn Widget>) -> usize {
        self.slots.push(Child { item, widget: Some(widget) })
    }

    /// Adds a raw [`Item`] — for a caller that already built one.
    pub fn item(mut self, item: Item) -> Self {
        self.push(item);
        self
    }

    /// Docks a band `thickness` tall against the top edge.
    pub fn top(self, thickness: f32) -> Self {
        self.item(band(DockStyle::Top, thickness))
    }

    /// Docks a band `thickness` tall against the bottom edge.
    pub fn bottom(self, thickness: f32) -> Self {
        self.item(band(DockStyle::Bottom, thickness))
    }

    /// Docks a column `thickness` wide against the left edge.
    pub fn left(self, thickness: f32) -> Self {
        self.item(band(DockStyle::Left, thickness))
    }

    /// Docks a column `thickness` wide against the right edge.
    pub fn right(self, thickness: f32) -> Self {
        self.item(band(DockStyle::Right, thickness))
    }

    /// Takes everything the bands docked *after* it left.
    ///
    /// Add it **first** (lowest z) when you want the remainder, which is what a
    /// content area wants: the engine resolves `Fill` at its own turn in the
    /// reverse walk and it consumes nothing, so a `Fill` added last takes the
    /// whole display rectangle and the bands overlap it. That is the toolkit's
    /// behaviour, measured, not a limitation of this wrapper.
    pub fn fill(self) -> Self {
        self.item(band(DockStyle::Fill, 0.0))
    }

    /// A child at a design rectangle, keeping its distance to the edges it is
    /// anchored to. Anchoring two opposite edges stretches it.
    pub fn anchored(self, bounds: Rect, anchor: AnchorStyles) -> Self {
        self.item(item(bounds, DockStyle::None, anchor))
    }

    /// A child pinned to the top-left at a design rectangle — `Anchor` at its
    /// toolkit default, so the child neither moves nor stretches.
    pub fn fixed(self, bounds: Rect) -> Self {
        self.anchored(bounds, AnchorStyles::default())
    }

    /// Attaches a primitive to the **last** child added, so the panel paints it
    /// into the rectangle the engine resolves.
    ///
    /// ```ignore
    /// Panel::new().top(56.0).painting(header).fill().painting(body)
    /// ```
    pub fn painting(mut self, widget: impl Widget + 'static) -> Self {
        if let Some(last) = self.slots.children.last_mut() {
            last.widget = Some(Box::new(widget));
        }
        self
    }

    /// Applies `f` to the last child added — for the constraints the builders do
    /// not spell (`min`, `max`, `visible`).
    ///
    /// ```ignore
    /// Panel::new().left(240.0).constrain(|i| i.min = Size::new(180.0, 0.0))
    /// ```
    pub fn constrain(mut self, f: impl FnOnce(&mut Item)) -> Self {
        if let Some(last) = self.slots.children.last_mut() {
            f(&mut last.item);
        }
        self
    }

    /// The children, in the order they were added — the order their rectangles
    /// come back in.
    pub fn children(&self) -> &[Child] {
        &self.slots.children
    }

    pub fn child_mut(&mut self, i: usize) -> Option<&mut Child> {
        self.slots.children.get_mut(i)
    }

    pub fn len(&self) -> usize {
        self.slots.children.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.children.is_empty()
    }

    pub fn clear(&mut self) {
        self.slots.children.clear();
    }

    // ── Placing them ─────────────────────────────────────────────────────

    /// **LOCAL space.** The children's `DisplayRectangle` for a panel occupying
    /// `bounds`: deflated by the padding and by the border, its origin carrying
    /// the padding inset — which is what makes a docked child (placed against
    /// this rectangle) and an anchored child (carrying client coordinates) live
    /// in one space.
    ///
    /// Derived from the `bounds` **argument**, never from the model's own
    /// rectangle: a container paints and places where it was put, not where it
    /// thinks it is. The arithmetic itself is
    /// [`kubuno_controls::containers::Panel::local_display_rect`] — the replica
    /// is handed the box it was asked about and answers.
    pub fn display_rect(&self, bounds: Rect) -> Rect {
        self.sized(bounds).local_display_rect()
    }

    /// **CANVAS space.** Where this panel's client origin — the `(0, 0)` its
    /// children's rectangles are measured from — lands, given the box it is
    /// painted into. [`kubuno_controls::containers::content_origin`] is the one
    /// place local space becomes canvas space, and this is the only call to it.
    pub fn content_origin(&self, bounds: Rect) -> Point {
        content_origin(bounds, self.border(), self.inner.auto_scroll_position)
    }

    /// **CANVAS space.** The client rectangle children are clipped to.
    pub fn client_rect(&self, bounds: Rect) -> Rect {
        client_rect_on_canvas(bounds, self.inner.control(), self.border())
    }

    /// **LOCAL space.** The children's rectangles, straight out of
    /// [`kubuno_controls::layout::layout`].
    ///
    /// Use this when the rectangles are going to be stored on the children (the
    /// toolkit's own convention). Most callers want
    /// [`Panel::layout_children`], which is this translated onto the canvas.
    pub fn layout_children_local(&self, bounds: Rect) -> Vec<Rect> {
        self.slots.arrange(self.display_rect(bounds))
    }

    /// **CANVAS space.** The rectangle for each child, in the order they were
    /// added — the call that replaces a page of `Rect::new`.
    ///
    /// Dock is resolved first (in reverse z-order, each band eating its edge),
    /// then Anchor against the design size. Both passes are the engine's; this
    /// method only crosses into canvas space.
    pub fn layout_children(&self, bounds: Rect) -> Vec<Rect> {
        let origin = self.content_origin(bounds);
        self.layout_children_local(bounds)
            .into_iter()
            .map(|r| translate(r, origin.x, origin.y))
            .collect()
    }

    /// **CANVAS space.** One child's rectangle. Convenient when a page needs two
    /// of six and would otherwise index into a `Vec` by a magic number.
    pub fn child_rect(&self, bounds: Rect, i: usize) -> Option<Rect> {
        self.layout_children(bounds).into_iter().nth(i)
    }

    /// The topmost visible child under a canvas-space point, or `None`.
    ///
    /// Walks in reverse order, so the child painted last — and therefore on top
    /// — answers first, which is [`Children::child_at`]'s rule.
    pub fn child_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        let rects = self.layout_children(bounds);
        self.slots
            .children
            .iter()
            .enumerate()
            .rev()
            .find(|(i, ch)| ch.item.visible && rects[*i].contains(x, y))
            .map(|(i, _)| i)
    }

    /// The virtual content size for a panel occupying `bounds`: how far the
    /// children reach, grown by `AutoScrollMargin` and floored by
    /// `AutoScrollMinSize`.
    ///
    /// Computed by the replica —
    /// [`kubuno_controls::containers::ScrollableControl::scroll_content_size_within`]
    /// — over stand-in children carrying the rectangles this panel resolved, so
    /// the rule is the toolkit's and not a second copy of it.
    pub fn content_size(&self, bounds: Rect) -> Size {
        let p = self.populated(bounds);
        p.scroll_content_size_within(p.local_display_rect())
    }

    /// The border thickness the replica's `BorderStyle` reserves.
    fn border(&self) -> f32 {
        border_thickness(self.inner.border_style)
    }

    /// The replica, told how big the box it is being asked about is — and
    /// nothing else. `local_display_rect` reads only the size and the padding,
    /// so this is the honest way to ask the replica a question about a rectangle
    /// it does not own.
    fn sized(&self, bounds: Rect) -> kc::Panel {
        let mut p = self.inner.clone();
        p.children = Children::new();
        p.control_mut().bounds =
            Rect::new(0.0, 0.0, bounds.right - bounds.left, bounds.bottom - bounds.top);
        p
    }

    /// [`Panel::sized`] plus a stand-in child per real child, carrying the
    /// rectangle this panel resolved for it. Only the queries that must read the
    /// child collection (`content_size`) build it.
    fn populated(&self, bounds: Rect) -> kc::Panel {
        let mut p = self.sized(bounds);
        let placed = self.layout_children_local(bounds);
        fill_children(&mut p.children, self.slots.children.iter().map(|c| c.item).zip(placed));
        p
    }

    /// The replica, unsized, holding one stand-in per child at the rectangle
    /// given — what a question about the *content* rather than the box is asked
    /// of.
    fn stand_ins(&self, items: impl Iterator<Item = Item>) -> kc::Panel {
        let mut p = self.inner.clone();
        p.children = Children::new();
        fill_children(&mut p.children, items.map(|i| (i, i.bounds)));
        p
    }

    /// **CANVAS space.** The box inside the frame: `bounds` deflated by
    /// [`frame_inset`]. What content is clipped to — NOT the padded client
    /// rectangle, because the web clips nothing at the padding (`overflow:
    /// visible`), so a child's ink that strays into the padding (a focus ring,
    /// an outlined field's floated label, a shadow) must survive.
    pub fn border_box(&self, bounds: Rect) -> Rect {
        deflate(bounds, frame_inset(self.surface.is_stroked(), self.border()))
    }

    /// The corner radius of [`Panel::border_box`] — concentric with the frame.
    pub fn border_box_radius(&self) -> f32 {
        inner_radius(self.surface.radius(), frame_inset(self.surface.is_stroked(), self.border()))
    }

    /// Announces this panel's surface as the parent background until the
    /// returned scope is dropped — so the controls a caller paints into
    /// [`Panel::client_rect`] after [`Widget::paint`] read the panel's ground as
    /// `current_bg()`, not the window's. A no-op scope for [`Surface::None`]
    /// (transparent: the caller's background stays the right one).
    pub fn push_surface<'a>(&self, canvas: &'a dyn Canvas) -> Scope<'a> {
        Scope::new(canvas, self.surface.fill(canvas), ClipKind::None)
    }

    /// Paints the panel, then runs `content` with the panel's surface pushed
    /// and the drawing clipped to [`Panel::border_box`] (rounded like the
    /// frame) when the panel has a visible surface. `content` receives the
    /// padded client rectangle — the area a caller lays controls out in.
    ///
    /// The frame is stroked AFTER `content`, so nothing a caller paints can
    /// cover it.
    pub fn paint_with(
        &self,
        canvas: &dyn Canvas,
        bounds: Rect,
        state: WidgetState,
        content: impl FnOnce(&dyn Canvas, Rect),
    ) {
        self.surface.paint_ground(canvas, bounds);
        {
            let _scope = self.content_scope(canvas, bounds, None);
            self.paint_children_unclipped(canvas, bounds, state);
            content(canvas, self.client_rect(bounds));
        }
        self.surface.paint_frame(canvas, bounds, self.border());
    }

    /// The background + clip scope content is painted under: the surface's
    /// ground pushed, the border box clipped (rounded) when the surface is
    /// visible, and optionally an axis band inside it (a card's body between
    /// its header and footer).
    fn content_scope<'a>(&self, canvas: &'a dyn Canvas, bounds: Rect, band: Option<Rect>) -> Scope<'a> {
        let bg = self.surface.fill(canvas);
        if self.surface == Surface::None {
            if let Some(b) = band {
                canvas.push_clip(&b);
                return Scope::new(canvas, bg, ClipKind::Axis);
            }
            return Scope::new(canvas, bg, ClipKind::None);
        }
        canvas.push_clip_rounded(&self.border_box(bounds), self.border_box_radius());
        match band {
            Some(b) => {
                canvas.push_clip(&b);
                Scope::new(canvas, bg, ClipKind::RoundedAxis)
            }
            None => Scope::new(canvas, bg, ClipKind::Rounded),
        }
    }

    /// Paints any child that carries a primitive, clipped to the border box.
    ///
    /// Factored out of [`Widget::paint`] so [`Card`] can put its header band
    /// between the ground and the children.
    fn paint_children(&self, c: &dyn Canvas, bounds: Rect, state: WidgetState) {
        if self.slots.children.iter().all(|ch| ch.widget.is_none()) {
            return;
        }
        let _scope = self.content_scope(c, bounds, None);
        self.paint_children_unclipped(c, bounds, state);
    }

    /// The children alone, under whatever clip the caller set.
    fn paint_children_unclipped(&self, c: &dyn Canvas, bounds: Rect, state: WidgetState) {
        if self.slots.children.iter().all(|ch| ch.widget.is_none()) {
            return;
        }
        for (child, r) in self.slots.children.iter().zip(self.layout_children(bounds)) {
            if !child.item.visible {
                continue;
            }
            if let Some(w) = child.widget.as_ref() {
                // Only `disabled` is inherited: a container greys its whole
                // subtree, but the pointer is over ONE child and the container
                // does not know which — the caller that tracks hover passes it
                // to the child itself.
                w.paint(c, r, WidgetState::REST.disabled(state.disabled));
            }
        }
    }
}

impl Deref for Panel {
    type Target = kc::Panel;
    fn deref(&self) -> &kc::Panel {
        &self.inner
    }
}

impl DerefMut for Panel {
    fn deref_mut(&mut self) -> &mut kc::Panel {
        &mut self.inner
    }
}

impl Widget for Panel {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    /// What the panel needs to hold its children: the content extent plus the
    /// padding and the two border lines.
    ///
    /// Measured over the children's **design** rectangles, not their laid-out
    /// ones — asking a panel how big it wants to be before it has a box is
    /// asking how big its content is, and the laid-out rectangles do not exist
    /// yet. The sum itself is the replica's own `preferred_size`.
    fn measure(&self, canvas: &dyn Canvas) -> Size {
        self.stand_ins(self.slots.children.iter().map(|c| c.item)).preferred_size(canvas)
    }

    /// Exception to the opaque-ground rule, deliberately: a container is
    /// transparent unless its [`Surface`] paints (the web's default). Filling
    /// the box with `current_bg()` first painted SQUARE corners of the caller's
    /// colour around a rounded surface whenever the caller had not pushed its
    /// own ground.
    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.surface.paint_ground(canvas, bounds);
        // Children inherit our surface (the scope pushes it): if this panel
        // painted a card, they read `current_bg()` = the card. `Surface::None`
        // falls through to the caller's background.
        self.paint_children(canvas, bounds, state);
        self.surface.paint_frame(canvas, bounds, self.border());
    }

    fn type_name(&self) -> &'static str {
        "Panel"
    }
}

/// Gives a container built **on** [`Panel`] the panel's own builder surface.
///
/// The builders consume `self` — which is what makes the chain read the way it
/// does — and a consuming method cannot be reached through [`Deref`], because
/// deref hands out a reference and a builder needs the value. Rather than write
/// the same twelve methods twice, they are forwarded here: one definition, in
/// `Panel`, reachable from every type that wraps one.
macro_rules! panel_builders {
    ($ty:ident) => {
        impl $ty {
            /// See [`Panel::item`].
            pub fn item(mut self, item: Item) -> Self {
                self.panel = std::mem::take(&mut self.panel).item(item);
                self
            }

            /// See [`Panel::top`].
            pub fn top(self, thickness: f32) -> Self {
                self.item(band(DockStyle::Top, thickness))
            }

            /// See [`Panel::bottom`].
            pub fn bottom(self, thickness: f32) -> Self {
                self.item(band(DockStyle::Bottom, thickness))
            }

            /// See [`Panel::left`].
            pub fn left(self, thickness: f32) -> Self {
                self.item(band(DockStyle::Left, thickness))
            }

            /// See [`Panel::right`].
            pub fn right(self, thickness: f32) -> Self {
                self.item(band(DockStyle::Right, thickness))
            }

            /// See [`Panel::fill`].
            pub fn fill(self) -> Self {
                self.item(band(DockStyle::Fill, 0.0))
            }

            /// See [`Panel::anchored`].
            pub fn anchored(self, bounds: Rect, anchor: AnchorStyles) -> Self {
                self.item(item(bounds, DockStyle::None, anchor))
            }

            /// See [`Panel::fixed`].
            pub fn fixed(self, bounds: Rect) -> Self {
                self.anchored(bounds, AnchorStyles::default())
            }

            /// See [`Panel::painting`].
            pub fn painting(mut self, widget: impl Widget + 'static) -> Self {
                self.panel = std::mem::take(&mut self.panel).painting(widget);
                self
            }

            /// See [`Panel::constrain`].
            pub fn constrain(mut self, f: impl FnOnce(&mut Item)) -> Self {
                self.panel = std::mem::take(&mut self.panel).constrain(f);
                self
            }

            /// See [`Panel::with_design_size`].
            pub fn with_design_size(mut self, size: Size) -> Self {
                self.panel.set_design_size(size);
                self
            }
        }
    };
}

// ─────────────────────────────────────────────────────────────────────────────
// Card — the Kubuno surface, built ON Panel.
// ─────────────────────────────────────────────────────────────────────────────

/// A titled block of content on the Kubuno card surface.
///
/// Built **on** [`Panel`], not beside it: a card is a panel that paints
/// [`Surface::Card`] and reserves a header band, so everything a panel can do —
/// dock a toolbar, anchor a field, place a list — a card can do, inside its
/// body.
///
/// The composition is `@ui/Card.tsx`, READ (the desktop has no browser to
/// measure in): `min-w-0 rounded-xl border border-border bg-surface-0`; a header
/// row `flex items-start gap-3 border-b border-border px-4 py-3` holding an
/// optional leading icon (`mt-0.5 text-text-secondary`), the title block
/// (`truncate font-medium` at `--kb-text-heading`, the subtitle `mt-0.5
/// text-text-secondary` at `--kb-text-meta`) and an optional actions cluster
/// (`flex shrink-0 items-center gap-1.5`); a body padded `p-4`; an optional
/// footer `border-t border-border bg-surface-1 rounded-b-xl px-4 py-3`.
/// `dense` tightens the header/footer to `px-3 py-2.5`, the body to `p-3` and
/// the title to the body size; `flush` drops the body padding so a table or a
/// list bleeds to the card's edges. See [`card_metrics`].
///
/// The header and footer bands, the body padding and the 1-DIP `border` are
/// all folded into the replica's **`Padding`** / `BorderStyle`, which is why
/// [`Panel::layout_children`] needs no override: a child docked `Top` inside a
/// card lands under the title, because the display rectangle already starts
/// there.
///
/// ## Painting controls into a card
///
/// The controls a caller paints into [`Card::body_rect`] must read the card's
/// ground as `current_bg()`. Either paint them inside
/// [`Card::paint_body`] (which also clips them to the card and strokes the frame
/// last), or hold [`Card::push_body`]'s scope while painting them.
///
/// ## Deviation
///
/// The web subtitle is a wrapping `<p>`; here it is one line with an ellipsis,
/// because the header's height must be known without a canvas (it is folded
/// into the layout padding).
pub struct Card {
    panel: Panel,
    title: String,
    subtitle: String,
    dense: bool,
    flush: bool,
    icon: Option<&'static str>,
    /// The actions cluster's size, `(width, height)`, when the caller reserves
    /// one. The card places it; the caller paints into [`Card::actions_rect`].
    actions: Option<(f32, f32)>,
    /// The footer's CONTENT height (without its padding and rule), when the card
    /// has a footer.
    footer: Option<f32>,
}

impl Default for Card {
    fn default() -> Self {
        let mut card = Self {
            panel: Panel::new().with_surface(Surface::Card).with_border(BorderStyle::FixedSingle),
            title: String::new(),
            subtitle: String::new(),
            dense: false,
            flush: false,
            icon: None,
            actions: None,
            footer: None,
        };
        card.sync_padding();
        card
    }
}

impl Card {
    pub fn new() -> Self {
        Self::default()
    }

    /// A card with a title.
    pub fn titled(title: impl Into<String>) -> Self {
        let mut card = Self::new();
        card.set_title(title);
        card
    }

    /// The secondary line under the title (`@ui/Card.tsx`'s `subtitle`).
    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = subtitle.into();
        self.sync_padding();
        self
    }

    /// `dense`: tighter header, footer and body padding and a body-size title,
    /// for cards stacked in a dense settings column.
    pub fn dense(mut self) -> Self {
        self.dense = true;
        self.sync_padding();
        self
    }

    /// `flush`: no body padding, for a table or a list that must bleed to the
    /// card's edges. The body is still clipped to the card's rounded frame.
    pub fn flush(mut self) -> Self {
        self.flush = true;
        self.sync_padding();
        self
    }

    /// A leading glyph before the title (`icon`), in `text_secondary`.
    pub fn with_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self.sync_padding();
        self
    }

    /// Reserves the header's trailing actions cluster (`actions`), `width` ×
    /// `height` DIP. The card places it ([`Card::actions_rect`]) and narrows the
    /// title; the caller paints its buttons there. A cluster taller than the
    /// title line grows the header, as the web's `items-start` row does.
    pub fn with_actions(mut self, width: f32, height: f32) -> Self {
        self.actions = Some((width.max(0.0), height.max(0.0)));
        self.sync_padding();
        self
    }

    /// Adds the bottom band (`footer`): a hairline, the surface-1 ground and
    /// the header's padding around `content_height` DIP of the caller's content
    /// ([`Card::footer_body_rect`]).
    pub fn with_footer(mut self, content_height: f32) -> Self {
        self.footer = Some(content_height.max(0.0));
        self.sync_padding();
        self
    }

    /// Lifts the card off the page — [`Surface::Raised`].
    pub fn raised(mut self) -> Self {
        self.panel.surface = Surface::Raised;
        self
    }

    /// The console's current card look — `layer_background` at `radius::TILE`,
    /// which is what `admin_storage::card_frame` paints today. Offered so a page
    /// can adopt this type without changing a pixel, then move to the Kubuno
    /// card surface as its own change.
    pub fn on_layer(mut self) -> Self {
        self.panel.surface = Surface::Layer;
        self
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
        self.sync_padding();
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn subtitle(&self) -> &str {
        &self.subtitle
    }

    pub fn is_dense(&self) -> bool {
        self.dense
    }

    pub fn is_flush(&self) -> bool {
        self.flush
    }

    /// Whether the card renders a header band — `title != null || icon !=
    /// null || actions != null` in `@ui/Card.tsx`.
    pub fn has_header(&self) -> bool {
        !self.title.is_empty() || self.icon.is_some() || self.actions.is_some()
    }

    fn pad_x(&self) -> f32 {
        if self.dense {
            card_metrics::DENSE_PAD_X
        } else {
            card_metrics::PAD_X
        }
    }

    fn pad_y(&self) -> f32 {
        if self.dense {
            card_metrics::DENSE_PAD_Y
        } else {
            card_metrics::PAD_Y
        }
    }

    fn body_pad(&self) -> f32 {
        if self.flush {
            0.0
        } else if self.dense {
            card_metrics::DENSE_BODY_PAD
        } else {
            card_metrics::BODY_PAD
        }
    }

    fn title_line(&self) -> f32 {
        if self.dense {
            card_metrics::DENSE_TITLE_LINE
        } else {
            CARD_TITLE_LINE
        }
    }

    /// The height of the header row's content: the tallest of the title block,
    /// the icon and the actions cluster (`items-start`).
    fn header_content(&self) -> f32 {
        let mut block = if self.title.is_empty() { 0.0 } else { self.title_line() };
        if !self.subtitle.is_empty() {
            block += CARD_SUBTITLE_GAP + CARD_SUBTITLE_LINE;
        }
        let icon = if self.icon.is_some() { card_metrics::ICON_TOP + card_metrics::ICON } else { 0.0 };
        let actions = self.actions.map_or(0.0, |(_, h)| h);
        block.max(icon).max(actions)
    }

    /// The height the header band takes — its padding, its content and the
    /// `border-b` hairline that closes it — or `0.0` when the card has no
    /// header: `@ui/Card.tsx` renders no header at all in that case, rather
    /// than an empty one.
    pub fn header_height(&self) -> f32 {
        if !self.has_header() {
            return 0.0;
        }
        self.pad_y() * 2.0 + self.header_content() + card_metrics::RULE
    }

    /// The height of the footer band (its `border-t`, padding and content), or
    /// `0.0` without a footer.
    pub fn footer_height(&self) -> f32 {
        self.footer.map_or(0.0, |h| card_metrics::RULE + self.pad_y() * 2.0 + h)
    }

    /// **CANVAS space.** Inside the card's own 1-DIP border.
    fn inner(&self, bounds: Rect) -> Rect {
        deflate(bounds, self.panel.border())
    }

    /// **CANVAS space.** The header band (its closing hairline included), or
    /// `None` when there is no header.
    pub fn header_rect(&self, bounds: Rect) -> Option<Rect> {
        let h = self.header_height();
        let i = self.inner(bounds);
        (h > 0.0).then(|| Rect::new(i.left, i.top, i.right, (i.top + h).min(i.bottom)))
    }

    /// **CANVAS space.** The footer band (its top hairline included), or
    /// `None` when there is no footer.
    pub fn footer_rect(&self, bounds: Rect) -> Option<Rect> {
        let h = self.footer_height();
        let i = self.inner(bounds);
        (h > 0.0).then(|| Rect::new(i.left, (i.bottom - h).max(i.top), i.right, i.bottom))
    }

    /// **CANVAS space.** Where the caller paints the footer's content: the band
    /// minus its hairline and its `px-4 py-3` padding.
    pub fn footer_body_rect(&self, bounds: Rect) -> Option<Rect> {
        let (px, py) = (self.pad_x(), self.pad_y());
        self.footer_rect(bounds).map(|f| {
            let l = f.left + px;
            let t = f.top + card_metrics::RULE + py;
            Rect::new(l, t, (f.right - px).max(l), (f.bottom - py).max(t))
        })
    }

    /// **CANVAS space.** The leading icon's box, when the card has one.
    pub fn icon_rect(&self, bounds: Rect) -> Option<Rect> {
        let header = self.header_rect(bounds)?;
        self.icon?;
        let x = header.left + self.pad_x();
        let y = header.top + self.pad_y() + card_metrics::ICON_TOP;
        Some(Rect::new(x, y, x + card_metrics::ICON, y + card_metrics::ICON))
    }

    /// **CANVAS space.** The actions cluster, when one was reserved with
    /// [`Card::with_actions`] — right-aligned in the header row.
    pub fn actions_rect(&self, bounds: Rect) -> Option<Rect> {
        let header = self.header_rect(bounds)?;
        let (w, h) = self.actions?;
        let right = header.right - self.pad_x();
        let top = header.top + self.pad_y();
        Some(Rect::new((right - w).max(header.left), top, right, top + h))
    }

    /// **CANVAS space.** The title line — what is left of the header row once
    /// the icon and the actions took their room, with the row's `gap-3`.
    pub fn title_rect(&self, bounds: Rect) -> Option<Rect> {
        let header = self.header_rect(bounds)?;
        let mut left = header.left + self.pad_x();
        if let Some(i) = self.icon_rect(bounds) {
            left = i.right + card_metrics::HEADER_GAP;
        }
        let mut right = header.right - self.pad_x();
        if let Some(a) = self.actions_rect(bounds) {
            right = a.left - card_metrics::HEADER_GAP;
        }
        let top = header.top + self.pad_y();
        Some(Rect::new(left, top, right.max(left), top + self.title_line()))
    }

    /// **CANVAS space.** The padded body — the rectangle a caller draws into,
    /// and the one [`Panel::layout_children`] places children inside.
    pub fn body_rect(&self, bounds: Rect) -> Rect {
        self.panel.client_rect(bounds)
    }

    /// **CANVAS space.** The band between the header and the footer, inside the
    /// frame — what the body's content is clipped to. Its padding is part of
    /// it: the web clips nothing at the padding, so a focus ring or a floated
    /// label near the body's edge survives.
    pub fn body_band(&self, bounds: Rect) -> Rect {
        let i = self.panel.border_box(bounds);
        let top = (i.top + self.header_height()).min(i.bottom);
        let bottom = (i.bottom - self.footer_height()).max(top);
        Rect::new(i.left, top, i.right, bottom)
    }

    /// Announces the card's ground and clips to [`Card::body_band`] (rounded
    /// with the frame) until the returned scope is dropped. Hold it while
    /// painting controls into [`Card::body_rect`] after [`Widget::paint`].
    pub fn push_body<'a>(&self, canvas: &'a dyn Canvas, bounds: Rect) -> Scope<'a> {
        self.panel.content_scope(canvas, bounds, Some(self.body_band(bounds)))
    }

    /// Announces the footer's surface-1 ground and clips to the footer band
    /// until the scope is dropped. A no-op scope without a footer.
    pub fn push_footer<'a>(&self, canvas: &'a dyn Canvas, bounds: Rect) -> Scope<'a> {
        match self.footer_rect(bounds) {
            Some(f) => {
                canvas.push_clip_rounded(&self.panel.border_box(bounds), self.panel.border_box_radius());
                canvas.push_clip(&f);
                Scope::new(canvas, Some(canvas.theme().card_background), ClipKind::RoundedAxis)
            }
            None => Scope::new(canvas, None, ClipKind::None),
        }
    }

    /// Paints the card, then runs `body` with the card's ground pushed and the
    /// drawing clipped to the body band; the frame is stroked last, so nothing
    /// `body` paints can cover it. `body` receives [`Card::body_rect`].
    pub fn paint_body(
        &self,
        canvas: &dyn Canvas,
        bounds: Rect,
        state: WidgetState,
        body: impl FnOnce(&dyn Canvas, Rect),
    ) {
        self.paint_chrome(canvas, bounds, state);
        {
            let _scope = self.push_body(canvas, bounds);
            self.panel.paint_children_unclipped(canvas, bounds, state);
            body(canvas, self.body_rect(bounds));
        }
        self.panel.surface.paint_frame(canvas, bounds, self.panel.border());
    }

    /// Folds the header and footer bands and the body padding into the
    /// replica's `Padding`, so the inherited placement needs no special case.
    /// The 1-DIP border is the replica's `BorderStyle::FixedSingle`.
    fn sync_padding(&mut self) {
        let pad = self.body_pad();
        self.panel.inner.padding = Padding::new(
            pad,
            self.header_height() + pad,
            pad,
            self.footer_height() + pad,
        );
    }

    /// Ground, header and footer — everything but the body and the frame.
    fn paint_chrome(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        let f = canvas.formats();
        self.panel.surface.paint_ground(canvas, bounds);

        if let Some(header) = self.header_rect(bounds) {
            let _bg = self.panel.push_surface(canvas);
            let primary = if state.disabled { t.text_tertiary } else { t.text_primary };
            if let (Some(name), Some(r)) = (self.icon, self.icon_rect(bounds)) {
                canvas.vector_icon(name, &r, card_metrics::ICON, &t.text_secondary);
            }
            if let Some(title) = self.title_rect(bounds) {
                if !self.title.is_empty() {
                    let format = if self.dense { &f.body_strong } else { &f.heading };
                    canvas.text_ellipsis(&self.title, &title, format, &primary);
                }
                if !self.subtitle.is_empty() {
                    let top = if self.title.is_empty() { title.top } else { title.bottom + CARD_SUBTITLE_GAP };
                    let sub = Rect::new(title.left, top, title.right, top + CARD_SUBTITLE_LINE);
                    canvas.text_ellipsis(&self.subtitle, &sub, &f.caption, &t.text_secondary);
                }
            }
            // The hairline that closes the band — `border-b border-border`.
            let rule = Rect::new(header.left, header.bottom - card_metrics::RULE, header.right, header.bottom);
            canvas.fill_rounded(&rule, 0.0, &t.card_stroke);
        }

        if let Some(footer) = self.footer_rect(bounds) {
            // `bg-surface-1 rounded-b-xl`: the band's fill, rounded at the
            // bottom with the card, then its `border-t`.
            canvas.push_clip_rounded(&self.panel.border_box(bounds), self.panel.border_box_radius());
            canvas.fill_rounded(&footer, 0.0, &t.card_background);
            canvas.pop_clip_rounded();
            let rule = Rect::new(footer.left, footer.top, footer.right, footer.top + card_metrics::RULE);
            canvas.fill_rounded(&rule, 0.0, &t.card_stroke);
        }
    }
}

panel_builders!(Card);

impl Deref for Card {
    type Target = Panel;
    fn deref(&self) -> &Panel {
        &self.panel
    }
}

impl DerefMut for Card {
    fn deref_mut(&mut self) -> &mut Panel {
        &mut self.panel
    }
}

impl Widget for Card {
    fn model(&self) -> &dyn Control {
        self.panel.model()
    }

    /// The panel's own measurement (children + padding + border), widened to
    /// the header row's intrinsic width — icon, title, actions and their gaps —
    /// which is what the web's card would size to in a shrink-to-fit context.
    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let base = self.panel.measure(canvas);
        if !self.has_header() {
            return base;
        }
        let f = canvas.formats();
        let format = if self.dense { &f.body_strong } else { &f.heading };
        let mut row = canvas.measure(&self.title, format).max(canvas.measure(&self.subtitle, &f.caption));
        if self.icon.is_some() {
            row += card_metrics::ICON + card_metrics::HEADER_GAP;
        }
        if let Some((w, _)) = self.actions {
            row += w + card_metrics::HEADER_GAP;
        }
        let width = row + self.pad_x() * 2.0 + self.panel.border() * 2.0;
        Size::new(base.width.max(width.ceil()), base.height)
    }

    /// Transparent outside its rounded surface (see [`Panel`]'s paint): no
    /// square `current_bg()` block behind the corners.
    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.paint_chrome(canvas, bounds, state);
        {
            let _scope = self.push_body(canvas, bounds);
            self.panel.paint_children_unclipped(canvas, bounds, state);
        }
        self.panel.surface.paint_frame(canvas, bounds, self.panel.border());
    }

    fn type_name(&self) -> &'static str {
        "Card"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GroupBox — the captioned frame, on the replica.
// ─────────────────────────────────────────────────────────────────────────────

/// A captioned frame around a group of related controls.
///
/// Owns [`kubuno_controls::containers::GroupBox`], which is where the geometry
/// that makes a group box a group box lives: the caption band at the top, the
/// asymmetric display rectangle underneath, the preferred size that accounts for
/// a caption wider than the content. Kubuno replaces only the pixels — a single
/// rounded hairline in `card_stroke` instead of the toolkit's engraved
/// `BP_GROUPBOX` groove, and the caption in `text_secondary` at the meta size,
/// which is how the web labels a `<fieldset>`-shaped block.
///
/// No predecessor and no `@ui` component: the caption inset comes from
/// [`crate::metrics::control::GROUP_LABEL_INSET`], the crate's table.
#[derive(Default)]
pub struct GroupBox {
    inner: kc::GroupBox,
    slots: Slots,
}

impl GroupBox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn titled(text: impl Into<String>) -> Self {
        let mut g = Self::new();
        g.inner.text = text.into();
        g
    }

    pub fn with_padding(mut self, padding: Padding) -> Self {
        self.inner.padding = padding;
        self
    }

    pub fn push(&mut self, item: Item) -> usize {
        self.slots.push(Child::new(item))
    }

    pub fn item(mut self, item: Item) -> Self {
        self.push(item);
        self
    }

    pub fn top(self, thickness: f32) -> Self {
        self.item(band(DockStyle::Top, thickness))
    }

    pub fn fill(self) -> Self {
        self.item(band(DockStyle::Fill, 0.0))
    }

    pub fn anchored(self, bounds: Rect, anchor: AnchorStyles) -> Self {
        self.item(item(bounds, DockStyle::None, anchor))
    }

    pub fn children(&self) -> &[Child] {
        &self.slots.children
    }

    /// **LOCAL space.** The replica's own `DisplayRectangle` — 1 DIP of frame on
    /// three sides and the whole caption band on top, which is why it is not the
    /// symmetric inset every other container has.
    pub fn display_rect(&self, bounds: Rect) -> Rect {
        self.sized(bounds).local_display_rect()
    }

    /// **CANVAS space.** A group box does not scroll and its frame is already in
    /// its children's coordinates, so its client origin is simply where the box
    /// sits — the replica's own answer.
    pub fn content_origin(&self, bounds: Rect) -> Point {
        self.inner.content_origin(bounds)
    }

    /// **LOCAL space.** The grouped children's rectangles.
    pub fn layout_children_local(&self, bounds: Rect) -> Vec<Rect> {
        self.slots.arrange(self.display_rect(bounds))
    }

    /// **CANVAS space.** The grouped children's rectangles.
    pub fn layout_children(&self, bounds: Rect) -> Vec<Rect> {
        let o = self.content_origin(bounds);
        self.layout_children_local(bounds)
            .into_iter()
            .map(|r| translate(r, o.x, o.y))
            .collect()
    }

    fn sized(&self, bounds: Rect) -> kc::GroupBox {
        let mut g = self.inner.clone();
        g.children = Children::new();
        g.control_mut().bounds =
            Rect::new(0.0, 0.0, bounds.right - bounds.left, bounds.bottom - bounds.top);
        g
    }

    /// The caption band's height — the replica's display rect starts below it.
    fn band_height(&self, bounds: Rect) -> f32 {
        self.display_rect(bounds).top - self.inner.padding.top
    }

    /// **CANVAS space.** The rounded frame: from the caption band's centre
    /// line down to the box's bottom.
    pub fn frame_rect(&self, bounds: Rect) -> Rect {
        let band_h = self.band_height(bounds);
        Rect::new(bounds.left, (bounds.top + band_h * 0.5).min(bounds.bottom), bounds.right, bounds.bottom)
    }

    /// **CANVAS space.** The caption's box, clamped to the frame: at most
    /// `width − 2 × GROUP_LABEL_INSET` wide, so a long caption ends in an
    /// ellipsis instead of running past the right edge. `None` without a
    /// caption.
    pub fn caption_rect(&self, canvas: &dyn Canvas, bounds: Rect) -> Option<Rect> {
        if self.inner.text.is_empty() {
            return None;
        }
        let x = bounds.left + control::GROUP_LABEL_INSET;
        let max = caption_max_width(bounds.right - bounds.left);
        let w = caption_box_width(canvas.measure(&self.inner.text, &canvas.formats().caption), max);
        Some(Rect::new(x, bounds.top, x + w, bounds.top + self.band_height(bounds)))
    }

    /// **CANVAS space.** The inside of the frame (below the caption band's
    /// centre line, within the hairline) — what grouped content is clipped
    /// to. The padding is part of it, as on the web.
    pub fn inner_rect(&self, bounds: Rect) -> Rect {
        deflate(self.frame_rect(bounds), card_metrics::RULE)
    }

    /// Clips to [`GroupBox::inner_rect`] (rounded with the frame) until the
    /// scope is dropped. The group box has no ground of its own, so the
    /// caller's `current_bg()` stays the right one — nothing is pushed.
    pub fn push_content_clip<'a>(&self, canvas: &'a dyn Canvas, bounds: Rect) -> Scope<'a> {
        canvas.push_clip_rounded(&self.inner_rect(bounds), inner_radius(radius::LG, card_metrics::RULE));
        Scope::new(canvas, None, ClipKind::Rounded)
    }

    /// Paints the frame and caption, then runs `content` clipped to the
    /// frame's inside. `content` receives the padded display rectangle on the
    /// canvas (where the grouped controls go).
    pub fn paint_with(
        &self,
        canvas: &dyn Canvas,
        bounds: Rect,
        state: WidgetState,
        content: impl FnOnce(&dyn Canvas, Rect),
    ) {
        self.paint_frame(canvas, bounds, state);
        let _clip = self.push_content_clip(canvas, bounds);
        self.paint_children(canvas, bounds, state);
        let o = self.content_origin(bounds);
        content(canvas, translate(self.display_rect(bounds), o.x, o.y));
    }

    /// The frame, with a gap for the caption, and the caption itself.
    fn paint_frame(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        let f = canvas.formats();
        let frame = self.frame_rect(bounds);
        match self.caption_rect(canvas, bounds) {
            None => canvas.stroke_rounded(&frame, radius::LG, &t.card_stroke),
            Some(caption) => {
                let (gap_l, gap_r) = caption_gap(caption.left, caption.right);
                let band_bottom = caption.bottom;
                // Three clips that together cover everything but the gap.
                for clip in [
                    Rect::new(bounds.left, bounds.top, gap_l, bounds.bottom),
                    Rect::new(gap_r, bounds.top, bounds.right, bounds.bottom),
                    Rect::new(gap_l, band_bottom, gap_r, bounds.bottom),
                ] {
                    if clip.right <= clip.left || clip.bottom <= clip.top {
                        continue;
                    }
                    canvas.push_clip(&clip);
                    canvas.stroke_rounded(&frame, radius::LG, &t.card_stroke);
                    canvas.pop_clip();
                }
                let colour = if state.disabled { t.text_tertiary } else { t.text_secondary };
                canvas.text_ellipsis(&self.inner.text, &caption, &f.caption, &colour);
            }
        }
    }

    fn paint_children(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        for (child, r) in self.slots.children.iter().zip(self.layout_children(bounds)) {
            if !child.item.visible {
                continue;
            }
            if let Some(w) = child.widget.as_ref() {
                w.paint(canvas, r, WidgetState::REST.disabled(state.disabled));
            }
        }
    }
}

/// The widest a group box caption may be in a box `width` DIP wide: the inset
/// on both sides, never negative.
pub fn caption_max_width(width: f32) -> f32 {
    (width - 2.0 * control::GROUP_LABEL_INSET).max(0.0)
}

/// Slack added to a measured caption before it is boxed: the renderer snaps
/// the text rectangle to device pixels, so a box exactly as wide as the
/// DirectWrite measure can lose a fraction of a pixel and trim a caption that
/// fits (« Identi… » in a 500-DIP group box).
const CAPTION_SLACK: f32 = 2.0;

/// The caption box's width for a caption measured at `measured` DIP, capped
/// at `max` (see [`caption_max_width`]).
fn caption_box_width(measured: f32, max: f32) -> f32 {
    (measured + CAPTION_SLACK).ceil().min(max)
}

/// The frame's gap around a caption spanning `left..right`: the caption plus
/// `space::XS` of air on each side (the replica's notch).
fn caption_gap(left: f32, right: f32) -> (f32, f32) {
    (left - space::XS, right + space::XS)
}

impl Deref for GroupBox {
    type Target = kc::GroupBox;
    fn deref(&self) -> &kc::GroupBox {
        &self.inner
    }
}

impl DerefMut for GroupBox {
    fn deref_mut(&mut self) -> &mut kc::GroupBox {
        &mut self.inner
    }
}

impl Widget for GroupBox {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    /// The replica's own measurement: wide enough for the children **or** the
    /// caption, whichever is wider, and tall enough for the caption band plus
    /// the children. Measured over the design rectangles, for the reason
    /// [`Panel::measure`] gives.
    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let mut g = self.inner.clone();
        g.children = Children::new();
        fill_children(&mut g.children, self.slots.children.iter().map(|c| (c.item, c.item.bounds)));
        g.preferred_size(canvas)
    }

    /// The frame runs along the caption band's centre line — the replica's
    /// arrangement, kept exactly — and the caption sits in a GAP of the frame.
    ///
    /// The gap is not erased with a colour (that painted a patch of
    /// `card_background` behind the caption on any other ground): the frame is
    /// stroked three times under three clips — left of the caption, right of
    /// it, and below the caption band — so the ground shows through untouched.
    /// Transparent otherwise, like a `<fieldset>`.
    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.paint_frame(canvas, bounds, state);
        if self.slots.children.iter().all(|ch| ch.widget.is_none()) {
            return;
        }
        let _clip = self.push_content_clip(canvas, bounds);
        self.paint_children(canvas, bounds, state);
    }

    fn type_name(&self) -> &'static str {
        "GroupBox"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Splitter — two panes and the band between them.
// ─────────────────────────────────────────────────────────────────────────────

/// Two panes separated by a draggable band.
///
/// The model is [`kubuno_controls::layout_panels::SplitContainer`]:
/// `splitter_distance`, `splitter_width`, `fixed_panel`, `panel1_min_size`,
/// `panel2_min_size`, `orientation` and the collapse flags are its fields,
/// reached through [`Deref`]. The three rectangles come from
/// [`kubuno_controls::layout_panels::split_layout`], the clamp from
/// [`kubuno_controls::layout_panels::clamp_distance`], and the resize rule from
/// [`kubuno_controls::layout_panels::adjusted_distance`].
///
/// **Watch the naming**, it is the toolkit's own trap:
/// `Orientation::Vertical` (the default) is a *vertical bar*, so the panes sit
/// side by side and the distance is pane 1's **width**.
///
/// ## The predecessor
///
/// `drive-app-controls::grid_splitter` is what ships today: the pure clamp of
/// `GridSplitter.Helper.cs`, used by Drive for the sidebar and the info pane.
/// It paints nothing — the divider in Drive is a gap — so « pixel-identical »
/// here means **arithmetically identical**, and it is pinned by
/// `drag_matches_the_grid_splitter` below: for the same pane, the same drag and
/// the same bounds, [`Splitter::drag_by`] lands on the DIP
/// `grid_splitter::resize` lands on.
#[derive(Default)]
pub struct Splitter {
    inner: kl::SplitContainer,
    /// How the divider is drawn. [`SplitterStyle::Hairline`] by default.
    pub style: SplitterStyle,
    /// The distance to go back to when the collapsed pane is restored with
    /// Enter (WAI-ARIA window splitter). `None` while not collapsed. Kept on the
    /// splitter, so a caller that keeps the `Splitter` between frames gets the
    /// toggle for free.
    pub restore: Option<f32>,
}

/// The look of a [`Splitter`]'s divider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SplitterStyle {
    /// A one-DIP `divider` line that thickens on hover and turns to the accent
    /// while dragged — the desktop shell's own divider (and the gallery's).
    #[default]
    Hairline,
    /// `@ui/ResizeHandle.tsx`: a 5-DIP `bg-border` rounded line, turning
    /// `primary/40` on hover, with a 14 × 36 grip pill (`bg-surface-0 border
    /// shadow-sm`, two columns of dots in `text-tertiary`, `opacity-80`) that
    /// turns `primary-light` / `primary` on hover. See [`handle_metrics`].
    Handle,
}

impl Splitter {
    /// The web's `ResizeHandle` look — see [`SplitterStyle::Handle`].
    pub fn with_handle(mut self) -> Self {
        self.style = SplitterStyle::Handle;
        self
    }

    /// The pointer shape over the grab band (and during a drag): `ew-resize`
    /// for a vertical bar, `ns-resize` for a horizontal one.
    pub fn cursor(&self) -> Cursor {
        if self.inner.orientation == Orientation::Vertical {
            Cursor::ResizeEW
        } else {
            Cursor::ResizeNS
        }
    }

    /// Keyboard control of a focused splitter, following the WAI-ARIA
    /// « window splitter » pattern (the web handle has none; this is the
    /// accessible behaviour it lacks):
    ///
    /// * the two arrows ALONG the split axis move the bar by
    ///   [`scroll_metrics::SPLIT_STEP`] (×5 with Shift) — Left/Right for a
    ///   vertical bar, Up/Down for a horizontal one;
    /// * Home / End move it to its minimum / maximum;
    /// * Enter collapses pane 1 to its minimum, and restores the previous
    ///   distance when pressed again.
    ///
    /// Returns whether the key was used (so the caller consumes it).
    pub fn handle_key(&mut self, bounds: Rect, key: u16, mods: Modifiers) -> bool {
        if mods.ctrl || mods.alt {
            return false;
        }
        let vertical = self.inner.orientation == Orientation::Vertical;
        let step = if mods.shift { scroll_metrics::SPLIT_STEP_LARGE } else { scroll_metrics::SPLIT_STEP };
        let (lower, higher) = if vertical { (vk::LEFT, vk::RIGHT) } else { (vk::UP, vk::DOWN) };
        let (min, max) = self.distance_range(bounds);
        let before = self.inner.splitter_distance;
        match key {
            k if k == lower => self.drag_by(bounds, -step),
            k if k == higher => self.drag_by(bounds, step),
            k if k == vk::HOME && !mods.shift => self.inner.splitter_distance = min,
            k if k == vk::END && !mods.shift => self.inner.splitter_distance = max,
            k if k == vk::ENTER && !mods.shift => {
                match self.restore.take() {
                    Some(back) => self.inner.splitter_distance = self.clamp(bounds, back),
                    None => {
                        self.restore = Some(before);
                        self.inner.splitter_distance = min;
                    }
                }
                return true;
            }
            _ => return false,
        }
        // Any move other than the Enter toggle forgets the collapse.
        if self.inner.splitter_distance != before {
            self.restore = None;
        }
        true
    }
    /// A vertical bar: two panes side by side, the distance is pane 1's width.
    pub fn vertical() -> Self {
        Self::default()
    }

    /// A horizontal bar: two panes stacked, the distance is pane 1's height.
    pub fn horizontal() -> Self {
        let mut s = Self::default();
        s.inner.orientation = Orientation::Horizontal;
        s
    }

    /// Pane 1's extent, in DIP.
    pub fn with_distance(mut self, distance: f32) -> Self {
        self.inner.splitter_distance = distance;
        self
    }

    /// The minimum extent of each pane (`Panel1MinSize` / `Panel2MinSize`).
    pub fn with_minimums(mut self, panel1: f32, panel2: f32) -> Self {
        self.inner.panel1_min_size = panel1;
        self.inner.panel2_min_size = panel2;
        self
    }

    /// Which pane keeps its size when the container is resized.
    pub fn with_fixed_panel(mut self, fixed: FixedPanel) -> Self {
        self.inner.fixed_panel = fixed;
        self
    }

    /// **LOCAL space.** The client rectangle the split is computed in.
    pub fn display_rect(&self, bounds: Rect) -> Rect {
        self.sized(bounds).local_display_rect()
    }

    /// **CANVAS space.** The three rectangles: pane 1, the bar, pane 2.
    ///
    /// The crossing adds no border inset — a `SplitContainer`'s own
    /// `BorderStyle` belongs to its two panes, not to the split, which the
    /// replica documents and measured.
    pub fn arrange(&self, bounds: Rect) -> SplitRects {
        let local = self.sized(bounds).arrange();
        SplitRects {
            panel1: translate(local.panel1, bounds.left, bounds.top),
            splitter: translate(local.splitter, bounds.left, bounds.top),
            panel2: translate(local.panel2, bounds.left, bounds.top),
        }
    }

    /// **CANVAS space.** Pane 1.
    pub fn panel1_rect(&self, bounds: Rect) -> Rect {
        self.arrange(bounds).panel1
    }

    /// **CANVAS space.** Pane 2.
    pub fn panel2_rect(&self, bounds: Rect) -> Rect {
        self.arrange(bounds).panel2
    }

    /// **CANVAS space.** The bar itself, `SplitterWidth` thick.
    pub fn splitter_rect(&self, bounds: Rect) -> Rect {
        self.arrange(bounds).splitter
    }

    /// **CANVAS space.** The band the pointer may grab: the bar, widened to at
    /// least [`crate::metrics::control::SPLITTER`] and centred on it.
    ///
    /// A 4 DIP bar is a 4 DIP target, which is below anything comfortable; the
    /// web's own `@ui/ResizeHandle` does exactly this, hanging a 12 px hit zone
    /// off a 5 px line. The number here is the crate's table, not the web's.
    pub fn grip_rect(&self, bounds: Rect) -> Rect {
        let r = self.splitter_rect(bounds);
        let grow = ((control::SPLITTER - self.inner.splitter_width) * 0.5).max(0.0);
        if self.inner.orientation == Orientation::Vertical {
            Rect::new(r.left - grow, r.top, r.right + grow, r.bottom)
        } else {
            Rect::new(r.left, r.top - grow, r.right, r.bottom + grow)
        }
    }

    /// Whether a canvas-space point is on the grab band.
    pub fn hit_test_grip(&self, bounds: Rect, x: f32, y: f32) -> bool {
        !self.inner.panel1_collapsed
            && !self.inner.panel2_collapsed
            && self.grip_rect(bounds).contains(x, y)
    }

    /// The distance the pointer at `(x, y)` asks for, already clamped to the
    /// legal range.
    pub fn distance_for(&self, bounds: Rect, x: f32, y: f32) -> f32 {
        let d = self.display_rect(bounds);
        let along = if self.inner.orientation == Orientation::Vertical {
            x - bounds.left - d.left
        } else {
            y - bounds.top - d.top
        };
        self.clamp(bounds, along)
    }

    /// Moves the bar so it follows a canvas-space pointer.
    pub fn drag_to(&mut self, bounds: Rect, x: f32, y: f32) {
        self.inner.splitter_distance = self.distance_for(bounds, x, y);
    }

    /// Moves the bar by `delta` DIP along the split axis, clamped.
    ///
    /// This is the operation `grid_splitter::resize` performs, expressed through
    /// the replica's [`clamp_distance`] — see the type docs, and the test that
    /// pins the two together.
    pub fn drag_by(&mut self, bounds: Rect, delta: f32) {
        let target = self.inner.splitter_distance + delta;
        self.inner.splitter_distance = self.clamp(bounds, target);
    }

    /// The legal range for `splitter_distance`, as `(min, max)`.
    ///
    /// The maximum is what the far pane's minimum and the bar leave — the same
    /// quantity `grid_splitter::available_max` computes for Drive's info pane.
    pub fn distance_range(&self, bounds: Rect) -> (f32, f32) {
        let total = self.axis_extent(bounds);
        let min = self.inner.panel1_min_size;
        let max = (total - self.inner.splitter_width - self.inner.panel2_min_size).max(min.min(total));
        (min, max)
    }

    /// Re-derives the distance after the container changed size along the split
    /// axis, honouring `FixedPanel`. Straight
    /// [`adjusted_distance`] — including its truncation, because
    /// `SplitterDistance` is an `Int32` in the toolkit.
    pub fn resized(&mut self, old_total: f32, new_total: f32) {
        self.inner.splitter_distance = adjusted_distance(
            old_total,
            new_total,
            self.inner.splitter_distance,
            self.inner.fixed_panel,
            self.inner.splitter_width,
        );
    }

    fn clamp(&self, bounds: Rect, distance: f32) -> f32 {
        clamp_distance(
            self.axis_extent(bounds),
            distance,
            self.inner.splitter_width,
            self.inner.panel1_min_size,
            self.inner.panel2_min_size,
        )
    }

    /// The extent of the client area along the split axis.
    fn axis_extent(&self, bounds: Rect) -> f32 {
        let d = self.display_rect(bounds);
        if self.inner.orientation == Orientation::Vertical {
            d.right - d.left
        } else {
            d.bottom - d.top
        }
    }

    fn sized(&self, bounds: Rect) -> kl::SplitContainer {
        let mut s = self.inner.clone();
        s.control_mut().bounds =
            Rect::new(0.0, 0.0, bounds.right - bounds.left, bounds.bottom - bounds.top);
        s
    }
}

impl Deref for Splitter {
    type Target = kl::SplitContainer;
    fn deref(&self) -> &kl::SplitContainer {
        &self.inner
    }
}

impl DerefMut for Splitter {
    fn deref_mut(&mut self) -> &mut kl::SplitContainer {
        &mut self.inner
    }
}

impl Widget for Splitter {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    /// Both minimums plus the bar — the smallest box in which the split still
    /// means something. The cross axis is the caller's business.
    fn measure(&self, _canvas: &dyn Canvas) -> Size {
        let extent =
            self.inner.panel1_min_size + self.inner.splitter_width + self.inner.panel2_min_size;
        if self.inner.orientation == Orientation::Vertical {
            Size::new(extent, 0.0)
        } else {
            Size::new(0.0, extent)
        }
    }

    /// Paints the divider only. The two panes are the caller's — a splitter that
    /// filled them would be painting over content it knows nothing about.
    ///
    /// At rest a [`crate::metrics::control::SPLITTER_LINE`] hairline in
    /// `divider`, centred in the bar; under the pointer the line grows to the
    /// bar's full width in `border_strong`; while dragged, `accent`. Rounded
    /// like the web's handle (`rounded-full`), which is [`pill`] of its
    /// thickness.
    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        // Exception to the opaque-background rule: a splitter only paints its
        // divider — its two panes belong to the caller (see doc above). Painting
        // a solid background here would overwrite them each frame.
        if self.inner.panel1_collapsed || self.inner.panel2_collapsed {
            return;
        }
        let t = canvas.theme();
        let bar = self.splitter_rect(bounds);
        let vertical = self.inner.orientation == Orientation::Vertical;
        let line_of = |thickness: f32| {
            if vertical {
                let cx = (bar.left + bar.right) * 0.5;
                Rect::new(cx - thickness * 0.5, bar.top, cx + thickness * 0.5, bar.bottom)
            } else {
                let cy = (bar.top + bar.bottom) * 0.5;
                Rect::new(bar.left, cy - thickness * 0.5, bar.right, cy + thickness * 0.5)
            }
        };
        let active = state.hot || state.pressed;
        let ring_around = match self.style {
            SplitterStyle::Hairline => {
                let (thickness, colour) = if state.pressed {
                    (self.inner.splitter_width, t.accent)
                } else if state.hot {
                    (self.inner.splitter_width, t.border_strong)
                } else {
                    (control::SPLITTER_LINE, t.divider)
                };
                let line = line_of(thickness);
                canvas.fill_rounded(&line, pill(thickness), &colour);
                self.grip_rect(bounds)
            }
            SplitterStyle::Handle => {
                use handle_metrics as h;
                // The line: `bg-border`, `group-hover:bg-primary/40`.
                let line = line_of(h::LINE);
                let colour = if active { with_alpha(t.accent, h::HOVER_ALPHA) } else { t.card_stroke };
                canvas.fill_rounded(&line, pill(h::LINE), &colour);
                // The grip pill, centred on the bar.
                let (cx, cy) = ((bar.left + bar.right) * 0.5, (bar.top + bar.bottom) * 0.5);
                let (pw, ph) = if vertical { (h::PILL_W, h::PILL_H) } else { (h::PILL_H, h::PILL_W) };
                let grip = Rect::new(cx - pw * 0.5, cy - ph * 0.5, cx + pw * 0.5, cy + ph * 0.5);
                let alpha = if active { 1.0 } else { h::REST_OPACITY };
                let (fill, stroke, dots) = if active {
                    (t.accent_light, with_alpha(t.accent, h::HOVER_ALPHA), t.accent)
                } else {
                    (t.layer_background, t.card_stroke, t.text_tertiary)
                };
                let r = pill(pw.min(ph));
                canvas.draw_shadow(&grip, r, &SHADOW_SM, SHADOW_GREY);
                canvas.fill_rounded(&grip, r, &with_alpha(fill, alpha));
                canvas.stroke_rounded(&grip, r, &with_alpha(stroke, alpha));
                // `GripVertical`: two columns of three dots (rotated for a
                // horizontal bar).
                for i in [-1.0_f32, 0.0, 1.0] {
                    for j in [-1.0_f32, 1.0] {
                        let (dx, dy) = if vertical {
                            (j * h::DOT_DX, i * h::DOT_DY)
                        } else {
                            (i * h::DOT_DY, j * h::DOT_DX)
                        };
                        let d = Rect::new(cx + dx - h::DOT_R, cy + dy - h::DOT_R, cx + dx + h::DOT_R, cy + dy + h::DOT_R);
                        canvas.fill_rounded(&d, h::DOT_R, &with_alpha(dots, alpha));
                    }
                }
                grip
            }
        };
        // `:focus-visible` — the ring a keyboard user follows.
        if state.show_focus_ring() {
            // Kept inside the splitter's own box: the ring never paints over
            // whatever sits past its ends.
            let g = ring_around.inflate(scroll_metrics::FOCUS_RING, scroll_metrics::FOCUS_RING);
            let ring = Rect::new(
                g.left.max(bounds.left),
                g.top.max(bounds.top),
                g.right.min(bounds.right),
                g.bottom.min(bounds.bottom),
            );
            let r = pill((ring.right - ring.left).min(ring.bottom - ring.top));
            canvas.stroke_rounded_w(&ring, r, &t.accent, scroll_metrics::FOCUS_RING);
        }
    }

    /// The grab band, not the bar — a 4 DIP target is not one.
    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        self.hit_test_grip(bounds, x, y)
    }

    fn type_name(&self) -> &'static str {
        "Splitter"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ScrollView — a Panel with auto-scroll, and the ranges a scroll bar needs.
// ─────────────────────────────────────────────────────────────────────────────

/// Which axis a scroll query is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// A [`Panel`] whose content may be larger than its box.
///
/// It scrolls the content and **publishes the ranges** — it does not paint a
/// scroll bar. That is deliberate: the bar belongs to the `range` family, and a
/// container that drew its own would be the second implementation of an
/// arithmetic the replica already owns.
///
/// ## The junction with the `range` family
///
/// [`ScrollView::range`] returns a
/// [`kubuno_controls::containers::ScrollProperties`] — the replica's own
/// `HScrollProperties`/`VScrollProperties` — filled in for the box it was asked
/// about:
///
/// | field | value | why |
/// |---|---|---|
/// | `minimum` | `0` | the content starts at its own origin |
/// | `maximum` | `content − 1` | .NET's `Maximum` is **inclusive**, so the span `Maximum − Minimum + 1` is the content extent — that is the denominator `ScrollBar::thumb_fraction` divides by |
/// | `large_change` | the viewport extent | a page is what you can see, so the thumb reads as `viewport / content` |
/// | `value` | the scroll offset, ≥ 0 | the replica stores `AutoScrollPosition` as ≤ 0 (the shift applied to the content); a scroll bar wants the positive distance |
/// | `visible` | `content > viewport` | the toolkit hides a bar with nothing to scroll |
///
/// `small_change` and `enabled` are carried through from the replica untouched.
/// A caller wires it up as `ScrollBar::set_maximum(range.maximum as i32)` … and
/// paints it in [`ScrollView::track`].
pub struct ScrollView {
    panel: Panel,
}

impl Default for ScrollView {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollView {
    pub fn new() -> Self {
        let mut v = Self { panel: Panel::new() };
        v.panel.auto_scroll = true;
        v
    }

    pub fn with_padding(mut self, padding: Padding) -> Self {
        self.panel.padding = padding;
        self
    }

    pub fn with_surface(mut self, surface: Surface) -> Self {
        self.panel.surface = surface;
        self
    }

    /// **CANVAS space.** What the viewer sees — the padded client rectangle,
    /// kept INSIDE the frame: a stroked surface draws its hairline even with
    /// `BorderStyle::None`, so the viewport is inset by [`frame_inset`], never
    /// by less. Content clipped to it (see [`ScrollView::push_content_clip`])
    /// leaves the border and the rounded corners intact.
    pub fn viewport(&self, bounds: Rect) -> Rect {
        let border = self.panel.border();
        let extra = frame_inset(self.panel.surface.is_stroked(), border) - border;
        deflate(self.panel.client_rect(bounds), extra)
    }

    /// The corner radius content is clipped with — concentric with the frame,
    /// reduced by how far the viewport sits inside it.
    pub fn viewport_radius(&self, bounds: Rect) -> f32 {
        let v = self.viewport(bounds);
        let d = (v.left - bounds.left).max(v.top - bounds.top);
        inner_radius(self.panel.surface.radius(), d)
    }

    /// Clips to the viewport (rounded with the frame) and announces the
    /// view's ground until the scope is dropped — the one call a caller makes
    /// before painting scrolled content, so it can neither overpaint the
    /// border nor cut the corners square.
    pub fn push_content_clip<'a>(&self, canvas: &'a dyn Canvas, bounds: Rect) -> Scope<'a> {
        let v = self.viewport(bounds);
        let r = self.viewport_radius(bounds);
        let bg = self.panel.surface.fill(canvas);
        if r > 0.0 {
            canvas.push_clip_rounded(&v, r);
            Scope::new(canvas, bg, ClipKind::Rounded)
        } else {
            canvas.push_clip(&v);
            Scope::new(canvas, bg, ClipKind::Axis)
        }
    }

    /// Paints the view, then runs `content` under
    /// [`ScrollView::push_content_clip`], then strokes the frame on top.
    /// `content` receives the viewport; shift by [`ScrollView::offset`] to
    /// place scrolled content.
    pub fn paint_content(
        &self,
        canvas: &dyn Canvas,
        bounds: Rect,
        state: WidgetState,
        content: impl FnOnce(&dyn Canvas, Rect),
    ) {
        self.panel.surface.paint_ground(canvas, bounds);
        {
            let _clip = self.push_content_clip(canvas, bounds);
            self.panel.paint_children_unclipped(canvas, bounds, state);
            content(canvas, self.viewport(bounds));
        }
        self.paint_frame(canvas, bounds, state);
    }

    /// The frame, and the `:focus-visible` ring of a focused scroller.
    fn paint_frame(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.panel.surface.paint_frame(canvas, bounds, self.panel.border());
        if state.show_focus_ring() {
            canvas.stroke_rounded_w(
                &bounds,
                self.panel.surface.radius(),
                &canvas.theme().accent,
                scroll_metrics::FOCUS_RING,
            );
        }
    }

    /// Applies a wheel movement, in DIP with the web's sign (`dy > 0` scrolls
    /// DOWN). With Shift held, a vertical wheel scrolls horizontally, as the
    /// browsers do. Returns whether the offset changed — a view already at its
    /// end lets the caller pass the wheel on to its parent (scroll chaining).
    pub fn wheel(&mut self, bounds: Rect, delta: (f32, f32), shift: bool) -> bool {
        let (dx, dy) = if shift && delta.0 == 0.0 { (delta.1, 0.0) } else { delta };
        let before = (self.offset(Axis::Horizontal), self.offset(Axis::Vertical));
        self.scroll_by(bounds, dx, dy);
        before != (self.offset(Axis::Horizontal), self.offset(Axis::Vertical))
    }

    /// Keyboard scrolling of a focused view, the browser's keys and steps
    /// ([`scroll_metrics`]): arrows scroll a line (40), Page Up / Page Down and
    /// Space / Shift+Space a page (87.5 % of the viewport), Home / End go to
    /// the top / bottom. Returns whether the key was used.
    pub fn handle_key(&mut self, bounds: Rect, key: u16, mods: Modifiers) -> bool {
        if mods.ctrl || mods.alt {
            return false;
        }
        let v = self.viewport(bounds);
        let page_y = (v.bottom - v.top) * scroll_metrics::PAGE_FRACTION;
        let line = scroll_metrics::LINE_STEP;
        let (x, y) = (self.offset(Axis::Horizontal), self.offset(Axis::Vertical));
        let target = match key {
            k if k == vk::UP && !mods.shift => (x, y - line),
            k if k == vk::DOWN && !mods.shift => (x, y + line),
            k if k == vk::LEFT && !mods.shift => (x - line, y),
            k if k == vk::RIGHT && !mods.shift => (x + line, y),
            k if k == vk::PAGE_UP && !mods.shift => (x, y - page_y),
            k if k == vk::PAGE_DOWN && !mods.shift => (x, y + page_y),
            k if k == vk::SPACE => (x, if mods.shift { y - page_y } else { y + page_y }),
            k if k == vk::HOME && !mods.shift => (x, 0.0),
            k if k == vk::END && !mods.shift => (x, self.max_offset(bounds, Axis::Vertical)),
            _ => return false,
        };
        self.scroll_to(bounds, target.0, target.1);
        true
    }

    /// Scrolls the least amount that brings `target` (CANVAS space, as laid
    /// out at the CURRENT offset) fully into the viewport — the browser's
    /// `scrollIntoView({ block: 'nearest' })`, which is what focusing a
    /// control inside a scroller does. Returns whether the offset changed.
    pub fn ensure_visible(&mut self, bounds: Rect, target: Rect) -> bool {
        let v = self.viewport(bounds);
        let dy = nearest_delta(v.top, v.bottom, target.top, target.bottom);
        let dx = nearest_delta(v.left, v.right, target.left, target.right);
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        let before = (self.offset(Axis::Horizontal), self.offset(Axis::Vertical));
        self.scroll_by(bounds, dx, dy);
        before != (self.offset(Axis::Horizontal), self.offset(Axis::Vertical))
    }

    /// A scroll bar for `axis`, loaded from this view's content, viewport and
    /// offset — the junction with the `range` family, which paints it into
    /// [`ScrollView::track`]. `None` when there is nothing to scroll.
    pub fn scroll_bar(&self, bounds: Rect, axis: Axis) -> Option<ScrollBar> {
        let (content, viewport) = self.extents(bounds, axis);
        ScrollBar::from_content(axis == Axis::Horizontal, content, viewport, self.offset(axis))
    }

    /// The offset that puts the thumb's near edge at `thumb_start` along the
    /// track (a thumb drag): the inverse of the bar's own mapping, clamped.
    pub fn offset_for_thumb(&self, bounds: Rect, axis: Axis, track_len: f32, thumb_len: f32, thumb_start: f32) -> f32 {
        let travel = (track_len - thumb_len).max(0.0);
        if travel <= 0.0 {
            return 0.0;
        }
        (thumb_start / travel).clamp(0.0, 1.0) * self.max_offset(bounds, axis)
    }

    /// The virtual content size, from the replica.
    pub fn content_size(&self, bounds: Rect) -> Size {
        self.panel.content_size(bounds)
    }

    /// The current offset on one axis, as a **positive** distance scrolled.
    ///
    /// The replica stores `AutoScrollPosition` as ≤ 0, so this is its negation —
    /// and the `if` is not decoration: negating a resting `0.0` yields `-0.0`,
    /// which compares equal to zero but *formats* as `-0` and would put a minus
    /// sign in front of every unscrolled range a caller prints.
    pub fn offset(&self, axis: Axis) -> f32 {
        let p = self.panel.auto_scroll_position;
        let v = match axis {
            Axis::Horizontal => -p.x,
            Axis::Vertical => -p.y,
        };
        if v == 0.0 {
            0.0
        } else {
            v
        }
    }

    /// The furthest this view can scroll on one axis.
    pub fn max_offset(&self, bounds: Rect, axis: Axis) -> f32 {
        let (content, viewport) = self.extents(bounds, axis);
        (content - viewport).max(0.0)
    }

    /// Scrolls to an absolute positive offset, clamped to `0..=max_offset`.
    /// Writes the replica's `AutoScrollPosition` in its own ≤ 0 convention.
    pub fn scroll_to(&mut self, bounds: Rect, x: f32, y: f32) {
        let x = x.clamp(0.0, self.max_offset(bounds, Axis::Horizontal));
        let y = y.clamp(0.0, self.max_offset(bounds, Axis::Vertical));
        self.panel.auto_scroll_position = Point::new(-x, -y);
    }

    /// Scrolls by a delta — what a wheel notch or a drag produces.
    pub fn scroll_by(&mut self, bounds: Rect, dx: f32, dy: f32) {
        let x = self.offset(Axis::Horizontal) + dx;
        let y = self.offset(Axis::Vertical) + dy;
        self.scroll_to(bounds, x, y);
    }

    /// The range to hand a scroll bar for one axis. See the type docs for what
    /// each field means and why.
    pub fn range(&self, bounds: Rect, axis: Axis) -> ScrollProperties {
        let (content, viewport) = self.extents(bounds, axis);
        let mut p = match axis {
            Axis::Horizontal => self.panel.horizontal_scroll,
            Axis::Vertical => self.panel.vertical_scroll,
        };
        p.minimum = 0.0;
        p.maximum = (content - 1.0).max(0.0);
        p.large_change = viewport;
        p.value = self.offset(axis).min((content - viewport).max(0.0));
        p.visible = content > viewport;
        p
    }

    /// **CANVAS space.** Where a scroll bar for `axis` goes: a
    /// [`crate::metrics::control::SCROLLBAR`]-thick strip along the far edge of
    /// the viewport, shortened to stay clear of the viewport's rounded corners
    /// (a card's `rounded-xl`), so the bar never pokes out of the frame. The
    /// `range` family paints into this; this family never does.
    pub fn track(&self, bounds: Rect, axis: Axis) -> Rect {
        let v = self.viewport(bounds);
        let rail = match axis {
            Axis::Vertical => Rect::new(v.right - control::SCROLLBAR, v.top, v.right, v.bottom),
            Axis::Horizontal => {
                Rect::new(v.left, v.bottom - control::SCROLLBAR, v.right, v.bottom)
            }
        };
        crate::range::fit_rail(rail, v, self.viewport_radius(bounds))
    }

    /// `(content, viewport)` along one axis.
    fn extents(&self, bounds: Rect, axis: Axis) -> (f32, f32) {
        let content = self.content_size(bounds);
        let v = self.viewport(bounds);
        match axis {
            Axis::Horizontal => (content.width, v.right - v.left),
            Axis::Vertical => (content.height, v.bottom - v.top),
        }
    }
}

panel_builders!(ScrollView);

impl Deref for ScrollView {
    type Target = Panel;
    fn deref(&self) -> &Panel {
        &self.panel
    }
}

impl DerefMut for ScrollView {
    fn deref_mut(&mut self) -> &mut Panel {
        &mut self.panel
    }
}

impl Widget for ScrollView {
    fn model(&self) -> &dyn Control {
        self.panel.model()
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        self.panel.measure(canvas)
    }

    /// The ground, the children scrolled and clipped to the viewport, then the
    /// frame (and a focus ring on `state.show_focus_ring()`). Transparent
    /// outside its rounded surface, like [`Panel`].
    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.paint_content(canvas, bounds, state, |_, _| {});
    }

    fn type_name(&self) -> &'static str {
        "ScrollView"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Stack — a run of full-width blocks.
// ─────────────────────────────────────────────────────────────────────────────

/// A run of blocks along one axis, each stretched across the other, with a fixed
/// gap between them.
///
/// ## Why this is not just a `FlowLayoutPanel`
///
/// The flow engine is exactly right and is what runs underneath — this type
/// calls [`kubuno_controls::layout_panels::flow_layout`] and adds no
/// arithmetic. What `FlowLayoutPanel` does **not** do is stretch a child across
/// the cross axis: every child keeps the width it was given, because that is
/// what the toolkit does. The pattern the console is full of —
/// `shell/src/admin_storage.rs::blocks`, a column of full-width cards with
/// `y += h + GAP` — needs the opposite, and asking `FlowLayoutPanel` for it
/// would mean the caller computing each card's width, which is the `Rect::new`
/// this family exists to remove.
///
/// So: the gap becomes each cell's trailing margin, the cross extent becomes its
/// size, and the engine does the rest. `stack_matches_the_hand_written_column`
/// pins the result against the arithmetic the console writes today.
pub struct Stack {
    panel: Panel,
    /// The direction blocks run in. `TopDown` (a column) is the default.
    pub direction: FlowDirection,
    /// The space between two blocks.
    pub gap: f32,
    /// Each block's extent along the run — its height in a column, its width in
    /// a row.
    extents: Vec<f32>,
}

impl Default for Stack {
    fn default() -> Self {
        Self {
            panel: Panel::new(),
            direction: FlowDirection::TopDown,
            gap: space::MD,
            extents: Vec::new(),
        }
    }
}

impl Stack {
    /// A column: blocks run downward, each as wide as the client area.
    pub fn column(gap: f32) -> Self {
        Self { gap, ..Self::default() }
    }

    /// A row: blocks run left to right, each as tall as the client area.
    pub fn row(gap: f32) -> Self {
        Self { direction: FlowDirection::LeftToRight, gap, ..Self::default() }
    }

    pub fn with_padding(mut self, padding: Padding) -> Self {
        self.panel.padding = padding;
        self
    }

    pub fn with_surface(mut self, surface: Surface) -> Self {
        self.panel.surface = surface;
        self
    }

    /// Appends a block `extent` long, and returns its index.
    pub fn push(&mut self, extent: f32) -> usize {
        self.extents.push(extent);
        self.extents.len() - 1
    }

    /// Appends a block, builder-style.
    pub fn block(mut self, extent: f32) -> Self {
        self.push(extent);
        self
    }

    pub fn len(&self) -> usize {
        self.extents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.extents.is_empty()
    }

    pub fn clear(&mut self) {
        self.extents.clear();
    }

    /// **CANVAS space.** One rectangle per block, in order.
    pub fn layout_children(&self, bounds: Rect) -> Vec<Rect> {
        let display = self.panel.display_rect(bounds);
        let o = self.panel.content_origin(bounds);
        let cells = self.cells(display);
        flow_layout(display, self.direction, false, &cells)
            .into_iter()
            .map(|r| translate(r, o.x, o.y))
            .collect()
    }

    /// The run's total length, gaps included — what a [`ScrollView`] hosting
    /// this stack scrolls over.
    pub fn content_extent(&self) -> f32 {
        self.extents.iter().map(|e| e + self.gap).sum()
    }

    /// The flow cells: the cross extent as size, the gap as a trailing margin.
    /// A trailing gap after the last block is intentional — it is what
    /// `y += h + GAP` leaves too, and it is the bottom breathing room a scrolled
    /// column wants.
    fn cells(&self, display: Rect) -> Vec<FlowChild> {
        let horizontal = matches!(
            self.direction,
            FlowDirection::LeftToRight | FlowDirection::RightToLeft
        );
        self.extents
            .iter()
            .map(|&e| {
                let (size, margin) = if horizontal {
                    (
                        Size::new(e, display.bottom - display.top),
                        Padding::new(0.0, 0.0, self.gap, 0.0),
                    )
                } else {
                    (
                        Size::new(display.right - display.left, e),
                        Padding::new(0.0, 0.0, 0.0, self.gap),
                    )
                };
                FlowChild { size, margin, flow_break: false }
            })
            .collect()
    }
}

impl Deref for Stack {
    type Target = Panel;
    fn deref(&self) -> &Panel {
        &self.panel
    }
}

impl DerefMut for Stack {
    fn deref_mut(&mut self) -> &mut Panel {
        &mut self.panel
    }
}

impl Widget for Stack {
    fn model(&self) -> &dyn Control {
        self.panel.model()
    }

    fn measure(&self, _canvas: &dyn Canvas) -> Size {
        let p = self.panel.padding;
        let run = self.content_extent();
        if matches!(self.direction, FlowDirection::LeftToRight | FlowDirection::RightToLeft) {
            Size::new(run + p.horizontal(), p.vertical())
        } else {
            Size::new(p.horizontal(), run + p.vertical())
        }
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, _state: WidgetState) {
        self.panel.surface.paint(canvas, bounds, self.panel.border());
    }

    fn type_name(&self) -> &'static str {
        "Stack"
    }
}

/// Same rectangle, moved — the local→canvas step, applied in exactly the places
/// that cross the boundary.
fn translate(r: Rect, dx: f32, dy: f32) -> Rect {
    Rect::new(r.left + dx, r.top + dy, r.right + dx, r.bottom + dy)
}

/// The scroll delta that brings `lo..hi` inside `view_lo..view_hi` with the
/// least movement (`block: 'nearest'`): 0 when already inside, align the near
/// edge when it is above/left, the far edge when below/right — and the near
/// edge when the target is larger than the view.
fn nearest_delta(view_lo: f32, view_hi: f32, lo: f32, hi: f32) -> f32 {
    if lo >= view_lo && hi <= view_hi {
        0.0
    } else if lo < view_lo || hi - lo > view_hi - view_lo {
        lo - view_lo
    } else {
        hi - view_hi
    }
}

/// Fills a replica child collection with one stand-in per `(item, rect)` pair.
///
/// The replica's container arithmetic — the content extent, the scrollable
/// area, `preferred_size` — reads `Controls`, and a Kubuno container keeps its
/// children as layout items rather than as boxed controls. Rather than restate
/// that arithmetic (five lines that would then have to stay right forever),
/// the question is put to the replica over stand-ins carrying the geometry that
/// matters to it: the rectangle and the visibility.
fn fill_children(into: &mut Children, children: impl Iterator<Item = (Item, Rect)>) {
    for (item, rect) in children {
        let mut stand_in = kc::Panel::new();
        stand_in.control_mut().bounds = rect;
        stand_in.control_mut().visible = item.visible;
        into.push(Box::new(stand_in));
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// ScrollArea
// ═════════════════════════════════════════════════════════════════════════════

/// A thumb drag in progress: which bar, and how far into the thumb (along its
/// axis) the pointer grabbed it, so the thumb does not jump under the pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BarDrag {
    horizontal: bool,
    grab:       f32,
}

/// A live region whose content scrolls — the web's `overflow: auto`.
///
/// Unlike [`ScrollView`], which scrolls a replica whose children are known,
/// this one scrolls whatever is PAINTED into it: the content is laid out in its
/// own coordinates (at scroll zero they are the caller's), painted under a
/// translation, and measured while it paints (`Canvas::begin_extent`), so a
/// bar shows up as soon as anything reaches past the region — on either axis —
/// without the content declaring its size.
///
/// The content receives a [`Frame`] in its own coordinates: the pointer is
/// shifted by the scroll, and parked at [`host::POINTER_AWAY`] while it is
/// outside the region or on a bar, so nothing scrolled out of view lights up
/// or takes a click. Floating surfaces the content opens ([`host::popup`])
/// and its focus-ring rectangles are converted back by the host.
///
/// The bars overlay the content's far edges, the Kubuno skin's resting
/// indicator unfolding into a full gutter under the pointer. The wheel scrolls
/// the region unless a control inside claimed it ([`host::claim_wheel`]);
/// Shift+wheel scrolls sideways.
#[derive(Debug, Default, Clone)]
pub struct ScrollArea {
    /// The corner radius of the region's outline, when it is rounded: the
    /// bars keep clear of the corners. `0` (the default) for a square region.
    pub corner:    f32,
    /// Current scroll, in DIP, `(x, y)`.
    scroll:        (f32, f32),
    /// How far the content reached last frame, from the region's top-left.
    extent:        (f32, f32),
    drag:          Option<BarDrag>,
    prev_down:     bool,
    /// The left button went down on the content, so the content keeps the
    /// pointer until release even outside the region (a drag).
    content_press: bool,
}

/// What one [`ScrollArea::frame`] did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollAreaRun {
    /// The scroll after this frame.
    pub scroll:     (f32, f32),
    /// Which bars were shown: `(horizontal, vertical)`.
    pub bars:       (bool, bool),
    /// The pointer is on a bar (or dragging one).
    pub on_bar:     bool,
}

impl ScrollArea {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder form of [`ScrollArea::corner`].
    pub fn with_corner(mut self, radius: f32) -> Self {
        self.corner = radius.max(0.0);
        self
    }

    /// The scroll offset, `(x, y)` in DIP.
    pub fn scroll(&self) -> (f32, f32) {
        self.scroll
    }

    /// Sets the scroll offset; clamped to the content on the next frame.
    pub fn set_scroll(&mut self, x: f32, y: f32) {
        self.scroll = (x.max(0.0), y.max(0.0));
    }

    /// The content's size as last measured, from the region's top-left.
    pub fn content_size(&self) -> (f32, f32) {
        self.extent
    }

    /// Which bars `bounds` needs for the last measured content:
    /// `(horizontal, vertical)`. The bars overlay the content, so neither one
    /// shrinks the viewport the other is measured against.
    pub fn bars_needed(&self, bounds: Rect) -> (bool, bool) {
        (
            ScrollBar::needed(self.extent.0, bounds.right - bounds.left),
            ScrollBar::needed(self.extent.1, bounds.bottom - bounds.top),
        )
    }

    fn max_scroll(&self, bounds: Rect) -> (f32, f32) {
        (
            (self.extent.0 - (bounds.right - bounds.left)).max(0.0),
            (self.extent.1 - (bounds.bottom - bounds.top)).max(0.0),
        )
    }

    /// The bar for one axis and the gutter it lies in, or `None` when that
    /// axis does not scroll. With both bars up, each stops short of the corner
    /// the other one occupies.
    fn bar(&self, horizontal: bool, bounds: Rect, need: (bool, bool)) -> Option<(ScrollBar, Rect)> {
        let (extent, viewport, scroll) = if horizontal {
            (self.extent.0, bounds.right - bounds.left, self.scroll.0)
        } else {
            (self.extent.1, bounds.bottom - bounds.top, self.scroll.1)
        };
        let bar = ScrollBar::from_content(horizontal, extent, viewport, scroll)?;
        let rail = if horizontal {
            let right = bounds.right - if need.1 { control::SCROLLBAR } else { 0.0 };
            Rect::new(bounds.left, bounds.bottom - control::SCROLLBAR, right, bounds.bottom)
        } else {
            let bottom = bounds.bottom - if need.0 { control::SCROLLBAR } else { 0.0 };
            Rect::new(bounds.right - control::SCROLLBAR, bounds.top, bounds.right, bottom)
        };
        Some((bar, crate::range::fit_rail(rail, bounds, self.corner)))
    }

    fn set_axis(&mut self, horizontal: bool, v: f32) {
        if horizontal {
            self.scroll.0 = v;
        } else {
            self.scroll.1 = v;
        }
    }

    /// The frame the content sees: pointer and screen origin shifted into
    /// content coordinates. The pointer reaches the content while it is over
    /// the region (not on a bar), during a drag that started on the content,
    /// or over a menu the content opened; elsewhere it is parked away.
    fn content_frame(&self, bounds: Rect, f: &host::Frame, on_bar: bool) -> host::Frame {
        let (mx, my) = f.mouse;
        let (sx, sy) = self.scroll;
        let reaches = if f.mouse_down {
            self.content_press || host::over_popup(mx, my)
        } else {
            host::over_popup(mx, my) || (!on_bar && bounds.contains(mx, my))
        };
        host::Frame {
            mouse: if reaches { (mx + sx, my + sy) } else { (host::POINTER_AWAY, host::POINTER_AWAY) },
            client_origin: (f.client_origin.0 - sx, f.client_origin.1 - sy),
            ..*f
        }
    }

    /// Runs one frame of the region `bounds`: pointer on the bars, the content
    /// painted by `paint` (clipped, translated, measured), the wheel, then the
    /// bars on top.
    ///
    /// `paint` receives the canvas and a [`Frame`] in content coordinates; it
    /// lays out exactly as it would without the area.
    pub fn frame(
        &mut self,
        c: &dyn Canvas,
        bounds: Rect,
        f: &host::Frame,
        paint: impl FnOnce(&dyn Canvas, &host::Frame),
    ) -> ScrollAreaRun {
        let (mx, my) = f.mouse;
        let pressed = f.mouse_down && !self.prev_down;
        self.prev_down = f.mouse_down;
        if !f.mouse_down {
            self.drag = None;
            self.content_press = false;
        }

        // Clamp to what the content allows, on the device-pixel grid so the
        // pointer mapping matches the translation exactly.
        let need = self.bars_needed(bounds);
        let max = self.max_scroll(bounds);
        let s = c.scale().max(0.01);
        let snap = |v: f32| (v * s).round() / s;
        self.scroll = (snap(self.scroll.0.clamp(0.0, max.0)), snap(self.scroll.1.clamp(0.0, max.1)));

        // ── The bars: press, drag ─────────────────────────────────────────
        let mut on_bar = self.drag.is_some();
        for horizontal in [false, true] {
            let Some((mut bar, rail)) = self.bar(horizontal, bounds, need) else { continue };
            // Hit-tested unfolded: that is how it looks under the pointer.
            bar.expanded = true;
            if rail.contains(mx, my) {
                on_bar = true;
                if pressed {
                    match bar.part_at(rail, mx, my) {
                        Some(ScrollPart::Thumb) => {
                            let thumb = bar.thumb_rect(rail);
                            let start = if horizontal { thumb.left } else { thumb.top };
                            self.drag = Some(BarDrag { horizontal, grab: bar.along(mx, my) - start });
                        }
                        Some(part) => {
                            bar.apply_part(part);
                            self.set_axis(horizontal, bar.value() as f32);
                        }
                        None => {}
                    }
                }
            }
            if let Some(d) = self.drag.filter(|d| d.horizontal == horizontal) {
                bar.drag_to(rail, bar.along(mx, my), d.grab);
                self.set_axis(horizontal, bar.value() as f32);
            }
        }
        if pressed && !on_bar && bounds.contains(mx, my) {
            self.content_press = true;
        }

        // ── The content ───────────────────────────────────────────────────
        let (sx, sy) = self.scroll;
        let content_frame = self.content_frame(bounds, f, on_bar);
        if self.corner > 0.0 {
            c.push_clip_rounded(&bounds, self.corner);
        } else {
            c.push_clip(&bounds);
        }
        c.push_offset(-sx, -sy);
        c.begin_extent();
        paint(c, &content_frame);
        let reach = c.end_extent();
        c.pop_offset();
        if self.corner > 0.0 {
            c.pop_clip_rounded();
        } else {
            c.pop_clip();
        }
        let extent = reach.map_or((0.0, 0.0), |(x, y)| ((x - bounds.left).max(0.0), (y - bounds.top).max(0.0)));
        if (extent.0 - self.extent.0).abs() > 0.5 || (extent.1 - self.extent.1).abs() > 0.5 {
            // The bars follow the content on the very next frame.
            self.extent = extent;
            host::request_repaint_after(0);
        }

        // ── The wheel, unless a control inside used it ───────────────────
        let (wx, wy) = f.wheel_dip();
        if (wx != 0.0 || wy != 0.0)
            && !host::wheel_claimed()
            && bounds.contains(mx, my)
            && !host::over_popup(mx, my)
        {
            let (dx, dy) = if f.mods.shift && wx == 0.0 { (wy, 0.0) } else { (wx, wy) };
            let max = self.max_scroll(bounds);
            let next = ((sx + dx).clamp(0.0, max.0), (sy + dy).clamp(0.0, max.1));
            if next != self.scroll {
                self.scroll = (snap(next.0), snap(next.1));
                host::claim_wheel();
                host::request_repaint_after(0);
            }
        }

        // ── The bars, over the content ────────────────────────────────────
        let need = self.bars_needed(bounds);
        for horizontal in [false, true] {
            let Some((mut bar, rail)) = self.bar(horizontal, bounds, need) else { continue };
            let dragging = self.drag.is_some_and(|d| d.horizontal == horizontal);
            let hot = rail.contains(mx, my);
            bar.expanded = hot || dragging;
            bar.paint(c, rail, WidgetState::REST.hot(hot).pressed(dragging));
        }
        ScrollAreaRun { scroll: self.scroll, bars: need, on_bar }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use drive_app_controls::grid_splitter;

    fn tuple(r: Rect) -> (f32, f32, f32, f32) {
        (r.left, r.top, r.right, r.bottom)
    }

    // ── Panel: the placement is the engine's, to the DIP ─────────────────

    /// The whole point of this family: what a panel answers must be what
    /// `layout::layout` answers, for the same inputs, with nothing in between
    /// but the space crossing.
    #[test]
    fn layout_children_agrees_with_the_engine() {
        let panel = Panel::new()
            .with_padding(Padding::new(10.0, 8.0, 10.0, 8.0))
            .with_border(BorderStyle::FixedSingle)
            .with_design_size(Size::new(400.0, 300.0))
            .fill()
            .top(56.0)
            .left(240.0)
            .anchored(
                Rect::new(20.0, 100.0, 180.0, 130.0),
                AnchorStyles::LEFT.union(AnchorStyles::RIGHT),
            );

        let box_ = Rect::new(37.0, 11.0, 637.0, 511.0); // 600 × 500, moved
        let display = panel.display_rect(box_);
        let design = panel.display_rect(Rect::new(0.0, 0.0, 400.0, 300.0));
        let items: Vec<Item> = panel.children().iter().map(|c| c.item).collect();
        let expected = layout(display, design, &items);

        assert_eq!(
            panel.layout_children_local(box_).iter().map(|r| tuple(*r)).collect::<Vec<_>>(),
            expected.iter().map(|r| tuple(*r)).collect::<Vec<_>>(),
        );
    }

    /// The canvas rectangles are the local ones plus the content origin, and
    /// nothing else — no second padding, no re-derived border offset.
    #[test]
    fn canvas_rects_are_local_rects_plus_the_content_origin() {
        let panel = Panel::new()
            .with_padding(Padding::all(12.0))
            .with_border(BorderStyle::Fixed3D)
            .fill()
            .top(40.0);
        let box_ = Rect::new(100.0, 50.0, 500.0, 350.0);
        let o = panel.content_origin(box_);
        for (local, canvas) in
            panel.layout_children_local(box_).iter().zip(panel.layout_children(box_))
        {
            assert_eq!(tuple(canvas), tuple(translate(*local, o.x, o.y)));
        }
        // The origin is the box, plus the BORDER only: the padding is already in
        // the display rect's own origin (`local_display_rect`'s contract).
        assert_eq!((o.x, o.y), (102.0, 52.0));
    }

    /// The padding insets the children, and the border costs SIZE rather than
    /// offset — the correction the replica's parity harness forced.
    #[test]
    fn padding_insets_and_the_border_costs_size_not_offset() {
        let panel = Panel::new().with_padding(Padding::all(10.0)).fill();
        let d = panel.display_rect(Rect::new(0.0, 0.0, 200.0, 100.0));
        assert_eq!(tuple(d), (10.0, 10.0, 190.0, 90.0), "padding only");

        let bordered = Panel::new()
            .with_padding(Padding::all(10.0))
            .with_border(BorderStyle::FixedSingle)
            .fill();
        let d = bordered.display_rect(Rect::new(0.0, 0.0, 200.0, 100.0));
        assert_eq!(tuple(d), (10.0, 10.0, 188.0, 88.0), "the border takes two lines of size");
        assert_eq!(
            bordered.content_origin(Rect::new(0.0, 0.0, 200.0, 100.0)).x,
            1.0,
            "…and one line of offset",
        );

        // The filled child really lands inside the padding, on the canvas.
        let box_ = Rect::new(50.0, 20.0, 250.0, 120.0);
        assert_eq!(tuple(panel.layout_children(box_)[0]), (60.0, 30.0, 240.0, 110.0));
    }

    /// The docked bands stack in reverse z-order and `Fill` takes what is left —
    /// the arrangement every admin page needs, end to end.
    #[test]
    fn a_page_of_bands_places_the_way_the_toolkit_docks_them() {
        let page = Panel::new().fill().top(74.0).bottom(28.0).left(200.0);
        let r = page.layout_children(Rect::new(0.0, 0.0, 800.0, 600.0));
        assert_eq!(tuple(r[3]), (0.0, 0.0, 200.0, 600.0), "the left column, outermost");
        assert_eq!(tuple(r[2]), (200.0, 572.0, 800.0, 600.0), "the status band");
        assert_eq!(tuple(r[1]), (200.0, 0.0, 800.0, 74.0), "the header");
        assert_eq!(tuple(r[0]), (200.0, 74.0, 800.0, 572.0), "and the body gets the rest");
    }

    /// Placement is a pure function of the box: the same question twice gets the
    /// same answer, which the toolkit's « delta since the last pass » rule would
    /// not give (see the module docs).
    #[test]
    fn layout_children_is_idempotent() {
        let panel = Panel::new()
            .with_design_size(Size::new(200.0, 100.0))
            .anchored(
                Rect::new(10.0, 10.0, 100.0, 30.0),
                AnchorStyles::LEFT.union(AnchorStyles::RIGHT),
            );
        let grown = Rect::new(0.0, 0.0, 300.0, 100.0);
        let once = tuple(panel.layout_children(grown)[0]);
        let twice = tuple(panel.layout_children(grown)[0]);
        assert_eq!(once, twice);
        assert_eq!(once, (10.0, 10.0, 200.0, 30.0), "stretched by the +100");
    }

    /// A child's `min` and `max` reach the engine untouched.
    #[test]
    fn a_constrained_band_obeys_its_maximum() {
        let panel = Panel::new().top(120.0).constrain(|i| i.max = Size::new(0.0, 50.0));
        let r = panel.layout_children(Rect::new(0.0, 0.0, 200.0, 300.0))[0];
        assert_eq!(r.bottom - r.top, 50.0);
    }

    /// The content extent is the replica's, not a second copy: a hidden child
    /// contributes nothing, and the padding is already counted in the origin.
    #[test]
    fn content_size_matches_the_replica() {
        let panel = Panel::new()
            .with_padding(Padding::all(10.0))
            .fixed(Rect::new(10.0, 10.0, 120.0, 60.0))
            .fixed(Rect::new(10.0, 70.0, 200.0, 140.0))
            .constrain(|i| i.visible = false);
        let box_ = Rect::new(0.0, 0.0, 300.0, 200.0);

        let mut replica = kc::Panel::new();
        replica.padding = Padding::all(10.0);
        replica.control_mut().bounds = box_;
        for r in panel.layout_children_local(box_) {
            let mut child = kc::Panel::new();
            child.control_mut().bounds = r;
            replica.children.push(Box::new(child));
        }
        replica.children.iter_mut().nth(1).expect("two children").control_mut().visible = false;

        let expected = replica.scroll_content_size_within(replica.local_display_rect());
        assert_eq!(panel.content_size(box_), expected);
    }

    /// `child_at` answers in reverse order — the child on top wins.
    #[test]
    fn child_at_answers_topmost_first() {
        let panel = Panel::new()
            .fixed(Rect::new(0.0, 0.0, 100.0, 100.0))
            .fixed(Rect::new(50.0, 50.0, 150.0, 150.0));
        let box_ = Rect::new(0.0, 0.0, 200.0, 200.0);
        assert_eq!(panel.child_at(box_, 60.0, 60.0), Some(1));
        assert_eq!(panel.child_at(box_, 10.0, 10.0), Some(0));
        assert_eq!(panel.child_at(box_, 190.0, 190.0), None);
    }

    // ── Card ─────────────────────────────────────────────────────────────

    /// The header band is real geometry, not decoration: a child placed in a
    /// card starts below the title, because the band is folded into the
    /// replica's `Padding`.
    #[test]
    fn a_card_places_its_children_under_the_header() {
        let plain = Card::new().fill();
        let titled = Card::titled("Stockage").fill();
        let box_ = Rect::new(0.0, 0.0, 400.0, 300.0);

        assert_eq!(plain.header_height(), 0.0, "no title, no band");
        // `py-3` twice, the title line, and the `border-b` hairline.
        assert_eq!(titled.header_height(), space::MD * 2.0 + CARD_TITLE_LINE + card_metrics::RULE);

        let body = titled.layout_children(box_)[0];
        // The card's own 1-DIP `border`, the header band, then `p-4`.
        assert_eq!(body.top, card_metrics::RULE + titled.header_height() + space::LG);
        assert_eq!(tuple(body), tuple(titled.body_rect(box_)));
        assert_eq!(plain.layout_children(box_)[0].top, card_metrics::RULE + space::LG);
        assert_eq!(body.left, card_metrics::RULE + space::LG);
    }

    /// `flush` drops the body padding (the content meets the border), `dense`
    /// tightens every band — `@ui/Card.tsx`'s two density props.
    #[test]
    fn flush_and_dense_change_the_padding_the_web_way() {
        let box_ = Rect::new(0.0, 0.0, 400.0, 300.0);
        let flush = Card::titled("Membres").flush();
        let b = flush.body_rect(box_);
        assert_eq!((b.left, b.right), (1.0, 399.0), "bleeds to the border");
        assert_eq!(b.top, 1.0 + flush.header_height());
        assert_eq!(b.bottom, 299.0);

        let dense = Card::titled("Réglages").dense();
        assert_eq!(
            dense.header_height(),
            card_metrics::DENSE_PAD_Y * 2.0 + card_metrics::DENSE_TITLE_LINE + card_metrics::RULE,
        );
        assert_eq!(dense.body_rect(box_).left, 1.0 + card_metrics::DENSE_BODY_PAD);
    }

    /// The footer is a band at the bottom: `border-t`, `py-3` around the
    /// caller's content, and the body stops above it.
    #[test]
    fn a_footer_takes_the_bottom_band() {
        let box_ = Rect::new(0.0, 0.0, 400.0, 300.0);
        let card = Card::titled("Quota").with_footer(20.0);
        assert_eq!(card.footer_height(), 1.0 + 2.0 * card_metrics::PAD_Y + 20.0);
        let f = card.footer_rect(box_).expect("a footer");
        assert_eq!((f.top, f.bottom), (299.0 - card.footer_height(), 299.0));
        let fb = card.footer_body_rect(box_).expect("a footer body");
        assert_eq!(fb.bottom - fb.top, 20.0);
        assert_eq!(fb.left, 1.0 + card_metrics::PAD_X);
        assert_eq!(card.body_rect(box_).bottom, f.top - card_metrics::BODY_PAD);
        assert_eq!(card.body_band(box_).bottom, f.top);
        assert!(Card::titled("x").footer_rect(box_).is_none());
    }

    /// The header row: icon · title · actions with `gap-3`, the actions
    /// right-aligned, and a cluster taller than the title growing the band.
    #[test]
    fn the_header_row_places_icon_title_and_actions() {
        let box_ = Rect::new(0.0, 0.0, 400.0, 300.0);
        let card = Card::titled("Stockage").with_icon("HardDrive").with_actions(32.0, 32.0);
        let icon = card.icon_rect(box_).expect("icon");
        let title = card.title_rect(box_).expect("title");
        let actions = card.actions_rect(box_).expect("actions");
        assert_eq!(icon.left, 1.0 + card_metrics::PAD_X);
        assert_eq!(title.left, icon.right + card_metrics::HEADER_GAP);
        assert_eq!(actions.right, 399.0 - card_metrics::PAD_X);
        assert_eq!(title.right, actions.left - card_metrics::HEADER_GAP);
        assert_eq!(card.header_height(), card_metrics::PAD_Y * 2.0 + 32.0 + card_metrics::RULE);
        // An icon or actions alone still make a header, as on the web.
        assert!(Card::new().with_actions(20.0, 20.0).has_header());
        assert!(!Card::new().has_header());
    }

    /// The body band sits inside the frame, between header and footer.
    #[test]
    fn the_body_band_is_inside_the_frame() {
        let box_ = Rect::new(10.0, 10.0, 210.0, 210.0);
        let card = Card::titled("x");
        let band = card.body_band(box_);
        assert_eq!((band.left, band.right, band.bottom), (11.0, 209.0, 209.0));
        assert_eq!(band.top, 11.0 + card.header_height());
    }

    /// A subtitle grows the band by its own line and the gap above it.
    #[test]
    fn a_subtitle_grows_the_header_band() {
        let bare = Card::titled("Comptes");
        let with_sub = Card::titled("Comptes").with_subtitle("42 actifs");
        assert_eq!(
            with_sub.header_height() - bare.header_height(),
            CARD_SUBTITLE_GAP + CARD_SUBTITLE_LINE,
        );
    }

    // ── GroupBox ─────────────────────────────────────────────────────────

    /// The grouped children sit under the caption band and inside the frame —
    /// the replica's asymmetric display rectangle, unchanged.
    #[test]
    fn a_group_box_places_children_under_its_caption() {
        let group = GroupBox::titled("Quotas").with_padding(Padding::all(8.0)).fill();
        let box_ = Rect::new(20.0, 30.0, 320.0, 230.0);

        let mut replica = kc::GroupBox::new();
        replica.padding = Padding::all(8.0);
        replica.control_mut().bounds = Rect::new(0.0, 0.0, 300.0, 200.0);
        assert_eq!(tuple(group.display_rect(box_)), tuple(replica.local_display_rect()));

        let child = group.layout_children(box_)[0];
        assert_eq!(child.left, box_.left + replica.local_display_rect().left);
        assert!(child.top > box_.top + 8.0, "the caption band is above it");
    }

    /// A caption is capped to the box minus the inset on both sides, and the
    /// frame's gap follows the (capped) caption with `XS` of air.
    #[test]
    fn a_long_caption_is_capped_to_the_frame() {
        assert_eq!(caption_max_width(300.0), 300.0 - 2.0 * control::GROUP_LABEL_INSET);
        assert_eq!(caption_max_width(10.0), 0.0, "never negative");
        assert_eq!(caption_gap(12.0, 80.0), (12.0 - space::XS, 80.0 + space::XS));
    }

    /// A caption that fits gets a little slack (pixel snapping must not trim
    /// it), rounded up; one that does not is capped at the frame.
    #[test]
    fn a_short_caption_keeps_slack_for_pixel_snapping() {
        assert_eq!(caption_box_width(40.3, 200.0), (40.3_f32 + CAPTION_SLACK).ceil());
        assert!(caption_box_width(40.3, 200.0) > 40.3);
        assert_eq!(caption_box_width(500.0, 200.0), 200.0);
        assert_eq!(caption_box_width(199.5, 200.0), 200.0);
    }

    /// The content clip sits inside the frame's hairline.
    #[test]
    fn group_content_is_clipped_inside_the_frame() {
        let group = GroupBox::titled("Quotas").with_padding(Padding::all(8.0));
        let box_ = Rect::new(0.0, 0.0, 300.0, 200.0);
        let frame = group.frame_rect(box_);
        let inner = group.inner_rect(box_);
        assert_eq!(tuple(inner), tuple(deflate(frame, 1.0)));
        assert!(frame.top > 0.0, "the frame starts at the caption's centre line");
    }

    // ── Scopes and insets ────────────────────────────────────────────────

    /// A stroked surface costs at least its hairline inside the box, even with
    /// `BorderStyle::None`; an unstroked one costs only its border style.
    #[test]
    fn the_frame_inset_covers_the_hairline() {
        assert_eq!(frame_inset(true, 0.0), 1.0);
        assert_eq!(frame_inset(true, 2.0), 2.0);
        assert_eq!(frame_inset(false, 0.0), 0.0);
        assert_eq!(frame_inset(false, 1.0), 1.0);
        assert_eq!(inner_radius(8.0, 1.0), 7.0);
        assert_eq!(inner_radius(1.0, 3.0), 0.0);
        let d = deflate(Rect::new(0.0, 0.0, 4.0, 4.0), 3.0);
        assert!(d.right >= d.left && d.bottom >= d.top, "never inverted");
    }

    /// A visible panel clips its content to the border box, not the padded
    /// client rect (the web clips nothing at the padding).
    #[test]
    fn a_panel_clips_to_its_border_box() {
        let p = Panel::new().with_surface(Surface::Layer).with_padding(Padding::all(16.0));
        let box_ = Rect::new(0.0, 0.0, 200.0, 100.0);
        assert_eq!(tuple(p.border_box(box_)), (1.0, 1.0, 199.0, 99.0));
        assert_eq!(p.border_box_radius(), radius::TILE - 1.0);
        let bare = Panel::new();
        assert_eq!(tuple(bare.border_box(box_)), tuple(box_));
    }

    // ── Splitter ─────────────────────────────────────────────────────────

    /// The three rectangles tile the box exactly, and the bar is where the
    /// distance says.
    #[test]
    fn the_split_tiles_the_box() {
        let s = Splitter::vertical().with_distance(220.0);
        let box_ = Rect::new(10.0, 10.0, 810.0, 410.0);
        let r = s.arrange(box_);
        assert_eq!(tuple(r.panel1), (10.0, 10.0, 230.0, 410.0));
        assert_eq!(tuple(r.splitter), (230.0, 10.0, 234.0, 410.0), "SplitterWidth = 4");
        assert_eq!(tuple(r.panel2), (234.0, 10.0, 810.0, 410.0));
    }

    /// Dragging stops at both minimums — pane 1's own, and what pane 2's leaves.
    #[test]
    fn dragging_clamps_at_both_minimums() {
        let box_ = Rect::new(0.0, 0.0, 400.0, 200.0);
        let mut s = Splitter::vertical().with_distance(200.0).with_minimums(80.0, 120.0);
        let (min, max) = s.distance_range(box_);
        assert_eq!((min, max), (80.0, 400.0 - 4.0 - 120.0));

        s.drag_by(box_, -1000.0);
        assert_eq!(s.splitter_distance, min);
        s.drag_by(box_, 1000.0);
        assert_eq!(s.splitter_distance, max);
    }

    /// **The non-regression gate.** `grid_splitter::resize` is what Drive's
    /// sidebar and info pane resize with today; a drag here must land on the
    /// same DIP, or the port is a behaviour change wearing a new name.
    #[test]
    fn drag_matches_the_grid_splitter() {
        let box_ = Rect::new(0.0, 0.0, 900.0, 500.0);
        let (min1, min2, width) = (180.0_f32, 100.0_f32, 4.0_f32);
        // The far pane's minimum and the bar are what bound the near pane — the
        // same quantity `available_max` computes for the info pane.
        let max = grid_splitter::available_max(900.0 - width, min2, min1);

        for start in [180.0_f32, 300.0, 500.0, 780.0] {
            for delta in [-1000.0_f32, -50.0, -1.0, 0.0, 1.0, 50.0, 1000.0] {
                let mut s = Splitter::vertical()
                    .with_distance(start)
                    .with_minimums(min1, min2);
                s.splitter_width = width;
                s.drag_by(box_, delta);
                assert_eq!(
                    s.splitter_distance,
                    grid_splitter::resize(start, delta, min1, max, 0.0),
                    "start {start}, delta {delta}",
                );
            }
        }
    }

    /// `FixedPanel` decides who absorbs a resize — the replica's rule,
    /// truncation included.
    #[test]
    fn resizing_honours_the_fixed_panel() {
        for (fixed, expected) in [
            (FixedPanel::Panel1, 70.0),
            (FixedPanel::Panel2, 270.0),
            (FixedPanel::None, 116.0), // 70 × 500/300 = 116.67, truncated
        ] {
            let mut s = Splitter::vertical().with_distance(70.0).with_fixed_panel(fixed);
            s.resized(300.0, 500.0);
            assert_eq!(s.splitter_distance, expected, "{fixed:?}");
        }
    }

    /// The grab band is wider than the bar, centred on it, and it is what
    /// `hit_test` answers about.
    #[test]
    fn the_grab_band_is_wider_than_the_bar() {
        let s = Splitter::vertical().with_distance(200.0);
        let box_ = Rect::new(0.0, 0.0, 600.0, 400.0);
        let bar = s.splitter_rect(box_);
        let grip = s.grip_rect(box_);
        assert_eq!(grip.right - grip.left, control::SPLITTER);
        assert_eq!((bar.left + bar.right) * 0.5, (grip.left + grip.right) * 0.5, "centred");
        assert!(s.hit_test(box_, bar.left - 1.0, 200.0), "just outside the bar, on the band");
        assert!(!s.hit_test(box_, grip.left - 1.0, 200.0));
    }

    /// A pointer drives the bar to where it is, in canvas space.
    #[test]
    fn dragging_to_a_point_follows_the_pointer() {
        let box_ = Rect::new(100.0, 40.0, 700.0, 440.0);
        let mut s = Splitter::vertical().with_minimums(50.0, 50.0);
        s.drag_to(box_, 380.0, 200.0);
        assert_eq!(s.splitter_distance, 280.0);
        assert_eq!(s.splitter_rect(box_).left, 380.0);
    }

    /// The WAI-ARIA window-splitter keys: arrows along the axis, Shift for a
    /// large step, Home / End to the bounds, Enter to collapse and restore.
    #[test]
    fn the_splitter_follows_the_window_splitter_keys() {
        let box_ = Rect::new(0.0, 0.0, 400.0, 200.0);
        let mut s = Splitter::vertical().with_distance(200.0).with_minimums(80.0, 120.0);
        assert!(s.handle_key(box_, vk::RIGHT, Modifiers::NONE));
        assert_eq!(s.splitter_distance, 200.0 + scroll_metrics::SPLIT_STEP);
        assert!(s.handle_key(box_, vk::LEFT, Modifiers::SHIFT));
        assert_eq!(s.splitter_distance, 210.0 - scroll_metrics::SPLIT_STEP_LARGE);
        assert!(!s.handle_key(box_, vk::UP, Modifiers::NONE), "not along a vertical bar's axis");
        assert!(!s.handle_key(box_, vk::RIGHT, Modifiers::CTRL));

        let (min, max) = s.distance_range(box_);
        assert!(s.handle_key(box_, vk::END, Modifiers::NONE));
        assert_eq!(s.splitter_distance, max);
        assert!(s.handle_key(box_, vk::HOME, Modifiers::NONE));
        assert_eq!(s.splitter_distance, min);

        s.splitter_distance = 150.0;
        assert!(s.handle_key(box_, vk::ENTER, Modifiers::NONE));
        assert_eq!(s.splitter_distance, min, "collapsed");
        assert!(s.handle_key(box_, vk::ENTER, Modifiers::NONE));
        assert_eq!(s.splitter_distance, 150.0, "restored");
        assert_eq!(s.restore, None);

        let mut h = Splitter::horizontal().with_distance(100.0).with_minimums(20.0, 20.0);
        assert!(h.handle_key(box_, vk::DOWN, Modifiers::NONE));
        assert_eq!(h.splitter_distance, 110.0);
        assert_eq!(h.cursor(), Cursor::ResizeNS);
        assert_eq!(Splitter::vertical().cursor(), Cursor::ResizeEW);
    }

    // ── ScrollView ───────────────────────────────────────────────────────

    /// A stroked view keeps its content inside the hairline: the viewport is
    /// inset by one DIP even with `BorderStyle::None`, and the clip radius is
    /// concentric with the frame.
    #[test]
    fn the_viewport_stays_inside_the_frame() {
        let box_ = Rect::new(0.0, 0.0, 300.0, 200.0);
        let framed = ScrollView::new().with_surface(Surface::Layer);
        assert_eq!(tuple(framed.viewport(box_)), (1.0, 1.0, 299.0, 199.0));
        assert_eq!(framed.viewport_radius(box_), radius::TILE - 1.0);
        let bare = ScrollView::new();
        assert_eq!(tuple(bare.viewport(box_)), tuple(box_));
        let bordered = ScrollView::new().with_surface(Surface::Layer);
        let mut bordered = bordered;
        bordered.border_style = BorderStyle::FixedSingle;
        assert_eq!(tuple(bordered.viewport(box_)), (1.0, 1.0, 299.0, 199.0), "no double inset");
    }

    fn tall_view() -> (ScrollView, Rect) {
        let mut view = ScrollView::new();
        view.push(item(Rect::new(0.0, 0.0, 600.0, 1000.0), DockStyle::None, AnchorStyles::default()));
        (view, Rect::new(0.0, 0.0, 300.0, 400.0))
    }

    /// The wheel scrolls with the web's sign, Shift turns it horizontal, and a
    /// view at its end reports that it did not move (scroll chaining).
    #[test]
    fn the_wheel_scrolls_and_reports_the_end() {
        let (mut view, box_) = tall_view();
        assert!(view.wheel(box_, (0.0, 100.0), false));
        assert_eq!(view.offset(Axis::Vertical), 100.0);
        assert!(view.wheel(box_, (0.0, 100.0), true));
        assert_eq!(view.offset(Axis::Horizontal), 100.0, "Shift + wheel scrolls sideways");
        assert_eq!(view.offset(Axis::Vertical), 100.0);
        assert!(view.wheel(box_, (0.0, -10_000.0), false));
        assert!(!view.wheel(box_, (0.0, -100.0), false), "already at the top");
    }

    /// The browser's keys: a line is 40, a page 87.5 % of the viewport,
    /// Home / End go to the ends.
    #[test]
    fn the_keyboard_scrolls_like_a_browser() {
        let (mut view, box_) = tall_view();
        assert!(view.handle_key(box_, vk::DOWN, Modifiers::NONE));
        assert_eq!(view.offset(Axis::Vertical), scroll_metrics::LINE_STEP);
        assert!(view.handle_key(box_, vk::PAGE_DOWN, Modifiers::NONE));
        assert_eq!(view.offset(Axis::Vertical), 40.0 + 400.0 * scroll_metrics::PAGE_FRACTION);
        assert!(view.handle_key(box_, vk::SPACE, Modifiers::SHIFT));
        assert_eq!(view.offset(Axis::Vertical), 40.0);
        assert!(view.handle_key(box_, vk::END, Modifiers::NONE));
        assert_eq!(view.offset(Axis::Vertical), 600.0);
        assert!(view.handle_key(box_, vk::HOME, Modifiers::NONE));
        assert_eq!(view.offset(Axis::Vertical), 0.0);
        assert!(view.handle_key(box_, vk::RIGHT, Modifiers::NONE));
        assert_eq!(view.offset(Axis::Horizontal), 40.0);
        assert!(!view.handle_key(box_, vk::DOWN, Modifiers::CTRL), "Ctrl chords are not ours");
        assert!(!view.handle_key(box_, vk::letter('a'), Modifiers::NONE));
    }

    /// `scrollIntoView({ block: 'nearest' })`.
    #[test]
    fn ensure_visible_moves_the_least() {
        assert_eq!(nearest_delta(0.0, 100.0, 10.0, 20.0), 0.0, "already visible");
        assert_eq!(nearest_delta(0.0, 100.0, -30.0, -10.0), -30.0, "above: align the top");
        assert_eq!(nearest_delta(0.0, 100.0, 120.0, 150.0), 50.0, "below: align the bottom");
        assert_eq!(nearest_delta(0.0, 100.0, 50.0, 300.0), 50.0, "too tall: align the top");

        let (mut view, box_) = tall_view();
        assert!(view.ensure_visible(box_, Rect::new(0.0, 500.0, 50.0, 540.0)));
        assert_eq!(view.offset(Axis::Vertical), 140.0);
        assert!(!view.ensure_visible(box_, Rect::new(0.0, 100.0, 50.0, 140.0)));
    }

    /// The thumb drag's inverse mapping, and the bar the range family paints.
    #[test]
    fn the_thumb_maps_back_to_an_offset() {
        let (view, box_) = tall_view();
        assert_eq!(view.offset_for_thumb(box_, Axis::Vertical, 400.0, 160.0, 120.0), 300.0);
        assert_eq!(view.offset_for_thumb(box_, Axis::Vertical, 400.0, 160.0, 9_999.0), 600.0);
        assert_eq!(view.offset_for_thumb(box_, Axis::Vertical, 100.0, 100.0, 50.0), 0.0);
        assert!(view.scroll_bar(box_, Axis::Vertical).is_some());
        let short = ScrollView::new();
        assert!(short.scroll_bar(box_, Axis::Vertical).is_none(), "nothing to scroll");
    }

    /// The published range is what a scroll bar needs: an inclusive `Maximum`
    /// (so the span is the content), the viewport as a page, and a positive
    /// value.
    #[test]
    fn the_scroll_range_describes_the_content() {
        let mut view = ScrollView::new();
        view.push(item(Rect::new(0.0, 0.0, 200.0, 900.0), DockStyle::None, AnchorStyles::default()));
        let box_ = Rect::new(0.0, 0.0, 300.0, 400.0);

        let v = view.range(box_, Axis::Vertical);
        assert!(v.visible, "900 of content in a 400 viewport");
        assert_eq!(v.minimum, 0.0);
        assert_eq!(v.maximum, 899.0, "Maximum is inclusive: span = 900");
        assert_eq!(v.large_change, 400.0);
        assert_eq!(v.value, 0.0);

        let h = view.range(box_, Axis::Horizontal);
        assert!(!h.visible, "200 of content fits in 300");
    }

    /// Scrolling clamps, writes the replica's ≤ 0 convention, and moves the
    /// children by exactly that much.
    #[test]
    fn scrolling_clamps_and_shifts_the_children() {
        let mut view = ScrollView::new();
        view.push(item(Rect::new(0.0, 0.0, 200.0, 900.0), DockStyle::None, AnchorStyles::default()));
        let box_ = Rect::new(0.0, 0.0, 300.0, 400.0);
        let rest = view.layout_children(box_)[0].top;

        view.scroll_by(box_, 0.0, 120.0);
        assert_eq!(view.offset(Axis::Vertical), 120.0);
        assert_eq!(view.auto_scroll_position.y, -120.0, "the replica's own sign");
        assert_eq!(view.layout_children(box_)[0].top, rest - 120.0);

        view.scroll_by(box_, 0.0, 10_000.0);
        assert_eq!(view.offset(Axis::Vertical), 500.0, "900 − 400");
        view.scroll_by(box_, 0.0, -10_000.0);
        assert_eq!(view.offset(Axis::Vertical), 0.0);
        // Not `-0.0`: it compares equal to zero and prints as « -0 », which is
        // what a caller formatting the range would show.
        assert!(view.offset(Axis::Vertical).is_sign_positive(), "no signed zero");
        assert!(view.range(box_, Axis::Vertical).value.is_sign_positive());
    }

    /// The track is where the `range` family paints, and only there.
    #[test]
    fn the_track_hugs_the_far_edge_of_the_viewport() {
        let view = ScrollView::new().with_padding(Padding::all(8.0));
        let box_ = Rect::new(0.0, 0.0, 300.0, 400.0);
        let v = view.viewport(box_);
        let t = view.track(box_, Axis::Vertical);
        assert_eq!(tuple(t), (v.right - control::SCROLLBAR, v.top, v.right, v.bottom));
    }

    // ── Stack ────────────────────────────────────────────────────────────

    /// The column a console page writes by hand — `y += h + GAP`, each block as
    /// wide as the area — comes out of the flow engine unchanged.
    #[test]
    fn stack_matches_the_hand_written_column() {
        const GAP: f32 = 12.0;
        let heights = [196.0_f32, 148.0, 220.0];
        let area = Rect::new(40.0, 100.0, 840.0, 700.0);

        let mut stack = Stack::column(GAP);
        for h in heights {
            stack.push(h);
        }
        let got = stack.layout_children(area);

        let mut y = area.top;
        for (i, h) in heights.iter().enumerate() {
            assert_eq!(tuple(got[i]), (area.left, y, area.right, y + h), "block {i}");
            y += h + GAP;
        }
        assert_eq!(stack.content_extent(), heights.iter().sum::<f32>() + 3.0 * GAP);
    }

    /// The engine is the flow panel's, called with the cells the stack builds —
    /// no second arithmetic.
    #[test]
    fn stack_delegates_to_the_flow_engine() {
        let stack = Stack::column(8.0).block(40.0).block(60.0);
        let area = Rect::new(0.0, 0.0, 200.0, 400.0);
        let display = stack.display_rect(area);
        let cells = stack.cells(display);
        let expected = flow_layout(display, FlowDirection::TopDown, false, &cells);
        assert_eq!(
            stack.layout_children(area).iter().map(|r| tuple(*r)).collect::<Vec<_>>(),
            expected.iter().map(|r| tuple(*r)).collect::<Vec<_>>(),
        );
    }

    /// A row stretches the other way.
    #[test]
    fn a_row_stretches_across_the_height() {
        let stack = Stack::row(10.0).block(100.0).block(150.0);
        let r = stack.layout_children(Rect::new(0.0, 0.0, 500.0, 60.0));
        assert_eq!(tuple(r[0]), (0.0, 0.0, 100.0, 60.0));
        assert_eq!(tuple(r[1]), (110.0, 0.0, 260.0, 60.0));
    }

    // ── Surfaces ─────────────────────────────────────────────────────────

    /// The radii are the tokens, and the console's current card look is
    /// reachable so a page can move without a repaint.
    #[test]
    fn surfaces_carry_the_design_tokens() {
        assert_eq!(Surface::Card.radius(), radius::XL);
        assert_eq!(Surface::Layer.radius(), radius::TILE);
        assert_eq!(Surface::None.radius(), 0.0);
        assert_eq!(Card::new().on_layer().surface, Surface::Layer);
        assert_eq!(Card::new().raised().surface, Surface::Raised);
    }

    /// Every type says what it is, and reaches its replica.
    #[test]
    fn each_primitive_names_itself_and_owns_a_replica() {
        assert_eq!(Panel::new().type_name(), "Panel");
        assert_eq!(Card::new().type_name(), "Card");
        assert_eq!(GroupBox::new().type_name(), "GroupBox");
        assert_eq!(Splitter::vertical().type_name(), "Splitter");
        assert_eq!(ScrollView::new().type_name(), "ScrollView");
        assert_eq!(Stack::column(8.0).type_name(), "Stack");

        assert_eq!(Panel::new().model().type_name(), "Panel");
        assert_eq!(GroupBox::new().model().type_name(), "GroupBox");
        assert_eq!(Splitter::vertical().model().type_name(), "SplitContainer");
    }

    /// The replica is reached through `Deref`, so the .NET surface is not
    /// restated here — a panel's `padding` really is `ControlBase::padding`.
    #[test]
    fn the_dotnet_surface_comes_through_deref() {
        let mut panel = Panel::new();
        panel.padding = Padding::all(6.0);
        panel.border_style = BorderStyle::Fixed3D;
        panel.auto_scroll = true;
        panel.dock = DockStyle::Fill;
        assert_eq!(panel.control().padding.left, 6.0);
        assert_eq!(border_thickness(panel.border_style), 2.0);
        assert!(panel.auto_scroll);
        assert_eq!(panel.control().dock, DockStyle::Fill);
    }

    fn area_frame(mouse: (f32, f32), down: bool) -> host::Frame {
        host::Frame {
            size: (800.0, 600.0),
            mouse,
            mouse_down: down,
            right_down: false,
            middle_down: false,
            dismiss: false,
            scale: 1.0,
            client_origin: (100.0, 50.0),
            work_area: (0.0, 0.0, 1920.0, 1080.0),
            chrome_top: 0.0,
            mods: Modifiers::NONE,
            wheel: (0.0, 0.0),
            click_count: 0,
            window_focused: true,
        }
    }

    #[test]
    fn scroll_area_needs_a_bar_only_past_its_edges() {
        let bounds = Rect::new(10.0, 20.0, 410.0, 320.0);
        let mut area = ScrollArea::new();
        area.extent = (400.0, 300.0);
        assert_eq!(area.bars_needed(bounds), (false, false), "content that fits: no bar");
        area.extent = (400.0, 900.0);
        assert_eq!(area.bars_needed(bounds), (false, true));
        assert_eq!(area.max_scroll(bounds), (0.0, 600.0));
        area.extent = (650.0, 300.0);
        assert_eq!(area.bars_needed(bounds), (true, false));
        assert_eq!(area.max_scroll(bounds), (250.0, 0.0));
    }

    #[test]
    fn scroll_area_maps_the_pointer_into_content_coordinates() {
        let bounds = Rect::new(0.0, 100.0, 400.0, 500.0);
        let mut area = ScrollArea::new();
        area.scroll = (30.0, 200.0);
        let cf = area.content_frame(bounds, &area_frame((50.0, 150.0), false), false);
        assert_eq!(cf.mouse, (80.0, 350.0), "shifted by the scroll");
        assert_eq!(cf.client_origin, (70.0, -150.0), "screen_area stays true in content space");
        // Outside the region, or on a bar: nothing in the content lights up.
        let away = (host::POINTER_AWAY, host::POINTER_AWAY);
        assert_eq!(area.content_frame(bounds, &area_frame((50.0, 50.0), false), false).mouse, away);
        assert_eq!(area.content_frame(bounds, &area_frame((395.0, 150.0), false), true).mouse, away);
        // A press that started elsewhere does not reach the content…
        assert_eq!(area.content_frame(bounds, &area_frame((50.0, 150.0), true), false).mouse, away);
        // …while a drag that started on the content follows the pointer out.
        area.content_press = true;
        assert_eq!(area.content_frame(bounds, &area_frame((50.0, 50.0), true), false).mouse, (80.0, 250.0));
    }
}
