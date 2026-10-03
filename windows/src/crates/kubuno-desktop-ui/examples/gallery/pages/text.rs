//! Gallery page — text fields.
//!
//! The exposition (left) shows every state of the family at rest: the
//! `@ui/Input` box in each state, overflow (an ellipsised field at rest, a
//! focused one scrolled to its caret, a soft-wrapped text area with its scroll
//! bar), the mask, the compact search pill, and the family NESTED in a group
//! box, in a toolbar row where a search pill, a field and an `md` button share
//! one 36 DIP line.
//!
//! The live column (right) is really editable: click, drag, double / triple
//! click, Shift+arrows, Ctrl+arrows, Home / End, Ctrl+A / C / X / V / Z / Y,
//! the right-click menu (a popup that may leave the window), Tab / Shift+Tab
//! between the fields, the wheel in the text area, Enter and Escape.

use std::cell::RefCell;

use kubuno_desktop_controls::host::{Frame, POINTER_AWAY};
use kubuno_desktop_ui::buttons::{Button, Variant};
use kubuno_desktop_ui::containers::GroupBox;
use kubuno_desktop_ui::focus::{FocusOpts, FocusState};
use kubuno_desktop_ui::text::{EditInput, EditOutcome, MaskedField, SearchField, TextArea, TextField};
use kubuno_desktop_ui::{Canvas, Rect, Widget, WidgetState};

use super::interact::{self, Live};
use super::sheet::{self, Page};

/// The gutter between the two exposition columns.
const GUTTER: f32 = 32.0;
/// The state name above an example.
const LABEL_H: f32 = 18.0;
/// Below this width the exposition falls back to one column.
const MIN_TWO_COLUMN: f32 = 560.0;
/// A field row: `h-9`.
const FIELD_H: f32 = 36.0;
/// The exposition's text area: shorter than `h-36` so its text overflows and
/// shows the scroll bar.
const AREA_H: f32 = 96.0;
/// The live text area.
const LIVE_AREA_H: f32 = 104.0;
/// A narrow search pill, to show the ✕ never covering the text.
const NARROW_SEARCH_W: f32 = 132.0;
/// Space between the live rows.
const LIVE_GAP: f32 = 12.0;
/// The toolbar row's button width.
const BUTTON_W: f32 = 96.0;

const LONG: &str = "Documents/Rapports/2026/Trimestre 3/Synthèse des ventes par région.xlsx";
const PARAGRAPH: &str = "Le champ multiligne replie ses lignes à la largeur de la boîte, comme un <textarea> : \
les espaces de fin restent en bout de ligne et un mot trop long se coupe entre deux caractères.\n\
Une deuxième ligne, puis une troisième qui déborde : la barre de défilement apparaît.";

pub fn draw(c: &dyn Canvas, f: &Frame) {
    let content_w = f.size.0 - interact::PANEL_W();
    let mut page = Page::new(c, content_w, f.size.1);
    page.section("Champs de saisie — @ui/Input, @ui/Textarea, barre de recherche");
    let top = page.y;

    let avail = content_w - 2.0 * sheet::MARGIN;
    let two = avail >= MIN_TWO_COLUMN;
    let cell = if two { (avail - GUTTER) / 2.0 } else { avail.max(160.0) };
    let mut a = Column { c, x: sheet::MARGIN, y: top, w: cell };

    a.entry("repos", FIELD_H, |r| field("Documents/Rapports").paint(c, r, WidgetState::REST));
    a.entry("vide → texte indicatif", FIELD_H, |r| {
        let mut t = field("");
        t.placeholder_text = "Nom du dossier…".into();
        t.paint(c, r, WidgetState::REST);
    });
    a.entry("focus (contour 3 px) + sélection", FIELD_H, |r| {
        let mut t = field("Documents/Rapports");
        t.select(0, 9);
        t.paint(c, r, WidgetState::REST.focused(true));
    });
    a.entry("texte long au repos → points de suspension", FIELD_H, |r| field(LONG).paint(c, r, WidgetState::REST));
    a.entry("texte long focalisé → défilé jusqu'au caret", FIELD_H, |r| {
        let mut t = field(LONG);
        let n = t.text().chars().count() as i32;
        t.select(n, 0);
        t.paint(c, r, WidgetState::REST.focused(true));
    });
    a.entry("invalide + focus (contour danger)", FIELD_H, |r| {
        let mut t = field("hôte::invalide");
        t.invalid = true;
        t.paint(c, r, WidgetState::REST.focused(true));
    });
    a.entry("désactivé (opacité 60 %)", FIELD_H, |r| {
        field("Documents/Rapports").paint(c, r, WidgetState::REST.disabled(true))
    });
    a.entry("lecture seule, focalisé (pas de caret)", FIELD_H, |r| {
        let mut t = field("Documents/Rapports");
        t.read_only = true;
        t.paint(c, r, WidgetState::REST.focused(true));
    });
    a.entry("icône + mot de passe", FIELD_H, |r| {
        let mut t = field("s3cret42");
        t.leading_icon = Some("Lock");
        t.use_system_password_char = true;
        t.paint(c, r, WidgetState::REST);
    });

    let mut b = if two { Column { c, x: sheet::MARGIN + cell + GUTTER, y: top, w: cell } } else { a };
    b.entry("TextArea — repli des lignes + barre de défilement", AREA_H, |r| {
        let mut t = TextArea::new();
        t.set_text(PARAGRAPH);
        t.paint(c, r, WidgetState::REST);
    });
    b.entry("MaskedField — 00/00/0000 (moteur de la réplique)", FIELD_H, |r| {
        let mut m = MaskedField::new();
        m.set_mask("00/00/0000");
        m.set_text("3112");
        m.paint(c, r, WidgetState::REST);
    });
    b.entry("recherche — repos (search-bg, sans bordure)", FIELD_H, |r| {
        let mut s = SearchField::new();
        s.placeholder_text = "Rechercher dans Drive".into();
        s.paint(c, r, WidgetState::REST);
    });
    b.entry("recherche — active (fond blanc + bordure, pas d'anneau)", FIELD_H, |r| {
        let mut s = SearchField::new();
        s.set_text("rapport");
        s.select(7, 0);
        s.paint(c, r, WidgetState::REST.focused(true));
    });
    b.entry("recherche étroite — le ✕ ne recouvre jamais le texte", FIELD_H, |r| {
        let mut s = SearchField::new();
        s.set_text("rapport trimestriel");
        s.paint(c, Rect::new(r.left, r.top, (r.left + NARROW_SEARCH_W).min(r.right), r.bottom), WidgetState::REST);
    });
    // Composition: the family inside a group box, in one toolbar row.
    let group_h = 2.0 * FIELD_H + 3.0 * sheet::ROW_GAP;
    b.entry("imbriqué — GroupBox, ligne d'outils alignée sur 36", group_h, |r| {
        let g = GroupBox::titled("Barre d'outils");
        g.paint(c, r, WidgetState::REST);
        let inner = g.inner_rect(r);
        let y = (inner.top + inner.bottom - FIELD_H) / 2.0;
        let w = inner.right - inner.left;
        let gap = sheet::ROW_GAP / 2.0;
        let search_w = ((w - BUTTON_W - 2.0 * gap) / 2.0).max(0.0);
        let s_r = Rect::new(inner.left, y, inner.left + search_w, y + FIELD_H);
        let t_r = Rect::new(s_r.right + gap, y, s_r.right + gap + search_w, y + FIELD_H);
        let b_r = Rect::new(t_r.right + gap, y, inner.right, y + FIELD_H);
        let mut s = SearchField::new();
        s.set_text("budget 2026");
        s.paint(c, s_r, WidgetState::REST);
        let mut t = field("");
        t.placeholder_text = "Filtre…".into();
        t.paint(c, t_r, WidgetState::REST);
        Button::new("Filtrer").variant(Variant::Secondary).paint(c, b_r, WidgetState::REST);
    });
}

/// A field with the given text, built the way a caller would.
fn field(text: &str) -> TextField {
    let mut t = TextField::new();
    t.set_text(text);
    t
}

/// A column of labelled examples walking down the page.
#[derive(Clone, Copy)]
struct Column<'a> {
    c: &'a dyn Canvas,
    x: f32,
    y: f32,
    w: f32,
}

impl Column<'_> {
    fn entry(&mut self, label: &str, h: f32, paint: impl FnOnce(Rect)) {
        let t = self.c.theme();
        let f = self.c.formats();
        let lr = Rect::new(self.x, self.y, self.x + self.w, self.y + LABEL_H);
        self.c.text_ellipsis(label, &lr, &f.caption, &t.text_secondary);
        let r = Rect::new(self.x, self.y + LABEL_H, self.x + self.w, self.y + LABEL_H + h);
        paint(r);
        self.y += LABEL_H + h + sheet::ROW_GAP;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Interactive column — the fields, really editable.
// ─────────────────────────────────────────────────────────────────────────────

/// Which live field is which (focus ids, and who owns an open menu).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Id {
    Name,
    Password,
    Email,
    Area,
    Search,
    Masked,
    ReadOnly,
}

impl Id {
    fn key(self) -> &'static str {
        match self {
            Id::Name => "text.name",
            Id::Password => "text.password",
            Id::Email => "text.email",
            Id::Area => "text.area",
            Id::Search => "text.search",
            Id::Masked => "text.masked",
            Id::ReadOnly => "text.readonly",
        }
    }
}

/// What the live column keeps between frames: the widgets themselves (their
/// editor state lives inside them) and the last event, shown at the bottom.
struct Ui {
    name: TextField,
    password: TextField,
    email: TextField,
    area: TextArea,
    search: SearchField,
    masked: MaskedField,
    read_only: TextField,
    disabled: TextField,
    status: String,
    prev_down: bool,
}

impl Default for Ui {
    fn default() -> Self {
        let mut name = TextField::new();
        name.placeholder_text = "Nom du dossier…".into();
        name.max_length = 40;
        let mut password = TextField::new();
        password.placeholder_text = "Mot de passe".into();
        password.use_system_password_char = true;
        password.leading_icon = Some("Lock");
        let mut email = TextField::new();
        email.set_text("martinien.kubuno.local");
        email.placeholder_text = "adresse@exemple.fr".into();
        let mut area = TextArea::new();
        area.set_text(PARAGRAPH);
        let mut search = SearchField::new();
        search.placeholder_text = "Rechercher".into();
        let mut masked = MaskedField::new();
        masked.set_mask("00/00/0000");
        let mut read_only = TextField::new();
        read_only.set_text("Lecture seule : sélectionnable et copiable");
        read_only.read_only = true;
        let mut disabled = TextField::new();
        disabled.set_text("Désactivé");
        disabled.enabled = false;
        Self {
            name,
            password,
            email,
            area,
            search,
            masked,
            read_only,
            disabled,
            status: "Tab / Maj+Tab parcourent les champs · clic droit : menu d'édition".into(),
            prev_down: false,
        }
    }
}

impl Ui {
    fn menu_owner(&self) -> Option<Id> {
        [
            (Id::Name, self.name.is_menu_open()),
            (Id::Password, self.password.is_menu_open()),
            (Id::Email, self.email.is_menu_open()),
            (Id::Area, self.area.is_menu_open()),
            (Id::Search, self.search.is_menu_open()),
            (Id::Masked, self.masked.is_menu_open()),
            (Id::ReadOnly, self.read_only.is_menu_open()),
        ]
        .into_iter()
        .find(|(_, open)| *open)
        .map(|(id, _)| id)
    }
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

/// The live input for field `id`: while ANOTHER field's context menu is open,
/// the pointer is parked away from this one (the menu's backdrop swallows the
/// click, as on the web) and it does not take part in focus.
fn input_for(f: &Frame, live: &Live, id: Id, r: Rect, owner: Option<Id>) -> (EditInput, FocusState) {
    let fs = match owner {
        Some(o) if o != id => FocusState::default(),
        _ => live.focus_with(id.key(), r, FocusOpts::TEXT),
    };
    let mut input = EditInput::new(f, fs);
    if owner.is_some_and(|o| o != id) {
        input.mouse = (POINTER_AWAY, POINTER_AWAY);
    }
    (input, fs)
}

/// A one-line report of what a field just did.
fn report(status: &mut String, label: &str, out: EditOutcome, text: &str) {
    if out.submitted {
        *status = format!("{label} : Entrée → « {text} »");
    } else if out.cleared {
        *status = format!("{label} : effacé");
    } else if out.changed {
        *status = format!("{label} : « {text} »");
    } else if out.escaped {
        *status = format!("{label} : Échap (laissé à la page)");
    }
}

pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;
        let owner = ui.menu_owner();
        if owner.is_some() {
            // An open menu keeps the focus where it is, wherever the click
            // lands (the web's backdrop): the menu itself closes on it.
            interact::with_focus(|ring| ring.keep_focus_in(f.screen_area()));
        }

        let (left, mut y, right) = interact::panel(c, interact::panel_rect(f.size));
        y = interact::caption(c, left, right, y, "Frappe, sélection (souris + clavier), presse-papiers, annuler/rétablir.");
        y += 4.0;

        macro_rules! live_field {
            ($field:expr, $id:expr, $label:expr, $h:expr) => {{
                y = interact::caption(c, left, right, y, $label);
                let r = Rect::new(left, y, right, y + $h);
                let (input, fs) = input_for(f, &live, $id, r, owner);
                let out = $field.update(c, r, &input);
                $field.paint(c, r, fs.apply(WidgetState::REST));
                y += $h + LIVE_GAP;
                out
            }};
        }

        let out = live_field!(ui.name, Id::Name, "TextField — maxlength 40", FIELD_H);
        let text = ui.name.text().to_string();
        report(&mut ui.status, "Nom", out, &text);

        let out = live_field!(ui.password, Id::Password, "Mot de passe — copier / couper refusés", FIELD_H);
        let dots = "●".repeat(ui.password.text().chars().count());
        report(&mut ui.status, "Mot de passe", out, &dots);

        // `error` while the address has no « @ ».
        ui.email.invalid = !ui.email.text().is_empty() && !ui.email.text().contains('@');
        let out = live_field!(ui.email, Id::Email, "Invalide tant qu'il manque « @ »", FIELD_H);
        let text = ui.email.text().to_string();
        report(&mut ui.status, "Courriel", out, &text);

        let out = live_field!(ui.area, Id::Area, "TextArea — Entrée = saut de ligne, molette", LIVE_AREA_H);
        if out.changed {
            let n = ui.area.text().chars().count();
            ui.status = format!("Texte : {n} caractères");
        }

        let out = live_field!(ui.search, Id::Search, "SearchField — ✕ ou Échap efface", FIELD_H);
        let text = ui.search.text().to_string();
        report(&mut ui.status, "Recherche", out, &text);

        let out = live_field!(ui.masked, Id::Masked, "MaskedField — 00/00/0000", FIELD_H);
        let text = ui.masked.display().to_string();
        report(&mut ui.status, "Date", out, &text);

        let out = live_field!(ui.read_only, Id::ReadOnly, "Lecture seule", FIELD_H);
        report(&mut ui.status, "Lecture seule", out, "");

        // Disabled: not registered with the focus ring, no editing, and the
        // not-allowed pointer (`disabled:cursor-not-allowed`).
        y = interact::caption(c, left, right, y, "Désactivé — ni focus ni édition");
        let r = Rect::new(left, y, right, y + FIELD_H);
        let input = EditInput::new(f, FocusState::default());
        ui.disabled.update(c, r, &input);
        ui.disabled.paint(c, r, WidgetState::REST.disabled(true));
        y += FIELD_H + LIVE_GAP;

        let status = ui.status.clone();
        interact::caption(c, left, right, y, &status);
    });
}
