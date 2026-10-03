//! `AppTileGrid` — the body of the app launcher (the web's `WaffleMenu`): the favourites card (its
//! ground, its tiles, the drop zone while editing) and every other app below it, as tiles of a
//! 48-DIP logo over a one-line label, three to a row.
//!
//! What no primitive does is the editing: a click on a favourite removes it, a click on another
//! app adds it, and a tile carried with the mouse lands in front of the favourite it is dropped on
//! (a thin bar in that favourite's left gutter shows where), or leaves the favourites when dropped
//! on the other apps. The draft is held here ([`crate::Draft`]: the ids this build does not know
//! are carried through untouched); each change raises `FavoritesEdited` with the list it would
//! save.
//!
//! Its data are properties, so a user control drives it with bindings: `Apps` (the tiles),
//! `Favorites` (the saved list) and `Editing` (the edit mode: `true` starts an edit from
//! `Favorites`, `false` abandons it). Code holding the instance may call [`AppTileGrid::set_apps`],
//! [`AppTileGrid::begin_edit`], [`AppTileGrid::cancel_edit`] and [`AppTileGrid::finish_edit`] instead.
//!
//! The card's header band (its title and pencil, or the edit buttons) is left free for the view's
//! own controls: [`header_height`] says how tall it is. Every metric was measured on the web panel
//! (the shell's previous launcher pinned them in its tests, kept below).

use kubuno::controls::host::access::AccessRole;
use kubuno::ui::graphics::Image;
use kubuno::ui::metrics::{pill, space};
use kubuno::ui::{Canvas, Rect, Size};
use kubuno::views::component::{AccessiblePart, Component as _, Control, ControlCore, EventCx, PaintEventCx, Shared};
use kubuno::views::events::{EmptyEventArgs, Event, MouseButton, MouseEventArgs};

use crate::model::favorites::Draft;
use crate::ShellControlsResources;

/// Three tiles to a row.
const COLS: usize = 3;
/// `py-4` + a 48 icon + `gap-2` + one 12/15 line: measured at 103 in the browser.
const TILE_H: f32 = 103.0;
/// `gap-1` between tiles, both ways.
const TILE_GAP: f32 = 4.0;
/// `p-4` inside the favourites grid.
const GRID_PAD: f32 = 16.0;
/// The card's side margin in the grid: the web's 10 as seen from the panel's edge (`--kb-waffle-side`),
/// less the grid's own 8 (its X is the panel's 0.6 border and the scroll bar's 8 gutter; the panel's interior starts at 0.6).
const CARD_MARGIN: f32 = 2.0;
/// The other apps' inset (`--kb-waffle-inset`: the card's margin plus its grid's 16): their columns
/// land on the favourites'.
const SIDE_PAD: f32 = CARD_MARGIN + GRID_PAD;
/// The card's top: the panel's 0.6 border and `mt-[10px]`.
const CARD_TOP: f32 = 10.6;
/// `mb-[15px]` under the card.
const CARD_BOTTOM_GAP: f32 = 15.0;
/// A module's group of other apps: `mt-1`, its label (`pt-2 pb-1`, an 11 uppercase line of 16).
const GROUP_TOP: f32 = 4.0;
const GROUP_LABEL_PAD_TOP: f32 = 8.0;
const GROUP_LABEL_LINE: f32 = 16.0;
const GROUP_LABEL_PAD_BOTTOM: f32 = 4.0;
const GROUP_HEAD: f32 = GROUP_TOP + GROUP_LABEL_PAD_TOP + GROUP_LABEL_LINE + GROUP_LABEL_PAD_BOTTOM;
const CARD_RADIUS: f32 = 20.0;
const TILE_RADIUS: f32 = 16.0;
const ICON: f32 = 48.0;
/// `text-xs leading-tight`: 12 over a 15 line, measured.
const LABEL_LINE: f32 = 15.0;
/// The card's header band: `px-5 pt-4 pb-1` around a 40 button; the help line adds 28 while
/// editing.
const HEADER: f32 = 60.0;
const EDIT_HELP: f32 = 28.0;
/// The dashed zone shown while editing with no favourite left.
const DROP_ZONE: f32 = 80.0;
const DROP_ZONE_ROOM: f32 = 96.0;
/// How far the pointer travels before a press is a drag rather than a click (a shaky click still
/// toggles a favourite).
const DRAG_SLOP: f32 = 6.0;
/// The grid's width in the 360-DIP launcher panel: the panel less its 0.6 borders and the scroll
/// bar's 8 gutters either side (`scrollbar-gutter: stable both-edges`).
pub const GRID_WIDTH: f32 = 342.8;

/// One app the launcher shows (`LauncherApp` of vskubuno `docs/SHELL-CONTROLS.md` §1).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tile {
    /// The app's id, as the server's `sidebar_items[].id`: the key the favourites are stored under.
    pub id: String,
    pub label: String,
    /// The Kubuno icon (a module's multi-coloured logo such as `DriveLogo`, or a glyph).
    pub icon: String,
    /// A logo the server serves, cached on disk: it wins over `icon`, as on the web.
    pub logo: Option<String>,
    /// The module the app belongs to (`module_id`); a module with several apps has its other apps
    /// grouped under `module_label`, as on the web.
    pub module: Option<String>,
    pub module_label: Option<String>,
}

/// Raised when an app is opened (a click on its tile outside the edit mode).
#[derive(kubuno::views::events::EventArgs, Debug, Clone, Default, PartialEq)]
pub struct TileEventArgs {
    /// The app's id.
    pub id: String,
}

/// Raised when the favourites being edited change: the list an « OK » would save now.
#[derive(kubuno::views::events::EventArgs, Debug, Clone, Default, PartialEq)]
pub struct FavoritesEventArgs {
    /// The shown favourites in order, then the ids this build does not know.
    pub favorites: Vec<String>,
}

/// The other apps as the web lists them: the apps of single-app modules sorted by label, then one
/// group per module with several apps, groups sorted by their module's label and their apps by label.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OtherApps {
    pub standalone: Vec<Tile>,
    /// `(module label, apps)`.
    pub groups: Vec<(String, Vec<Tile>)>,
}

impl OtherApps {
    /// The tiles in display order.
    pub fn tiles(&self) -> Vec<Tile> {
        self.standalone.iter().cloned().chain(self.groups.iter().flat_map(|(_, apps)| apps.iter().cloned())).collect()
    }

    /// `[standalone, group 1, group 2…]` tile counts (what [`layout`] takes).
    pub fn counts(&self) -> Vec<usize> {
        std::iter::once(self.standalone.len()).chain(self.groups.iter().map(|(_, a)| a.len())).collect()
    }
}

/// A sort key close to the web's `localeCompare`: case and the usual accents ignored.
pub fn collation_key(label: &str) -> String {
    label
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' => 'a',
            'ç' => 'c',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' | 'ì' => 'i',
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => 'o',
            'û' | 'ü' | 'ú' | 'ù' => 'u',
            'ÿ' => 'y',
            other => other,
        })
        .collect()
}

/// `others` (the apps not shown as favourites) arranged as the web does, the module counts taken over
/// every app (`all`): see [`OtherApps`].
pub fn arrange_others(all: &[Tile], others: Vec<Tile>) -> OtherApps {
    let module_of = |t: &Tile| t.module.clone().unwrap_or_else(|| t.id.clone());
    let count = |m: &str| all.iter().filter(|t| module_of(t) == m).count();
    let mut out = OtherApps::default();
    for t in others {
        let m = module_of(&t);
        if count(&m) > 1 {
            let label = t.module_label.clone().unwrap_or_else(|| m.clone());
            match out.groups.iter_mut().find(|(l, apps)| *l == label && apps.first().map(module_of).as_deref() == Some(m.as_str())) {
                Some((_, apps)) => apps.push(t),
                None => out.groups.push((label, vec![t])),
            }
        } else {
            out.standalone.push(t);
        }
    }
    out.standalone.sort_by_cached_key(|t| collation_key(&t.label));
    for (_, apps) in &mut out.groups {
        apps.sort_by_cached_key(|t| collation_key(&t.label));
    }
    out.groups.sort_by_cached_key(|g| collation_key(&g.0));
    out
}

/// Where everything of the grid goes, in its own coordinates (see the module doc).
#[derive(Debug, Clone, PartialEq)]
pub struct GridLayout {
    pub card: Rect,
    pub favorites: Vec<Rect>,
    /// The other apps' tiles, in display order (standalone, then each group's).
    pub others: Vec<Rect>,
    /// Each group's label line.
    pub group_labels: Vec<Rect>,
    pub drop_zone: Option<Rect>,
    /// The message under the card when every app is a favourite (while editing).
    pub all_favorites: Option<Rect>,
    /// The grid's whole height.
    pub height: f32,
}

/// The tiles' width in a grid `width` wide.
fn tile_width(width: f32) -> f32 {
    (width - CARD_MARGIN * 2.0 - GRID_PAD * 2.0 - TILE_GAP * (COLS - 1) as f32) / COLS as f32
}

/// The height of `n` tiles' rows (none for no tile).
fn rows_height(n: usize) -> f32 {
    let rows = n.div_ceil(COLS) as f32;
    if rows == 0.0 {
        0.0
    } else {
        rows * TILE_H + (rows - 1.0) * TILE_GAP
    }
}

/// The card's header band (see the module doc).
pub fn header_height(editing: bool) -> f32 {
    HEADER + if editing { EDIT_HELP } else { 0.0 }
}

/// Lays out `favorites` tiles and the other apps — `others[0]` standalone tiles, then one group per
/// further count — in a grid `width` wide.
pub fn layout(width: f32, favorites: usize, others: &[usize], editing: bool) -> GridLayout {
    let tw = tile_width(width);
    let step = tw + TILE_GAP;
    let header = header_height(editing);
    let fav_grid = match (favorites, editing) {
        // View mode shows nothing at all for no favourite, as the web does.
        (0, false) => 0.0,
        (0, true) => DROP_ZONE_ROOM,
        (n, _) => GRID_PAD + rows_height(n) + GRID_PAD,
    };
    let card = Rect::new(CARD_MARGIN, CARD_TOP, width - CARD_MARGIN, CARD_TOP + header + fav_grid);
    let grid_top = card.top + header;
    let favorites = (0..favorites)
        .map(|i| {
            let left = card.left + GRID_PAD + (i % COLS) as f32 * step;
            let top = grid_top + GRID_PAD + (i / COLS) as f32 * (TILE_H + TILE_GAP);
            Rect::new(left, top, left + tw, top + TILE_H)
        })
        .collect::<Vec<_>>();
    let drop_zone = (editing && favorites.is_empty()).then(|| Rect::new(card.left + GRID_PAD, grid_top, card.right - GRID_PAD, grid_top + DROP_ZONE));
    let total_others: usize = others.iter().sum();
    let mut y = card.bottom + CARD_BOTTOM_GAP;
    let mut others_rects = Vec::new();
    let mut group_labels = Vec::new();
    for (section, &n) in others.iter().enumerate() {
        if section > 0 {
            if n == 0 {
                continue;
            }
            let label_top = y + GROUP_TOP + GROUP_LABEL_PAD_TOP;
            group_labels.push(Rect::new(SIDE_PAD, label_top, width - SIDE_PAD, label_top + GROUP_LABEL_LINE));
            y += GROUP_HEAD;
        }
        for i in 0..n {
            let left = SIDE_PAD + (i % COLS) as f32 * step;
            let top = y + (i / COLS) as f32 * (TILE_H + TILE_GAP);
            others_rects.push(Rect::new(left, top, left + tw, top + TILE_H));
        }
        y += rows_height(n);
    }
    let all_favorites = (editing && total_others == 0).then(|| Rect::new(card.left, card.bottom + space::LG, card.right, card.bottom + space::LG + 24.0));
    let height = if total_others == 0 {
        card.bottom + CARD_BOTTOM_GAP + if editing { space::LG + 24.0 } else { 0.0 } + space::LG
    } else {
        // `pb-4` under the last row.
        y + space::LG
    };
    GridLayout { card, favorites, others: others_rects, group_labels, drop_zone, all_favorites, height }
}

/// How many of `favorites` (a saved list) are apps of `tiles`: the favourites shown.
pub fn shown_count(tiles: &[Tile], favorites: &[String]) -> usize {
    favorites.iter().filter(|id| tiles.iter().any(|t| &t.id == *id)).count()
}

/// The grid's height for `tiles` with `favorites` shown first (what a host sizes its panel to).
pub fn content_height(tiles: &[Tile], favorites: &[String], editing: bool) -> f32 {
    let shown: Vec<&String> = favorites.iter().filter(|id| tiles.iter().any(|t| &t.id == *id)).collect();
    let others: Vec<Tile> = tiles.iter().filter(|t| !shown.contains(&&t.id)).cloned().collect();
    layout(GRID_WIDTH, shown.len(), &arrange_others(tiles, others).counts(), editing).height
}

/// A press that may become a drag (edit mode).
#[derive(Debug, Clone)]
struct Press {
    id: String,
    from_favorites: bool,
    origin: (f32, f32),
    /// Past the slop: a drag.
    active: bool,
}

/// The app launcher's tiles (see the module doc). `TileInvoked` opens an app; `FavoritesEdited`
/// follows every change of the draft while editing.
#[derive(kubuno::views::component::Component, Default)]
#[kubuno(extends = Control, overrides(Control))]
#[category("Kubuno")]
#[toolbox(icon = "layout-grid")]
#[default_event("TileInvoked")]
pub struct AppTileGrid {
    base: ControlCore,
    /// The apps to show (a binding to a `Shared<Vec<Tile>>`; or `set_apps` from code).
    #[property(bindable, on_change = "apps_changed")]
    #[category("Data")]
    pub apps: Shared<Vec<Tile>>,
    /// The favourites as the server holds them, shown first in their order (ids of no app are
    /// ignored here, and kept by an edit).
    #[property(bindable, on_change = "favorites_changed")]
    #[category("Data")]
    pub favorites: Vec<String>,
    /// Whether the favourites are being edited: `true` starts an edit from `Favorites`, `false`
    /// abandons it.
    #[property(bindable, on_change = "editing_changed")]
    #[category("Behavior")]
    pub editing: bool,
    /// Occurs when an app's tile is clicked (outside the edit mode).
    #[event]
    #[category("Action")]
    pub tile_invoked: Event<TileEventArgs>,
    /// Occurs when the favourites being edited change (added, removed, moved).
    #[event]
    #[category("Action")]
    pub favorites_edited: Event<FavoritesEventArgs>,
    tiles: Vec<Tile>,
    /// The favourites being edited.
    draft: Option<Draft>,
    /// The tile under the pointer: `(favourite, index)`.
    hot: Option<(bool, usize)>,
    press: Option<Press>,
    /// The bounds of the last paint (the pointer arrives in local coordinates).
    bounds: Rect,
}

impl AppTileGrid {
    fn apps_changed(&mut self) {
        self.tiles = (*self.apps).clone();
        self.hot = None;
        self.invalidate();
    }

    fn favorites_changed(&mut self) {
        self.hot = None;
        self.invalidate();
    }

    fn editing_changed(&mut self) {
        match (self.editing, self.draft.is_some()) {
            (true, false) => {
                let saved = self.favorites.clone();
                self.begin_edit(&saved);
            }
            (false, true) => self.cancel_edit(),
            _ => {}
        }
    }

    /// Shows `tiles`, `favorites` first (in their order; ids of no tile are ignored here).
    pub fn set_apps(&mut self, tiles: Vec<Tile>, favorites: Vec<String>) {
        self.tiles = tiles;
        self.favorites = favorites;
        self.hot = None;
        self.invalidate();
    }

    /// Starts editing the favourites, from `saved` (the server's list, unknown ids included).
    pub fn begin_edit(&mut self, saved: &[String]) {
        let installed: Vec<String> = self.tiles.iter().map(|t| t.id.clone()).collect();
        self.draft = Some(Draft::new(saved, &installed));
        self.editing = true;
        self.hot = None;
        self.invalidate();
    }

    /// Leaves the edit mode without keeping anything.
    pub fn cancel_edit(&mut self) {
        self.draft = None;
        self.editing = false;
        self.press = None;
        self.invalidate();
    }

    /// Leaves the edit mode: the list to save (unknown ids carried at its end), shown from now on.
    pub fn finish_edit(&mut self) -> Option<Vec<String>> {
        let draft = self.draft.take()?;
        let saved = draft.to_saved();
        self.favorites = saved.clone();
        self.editing = false;
        self.press = None;
        self.invalidate();
        Some(saved)
    }

    /// Whether the favourites are being edited.
    pub fn is_editing(&self) -> bool {
        self.draft.is_some()
    }

    /// The favourite ids shown (the draft while editing).
    pub fn shown_favorites(&self) -> Vec<String> {
        let ids = self.draft.as_ref().map_or(&self.favorites, |d| &d.known);
        ids.iter().filter(|id| self.tiles.iter().any(|t| &t.id == *id)).cloned().collect()
    }

    /// The favourites' tiles, and the other apps arranged as the web lists them.
    pub fn arranged(&self) -> (Vec<Tile>, OtherApps) {
        let ids = self.shown_favorites();
        let favorites = ids.iter().filter_map(|id| self.tiles.iter().find(|t| &t.id == id).cloned()).collect();
        let others = self.tiles.iter().filter(|t| !ids.contains(&t.id)).cloned().collect();
        (favorites, arrange_others(&self.tiles, others))
    }

    /// The favourites' tiles, then the others', in display order.
    pub fn sections(&self) -> (Vec<Tile>, Vec<Tile>) {
        let (favorites, others) = self.arranged();
        (favorites, others.tiles())
    }

    /// Its layout at `width`.
    pub fn layout_at(&self, width: f32) -> GridLayout {
        let (favorites, others) = self.arranged();
        layout(width, favorites.len(), &others.counts(), self.is_editing())
    }

    fn width(&self) -> f32 {
        let w = self.bounds.right - self.bounds.left;
        if w > 0.0 { w } else { GRID_WIDTH }
    }

    /// The tile at `(x, y)` (local): `(favourite, index)`.
    fn tile_at(&self, x: f32, y: f32) -> Option<(bool, usize)> {
        let l = self.layout_at(self.width());
        if let Some(i) = l.favorites.iter().position(|r| r.contains(x, y)) {
            return Some((true, i));
        }
        l.others.iter().position(|r| r.contains(x, y)).map(|i| (false, i))
    }

    fn raise_draft(&mut self) {
        if let Some(favorites) = self.draft.as_ref().map(Draft::to_saved) {
            self.raise_favorites_edited(FavoritesEventArgs { favorites });
        }
    }

    /// Edit mode: a click on a favourite removes it, one on another app adds it.
    fn toggle(&mut self, id: &str) {
        if let Some(draft) = self.draft.as_mut() {
            draft.toggle(id);
            self.raise_draft();
            self.invalidate();
        }
    }

    /// Edit mode: `press` dropped on the tile `target` (`(favourite, index)`), or elsewhere.
    fn drop(&mut self, press: &Press, target: Option<(bool, usize)>) {
        let (favorites, _) = self.sections();
        let onto = target.filter(|(fav, _)| *fav).and_then(|(_, i)| favorites.get(i)).map(|t| t.id.clone());
        let onto_others = target.is_some_and(|(fav, _)| !fav);
        let Some(draft) = self.draft.as_mut() else { return };
        match (press.from_favorites, onto, onto_others) {
            // Reorder: the carried tile lands in front of the one under the pointer.
            (true, Some(before), _) => draft.move_before(&press.id, &before),
            // A favourite dragged onto the other apps leaves the favourites.
            (true, None, true) if draft.contains(&press.id) => draft.toggle(&press.id),
            // An app dragged into the card: added, in front of the favourite it is dropped on.
            (false, before, false) if !draft.contains(&press.id) => {
                draft.toggle(&press.id);
                if let Some(before) = before {
                    draft.move_before(&press.id, &before);
                }
            }
            _ => return,
        }
        self.raise_draft();
    }

    fn paint_tile(&self, g: &kubuno::ui::graphics::Graphics<'_>, r: Rect, tile: &Tile, hot: bool, carried: bool) {
        let c: &dyn Canvas = g;
        let t = c.theme();
        if hot && !carried {
            // `hover:bg-black/[0.06]` — the text colour at 6 %, so it also reads in the dark theme.
            let mut hover = t.text_primary;
            hover.a = 0.06;
            c.fill_rounded(&r, TILE_RADIUS, &hover);
        }
        let alpha = if carried { 0.4 } else { 1.0 };
        let cx = (r.left + r.right) / 2.0;
        let icon = Rect::new(cx - ICON / 2.0, r.top + space::LG, cx + ICON / 2.0, r.top + space::LG + ICON);
        match &tile.logo {
            // An SVG logo goes through the icon pipeline (vector, sharp at any DPI); `Image` decodes rasters only.
            Some(path) if path.to_ascii_lowercase().ends_with(".svg") => {
                if let Some(vector) = kubuno::views::icon::resolve(path) {
                    let mut colour = t.text_secondary;
                    colour.a *= alpha;
                    c.vector_icon(vector, &icon, ICON, &colour);
                }
            }
            Some(path) => g.draw_image_with(&Image::from_file(path), icon, None, alpha),
            None => {
                let mut colour = t.text_secondary;
                colour.a *= alpha;
                // An unknown name falls back to `Cloud`, as `getIcon` does on the web.
                let glyph = kubuno::views::icon::glyph(&tile.icon).unwrap_or("Cloud");
                c.vector_icon(glyph, &icon, ICON, &colour);
            }
        }
        let mut ink = t.text_secondary;
        ink.a *= alpha;
        let label = Rect::new(r.left + space::SM, icon.bottom + space::SM, r.right - space::SM, icon.bottom + space::SM + LABEL_LINE);
        c.text_ellipsis_center(&tile.label, &label, &c.formats().caption, &ink);
    }
}

impl Control for AppTileGrid {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        Size { width: GRID_WIDTH, height: self.layout_at(GRID_WIDTH).height }
    }

    /// Every tile, by its app's name, for a screen reader.
    fn accessible_parts(&self) -> Vec<AccessiblePart> {
        let l = self.layout_at(self.width());
        let (favorites, others) = self.sections();
        favorites
            .iter()
            .zip(l.favorites)
            .chain(others.iter().zip(l.others))
            .map(|(tile, bounds)| AccessiblePart { name: tile.label.clone(), role: AccessRole::ListItem, bounds })
            .collect()
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        let bounds = e.clip_rectangle;
        self.bounds = bounds;
        // The designer shows a launcher with the sample apps.
        if self.tiles.is_empty() && self.design_mode() {
            self.tiles = design_tiles();
            self.favorites = design_favorites();
        }
        let g = e.graphics;
        let c: &dyn Canvas = g;
        let t = c.theme().clone();
        let at = |r: Rect| Rect::new(bounds.left + r.left, bounds.top + r.top, bounds.left + r.right, bounds.top + r.bottom);
        let (favorites, arranged) = self.arranged();
        let others = arranged.tiles();
        let l = layout(bounds.right - bounds.left, favorites.len(), &arranged.counts(), self.is_editing());
        // Each module group's label (`text-[11px] font-semibold uppercase tracking-wide text-tertiary`).
        let group_names: Vec<String> = arranged.groups.iter().filter(|(_, a)| !a.is_empty()).map(|(l, _)| l.to_uppercase()).collect();
        for (r, name) in l.group_labels.iter().zip(&group_names) {
            c.text_ellipsis(name, &at(*r), &c.formats().micro, &t.text_tertiary);
        }
        // `bg-white`: the opaque card over the translucent panel — the application theme's surface,
        // not the ambient colour of the panel (its tint, which a free `BackColor` hands its children).
        c.fill_rounded(&at(l.card), CARD_RADIUS, &kubuno::Application::theme().layer_background);
        if let Some(zone) = l.drop_zone {
            c.stroke_rounded(&at(zone), CARD_RADIUS, &t.card_stroke);
            c.text(ShellControlsResources::launcher_drop_here(), &at(zone), &c.formats().caption, &t.text_tertiary, true);
        }
        let carried = self.press.as_ref().filter(|p| p.active).map(|p| p.id.clone());
        let hovered = carried.as_ref().and(self.hot).filter(|(fav, _)| *fav).map(|(_, i)| i);
        for (i, tile) in favorites.iter().enumerate() {
            let is_carried = carried.as_deref() == Some(tile.id.as_str());
            self.paint_tile(g, at(l.favorites[i]), tile, self.hot == Some((true, i)), is_carried);
            // The insertion bar: in the hovered favourite's LEFT gutter (the carried tile lands
            // in front of it), never on the tile itself.
            if hovered == Some(i) && !is_carried {
                let r = at(l.favorites[i]);
                let bar = Rect::new(r.left - 2.0, r.top + space::MD, r.left + 1.0, r.bottom - space::MD);
                c.fill_rounded(&bar, pill(3.0), &t.text_secondary);
            }
        }
        if let Some(r) = l.all_favorites {
            c.text(ShellControlsResources::launcher_all_favorites(), &at(r), &c.formats().caption, &t.text_tertiary, true);
        }
        for (i, tile) in others.iter().enumerate() {
            self.paint_tile(g, at(l.others[i]), tile, self.hot == Some((false, i)) && carried.is_none(), false);
        }
        e.raise(self, "OnPaint");
    }

    fn on_mouse_move(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let (x, y) = (e.args().x, e.args().y);
        if e.args().button != MouseButton::Left {
            self.press = None;
        }
        if let Some(p) = self.press.as_mut() {
            if !p.active && ((x - p.origin.0).powi(2) + (y - p.origin.1).powi(2)).sqrt() > DRAG_SLOP {
                p.active = true;
                self.invalidate();
            }
        }
        let hot = self.tile_at(x, y);
        if hot != self.hot {
            self.hot = hot;
            self.invalidate();
        }
        e.raise(&*self, "OnMouseMove");
    }

    fn on_mouse_down(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let (x, y) = (e.args().x, e.args().y);
        if self.is_editing() && e.args().button == MouseButton::Left {
            let (favorites, others) = self.sections();
            self.press = self.tile_at(x, y).and_then(|(fav, i)| {
                let tile = if fav { favorites.get(i) } else { others.get(i) }?;
                Some(Press { id: tile.id.clone(), from_favorites: fav, origin: (x, y), active: false })
            });
        }
        e.raise(&*self, "OnMouseDown");
    }

    fn on_mouse_up(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let (x, y) = (e.args().x, e.args().y);
        let target = self.tile_at(x, y);
        match self.press.take() {
            // A drag that travelled ends here, and replaces the click.
            Some(p) if p.active => {
                self.drop(&p, target);
                self.invalidate();
            }
            Some(p) => self.toggle(&p.id),
            None if !self.is_editing() => {
                let (favorites, others) = self.sections();
                let tile = target.and_then(|(fav, i)| if fav { favorites.get(i) } else { others.get(i) });
                if let Some(tile) = tile {
                    self.raise_tile_invoked(TileEventArgs { id: tile.id.clone() });
                }
            }
            None => {}
        }
        e.raise(&*self, "OnMouseUp");
    }

    fn on_mouse_leave(&mut self, e: &mut EventCx<'_, EmptyEventArgs>) {
        if self.hot.take().is_some() {
            self.invalidate();
        }
        e.raise(&*self, "OnMouseLeave");
    }
}

/// One app of the designer's sample (`design/apps.json`).
#[derive(serde::Deserialize)]
struct DesignApp {
    id: String,
    label: String,
    icon: String,
    #[serde(default)]
    favorite: bool,
    #[serde(default)]
    module: Option<String>,
    #[serde(default)]
    module_label: Option<String>,
}

fn design_apps() -> Vec<DesignApp> {
    serde_json::from_str(include_str!("design/apps.json")).unwrap_or_default()
}

/// What the designer shows: the sample instance's apps (`design/apps.json`, twelve of them).
pub fn design_tiles() -> Vec<Tile> {
    design_apps().into_iter().map(|a| Tile { id: a.id, label: a.label, icon: a.icon, logo: None, module: a.module, module_label: a.module_label }).collect()
}

/// The sample's favourites, in order (`"favorite": true` in `design/apps.json`).
pub fn design_favorites() -> Vec<String> {
    design_apps().into_iter().filter(|a| a.favorite).map(|a| a.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panel's scroll area: 360, less its 0.6 borders and 8 gutters.
    const WIDTH: f32 = GRID_WIDTH;

    /// Geometry measured on the web panel as built (2026-10-02 reference captures, 1 CSS px = 1 DIP).
    #[test]
    fn the_tiles_match_the_measured_web_panel() {
        let l = layout(WIDTH, 9, &[12], false);
        // The card: 10 from the panel's interior (10.6 from its edge; 2 in the grid), 338.8 wide, `mt-[10px]`.
        assert!((l.card.left - 2.0).abs() < 0.05 && (l.card.right - l.card.left - 338.8).abs() < 0.05);
        assert!((l.card.top - 10.6).abs() < 0.05);
        assert!((l.card.bottom - l.card.top - 409.0).abs() < 0.05, "three rows of favourites");
        // Tiles 99.6 wide, 18 from the grid (26.6 from the panel: `--kb-waffle-inset`), `gap-1`.
        let t0 = l.favorites[0];
        assert!((t0.right - t0.left - 99.6).abs() < 0.05, "tile w {}", t0.right - t0.left);
        assert!((t0.left - 18.0).abs() < 0.05);
        assert!((t0.top - l.card.top - 76.0).abs() < 0.05);
        // The other apps share the favourites' columns and follow `mb-[15px]`.
        assert!((l.others[0].left - t0.left).abs() < 0.05);
        assert!((l.others[0].top - l.card.bottom - 15.0).abs() < 0.05);
    }

    #[test]
    fn a_module_with_several_apps_is_grouped_under_its_label() {
        let t = |id: &str, label: &str, module: &str| Tile { id: id.into(), label: label.into(), module: Some(module.into()), module_label: Some(if module == "office" { "Office".into() } else { label.into() }), ..Tile::default() };
        let all = vec![t("mail", "Mail", "mail"), t("drive", "Drive", "drive"), t("office-sheets", "Tableurs", "office"), t("office-docs", "Documents", "office"), t("chat", "Chat", "chat"), t("media", "Médias", "media"), t("maps", "Plans", "maps")];
        let arranged = arrange_others(&all, all.clone());
        let ids: Vec<&str> = arranged.standalone.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(ids, ["Chat", "Drive", "Mail", "Médias", "Plans"], "sorted as `localeCompare`: accents ignored");
        assert_eq!(arranged.groups.len(), 1);
        assert_eq!(arranged.groups[0].0, "Office");
        assert_eq!(arranged.groups[0].1.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), ["Documents", "Tableurs"]);
        let l = layout(WIDTH, 0, &arranged.counts(), false);
        assert_eq!(l.group_labels.len(), 1);
        assert!((l.others[5].top - l.group_labels[0].bottom - GROUP_LABEL_PAD_BOTTOM).abs() < 0.05, "the group's tiles under its label");
    }

    #[test]
    fn the_edit_mode_makes_room_for_its_help_and_its_drop_zone() {
        assert_eq!(header_height(true) - header_height(false), EDIT_HELP);
        assert!(layout(WIDTH, 0, &[6], true).drop_zone.is_some());
        assert!(layout(WIDTH, 0, &[6], false).drop_zone.is_none());
        assert!(layout(WIDTH, 1, &[6], true).drop_zone.is_none());
        assert!(layout(WIDTH, 6, &[0], true).all_favorites.is_some());
        // No favourite outside the edit mode: the card is its header alone.
        let l = layout(WIDTH, 0, &[3], false);
        assert_eq!(l.card.bottom - l.card.top, HEADER);
    }

    #[test]
    fn tiles_wrap_into_three_columns_inside_the_grid() {
        let l = layout(WIDTH, 7, &[8], false);
        assert_eq!(l.favorites[0].top, l.favorites[2].top);
        assert!(l.favorites[3].top > l.favorites[0].top);
        assert_eq!(l.favorites[3].left, l.favorites[0].left);
        for r in l.favorites.iter().chain(l.others.iter()) {
            assert!(r.left >= 0.0 && r.right <= WIDTH, "a tile leaves the grid");
            assert!(r.bottom <= l.height);
        }
        for r in &l.favorites {
            assert!(r.top >= l.card.top && r.bottom <= l.card.bottom, "a favourite leaves its card");
        }
    }

    #[test]
    fn the_designer_sample_has_twelve_apps_with_logos() {
        let tiles = design_tiles();
        assert_eq!(tiles.len(), 12);
        assert!(tiles.iter().all(|t| !t.id.is_empty() && !t.label.is_empty() && !t.icon.is_empty()));
    }

    fn grid() -> AppTileGrid {
        let mut g = AppTileGrid::default();
        let tiles = design_tiles();
        g.set_apps(tiles, vec!["drive".into(), "mail".into(), "calendar".into()]);
        g.bounds = Rect::new(0.0, 0.0, WIDTH, 800.0);
        g
    }

    fn centre(r: Rect) -> (f32, f32) {
        ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0)
    }

    #[test]
    fn editing_toggles_and_reorders_the_draft_and_keeps_unknown_ids() {
        let mut g = grid();
        g.begin_edit(&["drive".into(), "mail".into(), "calendar".into(), "gone".into()]);
        assert_eq!(g.shown_favorites(), ["drive", "mail", "calendar"]);
        g.toggle("mail");
        assert_eq!(g.shown_favorites(), ["drive", "calendar"]);
        g.toggle("chat");
        assert_eq!(g.shown_favorites(), ["drive", "calendar", "chat"]);
        // `chat` carried onto `drive`: in front of it.
        let press = Press { id: "chat".into(), from_favorites: true, origin: (0.0, 0.0), active: true };
        g.drop(&press, Some((true, 0)));
        assert_eq!(g.shown_favorites(), ["chat", "drive", "calendar"]);
        // An app carried into the card, onto `calendar`.
        let press = Press { id: "notes".into(), from_favorites: false, origin: (0.0, 0.0), active: true };
        g.drop(&press, Some((true, 2)));
        assert_eq!(g.shown_favorites(), ["chat", "drive", "notes", "calendar"]);
        // A favourite carried onto the other apps leaves.
        let press = Press { id: "drive".into(), from_favorites: true, origin: (0.0, 0.0), active: true };
        g.drop(&press, Some((false, 0)));
        assert_eq!(g.shown_favorites(), ["chat", "notes", "calendar"]);
        assert_eq!(g.finish_edit(), Some(vec!["chat".into(), "notes".into(), "calendar".into(), "gone".into()]));
        assert!(!g.is_editing() && !g.editing);
        assert_eq!(g.shown_favorites(), ["chat", "notes", "calendar"]);
    }

    #[test]
    fn a_cancelled_edit_keeps_the_saved_list() {
        let mut g = grid();
        g.begin_edit(&["drive".into(), "mail".into()]);
        g.toggle("drive");
        g.cancel_edit();
        assert_eq!(g.shown_favorites(), ["drive", "mail", "calendar"]);
    }

    /// The bindings drive it as the methods do: `Editing` starts an edit from `Favorites` and
    /// abandons it.
    #[test]
    fn the_editing_property_starts_and_abandons_an_edit() {
        let mut g = grid();
        g.editing = true;
        g.editing_changed();
        assert!(g.is_editing());
        g.toggle("drive");
        assert_eq!(g.shown_favorites(), ["mail", "calendar"]);
        g.editing = false;
        g.editing_changed();
        assert!(!g.is_editing());
        assert_eq!(g.shown_favorites(), ["drive", "mail", "calendar"]);
        g.apps = Shared::new(design_tiles()[..2].to_vec());
        g.apps_changed();
        assert_eq!(g.shown_favorites(), ["drive", "mail"]);
    }

    #[test]
    fn the_pointer_finds_the_tile_under_it() {
        let g = grid();
        let l = g.layout_at(WIDTH);
        let (x, y) = centre(l.favorites[1]);
        assert_eq!(g.tile_at(x, y), Some((true, 1)));
        let (x, y) = centre(l.others[0]);
        assert_eq!(g.tile_at(x, y), Some((false, 0)));
        assert_eq!(g.tile_at(l.card.left + 2.0, l.card.top + 2.0), None, "the card's header is no tile");
    }
}
