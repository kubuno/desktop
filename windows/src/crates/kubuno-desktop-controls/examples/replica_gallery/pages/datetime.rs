//! `07-datetime` — DateTimePicker, MonthCalendar.
//!
//! ## Two differences against `07-datetime.png` that are the HARNESS, not the port
//!
//! Both were investigated once against the real toolkit and found not to be port
//! defects. They are recorded here so the next person comparing the two sheets
//! does not spend the same afternoon on them.
//!
//! * **The calendar's selection.** The reference shows 15–21 June selected; this
//!   page shows 15 June alone. `MonthCalendar::set_selection_start` *collapses*
//!   the range onto one day (`SelectionStart = d` in WinForms sets both ends),
//!   which is what this page calls. The reference's seven days are what the
//!   native control had left over from its own `MaxSelectionCount`; the port's
//!   selection painting is exercised either way.
//! * **`ShowCheckBox` shows a different date.** The reference reads `lundi 17
//!   août` — a real clock — while the port reads `lundi 15 juin` from
//!   [`kubuno_desktop_controls::datetime::DEFAULT_TODAY`], the deterministic sentinel
//!   the library uses because a control is a pure value with no access to the
//!   time. The same sentinel is why the footer says « Aujourd'hui : 01/01/2000 »
//!   where the reference says 17/08/2026. Nothing about the picker's painting
//!   differs.

use kubuno_desktop_controls::datetime::{
    Date, DateTime, DateTimePicker, DateTimePickerFormat, MonthCalendar, Time,
};

use crate::sheet::{group, kid, Group, Sheet};

/// The reference's fixed instant: 15 June 2026, 14:30 — a Monday, which is what
/// makes the long format worth showing.
const WHEN: DateTime = DateTime::new(Date::new(2026, 6, 15), Time { hour: 14, minute: 30, second: 0 });

pub fn build() -> Sheet {
    Sheet::new(vec![formats(), with_check_box(), calendar()])
}

fn picker(format: DateTimePickerFormat) -> DateTimePicker {
    let mut p = DateTimePicker::new();
    p.set_value(WHEN);
    p.format = format;
    p
}

fn formats() -> Group {
    let mut time = picker(DateTimePickerFormat::Time);
    // `ShowUpDown` replaces the drop-down button with a spinner — the shape a
    // time-only picker takes.
    time.show_up_down = true;

    let mut custom = picker(DateTimePickerFormat::Custom);
    custom.custom_format = Some("yyyy-MM-dd HH:mm".to_string());

    group(
        "DateTimePicker — Format",
        320.0,
        vec![
            kid(picker(DateTimePickerFormat::Long)).w(260.0),
            kid(picker(DateTimePickerFormat::Short)).w(260.0),
            kid(time).w(260.0),
            kid(custom).w(260.0),
        ],
    )
}

/// An unchecked box means « no value selected », so the toolkit greys the text
/// out while still showing it.
fn with_check_box() -> Group {
    let mut p = picker(DateTimePickerFormat::Long);
    p.show_check_box = true;
    p.checked = false;
    group("DateTimePicker — ShowCheckBox", 320.0, vec![kid(p).w(260.0)])
}

fn calendar() -> Group {
    let mut m = MonthCalendar::new();
    m.set_selection_start(WHEN.date);
    m.show_today = true;
    m.show_week_numbers = true;
    group("MonthCalendar", 320.0, vec![kid(m)])
}
