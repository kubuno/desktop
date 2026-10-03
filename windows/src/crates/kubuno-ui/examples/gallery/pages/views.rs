//! Gallery page — **views**: [`ListView`] in its two painted modes, and
//! [`TreeView`].
//!
//! The static exposition is built fresh every frame from plain values, so
//! everything it shows is a function of the model and of the pointer: the
//! hovered row, the hovered column header and the chevron under the cursor are
//! all resolved through the family's own hit tests (`row_at`, `header_at`,
//! `node_at`, `chevron_hit`). It also shows the states a still picture can
//! carry: a merged selection run, the keyboard cursor on an unselected row,
//! the `:focus-visible` ring, the overlay scroll bar of a view whose content
//! overflows, an inline F2 rename, and long names ellipsised in their cells.
//!
//! The interactive column is the real thing: mouse (click, Ctrl/Shift+click,
//! double-click, right-click context menu in a `host::popup`, wheel, scroll-bar
//! track and thumb drag, sortable headers) and keyboard (arrows, Home/End,
//! PageUp/PageDown, Left/Right to fold the tree, type-ahead, Enter, F2 rename,
//! Delete, Ctrl+A, Escape, the menu key / Shift+F10), with the focus handled by
//! the page's `FocusRing`.
//!
//! There is no `pair` cell on this page: neither view has a hand-written
//! predecessor in `drive-app-controls`. Their reference is the product itself
//! (`drive-app/src/views/layouts/*`, `drive-app-controls/src/sidebar/`, and the
//! web explorer in `core/frontend/src/drive/storage-explorer/`).

use std::cell::RefCell;

use kubuno_controls::host::{self, vk, Frame, Modifiers};
use kubuno_controls::toolstrip::StripItem;
use kubuno_controls::views::TreeView as TreeModel;
use kubuno_ui::focus::caret_visible;
use kubuno_ui::lists::{separator, Menu, MenuEntry, MenuKey, MenuOutcome};
use kubuno_ui::range::ScrollPart;
use kubuno_ui::views::{
    ColumnHeader, ListAction, ListView, ListViewItem, NodePath, SortOrder, TreeAction, TreeNode,
    TreeView, View,
};
use kubuno_ui::{Canvas, Rect, Widget, WidgetState};

use super::interact::{self, Live};
use super::sheet::{Page, MARGIN};

/// The icon names an item's `ImageIndex` selects, shared by both lists.
const ICONS: [&str; 3] = ["Folder", "File", "Image"];

/// The sample listing, as the product's own file area shows it — with one name
/// far too long for its column, so the ellipsis is on show.
fn rows() -> Vec<ListViewItem> {
    const DATA: [(&str, &str, &str, &str, i32); 9] = [
        ("Documents", "12 août 2026", "Dossier", "—", 0),
        ("Photos de vacances", "09 août 2026", "Dossier", "—", 0),
        ("Projets", "02 août 2026", "Dossier", "—", 0),
        ("budget-2026.xlsx", "17 août 2026", "Classeur", "48 Ko", 1),
        ("contrat.pdf", "15 août 2026", "Document PDF", "1,2 Mo", 1),
        ("notes.md", "14 août 2026", "Markdown", "3 Ko", 1),
        (
            "Compte rendu de la réunion de lancement du projet de refonte du site — version finale relue.docx",
            "13 août 2026",
            "Document Word",
            "96 Ko",
            1,
        ),
        ("logo-kubuno.png", "11 août 2026", "Image PNG", "212 Ko", 2),
        ("capture.png", "10 août 2026", "Image PNG", "804 Ko", 2),
    ];
    DATA.iter()
        .map(|(name, modified, kind, size, icon)| {
            let mut it = ListViewItem::new(*name).with_sub(*modified).with_sub(*kind).with_sub(*size);
            it.image_index = *icon;
            it
        })
        .collect()
}

/// The Details list: four columns, a sorted column, grid lines, a multiple
/// selection run (a plain click then a shift-click), and the keyboard cursor
/// moved on with Ctrl+Down so it sits on a row it did not select.
fn details(width: f32) -> ListView {
    let mut v = ListView::new();
    v.view = View::Details;
    v.full_row_select = true;
    v.grid_lines = true;
    v.multi_select = true;
    // The product's own column widths: the name takes what is left, then
    // 140 / 150 / 100 (`file_columns_for` in `drive-app/src/ui/hot.rs`).
    let name = (width - 390.0).max(160.0) as i32;
    v.columns = vec![
        ColumnHeader::new("Nom", name),
        ColumnHeader::new("Modifié", 140),
        ColumnHeader::new("Type", 150),
        ColumnHeader::new("Taille", 100),
    ];
    v.columns[3].text_align = kubuno_controls::enums::HorizontalAlignment::Right;
    v.items = rows();
    v.image_list = ICONS.to_vec();
    // Sorted on « Modifié », descending — the glyph goes on that column, not
    // on column 0 the way the replica's painter puts it.
    v.sort_column = 1;
    v.sorting = SortOrder::Descending;
    v.click(3, false, false);
    v.click(5, false, true);
    v.cursor = Some(6);
    v
}

/// The same listing as tiles.
fn tiles() -> ListView {
    let mut v = ListView::new();
    v.view = View::LargeIcon;
    v.check_boxes = true;
    v.items = rows();
    v.image_list = ICONS.to_vec();
    v.click(0, false, false);
    v.click(4, true, false);
    v
}

/// Gives every node the folder icon, so the pane reads like the product's.
fn folder_icons(nodes: &mut [TreeNode]) {
    for n in nodes.iter_mut() {
        n.image_index = 0;
        folder_icons(&mut n.children);
    }
}

/// A tree with open and closed branches, three levels of indent, check boxes,
/// a label too long for the pane and more rows than the pane holds.
fn tree() -> TreeView {
    let mut t = TreeView::new();
    t.check_boxes = true;
    let mut documents = TreeNode::new("Documents")
        .expanded()
        .child(
            TreeNode::new("Contrats")
                .expanded()
                .child(TreeNode::new("2025"))
                .child(TreeNode::new("2026")),
        )
        .child(TreeNode::new("Factures").child(TreeNode::new("Archivées")))
        .child(TreeNode::new("Notes"));
    documents.checked = true;
    let medias = TreeNode::new("Médias")
        .expanded()
        .child(TreeNode::new("Photos"))
        .child(TreeNode::new("Vidéos"))
        .child(TreeNode::new("Musique"));
    let mut partages = TreeNode::new("Partagés avec moi par l'équipe comptabilité et direction");
    partages.checked = true;
    t.nodes = vec![documents, medias, partages, TreeNode::new("Archives")];
    t.image_list = ICONS.to_vec();
    folder_icons(&mut t.nodes);
    t.selected_path = Some(vec![0, 0, 1]);
    t
}

pub fn draw(c: &dyn Canvas, f: &Frame) {
    // The static exposition takes the window minus the interactive column, so it
    // lays out to the left of it rather than under it.
    let mut page = Page::new(c, f.size.0 - interact::PANEL_W(), f.size.1);
    let (mx, my) = f.mouse;
    let right = page.area.right - MARGIN;

    // ── Details ──────────────────────────────────────────────────────────
    page.section("ListView — Details");
    page.caption(
        "tri · sélection fusionnée · curseur clavier (Ctrl+↓) · focus visible · défilée de 50 · renommage F2 · nom tronqué",
    );
    let top = page.y;
    // Six rows of body for nine items: the view overflows and wears its bar.
    let list_rect = Rect::new(MARGIN, top, right, top + 28.0 + 6.0 * 40.0);
    let mut list = details(list_rect.right - list_rect.left);
    list.scroll = 50.0;
    list.hot_index = list.row_at(list_rect, mx, my);
    list.hot_column = list.header_at(list_rect, mx, my);
    list.scrollbar_hot = list.scrollbar_rail(list_rect).is_some_and(|r| r.contains(mx, my));
    list.paint(c, list_rect, WidgetState::REST.focused(true).focus_visible(true));
    // An F2 rename under way on « Projets ».
    list.paint_rename(c, list_rect, 2, "Projets 2026", true);
    page.advance(list_rect.bottom - list_rect.top);

    // ── LargeIcon, and the tree beside it ────────────────────────────────
    page.section("ListView — LargeIcon · TreeView");
    page.caption(
        "tuiles, cases de sélection · arbre : branches, indentation, cases, curseur clavier (anneau), débordement",
    );
    let top = page.y;
    let bottom = (top + 2.0 * 176.0 + 24.0).min(page.area.bottom - MARGIN);

    // Three tiles across: the nine items overflow and the grid wears its bar,
    // and the tree beside it keeps a readable width.
    let tiles_rect = Rect::new(MARGIN, top, MARGIN + 3.0 * 132.0 + 12.0, bottom);
    let mut grid = tiles();
    grid.hot_index = grid.row_at(tiles_rect, mx, my);
    grid.scrollbar_hot = grid.scrollbar_rail(tiles_rect).is_some_and(|r| r.contains(mx, my));
    grid.paint(c, tiles_rect, WidgetState::REST.focused(true));

    let tree_rect = Rect::new(tiles_rect.right + 24.0, top, right, bottom);
    let mut nav = tree();
    // The cursor sits one row under the selection, with the ring of a
    // keyboard focus.
    nav.cursor = Some(vec![0, 1]);
    nav.hot_row = nav.node_at(tree_rect, mx, my);
    nav.scrollbar_hot = nav.scrollbar_rail(tree_rect).is_some_and(|r| r.contains(mx, my));
    nav.paint(c, tree_rect, WidgetState::REST.focused(true).focus_visible(true));

    // The chevron under the pointer, called out so the split between « expand
    // me » and « select me » is visible rather than merely tested.
    if let Some(i) = nav.chevron_hit(tree_rect, mx, my) {
        let t = c.theme();
        let row = nav.row_rect(tree_rect, i, nav.scroll);
        let depth = nav.rows().get(i).map(|r| r.depth).unwrap_or(0);
        c.stroke_rounded(&nav.chevron_rect(row, depth), 4.0, &t.accent);
    }

    page.advance(bottom - top);
}

// ─────────────────────────────────────────────────────────────────────────────
//  The interactive column — the same two views, live under the pointer and the
//  keyboard.
// ─────────────────────────────────────────────────────────────────────────────

/// Which view an action, a drag or a menu belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Which {
    List,
    Tree,
}

/// What a context menu or a rename acts on.
#[derive(Clone, PartialEq, Eq)]
enum Target {
    List(usize),
    Tree(NodePath),
}

/// An open context menu: the menu itself (its hot row persists between
/// frames), what it acts on, where it opens, and last frame's panel — the
/// geometry a click is routed against at the top of the next frame.
struct OpenMenu {
    menu: Menu,
    target: Target,
    at: (f32, f32),
    panel: Option<Rect>,
}

/// An inline rename under way.
struct Rename {
    target: Target,
    text: String,
    last_input: u64,
    /// Where the field was painted last frame (client DIP): a click outside
    /// it commits the rename.
    rect: Option<Rect>,
}

/// What the interactive column remembers between frames.
///
/// A [`ListView`] and a [`TreeView`] carry their own state (per-item
/// selection, per-node expand flags, cursor, scroll), so the whole widgets are
/// kept here and mutated in place. Both are built lazily on the first frame,
/// once the column width is known.
#[derive(Default)]
struct Ui {
    list: Option<ListView>,
    tree: Option<TreeView>,
    prev_down: bool,
    prev_right: bool,
    /// A scroll-bar thumb being dragged: which view, and the grab offset.
    drag: Option<(Which, f32)>,
    menu: Option<OpenMenu>,
    rename: Option<Rename>,
    status: String,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

const LIST_ROWS: f32 = 6.0;
const TREE_ROWS: f32 = 5.0;
/// `MenuDropdown` keeps its panel 8 DIP inside the viewport.
const MENU_EDGE: f32 = 8.0;

/// A multiple-selection Details list for the column: two columns that fit the
/// panel width, and more files than the six visible rows hold.
fn panel_list(width: f32) -> ListView {
    const FILES: [(&str, &str, i32); 18] = [
        ("Documents", "—", 0),
        ("Photos de vacances en Bretagne, été 2026", "—", 0),
        ("Projets", "—", 0),
        ("Archives", "—", 0),
        ("budget-2026.xlsx", "48 Ko", 1),
        ("contrat.pdf", "1,2 Mo", 1),
        ("notes.md", "3 Ko", 1),
        ("logo-kubuno.png", "212 Ko", 2),
        ("capture.png", "804 Ko", 2),
        ("Compte rendu de réunion — version finale relue et validée.docx", "96 Ko", 1),
        ("devis-client.pdf", "310 Ko", 1),
        ("facture-0042.pdf", "88 Ko", 1),
        ("planning.xlsx", "61 Ko", 1),
        ("présentation.pptx", "4,8 Mo", 1),
        ("README.txt", "1 Ko", 1),
        ("sauvegarde.zip", "1,1 Go", 1),
        ("vacances-01.jpg", "3,2 Mo", 2),
        ("vacances-02.jpg", "2,9 Mo", 2),
    ];
    let mut v = ListView::new();
    v.view = View::Details;
    v.full_row_select = true;
    v.multi_select = true;
    let name = (width - 90.0).max(120.0) as i32;
    v.columns = vec![ColumnHeader::new("Nom", name), ColumnHeader::new("Taille", 80)];
    v.columns[1].text_align = kubuno_controls::enums::HorizontalAlignment::Right;
    v.items = FILES
        .iter()
        .map(|(name, size, icon)| {
            let mut it = ListViewItem::new(*name).with_sub(*size);
            it.image_index = *icon;
            it
        })
        .collect();
    v.image_list = ICONS.to_vec();
    v.sorting = SortOrder::Ascending;
    v.click(0, false, false);
    v
}

/// A multiple-selection tree for the column, its top branch open, taller than
/// the five rows it is given.
fn panel_tree() -> TreeView {
    let mut t = TreeView::new();
    t.multi_select = true;
    let documents = TreeNode::new("Documents")
        .expanded()
        .child(
            TreeNode::new("Contrats et avenants signés")
                .child(TreeNode::new("2025"))
                .child(TreeNode::new("2026")),
        )
        .child(TreeNode::new("Factures"))
        .child(TreeNode::new("Notes"));
    let medias = TreeNode::new("Médias")
        .child(TreeNode::new("Photos"))
        .child(TreeNode::new("Vidéos"))
        .child(TreeNode::new("Musique"));
    let partages = TreeNode::new("Partagés avec moi par l'équipe comptabilité");
    t.nodes = vec![documents, medias, partages, TreeNode::new("Archives"), TreeNode::new("Corbeille")];
    t.image_list = ICONS.to_vec();
    folder_icons(&mut t.nodes);
    t.select_row(0);
    t
}

/// The context menu both views open — `MenuDropdown` rows built from the
/// replica's item model.
fn context_menu() -> Menu {
    Menu::with_items(vec![
        MenuEntry::new("Ouvrir").icon("FolderOpen").shortcut_text("Entrée").build(),
        MenuEntry::new("Renommer").icon("FileEdit").shortcut_text("F2").build(),
        separator(),
        MenuEntry::new("Supprimer").icon("Trash2").shortcut_text("Suppr").danger().build(),
    ])
}

/// The label of menu row `i`, if it is an enabled action.
fn menu_label(menu: &Menu, i: usize) -> Option<String> {
    match menu.items().get(i) {
        Some(StripItem::MenuItem(m)) if m.base.item.enabled => Some(m.base.item.text.clone()),
        _ => None,
    }
}

/// Removes the node at `path` from a forest.
fn remove_node(nodes: &mut Vec<TreeNode>, path: &[usize]) {
    match path {
        [] => {}
        [i] => {
            if *i < nodes.len() {
                nodes.remove(*i);
            }
        }
        [i, rest @ ..] => {
            if let Some(n) = nodes.get_mut(*i) {
                remove_node(&mut n.children, rest);
            }
        }
    }
}

/// A node's label.
fn tree_label(tree: &TreeView, path: &[usize]) -> String {
    TreeModel::node_at(&tree.nodes, path).map(|n| n.text.clone()).unwrap_or_default()
}

/// Runs a context-menu choice or its keyboard shortcut on `target`.
fn run(ui: &mut Ui, label: &str, target: Target) {
    match label {
        "Ouvrir" => open(ui, target),
        "Renommer" => start_rename(ui, target),
        "Supprimer" => delete(ui, target_view(&target), false),
        _ => {}
    }
}

fn target_view(t: &Target) -> Which {
    match t {
        Target::List(_) => Which::List,
        Target::Tree(_) => Which::Tree,
    }
}

fn open(ui: &mut Ui, target: Target) {
    ui.status = match &target {
        Target::List(i) => {
            let name = ui.list.as_ref().and_then(|l| l.items.get(*i)).map(|it| it.text.clone());
            format!("Ouvrir : {}", name.unwrap_or_default())
        }
        Target::Tree(p) => {
            let name = ui.tree.as_ref().map(|t| tree_label(t, p)).unwrap_or_default();
            format!("Ouvrir : {name}")
        }
    };
}

fn start_rename(ui: &mut Ui, target: Target) {
    let text = match &target {
        Target::List(i) => ui.list.as_ref().and_then(|l| l.items.get(*i)).map(|it| it.text.clone()),
        Target::Tree(p) => ui.tree.as_ref().map(|t| tree_label(t, p)),
    };
    if let Some(text) = text {
        ui.rename = Some(Rename { target, text, last_input: host::now_ms(), rect: None });
    }
}

/// Commits the rename under way (Enter, or a click elsewhere — Explorer's
/// rule). An empty name is refused: the old one stays.
fn commit_rename(ui: &mut Ui) {
    let Some(r) = ui.rename.take() else {
        return;
    };
    let name = r.text.trim().to_string();
    if name.is_empty() {
        return;
    }
    match r.target {
        Target::List(i) => {
            if let Some(it) = ui.list.as_mut().and_then(|l| l.items.get_mut(i)) {
                it.text = name.clone();
            }
        }
        Target::Tree(p) => {
            if let Some(n) = ui.tree.as_mut().and_then(|t| TreeModel::node_at_mut(&mut t.nodes, &p)) {
                n.text = name.clone();
            }
        }
    }
    ui.status = format!("Renommé en « {name} »");
}

/// Deletes the selection of one view — for real, so the list shrinks and the
/// scroll clamps.
fn delete(ui: &mut Ui, which: Which, permanent: bool) {
    let how = if permanent { "supprimé(s) définitivement" } else { "mis à la corbeille" };
    match which {
        Which::List => {
            let Some(list) = ui.list.as_mut() else { return };
            let before = list.items.len();
            list.items.retain(|it| !it.selected);
            let n = before - list.items.len();
            let last = list.items.len().checked_sub(1);
            list.cursor = list.cursor.and_then(|c| last.map(|l| c.min(l)));
            list.focused_index = list.cursor;
            ui.status = format!("{n} élément(s) {how}");
        }
        Which::Tree => {
            let Some(tree) = ui.tree.as_mut() else { return };
            let mut paths = tree.selected_paths();
            // Deepest and last first, so removing one never shifts another.
            paths.sort();
            paths.reverse();
            for p in &paths {
                remove_node(&mut tree.nodes, p);
            }
            tree.selection.clear();
            tree.selected_path = None;
            tree.cursor = None;
            ui.status = format!("{} dossier(s) {how}", paths.len());
        }
    }
}

/// Opens the context menu for `target` at `at` (client DIP).
fn open_menu(ui: &mut Ui, target: Target, at: (f32, f32), from_keyboard: bool) {
    let mut menu = context_menu();
    // Opened from the keyboard, the first action is hot so Enter works at
    // once (the ARIA menu pattern); from the pointer, nothing is.
    if from_keyboard {
        menu.hot_index = menu.next_actionable(None, true);
    }
    ui.menu = Some(OpenMenu { menu, target, at, panel: None });
}

/// The column: the same two views as the page, but live.
pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let ui = &mut *ui;
        let mut live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;
        let right_click = f.right_down && !ui.prev_right;
        ui.prev_right = f.right_down;
        if !f.mouse_down {
            ui.drag = None;
        }

        // A click on the desktop or another app closes the menu.
        if f.dismiss {
            ui.menu = None;
        }

        // ── An open menu takes the pointer and the keys first ────────────
        // As the web's backdrop does: a click on an action runs it, anywhere
        // off the panel closes the menu, and nothing underneath sees that
        // click nor lights up under the pointer.
        if let Some(mut m) = ui.menu.take() {
            let (px, py) = f.mouse;
            let mut keep = true;
            let mut chosen: Option<String> = None;
            let on_panel = m.panel.is_some_and(|p| p.contains(px, py));
            if let Some(panel) = m.panel {
                m.menu.hot_index = m.menu.item_at(panel, px, py).filter(|&i| m.menu.is_actionable(i));
            }
            if live.clicked || right_click {
                if on_panel {
                    if live.clicked {
                        let i = m.panel.and_then(|p| m.menu.item_at(p, px, py));
                        chosen = i.and_then(|i| menu_label(&m.menu, i));
                    }
                } else {
                    keep = false;
                }
                live.clicked = false;
            }
            for (code, key) in [
                (vk::DOWN, MenuKey::Down),
                (vk::UP, MenuKey::Up),
                (vk::HOME, MenuKey::Home),
                (vk::END, MenuKey::End),
                (vk::ENTER, MenuKey::Enter),
                (vk::SPACE, MenuKey::Space),
                (vk::ESCAPE, MenuKey::Escape),
            ] {
                for _ in 0..host::take_key(code, Modifiers::NONE) {
                    match m.menu.navigate(key) {
                        MenuOutcome::Chosen { index, .. } => chosen = menu_label(&m.menu, index),
                        MenuOutcome::Close => keep = false,
                        _ => {}
                    }
                }
            }
            if let Some(label) = chosen {
                let target = m.target.clone();
                run(ui, &label, target);
                keep = false;
            }
            if keep {
                ui.menu = Some(m);
                live.mouse = (f32::NEG_INFINITY, f32::NEG_INFINITY);
            } else {
                // Back to the view it came from, with its ring.
                let id = match target_view(&m.target) {
                    Which::List => "views-list",
                    Which::Tree => "views-tree",
                };
                interact::with_focus(|r| r.focus_visibly(id));
            }
        }

        // ── An inline rename takes the text keys next ─────────────────────
        // A click anywhere outside the field commits it, as in Explorer; the
        // click then goes on to do what it does.
        if live.clicked
            && ui.rename.as_ref().is_some_and(|r| !r.rect.is_some_and(|b| b.contains(f.mouse.0, f.mouse.1)))
        {
            commit_rename(ui);
        }
        if let Some(r) = ui.rename.as_mut() {
            let typed = live.take_text();
            if !typed.is_empty() {
                r.text.push_str(&typed);
                r.last_input = host::now_ms();
            }
            for _ in 0..host::take_key(vk::BACK, Modifiers::NONE) {
                r.text.pop();
                r.last_input = host::now_ms();
            }
            if live.take_key(vk::letter('V'), Modifiers::CTRL) {
                if let Some(t) = host::clipboard_text() {
                    r.text.push_str(t.lines().next().unwrap_or(""));
                    r.last_input = host::now_ms();
                }
            }
            if live.take_key(vk::ESCAPE, Modifiers::NONE) {
                ui.rename = None;
                ui.status = "Renommage annulé".into();
            } else if live.take_key(vk::ENTER, Modifiers::NONE) {
                commit_rename(ui);
            }
        }
        let (mx, my) = live.mouse;

        let (left, mut y, right) = interact::panel(c, interact::panel_rect(f.size));

        // ── ListView ─────────────────────────────────────────────────────
        y = interact::caption(
            c,
            left,
            right,
            y,
            "ListView — clic, Ctrl/Maj+clic, double-clic, clic droit",
        );
        y = interact::caption(c, left, right, y, "molette, barre de défilement, en-têtes triables");
        y = interact::caption(c, left, right, y, "↑↓ Début Fin PgPréc PgSuiv · Maj/Ctrl+flèches · Ctrl+A");
        y = interact::caption(c, left, right, y, "lettres (recherche) · Entrée · F2 · Suppr · Échap");
        let list = ui.list.get_or_insert_with(|| panel_list(right - left));
        let list_rect =
            Rect::new(left, y, right, y + list.header_height() + LIST_ROWS * list.row_height());
        let list_focus = live.focus("views-list", list_rect);
        let mut renaming_list = matches!(ui.rename, Some(Rename { target: Target::List(_), .. }));
        if renaming_list && !list_focus.focused && ui.menu.is_none() {
            // Tabbing away commits too.
            commit_rename(ui);
            renaming_list = false;
        }
        let list = ui.list.get_or_insert_with(|| panel_list(right - left));
        let mut list_actions: Vec<ListAction> = Vec::new();

        // Scroll bar first: it sits over the rows and takes its own presses.
        let over_rail = list.scrollbar_rail(list_rect).is_some_and(|r| r.contains(mx, my));
        if ui.drag.is_some_and(|(w, _)| w == Which::List) {
            if let Some((_, grab)) = ui.drag {
                list.scroll_to_thumb(list_rect, my - grab);
            }
        } else if live.clicked && over_rail {
            if list.scrollbar_press(list_rect, mx, my) == Some(ScrollPart::Thumb) {
                if let Some(t) = list.scrollbar_thumb(list_rect) {
                    ui.drag = Some((Which::List, my - t.top));
                }
            }
            live.clicked = false;
        }
        list.scrollbar_hot = over_rail || ui.drag.is_some_and(|(w, _)| w == Which::List);
        let (_, dy) = live.wheel_over(list_rect);
        if dy != 0.0 {
            list.scroll_by(list_rect, dy);
        }

        if live.clicked && list_rect.contains(mx, my) {
            if let Some(col) = list.header_at(list_rect, mx, my) {
                // `DetailsHeader.sortBy`: the sorted column flips direction,
                // another one sorts ascending.
                if list.sort_column == col {
                    list.sorting = if list.sorting == SortOrder::Ascending {
                        SortOrder::Descending
                    } else {
                        SortOrder::Ascending
                    };
                } else {
                    list.sort_column = col;
                    list.sorting = SortOrder::Ascending;
                }
                list.sort();
                list.cursor = list.items.iter().position(|i| i.selected);
                list.focused_index = list.cursor;
            } else if let Some(i) = list.row_at(list_rect, mx, my) {
                if live.click_count == 2 && live.mods.is_none() {
                    list_actions.push(ListAction::Open(i));
                } else {
                    list.click_mods(i, live.mods);
                }
            } else if !live.mods.ctrl && !live.mods.shift {
                // A click on the empty area under the rows clears the
                // selection (the web's marquee start).
                list.clear_selection();
            }
        }
        let list = ui.list.get_or_insert_with(|| panel_list(right - left));
        if right_click && list_rect.contains(f.mouse.0, f.mouse.1) && ui.menu.is_none() {
            if let Some(i) = list.row_at(list_rect, f.mouse.0, f.mouse.1) {
                list.context_click(i);
                open_menu(ui, Target::List(i), f.mouse, false);
                interact::with_focus(|r| r.focus("views-list"));
            }
        }
        let list = ui.list.get_or_insert_with(|| panel_list(right - left));
        if list_focus.focused && !renaming_list && ui.menu.is_none() {
            list_actions.extend(list.take_input(list_rect));
        }
        list.hot_index = if over_rail { None } else { list.row_at(list_rect, mx, my) };
        list.hot_column = list.header_at(list_rect, mx, my);
        let st = WidgetState::REST.focused(list_focus.focused).focus_visible(list_focus.visible);
        list.paint(c, list_rect, st);
        if let Some(Rename { target: Target::List(i), text, last_input, rect }) = &mut ui.rename {
            let caret = live.window_focused && caret_visible(*last_input);
            list.paint_rename(c, list_rect, *i, text, caret);
            *rect = Some(list.name_rect(list_rect, *i));
        }
        let anchor = list.context_anchor(list_rect);
        for a in list_actions {
            match a {
                ListAction::Open(i) => open(ui, Target::List(i)),
                ListAction::Rename(i) => start_rename(ui, Target::List(i)),
                ListAction::Delete { permanent } => delete(ui, Which::List, permanent),
                ListAction::ContextMenu(i) => {
                    if let Some(at) = anchor {
                        open_menu(ui, Target::List(i), at, true);
                    }
                }
                _ => {}
            }
        }
        y = list_rect.bottom + 16.0;

        // ── TreeView ─────────────────────────────────────────────────────
        y = interact::caption(
            c,
            left,
            right,
            y,
            "TreeView — chevron ou double-clic pour déplier",
        );
        y = interact::caption(c, left, right, y, "Ctrl/Maj+clic, clic droit, molette");
        y = interact::caption(c, left, right, y, "↑↓ · → ouvre / enfant · ← ferme / parent");
        y = interact::caption(c, left, right, y, "lettres (recherche) · Entrée · F2 · Suppr");
        let tree = ui.tree.get_or_insert_with(panel_tree);
        let tree_rect = Rect::new(left, y, right, y + TREE_ROWS * tree.row_height());
        let tree_focus = live.focus("views-tree", tree_rect);
        let mut renaming_tree = matches!(ui.rename, Some(Rename { target: Target::Tree(_), .. }));
        if renaming_tree && !tree_focus.focused && ui.menu.is_none() {
            commit_rename(ui);
            renaming_tree = false;
        }
        let tree = ui.tree.get_or_insert_with(panel_tree);
        let mut tree_actions: Vec<TreeAction> = Vec::new();

        let over_rail = tree.scrollbar_rail(tree_rect).is_some_and(|r| r.contains(mx, my));
        if ui.drag.is_some_and(|(w, _)| w == Which::Tree) {
            if let Some((_, grab)) = ui.drag {
                tree.scroll_to_thumb(tree_rect, my - grab);
            }
        } else if live.clicked && over_rail {
            if tree.scrollbar_press(tree_rect, mx, my) == Some(ScrollPart::Thumb) {
                if let Some(t) = tree.scrollbar_thumb(tree_rect) {
                    ui.drag = Some((Which::Tree, my - t.top));
                }
            }
            live.clicked = false;
        }
        tree.scrollbar_hot = over_rail || ui.drag.is_some_and(|(w, _)| w == Which::Tree);
        let (_, dy) = live.wheel_over(tree_rect);
        if dy != 0.0 {
            tree.scroll_by(tree_rect, dy);
        }

        if live.clicked && tree_rect.contains(mx, my) {
            // The chevron wins over the row: it is the smaller target sitting
            // on top of it, and it means "expand", not "select".
            if let Some(i) = tree.chevron_hit(tree_rect, mx, my) {
                if let Some(path) = tree.path_at(i) {
                    tree.toggle_expanded(&path);
                    tree.clamp_scroll(tree_rect);
                }
            } else if let Some(i) = tree.node_at(tree_rect, mx, my) {
                if live.click_count == 2 && live.mods.is_none() {
                    // A double-click folds a branch and activates a leaf,
                    // as Explorer's navigation pane does.
                    let row = tree.rows().get(i).map(|r| (r.path.clone(), r.has_children));
                    match row {
                        Some((path, true)) => {
                            tree.toggle_expanded(&path);
                            tree.clamp_scroll(tree_rect);
                        }
                        Some((path, false)) => tree_actions.push(TreeAction::Activate(path)),
                        None => {}
                    }
                } else {
                    tree.click_mods(i, live.mods);
                }
            }
        }
        let tree = ui.tree.get_or_insert_with(panel_tree);
        if right_click && tree_rect.contains(f.mouse.0, f.mouse.1) && ui.menu.is_none() {
            if let Some(i) = tree.node_at(tree_rect, f.mouse.0, f.mouse.1) {
                tree.context_click(i);
                if let Some(path) = tree.path_at(i) {
                    open_menu(ui, Target::Tree(path), f.mouse, false);
                    interact::with_focus(|r| r.focus("views-tree"));
                }
            }
        }
        let tree = ui.tree.get_or_insert_with(panel_tree);
        if tree_focus.focused && !renaming_tree && ui.menu.is_none() {
            tree_actions.extend(tree.take_input(tree_rect));
            tree.clamp_scroll(tree_rect);
        }
        tree.hot_row = if over_rail { None } else { tree.node_at(tree_rect, mx, my) };
        let st = WidgetState::REST.focused(tree_focus.focused).focus_visible(tree_focus.visible);
        tree.paint(c, tree_rect, st);
        if let Some(Rename { target: Target::Tree(p), text, last_input, rect }) = &mut ui.rename {
            if let Some(i) = tree.rows().iter().position(|r| &r.path == p) {
                let caret = live.window_focused && caret_visible(*last_input);
                tree.paint_rename(c, tree_rect, i, text, caret);
                *rect = Some(tree.name_rect(tree_rect, i));
            }
        }
        let anchor = tree.context_anchor(tree_rect);
        for a in tree_actions {
            match a {
                TreeAction::Activate(p) => open(ui, Target::Tree(p)),
                TreeAction::Rename(p) => start_rename(ui, Target::Tree(p)),
                TreeAction::Delete { permanent } => delete(ui, Which::Tree, permanent),
                TreeAction::ContextMenu(p) => {
                    if let Some(at) = anchor {
                        open_menu(ui, Target::Tree(p), at, true);
                    }
                }
                _ => {}
            }
        }
        y = tree_rect.bottom + 12.0;

        // ── What happened last ───────────────────────────────────────────
        let status = if ui.status.is_empty() { "Aucune action pour l'instant" } else { &ui.status };
        interact::caption(c, left, right, y, &format!("Dernière action : {status}"));

        // ── The context menu, in a popup of its own ──────────────────────
        // Placed against the SCREEN, it may hang past the window's edges, and
        // its hover and clicks come back to this page in client DIP.
        if let Some(m) = ui.menu.as_mut() {
            m.menu.viewport = Some(f.screen_area());
            let want = m.menu.measure(c);
            let area = f.screen_area();
            let (ax, ay) = m.at;
            // `MenuDropdown`'s rule: open at the point, pulled back inside the
            // viewport, and above the point when it would run off the bottom.
            let x = if ax + want.width > area.right - MENU_EDGE {
                (area.right - MENU_EDGE - want.width).max(area.left + MENU_EDGE)
            } else {
                ax
            };
            let top = if ay + want.height > area.bottom - MENU_EDGE {
                (ay - want.height).max(area.top + MENU_EDGE)
            } else {
                ay
            };
            let panel = Rect::new(x, top, x + want.width, top + want.height);
            m.panel = Some(panel);
            // A press on the panel must not blur the view the menu belongs to.
            interact::with_focus(|r| r.keep_focus_in(panel));
            let pb = m.menu.paint_bounds(c, panel);
            let local = Rect::new(
                panel.left - pb.left,
                panel.top - pb.top,
                panel.right - pb.left,
                panel.bottom - pb.top,
            );
            let menu = m.menu.clone();
            host::popup(pb, move |canvas| menu.paint(canvas, local, WidgetState::REST));
        }
    });
}
