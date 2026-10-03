//! Gallery page — the date family.
//!
//! Neither [`DatePicker`] nor [`MonthCalendar`] has a hand-written predecessor
//! in `drive-app-controls`, so there is no [`super::sheet::pair`] here: there is
//! nothing to sit beside. What the page shows instead is every state the family
//! can be in, painted from the same inputs the unit tests use:
//!
//! * the field at rest, hovered, with a keyboard-visible focus and its edited
//!   segment highlighted, open (`kb-field-focus--on`), disabled, invalid, with
//!   an unchecked `ShowCheckBox`, as a `Time` / `ShowUpDown` spinner with a
//!   hovered spin button, and a long date squeezed into a narrow cell (the
//!   value ellipsises, it never spills);
//! * the panels: a dropped picker whose `MinDate` / `MaxDate` grey the days
//!   outside their window and whose keyboard cursor is ringed, a standalone
//!   calendar with everything the replica models turned on, and a Sunday-first
//!   one.
//!
//! The interactive column is the live version: the calendar takes the
//! keyboard (arrows, PageUp/PageDown, Home/End, Enter), the date field edits
//! by segment (←/→, ↑/↓, digits, Alt+↓ / F4 / Enter to open) and drops its
//! calendar in a `host::popup` placed against the SCREEN — it flips above the
//! field near the bottom of the monitor and may hang out of the window — and
//! the time field steps with the keyboard, the wheel or its spin buttons.

use std::cell::RefCell;

use kubuno_controls::datetime::{Date, DateTime, Day, Time};
use kubuno_controls::host::{self, Cursor, Frame};
use kubuno_ui::datetime::{DatePicker, FieldPart, HeaderPart, MonthCalendar, Segment};
use kubuno_ui::focus::FocusOpts;
use kubuno_ui::{Canvas, Rect, Widget, WidgetState};

use super::interact::{self, Live};
use super::sheet::{Page, MARGIN};

/// The gap between two cells on this page.
const GAP: f32 = 12.0;
/// The label under a field cell in the exposition.
const LABEL_H: f32 = 16.0;
/// The narrow cell the long date is squeezed into, to show the ellipsis.
const NARROW_W: f32 = 120.0;

/// The day every cell is seeded with. The replica has no clock — `TodayDate`
/// and `Value` are set explicitly, which is what [`DEFAULT_TODAY`] exists for —
/// so the page pins a date and every capture is reproducible.
///
/// [`DEFAULT_TODAY`]: kubuno_controls::datetime::DEFAULT_TODAY
const TODAY: Date = Date::new(2026, 8, 17);

/// A time field seeded at 14:30:00.
fn clock() -> DatePicker {
    let mut clock = DatePicker::time().on(TODAY);
    clock.set_value(DateTime::new(TODAY, Time { hour: 14, minute: 30, second: 0 }));
    clock
}

pub fn draw(c: &dyn Canvas, f: &Frame) {
    // The static exposition takes the window minus the interactive column, so it
    // lays out to the left of it rather than under it.
    let expo_w = f.size.0 - interact::PANEL_W();
    let mut page = Page::new(c, expo_w, f.size.1);
    let (mx, my) = f.mouse;
    let right = expo_w - MARGIN;
    let t = c.theme();

    // ── The field ────────────────────────────────────────────────────────────
    page.section("DatePicker");
    let top = page.caption(
        "chaque état est nommé sous son champ · la date longue est tronquée à 120 DIP",
    );

    let focus = WidgetState::REST.focused(true).focus_visible(true);
    let mut fields: Vec<(DatePicker, WidgetState, &str, Option<f32>)> = Vec::new();
    fields.push((DatePicker::short().on(TODAY), WidgetState::REST, "repos", None));
    fields.push((DatePicker::short().on(TODAY), WidgetState::REST.hot(true), "survol", None));
    let mut editing = DatePicker::short().on(TODAY);
    editing.active_segment = Some(Segment::Month);
    fields.push((editing, focus, "focus · mois", None));
    let mut open = DatePicker::short().on(TODAY);
    open.open = true;
    fields.push((open, WidgetState::REST, "ouvert", None));
    fields.push((DatePicker::short().on(TODAY), WidgetState::REST.disabled(true), "désactivé", None));
    let mut invalid = DatePicker::short().on(TODAY);
    invalid.invalid = true;
    fields.push((invalid.clone(), WidgetState::REST, "invalide", None));
    invalid.active_segment = Some(Segment::Day);
    fields.push((invalid, focus, "invalide · focus", None));

    // `ShowCheckBox` with `Checked = false` is the toolkit's « no value is
    // selected »: the value stays on screen and greys out.
    let mut unset = DatePicker::short().on(TODAY);
    unset.show_check_box = true;
    unset.checked = false;
    fields.push((unset, WidgetState::REST, "ShowCheckBox", None));

    // A time field IS the same control — `Format = Time` plus the spinner the
    // toolkit puts there instead of a calendar.
    let mut spin = clock();
    spin.hot_part = Some(FieldPart::SpinUp);
    spin.active_segment = Some(Segment::Minute);
    fields.push((spin, focus, "Time · minutes", None));

    // The long pattern in a cell narrower than its value: `flex-1 truncate`.
    fields.push((DatePicker::new().on(TODAY), WidgetState::REST, "Long, 120 DIP", Some(NARROW_W)));

    let field_h = fields[0].0.field_height();
    let mut x = MARGIN;
    let mut y = top;
    for (picker, state, label, forced_w) in &fields {
        let w = forced_w.unwrap_or_else(|| picker.measure(c).width);
        if x > MARGIN && x + w > right {
            x = MARGIN;
            y += field_h + LABEL_H + GAP;
        }
        let cell = Rect::new(x, y, x + w, y + field_h);
        // The pointer lights whichever field it is over — `field_at` is the
        // call a host makes before it dispatches the click.
        let hot = state.hot || picker.field_at(cell, mx, my).is_some();
        // Painted WITHOUT the panel: an « open » field in a row of fields
        // would otherwise drop its calendar over its neighbours.
        picker.paint_field_only(c, cell, state.hot(hot));
        c.text(
            label,
            &Rect::new(x, cell.bottom + 2.0, x + w.max(90.0), cell.bottom + 2.0 + LABEL_H),
            &c.formats().caption,
            &t.text_tertiary,
            false,
        );
        x += w.max(90.0) + GAP;
    }
    page.advance(y + field_h + LABEL_H - top);

    // ── The panels ───────────────────────────────────────────────────────────
    page.section("MonthCalendar");
    let top = page.caption(
        "panneau déroulé (MinDate/MaxDate, curseur clavier) · calendrier complet · dimanche",
    );

    // 1 — a dropped picker. Its window is a week wide on either side of the
    //     value, so a third of the month greys out; the keyboard cursor sits
    //     two days after the value, ringed.
    let mut picker = DatePicker::short()
        .between(Date::new(2026, 8, 10), Date::new(2026, 8, 24))
        .on(TODAY);
    picker.open_panel_from_keyboard();
    picker.calendar.move_focus_to(Date::new(2026, 8, 19));
    let size = picker.calendar.panel_size();
    let picker_cell = Rect::new(
        MARGIN,
        top,
        MARGIN + size.width,
        top + picker.field_height() + 4.0 + size.height,
    );
    let panel = picker.drop_down_rect(picker_cell);
    picker.calendar.hot_day =
        picker.day_at(picker_cell, mx, my).filter(|d| picker.calendar.is_selectable(*d));
    picker.calendar.hot_header = picker.calendar.header_at(panel, mx, my);
    let picker_state = WidgetState::REST.hot(picker.field_at(picker_cell, mx, my).is_some());
    picker.paint(c, picker_cell, picker_state);

    // 2 — a standalone calendar, with everything the replica models turned on.
    let mut cal = MonthCalendar::new().on(Date::new(2026, 8, 10));
    cal.first_day_of_week = Day::Monday;
    cal.show_week_numbers = true;
    cal.set_today_date(TODAY);
    // A five-day range, well under the toolkit's `MaxSelectionCount = 7`.
    cal.set_selection_range(Date::new(2026, 8, 10), Date::new(2026, 8, 14));
    // One of each bold list: an exact date, every 15 August, and the 1st of
    // any month.
    cal.bolded_dates.push(Date::new(2026, 8, 4));
    cal.annually_bolded_dates.push(Date::new(2000, 8, 15));
    cal.monthly_bolded_dates.push(Date::new(2000, 1, 1));

    // 3 — the same month with the week starting on SUNDAY, so the rotation of
    //     both the weekday strip and the grid is visible, and with no today
    //     footer: the two `Show*` flags a picker's panel turns off.
    let mut sunday = MonthCalendar::new().on(TODAY);
    sunday.first_day_of_week = Day::Sunday;
    sunday.show_today = false;
    sunday.set_today_date(TODAY);
    sunday.max_selection_count = 1;

    // Both panels are placed only while there is room to their right — the same
    // clamp the lists page applies to its cascaded submenu. The dropped panel's
    // shadow reaches ~26 DIP to its right, so the next panel keeps clear of it.
    let (_, _, shadow_r, _) = kubuno_ui::datetime::shadow_outset(&kubuno_ui::datetime::SHADOW_2XL);
    let mut left = picker_cell.right + shadow_r;
    let mut bottom = picker_cell.bottom;
    for panel in [&mut cal, &mut sunday] {
        let size = panel.panel_size();
        if left + size.width > right {
            break;
        }
        let cell = Rect::new(left, top, left + size.width, top + size.height);
        panel.hot_day = panel.day_at(cell, mx, my).filter(|d| panel.is_selectable(*d));
        panel.hot_header = panel.header_at(cell, mx, my);
        panel.paint(c, cell, WidgetState::REST);
        left = cell.right + GAP;
        bottom = bottom.max(cell.bottom);
    }
    page.advance(bottom - top);
}

/// What the interactive column remembers between frames.
struct Ui {
    /// Seeded on the first frame — a `Default` `MonthCalendar`/`DatePicker` has
    /// no `TODAY`, so the widgets are built once with the page's fixed date.
    ready:     bool,
    calendar:  MonthCalendar,
    picker:    DatePicker,
    clock:     DatePicker,
    prev_down: bool,
    /// Last frame's field rectangles: an open panel is hit-tested against the
    /// geometry it was shown with, BEFORE anything else sees the click.
    date_bounds: Rect,
    /// Whether each field held the focus last frame — losing it commits the
    /// typed digits and closes the panel.
    date_focused:  bool,
    clock_focused: bool,
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            ready:     false,
            calendar:  MonthCalendar::new(),
            picker:    DatePicker::new(),
            clock:     DatePicker::time(),
            prev_down: false,
            date_bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            date_focused:  false,
            clock_focused: false,
        }
    }
}

impl Ui {
    /// Builds the live controls once, pinned to [`TODAY`] so the column is
    /// reproducible frame to frame just like the static exposition.
    fn seed(&mut self) {
        if self.ready {
            return;
        }
        // A standalone calendar that selects ONE day: the toolkit's default cap
        // of 7 is for a range picker, and the today footer is dropped purely to
        // keep the column short enough for the two fields below it.
        let mut calendar = MonthCalendar::new().on(TODAY);
        calendar.max_selection_count = 1;
        calendar.show_today = false;
        calendar.set_today_date(TODAY);
        self.calendar = calendar;

        self.picker = DatePicker::short().on(TODAY);
        self.picker.calendar.set_today_date(TODAY);
        self.clock = clock();
        self.ready = true;
    }
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

/// Handles a click on a field: the check box toggles, a spin button steps the
/// selected segment, the value selects the segment under the pointer and —
/// when the field has a panel — toggles it, the way the web trigger does.
fn click_field(c: &dyn Canvas, picker: &mut DatePicker, bounds: Rect, x: f32, y: f32) {
    match picker.field_at(bounds, x, y) {
        Some(FieldPart::CheckBox) => picker.checked = !picker.checked,
        Some(FieldPart::SpinUp) => picker.step(1),
        Some(FieldPart::SpinDown) => picker.step(-1),
        Some(FieldPart::Value) => {
            if let Some(seg) = picker.segment_at(c, bounds, x, y) {
                picker.select_segment(seg);
            }
            if picker.has_panel() {
                picker.toggle_panel();
            }
        }
        None => {}
    }
}

/// The right-hand column: the same controls as the page, but live.
pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        ui.seed();
        let mut live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;
        ui.picker.viewport = Some(f.screen_area());

        // A click on the desktop or another application closes the panel,
        // exactly as the web closes it on an outside `mousedown`.
        if f.dismiss {
            ui.picker.close_panel();
        }

        // ── The open panel takes the pointer first ──────────────────────────
        // It is its own popup window, stacked over the column: a click inside
        // it is the calendar's and nothing underneath sees it; a click outside
        // it (and outside the field, which toggles it itself) closes it and
        // then goes on to whatever it landed on — the web has no backdrop.
        let mut over_panel = false;
        if ui.picker.open {
            let bounds = ui.date_bounds;
            let panel = ui.picker.drop_down_rect(bounds);
            let (px, py) = live.mouse;
            over_panel = panel.contains(px, py);
            if live.clicked {
                if over_panel {
                    match ui.picker.calendar.header_at(panel, px, py) {
                        Some(HeaderPart::Prev) => ui.picker.calendar.prev_month(),
                        Some(HeaderPart::Next) => ui.picker.calendar.next_month(),
                        Some(HeaderPart::Title) => {}
                        None => {
                            if let Some(date) = ui.picker.day_at(bounds, px, py) {
                                // `pick` refuses a day outside the bounds.
                                ui.picker.pick(date);
                            }
                        }
                    }
                    live.clicked = false;
                } else if !ui.picker.field_rect(bounds).contains(px, py) {
                    ui.picker.close_panel();
                }
            }
            if ui.picker.open {
                ui.picker.calendar.hot_day = ui
                    .picker
                    .day_at(bounds, px, py)
                    .filter(|d| ui.picker.calendar.is_selectable(*d));
                ui.picker.calendar.hot_header = ui.picker.calendar.header_at(panel, px, py);
                if ui.picker.calendar.day_at(panel, px, py).is_some_and(|d| !ui.picker.calendar.is_selectable(d)) {
                    host::set_cursor(Cursor::NotAllowed);
                }
            }
        }
        // Nothing under the popup lights up while the pointer is over it.
        if over_panel {
            live.mouse = (host::POINTER_AWAY, host::POINTER_AWAY);
        }
        let (mx, my) = live.mouse;

        let (left, mut y, right) = interact::panel(c, interact::panel_rect(f.size));

        // ── A standalone calendar ────────────────────────────────────────────
        y = interact::caption(
            c,
            left,
            right,
            y,
            "MonthCalendar — clic, flèches, PgPréc/PgSuiv, Début/Fin, Entrée",
        );
        let cal_size = ui.calendar.panel_size();
        let cal_rect = Rect::new(left, y, left + cal_size.width, y + cal_size.height);
        let cal_focus = live.focus("cal", cal_rect);
        if live.hit(cal_rect) {
            match ui.calendar.header_at(cal_rect, mx, my) {
                Some(HeaderPart::Prev) => ui.calendar.prev_month(),
                Some(HeaderPart::Next) => ui.calendar.next_month(),
                Some(HeaderPart::Title) => {}
                None => {
                    if let Some(date) = ui.calendar.day_at(cal_rect, mx, my) {
                        // `click_day` refuses a day outside `[MinDate, MaxDate]`.
                        ui.calendar.click_day(date);
                        ui.calendar.focus_day = Some(date);
                    }
                }
            }
        }
        if cal_focus.focused {
            ui.calendar.take_keys();
        }
        ui.calendar.hot_day = ui.calendar.day_at(cal_rect, mx, my).filter(|d| ui.calendar.is_selectable(*d));
        ui.calendar.hot_header = ui.calendar.header_at(cal_rect, mx, my);
        ui.calendar.paint(c, cal_rect, cal_focus.apply(WidgetState::REST));
        y = cal_rect.bottom + 8.0;
        let start = ui.calendar.selection_start();
        let cursor = ui.calendar.active_day();
        c.text(
            &format!(
                "Sélection : {:02}/{:02}/{} · curseur : {:02}/{:02}/{}",
                start.day, start.month, start.year, cursor.day, cursor.month, cursor.year
            ),
            &Rect::new(left, y, right, y + 18.0),
            &c.formats().caption,
            &c.theme().text_secondary,
            false,
        );
        y += 18.0 + 14.0;

        // ── A date picker: segment editing + a calendar in a popup ────────────
        y = interact::caption(
            c,
            left,
            right,
            y,
            "DatePicker — ←/→, ↑/↓, chiffres, Alt+↓ ouvre, Échap ferme",
        );
        let field_w = ui.picker.measure(c).width.max(160.0);
        let date_rect = Rect::new(left, y, left + field_w, y + ui.picker.field_height());
        let date_focus = live.focus_with("date", date_rect, FocusOpts::TEXT);
        if live.hit(date_rect) {
            click_field(c, &mut ui.picker, date_rect, mx, my);
        }
        if date_focus.focused {
            if date_focus.gained && ui.picker.active_segment.is_none() {
                ui.picker.move_segment(1);
            }
            ui.picker.take_input();
        } else if ui.date_focused {
            ui.picker.blur();
        }
        ui.date_focused = date_focus.focused;
        ui.date_bounds = date_rect;
        ui.picker.paint_field_only(c, date_rect, date_focus.apply(live.state(date_rect)));
        if let Some(pb) = ui.picker.popup_drop_down(date_rect) {
            interact::with_focus(|r| r.keep_focus_in(pb));
        }
        y = date_rect.bottom + 8.0;
        c.text(
            &format!("Valeur : {}", ui.picker.display_text()),
            &Rect::new(left, y, right, y + 18.0),
            &c.formats().caption,
            &c.theme().text_secondary,
            false,
        );
        y += 18.0 + 14.0;

        // ── A time picker: keyboard, wheel and spin buttons ───────────────────
        y = interact::caption(
            c,
            left,
            right,
            y,
            "TimePicker — ←/→, ↑/↓, chiffres, molette, flèches",
        );
        let clock_w = ui.clock.measure(c).width;
        let clock_rect = Rect::new(left, y, left + clock_w, y + ui.clock.field_height());
        let clock_focus = live.focus_with("time", clock_rect, FocusOpts::TEXT);
        if live.hit(clock_rect) {
            click_field(c, &mut ui.clock, clock_rect, mx, my);
        }
        // The wheel over a focused spinner steps it, as over a native one.
        // One step per notch; a notch rolled away from the user (web sign < 0)
        // steps up. Fractional touchpad travel is rounded per frame.
        let notches = if live.hover(clock_rect) { (-live.wheel.1).round() as i32 } else { 0 };
        if clock_focus.focused && notches != 0 {
            host::claim_wheel();
            ui.clock.step(notches);
        }
        if clock_focus.focused {
            if clock_focus.gained && ui.clock.active_segment.is_none() {
                ui.clock.move_segment(1);
            }
            ui.clock.take_input();
        } else if ui.clock_focused {
            ui.clock.blur();
        }
        ui.clock_focused = clock_focus.focused;
        ui.clock.hot_part = ui
            .clock
            .field_at(clock_rect, mx, my)
            .filter(|p| matches!(p, FieldPart::SpinUp | FieldPart::SpinDown));
        ui.clock.paint_field_only(c, clock_rect, clock_focus.apply(live.state(clock_rect)));
    });
}
