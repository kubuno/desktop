//! Kubuno primitives — **views**: [`ListView`] and [`TreeView`].
//!
//! These two are the only primitives in the crate whose predecessor is not a
//! control in `kubuno-drive-desktop-app-controls` but a **whole screen of the shipping
//! product**. Drive already draws a sortable file list and a lateral tree, in
//! hard-coded form, and that rendering is the reference this family reproduces:
//!
//! | what | where the product draws it |
//! |---|---|
//! | Details list (header, sort glyph, rows, selection, hover) | `drive/crates/kubuno-drive-desktop/src/views/layouts/details_layout_page.rs` |
//! | Large-icon tiles (fill, frame, selection border, check box) | `drive/crates/kubuno-drive-desktop/src/views/layouts/grid_layout_page.rs` |
//! | Row / column / tile geometry | `drive/crates/kubuno-drive-desktop/src/ui/layout.rs`, `ui/hot.rs`, `view_models/shell_view_model.rs` |
//! | Lateral tree (row pill, chevron, indent, icon, label) | `drive/crates/kubuno-drive-desktop-app-controls/src/sidebar/sidebar_view.rs` |
//!
//! Nothing here is invented: every number below carries the file it was read
//! from, and the colours are the tokens those files already use.
//!
//! ## What comes from the replica, and what this layer adds
//!
//! The **model is not restated**. Columns, items, sub-items, `view`,
//! `full_row_select`, `grid_lines`, `check_boxes`, `multi_select`, `sorting`,
//! `focused_index`, `indent`, `item_height`, `show_lines`, `show_plus_minus`,
//! `hide_selection`, the node tree and its expand flags all live in
//! [`kubuno_desktop_controls::views`] and are reached through [`Deref`]. So does the
//! **selection machine**: [`kc::ListView::set_selected`] is what enforces
//! `MultiSelect`, [`kc::ListView::selected_indices`] is what derives the
//! selection, [`kc::ListView::sort`] is what orders by `Text`, and
//! [`kc::TreeView::visible_rows`] is what flattens the tree. This layer adds
//! only what .NET has no concept of: a **density**, a **pixel scroll offset**,
//! a **hot row/column**, a **sorted column** (the replica always puts the glyph
//! on column 0), and a **vector-icon list** an item's `ImageIndex` selects.
//!
//! ## Virtualisation
//!
//! [`ListView`] is virtual by construction: [`ListView::visible_range`] is
//! pure arithmetic on the scroll offset and the row pitch, and the painter
//! walks that range only — 100 000 items cost exactly as much as a screenful.
//! Note this is a **departure** from the replica, whose painter loops
//! `0..item_count()` and breaks at the bottom edge (correct, but only because
//! it has no scroll offset to start below row 0).
//!
//! [`TreeView`] paints only its visible slice too, but it cannot be fully
//! virtual: the replica's only flattening, [`kc::TreeView::visible_rows`],
//! returns a `Vec` and a depth-first walk has to visit every *expanded* row to
//! number them. Collapsed subtrees are skipped — 100 000 nodes under collapsed
//! roots cost nothing — but 100 000 *expanded* rows cost one walk per frame.
//! Fixing that means an indexed flattening in the replica, which this family
//! may not add.
//!
//! ## Web parity: rows, keyboard, overflow
//!
//! The row *states* of the Details list follow the web explorer
//! (`core/frontend/src/drive/storage-explorer/rows.tsx` and `rowStyles.ts`):
//! square bands, `hover:bg-surface-1`, a selected row tinted and framed by a
//! 2 DIP inset ring that merges across a run of selected rows, and the
//! keyboard cursor on an unselected row marked by a 40 % accent edge. Its
//! header follows `DetailsHeader.tsx`.
//!
//! Both views take the keyboard the way the web explorer's
//! `useExplorerKeyboard.ts` and the WAI-ARIA listbox / tree patterns do —
//! arrows, Home/End, PageUp/PageDown, Shift to extend from a fixed anchor,
//! Ctrl to move without selecting, Space / Ctrl+Space, Ctrl+A, Escape,
//! type-ahead, Enter, F2, Delete, the menu key — as pure methods
//! ([`ListView::key`], [`TreeView::key`], `type_ahead`) plus a `take_input`
//! that reads the host's per-frame queue. What an action *means* (open,
//! rename, delete, show a menu) is returned to the caller as a
//! [`ListAction`] / [`TreeAction`].
//!
//! When the content is taller than the view, both paint the range family's
//! [`ScrollBar`] as Drive's overlay bar (a 2 DIP indicator that unfolds under
//! the pointer), and expose its geometry and gestures (`scrollbar_press`,
//! `scroll_to_thumb`, `scroll_by`, `ensure_visible`).

use std::ops::{Deref, DerefMut, Range};

use kubuno_drive_desktop_app_controls::{Canvas, Rect};
use kubuno_desktop_controls::enums::{HorizontalAlignment, Size};
use kubuno_desktop_controls::host::{self, vk, InputEvent, Modifiers};
use kubuno_desktop_controls::views as kc;
use kubuno_desktop_controls::Control;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

use crate::metrics::{control, height, radius, space};
use crate::range::{ScrollBar, ScrollPart};
use crate::graphics::owner_draw::{self, DrawItemEventArgs, DrawItemState};
use crate::graphics::Graphics;
use crate::widget::{Widget, WidgetState};

pub use kubuno_desktop_controls::views::{
    ColumnHeader, ColumnHeaderStyle, ListViewItem, ListViewSubItem, NodePath, SortOrder, TreeNode,
    View, VisibleRow,
};

// ── Family-local metrics ─────────────────────────────────────────────────────
//
// They live here rather than in `crate::metrics` because they describe these
// two views and nothing else, and because `metrics.rs` is shared with eight
// families being written in parallel. Every one of them names the product file
// it was read from — none is a guess.

/// The Details header band: `file_header` spans `content.top + 8` to
/// `content.top + 36` in `kubuno-drive-desktop/src/ui/layout.rs`, i.e. 28 DIP tall.
const HEADER_HEIGHT: f32 = 28.0;

/// The gutter before a row's icon, and the icon itself at the default density
/// (`icon_size_for(Details, 2)` = 20 in `shell_view_model.rs`).
const ICON_GUTTER: f32 = space::SM;
const ICON_SIZE: f32 = 20.0;

/// The gap a cell leaves before the next column starts — the `- 8.0` every
/// cell rectangle in `details_layout_page.rs` is built with.
const CELL_GAP: f32 = space::SM;

/// The sort chevron's box, and the gap between the label and it
/// (`approx_text_width(label) + space::XS`, then a 12 DIP glyph).
const SORT_GLYPH: f32 = 12.0;

/// A tile in the icon view: `grid_item_width_for(3)` = 60 + 20×3, then the name
/// row (`GRID_NAME_ROW` = 44), with `space::MD` between tiles and around them —
/// all four from `shell_view_model.rs` / `ui/layout.rs`.
const TILE_WIDTH: f32 = 120.0;
const TILE_NAME_ROW: f32 = 44.0;
const TILE_GAP: f32 = space::MD;
const TILE_PAD: f32 = space::MD;
/// The icon inside a tile: `ItemWidthGridView` with `Margin="12"`, so the box
/// side less 24 (`icon_size_for(Grid, …)`).
const TILE_ICON_INSET: f32 = 24.0;
/// The accent frame a selected tile wears (`SELECTION_BORDER`).
const TILE_SELECTION_BORDER: f32 = 2.0;

/// A check box drawn over a tile or in a row: `CHECKBOX_*` in
/// `grid_layout_page.rs` — an 18 DIP box with a 2 DIP border at `--radius-sm`,
/// 6 DIP off the preview corner. The 18 is also [`control::CHECK_BOX`].
const CHECK_BORDER: f32 = 2.0;
const CHECK_MARGIN: f32 = 6.0;

/// A tree row's leading pad, its chevron column and the gap after it. The
/// sidebar puts the chevron in `indent - 24 … indent - 4` (a 20 DIP box then a
/// 4 DIP gap) and starts the row 8 DIP in (`ROW_LEFT_PAD` in
/// `kubuno-drive-desktop-app-controls/src/sidebar/sidebar_item.rs`).
const TREE_PAD: f32 = space::SM;
const CHEVRON_BOX: f32 = 20.0;
const CHEVRON_GAP: f32 = space::XS;
/// The chevron geometry itself is drawn at the sidebar's icon size (16 DIP
/// inside a 20 DIP box).
const CHEVRON_GLYPH: f32 = 16.0;

/// A tree row's icon box and the gap to its label (`ROW_ICON_SIZE` = 20,
/// `ROW_ICON_TEXT_GAP` = `space::MD`).
const TREE_ICON: f32 = 20.0;
const TREE_ICON_GAP: f32 = space::MD;
/// `INDENT_PER_LEVEL` = 16 and `shape::height::SIDEBAR_ROW` = 36: the two
/// numbers a Kubuno tree replaces the toolkit's 19/19 defaults with.
const TREE_INDENT: i32 = 16;
const TREE_ROW: i32 = height::SIDEBAR_ROW as i32;

/// The web explorer's selected row: a 2 DIP **inset ring** in
/// `--color-primary`, whose top edge is omitted when the row above is also
/// selected and whose bottom edge is omitted when the row below is, so a run
/// of selected rows reads as ONE frame (`selectionRingShadow` in
/// `core/frontend/src/drive/storage-explorer/rowStyles.ts`).
const SELECTION_RING: f32 = 2.0;

/// The web explorer's keyboard cursor on a row it did not select:
/// `inset 3px 0 0 0 rgba(26,115,232,0.4)` (`rowAccentShadow({ focused })` in
/// `rowStyles.ts`) — the accent at 40 % as a 3 DIP left edge.
const CURSOR_BAR: f32 = 3.0;
const CURSOR_BAR_ALPHA: f32 = 0.4;

/// `focus:ring-2` — the accent ring a focused control wears, drawn inward.
/// Same number as `text::FOCUS_RING` and `range`'s field ring.
const FOCUS_RING: f32 = 2.0;

/// The Details header labels: `gap-1` between a label and its sort chevron
/// (`DetailsHeader.tsx`, `cell = 'flex items-center gap-1 …'`).
const SORT_GAP: f32 = space::XS;

/// Type-ahead: keys typed within this delay extend the same search, a longer
/// pause starts a new one. The WAI-ARIA APG listbox and tree examples reset
/// their buffer after 500 ms; Windows' list views wait about a second. The
/// longer one is kept so a slow typist still reaches « Photos de vacances ».
pub const TYPEAHEAD_RESET_MS: u64 = 1000;

/// The inline rename box (`F2`): the web explorer renames in an `@ui/Input`,
/// i.e. `px-3` inside a `focus:ring-2` ring at `--radius-sm`, over the name
/// cell. The horizontal pad is trimmed to `px-2` because the box sits inside
/// a 40 DIP row rather than on a form.
const RENAME_PAD_X: f32 = space::SM;
/// The caret: one DIP wide, in the text colour (a native `<input>` caret),
/// and one `text-xs` line tall (`line-height: 1rem`).
const CARET_WIDTH: f32 = 1.0;
const CARET_HEIGHT: f32 = 16.0;

// =============================================================================
//  Keyboard — shared by both views
// =============================================================================

/// A key a view reacts to, decoded from a host key event by
/// [`NavKey::from_vk`]. Kept as its own enum so the navigation logic
/// ([`ListView::key`], [`TreeView::key`]) is pure and testable without a
/// window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKey {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    /// Space: select the cursor item (Ctrl+Space toggles it).
    Space,
    /// Enter: open / activate the cursor item.
    Enter,
    /// F2: rename the cursor item.
    F2,
    /// Escape: the web explorer clears the selection.
    Escape,
    /// Delete: the web explorer trashes the selection.
    Delete,
    /// Ctrl+A.
    SelectAll,
    /// The context-menu key, or Shift+F10.
    ContextMenu,
}

impl NavKey {
    /// Decodes a key-down. Alt chords are never navigation (they belong to the
    /// window), and Ctrl only combines with the arrows, Home/End, the page
    /// keys and Space — plus Ctrl+A, which becomes [`NavKey::SelectAll`].
    pub fn from_vk(code: u16, mods: Modifiers) -> Option<NavKey> {
        if mods.alt {
            return None;
        }
        if code == vk::letter('A') {
            return (mods.ctrl && !mods.shift).then_some(NavKey::SelectAll);
        }
        if code == vk::APPS || (code == vk::F10 && mods.shift && !mods.ctrl) {
            return Some(NavKey::ContextMenu);
        }
        let key = match code {
            vk::UP => NavKey::Up,
            vk::DOWN => NavKey::Down,
            vk::LEFT => NavKey::Left,
            vk::RIGHT => NavKey::Right,
            vk::HOME => NavKey::Home,
            vk::END => NavKey::End,
            vk::PAGE_UP => NavKey::PageUp,
            vk::PAGE_DOWN => NavKey::PageDown,
            vk::SPACE => NavKey::Space,
            vk::ENTER => NavKey::Enter,
            vk::F2 => NavKey::F2,
            vk::ESCAPE => NavKey::Escape,
            vk::DELETE => NavKey::Delete,
            _ => return None,
        };
        // Enter / F2 / Escape / Delete are plain keys; with Ctrl they are
        // someone else's shortcut. (Shift+Delete is the web's « delete
        // permanently », so Shift is let through.)
        if mods.ctrl && matches!(key, NavKey::Enter | NavKey::F2 | NavKey::Escape | NavKey::Delete) {
            return None;
        }
        Some(key)
    }
}

/// The index of the first label, searching from `start` and wrapping around,
/// that starts with `query` — case-insensitively, the way the APG type-ahead
/// matches. `None` when nothing matches or `query` is empty.
fn find_prefix<S: AsRef<str>>(labels: &[S], start: usize, query: &str) -> Option<usize> {
    if labels.is_empty() || query.is_empty() {
        return None;
    }
    let q = query.to_lowercase();
    let n = labels.len();
    (0..n)
        .map(|k| (start + k) % n)
        .find(|&i| labels[i].as_ref().to_lowercase().starts_with(&q))
}

/// The type-ahead buffer both views keep: what was typed, and when.
#[derive(Debug, Clone, Default)]
struct TypeAhead {
    buffer: String,
    at: u64,
}

impl TypeAhead {
    /// Whether a new keystroke at `now` starts a new search.
    fn idle(&self, now: u64) -> bool {
        self.buffer.is_empty() || now.saturating_sub(self.at) > TYPEAHEAD_RESET_MS
    }

    /// Appends `text` and returns the query plus whether it is one character
    /// typed repeatedly (« aaa »), which the APG treats as « cycle through
    /// the items starting with a » rather than as a three-letter prefix.
    fn push(&mut self, text: &str, now: u64) -> (String, bool) {
        if self.idle(now) {
            self.buffer.clear();
        }
        self.buffer.push_str(text);
        self.at = now;
        let mut chars = self.buffer.chars();
        let first = chars.next();
        let repeated = self.buffer.chars().count() > 1 && chars.all(|c| Some(c) == first);
        match (repeated, first) {
            (true, Some(c)) => (c.to_string(), true),
            _ => (self.buffer.clone(), false),
        }
    }
}

// =============================================================================
//  Overlay scroll bar — shared by both views
// =============================================================================

/// The vertical overlay bar a view wears when its content is taller than the
/// area it scrolls in — the range family's [`ScrollBar`], resting as the 2 DIP
/// indicator and unfolding when `hot`, painted OVER the rows like Drive's file
/// area (`kubuno-drive-desktop-app-controls/src/scrollbar`). `None` when nothing overflows.
/// `frame` and `radius` are the view's outline: the rail is shortened to stay
/// clear of its rounded corners.
fn overlay_bar(area: Rect, frame: Rect, radius: f32, extent: f32, scroll: f32, hot: bool) -> Option<(Rect, ScrollBar)> {
    let viewport = area.bottom - area.top;
    let mut bar = ScrollBar::from_content(false, extent, viewport, scroll)?;
    bar.expanded = hot;
    let rail = crate::range::fit_rail(bar.rail(&area), frame, radius);
    Some((rail, bar))
}

/// The scroll offset that puts the thumb's top edge at `thumb_top` — the
/// inverse of [`ScrollBar::thumb_rect`], for a thumb drag.
fn scroll_for_thumb(bar: &ScrollBar, rail: Rect, thumb_top: f32, max_scroll: f32) -> f32 {
    let thumb = bar.thumb_rect(rail);
    let len = thumb.bottom - thumb.top;
    let inset = if bar.expanded { control::SCROLLBAR_ARROW } else { 0.0 };
    let travel = (rail.bottom - rail.top) - 2.0 * inset - len;
    if travel <= 0.0 || max_scroll <= 0.0 {
        return 0.0;
    }
    ((thumb_top - rail.top - inset) / travel).clamp(0.0, 1.0) * max_scroll
}

/// The scroll offset that brings the band `top..bottom` (content space) into
/// a `viewport`-tall window currently at `scroll` — the minimal move, as
/// `scrollIntoView({ block: 'nearest' })` does in the web explorer.
fn nearest_scroll(scroll: f32, viewport: f32, top: f32, bottom: f32) -> f32 {
    if top < scroll {
        top
    } else if bottom > scroll + viewport {
        (bottom - viewport).min(top)
    } else {
        scroll
    }
}

/// `color` at `alpha` of its own opacity — a token, faded.
fn faded(color: &D2D1_COLOR_F, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: color.a * alpha, ..*color }
}

/// Text in a cell with the web's `truncate`: aligned as asked while it fits,
/// ellipsised (from the leading edge) once it does not — a right-aligned
/// size that grew wider than its column must not spill into the next one.
fn cell_text(
    c: &dyn Canvas,
    text: &str,
    rect: &Rect,
    format: &IDWriteTextFormat,
    colour: &D2D1_COLOR_F,
    align: HorizontalAlignment,
) {
    if align == HorizontalAlignment::Left || c.measure(text, format) > rect.right - rect.left {
        c.text_ellipsis(text, rect, format, colour);
    } else {
        c.text_aligned(text, rect, format, colour, align.dwrite());
    }
}

/// The inline rename box both views open on F2: an `@ui/Input` (ground,
/// `focus:ring-2` accent ring at `--radius-sm`) holding `text` and, when
/// `caret` is true, a caret after it. A name longer than the box shows its
/// END — the part being typed — the way a native input scrolls to its caret.
fn paint_inline_editor(c: &dyn Canvas, rect: Rect, text: &str, caret: bool) {
    let t = c.theme();
    let f = c.formats();
    c.fill_rounded(&rect, radius::SM, &t.layer_background);
    c.stroke_rounded_w(&rect, radius::SM, &t.accent, FOCUS_RING);
    let inner = Rect::new(rect.left + RENAME_PAD_X, rect.top, rect.right - RENAME_PAD_X, rect.bottom);
    let width = (inner.right - inner.left).max(0.0);
    let w = c.measure(text, &f.body);
    c.push_clip(&inner);
    let x0 = if w > width { inner.right - w } else { inner.left };
    let run = Rect::new(x0, inner.top, x0 + w.max(width), inner.bottom);
    c.text_aligned(text, &run, &f.body, &t.text_primary, HorizontalAlignment::Left.dwrite());
    if caret {
        let cx = (x0 + w).min(inner.right - CARET_WIDTH);
        let cy = (rect.top + rect.bottom) / 2.0;
        let half = CARET_HEIGHT / 2.0;
        c.fill_rounded(&Rect::new(cx, cy - half, cx + CARET_WIDTH, cy + half), 0.0, &t.text_primary);
    }
    c.pop_clip();
}

// =============================================================================
//  ListView
// =============================================================================

/// How tall a row is. WinForms derives a row height from the font; the Kubuno
/// design system picks it from a **density**, exactly as
/// `shell_view_model::row_height_for` does — which is why this is a field the
/// replica has no counterpart for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Density {
    /// `row_height_for(1)` — `FILE_ROW - 8`.
    Compact,
    /// `row_height_for(2)` — [`height::FILE_ROW`], the product's default.
    #[default]
    Normal,
    /// `row_height_for(3)` — `FILE_ROW + 4`.
    Comfortable,
}

impl Density {
    /// The row height in DIP.
    pub fn row_height(self) -> f32 {
        match self {
            Density::Compact => height::FILE_ROW - 8.0,
            Density::Normal => height::FILE_ROW,
            Density::Comfortable => height::FILE_ROW + 4.0,
        }
    }
}

/// A Kubuno list view — [`kc::ListView`] with the product's pixels.
///
/// ```text
///   ListView
///     ├── inner: kubuno_desktop_controls::views::ListView  ← columns, items, sub-items,
///     │                                              view, full_row_select,
///     │                                              grid_lines, check_boxes,
///     │                                              multi_select, sorting,
///     │                                              set_selected / sort
///     ├── density / scroll / hot_index / hot_column / sort_column
///     └── paint(&dyn Canvas, …)                    ← Kubuno pixels
/// ```
pub struct ListView {
    inner: kc::ListView,

    /// The row height's density — see [`Density`].
    pub density: Density,
    /// The **pixel** scroll offset of the body, in DIP.
    ///
    /// The replica carries `TopItem` (a whole-row index), which cannot express
    /// a partially scrolled first row; a smooth-scrolling Kubuno view needs the
    /// finer number, so it is added here rather than approximated.
    pub scroll: f32,
    /// The row the pointer is over — the product's `Hot::FileRow(i)`. The
    /// replica has only `hot_tracking: bool`, which says whether to react, not
    /// to what.
    pub hot_index: Option<usize>,
    /// The column header the pointer is over (`Hot::FileHeaderCol(i)`).
    pub hot_column: Option<usize>,
    /// Which column carries the sort glyph. The replica always puts it on
    /// column 0 because its `sort()` orders by `Text`; the product sorts by any
    /// column (`tab.sort_column`), so the column is a value here.
    pub sort_column: usize,
    /// The vector-icon names an item's `ImageIndex` selects.
    ///
    /// The replica deliberately models the *index* and not the list (an
    /// `ImageList` is a device-bound bitmap collection). A Kubuno view paints
    /// geometry, not bitmaps, so its "image list" is a list of icon names from
    /// `assets/themed-icons.txt` / `assets/lucide-icons.txt`.
    pub image_list: Vec<&'static str>,
    /// Which `image_list` entry marks an item as a **container**.
    ///
    /// The product tints a selected folder card `selected_folder` and a
    /// selected file card `selected_card` (`grid_layout_page.rs`), and the
    /// replica's item model carries no "is a directory" bit — so the rule is
    /// declared here rather than guessed from the text.
    pub container_icon: Option<&'static str>,
    /// The **keyboard cursor** — the web explorer's `cursorId`: the item the
    /// arrows move from, which Enter opens and F2 renames. It is distinct from
    /// the selection *anchor* ([`kc::ListView::focused_index`]) so that
    /// Shift+arrows can extend from a fixed anchor, and Ctrl+arrows can move
    /// it without selecting. `None` falls back to the anchor.
    pub cursor: Option<usize>,
    /// The pointer is over the overlay scroll bar's gutter: the bar unfolds
    /// (see [`ScrollBar::expanded`]) and its thumb takes the hover colour.
    pub scrollbar_hot: bool,
    /// Clip the whole view to this corner radius — for a view laid flush in a
    /// rounded container (a `Card`), whose corners it would otherwise square
    /// off. `0.0` (the default) is the product's flat file area.
    pub corner_radius: f32,
    typeahead: TypeAhead,
}

/// What a key, a type-ahead or a gesture did to a [`ListView`] — what the
/// caller has to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ListAction {
    /// The cursor moved to this item (and the selection may have followed).
    CursorMoved(usize),
    /// The selection changed without the cursor moving (Space, Ctrl+A,
    /// Escape).
    SelectionChanged,
    /// Enter on this item — the web's `openItem`.
    Open(usize),
    /// F2 on this item — start an inline rename.
    Rename(usize),
    /// Delete on a non-empty selection; `true` for Shift+Delete, the web's
    /// « delete permanently ».
    Delete { permanent: bool },
    /// The context-menu key or Shift+F10 on this item: open its menu at
    /// [`ListView::context_anchor`].
    ContextMenu(usize),
}

impl Default for ListView {
    fn default() -> Self {
        let mut inner = kc::ListView::new();
        // The Kubuno list is a flat surface on the module panel: the toolkit's
        // sunken 3D well is the one default this layer overrides, because the
        // product's file area has no frame at all.
        inner.border_style = kubuno_desktop_controls::enums::BorderStyle::None;
        Self {
            inner,
            density: Density::default(),
            scroll: 0.0,
            hot_index: None,
            hot_column: None,
            sort_column: 0,
            image_list: Vec::new(),
            container_icon: Some("Folder"),
            cursor: None,
            scrollbar_hot: false,
            corner_radius: 0.0,
            typeahead: TypeAhead::default(),
        }
    }
}

impl ListView {
    pub fn new() -> Self {
        Self::default()
    }

    // ── Geometry ─────────────────────────────────────────────────────────

    /// The row height, from the density.
    pub fn row_height(&self) -> f32 {
        self.density.row_height()
    }

    /// The header band's height — zero when it is hidden, or when the view is
    /// not Details (the replica's own rule, in `details_geometry`).
    pub fn header_height(&self) -> f32 {
        if self.inner.header_style == ColumnHeaderStyle::None || self.inner.view != View::Details {
            0.0
        } else {
            HEADER_HEIGHT
        }
    }

    /// The replica's [`kc::DetailsGeometry`], filled with **Kubuno** numbers.
    ///
    /// The struct and its arithmetic (`column_x_offsets`, `row_rect`,
    /// `hit_test`) are reused verbatim; only the three metrics it is built from
    /// are the design system's rather than the toolkit's.
    pub fn geometry(&self, bounds: Rect) -> kc::DetailsGeometry {
        kc::DetailsGeometry {
            content: bounds,
            header_height: self.header_height(),
            row_height: self.row_height(),
        }
    }

    /// The area rows are laid out in — everything under the header.
    pub fn body(&self, bounds: Rect) -> Rect {
        Rect::new(bounds.left, bounds.top + self.header_height(), bounds.right, bounds.bottom)
    }

    /// The left x of every column, plus one entry at the right edge of the last
    /// one — [`kc::DetailsGeometry::column_x_offsets`].
    pub fn column_x_offsets(&self, bounds: Rect) -> Vec<f32> {
        self.geometry(bounds).column_x_offsets(&self.inner.columns)
    }

    /// The sum of the declared column widths.
    pub fn total_column_width(&self) -> f32 {
        self.inner.columns.iter().map(|c| c.width.max(0) as f32).sum()
    }

    /// The x of each column **separator** — the draggable edge between column
    /// `i` and `i + 1`. There is one per column, the last being the right edge
    /// of the last column, which is what a resize grip needs.
    pub fn separators(&self, bounds: Rect) -> Vec<f32> {
        self.column_x_offsets(bounds).into_iter().skip(1).collect()
    }

    /// The column whose right edge is within `tolerance` of `x` — the hit test
    /// a column-resize cursor is driven by. Ties go to the leftmost separator.
    pub fn separator_at(&self, bounds: Rect, x: f32, tolerance: f32) -> Option<usize> {
        self.separators(bounds)
            .into_iter()
            .enumerate()
            .find(|(_, sx)| (x - sx).abs() <= tolerance)
            .map(|(i, _)| i)
    }

    /// Where a row's **name** starts, measured from the row's left edge:
    /// gutter + optional check box + icon + gap, never under 36 DIP
    /// (`name_left_for` in `shell_view_model.rs`). The header and every cell
    /// read it from here, so a check box pushes the name instead of landing
    /// under it.
    pub fn name_left(&self) -> f32 {
        let check =
            if self.inner.check_boxes { control::CHECK_BOX + control::CHECK_GAP } else { 0.0 };
        (ICON_GUTTER + check + ICON_SIZE + space::SM).max(36.0)
    }

    /// A tile's size in the icon views: the replica's `TileSize` when it was
    /// set, else the product's own tile (`grid_item_width_for` + the name row).
    pub fn tile_size(&self) -> (f32, f32) {
        if self.inner.tile_size.is_empty() {
            (TILE_WIDTH, TILE_WIDTH + TILE_NAME_ROW)
        } else {
            (self.inner.tile_size.width, self.inner.tile_size.height)
        }
    }

    /// How many tiles fit across `bounds` — `((inner + gap) / (tile + gap))`,
    /// the wrap arithmetic of `ui/layout.rs`. Never zero.
    pub fn tiles_per_row(&self, bounds: Rect) -> usize {
        let (tw, _) = self.tile_size();
        let inner = (bounds.right - bounds.left) - TILE_PAD * 2.0;
        (((inner + TILE_GAP) / (tw + TILE_GAP)).floor().max(1.0)) as usize
    }

    /// The number of rows to reason about — [`kc::ListView::item_count`], which
    /// answers `VirtualListSize` in virtual mode.
    fn count(&self) -> usize {
        self.inner.item_count()
    }

    /// **The virtualisation entry point**: the half-open range of item indices
    /// that intersect `bounds` at `scroll`.
    ///
    /// Both ends are inclusive of a *partially* visible item: a view scrolled
    /// by half a row starts at that half row, and ends with the row the bottom
    /// edge cuts through. Painting anything outside this range is wasted work,
    /// and painting less than it leaves a gap at an edge.
    pub fn visible_range(&self, bounds: Rect, scroll: f32) -> Range<usize> {
        let count = self.count();
        let body = self.body(bounds);
        let h = body.bottom - body.top;
        if count == 0 || h <= 0.0 {
            return 0..0;
        }
        match self.inner.view {
            View::Details => {
                let rh = self.row_height();
                if rh <= 0.0 {
                    return 0..0;
                }
                let first = (scroll / rh).floor().max(0.0) as usize;
                let last = (((scroll + h) / rh).ceil().max(0.0) as usize).min(count);
                first.min(last)..last
            }
            View::LargeIcon => {
                let (_, th) = self.tile_size();
                let pitch = th + TILE_GAP;
                if pitch <= 0.0 {
                    return 0..0;
                }
                let per_row = self.tiles_per_row(bounds);
                // The first row whose BOTTOM is still below the top edge, and
                // the first one whose TOP is already past the bottom edge. A
                // tile is `pitch` apart but only `th` tall, so using the pitch
                // for both ends would keep painting a row that has entirely
                // left through the top.
                let first_row = (((scroll - TILE_PAD - th) / pitch).floor() + 1.0).max(0.0) as usize;
                let last_row = ((scroll + h - TILE_PAD) / pitch).ceil().max(0.0) as usize;
                let first = (first_row * per_row).min(count);
                let last = (last_row * per_row).min(count);
                first.min(last)..last
            }
            // List, SmallIcon and Tile have no Kubuno layout — see `paint`.
            _ => 0..0,
        }
    }

    /// The total height the items occupy, and therefore the largest scroll
    /// offset that still shows content.
    pub fn content_height(&self, bounds: Rect) -> f32 {
        let count = self.count();
        match self.inner.view {
            View::Details => count as f32 * self.row_height(),
            View::LargeIcon => {
                let (_, th) = self.tile_size();
                let rows = count.div_ceil(self.tiles_per_row(bounds).max(1));
                if rows == 0 {
                    0.0
                } else {
                    TILE_PAD * 2.0 + rows as f32 * th + (rows - 1) as f32 * TILE_GAP
                }
            }
            _ => 0.0,
        }
    }

    /// The scroll offset that puts the last item against the bottom edge.
    pub fn max_scroll(&self, bounds: Rect) -> f32 {
        let body = self.body(bounds);
        (self.content_height(bounds) - (body.bottom - body.top)).max(0.0)
    }

    /// The rectangle item `index` occupies at `scroll` — a row band in Details,
    /// a tile in the icon view.
    pub fn item_rect(&self, bounds: Rect, index: usize, scroll: f32) -> Rect {
        let body = self.body(bounds);
        match self.inner.view {
            View::LargeIcon => {
                let (tw, th) = self.tile_size();
                let per_row = self.tiles_per_row(bounds);
                let col = index % per_row;
                let row = index / per_row;
                let x = body.left + TILE_PAD + col as f32 * (tw + TILE_GAP);
                let y = body.top + TILE_PAD + row as f32 * (th + TILE_GAP) - scroll;
                Rect::new(x, y, x + tw, y + th)
            }
            _ => {
                let rh = self.row_height();
                let top = body.top + index as f32 * rh - scroll;
                Rect::new(body.left, top, body.right, top + rh)
            }
        }
    }

    /// The item under `(x, y)`, or `None` for the header band, a gap between
    /// tiles, or empty space past the last item.
    ///
    /// Uses the view's own [`ListView::scroll`], because a hit test answers a
    /// question about what is on screen *now*.
    pub fn row_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        let body = self.body(bounds);
        if !body.contains(x, y) {
            return None;
        }
        let count = self.count();
        match self.inner.view {
            View::Details => {
                let rh = self.row_height();
                if rh <= 0.0 {
                    return None;
                }
                let offset = y - body.top + self.scroll;
                if offset < 0.0 {
                    return None;
                }
                let index = (offset / rh).floor() as usize;
                (index < count).then_some(index)
            }
            View::LargeIcon => {
                let (tw, th) = self.tile_size();
                let per_row = self.tiles_per_row(bounds);
                let dx = x - (body.left + TILE_PAD);
                let dy = y - (body.top + TILE_PAD) + self.scroll;
                if dx < 0.0 || dy < 0.0 {
                    return None;
                }
                let col = (dx / (tw + TILE_GAP)).floor() as usize;
                let row = (dy / (th + TILE_GAP)).floor() as usize;
                // Reject the gap between two tiles: it belongs to neither.
                if dx - col as f32 * (tw + TILE_GAP) >= tw
                    || dy - row as f32 * (th + TILE_GAP) >= th
                    || col >= per_row
                {
                    return None;
                }
                let index = row * per_row + col;
                (index < count).then_some(index)
            }
            _ => None,
        }
    }

    /// The column `x` falls in, header band included — the Details column
    /// arithmetic, so a header click and a cell click agree.
    pub fn column_at(&self, bounds: Rect, x: f32) -> Option<usize> {
        let xs = self.column_x_offsets(bounds);
        xs.windows(2).position(|w| x >= w[0] && x < w[1])
    }

    /// Whether `(x, y)` lands in the header band.
    pub fn header_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        let h = self.header_height();
        if h <= 0.0 || y < bounds.top || y >= bounds.top + h || x < bounds.left || x >= bounds.right
        {
            return None;
        }
        self.column_at(bounds, x)
    }

    // ── Selection ────────────────────────────────────────────────────────
    //
    // The state machine is the replica's: `set_selected` is what enforces
    // `MultiSelect`, and `selected_indices` is what derives the selection from
    // the per-item flag. What is added is the part WinForms implements in its
    // *input* layer rather than in the control value — the modifier rules.

    /// Deselects everything.
    pub fn clear_selection(&mut self) {
        for it in &mut self.inner.items {
            it.selected = false;
        }
    }

    /// Selects the closed interval between two indices, in either order,
    /// clearing whatever was selected before — a shift-click's extension.
    ///
    /// With `MultiSelect` off, only `to` survives, because that is what
    /// [`kc::ListView::set_selected`] enforces.
    pub fn select_range(&mut self, from: usize, to: usize) {
        self.clear_selection();
        let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
        if !self.inner.multi_select {
            self.inner.set_selected(to, true);
            return;
        }
        for i in lo..=hi.min(self.inner.items.len().saturating_sub(1)) {
            self.inner.set_selected(i, true);
        }
    }

    /// A click on item `index` with the two modifiers that change what it
    /// means.
    ///
    /// * plain — the item becomes the whole selection, and the anchor;
    /// * ctrl — the item's own flag flips, the rest is untouched, and it
    ///   becomes the anchor;
    /// * shift — the selection becomes the interval from the anchor to the
    ///   item, and the **anchor does not move** (so a second shift-click
    ///   re-extends from the same place).
    ///
    /// The anchor is [`kc::ListView::focused_index`] — the replica's
    /// `FocusedItem` — rather than a second field, because two places holding
    /// "where the selection started" is how the two disagree.
    pub fn click(&mut self, index: usize, ctrl: bool, shift: bool) {
        if index >= self.inner.items.len() {
            return;
        }
        // Whatever the modifiers, the clicked item takes the keyboard cursor —
        // the web's `focusCursor` on click, so the arrows continue from it.
        self.cursor = Some(index);
        match (shift && self.inner.multi_select, ctrl && self.inner.multi_select) {
            (true, _) => {
                let anchor = self.inner.focused_index.unwrap_or(index);
                self.select_range(anchor, index);
            }
            (false, true) => {
                let now = self.inner.items[index].selected;
                self.inner.set_selected(index, !now);
                self.inner.focused_index = Some(index);
            }
            (false, false) => {
                self.clear_selection();
                self.inner.set_selected(index, true);
                self.inner.focused_index = Some(index);
            }
        }
    }

    /// Sorts the items by [`ListView::sort_column`], in the replica's
    /// `Sorting` direction.
    ///
    /// Column 0 delegates to [`kc::ListView::sort`] — the replica already
    /// orders by `Text`, which *is* column 0. Any other column sorts on
    /// [`kc::ListViewItem::cell`], the replica's own column accessor.
    pub fn sort(&mut self) {
        if self.sort_column == 0 {
            self.inner.sort();
            return;
        }
        let col = self.sort_column;
        match self.inner.sorting {
            SortOrder::None => {}
            SortOrder::Ascending => {
                self.inner.items.sort_by(|a, b| a.cell(col).cmp(b.cell(col)));
            }
            SortOrder::Descending => {
                self.inner.items.sort_by(|a, b| b.cell(col).cmp(a.cell(col)));
            }
        }
    }

    /// [`ListView::click`] with the host's modifiers.
    pub fn click_mods(&mut self, index: usize, mods: Modifiers) {
        self.click(index, mods.ctrl, mods.shift);
    }

    /// A right click on item `index`, before its context menu opens: an item
    /// that is already selected keeps the whole selection (the menu acts on
    /// all of it), any other one becomes the selection — Explorer's rule and
    /// the web explorer's `onContextMenu`.
    pub fn context_click(&mut self, index: usize) {
        if index >= self.inner.items.len() {
            return;
        }
        if !self.inner.items[index].selected {
            self.click(index, false, false);
        }
        self.cursor = Some(index);
    }

    // ── Keyboard ─────────────────────────────────────────────────────────

    /// The item the keyboard acts on: the cursor, else the anchor, else the
    /// last selected item (the web's `selectionAnchor`).
    pub fn cursor_index(&self) -> Option<usize> {
        let count = self.count();
        self.cursor
            .or(self.inner.focused_index)
            .or_else(|| self.inner.items.iter().rposition(|i| i.selected))
            .filter(|&i| i < count)
    }

    /// How many items one PageUp / PageDown moves by: the whole rows that fit
    /// in the body, times the tiles per row in the icon view. Never zero.
    pub fn page_items(&self, bounds: Rect) -> usize {
        let body = self.body(bounds);
        let h = (body.bottom - body.top).max(0.0);
        match self.inner.view {
            View::LargeIcon => {
                let (_, th) = self.tile_size();
                let rows = ((h + TILE_GAP) / (th + TILE_GAP)).floor().max(1.0) as usize;
                rows * self.tiles_per_row(bounds)
            }
            _ => (h / self.row_height()).floor().max(1.0) as usize,
        }
    }

    /// Moves the cursor to `target` the way an arrow key with `mods` does:
    ///
    /// * plain — `target` becomes the whole selection and the anchor;
    /// * Shift — the selection becomes anchor…target, the anchor stays;
    /// * Ctrl — only the cursor moves (Space then toggles the item).
    ///
    /// Then scrolls it into view.
    fn move_cursor(&mut self, bounds: Rect, target: usize, mods: Modifiers) {
        if mods.shift && self.inner.multi_select {
            let anchor = self.inner.focused_index.or(self.cursor).unwrap_or(target);
            self.select_range(anchor, target);
            self.inner.focused_index = Some(anchor);
        } else if !mods.ctrl {
            self.clear_selection();
            self.inner.set_selected(target, true);
            self.inner.focused_index = Some(target);
        }
        self.cursor = Some(target);
        self.ensure_visible(bounds, target);
    }

    /// Reacts to one navigation key, as the web explorer's keyboard hook and
    /// the WAI-ARIA listbox pattern do. Returns what the caller must act on,
    /// or `None` when the key means nothing here (so it stays in the queue
    /// for someone else — Left/Right in Details, Escape with nothing
    /// selected).
    ///
    /// Up/Down move one row (one row of tiles in the icon view, where
    /// Left/Right move one tile), Home/End go to the ends, PageUp/PageDown
    /// move a screenful; see [`ListView::move_cursor`] for what Shift and
    /// Ctrl do to the selection.
    pub fn key(&mut self, bounds: Rect, key: NavKey, mods: Modifiers) -> Option<ListAction> {
        let count = self.count();
        if count == 0 {
            return None;
        }
        let last = count - 1;
        let cur = self.cursor_index();
        let tiles = self.inner.view == View::LargeIcon;
        let step = if tiles { self.tiles_per_row(bounds) } else { 1 };
        let page = self.page_items(bounds);
        // Without a cursor, every movement key lands on the first item — the
        // web's `if (!anchor) focusCursor(orderedIds[0])`.
        let target = match key {
            NavKey::Up => Some(cur.map_or(0, |c| c.saturating_sub(step))),
            NavKey::Down => Some(cur.map_or(0, |c| (c + step).min(last))),
            NavKey::Left if tiles => Some(cur.map_or(0, |c| c.saturating_sub(1))),
            NavKey::Right if tiles => Some(cur.map_or(0, |c| (c + 1).min(last))),
            NavKey::Home => Some(0),
            NavKey::End => Some(last),
            NavKey::PageUp => Some(cur.map_or(0, |c| c.saturating_sub(page))),
            NavKey::PageDown => Some(cur.map_or(0, |c| (c + page).min(last))),
            _ => None,
        };
        if let Some(t) = target {
            self.move_cursor(bounds, t, mods);
            return Some(ListAction::CursorMoved(t));
        }
        match key {
            NavKey::Space => {
                let c = cur?;
                if mods.ctrl && self.inner.multi_select {
                    let now = self.inner.items.get(c).is_some_and(|i| i.selected);
                    self.inner.set_selected(c, !now);
                    self.inner.focused_index = Some(c);
                } else {
                    self.move_cursor(bounds, c, Modifiers { ctrl: false, ..mods });
                }
                Some(ListAction::SelectionChanged)
            }
            NavKey::Enter => cur.map(ListAction::Open),
            NavKey::F2 => cur.map(ListAction::Rename),
            NavKey::ContextMenu => cur.map(ListAction::ContextMenu),
            NavKey::Delete => self
                .inner
                .items
                .iter()
                .any(|i| i.selected)
                .then_some(ListAction::Delete { permanent: mods.shift }),
            NavKey::SelectAll if self.inner.multi_select => {
                for i in 0..self.inner.items.len() {
                    self.inner.set_selected(i, true);
                }
                Some(ListAction::SelectionChanged)
            }
            NavKey::Escape if self.inner.items.iter().any(|i| i.selected) => {
                self.clear_selection();
                Some(ListAction::SelectionChanged)
            }
            _ => None,
        }
    }

    /// Type-ahead: `text` typed at `now_ms` extends the search (or starts a
    /// new one after [`TYPEAHEAD_RESET_MS`]) and moves the cursor — and the
    /// selection — to the next item whose name starts with it. Typing the
    /// same letter again cycles through the items starting with it.
    pub fn type_ahead(&mut self, bounds: Rect, text: &str, now_ms: u64) -> Option<ListAction> {
        let (query, repeated) = self.typeahead.push(text, now_ms);
        let labels: Vec<&str> = self.inner.items.iter().map(|i| i.text.as_str()).collect();
        let cur = self.cursor_index();
        // A new or repeated letter looks PAST the cursor; a longer prefix
        // keeps the cursor if it still matches.
        let start = match cur {
            Some(c) if repeated || query.chars().count() == 1 => c + 1,
            Some(c) => c,
            None => 0,
        };
        let hit = find_prefix(&labels, start, &query)?;
        self.move_cursor(bounds, hit, Modifiers::NONE);
        Some(ListAction::CursorMoved(hit))
    }

    /// Takes this frame's keys and typed text from the host queue — only what
    /// the view reacts to, the rest is left for the next consumer — and
    /// returns what happened. Call it once per frame while the view holds the
    /// focus, before painting.
    pub fn take_input(&mut self, bounds: Rect) -> Vec<ListAction> {
        let now = host::now_ms();
        let mut actions = Vec::new();
        host::consume(|e| match e {
            InputEvent::Key { vk: code, down: true, mods, .. } => {
                match NavKey::from_vk(*code, *mods).and_then(|k| self.key(bounds, k, *mods)) {
                    Some(a) => {
                        actions.push(a);
                        true
                    }
                    None => false,
                }
            }
            // The Space key already acted; its text is swallowed unless a
            // type-ahead is under way (« Photos de vacances »).
            InputEvent::Text(s) if s.trim().is_empty() && self.typeahead.idle(now) => true,
            InputEvent::Text(s) => {
                if let Some(a) = self.type_ahead(bounds, s, now) {
                    actions.push(a);
                }
                true
            }
            _ => false,
        });
        actions
    }

    // ── Scrolling ────────────────────────────────────────────────────────

    /// Clamps [`ListView::scroll`] into `0..=max_scroll` — after a resize, or
    /// after items were removed.
    pub fn clamp_scroll(&mut self, bounds: Rect) {
        self.scroll = self.scroll.clamp(0.0, self.max_scroll(bounds));
    }

    /// Scrolls by `dy` DIP (positive = down, the wheel's web sign), clamped.
    pub fn scroll_by(&mut self, bounds: Rect, dy: f32) {
        self.scroll += dy;
        self.clamp_scroll(bounds);
    }

    /// Scrolls the least that brings item `index` fully into view.
    pub fn ensure_visible(&mut self, bounds: Rect, index: usize) {
        let body = self.body(bounds);
        let viewport = body.bottom - body.top;
        let (top, bottom) = match self.inner.view {
            View::LargeIcon => {
                let (_, th) = self.tile_size();
                let row = index / self.tiles_per_row(bounds).max(1);
                let top = TILE_PAD + row as f32 * (th + TILE_GAP);
                // The first and last rows bring their outer padding along.
                let top = if row == 0 { 0.0 } else { top };
                (top, top + th + TILE_PAD)
            }
            _ => {
                let rh = self.row_height();
                (index as f32 * rh, (index + 1) as f32 * rh)
            }
        };
        self.scroll = nearest_scroll(self.scroll, viewport, top, bottom);
        self.clamp_scroll(bounds);
    }

    /// The overlay scroll bar's gutter, or `None` when everything fits.
    pub fn scrollbar_rail(&self, bounds: Rect) -> Option<Rect> {
        overlay_bar(self.body(bounds), bounds, self.corner_radius, self.content_height(bounds), self.scroll, self.scrollbar_hot)
            .map(|(rail, _)| rail)
    }

    /// The overlay bar's thumb, or `None` when everything fits.
    pub fn scrollbar_thumb(&self, bounds: Rect) -> Option<Rect> {
        overlay_bar(self.body(bounds), bounds, self.corner_radius, self.content_height(bounds), self.scroll, self.scrollbar_hot)
            .map(|(rail, bar)| bar.thumb_rect(rail))
    }

    /// Which part of the overlay bar `(x, y)` lands on.
    pub fn scrollbar_part_at(&self, bounds: Rect, x: f32, y: f32) -> Option<ScrollPart> {
        let (rail, bar) = overlay_bar(
            self.body(bounds),
            bounds,
            self.corner_radius,
            self.content_height(bounds),
            self.scroll,
            self.scrollbar_hot,
        )?;
        bar.part_at(rail, x, y)
    }

    /// Scrolls so the thumb's top edge lands at `thumb_top` — a thumb drag,
    /// where the caller keeps the grab offset.
    pub fn scroll_to_thumb(&mut self, bounds: Rect, thumb_top: f32) {
        if let Some((rail, bar)) = overlay_bar(
            self.body(bounds),
            bounds,
            self.corner_radius,
            self.content_height(bounds),
            self.scroll,
            self.scrollbar_hot,
        ) {
            self.scroll = scroll_for_thumb(&bar, rail, thumb_top, self.max_scroll(bounds));
        }
    }

    /// What a press on the overlay bar does to the scroll offset: a page on
    /// the track, a row on an arrow. Returns the part so a caller can start a
    /// thumb drag on [`ScrollPart::Thumb`].
    pub fn scrollbar_press(&mut self, bounds: Rect, x: f32, y: f32) -> Option<ScrollPart> {
        let part = self.scrollbar_part_at(bounds, x, y)?;
        let body = self.body(bounds);
        let page = (body.bottom - body.top).max(self.row_height());
        match part {
            ScrollPart::ArrowLow => self.scroll_by(bounds, -self.row_height()),
            ScrollPart::ArrowHigh => self.scroll_by(bounds, self.row_height()),
            ScrollPart::PageLow => self.scroll_by(bounds, -page),
            ScrollPart::PageHigh => self.scroll_by(bounds, page),
            ScrollPart::Thumb => {}
        }
        Some(part)
    }

    // ── Inline rename and context menu anchors ───────────────────────────

    /// The box item `index`'s name occupies — the Details name cell, or a
    /// tile's name row — where an F2 rename field goes.
    pub fn name_rect(&self, bounds: Rect, index: usize) -> Rect {
        let r = self.item_rect(bounds, index, self.scroll);
        match self.inner.view {
            View::LargeIcon => {
                let side = r.right - r.left;
                Rect::new(r.left + space::XS, r.top + side, r.right - space::XS, r.bottom - space::XS)
            }
            _ => {
                let xs = self.column_x_offsets(bounds);
                let left = xs.first().copied().unwrap_or(r.left) + self.name_left() - RENAME_PAD_X;
                let right = xs.get(1).map_or(r.right, |x| x - CELL_GAP).max(left);
                Rect::new(left, r.top + space::XS, right, r.bottom - space::XS)
            }
        }
    }

    /// Where a keyboard-opened context menu (the menu key, Shift+F10) should
    /// appear: under the cursor item's name, the way Explorer places it.
    pub fn context_anchor(&self, bounds: Rect) -> Option<(f32, f32)> {
        let i = self.cursor_index()?;
        let r = self.name_rect(bounds, i);
        Some((r.left, r.bottom))
    }

    /// Paints the inline rename field over item `index`'s name, holding
    /// `text`, with a caret when `caret` is true. The caller owns the edit
    /// (typed text, Backspace, Enter to commit, Escape to cancel); this is its
    /// look — `@ui/Input` with its focus ring — clipped to the body.
    pub fn paint_rename(&self, c: &dyn Canvas, bounds: Rect, index: usize, text: &str, caret: bool) {
        c.push_clip(&self.body(bounds));
        paint_inline_editor(c, self.name_rect(bounds, index), text, caret);
        c.pop_clip();
    }

    // ── Painting ─────────────────────────────────────────────────────────

    /// The icon name item `index` selects, if any.
    fn icon_of(&self, index: usize) -> Option<&'static str> {
        let idx = self.inner.items.get(index)?.image_index;
        usize::try_from(idx).ok().and_then(|i| self.image_list.get(i)).copied()
    }

    /// Whether item `index` is a container — see [`ListView::container_icon`].
    fn is_container(&self, index: usize) -> bool {
        match (self.container_icon, self.icon_of(index)) {
            (Some(folder), Some(icon)) => folder == icon,
            _ => false,
        }
    }

    /// A check box, drawn as the web `@ui/Checkbox` the product's tiles wear:
    /// filled and stroked with the accent plus a white tick when checked, the
    /// card surface inside a `card_stroke` border when not.
    fn paint_check(&self, c: &dyn Canvas, box_rect: Rect, checked: bool) {
        let t = c.theme();
        if checked {
            c.fill_rounded(&box_rect, radius::SM, &t.accent);
            c.stroke_rounded_w(&box_rect, radius::SM, &t.accent, CHECK_BORDER);
            c.vector_icon("Check", &box_rect, control::CHECK_TICK, &t.accent_foreground);
        } else {
            c.fill_rounded(&box_rect, radius::SM, &t.card_background);
            c.stroke_rounded_w(&box_rect, radius::SM, &t.card_stroke, CHECK_BORDER);
        }
    }

    /// The Details header band, as the web explorer's `DetailsHeader.tsx`
    /// draws it: `bg-white` (the panel ground), one `text-xs
    /// text-text-secondary` label per column that darkens to `text-primary`
    /// on hover, the 12 DIP sort chevron `gap-1` after the sorted column's
    /// label in the same colour, and the single `border-b border-border` rule.
    ///
    /// Every label is `truncate`d: a narrow column ellipsises its label and
    /// keeps room for its chevron rather than drawing into its neighbour.
    fn paint_header(&self, c: &dyn Canvas, bounds: Rect) {
        let t = c.theme();
        let f = c.formats();
        let h = self.header_height();
        if h <= 0.0 {
            return;
        }
        let band = Rect::new(bounds.left, bounds.top, bounds.right, bounds.top + h);
        c.fill_rounded(&band, 0.0, &t.layer_background);

        let xs = self.column_x_offsets(bounds);
        let name_left = self.name_left();
        c.push_clip(&band);
        for (i, col) in self.inner.columns.iter().enumerate() {
            let left = if i == 0 { xs[i] + name_left } else { xs[i] };
            let right = (xs[i + 1] - CELL_GAP).max(left);
            let colour =
                if self.hot_column == Some(i) { &t.text_primary } else { &t.text_secondary };
            let sorted = i == self.sort_column && self.inner.sorting != SortOrder::None;
            let glyph_room = if sorted { SORT_GAP + SORT_GLYPH } else { 0.0 };
            // Column 0 is always left-aligned: the Win32 header cannot align
            // its first column, and the replica's painter enforces the same.
            let align = align_of(i, col);
            let w = c.measure(&col.text, &f.body).min((right - left - glyph_room).max(0.0));
            let label_left = match align {
                HorizontalAlignment::Right => (right - glyph_room - w).max(left),
                HorizontalAlignment::Center => {
                    (left + (right - left - glyph_room - w) / 2.0).max(left)
                }
                _ => left,
            };
            // One DIP of slack so a label measured to fit exactly is not
            // ellipsised by DirectWrite's own rounding.
            let limit = (right - glyph_room).max(label_left);
            let cell = Rect::new(label_left, band.top, (label_left + w + 1.0).min(limit), band.bottom);
            c.text_ellipsis(&col.text, &cell, &f.body, colour);

            if sorted {
                let ax = (label_left + w + SORT_GAP).min(right - SORT_GLYPH).max(left);
                let arrow = Rect::new(ax, band.top, ax + SORT_GLYPH, band.bottom);
                let glyph = if self.inner.sorting == SortOrder::Ascending {
                    "ChevronUp"
                } else {
                    "ChevronDown"
                };
                c.vector_icon(glyph, &arrow, SORT_GLYPH, colour);
            }
        }
        c.pop_clip();
        let rule = Rect::new(band.left, band.bottom - 1.0, band.right, band.bottom);
        c.fill_rounded(&rule, 0.0, &t.card_stroke);
    }

    /// The Details body — **only the visible range**, clipped to it.
    fn paint_details(&self, c: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = c.theme();
        let f = c.formats();
        let body = self.body(bounds);
        let xs = self.column_x_offsets(bounds);
        let name_left = self.name_left();
        let primary = if state.disabled { &t.text_tertiary } else { &t.text_primary };

        let selected = |i: usize| self.inner.items.get(i).is_some_and(|it| it.selected);
        let cursor = if state.focused { self.cursor_index() } else { None };

        c.push_clip(&body);
        let owner = (self.inner.owner_draw && owner_draw::has_handler()).then(|| Graphics::new(c));
        for i in self.visible_range(bounds, self.scroll) {
            let row = self.item_rect(bounds, i, self.scroll);
            let Some(item) = self.inner.items.get(i) else {
                continue;
            };
            // `OwnerDraw`: the handler draws the row (`DrawDefault` gives it back).
            if let Some(g) = &owner {
                let st = self.owner_state(i, item.selected, cursor == Some(i), state);
                let mut e = DrawItemEventArgs::new(g, "ListView", Some(i), row, st, item.text.as_str());
                if owner_draw::draw_item(&mut e) {
                    continue;
                }
            }

            // The web explorer's row states (`FileRowBase` / `FolderRowBase`
            // in `storage-explorer/rows.tsx`, accents in `rowStyles.ts`), in
            // its order of precedence: selected, cursor, hover. Rows are
            // square-cornered bands there.
            if item.selected {
                // `bg-[#e8f0fe]` plus the 2 DIP inset ring, merged with the
                // selected neighbours so a run reads as one frame.
                c.fill_rounded(&row, 0.0, &t.selected_card);
                let merge_top = i > 0 && selected(i - 1);
                let merge_bottom = selected(i + 1);
                let r = SELECTION_RING;
                c.fill_rounded(&Rect::new(row.left, row.top, row.left + r, row.bottom), 0.0, &t.accent);
                c.fill_rounded(&Rect::new(row.right - r, row.top, row.right, row.bottom), 0.0, &t.accent);
                if !merge_top {
                    c.fill_rounded(&Rect::new(row.left, row.top, row.right, row.top + r), 0.0, &t.accent);
                }
                if !merge_bottom {
                    c.fill_rounded(&Rect::new(row.left, row.bottom - r, row.right, row.bottom), 0.0, &t.accent);
                }
            } else if cursor == Some(i) {
                // `focused ? 'bg-surface-1'` + `inset 3px 0 0 0` accent at 40 %.
                c.fill_rounded(&row, 0.0, &t.card_background);
                let bar = Rect::new(row.left, row.top, row.left + CURSOR_BAR, row.bottom);
                c.fill_rounded(&bar, 0.0, &faded(&t.accent, CURSOR_BAR_ALPHA));
            } else if self.hot_index == Some(i) && !state.disabled {
                // `hover:bg-surface-1`.
                c.fill_rounded(&row, 0.0, &t.card_background);
            }

            let mut x = row.left + ICON_GUTTER;
            if self.inner.check_boxes {
                let cy = (row.top + row.bottom) / 2.0;
                let bx = Rect::new(
                    x,
                    cy - control::CHECK_BOX / 2.0,
                    x + control::CHECK_BOX,
                    cy + control::CHECK_BOX / 2.0,
                );
                self.paint_check(c, bx, item.checked);
                x += control::CHECK_BOX + control::CHECK_GAP;
            }
            if let Some(icon) = self.icon_of(i) {
                let cy = (row.top + row.bottom) / 2.0;
                let ir = Rect::new(x, cy - ICON_SIZE / 2.0, x + ICON_SIZE, cy + ICON_SIZE / 2.0);
                c.vector_icon(icon, &ir, ICON_SIZE, &t.text_secondary);
            }

            for (col, header) in self.inner.columns.iter().enumerate() {
                let left = if col == 0 { xs[col] + name_left } else { xs[col] };
                let right = (xs[col + 1] - CELL_GAP).max(left);
                let cell = Rect::new(left, row.top, right, row.bottom);
                // `FileRow`: the name is 14 / `text_primary`, every metadata
                // cell is 12 / `text_tertiary` — one step paler.
                let (format, colour) =
                    if col == 0 { (&f.body, primary) } else { (&f.caption, &t.text_tertiary) };
                cell_text(c, item.cell(col), &cell, format, colour, align_of(col, header));
            }

            // `GridLines` is a replica flag the product's DataTable never sets
            // (its only rule is under the header), so it is drawn in the same
            // hairline token as every other frame: `card_stroke`.
            if self.inner.grid_lines {
                let h = Rect::new(body.left, row.bottom - 1.0, body.right, row.bottom);
                c.fill_rounded(&h, 0.0, &t.card_stroke);
                for sx in xs.iter().skip(1) {
                    c.fill_rounded(&Rect::new(*sx - 1.0, row.top, *sx, row.bottom), 0.0, &t.card_stroke);
                }
            }
        }
        c.pop_clip();
    }

    /// The icon view — the product's grid tile, verbatim from
    /// `grid_layout_page.rs`: a framed card at `--radius-xl` whose fill says
    /// what the item is and how it is selected, the icon box, the selection
    /// check box, and the name under it.
    ///
    /// The name is **one centred ellipsised line**, where the product wraps it
    /// onto at most two (`text_wrap_ellipsis`). That method is not a [`Canvas`]
    /// primitive, so the two-line form is not reachable from this layer.
    /// The owner-draw state of item `i`.
    fn owner_state(&self, i: usize, selected: bool, cursor: bool, state: WidgetState) -> DrawItemState {
        DrawItemState::NONE
            .with(DrawItemState::SELECTED, selected)
            .with(DrawItemState::FOCUS, cursor)
            .with(DrawItemState::HOT_LIGHT, self.hot_index == Some(i) && !state.disabled)
            .with(DrawItemState::DISABLED, state.disabled)
    }

    fn paint_large_icons(&self, c: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = c.theme();
        let f = c.formats();
        let body = self.body(bounds);
        let primary = if state.disabled { &t.text_tertiary } else { &t.text_primary };

        c.push_clip(&body);
        let owner = (self.inner.owner_draw && owner_draw::has_handler()).then(|| Graphics::new(c));
        let cursor = if state.focused { self.cursor_index() } else { None };
        for i in self.visible_range(bounds, self.scroll) {
            let tile = self.item_rect(bounds, i, self.scroll);
            let Some(item) = self.inner.items.get(i) else {
                continue;
            };
            if let Some(g) = &owner {
                let st = self.owner_state(i, item.selected, cursor == Some(i), state);
                let mut e = DrawItemEventArgs::new(g, "ListView", Some(i), tile, st, item.text.as_str());
                if owner_draw::draw_item(&mut e) {
                    continue;
                }
            }
            let hot = self.hot_index == Some(i);
            let container = self.is_container(i);
            let fill = if item.selected {
                if container {
                    &t.selected_folder
                } else {
                    &t.selected_card
                }
            } else if hot {
                &t.row_hover
            } else if container {
                &t.card_preview_background
            } else {
                &t.card_background
            };
            c.fill_rounded(&tile, radius::XL, fill);
            c.stroke_rounded(&tile, radius::XL, &t.card_stroke);
            if item.selected {
                c.stroke_rounded_w(&tile, radius::XL, &t.accent, TILE_SELECTION_BORDER);
            } else if state.focused && self.cursor_index() == Some(i) {
                // The keyboard cursor on a tile it did not select: the same
                // frame at the web cursor's 40 % accent (`rowStyles.ts`).
                c.stroke_rounded_w(
                    &tile,
                    radius::XL,
                    &faded(&t.accent, CURSOR_BAR_ALPHA),
                    TILE_SELECTION_BORDER,
                );
            }

            let side = tile.right - tile.left;
            let icon_box = Rect::new(tile.left, tile.top, tile.right, tile.top + side);
            if let Some(icon) = self.icon_of(i) {
                c.vector_icon(icon, &icon_box, side - TILE_ICON_INSET, &t.text_secondary);
            }
            // `SelectionCheckbox`: top-left of the preview box, visible when
            // selected or hovered (`UpdateCheckboxVisibility`).
            if self.inner.check_boxes && (item.selected || hot) {
                let l = icon_box.left + CHECK_MARGIN;
                let tp = icon_box.top + CHECK_MARGIN;
                let bx = Rect::new(l, tp, l + control::CHECK_BOX, tp + control::CHECK_BOX);
                self.paint_check(c, bx, item.checked || item.selected);
            }

            let name = Rect::new(
                tile.left + space::XS,
                icon_box.bottom,
                tile.right - space::XS,
                tile.bottom - space::SM,
            );
            c.text_ellipsis_center(&item.text, &name, &f.caption, primary);
        }
        c.pop_clip();
    }
}

impl Deref for ListView {
    type Target = kc::ListView;
    fn deref(&self) -> &kc::ListView {
        &self.inner
    }
}
impl DerefMut for ListView {
    fn deref_mut(&mut self) -> &mut kc::ListView {
        &mut self.inner
    }
}

impl Widget for ListView {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    /// Details measures as the declared column widths by the header plus the
    /// rows. The icon views have no measurement of their own — their layout
    /// depends on the width they are given — so they fall through to the
    /// replica's answer, which is the control's current box.
    fn measure(&self, canvas: &dyn Canvas) -> Size {
        match self.inner.view {
            View::Details => Size::new(
                self.total_column_width(),
                self.header_height() + self.count() as f32 * self.row_height(),
            ),
            _ => self.inner.preferred_size(canvas),
        }
    }

    /// Paints into the `bounds` **argument** — never the model's own rectangle.
    ///
    /// Only `Details` and `LargeIcon` have Kubuno pixels: they are the two
    /// views the product uses. `List`, `SmallIcon` and `Tile` paint the ground
    /// and nothing else, deliberately — approximating them from the replica's
    /// WinForms tiling would put layouts on screen the design system has never
    /// described.
    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        // Rule: every widget lands on an opaque background. What follows
        // may overpaint most of it (a fill, a card, a track); this makes
        // sure the parts that stay uncovered still read as an opaque
        // surface, so a widget never shows what is behind it.
        let rounded = self.corner_radius > 0.0;
        if rounded {
            canvas.push_clip_rounded(&bounds, self.corner_radius);
        }
        canvas.fill_rounded(&bounds, self.corner_radius, &canvas.current_bg());
        let t = canvas.theme();
        // The module panel the file area sits on (`--color-surface-0`).
        canvas.fill_rounded(&bounds, self.corner_radius, &t.layer_background);
        match self.inner.view {
            View::Details => {
                self.paint_header(canvas, bounds);
                self.paint_details(canvas, bounds, state);
            }
            View::LargeIcon => self.paint_large_icons(canvas, bounds, state),
            View::List | View::SmallIcon | View::Tile => {}
        }
        // Overflow: the overlay bar, over the rows as in Drive's file area.
        if let Some((rail, bar)) = overlay_bar(
            self.body(bounds),
            bounds,
            self.corner_radius,
            self.content_height(bounds),
            self.scroll,
            self.scrollbar_hot,
        ) {
            let bs = WidgetState::REST.hot(self.scrollbar_hot).disabled(state.disabled);
            bar.paint(canvas, rail, bs);
        }
        // `:focus-visible`: the accent ring, inward, around the whole view —
        // the cursor row alone does not say « this list has the keyboard ».
        if state.show_focus_ring() {
            let inset = FOCUS_RING / 2.0;
            let r = Rect::new(
                bounds.left + inset,
                bounds.top + inset,
                bounds.right - inset,
                bounds.bottom - inset,
            );
            canvas.stroke_rounded_w(&r, (self.corner_radius - inset).max(0.0), &t.accent, FOCUS_RING);
        }
        if rounded {
            canvas.pop_clip_rounded();
        }
    }

    fn type_name(&self) -> &'static str {
        "ListView"
    }
}

/// The alignment a Details cell in column `index` actually paints with.
///
/// Column 0 is forced `Left`: the Win32 header control cannot align its first
/// column, so WinForms silently ignores `ColumnHeader[0].TextAlign` — the same
/// rule the replica's painter enforces, kept here so a Kubuno list and a
/// toolkit list put the same text in the same place.
fn align_of(index: usize, column: &ColumnHeader) -> HorizontalAlignment {
    if index == 0 {
        HorizontalAlignment::Left
    } else {
        column.text_align
    }
}

// =============================================================================
//  TreeView
// =============================================================================

/// A Kubuno tree — [`kc::TreeView`] painted as the product's lateral pane.
///
/// The reference is `kubuno-drive-desktop-app-controls/src/sidebar/sidebar_view.rs`, which is
/// the shipping rendering of exactly this control: a full-pill row, the active
/// one filled `accent_light` with a `text_nav_active` label, hover
/// `control_fill_hover`, a 20 DIP icon then a `space::MD` gap, and the chevron
/// in its own 20 DIP box to the left of the icon.
pub struct TreeView {
    inner: kc::TreeView,

    /// The pixel scroll offset — same reason as [`ListView::scroll`].
    pub scroll: f32,
    /// The **visible row** the pointer is over.
    pub hot_row: Option<usize>,
    /// The vector-icon names a node's `ImageIndex` selects — see
    /// [`ListView::image_list`].
    pub image_list: Vec<&'static str>,
    /// The keyboard cursor (the WAI-ARIA tree's focused `treeitem`): the node
    /// the arrows move from, Enter activates and F2 renames. `None` falls back
    /// to [`kc::TreeView::selected_path`].
    pub cursor: Option<NodePath>,
    /// Ctrl/Shift extend the selection. The replica is WinForms' single
    /// `SelectedNode`, which stays the *primary* selection
    /// ([`kc::TreeView::selected_path`]); the extra nodes live in
    /// [`TreeView::selection`]. Off by default, as in WinForms.
    pub multi_select: bool,
    /// The nodes selected besides `selected_path` in a multiple selection —
    /// see [`TreeView::selected_paths`] for the whole set.
    pub selection: Vec<NodePath>,
    /// The rows that have their own height (a node path → its height, DIP); every
    /// other row is [`TreeView::row_height`] high.
    pub node_heights: std::collections::HashMap<NodePath, f32>,
    /// The pointer is over the overlay scroll bar — see
    /// [`ListView::scrollbar_hot`].
    pub scrollbar_hot: bool,
    /// Where a Shift+click or Shift+arrow extends from.
    anchor: Option<NodePath>,
    typeahead: TypeAhead,
}

/// What a key, a type-ahead or a gesture did to a [`TreeView`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TreeAction {
    /// The cursor moved to this node (and the selection may have followed).
    CursorMoved(NodePath),
    /// The selection changed without the cursor moving (Space, Ctrl+A).
    SelectionChanged,
    /// Right Arrow opened this node.
    Expanded(NodePath),
    /// Left Arrow closed this node.
    Collapsed(NodePath),
    /// Enter on this node.
    Activate(NodePath),
    /// F2 on this node — start an inline rename.
    Rename(NodePath),
    /// Delete on a non-empty selection; `true` for Shift+Delete.
    Delete { permanent: bool },
    /// The context-menu key or Shift+F10 on this node.
    ContextMenu(NodePath),
}

impl Default for TreeView {
    /// The replica's defaults, with the two metrics the Kubuno system answers
    /// differently: a 36 DIP row (`shape::height::SIDEBAR_ROW`) instead of the
    /// toolkit's font-derived 19, and a 16 DIP indent (`INDENT_PER_LEVEL`)
    /// instead of 19. Both are the replica's own fields, set — not shadowed.
    fn default() -> Self {
        let mut inner = kc::TreeView::new();
        inner.item_height = TREE_ROW;
        inner.indent = TREE_INDENT;
        inner.border_style = kubuno_desktop_controls::enums::BorderStyle::None;
        // The product's nav tree draws no branch lines; the flag stays
        // available (see `paint`), it is simply off by default here.
        inner.show_lines = false;
        Self {
            inner,
            scroll: 0.0,
            hot_row: None,
            image_list: Vec::new(),
            cursor: None,
            multi_select: false,
            selection: Vec::new(),
            node_heights: std::collections::HashMap::new(),
            scrollbar_hot: false,
            anchor: None,
            typeahead: TypeAhead::default(),
        }
    }
}

impl TreeView {
    pub fn new() -> Self {
        Self::default()
    }

    /// The flattened visible rows — [`kc::TreeView::visible_rows`], the
    /// replica's own tested flattening.
    pub fn rows(&self) -> Vec<VisibleRow> {
        self.inner.visible_rows()
    }

    /// A row's height — the replica's `ItemHeight`.
    pub fn row_height(&self) -> f32 {
        self.inner.item_height.max(1) as f32
    }

    /// The height of visible row `row`: its own ([`TreeView::node_heights`]), else the tree's.
    fn height_of(&self, row: &VisibleRow) -> f32 {
        self.node_heights.get(&row.path).copied().filter(|h| *h > 0.0).unwrap_or_else(|| self.row_height())
    }

    /// The top of every visible row, then the total height, when some rows have their own
    /// height; `None` when every row is [`TreeView::row_height`] high.
    fn offsets(&self, rows: &[VisibleRow]) -> Option<Vec<f32>> {
        if self.node_heights.is_empty() {
            return None;
        }
        let mut out = Vec::with_capacity(rows.len() + 1);
        let mut y = 0.0;
        for r in rows {
            out.push(y);
            y += self.height_of(r);
        }
        out.push(y);
        Some(out)
    }

    /// The top and bottom of visible row `index` in the content (before scrolling).
    fn span(&self, offsets: Option<&[f32]>, index: usize) -> (f32, f32) {
        match offsets {
            Some(o) => {
                let last = o.len().saturating_sub(1);
                (o[index.min(last)], o[(index + 1).min(last)])
            }
            None => {
                let rh = self.row_height();
                (index as f32 * rh, (index + 1) as f32 * rh)
            }
        }
    }

    /// The row range that intersects `bounds` at `scroll`, partial rows at both
    /// ends included.
    ///
    /// It has to flatten the tree to know how many rows there are; see the
    /// module docs on what that costs and why the replica cannot answer it
    /// without walking.
    pub fn visible_range(&self, bounds: Rect, scroll: f32) -> Range<usize> {
        self.range_of(bounds, scroll, self.rows().len())
    }

    /// [`TreeView::visible_range`] over an already-flattened row count, so the
    /// painter flattens once per frame instead of twice.
    fn range_of(&self, bounds: Rect, scroll: f32, count: usize) -> Range<usize> {
        if !self.node_heights.is_empty() {
            let rows = self.rows();
            return self.range_in(bounds, scroll, self.offsets(&rows).as_deref(), count);
        }
        self.range_in(bounds, scroll, None, count)
    }

    /// [`TreeView::range_of`] with the rows' offsets already computed.
    fn range_in(&self, bounds: Rect, scroll: f32, offsets: Option<&[f32]>, count: usize) -> Range<usize> {
        if let Some(o) = offsets {
            let h = bounds.bottom - bounds.top;
            if count == 0 || h <= 0.0 {
                return 0..0;
            }
            let tops = &o[..count.min(o.len().saturating_sub(1))];
            let first = tops.partition_point(|&t| t <= scroll).saturating_sub(1);
            let last = tops.partition_point(|&t| t < scroll + h).min(count);
            return first.min(last)..last;
        }
        let rh = self.row_height();
        let h = bounds.bottom - bounds.top;
        if count == 0 || h <= 0.0 || rh <= 0.0 {
            return 0..0;
        }
        let first = (scroll / rh).floor().max(0.0) as usize;
        let last = (((scroll + h) / rh).ceil().max(0.0) as usize).min(count);
        first.min(last)..last
    }

    /// The band visible row `index` occupies at `scroll`.
    pub fn row_rect(&self, bounds: Rect, index: usize, scroll: f32) -> Rect {
        let offsets = if self.node_heights.is_empty() { None } else { self.offsets(&self.rows()) };
        self.row_rect_in(bounds, index, scroll, offsets.as_deref())
    }

    /// [`TreeView::row_rect`] with the rows' offsets already computed.
    fn row_rect_in(&self, bounds: Rect, index: usize, scroll: f32, offsets: Option<&[f32]>) -> Rect {
        let (top, bottom) = self.span(offsets, index);
        Rect::new(bounds.left, bounds.top + top - scroll, bounds.right, bounds.top + bottom - scroll)
    }

    /// The visible row under `(x, y)`, or `None` past the last one.
    pub fn node_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        if !bounds.contains(x, y) {
            return None;
        }
        let rh = self.row_height();
        let offset = y - bounds.top + self.scroll;
        if offset < 0.0 {
            return None;
        }
        let rows = self.rows();
        if let Some(o) = self.offsets(&rows) {
            let index = o.partition_point(|&t| t <= offset).saturating_sub(1);
            return (index < rows.len()).then_some(index);
        }
        let index = (offset / rh).floor() as usize;
        (index < rows.len()).then_some(index)
    }

    /// The node path visible row `index` stands for.
    pub fn path_at(&self, index: usize) -> Option<NodePath> {
        self.rows().get(index).map(|r| r.path.clone())
    }

    /// The chevron box of a row at `depth` — a 20 DIP square, vertically
    /// centred, at the row's own indent level.
    pub fn chevron_rect(&self, row: Rect, depth: usize) -> Rect {
        let x = row.left + TREE_PAD + depth as f32 * self.inner.indent as f32;
        let cy = (row.top + row.bottom) / 2.0;
        Rect::new(x, cy - CHEVRON_BOX / 2.0, x + CHEVRON_BOX, cy + CHEVRON_BOX / 2.0)
    }

    /// The visible row whose **chevron** `(x, y)` lands on — `None` for a leaf,
    /// for a row whose chevron is hidden (`ShowPlusMinus` off) or for a point
    /// anywhere else on the row.
    ///
    /// This is what separates "expand me" from "select me": the two live on the
    /// same row and only the box tells them apart.
    pub fn chevron_hit(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        if !self.inner.show_plus_minus {
            return None;
        }
        let rows = self.rows();
        let index = self.node_at(bounds, x, y)?;
        let row = rows.get(index)?;
        if !row.has_children {
            return None;
        }
        let rect = self.chevron_rect(self.row_rect(bounds, index, self.scroll), row.depth);
        rect.contains(x, y).then_some(index)
    }

    /// Flips one node's expand flag, through the replica's own path resolver.
    pub fn toggle_expanded(&mut self, path: &[usize]) {
        let now = kc::TreeView::node_at(&self.inner.nodes, path).is_some_and(|n| n.expanded);
        self.set_expanded(path, !now);
    }

    /// Sets one node's expand flag.
    ///
    /// Collapsing a node that hides the cursor or the primary selection moves
    /// them onto the collapsed node — WinForms moves `SelectedNode` there, and
    /// the ARIA tree keeps its focus on a visible item.
    pub fn set_expanded(&mut self, path: &[usize], value: bool) {
        if let Some(n) = kc::TreeView::node_at_mut(&mut self.inner.nodes, path) {
            n.expanded = value;
        }
        if !value {
            let hidden = |p: &Option<NodePath>| {
                p.as_ref().is_some_and(|p| p.len() > path.len() && p.starts_with(path))
            };
            if hidden(&self.cursor) {
                self.cursor = Some(path.to_vec());
            }
            if hidden(&self.inner.selected_path) {
                self.inner.selected_path = Some(path.to_vec());
            }
            self.selection.retain(|p| !(p.len() > path.len() && p.starts_with(path)));
        }
    }

    /// Selects the node at a visible row index (`SelectedNode`) — and makes
    /// it the whole selection, the cursor and the anchor.
    pub fn select_row(&mut self, index: usize) {
        if let Some(p) = self.path_at(index) {
            self.select_only(p);
        } else {
            self.inner.selected_path = None;
        }
    }

    // ── Selection ────────────────────────────────────────────────────────

    /// `path` becomes the whole selection, the cursor and the anchor.
    fn select_only(&mut self, path: NodePath) {
        self.selection.clear();
        self.inner.selected_path = Some(path.clone());
        self.anchor = Some(path.clone());
        self.cursor = Some(path);
    }

    /// Whether `path` is selected — the primary `SelectedNode` or one of the
    /// extra nodes of a multiple selection.
    pub fn is_path_selected(&self, path: &[usize]) -> bool {
        self.inner.selected_path.as_deref() == Some(path)
            || self.selection.iter().any(|p| p.as_slice() == path)
    }

    /// Every selected node, primary first, without duplicates.
    pub fn selected_paths(&self) -> Vec<NodePath> {
        let mut out: Vec<NodePath> = self.inner.selected_path.iter().cloned().collect();
        for p in &self.selection {
            if !out.contains(p) {
                out.push(p.clone());
            }
        }
        out
    }

    /// Ctrl: flips `path` in or out of the selection; it becomes the cursor
    /// and the anchor either way.
    fn toggle_path(&mut self, path: NodePath) {
        if self.is_path_selected(&path) {
            self.selection.retain(|p| p != &path);
            if self.inner.selected_path.as_ref() == Some(&path) {
                self.inner.selected_path = self.selection.last().cloned();
            }
        } else {
            if let Some(primary) = self.inner.selected_path.take() {
                if !self.selection.contains(&primary) {
                    self.selection.push(primary);
                }
            }
            self.selection.push(path.clone());
            self.inner.selected_path = Some(path.clone());
        }
        self.anchor = Some(path.clone());
        self.cursor = Some(path);
    }

    /// Shift: the selection becomes every visible row between the anchor and
    /// `to` (inclusive); the anchor stays, `to` becomes primary and cursor.
    fn select_range_rows(&mut self, rows: &[VisibleRow], to: usize) {
        let Some(target) = rows.get(to).map(|r| r.path.clone()) else {
            return;
        };
        let from = self
            .anchor
            .as_ref()
            .or(self.cursor.as_ref())
            .or(self.inner.selected_path.as_ref())
            .and_then(|a| rows.iter().position(|r| &r.path == a))
            .unwrap_or(to);
        let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
        self.selection = rows[lo..=hi].iter().map(|r| r.path.clone()).collect();
        if self.anchor.is_none() {
            self.anchor = rows.get(from).map(|r| r.path.clone());
        }
        self.inner.selected_path = Some(target.clone());
        self.cursor = Some(target);
    }

    /// A click on visible row `index` with the two modifiers that change what
    /// it means — the same rules as [`ListView::click`]: plain selects only it,
    /// Ctrl toggles it, Shift selects the run from the anchor. Without
    /// [`TreeView::multi_select`] every click is a plain one.
    pub fn click(&mut self, index: usize, ctrl: bool, shift: bool) {
        let rows = self.rows();
        let Some(path) = rows.get(index).map(|r| r.path.clone()) else {
            return;
        };
        if shift && self.multi_select {
            self.select_range_rows(&rows, index);
        } else if ctrl && self.multi_select {
            self.toggle_path(path);
        } else {
            self.select_only(path);
        }
    }

    /// [`TreeView::click`] with the host's modifiers.
    pub fn click_mods(&mut self, index: usize, mods: Modifiers) {
        self.click(index, mods.ctrl, mods.shift);
    }

    /// A right click on visible row `index`: a selected node keeps the
    /// selection, another one becomes it (see [`ListView::context_click`]).
    pub fn context_click(&mut self, index: usize) {
        let Some(path) = self.path_at(index) else {
            return;
        };
        if !self.is_path_selected(&path) {
            self.select_only(path.clone());
        }
        self.cursor = Some(path);
    }

    // ── Keyboard ─────────────────────────────────────────────────────────

    /// The visible row the keyboard acts on: the cursor, else the primary
    /// selection.
    fn cursor_row(&self, rows: &[VisibleRow]) -> Option<usize> {
        let target = self.cursor.as_ref().or(self.inner.selected_path.as_ref())?;
        rows.iter().position(|r| &r.path == target)
    }

    /// The visible row index of the cursor — see [`TreeView::cursor`].
    pub fn cursor_index(&self) -> Option<usize> {
        self.cursor_row(&self.rows())
    }

    /// How many rows one PageUp / PageDown moves by. Never zero.
    pub fn page_rows(&self, bounds: Rect) -> usize {
        ((bounds.bottom - bounds.top).max(0.0) / self.row_height()).floor().max(1.0) as usize
    }

    /// Moves the cursor to visible row `to` as an arrow key with `mods` does
    /// (plain selects, Shift extends, Ctrl only moves), then scrolls it into
    /// view.
    fn move_cursor(&mut self, bounds: Rect, rows: &[VisibleRow], to: usize, mods: Modifiers) {
        let Some(path) = rows.get(to).map(|r| r.path.clone()) else {
            return;
        };
        if mods.shift && self.multi_select {
            self.select_range_rows(rows, to);
        } else if !mods.ctrl || !self.multi_select {
            self.select_only(path.clone());
        }
        self.cursor = Some(path);
        self.ensure_visible(bounds, to);
    }

    /// Reacts to one navigation key, following the WAI-ARIA tree view
    /// pattern: Up/Down move a row; Right opens a closed node, or moves to
    /// the first child of an open one; Left closes an open node, or moves to
    /// the parent; Home/End and PageUp/PageDown jump; Enter activates; F2
    /// renames; Space selects (Ctrl+Space toggles). Returns `None` when the
    /// key means nothing here, so it stays for the next consumer.
    pub fn key(&mut self, bounds: Rect, key: NavKey, mods: Modifiers) -> Option<TreeAction> {
        let rows = self.rows();
        if rows.is_empty() {
            return None;
        }
        let last = rows.len() - 1;
        let cur = self.cursor_row(&rows);
        let page = self.page_rows(bounds);
        let target = match key {
            NavKey::Up => Some(cur.map_or(0, |c| c.saturating_sub(1))),
            NavKey::Down => Some(cur.map_or(0, |c| (c + 1).min(last))),
            NavKey::Home => Some(0),
            NavKey::End => Some(last),
            NavKey::PageUp => Some(cur.map_or(0, |c| c.saturating_sub(page))),
            NavKey::PageDown => Some(cur.map_or(0, |c| (c + page).min(last))),
            NavKey::Right => {
                let c = cur?;
                let row = &rows[c];
                if !row.has_children {
                    return None;
                }
                if !row.expanded {
                    let path = row.path.clone();
                    self.set_expanded(&path, true);
                    return Some(TreeAction::Expanded(path));
                }
                Some((c + 1).min(last))
            }
            NavKey::Left => {
                let c = cur?;
                let row = &rows[c];
                if row.has_children && row.expanded {
                    let path = row.path.clone();
                    self.set_expanded(&path, false);
                    return Some(TreeAction::Collapsed(path));
                }
                let parent = row.path.split_last().map(|(_, p)| p).filter(|p| !p.is_empty())?;
                rows.iter().position(|r| r.path.as_slice() == parent)
            }
            _ => None,
        };
        if let Some(t) = target {
            self.move_cursor(bounds, &rows, t, mods);
            return rows.get(t).map(|r| TreeAction::CursorMoved(r.path.clone()));
        }
        let cur_path = cur.and_then(|c| rows.get(c)).map(|r| r.path.clone());
        match key {
            NavKey::Space => {
                let path = cur_path?;
                if mods.ctrl && self.multi_select {
                    self.toggle_path(path);
                } else {
                    self.select_only(path);
                }
                Some(TreeAction::SelectionChanged)
            }
            NavKey::Enter => cur_path.map(TreeAction::Activate),
            NavKey::F2 => cur_path.map(TreeAction::Rename),
            NavKey::ContextMenu => cur_path.map(TreeAction::ContextMenu),
            NavKey::Delete => (!self.selected_paths().is_empty())
                .then_some(TreeAction::Delete { permanent: mods.shift }),
            NavKey::SelectAll if self.multi_select => {
                self.selection = rows.iter().map(|r| r.path.clone()).collect();
                Some(TreeAction::SelectionChanged)
            }
            _ => None,
        }
    }

    /// Type-ahead over the visible rows — see [`ListView::type_ahead`].
    pub fn type_ahead(&mut self, bounds: Rect, text: &str, now_ms: u64) -> Option<TreeAction> {
        let (query, repeated) = self.typeahead.push(text, now_ms);
        let rows = self.rows();
        let labels: Vec<&str> = rows
            .iter()
            .map(|r| {
                kc::TreeView::node_at(&self.inner.nodes, &r.path).map_or("", |n| n.text.as_str())
            })
            .collect();
        let cur = self.cursor_row(&rows);
        let start = match cur {
            Some(c) if repeated || query.chars().count() == 1 => c + 1,
            Some(c) => c,
            None => 0,
        };
        let hit = find_prefix(&labels, start, &query)?;
        self.move_cursor(bounds, &rows, hit, Modifiers::NONE);
        rows.get(hit).map(|r| TreeAction::CursorMoved(r.path.clone()))
    }

    /// Takes this frame's keys and typed text from the host queue — see
    /// [`ListView::take_input`].
    pub fn take_input(&mut self, bounds: Rect) -> Vec<TreeAction> {
        let now = host::now_ms();
        let mut actions = Vec::new();
        host::consume(|e| match e {
            InputEvent::Key { vk: code, down: true, mods, .. } => {
                match NavKey::from_vk(*code, *mods).and_then(|k| self.key(bounds, k, *mods)) {
                    Some(a) => {
                        actions.push(a);
                        true
                    }
                    None => false,
                }
            }
            InputEvent::Text(s) if s.trim().is_empty() && self.typeahead.idle(now) => true,
            InputEvent::Text(s) => {
                if let Some(a) = self.type_ahead(bounds, s, now) {
                    actions.push(a);
                }
                true
            }
            _ => false,
        });
        actions
    }

    // ── Scrolling ────────────────────────────────────────────────────────

    /// The height of every visible row together.
    pub fn content_height(&self) -> f32 {
        let rows = self.rows();
        match self.offsets(&rows) {
            Some(o) => o.last().copied().unwrap_or(0.0),
            None => rows.len() as f32 * self.row_height(),
        }
    }

    /// The largest scroll offset that still shows content.
    pub fn max_scroll(&self, bounds: Rect) -> f32 {
        (self.content_height() - (bounds.bottom - bounds.top)).max(0.0)
    }

    /// Clamps [`TreeView::scroll`] into `0..=max_scroll` — after a collapse,
    /// which can leave it past the end.
    pub fn clamp_scroll(&mut self, bounds: Rect) {
        self.scroll = self.scroll.clamp(0.0, self.max_scroll(bounds));
    }

    /// Scrolls by `dy` DIP (positive = down), clamped.
    pub fn scroll_by(&mut self, bounds: Rect, dy: f32) {
        self.scroll += dy;
        self.clamp_scroll(bounds);
    }

    /// Scrolls the least that brings visible row `index` fully into view.
    pub fn ensure_visible(&mut self, bounds: Rect, index: usize) {
        let offsets = self.offsets(&self.rows());
        let (top, bottom) = self.span(offsets.as_deref(), index);
        self.scroll = nearest_scroll(self.scroll, bounds.bottom - bounds.top, top, bottom);
        self.clamp_scroll(bounds);
    }

    fn bar(&self, bounds: Rect) -> Option<(Rect, ScrollBar)> {
        overlay_bar(bounds, bounds, 0.0, self.content_height(), self.scroll, self.scrollbar_hot)
    }

    /// The overlay scroll bar's gutter, or `None` when every row fits.
    pub fn scrollbar_rail(&self, bounds: Rect) -> Option<Rect> {
        self.bar(bounds).map(|(rail, _)| rail)
    }

    /// The overlay bar's thumb, or `None` when every row fits.
    pub fn scrollbar_thumb(&self, bounds: Rect) -> Option<Rect> {
        self.bar(bounds).map(|(rail, bar)| bar.thumb_rect(rail))
    }

    /// Which part of the overlay bar `(x, y)` lands on.
    pub fn scrollbar_part_at(&self, bounds: Rect, x: f32, y: f32) -> Option<ScrollPart> {
        let (rail, bar) = self.bar(bounds)?;
        bar.part_at(rail, x, y)
    }

    /// Scrolls so the thumb's top edge lands at `thumb_top` (a thumb drag).
    pub fn scroll_to_thumb(&mut self, bounds: Rect, thumb_top: f32) {
        if let Some((rail, bar)) = self.bar(bounds) {
            self.scroll = scroll_for_thumb(&bar, rail, thumb_top, self.max_scroll(bounds));
        }
    }

    /// A press on the overlay bar: a page on the track, a row on an arrow —
    /// see [`ListView::scrollbar_press`].
    pub fn scrollbar_press(&mut self, bounds: Rect, x: f32, y: f32) -> Option<ScrollPart> {
        let part = self.scrollbar_part_at(bounds, x, y)?;
        let rh = self.row_height();
        let page = (bounds.bottom - bounds.top).max(rh);
        match part {
            ScrollPart::ArrowLow => self.scroll_by(bounds, -rh),
            ScrollPart::ArrowHigh => self.scroll_by(bounds, rh),
            ScrollPart::PageLow => self.scroll_by(bounds, -page),
            ScrollPart::PageHigh => self.scroll_by(bounds, page),
            ScrollPart::Thumb => {}
        }
        Some(part)
    }

    // ── Inline rename and context menu anchors ───────────────────────────

    /// The box visible row `index`'s label occupies — where an F2 rename
    /// field goes.
    pub fn name_rect(&self, bounds: Rect, index: usize) -> Rect {
        let row = self.row_rect(bounds, index, self.scroll);
        let depth = self.rows().get(index).map_or(0, |r| r.depth);
        let left = row.left + self.label_left(depth) - RENAME_PAD_X;
        let right = (row.right - TREE_PAD).max(left);
        Rect::new(left, row.top + space::XS, right, row.bottom - space::XS)
    }

    /// Where a keyboard-opened context menu should appear: under the cursor
    /// node's label.
    pub fn context_anchor(&self, bounds: Rect) -> Option<(f32, f32)> {
        let i = self.cursor_index()?;
        let r = self.name_rect(bounds, i);
        Some((r.left, r.bottom))
    }

    /// Paints the inline rename field over visible row `index` — see
    /// [`ListView::paint_rename`].
    pub fn paint_rename(&self, c: &dyn Canvas, bounds: Rect, index: usize, text: &str, caret: bool) {
        c.push_clip(&bounds);
        paint_inline_editor(c, self.name_rect(bounds, index), text, caret);
        c.pop_clip();
    }

    /// Where a row's label starts, from the row's left edge — chevron column,
    /// optional check box, icon, gap. Shared by the painter and by anything
    /// measuring a row, so a check box pushes the label rather than landing
    /// under it.
    fn label_left(&self, depth: usize) -> f32 {
        let mut x = TREE_PAD + depth as f32 * self.inner.indent as f32;
        if self.inner.show_plus_minus {
            x += CHEVRON_BOX + CHEVRON_GAP;
        }
        if self.inner.check_boxes {
            x += control::CHECK_BOX + control::CHECK_GAP;
        }
        x + TREE_ICON + TREE_ICON_GAP
    }

    fn icon_of(&self, node: &TreeNode) -> Option<&'static str> {
        usize::try_from(node.image_index).ok().and_then(|i| self.image_list.get(i)).copied()
    }
}

impl Deref for TreeView {
    type Target = kc::TreeView;
    fn deref(&self) -> &kc::TreeView {
        &self.inner
    }
}
impl DerefMut for TreeView {
    fn deref_mut(&mut self) -> &mut kc::TreeView {
        &mut self.inner
    }
}

impl Widget for TreeView {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    /// The widest visible row (indent + chevron + check + icon + label) by the
    /// total height of the visible rows. Same structure as the replica's
    /// `preferred_size` — which is where the mistakes are — with the Kubuno
    /// columns instead of the toolkit's 9 DIP glyph box.
    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let f = canvas.formats();
        let rows = self.rows();
        let mut width = 0.0f32;
        for r in &rows {
            let label = kc::TreeView::node_at(&self.inner.nodes, &r.path)
                .map(|n| n.text.as_str())
                .unwrap_or("");
            width = width
                .max(self.label_left(r.depth) + canvas.measure(label, &f.body) + TREE_PAD);
        }
        let height = self.offsets(&rows).and_then(|o| o.last().copied()).unwrap_or(rows.len() as f32 * self.row_height());
        Size::new(width, height)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        // Rule: every widget lands on an opaque background. What follows
        // may overpaint most of it (a fill, a card, a track); this makes
        // sure the parts that stay uncovered still read as an opaque
        // surface, so a widget never shows what is behind it.
        canvas.fill_rounded(&bounds, 0.0, &canvas.current_bg());
        let t = canvas.theme();
        let f = canvas.formats();
        let rows = self.rows();
        // The ring marks the keyboard cursor, and only on `:focus-visible` —
        // a click on a nav row leaves no ring behind, a Tab or an arrow does.
        let ring_row = if state.show_focus_ring() { self.cursor_row(&rows) } else { None };
        // `HideSelection`: the toolkit hides the band when the control has lost
        // the focus. The replica stores the flag without simulating it (a pure
        // value has no focus); here `WidgetState` carries one, so it is real.
        let show_selection = state.focused || !self.inner.hide_selection;

        canvas.push_clip(&bounds);
        // `DrawMode`: `OwnerDrawAll` hands the whole row to the owner, `OwnerDrawText` its label.
        let owner = (self.inner.draw_mode != kc::TreeViewDrawMode::Normal && owner_draw::has_handler()).then(|| Graphics::new(canvas));
        let offsets = self.offsets(&rows);
        for i in self.range_in(bounds, self.scroll, offsets.as_deref(), rows.len()) {
            let Some(row) = rows.get(i) else {
                continue;
            };
            let rect = self.row_rect_in(bounds, i, self.scroll, offsets.as_deref());
            let node = kc::TreeView::node_at(&self.inner.nodes, &row.path);
            let is_selected = show_selection && self.is_path_selected(&row.path);
            let owner_state = DrawItemState::NONE
                .with(DrawItemState::SELECTED, is_selected)
                .with(DrawItemState::FOCUS, ring_row == Some(i))
                .with(DrawItemState::HOT_LIGHT, self.hot_row == Some(i) && !state.disabled)
                .with(DrawItemState::CHECKED, row.checked)
                .with(DrawItemState::DISABLED, state.disabled);
            if let (Some(g), kc::TreeViewDrawMode::OwnerDrawAll) = (&owner, self.inner.draw_mode) {
                let text = node.map(|n| n.text.as_str()).unwrap_or("");
                let mut e = DrawItemEventArgs::new(g, "TreeView", Some(i), rect, owner_state, text);
                if owner_draw::draw_item(&mut e) {
                    continue;
                }
            }

            // The web nav row is a full pill: the active one keeps its
            // `accent_light` pastille on hover — the two fills never stack.
            let corner = crate::metrics::pill(rect.bottom - rect.top);
            let band = if self.inner.full_row_select {
                rect
            } else {
                Rect::new(rect.left + TREE_PAD, rect.top, rect.right - TREE_PAD, rect.bottom)
            };
            if is_selected {
                canvas.fill_rounded(&band, corner, &t.accent_light);
            } else if self.hot_row == Some(i) && !state.disabled {
                canvas.fill_rounded(&band, corner, &t.row_hover);
            }
            if ring_row == Some(i) {
                // `focus-visible:ring-2 ring-primary`, drawn inward on the pill.
                let h = FOCUS_RING / 2.0;
                let inner = Rect::new(band.left + h, band.top + h, band.right - h, band.bottom - h);
                canvas.stroke_rounded_w(&inner, (corner - h).max(0.0), &t.accent, FOCUS_RING);
            }

            // `ShowLines`: the product's nav tree has no branch lines, so the
            // flag is honoured with the design system's own hairline —
            // `divider`, one DIP, at each ancestor level.
            if self.inner.show_lines {
                for level in 0..=row.depth {
                    let x = rect.left + TREE_PAD + level as f32 * self.inner.indent as f32
                        + CHEVRON_BOX / 2.0;
                    canvas.fill_rounded(
                        &Rect::new(x, rect.top, x + 1.0, rect.bottom),
                        0.0,
                        &t.divider,
                    );
                }
            }

            let mut x = rect.left + TREE_PAD + row.depth as f32 * self.inner.indent as f32;
            if self.inner.show_plus_minus {
                if row.has_children {
                    let cy = (rect.top + rect.bottom) / 2.0;
                    let box_rect = Rect::new(
                        x,
                        cy - CHEVRON_BOX / 2.0,
                        x + CHEVRON_BOX,
                        cy + CHEVRON_BOX / 2.0,
                    );
                    // Geometry, never a character: the embedded face carries no
                    // chevron and a text draw would be tofu.
                    let glyph = if row.expanded { "ChevronDown" } else { "ChevronRight" };
                    canvas.vector_icon(glyph, &box_rect, CHEVRON_GLYPH, &t.text_secondary);
                }
                x += CHEVRON_BOX + CHEVRON_GAP;
            }
            if self.inner.check_boxes {
                let cy = (rect.top + rect.bottom) / 2.0;
                let bx = Rect::new(
                    x,
                    cy - control::CHECK_BOX / 2.0,
                    x + control::CHECK_BOX,
                    cy + control::CHECK_BOX / 2.0,
                );
                if row.checked {
                    canvas.fill_rounded(&bx, radius::SM, &t.accent);
                    canvas.stroke_rounded_w(&bx, radius::SM, &t.accent, CHECK_BORDER);
                    canvas.vector_icon(
                        "Check",
                        &bx,
                        control::CHECK_TICK,
                        &t.accent_foreground,
                    );
                } else {
                    canvas.stroke_rounded_w(&bx, radius::SM, &t.card_stroke, CHECK_BORDER);
                }
                x += control::CHECK_BOX + control::CHECK_GAP;
            }
            if let Some(icon) = node.and_then(|n| self.icon_of(n)) {
                let cy = (rect.top + rect.bottom) / 2.0;
                let ir = Rect::new(x, cy - TREE_ICON / 2.0, x + TREE_ICON, cy + TREE_ICON / 2.0);
                let colour = if is_selected { &t.accent } else { &t.text_secondary };
                canvas.vector_icon(icon, &ir, CHEVRON_GLYPH, colour);
            }
            x += TREE_ICON + TREE_ICON_GAP;

            let label = node.map(|n| n.text.as_str()).unwrap_or("");
            let colour = if state.disabled {
                &t.text_tertiary
            } else if is_selected {
                &t.text_nav_active
            } else {
                &t.text_primary
            };
            let label_rect =
                Rect::new(x, rect.top, (rect.right - TREE_ICON_GAP).max(x), rect.bottom);
            if let (Some(g), kc::TreeViewDrawMode::OwnerDrawText) = (&owner, self.inner.draw_mode) {
                let mut e = DrawItemEventArgs::new(g, "TreeView", Some(i), label_rect, owner_state, label);
                e.fore_color = (*colour).into();
                if owner_draw::draw_item(&mut e) {
                    continue;
                }
            }
            canvas.text_ellipsis(label, &label_rect, &f.body, colour);
        }
        // Overflow: the overlay bar, over the rows.
        if let Some((rail, bar)) = self.bar(bounds) {
            let bs = WidgetState::REST.hot(self.scrollbar_hot).disabled(state.disabled);
            bar.paint(canvas, rail, bs);
        }
        canvas.pop_clip();
    }

    fn type_name(&self) -> &'static str {
        "TreeView"
    }
}

// =============================================================================
//  Tests — pure geometry and pure state, no device needed.
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: usize) -> ListView {
        let mut v = ListView::new();
        v.view = View::Details;
        v.columns = vec![
            ColumnHeader::new("Nom", 300),
            ColumnHeader::new("Modifié", 140),
            ColumnHeader::new("Type", 150),
            ColumnHeader::new("Taille", 100),
        ];
        v.items = (0..items)
            .map(|i| {
                ListViewItem::new(format!("item {i:05}"))
                    .with_sub("hier")
                    .with_sub("Dossier")
                    .with_sub("—")
            })
            .collect();
        v
    }

    /// 4 rows of 40 fit exactly; the body starts under the 28 DIP header.
    fn bounds() -> Rect {
        Rect::new(0.0, 0.0, 800.0, 28.0 + 160.0)
    }

    #[test]
    fn a_row_starts_at_its_own_top_edge_and_ends_before_the_next() {
        let v = list(10);
        let b = bounds();
        let top = 28.0; // body top
        assert_eq!(v.row_at(b, 10.0, top), Some(0));
        assert_eq!(v.row_at(b, 10.0, top + 39.999), Some(0));
        assert_eq!(v.row_at(b, 10.0, top + 40.0), Some(1));
        assert_eq!(v.row_at(b, 10.0, top + 79.999), Some(1));
        // The header band is not a row.
        assert_eq!(v.row_at(b, 10.0, 0.0), None);
        assert_eq!(v.row_at(b, 10.0, 27.999), None);
    }

    #[test]
    fn a_row_hit_follows_the_scroll_offset() {
        let mut v = list(10);
        v.scroll = 40.0;
        let b = bounds();
        // Scrolled by one whole row: the first band now shows item 1.
        assert_eq!(v.row_at(b, 10.0, 28.0), Some(1));
        v.scroll = 20.0;
        assert_eq!(v.row_at(b, 10.0, 28.0), Some(0));
        assert_eq!(v.row_at(b, 10.0, 28.0 + 20.0), Some(1));
    }

    #[test]
    fn nothing_is_hit_past_the_last_item() {
        let v = list(2);
        let b = bounds();
        assert_eq!(v.row_at(b, 10.0, 28.0 + 79.0), Some(1));
        assert_eq!(v.row_at(b, 10.0, 28.0 + 80.0), None);
    }

    #[test]
    fn the_visible_range_includes_a_partial_first_and_last_row() {
        let v = list(100);
        let b = bounds(); // 160 DIP of body = 4 rows
        assert_eq!(v.visible_range(b, 0.0), 0..4);
        // Half a row scrolled: five rows are now touched by the viewport.
        assert_eq!(v.visible_range(b, 20.0), 0..5);
        assert_eq!(v.visible_range(b, 40.0), 1..5);
    }

    #[test]
    fn the_visible_range_at_the_maximum_scroll_ends_on_the_last_item() {
        let v = list(100);
        let b = bounds();
        let max = v.max_scroll(b);
        assert_eq!(max, 100.0 * 40.0 - 160.0);
        let r = v.visible_range(b, max);
        assert_eq!(r, 96..100);
        // And it never runs past the item count, whatever it is asked.
        assert_eq!(v.visible_range(b, max * 10.0).end, 100);
    }

    #[test]
    fn a_hundred_thousand_items_still_paint_a_screenful() {
        let mut v = list(0);
        v.virtual_mode = true;
        v.virtual_list_size = 100_000;
        let b = bounds();
        // Deep in the middle of the list: four rows, not a hundred thousand.
        let mid = v.visible_range(b, 1_000_000.0);
        assert!(mid.end - mid.start <= 5, "range {mid:?} is not a screenful");
        assert_eq!(mid, 25_000..25_004);
        // And at the very bottom it stops exactly on the last item.
        assert_eq!(v.visible_range(b, v.max_scroll(b)), 99_996..100_000);
        // Past the end there is nothing left to paint.
        assert!(v.visible_range(b, 10_000_000.0).is_empty());
    }

    #[test]
    fn an_empty_view_has_an_empty_range() {
        let v = list(0);
        assert_eq!(v.visible_range(bounds(), 0.0), 0..0);
    }

    #[test]
    fn the_columns_add_up_and_their_edges_are_where_the_separators_are() {
        let v = list(3);
        let b = bounds();
        assert_eq!(v.total_column_width(), 690.0);
        let xs = v.column_x_offsets(b);
        assert_eq!(xs, vec![0.0, 300.0, 440.0, 590.0, 690.0]);
        assert_eq!(v.separators(b), vec![300.0, 440.0, 590.0, 690.0]);
        assert_eq!(v.separator_at(b, 302.0, 4.0), Some(0));
        assert_eq!(v.separator_at(b, 306.0, 4.0), None);
        assert_eq!(v.separator_at(b, 690.0, 4.0), Some(3));
    }

    #[test]
    fn a_column_owns_its_left_edge_and_not_the_next_ones() {
        let v = list(3);
        let b = bounds();
        assert_eq!(v.column_at(b, 0.0), Some(0));
        assert_eq!(v.column_at(b, 299.999), Some(0));
        assert_eq!(v.column_at(b, 300.0), Some(1));
        assert_eq!(v.column_at(b, 689.999), Some(3));
        // Past the last column there is no column at all.
        assert_eq!(v.column_at(b, 690.0), None);
    }

    #[test]
    fn the_header_band_answers_only_inside_itself() {
        let v = list(3);
        let b = bounds();
        assert_eq!(v.header_at(b, 310.0, 0.0), Some(1));
        assert_eq!(v.header_at(b, 310.0, 27.999), Some(1));
        assert_eq!(v.header_at(b, 310.0, 28.0), None);
    }

    #[test]
    fn a_plain_click_replaces_the_selection_and_sets_the_anchor() {
        let mut v = list(10);
        v.click(3, false, false);
        assert_eq!(v.selected_indices(), vec![3]);
        assert_eq!(v.focused_index, Some(3));
        v.click(5, false, false);
        assert_eq!(v.selected_indices(), vec![5]);
    }

    #[test]
    fn ctrl_click_toggles_one_item_and_leaves_the_rest() {
        let mut v = list(10);
        v.click(1, false, false);
        v.click(4, true, false);
        v.click(7, true, false);
        assert_eq!(v.selected_indices(), vec![1, 4, 7]);
        v.click(4, true, false);
        assert_eq!(v.selected_indices(), vec![1, 7]);
    }

    #[test]
    fn shift_click_extends_from_the_anchor_and_does_not_move_it() {
        let mut v = list(10);
        v.click(2, false, false);
        v.click(5, false, true);
        assert_eq!(v.selected_indices(), vec![2, 3, 4, 5]);
        assert_eq!(v.focused_index, Some(2), "the anchor must not follow a shift-click");
        // A second shift-click re-extends from the SAME anchor, in either
        // direction, and never accumulates the previous run.
        v.click(0, false, true);
        assert_eq!(v.selected_indices(), vec![0, 1, 2]);
    }

    #[test]
    fn multi_select_off_collapses_every_path_to_one_item() {
        let mut v = list(10);
        v.multi_select = false;
        v.click(2, false, false);
        v.click(5, false, true);
        assert_eq!(v.selected_indices(), vec![5]);
        v.click(7, true, false);
        assert_eq!(v.selected_indices(), vec![7]);
    }

    #[test]
    fn sorting_orders_by_the_sorted_column_in_the_declared_direction() {
        let mut v = ListView::new();
        v.view = View::Details;
        v.columns = vec![ColumnHeader::new("Nom", 200), ColumnHeader::new("Type", 100)];
        v.items = vec![
            ListViewItem::new("charlie").with_sub("b"),
            ListViewItem::new("alpha").with_sub("c"),
            ListViewItem::new("bravo").with_sub("a"),
        ];
        v.sorting = SortOrder::Ascending;
        v.sort(); // column 0 — delegated to the replica's own `sort()`
        assert_eq!(v.items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), [
            "alpha", "bravo", "charlie"
        ]);
        v.sort_column = 1;
        v.sort();
        assert_eq!(v.items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), [
            "bravo", "charlie", "alpha"
        ]);
        v.sorting = SortOrder::Descending;
        v.sort();
        assert_eq!(v.items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), [
            "alpha", "charlie", "bravo"
        ]);
    }

    // ── Icon view ────────────────────────────────────────────────────────

    fn tiles(items: usize) -> ListView {
        let mut v = list(items);
        v.view = View::LargeIcon;
        v
    }

    #[test]
    fn tiles_wrap_at_the_width_they_are_given() {
        let v = tiles(20);
        // 800 wide, 12 of pad either side, tiles of 120 with a 12 gap:
        // (776 + 12) / 132 = 5.96 → 5 per row.
        let b = Rect::new(0.0, 0.0, 800.0, 400.0);
        assert_eq!(v.tiles_per_row(b), 5);
        let first = v.item_rect(b, 0, 0.0);
        assert_eq!((first.left, first.top), (12.0, 12.0));
        let sixth = v.item_rect(b, 5, 0.0);
        assert_eq!((sixth.left, sixth.top), (12.0, 12.0 + 164.0 + 12.0));
    }

    #[test]
    fn a_point_in_the_gap_between_two_tiles_hits_neither() {
        let v = tiles(20);
        let b = Rect::new(0.0, 0.0, 800.0, 400.0);
        assert_eq!(v.row_at(b, 12.0, 12.0), Some(0));
        assert_eq!(v.row_at(b, 131.0, 100.0), Some(0)); // 12 + 120 - 1
        assert_eq!(v.row_at(b, 136.0, 100.0), None); // in the 12 DIP gap
        assert_eq!(v.row_at(b, 144.0, 100.0), Some(1)); // next tile
    }

    #[test]
    fn the_icon_view_range_is_whole_rows_of_tiles() {
        let v = tiles(100);
        let b = Rect::new(0.0, 0.0, 800.0, 400.0);
        // 5 per row, a row pitch of 176: 400 DIP shows rows 0..3.
        assert_eq!(v.visible_range(b, 0.0), 0..15);
        // Scrolled by exactly two pitches: row 1 has just left through the top
        // edge, so the range starts at row 2 — and a single DIP less keeps it.
        assert_eq!(v.visible_range(b, 352.0).start, 10);
        assert_eq!(v.visible_range(b, 351.0).start, 5, "a partial row is still painted");
        assert!(v.visible_range(b, 352.0).end <= 100);
    }

    // ── TreeView ─────────────────────────────────────────────────────────

    /// A branch `depth` levels deep, every level carrying two children so the
    /// flattening has something to skip.
    fn deep(depth: usize) -> TreeNode {
        let mut node = TreeNode::new(format!("level {depth}"));
        if depth > 0 {
            node.children.push(deep(depth - 1));
            node.children.push(TreeNode::new(format!("leaf {depth}")));
        }
        node
    }

    fn expand_all(node: &mut TreeNode) {
        node.expanded = true;
        for c in &mut node.children {
            expand_all(c);
        }
    }

    fn tree() -> TreeView {
        let mut t = TreeView::new();
        let mut root = deep(5);
        expand_all(&mut root);
        t.nodes = vec![root, TreeNode::new("autre racine")];
        t
    }

    #[test]
    fn rows_with_their_own_height_move_the_rows_after_them() {
        let mut t = tree();
        let rh = t.row_height();
        let rows = t.rows();
        let uniform = t.content_height();
        t.node_heights.insert(rows[1].path.clone(), 80.0);
        let b = Rect::new(0.0, 0.0, 300.0, 1000.0);
        assert_eq!(t.row_rect(b, 1, 0.0), Rect::new(0.0, rh, 300.0, rh + 80.0));
        assert_eq!(t.row_rect(b, 2, 0.0).top, rh + 80.0);
        assert_eq!(t.content_height(), uniform - rh + 80.0);
        assert_eq!(t.node_at(b, 10.0, rh + 70.0), Some(1), "inside the tall row");
        assert_eq!(t.node_at(b, 10.0, rh + 81.0), Some(2));
        let small = Rect::new(0.0, 0.0, 300.0, 50.0);
        assert_eq!(t.visible_range(small, rh + 10.0), 1..2, "only the tall row fills the box");
        t.ensure_visible(small, 2);
        assert!((t.scroll - (rh + 80.0 + rh - 50.0)).abs() < 0.01, "{}", t.scroll);
    }

    #[test]
    fn expanding_and_collapsing_changes_exactly_the_subtree_below() {
        let mut t = tree();
        // 5 nested levels + a leaf each, the deepest level being a lone node,
        // plus the second root.
        let all = t.rows().len();
        assert_eq!(all, 12);
        // The deepest branch that still has children carries exactly two rows:
        // collapsing it hides those and nothing else.
        t.set_expanded(&[0, 0, 0, 0, 0], false);
        assert_eq!(t.rows().len(), all - 2);
        t.toggle_expanded(&[0, 0, 0, 0, 0]);
        assert_eq!(t.rows().len(), all);
        // One level up hides that branch AND its own leaf: four rows.
        t.set_expanded(&[0, 0, 0, 0], false);
        assert_eq!(t.rows().len(), all - 4);
        t.toggle_expanded(&[0, 0, 0, 0]);
        // Collapsing the root leaves the root and the second root.
        t.set_expanded(&[0], false);
        assert_eq!(t.rows().len(), 2);
        assert!(!t.rows()[0].expanded);
        assert!(t.rows()[0].has_children);
    }

    #[test]
    fn a_tree_row_starts_at_its_own_top_edge() {
        let t = tree();
        let b = Rect::new(0.0, 0.0, 300.0, 200.0);
        assert_eq!(t.node_at(b, 10.0, 0.0), Some(0));
        assert_eq!(t.node_at(b, 10.0, 35.999), Some(0));
        assert_eq!(t.node_at(b, 10.0, 36.0), Some(1));
        assert_eq!(t.node_at(b, 10.0, 71.999), Some(1));
    }

    #[test]
    fn a_tree_hit_past_the_last_row_is_nothing() {
        let mut t = TreeView::new();
        t.nodes = vec![TreeNode::new("a"), TreeNode::new("b")];
        let b = Rect::new(0.0, 0.0, 300.0, 200.0);
        assert_eq!(t.node_at(b, 10.0, 71.0), Some(1));
        assert_eq!(t.node_at(b, 10.0, 72.0), None);
    }

    #[test]
    fn the_visible_range_of_a_tree_covers_the_partial_rows_too() {
        let mut t = TreeView::new();
        t.nodes = (0..1000).map(|i| TreeNode::new(format!("n{i}"))).collect();
        let b = Rect::new(0.0, 0.0, 300.0, 180.0); // 5 rows of 36
        assert_eq!(t.visible_range(b, 0.0), 0..5);
        assert_eq!(t.visible_range(b, 18.0), 0..6);
        assert_eq!(t.visible_range(b, 36.0), 1..6);
        let max = 1000.0 * 36.0 - 180.0;
        assert_eq!(t.visible_range(b, max), 995..1000);
    }

    #[test]
    fn the_chevron_is_hit_only_on_its_own_box_and_only_on_a_parent() {
        let t = tree();
        let b = Rect::new(0.0, 0.0, 300.0, 600.0);
        // Row 0 is a root with children: its chevron box is 8..28 across, and
        // the row is 36 tall so the box spans y 8..28 too.
        assert_eq!(t.chevron_hit(b, 10.0, 18.0), Some(0));
        assert_eq!(t.chevron_hit(b, 27.9, 18.0), Some(0));
        assert_eq!(t.chevron_hit(b, 28.1, 18.0), None, "past the box is the label");
        assert_eq!(t.chevron_hit(b, 10.0, 5.0), None, "above the box");
        // The last root is a leaf — no chevron anywhere on it.
        let last = t.rows().len() - 1;
        let y = last as f32 * 36.0 + 18.0;
        assert_eq!(t.node_at(b, 10.0, y), Some(last));
        assert_eq!(t.chevron_hit(b, 10.0, y), None);
    }

    #[test]
    fn a_chevron_indents_with_its_level() {
        let t = tree();
        let b = Rect::new(0.0, 0.0, 300.0, 400.0);
        // Row 1 is the first child: one level in, so 16 DIP further right.
        let row = t.row_rect(b, 1, 0.0);
        let deep_box = t.chevron_rect(row, t.rows()[1].depth);
        assert_eq!(deep_box.left, 8.0 + 16.0);
        assert_eq!(t.chevron_hit(b, 26.0, 54.0), Some(1));
        assert_eq!(t.chevron_hit(b, 10.0, 54.0), None, "the parent's column, not this row's");
    }

    #[test]
    fn hiding_the_plus_minus_takes_the_chevron_target_away() {
        let mut t = tree();
        t.show_plus_minus = false;
        let b = Rect::new(0.0, 0.0, 300.0, 400.0);
        assert_eq!(t.chevron_hit(b, 10.0, 18.0), None);
        assert_eq!(t.node_at(b, 10.0, 18.0), Some(0), "the row is still hit");
    }

    #[test]
    fn selecting_a_row_stores_the_replicas_own_path() {
        let mut t = tree();
        t.select_row(2);
        assert_eq!(t.selected_path, t.path_at(2));
        assert!(t.selected_node().is_some());
    }

    // ── Keyboard, type-ahead, scrolling ──────────────────────────────────

    const NONE: Modifiers = Modifiers::NONE;
    const SHIFT: Modifiers = Modifiers::SHIFT;
    const CTRL: Modifiers = Modifiers::CTRL;

    #[test]
    fn keys_decode_like_the_web_and_leave_alt_chords_alone() {
        assert_eq!(NavKey::from_vk(vk::DOWN, NONE), Some(NavKey::Down));
        assert_eq!(NavKey::from_vk(vk::DOWN, SHIFT), Some(NavKey::Down));
        assert_eq!(NavKey::from_vk(vk::letter('a'), CTRL), Some(NavKey::SelectAll));
        assert_eq!(NavKey::from_vk(vk::letter('a'), NONE), None, "plain A is type-ahead text");
        assert_eq!(NavKey::from_vk(vk::F10, SHIFT), Some(NavKey::ContextMenu));
        assert_eq!(NavKey::from_vk(vk::APPS, NONE), Some(NavKey::ContextMenu));
        assert_eq!(NavKey::from_vk(vk::DOWN, Modifiers::ALT), None);
        assert_eq!(NavKey::from_vk(vk::ENTER, CTRL), None);
        assert_eq!(NavKey::from_vk(vk::TAB, NONE), None, "Tab belongs to the focus ring");
    }

    #[test]
    fn a_prefix_search_wraps_and_ignores_case() {
        let labels = ["Alpha", "bravo", "Beta", "charlie"];
        assert_eq!(find_prefix(&labels, 0, "b"), Some(1));
        assert_eq!(find_prefix(&labels, 2, "B"), Some(2));
        assert_eq!(find_prefix(&labels, 3, "a"), Some(0), "wraps past the end");
        assert_eq!(find_prefix(&labels, 0, "be"), Some(2));
        assert_eq!(find_prefix(&labels, 0, "z"), None);
        assert_eq!(find_prefix(&labels, 0, ""), None);
    }

    #[test]
    fn the_typeahead_buffer_extends_repeats_and_resets() {
        let mut ta = TypeAhead::default();
        assert_eq!(ta.push("p", 0), ("p".into(), false));
        assert_eq!(ta.push("h", 100), ("ph".into(), false));
        // A pause longer than the reset starts over.
        assert_eq!(ta.push("b", 100 + TYPEAHEAD_RESET_MS + 1), ("b".into(), false));
        // The same letter again cycles on that letter.
        assert_eq!(ta.push("b", 2000), ("b".into(), true));
        assert_eq!(ta.push("b", 2100), ("b".into(), true));
    }

    #[test]
    fn arrows_move_the_cursor_and_the_selection_follows() {
        let mut v = list(10);
        let b = bounds();
        // No cursor yet: the first movement lands on item 0.
        assert_eq!(v.key(b, NavKey::Down, NONE), Some(ListAction::CursorMoved(0)));
        assert_eq!(v.key(b, NavKey::Down, NONE), Some(ListAction::CursorMoved(1)));
        assert_eq!(v.selected_indices(), vec![1]);
        assert_eq!(v.key(b, NavKey::Up, NONE), Some(ListAction::CursorMoved(0)));
        assert_eq!(v.key(b, NavKey::Up, NONE), Some(ListAction::CursorMoved(0)), "clamped");
        assert_eq!(v.key(b, NavKey::End, NONE), Some(ListAction::CursorMoved(9)));
        assert_eq!(v.key(b, NavKey::Home, NONE), Some(ListAction::CursorMoved(0)));
        // A page is the four whole rows the 160 DIP body holds.
        assert_eq!(v.page_items(b), 4);
        assert_eq!(v.key(b, NavKey::PageDown, NONE), Some(ListAction::CursorMoved(4)));
        // Left / Right mean nothing in Details: they stay in the queue.
        assert_eq!(v.key(b, NavKey::Left, NONE), None);
    }

    #[test]
    fn shift_arrows_extend_from_a_fixed_anchor_and_ctrl_only_moves() {
        let mut v = list(10);
        v.multi_select = true;
        let b = bounds();
        v.click(2, false, false);
        v.key(b, NavKey::Down, SHIFT);
        v.key(b, NavKey::Down, SHIFT);
        assert_eq!(v.selected_indices(), vec![2, 3, 4]);
        v.key(b, NavKey::Up, SHIFT);
        v.key(b, NavKey::Up, SHIFT);
        v.key(b, NavKey::Up, SHIFT);
        assert_eq!(v.selected_indices(), vec![1, 2], "the run flips over the anchor");
        // Ctrl moves the cursor alone, Ctrl+Space adds the item under it.
        v.key(b, NavKey::Down, CTRL);
        v.key(b, NavKey::Down, CTRL);
        assert_eq!(v.cursor, Some(3));
        assert_eq!(v.selected_indices(), vec![1, 2]);
        assert_eq!(v.key(b, NavKey::Space, CTRL), Some(ListAction::SelectionChanged));
        assert_eq!(v.selected_indices(), vec![1, 2, 3]);
        assert_eq!(v.key(b, NavKey::SelectAll, CTRL), Some(ListAction::SelectionChanged));
        assert_eq!(v.selected_indices().len(), 10);
        assert_eq!(v.key(b, NavKey::Escape, NONE), Some(ListAction::SelectionChanged));
        assert!(v.selected_indices().is_empty());
        assert_eq!(v.key(b, NavKey::Escape, NONE), None, "nothing left to clear");
    }

    #[test]
    fn enter_f2_and_delete_report_what_to_do() {
        let mut v = list(5);
        let b = bounds();
        v.click(3, false, false);
        assert_eq!(v.key(b, NavKey::Enter, NONE), Some(ListAction::Open(3)));
        assert_eq!(v.key(b, NavKey::F2, NONE), Some(ListAction::Rename(3)));
        assert_eq!(v.key(b, NavKey::ContextMenu, NONE), Some(ListAction::ContextMenu(3)));
        assert_eq!(
            v.key(b, NavKey::Delete, SHIFT),
            Some(ListAction::Delete { permanent: true })
        );
    }

    #[test]
    fn moving_the_cursor_scrolls_it_into_view_by_the_least_amount() {
        let mut v = list(20);
        let b = bounds(); // 4 rows of 40 visible
        v.click(0, false, false);
        for _ in 0..4 {
            v.key(b, NavKey::Down, NONE);
        }
        // Item 4 is the fifth row: its bottom (200) against the 160 viewport.
        assert_eq!(v.scroll, 40.0);
        v.key(b, NavKey::End, NONE);
        assert_eq!(v.scroll, v.max_scroll(b));
        v.key(b, NavKey::Home, NONE);
        assert_eq!(v.scroll, 0.0);
    }

    #[test]
    fn tiles_move_by_a_row_vertically_and_by_one_sideways() {
        let mut v = tiles(20);
        let b = Rect::new(0.0, 0.0, 800.0, 400.0); // 5 per row
        v.click(0, false, false);
        assert_eq!(v.key(b, NavKey::Down, NONE), Some(ListAction::CursorMoved(5)));
        assert_eq!(v.key(b, NavKey::Right, NONE), Some(ListAction::CursorMoved(6)));
        assert_eq!(v.key(b, NavKey::Left, NONE), Some(ListAction::CursorMoved(5)));
        assert_eq!(v.key(b, NavKey::Up, NONE), Some(ListAction::CursorMoved(0)));
    }

    #[test]
    fn typing_jumps_to_the_matching_name_and_repeating_cycles() {
        let mut v = ListView::new();
        v.items = ["Documents", "Photos", "Projets", "budget.xlsx", "photo.png"]
            .iter()
            .map(|s| ListViewItem::new(*s))
            .collect();
        let b = bounds();
        assert_eq!(v.type_ahead(b, "p", 0), Some(ListAction::CursorMoved(1)));
        assert_eq!(v.type_ahead(b, "r", 50), Some(ListAction::CursorMoved(2)), "« pr »");
        assert_eq!(v.selected_indices(), vec![2]);
        // After a pause, « p » again moves PAST the cursor, and repeating it
        // keeps cycling through the p-names.
        assert_eq!(v.type_ahead(b, "p", 5000), Some(ListAction::CursorMoved(4)));
        assert_eq!(v.type_ahead(b, "p", 5050), Some(ListAction::CursorMoved(1)));
        assert_eq!(v.type_ahead(b, "z", 9000), None);
    }

    #[test]
    fn a_right_click_keeps_a_selection_it_lands_in() {
        let mut v = list(10);
        v.multi_select = true;
        v.click(2, false, false);
        v.click(4, false, true);
        v.context_click(3);
        assert_eq!(v.selected_indices(), vec![2, 3, 4]);
        assert_eq!(v.cursor, Some(3));
        v.context_click(8);
        assert_eq!(v.selected_indices(), vec![8]);
    }

    #[test]
    fn the_overlay_bar_appears_only_when_the_rows_overflow() {
        let b = bounds();
        assert!(list(4).scrollbar_rail(b).is_none(), "four rows fit exactly");
        let v = list(5);
        let rail = v.scrollbar_rail(b).expect("a fifth row overflows");
        // Along the body's right edge, under the header.
        assert_eq!(rail.right, b.right);
        assert_eq!(rail.top, 28.0);
        assert_eq!(rail.bottom, b.bottom);
    }

    #[test]
    fn wheel_and_thumb_scrolling_stay_in_range() {
        let mut v = list(20); // 800 of content in a 160 viewport
        let b = bounds();
        v.scroll_by(b, -100.0);
        assert_eq!(v.scroll, 0.0);
        v.scroll_by(b, 10_000.0);
        assert_eq!(v.scroll, 640.0);
        // Dragging the thumb to the top of the rail scrolls to 0, to the far
        // end scrolls to the max, and the thumb then sits where it was put.
        let rail = v.scrollbar_rail(b).expect("overflow");
        v.scroll_to_thumb(b, rail.top);
        assert_eq!(v.scroll, 0.0);
        v.scroll_to_thumb(b, rail.bottom);
        assert_eq!(v.scroll, 640.0);
        v.scroll = 320.0;
        let thumb = v.scrollbar_thumb(b).expect("overflow");
        v.scroll_to_thumb(b, thumb.top);
        assert!((v.scroll - 320.0).abs() < 1.0, "thumb → scroll is the inverse, got {}", v.scroll);
        // A press on the track below the thumb pages down by the viewport.
        v.scroll = 0.0;
        let t = v.scrollbar_thumb(b).expect("overflow");
        assert_eq!(v.scrollbar_press(b, t.left + 1.0, b.bottom - 1.0), Some(ScrollPart::PageHigh));
        assert_eq!(v.scroll, 160.0);
    }

    #[test]
    fn the_tree_follows_the_aria_arrow_rules() {
        let mut t = TreeView::new();
        t.nodes = vec![
            TreeNode::new("Documents").child(TreeNode::new("Contrats")).child(TreeNode::new("Notes")),
            TreeNode::new("Médias"),
        ];
        let b = Rect::new(0.0, 0.0, 300.0, 400.0);
        t.select_row(0);
        // Right on a closed parent opens it and does not move.
        assert_eq!(t.key(b, NavKey::Right, NONE), Some(TreeAction::Expanded(vec![0])));
        assert_eq!(t.rows().len(), 4);
        // Right again goes to the first child.
        assert_eq!(t.key(b, NavKey::Right, NONE), Some(TreeAction::CursorMoved(vec![0, 0])));
        assert_eq!(t.selected_path, Some(vec![0, 0]));
        // Right on a leaf means nothing.
        assert_eq!(t.key(b, NavKey::Right, NONE), None);
        // Left on a child goes to its parent, Left on an open parent closes it.
        assert_eq!(t.key(b, NavKey::Left, NONE), Some(TreeAction::CursorMoved(vec![0])));
        assert_eq!(t.key(b, NavKey::Left, NONE), Some(TreeAction::Collapsed(vec![0])));
        assert_eq!(t.rows().len(), 2);
        // Left on a closed root goes nowhere.
        assert_eq!(t.key(b, NavKey::Left, NONE), None);
        assert_eq!(t.key(b, NavKey::End, NONE), Some(TreeAction::CursorMoved(vec![1])));
        assert_eq!(t.key(b, NavKey::Enter, NONE), Some(TreeAction::Activate(vec![1])));
        assert_eq!(t.key(b, NavKey::F2, NONE), Some(TreeAction::Rename(vec![1])));
    }

    #[test]
    fn collapsing_a_branch_brings_a_hidden_cursor_back_onto_it() {
        let mut t = tree();
        let deep_path = vec![0, 0, 0];
        t.select_row(t.rows().iter().position(|r| r.path == deep_path).expect("visible"));
        t.set_expanded(&[0, 0], false);
        assert_eq!(t.selected_path, Some(vec![0, 0]));
        assert_eq!(t.cursor, Some(vec![0, 0]));
    }

    #[test]
    fn the_tree_multi_selects_with_ctrl_and_shift() {
        let mut t = TreeView::new();
        t.multi_select = true;
        t.nodes = (0..6).map(|i| TreeNode::new(format!("n{i}"))).collect();
        t.click(1, false, false);
        t.click(3, true, false);
        assert_eq!(t.selected_paths().len(), 2);
        assert!(t.is_path_selected(&[1]) && t.is_path_selected(&[3]));
        // Shift extends from the anchor (the last ctrl-clicked node).
        t.click(5, false, true);
        let mut got = t.selected_paths();
        got.sort();
        assert_eq!(got, vec![vec![3], vec![4], vec![5]]);
        // Ctrl toggles one out.
        t.click(4, true, false);
        assert!(!t.is_path_selected(&[4]));
        // Without multi_select every click is plain.
        t.multi_select = false;
        t.click(0, true, false);
        assert_eq!(t.selected_paths(), vec![vec![0]]);
    }

    #[test]
    fn the_tree_scrolls_its_cursor_into_view_and_shows_a_bar_on_overflow() {
        let mut t = TreeView::new();
        t.nodes = (0..20).map(|i| TreeNode::new(format!("n{i}"))).collect();
        let b = Rect::new(0.0, 0.0, 300.0, 180.0); // 5 rows of 36
        assert!(t.scrollbar_rail(b).is_some());
        t.select_row(0);
        assert_eq!(t.key(b, NavKey::PageDown, NONE), Some(TreeAction::CursorMoved(vec![5])));
        assert_eq!(t.scroll, 36.0);
        assert_eq!(t.type_ahead(b, "n1", 0), Some(TreeAction::CursorMoved(vec![10])));
        assert_eq!(t.scroll, 11.0 * 36.0 - 180.0);
        let mut small = TreeView::new();
        small.nodes = vec![TreeNode::new("a")];
        assert!(small.scrollbar_rail(b).is_none());
    }

    #[test]
    fn the_kubuno_tree_replaces_the_toolkits_two_metrics() {
        let t = TreeView::new();
        assert_eq!(t.row_height(), 36.0, "shape::height::SIDEBAR_ROW");
        assert_eq!(t.indent, 16, "INDENT_PER_LEVEL");
    }
}
