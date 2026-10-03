//! Gallery page — the editor family.
//!
//! None of these five primitives has a hand-written predecessor in
//! `kubuno-drive-desktop-app-controls`, so there is no [`super::sheet::pair`] here: there is
//! nothing to sit beside. What the page shows instead is every **state** the
//! family can be in, painted from the same inputs the unit tests use.
//!
//! The static exposition (left) paints each state once — including the
//! keyboard-focus rings (`:focus-visible`), the overflow cases (a label too
//! long for its trigger, a name too long for its field, a toolbar narrower
//! than its commands) and the three dropped lists, INLINE, so they can be
//! compared side by side.
//!
//! The live column (right) is the real thing: every dropped list is a floating
//! surface in a `host::popup` window of its own — placed against the MONITOR,
//! so it may hang past the gallery window's edge and flips above its trigger
//! when there is no room below — with its hover and clicks reported back to
//! this page. The keyboard follows the web (`Dropdown.tsx`,
//! `FontPicker.tsx`, `FontSizeField.tsx`, the WAI-ARIA toolbar):
//!
//! * `Tab` / `Shift+Tab` walk the six controls (the toolbar is ONE stop);
//! * a dropdown: `↓` `↑` `Enter` `Space` open, arrows / `Home` / `End` /
//!   `PageUp` / `PageDown` move, letters type-ahead, `Enter` chooses, `Escape`
//!   closes, `Tab` takes the highlighted row with it;
//! * the font picker: `Enter` opens it on the current font, typing searches,
//!   arrows move, `Enter` chooses;
//! * the size field: type a size, `↑` / `↓` step, `Enter` commits, `Escape`
//!   reverts, `Alt+↓` drops the presets;
//! * the toolbar: `←` / `→` / `Home` / `End` move, `Enter` / `Space` press;
//! * the editable: click (caret), double click (word), triple click (all),
//!   `Enter` / `F2` start editing from the keyboard, `Ctrl+A/C/X/V`.

use std::cell::RefCell;

use kubuno_desktop_controls::enums::Size;
use kubuno_desktop_controls::host::{self, Cursor, Frame};
use kubuno_desktop_ui::display::{place, Placement, Side, Tooltip};
use kubuno_desktop_ui::editors::{
    rebase, shadow_margin, system_font_families, Dropdown, DropdownVariant, EditKey, Editable,
    FontPicker, FontSizeField, ListKey, RichTextCommand, RichTextToolbar, SizeKey, SquaredEdge,
    ToolbarKey,
};
use kubuno_desktop_ui::focus::{caret_visible, FocusOpts};
use kubuno_desktop_ui::{Canvas, Rect, Widget, WidgetState};

use super::interact::{self, Live};
use super::sheet::{cells, Page, MARGIN};

/// The gap between two cells on this page.
const GAP: f32 = 16.0;
/// The height a `Dropdown`, an `Editable` and a `FontPicker` trigger take —
/// `height::BUTTON_MD`, the `h-9` every field in this system is.
const FIELD_H: f32 = 36.0;
/// The toolbar height a glued font field is set at (`FontSizeField.tsx`'s
/// `height = 30`), and its two widths (`fontWidth = 150`, `sizeWidth = 62`).
const TOOLBAR_H: f32 = 30.0;
const PAIR_FONT_W: f32 = 150.0;
const PAIR_SIZE_W: f32 = 62.0;
/// A tooltip waits this long under a still pointer before it shows — the
/// browser's delay for a `title`, which is what `@ui/RichText`'s buttons use.
const TOOLTIP_DELAY_MS: u64 = 500;
/// The search typed into the static exposition's open font picker: narrow
/// enough that its matches fit under the trigger in the exposition (the web's
/// 340 DIP list cap would otherwise run past the page's bottom edge).
const FONT_QUERY: &str = "segoe ui";

const SORTS: [(&str, Option<&str>); 4] = [
    ("Trier par nom", Some("Type")),
    ("Trier par date", Some("Clock")),
    ("Trier par taille", Some("HardDrive")),
    ("Trier par type", Some("Filter")),
];

/// A list long enough to scroll (`maxHeight: 280` holds 9 rows) and to make
/// the type-ahead worth having.
const COUNTRIES: [&str; 20] = [
    "Allemagne", "Autriche", "Belgique", "Bulgarie", "Canada", "Croatie", "Danemark", "Espagne",
    "Estonie", "Finlande", "France", "Grèce", "Hongrie", "Irlande", "Italie", "Lettonie",
    "Lituanie", "Luxembourg", "Malte", "Pays-Bas",
];

pub fn draw(c: &dyn Canvas, f: &Frame) {
    // The static exposition takes the window minus the interactive column, so it
    // lays out to the left of it rather than under it.
    let content_w = f.size.0 - interact::PANEL_W();
    let mut page = Page::new(c, content_w, f.size.1);
    let (mx, my) = f.mouse;
    let right = content_w - MARGIN;
    let span = right - MARGIN;

    // ── Dropdown ─────────────────────────────────────────────────────────────
    page.section("Dropdown");
    let top = page.caption(
        "libellé trop long (ellipse) · survolé + placeholder · ghost (barre d'outils, taille \
         naturelle) · focus clavier (anneau) · désactivé",
    );
    let w5 = (span - 4.0 * GAP) / 5.0;
    let row = cells(MARGIN, top, w5, FIELD_H, GAP, 5);

    // 1 — a label longer than the trigger: `truncate`, never spilled.
    let mut long = sort_dropdown();
    long.add_option("Trier par date de dernière modification", Some("Clock"));
    long.set_selected_index(4);
    long.paint(c, row[0], WidgetState::REST);

    // 2 — the hover fill, and a placeholder standing in for an empty
    //     selection (`selected?.label ?? placeholder`).
    let mut hovered = sort_dropdown();
    hovered.placeholder = "Choisir un tri…".into();
    hovered.paint(c, row[1], WidgetState::REST.hot(true));

    // 3 — `ghost`: no border, secondary ink, at the toolbar height and its
    //     OWN measured width (« omit `width` for natural sizing »).
    let mut ghost = sort_dropdown();
    ghost.variant = DropdownVariant::Ghost;
    ghost.height = 28.0;
    ghost.set_selected_index(2);
    let natural = ghost.measure(c);
    let ghost_cell = Rect::new(
        row[2].left,
        row[2].top + (FIELD_H - natural.height) / 2.0,
        row[2].left + natural.width.min(w5),
        row[2].top + (FIELD_H + natural.height) / 2.0,
    );
    ghost.paint(c, ghost_cell, WidgetState::REST);

    // 4 — reached with Tab: the focused `<Input>`'s accent border + ring.
    let mut keyed = sort_dropdown();
    keyed.set_selected_index(0);
    keyed.paint(c, row[3], WidgetState::REST.focused(true).focus_visible(true));

    // 5 — disabled: `opacity: 0.5`, and the list refuses to drop.
    let mut dead = sort_dropdown();
    dead.set_selected_index(1);
    dead.open = true;
    dead.paint(c, row[4], WidgetState::REST.disabled(true));

    page.advance(FIELD_H);

    // ── Editable ─────────────────────────────────────────────────────────────
    page.section("Editable");
    let top = page.caption(
        "repos (nom trop long : ellipse) · survol (cadre de l'Input, placeholder) · focus clavier \
         · édition (sélection) — MÊME rectangle, MÊME ligne de base",
    );
    let w4 = (span - 3.0 * GAP) / 4.0;
    let row = cells(MARGIN, top, w4, FIELD_H, GAP, 4);

    // 1 — at rest: a bare label, ellipsised when too long for the cell.
    named("Rapport annuel 2025 — version définitive relue").paint(c, row[0], WidgetState::REST);

    // 2 — hovered and empty: `@ui/Input`'s frame and the greyed placeholder.
    named("").paint(c, row[1], WidgetState::REST.hot(true));

    // 3 — reached with Tab at rest: the frame and the field's ring.
    named("Rapport annuel").paint(c, row[2], WidgetState::REST.focused(true).focus_visible(true));

    // 4 — editing: the SAME string, now a `TextField` — caret, selection band
    //     and scroll are `edit_box::EditView`'s.
    let mut editing = named("Rapport annuel");
    editing.begin_edit();
    editing.select(0, 7);
    editing.paint(c, row[3], WidgetState::REST);

    // A hairline under the four, so the reader can check by eye that the
    // strings sit on one baseline — the whole point of the cell.
    let t = c.theme();
    let base = row[0].bottom + 3.0;
    c.fill_rounded(&Rect::new(row[0].left, base, row[3].right, base + 1.0), 0.0, &t.divider);

    page.advance(FIELD_H + 4.0);

    // ── RichTextToolbar ──────────────────────────────────────────────────────
    page.section("RichTextToolbar");
    let top = page.caption(
        "gras + liste à puces actifs, focus clavier sur « Italique » · désactivée · trop étroite \
         (coupée à son bord) — puis le groupe d'alignement (extension)",
    );
    let bar_h = kubuno_desktop_ui::editors::rich_metrics::HEIGHT;

    // 1 — `@ui/RichText`'s bar exactly, with two commands on and the roving
    //     focus ringed on the second button.
    let mut bar = RichTextToolbar::standard();
    bar.apply(RichTextCommand::Bold);
    bar.apply(RichTextCommand::BulletList);
    bar.focus_index = Some(1);
    // 2 — disabled: every glyph fades, and no command lights.
    let mut off = RichTextToolbar::standard();
    off.set_active(RichTextCommand::Link, true);
    // 3 — narrower than its commands: `overflow-hidden` cuts them at its edge.
    let narrow = RichTextToolbar::standard();

    let mut x = MARGIN;
    let states = [
        WidgetState::REST.focused(true).focus_visible(true),
        WidgetState::REST.disabled(true),
        WidgetState::REST,
    ];
    for (i, tb) in [&mut bar, &mut off].into_iter().enumerate() {
        let w = tb.measure(c).width;
        let cell = Rect::new(x, top, x + w, top + bar_h);
        tb.hot_index = tb.item_at(cell, mx, my);
        tb.paint(c, cell, states[i]);
        x = cell.right + GAP;
    }
    let narrow_cell = Rect::new(x, top, (x + 130.0).min(right), top + bar_h);
    narrow.paint(c, narrow_cell, states[2]);

    // 4 — the alignment group, which `@ui/RichText` does not have; centring
    //     is exclusive with the other three.
    let top2 = top + bar_h + space_sm();
    let mut aligned = RichTextToolbar::with_alignments();
    for cmd in [RichTextCommand::Italic, RichTextCommand::Underline, RichTextCommand::AlignRight, RichTextCommand::AlignCenter] {
        aligned.apply(cmd);
    }
    let w = aligned.measure(c).width.min(span);
    let cell = Rect::new(MARGIN, top2, MARGIN + w, top2 + bar_h);
    aligned.hot_index = aligned.item_at(cell, mx, my);
    aligned.paint(c, cell, WidgetState::REST);

    page.advance(2.0 * bar_h + space_sm());

    // ── The open popups ──────────────────────────────────────────────────────
    page.section("Listes ouvertes (ici en ligne ; dans la colonne, en fenêtres flottantes)");
    // How many families were actually enumerated goes in the caption: a picker
    // showing nothing is an enumeration that failed, and the page must say so
    // rather than leave an empty panel to be read as a paint bug.
    let top = page.caption(&format!(
        "20 pays (9 visibles, barre) · paire « collée », tailles ouvertes, saisie · police \
         filtrée par « {FONT_QUERY} » (surlignage, « Effacer ») · {} familles installées",
        system_font_families().len(),
    ));
    let wide = (span - 2.0 * GAP) / 3.0;
    let open_row = cells(MARGIN, top, wide, FIELD_H, GAP, 3);

    // 1 — a long dropdown, scrolled, with its bar and its checked row.
    let mut dropped = country_dropdown();
    dropped.set_selected_index(10);
    dropped.open = true;
    dropped.scroll = 5;
    dropped.hot_index = dropped.item_at(open_row[0], mx, my).or(Some(9));
    dropped.paint(c, open_row[0], WidgetState::REST);

    // 2 — the pair the web calls `FontSizeField`: a picker and a size sharing
    //     one height, the joined corners squared and the middle borders
    //     overlapped into one divider (the -1). The size is being typed into
    //     (all selected, as on focus) and its preset list is dropped.
    let pair_top = open_row[1].top;
    let glued = Rect::new(open_row[1].left, pair_top, open_row[1].left + PAIR_FONT_W, pair_top + TOOLBAR_H);
    let mut pair_picker = picker_over_real_fonts();
    pair_picker.height = TOOLBAR_H;
    pair_picker.joined = SquaredEdge::Right;
    pair_picker.paint(c, glued, WidgetState::REST);

    let size_cell = Rect::new(glued.right - 1.0, pair_top, glued.right - 1.0 + PAIR_SIZE_W, pair_top + TOOLBAR_H);
    let mut size = size_field(14.0);
    size.height = TOOLBAR_H;
    size.joined = SquaredEdge::Left;
    size.begin_edit();
    size.caret_visible = true;
    size.open = true;
    size.hot_index = size.item_at(size_cell, mx, my).or(Some(7));
    size.paint(c, size_cell, WidgetState::REST.focused(true));

    // 3 — the font picker, open and searching. Last, so its panel is on top.
    let mut picker = picker_over_real_fonts();
    picker.open = true;
    picker.query = FONT_QUERY.into();
    picker.search_caret = true;
    // Placed as the web's `useLayoutEffect` does — `max-content` wide, and
    // slid back inside the viewport (here: the exposition, less its margin)
    // when it would run past its bottom edge instead of being cut there.
    let viewport = Rect::new(page.area.left, page.area.top, page.area.right, page.area.bottom - MARGIN / 2.0);
    picker.place_popup(c, open_row[2], viewport);
    picker.hot_index = picker.item_at(open_row[2], mx, my).or(Some(0));
    picker.paint(c, open_row[2], WidgetState::REST);
}

/// `space::SM` — the gap between two stacked toolbar specimens.
fn space_sm() -> f32 {
    kubuno_desktop_ui::metrics::space::SM
}

// ─────────────────────────────────────────────────────────────────────────────
// The interactive column — the same editors, live, keyboard included.
// ─────────────────────────────────────────────────────────────────────────────

/// Which floating list, if any, is dropped. One at a time: opening one closes
/// the others, as every web popup's outside-press handler does.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
enum Open {
    #[default]
    None,
    Sort,
    Country,
    Font,
    Size,
}

/// What the interactive column remembers between frames: the live controls
/// themselves (their open state, highlight, scroll and type-ahead are runtime
/// state the web keeps in hooks), which list is open and where it floated
/// last frame, and the last action for the status line.
struct Ui {
    prev_down: bool,
    ed: Editable,
    bar: RichTextToolbar,
    /// The toolbar cell under a still pointer, and since when (tooltip delay).
    tip_cell: Option<usize>,
    tip_since: u64,
    sort: Dropdown,
    country: Dropdown,
    font: FontPicker,
    size: FontSizeField,
    open: Open,
    /// Last frame's open panel, in client DIP — clicks are routed against it
    /// at the top of the next frame, before anything underneath sees them.
    panel: Option<Rect>,
    /// Last frame's trigger of the open control (a click there closes).
    trigger: Option<Rect>,
    last_mouse: (f32, f32),
    font_input_ms: u64,
    size_input_ms: u64,
    status: String,
}

impl Default for Ui {
    fn default() -> Self {
        let mut sort = sort_dropdown();
        sort.set_selected_index(0);
        let mut country = country_dropdown();
        country.placeholder = "Choisir un pays…".into();
        let mut font = picker_over_real_fonts();
        font.height = TOOLBAR_H;
        font.joined = SquaredEdge::Right;
        let mut size = size_field(14.0);
        size.height = TOOLBAR_H;
        size.joined = SquaredEdge::Left;
        Self {
            prev_down: false,
            ed: named("Rapport annuel"),
            bar: RichTextToolbar::standard(),
            tip_cell: None,
            tip_since: 0,
            sort,
            country,
            font,
            size,
            open: Open::None,
            panel: None,
            trigger: None,
            last_mouse: (0.0, 0.0),
            font_input_ms: 0,
            size_input_ms: 0,
            status: String::new(),
        }
    }
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

/// Closes whatever list is dropped.
fn close_all(ui: &mut Ui) {
    ui.sort.close();
    ui.sort.clear_placement();
    ui.country.close();
    ui.country.clear_placement();
    ui.font.close();
    ui.font.clear_placement();
    ui.size.close();
    ui.size.clear_placement();
    ui.open = Open::None;
    ui.panel = None;
    ui.trigger = None;
}

/// A click that landed inside last frame's open panel: a row chooses, the
/// font picker's « Effacer » clears the search, anything else (padding, a
/// header, the search box) keeps the list open. The popup swallows it.
fn route_panel_click(c: &dyn Canvas, ui: &mut Ui, panel: Rect, x: f32, y: f32) {
    match ui.open {
        Open::Sort | Open::Country => {
            let d = if ui.open == Open::Sort { &mut ui.sort } else { &mut ui.country };
            if let Some(i) = d.item_at_in(panel, x, y) {
                d.commit(i);
                let label = d.selected_item().unwrap_or_default().to_string();
                ui.status = format!("Dropdown : « {label} »");
                close_all(ui);
            }
        }
        Open::Font => {
            if ui.font.clear_rect_in(c, panel).is_some_and(|r| r.contains(x, y)) {
                ui.font.clear_query();
                ui.font_input_ms = host::now_ms();
            } else if let Some(flat) = ui.font.item_at_in(panel, x, y) {
                if let Some(name) = ui.font.choose(flat) {
                    ui.status = format!("Police : « {name} »");
                }
                close_all(ui);
            }
        }
        Open::Size => {
            if let Some(i) = ui.size.list_view().item_at_in(panel, x, y) {
                if let Some(v) = ui.size.choose_preset(i) {
                    ui.status = format!("Taille : {v}");
                }
                close_all(ui);
            }
        }
        Open::None => {}
    }
}

/// The right-hand column: the same editors as the page, live under the
/// pointer and the keyboard.
pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|cell| {
        let mut guard = cell.borrow_mut();
        let ui = &mut *guard;
        let mut live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;
        let now = host::now_ms();
        let (px, py) = f.mouse;
        let moved = (px, py) != ui.last_mouse;
        ui.last_mouse = (px, py);

        // ── The open surface takes the pointer first ─────────────────────────
        // A click on the desktop or another app arrives as `dismiss`. A click
        // in the panel is the panel's (routed against LAST frame's geometry,
        // swallowed); on the open control's trigger it closes the list (the
        // web's toggle) and is swallowed too; anywhere else it closes the list
        // and goes through — the web's `pointerdown` close, which is not a
        // backdrop.
        if f.dismiss && ui.open != Open::None {
            close_all(ui);
        }
        if ui.open != Open::None && live.clicked {
            if let Some(panel) = ui.panel.filter(|p| p.contains(px, py)) {
                route_panel_click(c, ui, panel, px, py);
                live.clicked = false;
            } else if ui.trigger.is_some_and(|t| t.contains(px, py)) && ui.open != Open::Size {
                close_all(ui);
                live.clicked = false;
            } else if !(ui.open == Open::Size && ui.trigger.is_some_and(|t| t.contains(px, py))) {
                close_all(ui);
            }
        }
        // The wheel over the open panel scrolls it.
        if let Some(panel) = ui.panel {
            let (_, dy) = live.wheel_over(panel);
            match ui.open {
                Open::Sort => ui.sort.scroll_by_dip(dy),
                Open::Country => ui.country.scroll_by_dip(dy),
                Open::Font => ui.font.scroll_by(dy),
                Open::Size => ui.size.scroll_by_dip(dy),
                Open::None => {}
            }
            // Nothing under a floating panel lights up.
            if panel.contains(px, py) {
                live.mouse = (host::POINTER_AWAY, host::POINTER_AWAY);
            }
        }
        let (mx, my) = live.mouse;

        let (left, mut y, right) = interact::panel(c, interact::panel_rect(f.size));
        let field_w = (right - left).min(280.0);

        // ── Editable ─────────────────────────────────────────────────────────
        y = interact::caption(c, left, right, y, "Editable — clic, double clic (mot), triple clic · Entrée/F2 · Échap · Ctrl+A/C/X/V");
        let ed_r = Rect::new(left, y, left + field_w, y + FIELD_H);
        let st = live.focus_with("ed", ed_r, FocusOpts::TEXT);
        if live.hover(ed_r) {
            host::set_cursor(Cursor::IBeam);
        }
        if live.hit(ed_r) {
            match live.click_count {
                n if n >= 3 => ui.ed.begin_edit(),
                2 => ui.ed.select_word_at(c, ed_r, mx),
                _ => {
                    let extend = live.mods.shift && ui.ed.editing;
                    ui.ed.click_at(c, ed_r, mx, extend);
                }
            }
        }
        if st.focused {
            match ui.ed.take_input() {
                EditKey::Committed => ui.status = format!("Editable validé : « {} »", ui.ed.text()),
                EditKey::Cancelled => ui.status = "Editable : modification annulée (Échap)".into(),
                _ => {}
            }
        } else if ui.ed.editing {
            // Leaving the field keeps what was typed, as a rename does.
            ui.ed.commit_edit();
            ui.status = format!("Editable validé (focus perdu) : « {} »", ui.ed.text());
        }
        ui.ed.paint(c, ed_r, st.apply(live.state(ed_r)));
        y = ed_r.bottom + GAP;

        // ── RichTextToolbar — one Tab stop, arrows inside ────────────────────
        y = interact::caption(c, left, right, y, "RichTextToolbar — clic ou ←/→ + Entrée · listes et styles exclusifs comme l'éditeur");
        let bar_w = ui.bar.measure(c).width.min(right - left);
        let bar_r = Rect::new(left, y, left + bar_w, y + kubuno_desktop_ui::editors::rich_metrics::HEIGHT);
        let st = live.focus("rt", bar_r);
        let hot = ui.bar.item_at(bar_r, mx, my);
        ui.bar.hot_index = hot;
        ui.bar.pressed_index = hot.filter(|_| live.down);
        if live.clicked {
            if let Some(cmd) = hot.and_then(|i| ui.bar.activate(i)) {
                ui.status = command_status(&ui.bar, cmd);
            }
        }
        if st.focused {
            if st.gained && ui.bar.focus_index.is_none() {
                ui.bar.focus_index = ui.bar.entry_index();
            }
            if let ToolbarKey::Activated(i) = ui.bar.take_input() {
                if let Some(cmd) = ui.bar.command_at(i) {
                    ui.status = command_status(&ui.bar, cmd);
                }
            }
        }
        ui.bar.paint(c, bar_r, st.apply(live.state(bar_r)));
        // The web's `title`: a tooltip under a still pointer, floating.
        if hot != ui.tip_cell {
            ui.tip_cell = hot;
            ui.tip_since = now;
        }
        if let Some(i) = hot.filter(|_| !live.down) {
            let waited = now.saturating_sub(ui.tip_since);
            if waited >= TOOLTIP_DELAY_MS {
                if let (Some(text), Some(cell)) = (ui.bar.tooltip_of(i), ui.bar.item_rect(bar_r, i)) {
                    float_tooltip(c, f, text, cell);
                }
            } else {
                host::request_repaint_after((TOOLTIP_DELAY_MS - waited) as u32);
            }
        }
        y = bar_r.bottom + GAP;

        // ── Two dropdowns ────────────────────────────────────────────────────
        y = interact::caption(c, left, right, y, "Dropdown — icônes · clavier : ↓ Entrée Espace, flèches, Échap, Tab");
        let sort_b = Rect::new(left, y, left + field_w, y + FIELD_H);
        drive_dropdown(c, &live, ui, Open::Sort, sort_b, now);
        y = sort_b.bottom + GAP;

        y = interact::caption(c, left, right, y, "Dropdown — 20 pays : molette, saisie rapide (« f », « fi », « l » « l »…)");
        let country_b = Rect::new(left, y, left + field_w, y + FIELD_H);
        drive_dropdown(c, &live, ui, Open::Country, country_b, now);
        y = country_b.bottom + GAP;

        // ── The glued pair: FontPicker | FontSizeField ───────────────────────
        y = interact::caption(c, left, right, y, "FontPicker + FontSizeField — Entrée/recherche · taille : saisir, ↑/↓, Alt+↓");
        let font_b = Rect::new(left, y, left + PAIR_FONT_W, y + TOOLBAR_H);
        let size_b = Rect::new(font_b.right - 1.0, y, font_b.right - 1.0 + PAIR_SIZE_W, y + TOOLBAR_H);

        // The picker's trigger.
        let fst = live.focus("fp", font_b);
        if live.hover(font_b) {
            host::set_cursor(Cursor::Hand);
        }
        if live.hit(font_b) {
            close_all(ui);
            ui.font.open_menu();
            ui.open = Open::Font;
            ui.font_input_ms = now;
        }
        if fst.focused {
            match ui.font.take_input() {
                ListKey::Opened => {
                    close_other(ui, Open::Font);
                    ui.open = Open::Font;
                    ui.font_input_ms = now;
                }
                ListKey::Committed(_) => {
                    ui.status = format!("Police : « {} »", ui.font.display_text());
                    close_all(ui);
                }
                ListKey::Closed => close_all(ui),
                ListKey::Moved => ui.font_input_ms = now,
                _ => {}
            }
        } else if ui.open == Open::Font {
            close_all(ui);
        }
        ui.font.paint_field(c, font_b, fst.apply(live.state(font_b)));

        // The size input and its caret button (`tabIndex={-1}`: a click on
        // the button must not take the focus from where it is).
        let input = ui.size.input_rect(size_b);
        let caret_btn = ui.size.caret_rect(size_b);
        interact::with_focus(|r| r.keep_focus_in(caret_btn));
        let sst = live.focus_with("fs", input, FocusOpts::TEXT);
        if live.hover(input) {
            host::set_cursor(Cursor::IBeam);
        } else if live.hover(caret_btn) {
            host::set_cursor(Cursor::Hand);
        }
        if live.hit(caret_btn) {
            if ui.open == Open::Size {
                close_all(ui);
            } else {
                close_all(ui);
                ui.size.open_list();
                ui.open = Open::Size;
            }
        }
        if sst.gained {
            ui.size.begin_edit();
            ui.size_input_ms = now;
        }
        if sst.focused {
            match ui.size.take_input() {
                SizeKey::Committed(v) => {
                    ui.status = format!("Taille : {v}");
                    close_all(ui);
                    interact::with_focus(|r| r.blur());
                }
                SizeKey::Cancelled => {
                    ui.status = "Taille : saisie annulée (Échap)".into();
                    interact::with_focus(|r| r.blur());
                }
                SizeKey::Opened => {
                    close_other(ui, Open::Size);
                    ui.open = Open::Size;
                }
                SizeKey::Closed => close_all(ui),
                SizeKey::Stepped(_) | SizeKey::Edited => ui.size_input_ms = now,
                SizeKey::Ignored => {}
            }
        } else if ui.size.editing {
            let v = ui.size.end_edit();
            ui.status = format!("Taille : {v}");
        }
        ui.size.caret_visible = sst.focused && live.window_focused && caret_visible(ui.size_input_ms);
        let field = ui.size.field_rect(size_b);
        ui.size.paint_field(c, size_b, sst.apply(live.state(field)));
        y = font_b.bottom + GAP;

        // ── Status ───────────────────────────────────────────────────────────
        let status = if ui.status.is_empty() { "Aucune action pour l'instant".to_string() } else { ui.status.clone() };
        let t = c.theme();
        let fm = c.formats();
        c.text_ellipsis(&status, &Rect::new(left, y, right, y + 20.0), &fm.caption, &t.text_secondary);

        // ── The floating surface of the open control ─────────────────────────
        let area = f.screen_area();
        let mut surface: Option<(Rect, Rect)> = None; // (panel, paint bounds)
        match ui.open {
            Open::Sort | Open::Country => {
                let (d, b) = if ui.open == Open::Sort { (&mut ui.sort, sort_b) } else { (&mut ui.country, country_b) };
                d.place_drop_down(c, b, area);
                let panel = d.drop_down_rect(b);
                if moved && panel.contains(px, py) {
                    if let Some(i) = d.item_at_in(panel, px, py) {
                        d.hot_index = Some(i);
                    }
                }
                let pb = d.drop_down_paint_bounds(b);
                let local = rebase(panel, pb);
                let snap = d.clone();
                host::popup(pb, move |cv| snap.paint_drop_down_at(cv, local));
                ui.trigger = Some(d.trigger_rect(b));
                surface = Some((panel, pb));
            }
            Open::Font => {
                ui.font.place_popup(c, font_b, area);
                let panel = ui.font.popup_rect(font_b);
                if moved && panel.contains(px, py) {
                    if let Some(i) = ui.font.item_at_in(panel, px, py) {
                        ui.font.hot_index = Some(i);
                    }
                }
                let search = ui.font.search_rect(font_b);
                if search.contains(px, py) && !ui.font.clear_rect_in(c, panel).is_some_and(|r| r.contains(px, py)) {
                    host::set_cursor(Cursor::IBeam);
                }
                ui.font.search_caret = live.window_focused && caret_visible(ui.font_input_ms);
                let pb = ui.font.popup_paint_bounds(font_b);
                let local = rebase(panel, pb);
                let snap = ui.font.clone();
                host::popup(pb, move |cv| snap.paint_popup_at(cv, local));
                ui.trigger = Some(font_b);
                surface = Some((panel, pb));
            }
            Open::Size => {
                ui.size.place_drop_down(size_b, area);
                let panel = ui.size.drop_down_rect(size_b);
                if moved && panel.contains(px, py) {
                    ui.size.hot_index = ui.size.list_view().item_at_in(panel, px, py);
                }
                let pb = ui.size.drop_down_paint_bounds(size_b);
                let local = rebase(panel, pb);
                let snap = ui.size.list_view();
                host::popup(pb, move |cv| snap.paint_at(cv, local));
                ui.trigger = Some(field);
                surface = Some((panel, pb));
            }
            Open::None => {}
        }
        ui.panel = surface.map(|(p, _)| p);
        if let Some((_, pb)) = surface {
            // A click in the popup must not blur the trigger that owns it.
            interact::with_focus(|r| r.keep_focus_in(pb));
        }
    });
}

/// Closes every list but `keep`.
fn close_other(ui: &mut Ui, keep: Open) {
    if ui.open != keep && ui.open != Open::None {
        let status = std::mem::take(&mut ui.status);
        close_all(ui);
        ui.status = status;
    }
}

/// One live dropdown: its focus stop, its trigger's click and keys, and its
/// field. The dropped list itself is painted by the caller, in a popup.
fn drive_dropdown(c: &dyn Canvas, live: &Live, ui: &mut Ui, me: Open, bounds: Rect, now: u64) {
    let id = if me == Open::Sort { "dd-sort" } else { "dd-country" };
    let trig = if me == Open::Sort { ui.sort.trigger_rect(bounds) } else { ui.country.trigger_rect(bounds) };
    let st = live.focus(id, trig);
    if live.hover(trig) {
        host::set_cursor(Cursor::Hand);
    }
    if live.hit(trig) {
        close_all(ui);
        let d = if me == Open::Sort { &mut ui.sort } else { &mut ui.country };
        d.open_with(d.selected());
        ui.open = me;
    }
    let mut result = ListKey::Ignored;
    let mut tabbed_away = false;
    {
        let d = if me == Open::Sort { &mut ui.sort } else { &mut ui.country };
        if st.focused {
            result = d.take_input(now);
        } else if ui.open == me {
            // `Tab` while open: « leaving takes the highlighted row with it ».
            result = d.commit_highlight();
            tabbed_away = true;
        }
    }
    match result {
        ListKey::Opened => {
            close_other(ui, me);
            ui.open = me;
        }
        ListKey::Committed(_) => {
            let d = if me == Open::Sort { &ui.sort } else { &ui.country };
            let how = if tabbed_away { " (Tab)" } else { "" };
            ui.status = format!("Dropdown{how} : « {} »", d.selected_item().unwrap_or_default());
            close_all(ui);
        }
        ListKey::Closed => close_all(ui),
        _ => {}
    }
    let d = if me == Open::Sort { &ui.sort } else { &ui.country };
    d.paint_field(c, bounds, st.apply(live.state(trig)));
}

/// The status line after a toolbar command: what it did and the bar's state.
fn command_status(bar: &RichTextToolbar, cmd: RichTextCommand) -> String {
    let on: Vec<&str> = RichTextCommand::ALL.iter().filter(|c| bar.is_active(**c)).map(|c| c.label()).collect();
    let state = if on.is_empty() { "aucun".to_string() } else { on.join(", ") };
    format!("{} — actifs : {state}", cmd.label())
}

/// A tooltip under `anchor`, floated in an overlay window (the pointer passes
/// through it) and placed against the monitor, not the window.
fn float_tooltip(c: &dyn Canvas, f: &Frame, text: &str, anchor: Rect) {
    let tip = Tooltip::new(text).side(Side::Bottom);
    let size = tip.measure(c);
    let area = f.screen_area();
    let rel = Rect::new(anchor.left - area.left, anchor.top - area.top, anchor.right - area.left, anchor.bottom - area.top);
    let placed = place(rel, size, Side::Bottom, Size::new(area.right - area.left, area.bottom - area.top));
    let rect = Rect::new(placed.rect.left + area.left, placed.rect.top + area.top, placed.rect.right + area.left, placed.rect.bottom + area.top);
    let (tx, ty) = (placed.tip.0 + area.left, placed.tip.1 + area.top);
    let m = shadow_margin();
    let pb = Rect::new(rect.left.min(tx) - m, rect.top.min(ty) - m, rect.right.max(tx) + m, rect.bottom.max(ty) + m);
    let local = Placement { rect: rebase(rect, pb), side: placed.side, tip: (tx - pb.left, ty - pb.top) };
    host::overlay(pb, move |cv| tip.paint_placed(cv, &local, WidgetState::REST));
}

/// The four sort options, three of which carry an icon — so the whole list
/// reserves the gutter (`anyIcon`) and every label stays in one column.
fn sort_dropdown() -> Dropdown {
    let mut d = Dropdown::new();
    for (label, icon) in SORTS {
        d.add_option(label, icon);
    }
    d
}

/// Twenty countries, no icons.
fn country_dropdown() -> Dropdown {
    let mut d = Dropdown::new();
    for name in COUNTRIES {
        d.add_option(name, None);
    }
    d
}

/// An `Editable` holding `text` — one storage location, the `TextBox` replica.
fn named(text: &str) -> Editable {
    let mut e = Editable::new();
    e.set_text(text);
    e.placeholder_text = "Sans titre".into();
    e
}

/// A size field over the default presets.
fn size_field(value: f64) -> FontSizeField {
    FontSizeField::with_presets(&FontSizeField::DEFAULT_PRESETS, value).unwrap_or_else(|_| FontSizeField::new())
}

/// A picker over the families this machine has installed, with a couple of
/// them pinned as « Récentes » so the header block is visible.
fn picker_over_real_fonts() -> FontPicker {
    let mut p = FontPicker::system();
    // `recent` is a plain list of names: the ones present are pinned, the ones
    // absent are simply not there — no invented families.
    p.recent = ["Segoe UI", "Consolas", "Georgia"].iter().map(|s| s.to_string()).collect();
    p.placeholder = "Police mixte".into();
    let want = p.items.iter().position(|f| f == "Segoe UI").unwrap_or(0);
    p.set_selected_index(want as i32);
    p
}
