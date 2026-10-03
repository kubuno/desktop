//! Gallery page — the list family.
//!
//! None of these four primitives has a hand-written predecessor in
//! `drive-app-controls`, so there is no [`super::sheet::pair`] here: there is
//! nothing to sit beside. What the page shows instead is every **state** the
//! family can be in, painted from the same inputs the unit tests use:
//!
//! * list boxes — a selection with the keyboard's focus ring and active row, a
//!   multi-selection, three check states, a disabled list, then a list longer
//!   than its box (scroll indicator, `TopIndex`) and labels too long for it
//!   (ellipsis);
//! * combos — closed, keyboard focus, hover, open with more items than
//!   `MaxDropDownItems` (so the list scrolls and says so), disabled, and a long
//!   selection ellipsized in a narrow field;
//! * a full `MenuDropdown` with icons, shortcuts, a section label, separators,
//!   a checked row, a dead row, a cascaded submenu and a destructive entry —
//!   and a second menu pressed against the pane's right edge, whose submenu
//!   flips to the LEFT as the web's does.
//!
//! The interactive column is live and keyboard-driven through the gallery's
//! focus ring: Tab reaches each control; the lists answer arrows, Home/End,
//! PageUp/PageDown, Space and type-ahead, and scroll with the wheel; the combo
//! opens with Alt+Down / Down / Enter / Space in a `host::popup` that may leave
//! the window; the menu (trigger or right click) runs in a popup too, with
//! arrows, Right/Left for the submenu, Enter and Escape.

use std::cell::RefCell;

use kubuno_controls::enums::CheckState;
use kubuno_controls::host::{self, vk, Cursor, Frame, Modifiers};
use kubuno_controls::lists::{ComboBoxStyle, SelectionMode};
use kubuno_controls::toolstrip::StripItem;
use kubuno_ui::lists::{section, separator, FLOAT_SHADOW_MARGIN};
use kubuno_ui::lists::{
    CheckedListBox, ComboBox, ComboKey, ListBox, ListKey, Menu, MenuEntry, MenuKey, MenuOutcome,
};
use kubuno_ui::{Canvas, Rect, Widget, WidgetState};

use super::interact::{self, Live};
use super::sheet::{cells, Page, MARGIN};

/// The gap between two cells on this page.
const GAP: f32 = 16.0;
/// Four cells across: the four states each family member is shown in.
const COLUMNS: usize = 4;
/// Five rows is enough to show a selection, a hover and the rows around them.
const ROWS: usize = 5;
/// How many rows the open combo drops in the exposition — `MaxDropDownItems`,
/// cut below the six languages so the list has to scroll.
const DROP_ROWS: i32 = 4;
/// The live combo shows five of its six languages: « Português » is reached by
/// scrolling, which the indicator announces.
const LIVE_DROP_ROWS: i32 = 5;

const FOLDERS: [&str; 5] = ["Documents", "Images", "Musique", "Téléchargements", "Vidéos"];
const MANY: [&str; 10] = [
    "Archives",
    "Bureau",
    "Documents",
    "Images",
    "Modèles",
    "Musique",
    "Projets",
    "Public",
    "Téléchargements",
    "Vidéos",
];
const LANGUAGES: [&str; 6] = ["Deutsch", "English", "Español", "Français", "Italiano", "Português"];

pub fn draw(c: &dyn Canvas, f: &Frame) {
    // The static exposition takes the window minus the interactive column, so it
    // lays out to the left of it rather than under it.
    let mut page = Page::new(c, f.size.0 - interact::PANEL_W(), f.size.1);
    let (mx, my) = f.mouse;
    let right = page.area.right - MARGIN;
    let cell_w = ((right - MARGIN) - (COLUMNS as f32 - 1.0) * GAP) / COLUMNS as f32;

    // ── ListBox ──────────────────────────────────────────────────────────────
    page.section("ListBox");
    let top = page.caption(
        "focus clavier (anneau + ligne active) · sélection multiple (MultiExtended) · cases à cocher · désactivée",
    );
    let h = ListBox::new().height_for_rows(ROWS);
    let row = cells(MARGIN, top, cell_w, h, GAP, COLUMNS);

    // 1 — single selection, focused from the keyboard: the ring and the active
    //     row (moved with Ctrl+arrow, off the selection) are both visible.
    let mut single = filled_list(&FOLDERS);
    single.set_selected_index(1);
    single.focus_index = Some(3);
    single.hot_index = single.item_at(row[0], mx, my);
    single.paint(c, row[0], WidgetState::REST.focused(true).focus_visible(true));

    // 2 — a multi-selection, built with the replica's own click machine so the
    //     page and the tests exercise the same code path.
    let mut multi = filled_list(&FOLDERS);
    multi.set_selection_mode(SelectionMode::MultiExtended);
    multi.click(1, false, false);
    multi.click(3, false, true); // Shift extends the range: 1, 2, 3.
    multi.hot_index = multi.item_at(row[1], mx, my);
    multi.paint(c, row[1], WidgetState::REST);

    // 3 — check boxes, including the Indeterminate state only
    //     `SetItemCheckState` can produce.
    let mut checked = CheckedListBox::new();
    for (i, name) in FOLDERS.iter().enumerate() {
        let state = match i {
            0 | 4 => CheckState::Checked,
            2 => CheckState::Indeterminate,
            _ => CheckState::Unchecked,
        };
        checked.add_item(*name, state);
    }
    checked.set_selected_index(0);
    checked.hot_index = checked.item_at(row[2], mx, my);
    checked.paint(c, row[2], WidgetState::REST);

    // 4 — disabled.
    let mut dead = filled_list(&FOLDERS);
    dead.set_selected_index(2);
    dead.paint(c, row[3], WidgetState::REST.disabled(true));
    page.advance(h);

    // ── ComboBox ─────────────────────────────────────────────────────────────
    page.section("ComboBox — et, dessous, le débordement des listes");
    let top = page.caption(
        "fermée · focus clavier · ouverte (6 langues, 4 visibles : défile) · désactivée — dessous : défilée, ellipses",
    );
    // The open one is the tallest: field + its 2 DIP offset + the drop-down.
    let band = {
        let probe = filled_combo(&LANGUAGES, DROP_ROWS);
        probe.field_height() + 2.0 + probe.drop_down_height()
    };
    let row = cells(MARGIN, top, cell_w, band, GAP, COLUMNS);

    let closed = filled_combo(&LANGUAGES, DROP_ROWS);
    let closed_field = closed.field_rect(row[0]);
    closed.paint(c, row[0], WidgetState::REST.hot(closed_field.contains(mx, my)));
    filled_combo(&LANGUAGES, DROP_ROWS).paint(c, row[1], WidgetState::REST.focused(true).focus_visible(true));

    // Under the first two fields, the list overflow cases: a list longer than
    // its box (scrolled, with its indicator), and labels wider than the list
    // (ellipsis, never a spill).
    let under = closed.field_height() + GAP;
    let lh = ListBox::new().height_for_rows(3);
    let below = |r: Rect| Rect::new(r.left, r.top + under, r.right, r.top + under + lh);
    let mut long = filled_list(&MANY);
    long.set_top_index(3);
    long.set_selected_index(4);
    long.hot_index = long.item_at(below(row[0]), mx, my);
    long.paint(c, below(row[0]), WidgetState::REST);

    let mut wordy = ListBox::new();
    for s in [
        "Rapport annuel de l'équipe documentation 2026.pdf",
        "Présentation client — version définitive (3).pptx",
        "Budget prévisionnel consolidé des filiales.xlsx",
    ] {
        wordy.add_item(s);
    }
    wordy.set_selected_index(0);
    wordy.hot_index = wordy.item_at(below(row[1]), mx, my);
    wordy.paint(c, below(row[1]), WidgetState::REST);

    let mut open = filled_combo(&LANGUAGES, DROP_ROWS);
    open.set_selected_index(3);
    open.open(); // Starts on « Français », scrolled into view.
    open.hot_index = open.item_at(row[2], mx, my).or(open.hot_index);
    open.paint(c, row[2], WidgetState::REST);

    let mut disabled = filled_combo(&LANGUAGES, DROP_ROWS);
    // A `DropDownList` is the non-editable style: the field is a button, and a
    // dead one shows the input's own disabled face.
    disabled.drop_down_style = ComboBoxStyle::DropDownList;
    disabled.paint(c, row[3], WidgetState::REST.disabled(true));

    // Under the disabled one: a long selection in a narrow field is truncated
    // before the chevron, as `truncate` does.
    let mut wordy = ComboBox::new();
    wordy.add_item("Téléchargements partagés de l'équipe documentation");
    wordy.set_selected_index(0);
    let narrow = Rect::new(row[3].left, row[3].top + under, row[3].right, row[3].top + under + wordy.field_height());
    wordy.paint(c, narrow, WidgetState::REST.hot(narrow.contains(mx, my)));
    page.advance(band);

    // ── Menu ─────────────────────────────────────────────────────────────────
    page.section("Menu — le MenuDropdown du produit");
    let top = page.caption(
        "icônes · raccourcis · séparateurs · en-tête · cochée · désactivée · sous-menu — au bord : sous-menu à GAUCHE",
    );

    let viewport = Rect::new(page.area.left, page.area.top, right, f.size.1);
    let mut menu = context_menu();
    menu.viewport = Some(viewport);
    let want = menu.measure(c);
    let panel = Rect::new(MARGIN, top, MARGIN + want.width, top + want.height);
    menu.hot_index = menu.item_at(panel, mx, my).filter(|&i| menu.is_actionable(i));
    menu.open_submenu = Some(SUBMENU_ROW);
    menu.paint(c, panel, WidgetState::REST);

    // The second menu hugs the right edge: no room for the cascade on the
    // right, so `submenu_rect_in` opens it on the left of its row. Its long
    // labels also prove the panel grows to its content (no ellipsis).
    let mut edge = row_menu();
    edge.viewport = Some(viewport);
    let want2 = edge.measure(c);
    let left2 = right - want2.width;
    let mut panel2 = Rect::new(left2, top, right, top + want2.height);
    edge.hot_index = Some(EDGE_SUBMENU_ROW);
    edge.open_submenu = Some(EDGE_SUBMENU_ROW);
    edge.submenu_hot = Some(1);
    // When the pane is too narrow for both cascades side by side, the edge
    // menu's left-flipped submenu would land on « Ouvrir avec »'s cascade:
    // drop the edge menu until its submenu starts under that cascade, so
    // neither submenu covers the other.
    let sub1 = menu.submenu_rect_in(c, panel, SUBMENU_ROW);
    let sub2 = edge.submenu_rect_in(c, panel2, EDGE_SUBMENU_ROW);
    if let (Some(s1), Some(s2)) = (sub1, sub2) {
        let clash = s2.left < s1.right + GAP && s2.right + GAP > s1.left;
        let drop = s1.bottom + GAP - s2.top;
        if clash && drop > 0.0 {
            // Never below the window: past that the page would clip it.
            let room = (f.size.1 - MARGIN - panel2.bottom).max(0.0);
            let dy = drop.min(room);
            panel2 = Rect::new(panel2.left, panel2.top + dy, panel2.right, panel2.bottom + dy);
        }
    }
    edge.paint(c, panel2, WidgetState::REST);

    page.advance(want.height.max(panel2.bottom - top));
}

/// Which row of [`context_menu`] owns the cascaded submenu.
const SUBMENU_ROW: usize = 3;
/// Which row of [`row_menu`] owns its submenu.
const EDGE_SUBMENU_ROW: usize = 2;

/// Which surface the open menu came from.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum MenuFrom {
    #[default]
    Trigger,
    Pointer,
}

/// What the interactive column remembers between frames.
#[derive(Default)]
struct Ui {
    /// The live controls themselves: their selection, active row, scroll
    /// position and type-ahead buffer persist from frame to frame.
    list:        Option<ListBox>,
    clb:         Option<CheckedListBox>,
    combo:       Option<ComboBox>,
    menu:        Option<Menu>,
    menu_from:   MenuFrom,
    /// Where the menu opens (client DIP): under the trigger, or at the pointer.
    menu_at:     (f32, f32),
    /// Last frame's geometry, in client DIP: the menu panel, its cascaded
    /// submenu, the combo's dropped list, the right-click zone and the combo
    /// field. A click is routed against these at the top of the frame, before
    /// anything underneath can see it — the web's backdrop.
    menu_panel:  Option<Rect>,
    menu_sub:    Option<Rect>,
    combo_panel: Option<Rect>,
    combo_field: Option<Rect>,
    menu_zone:   Option<Rect>,
    /// The last thing chosen, shown under the zone.
    choice:      Option<String>,
    /// The pointer last frame: hover only moves an active row when the pointer
    /// actually moves, so it does not fight the arrows.
    last_mouse:  (f32, f32),
    prev_down:   bool,
    prev_right:  bool,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

/// Focus ids of the live controls (unique within the page).
const ID_LIST: &str = "lists.listbox";
const ID_CLB: &str = "lists.checked";
const ID_COMBO: &str = "lists.combo";
const ID_MENU: &str = "lists.menu-trigger";

/// The right-hand column: the same four primitives as the page, live — mouse
/// AND keyboard.
pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let ui = &mut *ui;
        let mut live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;
        let right_click = f.right_down && !ui.prev_right;
        ui.prev_right = f.right_down;
        let moved = f.mouse != ui.last_mouse;
        ui.last_mouse = f.mouse;

        let mut list = ui.list.take().unwrap_or_else(|| filled_list(&MANY));
        let mut clb = ui.clb.take().unwrap_or_else(|| {
            let mut c = CheckedListBox::new();
            for name in MANY {
                c.add_item(name, CheckState::Unchecked);
            }
            c
        });
        let mut combo = ui.combo.take().unwrap_or_else(|| filled_combo(&LANGUAGES, LIVE_DROP_ROWS));
        list.set_selection_mode(SelectionMode::MultiExtended);

        // Losing the window (a click on the desktop or another app) closes every
        // floating surface, as `useMenuDismiss` does on `window.blur`.
        if f.dismiss {
            ui.menu = None;
            combo.close();
        }

        // ── Click routing for the open surfaces (last frame's geometry) ──────
        let (px, py) = f.mouse;
        if let Some(menu) = ui.menu.as_mut() {
            let on_zone = ui.menu_zone.is_some_and(|z| z.contains(px, py));
            if live.clicked || (right_click && !on_zone) {
                let outcome = route_menu_click(menu, ui.menu_panel, ui.menu_sub, px, py, live.clicked);
                match outcome {
                    Some(MenuOutcome::Chosen { index, sub }) => {
                        ui.choice = chosen_label(menu, index, sub);
                        ui.menu = None;
                    }
                    Some(_) => {}
                    None => ui.menu = None,
                }
                live.clicked = false;
            }
            live.mouse = (host::POINTER_AWAY, host::POINTER_AWAY);
        }
        if combo.open && live.clicked {
            // A click in the list picks a row; a click on the field toggles it
            // shut (both swallowed, so the field does not reopen it). Anywhere
            // else `Combobox.tsx`' outside `pointerdown` closes the list and
            // the click still reaches what it landed on — there is no backdrop.
            let on_field = ui.combo_field.is_some_and(|r| r.contains(px, py));
            match ui.combo_panel.filter(|p| p.contains(px, py)) {
                Some(panel) => {
                    if let Some(i) = combo.item_at_panel(panel, px, py) {
                        combo.commit(i);
                        ui.choice = Some(format!("Langue : {}", LANGUAGES[i]));
                    }
                    live.clicked = false;
                }
                None => {
                    combo.close();
                    if on_field {
                        live.clicked = false;
                    }
                }
            }
        }
        if combo.open && ui.combo_panel.is_some_and(|p| p.contains(px, py)) {
            // The pointer is over the dropped list: nothing under it lights up.
            live.mouse = (host::POINTER_AWAY, host::POINTER_AWAY);
        }
        let keys_free = ui.menu.is_none();
        let (mx, my) = live.mouse;

        let (left, mut y, right) = interact::panel(c, interact::panel_rect(f.size));

        // ── ListBox — MultiExtended, 10 items in 5 rows ─────────────────────
        y = interact::caption(c, left, right, y, "ListBox — clic, Maj, Ctrl, flèches, Début/Fin, PgPréc/Suiv, frappe");
        let list_rect = Rect::new(left, y, right, y + list.height_for_rows(ROWS));
        let st = live.focus(ID_LIST, list_rect);
        if live.clicked {
            if let Some(i) = list.item_at(list_rect, mx, my) {
                list.pointer_select(i, live.mods.ctrl, live.mods.shift);
            }
        }
        if st.focused && keys_free {
            for (key, mods) in take_list_keys() {
                list.handle_key(list_rect, key, mods.ctrl, mods.shift);
            }
            for mods in host::take_key_any(vk::SPACE) {
                list.press_space(mods.ctrl);
            }
            let typed = typed_text();
            if !typed.is_empty() {
                list.type_to_select(list_rect, &typed, host::now_ms());
            }
        }
        let rows = wheel_rows(&live, list_rect, list.row_height());
        if rows != 0 {
            list.scroll_rows(list_rect, rows);
        }
        list.hot_index = list.item_at(list_rect, mx, my);
        if list.hot_index.is_some() {
            host::set_cursor(Cursor::Hand);
        }
        list.paint(c, list_rect, st.apply(WidgetState::REST));
        y = list_rect.bottom + 16.0;

        // ── CheckedListBox — the well toggles, the label selects ────────────
        y = interact::caption(c, left, right, y, "CheckedListBox — case, Espace, flèches, frappe, molette");
        let clb_rect = Rect::new(left, y, right, y + clb.height_for_rows(4));
        let st = live.focus(ID_CLB, clb_rect);
        if live.clicked {
            if let Some(i) = clb.item_at(clb_rect, mx, my) {
                let on_well = clb.check_at(clb_rect, mx, my).is_some();
                clb.pointer_select(i, on_well);
            }
        }
        if st.focused && keys_free {
            for (key, _) in take_list_keys() {
                clb.handle_key(clb_rect, key);
            }
            if host::take_key(vk::SPACE, Modifiers::NONE) > 0 {
                clb.press_space();
            }
            let typed = typed_text();
            if !typed.is_empty() {
                clb.type_to_select(clb_rect, &typed, host::now_ms());
            }
        }
        let rows = wheel_rows(&live, clb_rect, clb.row_height());
        if rows != 0 {
            clb.scroll_rows(clb_rect, rows);
        }
        clb.hot_index = clb.item_at(clb_rect, mx, my);
        if clb.hot_index.is_some() {
            host::set_cursor(Cursor::Hand);
        }
        clb.paint(c, clb_rect, st.apply(WidgetState::REST));
        y = clb_rect.bottom + 16.0;

        // ── ComboBox — its list in a popup that may leave the window ────────
        y = interact::caption(c, left, right, y, "ComboBox — clic, Alt+↓, ↓, Entrée, flèches, Échap, frappe");
        let combo_bounds = Rect::new(left, y, right, y + combo.field_height());
        let field = combo.field_rect(combo_bounds);
        let st = live.focus(ID_COMBO, field);
        if live.hit(field) {
            combo.open();
        }
        if st.focused && keys_free {
            for key in take_combo_keys(combo.open) {
                if let kubuno_ui::lists::ComboOutcome::Committed(i) = combo.handle_key(key) {
                    ui.choice = Some(format!("Langue : {}", LANGUAGES[i]));
                }
            }
            let typed = typed_text();
            if !typed.is_empty() {
                combo.type_to_select(&typed, host::now_ms());
            }
        } else if combo.open && !st.gained {
            // Tab (or a click elsewhere) took the focus: the list closes.
            combo.close();
        }
        let area = f.screen_area();
        if combo.open {
            let panel = combo.drop_down_rect_in(combo_bounds, area);
            let (px, py) = f.mouse;
            if panel.contains(px, py) {
                let rows = if f.wheel.1 != 0.0 { (f.wheel.1 * host::WHEEL_NOTCH_DIP / combo.row_height()).round() as i32 } else { 0 };
                if rows != 0 {
                    combo.scroll_drop_down(rows);
                    host::claim_wheel();
                }
                if moved {
                    if let Some(i) = combo.item_at_panel(panel, px, py) {
                        combo.hot_index = Some(i);
                    }
                }
                host::set_cursor(Cursor::Hand);
            }
            // The open list keeps the focus on the combo when it is clicked.
            interact::with_focus(|r| r.keep_focus_in(panel));
            let pb = Rect::new(
                panel.left - FLOAT_SHADOW_MARGIN,
                panel.top - FLOAT_SHADOW_MARGIN,
                panel.right + FLOAT_SHADOW_MARGIN,
                panel.bottom + FLOAT_SHADOW_MARGIN,
            );
            let local = rebase(panel, pb);
            let snapshot = combo.clone();
            host::popup(pb, move |canvas| snapshot.paint_drop_down_at(canvas, local));
            ui.combo_panel = Some(panel);
        } else {
            ui.combo_panel = None;
        }
        ui.combo_field = Some(field);
        combo.paint_trigger(c, combo_bounds, st.apply(live.state(field)));
        y = combo_bounds.bottom + 16.0;

        // ── Menu — from its trigger, or as a context menu on a right click ──
        y = interact::caption(c, left, right, y, "Menu — clic, Entrée ou ↓, clic droit ; ↑↓ → ← Entrée Échap");
        let trigger = Rect::new(left, y, left + 200.0, y + 32.0);
        let tst = live.focus(ID_MENU, trigger);
        let trigger_state = tst.apply(live.state(trigger));
        paint_trigger(c, trigger, ui.menu.is_some() && ui.menu_from == MenuFrom::Trigger, trigger_state);
        let key_open = tst.focused
            && ui.menu.is_none()
            && (host::take_key(vk::ENTER, Modifiers::NONE) > 0
                || host::take_key(vk::SPACE, Modifiers::NONE) > 0
                || host::take_key(vk::DOWN, Modifiers::NONE) > 0);
        if live.hit(trigger) || key_open {
            let mut m = context_menu();
            if key_open {
                // ARIA menu button: opening from the keyboard lands on the
                // first item.
                m.navigate(MenuKey::Down);
            }
            ui.menu = Some(m);
            ui.menu_from = MenuFrom::Trigger;
            // `useMenuDropdown.open`: `top: r.bottom + 2, left: r.left`.
            ui.menu_at = (trigger.left, trigger.bottom + 2.0);
        }
        y += 32.0 + 12.0;

        let t = c.theme();
        let zone = Rect::new(left, y, right, y + 64.0);
        c.fill_rounded(&zone, 6.0, &t.surface_2);
        c.stroke_rounded(&zone, 6.0, &t.card_stroke);
        c.text("Clic droit ici — le menu s'ouvre au pointeur", &zone, &c.formats().body, &t.text_secondary, true);
        if right_click && zone.contains(f.mouse.0, f.mouse.1) {
            ui.menu = Some(context_menu());
            ui.menu_from = MenuFrom::Pointer;
            ui.menu_at = f.mouse;
        }
        ui.menu_zone = Some(zone);
        y += 64.0 + 8.0;
        let chosen = match &ui.choice {
            Some(label) => format!("Dernier choix : {label}"),
            None => "Aucun choix pour l'instant".to_string(),
        };
        interact::caption(c, left, right, y, &chosen);

        if let Some(menu) = ui.menu.as_mut() {
            // ── Keyboard: the queue reaches the main window while the popup is
            //    open, so the menu reads its keys here.
            let mut outcome = MenuOutcome::Ignored;
            for (vk_code, key) in [
                (vk::UP, MenuKey::Up),
                (vk::DOWN, MenuKey::Down),
                (vk::HOME, MenuKey::Home),
                (vk::END, MenuKey::End),
                (vk::RIGHT, MenuKey::Right),
                (vk::LEFT, MenuKey::Left),
                (vk::ENTER, MenuKey::Enter),
                (vk::SPACE, MenuKey::Space),
            ] {
                for _ in 0..host::take_key(vk_code, Modifiers::NONE) {
                    outcome = menu.navigate(key);
                    if matches!(outcome, MenuOutcome::Chosen { .. } | MenuOutcome::Close) {
                        break;
                    }
                }
                if matches!(outcome, MenuOutcome::Chosen { .. } | MenuOutcome::Close) {
                    break;
                }
            }
            if live.take_escape() && !matches!(outcome, MenuOutcome::Chosen { .. }) {
                outcome = menu.navigate(MenuKey::Escape);
            }
            let typed = typed_text();
            if !typed.is_empty() {
                menu.type_to_focus(&typed);
            }
            match outcome {
                MenuOutcome::Chosen { index, sub } => {
                    ui.choice = chosen_label(menu, index, sub);
                    ui.menu = None;
                    interact::with_focus(|r| r.focus_visibly(ID_MENU));
                }
                MenuOutcome::Close => {
                    ui.menu = None;
                    if ui.menu_from == MenuFrom::Trigger {
                        // Escape gives the focus back to the menu button.
                        interact::with_focus(|r| r.focus_visibly(ID_MENU));
                    }
                }
                _ => {}
            }
        }

        if let Some(menu) = ui.menu.as_mut() {
            let area = f.screen_area();
            menu.viewport = Some(area);
            let want = menu.measure(c);
            // `MenuDropdown`'s clamp: open at the point, then pulled back
            // inside the viewport (8 DIP) on every side.
            const EDGE: f32 = kubuno_ui::lists::VIEWPORT_EDGE;
            let (ax, ay) = ui.menu_at;
            let x = ax.min(area.right - EDGE - want.width).max(area.left + EDGE);
            let top = ay.min(area.bottom - EDGE - want.height).max(area.top + EDGE);
            let panel = Rect::new(x, top, x + want.width, top + want.height);

            // Hover (only when the pointer moves, so it does not fight the
            // arrows) behaves like the web's roving focus: only a pointer
            // moving OVER another actionable row moves the highlight. In the
            // submenu it lights the child row and keeps the parent lit; in the
            // panel it moves the hot row and opens / closes the cascade like
            // `SubmenuItem`'s `onMouseEnter`. Leaving the menu, or crossing a
            // separator, a section label or a dead row, keeps the highlight
            // where the keyboard (or the last hovered row) left it.
            let (px, py) = f.mouse;
            let sub_rect = menu.open_submenu.and_then(|i| menu.submenu_rect_in(c, panel, i));
            if moved {
                if let (Some(sr), Some(parent)) = (sub_rect.filter(|r| r.contains(px, py)), menu.open_submenu) {
                    if let Some(sub) = menu.submenu(parent) {
                        if let Some(j) = sub.item_at(sr, px, py).filter(|&j| sub.is_actionable(j)) {
                            menu.submenu_hot = Some(j);
                            menu.hot_index = Some(parent);
                        }
                    }
                } else if panel.contains(px, py) {
                    let hot = menu.item_at(panel, px, py).filter(|&i| menu.is_actionable(i));
                    if let Some(i) = hot {
                        menu.hot_index = Some(i);
                        menu.submenu_hot = None;
                        menu.open_submenu = menu.submenu(i).is_some().then_some(i);
                    }
                }
            }
            // A dead row says so under the pointer; a live one is clickable.
            let over = menu.item_at(panel, px, py);
            match over {
                Some(i) if menu.is_actionable(i) => host::set_cursor(Cursor::Hand),
                Some(_) if panel.contains(px, py) => host::set_cursor(Cursor::NotAllowed),
                _ => {}
            }
            let sub_rect = menu.open_submenu.and_then(|i| menu.submenu_rect_in(c, panel, i));
            ui.menu_panel = Some(panel);
            ui.menu_sub = sub_rect;

            // One popup covers the panel, the submenu when open, and the shadow.
            let pb = menu.paint_bounds(c, panel);
            interact::with_focus(|r| r.keep_focus_in(pb));
            let mut snapshot = menu.clone();
            snapshot.viewport = Some(rebase(area, pb));
            let local = rebase(panel, pb);
            host::popup(pb, move |canvas| snapshot.paint(canvas, local, WidgetState::REST));
        } else {
            ui.menu_panel = None;
            ui.menu_sub = None;
        }

        ui.list = Some(list);
        ui.clb = Some(clb);
        ui.combo = Some(combo);
    });
}

/// `r` in the local space of a popup whose bounds are `pb`.
fn rebase(r: Rect, pb: Rect) -> Rect {
    Rect::new(r.left - pb.left, r.top - pb.top, r.right - pb.left, r.bottom - pb.top)
}

/// Takes every list navigation key of this frame, in order, with its
/// modifiers. Alt chords are left alone (they belong to the window).
fn take_list_keys() -> Vec<(ListKey, Modifiers)> {
    let mut out = Vec::new();
    for e in host::consume(|e| match e {
        host::InputEvent::Key { vk: code, down: true, mods, .. } => {
            !mods.alt
                && [vk::UP, vk::DOWN, vk::HOME, vk::END, vk::PAGE_UP, vk::PAGE_DOWN].contains(code)
        }
        _ => false,
    }) {
        if let host::InputEvent::Key { vk: code, mods, .. } = e {
            let key = match code {
                c if c == vk::UP => ListKey::Up,
                c if c == vk::DOWN => ListKey::Down,
                c if c == vk::HOME => ListKey::Home,
                c if c == vk::END => ListKey::End,
                c if c == vk::PAGE_UP => ListKey::PageUp,
                _ => ListKey::PageDown,
            };
            out.push((key, mods));
        }
    }
    out
}

/// Takes the combo's keys of this frame. Escape is taken through the focus
/// ring only while the list is open, so a closed combo lets it through.
fn take_combo_keys(open: bool) -> Vec<ComboKey> {
    let mut out = Vec::new();
    for e in host::consume(|e| match e {
        host::InputEvent::Key { vk: code, down: true, mods, .. } => {
            let nav = [vk::UP, vk::DOWN].contains(code) && (mods.is_none() || mods.matches(Modifiers::ALT));
            let plain = mods.is_none()
                && [vk::HOME, vk::END, vk::PAGE_UP, vk::PAGE_DOWN, vk::ENTER, vk::SPACE, vk::F4].contains(code);
            nav || plain
        }
        _ => false,
    }) {
        if let host::InputEvent::Key { vk: code, mods, .. } = e {
            let key = match code {
                c if c == vk::UP && mods.alt => ComboKey::AltUp,
                c if c == vk::DOWN && mods.alt => ComboKey::AltDown,
                c if c == vk::UP => ComboKey::Up,
                c if c == vk::DOWN => ComboKey::Down,
                c if c == vk::HOME => ComboKey::Home,
                c if c == vk::END => ComboKey::End,
                c if c == vk::PAGE_UP => ComboKey::PageUp,
                c if c == vk::PAGE_DOWN => ComboKey::PageDown,
                c if c == vk::ENTER => ComboKey::Enter,
                c if c == vk::SPACE => ComboKey::Space,
                _ => ComboKey::F4,
            };
            out.push(key);
        }
    }
    if open && interact::with_focus(|r| r.take_escape()) {
        out.push(ComboKey::Escape);
    }
    out
}

/// This frame's typed text for type-ahead, without the space (Space is a
/// command in a list, not a character to search for).
fn typed_text() -> String {
    host::take_text().chars().filter(|c| !c.is_whitespace()).collect()
}

/// Wheel travel over `r` in whole rows (positive = down).
fn wheel_rows(live: &Live, r: Rect, row_h: f32) -> i32 {
    let (_, dy) = live.wheel_over(r);
    if dy == 0.0 || row_h <= 0.0 {
        return 0;
    }
    (dy / row_h).round() as i32
}

/// Routes a click (or a right click) made while the menu is open, against
/// last frame's geometry. `None` = it missed the menu (close it); otherwise
/// what it did. A left click on an enabled leaf chooses it; on a submenu's row
/// it opens the cascade; a dead row, a separator or the padding do nothing.
fn route_menu_click(
    menu: &mut Menu,
    panel: Option<Rect>,
    sub: Option<Rect>,
    x: f32,
    y: f32,
    left: bool,
) -> Option<MenuOutcome> {
    if let (Some(sr), Some(parent)) = (sub.filter(|r| r.contains(x, y)), menu.open_submenu) {
        let child = menu.submenu(parent)?;
        return Some(match child.item_at(sr, x, y) {
            Some(j) if left && child.is_actionable(j) && child.submenu(j).is_none() => {
                MenuOutcome::Chosen { index: parent, sub: Some(j) }
            }
            _ => MenuOutcome::Ignored,
        });
    }
    let panel = panel.filter(|p| p.contains(x, y))?;
    Some(match menu.item_at(panel, x, y) {
        Some(i) if left && menu.is_actionable(i) => {
            if menu.submenu(i).is_some() {
                menu.hot_index = Some(i);
                menu.navigate(MenuKey::Right)
            } else {
                MenuOutcome::Chosen { index: i, sub: None }
            }
        }
        _ => MenuOutcome::Ignored,
    })
}

/// The label of what was chosen.
fn chosen_label(menu: &Menu, index: usize, sub: Option<usize>) -> Option<String> {
    let item = match sub {
        Some(j) => menu.submenu(index)?.items().get(j)?.clone(),
        None => menu.items().get(index)?.clone(),
    };
    match item {
        StripItem::MenuItem(m) => Some(m.base.item.text.clone()),
        _ => None,
    }
}

/// The menu's anchor button. It is not a kubuno primitive — a menu is opened
/// from a caller's own control — so it is painted plainly from theme tokens:
/// a bordered field that lights on hover, shows its open state, and wears the
/// accent ring on keyboard focus.
fn paint_trigger(c: &dyn Canvas, r: Rect, open: bool, state: WidgetState) {
    let t = c.theme();
    let f = c.formats();
    let face = if open || state.pressed { t.surface_2 } else { t.layer_background };
    c.fill_rounded(&r, 6.0, &face);
    if state.show_focus_ring() {
        c.stroke_rounded_w(&r, 6.0, &t.accent, 2.0);
    } else {
        let stroke = if state.hot { &t.border_strong } else { &t.card_stroke };
        c.stroke_rounded(&r, 6.0, stroke);
    }
    let label = Rect::new(r.left + 12.0, r.top, r.right - 12.0, r.bottom);
    c.text_ellipsis("Menu contextuel", &label, &f.body, &t.text_primary);
}

fn filled_list(items: &[&str]) -> ListBox {
    let mut l = ListBox::new();
    for name in items {
        l.add_item(*name);
    }
    l
}

fn filled_combo(items: &[&str], rows: i32) -> ComboBox {
    let mut c = ComboBox::new();
    for name in items {
        c.add_item(*name);
    }
    c.max_drop_down_items = rows;
    c.set_selected_index(0);
    c
}

/// The « Ouvrir avec » submenu.
fn sub_items() -> Vec<StripItem> {
    vec![
        MenuEntry::new("Éditeur de code").icon("FileEdit").build(),
        MenuEntry::new("Aperçu").icon("PlayCircle").build(),
        separator(),
        MenuEntry::new("Choisir une application…").build(),
    ]
}

/// The full menu, built out of the replica's item model.
fn context_menu() -> Menu {
    Menu::with_items(vec![
        section("Actions"),
        MenuEntry::new("Ouvrir").icon("FolderOpen").shortcut(true, false, false, "O").build(),
        MenuEntry::new("Renommer").icon("FileEdit").shortcut(false, false, false, "F2").build(),
        MenuEntry::new("Ouvrir avec").icon("FileText").submenu(sub_items()).build(),
        separator(),
        MenuEntry::new("Partager").icon("Share2").shortcut(true, false, true, "P").build(),
        MenuEntry::new("Ajouter aux favoris").icon("Star").checked(true).build(),
        MenuEntry::new("Historique des versions").icon("Clock").enabled(false).build(),
        separator(),
        MenuEntry::new("Supprimer").icon("Trash2").shortcut_text("Suppr").danger().build(),
    ])
}

/// A data-table row menu with long labels — the composition audit's case.
fn row_menu() -> Menu {
    Menu::with_items(vec![
        MenuEntry::new("Modifier le profil").icon("PenLine").build(),
        MenuEntry::new("Réinitialiser le mot de passe").icon("KeyRound").build(),
        MenuEntry::new("Changer de rôle")
            .icon("Users")
            .submenu(vec![
                MenuEntry::new("Administrateur").build(),
                MenuEntry::new("Membre").checked(true).build(),
                MenuEntry::new("Invité").build(),
            ])
            .build(),
        MenuEntry::new("Transférer la propriété…").icon("ArrowRightLeft").build(),
        separator(),
        MenuEntry::new("Supprimer définitivement").icon("Trash2").danger().build(),
    ])
}
