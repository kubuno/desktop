//! Gallery page — the button family.
//!
//! The static exposition (left) shows every component in every state the web
//! design system distinguishes: the predecessor parity pairs for `Button`,
//! `IconButton` and `Switch` (the old control and its rebuilt version get
//! rectangles of identical size from one canvas and one theme), then the web's
//! states the predecessor never had — focus-visible ring, loading, overflow —
//! and the labelled controls nested in a caller-painted card, where a wrong
//! ground or a spilling label shows at once. The exposition is taller than the
//! window: the tab panel it is painted into scrolls it.
//!
//! The interactive column (right) drives the same components live: mouse
//! (hover, press, click, cursor), keyboard through the focus ring (Tab /
//! Shift+Tab, Enter and Space on buttons, Space on check boxes and switches,
//! arrows inside the radio group with a roving Tab stop), and the web's
//! check / radio / switch transitions.

use std::cell::RefCell;

use kubuno_drive_desktop_app_controls::button as web_button;
use kubuno_drive_desktop_app_controls::switch as web_switch;
use kubuno_desktop_controls::enums::CheckState;
use kubuno_desktop_controls::host::{self, vk, Frame, Modifiers};
use kubuno_desktop_ui::buttons::{
    radio_arrow_target, radio_tab_stop, Button, CheckBox, IconButton, RadioButton, Size, State,
    Switch, SwitchSize, Transition, Variant, CHECK_TRANSITION_MS, SWITCH_TRANSITION_MS,
};
use kubuno_desktop_ui::{Canvas, FocusId, FocusOpts, Rect, Widget, WidgetState};

use super::interact::{self, Live};
use super::sheet::{self, Page, MARGIN};

/// The four states every parity row walks, in the order the paint's own ladder
/// resolves them.
const STATES: [State; 4] = [State::Rest, State::Hover, State::Active, State::Disabled];

/// The six variants, under the name the web's `VARIANT` map gives each.
const VARIANTS: [(Variant, &str); 6] = [
    (Variant::Primary, "primary"),
    (Variant::Secondary, "secondary"),
    (Variant::Ghost, "ghost"),
    (Variant::Text, "text"),
    (Variant::Danger, "danger"),
    (Variant::TextDanger, "textDanger"),
];

/// The label the variant grid carries — a real word rather than "Aa", because
/// the width is measured from the font and a short label would hide a padding
/// bug. The size row uses [`SHORT`] instead, purely so its six buttons fit.
const LABEL: &str = "Envoyer";
const SHORT: &str = "OK";

/// Gap between two cells of one strip, and the column reserved on the left for
/// a row's name.
const CELL_GAP: f32 = 8.0;
const ROW_LABEL: f32 = 96.0;

/// The variant grid's rhythm: six rows of one thing.
const TIGHT_GAP: f32 = 8.0;

/// How long the live « Enregistrer » button stays in its loading state.
const SAVE_DEMO_MS: u64 = 4000;

/// Rounds a pair cell UP to the next multiple of 4 DIP — see the comment in
/// [`variants_section`] for why the pixel diff depends on it.
fn quantise(w: f32) -> f32 {
    (w / 4.0).ceil() * 4.0
}

/// The [`WidgetState`] that makes the rebuilt primitive paint in `state`.
fn widget_state(state: State) -> WidgetState {
    match state {
        State::Rest => WidgetState::REST,
        State::Hover => WidgetState::REST.hot(true),
        State::Active => WidgetState::REST.pressed(true),
        State::Disabled => WidgetState::REST.disabled(true),
    }
}

/// A keyboard-focused control, as the focus ring reports it after Tab.
const KEY_FOCUS: WidgetState = WidgetState { focused: true, focus_visible: true, ..WidgetState::REST };

pub fn draw(c: &dyn Canvas, f: &Frame) {
    // The static exposition takes the window minus the interactive column, so it
    // lays out to the left of it rather than under it.
    let width = f.size.0 - interact::PANEL_W();
    let mut p = Page::new(c, width, f.size.1);

    // The exposition is taller than the window: the tab panel it is painted
    // into scrolls it (bars, wheel), so it simply lays out top to bottom.
    variants_section(&mut p);
    sizes_and_round_section(&mut p);
    states_section(&mut p);
    glyph_section(&mut p);
    card_section(&mut p);
}

/// What the interactive column remembers between frames.
struct Ui {
    clicks: u32,
    saving_until: u64,
    switch: bool,
    switch_t: Transition,
    check: bool,
    check_t: Transition,
    radio: usize,
    radio_t: [Transition; 3],
    view_grid: bool,
    starred: bool,
    prev_down: bool,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            clicks: 0,
            saving_until: 0,
            switch: false,
            switch_t: Transition::settled(0.0),
            check: false,
            check_t: Transition::settled(0.0),
            radio: 0,
            radio_t: [Transition::settled(1.0), Transition::settled(0.0), Transition::settled(0.0)],
            view_grid: false,
            starred: false,
            prev_down: false,
        }
    }
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

/// `key` pressed with no modifier this frame, consumed.
fn take(live: &Live, key: u16) -> bool {
    live.take_key(key, Modifiers::NONE)
}

/// Sets the cursor for a control under the pointer.
fn cursor_over(live: &Live, r: Rect, cursor: host::Cursor) {
    if live.hover(r) {
        host::set_cursor(cursor);
    }
}

/// The right-hand column: the same controls as the page, but live — pointer,
/// keyboard through the focus ring, and the web's transitions.
pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;
        let now = host::now_ms();
        let mut animating = false;

        let (left, mut y, right) = interact::panel(c, interact::panel_rect(f.size));

        // ── Buttons: a counter, a loading save, a round toggle ──────────────
        y = interact::caption(c, left, right, y, "Button — clic, ou Tab puis Entrée / Espace");
        let label = if ui.clicks == 0 {
            "Cliquez-moi".to_string()
        } else {
            format!("Cliqué {} fois", ui.clicks)
        };
        let b = Button::new(&label);
        let btn = Rect::new(left, y, left + 180.0, y + Size::Md.height());
        let ws = live.focus_state("count", btn);
        if live.hit(btn) || (ws.focused && (take(&live, vk::ENTER) || take(&live, vk::SPACE))) {
            ui.clicks += 1;
        }
        cursor_over(&live, btn, b.cursor(ws));
        b.paint(c, btn, ws);

        // `loading`: the click starts a 4 s « save », the ring spins meanwhile.
        let loading = now < ui.saving_until;
        let save = Button::new("Enregistrer")
            .variant(Variant::Secondary)
            .loading(loading)
            .loading_phase((now % 1000) as f32 / 1000.0);
        let save_w = Button::new("Enregistrer").variant(Variant::Secondary).width(c).ceil();
        let sr = Rect::new(btn.right + 12.0, y, btn.right + 12.0 + save_w, y + Size::Md.height());
        let ss = if save.focusable() { live.focus_state("save", sr) } else { live.state(sr).disabled(true) };
        if save.focusable()
            && (live.hit(sr) || (ss.focused && (take(&live, vk::ENTER) || take(&live, vk::SPACE))))
        {
            ui.saving_until = now + SAVE_DEMO_MS;
        }
        cursor_over(&live, sr, save.cursor(ss));
        save.paint(c, sr, ss);
        animating |= loading;

        // A round toggle: the header's 36 px control.
        let star = Rect::new(sr.right + 12.0, y, sr.right + 12.0 + 36.0, y + 36.0);
        let ib = IconButton::header(if ui.starred { "Check" } else { "Plus" });
        let is = live.focus_state("star", star);
        let is = WidgetState { hot: is.hot && ib.hit_test(star, live.mouse.0, live.mouse.1), ..is };
        if (live.hit(star) && ib.hit_test(star, live.mouse.0, live.mouse.1))
            || (is.focused && (take(&live, vk::ENTER) || take(&live, vk::SPACE)))
        {
            ui.starred = !ui.starred;
        }
        ib.paint(c, star, is);
        y += Size::Md.height() + 16.0;

        // ── Switch with a label and a description ───────────────────────────
        y = interact::caption(c, left, right, y, "Switch — clic ou Espace, glissement 150 ms");
        let sw = Switch::new()
            .on(ui.switch)
            .label("Synchroniser en arrière-plan")
            .description("Les fichiers modifiés sont envoyés dès que le réseau revient.");
        let sw_h = sw.height_for_width(c, right - left);
        let sw_w = sw.measure(c).width.min(right - left);
        let swr = Rect::new(left, y, left + sw_w, y + sw_h);
        let st = live.focus_state("switch", swr);
        if live.hit(swr) || (st.focused && take(&live, vk::SPACE)) {
            ui.switch = !ui.switch;
            let to = if ui.switch { 1.0 } else { 0.0 };
            ui.switch_t.retarget(to, now, SWITCH_TRANSITION_MS);
        }
        cursor_over(&live, swr, sw.cursor(st));
        sw.paint_progress(c, swr, st, ui.switch_t.value(now));
        animating |= ui.switch_t.running(now);
        y += sw_h + 16.0;

        // ── A check box whose long label wraps ──────────────────────────────
        y = interact::caption(c, left, right, y, "CheckBox — libellé long qui passe à la ligne");
        let checked = if ui.check { CheckState::Checked } else { CheckState::Unchecked };
        let cb = CheckBox::new(
            "J'accepte les conditions d'utilisation et la politique de confidentialité du service",
        )
        .check(checked);
        let cb_h = cb.height_for_width(c, right - left);
        let cb_w = cb.measure(c).width.min(right - left);
        let cbr = Rect::new(left, y, left + cb_w, y + cb_h);
        let cs = live.focus_state("check", cbr);
        if live.hit(cbr) || (cs.focused && take(&live, vk::SPACE)) {
            ui.check = !ui.check;
            let to = if ui.check { 1.0 } else { 0.0 };
            ui.check_t.retarget(to, now, CHECK_TRANSITION_MS);
        }
        cursor_over(&live, cbr, cb.cursor(cs));
        cb.paint_progress(c, cbr, cs, ui.check_t.value(now));
        animating |= ui.check_t.running(now);
        y += cb_h + 16.0;

        // ── A radio group: one Tab stop, arrows move the choice ─────────────
        y = interact::caption(c, left, right, y, "RadioButton — Tab entre dans le groupe, flèches");
        let names = ["Petit", "Moyen", "Grand (désactivé)"];
        let enabled = [true, true, false];
        let stop = radio_tab_stop(Some(ui.radio), &enabled);
        let mut focused_radio = None;
        for (i, name) in names.into_iter().enumerate() {
            let rb = RadioButton::new(name).selected(ui.radio == i);
            let w = rb.measure(c).width.min(right - left);
            let r = Rect::new(left, y, left + w, y + 20.0);
            let rs = if enabled[i] {
                let opts = FocusOpts { skip_tab: stop != Some(i), ..FocusOpts::default() };
                live.focus_with(FocusId::indexed("radio", i), r, opts).apply(live.state(r))
            } else {
                WidgetState::REST.disabled(true)
            };
            if rs.focused {
                focused_radio = Some(i);
            }
            if enabled[i] && live.hit(r) {
                ui.radio = i;
            }
            cursor_over(&live, r, rb.cursor(rs));
            let target = if ui.radio == i { 1.0 } else { 0.0 };
            ui.radio_t[i].retarget(target, now, CHECK_TRANSITION_MS);
            rb.paint_progress(c, r, rs, ui.radio_t[i].value(now));
            animating |= ui.radio_t[i].running(now);
            y += 20.0 + 8.0;
        }
        if let Some(i) = focused_radio {
            let mut moved = None;
            for key in [vk::DOWN, vk::RIGHT, vk::UP, vk::LEFT] {
                if take(&live, key) {
                    moved = radio_arrow_target(moved.unwrap_or(i), &enabled, key).or(moved);
                }
            }
            if take(&live, vk::SPACE) {
                ui.radio = i;
            }
            if let Some(t) = moved {
                // Arrows move the focus AND the selection, as a native group.
                ui.radio = t;
                interact::with_focus(|ring| ring.focus_visibly(FocusId::indexed("radio", t)));
            }
        }
        y += 8.0;

        // ── A toggle-button pair — a segmented « Liste / Grille » ──────────
        y = interact::caption(c, left, right, y, "ToggleButton — Liste ou Grille (Entrée / Espace)");
        let on = |v: bool| if v { CheckState::Checked } else { CheckState::Unchecked };
        let list_b = CheckBox::toggle_button("Liste").check(on(!ui.view_grid));
        let grid_b = CheckBox::toggle_button("Grille").check(on(ui.view_grid));
        let lw = list_b.measure(c).width;
        let gw = grid_b.measure(c).width;
        let list = Rect::new(left, y, left + lw, y + Size::Md.height());
        let grid = Rect::new(list.right + 8.0, y, list.right + 8.0 + gw, y + Size::Md.height());
        let ls = live.focus_state("view-list", list);
        let gs = live.focus_state("view-grid", grid);
        if live.hit(list) || (ls.focused && (take(&live, vk::ENTER) || take(&live, vk::SPACE))) {
            ui.view_grid = false;
        }
        if live.hit(grid) || (gs.focused && (take(&live, vk::ENTER) || take(&live, vk::SPACE))) {
            ui.view_grid = true;
        }
        list_b.paint(c, list, ls);
        grid_b.paint(c, grid, gs);
        y += Size::Md.height() + 16.0;

        // ── Overflow: the same button at a width the label does not fit ────
        y = interact::caption(c, left, right, y, "Débordement — le libellé s'abrège, jamais ne déborde");
        let narrow = Rect::new(left, y, left + 150.0, y + Size::Md.height());
        let nb = Button::new("Enregistrer les modifications").variant(Variant::Secondary).icon("Plus");
        let ns = live.focus_state("narrow", narrow);
        nb.paint(c, narrow, ns);

        if animating {
            host::request_repaint_after(16);
        }
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Every variant, in every state, old against new.
// ─────────────────────────────────────────────────────────────────────────────

fn variants_section(p: &mut Page) {
    let c = p.c;
    p.section("Boutons — chaque variante en repos · survol · pressé · désactivé");

    // Rounded UP to a multiple of 4 DIP, which is what makes the pixel diff of
    // this page meaningful: every DPI Windows offers is a quarter step, so a
    // multiple of 4 DIP is always a whole number of pixels and both halves land
    // on the same pixel grid.
    let strip = quantise(4.0 * Button::new(LABEL).width(c) + 3.0 * CELL_GAP);
    let h = Size::Md.height();

    for (variant, name) in VARIANTS {
        let top = p.y;
        let label = Rect::new(
            MARGIN,
            top + sheet::CAPTION_H,
            MARGIN + ROW_LABEL,
            top + sheet::CAPTION_H + h,
        );
        c.text(name, &label, &c.formats().body, &c.theme().text_secondary, false);
        let used = sheet::pair(
            c,
            MARGIN + ROW_LABEL,
            top,
            strip,
            h,
            |r| old_strip(c, r, variant),
            |r| new_strip(c, r, variant),
        );
        p.y += used + TIGHT_GAP;
    }
}

/// The predecessor's four states, laid out left to right at its own intrinsic
/// width.
fn old_strip(c: &dyn Canvas, area: Rect, variant: Variant) {
    let b = web_button::Button::new(LABEL).variant(variant.into());
    let w = web_button::width(c, &b);
    for (i, state) in STATES.iter().enumerate() {
        let x = area.left + i as f32 * (w + CELL_GAP);
        let r = Rect::new(x, area.top, x + w, area.top + Size::Md.height());
        web_button::draw(c, &r, &b, (*state).into());
    }
}

/// The same four, from the rebuilt primitive.
fn new_strip(c: &dyn Canvas, area: Rect, variant: Variant) {
    let b = Button::new(LABEL).variant(variant);
    let w = b.width(c);
    for (i, state) in STATES.iter().enumerate() {
        let x = area.left + i as f32 * (w + CELL_GAP);
        let r = Rect::new(x, area.top, x + w, area.top + b.size.height());
        b.paint(c, r, widget_state(*state));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. The size ramp and the icon, then the round button and the switch — old
//    above new, since the two halves do not fit side by side.
// ─────────────────────────────────────────────────────────────────────────────

fn sizes_and_round_section(p: &mut Page) {
    let c = p.c;
    p.section("Tailles, icône en tête, IconButton et Switch");

    p.caption("sm · md · lg · md avec icône · md à px-6 (24) · icône seule");
    stacked_pair(p, Size::Lg.height(), |r| old_sizes(c, r), |r| new_sizes(c, r));

    p.caption("cercle 36 en quatre états · cercle 40 teinté · switch off/on, actif puis désactivé");
    stacked_pair(p, 40.0, |r| old_round(c, r), |r| new_round(c, r));
}

/// « actuel » over « reconstruit », each `h` tall, labelled in the row-name
/// column.
fn stacked_pair(p: &mut Page, h: f32, old: impl FnOnce(Rect), new: impl FnOnce(Rect)) {
    let c = p.c;
    let t = c.theme();
    let f = c.formats();
    let right = p.area.right - MARGIN;
    for (i, name) in ["actuel", "reconstruit"].into_iter().enumerate() {
        let top = p.y + i as f32 * (h + TIGHT_GAP);
        c.text(name, &Rect::new(MARGIN, top, MARGIN + ROW_LABEL, top + h), &f.caption, &t.text_tertiary, false);
    }
    let a = Rect::new(MARGIN + ROW_LABEL, p.y, right, p.y + h);
    let b = Rect::new(MARGIN + ROW_LABEL, p.y + h + TIGHT_GAP, right, p.y + 2.0 * h + TIGHT_GAP);
    old(a);
    new(b);
    p.advance(2.0 * h + TIGHT_GAP);
}

fn old_sizes(c: &dyn Canvas, area: Rect) {
    let secondary = web_button::Variant::Secondary;
    let items = [
        web_button::Button::new(SHORT).size(web_button::Size::Sm).variant(secondary),
        web_button::Button::new(SHORT).size(web_button::Size::Md).variant(secondary),
        web_button::Button::new(SHORT).size(web_button::Size::Lg).variant(secondary),
        web_button::Button::new(SHORT).icon("Plus"),
        web_button::Button::new(SHORT).pad_x(24.0),
        web_button::Button::new("").icon("Plus"),
    ];
    let mut x = area.left;
    for b in items {
        let (w, h) = (web_button::width(c, &b), b.size.height());
        let y = (area.top + area.bottom) / 2.0 - h / 2.0;
        web_button::draw(c, &Rect::new(x, y, x + w, y + h), &b, web_button::State::Rest);
        x += w + CELL_GAP;
    }
}

fn new_sizes(c: &dyn Canvas, area: Rect) {
    let items = [
        Button::new(SHORT).size(Size::Sm).variant(Variant::Secondary),
        Button::new(SHORT).size(Size::Md).variant(Variant::Secondary),
        Button::new(SHORT).size(Size::Lg).variant(Variant::Secondary),
        Button::new(SHORT).icon("Plus"),
        Button::new(SHORT).pad_x(24.0),
        Button::new("").icon("Plus"),
    ];
    let mut x = area.left;
    for b in items {
        let (w, h) = (b.width(c), b.size.height());
        let y = (area.top + area.bottom) / 2.0 - h / 2.0;
        b.paint(c, Rect::new(x, y, x + w, y + h), WidgetState::REST);
        x += w + CELL_GAP;
    }
}

/// Where each cell of the round row sits, so both halves place them identically.
struct RoundRow {
    header: [Rect; 4],
    tinted: Rect,
    /// The switch, as `(rect, on, enabled)`.
    switches: [(Rect, bool, bool); 4],
}

fn round_cells(area: Rect) -> RoundRow {
    let cy = (area.top + area.bottom) / 2.0;
    let disc = |x: f32, d: f32| Rect::new(x, cy - d / 2.0, x + d, cy + d / 2.0);

    let mut x = area.left;
    let mut header = [Rect::default(); 4];
    for slot in header.iter_mut() {
        *slot = disc(x, 36.0);
        x += 36.0 + CELL_GAP;
    }
    let tinted = disc(x, 40.0);
    x += 40.0 + 2.0 * CELL_GAP;

    let mut switches = [(Rect::default(), false, false); 4];
    for (i, slot) in switches.iter_mut().enumerate() {
        *slot = (Rect::new(x, cy - 10.0, x + 36.0, cy + 10.0), i % 2 == 1, i < 2);
        x += 36.0 + CELL_GAP;
    }
    RoundRow { header, tinted, switches }
}

fn old_round(c: &dyn Canvas, area: Rect) {
    let row = round_cells(area);
    let b = web_button::IconButton::header("Search");
    for (r, state) in row.header.iter().zip(STATES) {
        web_button::draw_icon_button(c, r, &b, state.into());
    }
    let t = web_button::IconButton::tinted("PenLine", 40.0, 16.0);
    web_button::draw_icon_button(c, &row.tinted, &t, web_button::State::Rest);
    for (r, on, enabled) in row.switches {
        web_switch::draw(c, (r.left, r.top), on, enabled);
    }
}

fn new_round(c: &dyn Canvas, area: Rect) {
    let row = round_cells(area);
    let b = IconButton::header("Search");
    for (r, state) in row.header.iter().zip(STATES) {
        b.paint(c, *r, widget_state(state));
    }
    IconButton::tinted("PenLine", 40.0, 16.0).paint(c, row.tinted, WidgetState::REST);
    for (r, on, enabled) in row.switches {
        let state = if enabled { WidgetState::REST } else { WidgetState::REST.disabled(true) };
        Switch::new().on(on).paint(c, r, state);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. What the predecessor never had: focus-visible, loading, overflow.
// ─────────────────────────────────────────────────────────────────────────────

fn states_section(p: &mut Page) {
    let c = p.c;
    p.section("États du web — anneau de focus clavier · chargement · débordement");

    p.caption("focus-visible:ring-2 ring-offset-1 — bouton, secondaire, cercle, switch, bouton bascule");
    let top = p.y;
    let h = Size::Md.height();
    // Room for the 3 DIP ring outside every control.
    let gap = CELL_GAP + 2.0 * kubuno_desktop_ui::buttons::FOCUS_OUTSET;
    let mut x = MARGIN + kubuno_desktop_ui::buttons::FOCUS_OUTSET;
    for b in [Button::new("Bouton focus"), Button::new("Secondaire").variant(Variant::Secondary)] {
        let w = b.width(c);
        b.paint(c, Rect::new(x, top, x + w, top + h), KEY_FOCUS);
        x += w + gap;
    }
    IconButton::header("Search").paint(c, Rect::new(x, top, x + 36.0, top + 36.0), KEY_FOCUS);
    x += 36.0 + gap;
    let sr = Rect::new(x, top + 8.0, x + Switch::WIDTH, top + 8.0 + Switch::HEIGHT);
    Switch::new().on(true).paint(c, sr, KEY_FOCUS);
    x += Switch::WIDTH + gap;
    let tb = CheckBox::toggle_button("Grille").check(CheckState::Checked);
    let tw = tb.measure(c).width;
    tb.paint(c, Rect::new(x, top, x + tw, top + h), KEY_FOCUS);
    x += tw + gap;
    // A click focus (no ring): identical to rest, as `:focus-visible` wants.
    let clicked = WidgetState::REST.focused(true);
    let b = Button::new("Focus souris");
    let w = b.width(c);
    b.paint(c, Rect::new(x, top, x + w, top + h), clicked);
    p.advance(h);

    p.caption("loading — anneau border-current en rotation, bouton inactif · largeur imposée plus petite que le libellé");
    let top = p.y;
    let mut x = MARGIN;
    for (v, phase) in [(Variant::Primary, 0.0), (Variant::Secondary, 0.3), (Variant::Danger, 0.6)] {
        let b = Button::new("Envoyer").variant(v).loading(true).loading_phase(phase);
        let w = b.width(c);
        b.paint(c, Rect::new(x, top, x + w, top + h), WidgetState::REST);
        x += w + CELL_GAP;
    }
    x += CELL_GAP;
    for b in [
        Button::new("Enregistrer les modifications"),
        Button::new("Enregistrer les modifications").variant(Variant::Secondary).icon("Plus"),
        Button::new("Supprimer définitivement").variant(Variant::TextDanger),
    ] {
        b.paint(c, Rect::new(x, top, x + 140.0, top + h), WidgetState::REST);
        x += 140.0 + CELL_GAP;
    }
    p.advance(h);
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. The two glyph controls and the toggle button — the web is the reference.
// ─────────────────────────────────────────────────────────────────────────────

/// One cell per state, wide enough for an 18 box, its 8 gap and a short label.
const GLYPH_PITCH: f32 = 116.0;

fn glyph_section(p: &mut Page) {
    let c = p.c;
    p.section("CheckBox · RadioButton · ToggleButton — référence : le web");
    p.caption("CheckBox : repos · survol · coché · partiel + focus clavier · désactivé");

    let h = 20.0;
    let boxes: [(CheckBox, WidgetState); 5] = [
        (CheckBox::new("Repos"), WidgetState::REST),
        (CheckBox::new("Survol"), WidgetState::REST.hot(true)),
        (CheckBox::new("Coché").check(CheckState::Checked), WidgetState::REST),
        (CheckBox::new("Partiel").tri_state().check(CheckState::Indeterminate), KEY_FOCUS),
        (CheckBox::new("Inactif").check(CheckState::Checked), WidgetState::REST.disabled(true)),
    ];
    let top = p.y;
    for (i, (b, state)) in boxes.iter().enumerate() {
        let x = MARGIN + kubuno_desktop_ui::buttons::FOCUS_OUTSET + i as f32 * GLYPH_PITCH;
        b.paint(c, Rect::new(x, top, x + GLYPH_PITCH - 2.0 * CELL_GAP, top + h), *state);
    }
    p.advance(h);

    p.caption("RadioButton : repos · survol · choisi · focus clavier · désactivé");
    let radios: [(RadioButton, WidgetState); 5] = [
        (RadioButton::new("Repos"), WidgetState::REST),
        (RadioButton::new("Survol"), WidgetState::REST.hot(true)),
        (RadioButton::new("Choisi").selected(true), WidgetState::REST),
        (RadioButton::new("Focus").selected(true), KEY_FOCUS),
        (RadioButton::new("Inactif").selected(true), WidgetState::REST.disabled(true)),
    ];
    let top = p.y;
    for (i, (b, state)) in radios.iter().enumerate() {
        let x = MARGIN + kubuno_desktop_ui::buttons::FOCUS_OUTSET + i as f32 * GLYPH_PITCH;
        b.paint(c, Rect::new(x, top, x + GLYPH_PITCH - 2.0 * CELL_GAP, top + h), *state);
    }
    p.advance(h);

    p.caption("ToggleButton — le même CheckBox en Appearance::Button : ghost au repos, bg-primary-light + text-primary une fois enfoncé");
    let top = p.y;
    let h = Size::Md.height();
    let toggles: [(CheckBox, WidgetState); 4] = [
        (CheckBox::toggle_button("Liste"), WidgetState::REST),
        (CheckBox::toggle_button("Liste"), WidgetState::REST.hot(true)),
        (CheckBox::toggle_button("Grille").check(CheckState::Checked), WidgetState::REST),
        (
            CheckBox::toggle_button("Grille").check(CheckState::Checked),
            WidgetState::REST.disabled(true),
        ),
    ];
    let mut x = MARGIN;
    for (b, state) in toggles.iter() {
        let w = b.measure(c).width;
        b.paint(c, Rect::new(x, top, x + w, top + h), *state);
        x += w + CELL_GAP;
    }
    p.advance(h);
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Nested in a caller-painted card: transparent grounds, long labels.
// ─────────────────────────────────────────────────────────────────────────────

fn card_section(p: &mut Page) {
    let c = p.c;
    let t = c.theme();
    p.section("Dans une carte — fond transparent, libellés longs (retour à la ligne ou « … »)");

    let card_pad = 16.0;
    let left = MARGIN;
    let right = p.area.right - MARGIN;
    let col_gap = 24.0;
    let col_w = ((right - left - 2.0 * card_pad - col_gap) / 2.0).max(0.0);
    let a_left = left + card_pad;
    let b_left = a_left + col_w + col_gap;
    let long = "Autoriser la modification des documents partagés par tous les membres de l'équipe";

    // Column A: check boxes and a radio.
    let one_line = CheckBox::new(long).check(CheckState::Checked);
    let wrapped = CheckBox::new(long);
    let described = RadioButton::new("Lecture seule pour tous les liens publics")
        .selected(true)
        .description("Les personnes disposant du lien peuvent consulter sans modifier.");
    let h_wrapped = wrapped.height_for_width(c, col_w);
    let h_desc = described.height_for_width(c, col_w);
    let col_a_h = 20.0 + 12.0 + h_wrapped + 12.0 + h_desc;

    // Column B: switches and a round button.
    let sw = Switch::new()
        .on(true)
        .label("Autoriser les invités à rejoindre les réunions sans compte")
        .description("Un invité reçoit un lien valable 24 heures.");
    let sm = Switch::new().with_size(SwitchSize::Sm).label("Petit (sm)");
    let h_sw = sw.height_for_width(c, col_w);
    let h_sm = sm.height_for_width(c, col_w);
    let col_b_h = h_sw + 12.0 + h_sm + 12.0 + 36.0;

    let top = p.y;
    let card = Rect::new(left, top, right, top + 2.0 * card_pad + col_a_h.max(col_b_h));
    c.fill_rounded(&card, kubuno_desktop_ui::metrics::radius::LG, &t.card_background);
    c.stroke_rounded(&card, kubuno_desktop_ui::metrics::radius::LG, &t.card_stroke);
    c.push_bg(t.card_background);

    let mut y = top + card_pad;
    one_line.paint(c, Rect::new(a_left, y, a_left + col_w, y + 20.0), WidgetState::REST);
    y += 20.0 + 12.0;
    wrapped.paint(c, Rect::new(a_left, y, a_left + col_w, y + h_wrapped), WidgetState::REST.hot(true));
    y += h_wrapped + 12.0;
    described.paint(c, Rect::new(a_left, y, a_left + col_w, y + h_desc), WidgetState::REST);

    let mut y = top + card_pad;
    sw.paint(c, Rect::new(b_left, y, b_left + col_w, y + h_sw), WidgetState::REST);
    y += h_sw + 12.0;
    let sm_w = sm.measure(c).width;
    sm.paint(c, Rect::new(b_left, y, b_left + sm_w, y + h_sm), WidgetState::REST);
    y += h_sm + 12.0;
    let mut x = b_left;
    for (state, filled) in [(WidgetState::REST, false), (WidgetState::REST.hot(true), false), (WidgetState::REST, true)] {
        let ib = if filled { IconButton::tinted("PenLine", 36.0, 16.0) } else { IconButton::header("Search") };
        ib.paint(c, Rect::new(x, y, x + 36.0, y + 36.0), state);
        x += 36.0 + CELL_GAP;
    }
    let ghost = Button::new("Annuler").variant(Variant::Ghost);
    let gw = ghost.width(c);
    ghost.paint(c, Rect::new(x, y, x + gw, y + 36.0), WidgetState::REST);

    c.pop_bg();
    p.advance(card.bottom - card.top);
}
