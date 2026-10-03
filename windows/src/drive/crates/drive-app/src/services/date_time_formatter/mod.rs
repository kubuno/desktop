//! Port of `Files.App/Services/DateTimeFormatter/*`:
//! Application (relative labels), System ("g", locale short date+time) and
//! Universal ("yyyy-MM-dd HH:mm:ss") formatters, driven by the
//! `GeneralSettingsService.DateTimeFormat` setting.
//!
//! The C# polymorphism (Application/System/UniversalDateTimeFormatter) is
//! replaced by a `match format` in `AbstractDateTimeFormatter`'s dispatch
//! (see `abstract_date_time_formatter`); the System/Universal arms therefore
//! stay inlined in `to_short_label` (see `deferred`).

pub mod abstract_date_time_formatter;
pub mod application_date_time_formatter;

pub use abstract_date_time_formatter::{
    current_language_name, time_span_label_unit, to_long_label, to_short_label,
};
pub use application_date_time_formatter::date_format_sample;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::settings::DateTimeFormat;
    use chrono::Local;

    #[test]
    fn universal_format_is_iso() {
        use chrono::TimeZone;
        let dt = Local.with_ymd_and_hms(2020, 1, 2, 3, 4, 5).unwrap();
        assert_eq!(to_short_label(dt, DateTimeFormat::Universal), "2020-01-02 03:04:05");
    }

    #[test]
    fn application_format_is_relative() {
        let dt = Local::now() - chrono::Duration::seconds(5);
        let label = to_short_label(dt, DateTimeFormat::Application);
        // French: "Il y a 5 secondes" / English fallback: "5 seconds ago".
        assert!(label.contains('5'), "{label}");
    }
}
