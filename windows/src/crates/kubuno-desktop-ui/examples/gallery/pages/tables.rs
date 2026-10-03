//! Gallery page — **tables**: [`DataTable`] in its four bodies, then fully live.
//!
//! The page is built fresh every frame from plain values, so everything the
//! exposition shows is a function of the model and of the pointer: the hovered
//! row, the hovered column header, the hovered pagination button and the
//! hovered bulk action are all resolved through the family's own hit tests
//! ([`DataTable::row_at`], [`DataTable::chrome_at`]).
//!
//! There is no `pair` cell on this page: the data table has no hand-written
//! predecessor in `kubuno-drive-desktop-app-controls` (it is the one family the web has and
//! WinForms does not). Its reference is `core/frontend/src/ui/data-table/`.
//!
//! The three cells of the second row are deliberately **narrow**, i.e. under
//! the 700 DIP `cardsBelow` threshold: the last one therefore switches to the
//! card layout on its own, from [`kubuno_desktop_ui::tables::layout_mode`] and nothing
//! else. The first two force `Layout::Table` so that the empty state and the
//! skeleton are shown as a *table* would show them.
//!
//! The interactive column holds ONE live table wired exactly as a product
//! screen would wire it: focus stops registered with the page's focus ring,
//! keys routed through [`DataTable::on_key`], clicks through
//! [`DataTable::activate`], the wheel through [`DataTable::scroll_by`], column
//! resizing by drag, and every menu (row actions, bulk overflow, column
//! chooser, right-click copy) in a `host::popup` that may hang past the window.

use std::cell::RefCell;

use kubuno_desktop_ui::display::{Badge, BadgeVariant};
use kubuno_desktop_ui::focus::FocusId;
use kubuno_desktop_ui::lists::Menu;
use kubuno_desktop_ui::tables::{
    aligned, column, flags, grid, with_flag, BulkAction, Cell, Chrome, DataTable, FocusPart,
    Layout, ListViewItem, SortOrder, TableCommand, TableEvent, TableMenu,
};
use kubuno_desktop_ui::{Canvas, Rect, Widget, WidgetState};

use kubuno_desktop_controls::enums::HorizontalAlignment;
use kubuno_desktop_controls::host::{self, vk, Frame, InputEvent, Modifiers};

use super::interact::{self, Live};
use super::sheet::{Page, MARGIN};

/// The sample listing: an administration screen's user table.
const DATA: [(&str, &str, &str, &str); 23] = [
    ("Amélie Rousseau", "Administratrice", "17 août 2026", "4,2 Go"),
    ("Bastien Laurent", "Membre", "16 août 2026", "812 Mo"),
    ("Camille Fontaine-Delaunay de la Tour d'Auvergne", "Membre", "16 août 2026", "1,9 Go"),
    ("Damien Perrot", "Invité", "15 août 2026", "42 Mo"),
    ("Élise Marchand", "Membre", "15 août 2026", "6,4 Go"),
    ("Farid Benali", "Administrateur", "14 août 2026", "980 Mo"),
    ("Gaëlle Nguyen", "Membre", "14 août 2026", "2,7 Go"),
    ("Hugo Delacroix", "Membre", "13 août 2026", "158 Mo"),
    ("Inès Chevalier", "Invitée", "13 août 2026", "12 Mo"),
    ("Jonas Wagner", "Membre", "12 août 2026", "3,1 Go"),
    ("Katia Moreau", "Membre", "12 août 2026", "744 Mo"),
    ("Lucas Bertrand", "Membre", "11 août 2026", "1,1 Go"),
    ("Maëva Dupont", "Administratrice", "11 août 2026", "5,8 Go"),
    ("Nadir Haddad", "Membre", "10 août 2026", "233 Mo"),
    ("Olivia Renard", "Invitée", "10 août 2026", "8 Mo"),
    ("Paul Lemaire", "Membre", "09 août 2026", "1,4 Go"),
    ("Quentin Roy", "Membre", "09 août 2026", "620 Mo"),
    ("Rachida Amrani", "Membre", "08 août 2026", "2,2 Go"),
    ("Sacha Vidal", "Invité", "08 août 2026", "31 Mo"),
    ("Théo Girard", "Membre", "07 août 2026", "917 Mo"),
    ("Ulysse Blanc", "Membre", "07 août 2026", "1,7 Go"),
    ("Valentine Ollier", "Administratrice", "06 août 2026", "7,3 Go"),
    ("Wassim Kaddour", "Membre", "06 août 2026", "486 Mo"),
];

/// The model index of the « Rôle » column in every table of this page.
const ROLE_COLUMN: usize = 1;

fn rows() -> Vec<ListViewItem> {
    DATA.iter()
        .map(|(name, role, seen, quota)| {
            ListViewItem::new(*name).with_sub(*role).with_sub(*seen).with_sub(*quota)
        })
        .collect()
}

/// The role as a badge — what the web column's `cell` renderer draws.
fn role_badge(role: &str) -> Badge {
    let v = match role {
        "Administratrice" | "Administrateur" => BadgeVariant::Primary,
        "Invité" | "Invitée" => BadgeVariant::Warning,
        _ => BadgeVariant::Default,
    };
    Badge::new(role).variant(v).dot(true)
}

/// The cell painter: the « Rôle » column is a badge, every other column the
/// table's own text. The badge is vertically centred at the cell's start and
/// never wider than the cell (the table clips it at the column edge anyway).
fn paint_role_cell(c: &dyn Canvas, cell: &Cell<'_>) -> bool {
    if cell.column != ROLE_COLUMN || cell.text.is_empty() {
        return false;
    }
    let b = role_badge(cell.text);
    let s = b.measure(c);
    let cy = (cell.rect.top + cell.rect.bottom) / 2.0;
    let right = (cell.rect.left + s.width).min(cell.rect.right);
    b.paint(c, Rect::new(cell.rect.left, cy - s.height / 2.0, right, cy + s.height / 2.0), WidgetState::REST);
    true
}

fn columns(name: i32, role: i32, seen: i32, quota: i32) -> Vec<kubuno_desktop_ui::tables::ColumnHeader> {
    vec![
        with_flag(
            with_flag(with_flag(column("name", "Nom", name), flags::SORTABLE), flags::PRIMARY),
            flags::REQUIRED,
        ),
        with_flag(column("role", "Rôle", role), flags::SORTABLE),
        with_flag(column("seen", "Dernière connexion", seen), flags::SORTABLE),
        aligned(with_flag(column("quota", "Quota", quota), flags::SORTABLE), HorizontalAlignment::Right),
    ]
}

/// A table whose « Nom » column takes what the three metadata columns leave —
/// the columns are declared, as everywhere in this crate, because
/// `table-layout: auto`'s content-driven distribution has no counterpart here.
fn base(width: f32) -> DataTable {
    let mut t = DataTable::new();
    t.selectable = true;
    t.row_actions = true;
    let name = (width - 40.0 - 48.0 - 180.0 - 170.0 - 120.0).max(200.0) as i32;
    t.columns = columns(name, 180, 170, 120);
    t.items = rows();
    t.cell_painter = Some(Box::new(paint_role_cell));
    t
}

/// The full table: sorted on « Nom », a partial selection so the bulk bar is up,
/// four bulk actions so one of them folds into the overflow, page 2 of 5, and
/// the keyboard cursor shown on a row (the ring a Tab into the rows draws).
fn full(width: f32) -> DataTable {
    let mut t = base(width);
    t.page_size = 5;
    t.page_size_options = vec![5, 10, 25, 50];
    t.page = 1;
    t.sort_column = 0;
    t.sorting = SortOrder::Ascending;
    t.sort();
    t.bulk_actions = vec![
        BulkAction::new("export", "Exporter").icon("Upload"),
        BulkAction::new("role", "Changer de rôle").icon("Users"),
        BulkAction::new("delete", "Supprimer").icon("Trash2").danger(true),
        BulkAction::new("archive", "Archiver").icon("Archive"),
    ];
    t.toggle_row(5);
    t.toggle_row(7);
    t.cursor = Some(8);
    t.focus_part = Some(FocusPart::Rows);
    t.focus_visible = true;
    t
}

pub fn draw(c: &dyn Canvas, f: &Frame) {
    let mut page = Page::new(c, f.size.0 - interact::PANEL_W(), f.size.1);
    let (mx, my) = f.mouse;
    let right = page.area.right - MARGIN;

    // ── The full table ───────────────────────────────────────────────────
    page.section("DataTable — tableau complet");
    page.caption(
        "tri · sélection partielle · actions groupées · badges (cellPainter) · ellipse · \
         curseur clavier (anneau focus-visible) · pagination",
    );
    let top = page.y;
    let rect = Rect::new(
        MARGIN,
        top,
        right,
        top + grid::BAR_HEIGHT + grid::TOOLBAR_GAP + grid::ROW * 6.0 + grid::FOOTER_HEIGHT,
    );
    let mut table = full(rect.right - rect.left);
    table.hot_index = table.row_at(rect, mx, my);
    table.hot_chrome = table.chrome_at(c, rect, mx, my);
    table.paint(c, rect, WidgetState::REST);
    page.advance(rect.bottom - rect.top);

    // ── The three other bodies, side by side ─────────────────────────────
    page.section("DataTable — état vide · squelette · cartes");
    page.caption(
        "trois boîtes étroites : les deux premières forcent le tableau, la troisième bascule \
         seule en cartes (largeur < cardsBelow = 700)",
    );
    let top = page.y;
    let bottom = (page.area.bottom - MARGIN).max(top + 120.0);
    let gap = 24.0;
    let width = ((right - MARGIN) - 2.0 * gap) / 3.0;
    let cell = |n: usize| {
        let x = MARGIN + n as f32 * (width + gap);
        Rect::new(x, top, x + width, bottom)
    };

    let mut empty = base(width);
    empty.layout = Layout::Table;
    empty.items.clear();
    empty.filtered = true;
    empty.title = "Utilisateurs".into();
    empty.configurable_columns = true;
    empty.hot_chrome = empty.chrome_at(c, cell(0), mx, my);
    empty.paint(c, cell(0), WidgetState::REST);

    let mut loading = base(width);
    loading.layout = Layout::Table;
    loading.loading = true;
    loading.skeleton_rows = 6;
    loading.title = "Utilisateurs".into();
    loading.paint(c, cell(1), WidgetState::REST);

    // Cards: the primary column titles each one, the rest become label/value
    // pairs (the role still through the cell painter), the pager goes compact.
    let mut cards = base(width);
    cards.page_size = 2;
    cards.row_actions = false;
    // A narrow bulk bar: icon-only actions, the rest folded into « … », and
    // the « N en sélection » label kept whole.
    cards.bulk_actions = vec![
        BulkAction::new("export", "Exporter").icon("Upload"),
        BulkAction::new("delete", "Supprimer").icon("Trash2").danger(true),
    ];
    cards.toggle_row(1);
    cards.hot_index = cards.row_at(cell(2), mx, my);
    cards.hot_chrome = cards.chrome_at(c, cell(2), mx, my);
    cards.paint(c, cell(2), WidgetState::REST);

    page.advance(bottom - top);
}

// ─────────────────────────────────────────────────────────────────────────────
//  Interactive column — one live DataTable driven by the real pointer and keys.
// ─────────────────────────────────────────────────────────────────────────────

/// The live table's box: toolbar (48 at its tallest, the bulk bar) + gap 8 +
/// header 40 + a body of about five rows + footer 44 — shorter than its page
/// of 8, so the body scrolls under a sticky header.
const TABLE_H: f32 = 48.0 + 8.0 + 40.0 + 5.5 * 40.0 + 44.0;

/// An open menu: which, and the right-click point for the copy menu.
#[derive(Clone, Copy)]
struct OpenMenu {
    kind: TableMenu,
    pointer: Option<(f32, f32)>,
}

#[derive(Default)]
struct Ui {
    table: Option<DataTable>,
    prev_down: bool,
    prev_right: bool,
    menu: Option<OpenMenu>,
    /// Last frame's menu panel, for routing this frame's click against it.
    menu_panel: Option<Rect>,
    /// The entry lit from the keyboard (the pointer overrides it).
    menu_hot: Option<usize>,
    /// What the last action did, shown under the table.
    status: String,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

fn live_table() -> DataTable {
    let mut t = DataTable::new();
    t.selectable = true;
    t.row_actions = true;
    t.title = "Utilisateurs".into();
    t.configurable_columns = true;
    // Keep it a table however narrow the column: the brief is the table's
    // own interaction (header, grips, pager), which cards have none of.
    t.layout = Layout::Table;
    // Declared widths wider than the column: the content scrolls sideways
    // inside the box (Maj + molette, or the bar), as the web's scroller does.
    t.columns = columns(200, 150, 160, 90);
    t.items = rows();
    t.page_size = 8;
    t.page_size_options = vec![8, 16, 24];
    t.bulk_actions = vec![
        BulkAction::new("export", "Exporter").icon("Upload"),
        BulkAction::new("delete", "Supprimer").icon("Trash2").danger(true),
        BulkAction::new("archive", "Archiver").icon("Archive"),
        BulkAction::new("role", "Changer de rôle").icon("Users"),
    ];
    t.row_menu_actions = vec![
        BulkAction::new("edit", "Modifier le profil").icon("PenLine"),
        BulkAction::new("role", "Changer de rôle").icon("Users"),
        BulkAction::new("reset", "Réinitialiser le mot de passe").icon("KeyRound"),
        BulkAction::new("delete", "Supprimer définitivement").icon("Trash2").danger(true),
    ];
    t.cell_painter = Some(Box::new(paint_role_cell));
    t
}

/// A stable focus id per focusable part — the stop list changes as the bulk
/// bar comes and goes, so ids are named, not positional.
fn focus_id(part: FocusPart) -> FocusId {
    match part {
        FocusPart::Rows => FocusId::of("dt-rows"),
        FocusPart::Chrome(ch) => match ch {
            Chrome::Columns => FocusId::of("dt-columns"),
            Chrome::ClearSelection => FocusId::of("dt-clear"),
            Chrome::BulkAction(i) => FocusId::indexed("dt-bulk", i),
            Chrome::BulkOverflow => FocusId::of("dt-more"),
            Chrome::SelectAll => FocusId::of("dt-all"),
            Chrome::Header(i) => FocusId::indexed("dt-head", i),
            Chrome::ResizeHandle(i) => FocusId::indexed("dt-grip", i),
            Chrome::PageSize(n) => FocusId::indexed("dt-size", n),
            Chrome::FirstPage => FocusId::of("dt-first"),
            Chrome::PrevPage => FocusId::of("dt-prev"),
            Chrome::NextPage => FocusId::of("dt-next"),
            Chrome::LastPage => FocusId::of("dt-last"),
            Chrome::RowCheck(i) => FocusId::indexed("dt-check", i),
            Chrome::RowMenu(i) => FocusId::indexed("dt-rowmenu", i),
            Chrome::ScrollBar { horizontal } => FocusId::indexed("dt-bar", horizontal as usize),
        },
    }
}

/// The entries of `menu` a keyboard can land on (actions, enabled).
fn actionable(table: &DataTable, kind: TableMenu, mode: kubuno_desktop_ui::tables::Mode, len: usize) -> Vec<usize> {
    (0..len).filter(|i| table.menu_command(kind, mode, *i).is_some()).collect()
}

/// Runs a menu command; returns whether the menu stays open (the column
/// chooser does, like the web's check boxes inside `MenuDropdown`).
fn run_command(ui: &mut Ui, cmd: TableCommand) -> bool {
    let Some(table) = ui.table.as_mut() else { return false };
    match cmd {
        TableCommand::Copy(text) => {
            let ok = host::set_clipboard_text(&text);
            let shown: String = text.chars().take(40).collect();
            ui.status = if ok { format!("Copié : {shown}") } else { "Presse-papiers indisponible".into() };
            false
        }
        TableCommand::RowAction { row, id } => {
            let name = table.items.get(row).map(|it| it.text.clone()).unwrap_or_default();
            ui.status = format!("Action « {id} » sur {name}");
            if id == "delete" && row < table.items.len() {
                table.items.remove(row);
                table.cursor = None;
            }
            false
        }
        TableCommand::BulkAction(id) => {
            bulk(ui, &id);
            false
        }
        cmd @ TableCommand::ToggleColumn(_) => {
            table.apply_command(&cmd);
            true
        }
    }
}

/// A bulk action on the selection: « Supprimer » really removes the rows, the
/// others report what they would do.
fn bulk(ui: &mut Ui, id: &str) {
    let Some(table) = ui.table.as_mut() else { return };
    let sel = table.selected();
    if id == "delete" {
        for i in sel.iter().rev() {
            if *i < table.items.len() {
                table.items.remove(*i);
            }
        }
        table.cursor = None;
        ui.status = format!("{} ligne(s) supprimée(s)", sel.len());
    } else {
        ui.status = format!("Action groupée « {id} » sur {} ligne(s)", sel.len());
    }
}

fn handle_event(ui: &mut Ui, ev: TableEvent, pointer: Option<(f32, f32)>) {
    match ev {
        TableEvent::OpenMenu(kind) => {
            ui.menu = Some(OpenMenu { kind, pointer });
            ui.menu_hot = None;
        }
        TableEvent::BulkAction(id) => bulk(ui, &id),
        TableEvent::RowActivated(row) => {
            let name = ui.table.as_ref().and_then(|t| t.items.get(row)).map(|it| it.text.clone()).unwrap_or_default();
            ui.status = format!("Ouvrir : {name}");
        }
    }
}

pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|cell| {
        let mut guard = cell.borrow_mut();
        let ui: &mut Ui = &mut guard;
        let mut live = Live::new(f, ui.prev_down);
        let released = !f.mouse_down && ui.prev_down;
        ui.prev_down = f.mouse_down;
        let right_click = f.right_down && !ui.prev_right;
        ui.prev_right = f.right_down;

        let (left, mut y, right) = interact::panel(c, interact::panel_rect(f.size));
        // One short line each: a caption never wraps, so it must fit the column.
        for line in [
            "Souris : tri · cases (Maj = plage) · poignées de colonne",
            "molette (Maj = horizontal) · clic droit = copier · ⋮ = menu",
            "Clavier : Tab · ↑↓ PgPr PgSv Début Fin · Espace · Maj+flèches",
            "Ctrl+A · Entrée · touche Menu / Maj+F10 · ←→ sur une poignée",
        ] {
            y = interact::caption(c, left, right, y, line);
        }
        let rect = Rect::new(left, y, right, y + TABLE_H);
        if ui.table.is_none() {
            ui.table = Some(live_table());
        }
        let area = f.screen_area();

        // ── An open menu takes the pointer and the keys first ─────────────
        if f.dismiss {
            ui.menu = None;
        }
        if let Some(open) = ui.menu {
            let (px, py) = f.mouse;
            let on_panel = ui.menu_panel.is_some_and(|p| p.contains(px, py));
            if live.clicked || right_click {
                let mut keep = false;
                if on_panel && live.clicked {
                    if let (Some(panel), Some(table)) = (ui.menu_panel, ui.table.as_ref()) {
                        let mode = table.layout_of(rect).mode;
                        let menu = table.menu(open.kind, mode);
                        let cmd = menu.item_at(panel, px, py).and_then(|i| table.menu_command(open.kind, mode, i));
                        match cmd {
                            Some(cmd) => keep = run_command(ui, cmd),
                            // The padding or a dead row: the menu stays.
                            None => keep = true,
                        }
                    }
                }
                if !keep {
                    ui.menu = None;
                }
                // The click never reaches what is under the menu — except a
                // right click, which re-opens the copy menu where it lands.
                live.clicked = false;
            }
            if let Some(open) = ui.menu {
                // Keys: the menu reads them from the same queue.
                if let Some(table) = ui.table.as_ref() {
                    let mode = table.layout_of(rect).mode;
                    let len = table.menu(open.kind, mode).items().len();
                    let stops = actionable(table, open.kind, mode, len);
                    let pos = ui.menu_hot.and_then(|h| stops.iter().position(|s| *s == h));
                    if live.take_key(vk::DOWN, Modifiers::NONE) && !stops.is_empty() {
                        ui.menu_hot = Some(stops[pos.map(|p| (p + 1) % stops.len()).unwrap_or(0)]);
                    }
                    if live.take_key(vk::UP, Modifiers::NONE) && !stops.is_empty() {
                        let n = stops.len();
                        ui.menu_hot = Some(stops[pos.map(|p| (p + n - 1) % n).unwrap_or(n - 1)]);
                    }
                    let enter = live.take_key(vk::ENTER, Modifiers::NONE) || live.take_key(vk::SPACE, Modifiers::NONE);
                    if enter {
                        if let Some(cmd) = ui.menu_hot.and_then(|h| table.menu_command(open.kind, mode, h)) {
                            if !run_command(ui, cmd) {
                                ui.menu = None;
                            }
                        }
                    }
                }
                if ui.menu.is_some() && live.take_escape() {
                    ui.menu = None;
                }
                if ui.menu.is_none() {
                    // Back to the trigger, visibly, as a closing menu does.
                    let back = match open.kind {
                        TableMenu::Columns => FocusPart::Chrome(Chrome::Columns),
                        TableMenu::BulkOverflow => FocusPart::Chrome(Chrome::BulkOverflow),
                        _ => FocusPart::Rows,
                    };
                    interact::with_focus(|r| r.focus_visibly(focus_id(back)));
                }
            }
            // Nothing under an open menu lights up.
            if ui.menu.is_some() {
                live.mouse = (f32::NEG_INFINITY, f32::NEG_INFINITY);
            }
        }
        let (mx, my) = live.mouse;

        // ── Focus: register the table's stops in Tab order ───────────────
        let mut events: Vec<(TableEvent, Option<(f32, f32)>)> = Vec::new();
        {
            let Some(table) = ui.table.as_mut() else { return };
            table.clamp_scroll(rect);
            table.focus_part = None;
            table.focus_visible = false;
            for (part, r) in table.focus_stops(c, rect) {
                let st = live.focus(focus_id(part), r);
                if st.focused {
                    table.focus_part = Some(part);
                    table.focus_visible = st.visible;
                    if part == FocusPart::Rows && st.gained && table.cursor.is_none() {
                        table.cursor = Some(table.page_range().start);
                    }
                }
            }
            if let (Some(_), Some(p)) = (ui.menu, ui.menu_panel) {
                interact::with_focus(|r| r.keep_focus_in(p));
            }

            // ── Keys, while a part of the table holds the focus ───────────
            if ui.menu.is_none() && table.focus_part.is_some() {
                for e in live.events() {
                    if let InputEvent::Key { vk: key, down: true, mods, .. } = e {
                        let (used, ev) = table.on_key(rect, key, mods);
                        if used {
                            host::take_key(key, mods);
                        }
                        if let Some(ev) = ev {
                            events.push((ev, None));
                        }
                    }
                }
            }

            // ── Pointer ──────────────────────────────────────────────────
            if released {
                table.end_resize();
                table.end_scroll_drag();
            }
            if f.mouse_down && !live.clicked {
                if table.resize.is_some() {
                    table.drag_resize(f.mouse.0);
                }
                if table.scroll_drag.is_some() {
                    table.drag_scroll(rect, f.mouse.0, f.mouse.1);
                }
            }
            if live.clicked {
                match table.chrome_at(c, rect, mx, my) {
                    Some(Chrome::ResizeHandle(i)) => {
                        if live.click_count >= 2 {
                            table.reset_column_width(i);
                        } else {
                            table.begin_resize(i, mx);
                        }
                    }
                    Some(Chrome::ScrollBar { horizontal }) => table.begin_scroll_drag(rect, horizontal, mx, my),
                    Some(chrome) => {
                        let at = table.chrome_rect(c, rect, chrome).map(|r| (r.left, r.bottom));
                        if let Some(ev) = table.activate(chrome, live.mods.shift) {
                            events.push((ev, at));
                        }
                    }
                    None => {
                        if let Some(row) = table.row_at(rect, mx, my) {
                            table.cursor = Some(row);
                            if live.click_count == 2 {
                                events.push((TableEvent::RowActivated(row), None));
                            }
                        }
                    }
                }
            }
            if right_click && rect.contains(f.mouse.0, f.mouse.1) {
                let (px, py) = f.mouse;
                if let (Some(row), Some(col)) = (table.row_at(rect, px, py), table.column_at(table.layout_of(rect).frame, px)) {
                    table.cursor = Some(row);
                    events.push((TableEvent::OpenMenu(TableMenu::Copy { row, column: col }), Some((px, py))));
                }
            }

            // ── Wheel: vertical, or sideways with Shift or a tilt wheel ────
            let (wx, wy) = live.wheel_over(rect);
            if wx != 0.0 || wy != 0.0 {
                let (dx, dy) = if live.mods.shift { (wy, 0.0) } else { (wx, wy) };
                table.scroll_by(rect, dx, dy);
            }

            if let Some(cur) = table.cursor_at(rect, f.mouse.0, f.mouse.1) {
                host::set_cursor(cur);
            }
        }
        for (ev, at) in events {
            // A copy menu keeps the pointer point; the others anchor on their
            // trigger, which the table computes itself.
            let pointer = match ev {
                TableEvent::OpenMenu(TableMenu::Copy { .. }) => at,
                _ => None,
            };
            handle_event(ui, ev, pointer);
        }

        // ── Paint ────────────────────────────────────────────────────────
        let status = ui.status.clone();
        let menu_open = ui.menu;
        let menu_hot = ui.menu_hot;
        let Some(table) = ui.table.as_mut() else { return };
        table.clamp_scroll(rect);
        table.hot_index = if table.resize.is_some() { None } else { table.row_at(rect, mx, my) };
        table.hot_chrome = match table.resize {
            Some(d) => Some(Chrome::ResizeHandle(d.column)),
            None => table.chrome_at(c, rect, mx, my),
        };
        table.paint(c, rect, WidgetState::REST);

        let note_y = rect.bottom + 12.0;
        let line = if status.is_empty() {
            format!(
                "{} ligne(s) sélectionnée(s) · défilement {:.0} / {:.0}",
                table.selected().len(),
                table.list().scroll,
                table.max_scroll_y(rect)
            )
        } else {
            status
        };
        interact::caption(c, left, right, note_y, &line);

        // ── The open menu, in a popup that may leave the window ─────────
        let mut panel_out = None;
        if let Some(open) = menu_open {
            let mode = table.layout_of(rect).mode;
            let mut menu: Menu = table.menu(open.kind, mode);
            let panel = table.menu_rect(c, rect, open.kind, open.pointer, area);
            let (px, py) = f.mouse;
            menu.hot_index = menu.item_at(panel, px, py).or(menu_hot);
            let s = grid::MENU_SHADOW;
            let pb = Rect::new(panel.left - s, panel.top - s, panel.right + s, panel.bottom + s);
            let local = Rect::new(s, s, s + (panel.right - panel.left), s + (panel.bottom - panel.top));
            host::popup(pb, move |canvas| menu.paint(canvas, local, WidgetState::REST));
            panel_out = Some(panel);
        }
        ui.menu_panel = panel_out;
        // The pointer over a live popup keeps it repainting under hover.
        if ui.menu.is_some() {
            host::request_repaint_after(16);
        }
    });
}
