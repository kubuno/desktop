//! Gallery page — the Office ribbon (`office/frontend/src/ribbon/Ribbon.tsx`).
//!
//! The exposition (left) runs three LIVE ribbons, declared as data exactly as
//! a web editor declares its tabs:
//!
//! * the Documents ribbon at full width — coloured strip, « Fichier » opening
//!   the Backstage, quick actions (Save / Undo / Redo), Accueil with every item
//!   kind (large paste, stacked buttons, font and size drop-downs, toggles,
//!   splits with their menus, a styles gallery, a « Sélectionner » menu),
//!   Insertion, Mise en page, Affichage, and a contextual « Image » tab;
//! * the same declaration in a narrow frame, whose right-most groups fold into
//!   chips that open the whole group in a popover;
//! * the plain workspace look (no coloured strip, accent underline).
//!
//! The interactive column (right) drives them: the contextual tab, a fragment merged by
//! another module (`RibbonExtension`), the
//! collapsed ribbon (Ctrl+F1 works too), the app tone, and the event log.

use std::cell::RefCell;

use drive_app_controls::Canvas;
use kubuno_controls::host::{self, Cursor, Frame};
use kubuno_ui::buttons::Switch;
use kubuno_ui::ribbon::{
    hex, tone, Backstage, BackstageSection, Ribbon, RibbonEvent, RibbonGroup, RibbonItem, RibbonOption, RibbonTab,
    RibbonTheme,
};
use kubuno_ui::{Rect, Widget};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use super::interact::{self, Live};
use super::sheet::{Page, MARGIN};

/// The document area under the full-width ribbon.
const DOC_H: f32 = 230.0;
/// The narrow ribbon's width.
const NARROW_W: f32 = 520.0;

/// The live formatting state the ribbon reflects.
#[derive(Clone)]
struct Doc {
    bold:      bool,
    italic:    bool,
    underline: bool,
    strike:    bool,
    sub:       bool,
    sup:       bool,
    align:     &'static str,
    marks:     bool,
    ruler:     bool,
    font:      String,
    size:      String,
    style:     String,
}

impl Default for Doc {
    fn default() -> Self {
        Self {
            bold: true,
            italic: false,
            underline: false,
            strike: false,
            sub: false,
            sup: false,
            align: "left",
            marks: false,
            ruler: true,
            font: "Calibri".into(),
            size: "11".into(),
            style: "normal".into(),
        }
    }
}

const TONES: [(&str, D2D1_COLOR_F); 8] = [
    ("Documents", tone::DOCUMENTS),
    ("Tableur", tone::SPREADSHEET),
    ("Présentation", tone::PRESENTATION),
    ("Projets", tone::PROJECTS),
    ("Diagrammes", tone::DIAGRAMS),
    ("Données", tone::DATA),
    ("Maths", tone::MATHS),
    ("Tableau blanc", tone::WHITEBOARD),
];

struct Ui {
    main:      Ribbon,
    narrow:    Ribbon,
    plain:     Ribbon,
    backstage: Backstage,
    doc:       Doc,
    image:     bool,
    tone:      usize,
    /// The « Assistant » fragment merged into Accueil (`RIBBON.md` §8).
    assistant: bool,
    log:       Vec<String>,
    prev_down: bool,
}

impl Ui {
    fn new() -> Self {
        let doc = Doc::default();
        let mut main = Ribbon::new(tabs(&doc, false), RibbonTheme::office(tone::DOCUMENTS));
        main.strip_actions = strip_actions();
        let mut narrow = Ribbon::new(tabs(&doc, false), RibbonTheme::office(tone::SPREADSHEET));
        // Only the first ribbon stands for an app: it colours the window's
        // caption; the two others are demos inside the page.
        narrow.owns_caption = false;
        narrow.strip_actions = strip_actions();
        let mut plain = Ribbon::new(tabs(&doc, false), RibbonTheme::plain());
        plain.owns_caption = false;
        let backstage = Backstage::new(vec![
            BackstageSection::view("home", "Accueil", "House"),
            BackstageSection::view("info", "Informations", "Info").separated(),
            BackstageSection::view("export", "Exporter", "FileDown"),
            BackstageSection::action("print", "Imprimer", "Printer"),
            BackstageSection::action("close", "Fermer", "X").separated(),
        ]);
        Self { main, narrow, plain, backstage, doc, image: false, tone: 0, assistant: false, log: Vec::new(), prev_down: false }
    }

    /// Applies what a ribbon reported to the document, and logs it.
    fn apply(&mut self, who: &str, events: Vec<RibbonEvent>) {
        for e in events {
            let d = &mut self.doc;
            match &e {
                RibbonEvent::Clicked(id) => match id.as_str() {
                    "bold" => d.bold = !d.bold,
                    "italic" => d.italic = !d.italic,
                    "underline" => d.underline = !d.underline,
                    "strike" => d.strike = !d.strike,
                    "sub" => {
                        d.sub = !d.sub;
                        d.sup = false;
                    }
                    "sup" => {
                        d.sup = !d.sup;
                        d.sub = false;
                    }
                    "marks" => d.marks = !d.marks,
                    "ruler" => d.ruler = !d.ruler,
                    a @ ("left" | "center" | "right" | "justify") => {
                        d.align = match a {
                            "center" => "center",
                            "right" => "right",
                            "justify" => "justify",
                            _ => "left",
                        }
                    }
                    _ => {}
                },
                RibbonEvent::Changed { item, value } => match item.as_str() {
                    "font" => d.font = value.clone(),
                    "size" => d.size = value.clone(),
                    "styles" => d.style = value.clone(),
                    _ => {}
                },
                _ => {}
            }
            let text = match e {
                RibbonEvent::TabChanged(id) => format!("{who} · onglet « {id} »"),
                RibbonEvent::Clicked(id) => format!("{who} · clic « {id} »"),
                RibbonEvent::DoubleClicked(id) => format!("{who} · double-clic « {id} »"),
                RibbonEvent::Chosen { item, entry } => format!("{who} · menu « {item} » → « {entry} »"),
                RibbonEvent::Changed { item, value } => format!("{who} · « {item} » = « {value} »"),
                RibbonEvent::Collapsed(on) => format!("{who} · ruban {}", if on { "réduit" } else { "développé" }),
                RibbonEvent::Launcher(g) => format!("{who} · lanceur « {g} »"),
                RibbonEvent::QatChanged(ids) => format!("{who} · accès rapide : {}", ids.join(", ")),
            };
            self.log.insert(0, text);
            self.log.truncate(8);
        }
    }

    /// Re-declares every ribbon from the live state, as a web editor does on
    /// each render.
    fn sync(&mut self) {
        let mut t = tabs(&self.doc, self.image);
        if self.assistant {
            kubuno_ui::ribbon::merge::merge_into(&mut t, &assistant_fragment());
        }
        self.main.tabs = t.clone();
        self.narrow.tabs = t.clone();
        self.plain.tabs = t;
        self.main.theme = RibbonTheme::office(TONES[self.tone].1);
    }
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> R {
    UI.with(|u| {
        let mut u = u.borrow_mut();
        f(u.get_or_insert_with(Ui::new))
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// The declaration — what `buildDocumentRibbon` returns, in Rust
// ─────────────────────────────────────────────────────────────────────────────

fn strip_actions() -> Vec<RibbonItem> {
    vec![
        RibbonItem::button("save", "Enregistrer", "Save").shortcut("Ctrl+S"),
        RibbonItem::button("undo", "Annuler", "Undo2").shortcut("Ctrl+Z"),
        RibbonItem::button("redo", "Rétablir", "Redo2").shortcut("Ctrl+Y"),
    ]
}

fn options(pairs: &[(&str, &str)]) -> Vec<RibbonOption> {
    pairs.iter().map(|(v, l)| RibbonOption::new(*v, *l)).collect()
}

fn tabs(d: &Doc, image: bool) -> Vec<RibbonTab> {
    let fonts = options(&[
        ("Arial", "Arial"),
        ("Calibri", "Calibri"),
        ("Cambria", "Cambria"),
        ("Consolas", "Consolas"),
        ("Georgia", "Georgia"),
        ("Segoe UI", "Segoe UI"),
        ("Times New Roman", "Times New Roman"),
        ("Verdana", "Verdana"),
    ]);
    let sizes: Vec<RibbonOption> = ["8", "9", "10", "11", "12", "14", "16", "18", "20", "24", "28", "36", "48", "72"]
        .iter()
        .map(|s| RibbonOption::new(*s, *s))
        .collect();
    let colours = |id: &str| {
        vec![
            RibbonItem::entry(format!("{id}-yellow"), "Jaune"),
            RibbonItem::entry(format!("{id}-green"), "Vert"),
            RibbonItem::entry(format!("{id}-blue"), "Bleu"),
            RibbonItem::separator("sep"),
            RibbonItem::entry(format!("{id}-none"), "Aucune couleur"),
        ]
    };
    let home = RibbonTab::new(
        "home",
        "Accueil",
        vec![
            RibbonGroup::new(
                "clipboard",
                "Presse-papiers",
                vec![
                    RibbonItem::split(
                        "paste",
                        "Coller",
                        "ClipboardPaste",
                        vec![
                            RibbonItem::entry("paste-keep", "Conserver la mise en forme"),
                            RibbonItem::entry("paste-text", "Texte seulement"),
                        ],
                    )
                    .large()
                    .shortcut("Ctrl+V"),
                    RibbonItem::button("cut", "Couper", "Scissors").shortcut("Ctrl+X"),
                    RibbonItem::button("copy", "Copier", "Copy").shortcut("Ctrl+C"),
                    RibbonItem::button("painter", "Reproduire", "Paintbrush")
                        .tooltip("Reproduire la mise en forme (double-clic : mode collant)"),
                ],
            ),
            RibbonGroup::new(
                "font",
                "Police",
                vec![
                    RibbonItem::dropdown("font", fonts, d.font.clone(), 130.0),
                    RibbonItem::dropdown("size", sizes, d.size.clone(), 56.0),
                    RibbonItem::toggle("bold", "", "Bold", d.bold).tooltip("Gras").shortcut("Ctrl+B"),
                    RibbonItem::toggle("italic", "", "Italic", d.italic).tooltip("Italique").shortcut("Ctrl+I"),
                    RibbonItem::toggle("underline", "", "Underline", d.underline).tooltip("Souligné").shortcut("Ctrl+U"),
                    RibbonItem::toggle("strike", "", "Strikethrough", d.strike).tooltip("Barré"),
                    RibbonItem::toggle("sub", "", "Subscript", d.sub).tooltip("Indice").shortcut("Ctrl+="),
                    RibbonItem::toggle("sup", "", "Superscript", d.sup).tooltip("Exposant"),
                    RibbonItem::split("highlight", "", "Highlighter", colours("highlight")).tooltip("Surligneur"),
                    RibbonItem::split("color", "", "Baseline", colours("color")).tooltip("Couleur de police"),
                    RibbonItem::button("case", "", "CaseSensitive").tooltip("Modifier la casse"),
                ],
            ),
            RibbonGroup::new(
                "paragraph",
                "Paragraphe",
                vec![
                    RibbonItem::split(
                        "bullets",
                        "",
                        "List",
                        vec![RibbonItem::entry("bullet-disc", "Puce ronde"), RibbonItem::entry("bullet-square", "Puce carrée")],
                    )
                    .tooltip("Puces"),
                    RibbonItem::split(
                        "numbering",
                        "",
                        "ListOrdered",
                        vec![RibbonItem::entry("num-123", "1. 2. 3."), RibbonItem::entry("num-abc", "a. b. c.")],
                    )
                    .tooltip("Numérotation"),
                    RibbonItem::button("outdent", "", "IndentDecrease").tooltip("Diminuer le retrait"),
                    RibbonItem::button("indent", "", "IndentIncrease").tooltip("Augmenter le retrait"),
                    RibbonItem::toggle("marks", "", "Pilcrow", d.marks).tooltip("Afficher tout").shortcut("Ctrl+*"),
                    RibbonItem::toggle("left", "", "AlignLeft", d.align == "left").tooltip("Aligner à gauche"),
                    RibbonItem::toggle("center", "", "AlignCenter", d.align == "center").tooltip("Centrer"),
                    RibbonItem::toggle("right", "", "AlignRight", d.align == "right").tooltip("Aligner à droite"),
                    RibbonItem::toggle("justify", "", "AlignJustify", d.align == "justify").tooltip("Justifier"),
                ],
            ),
            RibbonGroup::new(
                "styles",
                "Styles",
                vec![RibbonItem::gallery(
                    "styles",
                    options(&[("normal", "Normal"), ("h1", "Titre 1"), ("h2", "Titre 2"), ("quote", "Citation")]),
                )],
            ),
            RibbonGroup::new(
                "editing",
                "Édition",
                vec![
                    RibbonItem::button("find", "Rechercher", "Search").shortcut("Ctrl+F"),
                    RibbonItem::button("replace", "Remplacer", "Replace").shortcut("Ctrl+H"),
                    RibbonItem::menu(
                        "select",
                        "Sélectionner",
                        "MousePointer2",
                        vec![
                            RibbonItem::entry("select-all", "Sélectionner tout"),
                            RibbonItem::entry("select-objects", "Sélectionner les objets"),
                        ],
                    ),
                ],
            ),
        ],
    );
    let insert = RibbonTab::new(
        "insert",
        "Insertion",
        vec![
            RibbonGroup::new(
                "tables",
                "Tableaux",
                vec![RibbonItem::menu(
                    "table",
                    "Tableau",
                    "Table",
                    vec![RibbonItem::entry("table-2x2", "Tableau 2 × 2"), RibbonItem::entry("table-draw", "Dessiner un tableau")],
                )
                .large()],
            ),
            RibbonGroup::new(
                "illustrations",
                "Illustrations",
                vec![
                    RibbonItem::button("image", "Image", "Image").large(),
                    RibbonItem::menu(
                        "shapes",
                        "Formes",
                        "Shapes",
                        vec![RibbonItem::entry("shape-rect", "Rectangle"), RibbonItem::entry("shape-ellipse", "Ellipse")],
                    )
                    .large(),
                ],
            ),
            RibbonGroup::new(
                "links",
                "Liens",
                vec![RibbonItem::button("link", "Lien", "Link").large().shortcut("Ctrl+K")],
            ),
            RibbonGroup::new(
                "comments",
                "Commentaires",
                vec![RibbonItem::button("comment", "Nouveau commentaire", "MessageSquare").large()],
            ),
        ],
    );
    let layout = RibbonTab::new(
        "layout",
        "Mise en page",
        vec![RibbonGroup::new(
            "page",
            "Mise en page",
            vec![
                RibbonItem::menu(
                    "margins",
                    "Marges",
                    "PanelTop",
                    vec![RibbonItem::entry("m-normal", "Normales"), RibbonItem::entry("m-narrow", "Étroites")],
                )
                .large(),
                RibbonItem::menu(
                    "columns",
                    "Colonnes",
                    "Columns2",
                    vec![RibbonItem::entry("c-1", "Une"), RibbonItem::entry("c-2", "Deux")],
                )
                .large(),
            ],
        )],
    );
    let view = RibbonTab::new(
        "view",
        "Affichage",
        vec![
            RibbonGroup::new(
                "show",
                "Afficher",
                vec![
                    RibbonItem::toggle("ruler", "Règle", "Ruler", d.ruler),
                    RibbonItem::toggle("marks", "Marques", "Pilcrow", d.marks),
                ],
            ),
            RibbonGroup::new("zoom", "Zoom", vec![RibbonItem::button("zoom", "Zoom", "ZoomIn").large()]),
            RibbonGroup::new("spell", "Vérification", vec![RibbonItem::button("spell", "Orthographe", "SpellCheck").large()]),
        ],
    );
    let picture = RibbonTab::new(
        "picture",
        "Image",
        vec![
            RibbonGroup::new(
                "arrange",
                "Organiser",
                vec![
                    RibbonItem::button("crop", "Rogner", "Crop").large(),
                    RibbonItem::button("rotate", "Faire pivoter", "RotateCw").large(),
                ],
            ),
            RibbonGroup::new(
                "frame",
                "Bordure",
                vec![RibbonItem::gallery("border", options(&[("none", "Aucune"), ("thin", "Fine"), ("thick", "Épaisse")]))],
            ),
        ],
    )
    .contextual(hex(0xb7472a), image);
    vec![RibbonTab::file("file", "Fichier"), home, insert, layout, view, picture]
}

/// The fragment an « Assistant » module would merge into the Documents ribbon: a group after
/// Presse-papiers (`RIBBON.md` §8, customUI's `insertAfterMso`).
fn assistant_fragment() -> kubuno_ui::ribbon::merge::RibbonExtension {
    use kubuno_ui::ribbon::merge::{GroupMerge, RibbonExtension, TabMerge};
    RibbonExtension::new(vec![TabMerge::into_tab(
        "home",
        vec![GroupMerge::new(RibbonGroup::new("ai", "Assistant", vec![RibbonItem::button("summarise", "Résumer", "Sparkles").large()])).after("clipboard")],
    )])
}

// ─────────────────────────────────────────────────────────────────────────────
// The exposition
// ─────────────────────────────────────────────────────────────────────────────

pub fn draw(c: &dyn Canvas, f: &Frame) {
    let mut p = Page::new(c, f.size.0 - interact::PANEL_W(), f.size.1);
    with_ui(|ui| {
        ui.sync();
        let t = c.theme();
        let fm = c.formats();
        let right = p.area.right - MARGIN;

        // ── 1. The Documents ribbon over its document ─────────────────────────
        p.section("Ruban — @office Ribbon.tsx : bande d'onglets, groupes, repli responsive");
        p.caption("en direct : « Fichier » ouvre le Backstage · actions rapides · gros boutons, piles de 3, listes, bascules, boutons scindés, galerie · Ctrl+F1 réduit");
        let frame = Rect::new(MARGIN, p.y, right, p.y + 114.0 + DOC_H);
        c.fill_rounded(&frame, 8.0, &t.card_background);
        c.push_clip_rounded(&frame, 8.0);
        let run = ui.main.frame(c, frame, f);
        let doc = run.content;
        if run.backstage {
            let b = ui.backstage.frame(c, doc, f, &ui.main.theme);
            if b.back {
                ui.main.close_backstage();
            }
            if let Some(a) = b.action {
                ui.log.insert(0, format!("Backstage · action « {a} »"));
                if a == "close" {
                    ui.main.close_backstage();
                }
            }
            let panel = Rect::new(b.content.left + 32.0, b.content.top + 24.0, b.content.right - 24.0, b.content.top + 56.0);
            let title = match b.active.as_str() {
                "info" => "Informations",
                "export" => "Exporter",
                _ => "Accueil",
            };
            c.text(title, &panel, &fm.title, &t.text_primary, false);
        } else {
            paint_document(c, &ui.doc, doc);
        }
        c.pop_clip_rounded();
        c.stroke_rounded(&frame, 8.0, &t.card_stroke);
        let events = run.events;
        ui.apply("Documents", events);
        p.advance(frame.bottom - frame.top);

        // ── 2. Narrow: groups fold into chips ─────────────────────────────────
        p.caption("à 520 DIP : les groupes de droite se replient en boutons ; un clic ouvre le groupe entier en popover");
        let narrow = Rect::new(MARGIN, p.y, (MARGIN + NARROW_W).min(right), p.y + 114.0);
        c.push_clip(&narrow);
        let run = ui.narrow.frame(c, narrow, f);
        c.pop_clip();
        c.stroke_rounded(&narrow, 0.0, &t.card_stroke);
        ui.apply("Tableur (étroit)", run.events);
        p.advance(narrow.bottom - narrow.top);

        // ── 3. The plain workspace chrome ─────────────────────────────────────
        p.caption("sans bande colorée (WORKSPACE_LIGHT) : onglet actif souligné à l'accent");
        let plain = Rect::new(MARGIN, p.y, right, p.y + 114.0);
        c.push_clip(&plain);
        let run = ui.plain.frame(c, plain, f);
        c.pop_clip();
        c.stroke_rounded(&plain, 0.0, &t.card_stroke);
        ui.apply("Clair", run.events);
        p.advance(plain.bottom - plain.top);
    });
}

/// A white page on the document ground, its text in the ribbon's live format.
fn paint_document(c: &dyn Canvas, d: &Doc, area: Rect) {
    let t = c.theme();
    let fm = c.formats();
    c.fill_rounded(&area, 0.0, &t.card_background);
    let w = (area.right - area.left - 80.0).min(560.0);
    let x = (area.left + area.right) / 2.0 - w / 2.0;
    let page = Rect::new(x, area.top + 16.0, x + w, area.bottom + 40.0);
    c.draw_card_shadow(&page, 2.0);
    c.fill_rounded(&page, 2.0, &t.layer_background);
    let mut marks = Vec::new();
    for (on, name) in [
        (d.bold, "gras"),
        (d.italic, "italique"),
        (d.underline, "souligné"),
        (d.strike, "barré"),
        (d.sub, "indice"),
        (d.sup, "exposant"),
    ] {
        if on {
            marks.push(name);
        }
    }
    let marks = if marks.is_empty() { "aucun".to_string() } else { marks.join(", ") };
    let lines = [
        format!("{} · {} pt · style « {} »", d.font, d.size, d.style),
        format!("Attributs : {marks} · alignement : {}", d.align),
        format!("Règle : {} · marques de paragraphe : {}", if d.ruler { "oui" } else { "non" }, if d.marks { "oui" } else { "non" }),
    ];
    let mut y = page.top + 28.0;
    let head = if d.bold { &fm.heading_strong } else { &fm.heading };
    c.text("Rapport trimestriel", &Rect::new(page.left + 40.0, y, page.right - 40.0, y + 24.0), head, &t.text_primary, false);
    y += 36.0;
    for l in lines {
        c.text(&l, &Rect::new(page.left + 40.0, y, page.right - 40.0, y + 20.0), &fm.body, &t.text_secondary, false);
        y += 24.0;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The interactive column
// ─────────────────────────────────────────────────────────────────────────────

pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    with_ui(|ui| {
        let live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;
        let t = c.theme();
        let fm = c.formats();
        let (left, mut y, right) = interact::panel(c, interact::panel_rect(f.size));

        y = interact::caption(c, left, right, y, "Onglet contextuel — apparaît à droite, le ruban y bascule");
        let sw = Switch::new().on(ui.image).label("Image sélectionnée (onglet « Image »)");
        let h = sw.height_for_width(c, right - left);
        let r = Rect::new(left, y, left + sw.measure(c).width.min(right - left), y + h);
        if live.hit(r) {
            ui.image = !ui.image;
        }
        if live.hover(r) {
            host::set_cursor(Cursor::Hand);
        }
        sw.paint(c, r, live.state(r));
        y += h + 16.0;

        y = interact::caption(c, left, right, y, "Extension d'un autre module — un groupe fusionné dans Accueil");
        let sw = Switch::new().on(ui.assistant).label("Module « Assistant » installé (RibbonExtension)");
        let h = sw.height_for_width(c, right - left);
        let r = Rect::new(left, y, left + sw.measure(c).width.min(right - left), y + h);
        if live.hit(r) {
            ui.assistant = !ui.assistant;
        }
        if live.hover(r) {
            host::set_cursor(Cursor::Hand);
        }
        sw.paint(c, r, live.state(r));
        y += h + 16.0;

        y = interact::caption(c, left, right, y, "Réduction du ruban — clic sur un onglet : aperçu flottant");
        let collapsed = ui.main.is_collapsed();
        let sw = Switch::new().on(collapsed).label("Ruban réduit (Ctrl+F1)");
        let h = sw.height_for_width(c, right - left);
        let r = Rect::new(left, y, left + sw.measure(c).width.min(right - left), y + h);
        if live.hit(r) {
            ui.main.set_collapsed(!collapsed);
        }
        if live.hover(r) {
            host::set_cursor(Cursor::Hand);
        }
        sw.paint(c, r, live.state(r));
        y += h + 16.0;

        y = interact::caption(c, left, right, y, "Teinte de l'application (OFFICE_TONE)");
        let d = 26.0;
        let mut x = left;
        for (i, (_, colour)) in TONES.iter().enumerate() {
            if x + d > right {
                x = left;
                y += d + 8.0;
            }
            let r = Rect::new(x, y, x + d, y + d);
            if live.hit(r) {
                ui.tone = i;
            }
            if live.hover(r) {
                host::set_cursor(Cursor::Hand);
            }
            if i == ui.tone {
                c.stroke_rounded_w(&Rect::new(r.left - 3.0, r.top - 3.0, r.right + 3.0, r.bottom + 3.0), d, colour, 2.0);
            }
            c.fill_rounded(&r, d / 2.0, colour);
            x += d + 10.0;
        }
        y += d + 6.0;
        c.text(TONES[ui.tone].0, &Rect::new(left, y, right, y + 20.0), &fm.caption, &t.text_secondary, false);
        y += 32.0;

        y = interact::caption(c, left, right, y, "Événements (du plus récent au plus ancien)");
        if ui.log.is_empty() {
            c.text("Aucun pour l'instant", &Rect::new(left, y, right, y + 20.0), &fm.caption, &t.text_tertiary, false);
        }
        for l in &ui.log {
            c.text_ellipsis(l, &Rect::new(left, y, right, y + 20.0), &fm.caption, &t.text_secondary);
            y += 22.0;
        }
    });
}
