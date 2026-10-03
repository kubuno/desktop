//! Gallery page — the form-field family.
//!
//! `OutlinedField` is the base every form field is built on (`@ui/OutlinedField`).
//! The exposition shows it in every state the web distinguishes and a few the
//! desktop adds: resting (the label is the hint, in `text-secondary`), focused
//! (the label floats onto the border in the primary colour, the notch opens,
//! the caret shows), filled, a selection, the focused `placeholder`, a leading
//! icon, a trailing glyph (read-only select trigger), required, overflow (a
//! long value is clipped to the padding box, a long label is ellipsised),
//! password, invalid, disabled, large and a wrapping multiline field. Every
//! cell is laid out at `measure()`'s height: the floated label lives in the
//! field's own headroom, never above its bounds. Then the composites: a
//! `LabelCombobox` closed and open (its list filtered by the typed text), a
//! `FieldGroup` under one icon, and the `FloatCheckbox` over media cards.
//!
//! Under the fields, the help family: the « ? » [`HelpButton`] in its states
//! (the ring only on keyboard focus) and two [`HelpBubble`]s placed against
//! it — one below, one above with a second action and its focus ring.
//!
//! The right-hand column is live and fully editable: typing, selection with
//! the mouse (drag, double and triple click, Shift+click) and the keyboard
//! (arrows, Ctrl+arrows, Home/End, Shift), Backspace/Delete, Ctrl+A/C/X/V,
//! Ctrl+Z/Y, Tab between the controls (Tab-ing in selects the value), Enter
//! to « submit » a single-line field, a multiline field that wraps and scrolls
//! with the wheel. A live `FieldGroup` (its chevron toggles on click, Enter or
//! Space) and a `LabelCombobox` whose list opens on focus in a popup, narrows
//! as you type, and answers Up/Down (Alt+Down opens), Enter (pick), Escape
//! (close), a click on a row (pick) or anywhere else (close), and the wheel.
//! Under them, a label with a live « ? »: Enter or Space opens
//! its bubble in an interactive popup (it may hang past the window), « OK »
//! takes the focus, Tab cycles its buttons, Escape or « OK » closes it and
//! gives the focus back to the « ? »; a click anywhere else closes it too.

use std::cell::RefCell;

use kubuno_controls::host::{self, vk, Frame, InputEvent, Modifiers};
use kubuno_ui::buttons::{Button, Variant};
use kubuno_ui::display::Side;
use kubuno_ui::fields::{FieldGroup, FloatCheckbox, LabelCombobox, OutlinedField};
use kubuno_ui::focus::{FocusId, FocusOpts};
use kubuno_ui::help::{self, HelpBubble, HelpButton, HelpKey, HelpPart, HelpPlacement};
use kubuno_ui::{Canvas, Rect, Widget, WidgetState};

use super::interact::{self, Live};
use super::sheet::{self, Page};

/// The exposition's grid: four columns of fields.
const COLS: usize = 4;
const COL_GAP: f32 = 24.0;
const CAPTION_GAP: f32 = 4.0;
/// The mock media card a `FloatCheckbox` floats over, and the check's inset
/// from its corner (`absolute top-2 left-2`).
const CARD_W: f32 = 34.0;
const CARD_H: f32 = 34.0;
const CARD_GAP: f32 = 8.0;
const CHECK_INSET: f32 = 4.0;
/// `place_list` keeps its list this far inside the rectangle it is given;
/// the static exposition widens its cell by as much so the list stays flush.
const LIST_EDGE: f32 = 8.0;

/// Where the next exposition cell goes.
struct Grid {
    left: f32,
    col_w: f32,
    top: f32,
    col: usize,
    row_h: f32,
}

impl Grid {
    fn new(left: f32, right: f32, top: f32) -> Self {
        let col_w = ((right - left - COL_GAP * (COLS as f32 - 1.0)) / COLS as f32).max(160.0);
        Self { left, col_w, top, col: 0, row_h: 0.0 }
    }

    /// Paints one field under its caption, at the field's own measured height.
    fn field(&mut self, c: &dyn Canvas, caption: &str, field: &OutlinedField, state: WidgetState) {
        let t = c.theme();
        let f = c.formats();
        let x = self.left + self.col as f32 * (self.col_w + COL_GAP);
        let cap = Rect::new(x, self.top, x + self.col_w, self.top + sheet::CAPTION_H);
        c.text_ellipsis(caption, &cap, &f.caption, &t.text_secondary);
        let h = field.measure(c).height;
        let y = cap.bottom + CAPTION_GAP;
        field.paint(c, Rect::new(x, y, x + self.col_w, y + h), state);
        self.row_h = self.row_h.max(sheet::CAPTION_H + CAPTION_GAP + h);
        self.col += 1;
        if self.col == COLS {
            self.next_row();
        }
    }

    /// A cell spanning `span` columns, `h` tall under its caption; `paint`
    /// gets the cell's rectangle.
    fn cell(&mut self, c: &dyn Canvas, caption: &str, span: usize, h: f32, paint: impl FnOnce(Rect)) {
        let t = c.theme();
        let f = c.formats();
        if self.col + span > COLS {
            self.next_row();
        }
        let x = self.left + self.col as f32 * (self.col_w + COL_GAP);
        let w = span as f32 * self.col_w + (span as f32 - 1.0) * COL_GAP;
        let cap = Rect::new(x, self.top, x + w, self.top + sheet::CAPTION_H);
        c.text_ellipsis(caption, &cap, &f.caption, &t.text_secondary);
        let y = cap.bottom + CAPTION_GAP;
        paint(Rect::new(x, y, x + w, y + h));
        self.row_h = self.row_h.max(sheet::CAPTION_H + CAPTION_GAP + h);
        self.col += span;
        if self.col >= COLS {
            self.next_row();
        }
    }

    fn next_row(&mut self) {
        if self.col > 0 || self.row_h > 0.0 {
            self.top += self.row_h + sheet::ROW_GAP;
        }
        self.col = 0;
        self.row_h = 0.0;
    }
}

/// The Contacts « Libellé » presets.
const PRESETS: &[&str] = &["Domicile", "Professionnel", "Mobile", "Principal", "Autre"];

/// A mock media card (`surface-2`, a picture glyph) with a `FloatCheckbox` in
/// its top-left corner, painted in `state`.
fn media_card(c: &dyn Canvas, card: Rect, check: &FloatCheckbox, state: WidgetState) -> Rect {
    let t = c.theme();
    c.fill_rounded(&card, kubuno_ui::metrics::radius::LG, &t.surface_2);
    c.vector_icon("Image", &card, 20.0, &t.text_tertiary);
    let d = check.measure(c);
    let r = Rect::new(card.left + CHECK_INSET, card.top + CHECK_INSET, card.left + CHECK_INSET + d.width, card.top + CHECK_INSET + d.height);
    check.paint(c, r, state);
    r
}

/// The four `FloatCheckbox` states over their cards.
fn float_checks(c: &dyn Canvas, r: Rect) {
    let states = [
        (FloatCheckbox::new(false), WidgetState::REST),
        (FloatCheckbox::new(false).reveal(true), WidgetState::REST),
        (FloatCheckbox::new(true), WidgetState::REST),
        (FloatCheckbox::new(false), WidgetState::REST.focused(true).focus_visible(true)),
    ];
    let mut x = r.left;
    for (check, st) in &states {
        if x + CARD_W > r.right {
            break;
        }
        media_card(c, Rect::new(x, r.top, x + CARD_W, r.top + CARD_H), check, *st);
        x += CARD_W + CARD_GAP;
    }
}

fn filled(label: &str, value: &str) -> OutlinedField {
    OutlinedField::new(label).with_value(value)
}

pub fn draw(c: &dyn Canvas, f: &Frame) {
    // The static exposition takes the window minus the interactive column, so it
    // lays out to the left of it rather than under it.
    let mut p = Page::new(c, f.size.0 - interact::PANEL_W(), f.size.1);
    let focused = WidgetState::REST.focused(true).focus_visible(true);

    p.section("OutlinedField — la base de tous les champs de formulaire");
    let mut g = Grid::new(sheet::MARGIN, p.area.right - sheet::MARGIN, p.y);

    g.field(c, "au repos — le libellé est l'indice", &OutlinedField::new("Nom"), WidgetState::REST);
    g.field(c, "focus — libellé flotté, encoche, curseur", &OutlinedField::new("Adresse e-mail"), focused);
    g.field(c, "rempli — le libellé reste flotté", &filled("Nom", "Ada Lovelace"), WidgetState::REST);

    let mut sel = filled("Adresse e-mail", "ada@kubuno.com");
    sel.select_range(0, 3);
    g.field(c, "sélection (Maj+flèches, glisser)", &sel, focused);
    let mut ph = OutlinedField::new("Téléphone");
    ph.placeholder_text = "+33 6 12 34 56 78".into();
    g.field(c, "placeholder — seulement au focus", &ph, focused);
    g.field(c, "icône de tête, hors de la boîte", &filled("Adresse e-mail", "ada@kubuno.com").with_leading("AtSign"), WidgetState::REST);

    let mut trigger = filled("Trier par", "Date de modification").with_trailing("ChevronDown");
    trigger.read_only = true;
    g.field(c, "glyphe de fin — déclencheur (lecture seule)", &trigger, WidgetState::REST);
    g.field(c, "requis — astérisque en danger", &OutlinedField::new("Titre").required(true), focused);
    g.field(
        c,
        "débordement — valeur coupée, libellé en « … »",
        &filled(
            "Adresse de facturation complète (rue, code postal, ville)",
            "12 rue de la République prolongée, bâtiment C, 69002 Lyon",
        ),
        WidgetState::REST,
    );

    let mut pwd = filled("Mot de passe", "correct horse");
    pwd.use_system_password_char = true;
    g.field(c, "mot de passe — masqué, jamais copié", &pwd, WidgetState::REST);
    let mut invalid = filled("Adresse e-mail", "pas-un-email");
    invalid.invalid = true;
    g.field(c, "invalide — bordure et libellé en danger", &invalid, WidgetState::REST);
    g.field(c, "désactivé — opacité 60 %", &filled("Champ verrouillé", "lecture seule"), WidgetState::REST.disabled(true));

    g.field(c, "grand — une question par écran", &OutlinedField::new("Question").large(true), focused);
    let mut multi = OutlinedField::new("Réponse");
    multi.multiline = true;
    multi.set_value(
        "Une réponse longue, qui passe à la ligne aux espaces comme une zone de texte du web, \
         puis défile verticalement.",
    );
    g.field(c, "multiligne — retour à la ligne, défilement", &multi, WidgetState::REST);
    let mut multi_sel = OutlinedField::new("Notes");
    multi_sel.multiline = true;
    multi_sel.set_value("Première ligne\nDeuxième ligne sélectionnée\nTroisième");
    multi_sel.select_range(9, 30);
    g.field(c, "multiligne — sélection sur deux lignes", &multi_sel, focused);
    let mut combo = LabelCombobox::new("Libellé", PRESETS);
    combo.field.set_value("Mobile");
    let ch = combo.measure(c).height;
    g.cell(c, "LabelCombobox — libellé libre", 1, ch, |r| {
        combo.paint(c, Rect::new(r.left, r.top, r.left + combo.width().min(r.right - r.left), r.bottom), WidgetState::REST);
    });
    g.next_row();

    // The composites: a group under one icon, a combobox with its list open
    // (painted in place here; live, it is a popup), the floating checks.
    let group = FieldGroup::new(Some("UserRound")).field("first", "Prénom").field("last", "Nom").advanced("middle", "Deuxième prénom");
    let gh = group.measure(c).height;
    g.cell(c, "FieldGroup — une icône pour le groupe, le chevron révèle le reste", 2, gh, |r| {
        group.paint_parts(c, r, &|_| WidgetState::REST, WidgetState::REST.hot(true));
    });
    let mut open = LabelCombobox::new("Libellé", PRESETS);
    open.field.set_value("il");
    open.open = true;
    open.active = Some(1);
    let list_h = open.list_size(0.0).height;
    g.cell(c, "LabelCombobox — la liste se filtre, flèches", 1, ch + 4.0 + list_h, |r| {
        let fr = Rect::new(r.left, r.top, r.left + open.width().min(r.right - r.left), r.top + ch);
        open.paint(c, fr, focused);
        let bx = open.field.box_rect(fr);
        let list = LabelCombobox::place_list(bx, open.list_size(bx.right - bx.left), Rect::new(r.left - LIST_EDGE, r.top, r.right, r.bottom + LIST_EDGE));
        open.paint_list(c, list, None);
    });
    g.cell(c, "FloatCheckbox — repos · survol · coché · focus", 1, CARD_H, |r| float_checks(c, r));
    g.next_row();

    p.y = g.top;
    p.section("HelpBubble · HelpButton");
    help_exposition(c, Rect::new(sheet::MARGIN, p.y, p.area.right - sheet::MARGIN, f.size.1 - sheet::MARGIN));
}

/// A body-text label followed by its « ? », both on a 24 DIP line; returns the
/// « ? »'s rectangle, which is what a bubble points at.
fn labelled_help(c: &dyn Canvas, left: f32, y: f32, label: &str, button: &HelpButton, state: WidgetState) -> Rect {
    let f = c.formats();
    let lw = c.measure(label, &f.body).ceil();
    c.text(label, &Rect::new(left, y, left + lw, y + 24.0), &f.body, &c.theme().text_primary, false);
    let d = button.measure(c);
    let x = left + lw + 4.0;
    let top = y + (24.0 - d.height) / 2.0;
    let r = Rect::new(x, top, x + d.width, top + d.height);
    button.paint(c, r, state);
    r
}

/// The help family's exposition, in two columns: on the left a bubble below
/// its « ? » (title, body, OK); on the right the « ? » in its five states and,
/// under them, a bubble above its anchor (`prefer: Top`) with a second action,
/// its hover wash and the keyboard ring on « OK ».
fn help_exposition(c: &dyn Canvas, area: Rect) {
    let t = c.theme();
    let f = c.formats();
    let mid = area.left + ((area.right - area.left) / 2.0).max(help::WIDTH + 24.0);
    let open = HelpButton::new().open(true);

    // Left: below its « ? » — the default side.
    let q1 = labelled_help(c, area.left, area.top, "Quota de stockage", &open, WidgetState::REST);
    let b1 = HelpBubble::new(
        "Chaque fichier compte une seule fois, même partagé avec d'autres. \
         La corbeille est comptée jusqu'à ce qu'elle soit vidée.",
    )
    .title("Comment le quota est compté");
    let p1 = b1.place(c, q1, Rect::new(area.left, area.top, mid - 12.0, area.bottom));
    b1.paint_placed(c, &p1, None);

    // Right: the « ? » states.
    let right = Rect::new(mid, area.top, area.right, area.bottom);
    c.text_ellipsis(
        "le « ? » : repos · survol · ouvert · focus clavier · désactivé",
        &Rect::new(right.left, right.top, right.right, right.top + 16.0),
        &f.caption,
        &t.text_secondary,
    );
    let y = right.top + 22.0;
    let states = [
        (HelpButton::new(), WidgetState::REST),
        (HelpButton::new(), WidgetState::REST.hot(true)),
        (HelpButton::new().open(true), WidgetState::REST),
        (HelpButton::new(), WidgetState::REST.focused(true).focus_visible(true)),
        (HelpButton::new(), WidgetState::REST.disabled(true)),
    ];
    let mut x = right.left;
    for (b, s) in &states {
        let d = b.measure(c);
        b.paint(c, Rect::new(x, y, x + d.width, y + d.height), *s);
        x += d.width + 24.0;
    }

    // Above its « ? », with a second action: hover wash on the action, the
    // keyboard ring on « OK ».
    let b2 = HelpBubble::new("Les éléments supprimés restent 30 jours dans la corbeille.")
        .title("Rétention")
        .action("En savoir plus")
        .prefer(Side::Top);
    let h2 = b2.measure(c).height;
    let top = y + 24.0 + 16.0;
    let q2 = labelled_help(c, right.left, top + h2 + help::GAP, "Durée de rétention", &open, WidgetState::REST);
    let p2 = b2.place(c, q2, right);
    b2.paint_placed_focused(c, &p2, Some(HelpPart::Action), Some(HelpPart::Ok));
}

/// The live column's bubble — built in one place so the column (which places
/// it and routes its clicks and keys) and the popup (which paints it) agree.
fn live_bubble() -> HelpBubble {
    HelpBubble::new(
        "Si vous perdez l'accès à votre compte, un lien de réinitialisation est envoyé \
         à cette adresse. Elle n'est jamais montrée aux autres membres.",
    )
    .title("À quoi sert cette adresse ?")
    .action("En savoir plus")
    .width(help::WIDTH_ADMIN)
}

const HELP_Q: FocusId = FocusId::of("help-q");
const HELP_OK: FocusId = FocusId::of("help-ok");
const HELP_ACTION: FocusId = FocusId::of("help-action");
const REQUIRED: FocusId = FocusId::of("required");
const COMBO: FocusId = FocusId::of("combo");
const GROUP_TOGGLE: FocusId = FocusId::of("group-toggle");
/// The gap between two stacked live rows.
const ROW_GAP: f32 = 12.0;
/// The « Requis » button of the live column.
const BUTTON_W: f32 = 180.0;
const BUTTON_H: f32 = 36.0;

fn part_id(part: HelpPart) -> FocusId {
    if part == HelpPart::Action { HELP_ACTION } else { HELP_OK }
}

/// What the interactive column remembers between frames.
struct Ui {
    /// The live fields: they keep their text, caret, selection and undo.
    fields: Vec<OutlinedField>,
    /// A « Nom » group (Prénom, Nom, and Deuxième prénom behind the chevron).
    group: FieldGroup,
    /// The « Libellé » combobox, and where its list popup was last frame.
    combo: LabelCombobox,
    combo_list: Option<Rect>,
    /// Drives the third field's required asterisk, flipped by its own button.
    required: bool,
    /// Whether the live « ? »'s bubble is open, where it was placed last
    /// frame, which of its buttons has the focus, and how often its second
    /// action ran.
    help_open: bool,
    help_place: Option<HelpPlacement>,
    help_focus: HelpPart,
    help_actions: u32,
    /// The last value « submitted » with Enter.
    submitted: Option<String>,
    prev_down: bool,
}

impl Default for Ui {
    fn default() -> Self {
        let mut comment = OutlinedField::new("Commentaire");
        comment.multiline = true;
        let mut email = OutlinedField::new("Adresse e-mail").with_leading("AtSign");
        email.placeholder_text = "nom@exemple.fr".into();
        Self {
            fields: vec![email, OutlinedField::new("Titre"), comment],
            group: FieldGroup::new(Some("UserRound"))
                .field("first", "Prénom")
                .field("last", "Nom")
                .advanced("middle", "Deuxième prénom"),
            combo: LabelCombobox::new("Libellé", PRESETS),
            combo_list: None,
            required: false,
            help_open: false,
            help_place: None,
            help_focus: HelpPart::Ok,
            help_actions: 0,
            submitted: None,
            prev_down: false,
        }
    }
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

/// The keys an open bubble acts on, taken from the host queue in order and
/// applied: Tab / Shift+Tab move between its buttons, Enter / Space press the
/// focused one, Escape closes. Returns whether it closed.
fn bubble_keys(ui: &mut Ui) -> bool {
    let bubble = live_bubble();
    let focus = Some(ui.help_focus);
    let taken = host::consume(|e| match e {
        InputEvent::Key { vk: k, down: true, mods, .. } => bubble.key(*k, *mods, focus) != HelpKey::Ignored,
        _ => false,
    });
    for e in taken {
        if let InputEvent::Key { vk: k, mods, .. } = e {
            match bubble.key(k, mods, Some(ui.help_focus)) {
                HelpKey::Close => return true,
                HelpKey::Action => ui.help_actions += 1,
                HelpKey::Focus(part) => {
                    ui.help_focus = part;
                    interact::with_focus(|r| r.focus_visibly(part_id(part)));
                }
                HelpKey::Ignored => {}
            }
        }
    }
    false
}

/// One frame of the live combobox at `cr`: focus (the list opens when the
/// field takes it and closes when it loses it), the ARIA combobox keys taken
/// before the field sees the rest, the field's own editing, then the list in
/// an interactive popup placed against the screen. `picked` is a row picked
/// by this frame's click, routed at the top of the column.
fn combo_frame(c: &dyn Canvas, f: &Frame, ff: &Frame, live: &Live, ui: &mut Ui, cr: Rect, picked: bool) {
    let bx = ui.combo.field.box_rect(cr);
    let st = live.focus_with(COMBO, bx, FocusOpts::TEXT);
    if st.gained {
        ui.combo.open = true;
    }
    if !st.focused {
        ui.combo.open = false;
        ui.combo.active = None;
    } else {
        let taken = {
            let combo = &ui.combo;
            host::consume(|e| match e {
                InputEvent::Key { vk: k, down: true, mods, .. } => combo.handles_key(*k, *mods),
                _ => false,
            })
        };
        for e in taken {
            if let InputEvent::Key { vk: k, mods, .. } = e {
                ui.combo.key(k, mods);
            }
        }
    }
    let resp = ui.combo.field.handle_frame(cr, st, ff, live.clicked);
    if resp.changed {
        // Typing narrows the list and (re)opens it, as an ARIA combobox does.
        ui.combo.open = true;
        ui.combo.refilter();
    }
    if picked {
        // The pick keeps the focus in the field, caret after the value.
        let n = ui.combo.field.text.chars().count();
        ui.combo.field.select_range(n, n);
    }
    ui.combo.paint(c, cr, st.apply(live.state(bx)));

    ui.combo_list = None;
    if ui.combo.list_visible() {
        let size = ui.combo.list_size(bx.right - bx.left);
        let list = LabelCombobox::place_list(bx, size, f.screen_area());
        ui.combo_list = Some(list);
        let pb = LabelCombobox::list_paint_bounds(list);
        interact::with_focus(|r| r.keep_focus_in(pb));
        let hot = ui.combo.item_at(list, f.mouse.0, f.mouse.1);
        let view = ui.combo.list_view();
        let local = Rect::new(list.left - pb.left, list.top - pb.top, list.right - pb.left, list.bottom - pb.top);
        host::popup(pb, move |canvas| view.paint(canvas, local, hot));
    }
}

/// The right-hand column: a live `FieldGroup`, three editable `OutlinedField`s,
/// a `LabelCombobox` with its list in a popup, a focusable button
/// and a « ? » whose bubble works with the mouse and the keyboard.
pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let mut live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;

        // A click on the desktop or another app never reaches this window: the
        // host reports it as `dismiss`, and the bubble closes as on blur.
        if f.dismiss && ui.help_open {
            ui.help_open = false;
        }
        // An open bubble takes the pointer and the keys first, as the web's
        // full-window backdrop and its capturing `keydown` do: a click on
        // « OK » or anywhere off the bubble closes it, on the action counts, on
        // the bubble itself does nothing — and nothing underneath sees that
        // click, nor lights up under the pointer while it is open.
        if ui.help_open {
            let mut close_to_trigger = bubble_keys(&mut ui);
            let mut close_blur = false;
            if let (true, Some(p)) = (live.clicked, ui.help_place) {
                match live_bubble().part_at(c, &p, live.mouse.0, live.mouse.1) {
                    HelpPart::Ok => close_blur = true,
                    HelpPart::Outside => close_blur = true,
                    HelpPart::Action => {
                        ui.help_actions += 1;
                        ui.help_focus = HelpPart::Action;
                    }
                    HelpPart::Bubble => {}
                }
            }
            if close_blur {
                // The backdrop ate the press: whatever the ring focused under
                // it does not keep the focus.
                interact::with_focus(|r| r.blur());
                close_to_trigger = false;
                ui.help_open = false;
            }
            if close_to_trigger {
                ui.help_open = false;
                interact::with_focus(|r| r.focus(HELP_Q));
            }
            live.clicked = false;
            live.mouse = (host::POINTER_AWAY, host::POINTER_AWAY);
        }
        // The combobox's open list: a press on a row picks it (on mousedown,
        // as the web's `onMouseDown`), a press elsewhere on the list is
        // swallowed, a press anywhere else closes it (the web's capturing
        // `mousedown` listener — that press still reaches what is under it).
        // While the pointer is over the list, nothing underneath lights up.
        let mut picked = false;
        if let Some(list) = ui.combo_list {
            let (mx, my) = live.mouse;
            if list.contains(mx, my) {
                let (_, dy) = live.wheel_over(list);
                if dy != 0.0 {
                    ui.combo.scroll_by(dy);
                }
                if live.clicked {
                    if let Some(k) = ui.combo.item_at(list, mx, my) {
                        picked = ui.combo.pick(k);
                    }
                    interact::with_focus(|r| r.focus(COMBO));
                    live.clicked = false;
                }
                live.mouse = (host::POINTER_AWAY, host::POINTER_AWAY);
            } else if live.clicked {
                ui.combo.open = false;
                ui.combo.active = None;
            }
        }
        // The frame the fields and buttons see: the pointer parked away while
        // a floating surface holds it.
        let mut ff = *f;
        ff.mouse = live.mouse;

        let panel = interact::panel_rect(f.size);
        let (left, mut y, right) = interact::panel(c, panel);
        let mut submitted = None;

        // A FieldGroup: its sub-fields are ordinary live fields; its chevron
        // is a focusable button (Enter / Space, or a click, toggles).
        y = interact::caption(c, left, right, y, "FieldGroup — le chevron révèle « Deuxième prénom »");
        let gh = ui.group.measure(c).height;
        let gr = Rect::new(left, y, right, y + gh);
        let layout = ui.group.layout(gr);
        let mut states = Vec::new();
        for &(i, r) in &layout.fields {
            let fld = &mut ui.group.fields[i].field;
            let bx = fld.box_rect(r);
            let st = live.focus_with(("group", i), bx, FocusOpts::TEXT);
            let resp = fld.handle_frame(r, st, &ff, live.clicked);
            if resp.submitted {
                submitted = Some(fld.text.clone());
            }
            states.push((i, st.apply(live.state(bx))));
        }
        let mut toggle_state = WidgetState::REST;
        if let Some(t) = layout.toggle {
            toggle_state = live.focus_state(GROUP_TOGGLE, t);
            let keys = toggle_state.focused
                && (live.take_key(vk::ENTER, Modifiers::NONE) || live.take_key(vk::SPACE, Modifiers::NONE));
            if live.hit(t) || keys {
                ui.group.toggle();
            }
        }
        ui.group.paint_parts(
            c,
            gr,
            &|i| states.iter().find(|s| s.0 == i).map_or(WidgetState::REST, |s| s.1),
            toggle_state,
        );
        y += ui.group.measure(c).height.max(gh) + ROW_GAP;

        let captions = [
            "Icône de tête, placeholder au focus",
            "Requis — le bouton bascule l'astérisque",
            "Multiligne — retour à la ligne, molette",
        ];
        let required = ui.required;
        ui.fields[1].required = required;
        for (i, cap) in captions.iter().enumerate() {
            y = interact::caption(c, left, right, y, cap);
            let fld = &mut ui.fields[i];
            let h = fld.measure(c).height;
            let rect = Rect::new(left, y, right, y + h);
            let bx = fld.box_rect(rect);
            let st = live.focus_with(("field", i), bx, FocusOpts::TEXT);
            let resp = fld.handle_frame(rect, st, &ff, live.clicked);
            if resp.submitted {
                submitted = Some(fld.text.clone());
            }
            fld.paint(c, rect, st.apply(live.state(bx)));
            y += h + ROW_GAP;
        }
        if let Some(s) = submitted {
            ui.submitted = Some(s);
        }

        // The « Libellé » combobox, then a button that flips the « Titre »
        // field's required flag (Tab, then Enter or Space).
        y = interact::caption(c, left, right, y, "LabelCombobox — tapez pour filtrer, ↑ ↓ Entrée Échap");
        let ch = ui.combo.measure(c).height;
        let cr = Rect::new(left, y, left + ui.combo.width().min(right - left), y + ch);
        combo_frame(c, f, &ff, &live, &mut ui, cr, picked);

        let by = ui.combo.field.box_rect(cr);
        let btn_left = (cr.right + 16.0).min(right - BUTTON_W).max(left);
        let btn = Rect::new(btn_left, (by.top + by.bottom - BUTTON_H) / 2.0, btn_left + BUTTON_W, (by.top + by.bottom + BUTTON_H) / 2.0);
        let ws = live.focus_state(REQUIRED, btn);
        let key_press = ws.focused
            && (live.take_key(vk::ENTER, Modifiers::NONE) || live.take_key(vk::SPACE, Modifiers::NONE));
        if live.hit(btn) || key_press {
            ui.required = !ui.required;
        }
        let btn_label = if ui.required { "Requis : oui" } else { "Requis : non" };
        Button::new(btn_label).variant(Variant::Secondary).paint(c, btn, ws);
        y += ch + 16.0;

        // A label with a live « ? ». Its bubble is placed against the whole
        // SCREEN and hosted in an interactive popup window of its own: it sits
        // above everything and may hang past the window's edges, while its
        // hover and clicks still come back through `Frame`.
        y = interact::caption(c, left, right, y, "HelpBubble — clic, Entrée ou Espace sur le « ? »");
        let q = HelpButton::new().open(ui.help_open);
        let d = q.measure(c);
        let label = "Adresse de récupération";
        let lw = c.measure(label, &c.formats().body).ceil();
        let qr = Rect::new(left + lw + 4.0, y + (24.0 - d.height) / 2.0, left + lw + 4.0 + d.width, y + (24.0 + d.height) / 2.0);
        let q_focus = live.focus(HELP_Q, qr);
        let q_keys = q_focus.focused
            && !ui.help_open
            && (live.take_key(vk::ENTER, Modifiers::NONE) || live.take_key(vk::SPACE, Modifiers::NONE));
        if (live.clicked && q.hit_test(qr, live.mouse.0, live.mouse.1)) || q_keys {
            ui.help_open = true;
            ui.help_focus = live_bubble().initial_focus();
            // « OK » takes the focus (`autoFocus`); its ring shows only when
            // the bubble was opened from the keyboard, as `:focus-visible` does.
            let id = part_id(ui.help_focus);
            interact::with_focus(|r| r.focus(id));
        }
        let q_state = q_focus.apply(live.state(qr)).hot(q.hit_test(qr, live.mouse.0, live.mouse.1));
        labelled_help(c, left, y, label, &q, q_state);

        ui.help_place = ui.help_open.then(|| live_bubble().place(c, qr, f.screen_area()));
        if let Some(p) = ui.help_place {
            let b = live_bubble();
            // The bubble's buttons are focusable (they keep Tab: the bubble
            // cycles it between them), and a press on the bubble keeps the
            // focus where it is.
            let (ok, action) = b.buttons(c, p.rect);
            let keep = FocusOpts { wants_tab: true, ..FocusOpts::default() };
            let mut visible = None;
            if let Some(a) = action {
                let st = interact::with_focus(|r| r.register_with(HELP_ACTION, a, keep));
                if st.focused {
                    ui.help_focus = HelpPart::Action;
                    visible = st.visible.then_some(HelpPart::Action);
                }
            }
            let st = interact::with_focus(|r| r.register_with(HELP_OK, ok, keep));
            if st.focused {
                ui.help_focus = HelpPart::Ok;
                visible = st.visible.then_some(HelpPart::Ok);
            }
            interact::with_focus(|r| r.keep_focus_in(p.paint_bounds()));
            let hot = b.part_at(c, &p, f.mouse.0, f.mouse.1);
            let pb = p.paint_bounds();
            let local = p.offset(-pb.left, -pb.top);
            host::popup(pb, move |canvas| b.paint_placed_focused(canvas, &local, Some(hot), visible));
        }
        y += 24.0 + 8.0;

        let count = match ui.help_actions {
            0 => "« En savoir plus » : jamais activé".to_string(),
            1 => "« En savoir plus » : activé 1 fois".to_string(),
            n => format!("« En savoir plus » : activé {n} fois"),
        };
        y = interact::caption(c, left, right, y, &count);
        let sent = match &ui.submitted {
            Some(s) if s.is_empty() => "Entrée : champ vide soumis".to_string(),
            Some(s) => format!("Entrée : « {s} » soumis"),
            None => "Entrée dans un champ d'une ligne : rien soumis".to_string(),
        };
        interact::caption(c, left, right, y, &sent);
    });
}
