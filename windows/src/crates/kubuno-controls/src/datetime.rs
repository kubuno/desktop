//! `DateTimePicker` and `MonthCalendar` — the two date/time controls.
//!
//! ## Why these two share a file
//!
//! They are siblings in the toolkit (both derive straight from `Control`, not
//! from a common date base), but they compute on the *same* thing: a civil
//! calendar. WinForms leans on `System.DateTime` and `CultureInfo`; this crate
//! has neither, and — by rule — must not add a date/time dependency. So the
//! file carries its own **civil-date core** (Howard Hinnant's
//! `days_from_civil` / `civil_from_days`), a pure, exact, integer calendar that
//! both controls measure and paint against. Everything a test could check
//! without a window — leap years, month lengths, day-of-week, the month grid,
//! selection clamping, the format engine — is a free function over that core.
//!
//! ## What each control actually owns
//!
//! The catalogue lists 21 "declared" properties on `DateTimePicker` and 28 on
//! `MonthCalendar`, but several of those are `Control` properties the toolkit
//! merely *re-surfaces* (a `[Browsable]`/hiding override): `BackColor`,
//! `ForeColor`, `Text`, `BackgroundImage`, `BackgroundImageLayout`, and (for the
//! calendar) `Size`. Per the brief's rule #2 those live on [`ControlBase`] and
//! are reused through `Deref`, never duplicated here. Each struct below owns
//! only the state its counterpart genuinely *adds*.
//!
//! `BackgroundImage` is the one re-surfaced property `ControlBase` does not
//! model (the port has no image object); it is therefore not honoured by either
//! control. That is a shared gap, noted rather than faked.
//!
//! ## Localisation
//!
//! WinForms pulls month/day names and the Long/Short/Time patterns from the
//! ambient `CultureInfo`. The port has no culture service, and Kubuno ships in
//! French, so the names and default patterns here are **fr-FR** — which is also
//! what the reference sheet (`07-datetime.png`) was painted with. This is a
//! deliberate, documented choice, isolated in [`Names`] so a future culture
//! service can replace it in one place.
//!
//! ## What these two paint with
//!
//! Every colour, metric and font comes from [`crate::system::Visuals`] — the
//! values Windows itself publishes — and every shape is **square**: a WinForms
//! surface has no rounded corner anywhere, and the `DateTimePicker`'s field is
//! not a stroked box but a `DrawEdge` bevel (`Border3DStyle::Sunken`), the same
//! two-ring well a `TextBox` sits in. The only curve either control draws is the
//! `ShowTodayCircle` ring, which is genuinely round in the toolkit too.
//!
//! Two facts the system does not publish, and what is done instead:
//!
//! * **The title band.** The classic `MonthCalendar` defaults `TitleBackColor`
//!   to `SystemColors.ActiveCaption`, which `Visuals` does not read (it carries
//!   no caption colours). The band therefore falls back to `Window` /
//!   `WindowText` — which is also exactly what the reference sheet shows, the
//!   themed control painting its header on the calendar's own ground.
//! * **The today marker.** The native control hard-codes `Color.Red`. There is
//!   no red among the system colours, so the ring and the today swatch take
//!   `Highlight`, the colour the themed control (and the reference sheet) marks
//!   today with.
//!
//! Both are stated at their call sites as well, so neither reads as an
//! arbitrary choice.

use drive_app_controls::{Canvas, Rect};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

use crate::control::{Control, ControlBase, ControlCanvas, FontRole};
use crate::enums::{LeftRightAlignment, Size};
use crate::system::{Border3DSide, Border3DStyle};

// ═══════════════════════════════════════════════════════════════════════════
// The civil-date core
//
// A calendar is pure arithmetic on a day serial number. `days_from_civil`
// numbers days so that 1970-01-01 is 0; the inverse round-trips exactly for
// every date the controls can hold. Nothing here allocates, branches on a
// clock, or depends on a locale — it is the bedrock both controls stand on.
// ═══════════════════════════════════════════════════════════════════════════

/// A calendar date. Fields are ordered year→month→day so the derived `Ord` is
/// chronological — which is exactly what selection clamping and Min/Max bounds
/// compare on, so no hand-written comparator can drift from it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Date {
    pub year:  i32,
    pub month: u8,
    pub day:   u8,
}

/// A time of day, to the second. Same trick: field order makes `Ord` correct.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash, Default)]
pub struct Time {
    pub hour:   u8,
    pub minute: u8,
    pub second: u8,
}

/// A date and a time — `DateTimePicker.Value` is one of these.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct DateTime {
    pub date: Date,
    pub time: Time,
}

impl Date {
    pub const fn new(year: i32, month: u8, day: u8) -> Self {
        Self { year, month, day }
    }

    /// Serial day number, 1970-01-01 = 0 (Hinnant `days_from_civil`). Valid for
    /// any Gregorian date; the algorithm shifts the year so that leap days fall
    /// at the end of the internal year, which is what makes it branch-free.
    pub fn to_days(self) -> i64 {
        let (y, m, d) = (self.year as i64, self.month as i64, self.day as i64);
        let y = if m <= 2 { y - 1 } else { y };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400; // [0, 399]
        let mp = if m > 2 { m - 3 } else { m + 9 }; // Mar=0 … Feb=11
        let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
        era * 146097 + doe - 719468
    }

    /// The inverse of [`Date::to_days`]. Exact round-trip across centuries.
    pub fn from_days(z: i64) -> Self {
        let z = z + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = z - era * 146097; // [0, 146096]
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
        let mp = (5 * doy + 2) / 153; // [0, 11]
        let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
        let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
        Self {
            year:  (y + if m <= 2 { 1 } else { 0 }) as i32,
            month: m as u8,
            day:   d as u8,
        }
    }

    /// Day of week, 0 = Monday … 6 = Sunday. Derived from the serial number so
    /// it needs no separate table and stays correct for negative serials.
    pub fn weekday(self) -> u8 {
        // 1970-01-01 (serial 0) is a Thursday = 3 in this Monday-based scheme.
        ((self.to_days().rem_euclid(7)) as u8 + 3) % 7
    }

    /// Whether `year` is a Gregorian leap year.
    pub fn is_leap(year: i32) -> bool {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }

    /// Number of days in `month` of `year` (1-based month).
    pub fn days_in_month(year: i32, month: u8) -> u8 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => if Self::is_leap(year) { 29 } else { 28 },
            _ => 0,
        }
    }

    /// This date shifted by `days` (may cross months, years, the epoch).
    pub fn add_days(self, days: i64) -> Self {
        Self::from_days(self.to_days() + days)
    }

    /// The first day of this date's month — where a month grid starts from.
    pub fn first_of_month(self) -> Self {
        Self::new(self.year, self.month, 1)
    }
}

impl DateTime {
    pub const fn new(date: Date, time: Time) -> Self {
        Self { date, time }
    }

    /// Midnight on `date` — the value a date-only control holds.
    pub const fn at_midnight(date: Date) -> Self {
        Self { date, time: Time { hour: 0, minute: 0, second: 0 } }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Enumerations these two controls declare
// ═══════════════════════════════════════════════════════════════════════════

/// `DateTimePickerFormat` — whether the field shows a standard or custom
/// pattern. Discriminants match the toolkit's `[Flags]` values so a round-trip
/// through an integer is exact.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DateTimePickerFormat {
    /// The culture's long date pattern (`LongDatePattern`). The WinForms default.
    #[default]
    Long   = 1,
    /// The culture's short date pattern (`ShortDatePattern`).
    Short  = 2,
    /// The culture's time pattern (`LongTimePattern`).
    Time   = 4,
    /// `CustomFormat` drives the display.
    Custom = 8,
}

/// `System.Windows.Forms.Day` — the first day of the week, or `Default` to take
/// it from the locale. Discriminants match the toolkit (Monday = 0 … Sunday =
/// 6, Default = 7).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Day {
    Monday    = 0,
    Tuesday   = 1,
    Wednesday = 2,
    Thursday  = 3,
    Friday    = 4,
    Saturday  = 5,
    Sunday    = 6,
    /// « Use the locale's first day of week. »
    #[default]
    Default   = 7,
}

impl Day {
    /// Resolve to a concrete first day, 0 = Monday … 6 = Sunday. The port has no
    /// locale service, so `Default` resolves to **Monday** — the fr-FR first day
    /// the reference sheet is built with. A future culture service replaces just
    /// this line.
    pub fn resolved(self) -> u8 {
        match self {
            Day::Monday => 0,
            Day::Tuesday => 1,
            Day::Wednesday => 2,
            Day::Thursday => 3,
            Day::Friday => 4,
            Day::Saturday => 5,
            Day::Sunday => 6,
            Day::Default => 0,
        }
    }
}

/// `MonthCalendar.CalendarDimensions` — the grid of months shown at once.
/// WinForms models this as a `System.Drawing.Size` of integer columns × rows;
/// a dedicated struct keeps it from being confused with a DIP [`Size`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CalendarDimensions {
    pub columns: u8,
    pub rows:    u8,
}

impl CalendarDimensions {
    /// Clamp to the toolkit's rules: each side ≥ 1, and the product ≤ 12 (the
    /// native control refuses to show more than twelve months, shrinking `rows`
    /// first). Applied on every set so the invariant always holds.
    pub fn clamped(columns: i32, rows: i32) -> Self {
        let mut cols = columns.clamp(1, 12) as u8;
        let mut rows = rows.clamp(1, 12) as u8;
        // Reduce rows, then columns, until the product fits — mirrors WinForms.
        while cols as u16 * rows as u16 > 12 {
            if rows > 1 {
                rows -= 1;
            } else {
                cols -= 1;
            }
        }
        Self { columns: cols, rows }
    }
}

impl Default for CalendarDimensions {
    fn default() -> Self {
        Self { columns: 1, rows: 1 }
    }
}

/// `SelectionRange` — a `[start, end]` pair of dates. A view over the calendar's
/// `SelectionStart`/`SelectionEnd`, exactly as WinForms exposes it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SelectionRange {
    pub start: Date,
    pub end:   Date,
}

// ═══════════════════════════════════════════════════════════════════════════
// Documented bounds
//
// The toolkit publishes its own hard limits — `DateTimePicker.MinimumDateTime`
// / `MaximumDateTime`, and the same range for `MonthCalendar`. `MinDate`/
// `MaxDate` default to them and may never be set outside them.
// ═══════════════════════════════════════════════════════════════════════════

/// `DateTimePicker.MinimumDateTime` / `MonthCalendar.MinDate` default —
/// 1753-01-01, the SQL-Server-compatible floor the toolkit uses.
pub const MIN_DATE: Date = Date::new(1753, 1, 1);
/// `DateTimePicker.MaximumDateTime` / `MonthCalendar.MaxDate` default —
/// 9998-12-31, the toolkit's documented ceiling.
pub const MAX_DATE: Date = Date::new(9998, 12, 31);

/// The port has no clock, and a control is a pure value, so "today" / the
/// default `Value` cannot be `DateTime.Now`. This deterministic sentinel stands
/// in; the host (and the demo form) set `Value`, `TodayDate` and the selection
/// explicitly. Chosen inside `[MIN_DATE, MAX_DATE]` so every default is valid.
pub const DEFAULT_TODAY: Date = Date::new(2000, 1, 1);

// ═══════════════════════════════════════════════════════════════════════════
// Localised names + the format engine
// ═══════════════════════════════════════════════════════════════════════════

/// Month and day names, and the standard patterns, for one culture. Bundled so
/// the format engine is a pure function of `(value, pattern, names)` and the
/// fr-FR strings live in exactly one place.
pub struct Names {
    /// Full month names, index 1..=12 (index 0 unused).
    pub months:      [&'static str; 13],
    /// Abbreviated month names, index 1..=12.
    pub months_abbr: [&'static str; 13],
    /// Full day names, index 0 = Monday … 6 = Sunday.
    pub days:        [&'static str; 7],
    /// Abbreviated day names, index 0 = Monday … 6 = Sunday.
    pub days_abbr:   [&'static str; 7],
    /// AM / PM designators (fr-FR barely uses them; kept for engine fidelity).
    pub am:          &'static str,
    pub pm:          &'static str,
    /// `LongDatePattern`, `ShortDatePattern`, `LongTimePattern`.
    pub long_date:   &'static str,
    pub short_date:  &'static str,
    pub long_time:   &'static str,
}

/// The fr-FR names and patterns. Verified against the reference sheet: Short =
/// `15/06/2026`, Time = `14:30:00`, Long = `lundi 15 juin …`, header day names
/// `lun. mar. mer. jeu. ven. sam. dim.`.
pub const FR: Names = Names {
    months: [
        "", "janvier", "février", "mars", "avril", "mai", "juin", "juillet",
        "août", "septembre", "octobre", "novembre", "décembre",
    ],
    months_abbr: [
        "", "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août",
        "sept.", "oct.", "nov.", "déc.",
    ],
    days:      ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"],
    days_abbr: ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."],
    am: "AM",
    pm: "PM",
    long_date:  "dddd d MMMM yyyy",
    short_date: "dd/MM/yyyy",
    long_time:  "HH:mm:ss",
};

/// Format a value against a custom pattern (the same grammar WinForms and .NET
/// use for `CustomFormat`).
///
/// **Supported specifiers**: `d dd ddd dddd` (day / day-name), `M MM MMM MMMM`
/// (month / month-name), `y yy yyy yyyy` (year), `H HH` (24-h), `h hh` (12-h),
/// `m mm` (minute), `s ss` (second), `t tt` (AM/PM). Text inside `'…'` or `"…"`
/// is literal, and `\` escapes the next character.
///
/// **Not supported** — and, per the "honour or declare" rule, never silently
/// reinterpreted: `f`/`F` (fractional seconds), `g` (era), `K`/`z` (time-zone
/// offset). If one appears it is emitted **verbatim as a literal letter**; this
/// is a defined, documented fallback, not a substitution with another field.
pub fn format_custom(value: DateTime, pattern: &str, names: &Names) -> String {
    let (d, t) = (value.date, value.time);
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            // Quoted literal: copy until the matching quote (or end of string).
            '\'' | '"' => {
                let quote = ch;
                i += 1;
                while i < chars.len() && chars[i] != quote {
                    out.push(chars[i]);
                    i += 1;
                }
                i += 1; // skip the closing quote
            }
            // Escape: the next character is a literal.
            '\\' => {
                if i + 1 < chars.len() {
                    out.push(chars[i + 1]);
                }
                i += 2;
            }
            // A run of the same specifier letter.
            'd' | 'M' | 'y' | 'H' | 'h' | 'm' | 's' | 't' | 'f' | 'F' | 'g' | 'K' | 'z' => {
                let mut n = 1;
                while i + n < chars.len() && chars[i + n] == ch {
                    n += 1;
                }
                emit_run(&mut out, ch, n, d, t, names);
                i += n;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    out
}

/// Emit one specifier run of `n` copies of `ch`.
fn emit_run(out: &mut String, ch: char, n: usize, d: Date, t: Time, names: &Names) {
    use std::fmt::Write;
    let wd = d.weekday() as usize;
    let hour12 = { let h = t.hour % 12; if h == 0 { 12 } else { h } };
    match ch {
        'd' => match n {
            1 => { let _ = write!(out, "{}", d.day); }
            2 => { let _ = write!(out, "{:02}", d.day); }
            3 => out.push_str(names.days_abbr[wd]),
            _ => out.push_str(names.days[wd]),
        },
        'M' => match n {
            1 => { let _ = write!(out, "{}", d.month); }
            2 => { let _ = write!(out, "{:02}", d.month); }
            3 => out.push_str(names.months_abbr[d.month as usize]),
            _ => out.push_str(names.months[d.month as usize]),
        },
        'y' => match n {
            1 => { let _ = write!(out, "{}", (d.year % 100).unsigned_abs()); }
            2 => { let _ = write!(out, "{:02}", (d.year % 100).unsigned_abs()); }
            3 => { let _ = write!(out, "{:03}", d.year); }
            _ => { let _ = write!(out, "{:04}", d.year); }
        },
        'H' => { let _ = write!(out, "{:0w$}", t.hour, w = n.min(2)); }
        'h' => { let _ = write!(out, "{:0w$}", hour12, w = n.min(2)); }
        'm' => { let _ = write!(out, "{:0w$}", t.minute, w = n.min(2)); }
        's' => { let _ = write!(out, "{:0w$}", t.second, w = n.min(2)); }
        't' => {
            let s = if t.hour < 12 { names.am } else { names.pm };
            if n == 1 {
                if let Some(first) = s.chars().next() {
                    out.push(first);
                }
            } else {
                out.push_str(s);
            }
        }
        // Explicitly-unsupported specifiers: emitted verbatim (documented above).
        _ => {
            for _ in 0..n {
                out.push(ch);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Month-grid geometry — a pure, tested function
//
// Given a month and the first day of the week, which date is in each of the six
// rows × seven columns? This is the one bit of the calendar a bug would make
// invisible-but-wrong, so it is a free function with its own tests.
// ═══════════════════════════════════════════════════════════════════════════

/// One cell of a month grid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub date: Date,
    /// `false` for the trailing days of the previous/next month that fill the
    /// first and last rows — the toolkit paints them in `TrailingForeColor`.
    pub in_month: bool,
}

/// The 6×7 grid for `(year, month)`, laid out so column 0 is `first_day`
/// (0 = Monday … 6 = Sunday). Six rows always, matching the native control, so
/// the geometry never reflows as the month changes.
///
/// ## The leading week is never empty
///
/// The obvious offset — `(weekday(1st) - first_day) mod 7` — is **zero** when
/// the 1st happens to fall on the first day of the week, and the grid then opens
/// flush on the 1st. The native control does not: it always shows the whole
/// preceding week as trailing days, so the month never starts in the top-left
/// cell. June 2026 is the case that exposes it — the 1st is a Monday, and with
/// `FirstDayOfWeek = Monday` the reference sheet's first row is **25–31 May**,
/// not 1–7 June.
///
/// This was invisible to every test the port had (they all agreed with the
/// arithmetic) and only showed up against the real control, which is why the
/// rule is stated here rather than left implicit in a `% 7`. The offset is
/// therefore in `1..=7`, never `0`.
///
/// The trailing edge needs no equivalent rule: six rows of seven is six *whole*
/// weeks by construction, and the longest case (31 days behind a 7-day leading
/// week = 38 cells) still fits, so the grid always closes on a complete week.
pub fn month_grid(year: i32, month: u8, first_day: u8) -> [Cell; 42] {
    let first = Date::new(year, month, 1);
    // How many leading trailing-days precede the 1st, given where the week
    // starts. A whole week (7) rather than none (0) when the two coincide — see
    // the note above; this is the native control's behaviour, not a rounding.
    let offset = match (first.weekday() + 7 - first_day) % 7 {
        0 => 7,
        n => n,
    };
    let start = first.add_days(-(offset as i64));
    let mut cells = [Cell { date: first, in_month: true }; 42];
    for (idx, cell) in cells.iter_mut().enumerate() {
        let date = start.add_days(idx as i64);
        *cell = Cell { date, in_month: date.year == year && date.month == month };
    }
    cells
}

/// ISO-8601 week of the year (weeks start Monday; week 1 contains the first
/// Thursday).
///
/// The native `MonthCalendar` derives its week numbers from the OS locale's
/// `CalendarWeekRule` and `FirstDayOfWeek`, which can differ from ISO by ±1 at a
/// year boundary. The port has no locale service, so it uses the well-defined
/// ISO rule and documents the possible off-by-one rather than guessing at the
/// native heuristic. Listed as a known deviation.
pub fn iso_week(date: Date) -> u32 {
    // Thursday of this date's week decides the year the week belongs to.
    let thursday = date.add_days(3 - date.weekday() as i64);
    let jan1 = Date::new(thursday.year, 1, 1);
    ((thursday.to_days() - jan1.to_days()) / 7 + 1) as u32
}

// ═══════════════════════════════════════════════════════════════════════════
// Shared paint helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Resolve a [`FontRole`] to one of the shared DirectWrite formats.
///
/// **Measurement only.** Both controls *paint* with the system UI font
/// (`Visuals::fonts`), which is what the toolkit measures against — but
/// `Control::preferred_size` receives a bare [`Canvas`], not a
/// [`ControlCanvas`], so the system font is out of reach there and the shared
/// formats are the only ones available. Widening that signature is out of scope
/// for a repaint, so the gap is stated here rather than hidden: a picker's
/// natural width is measured in the shared body face and painted in Segoe UI.
/// Every picker on the reference sheet is given an explicit `Width`, so the
/// difference does not reach the comparison.
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

/// An explicit colour if set, else the system one — the ambient resolution the
/// `Calendar*`/`Title*`/`Trailing*` colour properties perform. In WinForms an
/// unset colour means « take the system default », never a literal transparent.
fn or_system(opt: Option<D2D1_COLOR_F>, fallback: D2D1_COLOR_F) -> D2D1_COLOR_F {
    opt.unwrap_or(fallback)
}

/// One **device** pixel expressed in the canvas's DIP — the thickness of every
/// hairline the two controls draw (the day-name rule, the week-number divider).
///
/// This is the one legitimate use of `Canvas::scale` in a family, and it
/// *divides* by it: a rule is one physical pixel at 96 DPI and at 175 %, never
/// 1.75 DIP wide.
fn hairline(c: &dyn Canvas) -> f32 {
    1.0 / c.scale().max(0.01)
}

// ═══════════════════════════════════════════════════════════════════════════
// DateTimePicker
// ═══════════════════════════════════════════════════════════════════════════

/// `System.Windows.Forms.DateTimePicker`.
///
/// Owns only what it *adds* to `Control`; `BackColor`, `ForeColor`, `Text`,
/// `BackgroundImage*` are inherited and reached through `Deref`.
#[derive(Clone)]
pub struct DateTimePicker {
    control: ControlBase,

    // ── Value & bounds ───────────────────────────────────────────────────
    /// `Value` — the current date/time. Clamped into `[min_date, max_date]` on
    /// every set. Defaults to [`DEFAULT_TODAY`] at midnight (see that constant
    /// for why it is not `DateTime.Now`).
    value:    DateTime,
    /// `MinDate` — never below [`MIN_DATE`].
    min_date: Date,
    /// `MaxDate` — never above [`MAX_DATE`].
    max_date: Date,

    // ── Format ───────────────────────────────────────────────────────────
    /// `Format`.
    pub format:        DateTimePickerFormat,
    /// `CustomFormat` — consulted only when `format == Custom`.
    pub custom_format: Option<String>,

    // ── Editing chrome ───────────────────────────────────────────────────
    /// `ShowUpDown` — a spin box instead of a drop-down calendar.
    pub show_up_down:  bool,
    /// `ShowCheckBox` — a leading check box; when unchecked "no value is
    /// selected" and the text greys out.
    pub show_check_box: bool,
    /// `Checked` — meaningful only with `show_check_box`.
    pub checked:       bool,
    /// `DropDownAlign`.
    pub drop_down_align: LeftRightAlignment,
    /// `RightToLeftLayout`.
    pub right_to_left_layout: bool,

    // ── Calendar appearance ──────────────────────────────────────────────
    /// `CalendarFont` — role the drop-down calendar paints its dates with.
    pub calendar_font:             Option<FontRole>,
    /// `CalendarForeColor`.
    pub calendar_fore_color:       Option<D2D1_COLOR_F>,
    /// `CalendarMonthBackground`.
    pub calendar_month_background: Option<D2D1_COLOR_F>,
    /// `CalendarTitleBackColor`.
    pub calendar_title_back_color: Option<D2D1_COLOR_F>,
    /// `CalendarTitleForeColor`.
    pub calendar_title_fore_color: Option<D2D1_COLOR_F>,
    /// `CalendarTrailingForeColor`.
    pub calendar_trailing_fore_color: Option<D2D1_COLOR_F>,
}

impl Default for DateTimePicker {
    /// Catalogue defaults: `Format = Long`, `Checked = true`,
    /// `ShowUpDown = false`, `ShowCheckBox = false`, `DropDownAlign = Left`,
    /// `RightToLeftLayout = false`, `MinDate = 1753-01-01`,
    /// `MaxDate = 9998-12-31`, appearance colours ambient (`None`).
    fn default() -> Self {
        Self {
            control:  ControlBase::new(),
            value:    DateTime::at_midnight(DEFAULT_TODAY),
            min_date: MIN_DATE,
            max_date: MAX_DATE,
            format:   DateTimePickerFormat::Long,
            custom_format: None,
            show_up_down:  false,
            show_check_box: false,
            checked:  true,
            // Stated explicitly: `LeftRightAlignment` deliberately has no
            // `Default`, because the two properties using it disagree — the
            // catalogue gives `DateTimePicker.DropDownAlign` = Left, but
            // `UpDownBase.UpDownAlign` = Right. The default belongs to the
            // property, so each owner names its own.
            drop_down_align: LeftRightAlignment::Left,
            right_to_left_layout: false,
            calendar_font:             None,
            calendar_fore_color:       None,
            calendar_month_background: None,
            calendar_title_back_color: None,
            calendar_title_fore_color: None,
            calendar_trailing_fore_color: None,
        }
    }
}

impl DateTimePicker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn value(&self) -> DateTime {
        self.value
    }

    /// `Value = v`, clamped into `[MinDate, MaxDate]` (the toolkit throws when
    /// out of range; a value type is friendlier clamping — documented here).
    pub fn set_value(&mut self, v: DateTime) {
        let lo = DateTime::at_midnight(self.min_date);
        let hi = DateTime::at_midnight(self.max_date);
        self.value = v.clamp(lo, hi);
    }

    pub fn min_date(&self) -> Date {
        self.min_date
    }

    /// `MinDate = d`, never below the toolkit floor; drags `Value` up if needed.
    pub fn set_min_date(&mut self, d: Date) {
        self.min_date = d.clamp(MIN_DATE, self.max_date);
        let v = self.value;
        self.set_value(v);
    }

    pub fn max_date(&self) -> Date {
        self.max_date
    }

    /// `MaxDate = d`, never above the toolkit ceiling; drags `Value` down.
    pub fn set_max_date(&mut self, d: Date) {
        self.max_date = d.clamp(self.min_date, MAX_DATE);
        let v = self.value;
        self.set_value(v);
    }

    /// The pattern that `Format` currently selects, resolving `Long`/`Short`/
    /// `Time` against the culture and `Custom` against `custom_format`.
    pub fn effective_pattern<'a>(&'a self, names: &'a Names) -> &'a str {
        match self.format {
            DateTimePickerFormat::Long => names.long_date,
            DateTimePickerFormat::Short => names.short_date,
            DateTimePickerFormat::Time => names.long_time,
            DateTimePickerFormat::Custom => self.custom_format.as_deref().unwrap_or(names.long_date),
        }
    }

    /// The text the control displays for its current value — the same string
    /// `Text` would return.
    pub fn display_text(&self, names: &Names) -> String {
        format_custom(self.value, self.effective_pattern(names), names)
    }
}

impl DateTime {
    /// Clamp into an inclusive `[lo, hi]` range.
    fn clamp(self, lo: DateTime, hi: DateTime) -> DateTime {
        if self < lo {
            lo
        } else if self > hi {
            hi
        } else {
            self
        }
    }
}

// Metrics for the picker box, in DIP. Kept as named constants so the paint code
// reads as geometry, not magic numbers.
//
// They are used AS-IS: the renderer calls `ID2D1DeviceContext::SetDpi`, so the
// Direct2D coordinate space is already DIP and scaling by `c.scale()` here would
// apply the DPI factor a second time (1.75× too large at 175 %). `c.scale()` is
// only ever right for choosing a PHYSICAL-pixel thickness, where one divides by
// it — and `stroke_rounded` already draws its hairline that way.
const PICKER_HEIGHT:   f32 = 23.0;
const PICKER_PAD_X:    f32 = 6.0;
const PICKER_GAP:      f32 = 4.0;
/// The button allowance used **when measuring**. The paint uses the real
/// `SM_CXVSCROLL` from [`crate::system::SystemMetrics`] — the metric the toolkit
/// sizes a combo/spin button with — but `preferred_size` takes a bare `Canvas`
/// and cannot reach it (see [`format_for`]). The two agree to a pixel at 96 DPI
/// (17 vs 18), and every picker on the sheet is given an explicit `Width`.
const BUTTON_W:        f32 = 18.0;
const CHECK_W:         f32 = 20.0;
const CHECK_BOX_SIDE:  f32 = 13.0;
/// Icon sizes passed to `Canvas::vector_icon` (it centres the glyph in the rect).
const DROPDOWN_GLYPH:  f32 = 12.0;
const SPINNER_GLYPH:   f32 = 9.0;
const CHECK_GLYPH:     f32 = 10.0;

impl Control for DateTimePicker {
    fn control(&self) -> &ControlBase {
        &self.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }

    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        let fmt = format_for(c, self.control.font.unwrap_or(FontRole::Body));
        let text = self.display_text(&FR);
        let mut w = c.measure(&text, fmt) + PICKER_PAD_X * 2.0 + BUTTON_W;
        if self.show_check_box {
            w += CHECK_W;
        }
        Size::new(w.ceil(), PICKER_HEIGHT)
    }

    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        let visuals = c.visuals();
        let colors = &visuals.colors;

        // The field is a `Fixed3D` well: the window colour under a SUNKEN
        // two-ring bevel, exactly what a `TextBox` paints and what the native
        // `DateTimePicker` sits in. `BackColor`, if set, wins.
        let back = or_system(self.control.back_color, colors.window);
        c.fill_rect(&bounds, &back);
        let field = c.draw_edge(&bounds, Border3DStyle::Sunken, Border3DSide::ALL);

        // The button band — the drop-down, or the up/down spinner under
        // `ShowUpDown` — is one vertical-scroll-bar wide, the metric the toolkit
        // sizes a combo button with (`SystemInformation.VerticalScrollBarWidth`).
        // It sits at the trailing edge, which is the LEFT edge under RTL.
        let width = (field.right - field.left).max(0.0);
        let btn_w = visuals.metrics.vertical_scroll_width.min(width);
        let (btn, text_rect) = if self.right_to_left_layout {
            (
                Rect::new(field.left, field.top, field.left + btn_w, field.bottom),
                Rect::new(field.left + btn_w, field.top, field.right, field.bottom),
            )
        } else {
            (
                Rect::new(field.right - btn_w, field.top, field.right, field.bottom),
                Rect::new(field.left, field.top, field.right - btn_w, field.bottom),
            )
        };

        // A control-faced, RAISED button — `DrawFrameControl(DFCS_SCROLLCOMBOBOX)`
        // for the drop-down, two stacked ones for the spinner. The chevrons stay
        // VECTOR icons, never text: the UI face has no arrow glyphs, so a
        // character like « ▾ » would render as a tofu box.
        let button = |r: &Rect, icon: &'static str, glyph: f32| {
            c.fill_rect(r, &colors.control);
            c.draw_edge(r, Border3DStyle::Raised, Border3DSide::ALL);
            c.vector_icon(icon, r, glyph, &colors.control_text);
        };
        if self.show_up_down {
            let mid = (btn.top + btn.bottom) * 0.5;
            button(&Rect::new(btn.left, btn.top, btn.right, mid), "ChevronUp", SPINNER_GLYPH);
            button(&Rect::new(btn.left, mid, btn.right, btn.bottom), "ChevronDown", SPINNER_GLYPH);
        } else {
            button(&btn, "ChevronDown", DROPDOWN_GLYPH);
        }

        // Optional leading check box — the toolkit's own `DFCS_BUTTONCHECK`: a
        // sunken well on the window colour with a black tick. When unchecked
        // "no value is selected", so the value greys out (documented WinForms
        // behaviour) while still being shown.
        let mut content = text_rect;
        let value_enabled = !self.show_check_box || self.checked;
        if self.show_check_box {
            let cy = (content.top + content.bottom) * 0.5;
            let cb = Rect::new(
                content.left + PICKER_PAD_X,
                cy - CHECK_BOX_SIDE * 0.5,
                content.left + PICKER_PAD_X + CHECK_BOX_SIDE,
                cy + CHECK_BOX_SIDE * 0.5,
            );
            c.fill_rect(&cb, &colors.window);
            c.draw_edge(&cb, Border3DStyle::Sunken, Border3DSide::ALL);
            if self.checked {
                // The tick, too, is a vector icon rather than a « ✓ » character.
                c.vector_icon("Check", &cb, CHECK_GLYPH, &colors.window_text);
            }
            content = Rect::new(cb.right + PICKER_GAP, content.top, content.right, content.bottom);
        }

        // The formatted value, in the system UI font.
        let text = self.display_text(&FR);
        let colour = if value_enabled {
            or_system(self.control.fore_color, colors.window_text)
        } else {
            colors.gray_text
        };
        let inner = Rect::new(
            content.left + PICKER_PAD_X,
            content.top,
            content.right - PICKER_PAD_X,
            content.bottom,
        );
        c.text_ellipsis(&text, &inner, &visuals.fonts.message, &colour);
    }

    fn type_name(&self) -> &'static str {
        "DateTimePicker"
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// MonthCalendar
// ═══════════════════════════════════════════════════════════════════════════

/// `System.Windows.Forms.MonthCalendar`.
///
/// Owns the calendar-specific state; `BackColor`, `ForeColor`, `Text`, `Size`
/// and `BackgroundImage*` are inherited from `Control` through `Deref`.
#[derive(Clone)]
pub struct MonthCalendar {
    control: ControlBase,

    // ── Selection ────────────────────────────────────────────────────────
    /// `SelectionStart` — kept ≤ `selection_end` and inside `[min, max]`.
    selection_start: Date,
    /// `SelectionEnd`.
    selection_end:   Date,
    /// `MaxSelectionCount` — the range may span at most this many days.
    pub max_selection_count: i32,

    // ── Bounds ───────────────────────────────────────────────────────────
    min_date: Date,
    max_date: Date,

    // ── Layout / behaviour ───────────────────────────────────────────────
    /// `FirstDayOfWeek`.
    pub first_day_of_week: Day,
    /// `CalendarDimensions` — the grid of months.
    pub calendar_dimensions: CalendarDimensions,
    /// `ScrollChange` — months scrolled per next/prev click. `0` means "the
    /// number of months currently shown" (the toolkit's documented default).
    pub scroll_change: i32,
    /// `RightToLeftLayout`.
    pub right_to_left_layout: bool,

    // ── Today ────────────────────────────────────────────────────────────
    today_date:     Date,
    /// `TodayDateSet` (get-only in WinForms) — whether `TodayDate` was set
    /// explicitly rather than taken from the clock.
    today_date_set: bool,

    // ── Show flags ───────────────────────────────────────────────────────
    /// `ShowToday`.
    pub show_today: bool,
    /// `ShowTodayCircle`.
    ///
    /// **Colour deviation, recorded here rather than left to be read off the
    /// pixels.** The native control hard-codes `Color.Red` for this ring (and
    /// for the today swatch in the footer). [`crate::system::Visuals`] carries
    /// only what `GetSysColor` answers, and no system colour is red — inventing
    /// one would be exactly the hard-coding `system.rs` exists to prevent. So
    /// both take `Highlight`, which is what the *themed* control marks today
    /// with and what the reference sheet's swatch is painted in. Over a selected
    /// day (also `Highlight`) the ring would vanish, so it flips to
    /// `HighlightText` there.
    pub show_today_circle: bool,
    /// `ShowWeekNumbers`.
    pub show_week_numbers: bool,

    // ── Bolded dates ─────────────────────────────────────────────────────
    /// `BoldedDates` — specific dates.
    pub bolded_dates: Vec<Date>,
    /// `AnnuallyBoldedDates` — recur every year; only month+day are significant.
    pub annually_bolded_dates: Vec<Date>,
    /// `MonthlyBoldedDates` — recur every month; only the day is significant.
    pub monthly_bolded_dates: Vec<Date>,

    // ── Colours ──────────────────────────────────────────────────────────
    /// `TitleBackColor`.
    pub title_back_color: Option<D2D1_COLOR_F>,
    /// `TitleForeColor`.
    pub title_fore_color: Option<D2D1_COLOR_F>,
    /// `TrailingForeColor`.
    pub trailing_fore_color: Option<D2D1_COLOR_F>,
}

impl Default for MonthCalendar {
    /// Catalogue defaults: `MaxSelectionCount = 7`, `ShowToday = true`,
    /// `ShowTodayCircle = true`, `ShowWeekNumbers = false`,
    /// `FirstDayOfWeek = Default`, `ScrollChange = 0`,
    /// `CalendarDimensions = 1×1`, `RightToLeftLayout = false`,
    /// `MinDate = 1753-01-01`, `MaxDate = 9998-12-31`. Selection collapses on
    /// today; `TodayDateSet = false`.
    fn default() -> Self {
        Self {
            control: ControlBase::new(),
            selection_start: DEFAULT_TODAY,
            selection_end:   DEFAULT_TODAY,
            max_selection_count: 7,
            min_date: MIN_DATE,
            max_date: MAX_DATE,
            first_day_of_week: Day::Default,
            calendar_dimensions: CalendarDimensions::default(),
            scroll_change: 0,
            right_to_left_layout: false,
            today_date: DEFAULT_TODAY,
            today_date_set: false,
            show_today: true,
            show_today_circle: true,
            show_week_numbers: false,
            bolded_dates: Vec::new(),
            annually_bolded_dates: Vec::new(),
            monthly_bolded_dates: Vec::new(),
            title_back_color: None,
            title_fore_color: None,
            trailing_fore_color: None,
        }
    }
}

impl MonthCalendar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn selection_start(&self) -> Date {
        self.selection_start
    }
    pub fn selection_end(&self) -> Date {
        self.selection_end
    }

    /// `SelectionRange` — the pair, always with `start ≤ end`.
    pub fn selection_range(&self) -> SelectionRange {
        SelectionRange { start: self.selection_start, end: self.selection_end }
    }

    /// Set the whole range at once, applying every rule the toolkit enforces:
    /// order the ends, clamp both into `[MinDate, MaxDate]`, then cap the span
    /// at `MaxSelectionCount` days (WinForms throws instead of capping; the port
    /// caps and documents it). The anchor (`start`) is kept; `end` moves.
    pub fn set_selection_range(&mut self, start: Date, end: Date) {
        let (mut s, mut e) = if start <= end { (start, end) } else { (end, start) };
        s = s.clamp(self.min_date, self.max_date);
        e = e.clamp(self.min_date, self.max_date);
        let max = self.max_selection_count.max(1) as i64;
        if e.to_days() - s.to_days() + 1 > max {
            e = s.add_days(max - 1);
            // The cap must still respect MaxDate.
            e = e.clamp(self.min_date, self.max_date);
        }
        self.selection_start = s;
        self.selection_end = e;
    }

    /// `SelectionStart = d` — collapses the range onto a single day, then re-runs
    /// the range rules.
    pub fn set_selection_start(&mut self, d: Date) {
        self.set_selection_range(d, d);
    }

    /// `SelectionEnd = d` — extends from the current start.
    pub fn set_selection_end(&mut self, d: Date) {
        let s = self.selection_start;
        self.set_selection_range(s, d);
    }

    pub fn min_date(&self) -> Date {
        self.min_date
    }
    pub fn set_min_date(&mut self, d: Date) {
        self.min_date = d.clamp(MIN_DATE, self.max_date);
        let (s, e) = (self.selection_start, self.selection_end);
        self.set_selection_range(s, e);
    }

    pub fn max_date(&self) -> Date {
        self.max_date
    }
    pub fn set_max_date(&mut self, d: Date) {
        self.max_date = d.clamp(self.min_date, MAX_DATE);
        let (s, e) = (self.selection_start, self.selection_end);
        self.set_selection_range(s, e);
    }

    pub fn today_date(&self) -> Date {
        self.today_date
    }
    /// `TodayDate = d` — also flips `TodayDateSet` true, as the toolkit does.
    pub fn set_today_date(&mut self, d: Date) {
        self.today_date = d.clamp(self.min_date, self.max_date);
        self.today_date_set = true;
    }
    pub fn today_date_set(&self) -> bool {
        self.today_date_set
    }

    /// The concrete first day of week, 0 = Monday … 6 = Sunday.
    pub fn first_day(&self) -> u8 {
        self.first_day_of_week.resolved()
    }

    /// Whether `date` should paint bold, considering all three bold lists
    /// (`BoldedDates` exact, `AnnuallyBoldedDates` by month+day,
    /// `MonthlyBoldedDates` by day).
    pub fn is_bold(&self, date: Date) -> bool {
        self.bolded_dates.contains(&date)
            || self.annually_bolded_dates.iter().any(|b| b.month == date.month && b.day == date.day)
            || self.monthly_bolded_dates.iter().any(|b| b.day == date.day)
    }

    /// `SingleMonthSize` (get-only) — the DIP size one month needs, before the
    /// week-number column and today row. A nominal, at 96 DPI, matching the
    /// reference sheet's proportions.
    pub fn single_month_size(&self) -> Size {
        Size::new(
            7.0 * MC_CELL_W + MC_MONTH_PAD * 2.0,
            MC_TITLE_H + MC_DOW_H + 6.0 * MC_CELL_H + MC_MONTH_PAD,
        )
    }
}

// Metrics for the calendar, in DIP — used as-is, for the same reason as the
// picker's: the canvas is already in DIP (the renderer calls `SetDpi`), so these
// must NOT be multiplied by `c.scale()`.
const MC_CELL_W:       f32 = 26.0;
const MC_CELL_H:       f32 = 22.0;
const MC_TITLE_H:      f32 = 26.0;
const MC_DOW_H:        f32 = 18.0;
const MC_WEEKNUM_W:    f32 = 24.0;
const MC_TODAY_H:      f32 = 24.0;
const MC_MONTH_PAD:    f32 = 6.0;
/// The today ring's stroke width. Not a corner radius: the ring is genuinely
/// round (`ShowTodayCircle`), which is the one curve the native control draws.
const MC_TODAY_RING:   f32 = 1.5;
const MC_NAV_W:        f32 = 22.0;
const MC_NAV_GLYPH:    f32 = 12.0;
const MC_SWATCH_W:     f32 = 28.0;
const MC_SWATCH_INSET: f32 = 3.0;

impl MonthCalendar {
    /// Paint one month into `rect`. Factored out so a multi-month
    /// `CalendarDimensions` grid just tiles this.
    fn paint_month(
        &self,
        c: &dyn ControlCanvas,
        rect: Rect,
        year: i32,
        month: u8,
        show_today_row: bool,
    ) {
        let visuals = c.visuals();
        let colors = &visuals.colors;
        let fonts = &visuals.fonts;
        let first_day = self.first_day();
        let rule = hairline(c);

        // Title band: month + year, prev/next arrows. The band sits on the
        // calendar's own ground rather than a coloured caption bar — see the
        // note on `title_back_color` below.
        let title = Rect::new(rect.left, rect.top, rect.right, rect.top + MC_TITLE_H);
        let title_back = or_system(self.title_back_color, colors.window);
        c.fill_rect(&title, &title_back);
        let title_fore = or_system(self.title_fore_color, colors.window_text);
        let caption = format!("{} {}", FR.months[month as usize], year);
        c.text(&caption, &title, &fonts.message, &title_fore, true);
        let prev = Rect::new(title.left, title.top, title.left + MC_NAV_W, title.bottom);
        let next = Rect::new(title.right - MC_NAV_W, title.top, title.right, title.bottom);
        // Vector icons, not « ‹ » / « › » characters: the UI face has no arrow
        // glyphs, so a character would render as a tofu box.
        c.vector_icon("ChevronLeft", &prev, MC_NAV_GLYPH, &title_fore);
        c.vector_icon("ChevronRight", &next, MC_NAV_GLYPH, &title_fore);

        // The columns start after an optional week-number gutter.
        let grid_left = if self.show_week_numbers {
            rect.left + MC_WEEKNUM_W
        } else {
            rect.left + MC_MONTH_PAD
        };
        let cell_w = MC_CELL_W;
        let cell_h = MC_CELL_H;
        let grid_right = grid_left + 7.0 * cell_w;

        // Day-of-week header, rotated so column 0 is `first_day`.
        let dow_top = title.bottom;
        for col in 0..7u8 {
            let idx = ((first_day + col) % 7) as usize;
            let cx = grid_left + col as f32 * cell_w;
            let hr = Rect::new(cx, dow_top, cx + cell_w, dow_top + MC_DOW_H);
            c.text(FR.days_abbr[idx], &hr, &fonts.message, &colors.window_text, true);
        }

        // The 6×7 grid, under the hairline rule the native control draws between
        // the day names and the first week.
        let grid_top = dow_top + MC_DOW_H;
        c.fill_rect(
            &Rect::new(grid_left, grid_top, grid_right, grid_top + rule),
            &colors.control_light,
        );
        let grid_bottom = grid_top + 6.0 * cell_h;
        // The week-number gutter is separated from the days by a full-height
        // divider, not by whitespace.
        if self.show_week_numbers {
            c.fill_rect(
                &Rect::new(grid_left - rule, grid_top, grid_left, grid_bottom),
                &colors.control_text,
            );
        }

        let cells = month_grid(year, month, first_day);
        let cal_fore = or_system(self.control.fore_color, colors.window_text);
        let trailing = or_system(self.trailing_fore_color, colors.gray_text);

        for row in 0..6usize {
            let row_top = grid_top + row as f32 * cell_h;

            // Week-number gutter. A row made ENTIRELY of trailing days carries
            // no number — the leading week `month_grid` now always shows would
            // otherwise label a week of the previous month, which the reference
            // sheet leaves blank.
            let row_cells = &cells[row * 7..row * 7 + 7];
            if self.show_week_numbers && row_cells.iter().any(|cell| cell.in_month) {
                let wr = Rect::new(rect.left, row_top, grid_left - rule, row_top + cell_h);
                let wk = iso_week(row_cells[0].date);
                c.text(&wk.to_string(), &wr, &fonts.message, &colors.window_text, true);
            }

            for col in 0..7usize {
                let cell = cells[row * 7 + col];
                let cx = grid_left + col as f32 * cell_w;
                let cr = Rect::new(cx, row_top, cx + cell_w, row_top + cell_h);

                // Selection fill spans the whole selected inclusive range, and
                // the days inside it invert to `HighlightText`.
                let in_sel = (self.selection_start..=self.selection_end).contains(&cell.date);
                if in_sel {
                    c.fill_rect(&cr, &colors.highlight);
                }

                // The today ring. The native control hard-codes `Color.Red`
                // here; `Visuals` publishes no such colour (it reads only what
                // `GetSysColor` answers), so the ring takes `Highlight` — which
                // is what the themed control marks today with, and what the
                // reference sheet's today swatch is drawn in. Over a SELECTED
                // day that would be invisible, so it flips to `HighlightText`.
                if self.show_today_circle && cell.date == self.today_date {
                    let ring = if in_sel { colors.highlight_text } else { colors.highlight };
                    c.stroke_rounded_w(&cr.inflate(-1.0, -1.0), cell_h * 0.5, &ring, MC_TODAY_RING);
                }

                let colour = if in_sel {
                    colors.highlight_text
                } else if !cell.in_month {
                    trailing
                } else {
                    cal_fore
                };
                let fmt =
                    if self.is_bold(cell.date) { &fonts.message_bold } else { &fonts.message };
                c.text(&cell.date.day.to_string(), &cr, fmt, &colour, true);
            }
        }

        // Today footer: the today swatch + « Aujourd'hui : dd/MM/yyyy ».
        if show_today_row && self.show_today {
            let fr = Rect::new(rect.left, grid_bottom, rect.right, grid_bottom + MC_TODAY_H);
            let swatch = Rect::new(
                fr.left + MC_MONTH_PAD,
                fr.top + MC_SWATCH_INSET,
                fr.left + MC_MONTH_PAD + MC_SWATCH_W,
                fr.bottom - MC_SWATCH_INSET,
            );
            // Same colour as the today ring, for the same reason.
            c.stroke_rect(&swatch, &colors.highlight);
            let label = format!(
                "Aujourd'hui : {}",
                format_custom(DateTime::at_midnight(self.today_date), FR.short_date, &FR)
            );
            let lr = Rect::new(swatch.right + MC_MONTH_PAD, fr.top, fr.right, fr.bottom);
            c.text(&label, &lr, &fonts.message, &colors.window_text, false);
        }
    }
}

impl Control for MonthCalendar {
    fn control(&self) -> &ControlBase {
        &self.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }

    fn preferred_size(&self, _c: &dyn Canvas) -> Size {
        let single = self.single_month_size();
        let dims = self.calendar_dimensions;
        let mut w = single.width * dims.columns as f32;
        if self.show_week_numbers {
            w += MC_WEEKNUM_W * dims.columns as f32;
        }
        let mut h = single.height * dims.rows as f32;
        if self.show_today {
            h += MC_TODAY_H;
        }
        Size::new(w.ceil(), h.ceil())
    }

    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        let colors = &c.visuals().colors;

        // The calendar's ground is the WINDOW colour under a flat one-pixel
        // frame — square, like every other surface in the toolkit.
        // `BackColor` (which `MonthCalendar` re-surfaces) wins.
        let back = or_system(self.control.back_color, colors.window);
        c.fill_rect(&bounds, &back);
        c.stroke_rect(&bounds, &colors.control_dark);

        // Tile the month grid. The first displayed month comes from the
        // selection start; each subsequent tile is the following month.
        let dims = self.calendar_dimensions;
        let month_w = (bounds.right - bounds.left) / dims.columns.max(1) as f32;
        let footer_h = if self.show_today { MC_TODAY_H } else { 0.0 };
        let grid_bottom = bounds.bottom - footer_h;
        let month_h = (grid_bottom - bounds.top) / dims.rows.max(1) as f32;

        let base = self.selection_start;
        let mut ordinal = 0i32;
        for r in 0..dims.rows {
            for col in 0..dims.columns {
                // Advance `base` by `ordinal` months.
                let total = base.month as i32 - 1 + ordinal;
                let year = base.year + total.div_euclid(12);
                let month = (total.rem_euclid(12) + 1) as u8;
                let mr = Rect::new(
                    bounds.left + col as f32 * month_w,
                    bounds.top + r as f32 * month_h,
                    bounds.left + (col + 1) as f32 * month_w,
                    bounds.top + (r + 1) as f32 * month_h,
                );
                // Only the bottom-left month hosts the shared today footer.
                let host_today = r == dims.rows - 1 && col == 0;
                self.paint_month(c, mr, year, month, host_today);
                ordinal += 1;
            }
        }
    }

    fn type_name(&self) -> &'static str {
        "MonthCalendar"
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── The civil-date core ─────────────────────────────────────────────

    #[test]
    fn the_epoch_is_day_zero() {
        assert_eq!(Date::new(1970, 1, 1).to_days(), 0);
        assert_eq!(Date::from_days(0), Date::new(1970, 1, 1));
    }

    #[test]
    fn days_round_trip_across_centuries() {
        // A spread of dates including a pre-epoch one and a far-future one.
        for d in [
            Date::new(1753, 1, 1),
            Date::new(1899, 12, 31),
            Date::new(1900, 3, 1), // 1900 is NOT a leap year
            Date::new(2000, 2, 29), // 2000 IS a leap year
            Date::new(2026, 6, 15),
            Date::new(9998, 12, 31),
        ] {
            assert_eq!(Date::from_days(d.to_days()), d, "round-trip failed for {d:?}");
        }
    }

    #[test]
    fn leap_years_follow_the_gregorian_rule() {
        assert!(Date::is_leap(2000)); // divisible by 400
        assert!(!Date::is_leap(1900)); // divisible by 100, not 400
        assert!(Date::is_leap(2024));
        assert!(!Date::is_leap(2026));
        assert_eq!(Date::days_in_month(2024, 2), 29);
        assert_eq!(Date::days_in_month(2026, 2), 28);
        assert_eq!(Date::days_in_month(2026, 4), 30);
        assert_eq!(Date::days_in_month(2026, 12), 31);
    }

    #[test]
    fn weekday_is_monday_based() {
        // Known anchors: 2026-06-15 is a Monday; 2026-08-17 a Monday too.
        assert_eq!(Date::new(2026, 6, 15).weekday(), 0);
        assert_eq!(Date::new(2026, 6, 21).weekday(), 6); // Sunday
        assert_eq!(Date::new(1970, 1, 1).weekday(), 3); // Thursday
        // A pre-epoch date exercises the negative-serial branch.
        assert_eq!(Date::new(1969, 12, 29).weekday(), 0); // Monday
    }

    // ── Month grid geometry ─────────────────────────────────────────────

    #[test]
    fn grid_starts_on_the_chosen_first_day_monday() {
        // June 2026: the 1st is a Monday, so with first_day = Monday column 0
        // carries a Monday — and, per the rule below, that Monday is 25 May,
        // not the 1st.
        let g = month_grid(2026, 6, 0);
        assert_eq!(g[0].date.weekday(), 0);
        assert_eq!(g[7].date, Date::new(2026, 6, 1));
        assert!(g[7].in_month);
    }

    /// The bug the reference sheet caught and no unit test could: when the 1st
    /// falls on the first day of the week the naive offset is zero, and the port
    /// opened the grid flush on the 1st. The native control always shows the
    /// whole preceding week instead.
    ///
    /// June 2026 with `FirstDayOfWeek = Monday` is exactly the case that
    /// diverged — `07-datetime.png` opens on **25 May**.
    #[test]
    fn the_grid_always_opens_on_a_full_leading_week() {
        let g = month_grid(2026, 6, 0);
        assert_eq!(g[0].date, Date::new(2026, 5, 25), "the leading week must be shown in full");
        // That whole first row is trailing, and the 1st opens the SECOND row.
        assert!(g[..7].iter().all(|cell| !cell.in_month));
        assert_eq!(g[7].date, Date::new(2026, 6, 1));

        // The trailing edge needs no special rule — six rows of seven is six
        // whole weeks — but it must still close on one: 29–30 June, then 1–5
        // July, which is the reference's last row.
        assert_eq!(g[35].date, Date::new(2026, 6, 29));
        assert_eq!(g[41].date, Date::new(2026, 7, 5));
        assert_eq!(g[41].date.weekday(), 6, "the grid must end on the day before first_day");
    }

    /// The rule holds for EVERY first-day choice and every month, not just the
    /// one that exposed it: the grid never opens on the 1st, always closes on a
    /// whole week, and always contains the whole month.
    #[test]
    fn no_month_ever_opens_flush_on_its_first_day() {
        for first_day in 0..7u8 {
            for month in 1..=12u8 {
                let g = month_grid(2026, month, first_day);
                assert!(!g[0].in_month, "{month}/{first_day}: the leading row must be trailing");
                assert_eq!(g[0].date.weekday(), first_day);
                let days = Date::days_in_month(2026, month) as usize;
                let shown = g.iter().filter(|cell| cell.in_month).count();
                assert_eq!(shown, days, "{month}/{first_day}: the whole month must fit");
            }
        }
    }

    #[test]
    fn grid_starts_on_the_chosen_first_day_sunday() {
        // Same month, first_day = Sunday: column 0 is the Sunday before the 1st,
        // i.e. 2026-05-31, a trailing day.
        let g = month_grid(2026, 6, 6);
        assert_eq!(g[0].date, Date::new(2026, 5, 31));
        assert!(!g[0].in_month);
        assert_eq!(g[0].date.weekday(), 6);
        // The 1st then lands in column 1.
        assert_eq!(g[1].date, Date::new(2026, 6, 1));
    }

    #[test]
    fn grid_is_always_six_full_weeks_and_contiguous() {
        let g = month_grid(2026, 2, 0); // February, a short month
        assert_eq!(g.len(), 42);
        for w in g.windows(2) {
            assert_eq!(w[1].date, w[0].date.add_days(1), "cells must be contiguous");
        }
    }

    #[test]
    fn every_first_day_choice_puts_its_day_in_column_zero() {
        for fd in 0..7u8 {
            let g = month_grid(2026, 6, fd);
            assert_eq!(g[0].date.weekday(), fd, "first cell must match first_day {fd}");
        }
    }

    // ── The format engine ───────────────────────────────────────────────

    fn sample() -> DateTime {
        DateTime::new(Date::new(2026, 6, 15), Time { hour: 14, minute: 30, second: 5 })
    }

    #[test]
    fn standard_patterns_match_the_reference_sheet() {
        let v = DateTime::new(Date::new(2026, 6, 15), Time { hour: 14, minute: 30, second: 0 });
        assert_eq!(format_custom(v, FR.short_date, &FR), "15/06/2026");
        assert_eq!(format_custom(v, FR.long_time, &FR), "14:30:00");
        assert_eq!(format_custom(v, FR.long_date, &FR), "lundi 15 juin 2026");
        assert_eq!(format_custom(v, "yyyy-MM-dd HH:mm", &FR), "2026-06-15 14:30");
    }

    #[test]
    fn each_supported_specifier_renders() {
        let v = sample();
        assert_eq!(format_custom(v, "d", &FR), "15");
        assert_eq!(format_custom(v, "dd", &FR), "15");
        assert_eq!(format_custom(v, "ddd", &FR), "lun.");
        assert_eq!(format_custom(v, "dddd", &FR), "lundi");
        assert_eq!(format_custom(v, "M", &FR), "6");
        assert_eq!(format_custom(v, "MM", &FR), "06");
        assert_eq!(format_custom(v, "MMM", &FR), "juin");
        assert_eq!(format_custom(v, "MMMM", &FR), "juin");
        assert_eq!(format_custom(v, "y", &FR), "26");
        assert_eq!(format_custom(v, "yy", &FR), "26");
        assert_eq!(format_custom(v, "yyyy", &FR), "2026");
        assert_eq!(format_custom(v, "H", &FR), "14");
        assert_eq!(format_custom(v, "HH", &FR), "14");
        assert_eq!(format_custom(v, "h", &FR), "2"); // 14h → 2pm
        assert_eq!(format_custom(v, "hh", &FR), "02");
        assert_eq!(format_custom(v, "m", &FR), "30");
        assert_eq!(format_custom(v, "mm", &FR), "30");
        assert_eq!(format_custom(v, "s", &FR), "5");
        assert_eq!(format_custom(v, "ss", &FR), "05");
        assert_eq!(format_custom(v, "tt", &FR), "PM");
        assert_eq!(format_custom(v, "t", &FR), "P");
    }

    #[test]
    fn twelve_hour_midnight_is_twelve() {
        let midnight = DateTime::at_midnight(Date::new(2026, 6, 15));
        assert_eq!(format_custom(midnight, "h:mm tt", &FR), "12:00 AM");
    }

    #[test]
    fn quotes_and_escapes_are_literal() {
        let v = sample();
        assert_eq!(format_custom(v, "'le' d", &FR), "le 15");
        assert_eq!(format_custom(v, "\\d d", &FR), "d 15");
    }

    #[test]
    fn unsupported_specifiers_are_emitted_verbatim_not_reinterpreted() {
        // f/F/g/K/z are documented as unsupported: they appear as their literal
        // letters, never silently mapped onto a supported field.
        let v = sample();
        assert_eq!(format_custom(v, "ffz", &FR), "ffz");
    }

    // ── DateTimePicker ──────────────────────────────────────────────────

    #[test]
    fn picker_defaults_match_the_catalogue() {
        let p = DateTimePicker::new();
        assert_eq!(p.format, DateTimePickerFormat::Long);
        assert!(p.checked);
        assert!(!p.show_up_down);
        assert!(!p.show_check_box);
        assert_eq!(p.drop_down_align, LeftRightAlignment::Left);
        assert!(!p.right_to_left_layout);
        assert_eq!(p.min_date(), MIN_DATE);
        assert_eq!(p.max_date(), MAX_DATE);
        assert!((p.min_date()..=p.max_date()).contains(&p.value().date));
    }

    #[test]
    fn picker_clamps_value_into_min_max() {
        let mut p = DateTimePicker::new();
        p.set_min_date(Date::new(2026, 1, 1));
        p.set_max_date(Date::new(2026, 12, 31));
        p.set_value(DateTime::at_midnight(Date::new(2030, 5, 5)));
        assert_eq!(p.value().date, Date::new(2026, 12, 31));
        p.set_value(DateTime::at_midnight(Date::new(2000, 1, 1)));
        assert_eq!(p.value().date, Date::new(2026, 1, 1));
    }

    #[test]
    fn picker_min_max_never_leave_the_toolkit_bounds() {
        let mut p = DateTimePicker::new();
        p.set_min_date(Date::new(1000, 1, 1)); // below the floor
        assert_eq!(p.min_date(), MIN_DATE);
        p.set_max_date(Date::new(12000, 1, 1)); // above the ceiling
        assert_eq!(p.max_date(), MAX_DATE);
    }

    // ── MonthCalendar ───────────────────────────────────────────────────

    #[test]
    fn calendar_defaults_match_the_catalogue() {
        let m = MonthCalendar::new();
        assert_eq!(m.max_selection_count, 7);
        assert!(m.show_today);
        assert!(m.show_today_circle);
        assert!(!m.show_week_numbers);
        assert_eq!(m.first_day_of_week, Day::Default);
        assert_eq!(m.scroll_change, 0);
        assert_eq!(m.calendar_dimensions, CalendarDimensions { columns: 1, rows: 1 });
        assert!(!m.right_to_left_layout);
        assert_eq!(m.min_date(), MIN_DATE);
        assert_eq!(m.max_date(), MAX_DATE);
        assert_eq!(m.selection_start(), m.selection_end());
        assert!(!m.today_date_set());
    }

    #[test]
    fn selection_range_is_capped_at_max_selection_count() {
        let mut m = MonthCalendar::new();
        m.max_selection_count = 7;
        // Ask for a 10-day range; it must clamp to 7 days from the start.
        m.set_selection_range(Date::new(2026, 6, 1), Date::new(2026, 6, 10));
        assert_eq!(m.selection_start(), Date::new(2026, 6, 1));
        assert_eq!(m.selection_end(), Date::new(2026, 6, 7));
        let span = m.selection_end().to_days() - m.selection_start().to_days() + 1;
        assert_eq!(span, 7);
    }

    #[test]
    fn selection_orders_its_ends_and_clamps_to_bounds() {
        let mut m = MonthCalendar::new();
        m.set_min_date(Date::new(2026, 6, 5));
        m.set_max_date(Date::new(2026, 6, 25));
        // Reversed and out of range: ordered, then clamped into the window.
        m.set_selection_range(Date::new(2026, 6, 30), Date::new(2026, 6, 1));
        assert_eq!(m.selection_start(), Date::new(2026, 6, 5));
        assert_eq!(m.selection_end(), Date::new(2026, 6, 25).min(Date::new(2026, 6, 5).add_days(6)));
    }

    #[test]
    fn today_date_flips_the_set_flag() {
        let mut m = MonthCalendar::new();
        assert!(!m.today_date_set());
        m.set_today_date(Date::new(2026, 8, 17));
        assert!(m.today_date_set());
        assert_eq!(m.today_date(), Date::new(2026, 8, 17));
    }

    #[test]
    fn calendar_dimensions_clamp_to_twelve_months() {
        assert_eq!(CalendarDimensions::clamped(1, 1), CalendarDimensions { columns: 1, rows: 1 });
        assert_eq!(CalendarDimensions::clamped(3, 4), CalendarDimensions { columns: 3, rows: 4 });
        // 4×4 = 16 > 12: rows shrink until the product fits.
        let d = CalendarDimensions::clamped(4, 4);
        assert!(d.columns as u16 * d.rows as u16 <= 12);
        // Zero is lifted to one.
        assert_eq!(CalendarDimensions::clamped(0, 0), CalendarDimensions { columns: 1, rows: 1 });
    }

    #[test]
    fn bold_matching_covers_all_three_lists() {
        let mut m = MonthCalendar::new();
        m.bolded_dates.push(Date::new(2026, 6, 15));
        m.annually_bolded_dates.push(Date::new(2000, 12, 25)); // every Dec 25
        m.monthly_bolded_dates.push(Date::new(2000, 1, 1)); // the 1st of any month
        assert!(m.is_bold(Date::new(2026, 6, 15))); // exact
        assert!(m.is_bold(Date::new(2030, 12, 25))); // annual, different year
        assert!(m.is_bold(Date::new(2026, 3, 1))); // monthly, different month
        assert!(!m.is_bold(Date::new(2026, 6, 16)));
    }

    #[test]
    fn iso_week_is_defined_and_monotonic_within_a_month() {
        // Week numbers increase by one down the rows of a month.
        let g = month_grid(2026, 6, 0);
        let w0 = iso_week(g[0].date);
        let w1 = iso_week(g[7].date);
        assert_eq!(w1, w0 + 1);
    }
}
