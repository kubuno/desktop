//! Gallery page — the dock and the workspace shell
//! (`core/frontend/src/core/shell/workspace/`: `Dock.tsx`, `WorkspaceShell.tsx`,
//! `MenuBar.tsx`, `WorkspaceMenuBar.tsx`).
//!
//! The exposition (left) runs the two web usages LIVE:
//!
//! * the PaintSharp « Layer » editor: a dark `WorkspaceShell` (topbar, compact
//!   menu bar, options bar, tool rail, status bar) around a `DockArea` whose
//!   default arrangement is the web's own —
//!   `right: [['navigator'], ['layers'], ['brush', 'adjust', 'filters']]` —
//!   over the `#141414` canvas;
//! * the App builder: the light dock theme, `left: [['elements'], ['tree']]`,
//!   `right: [['inspector']]`, over the `#eef1f5` viewport.
//!
//! Drag a tab: the guide diamond appears over the pane under the pointer and
//! the window-edge arrows at the dock's sides; the ghost shows the landing
//! zone; dropping elsewhere tears the panel off as a float. Double-click a tab
//! to float it (or re-dock a float), right-click for its menu, drag the gutters
//! to resize. Ctrl+Tab / Ctrl+Shift+Tab cycle the panels of the focused dock,
//! Escape cancels a drag.
//!
//! The interactive column (right): hide the panels, reset the layouts, reopen
//! closed panels (the web's right-rail control), and the event log.

use std::cell::RefCell;

use drive_app_controls::Canvas;
use kubuno_controls::host::{self, Cursor, Frame};
use kubuno_ui::buttons::{Button, Switch, Variant};
use kubuno_ui::dock::{hex, DockArea, DockArrangement, DockEvent, DockPanel, DockSlot, DockTheme};
use kubuno_ui::workspace::{MenuBarStyle, WorkspaceShell, WorkspaceTheme, WsMenu, WsMenuItem};
use kubuno_ui::{Rect, Widget};

use super::interact::{self, Live};
use super::sheet::{Page, MARGIN};

/// The PaintSharp demo's height.
const PAINT_H: f32 = 540.0;
/// The App builder demo's height.
const APP_H: f32 = 400.0;
/// The default-theme demo's height.
const PLAIN_H: f32 = 300.0;

struct Ui {
    shell: WorkspaceShell,
    paint: DockArea,
    app: DockArea,
    /// A dock on the default theme (the app's tokens, light or dark).
    plain: DockArea,
    /// Which dock takes Ctrl+Tab (the last one clicked).
    focused_app: bool,
    log: Vec<String>,
    prev_down: bool,
}

fn paint_dock() -> DockArea {
    let panels = vec![
        DockPanel::new("navigator", "Navigateur").icon("Map"),
        DockPanel::new("layers", "Calques").icon("Layers"),
        DockPanel::new("brush", "Pinceau"),
        DockPanel::new("adjust", "Réglages"),
        DockPanel::new("filters", "Filtres"),
    ];
    let mut d = DockArea::new(panels, DockArrangement::new().right(&["navigator"]).right(&["layers"]).right(&["brush", "adjust", "filters"]))
        .with_theme(DockTheme::workspace_dark())
        .with_viewport_bg(hex(0x141414));
    d.move_title = Some("Glisser pour déplacer / détacher".into());
    d
}

/// The default dock theme: `DEFAULT_THEME` (the core tokens), so it follows
/// the app's light or dark palette; the viewport takes the window ground.
fn plain_dock() -> DockArea {
    let panels = vec![
        DockPanel::new("explorer", "Explorateur"),
        DockPanel::new("props", "Propriétés"),
        DockPanel::new("output", "Sortie"),
        DockPanel::new("problems", "Problèmes").closable(false),
    ];
    let mut d = DockArea::new(panels, DockArrangement::new().left(&["explorer"]).right(&["props"]).right(&["output", "problems"]));
    d.keyboard = false;
    d
}

fn app_dock() -> DockArea {
    let panels = vec![
        DockPanel::new("elements", "Éléments"),
        DockPanel::new("tree", "Arborescence"),
        DockPanel::new("inspector", "Inspecteur"),
    ];
    let mut d = DockArea::new(panels, DockArrangement::new().left(&["elements"]).left(&["tree"]).right(&["inspector"]))
        .with_theme(DockTheme::workspace_light())
        .with_viewport_bg(hex(0xeef1f5));
    d.move_title = Some("Glisser pour déplacer / détacher".into());
    d.keyboard = false;
    d
}

fn paint_menus() -> Vec<WsMenu> {
    let a = |id: &str, l: &str| WsMenuItem::action(id, l);
    vec![
        WsMenu::new("Fichier", vec![a("new", "Nouveau").shortcut("Ctrl+N"), a("open", "Ouvrir…").shortcut("Ctrl+O"), WsMenuItem::Separator, a("save", "Enregistrer").shortcut("Ctrl+S")]),
        WsMenu::new("Édition", vec![a("undo", "Annuler").shortcut("Ctrl+Z"), a("redo", "Rétablir").shortcut("Ctrl+Shift+Z")]),
        WsMenu::new("Image", vec![a("resize", "Taille de l'image…"), a("canvas", "Taille de la zone de travail…")]),
        WsMenu::new("Calque", vec![a("layer.new", "Nouveau calque"), a("layer.dup", "Dupliquer le calque")]),
        WsMenu::new("Fenêtre", vec![a("win.reset", "Réinitialiser la disposition"), WsMenuItem::Separator, a("win.layers", "Calques"), a("win.navigator", "Navigateur")]),
    ]
}

impl Ui {
    fn new() -> Self {
        let mut shell = WorkspaceShell::new(WorkspaceTheme::dark()).with_menus(paint_menus(), MenuBarStyle::Compact);
        shell.title = "Affiche festival".into();
        shell.title_icon = Some("Image");
        shell.subtitle = Some("Layer".into());
        shell.doc_info = Some("1920×1080".into());
        shell.show_back = true;
        shell.show_delete = true;
        shell.show_search = true;
        shell.topbar_actions_width = 64.0;
        shell.options_bar_height = 30.0;
        shell.tool_rail_width = 44.0;
        shell.status_height = 22.0;
        shell.status = vec!["100 %".into(), "RVB/8".into(), "Calque 2".into()];
        Self { shell, paint: paint_dock(), app: app_dock(), plain: plain_dock(), focused_app: false, log: Vec::new(), prev_down: false }
    }

    fn record(&mut self, who: &str, events: Vec<DockEvent>) {
        for e in events {
            let line = match e {
                DockEvent::Activated(p) => format!("{who} · activé « {p} »"),
                DockEvent::Closed(p) => format!("{who} · fermé « {p} »"),
                DockEvent::Opened(p) => format!("{who} · rouvert « {p} »"),
                DockEvent::LayoutChanged => format!("{who} · disposition modifiée"),
            };
            if self.log.first() != Some(&line) {
                self.log.insert(0, line);
            }
        }
        self.log.truncate(12);
    }

    fn sync_keyboard(&mut self) {
        self.paint.keyboard = !self.focused_app;
        self.app.keyboard = self.focused_app;
    }
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> R {
    UI.with(|u| f(u.borrow_mut().get_or_insert_with(Ui::new)))
}

// ─────────────────────────────────────────────────────────────────────────────
// The exposition
// ─────────────────────────────────────────────────────────────────────────────

pub fn draw(c: &dyn Canvas, f: &Frame) {
    let mut p = Page::new(c, f.size.0 - interact::PANEL_W(), f.size.1);
    with_ui(|ui| {
        // Clear of the page's vertical scroll bar.
        let right = p.area.right - MARGIN - 12.0;
        let t = c.theme();

        // ── 1. PaintSharp: WorkspaceShell (dark) + DockArea ─────────────────
        p.section("Dock — @core Dock.tsx + WorkspaceShell.tsx (PaintSharp « Layer »)");
        p.caption("glisser un onglet : losange + flèches de bord, fantôme ; ailleurs = flottant · double-clic : détacher · clic droit : menu");
        let frame = Rect::new(MARGIN, p.y, right, p.y + PAINT_H);
        if f.mouse_down && !ui.prev_down && frame.contains(f.mouse.0, f.mouse.1) {
            ui.focused_app = false;
        }
        c.push_clip_rounded(&frame, 8.0);
        let run = ui.shell.frame(c, frame, f);
        if let Some(id) = run.menu {
            ui.log.insert(0, format!("Menu · « {id} »"));
            match id.as_str() {
                "win.reset" => ui.paint.reset(),
                "win.layers" => ui.paint.user_open("layers"),
                "win.navigator" => ui.paint.user_open("navigator"),
                _ => {}
            }
        }
        if run.back {
            ui.log.insert(0, "Topbar · retour".into());
        }
        if run.delete {
            ui.log.insert(0, "Topbar · corbeille".into());
        }
        paint_topbar_actions(c, run.topbar_actions);
        if let Some(r) = run.options_bar {
            paint_options(c, r);
        }
        if let Some(r) = run.tool_rail {
            paint_tool_rail(c, r);
        }
        let dark = ui.shell.theme;
        let events = ui.paint.frame(c, run.body, f, &mut |c, _pf, slot, rect| match slot {
            DockSlot::Viewport => paint_canvas(c, rect),
            DockSlot::Panel(id) => paint_paint_panel(c, id, rect, &dark),
        });
        c.pop_clip_rounded();
        ui.record("PaintSharp", events.events);
        p.advance(PAINT_H);

        // ── 2. The default theme (follows light / dark) ─────────────────────
        p.caption("thème par défaut (DEFAULT_THEME : jetons de l'app, clair ou sombre) · « Problèmes » n'est pas fermable");
        let frame = Rect::new(MARGIN, p.y, right, p.y + PLAIN_H);
        c.push_clip_rounded(&frame, 8.0);
        let ground = t.window_background;
        ui.plain.viewport_bg = Some(ground);
        let events = ui.plain.frame(c, frame, f, &mut |c, _pf, slot, rect| match slot {
            DockSlot::Viewport => {
                let th = c.theme();
                c.text("Zone de travail", &rect, &c.formats().subtitle, &th.text_secondary, true);
            }
            DockSlot::Panel(id) => {
                let th = c.theme();
                let inner = Rect::new(rect.left + 12.0, rect.top + 10.0, rect.right - 12.0, rect.top + 30.0);
                let s = match id {
                    "explorer" => "src/ · main.rs · lib.rs",
                    "props" => "Nom : button1 · Texte : OK",
                    "output" => "Compilation terminée (0 erreur)",
                    _ => "Aucun problème",
                };
                text(c, s, inner, &th.text_secondary);
            }
        });
        c.pop_clip_rounded();
        c.stroke_rounded(&frame, 8.0, &t.card_stroke);
        ui.record("Défaut", events.events);
        p.advance(PLAIN_H);

        // ── 3. App builder: the light dock theme ────────────────────────────
        p.caption("App builder — thème clair (WORKSPACE_LIGHT)");
        let frame = Rect::new(MARGIN, p.y, right, p.y + APP_H);
        if f.mouse_down && !ui.prev_down && frame.contains(f.mouse.0, f.mouse.1) {
            ui.focused_app = true;
        }
        c.push_clip_rounded(&frame, 8.0);
        let events = ui.app.frame(c, frame, f, &mut |c, _pf, slot, rect| match slot {
            DockSlot::Viewport => paint_app_viewport(c, rect),
            DockSlot::Panel(id) => paint_app_panel(c, id, rect),
        });
        c.pop_clip_rounded();
        c.stroke_rounded(&frame, 8.0, &t.card_stroke);
        ui.record("App builder", events.events);
        p.advance(APP_H);
        ui.sync_keyboard();
    });
}

fn text(c: &dyn Canvas, s: &str, r: Rect, colour: &windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F) {
    c.text_ellipsis(s, &r, &c.formats().caption, colour);
}

fn paint_topbar_actions(c: &dyn Canvas, r: Rect) {
    let ink = hex(0x8e8e8e);
    let cy = (r.top + r.bottom) / 2.0;
    for (i, icon) in ["Undo2", "Redo2"].iter().enumerate() {
        let x = r.left + i as f32 * 32.0;
        c.vector_icon(icon, &Rect::new(x, cy - 14.0, x + 28.0, cy + 14.0), 16.0, &ink);
    }
}

fn paint_options(c: &dyn Canvas, r: Rect) {
    let ink = hex(0xd6d6d6);
    let dim = hex(0x8e8e8e);
    let mut x = r.left;
    for (label, value) in [("Taille", "24 px"), ("Dureté", "80 %"), ("Opacité", "100 %"), ("Flux", "100 %")] {
        text(c, label, Rect::new(x, r.top, x + 50.0, r.bottom), &dim);
        x += 46.0;
        let field = Rect::new(x, r.top + 5.0, x + 52.0, r.bottom - 5.0);
        c.fill_rounded(&field, 3.0, &hex(0x1e1e1e));
        text(c, value, Rect::new(field.left + 6.0, field.top, field.right, field.bottom), &ink);
        x = field.right + 16.0;
    }
}

fn paint_tool_rail(c: &dyn Canvas, r: Rect) {
    let ink = hex(0xd6d6d6);
    for (i, icon) in ["MousePointer2", "Paintbrush", "Eraser", "Type", "Crop", "ZoomIn"].iter().enumerate() {
        let y = r.top + i as f32 * 34.0;
        let cell = Rect::new(r.left + 6.0, y, r.right - 6.0, y + 30.0);
        if i == 1 {
            c.fill_rounded(&cell, 4.0, &hex(0x454545));
        }
        c.vector_icon(icon, &cell, 16.0, &ink);
    }
}

/// The PaintSharp canvas: a document on the `#141414` viewport.
fn paint_canvas(c: &dyn Canvas, r: Rect) {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let dw = (w - 80.0).max(40.0).min((h - 80.0).max(40.0) * 16.0 / 9.0);
    let dh = dw * 9.0 / 16.0;
    let doc = Rect::new(r.left + (w - dw) / 2.0, r.top + (h - dh) / 2.0, r.left + (w + dw) / 2.0, r.top + (h + dh) / 2.0);
    c.fill_rounded(&doc, 0.0, &hex(0xf4efe6));
    let sun = Rect::new(doc.left + dw * 0.62, doc.top + dh * 0.18, doc.left + dw * 0.62 + dh * 0.3, doc.top + dh * 0.48);
    c.fill_rounded(&sun, dh * 0.15, &hex(0xe8603c));
    let band = Rect::new(doc.left, doc.top + dh * 0.62, doc.right, doc.bottom);
    c.fill_rounded(&band, 0.0, &hex(0x2b3a67));
    let title = Rect::new(doc.left + dw * 0.08, doc.top + dh * 0.2, doc.left + dw * 0.6, doc.top + dh * 0.4);
    c.text("FESTIVAL", &title, &c.formats().title, &hex(0x2b3a67), false);
}

fn paint_paint_panel(c: &dyn Canvas, id: &str, r: Rect, th: &WorkspaceTheme) {
    let inner = Rect::new(r.left + 10.0, r.top + 8.0, r.right - 10.0, r.bottom - 8.0);
    match id {
        "navigator" => {
            let w = inner.right - inner.left;
            let h = (w * 9.0 / 16.0).min(inner.bottom - inner.top - 20.0).max(10.0);
            let thumb = Rect::new(inner.left, inner.top, inner.right, inner.top + h);
            c.fill_rounded(&thumb, 2.0, &hex(0x141414));
            let doc = Rect::new(thumb.left + 12.0, thumb.top + 8.0, thumb.right - 12.0, thumb.bottom - 8.0);
            c.fill_rounded(&doc, 0.0, &hex(0xf4efe6));
            c.stroke_rounded_w(&Rect::new(doc.left + 10.0, doc.top + 6.0, doc.right - 20.0, doc.bottom - 10.0), 0.0, &hex(0xe8603c), 1.5);
            text(c, "100 %", Rect::new(inner.left, thumb.bottom + 4.0, inner.right, thumb.bottom + 22.0), &th.text_dim);
        }
        "layers" => {
            for (i, name) in ["Titre", "Soleil", "Bande", "Fond"].iter().enumerate() {
                let y = inner.top + i as f32 * 30.0;
                if y + 28.0 > inner.bottom {
                    break;
                }
                let row = Rect::new(inner.left - 4.0, y, inner.right + 4.0, y + 28.0);
                if i == 1 {
                    c.fill_rounded(&row, 4.0, &th.active);
                }
                c.vector_icon("Eye", &Rect::new(row.left + 4.0, row.top, row.left + 24.0, row.bottom), 14.0, &th.text_dim);
                let sw = Rect::new(row.left + 30.0, row.top + 4.0, row.left + 50.0, row.bottom - 4.0);
                c.fill_rounded(&sw, 2.0, &[hex(0x2b3a67), hex(0xe8603c), hex(0x2b3a67), hex(0xf4efe6)][i]);
                text(c, name, Rect::new(sw.right + 8.0, row.top, row.right, row.bottom), &th.text);
            }
        }
        _ => {
            let rows: &[(&str, f32)] = match id {
                "brush" => &[("Taille", 0.3), ("Dureté", 0.8), ("Opacité", 1.0), ("Flux", 0.6)],
                "adjust" => &[("Luminosité", 0.5), ("Contraste", 0.55), ("Saturation", 0.4)],
                _ => &[("Flou", 0.2), ("Netteté", 0.35), ("Bruit", 0.1)],
            };
            for (i, (label, v)) in rows.iter().enumerate() {
                let y = inner.top + i as f32 * 34.0;
                if y + 30.0 > inner.bottom {
                    break;
                }
                text(c, label, Rect::new(inner.left, y, inner.right, y + 16.0), &th.text_dim);
                let track = Rect::new(inner.left, y + 20.0, inner.right, y + 24.0);
                c.fill_rounded(&track, 2.0, &hex(0x1e1e1e));
                let fillr = Rect::new(track.left, track.top, track.left + (track.right - track.left) * v, track.bottom);
                c.fill_rounded(&fillr, 2.0, &th.accent);
            }
        }
    }
}

fn paint_app_viewport(c: &dyn Canvas, r: Rect) {
    let w = (r.right - r.left - 60.0).clamp(40.0, 560.0);
    let x = (r.left + r.right) / 2.0 - w / 2.0;
    let page = Rect::new(x, r.top + 24.0, x + w, r.bottom + 20.0);
    c.draw_card_shadow(&page, 8.0);
    c.fill_rounded(&page, 8.0, &hex(0xffffff));
    let head = Rect::new(page.left, page.top, page.right, page.top + 48.0);
    c.fill_top_rounded(&head, 8.0, &hex(0x1a73e8));
    c.text("Mon application", &Rect::new(head.left + 16.0, head.top, head.right, head.bottom), &c.formats().subtitle, &hex(0xffffff), false);
    for i in 0..4 {
        let y = head.bottom + 20.0 + i as f32 * 44.0;
        let field = Rect::new(page.left + 16.0, y, page.right - 16.0, y + 32.0);
        c.stroke_rounded(&field, 6.0, &hex(0xdadce0));
        text(c, ["Nom", "Adresse e-mail", "Téléphone", "Message"][i], Rect::new(field.left + 10.0, field.top, field.right, field.bottom), &hex(0x5f6368));
    }
}

fn paint_app_panel(c: &dyn Canvas, id: &str, r: Rect) {
    let inner = Rect::new(r.left + 12.0, r.top + 10.0, r.right - 12.0, r.bottom - 8.0);
    let ink = hex(0x202124);
    let dim = hex(0x5f6368);
    let rows: &[&str] = match id {
        "elements" => &["Conteneur", "Titre", "Texte", "Bouton", "Image", "Formulaire"],
        "tree" => &["Page", "  En-tête", "  Formulaire", "    Champ « Nom »", "    Bouton « Envoyer »"],
        _ => &["Largeur : 100 %", "Marge : 16 px", "Couleur : #1a73e8", "Police : Inter"],
    };
    for (i, row) in rows.iter().enumerate() {
        let y = inner.top + i as f32 * 26.0;
        if y + 24.0 > inner.bottom {
            break;
        }
        if id == "elements" {
            let chip = Rect::new(inner.left, y, inner.right, y + 22.0);
            c.stroke_rounded(&chip, 4.0, &hex(0xdadce0));
            text(c, row, Rect::new(chip.left + 8.0, chip.top, chip.right, chip.bottom), &ink);
        } else {
            text(c, row, Rect::new(inner.left, y, inner.right, y + 22.0), if i == 0 { &ink } else { &dim });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The interactive column
// ─────────────────────────────────────────────────────────────────────────────

pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    with_ui(|ui| {
        let live = Live::new(f, ui.prev_down);
        let t = c.theme();
        let fm = c.formats();
        let (left, mut y, right) = interact::panel(c, interact::panel_rect(f.size));

        y = interact::caption(c, left, right, y, "Panneaux masqués (hidden) — le viewport seul, pleine taille");
        for (label, app) in [("PaintSharp : masquer les panneaux", false), ("App builder : masquer les panneaux", true)] {
            let dock = if app { &mut ui.app } else { &mut ui.paint };
            let sw = Switch::new().on(dock.hidden).label(label);
            let h = sw.height_for_width(c, right - left);
            let r = Rect::new(left, y, left + sw.measure(c).width.min(right - left), y + h);
            if live.hit(r) {
                dock.hidden = !dock.hidden;
            }
            if live.hover(r) {
                host::set_cursor(Cursor::Hand);
            }
            sw.paint(c, r, live.state(r));
            y += h + 10.0;
        }
        y += 6.0;

        y = interact::caption(c, left, right, y, "Disposition — réinitialiser (menu d'onglet « Réinitialiser la disposition »)");
        let mut x = left;
        for (label, app) in [("Réinitialiser PaintSharp", false), ("Réinitialiser App builder", true)] {
            let b = Button::new(label).variant(Variant::Secondary);
            let s = b.measure(c);
            let r = Rect::new(x, y, x + s.width, y + s.height);
            if live.hit(r) {
                if app {
                    ui.app.reset();
                } else {
                    ui.paint.reset();
                }
            }
            if live.hover(r) {
                host::set_cursor(Cursor::Hand);
            }
            b.paint(c, r, live.state(r));
            x = r.right + 8.0;
            if app {
                y = r.bottom + 16.0;
            }
        }

        y = interact::caption(c, left, right, y, "Panneaux fermés — le contrôle du rail droit (dockReopenStore)");
        let mut any = false;
        for (who, app) in [("PaintSharp", false), ("App builder", true)] {
            let dock = if app { &mut ui.app } else { &mut ui.paint };
            let btn = Rect::new(left, y, left + 40.0, y + 40.0);
            if dock.paint_reopen_button(c, btn, f) {
                any = true;
                let names: Vec<String> = dock.closed_panels().into_iter().map(|(_, l)| l).collect();
                c.text_ellipsis(&format!("{who} : {}", names.join(", ")), &Rect::new(btn.right + 10.0, y, right, y + 40.0), &fm.caption, &t.text_secondary);
                y += 48.0;
            }
        }
        if !any {
            c.text("Aucun — fermez un onglet (×) pour le voir ici", &Rect::new(left, y, right, y + 20.0), &fm.caption, &t.text_tertiary, false);
            y += 28.0;
        }
        y += 8.0;

        y = interact::caption(c, left, right, y, "Clavier");
        let focused = if ui.focused_app { "App builder" } else { "PaintSharp" };
        for line in [
            format!("Ctrl+Tab / Ctrl+Maj+Tab : panneau suivant / précédent ({focused})"),
            "Échap : annule un glissement ou un redimensionnement".to_string(),
            "Menu (ou Maj+F10) : menu de l'onglet actif".to_string(),
        ] {
            c.text_ellipsis(&line, &Rect::new(left, y, right, y + 20.0), &fm.caption, &t.text_secondary);
            y += 22.0;
        }
        y += 10.0;

        y = interact::caption(c, left, right, y, "Événements (du plus récent au plus ancien)");
        if ui.log.is_empty() {
            c.text("Aucun pour l'instant", &Rect::new(left, y, right, y + 20.0), &fm.caption, &t.text_tertiary, false);
            y += 22.0;
        }
        for l in &ui.log {
            c.text_ellipsis(l, &Rect::new(left, y, right, y + 20.0), &fm.caption, &t.text_secondary);
            y += 22.0;
        }
        y += 10.0;
        y = interact::caption(c, left, right, y, "Disposition PaintSharp (JSON, forme du web)");
        let json = ui.paint.save_layout();
        let per = ((right - left) / 6.2).max(20.0) as usize;
        let chars: Vec<char> = json.chars().collect();
        for chunk in chars.chunks(per).take(8) {
            let s: String = chunk.iter().collect();
            c.text(&s, &Rect::new(left, y, right, y + 18.0), &fm.caption, &t.text_tertiary, false);
            y += 18.0;
        }
        ui.prev_down = f.mouse_down;
    });
}
