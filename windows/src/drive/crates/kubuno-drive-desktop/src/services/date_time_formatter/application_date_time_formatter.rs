//! ApplicationDateTimeFormatter (mirror of `ApplicationDateTimeFormatter.cs`).
//!
//! Relative labels under 7 days ("N minutes ago"…), long date
//! beyond that, and the format sample displayed in settings
//! (`GeneralViewModel.DateFormatSample`).

use chrono::{DateTime, Datelike, Local};

use crate::services::settings::DateTimeFormat;

use super::abstract_date_time_formatter::{locale_date, to_short_label};

/// `ApplicationDateTimeFormatter.ToShortLabel`: relative labels below 7 days,
/// long date ("D") beyond.
pub(super) fn application_short_label(dt: DateTime<Local>) -> String {
    let tr = kubuno_drive_desktop_localization::tr;
    let elapsed = Local::now().signed_duration_since(dt);
    let secs = elapsed.num_seconds();
    let days = elapsed.num_days();
    let hours = elapsed.num_hours();
    let minutes = elapsed.num_minutes();
    if days >= 7 || secs < 0 {
        locale_date(dt, true)
    } else if days >= 2 {
        tr("DaysAgo_Plural").replace("{0}", &days.to_string())
    } else if days >= 1 {
        tr("DaysAgo_Singular").into()
    } else if hours >= 2 {
        tr("HoursAgo_Plural").replace("{0}", &hours.to_string())
    } else if hours >= 1 {
        tr("HoursAgo_Singular").into()
    } else if minutes >= 2 {
        tr("MinutesAgo_Plural").replace("{0}", &minutes.to_string())
    } else if minutes >= 1 {
        tr("MinutesAgo_Singular").into()
    } else if secs >= 2 {
        tr("SecondsAgo_Plural").replace("{0}", &secs.to_string())
    } else if secs >= 1 {
        tr("SecondsAgo_Singular").into()
    } else {
        tr("Now").into()
    }
}

/// `GeneralViewModel.DateFormatSample`: "ex : {sample1}, {sample2}" with
/// sample1 = now - 5s and sample2 = Dec 31 (year-5) 14:30.
pub fn date_format_sample(format: DateTimeFormat) -> String {
    use chrono::TimeZone;
    let now = Local::now();
    let sample1 = now - chrono::Duration::seconds(5);
    let sample2 = Local
        .with_ymd_and_hms(now.year() - 5, 12, 31, 14, 30, 0)
        .single()
        .unwrap_or(now);
    let s1 = to_short_label(sample1, format);
    let s2 = to_short_label(sample2, format);
    kubuno_drive_desktop_localization::tr("DateFormatSample")
        .replace("{0}", &s1)
        .replace("{1}", &s2)
}
