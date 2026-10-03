//! Gallery page — the navigation family.
//!
//! The static exposition shows each primitive in the states the web design
//! system distinguishes (rest, hover, pressed, keyboard ring, disabled,
//! selected) and in the overflow situations it must survive: a tool bar too
//! narrow for its commands (the « More » menu), a trail too long for its bar
//! (the head folded behind `…`, a long segment truncated), a tab strip too
//! narrow for its tabs (scroll arrows, the selected tab kept in view), a status
//! bar whose fixed cells keep their text. The command bar is still shown
//! beside its predecessor (`drive_app_controls::toolbar`), fed the rebuilt
//! bar's own rectangles, so the comparison is about the ink.
//!
//! The interactive column drives the same primitives with the mouse, the wheel
//! AND the keyboard, the ARIA way: a tool bar, a tab list and a navigation tree
//! are ONE Tab stop each whose arrows move inside (Home / End too); a trail is
//! a list of links, one Tab stop per link. The overflow menus open in floating
//! popup windows (`host::popup`) that may hang past the window, and the icon
//! commands show their name in a tooltip overlay (`host::overlay`).

use std::cell::RefCell;

use drive_app_controls::sidebar::SidebarMode;
use drive_app_controls::toolbar as web_toolbar;
use kubuno_controls::host::{self, vk, Frame, Modifiers};
use kubuno_controls::layout_panels::TabAlignment;
use kubuno_controls::toolstrip::{StripItem, ToolStripItemAlignment};
use kubuno_ui::containers::Card;
use kubuno_ui::display::{Tooltip, TooltipTrigger};
use kubuno_ui::lists::Menu;
use kubuno_ui::navigation::{
    icon_item, icon_label_item, menu_item, nav_item, place_menu, roving_step,
    section_item, separator_item, show_menu_popup, status_item, take_activate, take_nav_keys,
    toggle_item, with_tooltip, Breadcrumb, NavKey, Sidebar, StatusBar, StripPaint, Tabs, TabsPaint,
    TabsController, Toolbar, ToolbarTarget,
};
use kubuno_ui::{Canvas, Rect, Widget, WidgetState};

use super::interact::{self, Live};
use super::sheet::{self, Page, MARGIN};

// ─────────────────────────────────────────────────────────────────────────────
// One description of the command bar, rendered twice
//
// The two halves must not be allowed to describe *different* bars, so the bar
// is written once as a list of these, and each half turns one entry into its
// own vocabulary: a `StripItem` for the replica model, a `ButtonVisual` for the
// predecessor's flattened view.
// ─────────────────────────────────────────────────────────────────────────────

enum Cmd {
    New(&'static str),
    Separator,
    Icon(&'static str, &'static str),
    Disabled(&'static str, &'static str),
    Toggle(&'static str, &'static str, bool),
    Menu(&'static str, &'static str),
    Labelled(&'static str, &'static str),
}

impl Cmd {
    /// The replica item the rebuilt bar is built from.
    fn item(&self) -> StripItem {
        match self {
            Cmd::New(label) => menu_item("Plus", label),
            Cmd::Separator => separator_item(),
            Cmd::Icon(name, tip) => with_tooltip(icon_item(name), tip),
            Cmd::Disabled(name, tip) => {
                let mut it = with_tooltip(icon_item(name), tip);
                if let StripItem::Button(b) = &mut it {
                    b.item.enabled = false;
                }
                it
            }
            Cmd::Toggle(name, tip, on) => with_tooltip(toggle_item(name, *on), tip),
            Cmd::Menu(name, tip) => with_tooltip(menu_item(name, ""), tip),
            Cmd::Labelled(name, label) => icon_label_item(name, label),
        }
    }

    /// The predecessor's already-flattened visual for the same command.
    fn visual(&self, hot: bool) -> web_toolbar::ButtonVisual {
        use web_toolbar::ButtonVisual as V;
        match self {
            Cmd::New(label) => V::NewButton { label: (*label).to_string(), hot },
            Cmd::Separator => V::Separator,
            Cmd::Icon(name, _) => V::Icon { name, hot, enabled: true },
            Cmd::Disabled(name, _) => V::Icon { name, hot, enabled: false },
            Cmd::Toggle(name, _, on) => V::Toggle { name, on: *on, hot },
            Cmd::Menu(name, _) => V::IconChevron { name, accent_layer: false, hot },
            Cmd::Labelled(name, label) => {
                V::IconLabel { name, label: (*label).to_string(), hot, enabled: true }
            }
        }
    }
}

/// The full bar: the command set Drive's own « AlwaysVisible » context lays
/// out, plus the two toggles of its right block.
const BAR: [Cmd; 9] = [
    Cmd::New("Nouveau"),
    Cmd::Separator,
    Cmd::Icon("Cut", "Couper"),
    Cmd::Icon("Copy", "Copier"),
    Cmd::Disabled("Paste", "Coller"),
    Cmd::Labelled("RestoreDeleted", "Restaurer"),
    Cmd::Menu("Sorting", "Trier"),
    Cmd::Toggle("PanelRight", "Volet de détails", true),
    Cmd::Toggle("Shelf", "Étagère", false),
];

/// The part of it that fits a half-width cell, for the side-by-side pair.
const PAIR: [Cmd; 6] = [
    Cmd::New("Nouveau"),
    Cmd::Separator,
    Cmd::Icon("Cut", "Couper"),
    Cmd::Icon("Copy", "Copier"),
    Cmd::Disabled("Paste", "Coller"),
    Cmd::Toggle("PanelRight", "Volet de détails", true),
];

/// Which command is drawn under the pointer, in both halves.
const HOT: usize = 3;

fn toolbar_of(bar: &[Cmd]) -> Toolbar {
    let mut t = Toolbar::new();
    for cmd in bar {
        t.items.push(cmd.item());
    }
    t
}

pub fn draw(c: &dyn Canvas, f: &Frame) {
    // The static exposition takes the window minus the interactive column, so it
    // lays out to the left of it rather than under it.
    let mut p = Page::new(c, f.size.0 - interact::PANEL_W(), f.size.1);
    toolbar_section(&mut p);
    rest_section(&mut p);
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. The command bar: beside its predecessor, then too narrow, then in a card.
// ─────────────────────────────────────────────────────────────────────────────

fn toolbar_section(p: &mut Page) {
    let c = p.c;
    let t = c.theme();
    let f = c.formats();
    p.section("Barre de commandes — ToolStrip, toolbar::arrange, menu « Plus » flottant");

    let pair_bar = toolbar_of(&PAIR).with_band(true);
    let h = pair_bar.row_height;
    let strip = ((p.area.right - 2.0 * MARGIN - 40.0) / 2.0).min(420.0);
    p.caption("à côté du prédécesseur (bande Drive activée) : « Nouveau ▾ », séparateur, commandes dont une désactivée, bascule active — survol sur « Copier »");
    let top = p.y;
    let used = sheet::pair(
        c,
        MARGIN,
        top,
        strip,
        h,
        |r| old_toolbar(c, &pair_bar, &PAIR, r),
        |r| pair_bar.paint_items(c, r, Some(HOT), false),
    );
    p.advance(used);

    p.caption("trop étroit : la queue part dans le menu « Plus » (ouvert, anneau clavier sur « Couper », « Copier » enfoncé) — puis la même barre dans une Card : aucune bande blanche");
    let top = p.y;
    let bar = toolbar_of(&BAR);
    let narrow = Rect::new(MARGIN, top, MARGIN + 330.0, top + h);
    let a = bar.item_rects(c, narrow);
    bar.paint_with(
        c,
        narrow,
        &StripPaint {
            pressed: Some(3),
            focused: Some(2),
            focus_visible: true,
            overflow_open: true,
            ..StripPaint::default()
        },
    );
    // The open menu, painted in place as the stand-in for the popup window
    // the live column opens (a capture of the main window cannot show one).
    let mut menu_h = 0.0;
    if let Some(btn) = a.overflow_button {
        let mut menu = bar.overflow_menu(&a);
        menu.hot_index = Some(0);
        let want = menu.measure(c);
        let panel = place_menu(btn, want, p.area);
        menu.paint(c, panel, WidgetState::REST);
        menu_h = panel.bottom - top;
    }

    // The same bar inside a card: the band is off by default, so the card body
    // shows through between the commands.
    let card_left = MARGIN + 330.0 + 180.0;
    let card_rect = Rect::new(card_left, top, p.area.right - MARGIN, top + h + 32.0);
    let card = Card::new();
    card.paint(c, card_rect, WidgetState::REST);
    let body = card.body_rect(card_rect);
    let in_card = toolbar_of(&PAIR);
    let r = Rect::new(body.left, body.top, body.right, body.top + h);
    in_card.paint_items(c, r, Some(2), false);
    c.text(
        "Toolbar::new() — transparent",
        &Rect::new(card_rect.left, card_rect.bottom + 4.0, card_rect.right, card_rect.bottom + 22.0),
        &f.caption,
        &t.text_tertiary,
        false,
    );
    p.advance(menu_h.max(card_rect.bottom + 22.0 - top));
}

/// The predecessor, painted from the rebuilt bar's own arrangement.
fn old_toolbar(c: &dyn Canvas, bar: &Toolbar, cmds: &[Cmd], area: Rect) {
    let a = bar.item_rects(c, area);
    let buttons = a
        .visible
        .iter()
        .map(|(i, rect)| web_toolbar::ToolbarButton { rect: *rect, visual: cmds[*i].visual(*i == HOT) })
        .collect();
    web_toolbar::draw(c, &web_toolbar::ToolbarView { card: Some(area), buttons });
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Pane on the left; trail, tabs and status bar on the right.
// ─────────────────────────────────────────────────────────────────────────────

/// One pane row description.
struct Row {
    icon:    &'static str,
    label:   &'static str,
    section: bool,
    indent:  f32,
    chevron: Option<bool>,
}

const PANE: [Row; 4] = [
    Row { icon: "Home", label: "Accueil", section: false, indent: 28.0, chevron: None },
    Row { icon: "Star", label: "Épinglés", section: true, indent: 28.0, chevron: Some(true) },
    Row { icon: "Folder", label: "Documents", section: false, indent: 48.0, chevron: Some(false) },
    Row { icon: "Cloud", label: "Kubuno Drive", section: false, indent: 48.0, chevron: None },
];

fn sidebar_of(mode: SidebarMode, active: usize) -> Sidebar {
    let mut s = Sidebar::new();
    s.mode = mode;
    for (i, r) in PANE.iter().enumerate() {
        let item = if r.section {
            section_item(r.icon, r.label, r.indent)
        } else {
            nav_item(r.icon, r.label, i == active, r.indent)
        };
        s = s.with(item, r.chevron);
    }
    s
}

const TRAIL: [&str; 5] = ["Ce PC", "Documents", "Projets", "Kubuno", "desktop"];

fn breadcrumb_of(segments: &[&str]) -> Breadcrumb {
    let mut b = Breadcrumb::new();
    b.root_chevron = true;
    for seg in segments {
        b = b.with(seg);
    }
    b
}

fn rest_section(p: &mut Page) {
    let c = p.c;
    let t = c.theme();
    let f = c.formats();
    p.section("Volet, fil d'Ariane, onglets, barre d'état");
    let top = p.y;
    let label = |r: Rect, s: &str| {
        c.text(s, &Rect::new(r.left, r.top - sheet::CAPTION_H, r.right.max(r.left + 400.0), r.top), &f.caption, &t.text_secondary, false)
    };

    // ── Left: the pane, then its rail ─────────────────────────────────────
    let pane = sidebar_of(SidebarMode::Expanded, 0);
    let ph = pane.measure(c).height;
    let pane_rect = Rect::new(MARGIN, top + sheet::CAPTION_H, MARGIN + 220.0, top + sheet::CAPTION_H + ph);
    label(pane_rect, "volet : actif, survol, anneau");
    pane.paint_with(c, pane_rect, &StripPaint { hot: Some(3), focused: Some(2), focus_visible: true, ..StripPaint::default() });

    let rail = sidebar_of(SidebarMode::Compact, 0);
    let rail_top = pane_rect.bottom + 16.0 + sheet::CAPTION_H;
    let rail_rect = Rect::new(MARGIN, rail_top, MARGIN + rail.pane_width(), rail_top + ph);
    label(rail_rect, "rail replié");
    rail.paint_rows(c, rail_rect, Some(2));

    // A pane shorter than its rows scrolls (and clips) instead of spilling.
    let mut short = sidebar_of(SidebarMode::Expanded, 0);
    let short_rect = Rect::new(rail_rect.right + 16.0, rail_top, MARGIN + 220.0, rail_top + 84.0);
    short.scroll_y = 20.0;
    c.text("défilé (scroll_y 20)", &Rect::new(short_rect.left, rail_top - sheet::CAPTION_H, short_rect.right + 60.0, rail_top), &f.caption, &t.text_secondary, false);
    c.stroke_rounded(&short_rect, 0.0, &t.divider);
    short.paint_rows(c, short_rect, None);
    let left_bottom = rail_rect.bottom.max(short_rect.bottom);

    // ── Right column ──────────────────────────────────────────────────────
    let x0 = MARGIN + 220.0 + 32.0;
    let x1 = p.area.right - MARGIN;
    let mut y = top + sheet::CAPTION_H;

    // Breadcrumb, folded, with hover and a keyboard ring.
    let trail = breadcrumb_of(&TRAIL);
    let bh = trail.measure(c).height;
    let r = Rect::new(x0, y, (x0 + 330.0).min(x1), y + bh);
    label(r, "fil trop long : la tête se replie derrière « … » ; survol = texte accent, anneau clavier");
    let l = trail.layout(c, r);
    trail.paint_with(
        c,
        r,
        &StripPaint {
            hot: Some(l.start_index),
            focused: Some(l.start_index + 1),
            focus_visible: true,
            ..StripPaint::default()
        },
    );
    y = r.bottom + 8.0 + sheet::CAPTION_H;

    // A segment longer than `maxSegmentWidth` truncates.
    let long = breadcrumb_of(&["Documents", "Contrats et avenants signés par les deux parties", "2026"]);
    let r = Rect::new(x0, y, x1, y + bh);
    label(r, "segment long : tronqué à 14rem avec ellipse ; la page courante n'est pas un lien");
    long.paint_segments(c, r, None, false);
    y = r.bottom + 8.0 + sheet::CAPTION_H;

    // Tabs that fit: the widest tab's width for all, `gap-1`, hover.
    let mut tabs = Tabs::new().with("Détails").with("Aperçu").with("Historique").with("Partage");
    tabs.selected_index = 1;
    let th = tabs.measure(c).height;
    let r = Rect::new(x0, y, x1, y + th);
    label(r, "onglets md (h-12) : largeur du plus large, survol, anneau clavier sur l'actif");
    tabs.paint_with(c, r, &TabsPaint { hot: Some(2), focused: Some(1), focus_visible: true, ..TabsPaint::default() });
    y = r.bottom + 8.0 + sheet::CAPTION_H;

    // Tabs that do not fit: arrows, and the selected (last) tab kept in view.
    let mut over = Tabs::new().with("Général").with("Sécurité").with("Partage").with("Versions précédentes");
    over.selected_index = 3;
    let r = Rect::new(x0, y, (x0 + 300.0).min(x1), y + th);
    label(r, "trop étroit : flèches de défilement, l'onglet actif reste visible");
    over.paint_tabs(c, r, None);
    y = r.bottom + 8.0 + sheet::CAPTION_H;

    // The small size, along the bottom edge.
    let mut bottom = Tabs::new().with("Détails").with("Aperçu").with("Historique").small();
    bottom.selected_index = 0;
    bottom.alignment = TabAlignment::Bottom;
    let r = Rect::new(x0, y, (x0 + 330.0).min(x1), y + bottom.measure(c).height);
    label(r, "sm, alignés en bas : l'indicateur suit la page");
    bottom.paint_tabs(c, r, None);
    y = r.bottom + 8.0 + sheet::CAPTION_H;

    // The status bar: fixed cells whole, the spring absorbs / gives.
    let bar = status_bar();
    let sh = bar.measure(c).height;
    let r = Rect::new(x0, y, x1, y + sh);
    label(r, "barre d'état : cellules fixes entières, « Spring » absorbe la place ; survol, anneau");
    bar.paint_with(c, r, &StripPaint { hot: Some(3), focused: Some(4), focus_visible: true, ..StripPaint::default() });
    let r2 = Rect::new(x0, r.bottom + 8.0, (x0 + 300.0).min(x1), r.bottom + 8.0 + sh);
    bar.paint_items(c, r2, None);
    c.text("↑ 300 DIP : seul le Spring rétrécit", &Rect::new(r2.right + 8.0, r2.top, x1, r2.bottom), &f.caption, &t.text_tertiary, false);
    y = r2.bottom;

    p.advance(y.max(left_bottom) - top);
}

fn status_bar() -> StatusBar {
    let mut bar = StatusBar::new()
        .with(status_item("19 éléments", false))
        .with(separator_item())
        .with(status_item("2 sélectionnés · 482 Mo", true))
        .with(icon_label_item("Git", "3 / 0"))
        .with(icon_label_item("Git.Branch", "main"));
    // The two git widgets pack against the trailing edge, like `StatusBar.xaml`
    // puts them in its last two columns.
    for i in [3, 4] {
        if let StripItem::Button(b) = &mut bar.items[i] {
            b.item.alignment = ToolStripItemAlignment::Right;
        }
    }
    bar
}

// ─────────────────────────────────────────────────────────────────────────────
// The interactive column — the five primitives, driven by mouse and keyboard
// ─────────────────────────────────────────────────────────────────────────────

/// A live pane row: `parent` is the row whose chevron folds this one.
struct LiveRow {
    icon:    &'static str,
    label:   &'static str,
    section: bool,
    indent:  f32,
    parent:  Option<usize>,
    folds:   bool,
}

const LIVE_PANE: [LiveRow; 9] = [
    LiveRow { icon: "Home", label: "Accueil", section: false, indent: 28.0, parent: None, folds: false },
    LiveRow { icon: "Clock", label: "Récents", section: false, indent: 28.0, parent: None, folds: false },
    LiveRow { icon: "Star", label: "Épinglés", section: true, indent: 28.0, parent: None, folds: true },
    LiveRow { icon: "Folder", label: "Documents", section: false, indent: 48.0, parent: Some(2), folds: true },
    LiveRow { icon: "Folder", label: "Contrats", section: false, indent: 64.0, parent: Some(3), folds: false },
    LiveRow { icon: "Folder", label: "Factures", section: false, indent: 64.0, parent: Some(3), folds: false },
    LiveRow { icon: "Image", label: "Photos", section: false, indent: 48.0, parent: Some(2), folds: false },
    LiveRow { icon: "Cloud", label: "Kubuno Drive", section: false, indent: 28.0, parent: None, folds: false },
    LiveRow { icon: "Trash2", label: "Corbeille", section: false, indent: 28.0, parent: None, folds: false },
];

const LIVE_TRAIL: [&str; 6] = ["Ce PC", "Documents", "Projets", "Kubuno", "desktop", "windows"];
const LIVE_TABS: [&str; 6] = ["Fichiers", "Partages", "Activité récente", "Versions", "Sécurité", "Détails avancés"];

/// Which surface an open menu belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuOwner {
    Toolbar,
    Trail,
}

/// An open overflow menu: last frame's geometry (a click is routed against it
/// before anything underneath sees it) and what each row stands for.
struct OpenMenu {
    owner:   MenuOwner,
    panel:   Option<Rect>,
    hot:     Option<usize>,
    /// Per row: the item / segment it activates (`None` = inert row).
    targets: Vec<Option<usize>>,
}


/// An overflow menu with the trigger it hangs from.
type AnchoredMenu = (Menu, Rect);

struct Ui {
    prev_down:   bool,
    // Toolbar
    panel_right: bool,
    shelf:       bool,
    tb_focus:    usize,
    tip:         TooltipTrigger,
    tip_item:    Option<usize>,
    // Sidebar
    side_sel:    usize,
    side_focus:  usize,
    side_open:   [bool; LIVE_PANE.len()],
    side_scroll: f32,
    // Breadcrumb
    crumb_depth: usize,
    // Tabs
    tab_sel:     usize,
    /// The live strip's state, kept by the component's own controller.
    tabs_ctl:    TabsController,
    // Status bar
    message:     String,
    menu:        Option<OpenMenu>,
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            prev_down:   false,
            panel_right: true,
            shelf:       false,
            tb_focus:    0,
            tip:         TooltipTrigger::new(),
            tip_item:    None,
            side_sel:    0,
            side_focus:  0,
            side_open:   [true; LIVE_PANE.len()],
            side_scroll: 0.0,
            crumb_depth: LIVE_TRAIL.len(),
            tab_sel:     0,
            tabs_ctl:    TabsController::new(),
            message:     "Aucune action".to_string(),
            menu:        None,
        }
    }
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

/// The live command bar — long enough to overflow the column.
fn live_toolbar(ui: &Ui) -> Toolbar {
    Toolbar::new()
        .with(menu_item("Plus", "Nouveau"))
        .with(separator_item())
        .with(with_tooltip(icon_item("Cut"), "Couper"))
        .with(with_tooltip(icon_item("Copy"), "Copier"))
        .with(with_tooltip(icon_item("Share2"), "Partager"))
        .with(icon_label_item("RestoreDeleted", "Restaurer"))
        .with(separator_item())
        .with(with_tooltip(toggle_item("PanelRight", ui.panel_right), "Volet de détails"))
        .with(with_tooltip(toggle_item("Shelf", ui.shelf), "Étagère"))
        .with(with_tooltip(icon_item("Trash2"), "Supprimer"))
}

fn activate_command(ui: &mut Ui, bar: &Toolbar, i: usize) {
    match i {
        7 => ui.panel_right = !ui.panel_right,
        8 => ui.shelf = !ui.shelf,
        _ => {}
    }
    let it = bar.items[i].item();
    let name = if it.text.is_empty() { &it.tool_tip_text } else { &it.text };
    ui.message = format!("Commande : {name}");
}

/// Whether live pane row `i` is shown (every folding ancestor open).
fn row_shown(ui: &Ui, i: usize) -> bool {
    let mut p = LIVE_PANE[i].parent;
    while let Some(k) = p {
        if !ui.side_open[k] {
            return false;
        }
        p = LIVE_PANE[k].parent;
    }
    true
}

fn live_sidebar(ui: &Ui) -> Sidebar {
    let mut s = Sidebar::new();
    for (i, r) in LIVE_PANE.iter().enumerate() {
        let mut item = if r.section {
            section_item(r.icon, r.label, r.indent)
        } else {
            nav_item(r.icon, r.label, i == ui.side_sel, r.indent)
        };
        if let StripItem::Button(b) = &mut item {
            b.item.visible = row_shown(ui, i);
        } else if let StripItem::Label(l) = &mut item {
            l.item.visible = row_shown(ui, i);
        }
        s = s.with(item, r.folds.then_some(ui.side_open[i]));
    }
    s.scroll_y = ui.side_scroll;
    s
}

fn live_breadcrumb(depth: usize) -> Breadcrumb {
    breadcrumb_of(&LIVE_TRAIL[..depth.clamp(1, LIVE_TRAIL.len())])
}

fn live_tabs(ui: &Ui) -> Tabs {
    let mut t = Tabs::new();
    for s in LIVE_TABS {
        t = t.with(s);
    }
    t.selected_index = ui.tab_sel as i32;
    t
}

/// Runs the chosen row of the open menu.
fn choose(ui: &mut Ui, owner: MenuOwner, target: usize) {
    match owner {
        MenuOwner::Toolbar => {
            let bar = live_toolbar(ui);
            activate_command(ui, &bar, target);
        }
        MenuOwner::Trail => {
            ui.crumb_depth = target + 1;
            ui.message = format!("Navigation : {}", LIVE_TRAIL[target]);
        }
    }
}

pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let mut live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;
        let now = host::now_ms();

        // ── An open menu takes the input first, as the web backdrop does ──
        if f.dismiss {
            ui.menu = None;
        }
        if let Some(m) = ui.menu.as_mut() {
            let (px, py) = f.mouse;
            let targets = m.targets.clone();
            let usable = |k: usize| targets.get(k).copied().flatten().is_some();
            for key in take_nav_keys(true) {
                m.hot = roving_step(targets.len(), m.hot, key, true, usable);
            }
            let mut chosen = None;
            let mut close = false;
            if take_activate() {
                chosen = m.hot.and_then(|k| targets.get(k).copied().flatten());
                close = true;
            }
            if live.take_escape() || host::take_key(vk::TAB, Modifiers::NONE) > 0 {
                close = true;
            }
            if let Some(panel) = m.panel {
                interact::with_focus(|r| r.keep_focus_in(panel));
                if live.clicked {
                    if panel.contains(px, py) {
                        if let Some(k) = m.hot {
                            chosen = targets.get(k).copied().flatten();
                            close = chosen.is_some();
                        }
                    } else {
                        close = true;
                    }
                    live.clicked = false;
                }
            }
            let owner = m.owner;
            if close {
                ui.menu = None;
                let id = if owner == MenuOwner::Toolbar { "toolbar" } else { "crumb-more" };
                interact::with_focus(|r| r.focus_visibly(id));
            }
            if let Some(t) = chosen {
                choose(&mut ui, owner, t);
            }
            if ui.menu.is_some() {
                // Nothing underneath lights up while the menu is open.
                live.mouse = (host::POINTER_AWAY, host::POINTER_AWAY);
            }
        }
        let (mx, my) = live.mouse;

        let (left, mut y, right) = interact::panel(c, interact::panel_rect(f.size));

        // ── Toolbar ─────────────────────────────────────────────────────────
        y = interact::caption(c, left, right, y, "Toolbar — Tab puis ← → Début Fin, Entrée ; « … » ouvre le menu");
        let bar = live_toolbar(&ui);
        let h = bar.row_height;
        let rect = Rect::new(left, y, right, y + h);
        let a = bar.item_rects(c, rect);
        let order = bar.focus_order(&a);
        ui.tb_focus = ui.tb_focus.min(order.len().saturating_sub(1));
        let fs = live.focus("toolbar", rect);
        let hot = bar.item_at(c, rect, mx, my);
        let hot_more = bar.overflow_at(c, rect, mx, my);
        let mut open_tb_menu = false;
        if live.clicked {
            if let Some(i) = hot {
                if bar.is_focusable(i) {
                    if let Some(k) = order.iter().position(|t| *t == ToolbarTarget::Item(i)) {
                        ui.tb_focus = k;
                    }
                    activate_command(&mut ui, &bar, i);
                }
            } else if hot_more {
                ui.tb_focus = order.len().saturating_sub(1);
                open_tb_menu = true;
            }
        }
        if fs.focused && ui.menu.is_none() {
            for key in take_nav_keys(false) {
                ui.tb_focus = roving_step(order.len(), Some(ui.tb_focus), key, true, |_| true).unwrap_or(0);
            }
            let on_more = order.get(ui.tb_focus) == Some(&ToolbarTarget::Overflow);
            let down = on_more && host::take_key(vk::DOWN, Modifiers::NONE) > 0;
            if take_activate() || down {
                match order.get(ui.tb_focus) {
                    Some(ToolbarTarget::Item(i)) => activate_command(&mut ui, &bar, *i),
                    Some(ToolbarTarget::Overflow) => open_tb_menu = true,
                    None => {}
                }
            }
        }
        let bar = live_toolbar(&ui);
        let a = bar.item_rects(c, rect);
        let focused_item = match order.get(ui.tb_focus) {
            Some(ToolbarTarget::Item(i)) => Some(*i),
            _ => None,
        };
        let tb_menu_open = ui.menu.as_ref().is_some_and(|m| m.owner == MenuOwner::Toolbar);
        bar.paint_with(
            c,
            rect,
            &StripPaint {
                hot,
                pressed: hot.filter(|_| live.down),
                focused: focused_item,
                focus_visible: fs.focused && fs.visible,
                hot_overflow: hot_more,
                overflow_open: tb_menu_open,
                overflow_focused: fs.focused && order.get(ui.tb_focus) == Some(&ToolbarTarget::Overflow),
            },
        );
        let tb_menu = a.overflow_button.map(|btn| (bar.overflow_menu(&a), btn));
        if open_tb_menu {
            if tb_menu_open {
                ui.menu = None;
            } else {
                let menu = bar.overflow_menu(&a);
                let targets: Vec<Option<usize>> = (0..menu.items().len()).map(|k| bar.overflow_item_for_row(&a, k)).collect();
                let hot = if live.clicked { None } else { first_target(&targets) };
                ui.menu = Some(OpenMenu { owner: MenuOwner::Toolbar, panel: None, hot, targets });
            }
        }
        // The icon commands' names, in a tooltip overlay that may leave the
        // window (`title` on the web).
        let tip_target = hot.filter(|&i| {
            let it = bar.items[i].item();
            it.text.is_empty() && !it.tool_tip_text.is_empty()
        });
        if tip_target != ui.tip_item {
            ui.tip = TooltipTrigger::new();
            ui.tip_item = tip_target;
        }
        let tick = ui.tip.update(tip_target.is_some(), (mx, my), live.down, now, Tooltip::DELAY_MS);
        if let Some(ms) = tick.repaint_in_ms {
            host::request_repaint_after(ms);
        }
        if let (Some(at), Some(i)) = (tick.show_at, tip_target) {
            let tip = Tooltip::new(bar.items[i].item().tool_tip_text.clone());
            let pl = tip.place_at_pointer(c, at, f.screen_area());
            let pad = 10.0;
            let pb = pl.rect.inflate(pad, pad);
            let local = Rect::new(pad, pad, pad + pl.rect.right - pl.rect.left, pad + pl.rect.bottom - pl.rect.top);
            host::overlay(pb, move |canvas| tip.paint(canvas, local, WidgetState::REST));
        }
        y += h + 14.0;

        // ── Sidebar ─────────────────────────────────────────────────────────
        y = interact::caption(c, left, right, y, "Sidebar — ↑ ↓ Début Fin, Entrée, → ← déplient ; molette");
        let pane = live_sidebar(&ui);
        let pane_rect = Rect::new(left, y, left + 250.0_f32.min(right - left), y + 150.0);
        let fs = live.focus("pane", pane_rect);
        let (_, dy) = live.wheel_over(pane_rect);
        if dy != 0.0 {
            ui.side_scroll = (ui.side_scroll + dy * 0.4).clamp(0.0, pane.max_scroll(pane_rect));
        }
        if !pane.is_focusable(ui.side_focus) {
            ui.side_focus = pane.step_focus(None, NavKey::First).unwrap_or(0);
        }
        let hot = pane.item_at(pane_rect, mx, my);
        if live.clicked {
            if let Some(i) = hot {
                if LIVE_PANE[i].folds && (mx < pane_rect.left + LIVE_PANE[i].indent) {
                    ui.side_open[i] = !ui.side_open[i];
                } else if !LIVE_PANE[i].section {
                    ui.side_sel = i;
                    ui.side_focus = i;
                } else if LIVE_PANE[i].folds {
                    ui.side_open[i] = !ui.side_open[i];
                }
            }
        }
        if fs.focused && ui.menu.is_none() {
            let before = ui.side_focus;
            for key in take_nav_keys(true) {
                let p = live_sidebar(&ui);
                ui.side_focus = p.step_focus(Some(ui.side_focus), key).unwrap_or(ui.side_focus);
            }
            let i = ui.side_focus;
            if LIVE_PANE[i].folds {
                if host::take_key(vk::RIGHT, Modifiers::NONE) > 0 {
                    ui.side_open[i] = true;
                }
                if host::take_key(vk::LEFT, Modifiers::NONE) > 0 {
                    ui.side_open[i] = false;
                }
            } else if host::take_key(vk::LEFT, Modifiers::NONE) > 0 {
                // Left on a child moves to its parent — the tree pattern.
                if let Some(p) = LIVE_PANE[i].parent.filter(|&p| !LIVE_PANE[p].section) {
                    ui.side_focus = p;
                }
            }
            if take_activate() {
                ui.side_sel = ui.side_focus;
                ui.message = format!("Volet : {}", LIVE_PANE[ui.side_focus].label);
            }
            if ui.side_focus != before {
                let p = live_sidebar(&ui);
                ui.side_scroll = p.reveal_row(pane_rect, ui.side_focus);
            }
        }
        let pane = live_sidebar(&ui);
        ui.side_scroll = ui.side_scroll.min(pane.max_scroll(pane_rect));
        let pane = live_sidebar(&ui);
        c.stroke_rounded(&pane_rect, 0.0, &c.theme().divider);
        pane.paint_with(
            c,
            pane_rect,
            &StripPaint {
                hot,
                pressed: hot.filter(|_| live.down),
                focused: Some(ui.side_focus),
                focus_visible: fs.focused && fs.visible,
                ..StripPaint::default()
            },
        );
        y += 150.0 + 14.0;

        // ── Breadcrumb ──────────────────────────────────────────────────────
        y = interact::caption(c, left, right, y, "Breadcrumb — Tab de lien en lien, Entrée ; « … » : menu");
        let trail = live_breadcrumb(ui.crumb_depth);
        let bh = trail.measure(c).height;
        let rect = Rect::new(left, y, right, y + bh);
        let l = trail.layout(c, rect);
        let mut focused_seg = None;
        let mut more_focused = false;
        let mut open_trail_menu = false;
        if let Some(ell) = l.ellipsis {
            let st = live.focus("crumb-more", ell);
            if st.focused {
                more_focused = st.visible;
                if ui.menu.is_none() && (take_activate() || host::take_key(vk::DOWN, Modifiers::NONE) > 0) {
                    open_trail_menu = true;
                }
            }
            if live.hit(ell) {
                open_trail_menu = true;
            }
        }
        for (k, seg) in l.items.iter().enumerate() {
            let i = l.start_index + k;
            if !trail.is_link(i) {
                continue;
            }
            let st = live.focus(("crumb", i), *seg);
            if st.focused {
                if st.visible {
                    focused_seg = Some(i);
                }
                if ui.menu.is_none() && host::take_key(vk::ENTER, Modifiers::NONE) > 0 {
                    ui.crumb_depth = i + 1;
                    ui.message = format!("Navigation : {}", LIVE_TRAIL[i]);
                }
            }
            if live.hit(*seg) {
                ui.crumb_depth = i + 1;
                ui.message = format!("Navigation : {}", LIVE_TRAIL[i]);
            }
        }
        if live.clicked && trail.root_chevron_rect(rect).is_some_and(|r| r.contains(mx, my)) {
            ui.crumb_depth = LIVE_TRAIL.len();
        }
        let trail = live_breadcrumb(ui.crumb_depth);
        let l = trail.layout(c, rect);
        let hot = trail.item_at(c, rect, mx, my);
        let trail_menu_open = ui.menu.as_ref().is_some_and(|m| m.owner == MenuOwner::Trail);
        trail.paint_with(
            c,
            rect,
            &StripPaint {
                hot,
                focused: focused_seg,
                focus_visible: focused_seg.is_some() || more_focused,
                hot_overflow: trail.ellipsis_at(c, rect, mx, my),
                overflow_open: trail_menu_open,
                overflow_focused: more_focused,
                ..StripPaint::default()
            },
        );
        let trail_menu = l.ellipsis.map(|e| (trail.hidden_menu(&l), e));
        if open_trail_menu {
            if trail_menu_open {
                ui.menu = None;
            } else {
                let targets: Vec<Option<usize>> = trail.hidden_segments(&l).map(Some).collect();
                let hot = if live.clicked { None } else { first_target(&targets) };
                ui.menu = Some(OpenMenu { owner: MenuOwner::Trail, panel: None, hot, targets });
            }
        }
        y += bh + 14.0;

        // ── Tabs ────────────────────────────────────────────────────────────
        // Driven by the component's own `TabsController`: the click, the
        // arrows and wheel, the ← → Home End keys, the reveal and the sliding
        // indicator are the component's behaviour, the same the gallery's own
        // navigation strip gets — this page only reads back the selection.
        y = interact::caption(c, left, right, y, "Tabs — ← → Début Fin (activation), flèches et molette + Maj");
        let mut tabs = live_tabs(&ui);
        let th = tabs.measure(c).height;
        let rect = Rect::new(left, y, right, y + th);
        let fs = live.focus("tabs", rect);
        // An open menu owns the keys, so the strip only reads them without one.
        let focus = ui.menu.is_none().then_some(fs);
        let run = ui.tabs_ctl.frame(c, &mut tabs, rect, f, focus);
        if run.changed {
            ui.tab_sel = usize::try_from(tabs.selected_index).unwrap_or(0);
            ui.message = format!("Onglet : {}", LIVE_TABS[ui.tab_sel]);
        }
        y += th + 14.0;

        // ── Status bar ──────────────────────────────────────────────────────
        y = interact::caption(c, left, right, y, "StatusBar — les widgets Git sont des boutons (Tab, Entrée)");
        let sbar = status_bar();
        let sh = sbar.measure(c).height;
        let rect = Rect::new(left, y, right, y + sh);
        let lay = sbar.item_rects(c, rect);
        let mut sfocus = None;
        for i in 0..sbar.items.len() {
            if !sbar.is_focusable(i) {
                continue;
            }
            let r = lay.rects[i];
            let st = live.focus(("status", i), r);
            if st.focused && st.visible {
                sfocus = Some(i);
            }
            if live.hit(r) || (st.focused && ui.menu.is_none() && take_activate()) {
                ui.message = format!("Barre d'état : {}", sbar.items[i].item().text);
            }
        }
        let hot = sbar.item_at(c, rect, mx, my).filter(|&i| sbar.is_focusable(i));
        sbar.paint_with(
            c,
            rect,
            &StripPaint { hot, pressed: hot.filter(|_| live.down), focused: sfocus, focus_visible: sfocus.is_some(), ..StripPaint::default() },
        );
        y += sh + 10.0;
        interact::caption(c, left, right, y, &ui.message);

        // ── The open menu, in its own popup window ─────────────────────────
        paint_open_menu(c, f, &mut ui, (tb_menu, trail_menu));
    });
}

/// Places the open menu under its trigger against the MONITOR (it may hang
/// past the window) and shows it in a popup window. `menus` are this frame's
/// tool bar and trail overflow menus with their triggers, when they exist.
fn paint_open_menu(c: &dyn Canvas, f: &Frame, ui: &mut Ui, menus: (Option<AnchoredMenu>, Option<AnchoredMenu>)) {
    let Some(owner) = ui.menu.as_ref().map(|m| m.owner) else { return };
    let pick = match owner {
        MenuOwner::Toolbar => menus.0,
        MenuOwner::Trail => menus.1,
    };
    // The trigger went away (the bar grew, the path got short): so does the menu.
    let Some((mut menu, anchor)) = pick else {
        ui.menu = None;
        return;
    };
    let want = menu.measure(c);
    let panel = place_menu(anchor, want, f.screen_area());
    let Some(m) = ui.menu.as_mut() else { return };
    if panel.contains(f.mouse.0, f.mouse.1) {
        m.hot = menu.item_at(panel, f.mouse.0, f.mouse.1);
    }
    m.panel = Some(panel);
    menu.hot_index = m.hot;
    show_menu_popup(menu, panel);
}

/// The first row of a menu that does something — where a menu opened from the
/// keyboard puts its highlight.
fn first_target(targets: &[Option<usize>]) -> Option<usize> {
    targets.iter().position(|t| t.is_some())
}