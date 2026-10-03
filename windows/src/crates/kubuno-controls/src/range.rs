//! The **range** family: controls whose whole reason to exist is a number moving
//! between a `Minimum` and a `Maximum`.
//!
//! ```ignore
//! ScrollBar (abstract)         +6 range fields   (control::ControlBase)
//!   HScrollBar                 +0
//!   VScrollBar                 +1  RightToLeft
//! TrackBar                     +9 range fields    (control::ControlBase)
//! UpDownBase (abstract)        +5  (containers::ContainerControl)
//!   NumericUpDown              +7
//!   DomainUpDown               +4
//! ```
//!
//! ## Why the field counts are smaller than the catalogue's "declared" counts
//!
//! The reflection catalogue lists, for `ScrollBar`, properties such as
//! `BackColor`, `Font`, `Text`, `TabStop`, `AutoSize` as *declared* — but the
//! toolkit only **re-declares** them (with `new`/attributes) to hide them from
//! the designer or to change one default; the *state* still lives on `Control`.
//! Restating those as fields here would duplicate what `ControlBase` already
//! carries and let the two drift, which the shared brief forbids ("do not
//! re-implement what a base already carries"). So each level below owns only the
//! genuinely **new** state, and honours a changed default (e.g. a `ScrollBar` is
//! not a tab stop) inside its `Default` impl instead. The re-declared
//! properties are called out in comments where the difference matters.
//!
//! ## The arithmetic, and its traps
//!
//! Two facts about `ScrollBar` are famous foot-guns and are implemented and
//! tested here:
//!
//! * The highest `Value` the scroll box can *reach* is `Maximum - LargeChange
//!   + 1`, **not** `Maximum` — because the thumb represents `LargeChange` units
//!   of content and its far edge is what lines up with `Maximum`.
//! * The thumb's length is proportional to `LargeChange / (Maximum - Minimum +
//!   1)` — a bigger page is a bigger grip.
//!
//! And for `NumericUpDown`, `Value` is a decimal that **throws** if set outside
//! `Minimum`/`Maximum`; the documented consequence is that `Maximum` must be
//! raised *before* a larger `Value` is assigned. Both are reproduced as a
//! `Result`-returning setter rather than a silent clamp, so a caller cannot set
//! an out-of-range value without noticing.

use drive_app_controls::{Canvas, Rect};

use crate::control::{Control, ControlBase, ControlCanvas};
use crate::enums::{BorderStyle, HorizontalAlignment, LeftRightAlignment, RightToLeft, Size};

// ─────────────────────────────────────────────────────────────────────────────
// Enumerations this family declares.
//
// Only `Orientation` and `TickStyle` live here: `TrackBar` is the sole type that
// declares them, so they are correctly family-local. `HorizontalAlignment` and
// `LeftRightAlignment` are general WinForms enums shared with other families and
// live in `enums.rs` — several families had each grown their own copy, and the
// `LeftRightAlignment` copies had already drifted apart on their default.
// ─────────────────────────────────────────────────────────────────────────────

/// Layout axis of a `TrackBar` (`System.Windows.Forms.Orientation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    #[default]
    Horizontal = 0,
    Vertical = 1,
}

/// Where a `TrackBar` paints its ticks (`TickStyle`). The TrackBar default is
/// `BottomRight`; the reference sheet shows all four painted side by side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TickStyle {
    None = 0,
    TopLeft = 1,
    #[default]
    BottomRight = 2,
    Both = 3,
}

/// Raised when a `Value` setter is handed a number outside `[Minimum, Maximum]`.
/// WinForms throws `ArgumentOutOfRangeException` here; the port surfaces it as a
/// `Result` so the ordering trap (raise `Maximum` first) cannot be ignored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutOfRange {
    pub value: f64,
    pub minimum: f64,
    pub maximum: f64,
}

impl OutOfRange {
    /// A refusal of the form « must be at least `floor` » — the shape WinForms
    /// uses for `SmallChange`, `LargeChange`, `Increment` and `DecimalPlaces`,
    /// which have a floor but no meaningful ceiling.
    pub(crate) fn at_least(value: f64, floor: f64) -> Self {
        Self { value, minimum: floor, maximum: f64::INFINITY }
    }

    /// A refusal against a closed range.
    pub(crate) fn between(value: f64, minimum: f64, maximum: f64) -> Self {
        Self { value, minimum, maximum }
    }
}

impl std::fmt::Display for OutOfRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "value {} is outside the range [{}, {}]",
            self.value, self.minimum, self.maximum
        )
    }
}

impl std::error::Error for OutOfRange {}

// System metrics, in DIP. WinForms reads these from `SystemInformation`; on the
// default Windows theme they are 17 px, and the arrow buttons are square.
//
// They are used AS-IS by the painter, never multiplied by `Canvas::scale()`:
// `Renderer` calls `ID2D1DeviceContext::SetDpi`, so the Direct2D coordinate
// space is ALREADY in DIP — a rectangle at `y = 10.0` lands at 10 DIP whatever
// the monitor scaling. Multiplying here would apply the DPI factor a second
// time (every metric 1.75× too large at 175 %), which is invisible at 100 %.
// The rest of `drive-app-controls` multiplies by `scale()` in zero places, for
// exactly this reason.
const SCROLLBAR_THICKNESS: f32 = 17.0;
const SCROLLBAR_ARROW: f32 = 17.0;
/// The shortest a proportional thumb is allowed to shrink to, so a huge range
/// still leaves something to grab (WinForms clamps to `2 * arrow`-ish).
const SCROLLBAR_MIN_THUMB: f32 = 8.0;

/// Edge length, in DIP, of the chevron drawn in a scroll-bar arrow button or a
/// spin button. Small enough to sit inside the 17 DIP button with margin.
const ARROW_GLYPH: f32 = 8.0;

// TrackBar metrics, in DIP, measured off the reference sheet at 4× zoom
// (`shots/06-range.png`, the "TrackBar — TickStyle" group):
//   * the channel is a thin recessed groove ~4 DIP thick running the full width,
//   * the ticks are SHORT marks ~4 DIP long, ~3 DIP clear of the thumb — not
//     full-height lines, which is what made all three rows read as one picket
//     fence,
//   * the thumb is ~22-27 DIP on the cross axis and is anchored to the top of
//     the control, with any surplus height left unused (WinForms does not
//     centre it).
const TRACKBAR_CHANNEL: f32 = 4.0;
const TRACKBAR_TICK_LEN: f32 = 4.0;
const TRACKBAR_TICK_GAP: f32 = 3.0;
const TRACKBAR_THUMB_EXTENT: f32 = 22.0;

/// Where the thumb and each tick row sit on a TrackBar's CROSS axis (top/bottom
/// for a horizontal bar, left/right for a vertical one).
///
/// A tick row is `None` when the current `TickStyle` does not ask for it, which
/// is what makes `BottomRight`, `Both` and `None` three visibly different
/// layouts rather than the same one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackBands {
    pub thumb_lo: f32,
    pub thumb_hi: f32,
    /// Centre of the thumb — the channel is centred on this.
    pub centre:   f32,
    /// The tick row BEFORE the thumb (above / to the left), as `(lo, hi)`.
    pub tick_before: Option<(f32, f32)>,
    /// The tick row AFTER the thumb (below / to the right).
    pub tick_after:  Option<(f32, f32)>,
}

/// Resolves the cross-axis layout for `style` inside the extent `lo..hi`.
///
/// Pure — no canvas — so the three `TickStyle` rows are asserted in tests
/// instead of eyeballed against a screenshot.
pub fn track_bands(lo: f32, hi: f32, style: TickStyle) -> TrackBands {
    let before = matches!(style, TickStyle::TopLeft | TickStyle::Both);
    let after = matches!(style, TickStyle::BottomRight | TickStyle::Both);
    let band = TRACKBAR_TICK_LEN + TRACKBAR_TICK_GAP;

    // The thumb starts just past the leading tick row, so the control fills
    // from the top down exactly as the reference does.
    let avail_lo = lo + if before { band } else { 0.0 };
    let avail_hi = hi - if after { band } else { 0.0 };
    let thumb_lo = avail_lo;
    let thumb_hi = thumb_lo + TRACKBAR_THUMB_EXTENT.min((avail_hi - avail_lo).max(1.0));

    TrackBands {
        thumb_lo,
        thumb_hi,
        centre: (thumb_lo + thumb_hi) * 0.5,
        tick_before: before.then_some((thumb_lo - band, thumb_lo - TRACKBAR_TICK_GAP)),
        tick_after: after.then_some((thumb_hi + TRACKBAR_TICK_GAP, thumb_hi + band)),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Number formatting conventions
// ─────────────────────────────────────────────────────────────────────────────

/// The separators a `NumericUpDown` formats with, bundled so the convention
/// lives in ONE named place instead of being spelled inline at each call.
///
/// WinForms takes these from `CultureInfo.CurrentCulture`. A control here may
/// not: it does no I/O and the crate takes no locale dependency. So the culture
/// is a value the control carries, mirroring how [`crate::datetime::Names`] /
/// `datetime::FR` isolate the French month and day names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberFormat {
    /// `NumberDecimalSeparator`.
    pub decimal_separator: &'static str,
    /// `NumberGroupSeparator`, used only when `ThousandsSeparator` is on.
    pub group_separator: &'static str,
}

impl NumberFormat {
    /// fr-FR — the convention the rest of the library already speaks, and the
    /// one the reference sheet was captured under (`3,50`, `10 000`).
    ///
    /// The group separator is U+202F NARROW NO-BREAK SPACE, which is what
    /// current Windows/ICU fr-FR uses — a plain space would allow a line break
    /// in the middle of a number.
    pub const FR: Self = Self { decimal_separator: ",", group_separator: "\u{202F}" };

    /// The invariant culture (`3.50`, `10,000`) — kept because it is what a
    /// caller formatting for a machine-readable field wants.
    pub const INVARIANT: Self = Self { decimal_separator: ".", group_separator: "," };
}

impl Default for NumberFormat {
    /// French, to agree with `datetime`: the same form must not show a French
    /// date next to an English number.
    fn default() -> Self {
        Self::FR
    }
}

// The four chevrons used below (`ChevronLeft/Right/Up/Down`) all live in
// `drive-app-controls/assets/lucide-icons.txt`. They are VECTOR icons on
// purpose: drawn as text, the arrow characters would need glyphs the embedded
// Outfit face does not carry, and would render as tofu boxes.

// ═════════════════════════════════════════════════════════════════════════════
// ScrollBar  →  HScrollBar, VScrollBar
// ═════════════════════════════════════════════════════════════════════════════

/// The state `System.Windows.Forms.ScrollBar` adds over `Control`.
///
/// `ScrollBar` is abstract: only `HScrollBar` and `VScrollBar` are constructed.
/// The eight other properties the catalogue lists as declared (`BackColor`,
/// `Font`, `Text`, `AutoSize`, `BackgroundImage(Layout)`, `ForeColor`) are
/// re-declarations that only hide the property from the designer — their state
/// is `ControlBase`'s. The one changed default that matters, `TabStop = false`,
/// is applied in [`ScrollBar::default`].
#[derive(Clone)]
pub struct ScrollBar {
    control: ControlBase,

    minimum: i32,
    maximum: i32,
    /// Invariant `Minimum <= Value <= Maximum`; user scrolling additionally
    /// cannot pass [`ScrollBar::max_reachable_value`].
    value: i32,
    small_change: i32,
    large_change: i32,
    /// `ScaleScrollBarForDpiChange` — whether the bar rescales on a DPI change.
    /// Stored to honour the property; the painter already works in scaled DIP,
    /// so it has no separate effect here and is documented as not-yet-acted-on.
    scale_for_dpi: bool,
}

impl Default for ScrollBar {
    fn default() -> Self {
        // A scrollbar takes no focus: `TabStop` defaults to false, unlike the
        // `Control` default of true.
        let mut control = ControlBase::new();
        control.tab_stop = false;
        Self {
            control,
            minimum: 0,
            maximum: 100,
            value: 0,
            small_change: 1,
            large_change: 10,
            scale_for_dpi: true,
        }
    }
}

impl ScrollBar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn minimum(&self) -> i32 {
        self.minimum
    }
    pub fn maximum(&self) -> i32 {
        self.maximum
    }
    pub fn value(&self) -> i32 {
        self.value
    }
    /// `SmallChange` — the line step, **as the toolkit reports it**: never more
    /// than [`ScrollBar::large_change`], because a line may not outrun a page.
    ///
    /// The clamp lives in the GETTER, not the setter: WinForms keeps the value
    /// you assigned and only reports it clamped, so widening `LargeChange` later
    /// reveals the original again. Clamping on assignment would lose it.
    pub fn small_change(&self) -> i32 {
        self.small_change.min(self.large_change())
    }

    /// `LargeChange` — the page step, as the toolkit reports it: never more than
    /// the whole range, `Maximum - Minimum + 1`. Also a getter-side clamp.
    pub fn large_change(&self) -> i32 {
        self.large_change.min(self.span())
    }

    /// The number of distinct positions in `[Minimum, Maximum]`, saturating so a
    /// range spanning the whole `i32` cannot overflow.
    fn span(&self) -> i32 {
        self.maximum.saturating_sub(self.minimum).saturating_add(1)
    }

    pub fn scale_for_dpi(&self) -> bool {
        self.scale_for_dpi
    }
    pub fn set_scale_for_dpi(&mut self, v: bool) {
        self.scale_for_dpi = v;
    }

    /// `LargeChange` — the page step. A negative value is **refused**, as the
    /// WinForms setter throws `ArgumentOutOfRangeException`.
    ///
    /// It deliberately does **not** touch `Value`. WinForms leaves `Value` where
    /// it is even when the new page size puts it above the scroll ceiling — only
    /// the native thumb position is clamped — and the next scroll gesture snaps
    /// it down. Clamping here would both diverge and compound.
    pub fn set_large_change(&mut self, v: i32) -> Result<(), OutOfRange> {
        if v < 0 {
            return Err(OutOfRange::at_least(v as f64, 0.0));
        }
        self.large_change = v;
        Ok(())
    }

    /// `SmallChange` — the line step. Negative is refused, like `LargeChange`.
    pub fn set_small_change(&mut self, v: i32) -> Result<(), OutOfRange> {
        if v < 0 {
            return Err(OutOfRange::at_least(v as f64, 0.0));
        }
        self.small_change = v;
        Ok(())
    }

    /// Setting `Minimum` above `Maximum` drags `Maximum` up with it, matching
    /// the toolkit (the two are kept consistent, never inverted).
    ///
    /// `Value` is pulled into `[Minimum, Maximum]` — the ASSIGNABLE range, not
    /// the scroll ceiling: `Value = 100` then `Maximum = 50` leaves 50, not 41.
    pub fn set_minimum(&mut self, v: i32) {
        self.minimum = v;
        if self.maximum < self.minimum {
            self.maximum = self.minimum;
        }
        self.value = self.value.clamp(self.minimum, self.maximum);
    }

    /// Setting `Maximum` below `Minimum` drags `Minimum` down with it.
    pub fn set_maximum(&mut self, v: i32) {
        self.maximum = v;
        if self.minimum > self.maximum {
            self.minimum = self.maximum;
        }
        self.value = self.value.clamp(self.minimum, self.maximum);
    }

    /// Assigns `Value` exactly. Errors if it is outside `[Minimum, Maximum]`,
    /// as the WinForms setter throws — the caller must widen the range first.
    pub fn set_value(&mut self, v: i32) -> Result<(), OutOfRange> {
        if v < self.minimum || v > self.maximum {
            return Err(OutOfRange {
                value: v as f64,
                minimum: self.minimum as f64,
                maximum: self.maximum as f64,
            });
        }
        self.value = v;
        Ok(())
    }

    /// The raw ceiling a scroll gesture aims at: `Maximum - LargeChange + 1`.
    ///
    /// **Not** clamped — this is the bare arithmetic the toolkit performs, and
    /// when `LargeChange` is 0 it lands one PAST `Maximum`. The assignment that
    /// follows then fails, which is exactly why a gesture on a zero-page bar
    /// moves nothing (verified against the real control).
    fn scroll_ceiling(&self) -> i32 {
        self.maximum.saturating_sub(self.large_change()).saturating_add(1)
    }

    /// The highest `Value` a scroll gesture can actually reach: the ceiling
    /// `Maximum - LargeChange + 1`, confined to the assignable range. This is
    /// the trap — the thumb's far edge lines up with `Maximum`, so its near edge
    /// (the `Value`) stops `LargeChange - 1` short.
    ///
    /// The confinement matters: with `LargeChange = 0` the bare ceiling is
    /// `Maximum + 1`, which is not a value this control could ever hold.
    pub fn max_reachable_value(&self) -> i32 {
        self.scroll_ceiling().clamp(self.minimum, self.maximum)
    }

    /// Applies a gesture's computed target the way the toolkit does: through the
    /// same checked assignment `Value` uses, so a target outside
    /// `[Minimum, Maximum]` is REFUSED and the value stays put.
    fn gesture_to(&mut self, target: i32) -> Result<(), OutOfRange> {
        if target < self.minimum || target > self.maximum {
            return Err(OutOfRange::between(
                target as f64,
                self.minimum as f64,
                self.maximum as f64,
            ));
        }
        self.value = target;
        Ok(())
    }

    /// Scrolls toward `v` — what dragging the thumb or clicking the track does,
    /// as opposed to the exact [`ScrollBar::set_value`]. `i32::MIN` / `i32::MAX`
    /// are the two ends, which is how the native `SB_TOP` / `SB_BOTTOM` arrive.
    ///
    /// **Can fail**, and that is not a quirk of the port: scrolling to the end
    /// aims at the bare ceiling `Maximum - LargeChange + 1`, which with
    /// `LargeChange = 0` is one past `Maximum`. The toolkit assigns it through
    /// its own checked `Value` setter and throws; the port returns `Err` and
    /// leaves the value untouched. A zero-page bar simply cannot be scrolled.
    pub fn scroll_to(&mut self, v: i32) -> Result<(), OutOfRange> {
        let ceiling = self.scroll_ceiling();
        self.gesture_to(v.clamp(self.minimum, ceiling.max(self.minimum)))
    }

    // The four step gestures cannot fail, so they return nothing. Each clamps
    // its target to a value that is provably inside `[Minimum, Maximum]`:
    // the increments stop at `min(…, ceiling)` — and when the ceiling is the
    // out-of-range `Maximum + 1`, `LargeChange` is 0, which makes both steps 0
    // and the target the current value — while the decrements stop at
    // `Minimum`.

    /// One line step toward `Maximum` (`SmallChange`).
    pub fn line_down(&mut self) {
        let target = self.value.saturating_add(self.small_change()).min(self.scroll_ceiling());
        let _ = self.gesture_to(target);
    }
    /// One line step toward `Minimum`.
    pub fn line_up(&mut self) {
        let target = self.value.saturating_sub(self.small_change()).max(self.minimum);
        let _ = self.gesture_to(target);
    }
    /// One page step toward `Maximum` (`LargeChange`).
    pub fn page_down(&mut self) {
        let target = self.value.saturating_add(self.large_change()).min(self.scroll_ceiling());
        let _ = self.gesture_to(target);
    }
    /// One page step toward `Minimum`.
    pub fn page_up(&mut self) {
        let target = self.value.saturating_sub(self.large_change()).max(self.minimum);
        let _ = self.gesture_to(target);
    }

    /// The thumb's length as a fraction of the track: `LargeChange / (Maximum -
    /// Minimum + 1)`, clamped to `(0, 1]`. A one-page range fills the track.
    pub fn thumb_fraction(&self) -> f32 {
        let frac = self.large_change() as f32 / (self.span().max(1)) as f32;
        frac.clamp(0.0, 1.0)
    }

    /// The thumb's near-edge position as a fraction `[0, 1]` of the *travel*
    /// (the track minus the thumb): `(Value - Minimum) / (reachable span)`.
    fn thumb_travel_fraction(&self) -> f32 {
        let reach = self.max_reachable_value() - self.minimum;
        if reach <= 0 {
            return 0.0;
        }
        ((self.value - self.minimum) as f32 / reach as f32).clamp(0.0, 1.0)
    }

    /// Shared painter for both orientations. `horizontal` picks the axis; the
    /// track runs between the two arrow buttons and the thumb is proportional.
    fn paint_bar(&self, c: &dyn Canvas, bounds: Rect, horizontal: bool) {
        let th = c.theme();
        let arrow = SCROLLBAR_ARROW;

        // Track fills the whole box (behind the arrows too, as Windows paints
        // it), thumb rides the travel between the arrow buttons.
        c.fill_rounded(&bounds, 0.0, &th.scrollbar_track);

        let (start, end, cross0, cross1, along_len) = if horizontal {
            (bounds.left, bounds.right, bounds.top, bounds.bottom, bounds.right - bounds.left)
        } else {
            (bounds.top, bounds.bottom, bounds.left, bounds.right, bounds.bottom - bounds.top)
        };

        // The travel excludes the two arrow buttons at each end.
        let travel = (along_len - 2.0 * arrow).max(0.0);
        let thumb_len = (self.thumb_fraction() * travel).max(SCROLLBAR_MIN_THUMB).min(travel.max(1.0));
        let thumb_start = start + arrow + self.thumb_travel_fraction() * (travel - thumb_len).max(0.0);

        let thumb = if horizontal {
            Rect::new(thumb_start, cross0 + 1.0, thumb_start + thumb_len, cross1 - 1.0)
        } else {
            Rect::new(cross0 + 1.0, thumb_start, cross1 - 1.0, thumb_start + thumb_len)
        };
        c.fill_rounded(&thumb, 2.0, &th.scrollbar_thumb);

        // Arrow buttons: WM_PRINTCLIENT hides these in the reference capture,
        // but a real scrollbar has them, so paint a chevron at each end.
        //
        // These are VECTOR icons, not text: the embedded Outfit face
        // carries no `◄ ► ▲ ▼` glyphs, so drawing them as characters produces
        // tofu boxes.
        let (lo, hi) = if horizontal {
            ("ChevronLeft", "ChevronRight")
        } else {
            ("ChevronUp", "ChevronDown")
        };
        let lo_rect = if horizontal {
            Rect::new(start, cross0, start + arrow, cross1)
        } else {
            Rect::new(cross0, start, cross1, start + arrow)
        };
        let hi_rect = if horizontal {
            Rect::new(end - arrow, cross0, end, cross1)
        } else {
            Rect::new(cross0, end - arrow, cross1, end)
        };
        c.vector_icon(lo, &lo_rect, ARROW_GLYPH, &th.text_secondary);
        c.vector_icon(hi, &hi_rect, ARROW_GLYPH, &th.text_secondary);
    }
}

/// A horizontal scroll bar. It declares **nothing** of its own — every property
/// it exposes is `ScrollBar`'s — which is exactly why the abstract base exists.
#[derive(Clone, Default)]
pub struct HScrollBar {
    base: ScrollBar,
}

impl HScrollBar {
    pub fn new() -> Self {
        Self::default()
    }
}

impl std::ops::Deref for HScrollBar {
    type Target = ScrollBar;
    fn deref(&self) -> &ScrollBar {
        &self.base
    }
}
impl std::ops::DerefMut for HScrollBar {
    fn deref_mut(&mut self) -> &mut ScrollBar {
        &mut self.base
    }
}

impl Control for HScrollBar {
    fn control(&self) -> &ControlBase {
        &self.base.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base.control
    }
    fn preferred_size(&self, _c: &dyn Canvas) -> Size {
        // A horizontal bar wants the system thickness in height and keeps its
        // current width (it is normally docked or anchored to both edges).
        Size::new(self.base.control.width().max(2.0 * SCROLLBAR_ARROW), SCROLLBAR_THICKNESS)
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        self.base.paint_bar(c, bounds, true);
    }
    fn type_name(&self) -> &'static str {
        "HScrollBar"
    }
}

/// A vertical scroll bar. It re-declares exactly one property, `RightToLeft`,
/// which the base does not — so the port carries it here, and nowhere else.
#[derive(Clone, Default)]
pub struct VScrollBar {
    base: ScrollBar,
    /// `RightToLeft` — re-declared by `VScrollBar` (WinForms hides it from the
    /// designer for a vertical bar). Kept for fidelity; the default is
    /// `Inherit`, as on `Control`.
    pub right_to_left: RightToLeft,
}

impl VScrollBar {
    pub fn new() -> Self {
        Self::default()
    }
}

impl std::ops::Deref for VScrollBar {
    type Target = ScrollBar;
    fn deref(&self) -> &ScrollBar {
        &self.base
    }
}
impl std::ops::DerefMut for VScrollBar {
    fn deref_mut(&mut self) -> &mut ScrollBar {
        &mut self.base
    }
}

impl Control for VScrollBar {
    fn control(&self) -> &ControlBase {
        &self.base.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base.control
    }
    fn preferred_size(&self, _c: &dyn Canvas) -> Size {
        Size::new(SCROLLBAR_THICKNESS, self.base.control.height().max(2.0 * SCROLLBAR_ARROW))
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        self.base.paint_bar(c, bounds, false);
    }
    fn type_name(&self) -> &'static str {
        "VScrollBar"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// TrackBar
// ═════════════════════════════════════════════════════════════════════════════

/// `System.Windows.Forms.TrackBar` — a slider. It descends straight from
/// `Control` (not from `ScrollBar`, despite the shared vocabulary), and adds the
/// nine range/appearance fields below. The re-declared `Text`, `Font`,
/// `ForeColor`, `BackgroundImage(Layout)` keep their `ControlBase` state; the
/// one changed default, `AutoSize = true`, is applied in `Default`.
#[derive(Clone)]
pub struct TrackBar {
    control: ControlBase,

    minimum: i32,
    maximum: i32,
    value: i32,
    small_change: i32,
    large_change: i32,
    /// `TickFrequency` — one tick every N units. Coerced to `>= 1`.
    tick_frequency: i32,
    tick_style: TickStyle,
    orientation: Orientation,
    /// `RightToLeftLayout` — mirror the slider when `RightToLeft` is `Yes`.
    /// Stored to honour the property; the painter does not yet mirror, and this
    /// is documented as not-yet-honoured.
    right_to_left_layout: bool,
}

impl Default for TrackBar {
    fn default() -> Self {
        // A TrackBar auto-sizes by default, unlike the `Control` default.
        let mut control = ControlBase::new();
        control.auto_size = true;
        Self {
            control,
            minimum: 0,
            maximum: 10,
            value: 0,
            small_change: 1,
            large_change: 5,
            tick_frequency: 1,
            tick_style: TickStyle::default(),
            orientation: Orientation::default(),
            right_to_left_layout: false,
        }
    }
}

impl TrackBar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn minimum(&self) -> i32 {
        self.minimum
    }
    pub fn maximum(&self) -> i32 {
        self.maximum
    }
    pub fn value(&self) -> i32 {
        self.value
    }
    pub fn small_change(&self) -> i32 {
        self.small_change
    }
    pub fn large_change(&self) -> i32 {
        self.large_change
    }
    pub fn tick_frequency(&self) -> i32 {
        self.tick_frequency
    }
    pub fn tick_style(&self) -> TickStyle {
        self.tick_style
    }
    pub fn orientation(&self) -> Orientation {
        self.orientation
    }
    pub fn right_to_left_layout(&self) -> bool {
        self.right_to_left_layout
    }

    /// Negative is **refused** — the WinForms setter throws
    /// `ArgumentOutOfRangeException` rather than coercing to zero.
    pub fn set_small_change(&mut self, v: i32) -> Result<(), OutOfRange> {
        if v < 0 {
            return Err(OutOfRange::at_least(v as f64, 0.0));
        }
        self.small_change = v;
        Ok(())
    }

    /// Negative is refused, like `SmallChange`.
    pub fn set_large_change(&mut self, v: i32) -> Result<(), OutOfRange> {
        if v < 0 {
            return Err(OutOfRange::at_least(v as f64, 0.0));
        }
        self.large_change = v;
        Ok(())
    }

    /// `TickFrequency` — stored **verbatim**, including 0 and negatives.
    ///
    /// The toolkit does not validate or clamp this one: it stores `-3` and
    /// reports `-3` back (its native `TBM_GETNUMTICS` then returns nonsense).
    /// Forcing `>= 1` here would be a silent correction of a value the caller
    /// can read back, so the coercion is left to [`TrackBar::tick_values`],
    /// which simply draws no interior ticks for a non-positive frequency.
    pub fn set_tick_frequency(&mut self, v: i32) {
        self.tick_frequency = v;
    }
    pub fn set_tick_style(&mut self, v: TickStyle) {
        self.tick_style = v;
    }
    pub fn set_orientation(&mut self, v: Orientation) {
        self.orientation = v;
    }
    pub fn set_right_to_left_layout(&mut self, v: bool) {
        self.right_to_left_layout = v;
    }

    /// Setting `Minimum`/`Maximum` keeps them consistent and pulls `Value` back
    /// into range (the TrackBar setters clamp `Value`, they do not throw).
    pub fn set_minimum(&mut self, v: i32) {
        self.minimum = v;
        if self.maximum < self.minimum {
            self.maximum = self.minimum;
        }
        self.value = self.value.clamp(self.minimum, self.maximum);
    }
    pub fn set_maximum(&mut self, v: i32) {
        self.maximum = v;
        if self.minimum > self.maximum {
            self.minimum = self.maximum;
        }
        self.value = self.value.clamp(self.minimum, self.maximum);
    }

    /// Assigns `Value`. Errors outside `[Minimum, Maximum]` — the WinForms
    /// setter throws `ArgumentOutOfRangeException` for a TrackBar too.
    pub fn set_value(&mut self, v: i32) -> Result<(), OutOfRange> {
        if v < self.minimum || v > self.maximum {
            return Err(OutOfRange {
                value: v as f64,
                minimum: self.minimum as f64,
                maximum: self.maximum as f64,
            });
        }
        self.value = v;
        Ok(())
    }

    /// Where `value` sits along a track of length `track_len`. Horizontal maps
    /// `Minimum→0` at the left; vertical maps `Minimum→track_len` at the bottom
    /// (value grows upward), as the toolkit does. Pure, so the round-trip is
    /// testable without a window.
    pub fn offset_of(&self, value: i32, track_len: f32) -> f32 {
        let span = (self.maximum - self.minimum).max(1) as f32;
        let frac = (value - self.minimum) as f32 / span;
        match self.orientation {
            Orientation::Horizontal => frac * track_len,
            Orientation::Vertical => (1.0 - frac) * track_len,
        }
    }

    /// The thumb's centre offset for the current `Value`.
    pub fn thumb_offset(&self, track_len: f32) -> f32 {
        self.offset_of(self.value, track_len)
    }

    /// The `Value`s a tick is drawn at: `Minimum`, then every `TickFrequency`
    /// step strictly below `Maximum`, then `Maximum`.
    ///
    /// The final gap is therefore SHORTER whenever the range is not a whole
    /// multiple of the frequency — max 20 / frequency 3 gives
    /// `0,3,6,9,12,15,18,20`, eight marks, not seven spread evenly. Spacing them
    /// evenly (the port's first attempt) put every interior tick in the wrong
    /// place.
    ///
    /// A non-positive `TickFrequency` yields just the two ends, matching the
    /// toolkit's own two-tick report for frequency 0. (Its native count for a
    /// NEGATIVE frequency is meaningless — it returns `-4` — so there is nothing
    /// there to reproduce.)
    pub fn tick_values(&self) -> Vec<i32> {
        if self.maximum <= self.minimum {
            return vec![self.minimum];
        }
        let mut out = vec![self.minimum];
        if self.tick_frequency > 0 {
            let mut v = self.minimum.saturating_add(self.tick_frequency);
            while v < self.maximum {
                out.push(v);
                v = v.saturating_add(self.tick_frequency);
            }
        }
        out.push(self.maximum);
        out
    }

    /// How many ticks [`TrackBar::tick_values`] draws — the counterpart of the
    /// native `TBM_GETNUMTICS`.
    ///
    /// The real control reports one FEWER for wide ranges on an unsized bar
    /// (max 100 / frequency 10 gives 10, not 11); that is the common control
    /// dropping a mark it has no pixels for, and it disappears once the bar is
    /// given a realistic width. It is a rendering artefact, not a semantic, so
    /// it is deliberately not reproduced here.
    pub fn tick_count(&self) -> usize {
        self.tick_values().len()
    }

    /// The inverse of [`TrackBar::thumb_offset`]: the `Value` nearest a thumb
    /// dropped at `offset` along a `track_len` track, rounded and clamped. The
    /// round-trip `value → offset → value` is exact for every in-range value.
    pub fn value_at_offset(&self, offset: f32, track_len: f32) -> i32 {
        if track_len <= 0.0 {
            return self.minimum;
        }
        let span = (self.maximum - self.minimum).max(1) as f32;
        let frac = match self.orientation {
            Orientation::Horizontal => offset / track_len,
            Orientation::Vertical => 1.0 - offset / track_len,
        };
        let raw = self.minimum as f32 + frac * span;
        (raw.round() as i32).clamp(self.minimum, self.maximum)
    }

    fn paint_horizontal(&self, c: &dyn Canvas, bounds: Rect) {
        let th = c.theme();
        let inset = 8.0;
        let track_left = bounds.left + inset;
        let track_right = bounds.right - inset;
        let track_len = (track_right - track_left).max(1.0);
        let bands = track_bands(bounds.top, bounds.bottom, self.tick_style);

        // The recessed groove the thumb rides on, centred on the thumb. Filled
        // then outlined, which is what makes it read as recessed rather than as
        // a flat bar.
        let half_ch = TRACKBAR_CHANNEL * 0.5;
        let channel = Rect::new(track_left, bands.centre - half_ch, track_right, bands.centre + half_ch);
        c.fill_rounded(&channel, half_ch, &th.scrollbar_track);
        c.stroke_rounded(&channel, half_ch, &th.divider);

        // Short tick marks beside the channel, on whichever side(s) the style
        // asks for.
        self.paint_ticks(c, track_left, track_len, &bands, true);

        // The thumb.
        let x = track_left + self.thumb_offset(track_len);
        let half = 5.0;
        let thumb = Rect::new(x - half, bands.thumb_lo, x + half, bands.thumb_hi);
        c.fill_rounded(&thumb, 2.0, &th.accent);
    }

    /// Draws the tick rows `bands` asks for, along the track that starts at
    /// `start` and runs `track_len`. Shared by both orientations.
    fn paint_ticks(&self, c: &dyn Canvas, start: f32, track_len: f32, bands: &TrackBands, horizontal: bool) {
        if self.tick_style == TickStyle::None {
            return;
        }
        let th = c.theme();
        // Each tick is placed at its own VALUE, through the same mapping the
        // thumb uses — so a tick and the thumb at that value line up exactly,
        // and an uneven final gap is drawn uneven.
        for value in self.tick_values() {
            let along = start + self.offset_of(value, track_len);
            for (lo, hi) in [bands.tick_before, bands.tick_after].into_iter().flatten() {
                // The mark is 1 DIP across the track and `TICK_LEN` along the
                // cross axis; the two swap with the orientation.
                let t = if horizontal {
                    Rect::new(along - 0.5, lo, along + 0.5, hi)
                } else {
                    Rect::new(lo, along - 0.5, hi, along + 0.5)
                };
                c.fill_rounded(&t, 0.0, &th.text_tertiary);
            }
        }
    }

    fn paint_vertical(&self, c: &dyn Canvas, bounds: Rect) {
        let th = c.theme();
        let inset = 8.0;
        let track_top = bounds.top + inset;
        let track_bottom = bounds.bottom - inset;
        let track_len = (track_bottom - track_top).max(1.0);
        // Same layout, cross axis rotated: the bands run left-to-right.
        let bands = track_bands(bounds.left, bounds.right, self.tick_style);

        let half_ch = TRACKBAR_CHANNEL * 0.5;
        let channel = Rect::new(bands.centre - half_ch, track_top, bands.centre + half_ch, track_bottom);
        c.fill_rounded(&channel, half_ch, &th.scrollbar_track);
        c.stroke_rounded(&channel, half_ch, &th.divider);

        self.paint_ticks(c, track_top, track_len, &bands, false);

        let y = track_top + self.thumb_offset(track_len);
        let half = 5.0;
        let thumb = Rect::new(bands.thumb_lo, y - half, bands.thumb_hi, y + half);
        c.fill_rounded(&thumb, 2.0, &th.accent);
    }
}

impl Control for TrackBar {
    fn control(&self) -> &ControlBase {
        &self.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }
    fn preferred_size(&self, _c: &dyn Canvas) -> Size {
        // WinForms' default TrackBar is 104 x 45 DIP horizontally; vertically
        // the axes swap.
        match self.orientation {
            Orientation::Horizontal => Size::new(104.0, 45.0),
            Orientation::Vertical => Size::new(45.0, 104.0),
        }
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        match self.orientation {
            Orientation::Horizontal => self.paint_horizontal(c, bounds),
            Orientation::Vertical => self.paint_vertical(c, bounds),
        }
    }
    fn type_name(&self) -> &'static str {
        "TrackBar"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// UpDownBase  →  NumericUpDown, DomainUpDown
// ═════════════════════════════════════════════════════════════════════════════

/// The state `System.Windows.Forms.UpDownBase` adds over its base. In WinForms
/// that base is `ContainerControl` (an edit box plus a spin-button pair), so the
/// port **composes** [`crate::containers::ContainerControl`] rather than
/// restating the container surface here.
///
/// The re-declared `AutoScroll`, `AutoSize`, `BackColor`, `ForeColor`,
/// `MinimumSize`, `MaximumSize`, `BackgroundImage(Layout)`, `Text` keep their
/// base state; `ContextMenuStrip` is likewise a `Control` property, and it now
/// lives on `ControlBase` (as `context_menu_strip`, an opaque key the host
/// resolves) — reached through `Deref`, never restated here.
/// `PreferredHeight` is a computed getter, not
/// stored state, and is exposed as [`UpDownBase::preferred_height`].
///
/// The five genuinely new fields are below.
//
// `Clone` is required: `Control: ControlClone` (in `control.rs`) makes every
// control cloneable so a `Box<dyn Control>` can be, and the composed
// `ContainerControl` is itself `Clone`.
#[derive(Clone)]
pub struct UpDownBase {
    container: crate::containers::ContainerControl,

    border_style: BorderStyle,
    intercept_arrow_keys: bool,
    read_only: bool,
    text_align: HorizontalAlignment,
    up_down_align: LeftRightAlignment,
}

impl UpDownBase {
    /// The defaults `UpDownBase` documents: `BorderStyle = Fixed3D`,
    /// `InterceptArrowKeys = true`, `ReadOnly = false`, `TextAlign = Left`,
    /// `UpDownAlign = Right`.
    ///
    /// `up_down_align` is stated here rather than taken from a
    /// `LeftRightAlignment::default()`, because that type deliberately has no
    /// `Default`: `UpDownBase.UpDownAlign` defaults to `Right` while
    /// `DateTimePicker.DropDownAlign` defaults to `Left`, so the default
    /// belongs to the property and not to the shared type.
    pub fn new() -> Self {
        Self {
            container: crate::containers::ContainerControl::default(),
            border_style: BorderStyle::Fixed3D,
            intercept_arrow_keys: true,
            read_only: false,
            text_align: HorizontalAlignment::Left,
            up_down_align: LeftRightAlignment::Right,
        }
    }

    pub fn border_style(&self) -> BorderStyle {
        self.border_style
    }
    pub fn set_border_style(&mut self, v: BorderStyle) {
        self.border_style = v;
    }
    pub fn intercept_arrow_keys(&self) -> bool {
        self.intercept_arrow_keys
    }
    pub fn set_intercept_arrow_keys(&mut self, v: bool) {
        self.intercept_arrow_keys = v;
    }
    pub fn read_only(&self) -> bool {
        self.read_only
    }
    pub fn set_read_only(&mut self, v: bool) {
        self.read_only = v;
    }
    pub fn text_align(&self) -> HorizontalAlignment {
        self.text_align
    }
    pub fn set_text_align(&mut self, v: HorizontalAlignment) {
        self.text_align = v;
    }
    pub fn up_down_align(&self) -> LeftRightAlignment {
        self.up_down_align
    }
    pub fn set_up_down_align(&mut self, v: LeftRightAlignment) {
        self.up_down_align = v;
    }

    /// `PreferredHeight` — the font's line height plus the 3-D border, the way
    /// the toolkit derives it. Read-only, so a getter rather than a field.
    pub fn preferred_height(&self, c: &dyn Canvas) -> f32 {
        let line = c.measure("0", &c.formats().body).max(1.0);
        // `measure` returns a width; the line box is roughly the body size.
        // Border adds 2 DIP top and bottom in Fixed3D. All DIP — `Canvas` is
        // already a DIP coordinate space, so nothing is scaled here.
        let border = if self.border_style == BorderStyle::None { 0.0 } else { 4.0 };
        line.max(16.0) + 4.0 + border
    }
}

impl Default for UpDownBase {
    fn default() -> Self {
        Self::new()
    }
}

// Note the two-hop deref: `UpDownBase` → `ContainerControl`. Reaching
// `ControlBase` goes through the container's own `Deref` chain, so callers write
// `updown.text` and the container resolves it.
impl std::ops::Deref for UpDownBase {
    type Target = crate::containers::ContainerControl;
    fn deref(&self) -> &crate::containers::ContainerControl {
        &self.container
    }
}
impl std::ops::DerefMut for UpDownBase {
    fn deref_mut(&mut self) -> &mut crate::containers::ContainerControl {
        &mut self.container
    }
}

/// Draws the shared UpDown chrome — the framed edit box on the left and the
/// stacked up/down spin buttons on the right — and returns the rectangle left
/// for the text, so each concrete control only has to render its own string.
fn paint_updown_frame(base: &UpDownBase, c: &dyn Canvas, bounds: Rect) -> Rect {
    let th = c.theme();
    let button_w = 16.0;

    // Field. `stroke_rounded` draws a one-PHYSICAL-pixel border itself, so the
    // hairline stays crisp at any DPI without arithmetic here.
    c.fill_rounded(&bounds, 2.0, &th.card_background);
    if base.border_style != BorderStyle::None {
        c.stroke_rounded(&bounds, 2.0, &th.border_strong);
    }

    // Spin buttons on the side `UpDownAlign` asks for (default right).
    let on_right = base.up_down_align == LeftRightAlignment::Right;
    let buttons = if on_right {
        Rect::new(bounds.right - button_w, bounds.top, bounds.right, bounds.bottom)
    } else {
        Rect::new(bounds.left, bounds.top, bounds.left + button_w, bounds.bottom)
    };
    let mid_y = (buttons.top + buttons.bottom) * 0.5;
    let up = Rect::new(buttons.left, buttons.top, buttons.right, mid_y);
    let down = Rect::new(buttons.left, mid_y, buttons.right, buttons.bottom);
    c.fill_rounded(&up, 0.0, &th.layer_fill);
    c.fill_rounded(&down, 0.0, &th.layer_fill);
    // Vector chevrons, not text: the embedded face has no `▲ ▼` glyphs and
    // would render tofu boxes.
    c.vector_icon("ChevronUp", &up, ARROW_GLYPH, &th.text_secondary);
    c.vector_icon("ChevronDown", &down, ARROW_GLYPH, &th.text_secondary);

    // Text area is the field minus the buttons and a little padding.
    let pad = 4.0;
    if on_right {
        Rect::new(bounds.left + pad, bounds.top, buttons.left - pad, bounds.bottom)
    } else {
        Rect::new(buttons.right + pad, bounds.top, bounds.right - pad, bounds.bottom)
    }
}

// ── NumericUpDown ────────────────────────────────────────────────────────────

/// `System.Windows.Forms.NumericUpDown` — a spinner over a decimal `Value`.
///
/// WinForms stores `Value`/`Minimum`/`Maximum`/`Increment` as `System.Decimal`;
/// the port uses `f64`, which reproduces every behaviour the reference exercises
/// (decimal places, hex, thousands grouping) and is documented as the one place
/// exact base-10 arithmetic is traded for a float.
///
/// # Known limitation — binary floating point
///
/// This is the one place the port cannot match the toolkit digit for digit.
/// `Decimal` is base-10 and exact for these values; `f64` is base-2 and is not.
/// Stepping `Increment = 0.1` three times from zero gives `0.3` in WinForms and
/// `0.30000000000000004` here. It is a MODEL limitation, not a defect, and it is
/// invisible in the control's own output: `DecimalPlaces` rounds the displayed
/// [`NumericUpDown::display_text`] to `0,30` either way. Only a caller reading
/// [`NumericUpDown::value`] raw can observe it. Fixing it would mean carrying a
/// decimal type, which the crate deliberately does not take a dependency on.
#[derive(Clone)]
pub struct NumericUpDown {
    base: UpDownBase,

    minimum: f64,
    maximum: f64,
    value: f64,
    increment: f64,
    decimal_places: i32,
    hexadecimal: bool,
    thousands_separator: bool,
    /// **Port addition**, not a WinForms property: the toolkit reads the
    /// separators from `CultureInfo.CurrentCulture`, which a control here
    /// cannot query. Defaults to [`NumberFormat::FR`] so a form shows a French
    /// number beside its French dates.
    pub number_format: NumberFormat,
}

impl Default for NumericUpDown {
    fn default() -> Self {
        Self {
            base: UpDownBase::new(),
            minimum: 0.0,
            maximum: 100.0,
            value: 0.0,
            increment: 1.0,
            decimal_places: 0,
            hexadecimal: false,
            thousands_separator: false,
            number_format: NumberFormat::FR,
        }
    }
}

impl NumericUpDown {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn minimum(&self) -> f64 {
        self.minimum
    }
    pub fn maximum(&self) -> f64 {
        self.maximum
    }
    pub fn value(&self) -> f64 {
        self.value
    }
    pub fn increment(&self) -> f64 {
        self.increment
    }
    pub fn decimal_places(&self) -> i32 {
        self.decimal_places
    }
    pub fn hexadecimal(&self) -> bool {
        self.hexadecimal
    }
    pub fn thousands_separator(&self) -> bool {
        self.thousands_separator
    }

    /// `Increment` — the spin step. A negative increment is **refused**: the
    /// WinForms setter throws. Coercing instead was actively harmful, because a
    /// negative step made `up_button` walk *below* `Minimum`.
    pub fn set_increment(&mut self, v: f64) -> Result<(), OutOfRange> {
        if v < 0.0 {
            return Err(OutOfRange::at_least(v, 0.0));
        }
        self.increment = v;
        Ok(())
    }

    /// `DecimalPlaces` — digits shown after the separator. The toolkit accepts
    /// `0..=99` and throws outside that, so this refuses rather than coerces.
    pub fn set_decimal_places(&mut self, v: i32) -> Result<(), OutOfRange> {
        if !(0..=99).contains(&v) {
            return Err(OutOfRange::between(v as f64, 0.0, 99.0));
        }
        self.decimal_places = v;
        Ok(())
    }
    pub fn set_hexadecimal(&mut self, v: bool) {
        self.hexadecimal = v;
    }
    pub fn set_thousands_separator(&mut self, v: bool) {
        self.thousands_separator = v;
    }

    /// Setting `Minimum` above `Maximum` raises `Maximum` too, and pulls `Value`
    /// up if needed — the toolkit keeps `Minimum <= Value <= Maximum` here by
    /// coercion, unlike the `Value` setter which throws.
    pub fn set_minimum(&mut self, v: f64) {
        self.minimum = v;
        if self.maximum < self.minimum {
            self.maximum = self.minimum;
        }
        self.value = self.value.clamp(self.minimum, self.maximum);
    }
    pub fn set_maximum(&mut self, v: f64) {
        self.maximum = v;
        if self.minimum > self.maximum {
            self.minimum = self.maximum;
        }
        self.value = self.value.clamp(self.minimum, self.maximum);
    }

    /// Assigns `Value`. **Errors** if outside `[Minimum, Maximum]** — this is the
    /// documented trap: `numeric.Value = 255` throws while `Maximum` is still
    /// the default 100, so `Maximum` must be raised *first*. Reproduced as a
    /// `Result` so the failure cannot be swallowed.
    pub fn set_value(&mut self, v: f64) -> Result<(), OutOfRange> {
        if v < self.minimum || v > self.maximum {
            return Err(OutOfRange { value: v, minimum: self.minimum, maximum: self.maximum });
        }
        self.value = v;
        Ok(())
    }

    /// Steps `Value` by `+Increment` (the spin button never throws, it stops at
    /// the edge). Clamped to BOTH bounds, not just the one it moves toward — a
    /// step must never be able to leave the value outside the range.
    pub fn up_button(&mut self) {
        self.value = (self.value + self.increment).clamp(self.minimum, self.maximum);
    }
    /// Steps `Value` by `-Increment`, clamped to both bounds.
    pub fn down_button(&mut self) {
        self.value = (self.value - self.increment).clamp(self.minimum, self.maximum);
    }

    /// The string the control paints for `Value`, honouring `Hexadecimal`,
    /// `DecimalPlaces` and `ThousandsSeparator`. `Hexadecimal` wins over the
    /// decimal options, exactly as WinForms formats it; the separators come
    /// from [`NumericUpDown::number_format`], French by default, which is what
    /// the reference sheet shows (`3,50`, `10 000`).
    pub fn display_text(&self) -> String {
        format_numeric(
            self.value,
            self.decimal_places,
            self.hexadecimal,
            self.thousands_separator,
            &self.number_format,
        )
    }
}

/// Free function so the formatting is testable without a control instance.
fn format_numeric(
    value: f64,
    decimal_places: i32,
    hexadecimal: bool,
    thousands: bool,
    fmt: &NumberFormat,
) -> String {
    if hexadecimal {
        // Hex ignores decimal places and grouping; the value is truncated to an
        // integer and printed uppercase, as `NumericUpDown` does.
        return format!("{:X}", value.trunc() as i64);
    }

    let dp = decimal_places.max(0) as usize;
    let negative = value.is_sign_negative() && value != 0.0;
    let magnitude = value.abs();
    let fixed = format!("{magnitude:.dp$}");

    let (int_part, frac_part) = match fixed.split_once('.') {
        Some((i, f)) => (i.to_string(), Some(f.to_string())),
        None => (fixed, None),
    };

    let grouped = if thousands {
        group_thousands(&int_part, fmt.group_separator)
    } else {
        int_part
    };
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(&grouped);
    if let Some(f) = frac_part {
        out.push_str(fmt.decimal_separator);
        out.push_str(&f);
    }
    out
}

/// Inserts `separator` every three digits from the right.
fn group_thousands(digits: &str, separator: &str) -> String {
    let bytes = digits.as_bytes();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 * separator.len());
    let n = bytes.len();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (n - i).is_multiple_of(3) {
            out.push_str(separator);
        }
        out.push(*b as char);
    }
    out
}

impl std::ops::Deref for NumericUpDown {
    type Target = UpDownBase;
    fn deref(&self) -> &UpDownBase {
        &self.base
    }
}
impl std::ops::DerefMut for NumericUpDown {
    fn deref_mut(&mut self) -> &mut UpDownBase {
        &mut self.base
    }
}

impl Control for NumericUpDown {
    fn control(&self) -> &ControlBase {
        self.base.container.control()
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        self.base.container.control_mut()
    }
    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        Size::new(self.control().width().max(60.0), self.base.preferred_height(c))
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        let text_rect = paint_updown_frame(&self.base, c, bounds);
        let th = c.theme();
        // `TextAlign` honoured through the shared mapping, so a right-aligned
        // spinner (the usual choice for numbers) lines its digits up.
        c.text_aligned(
            &self.display_text(),
            &text_rect,
            &c.formats().body,
            &th.text_primary,
            self.base.text_align.dwrite(),
        );
    }
    fn type_name(&self) -> &'static str {
        "NumericUpDown"
    }
}

// ── DomainUpDown ─────────────────────────────────────────────────────────────

/// `System.Windows.Forms.DomainUpDown` — a spinner over a list of strings
/// instead of a number. Adds the items collection, the selection, and the two
/// behaviour switches `Sorted` and `Wrap`.
#[derive(Clone)]
pub struct DomainUpDown {
    base: UpDownBase,

    items: Vec<String>,
    /// `SelectedIndex` — `-1` when nothing is selected (the default).
    selected_index: i32,
    sorted: bool,
    wrap: bool,
}

impl Default for DomainUpDown {
    fn default() -> Self {
        Self {
            base: UpDownBase::new(),
            items: Vec::new(),
            selected_index: -1,
            sorted: false,
            wrap: false,
        }
    }
}

impl DomainUpDown {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn items(&self) -> &[String] {
        &self.items
    }
    pub fn sorted(&self) -> bool {
        self.sorted
    }
    pub fn wrap(&self) -> bool {
        self.wrap
    }
    pub fn set_wrap(&mut self, v: bool) {
        self.wrap = v;
    }

    /// Turning `Sorted` on re-sorts the existing items immediately, as the
    /// toolkit does.
    ///
    /// **`SelectedIndex` is preserved, not the selected item.** Verified against
    /// the real control: Charlie/Alpha/Bravo with index 0 selected, sorted, ends
    /// with index 0 — which is now `Alpha`, not `Charlie`. The port used to
    /// chase the item to its new position, which looks more helpful and is
    /// simply not what the toolkit does.
    pub fn set_sorted(&mut self, v: bool) {
        self.sorted = v;
        if v {
            self.items.sort();
            // The index stands; refresh the displayed text for whatever now
            // occupies it.
            self.sync_text();
        }
    }

    /// Adds an item. When `Sorted`, it is inserted at its sorted position.
    pub fn add(&mut self, item: impl Into<String>) {
        let item = item.into();
        if self.sorted {
            let pos = self.items.partition_point(|i| *i < item);
            self.items.insert(pos, item);
        } else {
            self.items.push(item);
        }
        self.sync_text();
    }

    pub fn selected_index(&self) -> i32 {
        self.selected_index
    }

    /// Sets the selection. Valid values are `-1` (clear) and `0..=len-1`;
    /// anything else is **refused**, as the WinForms setter throws
    /// `ArgumentOutOfRangeException`.
    ///
    /// Clearing with `-1` does **not** clear [`DomainUpDown::text`] — the
    /// toolkit leaves the last shown string in place, which is visible in the
    /// control long after `SelectedItem` has become empty.
    pub fn set_selected_index(&mut self, index: i32) -> Result<(), OutOfRange> {
        let last = self.items.len() as i32 - 1;
        if index < -1 || index > last {
            return Err(OutOfRange::between(index as f64, -1.0, last as f64));
        }
        self.selected_index = index;
        self.sync_text();
        Ok(())
    }

    pub fn selected_item(&self) -> Option<&str> {
        if self.selected_index < 0 {
            return None;
        }
        self.items.get(self.selected_index as usize).map(String::as_str)
    }

    /// `Text` — the string the control displays.
    ///
    /// It tracks the selection, but is **not** cleared when the selection is:
    /// after `SelectedIndex = -1` the toolkit still shows the previous item.
    pub fn text(&self) -> &str {
        &self.control().text
    }

    /// Copies the selected item into `Text`. A cleared selection leaves the
    /// previous text alone — that asymmetry is the toolkit's, not an oversight.
    fn sync_text(&mut self) {
        if let Some(item) = self.selected_item().map(str::to_owned) {
            self.control_mut().text = item;
        }
    }

    /// Moves the selection toward the end of the list (`DownButton`). With
    /// `Wrap`, stepping past the last item lands on the first; without it, the
    /// selection stays put at the end. From « nothing selected » it selects the
    /// first item.
    pub fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let last = self.items.len() as i32 - 1;
        self.selected_index = if self.selected_index < 0 {
            0
        } else if self.selected_index >= last {
            if self.wrap { 0 } else { last }
        } else {
            self.selected_index + 1
        };
        self.sync_text();
    }

    /// Moves the selection toward the start of the list (`UpButton`). With
    /// `Wrap`, stepping before the first item lands on the last.
    ///
    /// From « nothing selected » (`-1`) it selects **nothing** and stays at
    /// `-1`, unlike [`DomainUpDown::select_next`] which selects the first item.
    /// The asymmetry is the toolkit's: stepping backwards out of an empty
    /// selection has nowhere to go.
    pub fn select_previous(&mut self) {
        if self.items.is_empty() || self.selected_index < 0 {
            return;
        }
        let last = self.items.len() as i32 - 1;
        self.selected_index = if self.selected_index == 0 {
            if self.wrap { last } else { 0 }
        } else {
            self.selected_index - 1
        };
        self.sync_text();
    }
}

impl std::ops::Deref for DomainUpDown {
    type Target = UpDownBase;
    fn deref(&self) -> &UpDownBase {
        &self.base
    }
}
impl std::ops::DerefMut for DomainUpDown {
    fn deref_mut(&mut self) -> &mut UpDownBase {
        &mut self.base
    }
}

impl Control for DomainUpDown {
    fn control(&self) -> &ControlBase {
        self.base.container.control()
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        self.base.container.control_mut()
    }
    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        Size::new(self.control().width().max(60.0), self.base.preferred_height(c))
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        let text_rect = paint_updown_frame(&self.base, c, bounds);
        let th = c.theme();
        c.text_aligned(
            self.text(),
            &text_rect,
            &c.formats().body,
            &th.text_primary,
            self.base.text_align.dwrite(),
        );
    }
    fn type_name(&self) -> &'static str {
        "DomainUpDown"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Defaults, asserted against the catalogue ────────────────────────────

    #[test]
    fn scrollbar_defaults_match_the_toolkit() {
        let s = ScrollBar::new();
        assert_eq!((s.minimum(), s.maximum(), s.value()), (0, 100, 0));
        assert_eq!((s.small_change(), s.large_change()), (1, 10));
        assert!(s.scale_for_dpi());
        // A scrollbar is not a tab stop, unlike a plain Control.
        assert!(!s.control.tab_stop);
    }

    #[test]
    fn vscrollbar_declares_only_right_to_left() {
        let v = VScrollBar::new();
        assert_eq!(v.right_to_left, RightToLeft::Inherit);
        // Everything else comes from the shared base.
        assert_eq!(v.maximum(), 100);
    }

    #[test]
    fn trackbar_defaults_match_the_toolkit() {
        let t = TrackBar::new();
        assert_eq!((t.minimum(), t.maximum(), t.value()), (0, 10, 0));
        assert_eq!((t.small_change(), t.large_change()), (1, 5));
        assert_eq!(t.tick_frequency(), 1);
        assert_eq!(t.tick_style(), TickStyle::BottomRight);
        assert_eq!(t.orientation(), Orientation::Horizontal);
        // TrackBar auto-sizes by default.
        assert!(t.control.auto_size);
    }

    #[test]
    fn numericupdown_defaults_match_the_toolkit() {
        let n = NumericUpDown::new();
        assert_eq!((n.minimum(), n.maximum(), n.value()), (0.0, 100.0, 0.0));
        assert_eq!(n.increment(), 1.0);
        assert_eq!(n.decimal_places(), 0);
        assert!(!n.hexadecimal());
        assert!(!n.thousands_separator());
        // UpDownBase defaults reachable through Deref.
        assert_eq!(n.border_style(), BorderStyle::Fixed3D);
        assert!(n.intercept_arrow_keys());
        assert_eq!(n.up_down_align(), LeftRightAlignment::Right);
    }

    /// `LeftRightAlignment` has no `Default` on purpose — `UpDownBase.UpDownAlign`
    /// is `Right` while `DateTimePicker.DropDownAlign` is `Left`, so the default
    /// belongs to the property. This asserts THIS property's own default, which
    /// is the half of that divergence this family owns.
    #[test]
    fn updown_states_its_own_alignment_default() {
        assert_eq!(UpDownBase::new().up_down_align(), LeftRightAlignment::Right);
        assert_eq!(UpDownBase::default().up_down_align(), LeftRightAlignment::Right);
        // Both concrete spinners inherit it through the composition chain.
        assert_eq!(NumericUpDown::new().up_down_align(), LeftRightAlignment::Right);
        assert_eq!(DomainUpDown::new().up_down_align(), LeftRightAlignment::Right);
        // `TextAlign` is the ordinary WinForms default, and IS honoured on paint.
        assert_eq!(UpDownBase::new().text_align(), HorizontalAlignment::Left);
    }

    #[test]
    fn domainupdown_defaults_match_the_toolkit() {
        let d = DomainUpDown::new();
        assert!(d.items().is_empty());
        assert_eq!(d.selected_index(), -1);
        assert!(!d.sorted());
        assert!(!d.wrap());
        assert!(d.selected_item().is_none());
    }

    // ── The Maximum - LargeChange + 1 rule ──────────────────────────────────

    #[test]
    fn the_highest_reachable_value_is_maximum_minus_largechange_plus_one() {
        let s = ScrollBar::new(); // min 0, max 100, large 10
        assert_eq!(s.max_reachable_value(), 91);
    }

    #[test]
    fn scrolling_stops_at_the_reachable_maximum_but_set_value_reaches_maximum() {
        let mut s = ScrollBar::new();
        // Paging/dragging cannot pass 91… (`scroll_to` clamps rather than
        // refuses, so its result is deliberately discarded here).
        let _ = s.scroll_to(1000);
        assert_eq!(s.value(), 91);
        // …but assigning is allowed anywhere inside [Minimum, Maximum].
        s.set_value(100).expect("100 is within [0, 100]");
        assert_eq!(s.value(), 100);
        // Outside the range is an error, never a silent clamp.
        assert!(s.set_value(101).is_err());
        assert!(s.set_value(-1).is_err());
    }

    #[test]
    fn a_larger_page_makes_a_shorter_reach_and_a_longer_thumb() {
        // The reference sheet's two HScrollBars: LargeChange 20 vs 50.
        let mut a = ScrollBar::new();
        a.set_large_change(20).unwrap();
        let mut b = ScrollBar::new();
        b.set_large_change(50).unwrap();
        assert_eq!(a.max_reachable_value(), 81);
        assert_eq!(b.max_reachable_value(), 51);
        // Bigger page ⇒ bigger grip: thumb_fraction = LargeChange/(span+1).
        assert!(b.thumb_fraction() > a.thumb_fraction());
        assert!((a.thumb_fraction() - 20.0 / 101.0).abs() < 1e-6);
    }

    /// Setting `LargeChange` must LEAVE `Value` alone, even when the value now
    /// sits above the scroll ceiling — the toolkit strands it there deliberately
    /// and the next gesture snaps it down.
    #[test]
    fn setting_large_change_does_not_move_value() {
        let mut s = ScrollBar::new();
        s.set_value(91).unwrap();
        s.set_large_change(50).unwrap(); // ceiling drops to 51 …
        assert_eq!(s.value(), 91, "the toolkit keeps 91");
        // … and only the next gesture brings it down.
        s.page_up();
        assert_eq!(s.value(), 41, "91 - 50, not 51 - 50");
    }

    /// The getters report the toolkit's clamped view while the FIELD keeps what
    /// was assigned, so widening the page reveals the original line step again.
    #[test]
    fn the_step_getters_clamp_but_the_setters_remember() {
        let mut s = ScrollBar::new(); // 0..100, large 10
        s.set_small_change(40).unwrap();
        assert_eq!(s.small_change(), 10, "a line may not outrun a page");
        s.set_large_change(60).unwrap();
        assert_eq!(s.small_change(), 40, "the assigned 40 was never lost");

        let mut w = ScrollBar::new();
        w.set_large_change(500).unwrap();
        assert_eq!(w.large_change(), 101, "capped at the whole range, 100-0+1");

        // An empty range collapses both steps to the toolkit's floor of 1.
        let mut e = ScrollBar::new();
        e.set_minimum(200);
        assert_eq!((e.minimum(), e.maximum()), (200, 200));
        assert_eq!(e.large_change(), 1);
        assert_eq!(e.small_change(), 1);
    }

    /// A negative step is refused, not coerced to zero.
    #[test]
    fn negative_steps_are_refused() {
        let mut s = ScrollBar::new();
        assert!(s.set_large_change(-5).is_err());
        assert_eq!(s.large_change(), 10, "the refused value changed nothing");
        assert!(s.set_small_change(-1).is_err());
        assert_eq!(s.small_change(), 1);

        let mut t = TrackBar::new();
        assert!(t.set_small_change(-1).is_err());
        assert!(t.set_large_change(-3).is_err());
        assert_eq!((t.small_change(), t.large_change()), (1, 5));
    }

    /// With `LargeChange = 0` the ceiling lands one PAST `Maximum`, so the
    /// assignment a gesture would make is refused and nothing moves — verified
    /// against the real control, which behaves the same from 0 and from 50.
    #[test]
    fn a_zero_page_bar_does_not_scroll() {
        let mut s = ScrollBar::new();
        s.set_large_change(0).unwrap();
        assert_eq!(s.small_change(), 0, "a line is capped by the page");
        assert_eq!(s.max_reachable_value(), 100, "never past Maximum");

        // Scrolling to the end aims one PAST Maximum, so it is refused outright
        // rather than silently doing nothing — the toolkit throws here too.
        assert!(s.scroll_to(i32::MAX).is_err());
        assert_eq!(s.value(), 0, "and the value is untouched");

        s.set_value(50).unwrap();
        for _ in 0..3 {
            assert!(s.scroll_to(i32::MAX).is_err());
            s.line_down();
            s.page_down();
        }
        assert_eq!(s.value(), 50, "every gesture is a no-op");
        // Scrolling to the START is always reachable.
        assert!(s.scroll_to(i32::MIN).is_ok());
        assert_eq!(s.value(), 0);
    }

    /// Moving the range pulls `Value` into `[Minimum, Maximum]` — the
    /// assignable range, NOT the scroll ceiling.
    #[test]
    fn moving_the_range_clamps_value_to_maximum_not_to_the_ceiling() {
        let mut s = ScrollBar::new();
        s.set_value(100).unwrap();
        s.set_maximum(50);
        assert_eq!(s.value(), 50, "50, not 50-10+1 = 41");
    }

    // ── NumericUpDown: the out-of-range contract and formatting ─────────────

    #[test]
    fn numeric_value_throws_out_of_range_until_maximum_is_raised() {
        let mut n = NumericUpDown::new(); // max 100
        // The documented trap: Value = 255 fails while Maximum is 100.
        assert!(n.set_value(255.0).is_err());
        // Raise Maximum first, then the assignment succeeds — the required order.
        n.set_maximum(255.0);
        assert!(n.set_value(255.0).is_ok());
        assert_eq!(n.value(), 255.0);
    }

    /// The four NumericUpDowns on the reference sheet, in the library's own
    /// (French) convention: `42`, `3,50`, `FF`, `10 000`.
    #[test]
    fn numeric_formatting_covers_decimals_hex_and_grouping() {
        let fr = &NumberFormat::FR;
        assert_eq!(format_numeric(42.0, 0, false, false, fr), "42");
        assert_eq!(format_numeric(3.5, 2, false, false, fr), "3,50");
        assert_eq!(format_numeric(255.0, 0, true, false, fr), "FF");
        assert_eq!(format_numeric(10000.0, 0, false, true, fr), "10\u{202F}000");
        // Hex wins over decimal places and grouping.
        assert_eq!(format_numeric(255.0, 2, true, true, fr), "FF");
        // Grouping with a fractional part.
        assert_eq!(format_numeric(1234567.5, 1, false, true, fr), "1\u{202F}234\u{202F}567,5");
        // Negative keeps its sign in front of the grouped magnitude.
        assert_eq!(format_numeric(-2500.0, 0, false, true, fr), "-2\u{202F}500");
    }

    /// The default must be French, so a form never shows a French date beside
    /// an English number — the inconsistency this setting exists to remove.
    #[test]
    fn the_default_number_convention_is_french_like_the_dates() {
        assert_eq!(NumberFormat::default(), NumberFormat::FR);
        let mut n = NumericUpDown::new();
        n.set_decimal_places(2).unwrap();
        n.set_value(3.5).unwrap();
        assert_eq!(n.display_text(), "3,50");

        // …and the invariant convention is still reachable for a caller that
        // wants a machine-readable field.
        n.number_format = NumberFormat::INVARIANT;
        assert_eq!(n.display_text(), "3.50");
    }

    // ── TrackBar cross-axis layout: the three TickStyle rows differ ─────────

    /// `BottomRight` puts ticks below only, `Both` above and below, `None`
    /// neither — the three rows must not paint alike.
    #[test]
    fn the_tick_styles_produce_three_different_layouts() {
        let none = track_bands(0.0, 45.0, TickStyle::None);
        assert!(none.tick_before.is_none() && none.tick_after.is_none());

        let bottom = track_bands(0.0, 45.0, TickStyle::BottomRight);
        assert!(bottom.tick_before.is_none(), "BottomRight has no row above");
        assert!(bottom.tick_after.is_some(), "BottomRight has a row below");

        let top = track_bands(0.0, 45.0, TickStyle::TopLeft);
        assert!(top.tick_before.is_some() && top.tick_after.is_none());

        let both = track_bands(0.0, 45.0, TickStyle::Both);
        assert!(both.tick_before.is_some() && both.tick_after.is_some());

        // `Both` is not `BottomRight`: the leading row pushes the thumb down.
        assert_ne!(both.thumb_lo, bottom.thumb_lo);
    }

    /// Ticks are SHORT marks clear of the thumb, not full-height lines — the
    /// defect that made every row look like a picket fence.
    #[test]
    fn ticks_are_short_marks_that_never_overlap_the_thumb() {
        let b = track_bands(0.0, 45.0, TickStyle::Both);
        let (lo_a, lo_b) = b.tick_before.unwrap();
        let (hi_a, hi_b) = b.tick_after.unwrap();
        assert_eq!(lo_b - lo_a, TRACKBAR_TICK_LEN);
        assert_eq!(hi_b - hi_a, TRACKBAR_TICK_LEN);
        // A few DIP, nowhere near the 45 DIP control height: a tick row must be
        // under a tenth of the box, or the rows read as a picket fence again.
        assert!(lo_b - lo_a < 45.0 / 10.0, "tick {} DIP is too long", lo_b - lo_a);
        // Clear of the thumb on both sides.
        assert!(lo_b <= b.thumb_lo && hi_a >= b.thumb_hi);
        // And inside the control.
        assert!(lo_a >= 0.0 && hi_b <= 45.0);
    }

    /// The channel is centred on the thumb, so the thumb rides the groove.
    #[test]
    fn the_channel_is_centred_on_the_thumb() {
        for style in [TickStyle::None, TickStyle::TopLeft, TickStyle::BottomRight, TickStyle::Both] {
            let b = track_bands(0.0, 45.0, style);
            assert!((b.centre - (b.thumb_lo + b.thumb_hi) / 2.0).abs() < 1e-6);
            assert!(b.thumb_hi > b.thumb_lo, "the thumb must have extent");
        }
    }

    /// A control shorter than the intrinsic thumb must still lay out sanely
    /// rather than invert its bands.
    #[test]
    fn a_squeezed_trackbar_does_not_invert() {
        let b = track_bands(0.0, 10.0, TickStyle::Both);
        assert!(b.thumb_hi > b.thumb_lo);
    }

    #[test]
    fn numeric_spin_buttons_clamp_at_the_edges() {
        let mut n = NumericUpDown::new();
        n.set_maximum(2.0);
        n.set_increment(1.0).unwrap();
        n.up_button();
        n.up_button();
        n.up_button(); // would be 3, clamps at 2
        assert_eq!(n.value(), 2.0);
        for _ in 0..5 {
            n.down_button();
        }
        assert_eq!(n.value(), 0.0);
    }

    // ── TrackBar value ↔ position round-trip ────────────────────────────────

    #[test]
    fn trackbar_value_and_position_round_trip_horizontally() {
        let mut t = TrackBar::new();
        t.set_maximum(10);
        let track = 200.0;
        for v in 0..=10 {
            t.set_value(v).unwrap();
            let offset = t.thumb_offset(track);
            assert_eq!(t.value_at_offset(offset, track), v, "value {v} must survive the round-trip");
        }
        // Endpoints land where geometry says they must.
        t.set_value(0).unwrap();
        assert!((t.thumb_offset(track) - 0.0).abs() < 1e-4);
        t.set_value(10).unwrap();
        assert!((t.thumb_offset(track) - track).abs() < 1e-4);
    }

    #[test]
    fn trackbar_vertical_maps_minimum_to_the_bottom() {
        let mut t = TrackBar::new();
        t.set_orientation(Orientation::Vertical);
        t.set_maximum(10);
        let track = 200.0;
        // Minimum sits at the bottom (offset == track_len), Maximum at the top.
        t.set_value(0).unwrap();
        assert!((t.thumb_offset(track) - track).abs() < 1e-4);
        t.set_value(10).unwrap();
        assert!((t.thumb_offset(track) - 0.0).abs() < 1e-4);
        // Round-trip still holds on the inverted axis.
        for v in 0..=10 {
            t.set_value(v).unwrap();
            assert_eq!(t.value_at_offset(t.thumb_offset(track), track), v);
        }
    }

    #[test]
    fn trackbar_value_setter_rejects_out_of_range() {
        let mut t = TrackBar::new(); // 0..10
        assert!(t.set_value(11).is_err());
        assert!(t.set_value(-1).is_err());
        assert!(t.set_value(5).is_ok());
    }

    // ── DomainUpDown wrapping and sorting ───────────────────────────────────

    #[test]
    fn domain_selection_wraps_only_when_wrap_is_set() {
        let mut d = DomainUpDown::new();
        for day in ["Lundi", "Mardi", "Mercredi"] {
            d.add(day);
        }
        d.set_selected_index(2).unwrap(); // "Mercredi"

        // Without wrap, stepping past the end stays put.
        d.select_next();
        assert_eq!(d.selected_index(), 2);

        // With wrap, it cycles to the front and back.
        d.set_wrap(true);
        d.select_next();
        assert_eq!(d.selected_item(), Some("Lundi"));
        d.select_previous();
        assert_eq!(d.selected_item(), Some("Mercredi"));
    }

    #[test]
    fn domain_previous_clamps_at_zero_without_wrap() {
        let mut d = DomainUpDown::new();
        d.add("a");
        d.add("b");
        d.set_selected_index(0).unwrap();
        d.select_previous();
        assert_eq!(d.selected_index(), 0);
    }

    /// Stepping backwards out of « nothing selected » stays at `-1`, while
    /// stepping forwards from it selects the first item. The asymmetry is the
    /// toolkit's, verified against the real control.
    #[test]
    fn domain_up_from_no_selection_selects_nothing() {
        let mut d = DomainUpDown::new();
        d.add("Lundi");
        d.add("Mardi");
        assert_eq!(d.selected_index(), -1);

        d.select_previous();
        assert_eq!(d.selected_index(), -1, "UpButton has nowhere to go");
        assert_eq!(d.text(), "", "and nothing is displayed yet");

        d.select_next();
        assert_eq!(d.selected_item(), Some("Lundi"), "DownButton picks the first");
    }

    /// Sorting keeps the INDEX, not the item — index 0 was Charlie and is Alpha
    /// afterwards. The port used to chase the item, which reads as more helpful
    /// and is not what the toolkit does.
    #[test]
    fn domain_sorted_keeps_the_index_not_the_item() {
        let mut d = DomainUpDown::new();
        for s in ["Charlie", "Alpha", "Bravo"] {
            d.add(s);
        }
        d.set_selected_index(0).unwrap(); // "Charlie"
        assert_eq!(d.text(), "Charlie");

        d.set_sorted(true);
        assert_eq!(d.items(), ["Alpha", "Bravo", "Charlie"]);
        assert_eq!(d.selected_index(), 0, "the index stands");
        assert_eq!(d.selected_item(), Some("Alpha"), "which is now Alpha");
        assert_eq!(d.text(), "Alpha", "and the display follows the index");
    }

    /// An out-of-range `SelectedIndex` is refused, not clamped; `-1` is the one
    /// legal « nothing selected » value.
    #[test]
    fn domain_selected_index_refuses_out_of_range() {
        let mut empty = DomainUpDown::new();
        assert!(empty.set_selected_index(0).is_err(), "no items to select");
        assert!(empty.set_selected_index(-1).is_ok(), "clearing is always legal");

        let mut d = DomainUpDown::new();
        d.add("a");
        d.add("b");
        assert!(d.set_selected_index(5).is_err());
        assert!(d.set_selected_index(-2).is_err());
        assert_eq!(d.selected_index(), -1, "a refused assignment changed nothing");
        assert!(d.set_selected_index(1).is_ok());
    }

    /// Clearing the selection does not clear the displayed `Text` — the toolkit
    /// keeps showing the last item after `SelectedIndex = -1`.
    #[test]
    fn domain_clearing_the_selection_keeps_the_text() {
        let mut d = DomainUpDown::new();
        d.add("a");
        d.add("b");
        d.set_selected_index(1).unwrap();
        assert_eq!(d.text(), "b");

        d.set_selected_index(-1).unwrap();
        assert_eq!(d.selected_index(), -1);
        assert_eq!(d.selected_item(), None, "nothing is selected …");
        assert_eq!(d.text(), "b", "… but 'b' is still what the control shows");
    }

    /// A negative `DecimalPlaces` or `Increment` is refused. The increment
    /// especially: a negative step made `up_button` walk BELOW `Minimum`.
    #[test]
    fn numeric_refuses_negative_decimal_places_and_increment() {
        let mut n = NumericUpDown::new();
        assert!(n.set_decimal_places(-1).is_err());
        assert_eq!(n.decimal_places(), 0);
        assert!(n.set_decimal_places(100).is_err(), "the toolkit's ceiling is 99");
        assert!(n.set_decimal_places(99).is_ok());

        assert!(n.set_increment(-1.0).is_err());
        assert_eq!(n.increment(), 1.0, "the refused increment changed nothing");
        // And a step can never leave the value outside the range.
        n.up_button();
        assert!(n.value() >= n.minimum() && n.value() <= n.maximum());
    }

    // ── TrackBar ticks ──────────────────────────────────────────────────────

    /// Ticks sit at `Minimum`, every `TickFrequency` below `Maximum`, then
    /// `Maximum` — so the LAST gap is short when the range is not a whole
    /// multiple of the frequency. Spreading them evenly misplaced every one.
    #[test]
    fn tick_values_close_on_maximum_with_a_short_last_gap() {
        let mut t = TrackBar::new();
        t.set_maximum(20);
        t.set_tick_frequency(3);
        assert_eq!(t.tick_values(), [0, 3, 6, 9, 12, 15, 18, 20]);
        assert_eq!(t.tick_count(), 8, "eight marks, not seven");

        // An exact multiple has no short gap.
        t.set_tick_frequency(5);
        assert_eq!(t.tick_values(), [0, 5, 10, 15, 20]);

        // The default 0..10 by 1.
        let d = TrackBar::new();
        assert_eq!(d.tick_count(), 11);
    }

    /// `TickFrequency` is stored verbatim — the toolkit neither clamps nor
    /// refuses — and a non-positive one simply leaves the two end marks.
    #[test]
    fn a_non_positive_tick_frequency_is_kept_but_draws_only_the_ends() {
        let mut t = TrackBar::new();
        t.set_maximum(20);

        t.set_tick_frequency(0);
        assert_eq!(t.tick_frequency(), 0, "stored, not corrected to 1");
        assert_eq!(t.tick_values(), [0, 20]);

        t.set_tick_frequency(-3);
        assert_eq!(t.tick_frequency(), -3, "negatives are stored too");
        assert_eq!(t.tick_values(), [0, 20]);
    }

    /// Ticks are placed through the same mapping as the thumb, so a tick and the
    /// thumb at that value land on the same pixel.
    #[test]
    fn ticks_and_the_thumb_share_one_mapping() {
        let mut t = TrackBar::new();
        t.set_maximum(20);
        t.set_tick_frequency(3);
        let track = 200.0;
        for v in t.tick_values() {
            t.set_value(v).unwrap();
            assert!((t.offset_of(v, track) - t.thumb_offset(track)).abs() < 1e-6);
        }
        // The uneven final gap is real: 18→20 is shorter than 15→18.
        let a = t.offset_of(18, track) - t.offset_of(15, track);
        let b = t.offset_of(20, track) - t.offset_of(18, track);
        assert!(b < a, "the last gap must be the short one");
    }

    /// A degenerate range must not produce an empty or inverted tick list.
    #[test]
    fn an_empty_range_has_one_tick() {
        let mut t = TrackBar::new();
        t.set_maximum(0);
        assert_eq!(t.tick_values(), [0]);
    }

    #[test]
    fn setting_scrollbar_bounds_never_inverts_the_range() {
        let mut s = ScrollBar::new();
        s.set_minimum(200); // above the current maximum of 100
        assert!(s.maximum() >= s.minimum());
        let mut t = TrackBar::new();
        t.set_maximum(-5); // below the current minimum of 0
        assert!(t.minimum() <= t.maximum());
    }
}
