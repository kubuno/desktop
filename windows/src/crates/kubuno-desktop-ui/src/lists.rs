//! Kubuno primitives — the list family: [`ListBox`], [`CheckedListBox`],
//! [`ComboBox`] and [`Menu`].
//!
//! # Where the pixels come from
//!
//! None of these four has a hand-written predecessor in `kubuno-drive-desktop-app-controls`,
//! so — as `docs/UI_BRIEF.md` requires — the reference is the **web** design
//! system, read from the source (this machine cannot run the web app, so every
//! number below was *read*, not measured):
//!
//! | primitive | web source |
//! |---|---|
//! | [`Menu`] | `core/frontend/src/ui/MenuDropdown.tsx` — the product's ONE context menu, per `CLAUDE.md` |
//! | [`ComboBox`] field | `core/frontend/src/ui/Combobox.tsx:239` — its trigger, verbatim |
//! | [`ComboBox`] popup | the same file's `role="listbox"` panel; `Dropdown.tsx` for the anchor offset and the height cap |
//! | [`ListBox`] | that same `role="listbox"` panel — a list box and a select's list are one control in this system |
//! | check well | `@ui/Checkbox`, already tokenised in [`crate::metrics::control`] |
//!
//! `kubuno-drive-desktop/src/user_controls/flyout.rs` already ported `MenuDropdown` to
//! Direct2D once, from the same file; where it resolved something the CSS
//! leaves implicit (a glyph's drawn size, the caret column's width) its answer
//! is reused rather than re-derived, and the comment says so.
//!
//! # What comes from the replica, and is never restated here
//!
//! Everything about *what a list is*: `items`, `selection_mode`,
//! `selected_index` / `selected_indices`, `top_index`, `multi_column`,
//! `column_width`, `item_height`, `sorted`, `drop_down_style`,
//! `drop_down_height`, `max_drop_down_items`, `integral_height`, and the whole
//! **selection state machine** — [`replica::ListBox::click`] with its four
//! `SelectionMode` rules, [`replica::CheckedListBox::toggle_check`] with the
//! two-way (not three-way) click cycle, the index shifting done by `add_item` /
//! `remove_item`. It is checked against the real toolkit; re-deriving it here
//! would re-derive its bugs. Each primitive owns its replica and
//! [`Deref`]s to it.
//!
//! The menu's item model is the replica's too: [`ContextMenuStrip`] carries
//! text, image key, shortcut, enabled, checked, separator and the recursive
//! `drop_down_items`, and [`kubuno_desktop_controls::toolstrip::measure_menu_item`] is
//! the pure width rule.
//!
//! # What this layer adds
//!
//! Only concepts the replica genuinely does not model: the **hot row**
//! (a `ToolStripItem` is a `Component`, not a `Control` — it cannot observe the
//! pointer, which is exactly why [`kubuno_desktop_controls::toolstrip::ToolStrip::paint_items`]
//! takes the hot index from its host), the combo's **open** flag and dropped
//! list **scroll**, the menu's **open submenu**, and the **keyboard**: the
//! active row (`aria-activedescendant`), type-ahead, and the ARIA listbox /
//! select-only combobox / menu key maps ([`ListKey`], [`ComboKey`],
//! [`MenuKey`]) — pure functions over the replica's selection machine.
//!
//! # Floating parts
//!
//! A combo's list and a menu are floating surfaces. Each can report where it
//! goes inside a viewport ([`ComboBox::drop_down_rect_in`],
//! [`Menu::submenu_rect_in`] with [`Menu::viewport`] — flip above / flip left,
//! clamp to [`VIEWPORT_EDGE`]), what it paints including its shadow
//! ([`ComboBox::drop_down_paint_bounds`], [`Menu::paint_bounds`],
//! [`FLOAT_SHADOW_MARGIN`]), and paint itself alone
//! ([`ComboBox::paint_trigger`] + [`ComboBox::paint_drop_down_at`]), so a
//! host can put it in a `host::popup` and let it leave its container and the
//! window.

use std::ops::{Deref, DerefMut};

use kubuno_drive_desktop_app_controls::{icon_name, Canvas, Rect};
use kubuno_desktop_controls::enums::{CheckState, Size};
use kubuno_desktop_controls::lists as replica;
use kubuno_desktop_controls::toolstrip::{
    measure_menu_item, ContextMenuStrip, Shortcut, StripItem, ToolStripLabel, ToolStripMenuItem,
    ToolStripSeparator,
};
use kubuno_desktop_controls::Control;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::metrics::{control, height, radius, space};
use crate::metrics::{SHADOW_GREY, SHADOW_MENU};
use crate::graphics::owner_draw::{self, DrawItemEventArgs, DrawItemState, DrawMode};
use crate::graphics::Graphics;
use crate::{Widget, WidgetState};

// ─────────────────────────────────────────────────────────────────────────────
// Family metrics
//
// `crate::metrics` is the crate's ONE table and everything it already answers is
// taken from it (`height::MENU_ITEM` for a menu row, `control::COMBO_ROW` for a
// list/popup row — they are different controls and the table says so —
// `radius::MENU_ITEM`, `radius::FLOAT`, `control::COMBO_ARROW`, the check-box
// geometry, the spacing scale). What is left below is the `MenuDropdown` and
// `Combobox` grids themselves: the lengths those two files state literally and
// no token names. They live here, next to their only user, until the shared
// table adopts them; each one quotes the declaration it is.
// ─────────────────────────────────────────────────────────────────────────────

/// The `MenuDropdown` grid, verbatim from `MenuDropdown.tsx`.
pub mod menu_metrics {
    use crate::metrics::space;

    /// The panel's own `padding: 5` on the scrolling layer. It doubles as the
    /// gutter the highlight pill is inset by — the file says so in as many
    /// words: « Side padding forms the gutter the highlight pill is inset by ».
    pub const PANEL_PAD: f32 = 5.0;
    /// A row's `padding: '5px 12px 5px 10px'` — left, then right.
    pub const ROW_PAD_L: f32 = 10.0;
    pub const ROW_PAD_R: f32 = 12.0;
    /// The icon cell: `width: 20`.
    pub const ICON_CELL: f32 = 20.0;
    /// The glyph drawn inside that cell. The CSS sets `fontSize: 14` on a cell
    /// that holds an SVG, which does not size the SVG; `flyout.rs` resolved the
    /// drawn size to 16 DIP when it ported this menu, and that is reused.
    pub const ICON_GLYPH: f32 = 16.0;
    /// `columnGap: 8` between the icon, label and shortcut columns.
    pub const COLUMN_GAP: f32 = space::SM;
    /// The shortcut cell's own `paddingLeft: 16`, on top of the column gap.
    pub const SHORTCUT_INSET: f32 = space::LG;
    /// A separator: `height: 1` with `margin: '5px 6px'`.
    pub const SEPARATOR_LINE: f32 = 1.0;
    pub const SEPARATOR_MARGIN_V: f32 = 5.0;
    pub const SEPARATOR_MARGIN_H: f32 = 6.0;
    /// `minWidth = 200` on a menu, `SUB_W = 220` on a cascaded submenu.
    pub const MIN_WIDTH: f32 = 200.0;
    pub const SUBMENU_WIDTH: f32 = 220.0;
    /// A section label row: `padding: '4px 10px'` at `--kb-text-meta`.
    pub const LABEL_PAD_V: f32 = space::XS;
    /// The submenu caret's column, right-aligned against [`ROW_PAD_R`].
    /// **No source**: `MenuDropdown` draws « ▸ » as text and never states a
    /// width. 12 is `flyout.rs`' answer (`CONTENT_RIGHT + 12.0`), kept so the
    /// two menus measure alike.
    pub const CARET_COLUMN: f32 = 12.0;
    /// `disabled:opacity-40` on a dead row.
    pub const DISABLED_ALPHA: f32 = 0.4;
    /// `opacity: 0.85` on the shortcut when the accent pill is under it.
    pub const HOT_SHORTCUT_ALPHA: f32 = 0.85;

    /// Where the icon column starts, from the panel's left edge.
    pub const ICON_LEFT: f32 = PANEL_PAD + ROW_PAD_L;
    /// Where the label column starts: icon cell + `columnGap`.
    pub const LABEL_LEFT: f32 = ICON_LEFT + ICON_CELL + COLUMN_GAP;
    /// Where the shortcut / caret column ends, from the panel's right edge.
    pub const CONTENT_RIGHT: f32 = PANEL_PAD + ROW_PAD_R;
    /// The gap between the label and the shortcut columns.
    pub const SHORTCUT_GAP: f32 = COLUMN_GAP + SHORTCUT_INSET;
}

/// The list-panel grid, verbatim from `Combobox.tsx`'s `role="listbox"` — the
/// one panel both [`ListBox`] and a [`ComboBox`]'s popup are.
pub mod list_metrics {
    use crate::metrics::{control, space};

    /// The panel's `p-1`.
    pub const PANEL_PAD: f32 = space::XS;
    /// A row's `px-2`.
    pub const ROW_PAD_H: f32 = space::SM;
    /// `gap-2` between a row's cells.
    pub const CELL_GAP: f32 = space::SM;
    /// The selected-row tick gutter (`w-4`) and the tick itself
    /// (`<Check size={14} />`).
    pub const TICK_CELL: f32 = 16.0;
    pub const TICK_GLYPH: f32 = 14.0;
    /// The panel's own outline: `border` — a rule, like every other hairline
    /// in this system.
    pub const BORDER: f32 = control::SEPARATOR;
    /// `minWidth: 200` on the popup panel.
    pub const MIN_WIDTH: f32 = 200.0;
    /// `maxHeight` on the select's popup — `Dropdown.tsx` bounds its own list
    /// at 280 before it scrolls.
    pub const DROP_DOWN_MAX: f32 = 280.0;
}

/// The tag [`MenuEntry::danger`] writes into `ToolStripItem::tag` to mark a
/// destructive row.
///
/// `MenuItem` in the web has a `danger?: boolean`; .NET has no such concept, and
/// `Tag` is the toolkit's own general-purpose per-item payload — so the marker
/// rides there rather than becoming a second, parallel item list here.
pub const DANGER_TAG: &str = "kubuno:danger";

/// The same colour at a fraction of its alpha.
///
/// The web fades a disabled row and the hovered shortcut with CSS `opacity`,
/// for which there is no token: alpha over the surface composites the same way.
fn faded(color: &D2D1_COLOR_F, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: color.a * alpha, ..*color }
}

/// A layered menu/popup shadow under a floating panel.
fn drop_shadow(c: &dyn Canvas, panel: &Rect) {
    c.draw_shadow(panel, radius::FLOAT, &SHADOW_MENU, SHADOW_GREY);
}

/// How far the floating shadow ([`SHADOW_MENU`]) reaches outside its panel:
/// the largest `blur + spread + |dy|` of its layers (`0 2px 6px 2px` → 10).
/// A caller hosting a menu or a dropped list in its own `host::popup` grows
/// the popup by this much on every side so the shadow is not cut off.
pub const FLOAT_SHADOW_MARGIN: f32 = 10.0;

/// `rect` grown by `m` on every side.
fn inflate(rect: Rect, m: f32) -> Rect {
    Rect::new(rect.left - m, rect.top - m, rect.right + m, rect.bottom + m)
}

/// The smallest rectangle holding both.
fn union(a: Rect, b: Rect) -> Rect {
    Rect::new(a.left.min(b.left), a.top.min(b.top), a.right.max(b.right), a.bottom.max(b.bottom))
}

/// The margin a floating surface keeps from the viewport edge — `const M = 8`
/// in both `MenuDropdown.tsx`'s clamp and `Combobox.tsx`' `measure`.
pub const VIEWPORT_EDGE: f32 = 8.0;

/// A measured text width, made safe to lay text into.
///
/// DirectWrite trims a line whose layout box is even a fraction of a DIP
/// narrower than the text, and the painter snaps boxes onto the physical pixel
/// grid (which can take up to one device pixel off the right edge). A column
/// sized to the RAW measure therefore ellipsizes its own widest label — the
/// « Réinitialiser le mot de pas… » defect. Rounding up and adding one DIP is
/// what a browser's `auto` column does implicitly (it lays out at whole CSS px).
fn text_width(c: &dyn Canvas, text: &str, format: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat) -> f32 {
    let w = c.measure(text, format);
    if w <= 0.0 {
        0.0
    } else {
        w.ceil() + TEXT_SLACK
    }
}

/// See [`text_width`].
const TEXT_SLACK: f32 = 1.0;

// ─────────────────────────────────────────────────────────────────────────────
// Keyboard — the ARIA listbox / menu patterns, as pure functions
//
// The replica owns the SELECTION; what the keyboard adds is an active row (the
// web's `aria-activedescendant`) and the arithmetic that moves it. Everything
// here is canvas-free so the tests can drive it.
// ─────────────────────────────────────────────────────────────────────────────

/// A navigation key, as a list-like control understands it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKey {
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

/// Where the active row goes for `key` — the ARIA listbox rule: no wrap, Home /
/// End jump to the ends, PageUp / PageDown move by `page` rows (at least one).
/// With no active row yet, a downward key lands on the first row and an upward
/// one on the last. `None` for an empty list.
pub fn list_step(current: Option<usize>, len: usize, key: ListKey, page: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let last = len - 1;
    let page = page.max(1);
    let Some(i) = current.map(|i| i.min(last)) else {
        return Some(match key {
            ListKey::Up | ListKey::End | ListKey::PageUp => last,
            ListKey::Down | ListKey::Home | ListKey::PageDown => 0,
        });
    };
    Some(match key {
        ListKey::Up => i.saturating_sub(1),
        ListKey::Down => (i + 1).min(last),
        ListKey::Home => 0,
        ListKey::End => last,
        ListKey::PageUp => i.saturating_sub(page),
        ListKey::PageDown => (i + page).min(last),
    })
}

/// The next row `usable` accepts, walking from `from` in one direction and
/// wrapping — `Combobox.tsx`' `firstEnabled`, which never lets the highlight
/// rest on a disabled option. `from = None` starts before the first row
/// (forward) or after the last (backward). `None` when no row is usable.
pub fn step_wrapping(
    from: Option<usize>,
    len: usize,
    forward: bool,
    usable: impl Fn(usize) -> bool,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let mut i = match (from, forward) {
        (Some(i), true) => (i.min(len - 1) + 1) % len,
        (Some(i), false) => (i.min(len - 1) + len - 1) % len,
        (None, true) => 0,
        (None, false) => len - 1,
    };
    for _ in 0..len {
        if usable(i) {
            return Some(i);
        }
        i = if forward { (i + 1) % len } else { (i + len - 1) % len };
    }
    None
}

/// Case- and accent-insensitive form of a label, for type-ahead: the web folds
/// both sides through NFD and strips the marks (`uiText.foldText`); the Latin
/// letters a French UI meets are folded here without pulling in a Unicode
/// table.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars().flat_map(char::to_lowercase) {
        match ch {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => out.push('a'),
            'ç' => out.push('c'),
            'è' | 'é' | 'ê' | 'ë' => out.push('e'),
            'ì' | 'í' | 'î' | 'ï' => out.push('i'),
            'ñ' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => out.push('o'),
            'ù' | 'ú' | 'û' | 'ü' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'œ' => out.push_str("oe"),
            'æ' => out.push_str("ae"),
            other => out.push(other),
        }
    }
    out
}

/// The typed-prefix buffer of a list's type-ahead (ARIA listbox: « type a
/// character: focus moves to the next item with a name that starts with the
/// typed character; type several in rapid succession: focus moves to the item
/// starting with the string »).
#[derive(Debug, Clone, Default)]
pub struct TypeAhead {
    buffer:  String,
    last_ms: u64,
}

impl TypeAhead {
    /// A pause longer than this starts a new prefix.
    pub const TIMEOUT_MS: u64 = 1000;

    /// Appends `text` typed at `now_ms` and returns the current prefix.
    pub fn push(&mut self, text: &str, now_ms: u64) -> &str {
        if now_ms.saturating_sub(self.last_ms) > Self::TIMEOUT_MS {
            self.buffer.clear();
        }
        self.last_ms = now_ms;
        self.buffer.push_str(text);
        &self.buffer
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// The prefix typed so far.
    pub fn prefix(&self) -> &str {
        &self.buffer
    }
}

/// Which label a type-ahead `prefix` lands on, from the active row `current`.
///
/// A prefix of ONE repeated character (`d`, `dd`, `ddd`) cycles through the
/// labels starting with it, from the row after `current` — the Windows and
/// ARIA behaviour for tapping the same key. A longer prefix searches from
/// `current` itself, so extending « do » to « doc » keeps the row it is on.
/// Both wrap; matching is [`fold`]ed.
pub fn type_ahead_match<'a>(
    labels: impl IntoIterator<Item = &'a str>,
    prefix: &str,
    current: Option<usize>,
) -> Option<usize> {
    let labels: Vec<String> = labels.into_iter().map(fold).collect();
    let needle = fold(prefix);
    let mut chars = needle.chars();
    let first = chars.next()?;
    let len = labels.len();
    if len == 0 {
        return None;
    }
    let repeated = chars.all(|c| c == first);
    let (needle, start) = if repeated {
        (first.to_string(), current.map_or(0, |i| (i + 1) % len))
    } else {
        (needle, current.map_or(0, |i| i.min(len - 1)))
    };
    (0..len).map(|k| (start + k) % len).find(|&i| labels[i].starts_with(&needle))
}

/// Moves the active row of a replica list for `key` and applies the selection
/// rule of its `SelectionMode` — the ARIA listbox keyboard model mapped onto
/// the toolkit's:
///
/// * `One` — the selection follows the active row (single-select listbox);
/// * `MultiExtended` — plain arrows select just the row, `Shift` extends from
///   the anchor ([`replica::ListBox::click`] with shift), `Ctrl` moves the
///   active row without touching the selection;
/// * `MultiSimple` / `None` — the active row moves alone (Space toggles).
///
/// Returns whether anything moved.
fn list_key(
    list: &mut replica::ListBox,
    focus: &mut Option<usize>,
    key: ListKey,
    page: usize,
    ctrl: bool,
    shift: bool,
) -> bool {
    use replica::SelectionMode as M;
    let current = focus.or_else(|| usize::try_from(list.selected_index()).ok());
    let Some(next) = list_step(current, list.items.len(), key, page) else { return false };
    let moved = Some(next) != *focus;
    *focus = Some(next);
    match list.selection_mode {
        M::One => {
            if list.selected_index() != next as i32 {
                list.set_selected_index(next as i32);
                return true;
            }
        }
        M::MultiExtended if !ctrl => {
            list.click(next, false, shift);
            return true;
        }
        _ => {}
    }
    moved
}

/// Space on the active row: toggles it in a multi-selection (`Ctrl+Space` in
/// `MultiExtended`, plain Space in `MultiSimple`), selects it in `One`.
fn list_space(list: &mut replica::ListBox, focus: Option<usize>, ctrl: bool) -> bool {
    use replica::SelectionMode as M;
    let Some(i) = focus.filter(|&i| i < list.items.len()) else { return false };
    match list.selection_mode {
        M::One => list.set_selected_index(i as i32),
        M::MultiSimple => list.click(i, false, false),
        M::MultiExtended => list.click(i, ctrl, false),
        M::None => return false,
    }
    true
}

/// The `TopIndex` that brings row `i` into a window of `per_page` rows,
/// moving as little as possible (`scrollIntoView({ block: 'nearest' })`).
fn top_to_show(top: usize, i: usize, per_page: usize) -> usize {
    let per_page = per_page.max(1);
    if i < top {
        i
    } else if i >= top + per_page {
        i + 1 - per_page
    } else {
        top
    }
}

/// Paints the resting scroll indicator along the right edge of `content` when
/// `len` rows do not fit in `per_page` — the list's `overflow-y-auto`, drawn
/// as the product's thin overlay bar ([`crate::range::ScrollBar`]) so a
/// hidden row is never silently out of reach.
/// `panel` is the list's rounded outline (`rounded-lg`): the bar stays inside it.
fn paint_row_scrollbar(c: &dyn Canvas, panel: Rect, content: Rect, len: usize, per_page: usize, top: usize, row_h: f32) {
    if per_page == 0 || len <= per_page {
        return;
    }
    let extent = len as f32 * row_h;
    let viewport = per_page as f32 * row_h;
    if let Some(bar) = crate::range::ScrollBar::from_content(false, extent, viewport, top as f32 * row_h) {
        let (frame, r) = crate::range::inside_border(panel, radius::LG);
        let rail = crate::range::fit_rail(bar.rail(&content), frame, r);
        crate::range::paint_bar_in(c, &bar, rail, WidgetState::REST, frame, r);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The row grid shared by ListBox and CheckedListBox
// ─────────────────────────────────────────────────────────────────────────────

/// Where the rows of a list go inside its panel.
///
/// Pure geometry, split out so the two list boxes place, hit-test and measure
/// through one implementation — and so it is unit-testable with no canvas.
#[derive(Debug, Clone, Copy)]
struct Grid {
    /// How many items the list holds.
    len:       usize,
    /// `TopIndex` — the first row scrolled into view (single column only).
    top:       usize,
    row_h:     f32,
    /// `Some(width)` in `MultiColumn` mode.
    col_w:     Option<f32>,
}

impl Grid {
    /// The rectangle the rows live in: `bounds` less the panel's outline and
    /// its `p-1`.
    fn content(bounds: Rect) -> Rect {
        let inset = list_metrics::BORDER + list_metrics::PANEL_PAD;
        Rect::new(
            bounds.left + inset,
            bounds.top + inset,
            bounds.right - inset,
            bounds.bottom - inset,
        )
    }

    /// How many whole rows fit in `bounds` — WinForms' integral height, applied
    /// to the Kubuno row rather than the toolkit's.
    fn rows_per_column(&self, bounds: Rect) -> usize {
        let inner = Self::content(bounds);
        if self.row_h <= 0.0 {
            return 0;
        }
        (((inner.bottom - inner.top) / self.row_h).floor().max(0.0)) as usize
    }

    /// Where item `i` is drawn, or `None` when it is scrolled out of view or
    /// falls past the last column that fits.
    fn cell(&self, bounds: Rect, i: usize) -> Option<Rect> {
        if i >= self.len {
            return None;
        }
        let inner = Self::content(bounds);
        let per_col = self.rows_per_column(bounds);
        if per_col == 0 {
            return None;
        }
        match self.col_w {
            None => {
                let k = i.checked_sub(self.top)?;
                if k >= per_col {
                    return None;
                }
                let y = inner.top + k as f32 * self.row_h;
                Some(Rect::new(inner.left, y, inner.right, y + self.row_h))
            }
            Some(w) if w > 0.0 => {
                let (col, row) = (i / per_col, i % per_col);
                let x = inner.left + col as f32 * w;
                if x + w > inner.right {
                    return None;
                }
                let y = inner.top + row as f32 * self.row_h;
                Some(Rect::new(x, y, x + w, y + self.row_h))
            }
            Some(_) => None,
        }
    }

    /// Which item `(x, y)` lands on.
    fn at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        let inner = Self::content(bounds);
        if !inner.contains(x, y) || self.row_h <= 0.0 {
            return None;
        }
        let per_col = self.rows_per_column(bounds);
        if per_col == 0 {
            return None;
        }
        let row = ((y - inner.top) / self.row_h).floor() as usize;
        if row >= per_col {
            return None;
        }
        let i = match self.col_w {
            None => self.top + row,
            Some(w) if w > 0.0 => {
                let col = ((x - inner.left) / w).floor() as usize;
                col * per_col + row
            }
            Some(_) => return None,
        };
        (i < self.len).then_some(i)
    }

    /// The panel height that shows `rows` whole rows.
    fn height_for(rows: usize, row_h: f32) -> f32 {
        rows as f32 * row_h + 2.0 * (list_metrics::BORDER + list_metrics::PANEL_PAD)
    }
}

/// Paints a list panel: shadow-free surface, `--color-border` outline.
fn paint_list_panel(c: &dyn Canvas, bounds: Rect, disabled: bool) {
    let t = c.theme();
    // `Combobox.tsx`: the listbox panel is `rounded-lg border border-border
    // bg-white` — surface-0, which is `layer_background` on the desktop. A dead
    // control takes the field's disabled face (`Input`: `disabled:bg-surface-2`).
    let face = if disabled { t.surface_2 } else { t.layer_background };
    c.fill_rounded(&bounds, radius::LG, &face);
    c.stroke_rounded(&bounds, radius::LG, &t.card_stroke);
}

/// The focus-visible ring of a focusable list panel — `.kb-field-focus`'s
/// `outline … var(--color-primary)`, drawn inward at the crate's field-ring
/// width ([`FOCUS_RING`]) so it never leaves the control's bounds.
fn paint_list_ring(c: &dyn Canvas, bounds: Rect) {
    let t = c.theme();
    c.stroke_rounded_w(&bounds, radius::LG, &t.accent, FOCUS_RING);
}

/// One list row: the selection / hover fill and the label.
///
/// `tick` reserves the `w-4` gutter `Combobox.tsx` gives a selected option; a
/// plain [`ListBox`] does not use it, [`CheckedListBox`] replaces it with its
/// check well.
fn paint_list_row(c: &dyn Canvas, row: Rect, label: &str, st: RowState) {
    let t = c.theme();
    let f = c.formats();

    // `rounded-md` on the option, inset by the panel padding the panel already
    // applied — so the fill stops short of the outline like the web's does.
    if st.selected {
        c.fill_rounded(&row, radius::SM, &t.list_selected);
    } else if st.hot || st.active {
        // `isAct ? 'bg-surface-2'` — the keyboard's active option wears the
        // hover fill, exactly as the web's listbox shares one `active` state
        // between the arrows and `onMouseEnter`.
        c.fill_rounded(&row, radius::SM, &t.row_hover);
    }

    let ink = if st.disabled { faded(&t.text_primary, menu_metrics::DISABLED_ALPHA) } else { t.text_primary };
    let text_rect = Rect::new(row.left + st.text_inset, row.top, row.right - list_metrics::ROW_PAD_H, row.bottom);
    c.text_ellipsis(label, &text_rect, &f.body, &ink);
}

/// What a row is painted in.
#[derive(Debug, Clone, Copy, Default)]
struct RowState {
    selected:   bool,
    hot:        bool,
    /// The keyboard's active row, while the list holds the focus.
    active:     bool,
    disabled:   bool,
    /// Where the label starts, from the row's left edge.
    text_inset: f32,
}

// ─────────────────────────────────────────────────────────────────────────────
// ListBox
// ─────────────────────────────────────────────────────────────────────────────

/// A scrolling list of strings — the replica, with Kubuno rows.
///
/// Everything about the list itself is [`replica::ListBox`]'s: the items, the
/// four `SelectionMode` rules and the `Ctrl`/`Shift` arithmetic in
/// [`replica::ListBox::click`], `TopIndex`, `MultiColumn`, `Sorted`. What
/// changes is the row: a 32 DIP `--color-surface` row with a `rounded-md`
/// selection fill, instead of the toolkit's 15 DIP `#0078D7` band.
#[derive(Clone, Default)]
pub struct ListBox {
    inner: replica::ListBox,
    /// The row under the pointer, which the model cannot know: `SelectedIndex`
    /// is a selection, not a hover, and the replica has no pointer.
    pub hot_index: Option<usize>,
    /// The keyboard's active row — `aria-activedescendant`. Distinct from the
    /// selection in the multi-modes (Ctrl+arrows move it alone); painted as the
    /// web's `bg-surface-2` active option while the list holds the focus.
    pub focus_index: Option<usize>,
    /// The type-ahead prefix buffer.
    pub type_ahead: TypeAhead,
    /// `DrawMode.OwnerDrawVariable`: the height of each item, as `MeasureItem` answered — filled
    /// by [`ListBox::measure_items`]; used for painting, hit-testing and scrolling while it holds
    /// one height per item (single column only, as in WinForms).
    pub item_heights: Option<Vec<f32>>,
}

impl Deref for ListBox {
    type Target = replica::ListBox;
    fn deref(&self) -> &replica::ListBox {
        &self.inner
    }
}
impl DerefMut for ListBox {
    fn deref_mut(&mut self) -> &mut replica::ListBox {
        &mut self.inner
    }
}

impl ListBox {
    pub fn new() -> Self {
        Self::default()
    }

    /// The Kubuno row height: [`control::COMBO_ROW`] (the web's `py-1.5` around a
    /// 20 DIP line box = 32), unless `ItemHeight` was set explicitly — the
    /// toolkit lets an explicit value win, and so does this.
    pub fn row_height(&self) -> f32 {
        kubuno_row_height(self.inner.item_height)
    }

    fn grid(&self) -> Grid {
        Grid {
            len:   self.inner.items.len(),
            top:   self.inner.top_index,
            row_h: self.row_height(),
            col_w: self.inner.multi_column.then(|| self.column_width_dip()),
        }
    }

    /// The column width in `MultiColumn` mode. `ColumnWidth = 0` means « auto »,
    /// which WinForms resolves from the widest item; with no canvas in hand the
    /// fallback is a whole [`list_metrics::MIN_WIDTH`] column.
    fn column_width_dip(&self) -> f32 {
        if self.inner.column_width > 0 {
            self.inner.column_width as f32
        } else {
            list_metrics::MIN_WIDTH
        }
    }

    /// **Which item** `(x, y)` lands on, in the same space as the `bounds` it
    /// was painted into — what a host needs before it can call
    /// [`replica::ListBox::click`].
    ///
    /// Honours `TopIndex` and `MultiColumn`; returns `None` on the panel's
    /// padding, on an empty row past the last item, and outside `bounds`.
    pub fn item_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        if let Some(h) = self.variable_heights() {
            return var_at(h, self.inner.top_index, Grid::content(bounds), x, y);
        }
        self.grid().at(bounds, x, y)
    }

    /// Where item `i` is drawn, or `None` when it is scrolled out of view.
    pub fn item_rect(&self, bounds: Rect, i: usize) -> Option<Rect> {
        if let Some(h) = self.variable_heights() {
            return var_cell(h, self.inner.top_index, Grid::content(bounds), i);
        }
        self.grid().cell(bounds, i)
    }

    /// How many whole rows `bounds` shows.
    pub fn visible_rows(&self, bounds: Rect) -> usize {
        if let Some(h) = self.variable_heights() {
            return var_rows(h, self.inner.top_index, Grid::content(bounds));
        }
        self.grid().rows_per_column(bounds)
    }

    /// The panel height that shows `rows` whole rows — the Kubuno counterpart of
    /// `IntegralHeight`.
    pub fn height_for_rows(&self, rows: usize) -> f32 {
        if let Some(h) = self.variable_heights() {
            let sum: f32 = h.iter().take(rows).sum();
            return sum + 2.0 * (list_metrics::BORDER + list_metrics::PANEL_PAD);
        }
        Grid::height_for(rows, self.row_height())
    }

    /// The per-item heights in force: `OwnerDrawVariable`, single column, one height per item.
    fn variable_heights(&self) -> Option<&[f32]> {
        if self.inner.draw_mode != DrawMode::OwnerDrawVariable || self.inner.multi_column {
            return None;
        }
        self.item_heights.as_deref().filter(|h| h.len() == self.inner.items.len())
    }

    /// `OwnerDrawVariable`: asks the owner-draw handler lent for this paint
    /// ([`owner_draw::with_handler`]) for each item's height (`MeasureItem`) and keeps them in
    /// [`ListBox::item_heights`]; clears them in the other draw modes or without a handler. Call it
    /// before hit-testing and painting, inside the same `with_handler` scope.
    pub fn measure_items(&mut self, canvas: &dyn Canvas, bounds: Rect) {
        if self.inner.draw_mode != DrawMode::OwnerDrawVariable || self.inner.multi_column {
            self.item_heights = None;
            return;
        }
        let g = Graphics::new(canvas);
        let content = Grid::content(bounds);
        let items = &self.inner.items;
        let heights = owner_draw::measure_items(&g, "ListBox", items.len(), self.row_height(), content.right - content.left, |i| items[i].clone());
        self.item_heights = heights;
    }

    /// Handles a navigation key for a list painted into `bounds`: moves the
    /// active row, applies the `SelectionMode` rule (see [`ListKey`]) and
    /// scrolls the row into view. Returns whether anything changed.
    pub fn handle_key(&mut self, bounds: Rect, key: ListKey, ctrl: bool, shift: bool) -> bool {
        let page = self.visible_rows(bounds);
        let changed = list_key(&mut self.inner, &mut self.focus_index, key, page, ctrl, shift);
        if let Some(i) = self.focus_index {
            self.ensure_visible(bounds, i);
        }
        changed
    }

    /// Space on the active row — see [`ListKey`]. Returns whether it acted.
    pub fn press_space(&mut self, ctrl: bool) -> bool {
        list_space(&mut self.inner, self.focus_index, ctrl)
    }

    /// A pointer press on row `i`: the replica's click rule, and the active row
    /// follows the pointer (as a click on a web option moves the highlight).
    pub fn pointer_select(&mut self, i: usize, ctrl: bool, shift: bool) {
        if i < self.inner.items.len() {
            self.inner.click(i, ctrl, shift);
            self.focus_index = Some(i);
        }
    }

    /// Type-ahead: appends `text` (typed at `now_ms`) to the prefix and moves
    /// the active row — and, in a single-select list, the selection — to the
    /// matching item. Returns the row it landed on.
    pub fn type_to_select(&mut self, bounds: Rect, text: &str, now_ms: u64) -> Option<usize> {
        if text.is_empty() {
            return None;
        }
        let prefix = self.type_ahead.push(text, now_ms).to_string();
        let from = self.focus_index.or_else(|| usize::try_from(self.inner.selected_index()).ok());
        let i = type_ahead_match(self.inner.items.iter().map(String::as_str), &prefix, from)?;
        self.focus_index = Some(i);
        if matches!(self.inner.selection_mode, replica::SelectionMode::One | replica::SelectionMode::MultiExtended) {
            self.inner.click(i, false, false);
        }
        self.ensure_visible(bounds, i);
        Some(i)
    }

    /// Scrolls `TopIndex` just enough to show row `i` (single column).
    pub fn ensure_visible(&mut self, bounds: Rect, i: usize) {
        if self.inner.multi_column {
            return;
        }
        let per = self.visible_rows(bounds);
        let top = top_to_show(self.inner.top_index, i, per);
        self.set_top_index_clamped(bounds, top);
    }

    /// Scrolls by `rows` (positive = down), clamped so the last page stays
    /// full — what a wheel notch does to a web list.
    pub fn scroll_rows(&mut self, bounds: Rect, rows: i32) {
        let top = (self.inner.top_index as i64 + rows as i64).max(0) as usize;
        self.set_top_index_clamped(bounds, top);
    }

    /// The largest `TopIndex` that still fills `bounds`.
    pub fn max_top_index(&self, bounds: Rect) -> usize {
        if let Some(h) = self.variable_heights() {
            return var_max_top(h, Grid::content(bounds));
        }
        self.inner.items.len().saturating_sub(self.visible_rows(bounds).max(1))
    }

    fn set_top_index_clamped(&mut self, bounds: Rect, top: usize) {
        let top = top.min(self.max_top_index(bounds));
        self.inner.set_top_index(top);
    }
}

// ── Variable-height rows (`OwnerDrawVariable`, single column) ─────────────────────────────────

/// Where item `i` goes when rows have their own heights, `None` above `top` or below the content.
fn var_cell(heights: &[f32], top: usize, content: Rect, i: usize) -> Option<Rect> {
    if i < top || i >= heights.len() {
        return None;
    }
    let y = content.top + heights[top..i].iter().sum::<f32>();
    if y >= content.bottom {
        return None;
    }
    Some(Rect::new(content.left, y, content.right, y + heights[i]))
}

/// Which item `(x, y)` lands on when rows have their own heights.
fn var_at(heights: &[f32], top: usize, content: Rect, x: f32, y: f32) -> Option<usize> {
    if !content.contains(x, y) {
        return None;
    }
    let mut row_top = content.top;
    for (i, h) in heights.iter().enumerate().skip(top) {
        if y < row_top + h {
            return Some(i);
        }
        row_top += h;
        if row_top >= content.bottom {
            break;
        }
    }
    None
}

/// How many whole rows fit from `top` (at least one when any item is left).
fn var_rows(heights: &[f32], top: usize, content: Rect) -> usize {
    let avail = content.bottom - content.top;
    let mut used = 0.0;
    let mut n = 0;
    for h in heights.iter().skip(top) {
        if used + h > avail && n > 0 {
            break;
        }
        used += h;
        n += 1;
    }
    n
}

/// The largest `TopIndex` whose rows reach the last item within the content.
fn var_max_top(heights: &[f32], content: Rect) -> usize {
    let avail = content.bottom - content.top;
    let mut used = 0.0;
    let mut k = heights.len();
    while k > 0 && used + heights[k - 1] <= avail {
        used += heights[k - 1];
        k -= 1;
    }
    k.min(heights.len().saturating_sub(1))
}

/// The owner-draw state of a list row.
fn item_state(selected: bool, hot: bool, focus: bool, disabled: bool) -> DrawItemState {
    DrawItemState::NONE
        .with(DrawItemState::SELECTED, selected)
        .with(DrawItemState::HOT_LIGHT, hot)
        .with(DrawItemState::FOCUS, focus)
        .with(DrawItemState::DISABLED, disabled)
}

/// The Kubuno row height for a stored `ItemHeight` (`0` = auto).
fn kubuno_row_height(item_height: i32) -> f32 {
    if item_height > 0 {
        item_height as f32
    } else {
        control::COMBO_ROW
    }
}

impl Widget for ListBox {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let f = canvas.formats();
        let widest = self
            .inner
            .items
            .iter()
            .fold(0.0_f32, |w, it| w.max(text_width(canvas, it, &f.body)));
        let chrome = 2.0 * (list_metrics::BORDER + list_metrics::PANEL_PAD + list_metrics::ROW_PAD_H);
        let width = (widest + chrome).max(list_metrics::MIN_WIDTH);
        Size::new(width, self.height_for_rows(self.inner.items.len().max(1)))
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let dead = state.disabled || !self.inner.control().enabled;
        paint_list_panel(canvas, bounds, dead);

        // Rows are clipped to the panel's CONTENT box, so a partly scrolled row
        // never paints over the outline or its rounded corners.
        let content = Grid::content(bounds);
        canvas.push_clip(&content);
        let grid = self.grid();
        // Owner-draw (`DrawMode`): each item goes to the lent handler first; without a handler
        // (or when it asks for the default) the row is painted as usual.
        let owner = (self.inner.draw_mode != DrawMode::Normal && owner_draw::has_handler()).then(|| Graphics::new(canvas));
        // Heights measured for this paint when the host did not measure them beforehand.
        let measured = match (&owner, self.variable_heights()) {
            (Some(g), None) if self.inner.draw_mode == DrawMode::OwnerDrawVariable && !self.inner.multi_column => {
                let items = &self.inner.items;
                owner_draw::measure_items(g, "ListBox", items.len(), self.row_height(), content.right - content.left, |i| items[i].clone())
            }
            _ => None,
        };
        let heights = self.variable_heights().or(measured.as_deref());
        for (i, label) in self.inner.items.iter().enumerate() {
            let cell = match heights {
                Some(h) => var_cell(h, self.inner.top_index, content, i),
                None => grid.cell(bounds, i),
            };
            let Some(row) = cell else { continue };
            let row_state = RowState {
                selected:   self.inner.is_selected(i),
                hot:        self.hot_index == Some(i) && !dead,
                active:     state.focused && !dead && self.focus_index == Some(i),
                disabled:   dead,
                text_inset: list_metrics::ROW_PAD_H,
            };
            if let Some(g) = &owner {
                let st = item_state(row_state.selected, row_state.hot, row_state.active, dead);
                let mut e = DrawItemEventArgs::new(g, "ListBox", Some(i), row, st, label.as_str());
                if owner_draw::draw_item(&mut e) {
                    continue;
                }
            }
            paint_list_row(canvas, row, label, row_state);
        }
        if !self.inner.multi_column {
            match heights {
                Some(h) => {
                    // Scroll indicator in item units: the average row stands for one.
                    let per = var_rows(h, self.inner.top_index, content);
                    let avg = h.iter().sum::<f32>() / h.len().max(1) as f32;
                    paint_row_scrollbar(canvas, bounds, content, h.len(), per, self.inner.top_index, avg);
                }
                None => {
                    let per = grid.rows_per_column(bounds);
                    paint_row_scrollbar(canvas, bounds, content, grid.len, per, grid.top, grid.row_h);
                }
            }
        }
        canvas.pop_clip();
        if state.show_focus_ring() && !dead {
            paint_list_ring(canvas, bounds);
        }
    }

    fn type_name(&self) -> &'static str {
        "ListBox"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CheckedListBox
// ─────────────────────────────────────────────────────────────────────────────

/// A [`ListBox`] whose rows carry a check box.
///
/// The replica carries it usefully and this wrapper adds nothing to the model:
/// the per-item `CheckState`, `CheckOnClick`, the `CheckedIndices` that include
/// the indeterminate rows, the coercion of `SelectionMode` to `One`/`None`, and
/// — the one that catches ports — [`replica::CheckedListBox::toggle_check`],
/// where a click on an `Indeterminate` row goes to `Unchecked`, **not** to
/// `Checked`. All of it is reached through [`Deref`].
///
/// The check well is the Kubuno one: [`control::CHECK_BOX`] at 18 DIP with a
/// [`control::CHECK_TICK`] tick, not the toolkit's 13 DIP `BP_CHECKBOX`.
#[derive(Clone, Default)]
pub struct CheckedListBox {
    inner: replica::CheckedListBox,
    /// The row under the pointer — see [`ListBox::hot_index`].
    pub hot_index: Option<usize>,
    /// The keyboard's active row — see [`ListBox::focus_index`].
    pub focus_index: Option<usize>,
    /// The type-ahead prefix buffer.
    pub type_ahead: TypeAhead,
}

impl Deref for CheckedListBox {
    type Target = replica::CheckedListBox;
    fn deref(&self) -> &replica::CheckedListBox {
        &self.inner
    }
}
impl DerefMut for CheckedListBox {
    fn deref_mut(&mut self) -> &mut replica::CheckedListBox {
        &mut self.inner
    }
}

impl CheckedListBox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Same rule as [`ListBox::row_height`]. The Kubuno row is 32 and the check
    /// well 18, so — unlike the toolkit, which had to grow a 15 DIP row to 18 —
    /// nothing needs adding for the box to fit.
    pub fn row_height(&self) -> f32 {
        kubuno_row_height(self.inner.item_height)
    }

    fn grid(&self) -> Grid {
        Grid {
            len:   self.inner.items.len(),
            top:   self.inner.top_index,
            row_h: self.row_height(),
            col_w: None, // A CheckedListBox is single-column in the toolkit too.
        }
    }

    /// Which item `(x, y)` lands on — see [`ListBox::item_at`].
    pub fn item_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        self.grid().at(bounds, x, y)
    }

    /// Whether `(x, y)` lands on the **check well** of a row rather than on its
    /// label, which is what decides a click's meaning when `CheckOnClick` is
    /// off: the toolkit toggles from the box, selects from the label.
    pub fn check_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        let i = self.item_at(bounds, x, y)?;
        let row = self.grid().cell(bounds, i)?;
        check_rect(row).contains(x, y).then_some(i)
    }

    pub fn item_rect(&self, bounds: Rect, i: usize) -> Option<Rect> {
        self.grid().cell(bounds, i)
    }

    pub fn height_for_rows(&self, rows: usize) -> f32 {
        Grid::height_for(rows, self.row_height())
    }

    /// How many whole rows `bounds` shows.
    pub fn visible_rows(&self, bounds: Rect) -> usize {
        self.grid().rows_per_column(bounds)
    }

    /// Arrow / Home / End / PageUp / PageDown — see [`ListBox::handle_key`].
    /// The selection of a checked list is single, so the selection follows the
    /// active row; the checks are left alone (Space toggles them).
    pub fn handle_key(&mut self, bounds: Rect, key: ListKey) -> bool {
        let page = self.visible_rows(bounds);
        let changed = list_key(&mut self.inner.list, &mut self.focus_index, key, page, false, false);
        if let Some(i) = self.focus_index {
            self.ensure_visible(bounds, i);
        }
        changed
    }

    /// Space: toggles the active row's check with the toolkit's two-way cycle
    /// ([`replica::CheckedListBox::toggle_check`]) and selects it — what a
    /// check box row does under the ARIA `checkbox` pattern.
    pub fn press_space(&mut self) -> bool {
        let Some(i) = self.focus_index.filter(|&i| i < self.inner.items.len()) else { return false };
        self.inner.list.click(i, false, false);
        self.inner.toggle_check(i);
        true
    }

    /// A pointer press on row `i`. `on_well` says whether it hit the check
    /// well ([`CheckedListBox::check_at`]): the well always toggles, the label
    /// only when `CheckOnClick` is set — the toolkit's rule.
    pub fn pointer_select(&mut self, i: usize, on_well: bool) {
        if i >= self.inner.items.len() {
            return;
        }
        self.focus_index = Some(i);
        self.inner.list.click(i, false, false);
        if on_well || self.inner.check_on_click {
            self.inner.toggle_check(i);
        }
    }

    /// Type-ahead — see [`ListBox::type_to_select`].
    pub fn type_to_select(&mut self, bounds: Rect, text: &str, now_ms: u64) -> Option<usize> {
        if text.is_empty() {
            return None;
        }
        let prefix = self.type_ahead.push(text, now_ms).to_string();
        let from = self.focus_index.or_else(|| usize::try_from(self.inner.selected_index()).ok());
        let i = type_ahead_match(self.inner.items.iter().map(String::as_str), &prefix, from)?;
        self.focus_index = Some(i);
        self.inner.list.click(i, false, false);
        self.ensure_visible(bounds, i);
        Some(i)
    }

    /// Scrolls just enough to show row `i`.
    pub fn ensure_visible(&mut self, bounds: Rect, i: usize) {
        let per = self.visible_rows(bounds);
        let top = top_to_show(self.inner.top_index, i, per).min(self.max_top_index(bounds));
        self.inner.list.set_top_index(top);
    }

    /// Scrolls by `rows` (positive = down), clamped to the last full page.
    pub fn scroll_rows(&mut self, bounds: Rect, rows: i32) {
        let top = (self.inner.top_index as i64 + rows as i64).max(0) as usize;
        let top = top.min(self.max_top_index(bounds));
        self.inner.list.set_top_index(top);
    }

    /// The largest `TopIndex` that still fills `bounds`.
    pub fn max_top_index(&self, bounds: Rect) -> usize {
        self.inner.items.len().saturating_sub(self.visible_rows(bounds).max(1))
    }
}

/// The check well inside a row: [`control::CHECK_BOX`] square, vertically
/// centred, after the row's `px-2`.
fn check_rect(row: Rect) -> Rect {
    let cy = (row.top + row.bottom) / 2.0;
    let x = row.left + list_metrics::ROW_PAD_H;
    Rect::new(
        x,
        cy - control::CHECK_BOX / 2.0,
        x + control::CHECK_BOX,
        cy + control::CHECK_BOX / 2.0,
    )
}

/// Paints a check well in the Kubuno checkbox look (`@ui/Checkbox`).
fn paint_check(c: &dyn Canvas, well: Rect, state: CheckState, disabled: bool) {
    let t = c.theme();
    let on = state != CheckState::Unchecked;
    let accent = if disabled { faded(&t.accent, menu_metrics::DISABLED_ALPHA) } else { t.accent };
    // `CHECKBOX_GEOMETRY = { size: 18, border: 2, radius: 4, tick: 11 }` —
    // the very geometry the web's own canvas check box draws.
    if on {
        c.fill_rounded(&well, control::CHECK_RADIUS, &accent);
        // `Indeterminate` is a bar, `Checked` a tick — the two the web's
        // checkbox draws, and the states the replica models.
        let ink = t.accent_foreground;
        if state == CheckState::Indeterminate {
            let cy = (well.top + well.bottom) / 2.0;
            let half = control::CHECK_TICK / 2.0;
            let bar = Rect::new(
                (well.left + well.right) / 2.0 - half,
                cy - control::CHECK_BORDER / 2.0,
                (well.left + well.right) / 2.0 + half,
                cy + control::CHECK_BORDER / 2.0,
            );
            c.fill_rounded(&bar, control::CHECK_BORDER / 2.0, &ink);
        } else {
            c.vector_icon("Check", &well, control::CHECK_TICK, &ink);
        }
    } else {
        let stroke = if disabled { faded(&t.border_strong, menu_metrics::DISABLED_ALPHA) } else { t.border_strong };
        c.stroke_rounded_w(&well, control::CHECK_RADIUS, &stroke, control::CHECK_BORDER);
    }
}

impl Widget for CheckedListBox {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let f = canvas.formats();
        let widest = self
            .inner
            .items
            .iter()
            .fold(0.0_f32, |w, it| w.max(text_width(canvas, it, &f.body)));
        let chrome = 2.0 * (list_metrics::BORDER + list_metrics::PANEL_PAD + list_metrics::ROW_PAD_H)
            + control::CHECK_BOX
            + control::CHECK_GAP;
        let width = (widest + chrome).max(list_metrics::MIN_WIDTH);
        Size::new(width, self.height_for_rows(self.inner.items.len().max(1)))
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let dead = state.disabled || !self.inner.control().enabled;
        paint_list_panel(canvas, bounds, dead);

        let content = Grid::content(bounds);
        canvas.push_clip(&content);
        let grid = self.grid();
        for (i, label) in self.inner.items.iter().enumerate() {
            let Some(row) = grid.cell(bounds, i) else { continue };
            paint_list_row(
                canvas,
                row,
                label,
                RowState {
                    selected:   self.inner.is_selected(i),
                    hot:        self.hot_index == Some(i) && !dead,
                    active:     state.focused && !dead && self.focus_index == Some(i),
                    disabled:   dead,
                    text_inset: list_metrics::ROW_PAD_H + control::CHECK_BOX + control::CHECK_GAP,
                },
            );
            paint_check(canvas, check_rect(row), self.inner.get_item_check_state(i), dead);
        }
        let per = grid.rows_per_column(bounds);
        paint_row_scrollbar(canvas, bounds, content, grid.len, per, grid.top, grid.row_h);
        canvas.pop_clip();
        if state.show_focus_ring() && !dead {
            paint_list_ring(canvas, bounds);
        }
    }

    fn type_name(&self) -> &'static str {
        "CheckedListBox"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ComboBox
// ─────────────────────────────────────────────────────────────────────────────

/// A select: a field the height of an input, and a floating list.
///
/// `DropDownStyle`, `DropDownHeight`, `MaxDropDownItems`, `IntegralHeight`,
/// `Sorted`, `ItemHeight` and the selection are the replica's
/// ([`replica::ComboBox`]); so is [`replica::ComboBox::add_item`], which keeps
/// `Sorted` and `SelectedIndex` consistent with each other.
///
/// Both halves come from `Combobox.tsx`, and from nowhere else. The trigger is
/// literally `flex h-9 w-full items-center gap-2 rounded-md border px-3
/// text-left` (line 239) — an `Input`, so [`height::BUTTON_MD`], [`radius::SM`]
/// and [`space::MD`], with a `w-4` chevron. The popup is that file's own
/// `role="listbox"` panel, the same one a [`ListBox`] is: `rounded-lg border
/// border-border bg-white`, `p-1` inside, `rounded-md` rows of
/// [`control::COMBO_ROW`]. It is **not** a menu and does not take the 30 DIP
/// menu row.
#[derive(Clone, Default)]
pub struct ComboBox {
    inner: replica::ComboBox,
    /// Whether the list is dropped — `ComboBox.DroppedDown` in .NET, which the
    /// replica does not model (it is runtime state, not a designer property).
    pub open:       bool,
    /// The ACTIVE row of the open list — the web's `active`, which the arrows
    /// and `onMouseEnter` share (`aria-activedescendant`). Painted as the
    /// `bg-surface-2` highlight.
    pub hot_index:  Option<usize>,
    /// The first row of the dropped list scrolled into view. .NET's combo has
    /// no `TopIndex`; the web's list is `overflow-y-auto`, so the popup scrolls
    /// once there are more items than [`ComboBox::visible_rows`].
    pub scroll_top: usize,
    /// The type-ahead prefix buffer (select-only combobox: typing jumps).
    pub type_ahead: TypeAhead,
}

impl Deref for ComboBox {
    type Target = replica::ComboBox;
    fn deref(&self) -> &replica::ComboBox {
        &self.inner
    }
}
impl DerefMut for ComboBox {
    fn deref_mut(&mut self) -> &mut replica::ComboBox {
        &mut self.inner
    }
}

/// A key, as an ARIA 1.2 select-only combobox understands it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComboKey {
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Enter,
    Space,
    Escape,
    Tab,
    /// `Alt+Down` — opens the list (Windows and ARIA).
    AltDown,
    /// `Alt+Up` — commits the active row and closes (ARIA select-only).
    AltUp,
    /// `F4` — the Windows toggle.
    F4,
}

/// What a [`ComboKey`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComboOutcome {
    /// Not a key the combo acts on in its current state.
    Ignored,
    /// The list dropped open.
    Opened,
    /// The active row moved (list open).
    Moved,
    /// Row `.0` became the selection and the list closed.
    Committed(usize),
    /// The list closed without changing the selection.
    Closed,
}

impl ComboBox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drops the list open. On the closed → open transition the active row
    /// starts on the current selection (or the first row) and is scrolled into
    /// view — `Combobox.tsx`: « Reopening starts on the current selection ».
    /// Calling it again while open changes nothing, so a host may call it every
    /// frame.
    pub fn open(&mut self) {
        if self.open {
            return;
        }
        self.open = true;
        let sel = usize::try_from(self.inner.selected_index).ok().filter(|&i| i < self.inner.items.len());
        let start = sel.or(if self.inner.items.is_empty() { None } else { Some(0) });
        self.hot_index = start;
        if let Some(i) = start {
            self.ensure_visible(i);
        }
    }

    pub fn close(&mut self) {
        self.open = false;
        self.hot_index = None;
        self.type_ahead.clear();
    }

    /// Toggles the list, the way a click on the field does.
    pub fn toggle(&mut self) {
        if self.is_open() {
            self.close();
        } else {
            self.open();
        }
    }

    /// Whether a list is on screen: the flag, or `Simple`, whose list never
    /// closes.
    pub fn is_open(&self) -> bool {
        self.open || self.inner.drop_down_style == replica::ComboBoxStyle::Simple
    }

    /// The field's own height — an `Input`'s `h-9`.
    pub fn field_height(&self) -> f32 {
        height::BUTTON_MD
    }

    /// A popup row: [`control::COMBO_ROW`] — `py-1.5` over a 20 DIP line, the
    /// row `Combobox.tsx` gives its `role="option"`. A combo's popup is NOT a
    /// menu and does not take the 30 DIP menu row. An explicit `ItemHeight`
    /// still wins, as in the toolkit.
    pub fn row_height(&self) -> f32 {
        kubuno_row_height(self.inner.item_height)
    }

    /// How many rows the popup shows.
    ///
    /// The toolkit's rule, verbatim: `IntegralHeight` (the default) makes
    /// `MaxDropDownItems` govern and `DropDownHeight` be ignored; with it off,
    /// `DropDownHeight` is the height and the last row may be clipped. The
    /// web's own `maxHeight = 280` caps it on top of that, so a tall
    /// `ItemHeight` cannot push the list past the panel.
    pub fn visible_rows(&self) -> usize {
        let max = self.inner.max_drop_down_items.max(1) as usize;
        let len = self.inner.items.len().max(1);
        if self.inner.integral_height {
            let usable = list_metrics::DROP_DOWN_MAX - 2.0 * list_metrics::PANEL_PAD;
            let cap = ((usable / self.row_height()).floor().max(1.0)) as usize;
            len.min(max).min(cap).max(1)
        } else {
            let usable = self.inner.drop_down_height as f32 - 2.0 * list_metrics::PANEL_PAD;
            ((usable / self.row_height()).floor().max(1.0) as usize).min(len)
        }
    }

    /// The popup's height.
    pub fn drop_down_height(&self) -> f32 {
        let content = self.visible_rows() as f32 * self.row_height() + 2.0 * list_metrics::PANEL_PAD;
        if self.inner.integral_height {
            content.min(list_metrics::DROP_DOWN_MAX)
        } else {
            self.inner.drop_down_height as f32
        }
    }

    /// Whether the dropped list holds more rows than it shows — it then
    /// scrolls and paints its scroll indicator.
    pub fn drop_down_scrolls(&self) -> bool {
        self.inner.items.len() > self.visible_rows()
    }

    /// The largest [`ComboBox::scroll_top`].
    pub fn max_scroll_top(&self) -> usize {
        self.inner.items.len().saturating_sub(self.visible_rows())
    }

    /// Scrolls the dropped list by `rows` (positive = down), clamped.
    pub fn scroll_drop_down(&mut self, rows: i32) {
        let top = (self.scroll_top as i64 + rows as i64).max(0) as usize;
        self.scroll_top = top.min(self.max_scroll_top());
    }

    /// Scrolls just enough to show row `i` (`scrollIntoView({ block:
    /// 'nearest' })`, which the web runs on every highlight move).
    pub fn ensure_visible(&mut self, i: usize) {
        let top = top_to_show(self.scroll_top, i, self.visible_rows());
        self.scroll_top = top.min(self.max_scroll_top());
    }

    /// Where the field is inside `bounds`. A `Simple` combo stacks the field on
    /// top of its permanent list, so the field is only the first row of it; the
    /// other two styles ARE the field.
    pub fn field_rect(&self, bounds: Rect) -> Rect {
        Rect::new(bounds.left, bounds.top, bounds.right, bounds.top + self.field_height())
    }

    /// Where the open list is drawn, relative to `bounds`.
    ///
    /// The web anchors it under the trigger (`Dropdown.openDropdown`: `r.bottom
    /// + 2`), as wide as the trigger with `minWidth: 200`.
    pub fn drop_down_rect(&self, bounds: Rect) -> Rect {
        let top = self.field_rect(bounds).bottom + DROP_DOWN_OFFSET;
        let width = self.drop_down_width_for(bounds);
        Rect::new(bounds.left, top, bounds.left + width, top + self.drop_down_height())
    }

    fn drop_down_width_for(&self, bounds: Rect) -> f32 {
        (bounds.right - bounds.left).max(self.drop_down_min_width()).max(list_metrics::MIN_WIDTH)
    }

    /// Where the open list goes when it must stay inside `area` (the monitor
    /// work area, in the same space as `bounds`) — `Combobox.tsx`' `measure`:
    /// below the trigger, flipped ABOVE it when it would cross the bottom edge,
    /// and pulled left to keep [`VIEWPORT_EDGE`] from the right edge.
    ///
    /// A `Simple` combo's list is part of the control and never moves.
    pub fn drop_down_rect_in(&self, bounds: Rect, area: Rect) -> Rect {
        let below = self.drop_down_rect(bounds);
        if self.inner.drop_down_style == replica::ComboBoxStyle::Simple {
            return below;
        }
        let h = below.bottom - below.top;
        let w = below.right - below.left;
        let field = self.field_rect(bounds);
        let mut top = below.top;
        if top + h > area.bottom - VIEWPORT_EDGE {
            top = (field.top - DROP_DOWN_OFFSET - h).max(area.top + VIEWPORT_EDGE);
        }
        let mut left = below.left;
        if left + w > area.right - VIEWPORT_EDGE {
            left = (area.right - VIEWPORT_EDGE - w).max(area.left + VIEWPORT_EDGE);
        }
        if left < area.left + VIEWPORT_EDGE {
            left = area.left + VIEWPORT_EDGE;
        }
        Rect::new(left, top, left + w, top + h)
    }

    /// What painting the open list at [`ComboBox::drop_down_rect`] touches,
    /// its shadow included — the rectangle to hand `host::popup`.
    pub fn drop_down_paint_bounds(&self, bounds: Rect) -> Rect {
        inflate(self.drop_down_rect(bounds), FLOAT_SHADOW_MARGIN)
    }

    /// `DropDownWidth = 0` means « match the control », the toolkit's default.
    fn drop_down_min_width(&self) -> f32 {
        if self.inner.drop_down_width > 0 {
            self.inner.drop_down_width as f32
        } else {
            0.0
        }
    }

    /// Which item of the OPEN list `(x, y)` lands on, the list being at its
    /// default place under `bounds`. `None` when the list is closed or the
    /// point misses a row. Honours [`ComboBox::scroll_top`].
    pub fn item_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        if !self.is_open() {
            return None;
        }
        self.item_at_panel(self.drop_down_rect(bounds), x, y)
    }

    /// Which item `(x, y)` lands on for a list painted into `panel` — for a
    /// list placed with [`ComboBox::drop_down_rect_in`].
    pub fn item_at_panel(&self, panel: Rect, x: f32, y: f32) -> Option<usize> {
        if !panel.contains(x, y) {
            return None;
        }
        let first = panel.top + list_metrics::PANEL_PAD;
        if y < first {
            return None;
        }
        let k = ((y - first) / self.row_height()).floor() as usize;
        let i = self.scroll_top + k;
        (k < self.visible_rows() && i < self.inner.items.len()).then_some(i)
    }

    /// Where row `i` is drawn inside a list painted into `panel`, or `None`
    /// when it is scrolled out of view.
    pub fn drop_down_item_rect(&self, panel: Rect, i: usize) -> Option<Rect> {
        let k = i.checked_sub(self.scroll_top)?;
        if k >= self.visible_rows() || i >= self.inner.items.len() {
            return None;
        }
        let y = panel.top + list_metrics::PANEL_PAD + k as f32 * self.row_height();
        Some(Rect::new(
            panel.left + list_metrics::PANEL_PAD,
            y,
            panel.right - list_metrics::PANEL_PAD,
            y + self.row_height(),
        ))
    }

    /// The text the field shows: the selected item, or the control's `Text`
    /// (which an editable combo owns and the toolkit keeps in step).
    pub fn display_text(&self) -> &str {
        self.inner.selected_item().unwrap_or(&self.inner.control().text)
    }

    /// Makes row `i` the selection and closes the list — `commit` in
    /// `Combobox.tsx`.
    pub fn commit(&mut self, i: usize) {
        if i < self.inner.items.len() {
            self.inner.set_selected_index(i as i32);
        }
        self.close();
    }

    /// The ARIA 1.2 select-only combobox keyboard.
    ///
    /// Closed: `Down`, `Up`, `Enter`, `Space`, `Alt+Down` and `F4` open it (the
    /// web trigger's `ArrowDown / Enter / ' '`, plus the Windows chords);
    /// `Home` / `End` open it on the first / last row.
    ///
    /// Open: the arrows move the active row with wrap-around (`firstEnabled`),
    /// `Home` / `End` jump, `PageUp` / `PageDown` move by a page; `Enter`,
    /// `Space` and `Alt+Up` commit; `Escape`, `Tab` and `F4` close without
    /// committing.
    pub fn handle_key(&mut self, key: ComboKey) -> ComboOutcome {
        let len = self.inner.items.len();
        if !self.is_open() {
            return match key {
                ComboKey::Down | ComboKey::Up | ComboKey::Enter | ComboKey::Space | ComboKey::AltDown | ComboKey::F4 => {
                    self.open();
                    ComboOutcome::Opened
                }
                ComboKey::Home | ComboKey::End => {
                    self.open();
                    if len > 0 {
                        let i = if key == ComboKey::Home { 0 } else { len - 1 };
                        self.hot_index = Some(i);
                        self.ensure_visible(i);
                    }
                    ComboOutcome::Opened
                }
                _ => ComboOutcome::Ignored,
            };
        }
        let simple = self.inner.drop_down_style == replica::ComboBoxStyle::Simple;
        let page = self.visible_rows();
        let moved = |this: &mut Self, to: Option<usize>| {
            if let Some(i) = to {
                this.hot_index = Some(i);
                this.ensure_visible(i);
            }
            ComboOutcome::Moved
        };
        match key {
            ComboKey::Down => {
                let to = step_wrapping(self.hot_index, len, true, |_| true);
                moved(self, to)
            }
            ComboKey::Up => {
                let to = step_wrapping(self.hot_index, len, false, |_| true);
                moved(self, to)
            }
            ComboKey::Home => {
                let to = list_step(self.hot_index, len, ListKey::Home, page);
                moved(self, to)
            }
            ComboKey::End => {
                let to = list_step(self.hot_index, len, ListKey::End, page);
                moved(self, to)
            }
            ComboKey::PageUp => {
                let to = list_step(self.hot_index, len, ListKey::PageUp, page);
                moved(self, to)
            }
            ComboKey::PageDown => {
                let to = list_step(self.hot_index, len, ListKey::PageDown, page);
                moved(self, to)
            }
            ComboKey::Enter | ComboKey::Space | ComboKey::AltUp => match self.hot_index {
                Some(i) if i < len => {
                    self.commit(i);
                    ComboOutcome::Committed(i)
                }
                _ => {
                    self.close();
                    ComboOutcome::Closed
                }
            },
            ComboKey::Escape | ComboKey::Tab | ComboKey::F4 | ComboKey::AltDown => {
                if simple {
                    return ComboOutcome::Ignored;
                }
                self.close();
                ComboOutcome::Closed
            }
        }
    }

    /// Type-ahead: open, it moves the active row; closed, it changes the
    /// selection directly (the select-only combobox pattern). Returns the row.
    pub fn type_to_select(&mut self, text: &str, now_ms: u64) -> Option<usize> {
        if text.is_empty() {
            return None;
        }
        let prefix = self.type_ahead.push(text, now_ms).to_string();
        let from = if self.is_open() {
            self.hot_index
        } else {
            usize::try_from(self.inner.selected_index).ok()
        };
        let i = type_ahead_match(self.inner.items.iter().map(String::as_str), &prefix, from)?;
        if self.is_open() {
            self.hot_index = Some(i);
            self.ensure_visible(i);
        } else {
            self.inner.set_selected_index(i as i32);
        }
        Some(i)
    }

    /// Paints the field ALONE — the trigger — for a host that shows the open
    /// list in its own `host::popup` (see [`ComboBox::paint_drop_down_at`]).
    /// `bounds` is the control's; a `Simple` combo still paints only its field.
    pub fn paint_trigger(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let dead = state.disabled || !self.inner.control().enabled;
        self.paint_field(canvas, self.field_rect(bounds), state, dead);
    }

    /// Paints the open list ALONE at its default place under `bounds`
    /// ([`ComboBox::drop_down_rect`]). Nothing is painted while closed or
    /// disabled. Paint area: [`ComboBox::drop_down_paint_bounds`].
    pub fn paint_drop_down(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let dead = state.disabled || !self.inner.control().enabled;
        if self.is_open() && !dead {
            self.paint_drop_down_at(canvas, self.drop_down_rect(bounds));
        }
    }

    fn paint_field(&self, c: &dyn Canvas, field: Rect, state: WidgetState, dead: bool) {
        let t = c.theme();
        let f = c.formats();

        // `Combobox.tsx`' trigger: `bg-white`, `border-border hover:bg-surface-1`
        // at rest; `kb-field-focus--on` while open and `kb-field-focus` on
        // `:focus-visible` (the accent outline + border); `disabled:bg-surface-2
        // disabled:opacity-60`.
        let dropped = self.open && self.inner.drop_down_style != replica::ComboBoxStyle::Simple;
        let ring = !dead && (dropped || state.show_focus_ring());
        let face = if dead {
            t.surface_2
        } else if state.hot && !ring {
            t.card_background
        } else {
            t.layer_background
        };
        c.fill_rounded(&field, radius::SM, &face);
        if ring {
            c.stroke_rounded_w(&field, radius::SM, &t.accent, FOCUS_RING);
        } else {
            c.stroke_rounded(&field, radius::SM, &t.card_stroke);
        }

        // `flex h-9 w-full items-center gap-2 rounded-md border px-3` — the
        // chevron sits one `px-3` in from the trailing edge, and the label
        // (`min-w-0 flex-1 truncate`) stops one `gap-2` before it.
        let cy = (field.top + field.bottom) / 2.0;
        let half = control::COMBO_ARROW / 2.0;
        let arrow = Rect::new(
            field.right - space::MD - control::COMBO_ARROW,
            cy - half,
            field.right - space::MD,
            cy + half,
        );

        let ink = if dead { faded(&t.text_primary, DISABLED_FIELD_ALPHA) } else { t.text_primary };
        let label = Rect::new(field.left + space::MD, field.top, arrow.left - space::SM, field.bottom);
        if label.right > label.left {
            c.push_clip(&field);
            // Owner-draw: the edit field shows the selected item drawn by the owner
            // (`DrawItemState.ComboBoxEdit`, index `None` when nothing is selected).
            let mut drawn = false;
            if self.inner.draw_mode != DrawMode::Normal && owner_draw::has_handler() {
                let g = Graphics::new(c);
                let index = usize::try_from(self.inner.selected_index).ok();
                let st = item_state(false, false, state.focused, dead) | DrawItemState::COMBO_BOX_EDIT;
                let mut e = DrawItemEventArgs::new(&g, "ComboBox", index, label, st, self.display_text());
                drawn = owner_draw::draw_item(&mut e);
            }
            if !drawn {
                c.text_ellipsis(self.display_text(), &label, &f.body, &ink);
            }
            c.pop_clip();
        }

        // The chevron is GEOMETRY, never a character: `ChevronDown` from
        // `assets/lucide-icons.txt`, the very icon `Combobox.tsx` imports.
        let chevron = if dead { faded(&t.text_secondary, DISABLED_FIELD_ALPHA) } else { t.text_secondary };
        c.vector_icon("ChevronDown", &arrow, control::COMBO_ARROW, &chevron);
    }

    /// Records the visible rows of the open list, laid out in `panel` (the popup's coordinates),
    /// through the owner-draw `handler` — for a list painted later in its popup, which lends the
    /// recording ([`owner_draw::RecordedItems`]) around [`ComboBox::paint_drop_down_at`].
    pub fn record_drop_down_items(&self, panel: Rect, handler: &mut dyn owner_draw::OwnerDrawHandler) -> owner_draw::RecordedItems {
        let mut out = owner_draw::RecordedItems::new();
        if self.inner.draw_mode == DrawMode::Normal {
            return out;
        }
        let end = (self.scroll_top + self.visible_rows()).min(self.inner.items.len());
        for i in self.scroll_top..end {
            let (Some(row), Some(label)) = (self.drop_down_item_rect(panel, i), self.inner.items.get(i)) else { continue };
            let hot = self.hot_index == Some(i);
            let st = item_state(self.inner.selected_index == i as i32, hot, hot, false);
            out.record(handler, "ComboBox", i, row, st, label);
        }
        out
    }

    /// Paints the open list into an explicit `panel` rectangle — the popup
    /// case, where the caller placed it with [`ComboBox::drop_down_rect_in`]
    /// and rebased it into the popup's local space. The shadow spills
    /// [`FLOAT_SHADOW_MARGIN`] outside `panel`.
    pub fn paint_drop_down_at(&self, c: &dyn Canvas, panel: Rect) {
        let t = c.theme();
        let f = c.formats();

        // A combo's popup IS the list panel of `Combobox.tsx` — `overflow-hidden
        // rounded-lg border border-border bg-white`, `p-1` inside — lifted off
        // the page by the float shadow because, unlike a `ListBox`, it floats.
        drop_shadow(c, &panel);
        paint_list_panel(c, panel, false);

        let content = Rect::new(
            panel.left + list_metrics::PANEL_PAD,
            panel.top + list_metrics::PANEL_PAD,
            panel.right - list_metrics::PANEL_PAD,
            panel.bottom - list_metrics::PANEL_PAD,
        );
        c.push_clip(&content);
        let end = (self.scroll_top + self.visible_rows()).min(self.inner.items.len());
        // Owner-draw (`DrawMode`; `OwnerDrawVariable` rows keep the fixed height here).
        let owner = (self.inner.draw_mode != DrawMode::Normal && owner_draw::has_handler()).then(|| Graphics::new(c));
        for i in self.scroll_top..end {
            let (Some(row), Some(label)) = (self.drop_down_item_rect(panel, i), self.inner.items.get(i)) else {
                continue;
            };
            let selected = self.inner.selected_index == i as i32;
            if let Some(g) = &owner {
                let st = item_state(selected, self.hot_index == Some(i), self.hot_index == Some(i), false);
                let mut e = DrawItemEventArgs::new(g, "ComboBox", Some(i), row, st, label.as_str());
                if owner_draw::draw_item(&mut e) {
                    continue;
                }
            }
            // `isAct ? 'bg-surface-2'` on the active row, `rounded-md`. A
            // selected row has NO fill of its own: the tick and the
            // `text-primary` label mark it.
            if self.hot_index == Some(i) {
                c.fill_rounded(&row, radius::SM, &t.row_hover);
            }
            // `<span className="flex w-4 shrink-0 justify-center text-primary">`
            // — the tick gutter, then `gap-2`, then the label.
            let tick = Rect::new(
                row.left + list_metrics::ROW_PAD_H,
                row.top,
                row.left + list_metrics::ROW_PAD_H + list_metrics::TICK_CELL,
                row.bottom,
            );
            if selected {
                c.vector_icon("Check", &tick, list_metrics::TICK_GLYPH, &t.accent);
            }
            let label_rect = Rect::new(
                tick.right + list_metrics::CELL_GAP,
                row.top,
                row.right - list_metrics::ROW_PAD_H,
                row.bottom,
            );
            // `isSel ? 'text-primary' : 'text-text-primary'`, `truncate`.
            let ink = if selected { t.accent } else { t.text_primary };
            c.text_ellipsis(label, &label_rect, &f.body, &ink);
        }
        paint_row_scrollbar(c, panel, content, self.inner.items.len(), self.visible_rows(), self.scroll_top, self.row_height());
        c.pop_clip();
    }
}

/// `Dropdown.openDropdown` anchors the popup at `r.bottom + 2`.
const DROP_DOWN_OFFSET: f32 = 2.0;
/// `@ui/Input`: `focus:ring-2` — a 2 DIP ring in the accent, drawn inward;
/// the value every field of this crate uses (`text::FOCUS_RING`).
const FOCUS_RING: f32 = 2.0;
/// `Input`: `disabled:opacity-60`.
const DISABLED_FIELD_ALPHA: f32 = 0.6;

impl Widget for ComboBox {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let f = canvas.formats();
        let widest = self
            .inner
            .items
            .iter()
            .fold(0.0_f32, |w, it| w.max(text_width(canvas, it, &f.body)));
        // `px-3` on both sides, `gap-2` before the `w-4` chevron.
        let width = (widest + 2.0 * space::MD + space::SM + control::COMBO_ARROW)
            .max(list_metrics::MIN_WIDTH);
        let height = match self.inner.drop_down_style {
            // `Simple` stacks the list under the field, permanently.
            replica::ComboBoxStyle::Simple => {
                self.field_height() + DROP_DOWN_OFFSET + self.drop_down_height()
            }
            _ => self.field_height(),
        };
        Size::new(width, height)
    }

    /// The field, and the open list at its default place. A host that needs
    /// the list to escape its container paints [`ComboBox::paint_trigger`]
    /// here and [`ComboBox::paint_drop_down_at`] in a `host::popup` instead.
    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.paint_trigger(canvas, bounds, state);
        self.paint_drop_down(canvas, bounds, state);
    }

    /// The field, plus the open list — a click in the dropped list is a click on
    /// the combo, not on whatever is painted behind it.
    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        self.field_rect(bounds).contains(x, y)
            || (self.is_open() && self.drop_down_rect(bounds).contains(x, y))
    }

    fn type_name(&self) -> &'static str {
        "ComboBox"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Menu — the product's `MenuDropdown`
// ─────────────────────────────────────────────────────────────────────────────

/// A menu entry under construction — the web's `MenuItem` literal, spelled in
/// Rust.
///
/// It produces a [`ToolStripMenuItem`], so nothing about the item is stored
/// twice: the label is `ToolStripItem::text`, the icon is `ToolStripItem::image`
/// (« the key the host would resolve it with » — here, a geometry name from
/// `assets/lucide-icons.txt`), the shortcut is `ShortcutKeys`, the tick is
/// `Checked`, the submenu is `DropDownItems`, and `danger` — which .NET has no
/// concept of — rides in `Tag` as [`DANGER_TAG`].
pub struct MenuEntry(ToolStripMenuItem);

impl MenuEntry {
    pub fn new(label: impl Into<String>) -> Self {
        Self(ToolStripMenuItem::new(label))
    }

    /// A geometry name from `assets/lucide-icons.txt` (`"Star"`, `"Trash2"`…).
    pub fn icon(mut self, name: impl Into<String>) -> Self {
        self.0.base.item.image = Some(name.into());
        self
    }

    /// `ShortcutKeys` — the modifiers plus the key's display name.
    pub fn shortcut(mut self, ctrl: bool, alt: bool, shift: bool, key: impl Into<String>) -> Self {
        self.0.shortcut_keys = Some(Shortcut::new(ctrl, alt, shift, key));
        self
    }

    /// `ShortcutKeyDisplayString` — an explicit override of the text above.
    pub fn shortcut_text(mut self, text: impl Into<String>) -> Self {
        self.0.shortcut_key_display_string = Some(text.into());
        self
    }

    /// `Checked` — the row shows a tick in its icon cell.
    pub fn checked(mut self, on: bool) -> Self {
        self.0.checked = on;
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        self.0.base.item.enabled = on;
        self
    }

    /// A destructive row: `danger` in the web, [`DANGER_TAG`] here.
    pub fn danger(mut self) -> Self {
        self.0.base.item.tag = Some(DANGER_TAG.to_string());
        self
    }

    /// `DropDownItems` — the cascaded submenu.
    pub fn submenu(mut self, items: Vec<StripItem>) -> Self {
        self.0.base.drop_down_items = items;
        self
    }

    pub fn build(self) -> StripItem {
        StripItem::MenuItem(self.0)
    }
}

/// A `{ type: 'separator' }` row.
pub fn separator() -> StripItem {
    StripItem::Separator(ToolStripSeparator::default())
}

/// A `{ type: 'label' }` section header.
pub fn section(text: impl Into<String>) -> StripItem {
    StripItem::Label(ToolStripLabel::new(text))
}

/// **The** Kubuno context menu — `@ui/MenuDropdown`, which `CLAUDE.md` makes the
/// product's only menu.
///
/// The model is the replica's [`ContextMenuStrip`]: its `Items` are
/// `StripItem`s, so the toolkit's own tree (menu item, separator, label,
/// recursive `DropDownItems`) is what a caller builds — see [`MenuEntry`],
/// [`separator`] and [`section`]. The width rule is the replica's
/// [`measure_menu_item`]. Only the pixels are new.
#[derive(Clone, Default)]
pub struct Menu {
    inner: ContextMenuStrip,
    /// The row under the pointer. A `ToolStripItem` is a `Component`, not a
    /// `Control`: it cannot observe the mouse, which is why the replica's own
    /// [`kubuno_desktop_controls::toolstrip::ToolStrip::paint_items`] takes this from
    /// its host too.
    pub hot_index:    Option<usize>,
    /// The row whose submenu is cascaded open, if any.
    pub open_submenu: Option<usize>,
    /// The hot row INSIDE the open submenu (pointer or keyboard). While it is
    /// `Some`, the keyboard drives the submenu rather than this menu.
    pub submenu_hot:  Option<usize>,
    /// The viewport the menu must stay inside (client DIP — the monitor work
    /// area a host gets from `Frame::screen_area`). With it, the cascaded
    /// submenu flips to the LEFT of its row when the right has no room and is
    /// pulled back inside the edges, and the menu's measured width is capped
    /// at `viewport − 16`, all as `MenuDropdown.tsx` does. `None` = unbounded.
    pub viewport:     Option<Rect>,
    /// Owner-drawn items (the Win32 menu's `MF_OWNERDRAW`): each menu item is first offered to
    /// the owner-draw handler lent for the paint (`crate::graphics::owner_draw`) as a
    /// `DrawItem`; separators and section labels stay the menu's.
    pub owner_draw:   bool,
    /// The keyboard cues: per row, the character of its label to underline (its `&` mnemonic),
    /// shown while the host says so (a menu opened from the keyboard, Alt held). Empty: none.
    pub mnemonics:    Vec<Option<usize>>,
}

/// A key, as the ARIA menu pattern understands it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKey {
    Up,
    Down,
    Home,
    End,
    /// Opens the hot row's submenu and enters it.
    Right,
    /// Leaves an open submenu, back to its parent row.
    Left,
    Enter,
    Space,
    Escape,
}

/// What a [`MenuKey`] (or a click) did to a [`Menu`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOutcome {
    /// Nothing the menu acts on.
    Ignored,
    /// The hot row moved.
    Moved,
    /// A submenu opened (its first actionable row is hot).
    SubmenuOpened,
    /// The open submenu closed; the parent row stays hot.
    SubmenuClosed,
    /// Leaf `index` of the menu — or, with `sub = Some(j)`, row `j` of the
    /// submenu opened from row `index` — was chosen. The host runs it and
    /// closes the menu.
    Chosen { index: usize, sub: Option<usize> },
    /// Escape at the top level: the host closes the menu.
    Close,
}

impl Deref for Menu {
    type Target = ContextMenuStrip;
    fn deref(&self) -> &ContextMenuStrip {
        &self.inner
    }
}
impl DerefMut for Menu {
    fn deref_mut(&mut self) -> &mut ContextMenuStrip {
        &mut self.inner
    }
}

/// A row's height: 30 for an action, 11 for a separator, 28 for a section label.
fn menu_row_height(item: &StripItem) -> f32 {
    match item {
        StripItem::Separator(_) => {
            menu_metrics::SEPARATOR_MARGIN_V * 2.0 + menu_metrics::SEPARATOR_LINE
        }
        // `padding: '4px 10px'` around a `--kb-text-meta` line. The label's own
        // line-height is the browser's `normal` and is NOT stated in the source;
        // the 20 DIP line box the action rows declare explicitly is reused so
        // labels and actions sit on one rhythm. NUMBER WITHOUT A SOURCE.
        StripItem::Label(_) => menu_metrics::LABEL_PAD_V * 2.0 + MENU_LINE_BOX,
        _ => height::MENU_ITEM,
    }
}

/// `lineHeight: '20px'` on a `MenuDropdown` action row.
const MENU_LINE_BOX: f32 = 20.0;

impl Menu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a menu from its rows — the web's `items` array.
    pub fn with_items(items: Vec<StripItem>) -> Self {
        let mut m = Self::new();
        m.inner.items = items;
        m
    }

    /// The rows, as the replica stores them.
    pub fn items(&self) -> &[StripItem] {
        &self.inner.items
    }

    pub fn push(&mut self, item: StripItem) {
        self.inner.items.push(item);
    }

    /// Whether the icon / check gutter is reserved. `ShowImageMargin` defaults
    /// to `true` in the toolkit, and the web reserves its `width: 20` cell
    /// unconditionally — so the default reserves it, and turning both margins
    /// off closes it up.
    fn has_gutter(&self) -> bool {
        self.inner.show_image_margin || self.inner.show_check_margin
    }

    fn label_left(&self) -> f32 {
        if self.has_gutter() {
            menu_metrics::LABEL_LEFT
        } else {
            menu_metrics::ICON_LEFT
        }
    }

    /// Total height: every row, plus the panel's `padding: 5` twice.
    pub fn content_height(&self) -> f32 {
        self.inner.items.iter().map(menu_row_height).sum::<f32>() + 2.0 * menu_metrics::PANEL_PAD
    }

    /// Where row `i` is drawn inside `bounds`.
    pub fn item_rect(&self, bounds: Rect, i: usize) -> Option<Rect> {
        if i >= self.inner.items.len() {
            return None;
        }
        let y = bounds.top
            + menu_metrics::PANEL_PAD
            + self.inner.items[..i].iter().map(menu_row_height).sum::<f32>();
        Some(Rect::new(bounds.left, y, bounds.right, y + menu_row_height(&self.inner.items[i])))
    }

    /// **Which row** `(x, y)` lands on.
    ///
    /// A separator answers `None`: it is a row geometrically, but never a
    /// target, and a host that highlighted it would light a 1 px rule.
    pub fn item_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        if !bounds.contains(x, y) {
            return None;
        }
        let mut top = bounds.top + menu_metrics::PANEL_PAD;
        for (i, item) in self.inner.items.iter().enumerate() {
            let h = menu_row_height(item);
            if y >= top && y < top + h {
                return (!matches!(item, StripItem::Separator(_))).then_some(i);
            }
            top += h;
        }
        None
    }

    /// The shortcut column's width: the widest shortcut string in the menu, so
    /// the shortcuts read as one column (`textAlign: 'left'` on an `auto` grid
    /// column) rather than trailing each label.
    fn shortcut_column(&self, c: &dyn Canvas) -> f32 {
        let f = c.formats();
        self.inner.items.iter().fold(0.0_f32, |w, item| match item {
            StripItem::MenuItem(m) => {
                let s = m.shortcut_text();
                if s.is_empty() {
                    w
                } else {
                    w.max(text_width(c, &s, &f.body))
                }
            }
            _ => w,
        })
    }

    /// Where the cascaded submenu of row `i` opens.
    ///
    /// `MenuDropdown.SubmenuItem`: `top: r.top - 4`, `left: r.right - 2`, and
    /// `minWidth: SUB_W`. It does not clamp to a viewport here — that is the
    /// host's job, as it is the web's.
    pub fn submenu_rect(&self, bounds: Rect, i: usize) -> Option<Rect> {
        let row = self.item_rect(bounds, i)?;
        let StripItem::MenuItem(m) = &self.inner.items[i] else { return None };
        if !m.has_drop_down_items() {
            return None;
        }
        let sub = Menu::with_items(m.base.drop_down_items.clone());
        let top = row.top - SUBMENU_RISE;
        let left = row.right - SUBMENU_OVERLAP;
        Some(Rect::new(
            left,
            top,
            left + menu_metrics::SUBMENU_WIDTH,
            top + sub.content_height(),
        ))
    }

    /// The child menu of row `i`, when it has one.
    pub fn submenu(&self, i: usize) -> Option<Menu> {
        match self.inner.items.get(i)? {
            StripItem::MenuItem(m) if m.has_drop_down_items() => {
                let mut sub = Menu::with_items(m.base.drop_down_items.clone());
                sub.viewport = self.viewport;
                Some(sub)
            }
            _ => None,
        }
    }

    /// Where the submenu of row `i` really opens, measured and kept inside
    /// the viewport — `SubmenuItem.openNow` plus `MenuDropdown`'s clamp:
    ///
    /// * the panel is `minWidth: SUB_W` and grows to its content;
    /// * it cascades RIGHT (`left: r.right - 2`), unless that crosses the
    ///   viewport's right edge and there is room on the left, in which case it
    ///   opens LEFT of the row (`left: r.left - SUB_W + 2`);
    /// * `top: r.top - 4`, then pulled inside the viewport by [`VIEWPORT_EDGE`].
    ///
    /// With no [`Menu::viewport`] it always cascades right.
    pub fn submenu_rect_in(&self, c: &dyn Canvas, bounds: Rect, i: usize) -> Option<Rect> {
        let row = self.item_rect(bounds, i)?;
        let sub = self.submenu(i)?;
        let want = sub.measure(c);
        let w = want.width.max(menu_metrics::SUBMENU_WIDTH);
        Some(place_submenu(row, w, want.height, self.viewport))
    }

    /// Records the menu items laid out in `bounds` (the popup's coordinates) through the
    /// owner-draw `handler`, when the menu is [`Menu::owner_draw`] — for a menu painted later in
    /// its popup, which lends the recording ([`owner_draw::RecordedItems`]).
    pub fn record_items(&self, bounds: Rect, handler: &mut dyn owner_draw::OwnerDrawHandler) -> owner_draw::RecordedItems {
        let mut out = owner_draw::RecordedItems::new();
        if !self.owner_draw {
            return out;
        }
        for i in 0..self.inner.items.len() {
            let (Some(row), Some(StripItem::MenuItem(m))) = (self.item_rect(bounds, i), self.inner.items.get(i)) else { continue };
            let enabled = m.base.item.enabled;
            let hot = self.hot_index == Some(i) && enabled;
            let st = item_state(false, hot, hot, !enabled).with(DrawItemState::CHECKED, m.checked);
            out.record(handler, "Menu", i, row, st, &m.base.item.text);
        }
        out
    }

    /// The rectangle this menu paints into when drawn at `bounds` — the panel,
    /// the open submenu (where [`Menu::submenu_rect_in`] puts it), and the
    /// shadow around both. What a host sizes its `host::popup` to.
    pub fn paint_bounds(&self, c: &dyn Canvas, bounds: Rect) -> Rect {
        let mut u = bounds;
        if let Some(sub) = self.open_submenu.and_then(|i| self.submenu_rect_in(c, bounds, i)) {
            u = union(u, sub);
        }
        inflate(u, FLOAT_SHADOW_MARGIN)
    }

    /// Whether row `i` can be hot / chosen: an enabled menu item. Separators,
    /// section labels and dead rows are skipped by the keyboard.
    pub fn is_actionable(&self, i: usize) -> bool {
        matches!(self.inner.items.get(i), Some(StripItem::MenuItem(m)) if m.base.item.enabled)
    }

    /// The next actionable row from `from`, wrapping (native menus wrap).
    pub fn next_actionable(&self, from: Option<usize>, forward: bool) -> Option<usize> {
        step_wrapping(from, self.inner.items.len(), forward, |i| self.is_actionable(i))
    }

    /// The ARIA menu keyboard, over this menu and its open submenu.
    ///
    /// `Up` / `Down` walk the actionable rows with wrap-around, `Home` / `End`
    /// jump; `Right` (or `Enter` / `Space`) on a row with children opens its
    /// submenu and moves into it; `Left` or `Escape` inside a submenu go back to
    /// the parent row; `Enter` / `Space` on a leaf choose it; `Escape` at the
    /// top closes the menu. While [`Menu::submenu_hot`] is set the keys drive
    /// the submenu.
    pub fn navigate(&mut self, key: MenuKey) -> MenuOutcome {
        if let (Some(parent), Some(_)) = (self.open_submenu, self.submenu_hot) {
            let Some(sub) = self.submenu(parent) else {
                self.open_submenu = None;
                self.submenu_hot = None;
                return MenuOutcome::Ignored;
            };
            return match key {
                MenuKey::Up | MenuKey::Down => {
                    self.submenu_hot = sub.next_actionable(self.submenu_hot, key == MenuKey::Down);
                    MenuOutcome::Moved
                }
                MenuKey::Home => {
                    self.submenu_hot = sub.next_actionable(None, true);
                    MenuOutcome::Moved
                }
                MenuKey::End => {
                    self.submenu_hot = sub.next_actionable(None, false);
                    MenuOutcome::Moved
                }
                MenuKey::Left | MenuKey::Escape => {
                    self.submenu_hot = None;
                    self.open_submenu = None;
                    self.hot_index = Some(parent);
                    MenuOutcome::SubmenuClosed
                }
                MenuKey::Enter | MenuKey::Space => match self.submenu_hot {
                    Some(j) if sub.is_actionable(j) && sub.submenu(j).is_none() => {
                        MenuOutcome::Chosen { index: parent, sub: Some(j) }
                    }
                    _ => MenuOutcome::Ignored,
                },
                MenuKey::Right => MenuOutcome::Ignored,
            };
        }
        match key {
            MenuKey::Up | MenuKey::Down => {
                self.hot_index = self.next_actionable(self.hot_index, key == MenuKey::Down);
                self.open_submenu = None;
                MenuOutcome::Moved
            }
            MenuKey::Home => {
                self.hot_index = self.next_actionable(None, true);
                self.open_submenu = None;
                MenuOutcome::Moved
            }
            MenuKey::End => {
                self.hot_index = self.next_actionable(None, false);
                self.open_submenu = None;
                MenuOutcome::Moved
            }
            MenuKey::Right | MenuKey::Enter | MenuKey::Space => {
                let Some(i) = self.hot_index.filter(|&i| self.is_actionable(i)) else {
                    return MenuOutcome::Ignored;
                };
                if let Some(sub) = self.submenu(i) {
                    self.open_submenu = Some(i);
                    self.submenu_hot = sub.next_actionable(None, true);
                    MenuOutcome::SubmenuOpened
                } else if key == MenuKey::Right {
                    MenuOutcome::Ignored
                } else {
                    MenuOutcome::Chosen { index: i, sub: None }
                }
            }
            MenuKey::Left => {
                if self.open_submenu.take().is_some() {
                    MenuOutcome::SubmenuClosed
                } else {
                    MenuOutcome::Ignored
                }
            }
            MenuKey::Escape => MenuOutcome::Close,
        }
    }

    /// Type-ahead on a menu (ARIA: « moves focus to the next item with a label
    /// that starts with the typed character »): jumps the hot row of the active
    /// level to the next actionable row whose label starts with `text`'s first
    /// character. Returns whether it moved.
    pub fn type_to_focus(&mut self, text: &str) -> bool {
        let Some(ch) = text.chars().next() else { return false };
        let prefix = ch.to_string();
        let in_sub = self.open_submenu.is_some() && self.submenu_hot.is_some();
        let (menu, current) = if in_sub {
            match self.open_submenu.and_then(|p| self.submenu(p)) {
                Some(sub) => (sub, self.submenu_hot),
                None => return false,
            }
        } else {
            (self.clone(), self.hot_index)
        };
        let labels: Vec<&str> = menu
            .items()
            .iter()
            .enumerate()
            .map(|(i, it)| match it {
                StripItem::MenuItem(m) if menu.is_actionable(i) => m.base.item.text.as_str(),
                _ => "",
            })
            .collect();
        let Some(i) = type_ahead_match(labels, &prefix, current) else { return false };
        if in_sub {
            self.submenu_hot = Some(i);
        } else {
            self.hot_index = Some(i);
            self.open_submenu = None;
        }
        true
    }

    fn paint_panel(&self, c: &dyn Canvas, bounds: Rect) {
        let t = c.theme();
        drop_shadow(c, &bounds);
        // OPAQUE surface, not `flyout_background`: that colour is half-opaque
        // because the web's menu lives over a `backdrop-filter` and the
        // desktop's over a compositor blur. A primitive painted INSIDE a window
        // has neither, and the half-opaque colour would let the page read
        // straight through it (the reason `kubuno-drive-desktop`'s own in-window panel
        // takes `layer_background` too).
        c.fill_rounded(&bounds, radius::FLOAT, &t.layer_background);
        c.stroke_rounded(&bounds, radius::FLOAT, &t.card_stroke);
    }

    fn paint_row(&self, c: &dyn Canvas, row: Rect, i: usize, shortcut_col: f32, dead: bool) {
        let t = c.theme();
        let f = c.formats();

        match &self.inner.items[i] {
            StripItem::Separator(_) => {
                // `margin: '5px 6px'`: the rule stops short of the panel edge so
                // it lines up with the pills above and below.
                let cy = (row.top + row.bottom) / 2.0;
                let inset = menu_metrics::PANEL_PAD + menu_metrics::SEPARATOR_MARGIN_H;
                let line = Rect::new(
                    row.left + inset,
                    cy - menu_metrics::SEPARATOR_LINE / 2.0,
                    row.right - inset,
                    cy + menu_metrics::SEPARATOR_LINE / 2.0,
                );
                c.fill_rounded(&line, 0.0, &t.divider);
            }
            StripItem::Label(l) => {
                let r = Rect::new(
                    row.left + menu_metrics::ICON_LEFT,
                    row.top,
                    row.right - menu_metrics::CONTENT_RIGHT,
                    row.bottom,
                );
                // `--kb-text-meta`, `fontWeight: 600`, uppercased by CSS —
                // `caption_strong` is the 12 px semibold format, and the case is
                // applied here because DirectWrite has no `text-transform`.
                c.text_ellipsis(&l.item.text.to_uppercase(), &r, &f.caption_strong, &t.text_secondary);
            }
            StripItem::MenuItem(m) => {
                if self.owner_draw && owner_draw::has_handler() {
                    let g = Graphics::new(c);
                    let enabled = m.base.item.enabled && !dead;
                    let st = item_state(false, self.hot_index == Some(i) && enabled, self.hot_index == Some(i), !enabled).with(DrawItemState::CHECKED, m.checked);
                    let mut e = DrawItemEventArgs::new(&g, "Menu", Some(i), row, st, m.base.item.text.as_str());
                    if owner_draw::draw_item(&mut e) {
                        return;
                    }
                }
                self.paint_menu_item(c, row, m, MenuRowPaint { index: i, shortcut_col, dead })
            }
            other => {
                // Every other `StripItem` kind belongs to a TOOL strip, not to a
                // drop-down; it is drawn as its plain text rather than silently
                // skipped, so a mis-built menu is visible instead of empty.
                let r = Rect::new(
                    row.left + self.label_left(),
                    row.top,
                    row.right - menu_metrics::CONTENT_RIGHT,
                    row.bottom,
                );
                c.text_ellipsis(&other.item().text, &r, &f.body, &t.text_tertiary);
            }
        }
    }

    fn paint_menu_item(&self, c: &dyn Canvas, row: Rect, m: &ToolStripMenuItem, p: MenuRowPaint) {
        let t = c.theme();
        let f = c.formats();
        let MenuRowPaint { index, shortcut_col, dead } = p;

        let enabled = m.base.item.enabled && !dead;
        let danger = m.base.item.tag.as_deref() == Some(DANGER_TAG);
        // The submenu keeps its parent row lit while it is open — what macOS
        // does, and what `SubmenuItem` reproduces with `pos !== null`.
        let hot = enabled && (self.hot_index == Some(index) || self.open_submenu == Some(index));

        // THE Kubuno hover: a FULL accent pill (`background: c.hover`), inset by
        // the panel's own padding — which is why the label, icon and shortcut
        // all flip to `hoverText` under it.
        let pill = Rect::new(
            row.left + menu_metrics::PANEL_PAD,
            row.top,
            row.right - menu_metrics::PANEL_PAD,
            row.bottom,
        );
        if hot {
            c.fill_rounded(&pill, radius::MENU_ITEM, &t.accent);
        }

        let label_ink = if hot {
            t.accent_foreground
        } else if danger {
            t.danger
        } else {
            t.text_primary
        };
        // At rest the icon carries the ACCENT, not the label colour
        // (`color: c.accent` on the icon cell); a destructive row tints it danger.
        let icon_ink = if hot {
            t.accent_foreground
        } else if danger {
            t.danger
        } else {
            t.accent
        };
        let (label_ink, icon_ink) = if enabled {
            (label_ink, icon_ink)
        } else {
            (
                faded(&label_ink, menu_metrics::DISABLED_ALPHA),
                faded(&icon_ink, menu_metrics::DISABLED_ALPHA),
            )
        };

        // The icon cell: `{item.checked ? '✓' : item.icon}` — a tick REPLACES
        // the icon, it does not sit beside it.
        if self.has_gutter() {
            let cy = (row.top + row.bottom) / 2.0;
            let half = menu_metrics::ICON_GLYPH / 2.0;
            let x = row.left + menu_metrics::ICON_LEFT;
            let cell = Rect::new(x, cy - half, x + menu_metrics::ICON_GLYPH, cy + half);
            if m.checked {
                c.vector_icon("Check", &cell, menu_metrics::ICON_GLYPH, &icon_ink);
            } else if let Some(name) = m.base.item.image.as_deref().and_then(icon_name) {
                c.vector_icon(name, &cell, menu_metrics::ICON_GLYPH, &icon_ink);
            }
        }

        // The label yields room to the shortcut / caret column only when the
        // menu actually has one.
        let has_caret = m.has_drop_down_items();
        let reserved = if shortcut_col > 0.0 {
            menu_metrics::CONTENT_RIGHT + shortcut_col + menu_metrics::SHORTCUT_GAP
        } else if has_caret {
            menu_metrics::CONTENT_RIGHT + menu_metrics::CARET_COLUMN
        } else {
            menu_metrics::CONTENT_RIGHT
        };
        let label = Rect::new(
            row.left + self.label_left(),
            row.top,
            row.right - reserved,
            row.bottom,
        );
        c.text_ellipsis(&m.base.item.text, &label, &f.body, &label_ink);
        if let Some(at) = self.mnemonics.get(index).copied().flatten() {
            crate::mnemonic::underline(c, &m.base.item.text, at, &label, &f.body, &label_ink, windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_LEADING);
        }

        let shortcut = m.shortcut_text();
        if !shortcut.is_empty() && shortcut_col > 0.0 {
            // Left-aligned in its own `auto` column, so the shortcuts line up as
            // a column of their own — the web says so in as many words.
            let left = row.right - menu_metrics::CONTENT_RIGHT - shortcut_col;
            let r = Rect::new(left, row.top, row.right - menu_metrics::CONTENT_RIGHT, row.bottom);
            let ink = if hot {
                faded(&t.accent_foreground, menu_metrics::HOT_SHORTCUT_ALPHA)
            } else {
                t.text_secondary
            };
            let ink = if enabled { ink } else { faded(&ink, menu_metrics::DISABLED_ALPHA) };
            c.text_ellipsis(&shortcut, &r, &f.body, &ink);
        }

        if has_caret {
            // The caret shares the shortcut column, right-aligned in it — and it
            // is GEOMETRY (`ChevronRight`), never a « ▸ » character: the
            // embedded face does not carry one.
            let cy = (row.top + row.bottom) / 2.0;
            let half = menu_metrics::CARET_COLUMN / 2.0;
            let cx = row.right - menu_metrics::CONTENT_RIGHT - half;
            let caret = Rect::new(cx - half, cy - half, cx + half, cy + half);
            let ink = if hot { t.accent_foreground } else { t.text_secondary };
            let ink = if enabled { ink } else { faded(&ink, menu_metrics::DISABLED_ALPHA) };
            c.vector_icon("ChevronRight", &caret, menu_metrics::CARET_COLUMN, &ink);
        }
    }
}

/// What a menu row needs beyond its own item: where it sits in the collection
/// (a `ToolStripItem` does not know its own index), the menu-wide shortcut
/// column, and whether the whole menu is dead.
#[derive(Debug, Clone, Copy)]
struct MenuRowPaint {
    index:        usize,
    shortcut_col: f32,
    dead:         bool,
}

/// `SubmenuItem`: the child panel opens at `top: r.top - 4` …
const SUBMENU_RISE: f32 = 4.0;
/// … and `left: r.right - 2`, so it overlaps its parent by two pixels.
const SUBMENU_OVERLAP: f32 = 2.0;

/// Places a `w × h` submenu beside its parent `row` — see
/// [`Menu::submenu_rect_in`]. Pure, so the flip is unit-testable.
fn place_submenu(row: Rect, w: f32, h: f32, viewport: Option<Rect>) -> Rect {
    let mut left = row.right - SUBMENU_OVERLAP;
    let mut top = row.top - SUBMENU_RISE;
    if let Some(v) = viewport {
        // `openLeft = r.right + SUB_W > vw - 8 && r.left - SUB_W > 8`.
        let open_left = row.right + w > v.right - VIEWPORT_EDGE && row.left - w > v.left + VIEWPORT_EDGE;
        if open_left {
            left = row.left - w + SUBMENU_OVERLAP;
        }
        // The final clamp every `MenuDropdown` runs on itself.
        if left + w > v.right - VIEWPORT_EDGE {
            left = v.right - VIEWPORT_EDGE - w;
        }
        if top + h > v.bottom - VIEWPORT_EDGE {
            top = v.bottom - VIEWPORT_EDGE - h;
        }
        left = left.max(v.left + VIEWPORT_EDGE);
        top = top.max(v.top + VIEWPORT_EDGE);
    }
    Rect::new(left, top, left + w, top + h)
}

impl Widget for Menu {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    /// Width = the widest row, floor `minWidth = 200`; height = every row plus
    /// the panel padding.
    ///
    /// The rule is the replica's [`measure_menu_item`], applied **once** to the
    /// widest label and the menu-wide shortcut column — not once per row.
    ///
    /// That is what the web's own layout does: the panel is a
    /// `grid-template-columns: auto 1fr auto` and every row re-uses those
    /// columns through `subgrid`, so the shortcut column is as wide as the
    /// longest shortcut **for every row**, including the rows that have none.
    /// Measuring row by row under-measures a menu whose longest label and
    /// longest shortcut are on different rows — which is exactly the shape a
    /// real context menu has.
    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let f = canvas.formats();
        let shortcut_col = self.shortcut_column(canvas);
        let gutter = self.label_left();

        let mut widest_label = 0.0_f32;
        // A section label is not on the grid — it spans `1 / -1` and sits in the
        // row's own padding — so it is measured against its own inset instead.
        let mut widest_span = 0.0_f32;
        let mut any_caret = false;
        for item in &self.inner.items {
            match item {
                StripItem::MenuItem(m) => {
                    widest_label = widest_label.max(text_width(canvas, &m.base.item.text, &f.body));
                    any_caret |= m.has_drop_down_items();
                }
                StripItem::Label(l) => {
                    let w = text_width(canvas, &l.item.text.to_uppercase(), &f.caption_strong);
                    widest_span = widest_span
                        .max(menu_metrics::ICON_LEFT + w + menu_metrics::CONTENT_RIGHT);
                }
                StripItem::Separator(_) => {}
                other => {
                    widest_label = widest_label.max(text_width(canvas, &other.item().text, &f.body));
                }
            }
        }

        // The caret shares the shortcut column, so it only costs a column of its
        // own in a menu that has no shortcut at all.
        let arrow = if shortcut_col == 0.0 && any_caret {
            menu_metrics::CONTENT_RIGHT + menu_metrics::CARET_COLUMN
        } else {
            menu_metrics::CONTENT_RIGHT
        };
        let grid = measure_menu_item(
            widest_label,
            shortcut_col,
            gutter,
            arrow,
            menu_metrics::SHORTCUT_GAP,
        );

        let mut width = grid.width.max(widest_span).max(menu_metrics::MIN_WIDTH);
        // `el.style.maxWidth = vw - 2 * M`: a menu wider than the viewport is
        // capped, and only then do its labels ellipsize.
        if let Some(v) = self.viewport {
            let cap = (v.right - v.left) - 2.0 * VIEWPORT_EDGE;
            if cap > 0.0 {
                width = width.min(cap);
            }
        }
        Size::new(width, self.content_height())
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let dead = state.disabled || !self.inner.control().enabled;
        self.paint_panel(canvas, bounds);

        let shortcut_col = self.shortcut_column(canvas);
        canvas.push_clip(&bounds);
        for i in 0..self.inner.items.len() {
            let Some(row) = self.item_rect(bounds, i) else { continue };
            self.paint_row(canvas, row, i, shortcut_col, dead);
        }
        canvas.pop_clip();

        // The cascaded submenu, painted last so it lifts over its parent,
        // measured and placed by `submenu_rect_in` (flipped left when the
        // viewport has no room on the right), its own hot row lit.
        if let Some(i) = self.open_submenu {
            if let (Some(sub_rect), Some(mut sub)) = (self.submenu_rect_in(canvas, bounds, i), self.submenu(i)) {
                sub.hot_index = self.submenu_hot;
                sub.paint(canvas, sub_rect, WidgetState::REST.disabled(dead));
            }
        }
    }

    fn type_name(&self) -> &'static str {
        "Menu"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
//
// All pure: the measurement, the row grid and the state machines answer with no
// canvas at all, which is the point of keeping selection in the replica.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno_desktop_controls::lists::SelectionMode;

    fn list_of(n: usize) -> ListBox {
        let mut l = ListBox::new();
        for i in 0..n {
            l.add_item(format!("Item {i}"));
        }
        l
    }

    fn bounds(w: f32, h: f32) -> Rect {
        Rect::new(0.0, 0.0, w, h)
    }

    // ── Measurement ──────────────────────────────────────────────────────────

    #[test]
    fn a_list_of_n_items_is_n_rows_plus_its_chrome() {
        let l = list_of(5);
        assert_eq!(l.row_height(), control::COMBO_ROW);
        // 5 × 32 + 2 × (1 border + 4 padding) = 170.
        assert_eq!(l.height_for_rows(5), 5.0 * 32.0 + 10.0);
        assert_eq!(l.height_for_rows(0), 10.0);
    }

    #[test]
    fn an_explicit_item_height_wins_over_the_token() {
        let mut l = list_of(3);
        l.item_height = 48;
        assert_eq!(l.row_height(), 48.0);
        assert_eq!(l.height_for_rows(3), 3.0 * 48.0 + 10.0);
    }

    #[test]
    fn a_checked_row_is_a_list_row_the_kubuno_check_already_fits() {
        // The toolkit had to grow 15 → 18 for a 13 DIP well; the Kubuno row is
        // 32 and the well 18, so nothing is added.
        let c = CheckedListBox::new();
        assert_eq!(c.row_height(), control::COMBO_ROW);
        // The well is centred in the row, so both edges must stay inside it.
        let row = Rect::new(0.0, 0.0, 200.0, c.row_height());
        let well = check_rect(row);
        assert!(well.top > row.top && well.bottom < row.bottom);
    }

    #[test]
    fn a_menu_row_is_thirty_and_a_separator_eleven() {
        assert_eq!(menu_row_height(&MenuEntry::new("Ouvrir").build()), height::MENU_ITEM);
        assert_eq!(menu_row_height(&separator()), 11.0);
        assert_eq!(menu_row_height(&section("Trier")), 28.0);
    }

    #[test]
    fn a_menu_is_every_row_plus_the_panel_padding() {
        let m = Menu::with_items(vec![
            MenuEntry::new("Ouvrir").build(),
            separator(),
            MenuEntry::new("Supprimer").danger().build(),
        ]);
        // 30 + 11 + 30 + 2 × 5.
        assert_eq!(m.content_height(), 81.0);
    }

    #[test]
    fn the_menu_width_rule_is_the_replicas() {
        // The menu is gutter + WIDEST label + the shared shortcut column +
        // right padding, which is exactly what `measure_menu_item` composes —
        // `Menu::measure` calls it once, with the widest label and the widest
        // shortcut, because the web's `subgrid` gives every row the same
        // columns whether or not that row fills them.
        let gutter = menu_metrics::LABEL_LEFT;
        let plain = measure_menu_item(100.0, 0.0, gutter, menu_metrics::CONTENT_RIGHT, menu_metrics::SHORTCUT_GAP);
        assert_eq!(plain.width, gutter + 100.0 + menu_metrics::CONTENT_RIGHT);

        let with_shortcut =
            measure_menu_item(100.0, 40.0, gutter, menu_metrics::CONTENT_RIGHT, menu_metrics::SHORTCUT_GAP);
        assert_eq!(with_shortcut.width, plain.width + menu_metrics::SHORTCUT_GAP + 40.0);

        // A submenu row pays the caret column instead of a shortcut.
        let with_caret = measure_menu_item(
            100.0,
            0.0,
            gutter,
            menu_metrics::CONTENT_RIGHT + menu_metrics::CARET_COLUMN,
            menu_metrics::SHORTCUT_GAP,
        );
        assert_eq!(with_caret.width, plain.width + menu_metrics::CARET_COLUMN);
    }

    #[test]
    fn the_gutter_closes_when_both_margins_are_off() {
        let mut m = Menu::new();
        assert!(m.has_gutter(), "ShowImageMargin defaults to true");
        assert_eq!(m.label_left(), menu_metrics::LABEL_LEFT);
        m.show_image_margin = false;
        m.show_check_margin = false;
        assert!(!m.has_gutter());
        assert_eq!(m.label_left(), menu_metrics::ICON_LEFT);
    }

    // ── item_at, at the boundaries ───────────────────────────────────────────

    #[test]
    fn item_at_answers_the_row_under_the_point() {
        let l = list_of(4);
        let b = bounds(200.0, l.height_for_rows(4));
        let inset = list_metrics::BORDER + list_metrics::PANEL_PAD; // 5

        // The very first pixel of row 0, and the last of it.
        assert_eq!(l.item_at(b, 10.0, inset), Some(0));
        assert_eq!(l.item_at(b, 10.0, inset + 31.99), Some(0));
        // The first pixel of row 1 — the boundary belongs to the LOWER row.
        assert_eq!(l.item_at(b, 10.0, inset + 32.0), Some(1));
        assert_eq!(l.item_at(b, 10.0, inset + 3.0 * 32.0), Some(3));
        // The panel's own padding is nobody's row.
        assert_eq!(l.item_at(b, 10.0, inset - 0.5), None);
        // Outside the panel.
        assert_eq!(l.item_at(b, -1.0, inset + 5.0), None);
        assert_eq!(l.item_at(b, 10.0, b.bottom + 1.0), None);
    }

    #[test]
    fn item_at_never_invents_a_row_past_the_last_item() {
        let l = list_of(2);
        // A panel tall enough for six rows, holding two.
        let b = bounds(200.0, l.height_for_rows(6));
        assert_eq!(l.item_at(b, 10.0, 5.0 + 32.0), Some(1));
        assert_eq!(l.item_at(b, 10.0, 5.0 + 2.0 * 32.0 + 1.0), None);
    }

    #[test]
    fn item_at_and_item_rect_agree() {
        let l = list_of(6);
        let b = bounds(200.0, l.height_for_rows(6));
        for i in 0..6 {
            let r = l.item_rect(b, i).expect("row is on screen");
            let mid = ((r.top + r.bottom) / 2.0, (r.left + r.right) / 2.0);
            assert_eq!(l.item_at(b, mid.1, mid.0), Some(i));
        }
    }

    // ── Scrolling ────────────────────────────────────────────────────────────

    #[test]
    fn top_index_scrolls_what_item_at_reports() {
        let mut l = list_of(10);
        let b = bounds(200.0, l.height_for_rows(3)); // three rows visible
        assert_eq!(l.visible_rows(b), 3);
        assert_eq!(l.item_at(b, 10.0, 5.0 + 1.0), Some(0));

        l.set_top_index(4);
        assert_eq!(l.item_at(b, 10.0, 5.0 + 1.0), Some(4));
        assert_eq!(l.item_at(b, 10.0, 5.0 + 2.0 * 32.0 + 1.0), Some(6));
        // Row 3 is scrolled off the top: it has no rectangle at all.
        assert!(l.item_rect(b, 3).is_none());
        assert!(l.item_rect(b, 4).is_some());
        // And so is row 7, past the bottom.
        assert!(l.item_rect(b, 7).is_none());
    }

    #[test]
    fn set_top_index_is_the_replicas_clamp() {
        let mut l = list_of(3);
        l.set_top_index(99);
        assert_eq!(l.top_index, 2, "the replica clamps to the last row");
    }

    #[test]
    fn multi_column_lays_items_down_then_across() {
        let mut l = list_of(7);
        l.multi_column = true;
        l.column_width = 80;
        // Three rows per column.
        let b = bounds(300.0, l.height_for_rows(3));
        assert_eq!(l.item_at(b, 5.0 + 1.0, 5.0 + 1.0), Some(0));
        assert_eq!(l.item_at(b, 5.0 + 1.0, 5.0 + 2.0 * 32.0 + 1.0), Some(2));
        // Second column, first row.
        assert_eq!(l.item_at(b, 5.0 + 80.0 + 1.0, 5.0 + 1.0), Some(3));
        // Third column, first row — the seventh item.
        assert_eq!(l.item_at(b, 5.0 + 160.0 + 1.0, 5.0 + 1.0), Some(6));
        // The slot under it is empty, and an empty slot is nobody's row.
        assert_eq!(l.item_at(b, 5.0 + 160.0 + 1.0, 5.0 + 32.0 + 1.0), None);
    }

    // ── Selection — the replica's machine, reached through Deref ─────────────

    #[test]
    fn the_selection_machine_is_the_replicas() {
        let mut l = list_of(6);
        l.set_selection_mode(SelectionMode::MultiExtended);

        l.click(1, false, false);
        assert_eq!(l.selected_indices(), &[1usize][..]);

        // Shift extends from the anchor, replacing the set.
        l.click(4, false, true);
        assert_eq!(l.selected_indices(), &[1usize, 2, 3, 4][..]);

        // Ctrl toggles one item out of it.
        l.click(2, true, false);
        assert_eq!(l.selected_indices(), &[1usize, 3, 4][..]);
        assert_eq!(l.selected_index(), 1, "SelectedIndex mirrors the lowest member");

        // A bare click replaces everything.
        l.click(5, false, false);
        assert_eq!(l.selected_indices(), &[5usize][..]);
    }

    #[test]
    fn multi_simple_toggles_and_ignores_the_modifiers() {
        let mut l = list_of(4);
        l.set_selection_mode(SelectionMode::MultiSimple);
        l.click(0, false, false);
        l.click(2, false, true); // Shift is ignored in simple mode.
        assert_eq!(l.selected_indices(), &[0usize, 2][..]);
        l.click(0, false, false);
        assert_eq!(l.selected_indices(), &[2usize][..]);
    }

    #[test]
    fn narrowing_to_one_trims_an_existing_multi_selection() {
        let mut l = list_of(4);
        l.set_selection_mode(SelectionMode::MultiExtended);
        l.click(1, false, false);
        l.click(3, false, true);
        assert_eq!(l.selected_indices(), &[1usize, 2, 3][..]);
        l.set_selection_mode(SelectionMode::One);
        assert_eq!(l.selected_indices(), &[1usize][..]);
    }

    #[test]
    fn a_checked_list_keeps_the_two_way_click_cycle() {
        let mut c = CheckedListBox::new();
        c.add_item("Alpha", CheckState::Unchecked);
        c.add_item("Delta", CheckState::Indeterminate);
        c.check_on_click = true;

        c.click(0);
        assert_eq!(c.get_item_check_state(0), CheckState::Checked);
        // An indeterminate row collapses to Unchecked, NOT to Checked.
        c.click(1);
        assert_eq!(c.get_item_check_state(1), CheckState::Unchecked);
        assert_eq!(c.checked_indices(), vec![0]);
    }

    #[test]
    fn the_check_well_is_hit_testable_on_its_own() {
        let mut c = CheckedListBox::new();
        c.add_item("Alpha", CheckState::Unchecked);
        let b = bounds(200.0, c.height_for_rows(1));
        let row = c.item_rect(b, 0).expect("one row");
        let well = check_rect(row);
        assert_eq!(c.check_at(b, well.left + 1.0, (well.top + well.bottom) / 2.0), Some(0));
        // The label is a row hit but not a check hit.
        assert_eq!(c.item_at(b, 150.0, (row.top + row.bottom) / 2.0), Some(0));
        assert_eq!(c.check_at(b, 150.0, (row.top + row.bottom) / 2.0), None);
    }

    // ── ComboBox: opening, closing, and the drop-down height rule ────────────

    fn combo_of(n: usize) -> ComboBox {
        let mut c = ComboBox::new();
        for i in 0..n {
            c.add_item(format!("Option {i}"));
        }
        c
    }

    #[test]
    fn a_combo_opens_closes_and_toggles() {
        let mut c = combo_of(3);
        assert!(!c.is_open(), "a fresh DropDown combo is closed");
        assert_eq!(c.item_at(bounds(200.0, 200.0), 10.0, 60.0), None);

        c.toggle();
        assert!(c.is_open());
        c.hot_index = Some(1);

        c.close();
        assert!(!c.is_open());
        assert_eq!(c.hot_index, None, "closing drops the hot row with the list");

        c.open();
        assert!(c.is_open());
    }

    #[test]
    fn a_simple_combo_is_always_open() {
        let mut c = combo_of(3);
        c.drop_down_style = replica::ComboBoxStyle::Simple;
        assert!(c.is_open(), "Simple shows its list permanently");
        c.close();
        assert!(c.is_open(), "and closing cannot take it away");
    }

    #[test]
    fn max_drop_down_items_governs_while_integral_height_is_on() {
        let mut c = combo_of(20);
        assert!(c.integral_height, "the toolkit default");
        assert_eq!(c.max_drop_down_items, 8, "the toolkit default");
        assert_eq!(c.visible_rows(), 8);

        c.max_drop_down_items = 3;
        assert_eq!(c.visible_rows(), 3);
        assert_eq!(c.drop_down_height(), 3.0 * control::COMBO_ROW + 2.0 * list_metrics::PANEL_PAD);
    }

    #[test]
    fn drop_down_height_takes_over_when_integral_height_is_off() {
        let mut c = combo_of(20);
        c.integral_height = false;
        assert_eq!(c.drop_down_height, 106, "the toolkit default");
        assert_eq!(c.drop_down_height(), 106.0);
        // (106 − 2 × 4) / 32 = 3 whole rows.
        assert_eq!(c.visible_rows(), 3);
    }

    #[test]
    fn a_shorter_list_never_pads_itself_out_to_the_maximum() {
        let c = combo_of(2);
        assert_eq!(c.visible_rows(), 2);
        assert_eq!(c.drop_down_height(), 2.0 * control::COMBO_ROW + 2.0 * list_metrics::PANEL_PAD);
    }

    #[test]
    fn combo_item_at_walks_the_open_list_only() {
        let mut c = combo_of(4);
        let b = bounds(200.0, 400.0);
        assert_eq!(c.item_at(b, 20.0, 60.0), None, "closed: nothing to hit");

        c.open();
        let panel = c.drop_down_rect(b);
        let first = panel.top + list_metrics::PANEL_PAD;
        assert_eq!(c.item_at(b, 20.0, first), Some(0));
        assert_eq!(c.item_at(b, 20.0, first + control::COMBO_ROW - 0.01), Some(0));
        assert_eq!(c.item_at(b, 20.0, first + control::COMBO_ROW), Some(1));
        assert_eq!(c.item_at(b, 20.0, first - 1.0), None, "the panel padding is nobody's row");
        assert_eq!(c.item_at(b, 20.0, first + 4.0 * control::COMBO_ROW), None);
    }

    #[test]
    fn the_field_sits_above_the_list_and_both_are_hit_testable() {
        let mut c = combo_of(3);
        let b = bounds(200.0, 300.0);
        assert_eq!(c.field_rect(b).bottom, height::BUTTON_MD);
        c.open();
        assert_eq!(c.drop_down_rect(b).top, height::BUTTON_MD + DROP_DOWN_OFFSET);
        assert!(c.hit_test(b, 20.0, 10.0), "the field");
        assert!(c.hit_test(b, 20.0, height::BUTTON_MD + 20.0), "the open list");
        c.close();
        assert!(!c.hit_test(b, 20.0, height::BUTTON_MD + 20.0));
    }

    #[test]
    fn sorted_and_the_selection_follow_the_replica() {
        let mut c = ComboBox::new();
        c.sorted = true;
        c.add_item("Zeta");
        c.add_item("Alpha");
        assert_eq!(c.items, vec!["Alpha".to_string(), "Zeta".to_string()]);
        c.set_selected_index(1);
        assert_eq!(c.selected_item(), Some("Zeta"));
        assert_eq!(c.display_text(), "Zeta");
        // Inserting ahead of the selection carries it along.
        c.add_item("Beta");
        assert_eq!(c.selected_item(), Some("Zeta"));
    }

    // ── Menu geometry ────────────────────────────────────────────────────────

    #[test]
    fn menu_item_at_walks_the_rows_and_refuses_a_separator() {
        let m = Menu::with_items(vec![
            MenuEntry::new("Ouvrir").build(),
            separator(),
            MenuEntry::new("Renommer").build(),
        ]);
        let b = bounds(220.0, m.content_height());
        let pad = menu_metrics::PANEL_PAD;

        assert_eq!(m.item_at(b, 20.0, pad), Some(0));
        assert_eq!(m.item_at(b, 20.0, pad + 29.99), Some(0));
        // The separator's own band answers None — it is never a target.
        assert_eq!(m.item_at(b, 20.0, pad + 30.0), None);
        assert_eq!(m.item_at(b, 20.0, pad + 40.0), None);
        assert_eq!(m.item_at(b, 20.0, pad + 41.0), Some(2));
        assert_eq!(m.item_at(b, 20.0, b.bottom + 1.0), None);
    }

    #[test]
    fn menu_item_rect_stacks_the_rows_in_order() {
        let m = Menu::with_items(vec![
            MenuEntry::new("Un").build(),
            separator(),
            MenuEntry::new("Deux").build(),
        ]);
        let b = bounds(220.0, m.content_height());
        let r0 = m.item_rect(b, 0).expect("row 0");
        let r2 = m.item_rect(b, 2).expect("row 2");
        assert_eq!(r0.top, b.top + menu_metrics::PANEL_PAD);
        assert_eq!(r0.bottom - r0.top, height::MENU_ITEM);
        assert_eq!(r2.top, r0.bottom + 11.0);
        assert!(m.item_rect(b, 3).is_none());
    }

    #[test]
    fn a_submenu_opens_beside_its_row() {
        let m = Menu::with_items(vec![
            MenuEntry::new("Ouvrir").build(),
            MenuEntry::new("Ouvrir avec")
                .submenu(vec![MenuEntry::new("Éditeur").build(), MenuEntry::new("Aperçu").build()])
                .build(),
        ]);
        let b = bounds(220.0, m.content_height());
        let row = m.item_rect(b, 1).expect("row 1");
        let sub = m.submenu_rect(b, 1).expect("row 1 has a submenu");
        assert_eq!(sub.top, row.top - SUBMENU_RISE);
        assert_eq!(sub.left, row.right - SUBMENU_OVERLAP);
        assert_eq!(sub.right - sub.left, menu_metrics::SUBMENU_WIDTH);
        // Two rows plus the panel padding.
        assert_eq!(sub.bottom - sub.top, 2.0 * height::MENU_ITEM + 2.0 * menu_metrics::PANEL_PAD);
        // A row with no children has none.
        assert!(m.submenu_rect(b, 0).is_none());
    }

    #[test]
    fn the_item_model_is_the_replicas() {
        let item = MenuEntry::new("Supprimer")
            .icon("Trash2")
            .shortcut(false, false, false, "Suppr")
            .danger()
            .enabled(false)
            .build();
        let StripItem::MenuItem(m) = &item else { panic!("a menu item") };
        assert_eq!(m.base.item.text, "Supprimer");
        assert_eq!(m.base.item.image.as_deref(), Some("Trash2"));
        assert_eq!(m.shortcut_text(), "Suppr");
        assert_eq!(m.base.item.tag.as_deref(), Some(DANGER_TAG));
        assert!(!m.base.item.enabled);
    }

    // ── Keyboard: pure navigation ───────────────────────────────────────────

    #[test]
    fn list_step_follows_the_aria_listbox_rules() {
        use ListKey::*;
        assert_eq!(list_step(None, 0, Down, 5), None, "an empty list has no row");
        assert_eq!(list_step(None, 6, Down, 5), Some(0));
        assert_eq!(list_step(None, 6, Up, 5), Some(5));
        assert_eq!(list_step(Some(2), 6, Down, 5), Some(3));
        assert_eq!(list_step(Some(5), 6, Down, 5), Some(5), "no wrap at the end");
        assert_eq!(list_step(Some(0), 6, Up, 5), Some(0), "no wrap at the top");
        assert_eq!(list_step(Some(3), 6, Home, 5), Some(0));
        assert_eq!(list_step(Some(3), 6, End, 5), Some(5));
        assert_eq!(list_step(Some(1), 20, PageDown, 5), Some(6));
        assert_eq!(list_step(Some(18), 20, PageDown, 5), Some(19), "clamped");
        assert_eq!(list_step(Some(3), 20, PageUp, 5), Some(0));
        assert_eq!(list_step(Some(4), 20, PageDown, 0), Some(5), "a page is at least a row");
    }

    #[test]
    fn step_wrapping_skips_unusable_rows_and_wraps() {
        let usable = |i: usize| i != 1 && i != 3;
        assert_eq!(step_wrapping(Some(0), 5, true, usable), Some(2));
        assert_eq!(step_wrapping(Some(4), 5, true, usable), Some(0), "wraps forward");
        assert_eq!(step_wrapping(Some(0), 5, false, usable), Some(4), "wraps backward");
        assert_eq!(step_wrapping(None, 5, false, usable), Some(4));
        assert_eq!(step_wrapping(None, 3, true, |_| false), None);
    }

    #[test]
    fn type_ahead_cycles_a_repeated_letter_and_extends_a_prefix() {
        let items = ["Documents", "Dossiers", "Images", "Émissions", "Données"];
        let it = || items.iter().copied();
        // One letter: next row starting with it, after the current one.
        assert_eq!(type_ahead_match(it(), "d", None), Some(0));
        assert_eq!(type_ahead_match(it(), "d", Some(0)), Some(1));
        assert_eq!(type_ahead_match(it(), "dd", Some(1)), Some(4), "a repeated key cycles");
        assert_eq!(type_ahead_match(it(), "d", Some(4)), Some(0), "and wraps");
        // A longer prefix keeps the current row when it still matches.
        assert_eq!(type_ahead_match(it(), "do", Some(0)), Some(0));
        assert_eq!(type_ahead_match(it(), "dos", Some(0)), Some(1));
        // Accent- and case-insensitive, as the web's foldText.
        assert_eq!(type_ahead_match(it(), "em", None), Some(3));
        assert_eq!(type_ahead_match(it(), "DON", None), Some(4));
        assert_eq!(type_ahead_match(it(), "zz", None), None);
        assert_eq!(fold("Œuvre Été"), "oeuvre ete");
    }

    #[test]
    fn the_type_ahead_buffer_resets_after_a_pause() {
        let mut t = TypeAhead::default();
        assert_eq!(t.push("d", 10_000), "d");
        assert_eq!(t.push("o", 10_300), "do");
        assert_eq!(t.push("i", 10_300 + TypeAhead::TIMEOUT_MS + 1), "i");
    }

    #[test]
    fn top_to_show_moves_the_window_as_little_as_possible() {
        assert_eq!(top_to_show(0, 2, 5), 0, "already visible");
        assert_eq!(top_to_show(0, 7, 5), 3, "scrolled so it is the last row");
        assert_eq!(top_to_show(6, 2, 5), 2, "scrolled so it is the first row");
    }

    // ── Keyboard: ListBox / CheckedListBox ───────────────────────────────────

    #[test]
    fn listbox_arrows_move_the_selection_and_scroll_it_into_view() {
        let mut l = list_of(10);
        let b = bounds(200.0, l.height_for_rows(3));
        assert!(l.handle_key(b, ListKey::Down, false, false));
        assert_eq!(l.selected_index(), 0, "first arrow lands on the first row");
        l.handle_key(b, ListKey::End, false, false);
        assert_eq!(l.selected_index(), 9);
        assert_eq!(l.top_index, 7, "the last row is scrolled into view");
        l.handle_key(b, ListKey::PageUp, false, false);
        assert_eq!(l.selected_index(), 6);
        assert_eq!(l.top_index, 6);
        l.handle_key(b, ListKey::Home, false, false);
        assert_eq!((l.selected_index(), l.top_index), (0, 0));
    }

    #[test]
    fn listbox_shift_extends_and_ctrl_space_toggles_in_multi_extended() {
        let mut l = list_of(6);
        l.set_selection_mode(SelectionMode::MultiExtended);
        let b = bounds(200.0, l.height_for_rows(6));
        l.pointer_select(1, false, false);
        l.handle_key(b, ListKey::Down, false, true);
        l.handle_key(b, ListKey::Down, false, true);
        assert_eq!(l.selected_indices(), &[1usize, 2, 3][..], "Shift+Down extends from the anchor");
        // Ctrl+Down moves the active row alone; Ctrl+Space adds it.
        l.handle_key(b, ListKey::Down, true, false);
        assert_eq!(l.focus_index, Some(4));
        assert_eq!(l.selected_indices(), &[1usize, 2, 3][..]);
        assert!(l.press_space(true));
        assert_eq!(l.selected_indices(), &[1usize, 2, 3, 4][..]);
    }

    #[test]
    fn listbox_type_ahead_selects_the_matching_row() {
        let mut l = ListBox::new();
        for s in ["Documents", "Images", "Musique", "Téléchargements", "Vidéos"] {
            l.add_item(s);
        }
        let b = bounds(200.0, l.height_for_rows(2));
        assert_eq!(l.type_to_select(b, "t", 0), Some(3));
        assert_eq!(l.selected_index(), 3);
        assert_eq!(l.top_index, 2, "scrolled to show it");
        assert_eq!(l.type_to_select(b, "v", 5_000), Some(4));
    }

    #[test]
    fn listbox_wheel_scrolls_within_the_last_full_page() {
        let mut l = list_of(10);
        let b = bounds(200.0, l.height_for_rows(4));
        l.scroll_rows(b, 3);
        assert_eq!(l.top_index, 3);
        l.scroll_rows(b, 30);
        assert_eq!(l.top_index, 6, "10 rows, 4 visible → top 6 at most");
        l.scroll_rows(b, -30);
        assert_eq!(l.top_index, 0);
    }

    #[test]
    fn checked_list_space_toggles_the_active_row() {
        let mut c = CheckedListBox::new();
        for s in ["A", "B", "C"] {
            c.add_item(s, CheckState::Unchecked);
        }
        let b = bounds(200.0, c.height_for_rows(3));
        c.handle_key(b, ListKey::Down);
        c.handle_key(b, ListKey::Down);
        assert_eq!(c.focus_index, Some(1));
        assert_eq!(c.selected_index(), 1);
        assert!(c.press_space());
        assert_eq!(c.get_item_check_state(1), CheckState::Checked);
        // The well toggles on a click; the label only with CheckOnClick.
        c.pointer_select(2, false);
        assert_eq!(c.get_item_check_state(2), CheckState::Unchecked);
        c.pointer_select(2, true);
        assert_eq!(c.get_item_check_state(2), CheckState::Checked);
    }

    // ── Keyboard: ComboBox ───────────────────────────────────────────────────

    #[test]
    fn combo_keyboard_opens_moves_commits_and_escapes() {
        let mut c = combo_of(6);
        c.max_drop_down_items = 3;
        c.set_selected_index(4);
        assert_eq!(c.handle_key(ComboKey::Tab), ComboOutcome::Ignored);
        assert_eq!(c.handle_key(ComboKey::AltDown), ComboOutcome::Opened);
        assert_eq!(c.hot_index, Some(4), "reopening starts on the selection");
        assert_eq!(c.scroll_top, 2, "and scrolls it into view");
        c.handle_key(ComboKey::Down);
        c.handle_key(ComboKey::Down);
        assert_eq!(c.hot_index, Some(0), "the arrows wrap, like firstEnabled");
        assert_eq!(c.scroll_top, 0);
        c.handle_key(ComboKey::End);
        assert_eq!(c.hot_index, Some(5));
        assert_eq!(c.handle_key(ComboKey::Escape), ComboOutcome::Closed);
        assert_eq!(c.selected_index, 4, "Escape does not commit");
        c.handle_key(ComboKey::Down);
        c.handle_key(ComboKey::Up);
        assert_eq!(c.handle_key(ComboKey::Enter), ComboOutcome::Committed(3));
        assert!(!c.is_open());
        assert_eq!(c.selected_index, 3);
    }

    #[test]
    fn combo_type_ahead_changes_the_selection_while_closed() {
        let mut c = ComboBox::new();
        for s in ["Deutsch", "English", "Español", "Français"] {
            c.add_item(s);
        }
        assert_eq!(c.type_to_select("f", 0), Some(3));
        assert_eq!(c.selected_index, 3);
        c.open();
        assert_eq!(c.type_to_select("e", 5_000), Some(1));
        assert_eq!(c.hot_index, Some(1));
        assert_eq!(c.selected_index, 3, "open, it only moves the active row");
    }

    #[test]
    fn combo_scrolls_past_max_drop_down_items() {
        let mut c = combo_of(6);
        c.max_drop_down_items = 5;
        assert!(c.drop_down_scrolls());
        assert_eq!(c.max_scroll_top(), 1);
        c.open();
        let b = bounds(240.0, 400.0);
        let panel = c.drop_down_rect(b);
        let first = panel.top + list_metrics::PANEL_PAD + 1.0;
        assert_eq!(c.item_at(b, 20.0, first), Some(0));
        c.scroll_drop_down(3);
        assert_eq!(c.scroll_top, 1, "clamped to the last full page");
        assert_eq!(c.item_at(b, 20.0, first), Some(1));
        assert!(c.drop_down_item_rect(panel, 0).is_none(), "row 0 is scrolled out");
        assert!(c.drop_down_item_rect(panel, 5).is_some(), "Português is reachable");
    }

    #[test]
    fn combo_list_flips_above_and_stays_inside_the_viewport() {
        let mut c = combo_of(4);
        c.open();
        let area = Rect::new(0.0, 0.0, 800.0, 600.0);
        // Room below: under the field.
        let b = Rect::new(100.0, 100.0, 300.0, 136.0);
        let (r, d) = (c.drop_down_rect_in(b, area), c.drop_down_rect(b));
        assert_eq!((r.left, r.top, r.right, r.bottom), (d.left, d.top, d.right, d.bottom));
        // No room below: above the field, 2 DIP off it.
        let b = Rect::new(100.0, 540.0, 300.0, 576.0);
        let r = c.drop_down_rect_in(b, area);
        assert_eq!(r.bottom, b.top - DROP_DOWN_OFFSET);
        // Too far right: pulled back inside the right edge.
        let b = Rect::new(700.0, 100.0, 790.0, 136.0);
        let r = c.drop_down_rect_in(b, area);
        assert_eq!(r.right, area.right - VIEWPORT_EDGE);
        assert_eq!(r.right - r.left, list_metrics::MIN_WIDTH, "minWidth 200");
        // The paint bounds carry the shadow.
        let pb = c.drop_down_paint_bounds(b);
        assert_eq!(pb.left, c.drop_down_rect(b).left - FLOAT_SHADOW_MARGIN);
    }

    #[test]
    fn the_float_shadow_margin_covers_every_layer() {
        let reach = SHADOW_MENU.iter().fold(0.0_f32, |m, l| m.max(l.blur + l.spread + l.dy.abs()));
        assert!(FLOAT_SHADOW_MARGIN >= reach, "{FLOAT_SHADOW_MARGIN} < {reach}");
    }

    // ── Keyboard: Menu ───────────────────────────────────────────────────────

    fn nav_menu() -> Menu {
        Menu::with_items(vec![
            section("Actions"),
            MenuEntry::new("Ouvrir").build(),
            MenuEntry::new("Ouvrir avec")
                .submenu(vec![
                    MenuEntry::new("Éditeur").build(),
                    MenuEntry::new("Aperçu").enabled(false).build(),
                    MenuEntry::new("Autre").build(),
                ])
                .build(),
            separator(),
            MenuEntry::new("Historique").enabled(false).build(),
            MenuEntry::new("Supprimer").danger().build(),
        ])
    }

    #[test]
    fn menu_arrows_skip_labels_separators_and_dead_rows_and_wrap() {
        let mut m = nav_menu();
        assert_eq!(m.navigate(MenuKey::Down), MenuOutcome::Moved);
        assert_eq!(m.hot_index, Some(1), "the section label is skipped");
        m.navigate(MenuKey::Down);
        m.navigate(MenuKey::Down);
        assert_eq!(m.hot_index, Some(5), "separator and dead row skipped");
        m.navigate(MenuKey::Down);
        assert_eq!(m.hot_index, Some(1), "wraps");
        m.navigate(MenuKey::Up);
        assert_eq!(m.hot_index, Some(5));
        m.navigate(MenuKey::Home);
        assert_eq!(m.hot_index, Some(1));
        assert_eq!(m.navigate(MenuKey::Enter), MenuOutcome::Chosen { index: 1, sub: None });
        assert_eq!(m.navigate(MenuKey::Escape), MenuOutcome::Close);
    }

    #[test]
    fn menu_right_enters_the_submenu_and_left_leaves_it() {
        let mut m = nav_menu();
        m.hot_index = Some(2);
        assert_eq!(m.navigate(MenuKey::Right), MenuOutcome::SubmenuOpened);
        assert_eq!((m.open_submenu, m.submenu_hot), (Some(2), Some(0)));
        m.navigate(MenuKey::Down);
        assert_eq!(m.submenu_hot, Some(2), "the dead « Aperçu » is skipped");
        assert_eq!(m.navigate(MenuKey::Enter), MenuOutcome::Chosen { index: 2, sub: Some(2) });
        assert_eq!(m.navigate(MenuKey::Left), MenuOutcome::SubmenuClosed);
        assert_eq!((m.open_submenu, m.submenu_hot, m.hot_index), (None, None, Some(2)));
        // Right on a leaf does nothing; Escape inside a submenu only closes it.
        m.hot_index = Some(1);
        assert_eq!(m.navigate(MenuKey::Right), MenuOutcome::Ignored);
        m.hot_index = Some(2);
        m.navigate(MenuKey::Enter);
        assert_eq!(m.navigate(MenuKey::Escape), MenuOutcome::SubmenuClosed);
    }

    #[test]
    fn menu_type_ahead_jumps_to_the_next_matching_row() {
        let mut m = nav_menu();
        assert!(m.type_to_focus("o"));
        assert_eq!(m.hot_index, Some(1));
        assert!(m.type_to_focus("o"));
        assert_eq!(m.hot_index, Some(2));
        assert!(!m.type_to_focus("h"), "a dead row is not a target");
    }

    #[test]
    fn a_submenu_flips_left_when_the_right_has_no_room() {
        let v = Rect::new(0.0, 0.0, 1000.0, 700.0);
        // Room on the right: cascades right, overlapping by 2.
        let row = Rect::new(100.0, 100.0, 320.0, 130.0);
        let r = place_submenu(row, 220.0, 100.0, Some(v));
        assert_eq!((r.left, r.top), (row.right - SUBMENU_OVERLAP, row.top - SUBMENU_RISE));
        // No room on the right, room on the left: opens left of the row.
        let row = Rect::new(600.0, 100.0, 900.0, 130.0);
        let r = place_submenu(row, 220.0, 100.0, Some(v));
        assert_eq!(r.right, row.left + SUBMENU_OVERLAP);
        // Near the bottom: pulled up inside the viewport.
        let row = Rect::new(100.0, 650.0, 320.0, 680.0);
        let r = place_submenu(row, 220.0, 100.0, Some(v));
        assert_eq!(r.bottom, v.bottom - VIEWPORT_EDGE);
        // No viewport: the plain cascade.
        let r = place_submenu(row, 220.0, 100.0, None);
        assert_eq!(r.left, row.right - SUBMENU_OVERLAP);
    }

    #[test]
    fn a_shortcut_reads_in_the_toolkits_modifier_order() {
        let item = MenuEntry::new("Tout sélectionner").shortcut(true, false, true, "A").build();
        let StripItem::MenuItem(m) = &item else { panic!("a menu item") };
        assert_eq!(m.shortcut_text(), "Ctrl+Shift+A");

        // `ShortcutKeyDisplayString` overrides it, as the toolkit says.
        let item = MenuEntry::new("Fermer").shortcut(false, true, false, "F4").shortcut_text("Alt+F4").build();
        let StripItem::MenuItem(m) = &item else { panic!("a menu item") };
        assert_eq!(m.shortcut_text(), "Alt+F4");
    }
}
