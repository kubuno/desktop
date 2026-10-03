//! Kubuno primitives — **range**: the four controls whose whole reason to exist
//! is a number moving between a minimum and a maximum.
//!
//! | primitive | replica it owns | where its pixels come from |
//! |---|---|---|
//! | [`ScrollBar`] | `kubuno_controls::range::{HScrollBar, VScrollBar}` | the shipping [`drive_app_controls::Scrollbar`] — **pixel-identical**, proven in the tests below |
//! | [`Slider`] | `kubuno_controls::range::TrackBar` | `@ui/RangeSlider` (`core/frontend/src/ui/RangeSlider.tsx`) |
//! | [`ProgressBar`] | `kubuno_controls::labels::ProgressBar` | `@ui/ProgressBar` (`core/frontend/src/ui/ProgressBar.tsx`) |
//! | [`NumericField`] / [`DomainField`] | `NumericUpDown` / `DomainUpDown` | `@ui/NumberInput` + `@ui/Input` |
//!
//! ## The two range models, and the conversion between them
//!
//! This family is the one place two *different* descriptions of "a range" meet.
//!
//! * The **replica** speaks WinForms: `Minimum`, `Maximum`, `Value`,
//!   `SmallChange`, `LargeChange`. The thumb's length is
//!   `LargeChange / (Maximum − Minimum + 1)` and the highest value a gesture
//!   reaches is `Maximum − LargeChange + 1`. That arithmetic was checked against
//!   the real toolkit and is **never re-derived here** — [`ScrollBar`] calls
//!   `thumb_fraction()` and `max_reachable_value()` and does nothing else.
//! * The **predecessor** speaks documents: `extent` (how long the content is),
//!   `viewport` (how much of it shows) and `scroll` (how far down we are), all
//!   in DIP.
//!
//! [`ScrollBar::set_content`] is the bridge, and it is the mapping WinForms
//! itself documents for a document scroller:
//!
//! ```text
//!   minimum      = 0
//!   maximum      = round(extent) − 1        ← content length, MINUS ONE
//!   large_change = round(viewport)          ← the page IS the viewport
//!   value        = round(scroll)
//! ```
//!
//! It is not a convention picked for convenience — it is the only mapping under
//! which the two agree term for term:
//!
//! ```text
//!   span             = maximum − minimum + 1        = extent
//!   thumb_fraction   = large_change / span          = viewport / extent
//!   max_reachable    = maximum − large_change + 1   = extent − viewport
//!   travel_fraction  = value / max_reachable        = scroll / max_scroll
//! ```
//!
//! …which are, in order, the four quantities `drive_app_controls::Scrollbar`
//! computes by hand. The `maximum = extent − 1` is the part that is easy to get
//! wrong: drop the `− 1` and every thumb is one unit too short and one unit too
//! far up, which is invisible on a 10 000-line document and glaring on a
//! four-row list.
//!
//! The conversion is **exact for whole-DIP inputs** and rounds otherwise: the
//! replica counts in whole scroll units and cannot represent half a pixel of
//! content. The parity test below therefore drives both with whole numbers,
//! which is what a scrolled list actually produces.

use std::ops::{Deref, DerefMut};

use drive_app_controls::themes::shape::{ShadowLayer, SHADOW_BLACK};
use kubuno_controls::enums::{HorizontalAlignment, LeftRightAlignment, Size};
use kubuno_controls::host::{vk, Modifiers};
use kubuno_controls::labels::{ProgressBar as ProgressBarModel, ProgressBarStyle};
use kubuno_controls::range::{
    DomainUpDown, HScrollBar, NumericUpDown, Orientation, OutOfRange,
    ScrollBar as ScrollBarModel, TickStyle, TrackBar, VScrollBar,
};
use kubuno_controls::Control;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::metrics::{control as m, height, pill, radius, text as text_size};
use crate::{Canvas, Rect, Theme, Widget, WidgetState};

// ─────────────────────────────────────────────────────────────────────────────
// Metrics this family needs and `crate::metrics` does not carry.
//
// Every one of them names its source. Nothing below is a literal in a paint
// body, and nothing below was chosen because it looked right.
// ─────────────────────────────────────────────────────────────────────────────

/// The chevrons in the two `RepeatButton`s at the ends of an expanded bar.
///
/// These are Segoe Fluent Icons codepoints drawn through
/// [`Canvas::formats`]`().icon_tiny`, which IS that face — the crate rule that
/// forbids arrows-as-text targets the *text* faces (Outfit carries no
/// `◄ ► ▲ ▼` and renders tofu). Substituting a vector chevron here would break
/// the one thing this primitive has to guarantee: being indistinguishable from
/// the bar the shell already paints.
const GLYPH_CHEVRON_UP: &str = "\u{E70E}";
const GLYPH_CHEVRON_DOWN: &str = "\u{E70D}";
const GLYPH_CHEVRON_LEFT: &str = "\u{E76B}";
const GLYPH_CHEVRON_RIGHT: &str = "\u{E76C}";

/// `@ui/ProgressBar`: the indeterminate sliver is `w-1/3` of the track.
const PROGRESS_SLIVER: f32 = 1.0 / 3.0;
/// `@ui/ProgressBar`: the `warnAt` / `dangerAt` defaults of the `auto` variant —
/// the whole point of the component being that a quota bar goes amber before it
/// goes red, at the SAME ratio everywhere.
const PROGRESS_WARN_AT: f32 = 0.75;
const PROGRESS_DANGER_AT: f32 = 0.90;
/// `@keyframes kb-progress-slide`: the sliver travels from `translateX(-100%)`
/// to `translateX(300%)` of its own width — four sliver-widths in all.
const PROGRESS_SLIDE_FROM: f32 = -1.0;
const PROGRESS_SLIDE_TO: f32 = 3.0;

/// `@ui/RangeSlider`'s thumb halo: `box-shadow: 0 0 0 2px #fff`.
///
/// **Derived, not restated**: the metric table carries both the disc
/// ([`crate::metrics::control::SLIDER_DISC`], the web's `thumb(size = 12)`) and
/// the disc-plus-halo the pointer has to hit
/// ([`crate::metrics::control::SLIDER_THUMB`], 16). The halo is what is left
/// over on each side, so the three numbers cannot disagree.
const SLIDER_THUMB_RING: f32 = (m::SLIDER_THUMB - m::SLIDER_DISC) / 2.0;
/// The same CSS rule's drop shadow: `0 1px 3px rgba(0,0,0,0.35)`. Pure black,
/// not [`drive_app_controls::themes::shape::SHADOW_GREY`].
const SLIDER_THUMB_SHADOW: [ShadowLayer; 1] =
    [ShadowLayer { dy: 1.0, blur: 3.0, spread: 0.0, opacity: 0.35 }];
/// Tick marks. The web slider has none, so these come from the replica's own
/// `TRACKBAR_TICK_LEN` / `TRACKBAR_TICK_GAP` (private there), measured off the
/// WinForms reference sheet at 4× zoom.
const SLIDER_TICK_LEN: f32 = 4.0;
const SLIDER_TICK_GAP: f32 = 3.0;
/// A tick is one DIP across the track, as in the replica's painter.
const SLIDER_TICK_WIDTH: f32 = 1.0;

/// `@ui/NumberInput`: the spin column is `w-6`, and its chevrons `size={11}`.
const SPIN_COLUMN: f32 = 24.0;
const SPIN_GLYPH: f32 = 11.0;
/// `@ui/Input`: `px-3`.
const FIELD_PAD_X: f32 = 12.0;
/// `@ui/Input`: `focus:ring-2` — the accent ring of a focused field, drawn
/// inward. Same number `drive_app_controls::edit_box` uses for the same ring.
const FIELD_FOCUS_RING: f32 = 2.0;
/// A one-DIP rule: `border-l` between the field and its spin column, `border-b`
/// between the two spin buttons.
const FIELD_RULE: f32 = 1.0;
/// `@ui/NumberInput`: `disabled && 'opacity-50'` on the whole box.
const FIELD_DISABLED_OPACITY: f32 = 0.5;
/// `@ui/NumberInput`: a spin button at its bound is `disabled:opacity-40`.
const SPIN_DISABLED_OPACITY: f32 = 0.4;
/// The line box the selection band and the caret cover while a spinner is
/// edited — `edit_box::LINE_BOX` (private there), the same 20 DIP every
/// single-line Kubuno field highlights.
const EDIT_LINE_BOX: f32 = 20.0;
/// The caret rule: one DIP of `currentColor`, as `edit_box` draws it.
const EDIT_CARET_W: f32 = 1.0;
/// `edit_box`: « the web declares no `::selection` rule, so the field keeps the
/// browser highlight: `--color-primary` at 35 % ».
const EDIT_SELECTION_ALPHA: f32 = 0.35;

/// `@ui/RangeSlider`: `disabled && 'opacity-60'` on the whole slider.
const SLIDER_DISABLED_OPACITY: f32 = 0.6;
/// The keyboard focus ring around a slider's thumb: the same 2 DIP accent
/// ring every Kubuno control wears (`focus-visible:ring-2 ring-primary`),
/// drawn OUTSIDE the white halo so it reads as `ring-offset` — an accent ring
/// straight on the accent disc would merge with it and vanish (the defect the
/// composition audit caught). The slider reserves this much room around the
/// thumb so the ring never leaves the slider's bounds.
const SLIDER_FOCUS_RING: f32 = FIELD_FOCUS_RING;

/// `@ui/RangeSlider`'s value bubble: `px-1.5 py-0.5 text-xs` — 6 / 2 DIP of
/// padding around a 12 DIP line of `text-xs` (`line-height: 1rem` = 16).
const BUBBLE_PAD_X: f32 = 6.0;
const BUBBLE_PAD_Y: f32 = 2.0;
const BUBBLE_LINE: f32 = 16.0;
/// `-top-1`: the bubble's bottom edge floats 4 DIP above the track.
const BUBBLE_GAP: f32 = 4.0;
/// Tailwind's `shadow`: `0 1px 3px 0 rgb(0 0 0 / .1), 0 1px 2px -1px rgb(0 0 0 / .1)`.
const BUBBLE_SHADOW: [ShadowLayer; 2] = [
    ShadowLayer { dy: 1.0, blur: 3.0, spread: 0.0, opacity: 0.10 },
    ShadowLayer { dy: 1.0, blur: 2.0, spread: -1.0, opacity: 0.10 },
];
/// How far [`BUBBLE_SHADOW`] reaches past the bubble (`dy + blur`) — the
/// margin a floating surface needs around the bubble so the shadow is not cut
/// at the popup's edge.
pub const VALUE_BUBBLE_MARGIN: f32 = 4.0;

/// `@ui/ProgressBar`'s header (`label` / `showValue`): `mb-1` under a line of
/// `--kb-text-body` at the preflight's `line-height: 1.5`, `gap-2` between
/// the label and the value.
const PROGRESS_HEADER_LINE: f32 = text_size::BODY * 1.5;
const PROGRESS_HEADER_GAP: f32 = 4.0;
const PROGRESS_HEADER_SPACING: f32 = 8.0;

/// Windows' own scroll-bar button timing: the first repeat after the keyboard
/// delay (500 ms at the default setting), then one step every 50 ms while the
/// button stays pressed. The primitive owns no timer; a host reads these.
pub const SCROLL_REPEAT_DELAY_MS: u64 = 500;
pub const SCROLL_REPEAT_INTERVAL_MS: u64 = 50;

/// `c` with its alpha multiplied by `k` — how a CSS `opacity` on a whole
/// control is reproduced primitive by primitive.
fn faded(c: D2D1_COLOR_F, k: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: c.a * k, ..c }
}

/// CSS `cubic-bezier(x1, y1, x2, y2)` at time `t` (`0..=1`): solves the curve's
/// x for `t` by bisection (x is monotonic for `x1, x2` in `0..=1`, which CSS
/// requires), then returns its y.
fn css_cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let bez = |a: f32, b: f32, s: f32| {
        let u = 1.0 - s;
        3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
    };
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..32 {
        let mid = (lo + hi) / 2.0;
        if bez(x1, x2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez(y1, y2, (lo + hi) / 2.0)
}

// ═════════════════════════════════════════════════════════════════════════════
// Keyboard and wheel — shared by every control of the family
// ═════════════════════════════════════════════════════════════════════════════

/// A keyboard gesture on a range, independent of the key that produced it.
///
/// Each control maps keys onto these with its own table — a slider and a
/// scroll bar disagree on what `PageUp` means — then applies them with its own
/// step sizes (`SmallChange` / `LargeChange`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeKey {
    /// One small step toward the minimum.
    Decrease,
    /// One small step toward the maximum.
    Increase,
    /// One page toward the minimum.
    PageDecrease,
    /// One page toward the maximum.
    PageIncrease,
    /// Straight to the minimum.
    First,
    /// Straight to the maximum.
    Last,
}

impl RangeKey {
    /// The WAI-ARIA slider pattern, which is what the native
    /// `<input type="range">` inside `@ui/RangeSlider` implements: Right / Up
    /// increase, Left / Down decrease, `PageUp` / `PageDown` step a page UP /
    /// DOWN the value, `Home` / `End` jump to the ends. Unmodified keys only.
    pub fn for_slider(key: u16) -> Option<Self> {
        match key {
            vk::RIGHT | vk::UP => Some(RangeKey::Increase),
            vk::LEFT | vk::DOWN => Some(RangeKey::Decrease),
            vk::PAGE_UP => Some(RangeKey::PageIncrease),
            vk::PAGE_DOWN => Some(RangeKey::PageDecrease),
            vk::HOME => Some(RangeKey::First),
            vk::END => Some(RangeKey::Last),
            _ => None,
        }
    }

    /// A scrolled area (the web's focused scroll container, WinForms'
    /// `ScrollBar`): Down / Right and `PageDown` move TOWARD the end of the
    /// content, Up / Left and `PageUp` back toward its start — the opposite of
    /// a slider's `PageUp`, because a scroll value grows downward.
    pub fn for_scroll(key: u16) -> Option<Self> {
        match key {
            vk::DOWN | vk::RIGHT => Some(RangeKey::Increase),
            vk::UP | vk::LEFT => Some(RangeKey::Decrease),
            vk::PAGE_DOWN => Some(RangeKey::PageIncrease),
            vk::PAGE_UP => Some(RangeKey::PageDecrease),
            vk::HOME => Some(RangeKey::First),
            vk::END => Some(RangeKey::Last),
            _ => None,
        }
    }

    /// The keys either table knows, for a host that polls them one by one.
    pub const KEYS: [u16; 8] = [
        vk::LEFT,
        vk::RIGHT,
        vk::UP,
        vk::DOWN,
        vk::PAGE_UP,
        vk::PAGE_DOWN,
        vk::HOME,
        vk::END,
    ];
}

/// Turns wheel notches — fractional on a touchpad — into whole steps, keeping
/// the remainder for the next frame so a slow two-finger scroll still moves a
/// stepped control instead of rounding every event down to zero.
#[derive(Debug, Clone, Copy, Default)]
pub struct WheelSteps {
    rest: f32,
}

impl WheelSteps {
    /// Adds `notches` (the host's sign: positive = toward the user / down)
    /// and returns the whole steps now due, in the same sign.
    pub fn take(&mut self, notches: f32) -> i32 {
        if !notches.is_finite() {
            return 0;
        }
        let total = self.rest + notches;
        let whole = total.trunc();
        self.rest = total - whole;
        whole as i32
    }

    /// Drops a pending fraction — when the pointer leaves the control.
    pub fn reset(&mut self) {
        self.rest = 0.0;
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// ScrollBar
// ═════════════════════════════════════════════════════════════════════════════

/// The part of a scroll bar under the pointer — the WinForms hit zones, which
/// are also the four gestures the replica implements (`line_up`, `page_up`,
/// dragging, `page_down`, `line_down`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollPart {
    /// The arrow button at the low end (left / top). Only reachable while the
    /// bar is expanded: at rest there are no buttons.
    ArrowLow,
    /// The track before the thumb — a page step toward `Minimum`.
    PageLow,
    /// The grip.
    Thumb,
    /// The track after the thumb — a page step toward `Maximum`.
    PageHigh,
    /// The arrow button at the high end (right / bottom).
    ArrowHigh,
}

/// `HScrollBar` or `VScrollBar` — WinForms has no instantiable "ScrollBar", the
/// orientation *is* the type, so this port keeps that shape instead of adding
/// an orientation field the toolkit does not have.
#[derive(Clone)]
enum Bar {
    H(HScrollBar),
    V(VScrollBar),
}

/// A Kubuno scroll bar.
///
/// The **arithmetic is entirely the replica's** — see the module header for the
/// content/viewport ⇄ min/max/large-change conversion — and the **pixels are
/// entirely the predecessor's**: a thin resting indicator that unfolds into a
/// full gutter with two chevron buttons, the web's `::-webkit-scrollbar` skin.
/// `range_geometry_matches_the_predecessor` below drives both from the same
/// inputs and compares every edge.
pub struct ScrollBar {
    inner: Bar,
    /// The pointer is inside the gutter, so the bar is unfolded: the track and
    /// its two buttons appear and the thumb widens from 2 DIP to 8.
    ///
    /// A **Kubuno concept**: a WinForms bar is always expanded, and the replica
    /// has nothing to say about it.
    pub expanded: bool,
    /// The resting indicator's fade-out, `0.0..=1.0` — the predecessor's
    /// `alpha` argument. The control owns no timer (crate rule); the host
    /// drives it from `FADE_AFTER_MS` / `FADE_MS`.
    pub opacity: f32,
}

impl Deref for ScrollBar {
    type Target = ScrollBarModel;
    fn deref(&self) -> &ScrollBarModel {
        match &self.inner {
            Bar::H(h) => h,
            Bar::V(v) => v,
        }
    }
}

impl DerefMut for ScrollBar {
    fn deref_mut(&mut self) -> &mut ScrollBarModel {
        match &mut self.inner {
            Bar::H(h) => h,
            Bar::V(v) => v,
        }
    }
}

/// `rail`, shortened along its axis so the whole gutter lies inside the
/// rounded rectangle `frame` (corner radius `radius`) — what keeps a bar from
/// poking out of a rounded component at its corners.
///
/// A gutter running along an edge meets that edge's two corners; where the
/// corner's arc cuts into the gutter's thickness, the gutter starts further in.
/// A rail that stays clear of the corners (radius 0, or a gutter far enough
/// from the edge) is returned unchanged. The SAME rail must be used to paint
/// and to hit-test, since the thumb's geometry follows the rail's length.
pub fn fit_rail(rail: Rect, frame: Rect, radius: f32) -> Rect {
    let r = radius.min((frame.right - frame.left) / 2.0).min((frame.bottom - frame.top) / 2.0);
    if r <= 0.0 {
        return rail;
    }
    // How deep into the corner's square the gutter reaches, across its axis.
    let inset = |depth: f32| {
        let d = depth.clamp(0.0, r);
        if d <= 0.0 {
            0.0
        } else {
            r - (r * r - d * d).max(0.0).sqrt()
        }
    };
    let vertical = rail.bottom - rail.top >= rail.right - rail.left;
    if vertical {
        let depth = (rail.right - (frame.right - r)).max((frame.left + r) - rail.left);
        let i = inset(depth);
        Rect::new(rail.left, rail.top.max(frame.top + i), rail.right, rail.bottom.min(frame.bottom - i))
    } else {
        let depth = (rail.bottom - (frame.bottom - r)).max((frame.top + r) - rail.top);
        let i = inset(depth);
        Rect::new(rail.left.max(frame.left + i), rail.top, rail.right.min(frame.right - i), rail.bottom)
    }
}

/// The inside of a 1 DIP border drawn on `bounds` with corner `radius`: the
/// rounded box a scroll bar must stay within, so it neither crosses the
/// border line nor pokes out at a corner. Returns `(frame, radius)` for
/// [`fit_rail`] and [`paint_bar_in`].
pub fn inside_border(bounds: Rect, radius: f32) -> (Rect, f32) {
    let b = 1.0;
    (
        Rect::new(bounds.left + b, bounds.top + b, bounds.right - b, bounds.bottom - b),
        (radius - b).max(0.0),
    )
}

/// Paints `bar` in `rail` under a clip to the rounded `frame`: whatever the
/// bar draws (the unfolded track, its arrows, the thumb) never shows outside
/// the component's rounded outline. Pair it with [`fit_rail`] so the thumb's
/// ends are not cut either.
pub fn paint_bar_in(c: &dyn Canvas, bar: &ScrollBar, rail: Rect, state: WidgetState, frame: Rect, radius: f32) {
    if radius > 0.0 {
        c.push_clip_rounded(&frame, radius);
        bar.paint(c, rail, state);
        c.pop_clip_rounded();
    } else {
        c.push_clip(&frame);
        bar.paint(c, rail, state);
        c.pop_clip();
    }
}

impl ScrollBar {
    /// A horizontal bar, resting.
    pub fn horizontal() -> Self {
        Self { inner: Bar::H(HScrollBar::new()), expanded: false, opacity: 1.0 }
    }

    /// A vertical bar, resting.
    pub fn vertical() -> Self {
        Self { inner: Bar::V(VScrollBar::new()), expanded: false, opacity: 1.0 }
    }

    pub fn is_horizontal(&self) -> bool {
        matches!(self.inner, Bar::H(_))
    }

    /// Builder form of [`ScrollBar::expanded`].
    pub fn with_expanded(mut self, v: bool) -> Self {
        self.expanded = v;
        self
    }

    /// Builder form of [`ScrollBar::opacity`].
    pub fn with_opacity(mut self, v: f32) -> Self {
        self.opacity = v;
        self
    }

    /// Whether a bar is needed at all — the predecessor's `Scrollbar::new`
    /// returns `None` below this threshold, and the half-DIP slack is what stops
    /// a rounding error from producing a scroll bar for nothing.
    pub fn needed(extent: f32, viewport: f32) -> bool {
        extent > viewport + 0.5
    }

    /// Loads a content/viewport/scroll description into the replica's range
    /// model. See the module header for why this is the only mapping that makes
    /// the two agree.
    pub fn set_content(&mut self, extent: f32, viewport: f32, scroll: f32) {
        // The replica counts in whole scroll units; a fractional DIP of content
        // cannot be represented and is rounded to the nearest one.
        let extent_u = (extent.round() as i64).clamp(1, i32::MAX as i64) as i32;
        let viewport_u = (viewport.round() as i64).clamp(0, i32::MAX as i64) as i32;

        let bar: &mut ScrollBarModel = self;
        bar.set_minimum(0);
        bar.set_maximum(extent_u - 1);
        // `set_large_change` only refuses a NEGATIVE page, and `viewport_u` is
        // clamped at zero above, so this cannot fail.
        let _ = bar.set_large_change(viewport_u);
        // `set_value` refuses anything outside [Minimum, Maximum]; the target is
        // clamped into it first, which is also what the predecessor does when it
        // clamps `scroll / max_scroll` to `0..=1`.
        let target = scroll.round().clamp(0.0, (extent_u - 1) as f32) as i32;
        let _ = bar.set_value(target);
    }

    /// A bar already loaded from a content description, or `None` when there is
    /// nothing to scroll — the shape of the predecessor's constructor.
    pub fn from_content(horizontal: bool, extent: f32, viewport: f32, scroll: f32) -> Option<Self> {
        if !Self::needed(extent, viewport) {
            return None;
        }
        let mut bar = if horizontal { Self::horizontal() } else { Self::vertical() };
        bar.set_content(extent, viewport, scroll);
        Some(bar)
    }

    /// The gutter this bar occupies inside a `content` rectangle: a
    /// [`crate::metrics::control::SCROLLBAR`]-thick strip along the bottom edge
    /// (horizontal) or the right edge (vertical).
    ///
    /// Part of the conversion, not of the painting: [`Widget::paint`] takes the
    /// gutter it is given and never derives it.
    pub fn rail(&self, content: &Rect) -> Rect {
        if self.is_horizontal() {
            Rect::new(content.left, content.bottom - m::SCROLLBAR, content.right, content.bottom)
        } else {
            Rect::new(content.right - m::SCROLLBAR, content.top, content.right, content.bottom)
        }
    }

    /// The thumb's near-edge position as a fraction of the travel.
    ///
    /// Composed from the replica's public `max_reachable_value()` — the
    /// `Maximum − LargeChange + 1` rule and its clamping both stay down there.
    fn travel_fraction(&self) -> f32 {
        let bar: &ScrollBarModel = self;
        let reach = bar.max_reachable_value() - bar.minimum();
        if reach <= 0 {
            return 0.0;
        }
        ((bar.value() - bar.minimum()) as f32 / reach as f32).clamp(0.0, 1.0)
    }

    /// The thumb, inside the gutter `bounds`.
    ///
    /// Both numbers that shape it come from the replica: the LENGTH is
    /// `thumb_fraction()` (`LargeChange / span`) of the track, floored at
    /// [`crate::metrics::control::SCROLLBAR_THUMB_MIN`]; the POSITION is
    /// [`ScrollBar::travel_fraction`] of what is left over.
    /// `(first, travel, thumb_len)` along the axis: where the thumb's near edge
    /// sits at the minimum, how far it can move, and its length. The one place
    /// the thumb's geometry is derived — [`ScrollBar::thumb_rect`] and its
    /// inverse [`ScrollBar::value_at_thumb_start`] both read it.
    fn thumb_travel(&self, bounds: Rect) -> (f32, f32, f32) {
        let (origin, len) = if self.is_horizontal() {
            (bounds.left, bounds.right - bounds.left)
        } else {
            (bounds.top, bounds.bottom - bounds.top)
        };
        // Each arrow button eats one end of the track, but only when the bar is
        // expanded — at rest there is nothing but the thumb.
        let inset = if self.expanded { m::SCROLLBAR_ARROW } else { 0.0 };
        let track = len - 2.0 * inset;
        let thumb_len = (self.thumb_fraction() * track).max(m::SCROLLBAR_THUMB_MIN).min(track);
        (origin + inset, track - thumb_len, thumb_len)
    }

    pub fn thumb_rect(&self, bounds: Rect) -> Rect {
        let horizontal = self.is_horizontal();
        let (first, travel, thumb_len) = self.thumb_travel(bounds);
        let start = first + self.travel_fraction() * travel;

        let width = if self.expanded { m::SCROLLBAR_THUMB } else { m::SCROLLBAR_INDICATOR };
        // The thumb hugs the gutter's far edge with the same inset on both
        // sides — computed off the TOKEN thickness, as the predecessor does, so
        // an over-wide gutter does not move it. Since `metrics::control`
        // RE-EXPORTS the predecessor's own constants, the two are now the same
        // number by construction rather than by coincidence.
        if horizontal {
            let cy = bounds.bottom - (m::SCROLLBAR - width) / 2.0 - width / 2.0;
            Rect::new(start, cy - width / 2.0, start + thumb_len, cy + width / 2.0)
        } else {
            let cx = bounds.right - (m::SCROLLBAR - width) / 2.0 - width / 2.0;
            Rect::new(cx - width / 2.0, start, cx + width / 2.0, start + thumb_len)
        }
    }

    /// The two arrow buttons, low end first. Empty rectangles collapse to the
    /// ends when the bar is at rest, where there are no buttons to hit.
    fn arrow_rects(&self, bounds: Rect) -> (Rect, Rect) {
        let arrow = if self.expanded { m::SCROLLBAR_ARROW } else { 0.0 };
        if self.is_horizontal() {
            (
                Rect::new(bounds.left, bounds.top, bounds.left + arrow, bounds.bottom),
                Rect::new(bounds.right - arrow, bounds.top, bounds.right, bounds.bottom),
            )
        } else {
            (
                Rect::new(bounds.left, bounds.top, bounds.right, bounds.top + arrow),
                Rect::new(bounds.left, bounds.bottom - arrow, bounds.right, bounds.bottom),
            )
        }
    }

    /// Which part `(x, y)` lands on, in the space `bounds` was painted into.
    /// `None` when the point is outside the gutter.
    pub fn part_at(&self, bounds: Rect, x: f32, y: f32) -> Option<ScrollPart> {
        if !bounds.contains(x, y) {
            return None;
        }
        let horizontal = self.is_horizontal();
        let along = if horizontal { x } else { y };
        let (lo, hi) = self.arrow_rects(bounds);
        if self.expanded {
            if lo.contains(x, y) {
                return Some(ScrollPart::ArrowLow);
            }
            if hi.contains(x, y) {
                return Some(ScrollPart::ArrowHigh);
            }
        }
        let thumb = self.thumb_rect(bounds);
        let (t_lo, t_hi) =
            if horizontal { (thumb.left, thumb.right) } else { (thumb.top, thumb.bottom) };
        Some(if along < t_lo {
            ScrollPart::PageLow
        } else if along < t_hi {
            ScrollPart::Thumb
        } else {
            ScrollPart::PageHigh
        })
    }

    // ── Gestures ────────────────────────────────────────────────────────────
    //
    // Every one of them goes through the replica's own step gestures
    // (`line_up` … `scroll_to`), so the `Maximum − LargeChange + 1` ceiling
    // and its clamping stay down there. Each returns whether the value moved,
    // which is what tells a host to repaint and to keep an auto-repeat going.

    /// The coordinate along the bar's axis — `x` for a horizontal bar, `y` for
    /// a vertical one.
    pub fn along(&self, x: f32, y: f32) -> f32 {
        if self.is_horizontal() {
            x
        } else {
            y
        }
    }

    /// Applies a keyboard gesture: a line (`SmallChange`), a page
    /// (`LargeChange`) or an end. Map keys with [`RangeKey::for_scroll`].
    pub fn apply_key(&mut self, key: RangeKey) -> bool {
        let before = self.value();
        let bar: &mut ScrollBarModel = self;
        match key {
            RangeKey::Decrease => bar.line_up(),
            RangeKey::Increase => bar.line_down(),
            RangeKey::PageDecrease => bar.page_up(),
            RangeKey::PageIncrease => bar.page_down(),
            // `scroll_to` with the two sentinels the toolkit uses for
            // `SB_TOP` / `SB_BOTTOM`; a zero-page bar refuses, and stays put.
            RangeKey::First => {
                let _ = bar.scroll_to(i32::MIN);
            }
            RangeKey::Last => {
                let _ = bar.scroll_to(i32::MAX);
            }
        }
        self.value() != before
    }

    /// What a press on `part` does — the WinForms hit zones: an arrow steps a
    /// line, the track a page, the thumb nothing (it is dragged instead).
    /// A host repeats it every [`SCROLL_REPEAT_INTERVAL_MS`] after
    /// [`SCROLL_REPEAT_DELAY_MS`] while the button is held; a page repeat
    /// re-reads [`ScrollBar::part_at`] each time, so it stops by itself once
    /// the thumb has reached the pointer.
    pub fn apply_part(&mut self, part: ScrollPart) -> bool {
        match part {
            ScrollPart::ArrowLow => self.apply_key(RangeKey::Decrease),
            ScrollPart::ArrowHigh => self.apply_key(RangeKey::Increase),
            ScrollPart::PageLow => self.apply_key(RangeKey::PageDecrease),
            ScrollPart::PageHigh => self.apply_key(RangeKey::PageIncrease),
            ScrollPart::Thumb => false,
        }
    }

    /// Scrolls by `delta` units (DIP, for a bar loaded with
    /// [`ScrollBar::set_content`]) — a wheel turn, in the web's sign: positive
    /// moves toward the end. Clamped to the reachable range.
    pub fn scroll_by(&mut self, delta: f32) -> bool {
        if !delta.is_finite() {
            return false;
        }
        let before = self.value();
        let target = (before as f64 + delta.round() as f64).clamp(i32::MIN as f64, i32::MAX as f64) as i32;
        let bar: &mut ScrollBarModel = self;
        let _ = bar.scroll_to(target);
        self.value() != before
    }

    /// The value that puts the thumb's near edge at `start` (along the axis,
    /// in the space of `bounds`) — the inverse of [`ScrollBar::thumb_rect`],
    /// rounded to the nearest unit and clamped to the reachable range.
    pub fn value_at_thumb_start(&self, bounds: Rect, start: f32) -> i32 {
        let bar: &ScrollBarModel = self;
        let (lo, reach) = (bar.minimum(), bar.max_reachable_value() - bar.minimum());
        if reach <= 0 {
            return lo;
        }
        let (first, travel, _) = self.thumb_travel(bounds);
        if travel <= 0.0 {
            return lo;
        }
        let frac = ((start - first) / travel).clamp(0.0, 1.0);
        lo + (frac * reach as f32).round() as i32
    }

    /// Drags the thumb so the point grabbed `grab` units into it follows the
    /// pointer at `pointer` (along the axis). `grab` is the pointer's offset
    /// inside the thumb at the press, which keeps the thumb from jumping.
    pub fn drag_to(&mut self, bounds: Rect, pointer: f32, grab: f32) -> bool {
        let before = self.value();
        let target = self.value_at_thumb_start(bounds, pointer - grab);
        let bar: &mut ScrollBarModel = self;
        let _ = bar.scroll_to(target);
        self.value() != before
    }
}

impl Widget for ScrollBar {
    fn model(&self) -> &dyn Control {
        match &self.inner {
            Bar::H(h) => h,
            Bar::V(v) => v,
        }
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        // The LENGTH is the replica's (a bar is normally docked and keeps the
        // length it was given); the THICKNESS is Kubuno's 12, not the system's
        // 17 the replica would report.
        let replica = self.model().preferred_size(canvas);
        if self.is_horizontal() {
            Size::new(replica.width, m::SCROLLBAR)
        } else {
            Size::new(m::SCROLLBAR, replica.height)
        }
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        let f = canvas.formats();
        let fade = |c: D2D1_COLOR_F| D2D1_COLOR_F { a: c.a * self.opacity, ..c };

        if self.expanded {
            // The track and its two `RepeatButton`s.
            canvas.fill_rounded(&bounds, 0.0, &fade(t.scrollbar_track));
            let (lo_glyph, hi_glyph) = if self.is_horizontal() {
                (GLYPH_CHEVRON_LEFT, GLYPH_CHEVRON_RIGHT)
            } else {
                (GLYPH_CHEVRON_UP, GLYPH_CHEVRON_DOWN)
            };
            let (lo, hi) = self.arrow_rects(bounds);
            canvas.text(lo_glyph, &lo, &f.icon_tiny, &fade(t.text_secondary), true);
            canvas.text(hi_glyph, &hi, &f.icon_tiny, &fade(t.text_secondary), true);
        }

        let thumb = self.thumb_rect(bounds);
        // `::-webkit-scrollbar-thumb { border-radius: 4px }`, capped at half the
        // thickness so the 2 DIP resting indicator still reads as a pill.
        let thickness = if self.is_horizontal() {
            thumb.bottom - thumb.top
        } else {
            thumb.right - thumb.left
        };
        // The web CHANGES the thumb's colour on hover rather than brightening
        // it — the thumb is opaque in both palettes, so an alpha bump is a
        // no-op. `disabled` has no predecessor to match: a bar that cannot be
        // dragged fades to the border token.
        let colour = if state.disabled {
            t.card_stroke
        } else if state.hot || state.pressed {
            t.scrollbar_thumb_hover
        } else {
            t.scrollbar_thumb
        };
        canvas.fill_rounded(&thumb, radius::SM.min(thickness / 2.0), &fade(colour));
    }

    fn type_name(&self) -> &'static str {
        "ScrollBar"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Slider
// ═════════════════════════════════════════════════════════════════════════════

/// The part of a slider under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderPart {
    /// The filled side of the rail — before the thumb.
    TrackBefore,
    Thumb,
    /// The empty side of the rail — after the thumb.
    TrackAfter,
}

/// Where a slider's two tick rows sit on the cross axis, each as a `(lo, hi)`
/// band. A row is `None` when the current `TickStyle` does not ask for it.
#[derive(Debug, Clone, Copy, Default)]
struct TickBands {
    before: Option<(f32, f32)>,
    after: Option<(f32, f32)>,
}

impl TickBands {
    fn rows(self) -> [Option<(f32, f32)>; 2] {
        [self.before, self.after]
    }

    fn any(self) -> bool {
        self.before.is_some() || self.after.is_some()
    }
}

/// A Kubuno slider, over `kubuno_controls::range::TrackBar`.
///
/// Everything numeric is the replica's: `minimum` / `maximum` / `value`,
/// `small_change` / `large_change`, `tick_frequency` / `tick_style`,
/// `orientation`, the `value ⇄ offset` mapping (including a vertical bar's
/// value growing UPWARD) and the tick positions — which are placed at their own
/// value, so an uneven last gap is drawn uneven.
///
/// What Kubuno adds is the look of `@ui/RangeSlider`: a pill rail, an accent
/// fill, and a 12 DIP accent disc inside a 2 DIP halo.
pub struct Slider {
    inner: TrackBar,
}

impl Default for Slider {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for Slider {
    type Target = TrackBar;
    fn deref(&self) -> &TrackBar {
        &self.inner
    }
}

impl DerefMut for Slider {
    fn deref_mut(&mut self) -> &mut TrackBar {
        &mut self.inner
    }
}

impl Slider {
    /// A slider with the replica's range (`0..=10`, `SmallChange` 1,
    /// `LargeChange` 5) and **no ticks**: `@ui/RangeSlider` draws none, whereas
    /// a WinForms `TrackBar` defaults to `TickStyle::BottomRight`. Set
    /// `tick_style` back to get them.
    pub fn new() -> Self {
        let mut inner = TrackBar::new();
        inner.set_tick_style(TickStyle::None);
        Self { inner }
    }

    fn is_horizontal(&self) -> bool {
        self.inner.orientation() == Orientation::Horizontal
    }

    /// The track the thumb's CENTRE travels along, as `(origin, length)` on the
    /// main axis.
    ///
    /// Inset by half a thumb at each end so the thumb never leaves the
    /// rectangle it was handed. The web does not do this — a browser happily
    /// lets `translate(-50%)` hang the thumb over the ends — but a desktop
    /// control that paints outside its bounds corrupts its neighbour.
    ///
    /// The inset also leaves room for the keyboard focus ring
    /// ([`SLIDER_FOCUS_RING`]) around the thumb, so a focused slider parked at
    /// either end still draws its whole ring inside its bounds.
    fn track_span(&self, bounds: Rect) -> (f32, f32) {
        let half = Self::thumb_reach();
        if self.is_horizontal() {
            (bounds.left + half, (bounds.right - bounds.left - 2.0 * half).max(1.0))
        } else {
            (bounds.top + half, (bounds.bottom - bounds.top - 2.0 * half).max(1.0))
        }
    }

    /// How far the thumb's paint reaches from its centre: half the thumb, plus
    /// the focus ring outside it.
    fn thumb_reach() -> f32 {
        m::SLIDER_THUMB / 2.0 + SLIDER_FOCUS_RING
    }

    /// The pointer target: the whole length of the slider, one thumb-plus-ring
    /// thick across the rail — the web's native range input covers the track
    /// and its 20 DIP transparent thumb, not only the 6 DIP rail.
    pub fn hit_rect(&self, bounds: Rect) -> Rect {
        let c = self.cross_centre(bounds);
        let half = Self::thumb_reach();
        if self.is_horizontal() {
            Rect::new(bounds.left, c - half, bounds.right, c + half)
        } else {
            Rect::new(c - half, bounds.top, c + half, bounds.bottom)
        }
    }

    /// Assigns `value` folded into `[Minimum, Maximum]`; returns whether the
    /// value changed.
    pub fn set_value_clamped(&mut self, value: i32) -> bool {
        let before = self.inner.value();
        let v = value.clamp(self.inner.minimum(), self.inner.maximum());
        // Provably inside the range it was just clamped to.
        let _ = self.inner.set_value(v);
        self.inner.value() != before
    }

    /// Applies a keyboard gesture ([`RangeKey::for_slider`] maps the keys):
    /// arrows step `SmallChange`, `PageUp` / `PageDown` step `LargeChange`,
    /// `Home` / `End` jump to the ends. Returns whether the value moved.
    pub fn apply_key(&mut self, key: RangeKey) -> bool {
        let v = self.inner.value();
        let (small, large) = (self.inner.small_change(), self.inner.large_change());
        let target = match key {
            RangeKey::Decrease => v.saturating_sub(small),
            RangeKey::Increase => v.saturating_add(small),
            RangeKey::PageDecrease => v.saturating_sub(large),
            RangeKey::PageIncrease => v.saturating_add(large),
            RangeKey::First => self.inner.minimum(),
            RangeKey::Last => self.inner.maximum(),
        };
        self.set_value_clamped(target)
    }

    /// Wheel steps (from [`WheelSteps::take`], the host's sign: positive =
    /// toward the user). A notch away from the user RAISES the value by
    /// `SmallChange`, as a WinForms `TrackBar` does — the web's range input
    /// ignores the wheel, so the desktop convention decides here.
    pub fn apply_wheel(&mut self, steps: i32) -> bool {
        let delta = self.inner.small_change().saturating_mul(steps);
        self.set_value_clamped(self.inner.value().saturating_sub(delta))
    }

    /// Drags to the pointer: the value under `(x, y)`, clamped. Called on the
    /// press AND every frame while the button is held — the host captures the
    /// mouse, so the pointer keeps being reported outside the window.
    pub fn drag_to(&mut self, bounds: Rect, x: f32, y: f32) -> bool {
        let v = self.value_at(bounds, x, y);
        self.set_value_clamped(v)
    }

    /// Where the value bubble goes for `text`, in the space of `bounds`: centred
    /// on the thumb, its bottom [`BUBBLE_GAP`] above the RAIL (`-top-1
    /// -translate-y-full` on the web), so it floats OUTSIDE the slider. A
    /// vertical slider puts it beside the thumb, on the left.
    ///
    /// A floating surface: paint it in a `host::overlay` grown by
    /// [`VALUE_BUBBLE_MARGIN`] for its shadow, with
    /// [`Slider::paint_value_bubble`].
    pub fn value_bubble_rect(&self, canvas: &dyn Canvas, bounds: Rect, text: &str) -> Rect {
        let w = canvas.measure(text, &canvas.formats().caption_strong) + 2.0 * BUBBLE_PAD_X;
        let h = BUBBLE_LINE + 2.0 * BUBBLE_PAD_Y;
        let thumb = self.thumb_rect(bounds);
        let (cx, cy) = ((thumb.left + thumb.right) / 2.0, (thumb.top + thumb.bottom) / 2.0);
        let rail = self.rail_rect(bounds);
        if self.is_horizontal() {
            // Measured from the TRACK, as the web does (the bubble is
            // positioned against the track's wrapper), so it grazes the thumb's
            // halo by a DIP exactly like the original.
            let bottom = rail.top - BUBBLE_GAP;
            Rect::new(cx - w / 2.0, bottom - h, cx + w / 2.0, bottom)
        } else {
            let right = thumb.left - BUBBLE_GAP;
            Rect::new(right - w, cy - h / 2.0, right, cy + h / 2.0)
        }
    }

    /// Paints the value bubble into `rect` — `rounded-md bg-[accent]
    /// text-white text-xs font-semibold shadow`. An associated function, so a
    /// floating surface's `'static` paint closure needs only the text and the
    /// rectangle, not the slider.
    pub fn paint_value_bubble(canvas: &dyn Canvas, rect: Rect, text: &str) {
        let t = canvas.theme();
        canvas.draw_shadow(&rect, radius::SM, &BUBBLE_SHADOW, SHADOW_BLACK);
        canvas.fill_rounded(&rect, radius::SM, &t.accent);
        canvas.text(text, &rect, &canvas.formats().caption_strong, &t.accent_foreground, true);
    }

    /// The rail's centre on the CROSS axis.
    fn cross_centre(&self, bounds: Rect) -> f32 {
        if self.is_horizontal() {
            (bounds.top + bounds.bottom) / 2.0
        } else {
            (bounds.left + bounds.right) / 2.0
        }
    }

    /// The unfilled rail.
    pub fn rail_rect(&self, bounds: Rect) -> Rect {
        let (origin, len) = self.track_span(bounds);
        let c = self.cross_centre(bounds);
        let half = m::SLIDER_TRACK / 2.0;
        if self.is_horizontal() {
            Rect::new(origin, c - half, origin + len, c + half)
        } else {
            Rect::new(c - half, origin, c + half, origin + len)
        }
    }

    /// The thumb: a square of [`crate::metrics::control::SLIDER_THUMB`] centred
    /// on the value, painted as a disc.
    ///
    /// Its centre is `TrackBar::thumb_offset` — the replica's mapping, so a
    /// vertical slider fills from the bottom without a sign flip here.
    pub fn thumb_rect(&self, bounds: Rect) -> Rect {
        let (origin, len) = self.track_span(bounds);
        let along = origin + self.inner.thumb_offset(len);
        let cross = self.cross_centre(bounds);
        let half = m::SLIDER_THUMB / 2.0;
        if self.is_horizontal() {
            Rect::new(along - half, cross - half, along + half, cross + half)
        } else {
            Rect::new(cross - half, along - half, cross + half, along + half)
        }
    }

    /// The filled part of the rail — left of the thumb, or BELOW it on a
    /// vertical slider, because a `TrackBar`'s value grows upward.
    pub fn fill_rect(&self, bounds: Rect) -> Rect {
        let rail = self.rail_rect(bounds);
        let thumb = self.thumb_rect(bounds);
        if self.is_horizontal() {
            Rect::new(rail.left, rail.top, (thumb.left + thumb.right) / 2.0, rail.bottom)
        } else {
            Rect::new(rail.left, (thumb.top + thumb.bottom) / 2.0, rail.right, rail.bottom)
        }
    }

    /// Which part `(x, y)` lands on. `None` outside `bounds`.
    pub fn part_at(&self, bounds: Rect, x: f32, y: f32) -> Option<SliderPart> {
        if !bounds.contains(x, y) {
            return None;
        }
        let thumb = self.thumb_rect(bounds);
        if thumb.contains(x, y) {
            return Some(SliderPart::Thumb);
        }
        let (along, centre) = if self.is_horizontal() {
            (x, (thumb.left + thumb.right) / 2.0)
        } else {
            (y, (thumb.top + thumb.bottom) / 2.0)
        };
        // "Before" is toward the MINIMUM, which on a vertical bar is the bottom.
        let before = if self.is_horizontal() { along < centre } else { along > centre };
        Some(if before { SliderPart::TrackBefore } else { SliderPart::TrackAfter })
    }

    /// The value a click or a drag at `(x, y)` designates — the replica's
    /// `value_at_offset`, which rounds to the nearest unit and clamps.
    pub fn value_at(&self, bounds: Rect, x: f32, y: f32) -> i32 {
        let (origin, len) = self.track_span(bounds);
        let along = if self.is_horizontal() { x } else { y };
        self.inner.value_at_offset(along - origin, len)
    }

    /// Snaps `value` to the nearest tick.
    ///
    /// The ticks are the replica's [`TrackBar::tick_values`] — `Minimum`, then
    /// every `TickFrequency`, then `Maximum` — so the last interval is SHORTER
    /// when the range is not a whole multiple of the frequency, and snapping
    /// respects that instead of assuming an even grid. A non-positive
    /// frequency leaves only the two ends, and snapping then goes to whichever
    /// end is nearer.
    pub fn snap_to_tick(&self, value: i32) -> i32 {
        let mut best = self.inner.minimum();
        let mut best_d = i64::MAX;
        for tick in self.inner.tick_values() {
            let d = (tick as i64 - value as i64).abs();
            if d < best_d {
                best_d = d;
                best = tick;
            }
        }
        best
    }

    /// The two tick rows on the cross axis, `None` on a side `TickStyle` does
    /// not ask for — which is what makes `BottomRight`, `TopLeft`, `Both` and
    /// `None` four visibly different layouts rather than one.
    fn tick_bands(&self, bounds: Rect) -> TickBands {
        let style = self.inner.tick_style();
        let before = matches!(style, TickStyle::TopLeft | TickStyle::Both);
        let after = matches!(style, TickStyle::BottomRight | TickStyle::Both);
        let c = self.cross_centre(bounds);
        let edge = m::SLIDER_THUMB / 2.0 + SLIDER_TICK_GAP;
        TickBands {
            before: before.then_some((c - edge - SLIDER_TICK_LEN, c - edge)),
            after: after.then_some((c + edge, c + edge + SLIDER_TICK_LEN)),
        }
    }
}

impl Widget for Slider {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        // The LENGTH is the replica's default TrackBar extent (104 DIP); the
        // THICKNESS is Kubuno's — the thumb and its focus ring, or the tick rows
        // when they reach further. The rail is centred, so the thickness is
        // TWICE the furthest reach on either side: a one-sided tick row still
        // fits, rather than hanging below the bounds. WinForms' 45 DIP cross
        // axis is a Windows number, not a Kubuno one.
        let replica = self.inner.preferred_size(canvas);
        let bands = self.tick_bands(Rect::new(0.0, 0.0, 0.0, 0.0));
        let reach = bands
            .rows()
            .into_iter()
            .flatten()
            .map(|(lo, hi)| lo.abs().max(hi.abs()))
            .fold(Self::thumb_reach(), f32::max);
        let cross = 2.0 * reach;
        if self.is_horizontal() {
            Size::new(replica.width, cross)
        } else {
            Size::new(cross, replica.height)
        }
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        // No ground of its own: `@ui/RangeSlider` is a transparent wrapper
        // around a rail, so whatever surface holds it (a card, a form) shows
        // around the rail — an opaque band here read as a dark stripe inside
        // every card of the composition page.
        let t = canvas.theme();
        let horizontal = self.is_horizontal();
        let rail = self.rail_rect(bounds);
        let r = pill(m::SLIDER_TRACK);
        // `disabled && 'opacity-60'` — the whole slider fades, the accent does
        // not turn grey.
        let k = if state.disabled { SLIDER_DISABLED_OPACITY } else { 1.0 };

        // `@ui/RangeSlider`'s default `trackColor` is `rgba(0,0,0,0.10)`, which
        // is `drive_bar_track` verbatim — the token the desktop already uses for
        // every meter rail, and which has a dark-palette answer the CSS literal
        // does not.
        canvas.fill_rounded(&rail, r, &faded(t.drive_bar_track, k));

        // `fill = accent ?? var(--color-primary)`: the web does not change it on
        // hover or while dragging (only the cursor does), so neither does this.
        let fill = faded(t.accent, k);
        let filled = self.fill_rect(bounds);
        let filled_len =
            if horizontal { filled.right - filled.left } else { filled.bottom - filled.top };
        if filled_len > 0.0 {
            canvas.fill_rounded(&filled, r.min(filled_len / 2.0), &fill);
        }

        // Ticks, when the replica asks for them. Each is placed at its own
        // VALUE through the same mapping the thumb uses, so a tick and a thumb
        // parked on it line up exactly.
        let bands = self.tick_bands(bounds);
        if bands.any() {
            let (origin, len) = self.track_span(bounds);
            let half = SLIDER_TICK_WIDTH / 2.0;
            let tick = faded(t.text_tertiary, k);
            for value in self.inner.tick_values() {
                let along = origin + self.inner.offset_of(value, len);
                for (lo, hi) in bands.rows().into_iter().flatten() {
                    let mark = if horizontal {
                        Rect::new(along - half, lo, along + half, hi)
                    } else {
                        Rect::new(lo, along - half, hi, along + half)
                    };
                    canvas.fill_rounded(&mark, 0.0, &tick);
                }
            }
        }

        // The thumb: shadow, halo, disc. The halo is the surface colour rather
        // than the web's literal `#fff`, so it stays a halo in the dark palette.
        let thumb = self.thumb_rect(bounds);
        let thumb_r = pill(m::SLIDER_THUMB);
        if !state.disabled {
            canvas.draw_shadow(&thumb, thumb_r, &SLIDER_THUMB_SHADOW, SHADOW_BLACK);
        }
        canvas.fill_rounded(&thumb, thumb_r, &t.layer_background);
        let disc = thumb.inflate(-SLIDER_THUMB_RING, -SLIDER_THUMB_RING);
        canvas.fill_rounded(&disc, pill(m::SLIDER_DISC), &fill);

        // The keyboard focus ring, OUTSIDE the halo (`:focus-visible` only — a
        // click-drag does not light it). `stroke_rounded_w` strokes inward, so
        // the rectangle is the thumb grown by the ring's width.
        if state.show_focus_ring() && !state.disabled {
            let ring = thumb.inflate(SLIDER_FOCUS_RING, SLIDER_FOCUS_RING);
            canvas.stroke_rounded_w(&ring, pill(m::SLIDER_THUMB + 2.0 * SLIDER_FOCUS_RING), &t.accent, SLIDER_FOCUS_RING);
        }
    }

    /// A slider is grabbed anywhere along its length within a thumb's reach of
    /// the rail ([`Slider::hit_rect`]); the band beyond that is not a target.
    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        self.hit_rect(bounds).contains(x, y)
    }

    fn type_name(&self) -> &'static str {
        "Slider"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// ProgressBar
// ═════════════════════════════════════════════════════════════════════════════

/// `@ui/ProgressBar`'s `variant`. `Auto` is the default and the reason the
/// component exists: a quota bar must go amber before it goes red, at the same
/// ratio in every module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProgressVariant {
    #[default]
    Auto,
    Primary,
    Success,
    Warning,
    Danger,
}

/// The track's thickness — `@ui/ProgressBar`'s `size` prop, which offers
/// exactly these two and no others.
///
/// An earlier draft carried a third, `Hairline`, over the 4 DIP
/// `metrics::control::PROGRESS` of the day. That token turned out to be
/// invented — the web draws this bar at 6 or 8 and nowhere at 4 — and it has
/// since been removed from the table, so the variant goes with it rather than
/// being re-sourced: a size the design system does not offer is not a size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProgressSize {
    /// `size="sm"` (`h-1.5`).
    Sm,
    /// `size="md"` (`h-2`) — the component's own default.
    #[default]
    Md,
}

impl ProgressSize {
    pub fn track(self) -> f32 {
        match self {
            ProgressSize::Sm => m::PROGRESS_SM,
            ProgressSize::Md => m::PROGRESS_MD,
        }
    }
}

/// A Kubuno progress bar, over `kubuno_controls::labels::ProgressBar`.
///
/// `minimum` / `maximum` / `value` / `step` / `style` / `right_to_left_layout`
/// and the `Increment` / `PerformStep` behaviour (which clamps and does NOT
/// wrap) are all the replica's, reached through [`Deref`].
///
/// The **indeterminate** state is `ProgressBarStyle::Marquee`: the replica
/// already carries it, and `@ui/ProgressBar` already has the matching look
/// (`indeterminate` renders a `w-1/3` sliver instead of a fill), so the two
/// meet without a new field.
pub struct ProgressBar {
    inner: ProgressBarModel,
    pub variant: ProgressVariant,
    pub size: ProgressSize,
    /// Where the indeterminate sliver is, `0.0..=1.0`.
    ///
    /// The control owns no timer — the replica's `marquee_animation_speed` says
    /// as much — so the host advances this. It is the **eased** progress of
    /// `@keyframes kb-progress-slide`: the web's `ease-in-out` is applied by the
    /// caller, so no easing curve is invented here — [`ProgressBar::phase_at`]
    /// computes it from the web's own timing for a caller that has a clock.
    pub phase: f32,
    /// `@ui/ProgressBar`'s `label`: a line above the track, `truncate`d with an
    /// ellipsis when it does not fit (the value keeps its room).
    pub label: Option<String>,
    /// `@ui/ProgressBar`'s `showValue`: the rounded percentage (`42 %`),
    /// right-aligned on the header line.
    pub show_value: bool,
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for ProgressBar {
    type Target = ProgressBarModel;
    fn deref(&self) -> &ProgressBarModel {
        &self.inner
    }
}

impl DerefMut for ProgressBar {
    fn deref_mut(&mut self) -> &mut ProgressBarModel {
        &mut self.inner
    }
}

impl ProgressBar {
    pub fn new() -> Self {
        Self {
            inner: ProgressBarModel::new(),
            variant: ProgressVariant::default(),
            size: ProgressSize::default(),
            phase: 0.0,
            label: None,
            show_value: false,
        }
    }

    pub fn with_variant(mut self, v: ProgressVariant) -> Self {
        self.variant = v;
        self
    }

    pub fn with_size(mut self, s: ProgressSize) -> Self {
        self.size = s;
        self
    }

    /// Builder form of [`ProgressBar::label`].
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Builder form of [`ProgressBar::show_value`].
    pub fn with_value_shown(mut self, v: bool) -> Self {
        self.show_value = v;
        self
    }

    /// Whether the bar has a header line (a label, a value, or both).
    pub fn has_header(&self) -> bool {
        self.label.is_some() || self.show_value
    }

    /// The value as the header prints it: `${Math.round(pct)} %` — a plain
    /// space before the sign, as the web writes it.
    pub fn value_text(&self) -> String {
        format!("{} %", (self.inner.fraction() * 100.0).round() as i32)
    }

    /// `@keyframes kb-progress-slide` is `1.3s ease-in-out infinite`: one
    /// crossing every [`ProgressBar::SLIDE_MS`].
    pub const SLIDE_MS: u64 = 1300;

    /// The eased [`ProgressBar::phase`] at `ms` on a monotonic clock — CSS
    /// `ease-in-out`, i.e. `cubic-bezier(0.42, 0, 0.58, 1)`, over
    /// [`ProgressBar::SLIDE_MS`]. For a host animating the indeterminate
    /// sliver; the control itself still owns no timer.
    pub fn phase_at(ms: u64) -> f32 {
        let t = (ms % Self::SLIDE_MS) as f32 / Self::SLIDE_MS as f32;
        css_cubic_bezier(0.42, 0.0, 0.58, 1.0, t)
    }

    /// Turns the bar indeterminate, i.e. sets the replica's `Marquee` style.
    pub fn set_indeterminate(&mut self, v: bool) {
        self.inner.style =
            if v { ProgressBarStyle::Marquee } else { ProgressBarStyle::Continuous };
    }

    /// Whether progress is unknown.
    pub fn indeterminate(&self) -> bool {
        self.inner.style == ProgressBarStyle::Marquee
    }

    /// The variant actually painted: `Auto` resolves through the web's
    /// thresholds, anything else is taken as given (a download bar is not
    /// "dangerous" at 95 %).
    pub fn resolved_variant(&self) -> ProgressVariant {
        if self.variant != ProgressVariant::Auto {
            return self.variant;
        }
        let ratio = self.inner.fraction();
        if ratio >= PROGRESS_DANGER_AT {
            ProgressVariant::Danger
        } else if ratio >= PROGRESS_WARN_AT {
            ProgressVariant::Warning
        } else {
            ProgressVariant::Primary
        }
    }

    fn fill_colour(&self, t: &Theme, state: WidgetState) -> D2D1_COLOR_F {
        if state.disabled {
            return t.border_strong;
        }
        match self.resolved_variant() {
            ProgressVariant::Success => t.success,
            ProgressVariant::Warning => t.warning,
            ProgressVariant::Danger => t.danger,
            // `Auto` cannot survive `resolved_variant`; both remaining cases are
            // the accent.
            ProgressVariant::Primary | ProgressVariant::Auto => t.accent,
        }
    }

    /// The track, centred in `bounds` — the bar is a band, not the whole
    /// rectangle, so a caller can give it a row and get it vertically centred.
    ///
    /// With a header ([`ProgressBar::has_header`]) the header line comes first
    /// and the track follows it after `mb-1`, both centred as one block.
    pub fn track_rect(&self, bounds: Rect) -> Rect {
        let h = self.size.track();
        if self.has_header() {
            let block = PROGRESS_HEADER_LINE + PROGRESS_HEADER_GAP + h;
            let top = (bounds.top + bounds.bottom - block) / 2.0 + PROGRESS_HEADER_LINE + PROGRESS_HEADER_GAP;
            return Rect::new(bounds.left, top, bounds.right, top + h);
        }
        let cy = (bounds.top + bounds.bottom) / 2.0;
        Rect::new(bounds.left, cy - h / 2.0, bounds.right, cy + h / 2.0)
    }

    /// The header line above the track, or `None` without a header.
    pub fn header_rect(&self, bounds: Rect) -> Option<Rect> {
        if !self.has_header() {
            return None;
        }
        let track = self.track_rect(bounds);
        let bottom = track.top - PROGRESS_HEADER_GAP;
        Some(Rect::new(bounds.left, bottom - PROGRESS_HEADER_LINE, bounds.right, bottom))
    }

    /// The filled part, for a determinate bar. Honours the replica's
    /// `right_to_left_layout` by filling from the right edge.
    pub fn fill_rect(&self, bounds: Rect) -> Rect {
        let track = self.track_rect(bounds);
        let w = (track.right - track.left).max(0.0) * self.inner.fraction();
        if self.inner.right_to_left_layout {
            Rect::new(track.right - w, track.top, track.right, track.bottom)
        } else {
            Rect::new(track.left, track.top, track.left + w, track.bottom)
        }
    }

    /// The indeterminate sliver at the current [`ProgressBar::phase`].
    ///
    /// `w-1/3` of the track, travelling from `translateX(-100%)` to
    /// `translateX(300%)` of its own width — so it starts fully off the left
    /// edge and ends fully off the right one.
    pub fn sliver_rect(&self, bounds: Rect) -> Rect {
        let track = self.track_rect(bounds);
        let w = (track.right - track.left).max(0.0) * PROGRESS_SLIVER;
        let t = self.phase.clamp(0.0, 1.0);
        let shift = PROGRESS_SLIDE_FROM + (PROGRESS_SLIDE_TO - PROGRESS_SLIDE_FROM) * t;
        let x = track.left + shift * w;
        Rect::new(x, track.top, x + w, track.bottom)
    }
}

impl Widget for ProgressBar {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        // A bar is `w-full` on the web: it has no intrinsic width, so the length
        // stays the replica's (its designer size, or the toolkit's 100 DIP).
        // The height is the token track — never the toolkit's 23.
        let replica = self.inner.preferred_size(canvas);
        let header =
            if self.has_header() { PROGRESS_HEADER_LINE + PROGRESS_HEADER_GAP } else { 0.0 };
        Size::new(replica.width, header + self.size.track())
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        // No ground of its own: the web bar is a transparent `min-w-0` block
        // around a rounded track, so the holder's surface shows around it.
        let t = canvas.theme();
        let track = self.track_rect(bounds);

        // The header: `flex items-baseline justify-between gap-2`, the label
        // `min-w-0 truncate`, the value `shrink-0 tabular-nums` — the value
        // keeps its width and the label gives way with an ellipsis.
        if let Some(line) = self.header_rect(bounds) {
            let f = canvas.formats();
            let colour = if state.disabled { t.text_tertiary } else { t.text_secondary };
            let mut label_right = line.right;
            if self.show_value {
                let text = self.value_text();
                let w = canvas.measure(&text, &f.body).min(line.right - line.left);
                let r = Rect::new(line.right - w, line.top, line.right, line.bottom);
                canvas.text_aligned(&text, &r, &f.body, &colour, HorizontalAlignment::Right.dwrite());
                label_right = r.left - PROGRESS_HEADER_SPACING;
            }
            if let Some(label) = &self.label {
                if label_right > line.left {
                    let r = Rect::new(line.left, line.top, label_right, line.bottom);
                    canvas.text_ellipsis(label, &r, &f.body, &colour);
                }
            }
        }
        let h = self.size.track();
        let r = pill(h);

        // `rounded-full bg-surface-2` — and `overflow-hidden`, which is what
        // keeps the indeterminate sliver inside the pill at both ends.
        canvas.fill_rounded(&track, r, &t.surface_2);

        let colour = self.fill_colour(t, state);
        let bar = if self.indeterminate() { self.sliver_rect(bounds) } else { self.fill_rect(bounds) };
        let w = bar.right - bar.left;
        if w > 0.0 {
            canvas.push_clip_rounded(&track, r);
            canvas.fill_rounded(&bar, r.min(w / 2.0), &colour);
            canvas.pop_clip_rounded();
        }
    }

    fn type_name(&self) -> &'static str {
        "ProgressBar"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// NumericField / DomainField — the two spinners
// ═════════════════════════════════════════════════════════════════════════════

/// One of a spinner's two step buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpinPart {
    /// The upper button (`ChevronUp`): `+Increment`, or the previous item of a
    /// [`DomainField`].
    Up,
    /// The lower button (`ChevronDown`).
    Down,
}

/// Everything about a spinner's chrome that is not in [`WidgetState`].
#[derive(Debug, Clone, Copy)]
struct FieldLook {
    /// `UpDownAlign::Right` — the spin column on the right.
    on_right: bool,
    /// `@ui/NumberInput`'s `error`: a danger border, and a danger ring.
    invalid: bool,
    /// The step button under the pointer (`hover:bg-surface-2`).
    spin_hot: Option<SpinPart>,
    /// Whether each button can act — `disabled={atMax}` / `disabled={atMin}`.
    up_enabled: bool,
    down_enabled: bool,
}

/// The spin column of a spinner at `bounds`: one [`SPIN_COLUMN`]-wide strip on
/// the side `UpDownAlign` asks for.
fn spin_column(bounds: Rect, on_right: bool) -> Rect {
    if on_right {
        Rect::new(bounds.right - SPIN_COLUMN, bounds.top, bounds.right, bounds.bottom)
    } else {
        Rect::new(bounds.left, bounds.top, bounds.left + SPIN_COLUMN, bounds.bottom)
    }
}

/// The two step buttons — the column split at its middle (`flex-1` each).
fn spin_rects(bounds: Rect, on_right: bool) -> (Rect, Rect) {
    let column = spin_column(bounds, on_right);
    let mid = (column.top + column.bottom) / 2.0;
    (
        Rect::new(column.left, column.top, column.right, mid),
        Rect::new(column.left, mid, column.right, column.bottom),
    )
}

fn spin_part_at(bounds: Rect, on_right: bool, x: f32, y: f32) -> Option<SpinPart> {
    let (up, down) = spin_rects(bounds, on_right);
    if up.contains(x, y) {
        Some(SpinPart::Up)
    } else if down.contains(x, y) {
        Some(SpinPart::Down)
    } else {
        None
    }
}

/// The rectangle left for the text: `px-3` inside whatever the buttons left.
fn field_text_rect(bounds: Rect, on_right: bool) -> Rect {
    let column = spin_column(bounds, on_right);
    if on_right {
        Rect::new(bounds.left + FIELD_PAD_X, bounds.top, column.left - FIELD_PAD_X, bounds.bottom)
    } else {
        Rect::new(column.right + FIELD_PAD_X, bounds.top, bounds.right - FIELD_PAD_X, bounds.bottom)
    }
}

/// Draws the shared spinner chrome — `@ui/NumberInput`: an `h-9` `rounded-md`
/// box with a `w-6` spin column on the side `UpDownAlign` asks for — and returns
/// the rectangle left for the text.
///
/// Shared by [`NumericField`] and [`DomainField`] so the two cannot drift, which
/// is exactly why the replica layer has a `paint_updown_frame` of its own.
///
/// Paint order matters: the step buttons' hover ground is clipped to the
/// rounded box (`overflow-hidden`), and the border / focus ring goes LAST so a
/// hovered button never paints over it.
fn paint_field_frame(canvas: &dyn Canvas, bounds: Rect, look: FieldLook, state: WidgetState) -> Rect {
    let t = canvas.theme();
    // `disabled && 'opacity-50'` on the whole box.
    let k = if state.disabled { FIELD_DISABLED_OPACITY } else { 1.0 };

    // `bg-white`.
    canvas.fill_rounded(&bounds, radius::SM, &faded(t.layer_background, k));

    let (up, down) = spin_rects(bounds, look.on_right);
    let column = spin_column(bounds, look.on_right);

    // `hover:bg-surface-2` on an ENABLED step button, clipped to the box.
    if !state.disabled {
        let hot = match look.spin_hot {
            Some(SpinPart::Up) if look.up_enabled => Some(up),
            Some(SpinPart::Down) if look.down_enabled => Some(down),
            _ => None,
        };
        if let Some(r) = hot {
            canvas.push_clip_rounded(&bounds, radius::SM);
            canvas.fill_rounded(&r, 0.0, &t.surface_2);
            canvas.pop_clip_rounded();
        }
    }

    // `border-l border-border` between the field and its buttons…
    let rule_x = if look.on_right { column.left } else { column.right - FIELD_RULE };
    canvas.fill_rounded(
        &Rect::new(rule_x, bounds.top, rule_x + FIELD_RULE, bounds.bottom),
        0.0,
        &faded(t.card_stroke, k),
    );
    // …and `border-b` under the upper one.
    let mid = up.bottom;
    canvas.fill_rounded(
        &Rect::new(column.left, mid, column.right, mid + FIELD_RULE),
        0.0,
        &faded(t.card_stroke, k),
    );

    // Lucide `ChevronUp` / `ChevronDown`, as VECTOR geometry: the embedded text
    // face carries no arrow glyphs and would render tofu. `text-text-secondary
    // hover:text-text-primary`, and `disabled:opacity-40` at a bound.
    for (part, rect, enabled, name) in [
        (SpinPart::Up, up, look.up_enabled, "ChevronUp"),
        (SpinPart::Down, down, look.down_enabled, "ChevronDown"),
    ] {
        let hot = enabled && !state.disabled && look.spin_hot == Some(part);
        let base = if hot { t.text_primary } else { t.text_secondary };
        let alpha = k * if enabled { 1.0 } else { SPIN_DISABLED_OPACITY };
        canvas.vector_icon(name, &rect, SPIN_GLYPH, &faded(base, alpha));
    }

    // `border-border`, or `border-danger` for `error`; focused, the
    // `kb-field-focus` ring (danger for `error`), which subsumes the border.
    // A spinner is a TEXT input, so it shows its ring on any focus — the
    // browser's `:focus-visible` answers yes for every text field.
    if state.focused && !state.disabled {
        let ring = if look.invalid { t.danger } else { t.accent };
        canvas.stroke_rounded_w(&bounds, radius::SM, &ring, FIELD_FOCUS_RING);
    } else {
        let border = if look.invalid { t.danger } else { t.card_stroke };
        canvas.stroke_rounded(&bounds, radius::SM, &faded(border, k));
    }

    field_text_rect(bounds, look.on_right)
}

/// Paints `text` in the field's text rectangle, clipped to it — a long number
/// is cut at the padding like the web input's content, never spilled over the
/// spin column.
fn paint_field_text(
    canvas: &dyn Canvas,
    text_rect: Rect,
    text: &str,
    align: HorizontalAlignment,
    state: WidgetState,
) {
    let t = canvas.theme();
    let colour = if state.disabled { faded(t.text_primary, FIELD_DISABLED_OPACITY) } else { t.text_primary };
    let f = canvas.formats();
    // A string wider than the box keeps its START visible (the input's
    // scroll position at rest), whatever the alignment.
    let w = canvas.measure(text, &f.body);
    let (target, align) = if w <= text_rect.right - text_rect.left {
        (text_rect, align)
    } else {
        (Rect::new(text_rect.left, text_rect.top, text_rect.left + w, text_rect.bottom), HorizontalAlignment::Left)
    };
    canvas.push_clip(&text_rect);
    canvas.text_aligned(text, &target, &f.body, &colour, align.dwrite());
    canvas.pop_clip();
}

/// The width a spinner wants: its text, the horizontal padding, and the spin
/// column.
fn field_width(canvas: &dyn Canvas, text: &str, floor: f32) -> f32 {
    let text_w = canvas.measure(text, &canvas.formats().body);
    (text_w + 2.0 * FIELD_PAD_X + SPIN_COLUMN).max(floor)
}

// ─────────────────────────────────────────────────────────────────────────────
// NumericEdit — the text a spinner holds while it is being typed into
// ─────────────────────────────────────────────────────────────────────────────

/// What a key did to a [`NumericEdit`] — see [`NumericEdit::handle_key`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOutcome {
    /// Not an editing key: leave it for someone else.
    Ignored,
    /// The caret or the selection moved; the text did not change.
    Moved,
    /// The text changed.
    Edited,
    /// `Enter`: validate the text into the value.
    Commit,
    /// `Escape`: throw the text away and show the value again.
    Revert,
    /// `ArrowUp` / `ArrowDown` — the native number input's step keys.
    Step(SpinPart),
    /// `Ctrl+C`: put this on the clipboard.
    Copy(String),
    /// `Ctrl+X`: put this on the clipboard; it has already been removed.
    Cut(String),
    /// `Ctrl+V`: read the clipboard and hand it to [`NumericEdit::insert`].
    Paste,
}

/// The editing state of a spinner: its text, the caret and the selection
/// anchor (byte offsets, always on a character boundary).
///
/// Pure data with pure operations, so the whole of typing is testable without a
/// window; the host feeds it keys and text and paints it with
/// [`NumericField::paint_editing`]. What a spinner accepts is what a number can
/// hold — digits, a sign, a decimal separator, and hex digits in hexadecimal
/// mode — everything else typed or pasted is dropped, as `NumericUpDown` does
/// with non-numeric keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NumericEdit {
    text: String,
    caret: usize,
    anchor: usize,
}

impl NumericEdit {
    /// An edit over `text`, all of it selected — what focusing a field by
    /// keyboard does in every browser and in WinForms.
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let end = text.len();
        Self { text, caret: end, anchor: 0 }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The caret, in bytes.
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// The selection anchor, in bytes (equal to the caret when nothing is
    /// selected).
    pub fn anchor(&self) -> usize {
        self.anchor
    }

    /// `(start, end)` of the selection, in bytes.
    pub fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    pub fn has_selection(&self) -> bool {
        self.caret != self.anchor
    }

    pub fn selected_text(&self) -> &str {
        let (a, b) = self.selection();
        &self.text[a..b]
    }

    /// Places the caret at byte `at` (snapped back to a character boundary),
    /// extending the selection from the anchor when `extend` — a click, or a
    /// Shift+click / drag.
    pub fn set_caret(&mut self, at: usize, extend: bool) {
        let mut at = at.min(self.text.len());
        while !self.text.is_char_boundary(at) {
            at -= 1;
        }
        self.caret = at;
        if !extend {
            self.anchor = at;
        }
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.text.len();
    }

    fn prev_boundary(&self, at: usize) -> usize {
        self.text[..at].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self, at: usize) -> usize {
        self.text[at..].chars().next().map_or(at, |c| at + c.len_utf8())
    }

    /// Removes the selection; returns whether there was one.
    fn delete_selection(&mut self) -> bool {
        let (a, b) = self.selection();
        if a == b {
            return false;
        }
        self.text.replace_range(a..b, "");
        self.caret = a;
        self.anchor = a;
        true
    }

    /// Types or pastes `s` over the selection, keeping only what a number can
    /// hold (`hex` admits `a-f` too). Returns whether the text changed.
    pub fn insert(&mut self, s: &str, hex: bool) -> bool {
        let kept: String = s
            .chars()
            .filter(|c| {
                c.is_ascii_digit() || matches!(c, '-' | '+' | ',' | '.') || (hex && c.is_ascii_hexdigit())
            })
            .collect();
        if kept.is_empty() {
            return false;
        }
        self.delete_selection();
        self.text.insert_str(self.caret, &kept);
        self.caret += kept.len();
        self.anchor = self.caret;
        true
    }

    /// `Backspace`; with `to_start` (Ctrl) everything before the caret.
    pub fn backspace(&mut self, to_start: bool) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.caret == 0 {
            return false;
        }
        let from = if to_start { 0 } else { self.prev_boundary(self.caret) };
        self.text.replace_range(from..self.caret, "");
        self.caret = from;
        self.anchor = from;
        true
    }

    /// `Delete`; with `to_end` (Ctrl) everything after the caret.
    pub fn delete(&mut self, to_end: bool) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.caret >= self.text.len() {
            return false;
        }
        let to = if to_end { self.text.len() } else { self.next_boundary(self.caret) };
        self.text.replace_range(self.caret..to, "");
        true
    }

    /// Applies one key press. Arrows / Home / End move (Shift extends; Ctrl
    /// jumps to an end, a number being one word), Backspace / Delete erase,
    /// Ctrl+A selects all, Ctrl+C / X / V ask for the clipboard, Enter commits,
    /// Escape reverts, Up / Down step. Alt chords are never editing keys.
    pub fn handle_key(&mut self, key: u16, mods: Modifiers) -> EditOutcome {
        if mods.alt {
            return EditOutcome::Ignored;
        }
        let shift = mods.shift;
        let moved = |e: &mut Self, to: usize| {
            e.set_caret(to, shift);
            EditOutcome::Moved
        };
        match key {
            vk::LEFT => {
                // Without Shift, a selection collapses to its start first.
                if !shift && self.has_selection() && !mods.ctrl {
                    let (a, _) = self.selection();
                    self.set_caret(a, false);
                    return EditOutcome::Moved;
                }
                let to = if mods.ctrl { 0 } else { self.prev_boundary(self.caret) };
                moved(self, to)
            }
            vk::RIGHT => {
                if !shift && self.has_selection() && !mods.ctrl {
                    let (_, b) = self.selection();
                    self.set_caret(b, false);
                    return EditOutcome::Moved;
                }
                let to = if mods.ctrl { self.text.len() } else { self.next_boundary(self.caret) };
                moved(self, to)
            }
            vk::HOME => moved(self, 0),
            vk::END => {
                let end = self.text.len();
                moved(self, end)
            }
            vk::BACK => {
                if self.backspace(mods.ctrl) {
                    EditOutcome::Edited
                } else {
                    EditOutcome::Moved
                }
            }
            vk::DELETE => {
                if self.delete(mods.ctrl) {
                    EditOutcome::Edited
                } else {
                    EditOutcome::Moved
                }
            }
            vk::ENTER if !mods.ctrl => EditOutcome::Commit,
            vk::ESCAPE => EditOutcome::Revert,
            vk::UP if !mods.ctrl && !shift => EditOutcome::Step(SpinPart::Up),
            vk::DOWN if !mods.ctrl && !shift => EditOutcome::Step(SpinPart::Down),
            k if mods.ctrl && !shift && k == vk::letter('a') => {
                self.select_all();
                EditOutcome::Moved
            }
            k if mods.ctrl && !shift && k == vk::letter('c') => {
                EditOutcome::Copy(self.selected_text().to_string())
            }
            k if mods.ctrl && !shift && k == vk::letter('x') => {
                let cut = self.selected_text().to_string();
                if self.delete_selection() {
                    EditOutcome::Cut(cut)
                } else {
                    EditOutcome::Moved
                }
            }
            k if mods.ctrl && !shift && k == vk::letter('v') => EditOutcome::Paste,
            _ => EditOutcome::Ignored,
        }
    }
}

/// Reads a spinner's text back into a number: group separators and spaces are
/// dropped, the decimal separator of `field`'s `number_format` AND a plain `.`
/// are both read as the decimal point (a numeric keypad types `.` whatever the
/// locale), and hexadecimal mode reads base 16. `None` when nothing numeric is
/// left.
fn parse_numeric(field: &NumericUpDown, text: &str) -> Option<f64> {
    let fmt = field.number_format;
    let mut s: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{202F}' && *c != '\u{00A0}')
        .collect();
    if field.hexadecimal() {
        let neg = s.starts_with('-');
        let digits = s.trim_start_matches(['-', '+']);
        let v = i64::from_str_radix(digits, 16).ok()? as f64;
        return Some(if neg { -v } else { v });
    }
    if fmt.group_separator != fmt.decimal_separator && !fmt.group_separator.trim().is_empty() {
        s = s.replace(fmt.group_separator, "");
    }
    if fmt.decimal_separator != "." {
        s = s.replace(fmt.decimal_separator, ".");
    }
    let v: f64 = s.parse().ok()?;
    v.is_finite().then_some(v)
}

/// Where the edited text starts on screen: aligned as `TextAlign` asks while
/// it fits, otherwise scrolled so the caret stays in view — the input's own
/// horizontal scroll.
fn edit_origin(canvas: &dyn Canvas, text_rect: Rect, edit: &NumericEdit, align: HorizontalAlignment) -> f32 {
    let f = canvas.formats();
    let inner = text_rect.right - text_rect.left;
    let full = canvas.measure(edit.text(), &f.body);
    if full <= inner {
        return match align {
            HorizontalAlignment::Left => text_rect.left,
            HorizontalAlignment::Right => text_rect.right - full,
            HorizontalAlignment::Center => text_rect.left + (inner - full) / 2.0,
        };
    }
    let caret_w = canvas.measure(&edit.text()[..edit.caret()], &f.body);
    text_rect.left - (caret_w - inner).max(0.0)
}

/// A Kubuno numeric spinner, over `kubuno_controls::range::NumericUpDown`.
///
/// # The ordering trap, and why this type refuses to hit it
///
/// `NumericUpDown.Value` **throws** when it is handed a number outside
/// `[Minimum, Maximum]`, and `Maximum` defaults to **100**. So the obvious
/// three lines are wrong in the obvious way:
///
/// ```ignore
/// let mut n = NumericUpDown::new();
/// n.set_value(255.0)?;      // ← Err: 255 is outside [0, 100]
/// n.set_maximum(255.0);     //   too late
/// ```
///
/// The replica already refuses to swallow it (its setter returns a `Result`
/// rather than clamping), but a `Result` still has to be read. This type
/// removes the ordering from the caller's hands instead:
///
/// * [`NumericField::ranged`] is the only constructor that takes a range, and it
///   is applied before anything else exists;
/// * [`NumericField::with_value`] and [`NumericField::set_range_and_value`]
///   apply the range **first, in one call**, and validate before touching a
///   single field — a refused call leaves the control exactly as it was;
/// * [`NumericField::clamped`] is there for the caller who wants the toolkit's
///   *other* behaviour (fold into range) and should say so out loud.
///
/// `set_value` itself stays reachable through [`DerefMut`], with its `Result`,
/// for the caller who really does mean « exactly this, or tell me ».
///
/// # Typing
///
/// While focused, the field is edited through a [`NumericEdit`]
/// ([`NumericField::begin_edit`]) painted by [`NumericField::paint_editing`];
/// the text is validated into the value by [`NumericField::commit`] on Enter
/// and on focus loss — WinForms' `ValidateEditText` — folded into the range
/// and rounded to `DecimalPlaces`. Unparseable text is refused and the value
/// shown again.
pub struct NumericField {
    inner: NumericUpDown,
    /// `@ui/NumberInput`'s `error`: a danger border and a danger focus ring.
    pub invalid: bool,
    /// The step button under the pointer, lit `hover:bg-surface-2`. The host
    /// sets it from [`NumericField::spin_part_at`].
    pub spin_hot: Option<SpinPart>,
}

impl Default for NumericField {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for NumericField {
    type Target = NumericUpDown;
    fn deref(&self) -> &NumericUpDown {
        &self.inner
    }
}

impl DerefMut for NumericField {
    fn deref_mut(&mut self) -> &mut NumericUpDown {
        &mut self.inner
    }
}

impl NumericField {
    /// A spinner with the replica's defaults — including `Maximum = 100`.
    pub fn new() -> Self {
        Self { inner: NumericUpDown::new(), invalid: false, spin_hot: None }
    }

    /// A spinner over `[minimum, maximum]`, whatever the two are relative to the
    /// default range.
    ///
    /// `Maximum` is applied first and `Minimum` second, which is the order that
    /// works for **every** pair: raising `Maximum` can only drag `Minimum` down
    /// with it, never strand a value above the ceiling.
    pub fn ranged(minimum: f64, maximum: f64) -> Self {
        let mut f = Self::new();
        f.set_range(minimum, maximum);
        f
    }

    /// Widens or narrows the range, keeping `Minimum <= Maximum`. `Value` is
    /// folded back in by the replica's own setters.
    pub fn set_range(&mut self, minimum: f64, maximum: f64) {
        let (lo, hi) = if minimum <= maximum { (minimum, maximum) } else { (maximum, minimum) };
        self.inner.set_maximum(hi);
        self.inner.set_minimum(lo);
    }

    /// Builder form of `set_value`, so a field can be declared in one
    /// expression **after** its range.
    pub fn with_value(mut self, value: f64) -> Result<Self, OutOfRange> {
        self.inner.set_value(value)?;
        Ok(self)
    }

    /// Builder form of [`NumericField::invalid`].
    pub fn with_invalid(mut self, v: bool) -> Self {
        self.invalid = v;
        self
    }

    /// Range and value in ONE call, in the only order that works — and
    /// **atomically**: the arguments are checked against each other first, so a
    /// refusal leaves the control untouched rather than half-applied.
    pub fn set_range_and_value(
        &mut self,
        minimum: f64,
        maximum: f64,
        value: f64,
    ) -> Result<(), OutOfRange> {
        if minimum > maximum || value < minimum || value > maximum {
            return Err(OutOfRange { value, minimum, maximum });
        }
        self.set_range(minimum, maximum);
        self.inner.set_value(value)
    }

    /// Folds `value` into the current range and assigns it, returning what was
    /// actually stored.
    ///
    /// Cannot fail — and that is the point of it having a different name from
    /// `set_value`: clamping is a decision, not a fallback.
    pub fn clamped(&mut self, value: f64) -> f64 {
        let v = value.clamp(self.inner.minimum(), self.inner.maximum());
        // Provably inside the range that was just clamped to.
        let _ = self.inner.set_value(v);
        v
    }

    fn on_right(&self) -> bool {
        self.inner.up_down_align() == LeftRightAlignment::Right
    }

    fn look(&self) -> FieldLook {
        FieldLook {
            on_right: self.on_right(),
            invalid: self.invalid,
            spin_hot: self.spin_hot,
            up_enabled: self.can_step(SpinPart::Up),
            down_enabled: self.can_step(SpinPart::Down),
        }
    }

    /// The two step buttons at `bounds`, upper first.
    pub fn spin_rects(&self, bounds: Rect) -> (Rect, Rect) {
        spin_rects(bounds, self.on_right())
    }

    /// The step button under `(x, y)`, if any.
    pub fn spin_part_at(&self, bounds: Rect, x: f32, y: f32) -> Option<SpinPart> {
        spin_part_at(bounds, self.on_right(), x, y)
    }

    /// Where the text goes (`px-3` inside the box, beside the spin column) —
    /// the I-beam area, and what a click is mapped against.
    pub fn text_rect(&self, bounds: Rect) -> Rect {
        field_text_rect(bounds, self.on_right())
    }

    /// Whether `part` can act: `disabled={atMax}` / `disabled={atMin}`.
    pub fn can_step(&self, part: SpinPart) -> bool {
        match part {
            SpinPart::Up => self.inner.value() < self.inner.maximum(),
            SpinPart::Down => self.inner.value() > self.inner.minimum(),
        }
    }

    /// One press of a step button, or of ArrowUp / ArrowDown: the replica's
    /// `UpButton` / `DownButton` (`±Increment`, clamped). Returns whether the
    /// value moved.
    pub fn step(&mut self, part: SpinPart) -> bool {
        let before = self.inner.value();
        match part {
            SpinPart::Up => self.inner.up_button(),
            SpinPart::Down => self.inner.down_button(),
        }
        self.inner.value() != before
    }

    /// Wheel steps over a FOCUSED field (the browser's number input steps
    /// only then), in the host's sign: a notch away from the user steps up.
    pub fn apply_wheel(&mut self, steps: i32) -> bool {
        let part = if steps < 0 { SpinPart::Up } else { SpinPart::Down };
        let mut moved = false;
        for _ in 0..steps.unsigned_abs() {
            moved |= self.step(part);
        }
        moved
    }

    /// Starts editing: the displayed text, all selected.
    pub fn begin_edit(&self) -> NumericEdit {
        NumericEdit::new(self.inner.display_text())
    }

    /// The number `text` stands for, in this field's format — `None` when it
    /// is not a number. Not yet clamped nor rounded; see
    /// [`NumericField::commit`].
    pub fn parse_text(&self, text: &str) -> Option<f64> {
        parse_numeric(&self.inner, text)
    }

    /// Validates `edit` into the value: parsed, folded into the range and
    /// rounded to `DecimalPlaces` (`NumericUpDown.Constrain`). Returns `false`,
    /// leaving the value as it was, when the text is not a number — the
    /// caller then shows the value again.
    pub fn commit(&mut self, edit: &NumericEdit) -> bool {
        let Some(v) = self.parse_text(edit.text()) else {
            return false;
        };
        let v = if self.inner.hexadecimal() {
            v.trunc()
        } else {
            let scale = 10f64.powi(self.inner.decimal_places().clamp(0, 15));
            (v * scale).round() / scale
        };
        self.clamped(v);
        true
    }

    /// The byte offset in `edit` nearest to `x` — where a click in the text
    /// puts the caret.
    pub fn caret_at(&self, canvas: &dyn Canvas, bounds: Rect, edit: &NumericEdit, x: f32) -> usize {
        let text_rect = self.text_rect(bounds);
        let origin = edit_origin(canvas, text_rect, edit, self.inner.text_align());
        let f = canvas.formats();
        let text = edit.text();
        let mut best = (0usize, (x - origin).abs());
        for (i, c) in text.char_indices() {
            let end = i + c.len_utf8();
            let d = (origin + canvas.measure(&text[..end], &f.body) - x).abs();
            if d < best.1 {
                best = (end, d);
            }
        }
        best.0
    }

    /// Paints the field while it is typed into: the chrome, then `edit`'s
    /// selection band (`--color-primary` at 35 %), its text and — when
    /// `caret_on`, i.e. during the blink's visible phase — the caret. All of
    /// it clipped to the text rectangle, scrolled to keep the caret in view.
    pub fn paint_editing(
        &self,
        canvas: &dyn Canvas,
        bounds: Rect,
        state: WidgetState,
        edit: &NumericEdit,
        caret_on: bool,
    ) {
        let t = canvas.theme();
        let f = canvas.formats();
        let text_rect = paint_field_frame(canvas, bounds, self.look(), state);
        let origin = edit_origin(canvas, text_rect, edit, self.inner.text_align());
        let text = edit.text();

        let cy = (bounds.top + bounds.bottom) / 2.0;
        let line_top = (cy - EDIT_LINE_BOX / 2.0).max(bounds.top + FIELD_FOCUS_RING);
        let line_bottom = (cy + EDIT_LINE_BOX / 2.0).min(bounds.bottom - FIELD_FOCUS_RING);

        // The clip leaves one caret width past the right edge, so a caret at
        // the very end of an overflowing number stays visible.
        let clip = Rect::new(text_rect.left, bounds.top, text_rect.right + EDIT_CARET_W, bounds.bottom);
        canvas.push_clip(&clip);
        let (a, b) = edit.selection();
        if a != b {
            let x0 = origin + canvas.measure(&text[..a], &f.body);
            let x1 = origin + canvas.measure(&text[..b], &f.body);
            canvas.fill_rounded(
                &Rect::new(x0, line_top, x1, line_bottom),
                0.0,
                &faded(t.accent, EDIT_SELECTION_ALPHA),
            );
        }
        let w = canvas.measure(text, &f.body);
        canvas.text(text, &Rect::new(origin, bounds.top, origin + w + FIELD_PAD_X, bounds.bottom), &f.body, &t.text_primary, false);
        if caret_on {
            let x = origin + canvas.measure(&text[..edit.caret()], &f.body);
            canvas.fill_rounded(&Rect::new(x, line_top, x + EDIT_CARET_W, line_bottom), 0.0, &t.text_primary);
        }
        canvas.pop_clip();
    }
}

impl Widget for NumericField {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        // `@ui/NumberInput` is `h-9`, the same height as every other input in
        // the system — not the replica's font-derived `PreferredHeight`.
        Size::new(
            field_width(canvas, &self.inner.display_text(), self.inner.width()),
            height::BUTTON_MD,
        )
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        // No ground outside the rounded box: its corners show the holder's
        // surface, as the web's `rounded-md` does.
        let text = paint_field_frame(canvas, bounds, self.look(), state);
        // `TextAlign` is the replica's; a right-aligned spinner is the usual
        // choice for numbers and lines its digits up.
        paint_field_text(canvas, text, &self.inner.display_text(), self.inner.text_align(), state);
    }

    fn type_name(&self) -> &'static str {
        "NumericField"
    }
}

/// A Kubuno spinner over a list of strings, on
/// `kubuno_controls::range::DomainUpDown`.
///
/// Worth carrying: the replica holds the whole selection state machine, ends
/// included — `Wrap`, `Sorted` (which keeps the INDEX, not the item), and the
/// asymmetry where stepping down from « nothing selected » picks the first item
/// while stepping up from it picks nothing. Kubuno adds only the chrome, which
/// is `@ui/NumberInput`'s, shared with [`NumericField`].
pub struct DomainField {
    inner: DomainUpDown,
    /// A danger border and ring, as [`NumericField::invalid`].
    pub invalid: bool,
    /// The step button under the pointer, as [`NumericField::spin_hot`].
    pub spin_hot: Option<SpinPart>,
}

impl Default for DomainField {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for DomainField {
    type Target = DomainUpDown;
    fn deref(&self) -> &DomainUpDown {
        &self.inner
    }
}

impl DerefMut for DomainField {
    fn deref_mut(&mut self) -> &mut DomainUpDown {
        &mut self.inner
    }
}

impl DomainField {
    pub fn new() -> Self {
        Self { inner: DomainUpDown::new(), invalid: false, spin_hot: None }
    }

    /// A field over `items`, with the first one selected — the state a form
    /// actually wants, and one `set_selected_index` cannot reach on an empty
    /// list.
    pub fn with_items<I, S>(items: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut f = Self::new();
        for item in items {
            f.inner.add(item);
        }
        if !f.inner.items().is_empty() {
            // Provably valid: the list is not empty, so index 0 exists.
            let _ = f.inner.set_selected_index(0);
        }
        f
    }

    fn on_right(&self) -> bool {
        self.inner.up_down_align() == LeftRightAlignment::Right
    }

    /// Whether `part` can act: without `Wrap`, the upper button is spent on the
    /// first item and the lower one on the last.
    pub fn can_step(&self, part: SpinPart) -> bool {
        let n = self.inner.items().len() as i32;
        if n == 0 {
            return false;
        }
        if self.inner.wrap() {
            return true;
        }
        let i = self.inner.selected_index();
        match part {
            SpinPart::Up => i > 0,
            SpinPart::Down => i < n - 1,
        }
    }

    /// The two step buttons at `bounds`, upper first.
    pub fn spin_rects(&self, bounds: Rect) -> (Rect, Rect) {
        spin_rects(bounds, self.on_right())
    }

    /// The step button under `(x, y)`, if any.
    pub fn spin_part_at(&self, bounds: Rect, x: f32, y: f32) -> Option<SpinPart> {
        spin_part_at(bounds, self.on_right(), x, y)
    }

    /// One press of a step button (or ArrowUp / ArrowDown): `UpButton` is the
    /// PREVIOUS item, `DownButton` the next — the replica's machine. Returns
    /// whether the selection moved.
    pub fn step(&mut self, part: SpinPart) -> bool {
        let before = self.inner.selected_index();
        match part {
            SpinPart::Up => self.inner.select_previous(),
            SpinPart::Down => self.inner.select_next(),
        }
        self.inner.selected_index() != before
    }

    /// The keys a focused domain spinner answers: ArrowUp / ArrowDown step,
    /// Home / End jump to the ends of the list.
    pub fn apply_key(&mut self, key: u16) -> bool {
        match key {
            vk::UP => self.step(SpinPart::Up),
            vk::DOWN => self.step(SpinPart::Down),
            vk::HOME | vk::END => {
                let n = self.inner.items().len() as i32;
                if n == 0 {
                    return false;
                }
                let before = self.inner.selected_index();
                let target = if key == vk::HOME { 0 } else { n - 1 };
                let _ = self.inner.set_selected_index(target);
                self.inner.selected_index() != before
            }
            _ => false,
        }
    }
}

impl Widget for DomainField {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        // The widest item, not the current one: a field must not resize as the
        // selection moves.
        let widest = self
            .inner
            .items()
            .iter()
            .map(|i| field_width(canvas, i, 0.0))
            .fold(0.0_f32, f32::max);
        Size::new(widest.max(self.inner.width()), height::BUTTON_MD)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let look = FieldLook {
            on_right: self.on_right(),
            invalid: self.invalid,
            spin_hot: self.spin_hot,
            up_enabled: self.can_step(SpinPart::Up),
            down_enabled: self.can_step(SpinPart::Down),
        };
        let text = paint_field_frame(canvas, bounds, look, state);
        paint_field_text(canvas, text, self.inner.text(), self.inner.text_align(), state);
    }

    fn type_name(&self) -> &'static str {
        "DomainField"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use drive_app_controls::scrollbar::Scrollbar as OldScrollbar;

    /// `Rect` carries no `PartialEq` / `Debug`, so equality is spelled out.
    #[track_caller]
    fn assert_rect(a: Rect, b: Rect, what: &str) {
        let eps = 1e-3;
        assert!(
            (a.left - b.left).abs() < eps
                && (a.top - b.top).abs() < eps
                && (a.right - b.right).abs() < eps
                && (a.bottom - b.bottom).abs() < eps,
            "{what}: [{}, {}, {}, {}] != [{}, {}, {}, {}]",
            a.left,
            a.top,
            a.right,
            a.bottom,
            b.left,
            b.top,
            b.right,
            b.bottom,
        );
    }

    // ── The range-model conversion ───────────────────────────────────────────

    /// The mapping documented in the module header, term by term.
    #[test]
    fn the_content_model_maps_onto_the_winforms_range_model() {
        let mut bar = ScrollBar::vertical();
        bar.set_content(1000.0, 250.0, 0.0);

        assert_eq!(bar.minimum(), 0);
        assert_eq!(bar.maximum(), 999, "maximum is extent MINUS ONE");
        assert_eq!(bar.large_change(), 250, "the page IS the viewport");
        // span = 1000 ⇒ thumb covers a quarter of the track, like viewport/extent.
        assert!((bar.thumb_fraction() - 0.25).abs() < 1e-6);
        // max_reachable = maximum - large + 1 = 999 - 250 + 1 = 750 = extent - viewport.
        assert_eq!(bar.max_reachable_value(), 750);
        assert!((bar.travel_fraction() - 0.0).abs() < 1e-6);

        bar.set_content(1000.0, 250.0, 750.0);
        assert!((bar.travel_fraction() - 1.0).abs() < 1e-6, "fully scrolled");

        // Scrolling past the end clamps, exactly as the predecessor clamps t.
        bar.set_content(1000.0, 250.0, 5000.0);
        assert!((bar.travel_fraction() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_bar_is_needed_only_when_the_content_overflows() {
        assert!(!ScrollBar::needed(300.0, 300.0));
        assert!(!ScrollBar::needed(300.4, 300.0), "half a DIP of slack");
        assert!(ScrollBar::needed(301.0, 300.0));
        assert!(ScrollBar::from_content(false, 300.0, 300.0, 0.0).is_none());
        assert!(ScrollBar::from_content(false, 900.0, 300.0, 0.0).is_some());
    }

    // ── Geometry equality with the predecessor ───────────────────────────────

    /// **The non-regression gate.** For the same content description, the rebuilt
    /// bar must put the rail and the thumb where `drive_app_controls::Scrollbar`
    /// puts them — resting and expanded, both axes, both ends of the travel.
    #[test]
    fn scrollbar_geometry_matches_the_predecessor() {
        let content = Rect::new(20.0, 30.0, 320.0, 430.0);
        // Whole DIP, because the replica counts in whole scroll units.
        let cases: &[(f32, f32, f32)] = &[
            (1000.0, 400.0, 0.0),
            (1000.0, 400.0, 300.0),
            (1000.0, 400.0, 600.0), // the very end: extent - viewport
            (450.0, 400.0, 25.0),   // barely scrollable
            (40000.0, 400.0, 0.0),  // thumb pinned at the 24 DIP floor
            (40000.0, 400.0, 39600.0),
        ];

        for horizontal in [false, true] {
            // A horizontal bar's viewport is the content's WIDTH.
            let viewport_axis =
                if horizontal { content.right - content.left } else { content.bottom - content.top };
            for expanded in [false, true] {
                for &(extent, _, scroll) in cases {
                    let viewport = viewport_axis;
                    let old = OldScrollbar::new(&content, extent, scroll, horizontal, expanded)
                        .expect("the cases all overflow");

                    let mut new = if horizontal {
                        ScrollBar::horizontal()
                    } else {
                        ScrollBar::vertical()
                    };
                    new.expanded = expanded;
                    new.set_content(extent, viewport, scroll);

                    let rail = new.rail(&content);
                    assert_rect(rail, old.rail, "rail");
                    assert_rect(
                        new.thumb_rect(rail),
                        old.thumb,
                        &format!("thumb h={horizontal} exp={expanded} extent={extent} scroll={scroll}"),
                    );
                }
            }
        }
    }

    // ── Thumb geometry at the bounds ─────────────────────────────────────────

    #[test]
    fn the_thumb_sits_at_each_end_of_its_travel() {
        let rail = Rect::new(0.0, 0.0, m::SCROLLBAR, 400.0);
        let mut bar = ScrollBar::vertical();

        bar.set_content(1000.0, 400.0, 0.0);
        let at_min = bar.thumb_rect(rail);
        assert!((at_min.top - rail.top).abs() < 1e-3, "value = minimum ⇒ flush with the top");

        bar.set_content(1000.0, 400.0, 600.0);
        let at_max = bar.thumb_rect(rail);
        assert!(
            (at_max.bottom - rail.bottom).abs() < 1e-3,
            "value = max reachable ⇒ flush with the bottom"
        );

        // Same length at both ends — the thumb travels, it does not stretch.
        assert!(
            ((at_min.bottom - at_min.top) - (at_max.bottom - at_max.top)).abs() < 1e-3,
            "the thumb keeps its length"
        );
    }

    #[test]
    fn a_page_covering_the_whole_range_fills_the_track() {
        let rail = Rect::new(0.0, 0.0, m::SCROLLBAR, 400.0);
        let mut bar = ScrollBar::vertical();
        // LargeChange >= the whole range: the replica clamps the reported page to
        // the span, so the fraction is exactly 1 and the thumb IS the track.
        bar.set_content(100.0, 100.0, 0.0);
        let _ = bar.set_large_change(5000);
        assert!((bar.thumb_fraction() - 1.0).abs() < 1e-6);
        let thumb = bar.thumb_rect(rail);
        assert!((thumb.bottom - thumb.top - 400.0).abs() < 1e-3);
    }

    #[test]
    fn the_thumb_never_shrinks_below_the_minimum_length() {
        let rail = Rect::new(0.0, 0.0, m::SCROLLBAR, 400.0);
        let mut bar = ScrollBar::vertical();
        // 400 of 200 000: the proportional thumb would be well under a DIP.
        bar.set_content(200_000.0, 400.0, 0.0);
        let thumb = bar.thumb_rect(rail);
        assert!(
            (thumb.bottom - thumb.top - m::SCROLLBAR_THUMB_MIN).abs() < 1e-3,
            "floored at SCROLLBAR_THUMB_MIN"
        );
        // And it still reaches the far end.
        bar.set_content(200_000.0, 400.0, 199_600.0);
        let end = bar.thumb_rect(rail);
        assert!((end.bottom - rail.bottom).abs() < 1e-3);
    }

    #[test]
    fn an_expanded_bar_reports_five_parts_and_a_resting_one_reports_three() {
        let rail = Rect::new(0.0, 0.0, m::SCROLLBAR, 400.0);
        let mut bar = ScrollBar::vertical();
        bar.set_content(1000.0, 400.0, 300.0);
        bar.expanded = true;

        let x = m::SCROLLBAR / 2.0;
        assert_eq!(bar.part_at(rail, x, 2.0), Some(ScrollPart::ArrowLow));
        assert_eq!(bar.part_at(rail, x, 398.0), Some(ScrollPart::ArrowHigh));
        let thumb = bar.thumb_rect(rail);
        let mid = (thumb.top + thumb.bottom) / 2.0;
        assert_eq!(bar.part_at(rail, x, mid), Some(ScrollPart::Thumb));
        assert_eq!(bar.part_at(rail, x, thumb.top - 1.0), Some(ScrollPart::PageLow));
        assert_eq!(bar.part_at(rail, x, thumb.bottom + 1.0), Some(ScrollPart::PageHigh));
        assert_eq!(bar.part_at(rail, 999.0, mid), None, "outside the gutter");

        // At rest there are no buttons, so the ends are track.
        bar.expanded = false;
        assert_eq!(bar.part_at(rail, x, 2.0), Some(ScrollPart::PageLow));
    }

    // ── Slider ───────────────────────────────────────────────────────────────

    #[test]
    fn the_slider_thumb_sits_at_each_end_of_its_range() {
        let bounds = Rect::new(0.0, 0.0, 200.0, 40.0);
        let mut s = Slider::new(); // 0..=10
        let half = m::SLIDER_THUMB / 2.0;

        let ring = SLIDER_FOCUS_RING;

        s.set_value(0).expect("0 is in [0, 10]");
        let lo = s.thumb_rect(bounds);
        assert!((lo.left - ring - bounds.left).abs() < 1e-3, "minimum ⇒ flush left, ring included");

        s.set_value(10).expect("10 is in [0, 10]");
        let hi = s.thumb_rect(bounds);
        assert!((hi.right + ring - bounds.right).abs() < 1e-3, "maximum ⇒ flush right, ring included");

        // The fill grows with the value and stops at the thumb's centre.
        let fill = s.fill_rect(bounds);
        assert!((fill.right - (bounds.right - ring - half)).abs() < 1e-3);

        // Vertical: value grows UPWARD, so the minimum is at the BOTTOM.
        let mut v = Slider::new();
        v.set_orientation(Orientation::Vertical);
        let tall = Rect::new(0.0, 0.0, 40.0, 200.0);
        v.set_value(0).expect("0 is in [0, 10]");
        assert!((v.thumb_rect(tall).bottom + ring - tall.bottom).abs() < 1e-3);
        v.set_value(10).expect("10 is in [0, 10]");
        assert!((v.thumb_rect(tall).top - ring - tall.top).abs() < 1e-3);
    }

    #[test]
    fn the_slider_rounds_a_drop_to_the_nearest_value() {
        let bounds = Rect::new(0.0, 0.0, 200.0, 40.0);
        let mut s = Slider::new();
        s.set_maximum(10);
        let (origin, len) = s.track_span(bounds);

        // 43 % of a 0..10 range is 4.3 ⇒ 4; 45 % is 4.5 ⇒ 5.
        assert_eq!(s.value_at(bounds, origin + 0.43 * len, 20.0), 4);
        assert_eq!(s.value_at(bounds, origin + 0.45 * len, 20.0), 5);
        // And it clamps outside the track rather than running off.
        assert_eq!(s.value_at(bounds, origin - 500.0, 20.0), 0);
        assert_eq!(s.value_at(bounds, origin + len + 500.0, 20.0), 10);

        // Round trip: every in-range value maps to its own offset and back.
        for value in 0..=10 {
            let offset = s.offset_of(value, len);
            assert_eq!(s.value_at(bounds, origin + offset, 20.0), value);
        }
    }

    #[test]
    fn the_slider_snaps_to_the_replicas_own_ticks() {
        let mut s = Slider::new();
        s.set_maximum(20);
        s.set_tick_frequency(3);
        // The replica's ticks: 0, 3, 6, 9, 12, 15, 18, 20 — note the SHORT last
        // gap, which an evenly-spread grid would get wrong.
        assert_eq!(s.tick_values(), vec![0, 3, 6, 9, 12, 15, 18, 20]);

        assert_eq!(s.snap_to_tick(0), 0);
        assert_eq!(s.snap_to_tick(4), 3);
        assert_eq!(s.snap_to_tick(5), 6);
        assert_eq!(s.snap_to_tick(19), 18, "18 is one away, 20 is one away — the first wins");
        assert_eq!(s.snap_to_tick(20), 20);

        // A non-positive frequency leaves the two ends only.
        s.set_tick_frequency(0);
        assert_eq!(s.tick_values(), vec![0, 20]);
        assert_eq!(s.snap_to_tick(9), 0);
        assert_eq!(s.snap_to_tick(11), 20);
    }

    #[test]
    fn the_slider_reports_the_part_under_the_pointer() {
        let bounds = Rect::new(0.0, 0.0, 200.0, 40.0);
        let mut s = Slider::new();
        s.set_value(5).expect("5 is in [0, 10]");
        let thumb = s.thumb_rect(bounds);
        let cy = (bounds.top + bounds.bottom) / 2.0;
        assert_eq!(
            s.part_at(bounds, (thumb.left + thumb.right) / 2.0, cy),
            Some(SliderPart::Thumb)
        );
        assert_eq!(s.part_at(bounds, thumb.left - 20.0, cy), Some(SliderPart::TrackBefore));
        assert_eq!(s.part_at(bounds, thumb.right + 20.0, cy), Some(SliderPart::TrackAfter));
        assert_eq!(s.part_at(bounds, -1.0, cy), None);
    }

    // ── ProgressBar ──────────────────────────────────────────────────────────

    #[test]
    fn the_progress_fill_follows_the_replicas_fraction() {
        let bounds = Rect::new(0.0, 0.0, 200.0, 20.0);
        let mut p = ProgressBar::new();
        p.set_value(25);
        let fill = p.fill_rect(bounds);
        assert!((fill.right - fill.left - 50.0).abs() < 1e-3, "a quarter of 200");

        // The track is the token band, centred, not the whole rectangle.
        let track = p.track_rect(bounds);
        assert!((track.bottom - track.top - m::PROGRESS_MD).abs() < 1e-6);
        assert!(((track.top + track.bottom) / 2.0 - 10.0).abs() < 1e-6);
        // The two sizes the web offers, and only those.
        assert!((ProgressSize::Md.track() - m::PROGRESS_MD).abs() < 1e-6);
        assert!((ProgressSize::Sm.track() - m::PROGRESS_SM).abs() < 1e-6);
        assert!(ProgressSize::Sm.track() < ProgressSize::Md.track());

        // `RightToLeftLayout` fills from the other edge.
        p.right_to_left_layout = true;
        let rtl = p.fill_rect(bounds);
        assert!((rtl.right - bounds.right).abs() < 1e-3);
    }

    #[test]
    fn the_auto_variant_uses_the_webs_thresholds() {
        let mut p = ProgressBar::new(); // 0..=100, auto
        p.set_value(50);
        assert_eq!(p.resolved_variant(), ProgressVariant::Primary);
        p.set_value(75);
        assert_eq!(p.resolved_variant(), ProgressVariant::Warning);
        p.set_value(89);
        assert_eq!(p.resolved_variant(), ProgressVariant::Warning);
        p.set_value(90);
        assert_eq!(p.resolved_variant(), ProgressVariant::Danger);
        // An explicit variant is never overridden — a download is not dangerous
        // at 95 %.
        p.variant = ProgressVariant::Primary;
        assert_eq!(p.resolved_variant(), ProgressVariant::Primary);
    }

    #[test]
    fn the_indeterminate_sliver_crosses_the_track_and_leaves_it() {
        let bounds = Rect::new(0.0, 0.0, 300.0, 20.0);
        let mut p = ProgressBar::new();
        p.set_indeterminate(true);
        assert!(p.indeterminate());
        assert_eq!(p.style, ProgressBarStyle::Marquee);

        let w = 300.0 * PROGRESS_SLIVER;
        p.phase = 0.0;
        let start = p.sliver_rect(bounds);
        assert!((start.right - bounds.left).abs() < 1e-3, "translateX(-100 %): fully off-left");
        p.phase = 1.0;
        let end = p.sliver_rect(bounds);
        assert!((end.left - bounds.right).abs() < 1e-3, "translateX(300 %): fully off-right");
        assert!((start.right - start.left - w).abs() < 1e-3, "w-1/3 of the track");

        p.set_indeterminate(false);
        assert!(!p.indeterminate());
        assert_eq!(p.style, ProgressBarStyle::Continuous);
    }

    // ── NumericField: the ordering trap ──────────────────────────────────────

    /// The documented trap, reproduced — and then made unreachable.
    #[test]
    fn a_value_above_the_default_maximum_is_refused_unless_the_range_comes_first() {
        // The mistake: the default Maximum is 100, so 255 throws.
        let mut naive = NumericField::new();
        assert_eq!(naive.maximum(), 100.0, "the default nobody expects");
        assert!(naive.set_value(255.0).is_err());
        assert_eq!(naive.value(), 0.0, "a refused assignment changed nothing");

        // The fix the toolkit documents — and the only order this type offers.
        let ok = NumericField::ranged(0.0, 255.0)
            .with_value(255.0)
            .expect("the range was widened first");
        assert_eq!(ok.value(), 255.0);
        assert_eq!((ok.minimum(), ok.maximum()), (0.0, 255.0));

        // Same thing in one call.
        let mut one = NumericField::new();
        one.set_range_and_value(0.0, 255.0, 255.0).expect("range then value");
        assert_eq!(one.value(), 255.0);
    }

    #[test]
    fn a_refused_range_and_value_leaves_the_field_untouched() {
        let mut f = NumericField::new();
        // 300 is outside the range being asked for: refused BEFORE anything is
        // applied, so the field keeps its old range as well as its old value.
        assert!(f.set_range_and_value(0.0, 255.0, 300.0).is_err());
        assert_eq!((f.minimum(), f.maximum(), f.value()), (0.0, 100.0, 0.0));
        // An inverted range is refused too.
        assert!(f.set_range_and_value(10.0, 5.0, 7.0).is_err());
        assert_eq!((f.minimum(), f.maximum()), (0.0, 100.0));
    }

    #[test]
    fn clamping_is_a_named_decision_not_a_fallback() {
        let mut f = NumericField::new(); // 0..=100
        assert_eq!(f.clamped(255.0), 100.0);
        assert_eq!(f.value(), 100.0);
        assert_eq!(f.clamped(-5.0), 0.0);
        assert_eq!(f.value(), 0.0);
        assert_eq!(f.clamped(42.0), 42.0);
    }

    #[test]
    fn a_negative_range_is_reached_in_the_right_order_too() {
        // `ranged` applies Maximum first, which is what makes a range entirely
        // below the default 0..=100 work at all.
        let f = NumericField::ranged(-50.0, -10.0);
        assert_eq!((f.minimum(), f.maximum()), (-50.0, -10.0));
        // An inverted pair is straightened rather than refused: a constructor
        // has nowhere to report to.
        let g = NumericField::ranged(10.0, -10.0);
        assert_eq!((g.minimum(), g.maximum()), (-10.0, 10.0));
    }

    /// The formatting, the increment and the spin steps all stay the replica's —
    /// this only checks they are reachable and were not shadowed.
    #[test]
    fn the_replicas_surface_is_reachable_through_deref() {
        let mut f = NumericField::ranged(0.0, 20000.0);
        f.set_decimal_places(2).expect("0..=99");
        f.set_thousands_separator(true);
        f.set_value(10000.0).expect("in range");
        assert_eq!(f.display_text(), "10\u{202F}000,00", "fr-FR, from the replica");
        f.set_increment(0.5).expect("a positive step");
        f.up_button();
        assert_eq!(f.value(), 10000.5);
        assert!(f.set_increment(-1.0).is_err(), "a negative step stays refused");
    }

    // ── DomainField ──────────────────────────────────────────────────────────

    #[test]
    fn the_domain_field_keeps_the_replicas_selection_machine() {
        let mut d = DomainField::with_items(["Alpha", "Bravo", "Charlie"]);
        assert_eq!(d.selected_index(), 0);
        assert_eq!(d.text(), "Alpha");

        d.select_next();
        assert_eq!(d.selected_item(), Some("Bravo"));
        d.select_next();
        d.select_next();
        assert_eq!(d.selected_item(), Some("Charlie"), "no wrap by default");
        d.set_wrap(true);
        d.select_next();
        assert_eq!(d.selected_item(), Some("Alpha"));

        // An empty field selects nothing and shows nothing.
        let e = DomainField::new();
        assert_eq!(e.selected_index(), -1);
        assert_eq!(e.text(), "");
    }

    // ── Keyboard / wheel helpers ─────────────────────────────────────────────

    #[test]
    fn a_slider_and_a_scroll_bar_disagree_on_page_up() {
        // ARIA slider: PageUp RAISES the value.
        assert_eq!(RangeKey::for_slider(vk::PAGE_UP), Some(RangeKey::PageIncrease));
        assert_eq!(RangeKey::for_slider(vk::UP), Some(RangeKey::Increase));
        assert_eq!(RangeKey::for_slider(vk::LEFT), Some(RangeKey::Decrease));
        // A scrolled area: PageUp goes back toward the START.
        assert_eq!(RangeKey::for_scroll(vk::PAGE_UP), Some(RangeKey::PageDecrease));
        assert_eq!(RangeKey::for_scroll(vk::UP), Some(RangeKey::Decrease));
        assert_eq!(RangeKey::for_scroll(vk::END), Some(RangeKey::Last));
        assert_eq!(RangeKey::for_slider(vk::TAB), None);
        for k in RangeKey::KEYS {
            assert!(RangeKey::for_slider(k).is_some() && RangeKey::for_scroll(k).is_some());
        }
    }

    #[test]
    fn wheel_fractions_add_up_to_whole_steps() {
        let mut w = WheelSteps::default();
        assert_eq!(w.take(1.0), 1);
        assert_eq!(w.take(-2.0), -2);
        // A touchpad's small deltas accumulate instead of rounding to zero.
        assert_eq!(w.take(0.4), 0);
        assert_eq!(w.take(0.4), 0);
        assert_eq!(w.take(0.4), 1);
        w.reset();
        assert_eq!(w.take(0.9), 0);
        assert_eq!(w.take(f32::NAN), 0, "garbage in, nothing out");
    }

    // ── Slider: keyboard, wheel, geometry for composition ────────────────────

    #[test]
    fn the_slider_follows_the_aria_keyboard() {
        let mut s = Slider::new(); // 0..=10, small 1, large 5
        s.set_value(4).expect("in range");
        assert!(s.apply_key(RangeKey::Increase));
        assert_eq!(s.value(), 5);
        assert!(s.apply_key(RangeKey::PageIncrease));
        assert_eq!(s.value(), 10);
        assert!(!s.apply_key(RangeKey::PageIncrease), "clamped at the maximum: no move");
        assert!(s.apply_key(RangeKey::First));
        assert_eq!(s.value(), 0);
        assert!(!s.apply_key(RangeKey::Decrease));
        assert!(s.apply_key(RangeKey::Last));
        assert_eq!(s.value(), 10);
        assert!(s.apply_key(RangeKey::PageDecrease));
        assert_eq!(s.value(), 5);
    }

    #[test]
    fn the_slider_wheel_raises_the_value_away_from_the_user() {
        let mut s = Slider::new();
        s.set_value(5).expect("in range");
        // Host sign: -1 = one notch away from the user.
        assert!(s.apply_wheel(-1));
        assert_eq!(s.value(), 6);
        assert!(s.apply_wheel(3));
        assert_eq!(s.value(), 3);
        assert!(s.apply_wheel(100));
        assert_eq!(s.value(), 0, "clamped");
        assert!(!s.set_value_clamped(-5), "already at the minimum");
    }

    #[test]
    fn the_slider_paints_inside_what_it_measures() {
        // `measure` must hold the thumb, its focus ring and any tick row, the
        // rail being centred — for every tick style.
        for style in [TickStyle::None, TickStyle::BottomRight, TickStyle::TopLeft, TickStyle::Both] {
            let mut s = Slider::new();
            s.set_tick_style(style);
            let bands = s.tick_bands(Rect::new(0.0, 0.0, 0.0, 0.0));
            let reach = bands
                .rows()
                .into_iter()
                .flatten()
                .map(|(lo, hi)| lo.abs().max(hi.abs()))
                .fold(Slider::thumb_reach(), f32::max);
            let bounds = Rect::new(0.0, 0.0, 200.0, 2.0 * reach);
            for value in [0, 10] {
                s.set_value(value).expect("in range");
                let ring = s.thumb_rect(bounds).inflate(SLIDER_FOCUS_RING, SLIDER_FOCUS_RING);
                assert!(ring.left >= bounds.left - 1e-3 && ring.right <= bounds.right + 1e-3);
                assert!(ring.top >= bounds.top - 1e-3 && ring.bottom <= bounds.bottom + 1e-3);
                for (lo, hi) in s.tick_bands(bounds).rows().into_iter().flatten() {
                    assert!(lo >= bounds.top - 1e-3 && hi <= bounds.bottom + 1e-3, "{style:?}");
                }
            }
        }
    }

    #[test]
    fn the_slider_is_grabbed_along_its_whole_length() {
        let bounds = Rect::new(0.0, 0.0, 200.0, 40.0);
        let mut s = Slider::new();
        s.set_value(0).expect("in range");
        let hit = s.hit_rect(bounds);
        assert!(s.hit_test(bounds, 150.0, 20.0 + m::SLIDER_THUMB / 2.0), "beside the thin rail");
        assert!(!s.hit_test(bounds, 150.0, 1.0), "the empty band is not a target");
        assert!((hit.bottom - hit.top - (m::SLIDER_THUMB + 2.0 * SLIDER_FOCUS_RING)).abs() < 1e-3);
        // A drag maps the pointer onto the value, clamped.
        assert!(s.drag_to(bounds, 500.0, 20.0));
        assert_eq!(s.value(), 10);
    }

    // ── ScrollBar gestures ───────────────────────────────────────────────────

    #[test]
    fn the_scroll_bar_steps_through_the_replicas_gestures() {
        let mut bar = ScrollBar::vertical();
        bar.set_content(1000.0, 250.0, 0.0);
        bar.set_small_change(40).expect("positive");
        assert!(bar.apply_key(RangeKey::Increase));
        assert_eq!(bar.value(), 40);
        assert!(bar.apply_key(RangeKey::PageIncrease));
        assert_eq!(bar.value(), 290);
        assert!(bar.apply_key(RangeKey::Last));
        assert_eq!(bar.value(), 750, "the reachable end, extent - viewport");
        assert!(!bar.apply_key(RangeKey::Increase), "already at the end");
        assert!(bar.apply_key(RangeKey::First));
        assert_eq!(bar.value(), 0);

        assert!(bar.apply_part(ScrollPart::ArrowHigh));
        assert_eq!(bar.value(), 40);
        assert!(bar.apply_part(ScrollPart::PageLow));
        assert_eq!(bar.value(), 0);
        assert!(!bar.apply_part(ScrollPart::Thumb));

        assert!(bar.scroll_by(300.0));
        assert_eq!(bar.value(), 300);
        assert!(bar.scroll_by(10_000.0));
        assert_eq!(bar.value(), 750);
        assert!(!bar.scroll_by(f32::INFINITY));
    }

    #[test]
    fn dragging_the_thumb_is_the_inverse_of_placing_it() {
        let rail = Rect::new(0.0, 0.0, m::SCROLLBAR, 400.0);
        for expanded in [false, true] {
            let mut bar = ScrollBar::vertical().with_expanded(expanded);
            for scroll in [0.0, 120.0, 333.0, 600.0] {
                bar.set_content(1000.0, 400.0, scroll);
                let thumb = bar.thumb_rect(rail);
                assert_eq!(bar.value_at_thumb_start(rail, thumb.top), scroll as i32, "exp={expanded}");
            }
            // Grabbed 10 DIP into the thumb and pulled far past the end: clamps.
            bar.set_content(1000.0, 400.0, 0.0);
            assert!(bar.drag_to(rail, 5000.0, 10.0));
            assert_eq!(bar.value(), 600);
            assert!(bar.drag_to(rail, -5000.0, 10.0));
            assert_eq!(bar.value(), 0);
        }
    }

    // ── ProgressBar header and timing ────────────────────────────────────────

    #[test]
    fn the_progress_header_sits_above_the_track() {
        let bounds = Rect::new(0.0, 0.0, 200.0, 60.0);
        let mut p = ProgressBar::new().with_label("Stockage").with_value_shown(true);
        p.set_value(42);
        assert!(p.has_header());
        assert_eq!(p.value_text(), "42 %");
        let line = p.header_rect(bounds).expect("a header");
        let track = p.track_rect(bounds);
        assert!((track.top - line.bottom - PROGRESS_HEADER_GAP).abs() < 1e-3, "mb-1");
        // Header + gap + track, centred as one block.
        let block = PROGRESS_HEADER_LINE + PROGRESS_HEADER_GAP + m::PROGRESS_MD;
        assert!((line.top - (60.0 - block) / 2.0).abs() < 1e-3);
        // And without one the track stays centred, as before.
        let plain = ProgressBar::new();
        assert!(plain.header_rect(bounds).is_none());
        assert!(((plain.track_rect(bounds).top + plain.track_rect(bounds).bottom) / 2.0 - 30.0).abs() < 1e-3);
    }

    #[test]
    fn the_indeterminate_phase_follows_css_ease_in_out() {
        assert!(ProgressBar::phase_at(0).abs() < 1e-3);
        assert!((ProgressBar::phase_at(ProgressBar::SLIDE_MS / 2) - 0.5).abs() < 1e-3, "symmetric curve");
        assert!(ProgressBar::phase_at(ProgressBar::SLIDE_MS) < 1e-3, "it loops");
        // Slow at the ends: a quarter of the time covers far less than a quarter.
        assert!(ProgressBar::phase_at(ProgressBar::SLIDE_MS / 4) < 0.2);
        let mut last = -1.0;
        for ms in (0..ProgressBar::SLIDE_MS).step_by(50) {
            let p = ProgressBar::phase_at(ms);
            assert!(p >= last, "monotonic within a crossing");
            last = p;
        }
        // `linear` is the identity.
        assert!((css_cubic_bezier(0.0, 0.0, 1.0, 1.0, 0.3) - 0.3).abs() < 1e-3);
    }

    // ── NumericEdit / NumericField typing ────────────────────────────────────

    #[test]
    fn typing_keeps_only_what_a_number_can_hold() {
        let mut e = NumericEdit::new("42");
        assert_eq!(e.selection(), (0, 2), "focusing selects all");
        assert!(e.insert("1a2 b,5", false), "letters and spaces dropped");
        assert_eq!(e.text(), "12,5");
        assert!(!e.insert("xyz", false));
        assert!(e.insert("f", true), "hex admits a-f");
        assert_eq!(e.text(), "12,5f");
        assert!(e.backspace(false));
        assert_eq!(e.text(), "12,5");
        e.set_caret(0, false);
        assert!(e.delete(false));
        assert_eq!(e.text(), "2,5");
        assert!(!e.backspace(false), "nothing before the caret");
        assert!(e.delete(true));
        assert_eq!(e.text(), "");
    }

    #[test]
    fn the_caret_never_lands_inside_a_group_separator() {
        // fr-FR groups with U+202F, three bytes long.
        let mut e = NumericEdit::new("10\u{202F}000");
        e.set_caret(3, false); // mid-character
        assert!(e.text().is_char_boundary(e.caret()));
        e.set_caret(2, false);
        assert_eq!(e.handle_key(vk::RIGHT, Modifiers::NONE), EditOutcome::Moved);
        assert_eq!(e.caret(), 5, "one CHARACTER right");
        assert_eq!(e.handle_key(vk::BACK, Modifiers::NONE), EditOutcome::Edited);
        assert_eq!(e.text(), "10000");
    }

    #[test]
    fn editing_keys_report_what_they_did() {
        let mut e = NumericEdit::new("123");
        // Left with a selection collapses it to its start.
        assert_eq!(e.handle_key(vk::LEFT, Modifiers::NONE), EditOutcome::Moved);
        assert_eq!((e.caret(), e.anchor()), (0, 0));
        assert_eq!(e.handle_key(vk::END, Modifiers::SHIFT), EditOutcome::Moved);
        assert_eq!(e.selected_text(), "123");
        assert_eq!(e.handle_key(vk::letter('c'), Modifiers::CTRL), EditOutcome::Copy("123".into()));
        assert_eq!(e.handle_key(vk::letter('x'), Modifiers::CTRL), EditOutcome::Cut("123".into()));
        assert_eq!(e.text(), "");
        assert_eq!(e.handle_key(vk::letter('v'), Modifiers::CTRL), EditOutcome::Paste);
        assert_eq!(e.handle_key(vk::ENTER, Modifiers::NONE), EditOutcome::Commit);
        assert_eq!(e.handle_key(vk::ESCAPE, Modifiers::NONE), EditOutcome::Revert);
        assert_eq!(e.handle_key(vk::UP, Modifiers::NONE), EditOutcome::Step(SpinPart::Up));
        assert_eq!(e.handle_key(vk::DOWN, Modifiers::NONE), EditOutcome::Step(SpinPart::Down));
        assert_eq!(e.handle_key(vk::TAB, Modifiers::NONE), EditOutcome::Ignored, "Tab is the focus ring's");
        assert_eq!(e.handle_key(vk::letter('a'), Modifiers::ALT), EditOutcome::Ignored);
        e.insert("77", false);
        assert_eq!(e.handle_key(vk::letter('a'), Modifiers::CTRL), EditOutcome::Moved);
        assert_eq!(e.selection(), (0, 2));
    }

    #[test]
    fn commit_parses_folds_and_rounds() {
        let mut f = NumericField::ranged(0.0, 20_000.0);
        f.set_decimal_places(2).expect("0..=99");
        f.set_thousands_separator(true);
        // The displayed text round-trips.
        f.set_value(10_000.5).expect("in range");
        let e = f.begin_edit();
        assert_eq!(f.parse_text(e.text()), Some(10_000.5));
        // French comma, keypad point, rounding to two places, clamping.
        assert!(f.commit(&NumericEdit::new("1,23456")));
        assert_eq!(f.value(), 1.23);
        assert!(f.commit(&NumericEdit::new("2.5")));
        assert_eq!(f.value(), 2.5);
        assert!(f.commit(&NumericEdit::new("99999")));
        assert_eq!(f.value(), 20_000.0);
        // Not a number: refused, value untouched.
        assert!(!f.commit(&NumericEdit::new("-")));
        assert!(!f.commit(&NumericEdit::new("")));
        assert_eq!(f.value(), 20_000.0);

        let mut h = NumericField::ranged(0.0, 65_535.0);
        h.set_hexadecimal(true);
        assert!(h.commit(&NumericEdit::new("beef")));
        assert_eq!(h.value(), 48_879.0);
    }

    #[test]
    fn the_spin_buttons_stop_at_the_bounds() {
        let mut f = NumericField::ranged(0.0, 2.0);
        assert!(!f.can_step(SpinPart::Down), "atMin");
        assert!(f.step(SpinPart::Up));
        assert!(f.apply_wheel(-5), "away from the user steps up");
        assert_eq!(f.value(), 2.0);
        assert!(!f.can_step(SpinPart::Up), "atMax");
        assert!(!f.step(SpinPart::Up));

        // Geometry: the column is `w-6` on the UpDownAlign side, split in two.
        let b = Rect::new(0.0, 0.0, 120.0, 36.0);
        let (up, down) = f.spin_rects(b);
        assert!((up.left - (120.0 - SPIN_COLUMN)).abs() < 1e-3 && (up.bottom - 18.0).abs() < 1e-3);
        assert_eq!(f.spin_part_at(b, 110.0, 5.0), Some(SpinPart::Up));
        assert_eq!(f.spin_part_at(b, 110.0, 30.0), Some(SpinPart::Down));
        assert_eq!(f.spin_part_at(b, 10.0, 5.0), None);
        assert!((down.top - 18.0).abs() < 1e-3);
        let text = f.text_rect(b);
        assert!((text.left - FIELD_PAD_X).abs() < 1e-3 && (text.right - (up.left - FIELD_PAD_X)).abs() < 1e-3);
    }

    #[test]
    fn the_domain_field_steps_and_jumps() {
        let mut d = DomainField::with_items(["Alpha", "Bravo", "Charlie"]);
        assert!(!d.can_step(SpinPart::Up), "first item, no wrap");
        assert!(d.apply_key(vk::DOWN));
        assert_eq!(d.selected_item(), Some("Bravo"));
        assert!(d.apply_key(vk::END));
        assert_eq!(d.selected_item(), Some("Charlie"));
        assert!(!d.can_step(SpinPart::Down));
        assert!(d.apply_key(vk::HOME));
        assert!(!d.apply_key(vk::TAB));
        d.set_wrap(true);
        assert!(d.can_step(SpinPart::Up));
        assert!(!DomainField::new().can_step(SpinPart::Down), "nothing to step through");
    }

    fn ed(r: Rect) -> (f32, f32, f32, f32) {
        (r.left, r.top, r.right, r.bottom)
    }

    /// Whether every corner of `r` lies inside the rounded rectangle.
    fn inside_rounded(r: Rect, frame: Rect, radius: f32) -> bool {
        [(r.left, r.top), (r.right, r.top), (r.left, r.bottom), (r.right, r.bottom)].iter().all(|&(x, y)| {
            let cx = x.clamp(frame.left + radius, frame.right - radius);
            let cy = y.clamp(frame.top + radius, frame.bottom - radius);
            let (dx, dy) = (x - cx, y - cy);
            let on = x >= frame.left && x <= frame.right && y >= frame.top && y <= frame.bottom;
            on && dx * dx + dy * dy <= radius * radius + 1e-3
        })
    }

    #[test]
    fn a_rail_is_kept_inside_a_rounded_frame() {
        let frame = Rect::new(0.0, 0.0, 200.0, 100.0);
        for radius in [4.0, 8.0, 12.0, 16.0] {
            let v = fit_rail(Rect::new(188.0, 0.0, 200.0, 100.0), frame, radius);
            assert!(inside_rounded(v, frame, radius), "vertical, r={radius}: {:?}", ed(v));
            assert!(v.top > 0.0 && v.bottom < 100.0, "shortened at both corners");
            let h = fit_rail(Rect::new(0.0, 88.0, 200.0, 100.0), frame, radius);
            assert!(inside_rounded(h, frame, radius), "horizontal, r={radius}: {:?}", ed(h));
            assert!(h.left > 0.0 && h.right < 200.0);
        }
    }

    #[test]
    fn a_rail_clear_of_the_corners_is_left_alone() {
        let frame = Rect::new(0.0, 0.0, 200.0, 100.0);
        let square = Rect::new(188.0, 0.0, 200.0, 100.0);
        assert_eq!(ed(fit_rail(square, frame, 0.0)), ed(square), "no radius, nothing to avoid");
        // A gutter well inside the edge never meets the arc.
        let inner = Rect::new(170.0, 0.0, 182.0, 100.0);
        assert_eq!(ed(fit_rail(inner, frame, 12.0)), ed(inner));
        // A rail already starting below the corner keeps its ends.
        let short = Rect::new(188.0, 20.0, 200.0, 80.0);
        assert_eq!(ed(fit_rail(short, frame, 12.0)), ed(short));
    }
}
