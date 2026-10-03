//! Kubuno primitives — the date family: [`DatePicker`], [`MonthCalendar`] and
//! [`TimePicker`].
//!
//! # Why this family was missed, and what that changes
//!
//! The first eight families were picked off the WinForms replica list, and
//! `kubuno_controls::datetime` was on it from the start — it simply was not
//! read. Nothing about it is new: the replica already carries the whole civil
//! calendar ([`Date`], [`month_grid`], [`iso_week`]), the selection rules, the
//! bounds and the format engine, all of it tested. This layer adds pixels and
//! nothing else.
//!
//! # Where the pixels come from
//!
//! There is no hand-written predecessor in `drive-app-controls`, so — as
//! `docs/UI_BRIEF.md` requires — the reference is the **web**, read from source
//! (this machine cannot run the web app, so every number below was *read*, not
//! measured through CDP):
//!
//! | part | web source |
//! |---|---|
//! | the field | `core/frontend/src/ui/date-picker/DatePicker.tsx` — its trigger button, verbatim |
//! | the floating panel | `date-picker/PickerPopover.tsx` (`rounded-xl`, `p-3`, `shadow-2xl`) and `helpers.ts` (`popoverSize`, `computePos`) |
//! | the month grid | `date-picker/DayView.tsx` — the `grid-cols-7` of `h-8 w-8` day buttons, its weekday strip and its nav header |
//! | the spin column | `@ui/NumberInput`, the same `w-6` / `size={11}` pair [`crate::range`] uses |
//!
//! # What comes from the replica, and is never restated here
//!
//! *All* of the arithmetic. [`month_grid`] lays out the 6×7 cells — including
//! the rule that catches every port, that the leading week is shown **in full**
//! rather than collapsing to nothing when the 1st falls on the first day of the
//! week; [`Date`] owns leap years, month lengths and the day-of-week;
//! [`iso_week`] owns the week numbers; `MonthCalendar::set_selection_range`
//! owns ordering, `[MinDate, MaxDate]` clamping and the `MaxSelectionCount`
//! cap; `MonthCalendar::is_bold` owns the three bold lists;
//! `DateTimePicker::set_value` owns the value clamp and
//! `DateTimePicker::display_text` owns the fr-FR formatting. Not one of those is
//! re-derived below — each primitive owns its replica and [`Deref`]s to it.
//!
//! # What this layer adds
//!
//! Only concepts the replica genuinely does not model:
//!
//! * the **viewed month** — the toolkit derives the grid's month from
//!   `SelectionStart`, so a native calendar cannot be browsed without moving the
//!   selection; the web keeps a separate `viewDate` and so does
//!   [`MonthCalendar::view_month`];
//! * the **hot day** and the **hot header button** — a replica has no pointer;
//! * the **range phase** ([`MonthCalendar::extending`]) — the web's
//!   `rangePhase`, which the native control keeps in its drag state;
//! * the picker's **open** flag (`DroppedDown` is runtime state, not a designer
//!   property) and the [`MonthCalendar`] its panel is.
//!
//! # Stated gaps
//!
//! * `RightToLeftLayout` mirrors the field's *parts* (check box, icon and value
//!   move to the trailing edge, the spin column to the leading one) but not the
//!   value's text alignment: [`Canvas::text_ellipsis`] is leading-aligned by
//!   construction and there is no RTL-aware ellipsis to call.
//! * `CalendarDimensions` (a grid of up to twelve months) is honoured for
//!   `1×1` only; the web picker shows one month and has no multi-month layout to
//!   copy, so tiling it would be inventing a design rather than porting one.
//!   [`MonthCalendar::panel_size`] says so by measuring one month.
//! * `clearable` (the web's ✕ button) has no replica property. The toolkit's own
//!   « no value » affordance is `ShowCheckBox`, which *is* modelled and *is*
//!   painted, so the ✕ would be a second, parallel way to say the same thing.

use std::ops::{Deref, DerefMut};

use drive_app_controls::themes::shape::SHADOW_BLACK;
use drive_app_controls::{Canvas, Rect};
use kubuno_controls::control::FontRole;
use kubuno_controls::datetime as replica;
use kubuno_controls::datetime::{
    format_custom, iso_week, month_grid, Cell, Date, DateTime, DateTimePickerFormat, Names, FR,
};
use kubuno_controls::enums::{CheckState, Size};
use kubuno_controls::host::{self, vk, InputEvent, Modifiers};
use kubuno_controls::Control;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

use crate::metrics::{control, height, pill, radius, space, ShadowLayer};
use crate::{Widget, WidgetState};

// ─────────────────────────────────────────────────────────────────────────────
// Family metrics
//
// `crate::metrics` is the crate's ONE table and everything it already answers is
// taken from it (`height::BUTTON_MD` for the field, `radius::SM` for its
// corners, `radius::XL` for the panel, the spacing scale, the check-box
// geometry, `control::SEPARATOR` for a rule). What is left below is the picker's
// own grid: the lengths `DatePicker.tsx`, `PickerPopover.tsx`, `DayView.tsx` and
// `helpers.ts` state literally and no token names. Each one quotes the
// declaration it is. Nothing here is multiplied by `Canvas::scale`.
// ─────────────────────────────────────────────────────────────────────────────

/// The floating panel's grid, verbatim from `core/frontend/src/ui/date-picker/`.
pub mod calendar_metrics {
    use crate::metrics::{control, space};

    /// `popoverSize()` in `helpers.ts`: a calendar-bearing popover is **284**
    /// wide (the `time` mode's 172 has no calendar in it).
    pub const WIDTH: f32 = 284.0;
    /// `PickerPopover`: `p-3` around the calendar column.
    pub const PAD: f32 = space::MD;
    /// One column of the `grid-cols-7`: the panel's content width, split seven
    /// ways. Derived rather than stated because the web derives it too — the
    /// grid is `grid-cols-7` inside `p-3`, not a fixed track.
    pub const DAY_COLUMN: f32 = (WIDTH - 2.0 * PAD) / 7.0;
    /// `DayView`: a day button is `h-8 w-8`, centred in its track (`mx-auto`).
    /// So the disc is 32 and the track is wider — the gaps between discs are the
    /// web's, not a rounding.
    pub const DAY: f32 = 32.0;
    /// Six rows of seven. `calendarGrid` runs `startOfWeek(startOfMonth)` to
    /// `endOfWeek(endOfMonth)` and the replica's [`month_grid`] is exactly 42
    /// cells, so the two agree.
    ///
    /// [`month_grid`]: kubuno_controls::datetime::month_grid
    pub const ROWS: usize = 6;
    pub const COLUMNS: usize = 7;
    /// The navigation header: `w-7 h-7` round buttons over a row closed by
    /// `mb-2`.
    pub const HEADER: f32 = 28.0;
    pub const HEADER_GAP: f32 = space::SM;
    pub const NAV: f32 = 28.0;
    /// `<ChevronLeft size={14} />` / `<ChevronRight size={14} />`.
    pub const NAV_GLYPH: f32 = 14.0;
    /// The weekday strip: `h-7` cells closed by `mb-0.5`.
    pub const WEEKDAY: f32 = 28.0;
    pub const WEEKDAY_GAP: f32 = space::XXS;

    /// The footer, from `PickerPopover`'s own: `pt-3 mt-1 border-t border-border`
    /// over a `text-xs … py-1.5` row (a 16 DIP line inside 6 + 6).
    pub const FOOTER_RISE: f32 = space::XS;
    pub const FOOTER_RULE: f32 = control::SEPARATOR;
    pub const FOOTER_PAD: f32 = space::MD;
    pub const FOOTER_ROW: f32 = 28.0;
    /// What `ShowToday` costs a panel, in full.
    pub const FOOTER: f32 = FOOTER_RISE + FOOTER_RULE + FOOTER_PAD + FOOTER_ROW;

    /// `bg-primary/10` on a day INSIDE a selected range but not at its edge.
    pub const RANGE_ALPHA: f32 = 0.10;
    /// `opacity-30 cursor-not-allowed` on a day outside `[MinDate, MaxDate]`.
    pub const DISABLED_DAY_ALPHA: f32 = 0.30;

    /// The caption's month / year buttons: `text-sm font-semibold … px-1
    /// rounded hover:bg-surface-1` — a 4 DIP inset each side of the text, on
    /// a `text-sm` line box (20).
    pub const TITLE_PAD_X: f32 = space::XS;
    pub const TITLE_LINE: f32 = 20.0;

    /// The keyboard cursor's ring on a day disc. The web's day buttons carry
    /// no focus class of their own, so they wear the design system's
    /// `focus-visible` ring — `ring-2 ring-primary`, the same 2 DIP every
    /// Kubuno control paints.
    pub const DAY_FOCUS_RING: f32 = 2.0;
}

/// Tailwind's `shadow-2xl`, verbatim — what `PickerPopover` declares
/// (`bg-white rounded-xl shadow-2xl border border-border`):
/// `0 25px 50px -12px rgb(0 0 0 / .25)`. Pure black, as Tailwind writes it.
pub const SHADOW_2XL: [ShadowLayer; 1] =
    [ShadowLayer { dy: 25.0, blur: 50.0, spread: -12.0, opacity: 0.25 }];

/// How far the painter's layered shadow actually reaches, as a fraction of a
/// layer's CSS blur radius. This is `Painter::draw_layered_shadow`'s own
/// `REACH` (a gaussian keeps most of its density within ~3/4 of its radius):
/// the shadow is painted as rings out to `spread + blur × REACH`, so that is
/// the extent a popup must leave room for — no more, no less.
const SHADOW_REACH: f32 = 0.75;

/// One DIP of slack on every side of a shadow's reach: the outermost ring is
/// antialiased, and its soft edge must not land on the popup's border.
const SHADOW_SLACK: f32 = 1.0;

/// How far a layered shadow spills past its surface on each side, as
/// `(left, top, right, bottom)` — the pure answer to « how big must the popup
/// be so the shadow is not cut into a hard band ».
///
/// Every layer inflates the surface by `spread + blur × REACH` and then moves
/// it down by `dy`; the union of all layers is what is painted.
pub fn shadow_outset(layers: &[ShadowLayer]) -> (f32, f32, f32, f32) {
    let (mut side, mut top, mut bottom) = (0.0f32, 0.0f32, 0.0f32);
    for l in layers {
        let reach = (l.spread + l.blur * SHADOW_REACH).max(0.0);
        side = side.max(reach);
        top = top.max(reach - l.dy);
        bottom = bottom.max(reach + l.dy);
    }
    (side + SHADOW_SLACK, top.max(0.0) + SHADOW_SLACK, side + SHADOW_SLACK, bottom + SHADOW_SLACK)
}

use calendar_metrics as cal;

/// The weekday strip's labels, index 0 = Monday.
///
/// `helpers.ts` exports exactly this array (`WEEKDAYS = ['L','M','M','J','V','S','D']`)
/// and it is Monday-first, which is the same base
/// [`Date::weekday`](kubuno_controls::datetime::Date::weekday) counts on — so a
/// `FirstDayOfWeek` rotation is an index rotation and nothing else.
///
/// Note that these are the WEB's one-letter labels, not the replica's `lun.`
/// / `mar.` abbreviations: the replica paints the *system* look and this layer
/// paints the Kubuno one.
pub const WEEKDAYS: [&str; 7] = ["L", "M", "M", "J", "V", "S", "D"];

/// `DatePicker.tsx`' trigger: `flex items-center gap-2 px-3 rounded border` at
/// `h-9` — an `@ui/Input`, so its height, radius and inset are the shared
/// tokens and only the gap and the glyph are stated here.
const FIELD_PAD_X: f32 = space::MD;
const FIELD_GAP: f32 = space::SM;
/// `<Calendar size={14} />` / `<Clock size={14} />` in the trigger.
const FIELD_ICON: f32 = 14.0;
/// `focus:ring-2 focus:ring-primary focus:border-primary`.
const FIELD_FOCUS_RING: f32 = 2.0;
/// `disabled && 'bg-surface-2 cursor-not-allowed opacity-60'`.
const DISABLED_FIELD_ALPHA: f32 = 0.6;
/// `computePos` anchors the popover at `r.bottom + 4`. (A combo's is 2 — they
/// are different controls and the two files say different numbers.)
const PANEL_OFFSET: f32 = space::XS;
/// `computePos`' viewport margin: the popover keeps 8 px off every edge
/// (`window.innerHeight - r.bottom - 8`, `Math.max(8, …)`).
const PANEL_EDGE: f32 = space::SM;

/// The highlight behind the edited segment. The web declares no `::selection`
/// rule, so a field keeps the browser highlight — `--color-primary` at 35 %, a
/// plain rectangle on a 20 DIP line box — which is exactly what
/// `edit_box::draw_text` paints for a text selection. The segment IS the
/// selection of a date field, so it wears the same one.
const SEGMENT_ALPHA: f32 = 0.35;
const SEGMENT_LINE: f32 = 20.0;

/// `@ui/NumberInput`: the spin column is `w-6` and its chevrons `size={11}`,
/// separated from the field by a `border-l` and from each other by a
/// `border-b`. The same pair [`crate::range`]'s spinners use — `ShowUpDown` is
/// the toolkit calling a picker a spinner, so it must look like one.
const SPIN_COLUMN: f32 = 24.0;
const SPIN_GLYPH: f32 = 11.0;
const SPIN_RULE: f32 = control::SEPARATOR;

/// One **device** pixel expressed in DIP.
///
/// This is the one legitimate use of `Canvas::scale` in a family, and it
/// *divides* by it — the same helper, for the same reason, the replica keeps as
/// `hairline`. It is used here as a measuring allowance, not as a thickness:
/// `Painter::draw_layout` snaps the band it is handed onto the pixel grid
/// before laying the text out, so a band measured to the exact text width can
/// lose up to one physical pixel and trim a string that fits.
fn physical_pixel(c: &dyn Canvas) -> f32 {
    1.0 / c.scale().max(0.01)
}

/// The same colour at a fraction of its alpha — how this design system dims
/// (`opacity-30`, `opacity-60`), for which there is no token.
fn faded(color: &D2D1_COLOR_F, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: color.a * alpha, ..*color }
}

/// An explicitly-set colour if there is one, else the theme's.
///
/// The replica's own `or_system`, with a Kubuno token as the fallback instead of
/// a system colour: in WinForms an unset `TitleBackColor` / `TrailingForeColor`
/// / `ForeColor` means « take the ambient default », never a literal
/// transparent, and dropping that rule would silently ignore four modelled
/// properties.
fn or_theme(opt: Option<D2D1_COLOR_F>, fallback: D2D1_COLOR_F) -> D2D1_COLOR_F {
    opt.unwrap_or(fallback)
}

/// A [`FontRole`] resolved to the shared formats, as a (plain, emphasised)
/// pair.
///
/// The pair exists because `BoldedDates` has to read: the design system allows
/// six text sizes and exactly two weights, so a bolded day is the *medium* step
/// of the same size and a plain one the regular step. That does depart from
/// `DayView`, which draws every day at `font-medium` — but the web has no
/// bolded-dates concept at all, and painting every day medium would leave the
/// three bold lists nothing to say. Stated here rather than hidden in the paint.
fn day_formats(c: &dyn Canvas, role: FontRole) -> (&IDWriteTextFormat, &IDWriteTextFormat) {
    let f = c.formats();
    match role {
        FontRole::Caption | FontRole::CaptionStrong => (&f.caption, &f.caption_strong),
        FontRole::Body | FontRole::BodyStrong => (&f.body, &f.body_strong),
        FontRole::Heading => (&f.heading, &f.heading_strong),
        // The 22 DIP display size has no heavier sibling in `TextFormats`, so a
        // bolded date at that size is simply not emphasised. Said out loud
        // rather than substituted with a different size.
        FontRole::Title => (&f.title, &f.title),
    }
}

/// `date` shifted by `delta` months, the day clamped into the target month.
///
/// The month walk is the one `MonthCalendar::paint` performs to tile its grid
/// (`total = month - 1 + ordinal`, then `div_euclid` / `rem_euclid` by 12) —
/// the replica keeps it inside that loop rather than publishing it, so it is
/// spelled once here and used by nothing else. The clamp is
/// [`Date::days_in_month`](kubuno_controls::datetime::Date::days_in_month),
/// which is the replica's.
fn add_months(date: Date, delta: i32) -> Date {
    let total = date.month as i32 - 1 + delta;
    let year = date.year + total.div_euclid(12);
    let month = (total.rem_euclid(12) + 1) as u8;
    Date::new(year, month, date.day.min(Date::days_in_month(year, month)))
}

/// `janvier` → `Janvier`. `DayView` capitalises the month name the same way
/// (`mName.charAt(0).toUpperCase() + mName.slice(1)`); the fr-FR names
/// themselves are the replica's [`FR`], never re-listed here.
fn month_caption(year: i32, month: u8) -> String {
    let name = FR.months[month as usize];
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => format!("{}{} {year}", first.to_uppercase(), chars.as_str()),
        None => year.to_string(),
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// MonthCalendar
// ═════════════════════════════════════════════════════════════════════════════

/// Which button of the calendar's navigation header a point lands on.
///
/// `DayView` splits the caption into a month button and a year button (each
/// opening its own picker view). This layer reports one [`HeaderPart::Title`]
/// for the pair: splitting them needs a text measurement, and a hit test that
/// cannot be run without a live [`Canvas`] is a hit test the tests cannot pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderPart {
    /// `subMonths(viewDate, 1)`.
    Prev,
    /// `addMonths(viewDate, 1)`.
    Next,
    /// The « Août 2026 » caption.
    Title,
}

/// A month grid — the replica, with Kubuno pixels.
///
/// Everything about *being* a calendar is
/// [`kubuno_controls::datetime::MonthCalendar`]'s and is reached through
/// [`Deref`]: `SelectionStart` / `SelectionEnd` / `SelectionRange`,
/// `MaxSelectionCount`, `MinDate` / `MaxDate`, `FirstDayOfWeek`, `ScrollChange`,
/// `TodayDate` / `TodayDateSet`, `ShowToday` / `ShowTodayCircle` /
/// `ShowWeekNumbers`, the three bolded-date lists and the title / trailing
/// colours. The grid itself is [`month_grid`] and the week numbers are
/// [`iso_week`].
///
/// ## Where every number comes from
///
/// `core/frontend/src/ui/date-picker/DayView.tsx` and `PickerPopover.tsx`: a
/// `rounded-xl` panel with `p-3`, a `w-7 h-7` nav header closed by `mb-2`, an
/// `h-7` weekday strip closed by `mb-0.5`, and a `grid-cols-7` of `h-8 w-8` day
/// buttons — a filled accent disc when selected, an accent ring when today, a
/// `bg-primary/10` square inside a range, `hover:bg-surface-2` otherwise, and
/// `opacity-30` outside `[MinDate, MaxDate]`.
///
/// The panel paints its own surface and outline but **not** its shadow: a
/// `MonthCalendar` dropped in a form is not floating, and the one that IS —
/// [`DatePicker`]'s panel — draws the shadow before handing it the rectangle.
#[derive(Clone, Default)]
pub struct MonthCalendar {
    inner: replica::MonthCalendar,
    /// The month the grid shows, `None` to follow the toolkit's rule (the month
    /// of `SelectionStart`).
    ///
    /// The toolkit has no such property — a native calendar cannot be browsed
    /// without moving its selection — and the web does
    /// (`viewDate`), so this is a Kubuno addition rather than a duplicate.
    pub view_month: Option<Date>,
    /// The day under the pointer. The replica has no pointer, and
    /// `SelectionStart` is a selection, not a hover.
    pub hot_day: Option<Date>,
    /// The header button under the pointer.
    pub hot_header: Option<HeaderPart>,
    /// Whether the next click **extends** the range rather than starting a new
    /// one — the web's `rangePhase`, which the native control keeps in its drag
    /// state. Only ever set when `MaxSelectionCount > 1`.
    pub extending: bool,
    /// The keyboard cursor — the day the arrows move, the ARIA grid pattern's
    /// roving « focused cell ». `None` until a key moves it (a pointer click
    /// selects, it does not light a ring); [`MonthCalendar::active_day`] then
    /// falls back to the selection. The web's grid has no keyboard model of
    /// its own (each day is a Tab stop), so this follows the WAI-ARIA date
    /// picker dialog pattern instead: arrows, PageUp/PageDown, Home/End,
    /// Enter/Space.
    pub focus_day: Option<Date>,
}

/// What a key did to a [`MonthCalendar`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarKey {
    /// Not a calendar key: left in the queue for someone else.
    Ignored,
    /// The keyboard cursor moved (and the shown month followed it).
    Moved,
    /// Enter / Space picked this day — it went through
    /// [`MonthCalendar::click_day`], so the selection already holds it.
    Activated(Date),
}

impl Deref for MonthCalendar {
    type Target = replica::MonthCalendar;
    fn deref(&self) -> &replica::MonthCalendar {
        &self.inner
    }
}
impl DerefMut for MonthCalendar {
    fn deref_mut(&mut self) -> &mut replica::MonthCalendar {
        &mut self.inner
    }
}

impl MonthCalendar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shows `date`'s month and collapses the selection onto it — the two
    /// things a caller wants at once when it seeds a calendar.
    pub fn on(mut self, date: Date) -> Self {
        self.inner.set_selection_start(date);
        self.view_month = Some(date.first_of_month());
        self
    }

    // ── Which month, and moving between them ─────────────────────────────────

    /// The `(year, month)` the grid shows: [`MonthCalendar::view_month`], or the
    /// toolkit's own anchor (`SelectionStart`) when it is unset.
    pub fn shown_month(&self) -> (i32, u8) {
        let d = self.view_month.unwrap_or_else(|| self.inner.selection_start());
        (d.year, d.month)
    }

    /// How many months a next/prev click moves.
    ///
    /// `ScrollChange = 0` means « the number of months currently shown », which
    /// is the toolkit's documented default and what the replica's own doc
    /// comment on the property states.
    pub fn scroll_months(&self) -> i32 {
        if self.inner.scroll_change > 0 {
            self.inner.scroll_change
        } else {
            let d = self.inner.calendar_dimensions;
            (d.columns as i32 * d.rows as i32).max(1)
        }
    }

    /// Moves the grid by `delta × ScrollChange` months. Crossing a year
    /// boundary is the month walk's business, not a special case.
    pub fn scroll(&mut self, delta: i32) {
        let (year, month) = self.shown_month();
        let base = Date::new(year, month, 1);
        let months = delta * self.scroll_months();
        self.view_month = Some(add_months(base, months));
        // A keyboard cursor travels with the page, the way PageDown moves it
        // to the same day of the next month — otherwise it would be left on a
        // month nobody sees.
        if let Some(day) = self.focus_day {
            self.focus_day = Some(self.clamp_to_bounds(add_months(day, months)));
        }
    }

    // ── Keyboard ─────────────────────────────────────────────────────────────

    /// `date` pulled into `[MinDate, MaxDate]`.
    fn clamp_to_bounds(&self, date: Date) -> Date {
        date.clamp(self.inner.min_date(), self.inner.max_date())
    }

    /// The day the keyboard acts on: the cursor when it is on the shown
    /// month, else the selection's start when it is, else the 1st of the
    /// shown month — always inside `[MinDate, MaxDate]`.
    pub fn active_day(&self) -> Date {
        let (year, month) = self.shown_month();
        let on_page = |d: &Date| d.year == year && d.month == month;
        let day = self
            .focus_day
            .filter(on_page)
            .or_else(|| Some(self.inner.selection_start()).filter(on_page))
            .unwrap_or_else(|| Date::new(year, month, 1));
        self.clamp_to_bounds(day)
    }

    /// Puts the keyboard cursor on `date` (clamped into the bounds) and shows
    /// its month.
    pub fn move_focus_to(&mut self, date: Date) {
        let date = self.clamp_to_bounds(date);
        self.focus_day = Some(date);
        self.view_month = Some(date.first_of_month());
    }

    /// Whether `(vk, mods)` is a key [`MonthCalendar::key`] acts on — asked
    /// before the key is taken from the queue, so nothing else is swallowed.
    pub fn handles_key(vk: u16, mods: Modifiers) -> bool {
        match vk {
            vk::LEFT | vk::RIGHT | vk::UP | vk::DOWN | vk::HOME | vk::END | vk::ENTER | vk::SPACE => {
                mods.is_none()
            }
            vk::PAGE_UP | vk::PAGE_DOWN => mods.is_none() || mods.matches(Modifiers::SHIFT),
            _ => false,
        }
    }

    /// One key, the WAI-ARIA date grid way:
    ///
    /// | key | moves the cursor to |
    /// |---|---|
    /// | ← / → | the previous / next day |
    /// | ↑ / ↓ | the same weekday of the previous / next week |
    /// | Home / End | the first / last day of the week (`FirstDayOfWeek`) |
    /// | PageUp / PageDown | the same day of the previous / next month |
    /// | Shift + PageUp / PageDown | the same day of the previous / next year |
    /// | Enter / Space | — picks the cursor's day ([`CalendarKey::Activated`]) |
    ///
    /// The cursor never leaves `[MinDate, MaxDate]`, and a month walk clamps
    /// the day (31 January + 1 month = 28 February).
    pub fn key(&mut self, key: u16, mods: Modifiers) -> CalendarKey {
        if !Self::handles_key(key, mods) {
            return CalendarKey::Ignored;
        }
        let day = self.active_day();
        let in_week = (day.weekday() as i64 + 7 - self.inner.first_day() as i64) % 7;
        let year_step = mods.shift;
        let target = match key {
            vk::LEFT => day.add_days(-1),
            vk::RIGHT => day.add_days(1),
            vk::UP => day.add_days(-7),
            vk::DOWN => day.add_days(7),
            vk::HOME => day.add_days(-in_week),
            vk::END => day.add_days(6 - in_week),
            vk::PAGE_UP => add_months(day, if year_step { -12 } else { -1 }),
            vk::PAGE_DOWN => add_months(day, if year_step { 12 } else { 1 }),
            // Enter / Space.
            _ => {
                if !self.is_selectable(day) {
                    return CalendarKey::Ignored;
                }
                self.click_day(day);
                self.focus_day = Some(day);
                return CalendarKey::Activated(day);
            }
        };
        self.move_focus_to(target);
        CalendarKey::Moved
    }

    /// Takes this frame's calendar keys from the host queue (see
    /// [`MonthCalendar::key`]) and applies them in order. Call it while the
    /// calendar holds the focus; returns the day Enter / Space picked, if any.
    pub fn take_keys(&mut self) -> Option<Date> {
        let events = host::consume(|e| {
            matches!(e, InputEvent::Key { vk, down: true, mods, .. } if Self::handles_key(*vk, *mods))
        });
        let mut picked = None;
        for e in events {
            if let InputEvent::Key { vk, mods, .. } = e {
                if let CalendarKey::Activated(d) = self.key(vk, mods) {
                    picked = Some(d);
                }
            }
        }
        picked
    }

    /// `subMonths(viewDate, 1)` — one *scroll step*, which is one month at the
    /// default `ScrollChange`.
    pub fn prev_month(&mut self) {
        self.scroll(-1);
    }

    /// `addMonths(viewDate, 1)`.
    pub fn next_month(&mut self) {
        self.scroll(1);
    }

    /// The 42 cells of the shown month — [`month_grid`], with the replica's own
    /// `FirstDayOfWeek` resolution. Not one line of it is re-derived here.
    pub fn cells(&self) -> [Cell; 42] {
        let (year, month) = self.shown_month();
        month_grid(year, month, self.inner.first_day())
    }

    /// Whether `date` can be picked: inside `[MinDate, MaxDate]`. The web's
    /// `isDisabled` is the same test, and the replica clamps to the same window
    /// on every selection, so this only decides the *paint*.
    pub fn is_selectable(&self, date: Date) -> bool {
        (self.inner.min_date()..=self.inner.max_date()).contains(&date)
    }

    /// Whether `date` is inside the current selection, and whether it is one of
    /// its two edges — the web's `inRange` / `isEdge`.
    fn selection_of(&self, date: Date) -> (bool, bool) {
        let (start, end) = (self.inner.selection_start(), self.inner.selection_end());
        ((start..=end).contains(&date), date == start || date == end)
    }

    /// A click on `date`, run through the replica's selection rules.
    ///
    /// With `MaxSelectionCount == 1` it collapses the selection onto the day.
    /// Above that it walks the web's two phases: the first click anchors, the
    /// second extends — and the extension goes through
    /// `MonthCalendar::set_selection_end`, so ordering the ends, clamping into
    /// `[MinDate, MaxDate]` and capping the span at `MaxSelectionCount` are all
    /// the replica's, not this method's.
    pub fn click_day(&mut self, date: Date) {
        if !self.is_selectable(date) {
            return;
        }
        if self.inner.max_selection_count > 1 && self.extending {
            self.inner.set_selection_end(date);
            self.extending = false;
        } else {
            self.inner.set_selection_start(date);
            self.extending = self.inner.max_selection_count > 1;
        }
        self.view_month = Some(date.first_of_month());
    }

    // ── Geometry ─────────────────────────────────────────────────────────────

    /// How many columns the grid has: seven days, plus the week-number gutter
    /// when `ShowWeekNumbers` asks for one.
    ///
    /// The gutter is **one more column of the same grid**. The web has no week
    /// numbers to copy — this is therefore a Kubuno decision, and it is written
    /// as one: sizing the gutter off the day column is what keeps the grid
    /// regular instead of inventing a width for it.
    pub fn columns(&self) -> usize {
        cal::COLUMNS + usize::from(self.inner.show_week_numbers)
    }

    /// `bounds` less the panel's `p-3`.
    fn content(bounds: Rect) -> Rect {
        bounds.inflate(-cal::PAD, -cal::PAD)
    }

    /// One column's width inside `bounds` — the `grid-cols-7` track, or one
    /// eighth of it when a week gutter is shown.
    pub fn column_width(&self, bounds: Rect) -> f32 {
        let inner = Self::content(bounds);
        (inner.right - inner.left).max(0.0) / self.columns() as f32
    }

    /// The navigation header's row.
    pub fn header_rect(&self, bounds: Rect) -> Rect {
        let inner = Self::content(bounds);
        Rect::new(inner.left, inner.top, inner.right, inner.top + cal::HEADER)
    }

    /// The `w-7 h-7` previous-month button.
    pub fn prev_month_rect(&self, bounds: Rect) -> Rect {
        let h = self.header_rect(bounds);
        Rect::new(h.left, h.top, h.left + cal::NAV, h.top + cal::NAV)
    }

    /// The `w-7 h-7` next-month button.
    pub fn next_month_rect(&self, bounds: Rect) -> Rect {
        let h = self.header_rect(bounds);
        Rect::new(h.right - cal::NAV, h.top, h.right, h.top + cal::NAV)
    }

    /// The caption between the two nav buttons — `flex-1` in the web's header.
    pub fn title_rect(&self, bounds: Rect) -> Rect {
        let h = self.header_rect(bounds);
        Rect::new(h.left + cal::NAV, h.top, h.right - cal::NAV, h.bottom)
    }

    /// Which header button `(x, y)` lands on. The two nav buttons are round
    /// (`rounded-full`), so their corners do not answer — the same rule
    /// [`crate::buttons::circular_hit`] states for every circular control.
    pub fn header_at(&self, bounds: Rect, x: f32, y: f32) -> Option<HeaderPart> {
        if crate::buttons::circular_hit(self.prev_month_rect(bounds), x, y) {
            return Some(HeaderPart::Prev);
        }
        if crate::buttons::circular_hit(self.next_month_rect(bounds), x, y) {
            return Some(HeaderPart::Next);
        }
        self.title_rect(bounds).contains(x, y).then_some(HeaderPart::Title)
    }

    /// The weekday strip.
    pub fn weekday_rect(&self, bounds: Rect) -> Rect {
        let h = self.header_rect(bounds);
        let top = h.bottom + cal::HEADER_GAP;
        Rect::new(Self::content(bounds).left, top, Self::content(bounds).right, top + cal::WEEKDAY)
    }

    /// The seven day columns — the week-number gutter, when there is one, sits
    /// to their left and is NOT part of this rectangle.
    pub fn grid_rect(&self, bounds: Rect) -> Rect {
        let inner = Self::content(bounds);
        let w = self.column_width(bounds);
        let left = inner.left + w * (self.columns() - cal::COLUMNS) as f32;
        let top = self.weekday_rect(bounds).bottom + cal::WEEKDAY_GAP;
        Rect::new(left, top, left + w * cal::COLUMNS as f32, top + cal::ROWS as f32 * cal::DAY)
    }

    /// The week-number gutter, when `ShowWeekNumbers` is on.
    pub fn week_column(&self, bounds: Rect) -> Option<Rect> {
        if !self.inner.show_week_numbers {
            return None;
        }
        let grid = self.grid_rect(bounds);
        let inner = Self::content(bounds);
        Some(Rect::new(inner.left, grid.top, grid.left, grid.bottom))
    }

    /// The full track of cell `index` (0..42) — a column wide, [`cal::DAY`]
    /// tall.
    pub fn cell_rect(&self, bounds: Rect, index: usize) -> Rect {
        let grid = self.grid_rect(bounds);
        let w = self.column_width(bounds);
        let (row, col) = (index / cal::COLUMNS, index % cal::COLUMNS);
        let x = grid.left + col as f32 * w;
        let y = grid.top + row as f32 * cal::DAY;
        Rect::new(x, y, x + w, y + cal::DAY)
    }

    /// The `h-8 w-8` disc inside that track (`mx-auto`) — what is actually
    /// filled, ringed and clicked.
    pub fn day_rect(&self, bounds: Rect, index: usize) -> Rect {
        let cell = self.cell_rect(bounds, index);
        let cx = (cell.left + cell.right) / 2.0;
        Rect::new(cx - cal::DAY / 2.0, cell.top, cx + cal::DAY / 2.0, cell.bottom)
    }

    /// Which cell of the grid `(x, y)` lands on.
    pub fn cell_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        let grid = self.grid_rect(bounds);
        if !grid.contains(x, y) {
            return None;
        }
        let w = self.column_width(bounds);
        if w <= 0.0 {
            return None;
        }
        let col = ((x - grid.left) / w).floor() as usize;
        let row = ((y - grid.top) / cal::DAY).floor() as usize;
        (col < cal::COLUMNS && row < cal::ROWS).then(|| row * cal::COLUMNS + col)
    }

    /// Which **date** `(x, y)` lands on, trailing days of the neighbouring
    /// months included — there is no empty cell in this grid, because
    /// [`month_grid`] always fills the leading week.
    ///
    /// Pure geometry: a day outside `[MinDate, MaxDate]` still answers here, and
    /// [`MonthCalendar::is_selectable`] is what decides whether it may be
    /// picked. Keeping the two apart is what lets a caller light a day it is
    /// about to refuse.
    pub fn day_at(&self, bounds: Rect, x: f32, y: f32) -> Option<Date> {
        self.cell_at(bounds, x, y).map(|i| self.cells()[i].date)
    }

    /// The today footer's row, when `ShowToday` asks for one.
    pub fn footer_rect(&self, bounds: Rect) -> Option<Rect> {
        if !self.inner.show_today {
            return None;
        }
        let inner = Self::content(bounds);
        let top = self.grid_rect(bounds).bottom + cal::FOOTER_RISE + cal::FOOTER_RULE
            + cal::FOOTER_PAD;
        Some(Rect::new(inner.left, top, inner.right, top + cal::FOOTER_ROW))
    }

    /// The panel's size, with no canvas — the grid is fixed type-free geometry,
    /// so this is exact and a caller can lay a popup out before it paints.
    ///
    /// One month only: see the module's stated gaps for `CalendarDimensions`.
    pub fn panel_size(&self) -> Size {
        let width = 2.0 * cal::PAD + self.columns() as f32 * cal::DAY_COLUMN;
        let height = 2.0 * cal::PAD
            + cal::HEADER
            + cal::HEADER_GAP
            + cal::WEEKDAY
            + cal::WEEKDAY_GAP
            + cal::ROWS as f32 * cal::DAY
            + if self.inner.show_today { cal::FOOTER } else { 0.0 };
        Size::new(width, height)
    }

    // ── Paint ────────────────────────────────────────────────────────────────

    /// The navigation header: two round buttons and the month/year caption.
    fn paint_header(&self, c: &dyn Canvas, bounds: Rect, dead: bool) {
        let t = c.theme();
        let title_ink = or_theme(self.inner.title_fore_color, t.text_primary);
        let ink = if dead { faded(&title_ink, DISABLED_FIELD_ALPHA) } else { title_ink };

        // `TitleBackColor` is a modelled property with no web counterpart — the
        // web's header sits on the panel's own ground. Painted only when the
        // caller set it, which is exactly what "ambient when None" means.
        if let Some(back) = self.inner.title_back_color {
            c.fill_rounded(&self.header_rect(bounds), 0.0, &back);
        }

        // `w-7 h-7 … rounded-full hover:bg-surface-2 text-text-secondary`.
        let arrow = if dead { faded(&t.text_secondary, DISABLED_FIELD_ALPHA) } else { t.text_secondary };
        for (part, rect, glyph) in [
            (HeaderPart::Prev, self.prev_month_rect(bounds), "ChevronLeft"),
            (HeaderPart::Next, self.next_month_rect(bounds), "ChevronRight"),
        ] {
            if self.hot_header == Some(part) && !dead {
                c.fill_rounded(&rect, pill(cal::NAV), &t.surface_2);
            }
            c.vector_icon(glyph, &rect, cal::NAV_GLYPH, &arrow);
        }

        // `text-sm font-semibold text-text-primary hover:text-primary px-1
        // rounded hover:bg-surface-1` — one caption where the web has two
        // adjacent buttons (see [`HeaderPart::Title`]); on hover the pair
        // lights as one.
        let (year, month) = self.shown_month();
        let caption = month_caption(year, month);
        let f = &c.formats().body_strong;
        let slot = self.title_rect(bounds);
        let hot = self.hot_header == Some(HeaderPart::Title) && !dead;
        let ink = if hot {
            let w = (c.measure(&caption, f) + 2.0 * cal::TITLE_PAD_X).min(slot.right - slot.left);
            let (cx, cy) = ((slot.left + slot.right) / 2.0, (slot.top + slot.bottom) / 2.0);
            let pill_rect =
                Rect::new(cx - w / 2.0, cy - cal::TITLE_LINE / 2.0, cx + w / 2.0, cy + cal::TITLE_LINE / 2.0);
            c.fill_rounded(&pill_rect, radius::SM, &t.card_background);
            t.accent
        } else {
            ink
        };
        // Clipped to its slot: a caption wider than the space between the two
        // arrows (a calendar laid out narrower than its `panel_size`) is cut
        // there rather than painted over them.
        c.push_clip(&slot);
        c.text(&caption, &slot, f, &ink, true);
        c.pop_clip();
    }

    /// The weekday strip, rotated so column 0 is `FirstDayOfWeek`.
    fn paint_weekdays(&self, c: &dyn Canvas, bounds: Rect, dead: bool) {
        let t = c.theme();
        // `text-[11px] font-medium text-text-tertiary`. There is no 11 DIP
        // format object in `TextFormats` (the token exists, the format does
        // not), so this is the meta step at the same weight — one size up, and
        // said so rather than left to be read off the pixels.
        let f = &c.formats().caption_strong;
        let ink = if dead { faded(&t.text_tertiary, DISABLED_FIELD_ALPHA) } else { t.text_tertiary };
        let row = self.weekday_rect(bounds);
        let grid = self.grid_rect(bounds);
        let w = self.column_width(bounds);
        let first = self.inner.first_day();
        for col in 0..cal::COLUMNS {
            let idx = (first as usize + col) % 7;
            let x = grid.left + col as f32 * w;
            c.text(WEEKDAYS[idx], &Rect::new(x, row.top, x + w, row.bottom), f, &ink, true);
        }
    }

    /// One day cell, in the state ladder `DayView` writes its class list in.
    fn paint_day(&self, c: &dyn Canvas, bounds: Rect, index: usize, cell: Cell, dead: bool, cursor: Option<Date>) {
        let t = c.theme();
        let disc = self.day_rect(bounds, index);
        let date = cell.date;
        let (in_range, edge) = self.selection_of(date);
        let live = self.is_selectable(date) && !dead;
        let today = self.inner.show_today_circle && date == self.inner.today_date();
        let hot = self.hot_day == Some(date) && live;

        let fore = or_theme(self.inner.control().fore_color, t.text_primary);
        let trailing = or_theme(self.inner.trailing_fore_color, t.text_tertiary);

        // `(sel || edge) ? 'rounded-full bg-primary text-white'` first, then the
        // in-range square, then today's ring, then the plain day.
        let ink = if edge && in_range {
            c.fill_rounded(&disc, pill(cal::DAY), &t.accent);
            t.accent_foreground
        } else if in_range {
            // `bg-primary/10 text-primary` — a SQUARE, deliberately: it is what
            // makes a range read as continuous between its two round edges.
            c.fill_rounded(&disc, 0.0, &faded(&t.accent, cal::RANGE_ALPHA));
            t.accent
        } else if today {
            // `rounded-full border border-primary text-primary
            // hover:bg-primary-light`.
            if hot {
                c.fill_rounded(&disc, pill(cal::DAY), &t.accent_light);
            }
            c.stroke_rounded(&disc, pill(cal::DAY), &t.accent);
            t.accent
        } else {
            if hot {
                c.fill_rounded(&disc, pill(cal::DAY), &t.surface_2);
            }
            if cell.in_month {
                fore
            } else {
                trailing
            }
        };

        // `opacity-30 cursor-not-allowed` — applied over whatever the ladder
        // chose, exactly as the web's last class does.
        let alpha = if live { 1.0 } else { cal::DISABLED_DAY_ALPHA };
        let (plain, bold) = day_formats(c, self.inner.control().font.unwrap_or(FontRole::Caption));
        let f = if self.inner.is_bold(date) { bold } else { plain };
        c.text(&date.day.to_string(), &disc, f, &faded(&ink, alpha), true);

        // The keyboard cursor's `focus-visible` ring, drawn just outside the
        // disc so it reads on a filled (selected) day as well as a plain one.
        if cursor == Some(date) && !dead {
            let ring = disc.inflate(cal::DAY_FOCUS_RING / 2.0, cal::DAY_FOCUS_RING / 2.0);
            let r = ring.right - ring.left;
            c.stroke_rounded_w(&ring, pill(r), &t.accent, cal::DAY_FOCUS_RING);
        }
    }

    /// The week-number gutter. A row made ENTIRELY of trailing days carries no
    /// number — the replica's own rule, for the same reason: the leading week
    /// its grid always shows would otherwise label a week of the month before.
    fn paint_week_numbers(&self, c: &dyn Canvas, bounds: Rect, cells: &[Cell; 42], dead: bool) {
        let Some(gutter) = self.week_column(bounds) else { return };
        let t = c.theme();
        let ink = if dead { faded(&t.text_tertiary, DISABLED_FIELD_ALPHA) } else { t.text_tertiary };
        for row in 0..cal::ROWS {
            let week = &cells[row * cal::COLUMNS..row * cal::COLUMNS + cal::COLUMNS];
            if !week.iter().any(|cell| cell.in_month) {
                continue;
            }
            let top = gutter.top + row as f32 * cal::DAY;
            let r = Rect::new(gutter.left, top, gutter.right, top + cal::DAY);
            c.text(&iso_week(week[0].date).to_string(), &r, &c.formats().caption, &ink, true);
        }
    }

    /// The today footer.
    ///
    /// The replica draws a `Color.Red`-ish swatch here because the native
    /// control does; this layer draws the web's own footer instead — a rule and
    /// a `text-xs` row — because the design system has no swatch and the grid
    /// already marks today with an accent ring. The label is composed the way
    /// the replica composes it, from [`format_custom`] and `FR.short_date`.
    fn paint_footer(&self, c: &dyn Canvas, bounds: Rect, dead: bool) {
        let Some(row) = self.footer_rect(bounds) else { return };
        let t = c.theme();
        let rule_y = row.top - cal::FOOTER_PAD - cal::FOOTER_RULE;
        c.fill_rounded(
            &Rect::new(row.left, rule_y, row.right, rule_y + cal::FOOTER_RULE),
            0.0,
            &t.card_stroke,
        );
        let ink = if dead { faded(&t.text_secondary, DISABLED_FIELD_ALPHA) } else { t.text_secondary };
        let label = format!(
            "Aujourd'hui : {}",
            format_custom(DateTime::at_midnight(self.inner.today_date()), FR.short_date, &FR)
        );
        c.text(&label, &row, &c.formats().caption, &ink, false);
    }
}

impl Widget for MonthCalendar {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, _canvas: &dyn Canvas) -> Size {
        self.panel_size()
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        // Rule: every widget lands on an opaque background. What follows
        // may overpaint most of it (a fill, a card, a track); this makes
        // sure the parts that stay uncovered still read as an opaque
        // surface, so a widget never shows what is behind it.
        canvas.fill_rounded(&bounds, 0.0, &canvas.current_bg());
        let t = canvas.theme();
        let dead = state.disabled || !self.inner.control().enabled;

        // `PickerPopover`: `bg-white rounded-xl border border-border`. The
        // surface is `CalendarMonthBackground` when the caller set it —
        // `MonthCalendar` re-surfaces `BackColor` for exactly that.
        let face = or_theme(
            self.inner.control().back_color,
            if dead { t.surface_2 } else { t.layer_background },
        );
        canvas.fill_rounded(&bounds, radius::XL, &face);
        canvas.stroke_rounded(&bounds, radius::XL, &t.card_stroke);

        canvas.push_clip(&bounds);
        self.paint_header(canvas, bounds, dead);
        self.paint_weekdays(canvas, bounds, dead);
        let cells = self.cells();
        self.paint_week_numbers(canvas, bounds, &cells, dead);
        // The ring follows `:focus-visible`: shown when the grid holds a
        // keyboard-visible focus, on the day the arrows act on.
        let cursor = state.show_focus_ring().then(|| self.active_day());
        for (i, cell) in cells.iter().enumerate() {
            self.paint_day(canvas, bounds, i, *cell, dead, cursor);
        }
        self.paint_footer(canvas, bounds, dead);
        canvas.pop_clip();
    }

    fn type_name(&self) -> &'static str {
        "MonthCalendar"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// DatePicker
// ═════════════════════════════════════════════════════════════════════════════

/// Which part of the picker's field a point lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldPart {
    /// The `ShowCheckBox` well — the toolkit's « no value is selected » toggle.
    CheckBox,
    /// The rest of the field. A click here opens the panel, unless
    /// `ShowUpDown` replaced it with a spinner.
    Value,
    /// `ShowUpDown`'s upper button.
    SpinUp,
    /// `ShowUpDown`'s lower button.
    SpinDown,
}

/// One editable part of the field's text — the toolkit's « fields » of a
/// `DateTimePicker`, which the user selects with ←/→ and edits with ↑/↓ or by
/// typing digits. Which ones exist, and in which order, is read off the
/// format pattern (`dd/MM/yyyy` has three, `HH:mm:ss` three, `dddd d MMMM
/// yyyy` three — the weekday name is derived, not edited).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    Day,
    Month,
    Year,
    /// `H` / `HH` — 0 to 23.
    Hour,
    /// `h` / `hh` — 1 to 12, the AM/PM half kept.
    Hour12,
    Minute,
    Second,
    /// `t` / `tt`.
    AmPm,
}

/// Where a [`Segment`] sits in the field's text: a byte range of
/// [`DatePicker::field_text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentSpan {
    pub segment: Segment,
    pub start: usize,
    pub end: usize,
    /// How many letters the pattern run has (`yy` = 2, `yyyy` = 4): what a
    /// typed year must reach before it is complete.
    pub run: usize,
}

/// A piece of a format pattern: literal text, or a run of one specifier.
enum Token {
    Literal(String),
    Run(char, usize),
}

/// Splits `pattern` exactly the way `format_custom` walks it — quoted text and
/// `\`-escapes are literal, a run of one specifier letter is one token — so
/// formatting the runs one by one and concatenating gives the same string as
/// formatting the whole pattern. The grammar is the replica's; this only
/// keeps the boundaries it throws away.
fn tokenize(pattern: &str) -> Vec<Token> {
    const SPECIFIERS: &[char] = &['d', 'M', 'y', 'H', 'h', 'm', 's', 't', 'f', 'F', 'g', 'K', 'z'];
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\'' || ch == '"' {
            i += 1;
            while i < chars.len() && chars[i] != ch {
                lit.push(chars[i]);
                i += 1;
            }
            i += 1;
        } else if ch == '\\' {
            if let Some(next) = chars.get(i + 1) {
                lit.push(*next);
            }
            i += 2;
        } else if SPECIFIERS.contains(&ch) {
            let mut n = 1;
            while chars.get(i + n) == Some(&ch) {
                n += 1;
            }
            if !lit.is_empty() {
                out.push(Token::Literal(std::mem::take(&mut lit)));
            }
            out.push(Token::Run(ch, n));
            i += n;
        } else {
            lit.push(ch);
            i += 1;
        }
    }
    if !lit.is_empty() {
        out.push(Token::Literal(lit));
    }
    out
}

/// Which segment a specifier run edits, if any. `ddd` / `dddd` (a weekday
/// name) and the unsupported specifiers are text, not fields.
fn run_segment(ch: char, n: usize) -> Option<Segment> {
    match ch {
        'd' if n <= 2 => Some(Segment::Day),
        'M' => Some(Segment::Month),
        'y' => Some(Segment::Year),
        'H' => Some(Segment::Hour),
        'h' => Some(Segment::Hour12),
        'm' => Some(Segment::Minute),
        's' => Some(Segment::Second),
        't' => Some(Segment::AmPm),
        _ => None,
    }
}

/// The segment's current value, as an integer (AM/PM: 0 / 1).
fn segment_value(v: DateTime, seg: Segment) -> i32 {
    match seg {
        Segment::Day => v.date.day as i32,
        Segment::Month => v.date.month as i32,
        Segment::Year => v.date.year,
        Segment::Hour => v.time.hour as i32,
        Segment::Hour12 => match v.time.hour % 12 {
            0 => 12,
            h => h as i32,
        },
        Segment::Minute => v.time.minute as i32,
        Segment::Second => v.time.second as i32,
        Segment::AmPm => i32::from(v.time.hour >= 12),
    }
}

/// The inclusive range a segment takes — the day's depends on the month.
fn segment_range(v: DateTime, seg: Segment) -> (i32, i32) {
    match seg {
        Segment::Day => (1, Date::days_in_month(v.date.year, v.date.month) as i32),
        Segment::Month => (1, 12),
        Segment::Year => (replica::MIN_DATE.year, replica::MAX_DATE.year),
        Segment::Hour => (0, 23),
        Segment::Hour12 => (1, 12),
        Segment::Minute | Segment::Second => (0, 59),
        Segment::AmPm => (0, 1),
    }
}

/// `v` with one segment replaced by `value` (clamped into its range). A month
/// or year change clamps the day into the new month, as the toolkit does
/// (31 → février ⇒ 28/29); nothing carries into the neighbouring field.
fn with_segment(v: DateTime, seg: Segment, value: i32) -> DateTime {
    let (lo, hi) = segment_range(v, seg);
    let x = value.clamp(lo, hi);
    let (mut d, mut t) = (v.date, v.time);
    match seg {
        Segment::Day => d.day = x as u8,
        Segment::Month => {
            d.month = x as u8;
            d.day = d.day.min(Date::days_in_month(d.year, d.month));
        }
        Segment::Year => {
            d.year = x;
            d.day = d.day.min(Date::days_in_month(d.year, d.month));
        }
        Segment::Hour => t.hour = x as u8,
        Segment::Hour12 => t.hour = (x as u8 % 12) + if t.hour >= 12 { 12 } else { 0 },
        Segment::Minute => t.minute = x as u8,
        Segment::Second => t.second = x as u8,
        Segment::AmPm => t.hour = t.hour % 12 + if x == 1 { 12 } else { 0 },
    }
    DateTime::new(d, t)
}

/// `v` with one segment moved by `delta`: the toolkit's up/down, which WRAPS
/// inside the segment (59 → 00, 31 → 1) without carrying into the next one —
/// except the year, which has no wrap and stops at the bounds.
fn step_segment(v: DateTime, seg: Segment, delta: i32) -> DateTime {
    let (lo, hi) = segment_range(v, seg);
    let cur = segment_value(v, seg);
    let next = if seg == Segment::Year {
        cur.saturating_add(delta)
    } else {
        lo + (cur - lo + delta).rem_euclid(hi - lo + 1)
    };
    with_segment(v, seg, next)
}

/// A date (or time) field with a drop-down calendar.
///
/// The model is [`kubuno_controls::datetime::DateTimePicker`], reached through
/// [`Deref`]: `Value` and its clamp, `MinDate` / `MaxDate`, `Format` /
/// `CustomFormat` and the whole fr-FR format engine behind `display_text`,
/// `ShowUpDown`, `ShowCheckBox` / `Checked`, `DropDownAlign`,
/// `RightToLeftLayout` and the six `Calendar*` appearance properties. None of
/// them is restated here.
///
/// ## Where every number comes from
///
/// `DatePicker.tsx`'s trigger, in full: `w-full flex items-center gap-2 px-3
/// rounded border bg-white text-left` at `h-9`, with a leading `size={14}`
/// glyph — an `@ui/Input`, so [`height::BUTTON_MD`], [`radius::SM`] and
/// [`space::MD`], plus `focus:ring-2 focus:ring-primary` and
/// `disabled:opacity-60`. The panel is [`MonthCalendar`], anchored at
/// `r.bottom + 4` the way `computePos` anchors it.
///
/// ## The panel's calendar
///
/// [`DatePicker::calendar`] is a real [`MonthCalendar`], and
/// [`DatePicker::open_panel`] is the one place the picker's `Value`, bounds and
/// `Calendar*` colours are projected onto it — the same single direction, at the
/// same moment, as the web's `openPicker()`, which seeds `viewDate` from the
/// value when the popover opens. Picking a day writes back through
/// [`DatePicker::pick`], so the value has one home.
#[derive(Clone)]
pub struct DatePicker {
    inner: replica::DateTimePicker,
    /// Whether the panel is dropped — `DateTimePicker.DroppedDown`, which the
    /// replica does not model (it is runtime state, not a designer property).
    pub open: bool,
    /// The dropped panel.
    pub calendar: MonthCalendar,
    /// The viewport the panel places itself in — the monitor's work area
    /// (`Frame::screen_area`) when the panel is shown in a `host::popup`.
    /// `Some` turns on `computePos`' flip (above the field when there is not
    /// room below) and its horizontal clamp; `None` keeps the plain « under
    /// the field » anchor.
    pub viewport: Option<Rect>,
    /// The web's `error` prop: `border-danger`, and a danger focus ring
    /// (`kb-field-focus-danger`).
    pub invalid: bool,
    /// The segment the keyboard edits — the toolkit's selected field. `None`
    /// until the field is clicked or a key selects one.
    pub active_segment: Option<Segment>,
    /// The field part under the pointer — lights a spin button
    /// (`hover:bg-surface-2`). A replica has no pointer, hence a field here.
    pub hot_part: Option<FieldPart>,
    /// Digits typed into the active segment and not yet committed (a year is
    /// only complete at four).
    typed: String,
}

impl Default for DatePicker {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for DatePicker {
    type Target = replica::DateTimePicker;
    fn deref(&self) -> &replica::DateTimePicker {
        &self.inner
    }
}
impl DerefMut for DatePicker {
    fn deref_mut(&mut self) -> &mut replica::DateTimePicker {
        &mut self.inner
    }
}

/// A time field.
///
/// It is the SAME control: `DateTimePickerFormat::Time` is a property of
/// `DateTimePicker`, not a second type — so this is an alias, exactly as
/// [`crate::buttons::ToggleButton`] is one for a check box in button
/// appearance. Build one with [`DatePicker::time`].
///
/// What it deliberately does **not** get is the web's time popover: those
/// scrolling hour/minute columns (`TimeScroll.tsx`) are a control the replica
/// layer does not model at all, and building one here would be inventing a
/// primitive rather than porting one. The toolkit's own answer to « edit a time
/// in a picker » is `ShowUpDown`, which IS modelled and IS painted — hence
/// [`DatePicker::time`] turning it on.
pub type TimePicker = DatePicker;

impl DatePicker {
    /// A picker with the replica's defaults — `Format = Long`, so it shows
    /// « lundi 15 juin 2026 ».
    pub fn new() -> Self {
        let mut picker = Self {
            inner: replica::DateTimePicker::new(),
            open: false,
            calendar: MonthCalendar::new(),
            viewport: None,
            invalid: false,
            active_segment: None,
            hot_part: None,
            typed: String::new(),
        };
        // A date picker picks ONE date: the calendar's `MaxSelectionCount`
        // default is the toolkit's 7, which is right for a standalone calendar
        // and wrong for a field holding a single `Value`.
        picker.calendar.max_selection_count = 1;
        // The web's picker panel has no today row (`PickerPopover` shows a
        // footer only in its time modes). A standalone `MonthCalendar` keeps
        // the toolkit's `ShowToday = true`; this one does not.
        picker.calendar.show_today = false;
        picker.sync_calendar();
        picker
    }

    /// The `dd/MM/yyyy` field — `ShortDatePattern`, the shape the web's `date`
    /// mode shows and its `jj/mm/aaaa` placeholder describes.
    pub fn short() -> Self {
        let mut p = Self::new();
        p.inner.format = DateTimePickerFormat::Short;
        p
    }

    /// A time field: `LongTimePattern` and the toolkit's spinner — see
    /// [`TimePicker`].
    pub fn time() -> Self {
        let mut p = Self::new();
        p.inner.format = DateTimePickerFormat::Time;
        p.inner.show_up_down = true;
        p
    }

    /// Sets `Value` to midnight on `date`, through the replica's clamp.
    pub fn on(mut self, date: Date) -> Self {
        self.inner.set_value(DateTime::at_midnight(date));
        self.sync_calendar();
        self
    }

    /// Sets `MinDate` / `MaxDate`, through the replica's own bounds rules —
    /// which also drag `Value` into the new window.
    pub fn between(mut self, min: Date, max: Date) -> Self {
        self.inner.set_min_date(min);
        self.inner.set_max_date(max);
        self.sync_calendar();
        self
    }

    /// The text the field shows — the replica's, formatted against [`FR`].
    pub fn display_text(&self) -> String {
        self.inner.display_text(&FR)
    }

    /// The same, against another culture's [`Names`]. Kept so a future culture
    /// service reaches the paint without this layer holding a second table.
    pub fn display_text_in(&self, names: &Names) -> String {
        self.inner.display_text(names)
    }

    /// The leading glyph: `mode === 'time' ? <Clock/> : <Calendar/>`.
    pub fn leading_icon(&self) -> &'static str {
        if self.inner.format == DateTimePickerFormat::Time {
            "Clock"
        } else {
            "Calendar"
        }
    }

    /// Whether the value reads as set. `ShowCheckBox` with `Checked = false` is
    /// the toolkit's « no value is selected », and it greys the text out.
    pub fn value_shown(&self) -> bool {
        !self.inner.show_check_box || self.inner.checked
    }

    /// Whether a calendar can drop: `ShowUpDown` replaces it with a spinner,
    /// which is the toolkit's own rule.
    pub fn has_panel(&self) -> bool {
        !self.inner.show_up_down
    }

    /// Projects the picker's value, bounds and `Calendar*` appearance onto the
    /// panel — the one direction of the sync, run when the panel opens.
    pub fn sync_calendar(&mut self) {
        let date = self.inner.value().date;
        self.calendar.set_min_date(self.inner.min_date());
        self.calendar.set_max_date(self.inner.max_date());
        self.calendar.set_selection_start(date);
        self.calendar.view_month = Some(date.first_of_month());
        self.calendar.extending = false;
        self.calendar.focus_day = None;
        // The six `Calendar*` properties ARE the panel's appearance: honoured by
        // projection rather than by a second set of fields.
        self.calendar.control_mut().font = self.inner.calendar_font;
        self.calendar.control_mut().fore_color = self.inner.calendar_fore_color;
        self.calendar.control_mut().back_color = self.inner.calendar_month_background;
        self.calendar.title_back_color = self.inner.calendar_title_back_color;
        self.calendar.title_fore_color = self.inner.calendar_title_fore_color;
        self.calendar.trailing_fore_color = self.inner.calendar_trailing_fore_color;
    }

    /// Drops the panel, seeding it from the value — the web's `openPicker()`.
    pub fn open_panel(&mut self) {
        if !self.has_panel() {
            return;
        }
        self.sync_calendar();
        self.open = true;
    }

    pub fn close_panel(&mut self) {
        self.open = false;
        self.calendar.hot_day = None;
        self.calendar.hot_header = None;
    }

    /// Toggles the panel, the way a click on the field does.
    pub fn toggle_panel(&mut self) {
        if self.open {
            self.close_panel();
        } else {
            self.open_panel();
        }
    }

    /// Picks `date` from the panel: the value is written through the replica's
    /// clamp, the panel follows, and the popover closes — `handleSelectDate`
    /// for the web's `date` mode.
    pub fn pick(&mut self, date: Date) {
        if !self.calendar.is_selectable(date) {
            return;
        }
        let time = self.inner.value().time;
        self.inner.set_value(DateTime::new(date, time));
        self.calendar.click_day(date);
        self.close_panel();
    }

    /// Drops the panel from the keyboard: as [`DatePicker::open_panel`], with
    /// the calendar's keyboard cursor put on the value — the ARIA pattern's
    /// « focus moves to the selected date » — so its ring shows at once.
    pub fn open_panel_from_keyboard(&mut self) {
        self.open_panel();
        if self.open {
            let d = self.inner.value().date;
            self.calendar.move_focus_to(d);
        }
    }

    // ── Segment editing ──────────────────────────────────────────────────────

    /// The field's text and where each editable segment sits in it. The text
    /// is [`DatePicker::display_text`], except that digits typed into the
    /// active segment and not yet committed replace that segment's value — so
    /// a year being typed reads « 20 » rather than jumping to year 20.
    pub fn field_text(&self) -> (String, Vec<SegmentSpan>) {
        let value = self.inner.value();
        let pattern = self.inner.effective_pattern(&FR);
        let mut text = String::new();
        let mut spans = Vec::new();
        let mut substituted = false;
        for token in tokenize(pattern) {
            match token {
                Token::Literal(s) => text.push_str(&s),
                Token::Run(ch, n) => {
                    let start = text.len();
                    let seg = run_segment(ch, n);
                    let pending = !self.typed.is_empty() && seg.is_some() && seg == self.active_segment;
                    if pending && !substituted {
                        text.push_str(&self.typed);
                        substituted = true;
                    } else {
                        text.push_str(&format_custom(value, &ch.to_string().repeat(n), &FR));
                    }
                    if let Some(segment) = seg {
                        spans.push(SegmentSpan { segment, start, end: text.len(), run: n });
                    }
                }
            }
        }
        (text, spans)
    }

    /// The editable segments, in reading order, each once.
    pub fn segments(&self) -> Vec<Segment> {
        let mut out: Vec<Segment> = Vec::new();
        for span in self.field_text().1 {
            if !out.contains(&span.segment) {
                out.push(span.segment);
            }
        }
        out
    }

    /// The digits typed into the active segment and not committed yet.
    pub fn pending_digits(&self) -> &str {
        &self.typed
    }

    /// Whether the field can be edited at all: `ShowCheckBox` unchecked is
    /// « no value », and the toolkit greys its fields out until it is checked.
    fn editable(&self) -> bool {
        self.value_shown() && self.inner.control().enabled
    }

    /// Selects `seg` for editing, committing whatever was typed in the one
    /// before.
    pub fn select_segment(&mut self, seg: Segment) {
        self.commit_typed();
        self.active_segment = Some(seg);
    }

    /// Moves the selection one segment left (`-1`) or right (`+1`), stopping
    /// at the ends — ←/→ in a `DateTimePicker`.
    pub fn move_segment(&mut self, delta: i32) {
        self.commit_typed();
        let segs = self.segments();
        if segs.is_empty() {
            return;
        }
        let i = match self.active_segment.and_then(|s| segs.iter().position(|x| *x == s)) {
            Some(i) => (i as i32 + delta).clamp(0, segs.len() as i32 - 1) as usize,
            None if delta < 0 => segs.len() - 1,
            None => 0,
        };
        self.active_segment = Some(segs[i]);
    }

    /// The segment ↑/↓ and the spin buttons act on: the selected one, else
    /// the first.
    fn target_segment(&self) -> Option<Segment> {
        self.active_segment.or_else(|| self.segments().first().copied())
    }

    /// ↑ (`+1`) / ↓ (`-1`), or a spin button: the target segment steps, wrapping
    /// inside its own range, and the result goes through the replica's
    /// `[MinDate, MaxDate]` clamp.
    pub fn step(&mut self, delta: i32) {
        if !self.editable() {
            return;
        }
        self.commit_typed();
        let Some(seg) = self.target_segment() else { return };
        self.active_segment = Some(seg);
        let v = step_segment(self.inner.value(), seg, delta);
        self.inner.set_value(v);
    }

    /// One typed character. Digits fill the active segment the way a native
    /// date field does: the segment commits — and the selection moves on —
    /// as soon as no further digit could keep it in range (« 4 » in a day is
    /// complete, « 1 » waits for a second digit), or when it holds its full
    /// width. `a` / `p` set the AM/PM segment. Returns whether it was taken.
    pub fn type_char(&mut self, ch: char) -> bool {
        if !self.editable() {
            return false;
        }
        let Some(seg) = self.target_segment() else { return false };
        self.active_segment = Some(seg);
        if seg == Segment::AmPm {
            let lower = ch.to_lowercase().next().unwrap_or(ch);
            let first = |s: &str| s.chars().next().and_then(|c| c.to_lowercase().next());
            let value = if first(FR.am) == Some(lower) {
                0
            } else if first(FR.pm) == Some(lower) {
                1
            } else {
                return false;
            };
            let v = with_segment(self.inner.value(), seg, value);
            self.inner.set_value(v);
            self.move_segment(1);
            return true;
        }
        let Some(digit) = ch.to_digit(10) else { return false };
        self.typed.push(char::from(b'0' + digit as u8));
        let run = self.span_of(seg).map_or(2, |s| s.run);
        let width = if seg == Segment::Year { if run <= 2 { 2 } else { 4 } } else { 2 };
        let typed: i32 = self.typed.parse().unwrap_or(0);
        let (_, hi) = segment_range(self.inner.value(), seg);
        let full = self.typed.len() >= width;
        let no_room = seg != Segment::Year && typed.saturating_mul(10) > hi;
        if full || no_room {
            self.commit_typed();
            self.move_segment(1);
        }
        true
    }

    /// Backspace: drops the last typed digit. Returns whether there was one.
    pub fn backspace(&mut self) -> bool {
        self.typed.pop().is_some()
    }

    /// Where `seg` sits in [`DatePicker::field_text`].
    fn span_of(&self, seg: Segment) -> Option<SegmentSpan> {
        self.field_text().1.into_iter().find(|s| s.segment == seg)
    }

    /// Writes the typed digits into the active segment (through the replica's
    /// clamp) and clears them. A one- or two-digit year keeps the current
    /// century (« 27 » in 2026 is 2027).
    pub fn commit_typed(&mut self) {
        if self.typed.is_empty() {
            return;
        }
        let typed = std::mem::take(&mut self.typed);
        let Some(seg) = self.active_segment else { return };
        let Ok(n) = typed.parse::<i32>() else { return };
        let v = self.inner.value();
        let value = if seg == Segment::Year && typed.len() <= 2 {
            v.date.year - v.date.year.rem_euclid(100) + n
        } else {
            n
        };
        self.inner.set_value(with_segment(v, seg, value));
    }

    /// The field lost the keyboard focus: typed digits are committed and the
    /// panel closes — the web's outside-`mousedown`, and Tab away.
    pub fn blur(&mut self) {
        self.commit_typed();
        self.close_panel();
    }

    /// Whether `(key, mods)` is one [`DatePicker::key`] acts on in the
    /// picker's current state — asked before the key is taken from the queue,
    /// so Tab, Ctrl+Tab and every shortcut the field does not own stay there.
    pub fn handles_key(&self, key: u16, mods: Modifiers) -> bool {
        let alt = mods.matches(Modifiers::ALT);
        if self.open && self.has_panel() {
            return MonthCalendar::handles_key(key, mods)
                || key == vk::ESCAPE
                || (key == vk::UP && alt)
                || (key == vk::F4 && mods.is_none());
        }
        let plain = mods.is_none();
        match key {
            vk::DOWN if alt => self.has_panel(),
            vk::LEFT | vk::RIGHT | vk::UP | vk::DOWN | vk::HOME | vk::END => plain,
            vk::ENTER => plain,
            vk::SPACE => plain && (self.inner.show_check_box || self.has_panel()),
            vk::BACK => plain && !self.typed.is_empty(),
            vk::ESCAPE => !self.typed.is_empty(),
            vk::F4 => plain && self.has_panel(),
            _ => false,
        }
    }

    /// One key, in the picker's current state. Returns whether it acted.
    ///
    /// **Panel open** — the calendar's keys ([`MonthCalendar::key`]); Enter /
    /// Space pick the cursor's day and close; Escape, Alt+↑ and F4 close.
    ///
    /// **Panel closed** — the toolkit's field editing: ←/→ select the
    /// previous / next segment, Home/End the first / last, ↑/↓ step the
    /// selected one, Backspace drops a typed digit, Escape drops them all.
    /// Enter commits what was typed, or — like the web trigger, a `<button>` —
    /// opens the panel; Space toggles `ShowCheckBox` when there is one, else
    /// opens; Alt+↓ and F4 open (the native control's shortcuts).
    pub fn key(&mut self, key: u16, mods: Modifiers) -> bool {
        if !self.handles_key(key, mods) {
            return false;
        }
        if self.open && self.has_panel() {
            match key {
                vk::ESCAPE | vk::UP | vk::F4 if !MonthCalendar::handles_key(key, mods) => self.close_panel(),
                _ => {
                    if let CalendarKey::Activated(d) = self.calendar.key(key, mods) {
                        self.pick(d);
                    }
                }
            }
            return true;
        }
        let alt = mods.matches(Modifiers::ALT);
        match key {
            vk::DOWN if alt => self.open_panel_from_keyboard(),
            vk::F4 => self.open_panel_from_keyboard(),
            vk::LEFT => self.move_segment(-1),
            vk::RIGHT => self.move_segment(1),
            vk::HOME => {
                self.commit_typed();
                self.active_segment = self.segments().first().copied();
            }
            vk::END => {
                self.commit_typed();
                self.active_segment = self.segments().last().copied();
            }
            vk::UP => self.step(1),
            vk::DOWN => self.step(-1),
            vk::BACK => {
                self.backspace();
            }
            vk::ESCAPE => self.typed.clear(),
            vk::ENTER => {
                if self.typed.is_empty() {
                    self.open_panel_from_keyboard();
                } else {
                    self.commit_typed();
                }
            }
            vk::SPACE => {
                if self.inner.show_check_box {
                    self.inner.checked = !self.inner.checked;
                } else {
                    self.open_panel_from_keyboard();
                }
            }
            _ => return false,
        }
        true
    }

    /// Typed text: each character through [`DatePicker::type_char`]. Returns
    /// whether any was taken.
    pub fn text(&mut self, s: &str) -> bool {
        let mut took = false;
        for ch in s.chars() {
            took |= self.type_char(ch);
        }
        took
    }

    /// Takes this frame's keys and typed text meant for the field from the
    /// host queue and applies them — call it while the field holds the focus.
    /// Returns whether anything was taken. Keys the picker does not own (Tab,
    /// shortcuts) stay in the queue.
    pub fn take_input(&mut self) -> bool {
        let editable = self.editable() && !self.open;
        let letters = self.segments().contains(&Segment::AmPm);
        let events = host::consume(|e| match e {
            InputEvent::Key { vk, down: true, mods, .. } => self.handles_key(*vk, *mods),
            InputEvent::Text(s) => {
                editable && s.chars().any(|c| c.is_ascii_digit() || (letters && c.is_alphabetic()))
            }
            _ => false,
        });
        let took = !events.is_empty();
        for e in events {
            match e {
                InputEvent::Key { vk, mods, .. } => {
                    self.key(vk, mods);
                }
                InputEvent::Text(s) => {
                    self.text(&s);
                }
                _ => {}
            }
        }
        took
    }

    // ── Geometry ─────────────────────────────────────────────────────────────

    /// The field's height — an `@ui/Input`'s `h-9`.
    pub fn field_height(&self) -> f32 {
        height::BUTTON_MD
    }

    /// The field itself: the top strip of `bounds`.
    pub fn field_rect(&self, bounds: Rect) -> Rect {
        Rect::new(bounds.left, bounds.top, bounds.right, bounds.top + self.field_height())
    }

    /// Splits `r` into a `w`-wide band at the LEADING edge and the rest —
    /// mirrored under `RightToLeftLayout`, which is the one thing that flag
    /// changes here.
    fn split_leading(r: Rect, w: f32, rtl: bool) -> (Rect, Rect) {
        if rtl {
            (Rect::new(r.right - w, r.top, r.right, r.bottom), Rect::new(r.left, r.top, r.right - w, r.bottom))
        } else {
            (Rect::new(r.left, r.top, r.left + w, r.bottom), Rect::new(r.left + w, r.top, r.right, r.bottom))
        }
    }

    /// The spin column, when `ShowUpDown` is on: `w-6` at the trailing edge,
    /// split in two by a rule.
    pub fn spin_rects(&self, bounds: Rect) -> Option<(Rect, Rect)> {
        if !self.inner.show_up_down {
            return None;
        }
        let field = self.field_rect(bounds);
        // The spin column is at the trailing edge, so it takes the OTHER side
        // from the content — hence the inverted `rtl`.
        let (column, _) = Self::split_leading(field, SPIN_COLUMN, !self.inner.right_to_left_layout);
        let mid = (column.top + column.bottom) / 2.0;
        Some((
            Rect::new(column.left, column.top, column.right, mid),
            Rect::new(column.left, mid, column.right, column.bottom),
        ))
    }

    /// The field's content band: everything the spin column leaves, less the
    /// `px-3`.
    fn content_rect(&self, bounds: Rect) -> Rect {
        let field = self.field_rect(bounds);
        let band = if self.inner.show_up_down {
            let (_, rest) =
                Self::split_leading(field, SPIN_COLUMN, !self.inner.right_to_left_layout);
            rest
        } else {
            field
        };
        Rect::new(band.left + FIELD_PAD_X, band.top, band.right - FIELD_PAD_X, band.bottom)
    }

    /// The `ShowCheckBox` well: [`control::CHECK_BOX`] square, vertically
    /// centred at the leading edge.
    pub fn check_rect(&self, bounds: Rect) -> Option<Rect> {
        if !self.inner.show_check_box {
            return None;
        }
        let (well, _) = Self::split_leading(
            self.content_rect(bounds),
            control::CHECK_BOX,
            self.inner.right_to_left_layout,
        );
        let cy = (well.top + well.bottom) / 2.0;
        Some(Rect::new(well.left, cy - control::CHECK_BOX / 2.0, well.right, cy + control::CHECK_BOX / 2.0))
    }

    /// The leading `size={14}` glyph, after the optional check box.
    pub fn icon_rect(&self, bounds: Rect) -> Rect {
        let content = self.content_rect(bounds);
        let after_check = match self.check_rect(bounds) {
            Some(_) => {
                let (_, rest) = Self::split_leading(
                    content,
                    control::CHECK_BOX + control::CHECK_GAP,
                    self.inner.right_to_left_layout,
                );
                rest
            }
            None => content,
        };
        let (icon, _) = Self::split_leading(after_check, FIELD_ICON, self.inner.right_to_left_layout);
        icon
    }

    /// Where the formatted value is drawn: one `gap-2` past the glyph.
    pub fn text_rect(&self, bounds: Rect) -> Rect {
        let icon = self.icon_rect(bounds);
        let content = self.content_rect(bounds);
        if self.inner.right_to_left_layout {
            Rect::new(content.left, content.top, icon.left - FIELD_GAP, content.bottom)
        } else {
            Rect::new(icon.right + FIELD_GAP, content.top, content.right, content.bottom)
        }
    }

    /// Where the dropped panel goes: `computePos`' `r.bottom + 4`, aligned on
    /// the field's leading edge (`DropDownAlign`), at the panel's own size.
    ///
    /// With a [`DatePicker::viewport`] the rest of `computePos` applies too:
    /// the panel flips ABOVE the field (`r.top - popH - 4`) when there is not
    /// room for it below and there is more room above, and it is pulled back
    /// horizontally to stay 8 DIP inside the viewport.
    pub fn drop_down_rect(&self, bounds: Rect) -> Rect {
        let field = self.field_rect(bounds);
        let size = self.calendar.panel_size();
        let left = match self.inner.drop_down_align {
            kubuno_controls::enums::LeftRightAlignment::Right => field.right - size.width,
            kubuno_controls::enums::LeftRightAlignment::Left => field.left,
        };
        let Some(area) = self.viewport else {
            let top = field.bottom + PANEL_OFFSET;
            return Rect::new(left, top, left + size.width, top + size.height);
        };
        place_panel(field, left, size, area)
    }

    /// Everything the dropped panel paints: the panel plus the reach of its
    /// `shadow-2xl` ([`shadow_outset`]). This is the rectangle a
    /// `host::popup` must cover — sized any smaller, the shadow is cut into a
    /// hard band at the popup's edge.
    pub fn drop_down_paint_bounds(&self, bounds: Rect) -> Rect {
        let panel = self.drop_down_rect(bounds);
        let (l, t, r, b) = shadow_outset(&SHADOW_2XL);
        Rect::new(panel.left - l, panel.top - t, panel.right + r, panel.bottom + b)
    }

    /// Paints the dropped panel ALONE — its shadow and its calendar — at
    /// `panel`, which is wherever the caller put it (in a popup's local space,
    /// typically: [`DatePicker::drop_down_rect`] rebased on
    /// [`DatePicker::drop_down_paint_bounds`]' top-left). Pair it with
    /// [`DatePicker::paint_field_only`] so the panel is not painted twice.
    pub fn paint_drop_down(&self, c: &dyn Canvas, panel: Rect) {
        c.draw_shadow(&panel, radius::XL, &SHADOW_2XL, SHADOW_BLACK);
        // The calendar shows its keyboard cursor once a key has moved it.
        let keyboard = self.calendar.focus_day.is_some();
        let state = WidgetState::REST.focused(keyboard).focus_visible(keyboard);
        self.calendar.paint(c, panel, state);
    }

    /// Shows the dropped panel in an interactive `host::popup` of its own, so
    /// it overflows every container and the window itself. Returns the
    /// popup's bounds (client DIP) — hand them to the focus manager's
    /// `keep_focus_in` so a click in the calendar does not blur the field —
    /// or `None` when the panel is closed. The pointer over the popup comes
    /// back in the page's client coordinates, so [`DatePicker::day_at`] and
    /// [`MonthCalendar::header_at`] on [`DatePicker::drop_down_rect`] hit-test
    /// it unchanged.
    pub fn popup_drop_down(&self, bounds: Rect) -> Option<Rect> {
        if !self.open || !self.has_panel() {
            return None;
        }
        let panel = self.drop_down_rect(bounds);
        let pb = self.drop_down_paint_bounds(bounds);
        let local = Rect::new(panel.left - pb.left, panel.top - pb.top, panel.right - pb.left, panel.bottom - pb.top);
        let me = self.clone();
        host::popup(pb, move |canvas| me.paint_drop_down(canvas, local));
        Some(pb)
    }

    /// Each editable segment's box on screen, for a click to select it: the
    /// segment's text extent inside [`DatePicker::text_rect`], on the full
    /// field height. Needs the canvas to measure the text.
    pub fn segment_rects(&self, c: &dyn Canvas, bounds: Rect) -> Vec<(Segment, Rect)> {
        let (text, spans) = self.field_text();
        let band = self.text_rect(bounds);
        let f = &c.formats().body;
        spans
            .iter()
            .map(|s| {
                let x0 = band.left + c.measure(&text[..s.start], f);
                let x1 = band.left + c.measure(&text[..s.end], f);
                (s.segment, Rect::new(x0.min(band.right), band.top, x1.min(band.right), band.bottom))
            })
            .collect()
    }

    /// The segment a click at `(x, y)` selects: the one under it, else the
    /// nearest on the same line (a click past the end selects the last) —
    /// `None` outside the text band.
    pub fn segment_at(&self, c: &dyn Canvas, bounds: Rect, x: f32, y: f32) -> Option<Segment> {
        let band = self.text_rect(bounds);
        if y < band.top || y >= band.bottom || x < band.left - FIELD_GAP || x >= band.right {
            return None;
        }
        let rects = self.segment_rects(c, bounds);
        let dist = |r: &Rect| if x < r.left { r.left - x } else if x >= r.right { x - r.right } else { 0.0 };
        rects
            .iter()
            .min_by(|a, b| dist(&a.1).total_cmp(&dist(&b.1)))
            .map(|(s, _)| *s)
    }

    /// Which part of the field `(x, y)` lands on.
    pub fn field_at(&self, bounds: Rect, x: f32, y: f32) -> Option<FieldPart> {
        if !self.field_rect(bounds).contains(x, y) {
            return None;
        }
        if let Some((up, down)) = self.spin_rects(bounds) {
            if up.contains(x, y) {
                return Some(FieldPart::SpinUp);
            }
            if down.contains(x, y) {
                return Some(FieldPart::SpinDown);
            }
        }
        if let Some(well) = self.check_rect(bounds) {
            // The well's own square, not its whole column: a click beside a
            // check box toggles nothing in this design system.
            if well.contains(x, y) {
                return Some(FieldPart::CheckBox);
            }
        }
        Some(FieldPart::Value)
    }

    /// Which day of the DROPPED panel `(x, y)` lands on — `None` when the panel
    /// is closed.
    pub fn day_at(&self, bounds: Rect, x: f32, y: f32) -> Option<Date> {
        self.open.then(|| self.calendar.day_at(self.drop_down_rect(bounds), x, y)).flatten()
    }

    // ── Paint ────────────────────────────────────────────────────────────────

    fn paint_field(&self, c: &dyn Canvas, bounds: Rect, state: WidgetState, dead: bool) {
        let t = c.theme();
        let field = self.field_rect(bounds);

        // `@ui/Input`: a `bg-white` face on `border-border`, `rounded`;
        // `focus:ring-2 focus:ring-primary focus:border-primary`; disabled takes
        // `bg-surface-2` and 60 % ink.
        let face = if dead { t.surface_2 } else { t.layer_background };
        c.fill_rounded(&field, radius::SM, &face);
        // `kb-field-focus`: the stroke shows on `:focus-visible`, and on an
        // OPEN picker (`kb-field-focus--on` — opened by mouse, the trigger has
        // the focus but not `:focus-visible`, and the panel below is this
        // field's). `error` turns border and ring to `danger`.
        let ring = !dead && (state.show_focus_ring() || self.open);
        let accent = if self.invalid { t.danger } else { t.accent };
        if ring {
            c.stroke_rounded_w(&field, radius::SM, &accent, FIELD_FOCUS_RING);
        } else if self.invalid && !dead {
            c.stroke_rounded(&field, radius::SM, &t.danger);
        } else if state.hot && !dead {
            c.stroke_rounded(&field, radius::SM, &t.border_strong);
        } else {
            c.stroke_rounded(&field, radius::SM, &faded(&t.card_stroke, if dead { DISABLED_FIELD_ALPHA } else { 1.0 }));
        }

        let alpha = if dead { DISABLED_FIELD_ALPHA } else { 1.0 };
        if let Some(well) = self.check_rect(bounds) {
            let state = if self.inner.checked { CheckState::Checked } else { CheckState::Unchecked };
            paint_check(c, well, state, alpha);
        }

        // `<span className="text-text-tertiary shrink-0">{triggerIcon}</span>`.
        c.vector_icon(
            self.leading_icon(),
            &self.icon_rect(bounds),
            FIELD_ICON,
            &faded(&t.text_tertiary, alpha),
        );

        // `displayText ? 'text-text-primary' : 'text-text-tertiary'` — and the
        // toolkit greys the value when `ShowCheckBox` is unchecked, which is the
        // same « there is no value here » reading.
        let ink = if self.value_shown() {
            or_theme(self.inner.control().fore_color, t.text_primary)
        } else {
            t.text_tertiary
        };
        let band = self.text_rect(bounds);
        let (text, spans) = self.field_text();
        // The edited segment, while the field holds the focus: the browser's
        // selection highlight behind it (see `SEGMENT_ALPHA`). `flex-1
        // truncate` still rules the text: the highlight is clipped to the
        // band the ellipsised value occupies.
        if state.focused && !dead && self.value_shown() {
            if let Some(span) = self.active_segment.and_then(|s| spans.iter().find(|x| x.segment == s)) {
                let f = &c.formats().body;
                let x0 = band.left + c.measure(&text[..span.start], f);
                let x1 = band.left + c.measure(&text[..span.end], f);
                let cy = (band.top + band.bottom) / 2.0;
                let hl = Rect::new(
                    x0.min(band.right),
                    cy - SEGMENT_LINE / 2.0,
                    x1.min(band.right),
                    cy + SEGMENT_LINE / 2.0,
                );
                c.fill_rounded(&hl, 0.0, &faded(&t.accent, SEGMENT_ALPHA));
            }
        }
        c.text_ellipsis(&text, &band, &c.formats().body, &faded(&ink, alpha));

        if let Some((up, down)) = self.spin_rects(bounds) {
            let rule = &faded(&t.card_stroke, alpha);
            let glyph = &faded(&t.text_secondary, alpha);
            // `hover:bg-surface-2 hover:text-text-primary`, inside the field's
            // `overflow-hidden` rounded box — so the hovered half keeps the
            // field's corner.
            let hot = if dead { None } else { self.hot_part };
            c.push_clip_rounded(&field, radius::SM);
            for (part, rect) in [(FieldPart::SpinUp, up), (FieldPart::SpinDown, down)] {
                if hot == Some(part) {
                    c.fill_rounded(&rect, 0.0, &t.surface_2);
                }
            }
            c.pop_clip_rounded();
            let glyph_of = |part: FieldPart| if hot == Some(part) { &t.text_primary } else { glyph };
            // `border-l` against the field, `border-b` between the buttons.
            let edge = if self.inner.right_to_left_layout { up.right - SPIN_RULE } else { up.left };
            c.fill_rounded(&Rect::new(edge, field.top, edge + SPIN_RULE, field.bottom), 0.0, rule);
            c.fill_rounded(&Rect::new(up.left, up.bottom - SPIN_RULE, up.right, up.bottom), 0.0, rule);
            c.vector_icon("ChevronUp", &up, SPIN_GLYPH, glyph_of(FieldPart::SpinUp));
            c.vector_icon("ChevronDown", &down, SPIN_GLYPH, glyph_of(FieldPart::SpinDown));
        }
    }

    /// Paints the field WITHOUT its dropped panel — for a caller that shows
    /// the panel elsewhere ([`DatePicker::popup_drop_down`] or
    /// [`DatePicker::paint_drop_down`]). Same field, same states as
    /// [`Widget::paint`], the open ring included.
    pub fn paint_field_only(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        canvas.fill_rounded(&self.field_rect(bounds), 0.0, &canvas.current_bg());
        let dead = state.disabled || !self.inner.control().enabled;
        self.paint_field(canvas, bounds, state, dead);
    }
}

/// `computePos` in `helpers.ts`, with the viewport as a rectangle: below the
/// field (`r.bottom + 4`) when the panel fits there or there is at least as
/// much room below as above, else above it (`r.top - popH - 4`); `left`
/// pulled inside `[area.left + 8, area.right - popW - 8]`.
fn place_panel(field: Rect, left: f32, size: Size, area: Rect) -> Rect {
    let below = area.bottom - field.bottom - PANEL_EDGE;
    let above = field.top - area.top - PANEL_EDGE;
    let top = if below >= size.height || below >= above {
        field.bottom + PANEL_OFFSET
    } else {
        field.top - size.height - PANEL_OFFSET
    };
    let left = left.min(area.right - size.width - PANEL_EDGE).max(area.left + PANEL_EDGE);
    Rect::new(left, top, left + size.width, top + size.height)
}

/// Paints a check well in the Kubuno check-box look (`@ui/Checkbox`), at the
/// geometry `crate::metrics::control` carries — the web's own canvas painter's
/// `{ size: 18, border: 2, radius: 4, tick: 11 }`.
fn paint_check(c: &dyn Canvas, well: Rect, state: CheckState, alpha: f32) {
    let t = c.theme();
    if state == CheckState::Unchecked {
        c.stroke_rounded_w(
            &well,
            control::CHECK_RADIUS,
            &faded(&t.card_stroke, alpha),
            control::CHECK_BORDER,
        );
        return;
    }
    c.fill_rounded(&well, control::CHECK_RADIUS, &faded(&t.accent, alpha));
    c.vector_icon("Check", &well, control::CHECK_TICK, &faded(&t.accent_foreground, alpha));
}

impl Widget for DatePicker {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    /// The Kubuno size: an input's height, and a width that holds the formatted
    /// value plus every part in front of and behind it.
    ///
    /// The text term is rounded up and given [`physical_pixel`] of slack: a
    /// band exactly as wide as [`Canvas::measure`] reports is not reliably wide
    /// enough for [`Canvas::text_ellipsis`] to draw the same string, because the
    /// painter snaps that band onto the pixel grid first. A picker laid out at
    /// its own intrinsic width was showing « 17/08/20… », which is how this
    /// line came to exist.
    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let text = canvas.measure(&self.display_text(), &canvas.formats().body).ceil()
            + physical_pixel(canvas);
        let mut width = 2.0 * FIELD_PAD_X + FIELD_ICON + FIELD_GAP + text;
        if self.inner.show_check_box {
            width += control::CHECK_BOX + control::CHECK_GAP;
        }
        if self.inner.show_up_down {
            width += SPIN_COLUMN;
        }
        Size::new(width.ceil(), self.field_height())
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        // Rule: every widget lands on an opaque background. What follows
        // may overpaint most of it (a fill, a card, a track); this makes
        // sure the parts that stay uncovered still read as an opaque
        // surface, so a widget never shows what is behind it.
        canvas.fill_rounded(&bounds, 0.0, &canvas.current_bg());
        let dead = state.disabled || !self.inner.control().enabled;
        self.paint_field(canvas, bounds, state, dead);
        if self.open && self.has_panel() && !dead {
            // The panel floats, so IT wears the shadow — `shadow-2xl`, as
            // `PickerPopover` declares it. Painted in place, past `bounds`:
            // a caller that needs it to escape a container or the window
            // uses `paint_field_only` + `popup_drop_down` instead.
            self.paint_drop_down(canvas, self.drop_down_rect(bounds));
        }
    }

    /// The field, plus the dropped panel — a click in the calendar is a click
    /// on the picker, not on whatever is painted behind it.
    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        self.field_rect(bounds).contains(x, y)
            || (self.open && self.has_panel() && self.drop_down_rect(bounds).contains(x, y))
    }

    fn type_name(&self) -> &'static str {
        if self.inner.format == DateTimePickerFormat::Time {
            "TimePicker"
        } else {
            "DatePicker"
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests
//
// Two kinds, in this order: the GEOMETRY this layer adds (which cell is where,
// what the pointer lands on, what a panel measures), and the proof that the
// arithmetic underneath is still the replica's — the grid, the bounds, the
// selection cap and the month walk are asserted through this type, so a
// re-derivation here would show up as a divergence from the values the replica's
// own tests pin.
//
// Every rectangle below is built by hand: `Canvas` carries a `TextFormats`, a
// set of COM objects a `--lib` test cannot build, so anything needing a live
// canvas (the paint, `Widget::measure` for the picker) is exercised by the
// gallery page instead. `MonthCalendar::panel_size` is deliberately canvas-free
// for that reason.
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno_controls::datetime::{Day, Time};

    /// The panel, laid out where `panel_size` asks for it — the rectangle every
    /// hit test below is run against.
    fn panel_of(cal: &MonthCalendar) -> Rect {
        let s = cal.panel_size();
        Rect::new(0.0, 0.0, s.width, s.height)
    }

    /// The centre of cell `index`, in the same space as the panel.
    fn centre(cal: &MonthCalendar, bounds: Rect, index: usize) -> (f32, f32) {
        let r = cal.day_rect(bounds, index);
        ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0)
    }

    // ── The panel's measurement ──────────────────────────────────────────────

    /// The web's own numbers: a 284 wide popover whose calendar column is
    /// `p-3` + header + weekday strip + six rows of 32.
    #[test]
    fn the_panel_measures_the_webs_popover() {
        let cal = MonthCalendar::new();
        let s = cal.panel_size();
        assert_eq!(s.width, cal::WIDTH, "`popoverSize().w` for a calendar mode");
        // 12 + 28 + 8 + 28 + 2 + 192 + 12, plus the today footer the toolkit
        // turns on by default.
        assert_eq!(s.height, 282.0 + cal::FOOTER);

        let mut bare = MonthCalendar::new();
        bare.show_today = false; // the replica's field, through DerefMut
        assert_eq!(bare.panel_size().height, 282.0);

        // Seven `grid-cols-7` tracks fill the padded width exactly.
        assert_eq!(cal::DAY_COLUMN * 7.0 + 2.0 * cal::PAD, cal::WIDTH);
        // And a day disc is smaller than its track — the gaps between the discs
        // are the web's `mx-auto`, not a rounding. A `const` block, because both
        // operands are constants and the check is worth making at compile time.
        const { assert!(cal::DAY < cal::DAY_COLUMN) };
    }

    /// `ShowWeekNumbers` adds one more column of the same grid, and the day
    /// columns keep their width instead of being squeezed.
    #[test]
    fn the_week_gutter_is_one_more_column() {
        let mut cal = MonthCalendar::new();
        cal.show_week_numbers = true;
        assert_eq!(cal.columns(), 8);
        assert_eq!(cal.panel_size().width, cal::WIDTH + cal::DAY_COLUMN);

        let bounds = panel_of(&cal);
        assert!((cal.column_width(bounds) - cal::DAY_COLUMN).abs() < 0.001);
        // The gutter sits to the LEFT of the seven day columns, and they start
        // one column in.
        let gutter = cal.week_column(bounds).expect("the gutter is shown");
        let grid = cal.grid_rect(bounds);
        assert_eq!(gutter.right, grid.left);
        assert!((grid.left - (bounds.left + cal::PAD + cal::DAY_COLUMN)).abs() < 0.001);

        // Without it there is no gutter and the grid opens at the padding.
        let plain = MonthCalendar::new();
        let b = panel_of(&plain);
        assert!(plain.week_column(b).is_none());
        assert_eq!(plain.grid_rect(b).left, b.left + cal::PAD);
    }

    // ── `day_at`, at the exact borders ───────────────────────────────────────

    /// June 2026 opens on Monday 1 June — the case the replica's own tests call
    /// out, because the naive offset would be zero and the grid would open flush
    /// on the 1st. It does not: the whole preceding week is shown, so the 1st is
    /// cell 7 and the cell before it is 31 May.
    #[test]
    fn day_at_finds_the_first_the_last_and_the_cell_before_the_first() {
        let cal = MonthCalendar::new().on(Date::new(2026, 6, 15));
        let bounds = panel_of(&cal);

        let (x, y) = centre(&cal, bounds, 7);
        assert_eq!(cal.day_at(bounds, x, y), Some(Date::new(2026, 6, 1)));

        // The cell before it is NOT empty — this grid has no empty cell.
        let (x, y) = centre(&cal, bounds, 6);
        assert_eq!(cal.day_at(bounds, x, y), Some(Date::new(2026, 5, 31)));
        assert!(!cal.cells()[6].in_month, "and it reads as a trailing day");

        // 30 June is the last day of the month: cell 7 + 29.
        let (x, y) = centre(&cal, bounds, 36);
        assert_eq!(cal.day_at(bounds, x, y), Some(Date::new(2026, 6, 30)));
        assert!(cal.cells()[36].in_month);
        assert!(!cal.cells()[37].in_month, "1 July is already trailing");
    }

    /// The corners of the grid, to the pixel: cell 0 answers at its own
    /// top-left and stops answering one row above or one column left.
    #[test]
    fn day_at_is_exact_at_the_grids_edges() {
        let cal = MonthCalendar::new().on(Date::new(2026, 6, 15));
        let bounds = panel_of(&cal);
        let grid = cal.grid_rect(bounds);

        assert_eq!(cal.cell_at(bounds, grid.left, grid.top), Some(0));
        assert_eq!(cal.cell_at(bounds, grid.right - 0.01, grid.top), Some(6));
        assert_eq!(cal.cell_at(bounds, grid.left, grid.bottom - 0.01), Some(35));
        assert_eq!(cal.cell_at(bounds, grid.right - 0.01, grid.bottom - 0.01), Some(41));

        // Just outside, on each side.
        assert_eq!(cal.cell_at(bounds, grid.left - 0.01, grid.top), None);
        assert_eq!(cal.cell_at(bounds, grid.right, grid.top), None);
        assert_eq!(cal.cell_at(bounds, grid.left, grid.top - 0.01), None);
        assert_eq!(cal.cell_at(bounds, grid.left, grid.bottom), None);
        // And the weekday strip is not the grid.
        let strip = cal.weekday_rect(bounds);
        assert_eq!(cal.cell_at(bounds, grid.left, (strip.top + strip.bottom) / 2.0), None);
    }

    /// A month whose 1st is a Sunday and one whose 1st is a Monday, under both
    /// `FirstDayOfWeek` choices — the rotation is an index rotation, and the
    /// leading week is always shown in full.
    #[test]
    fn a_month_starting_sunday_and_one_starting_monday() {
        // 1 March 2026 is a Sunday; 1 June 2026 is a Monday.
        assert_eq!(Date::new(2026, 3, 1).weekday(), 6);
        assert_eq!(Date::new(2026, 6, 1).weekday(), 0);

        for (month, first_day, cell_of_the_first) in [
            // March, weeks starting Monday: the 1st is the LAST cell of the
            // leading week.
            (3u8, Day::Monday, 6usize),
            // March, weeks starting Sunday: the 1st opens the second row.
            (3, Day::Sunday, 7),
            // June, weeks starting Monday: the 1st opens the second row.
            (6, Day::Monday, 7),
            // June, weeks starting Sunday: one trailing day precedes it.
            (6, Day::Sunday, 1),
        ] {
            let mut cal = MonthCalendar::new().on(Date::new(2026, month, 15));
            cal.first_day_of_week = first_day;
            let bounds = panel_of(&cal);
            let (x, y) = centre(&cal, bounds, cell_of_the_first);
            assert_eq!(
                cal.day_at(bounds, x, y),
                Some(Date::new(2026, month, 1)),
                "month {month} with {first_day:?}"
            );
            // Column 0 is always the chosen first day, and the grid never opens
            // on the 1st.
            assert_eq!(cal.cells()[0].date.weekday(), first_day.resolved());
            assert!(!cal.cells()[0].in_month);
        }
    }

    /// A leap February has a 29th and the year after it does not — the grid
    /// takes both from the replica's `days_in_month`.
    #[test]
    fn february_of_a_leap_year_has_a_twenty_ninth() {
        let leap = MonthCalendar::new().on(Date::new(2024, 2, 10));
        let bounds = panel_of(&leap);
        // 1 February 2024 is a Thursday, so with Monday-first weeks it is cell 3.
        assert_eq!(Date::new(2024, 2, 1).weekday(), 3);
        let (x, y) = centre(&leap, bounds, 3 + 28);
        assert_eq!(leap.day_at(bounds, x, y), Some(Date::new(2024, 2, 29)));
        assert!(leap.cells()[3 + 28].in_month);
        assert_eq!(leap.cells().iter().filter(|c| c.in_month).count(), 29);

        let common = MonthCalendar::new().on(Date::new(2026, 2, 10));
        assert_eq!(common.cells().iter().filter(|c| c.in_month).count(), 28);
    }

    // ── Navigation ───────────────────────────────────────────────────────────

    /// December → January is a year boundary, not a special case, and it works
    /// in both directions.
    #[test]
    fn navigating_crosses_december_into_january() {
        let mut cal = MonthCalendar::new().on(Date::new(2026, 12, 15));
        assert_eq!(cal.shown_month(), (2026, 12));
        cal.next_month();
        assert_eq!(cal.shown_month(), (2027, 1));
        cal.prev_month();
        assert_eq!(cal.shown_month(), (2026, 12));
        cal.prev_month();
        assert_eq!(cal.shown_month(), (2026, 11));

        // Twelve steps back land on the same month a year earlier.
        let mut year = MonthCalendar::new().on(Date::new(2026, 1, 31));
        for _ in 0..12 {
            year.prev_month();
        }
        assert_eq!(year.shown_month(), (2025, 1));

        // Browsing does NOT move the selection — the whole point of
        // `view_month`, which the toolkit does not have.
        assert_eq!(cal.selection_start(), Date::new(2026, 12, 15));

        // `ScrollChange` governs the step, and 0 means « the months shown ».
        let mut fast = MonthCalendar::new().on(Date::new(2026, 1, 15));
        assert_eq!(fast.scroll_months(), 1, "1×1 months shown by default");
        fast.scroll_change = 3;
        fast.next_month();
        assert_eq!(fast.shown_month(), (2026, 4));
    }

    /// The month walk clamps the day, so 31 January + 1 month is 28 February
    /// and not an impossible date.
    #[test]
    fn the_month_walk_clamps_the_day() {
        assert_eq!(add_months(Date::new(2026, 1, 31), 1), Date::new(2026, 2, 28));
        assert_eq!(add_months(Date::new(2024, 1, 31), 1), Date::new(2024, 2, 29));
        assert_eq!(add_months(Date::new(2026, 3, 31), -1), Date::new(2026, 2, 28));
    }

    // ── Bounds and selection, all of them the replica's rules ────────────────

    /// `MinDate` / `MaxDate` decide what may be picked; the days outside them
    /// still have a rectangle (so they can be painted greyed) but a click on
    /// one changes nothing.
    #[test]
    fn min_and_max_close_the_window_without_hiding_the_days() {
        let mut cal = MonthCalendar::new().on(Date::new(2026, 6, 15));
        cal.set_min_date(Date::new(2026, 6, 10));
        cal.set_max_date(Date::new(2026, 6, 20));

        assert!(cal.is_selectable(Date::new(2026, 6, 10)), "the bounds are inclusive");
        assert!(cal.is_selectable(Date::new(2026, 6, 20)));
        assert!(!cal.is_selectable(Date::new(2026, 6, 9)));
        assert!(!cal.is_selectable(Date::new(2026, 6, 21)));

        // The 9th is still on the grid — a picker greys it, it does not vanish.
        let bounds = panel_of(&cal);
        let (x, y) = centre(&cal, bounds, 7 + 8);
        assert_eq!(cal.day_at(bounds, x, y), Some(Date::new(2026, 6, 9)));

        // And clicking it is refused, leaving the selection where it was.
        let before = cal.selection_range();
        cal.click_day(Date::new(2026, 6, 9));
        assert_eq!(cal.selection_range(), before);

        // The replica's own floor and ceiling still hold.
        cal.set_min_date(Date::new(1000, 1, 1));
        assert_eq!(cal.min_date(), replica::MIN_DATE);
    }

    /// A range is picked in two clicks and capped by `MaxSelectionCount` — both
    /// rules the replica owns, reached from this layer's phase flag.
    #[test]
    fn a_multi_day_selection_takes_two_clicks_and_is_capped() {
        let mut cal = MonthCalendar::new().on(Date::new(2026, 6, 15));
        cal.max_selection_count = 7;

        cal.click_day(Date::new(2026, 6, 8));
        assert_eq!(cal.selection_range().start, Date::new(2026, 6, 8));
        assert_eq!(cal.selection_range().end, Date::new(2026, 6, 8), "one click anchors");
        assert!(cal.extending, "the next click extends");

        cal.click_day(Date::new(2026, 6, 11));
        assert_eq!(cal.selection_range().start, Date::new(2026, 6, 8));
        assert_eq!(cal.selection_range().end, Date::new(2026, 6, 11));
        assert!(!cal.extending, "and the range is closed");

        // A third click starts a new one.
        cal.click_day(Date::new(2026, 6, 20));
        assert_eq!(cal.selection_range().start, Date::new(2026, 6, 20));

        // The cap is the replica's: 10 days asked, 7 granted.
        cal.click_day(Date::new(2026, 6, 29));
        assert_eq!(cal.selection_range().start, Date::new(2026, 6, 20));
        assert_eq!(cal.selection_range().end, Date::new(2026, 6, 26));

        // With `MaxSelectionCount = 1` every click collapses the selection —
        // there is no phase at all.
        let mut single = MonthCalendar::new();
        single.max_selection_count = 1;
        single.click_day(Date::new(2026, 6, 8));
        assert!(!single.extending);
        single.click_day(Date::new(2026, 6, 11));
        assert_eq!(single.selection_range().start, Date::new(2026, 6, 11));
        assert_eq!(single.selection_range().end, Date::new(2026, 6, 11));
    }

    /// The three bolded-date lists are the replica's matching, reached through
    /// `Deref` — this layer only chooses a heavier format for them.
    #[test]
    fn bolded_dates_are_the_replicas_three_lists() {
        let mut cal = MonthCalendar::new().on(Date::new(2026, 6, 15));
        cal.bolded_dates.push(Date::new(2026, 6, 15));
        cal.annually_bolded_dates.push(Date::new(2000, 6, 21));
        cal.monthly_bolded_dates.push(Date::new(2000, 1, 1));
        assert!(cal.is_bold(Date::new(2026, 6, 15)));
        assert!(cal.is_bold(Date::new(2026, 6, 21)));
        assert!(cal.is_bold(Date::new(2026, 6, 1)));
        assert!(!cal.is_bold(Date::new(2026, 6, 16)));
    }

    // ── Header hit-testing ───────────────────────────────────────────────────

    /// The two nav buttons are round, so their corners do not answer; the
    /// caption between them does.
    #[test]
    fn header_at_reports_the_two_round_nav_buttons() {
        let cal = MonthCalendar::new().on(Date::new(2026, 6, 15));
        let bounds = panel_of(&cal);

        let prev = cal.prev_month_rect(bounds);
        let next = cal.next_month_rect(bounds);
        assert_eq!(prev.right - prev.left, cal::NAV);
        assert_eq!(prev.bottom - prev.top, cal::NAV);
        assert_eq!(next.right, bounds.right - cal::PAD);

        let mid = |r: Rect| ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
        let (x, y) = mid(prev);
        assert_eq!(cal.header_at(bounds, x, y), Some(HeaderPart::Prev));
        let (x, y) = mid(next);
        assert_eq!(cal.header_at(bounds, x, y), Some(HeaderPart::Next));
        // The corner of a round button is outside it.
        assert_ne!(
            cal.header_at(bounds, prev.left + 0.5, prev.top + 0.5),
            Some(HeaderPart::Prev)
        );
        let (x, y) = mid(cal.title_rect(bounds));
        assert_eq!(cal.header_at(bounds, x, y), Some(HeaderPart::Title));
        // Below the header there is no header button.
        assert_eq!(cal.header_at(bounds, x, cal.grid_rect(bounds).top), None);
    }

    // ── DatePicker ───────────────────────────────────────────────────────────

    /// The field is an input: the token height, the token radius, the token
    /// inset — and the panel hangs 4 DIP under it.
    #[test]
    fn the_field_is_an_input_and_the_panel_hangs_under_it() {
        let p = DatePicker::short().on(Date::new(2026, 6, 15));
        assert_eq!(p.display_text(), "15/06/2026");
        assert_eq!(p.field_height(), height::BUTTON_MD);

        let bounds = Rect::new(10.0, 20.0, 210.0, 400.0);
        let field = p.field_rect(bounds);
        assert_eq!((field.left, field.top), (10.0, 20.0));
        assert_eq!(field.bottom - field.top, height::BUTTON_MD);

        // `px-3`, then the `size={14}` glyph, then `gap-2`, then the value.
        let icon = p.icon_rect(bounds);
        assert_eq!(icon.left, field.left + FIELD_PAD_X);
        assert_eq!(icon.right - icon.left, FIELD_ICON);
        assert_eq!(p.text_rect(bounds).left, icon.right + FIELD_GAP);
        assert_eq!(p.text_rect(bounds).right, field.right - FIELD_PAD_X);

        // `computePos`: `r.bottom + 4`, at the panel's own size.
        let panel = p.drop_down_rect(bounds);
        assert_eq!(panel.top, field.bottom + PANEL_OFFSET);
        assert_eq!(panel.left, field.left);
        assert_eq!(panel.right - panel.left, p.calendar.panel_size().width);
    }

    /// A check box takes the leading slot and pushes the glyph along; a spinner
    /// takes the trailing one and shortens the content band.
    #[test]
    fn the_check_box_and_the_spinner_take_the_two_ends() {
        let bounds = Rect::new(0.0, 0.0, 300.0, 100.0);

        let mut checked = DatePicker::short();
        checked.show_check_box = true; // the replica's field, through DerefMut
        let well = checked.check_rect(bounds).expect("the well is shown");
        assert_eq!(well.left, bounds.left + FIELD_PAD_X);
        assert_eq!(well.right - well.left, control::CHECK_BOX);
        assert_eq!(well.bottom - well.top, control::CHECK_BOX);
        assert_eq!(checked.icon_rect(bounds).left, well.right + control::CHECK_GAP);
        assert_eq!(
            checked.field_at(bounds, (well.left + well.right) / 2.0, (well.top + well.bottom) / 2.0),
            Some(FieldPart::CheckBox)
        );
        // Beside the well is the value, not the box.
        assert_eq!(
            checked.field_at(bounds, well.right + 20.0, well.top),
            Some(FieldPart::Value)
        );

        let time = DatePicker::time();
        assert!(time.show_up_down);
        let (up, down) = time.spin_rects(bounds).expect("the spinner is shown");
        assert_eq!(up.right, bounds.right);
        assert_eq!(up.right - up.left, SPIN_COLUMN);
        assert_eq!(up.bottom, down.top);
        assert_eq!(down.bottom, time.field_rect(bounds).bottom);
        assert_eq!(time.field_at(bounds, up.left + 1.0, up.top + 1.0), Some(FieldPart::SpinUp));
        assert_eq!(time.field_at(bounds, down.left + 1.0, down.bottom - 1.0), Some(FieldPart::SpinDown));
        // The value stops before the spin column.
        assert!(time.text_rect(bounds).right <= up.left);

        // Nothing answers outside the field.
        assert_eq!(time.field_at(bounds, 10.0, 90.0), None);
    }

    /// `RightToLeftLayout` mirrors the parts: the check box and glyph move to
    /// the trailing edge, the spin column to the leading one.
    #[test]
    fn right_to_left_mirrors_the_fields_parts() {
        let bounds = Rect::new(0.0, 0.0, 300.0, 100.0);
        let mut p = DatePicker::time();
        p.show_check_box = true;
        p.right_to_left_layout = true;

        let (up, _) = p.spin_rects(bounds).expect("the spinner is shown");
        assert_eq!(up.left, bounds.left, "the spin column leads under RTL");
        let well = p.check_rect(bounds).expect("the well is shown");
        assert_eq!(well.right, bounds.right - FIELD_PAD_X);
        assert!(p.icon_rect(bounds).right < well.left);
        assert!(p.text_rect(bounds).right <= p.icon_rect(bounds).left);
    }

    /// A time picker IS the picker with a time format — one control, two
    /// paints, no second type.
    #[test]
    fn a_time_picker_is_the_picker_in_a_time_format() {
        let plain = DatePicker::new();
        assert_eq!(plain.format, DateTimePickerFormat::Long);
        assert_eq!(plain.leading_icon(), "Calendar");
        assert_eq!(plain.type_name(), "DatePicker");

        let t: TimePicker = DatePicker::time();
        assert_eq!(t.format, DateTimePickerFormat::Time);
        assert_eq!(t.leading_icon(), "Clock");
        assert_eq!(t.type_name(), "TimePicker");
        // The value is a time now, and it is the replica formatting it.
        assert_eq!(t.display_text(), "00:00:00");
        // A spinner has no panel to drop.
        assert!(!t.has_panel());
        let mut t = t;
        t.open_panel();
        assert!(!t.open);
    }

    /// Opening the panel seeds it from the value; picking a day writes back
    /// through the replica's clamp and closes it. The value has ONE home.
    #[test]
    fn the_panel_is_seeded_from_the_value_and_writes_back_to_it() {
        let mut p = DatePicker::short()
            .between(Date::new(2026, 6, 10), Date::new(2026, 6, 20))
            .on(Date::new(2026, 6, 15));

        p.open_panel();
        assert!(p.open);
        assert_eq!(p.calendar.shown_month(), (2026, 6));
        assert_eq!(p.calendar.selection_start(), Date::new(2026, 6, 15));
        // The bounds travelled too, so the panel greys the same days the field
        // refuses.
        assert!(!p.calendar.is_selectable(Date::new(2026, 6, 21)));

        p.pick(Date::new(2026, 6, 18));
        assert_eq!(p.value().date, Date::new(2026, 6, 18));
        assert_eq!(p.calendar.selection_start(), Date::new(2026, 6, 18));
        assert!(!p.open, "picking closes the popover, as `handleSelectDate` does");

        // A day outside the window changes nothing at all.
        p.open_panel();
        p.pick(Date::new(2026, 7, 1));
        assert_eq!(p.value().date, Date::new(2026, 6, 18));
        assert!(p.open);

        // Browsing the panel does not move the value.
        p.calendar.next_month();
        assert_eq!(p.calendar.shown_month(), (2026, 7));
        assert_eq!(p.value().date, Date::new(2026, 6, 18));
    }

    /// A picker's panel picks ONE date and shows no today row — the web's
    /// panel, not the toolkit's standalone calendar.
    #[test]
    fn a_pickers_panel_is_the_webs_panel_not_the_toolkits_calendar() {
        let p = DatePicker::new();
        assert_eq!(p.calendar.max_selection_count, 1);
        assert!(!p.calendar.show_today);
        assert!(p.calendar.footer_rect(Rect::new(0.0, 0.0, 284.0, 282.0)).is_none());

        // A standalone one keeps the toolkit's defaults.
        let stand = MonthCalendar::new();
        assert_eq!(stand.max_selection_count, 7);
        assert!(stand.show_today);
        assert!(stand.show_today_circle);
        assert!(!stand.show_week_numbers);
        assert_eq!(stand.first_day_of_week, Day::Default);
    }

    /// `Value` is greyed but still shown when `ShowCheckBox` is unchecked — the
    /// toolkit's « no value is selected », which is why this layer paints no ✕.
    #[test]
    fn an_unchecked_check_box_means_no_value_is_selected() {
        let mut p = DatePicker::short().on(Date::new(2026, 6, 15));
        assert!(p.value_shown());
        p.show_check_box = true;
        assert!(p.checked, "the catalogue default");
        assert!(p.value_shown());
        p.checked = false;
        assert!(!p.value_shown());
        assert_eq!(p.display_text(), "15/06/2026", "and the text is still there");
    }

    /// The panel only answers a hit test while it is open.
    #[test]
    fn the_dropped_panel_is_part_of_the_picker() {
        let mut p = DatePicker::short().on(Date::new(2026, 6, 15));
        let bounds = Rect::new(0.0, 0.0, 300.0, 400.0);
        let panel = p.drop_down_rect(bounds);
        let (x, y) = ((panel.left + panel.right) / 2.0, (panel.top + panel.bottom) / 2.0);

        assert!(!p.hit_test(bounds, x, y));
        assert!(p.day_at(bounds, x, y).is_none());
        p.open_panel();
        assert!(p.hit_test(bounds, x, y));
        assert!(p.day_at(bounds, x, y).is_some());
        assert!(p.hit_test(bounds, 5.0, 5.0), "and so is the field");
    }

    /// `DropDownAlign = Right` anchors the panel on the field's trailing edge.
    #[test]
    fn drop_down_align_moves_the_panel_not_its_size() {
        use kubuno_controls::enums::LeftRightAlignment;
        let mut p = DatePicker::short();
        let bounds = Rect::new(0.0, 0.0, 400.0, 400.0);
        let left = p.drop_down_rect(bounds);
        p.drop_down_align = LeftRightAlignment::Right;
        let right = p.drop_down_rect(bounds);
        assert_eq!(right.right, p.field_rect(bounds).right);
        assert_eq!(right.right - right.left, left.right - left.left);
        assert_eq!(right.top, left.top);
    }

    /// The weekday labels are the web's, and their base is the replica's
    /// Monday-first weekday numbering — so a rotation is an index rotation.
    #[test]
    fn the_weekday_labels_are_the_webs_monday_first_array() {
        assert_eq!(WEEKDAYS, ["L", "M", "M", "J", "V", "S", "D"]);
        assert_eq!(WEEKDAYS[Date::new(2026, 6, 15).weekday() as usize], "L", "a Monday");
        assert_eq!(WEEKDAYS[Date::new(2026, 6, 21).weekday() as usize], "D", "a Sunday");
        assert_eq!(Day::Default.resolved(), 0, "the port's fr-FR first day");
    }

    // ── Keyboard: the calendar grid ──────────────────────────────────────────

    /// Arrows move the cursor by a day / a week, crossing month boundaries,
    /// and the shown month follows.
    #[test]
    fn arrows_move_the_calendar_cursor_across_months() {
        let mut cal = MonthCalendar::new().on(Date::new(2026, 6, 30));
        assert_eq!(cal.active_day(), Date::new(2026, 6, 30), "falls back on the selection");
        assert_eq!(cal.key(vk::RIGHT, Modifiers::NONE), CalendarKey::Moved);
        assert_eq!(cal.focus_day, Some(Date::new(2026, 7, 1)));
        assert_eq!(cal.shown_month(), (2026, 7), "the page follows the cursor");
        cal.key(vk::UP, Modifiers::NONE);
        assert_eq!(cal.focus_day, Some(Date::new(2026, 6, 24)));
        cal.key(vk::DOWN, Modifiers::NONE);
        cal.key(vk::LEFT, Modifiers::NONE);
        assert_eq!(cal.focus_day, Some(Date::new(2026, 6, 30)));
        // Moving is not selecting.
        assert_eq!(cal.selection_start(), Date::new(2026, 6, 30));
        // A modified arrow is not the grid's.
        assert_eq!(cal.key(vk::LEFT, Modifiers::CTRL), CalendarKey::Ignored);
        assert_eq!(cal.key(vk::TAB, Modifiers::NONE), CalendarKey::Ignored);
    }

    /// PageUp/PageDown move a month (Shift: a year), clamping the day; Home /
    /// End go to the ends of the week, under either first day of week.
    #[test]
    fn page_keys_move_months_and_home_end_the_week() {
        let mut cal = MonthCalendar::new().on(Date::new(2026, 1, 31));
        cal.key(vk::PAGE_DOWN, Modifiers::NONE);
        assert_eq!(cal.focus_day, Some(Date::new(2026, 2, 28)), "31 Jan + 1 month");
        cal.key(vk::PAGE_UP, Modifiers::SHIFT);
        assert_eq!(cal.focus_day, Some(Date::new(2025, 2, 28)), "a year back");
        cal.key(vk::PAGE_DOWN, Modifiers::SHIFT);
        assert_eq!(cal.shown_month(), (2026, 2));

        // Wednesday 17 June 2026: Monday-first weeks run 15 → 21.
        let mut week = MonthCalendar::new().on(Date::new(2026, 6, 17));
        week.key(vk::HOME, Modifiers::NONE);
        assert_eq!(week.focus_day, Some(Date::new(2026, 6, 15)));
        week.key(vk::END, Modifiers::NONE);
        assert_eq!(week.focus_day, Some(Date::new(2026, 6, 21)));
        // Sunday-first: 14 → 20.
        let mut sunday = MonthCalendar::new().on(Date::new(2026, 6, 17));
        sunday.first_day_of_week = Day::Sunday;
        sunday.key(vk::HOME, Modifiers::NONE);
        assert_eq!(sunday.focus_day, Some(Date::new(2026, 6, 14)));
        sunday.key(vk::END, Modifiers::NONE);
        assert_eq!(sunday.focus_day, Some(Date::new(2026, 6, 20)));
    }

    /// The cursor stays inside `[MinDate, MaxDate]`, and Enter / Space picks
    /// the cursor's day through the replica's selection rules.
    #[test]
    fn the_cursor_is_bounded_and_enter_selects() {
        let mut cal = MonthCalendar::new().on(Date::new(2026, 6, 15));
        cal.max_selection_count = 1;
        cal.set_min_date(Date::new(2026, 6, 10));
        cal.set_max_date(Date::new(2026, 6, 20));
        cal.key(vk::PAGE_DOWN, Modifiers::NONE);
        assert_eq!(cal.focus_day, Some(Date::new(2026, 6, 20)), "clamped to MaxDate");
        cal.key(vk::UP, Modifiers::NONE);
        cal.key(vk::UP, Modifiers::NONE);
        assert_eq!(cal.focus_day, Some(Date::new(2026, 6, 10)), "clamped to MinDate");
        assert_eq!(cal.key(vk::ENTER, Modifiers::NONE), CalendarKey::Activated(Date::new(2026, 6, 10)));
        assert_eq!(cal.selection_start(), Date::new(2026, 6, 10));
        cal.key(vk::RIGHT, Modifiers::NONE);
        assert_eq!(cal.key(vk::SPACE, Modifiers::NONE), CalendarKey::Activated(Date::new(2026, 6, 11)));
    }

    /// Browsing with the nav buttons carries the cursor along, so it is never
    /// left on a month nobody sees.
    #[test]
    fn scrolling_carries_the_cursor() {
        let mut cal = MonthCalendar::new().on(Date::new(2026, 3, 31));
        cal.key(vk::LEFT, Modifiers::NONE);
        cal.next_month();
        assert_eq!(cal.focus_day, Some(Date::new(2026, 4, 30)));
        assert_eq!(cal.active_day(), Date::new(2026, 4, 30));
        // Without a cursor, the active day on a browsed page is the 1st.
        let mut plain = MonthCalendar::new().on(Date::new(2026, 3, 15));
        plain.next_month();
        assert_eq!(plain.focus_day, None);
        assert_eq!(plain.active_day(), Date::new(2026, 4, 1));
    }

    // ── The dropped panel's placement and paint bounds ──────────────────────

    /// `computePos`: under the field when it fits, above it when it does not
    /// and there is more room above, and held 8 DIP inside the viewport.
    #[test]
    fn the_panel_flips_above_and_stays_inside_the_viewport() {
        let mut p = DatePicker::short().on(Date::new(2026, 6, 15));
        let size = p.calendar.panel_size();
        let area = Rect::new(0.0, 0.0, 1000.0, 800.0);
        p.viewport = Some(area);

        let low = Rect::new(100.0, 100.0, 260.0, 136.0);
        let panel = p.drop_down_rect(low);
        assert_eq!(panel.top, low.bottom + PANEL_OFFSET, "room below");

        let high = Rect::new(100.0, 700.0, 260.0, 736.0);
        let panel = p.drop_down_rect(high);
        assert_eq!(panel.bottom, high.top - PANEL_OFFSET, "flipped above");
        assert_eq!(panel.bottom - panel.top, size.height);

        let right = Rect::new(900.0, 100.0, 990.0, 136.0);
        let panel = p.drop_down_rect(right);
        assert_eq!(panel.right, area.right - PANEL_EDGE, "pulled back from the right edge");

        // A viewport that reaches past the window on the left (a second
        // monitor) is honoured as is: the panel may sit at negative x.
        p.viewport = Some(Rect::new(-500.0, 0.0, 1000.0, 800.0));
        let left = Rect::new(-300.0, 100.0, -200.0, 136.0);
        assert_eq!(p.drop_down_rect(left).left, -300.0);

        // No viewport: the old « always under » anchor.
        p.viewport = None;
        assert_eq!(p.drop_down_rect(high).top, high.bottom + PANEL_OFFSET);
    }

    /// The paint bounds cover the whole `shadow-2xl`: 25.5 DIP each side,
    /// 0.5 above and 50.5 below, plus a DIP of antialiasing slack.
    #[test]
    fn the_paint_bounds_hold_the_whole_shadow() {
        let (l, t, r, b) = shadow_outset(&SHADOW_2XL);
        assert!((l - 26.5).abs() < 1e-4 && (r - 26.5).abs() < 1e-4);
        assert!((t - 1.5).abs() < 1e-4);
        assert!((b - 51.5).abs() < 1e-4);

        let p = DatePicker::short();
        let bounds = Rect::new(0.0, 0.0, 200.0, 36.0);
        let panel = p.drop_down_rect(bounds);
        let pb = p.drop_down_paint_bounds(bounds);
        assert_eq!(pb.left, panel.left - l);
        assert_eq!(pb.bottom, panel.bottom + b);

        // The four-layer float ramp reaches far further below than to the
        // sides — the band the audit saw when a caller guessed 10 DIP.
        let (_, _, side, below) = shadow_outset(&crate::metrics::SHADOW_FLOAT);
        assert!(below > 60.0 && side > 30.0);
    }

    // ── Segment editing ──────────────────────────────────────────────────────

    /// The segments are read off the pattern: three in the short date, three
    /// in the long one (the weekday name is not a field), three in a time.
    #[test]
    fn the_segments_follow_the_format_pattern() {
        let p = DatePicker::short().on(Date::new(2026, 6, 5));
        let (text, spans) = p.field_text();
        assert_eq!(text, "05/06/2026");
        assert_eq!(p.segments(), vec![Segment::Day, Segment::Month, Segment::Year]);
        assert_eq!((spans[2].start, spans[2].end, spans[2].run), (6, 10, 4));

        let long = DatePicker::new().on(Date::new(2026, 6, 15));
        assert_eq!(long.field_text().0, long.display_text(), "the same string as the replica's");
        assert_eq!(long.segments(), vec![Segment::Day, Segment::Month, Segment::Year]);

        let t = DatePicker::time();
        assert_eq!(t.segments(), vec![Segment::Hour, Segment::Minute, Segment::Second]);

        // A custom pattern with quoted text and a 12-hour clock.
        let mut c = DatePicker::new();
        c.format = DateTimePickerFormat::Custom;
        c.custom_format = Some("'le' d/M 'à' hh:mm tt".into());
        assert_eq!(c.field_text().0, c.display_text());
        assert_eq!(
            c.segments(),
            vec![Segment::Day, Segment::Month, Segment::Hour12, Segment::Minute, Segment::AmPm]
        );
    }

    /// ←/→ walk the segments and stop at the ends; ↑/↓ step the selected one
    /// and WRAP inside it without carrying.
    #[test]
    fn arrows_select_and_step_segments() {
        let mut p = DatePicker::short().on(Date::new(2026, 1, 31));
        assert!(p.key(vk::RIGHT, Modifiers::NONE));
        assert_eq!(p.active_segment, Some(Segment::Day));
        p.key(vk::UP, Modifiers::NONE);
        assert_eq!(p.value().date, Date::new(2026, 1, 1), "31 wraps to 1, January stays");
        p.key(vk::DOWN, Modifiers::NONE);
        assert_eq!(p.value().date, Date::new(2026, 1, 31));
        p.key(vk::RIGHT, Modifiers::NONE);
        p.key(vk::UP, Modifiers::NONE);
        assert_eq!(p.value().date, Date::new(2026, 2, 28), "the day clamps into February");
        p.key(vk::RIGHT, Modifiers::NONE);
        p.key(vk::RIGHT, Modifiers::NONE);
        assert_eq!(p.active_segment, Some(Segment::Year), "stops at the last segment");
        p.key(vk::HOME, Modifiers::NONE);
        assert_eq!(p.active_segment, Some(Segment::Day));
        p.key(vk::END, Modifiers::NONE);
        assert_eq!(p.active_segment, Some(Segment::Year));

        // A time wraps 23 → 0 and 59 → 0 without touching its neighbour.
        let mut t = DatePicker::time();
        t.key(vk::DOWN, Modifiers::NONE);
        assert_eq!(t.value().time.hour, 23);
        t.key(vk::RIGHT, Modifiers::NONE);
        t.key(vk::DOWN, Modifiers::NONE);
        assert_eq!((t.value().time.hour, t.value().time.minute), (23, 59));
    }

    /// Typed digits fill a segment and move on when it is complete; a year
    /// shows its partial digits until the fourth.
    #[test]
    fn typing_digits_fills_segments() {
        let mut p = DatePicker::short().on(Date::new(2026, 6, 15));
        p.select_segment(Segment::Day);
        assert!(p.text("4"));
        assert_eq!(p.value().date.day, 4, "4 cannot start a two-digit day");
        assert_eq!(p.active_segment, Some(Segment::Month));
        p.text("1");
        assert_eq!(p.pending_digits(), "1", "1 may still become 10, 11, 12");
        assert_eq!(p.field_text().0, "04/1/2026");
        p.text("2");
        assert_eq!(p.value().date.month, 12);
        assert_eq!(p.active_segment, Some(Segment::Year));
        p.text("20");
        assert_eq!(p.field_text().0, "04/12/20", "the partial year reads as typed");
        assert_eq!(p.value().date.year, 2026, "and is not applied yet");
        p.text("31");
        assert_eq!(p.value().date, Date::new(2031, 12, 4));

        // Backspace drops a pending digit; Escape drops them all.
        p.select_segment(Segment::Year);
        p.text("19");
        assert!(p.key(vk::BACK, Modifiers::NONE));
        assert_eq!(p.pending_digits(), "1");
        assert!(p.key(vk::ESCAPE, Modifiers::NONE));
        assert_eq!(p.pending_digits(), "");
        assert!(!p.handles_key(vk::ESCAPE, Modifiers::NONE), "nothing left to cancel");

        // Leaving a segment commits it; a two-digit year keeps the century.
        p.text("27");
        p.key(vk::LEFT, Modifiers::NONE);
        assert_eq!(p.value().date.year, 2027);

        // The bounds still hold: a typed day outside them is clamped.
        let mut b = DatePicker::short()
            .between(Date::new(2026, 6, 10), Date::new(2026, 6, 20))
            .on(Date::new(2026, 6, 15));
        b.select_segment(Segment::Day);
        b.text("25");
        assert_eq!(b.value().date, Date::new(2026, 6, 20));
    }

    /// AM/PM takes a letter; a 12-hour hour keeps its half.
    #[test]
    fn a_twelve_hour_clock_keeps_its_half() {
        let mut p = DatePicker::new();
        p.format = DateTimePickerFormat::Custom;
        p.custom_format = Some("hh:mm tt".into());
        p.set_value(DateTime::new(Date::new(2026, 6, 15), Time { hour: 14, minute: 5, second: 0 }));
        assert_eq!(p.field_text().0, "02:05 PM");
        p.select_segment(Segment::Hour12);
        p.key(vk::UP, Modifiers::NONE);
        assert_eq!(p.value().time.hour, 15);
        p.select_segment(Segment::AmPm);
        assert!(p.text("a"));
        assert_eq!(p.value().time.hour, 3);
        p.select_segment(Segment::Hour12);
        p.text("12");
        assert_eq!(p.value().time.hour, 0, "12 AM is midnight");
    }

    /// With the panel closed, Enter / F4 / Alt+↓ open it with the cursor on
    /// the value; open, the calendar takes the keys, Enter picks and closes,
    /// Escape closes, and Tab is never taken.
    #[test]
    fn the_keyboard_opens_drives_and_closes_the_panel() {
        let mut p = DatePicker::short().on(Date::new(2026, 6, 15));
        assert!(!p.handles_key(vk::TAB, Modifiers::NONE));
        assert!(p.key(vk::DOWN, Modifiers::ALT));
        assert!(p.open);
        assert_eq!(p.calendar.focus_day, Some(Date::new(2026, 6, 15)));
        p.key(vk::RIGHT, Modifiers::NONE);
        p.key(vk::DOWN, Modifiers::NONE);
        assert_eq!(p.value().date, Date::new(2026, 6, 15), "browsing is not picking");
        assert!(p.key(vk::ENTER, Modifiers::NONE));
        assert!(!p.open);
        assert_eq!(p.value().date, Date::new(2026, 6, 23));

        p.key(vk::F4, Modifiers::NONE);
        assert!(p.open);
        assert!(!p.handles_key(vk::TAB, Modifiers::NONE));
        assert!(p.key(vk::ESCAPE, Modifiers::NONE));
        assert!(!p.open);
        p.key(vk::ENTER, Modifiers::NONE);
        assert!(p.open);
        p.key(vk::UP, Modifiers::ALT);
        assert!(!p.open);

        // A spinner has no panel: Alt+↓ is not its key, Space not either.
        let t = DatePicker::time();
        assert!(!t.handles_key(vk::DOWN, Modifiers::ALT));
        assert!(!t.handles_key(vk::SPACE, Modifiers::NONE));
        // With a check box, Space toggles it and editing stops while unchecked.
        let mut c = DatePicker::short().on(Date::new(2026, 6, 15));
        c.show_check_box = true;
        c.key(vk::SPACE, Modifiers::NONE);
        assert!(!c.checked);
        c.step(1);
        assert_eq!(c.value().date, Date::new(2026, 6, 15), "no value, nothing to edit");
        assert!(!c.text("3"));
    }

    /// A lost focus commits typed digits and closes the panel.
    #[test]
    fn blur_commits_and_closes() {
        let mut p = DatePicker::short().on(Date::new(2026, 6, 15));
        p.select_segment(Segment::Day);
        p.text("2");
        p.open_panel();
        p.blur();
        assert_eq!(p.value().date.day, 2);
        assert!(!p.open);
    }

    /// The month caption is capitalised the way `DayView` capitalises it, and
    /// the names themselves are the replica's.
    #[test]
    fn the_caption_capitalises_the_replicas_month_name() {
        assert_eq!(month_caption(2026, 6), "Juin 2026");
        assert_eq!(month_caption(2026, 2), "Février 2026");
        assert_eq!(month_caption(2026, 12), "Décembre 2026");
    }
}
