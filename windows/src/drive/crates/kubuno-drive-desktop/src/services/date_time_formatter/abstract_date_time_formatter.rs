//! AbstractDateTimeFormatter (mirror of `AbstractDateTimeFormatter.cs`).
//!
//! Common `ToShortLabel`/`ToLongLabel` dispatch on the
//! `GeneralSettingsService.DateTimeFormat` enum, years <= 1601 / >= 9999 guards,
//! `ToTimeSpanLabel` (grouping by date), plus the shared Win32 helpers
//! for locale formatting (`GetDateFormatEx`/`GetTimeFormatEx`) and the native
//! name of the current culture.

use chrono::{DateTime, Datelike, Local};

use crate::services::settings::DateTimeFormat;

use super::application_date_time_formatter::application_short_label;

/// `IDateTimeFormatter.ToShortLabel` for the given format.
pub fn to_short_label(dt: DateTime<Local>, format: DateTimeFormat) -> String {
    // AbstractDateTimeFormatter: years <= 1601 or >= 9999 -> " ".
    if dt.year() <= 1601 || dt.year() >= 9999 {
        return " ".into();
    }
    match format {
        DateTimeFormat::Application => application_short_label(dt),
        DateTimeFormat::System => {
            format!("{} {}", locale_date(dt, false), locale_time(dt))
        }
        DateTimeFormat::Universal => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
    }
}

/// `IDateTimeFormatter.ToLongLabel`: only the Application formatter
/// specializes it (long date "D" + time "t" + relative label if less than
/// 7 days ago); System and Universal fall back to the short label
/// (`AbstractDateTimeFormatter.ToLongLabel => ToShortLabel`).
pub fn to_long_label(dt: DateTime<Local>, format: DateTimeFormat) -> String {
    if dt.year() <= 1601 || dt.year() >= 9999 {
        return " ".into();
    }
    match format {
        DateTimeFormat::Application => {
            let elapsed = Local::now().signed_duration_since(dt);
            let base = format!("{} {}", locale_date(dt, true), locale_time(dt));
            if elapsed.num_days() < 7 && elapsed.num_seconds() >= 0 {
                format!("{base} {}", application_short_label(dt))
            } else {
                base
            }
        }
        _ => to_short_label(dt, format),
    }
}

/// Locale-formatted date via `GetDateFormatEx` (long = "D", short of "g").
pub(super) fn locale_date(dt: DateTime<Local>, long: bool) -> String {
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::Globalization::{GetDateFormatEx, DATE_LONGDATE, DATE_SHORTDATE};
    use chrono::Timelike;
    let st = SYSTEMTIME {
        wYear: dt.year() as u16,
        wMonth: dt.month() as u16,
        wDay: dt.day() as u16,
        wHour: dt.hour() as u16,
        wMinute: dt.minute() as u16,
        wSecond: dt.second() as u16,
        ..Default::default()
    };
    let flags = if long { DATE_LONGDATE } else { DATE_SHORTDATE };
    unsafe {
        let mut buf = [0u16; 128];
        let n = GetDateFormatEx(None, flags, Some(&st), None, Some(&mut buf), None);
        if n > 1 {
            return String::from_utf16_lossy(&buf[..(n as usize - 1)]);
        }
    }
    dt.format("%d/%m/%Y").to_string()
}

/// Locale-formatted short time via `GetTimeFormatEx` (the time part of "g").
pub(super) fn locale_time(dt: DateTime<Local>) -> String {
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::Globalization::{GetTimeFormatEx, TIME_NOSECONDS};
    use chrono::Timelike;
    let st = SYSTEMTIME {
        wYear: dt.year() as u16,
        wMonth: dt.month() as u16,
        wDay: dt.day() as u16,
        wHour: dt.hour() as u16,
        wMinute: dt.minute() as u16,
        wSecond: dt.second() as u16,
        ..Default::default()
    };
    unsafe {
        let mut buf = [0u16; 64];
        let n = GetTimeFormatEx(None, TIME_NOSECONDS, Some(&st), None, Some(&mut buf));
        if n > 1 {
            return String::from_utf16_lossy(&buf[..(n as usize - 1)]);
        }
    }
    dt.format("%H:%M").to_string()
}

/// Native display name of the current UI culture (`AppLanguageItem.Name`),
/// via `GetLocaleInfoEx(LOCALE_SNATIVEDISPLAYNAME)`.
pub fn current_language_name() -> String {
    use windows::core::HSTRING;
    use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_SNATIVEDISPLAYNAME};
    let culture = kubuno_drive_desktop_localization::culture();
    unsafe {
        let mut buf = [0u16; 256];
        let tag = HSTRING::from(culture);
        let n = GetLocaleInfoEx(
            windows::core::PCWSTR(tag.as_ptr()),
            LOCALE_SNATIVEDISPLAYNAME,
            Some(&mut buf),
        );
        if n > 1 {
            return String::from_utf16_lossy(&buf[..(n as usize - 1)]);
        }
    }
    culture.into()
}

/// Port of `AbstractDateTimeFormatter.ToTimeSpanLabel`, with the grouping
/// unit (`GroupByDateUnit`): the "Today / Yesterday / Earlier
/// this week / …" scale, then, after the fixed labels, Day groups by full
/// date, Month by month, Year by year. Returns (descending rank, label)
/// — the rank orders groups from most recent to oldest
/// (`SortIndexOverride`).
pub fn time_span_label_unit(
    t: std::time::SystemTime,
    unit: crate::services::settings::GroupByDateUnit,
) -> (i64, String) {
    use chrono::{DateTime, Datelike, Local};
    let tr = kubuno_drive_desktop_localization::tr;
    let time: DateTime<Local> = t.into();
    let now = Local::now();
    let week = |d: &DateTime<Local>| d.iso_week().year() * 100 + d.iso_week().week() as i32;
    let days = (now.date_naive() - time.date_naive()).num_days();

    if now.date_naive() < time.date_naive() {
        (1_000_000_006, tr("Future").to_string())
    } else if now.date_naive() == time.date_naive() {
        (1_000_000_005, tr("Today").to_string())
    } else if days == 1 {
        (1_000_000_004, tr("Yesterday").to_string())
    } else if unit == crate::services::settings::GroupByDateUnit::Day {
        // Grouping by day: the full date, calendar rank.
        (
            (time.year() as i64) * 10_000 + (time.month() as i64) * 100 + time.day() as i64,
            to_short_label(time, crate::services::settings::get().date_time_format),
        )
    } else if days <= 7 && week(&now) == week(&time) {
        (1_000_000_003, tr("EarlierThisWeek").to_string())
    } else if days <= 14 && week(&(now - chrono::Duration::days(7))) == week(&time) {
        (1_000_000_002, tr("LastWeek").to_string())
    } else if now.year() == time.year() && now.month() == time.month() {
        (1_000_000_001, tr("EarlierThisMonth").to_string())
    } else if (now.year() * 12 + now.month() as i32) - (time.year() * 12 + time.month() as i32) == 1
    {
        (1_000_000_000, tr("LastMonth").to_string())
    } else if unit == crate::services::settings::GroupByDateUnit::Month {
        // Grouping by month: "month year", calendar rank.
        (
            (time.year() as i64) * 10_000 + (time.month() as i64) * 100,
            format!("{} {}", month_name(time.month()), time.year()),
        )
    } else if now.year() == time.year() {
        (10_000_001, tr("EarlierThisYear").to_string())
    } else if now.year() - time.year() == 1 {
        (10_000_000, tr("LastYear").to_string())
    } else {
        (
            time.year() as i64,
            tr("YearN").replace("{0}", &time.year().to_string()),
        )
    }
}

/// The month name in the UI language (chrono only knows
/// English: goes through the system's `GetDateFormatEx`).
fn month_name(month: u32) -> String {
    use windows::core::PCWSTR;
    use windows::Win32::Globalization::GetDateFormatEx;
    // The 15th of the requested month, any year.
    let st = windows::Win32::Foundation::SYSTEMTIME {
        wYear: 2000,
        wMonth: month as u16,
        wDay: 15,
        ..Default::default()
    };
    let fmt: Vec<u16> = "MMMM".encode_utf16().chain([0]).collect();
    let mut buf = [0u16; 64];
    let n = unsafe {
        GetDateFormatEx(
            PCWSTR::null(),
            windows::Win32::Globalization::ENUM_DATE_FORMATS_FLAGS(0),
            Some(&st),
            PCWSTR(fmt.as_ptr()),
            Some(&mut buf),
            None,
        )
    };
    if n > 0 {
        String::from_utf16_lossy(&buf[..(n as usize - 1)])
    } else {
        month.to_string()
    }
}
