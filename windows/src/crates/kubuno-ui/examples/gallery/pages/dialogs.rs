//! Gallery page — **dialogs**: the two confirmations, the prompt and its field,
//! the name conflict, a floating window placed in a host of its own, a popover,
//! and a stack of toasts.
//!
//! There is no predecessor to pair against (nothing in `drive-app-controls`
//! draws a dialog — that is the whole reason this family exists), so the static
//! exposition is a variant × state matrix rather than an old/new diff. The
//! things worth staring at are the **footer** — the confirming action on the
//! left, the cancel on the right, both at least 96 wide, in every dialog — the
//! **wrap** of a message quoting a long file name (it breaks after its hyphens
//! instead of running past the dialog's edge), the **focus-visible rings** a
//! keyboard user sees, and the content-wide toast cards. The **placement** of
//! a modal (`top: 33%` of its host, veil included) is shown live: every modal
//! the column opens goes through [`kubuno_ui::dialogs::place`] in the column.
//!
//! The static exposition lays out to the LEFT of the interactive column. The
//! floating surfaces (the toasts and the popover) are painted last, so their
//! shadows sit over everything else.
//!
//! The right-hand [`interactive_column`] makes the same surfaces LIVE, with the
//! keyboard as well as the pointer:
//!
//! * the open-buttons take the focus (Tab) and open on Entrée / Espace;
//! * a modal dialog **traps** Tab inside itself (only its parts are registered
//!   with the focus ring while it is up), starts on the web's initial focus
//!   (the confirming action, the prompt's field, « Annuler » for a conflict),
//!   closes on Échap, confirms on Entrée, and gives the focus back to the button
//!   that opened it;
//! * the prompt's field is EDITABLE: typing, Retour arrière / Suppr, arrows
//!   (Maj to select, Ctrl for words), Origine / Fin, Ctrl+A / C / X / V, click
//!   to place the caret, drag or Maj+click to select, double-click to select
//!   all;
//! * the popover lives in a `host::popup`, placed against the SCREEN: the
//!   « bord de fenêtre » trigger at the bottom of the column opens it past the
//!   window's edge. Arrows move its active row, Entrée chooses, Échap closes;
//! * toasts stack (four at most, the oldest dropped), expire on their web
//!   durations, pause under the pointer, and carry a working action and ✕.

use std::cell::RefCell;

use kubuno_controls::host::{self, vk, Cursor, Frame, InputEvent, Modifiers};
use kubuno_ui::buttons::{Button, Variant};
use kubuno_ui::containers::band;
use kubuno_ui::dialogs::{
    place, rebase, surface_bounds, ActionId, Actions, Align, ConfirmDialog, ConflictChoice, ConflictDialog,
    ConflictKind, DialogCommand, DialogKey, DialogPart, FieldEdit, FloatingWindow, Popover, PromptDialog,
    Toast, ToastPart, ToastPlacement, ToastQueue,
};
use kubuno_ui::display::{Label, Role, Side};
use kubuno_ui::focus::{FocusId, FocusOpts};
use kubuno_ui::metrics::{height, radius, space};
use kubuno_ui::{Canvas, DockStyle, Padding, Rect, Size, Widget, WidgetState};

use super::interact::{self, Live};
use super::sheet::NAV_H;

/// The static region's own margin — narrower than the shared `MARGIN`, because
/// two 400-wide columns only fit the left region with a little on each side.
const M: f32 = 8.0;
/// The gap that opens between one item and the next in a column.
const IG: f32 = 18.0;
/// A caption line's height plus the small gap under it.
const CAPTION: f32 = 18.0;
const CAP_ADV: f32 = CAPTION + 4.0;

/// The long file name the composition audit caught overflowing a dialog.
const LONG_NAME: &str = "Compte-rendu-comité-de-pilotage-2026-T3-relu-v4-final.docx";

pub fn draw(c: &dyn Canvas, f: &Frame) {
    let (w, h) = f.size;
    // The static exposition takes the window minus the interactive column, so it
    // lays out to the left of it rather than under it.
    let region = Rect::new(0.0, 0.0, w - interact::PANEL_W(), h);

    static_exposition(c, region);
}

// ═════════════════════════════════════════════════════════════════════════════
// The static exposition — two columns inside the left region.
// ═════════════════════════════════════════════════════════════════════════════

/// Where the exposition starts. `sheet::NAV_H` (44) counts the nav strip
/// only; the Kubuno chrome's caption band sits above it, so a caption at
/// `NAV_H + M` would be painted under the strip. 24 is that band.
const TOP: f32 = NAV_H + 24.0 + M;

fn static_exposition(c: &dyn Canvas, region: Rect) {
    // Two columns, each half the region minus the three margins between and
    // around them.
    let half = (region.right - 3.0 * M) / 2.0;
    let x_a = M;
    let x_b = M + half + M;

    // Column A — the destructive confirmation (long name, keyboard ring), the
    // conflict, and the popover at the bottom.
    let ya = confirm_danger(c, x_a, TOP, half);
    conflict_row(c, x_a, ya + IG, half);

    // Column B — the prompt in both its states, then a toast stack in the room
    // left under them.
    let mut yb = prompt_value(c, x_b, TOP, half);
    yb = prompt_empty(c, x_b, yb + IG, half);
    let stage = Rect::new(x_b, yb + IG, x_b + half, region.bottom - M);
    toasts(c, stage);

    // The popover last, so its shadow sits over everything else.
    popover_static(c, Rect::new(x_a, region.top, x_a + half, region.bottom - M));
}

/// A single caption line, returning the row's top edge below it.
fn caption(c: &dyn Canvas, x: f32, y: f32, right: f32, text: &str) -> f32 {
    let t = c.theme();
    c.text_ellipsis(text, &Rect::new(x, y, right, y + CAPTION), &c.formats().caption, &t.text_secondary);
    y + CAP_ADV
}

/// The destructive confirmation quoting a file name longer than a line — it
/// breaks after its separators and stays inside the dialog — as a keyboard
/// user sees it after one Tab: the ring on « Annuler », the pointer over
/// « Supprimer ».
fn confirm_danger(c: &dyn Canvas, x: f32, y: f32, width: f32) -> f32 {
    let y = caption(c, x, y, x + width, "ConfirmDialog — nom trop long coupé · Tab : anneau sur « Annuler »");
    let mut d = ConfirmDialog::danger("Supprimer définitivement ?", format!("« {LONG_NAME} » sera supprimé."))
        .labels("Supprimer", "Annuler");
    d.set_focus(Some(DialogPart::Action(ActionId::Cancel)), true);
    d.hot_action = Some(ActionId::Confirm);
    let s = d.measure_at(c, width + 2.0 * M);
    let r = Rect::new(x, y, x + s.width, y + s.height);
    d.paint(c, r, WidgetState::REST);
    r.bottom
}

fn prompt_value(c: &dyn Canvas, x: f32, y: f32, width: f32) -> f32 {
    let y = caption(c, x, y, x + width, "PromptDialog — champ pré-rempli et sélectionné (focus initial)");
    let p = PromptDialog::new("Renommer")
        .with_message("Nouveau nom du dossier :")
        .with_value("Photos de vacances")
        .with_placeholder("Sans titre");
    let s = p.measure_at(c, width + 2.0 * M);
    let r = Rect::new(x, y, x + s.width, y + s.height);
    p.paint(c, r, WidgetState::REST);
    r.bottom
}

/// The same dialog with an EMPTY value, so the **disabled** confirming action is
/// on the page too — the state `canConfirm` puts it in, and the one a caller
/// most often gets wrong.
fn prompt_empty(c: &dyn Canvas, x: f32, y: f32, width: f32) -> f32 {
    let y = caption(c, x, y, x + width, "PromptDialog — vide : action de confirmation désactivée");
    let empty = PromptDialog::new("Nouveau dossier").with_placeholder("Sans titre");
    let s = empty.measure_at(c, width + 2.0 * M);
    let r = Rect::new(x, y, x + s.width, y + s.height);
    empty.paint(c, r, WidgetState::REST);
    r.bottom
}

fn conflict_row(c: &dyn Canvas, x: f32, y: f32, width: f32) -> f32 {
    let y = caption(c, x, y, x + width, "ConflictDialog — 1re option survolée, 2e au focus clavier");
    let mut d = ConflictDialog::new("Rapport annuel.pdf", ConflictKind::File);
    // The first row lit, so the hovered frame and its accent wash are visible
    // without the pointer having to be there; the second wears the ring Tab
    // gives it.
    d.hot = Some(0);
    d.set_focus(Some(DialogPart::Option(1)), true);
    let s = d.measure_at(c, width + 2.0 * M);
    let r = Rect::new(x, y, x + s.width, y + s.height);
    d.paint(c, r, WidgetState::REST);
    r.bottom
}

/// Three cards, stacked by a [`ToastQueue`] in the bottom-right corner of
/// `stage` — the `bottom-4 right-4` anchor the provider uses. Each card is as
/// wide as its content (up to 24rem); the warning and the failure stack UNDER
/// the success, as the web's assertive live region follows the polite one.
fn toasts(c: &dyn Canvas, stage: Rect) {
    caption(c, stage.left, stage.top, stage.right, "Toasts — largeur au contenu · ✕ survolé · polis puis assertifs");
    let mut q = ToastQueue::new();
    q.push(
        Toast::error("Le serveur n'a pas répondu. Vérifiez votre connexion, puis réessayez.")
            .with_title("Échec de la synchronisation")
            .with_action("Réessayer"),
    );
    q.push(Toast::success("Enregistré."));
    let mut hot = Toast::warning("Trois fichiers ont été ignorés (format non pris en charge).")
        .with_title("Import partiel");
    hot.close_hot = true;
    q.push(hot);
    for (i, rect) in q.layout(c, stage, ToastPlacement::BottomRight) {
        q.items()[i].toast.paint(c, rect, WidgetState::REST);
    }
}

/// A menu-shaped popover on a fixed anchor at the bottom of `column`, opening
/// to its RIGHT with the bottom edges aligned (`Side::Right`, `Align::End`).
///
/// The static page shows it at rest; the live one — in a popup placed against
/// the screen — lives in the interactive column.
fn popover_static(c: &dyn Canvas, column: Rect) {
    let t = c.theme();
    let anchor = Rect::new(column.left, column.bottom - 28.0, column.left + 120.0, column.bottom);
    let pop = menu_popover(Side::Right).align(Align::End);
    let placement = pop.place(anchor, Size::new(column.right + M, column.bottom + M));
    // The caption goes ABOVE the panel, not above the anchor: the panel opens to
    // the anchor's right and grows upwards (bottom edges aligned), so a caption
    // on the anchor's row would run under it.
    let top = placement.rect.top.min(anchor.top) - CAP_ADV;
    caption(c, column.left, top, column.right, "Popover — ancre fixe, à droite");
    // A visible marker for the anchor the popover hangs off.
    c.stroke_rounded(&anchor, space::XS, &t.border_strong);
    c.text("Actions…", &anchor, &c.formats().body, &t.text_secondary, true);

    pop.paint(c, placement.rect, WidgetState::REST);
    paint_menu_rows(c, &pop, placement.rect, Some(1));
}

/// The popover's four rows.
const MENU_ROWS: [&str; 4] = ["Ouvrir", "Renommer…", "Télécharger", "Supprimer"];

/// The popover the menu is shown in, shared by the static example and the live
/// one. Its content (the rows) is painted by [`paint_menu_rows`], so the active
/// row's ground can sit UNDER its label.
fn menu_popover(side: Side) -> Popover {
    let size = Size::new(200.0, MENU_ROWS.len() as f32 * height::MENU_ITEM + 2.0 * space::XS);
    Popover::sized(size).side(side).align(Align::Start)
}

/// The row rectangles inside a popover painted at `panel`.
fn menu_row_rects(panel: Rect) -> Vec<Rect> {
    (0..MENU_ROWS.len())
        .map(|i| {
            let top = panel.top + space::XS + i as f32 * height::MENU_ITEM;
            Rect::new(panel.left + space::XS, top, panel.right - space::XS, top + height::MENU_ITEM)
        })
        .collect()
}

/// The rows, the `active` one on the hover ground (`MenuDropdown`'s rows are
/// `rounded-md` inside a `p-1` panel).
fn paint_menu_rows(c: &dyn Canvas, _pop: &Popover, panel: Rect, active: Option<usize>) {
    let t = c.theme();
    for (i, (label, row)) in MENU_ROWS.iter().zip(menu_row_rects(panel)).enumerate() {
        if active == Some(i) {
            c.fill_rounded(&row, radius::MENU_ITEM, &t.row_hover);
        }
        let text = Rect::new(row.left + space::MD, row.top, row.right - space::MD, row.bottom);
        let ink = if i == MENU_ROWS.len() - 1 { t.danger } else { t.text_primary };
        c.text_ellipsis(label, &text, &c.formats().body, &ink);
    }
}

/// The window the FloatingWindow button raises — the same body as the static
/// host example, so the two cannot drift.
fn window_widget() -> FloatingWindow {
    let mut w = FloatingWindow::new("Propriétés — Rapport annuel.pdf")
        .modal()
        .with_icon("FileText")
        .with_actions(Actions::pair("Appliquer", "Annuler"));
    // The body's children are a Panel's children: four docked rows, placed by
    // the layout engine and painted by the container family. They are pushed in
    // REVERSE reading order on purpose — the engine walks the child list
    // backwards, so the row added LAST takes the topmost band.
    {
        let body = w.body_mut();
        body.padding = Padding::all(space::XL);
        for line in [
            "Emplacement : Drive / Documents / 2026",
            "Modifié : aujourd'hui, 14:32",
            "Taille : 2,4 Mo",
            "Type : Document PDF",
        ] {
            body.push_widget(
                band(DockStyle::Top, height::MENU_ITEM),
                Box::new(Label::new(line).role(Role::Body)),
            );
        }
    }
    w.content_height = 2.0 * space::XL + 4.0 * height::MENU_ITEM;
    w.focus = w.initial_focus();
    w
}

// ═════════════════════════════════════════════════════════════════════════════
// The interactive column — the same surfaces, live under pointer and keyboard.
// ═════════════════════════════════════════════════════════════════════════════

/// Which modal surface is currently open. Only one modal is up at a time — a
/// modal veils the column, so a second cannot be reached under it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Modal {
    Confirm,
    Prompt,
    Conflict,
    Window,
}

/// What one of the open-buttons launches.
#[derive(Clone, Copy)]
enum Surface {
    Modal(Modal),
    Popover,
    Toast,
}

/// What the interactive column remembers between frames.
#[derive(Default)]
struct Ui {
    /// The open modal and the index of the button that opened it (the focus
    /// goes back there on close).
    modal: Option<(Modal, usize)>,
    /// The prompt, kept across frames while open: its text is being edited.
    prompt: Option<PromptDialog>,
    /// A focus to give on the next registration (id, visibly).
    pending_focus: Option<(FocusId, bool)>,
    /// A focus to give back next frame, once the page's buttons register again.
    restore: Option<(FocusId, bool)>,
    /// A drag selecting in the prompt's field.
    field_drag: bool,

    popover: bool,
    popover_anchor: Rect,
    popover_opener: usize,
    /// The popover's panel as placed last frame (client DIP).
    popover_rect: Option<Rect>,
    popover_active: Option<usize>,

    toasts: ToastQueue,
    /// Last frame's toast rectangles, by id (the top-of-frame click routing
    /// and the pause test read them).
    toast_rects: Vec<(u64, Rect)>,
    toast_seq: usize,
    last_tick: u64,

    result: String,
    prev_down: bool,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

/// The six surfaces, in the order they fill the 2 × 3 grid of open-buttons.
const SURFACES: [(&str, Surface); 6] = [
    ("ConfirmDialog", Surface::Modal(Modal::Confirm)),
    ("PromptDialog", Surface::Modal(Modal::Prompt)),
    ("ConflictDialog", Surface::Modal(Modal::Conflict)),
    ("FloatingWindow", Surface::Modal(Modal::Window)),
    ("Popover", Surface::Popover),
    ("Toast", Surface::Toast),
];
/// The seventh opener, at the bottom of the column: a popover that must hang
/// past the window's edge.
const EDGE_OPENER: usize = SURFACES.len();

fn opener_id(i: usize) -> FocusId {
    FocusId::indexed("dialogs.opener", i)
}

fn toast_part_id(id: u64, part: ToastPart) -> FocusId {
    let base = match part {
        ToastPart::Action => "dialogs.toast.action",
        ToastPart::Close => "dialogs.toast.close",
    };
    FocusId::indexed(base, id as usize)
}

pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let mut live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;

        let host_rect = interact::panel_rect(f.size);
        let now = host::now_ms();

        // ── Toast timers: count down unless the pointer rests on the stack.
        let paused = ui.toast_rects.iter().any(|(_, r)| r.contains(f.mouse.0, f.mouse.1));
        let elapsed = if ui.last_tick == 0 { 0 } else { now.saturating_sub(ui.last_tick) };
        ui.last_tick = now;
        if !ui.toasts.tick(elapsed, paused).is_empty() {
            ui.result = "Toast expiré".to_string();
        }
        if ui.toasts.is_timed() {
            host::request_repaint_after(100);
        }

        // ── A click on the desktop or another app light-dismisses the popover.
        if f.dismiss && ui.popover {
            ui.popover = false;
        }

        // ── Click routing against LAST frame's geometry, topmost first, so an
        //    open surface swallows the click the way the web's backdrop does.
        if live.clicked {
            if let Some((id, part)) = toast_part_under(c, &ui, live.mouse) {
                if let Some(part) = part {
                    activate_toast(&mut ui, id, part);
                }
                live.clicked = false;
            } else if ui.popover {
                if let Some(panel) = ui.popover_rect {
                    let (x, y) = live.mouse;
                    let row = menu_row_rects(panel).iter().position(|r| r.contains(x, y));
                    ui.result = match row {
                        Some(i) => format!("Menu : « {} »", MENU_ROWS[i]),
                        None if panel.contains(x, y) => ui.result.clone(),
                        None => "Menu fermé".to_string(),
                    };
                    if row.is_some() || !panel.contains(x, y) {
                        ui.popover = false;
                    }
                }
                live.clicked = false;
            }
        }
        // While the popover is up nothing under it lights up.
        let pointer = live.mouse;
        if ui.popover {
            live.mouse = (host::POINTER_AWAY, host::POINTER_AWAY);
        }

        // A focus to hand back from last frame's close.
        if let Some((id, visible)) = ui.restore.take() {
            interact::with_focus(|r| if visible { r.focus_visibly(id) } else { r.focus(id) });
        }

        let (left, y0, right) = interact::panel(c, host_rect);

        // ── The open-buttons — one grid cell each. Behind a modal they are
        //    veiled: no hover, no click, and NOT registered with the focus ring,
        //    which is what keeps Tab inside the dialog.
        let modal_up = ui.modal.is_some();
        let mut y = interact::caption(c, left, right, y0, "Ouvrez une surface — souris, ou Tab puis Entrée / Espace");
        let gap = 8.0;
        let bw = (right - left - gap) / 2.0;
        let bh = 40.0;
        for (i, &(label, surface)) in SURFACES.iter().enumerate() {
            let col = (i % 2) as f32;
            let row = (i / 2) as f32;
            let x = left + col * (bw + gap);
            let ty = y + row * (bh + gap);
            let rect = Rect::new(x, ty, x + bw, ty + bh);
            let st = opener(&live, &mut ui, i, rect, modal_up, surface);
            Button::new(label).variant(Variant::Secondary).paint(c, rect, st);
        }
        y += 3.0 * (bh + gap) + 4.0;

        // The last outcome, echoed so a dismissal leaves a trace of its choice.
        let echo = if ui.result.is_empty() { "—".to_string() } else { ui.result.clone() };
        c.text_ellipsis(
            &format!("Résultat : {echo}"),
            &Rect::new(left, y, right, y + 20.0),
            &c.formats().body,
            &c.theme().text_secondary,
        );
        y += 28.0;
        let help = [
            "Dialogue : Tab reste piégé dedans, Échap ferme, Entrée confirme.",
            "Invite : saisie, ← → (Maj, Ctrl), Ctrl+A/C/X/V, clic, glisser.",
            "Popover : ↑ ↓ Origine Fin, Entrée choisit, Échap ferme.",
            "Toasts : 4 au plus, pause au survol, action et ✕ actifs.",
        ];
        for line in help {
            c.text_ellipsis(line, &Rect::new(left, y, right, y + 18.0), &c.formats().caption, &c.theme().text_tertiary);
            y += 18.0;
        }

        // The edge opener: a popover from here must hang below the window.
        let edge = Rect::new(left, host_rect.bottom - space::LG - bh, left + bw, host_rect.bottom - space::LG);
        interact::caption(c, left, right, edge.top - 20.0, "Popover au bord de la fenêtre — il en déborde");
        let st = opener(&live, &mut ui, EDGE_OPENER, edge, modal_up, Surface::Popover);
        Button::new("Popover — bord de fenêtre").variant(Variant::Secondary).paint(c, edge, st);

        // ── The modal, if one is up.
        if let Some((m, opener)) = ui.modal {
            // A modal opened by a click THIS frame must not see that click too:
            // the pointer is still on the opener, i.e. on the modal's veil, and
            // the dialog would read it as « click outside » and dismiss itself
            // in the very frame it appeared. The click belongs to the opener.
            let mut modal_live = live;
            if !modal_up {
                modal_live.clicked = false;
            }
            if let Some((text, keyboard)) = run_modal(c, &modal_live, &mut ui, host_rect, m) {
                ui.modal = None;
                ui.prompt = None;
                ui.result = text;
                ui.restore = Some((opener_id(opener), keyboard));
            }
        }

        // ── The popover: in a popup against the screen.
        if ui.popover {
            run_popover(f, &live, &mut ui, pointer);
        } else {
            ui.popover_rect = None;
        }

        // ── The toasts paint last, over everything in the window.
        paint_toasts(c, &live, &mut ui, host_rect, modal_up);
    });
}

/// One open-button: its state, its focus, and what a click or Entrée / Espace
/// opens.
fn opener(live: &Live, ui: &mut Ui, i: usize, rect: Rect, modal_up: bool, surface: Surface) -> WidgetState {
    if modal_up {
        return WidgetState::REST;
    }
    let st = live.focus_state(opener_id(i), rect);
    // While the popover is open, Entrée / Espace are its (the opener keeps the
    // focus under an open menu, as on the web).
    let by_key = !ui.popover
        && st.focused
        && (live.take_key(vk::ENTER, Modifiers::NONE) || live.take_key(vk::SPACE, Modifiers::NONE));
    if live.hit(rect) || by_key {
        open_surface(ui, surface, rect, i, by_key);
    }
    st
}

/// Records which surface was opened, and where the focus goes.
fn open_surface(ui: &mut Ui, surface: Surface, anchor: Rect, opener: usize, by_key: bool) {
    match surface {
        Surface::Modal(m) => {
            ui.modal = Some((m, opener));
            ui.result.clear();
            let initial = match m {
                Modal::Confirm => live_confirm().initial_focus(),
                Modal::Prompt => {
                    let p = live_prompt();
                    let first = p.initial_focus();
                    ui.prompt = Some(p);
                    first
                }
                Modal::Conflict => live_conflict().initial_focus(),
                Modal::Window => window_widget().initial_focus(),
            };
            ui.pending_focus = initial.map(|p| (p.focus_id(), by_key));
        }
        Surface::Popover => {
            ui.popover = true;
            ui.popover_anchor = anchor;
            ui.popover_opener = opener;
            // Opened from the keyboard, a menu starts on its first row
            // (`MenuDropdown`); from a click, on nothing.
            ui.popover_active = by_key.then_some(0);
            ui.result.clear();
        }
        Surface::Toast => {
            let toast = next_toast(ui.toast_seq);
            ui.toast_seq += 1;
            ui.toasts.push(toast);
            ui.result = format!("{} toast(s) affiché(s)", ui.toasts.len());
        }
    }
}

/// The toasts the button raises in turn: short and content-wide, with an
/// action, a sticky failure, a titled warning.
fn next_toast(seq: usize) -> Toast {
    match seq % 4 {
        0 => Toast::success("Fichier enregistré dans « Documents ».").with_action("Annuler"),
        1 => Toast::info("Lien copié."),
        2 => Toast::warning("Trois fichiers ont été ignorés (format non pris en charge).").with_title("Import partiel"),
        _ => Toast::error(format!("« {LONG_NAME} » n'a pas pu être envoyé."))
            .with_title("Échec de l'envoi")
            .with_action("Réessayer")
            .sticky(),
    }
}

fn live_confirm() -> ConfirmDialog {
    ConfirmDialog::danger(
        "Supprimer 3 éléments ?",
        format!("« {LONG_NAME} » et deux autres éléments seront déplacés dans la corbeille."),
    )
    .labels("Supprimer", "Annuler")
}

fn live_prompt() -> PromptDialog {
    PromptDialog::new("Renommer")
        .with_message("Nouveau nom du fichier :")
        .with_value("Rapport annuel")
        .with_placeholder("Sans titre")
}

fn live_conflict() -> ConflictDialog {
    ConflictDialog::new("Rapport annuel.pdf", ConflictKind::File)
}

/// Paints the open modal and runs its pointer and keyboard. Returns the outcome
/// text (and whether the keyboard closed it) when it closes.
fn run_modal(c: &dyn Canvas, live: &Live, ui: &mut Ui, host_rect: Rect, m: Modal) -> Option<(String, bool)> {
    match m {
        Modal::Confirm => {
            let mut d = live_confirm();
            let bounds = place(host_rect, d.measure_at(c, host_rect.right - host_rect.left));
            let order = d.focus_order();
            drive_window(c, live, ui, &mut d, bounds, &order, &[]);
            let cmd = key_command(|k| d.key_command(k), live)
                .map(|cmd| (cmd, true))
                .or_else(|| click_command(c, live, &d, host_rect, bounds).map(|cmd| (cmd, false)));
            d.paint_modal(c, host_rect, bounds, WidgetState::REST);
            cmd.map(|(cmd, kb)| (outcome(cmd, "Supprimé"), kb))
        }
        Modal::Window => {
            let mut w = window_widget();
            let bounds = place(host_rect, w.measure_at(c, host_rect.right - host_rect.left));
            let order = w.focus_order();
            drive_window(c, live, ui, &mut w, bounds, &order, &[]);
            let cmd = key_command(|k| w.key_command(k), live)
                .map(|cmd| (cmd, true))
                .or_else(|| click_command(c, live, &w, host_rect, bounds).map(|cmd| (cmd, false)));
            w.paint_modal(c, host_rect, bounds, WidgetState::REST);
            cmd.map(|(cmd, kb)| (outcome(cmd, "Appliqué"), kb))
        }
        Modal::Conflict => {
            let mut d = live_conflict();
            let bounds = place(host_rect, d.measure_at(c, host_rect.right - host_rect.left));
            let order = d.focus_order();
            let rows = d.option_rects(c, bounds);
            drive_window(c, live, ui, &mut d, bounds, &order, &rows);
            d.hot = rows.iter().position(|r| live.hover(*r));
            let mut out = key_command(|k| d.key_command(k), live).map(|cmd| (cmd, true));
            if out.is_none() && live.clicked {
                let (x, y) = live.mouse;
                out = d
                    .choice_at(c, bounds, x, y)
                    .map(|ch| {
                        let i = if ch == ConflictChoice::Overwrite { 0 } else { 1 };
                        (DialogCommand::Activate(DialogPart::Option(i)), false)
                    })
                    .or_else(|| click_command(c, live, &d, host_rect, bounds).map(|cmd| (cmd, false)));
            }
            d.paint_modal(c, host_rect, bounds, WidgetState::REST);
            out.map(|(cmd, kb)| {
                let text = match cmd {
                    DialogCommand::Activate(p) => match d.choice_of(p) {
                        Some(ConflictChoice::Overwrite) => "Fichier écrasé",
                        Some(ConflictChoice::KeepBoth) => "Les deux conservés",
                        _ => "Annulé",
                    },
                    _ => "Annulé",
                };
                (text.to_string(), kb)
            })
        }
        Modal::Prompt => {
            let mut p = ui.prompt.take().unwrap_or_else(live_prompt);
            let bounds = place(host_rect, p.measure_at(c, host_rect.right - host_rect.left));
            let order = p.focus_order();
            let field = p.field_rect(c, bounds);
            drive_window(c, live, ui, &mut p, bounds, &order, &[field]);
            if p.focus == Some(DialogPart::Field) {
                edit_prompt(c, live, ui, &mut p, bounds, field);
            }
            let cmd = key_command(|k| p.key_command(k), live)
                .map(|cmd| (cmd, true))
                .or_else(|| click_command(c, live, &p, host_rect, bounds).map(|cmd| (cmd, false)));
            p.paint_modal(c, host_rect, bounds, WidgetState::REST);
            let text = p.field.text().to_string();
            let value = if text.is_empty() { "vide".to_string() } else { format!("« {text} »") };
            let out = cmd.map(|(cmd, kb)| (outcome(cmd, &format!("Renommé en {value}")), kb));
            if out.is_none() {
                ui.prompt = Some(p);
            }
            out
        }
    }
}

/// The shared part of every modal: registers its parts with the focus ring IN
/// TAB ORDER (the trap: nothing else is registered while it is up), applies the
/// initial focus, mirrors the ring's focus into the window, and lights the
/// footer / ✕ under the pointer.
///
/// `body` holds the rectangles of the body's own parts, in the order they
/// appear in `order` (a prompt's field, a conflict's two rows).
fn drive_window(
    c: &dyn Canvas,
    live: &Live,
    ui: &mut Ui,
    w: &mut FloatingWindow,
    bounds: Rect,
    order: &[DialogPart],
    body: &[Rect],
) {
    if let Some((id, visible)) = ui.pending_focus.take() {
        interact::with_focus(|r| if visible { r.focus_visibly(id) } else { r.focus(id) });
    }
    let mut body_rects = body.iter();
    let mut focus = None;
    let mut visible = false;
    for &part in order {
        let rect = match part {
            DialogPart::Field | DialogPart::Option(_) => body_rects.next().copied(),
            _ => w.part_rect(c, bounds, part),
        };
        let Some(rect) = rect else { continue };
        let opts = if part == DialogPart::Field { FocusOpts::TEXT } else { FocusOpts::default() };
        let st = live.focus_with(part.focus_id(), rect, opts);
        if st.focused {
            focus = Some(part);
            visible = st.visible;
        }
    }
    w.set_focus(focus, visible);
    let (x, y) = live.mouse;
    w.close_hot = w.close_hit(bounds, x, y);
    w.hot_action = w.action_at(c, bounds, x, y);
    w.pressed_action = w.hot_action.filter(|_| live.down);
    // The dialog is modal: a click on the veil must not blur what it holds.
    interact::with_focus(|r| r.keep_focus_in(Rect::new(-1.0e6, -1.0e6, 1.0e6, 1.0e6)));
}

/// Entrée / Maj+Entrée / Espace / Échap, taken from the queue only when the
/// dialog answers them (Tab is the focus ring's, taken before the page).
fn key_command(resolve: impl Fn(DialogKey) -> Option<DialogCommand>, live: &Live) -> Option<DialogCommand> {
    if host::key_pressed(vk::ESCAPE, Modifiers::NONE) || host::key_pressed(vk::ESCAPE, Modifiers::SHIFT) {
        if let Some(cmd) = resolve(DialogKey::Escape) {
            if live.take_escape() {
                return Some(cmd);
            }
        }
    }
    for (key, mods, dk) in [
        (vk::ENTER, Modifiers::NONE, DialogKey::Enter),
        (vk::ENTER, Modifiers::SHIFT, DialogKey::ShiftEnter),
        (vk::SPACE, Modifiers::NONE, DialogKey::Space),
    ] {
        if host::key_pressed(key, mods) {
            if let Some(cmd) = resolve(dk) {
                live.take_key(key, mods);
                return Some(cmd);
            }
        }
    }
    None
}

/// A click on a window's footer action, its ✕, or its veil.
fn click_command(c: &dyn Canvas, live: &Live, w: &FloatingWindow, host_rect: Rect, bounds: Rect) -> Option<DialogCommand> {
    if !live.clicked {
        return None;
    }
    let (x, y) = live.mouse;
    if let Some(id) = w.action_at(c, bounds, x, y) {
        return Some(DialogCommand::Activate(DialogPart::Action(id)));
    }
    if w.close_hit(bounds, x, y) {
        return Some(DialogCommand::Activate(DialogPart::Close));
    }
    w.dismisses(host_rect, bounds, x, y).then_some(DialogCommand::Dismiss)
}

/// The outcome text of a closing command.
fn outcome(cmd: DialogCommand, confirmed: &str) -> String {
    match cmd {
        DialogCommand::Activate(DialogPart::Action(ActionId::Confirm)) => confirmed.to_string(),
        _ => "Annulé".to_string(),
    }
}

/// The prompt's field, while focused: typed text and editing keys (in the order
/// they were typed), the clipboard, and the pointer (click, Maj+click, drag,
/// double-click).
fn edit_prompt(c: &dyn Canvas, live: &Live, ui: &mut Ui, p: &mut PromptDialog, bounds: Rect, field: Rect) {
    let multiline = p.field.multiline;
    let clip_keys = [vk::letter('c'), vk::letter('x'), vk::letter('v')];
    let events = host::consume(|e| match e {
        InputEvent::Text(_) => true,
        InputEvent::Key { vk: k, down: true, mods, .. } => {
            FieldEdit::from_key(*k, *mods, multiline).is_some() || (mods.matches(Modifiers::CTRL) && clip_keys.contains(k))
        }
        _ => false,
    });
    for e in events {
        match e {
            InputEvent::Text(s) => {
                p.edit(FieldEdit::Insert(s));
            }
            InputEvent::Key { vk: k, mods, .. } => {
                if mods.matches(Modifiers::CTRL) && k == vk::letter('c') {
                    let sel = p.selected_text();
                    if !sel.is_empty() {
                        host::set_clipboard_text(&sel);
                    }
                } else if mods.matches(Modifiers::CTRL) && k == vk::letter('x') {
                    let sel = p.selected_text();
                    if !sel.is_empty() && host::set_clipboard_text(&sel) {
                        p.edit(FieldEdit::Insert(String::new()));
                    }
                } else if mods.matches(Modifiers::CTRL) && k == vk::letter('v') {
                    if let Some(t) = host::clipboard_text() {
                        p.edit(FieldEdit::Insert(t));
                    }
                } else if let Some(edit) = FieldEdit::from_key(k, mods, multiline) {
                    p.edit(edit);
                }
            }
            _ => {}
        }
    }

    // The pointer: an I-beam over the field, a click places the caret, Maj+click
    // or a drag extends, a double-click selects everything.
    let (x, _) = live.mouse;
    if live.hover(field) {
        host::set_cursor(Cursor::IBeam);
    }
    if live.clicked && live.hover(field) {
        if live.click_count >= 2 {
            p.edit(FieldEdit::SelectAll);
        } else {
            let at = p.caret_at(c, bounds, x);
            p.set_caret(at, live.mods.shift);
            ui.field_drag = true;
        }
    } else if ui.field_drag && live.down {
        let at = p.caret_at(c, bounds, x);
        if at != p.caret() {
            p.set_caret(at, true);
        }
    }
    if !live.down {
        ui.field_drag = false;
    }
}

/// The popover in its popup: placed against the screen, rows lit by the
/// pointer or the arrows, Entrée to choose, Échap / Tab to close.
fn run_popover(f: &Frame, live: &Live, ui: &mut Ui, pointer: (f32, f32)) {
    let pop = menu_popover(Side::Bottom);
    let placement = pop.place_in(ui.popover_anchor, f.screen_area());
    let panel = placement.rect;
    ui.popover_rect = Some(panel);
    let rows = menu_row_rects(panel);
    let n = rows.len();

    // Hover sets the active row, as `MenuDropdown` does.
    if let Some(i) = rows.iter().position(|r| r.contains(pointer.0, pointer.1)) {
        ui.popover_active = Some(i);
    }
    // The keyboard: the page's queue, even though the pointer is in the popup.
    if live.take_key(vk::DOWN, Modifiers::NONE) {
        ui.popover_active = Some(ui.popover_active.map_or(0, |i| (i + 1) % n));
    }
    if live.take_key(vk::UP, Modifiers::NONE) {
        ui.popover_active = Some(ui.popover_active.map_or(n - 1, |i| (i + n - 1) % n));
    }
    if live.take_key(vk::HOME, Modifiers::NONE) {
        ui.popover_active = Some(0);
    }
    if live.take_key(vk::END, Modifiers::NONE) {
        ui.popover_active = Some(n - 1);
    }
    let choose = live.take_key(vk::ENTER, Modifiers::NONE) || live.take_key(vk::SPACE, Modifiers::NONE);
    if choose {
        if let Some(i) = ui.popover_active {
            ui.result = format!("Menu : « {} »", MENU_ROWS[i]);
            close_popover(ui, true);
            return;
        }
    }
    if live.take_escape() {
        ui.result = "Menu fermé (Échap)".to_string();
        close_popover(ui, true);
        return;
    }
    // Tab is the focus ring's (taken before the page): once it has moved the
    // focus off the opener, the menu closes — a menu does not survive its
    // trigger losing the focus.
    let opener = opener_id(ui.popover_opener);
    if interact::with_focus(|r| r.focused()) != Some(opener) {
        ui.result = "Menu fermé (focus parti)".to_string();
        ui.popover = false;
        ui.popover_rect = None;
        return;
    }
    interact::with_focus(|r| r.keep_focus_in(panel));

    // A pointer-cursor hint on the rows; the paint happens in the popup, whose
    // closure draws with the popup's top-left at the origin.
    if rows.iter().any(|r| r.contains(pointer.0, pointer.1)) {
        host::set_cursor(Cursor::Hand);
    }
    let bounds = surface_bounds(panel);
    let local = rebase(panel, bounds);
    let active = ui.popover_active;
    host::popup(bounds, move |canvas| {
        pop.paint(canvas, local, WidgetState::REST);
        paint_menu_rows(canvas, &pop, local, active);
    });
}

fn close_popover(ui: &mut Ui, keyboard: bool) {
    ui.popover = false;
    ui.popover_rect = None;
    ui.restore = Some((opener_id(ui.popover_opener), keyboard));
}

/// The toast (and its button) under `p`, from last frame's layout.
fn toast_part_under(c: &dyn Canvas, ui: &Ui, p: (f32, f32)) -> Option<(u64, Option<ToastPart>)> {
    let (id, rect) = ui.toast_rects.iter().rev().find(|(_, r)| r.contains(p.0, p.1)).copied()?;
    let q = ui.toasts.items().iter().find(|q| q.id == id)?;
    Some((id, q.toast.part_at(c, rect, p.0, p.1)))
}

fn activate_toast(ui: &mut Ui, id: u64, part: ToastPart) {
    let label = ui
        .toasts
        .items()
        .iter()
        .find(|q| q.id == id)
        .and_then(|q| q.toast.action.clone())
        .unwrap_or_default();
    ui.toasts.dismiss(id);
    ui.result = match part {
        ToastPart::Action => format!("Action du toast : « {label} »"),
        ToastPart::Close => "Toast fermé".to_string(),
    };
}

/// Lays the stack out, lights and focuses its buttons, runs Entrée / Espace on
/// a focused one, and paints it.
fn paint_toasts(c: &dyn Canvas, live: &Live, ui: &mut Ui, host_rect: Rect, modal_up: bool) {
    let layout = ui.toasts.layout(c, host_rect, ToastPlacement::BottomRight);
    let (x, y) = live.mouse;
    let mut activate = None;
    let mut rects = Vec::new();
    for (i, rect) in layout {
        let Some(q) = ui.toasts.items_mut().get_mut(i) else { continue };
        let id = q.id;
        rects.push((id, rect));
        let toast = &mut q.toast;
        let action = toast.action_rect(c, rect);
        toast.close_hot = toast.close_hit(rect, x, y);
        toast.action_hot = action.is_some_and(|r| r.contains(x, y));
        toast.focus = None;
        toast.focus_visible = false;
        if !modal_up {
            // Registered after everything else: Tab reaches them last, as the
            // provider's portal comes last in the DOM.
            for part in toast.focus_order() {
                let r = if part == ToastPart::Action { action } else { Some(toast.close_rect(rect)) };
                let Some(r) = r else { continue };
                let st = live.focus(toast_part_id(id, part), r);
                if st.focused {
                    toast.focus = Some(part);
                    toast.focus_visible = st.visible;
                    if live.take_key(vk::ENTER, Modifiers::NONE) || live.take_key(vk::SPACE, Modifiers::NONE) {
                        activate = Some((id, part));
                    }
                }
            }
        }
        if toast.close_hot || toast.action_hot {
            host::set_cursor(Cursor::Hand);
        }
        toast.paint(c, rect, WidgetState::REST);
    }
    ui.toast_rects = rects;
    if let Some((id, part)) = activate {
        activate_toast(ui, id, part);
    }
}
