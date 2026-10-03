//! Gallery page — **feedback**.
//!
//! There is no desktop predecessor for any of these five, so nothing here is a
//! side-by-side pair: it is a variant × state sheet against the web components
//! the family was read from (`core/frontend/src/ui/{Spinner,Callout,Stepper,
//! EmptyState,Accordion}.tsx`).
//!
//! The spinner row paints the SAME spinner at four frozen phases, side by side,
//! so one screenshot shows the whole turn — plus one cell driven by the host's
//! clock (`host::now_ms` + `request_repaint_after`), which is the real
//! animation.
//!
//! The last row is the composition check: the same components inside narrow
//! cards, where the text must wrap (never drop a word), an unbreakable word
//! must break, and a horizontal stepper must switch to its compact summary.
//!
//! Every width comes from `f.size`, which is in DIP.

use std::cell::RefCell;

use kubuno_desktop_controls::host::{self, vk, Cursor, Frame, Modifiers};
use kubuno_desktop_ui::buttons::{Button, Size as ButtonSize, Variant as ButtonVariant};
use kubuno_desktop_ui::containers::Card;
use kubuno_desktop_ui::feedback::{
    Accordion, AccordionSection, AccordionSize, Callout, CalloutAction, CalloutVariant, EmptyState, EmptyStateVariant, HeaderKey, Spinner, SpinnerSize, Step,
    StepStatus, Stepper, EMPTY_STATE_VARIANTS, SPINNER_SIZES,
};
use kubuno_desktop_ui::{Canvas, FocusId, Rect, Widget, WidgetState};

use super::interact::{self, Live};
use super::sheet::{Page, CAPTION_H, MARGIN};

/// The four frozen phases the spinner row shows: the start of the turn, then
/// each quarter after it.
const PHASES: [f32; 4] = [0.0, 0.25, 0.5, 0.75];

/// The gap this page lays its cells out with.
const GAP: f32 = 12.0;

/// A small grey caption over a cell.
fn tag(c: &dyn Canvas, r: Rect, text: &str) {
    c.text(text, &r, &c.formats().caption, &c.theme().text_tertiary, false);
}

/// A card at `r`, returning its padded body.
fn card(c: &dyn Canvas, r: Rect) -> Rect {
    let card = Card::new();
    card.paint(c, r, WidgetState::REST);
    card.body_rect(r)
}

/// The vertical padding a `Card::new()` adds around its body.
fn card_pad() -> f32 {
    let r = Rect::new(0.0, 0.0, 400.0, 400.0);
    let b = Card::new().body_rect(r);
    (b.top - r.top) + (r.bottom - b.bottom)
}

/// Title and body per callout variant — four real sentences, because a banner
/// sized against « Lorem ipsum » is sized against nothing.
fn callout_text(v: CalloutVariant) -> (&'static str, &'static str) {
    match v {
        CalloutVariant::Info => (
            "Synchronisation",
            "Les fichiers hors ligne partiront à la reconnexion.",
        ),
        CalloutVariant::Success => (
            "Sauvegarde terminée",
            "Les 1 248 fichiers du dossier ont été copiés.",
        ),
        CalloutVariant::Warning => (
            "Espace bientôt plein",
            "Il reste 1,2 Go sur les 15 Go du quota de ce compte.",
        ),
        CalloutVariant::Danger => (
            "Envoi interrompu",
            "Le serveur a refusé trois fragments : la connexion a été coupée.",
        ),
    }
}

pub fn draw(c: &dyn Canvas, f: &Frame) {
    // The static exposition takes the window minus the interactive column, so it
    // lays out to the left of it rather than under it.
    let mut p = Page::new(c, f.size.0 - interact::PANEL_W(), f.size.1);
    let width = p.area.right - 2.0 * MARGIN;

    // ── Spinner ─────────────────────────────────────────────────────────────
    p.section("Spinner — quatre tailles, quatre phases figées, puis animé");
    let top = p.caption("xs 12 · sm 16 · md 24 · lg 32 — ¼ de tour par cellule · inactif · animé (1 tour/s)");
    // A cell is as wide as the largest ring (lg, 32 DIP); cells are spaced so
    // the lg rings never touch — at 32 DIP apart they sat rim to rim.
    let cell = 32.0;
    let pitch = cell + 12.0;
    let group = 4.0 * pitch + 12.0;
    for (col, size) in SPINNER_SIZES.iter().enumerate() {
        let x = MARGIN + col as f32 * group;
        tag(c, Rect::new(x, top, x + group, top + CAPTION_H), &format!("{size:?}"));
        for (i, phase) in PHASES.iter().enumerate() {
            let s = Spinner::new().with_size(*size).with_phase(*phase);
            let left = x + i as f32 * pitch;
            s.paint(
                c,
                Rect::new(left, top + CAPTION_H, left + cell, top + CAPTION_H + cell),
                WidgetState::REST,
            );
        }
    }
    // A disabled ring keeps turning, in grey: the work is still happening, it is
    // the surface that is inert.
    // The two live rings follow the groups on the same row when they fit, and
    // wrap onto a row of their own when the pane is too narrow — never cut at
    // the pane's edge.
    let band = CAPTION_H + cell;
    let (mut x, mut top, mut height) = (MARGIN + 4.0 * group, top, band);
    if x + 2.0 * pitch + cell > p.area.right - MARGIN {
        x = MARGIN;
        top += band + 12.0;
        height += band + 12.0;
    }
    tag(c, Rect::new(x, top, x + 2.0 * cell, top + CAPTION_H), "inactif");
    Spinner::new().with_size(SpinnerSize::Lg).at_time(host::now_ms()).paint(
        c,
        Rect::new(x, top + CAPTION_H, x + cell, top + CAPTION_H + cell),
        WidgetState::REST.disabled(true),
    );
    let x = x + 2.0 * pitch;
    tag(c, Rect::new(x, top, x + 2.0 * cell, top + CAPTION_H), "animé");
    Spinner::new().with_size(SpinnerSize::Lg).at_time(host::now_ms()).paint(
        c,
        Rect::new(x, top + CAPTION_H, x + cell, top + CAPTION_H + cell),
        WidgetState::REST,
    );
    host::request_repaint_after(Spinner::FRAME_MS);
    p.advance(height);

    // ── Callout ─────────────────────────────────────────────────────────────
    p.section("Callout — sévérités, action survolée, anneau de focus clavier");
    let top = p.caption(
        "gauche : action survolée · droite : croix au focus clavier (Tab) — puis deux corps seuls",
    );
    let col_w = (width - GAP) / 2.0;
    let mut used = 0.0_f32;
    for (i, variant) in [CalloutVariant::Info, CalloutVariant::Warning].iter().enumerate() {
        let (title, body) = callout_text(*variant);
        let x = MARGIN + i as f32 * (col_w + GAP);
        let mut full = Callout::new(body)
            .with_variant(*variant)
            .with_title(title)
            .with_action(CalloutAction::new("Réessayer").icon("RefreshCw"))
            .with_dismiss(true);
        // Per-part states: the left one has its action under the pointer, the
        // right one its close button focused from the keyboard.
        if i == 0 {
            full.action_state = Some(WidgetState::REST.hot(true));
        } else {
            full.dismiss_state = Some(WidgetState::REST.focused(true).focus_visible(true));
        }
        let h = full.height_at(c, col_w);
        full.paint(c, Rect::new(x, top, x + col_w, top + h), WidgetState::REST);

        let other = if i == 0 { CalloutVariant::Success } else { CalloutVariant::Danger };
        let bare = Callout::new(callout_text(other).1).with_variant(other);
        let bh = bare.height_at(c, col_w);
        let y = top + h + 8.0;
        bare.paint(c, Rect::new(x, y, x + col_w, y + bh), WidgetState::REST);
        used = used.max(h + 8.0 + bh);
    }
    p.advance(used);

    // ── Stepper ─────────────────────────────────────────────────────────────
    p.section("Stepper — la troisième étape EN ERREUR, survol et focus");
    let top = p.caption(
        "terminée · terminée · erreur · en cours · à venir (optionnelle) — survol sur « Stockage », \
         focus clavier sur « Compte »",
    );
    let mut stepper = Stepper::new()
        .step(Step::new("Compte").description("identité"))
        .step(Step::new("Stockage").description("quota"))
        .step(Step::new("Clés").description("clé refusée"))
        .step(Step::new("Partage").description("règles"))
        .step(Step::new("Résumé").description("vérification").optional(true))
        .with_optional_marker("optionnel")
        .with_counter_words("Étape", "sur")
        .at(3);
    stepper.steps[2].status = Some(StepStatus::Error);
    stepper.hovered = Some(1);
    stepper.focused = Some(0);
    let h = stepper.height_at(width);
    stepper.paint(c, Rect::new(MARGIN, top, MARGIN + width, top + h), WidgetState::REST);
    p.advance(h);

    // ── Composition: narrow cards ───────────────────────────────────────────
    p.section("Dans des cartes étroites — retour à la ligne, mot insécable, résumé compact");
    let top = p.caption(
        "EmptyState compact centré verticalement · Accordion (focus, désactivé, titre tronqué) · \
         Stepper < 560 → résumé · Callout avec un mot insécable",
    );
    let col = (width - 2.0 * GAP) / 3.0;
    let pad = card_pad();

    // Column 1 — an empty state whose title and description both wrap, in a
    // card taller than it needs: `justify-center` centres it.
    let empty = EmptyState::new("Search", "Aucun résultat pour « Anticonstitutionnellement »")
        .with_variant(EmptyStateVariant::NoResults)
        .with_compact(true)
        .with_description("Essayez un autre terme, ou retirez un des filtres actifs.")
        .with_action(Button::new("Effacer les filtres").size(ButtonSize::Sm))
        .with_doc("En savoir plus");

    // Column 2 — an accordion: a focused header, a disabled one, a long title.
    let mut acc = Accordion::new()
        .with_size(AccordionSize::Sm)
        .section(AccordionSection::new("Partage de liens", 28.0).icon("Share2").badge("3").open(true))
        .section(AccordionSection::new("Rétention", 0.0).icon("Clock").disabled(true))
        .section(
            AccordionSection::new("Chiffrement de bout en bout (expérimental)", 0.0)
                .icon("Lock")
                .badge("12"),
        );
    acc.focused = Some(2);
    acc.hovered = Some(0);

    // Column 3 — a stepper too narrow for its trail, then a callout carrying a
    // word longer than its column.
    let mut wizard = Stepper::new()
        .step(Step::new("Source").description("dossier local"))
        .step(Step::new("Correspondance").description("colonnes"))
        .step(Step::new("Validation").description("3 erreurs à corriger"))
        .step(Step::new("Import"))
        .with_counter_words("Étape", "sur")
        .at(2);
    wizard.steps[2].status = Some(StepStatus::Error);
    let long = Callout::new("Le fichier Anticonstitutionnellement_version_finale_définitive.pdf est verrouillé.")
        .with_variant(CalloutVariant::Danger)
        .with_title("Conflit de version sur un document partagé");

    let body_w = col - 2.0 * 16.0;
    let wizard_h = wizard.height_at(body_w);
    let long_h = long.height_at(c, col);
    let col3_h = wizard_h + pad + GAP + long_h;
    let acc_h = acc.total_height() + pad;
    let row_h = col3_h.max(acc_h).max(empty.height_at(c, body_w) + pad);

    // 1.
    let r1 = Rect::new(MARGIN, top, MARGIN + col, top + row_h);
    let b1 = card(c, r1);
    empty.paint(c, b1, WidgetState::REST);

    // 2.
    let x2 = r1.right + GAP;
    let r2 = Rect::new(x2, top, x2 + col, top + acc_h);
    let b2 = card(c, r2);
    let acc_rect = Rect::new(b2.left, b2.top, b2.right, b2.top + acc.total_height());
    acc.paint(c, acc_rect, WidgetState::REST);
    if let Some(&s0) = acc.section_rects(acc_rect).first() {
        let panel = acc.panel_rect(s0, 0);
        tag(c, panel, "contenu fourni par l'appelant");
    }

    // 3.
    let x3 = r2.right + GAP;
    let r3 = Rect::new(x3, top, x3 + col, top + wizard_h + pad);
    let b3 = card(c, r3);
    wizard.paint(c, Rect::new(b3.left, b3.top, b3.right, b3.top + wizard_h), WidgetState::REST);
    let y = r3.bottom + GAP;
    long.paint(c, Rect::new(x3, y, x3 + col, y + long_h), WidgetState::REST);

    p.advance(row_h);
}

/// What the interactive column remembers between frames: the live controls'
/// values, and last frame's button state so a click can be read as an edge.
#[derive(Default)]
struct Ui {
    /// The callout has been closed by its dismiss button.
    callout_dismissed: bool,
    /// How many times the callout's « Réessayer » ran.
    retries: u32,
    /// Which [`EMPTY_STATE_VARIANTS`] the empty state currently shows.
    empty_variant: usize,
    /// Whether each accordion section's panel is open.
    acc_open: [bool; 3],
    /// The stepper's current step index.
    step: usize,
    prev_down: bool,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

/// Enter or Space, the two keys that activate a focused `<button>`.
fn activated(live: &Live) -> bool {
    live.take_key(vk::ENTER, Modifiers::NONE) | live.take_key(vk::SPACE, Modifiers::NONE)
}

/// The right-hand column: the same feedback controls as the page, but live —
/// mouse AND keyboard. Tab walks every button in paint order; Enter / Space
/// activate the focused one; ↑ ↓ Home End move between accordion headers.
pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;

        let (left, mut y, right) = interact::panel(c, interact::panel_rect(f.size));
        let w = right - left;

        // ── Spinner — driven by the host clock ──────────────────────────────
        y = interact::caption(c, left, right, y, "Spinner — animé par l'horloge de l'hôte (1 tour/s)");
        let ring = 24.0;
        for (i, size) in SPINNER_SIZES.iter().enumerate() {
            let x = left + i as f32 * (ring + 12.0);
            Spinner::new().with_size(*size).at_time(host::now_ms()).paint(
                c,
                Rect::new(x, y, x + ring, y + ring),
                WidgetState::REST,
            );
        }
        host::request_repaint_after(Spinner::FRAME_MS);
        y += ring + 12.0;

        // ── Callout — action and close are two buttons ───────────────────────
        y = interact::caption(c, left, right, y, "Callout — Tab jusqu'à l'action ou la croix, Entrée");
        if ui.callout_dismissed {
            // Closed: a secondary button brings it back, so the interaction
            // stays testable rather than one-way.
            let restore = Button::new("Rétablir le bandeau").size(ButtonSize::Sm).variant(ButtonVariant::Secondary);
            let r = Rect::new(left, y, left + restore.width(c), y + ButtonSize::Sm.height());
            let st = live.focus_state("fb.callout.restore", r);
            if live.hit(r) || (st.focused && activated(&live)) {
                ui.callout_dismissed = false;
                interact::with_focus(|ring| ring.focus_visibly(FocusId::of("fb.callout.dismiss")));
            }
            restore.paint(c, r, st);
            y = r.bottom + 16.0;
        } else {
            let body = if ui.retries == 0 {
                "Les fichiers hors ligne partiront à la reconnexion.".to_string()
            } else {
                format!("Nouvelle tentative lancée ({} au total).", ui.retries)
            };
            let mut callout = Callout::new(body)
                .with_variant(CalloutVariant::Info)
                .with_title("Synchronisation")
                .with_action(CalloutAction::new("Réessayer").icon("RefreshCw"))
                .with_dismiss(true);
            let h = callout.height_at(c, w);
            let rect = Rect::new(left, y, right, y + h);
            let layout = callout.layout(c, rect);
            if let Some(a) = layout.action {
                let st = live.focus_state("fb.callout.action", a);
                if live.hit(a) || (st.focused && activated(&live)) {
                    ui.retries += 1;
                }
                callout.action_state = Some(st);
            }
            if let Some(d) = layout.dismiss {
                let st = live.focus_state("fb.callout.dismiss", d);
                if live.hit(d) || (st.focused && activated(&live)) {
                    ui.callout_dismissed = true;
                    interact::with_focus(|ring| ring.focus_visibly(FocusId::of("fb.callout.restore")));
                }
                callout.dismiss_state = Some(st);
            }
            // Each part carries its own state, so only the one under the
            // pointer lights and only the focused one wears the ring.
            callout.paint(c, rect, WidgetState::REST);
            y += h + 16.0;
        }

        // ── Accordion — click or Enter/Space toggles, arrows move ───────────
        y = interact::caption(c, left, right, y, "Accordion — clic / Entrée / Espace, ↑ ↓ Début Fin");
        let mut accordion = Accordion::new()
            .with_size(AccordionSize::Sm)
            .section(
                AccordionSection::new("Général", 32.0)
                    .icon("SlidersHorizontal")
                    .badge("4")
                    .open(ui.acc_open[0]),
            )
            .section(AccordionSection::new("Archives (réservé aux administrateurs)", 32.0).icon("Server").disabled(true))
            .section(
                AccordionSection::new("Partage", 32.0)
                    .icon("Share2")
                    .badge("12")
                    .open(ui.acc_open[2]),
            );
        let acc_rect = Rect::new(left, y, right, y + accordion.total_height());
        let sections = accordion.section_rects(acc_rect);
        accordion.track_pointer(acc_rect, live.mouse.0, live.mouse.1);
        let mut toggled = None;
        let mut move_to = None;
        for (i, rect) in sections.iter().enumerate() {
            let header = accordion.header_rect(*rect);
            if accordion.sections[i].disabled {
                // A disabled `<button>` is no Tab stop; `cursor-not-allowed`.
                if live.hover(header) {
                    host::set_cursor(Cursor::NotAllowed);
                }
                continue;
            }
            let st = live.focus(FocusId::indexed("fb.acc", i), header);
            if st.focused && st.visible {
                accordion.focused = Some(i);
            }
            if live.hit(header) || (st.focused && activated(&live)) {
                toggled = Some(i);
            }
            if st.focused {
                let key = if live.take_key(vk::DOWN, Modifiers::NONE) {
                    Some(HeaderKey::Next)
                } else if live.take_key(vk::UP, Modifiers::NONE) {
                    Some(HeaderKey::Previous)
                } else if live.take_key(vk::HOME, Modifiers::NONE) {
                    Some(HeaderKey::First)
                } else if live.take_key(vk::END, Modifiers::NONE) {
                    Some(HeaderKey::Last)
                } else {
                    None
                };
                move_to = key.and_then(|k| accordion.header_after_key(Some(i), k));
            }
        }
        if let Some(i) = toggled {
            if let Some(open) = ui.acc_open.get_mut(i) {
                *open = !*open;
                accordion.toggle(i); // keep this frame's paint in sync
            }
        }
        if let Some(j) = move_to {
            interact::with_focus(|ring| ring.focus_visibly(FocusId::indexed("fb.acc", j)));
            accordion.focused = Some(j);
        }
        // The stack may have grown or shrunk with the toggle.
        let acc_rect = Rect::new(left, y, right, y + accordion.total_height());
        accordion.paint(c, acc_rect, WidgetState::REST);
        for (i, rect) in accordion.section_rects(acc_rect).iter().enumerate() {
            if accordion.sections[i].is_open() {
                tag(c, accordion.panel_rect(*rect, i), "contenu du panneau");
            }
        }
        y += accordion.total_height() + 16.0;

        // ── EmptyState — its buttons are real Tab stops ──────────────────────
        y = interact::caption(c, left, right, y, "EmptyState — « Changer » (clic / Entrée) passe à la variante suivante");
        let variant = EMPTY_STATE_VARIANTS[ui.empty_variant % EMPTY_STATE_VARIANTS.len()];
        let mut empty = EmptyState::new("Inbox", "Aucun fichier partagé avec vous pour l'instant")
            .with_variant(variant)
            .with_compact(true)
            .with_description("Les fichiers partagés apparaîtront ici.")
            .with_action(Button::new("Changer").size(ButtonSize::Sm).icon("RefreshCw"))
            .with_secondary_action(Button::new("Aide").size(ButtonSize::Sm).variant(ButtonVariant::Ghost));
        let eh = empty.height_at(c, w);
        let erect = Rect::new(left, y, right, y + eh);
        for (slot, r) in empty.action_rects(c, erect) {
            let st = live.focus_state(FocusId::indexed("fb.empty", slot), r);
            if slot == 0 && (live.hit(r) || (st.focused && activated(&live))) {
                ui.empty_variant = (ui.empty_variant + 1) % EMPTY_STATE_VARIANTS.len();
            }
            if let Some(s) = empty.action_state.get_mut(slot) {
                *s = Some(st);
            }
        }
        empty.paint(c, erect, WidgetState::REST);
        y += eh + 12.0;

        // ── Stepper — reachable steps are buttons ─────────────────────────────
        y = interact::caption(c, left, right, y, "Stepper — clic / Entrée sur une étape atteinte, Précédent / Suivant");
        let last = 2; // three steps, zero-based
        let mut stepper = Stepper::new()
            .step(Step::new("Compte").description("identité"))
            .step(Step::new("Stockage").description("quota"))
            .step(Step::new("Résumé"))
            .with_counter_words("Étape", "sur")
            .at(ui.step)
            .vertical();
        let sh = stepper.total_height();
        let srect = Rect::new(left, y, right, y + sh);
        let rows = stepper.step_rects(srect);
        for i in stepper.reachable_steps() {
            let Some(&row) = rows.get(i) else { continue };
            let st = live.focus(FocusId::indexed("fb.step", i), row);
            if st.focused && st.visible {
                stepper.focused = Some(i);
            }
            if live.hover(row) {
                stepper.hovered = Some(i);
            }
            if live.hit(row) || (st.focused && activated(&live)) {
                ui.step = i;
            }
        }
        let btn_y = srect.bottom + 8.0;
        let prev = Rect::new(left, btn_y, left + 120.0, btn_y + ButtonSize::Sm.height());
        let next = Rect::new(prev.right + 8.0, btn_y, prev.right + 128.0, prev.bottom);
        let prev_st = live.focus_state("fb.prev", prev);
        let next_st = live.focus_state("fb.next", next);
        if live.hit(prev) || (prev_st.focused && activated(&live)) {
            ui.step = ui.step.saturating_sub(1);
        }
        if live.hit(next) || (next_st.focused && activated(&live)) {
            ui.step = (ui.step + 1).min(last);
        }
        stepper.current = ui.step;
        stepper.paint(c, srect, WidgetState::REST);
        Button::new("Précédent").size(ButtonSize::Sm).variant(ButtonVariant::Secondary).paint(c, prev, prev_st);
        Button::new("Suivant").size(ButtonSize::Sm).variant(ButtonVariant::Primary).paint(c, next, next_st);
    });
}
