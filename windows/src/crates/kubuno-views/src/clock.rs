//! Today's real local date.
//!
//! `kubuno_controls::datetime`'s calendar replica owns no clock — same "the
//! phase is a PARAMETER" rule `feedback.rs`'s `Spinner`/`range.rs`'s
//! `ProgressBar` document for animation — so nothing in `kubuno-ui`/
//! `kubuno-controls` ever sets `MonthCalendar`/`DatePicker`'s `TodayDate`
//! for a caller; it stays at `DEFAULT_TODAY` (`2000-01-01`, a deliberately
//! implausible sentinel — see that constant's own doc) until one calls
//! `set_today_date`. The gallery's own demo pages do this with a fixed
//! `const TODAY` for screenshot determinism, never with the real system
//! date; a real `.kbview` view wants the real date, which is what this
//! module supplies.

use kubuno_controls::datetime::Date;

/// The real local calendar date, read from the OS (`GetLocalTime`, not
/// `GetSystemTime`: a calendar `Date` is what the user's own clock and time
/// zone say today is, not UTC's). No caching — a `.kbview` view is already
/// rebuilt every frame (`XML_VIEWS.md` §0), and a `Date` is three `u16`s
/// read from one cheap, non-blocking syscall, not a hot path worth
/// memoizing. Only called from the `data`/`text` families (`<MonthCalendar>`/
/// `<DatePicker>`) — `#[allow(dead_code)]` so `--no-default-features`
/// (neither family compiled in) stays clippy-clean too.
#[allow(dead_code)]
pub(crate) fn today() -> Date {
    // SAFETY: this `windows`-crate wrapper owns its own local `SYSTEMTIME`
    // and returns it by value — no caller-supplied pointer/buffer, no
    // documented precondition beyond being callable from any thread, which
    // every frame-driven caller here already is.
    let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    Date::new(st.wYear as i32, st.wMonth as u8, st.wDay as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn today_is_a_plausible_recent_date_not_the_replica_sentinel() {
        let d = today();
        // Not a hard-coded expectation (this test must keep passing on any
        // future date) — just proof it is nowhere near `DEFAULT_TODAY`
        // (`kubuno_controls::datetime::DEFAULT_TODAY`, `2000-01-01`) and is
        // a structurally valid calendar date.
        assert!(d.year >= 2026, "expected a real, current year, got {}", d.year);
        assert!((1..=12).contains(&d.month));
        assert!((1..=31).contains(&d.day));
    }
}
