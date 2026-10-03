//! Kubuno primitives — the **data table**: [`DataTable`].
//!
//! # Where the pixels come from
//!
//! This family has no predecessor in `kubuno-drive-desktop-app-controls` and no WinForms
//! counterpart: `DataGridView` is a different control with a different model,
//! and the product never shipped one. The reference is therefore the **web**
//! design system, read from the source (this machine cannot run the web app, so
//! every number below was *read*, not measured):
//!
//! | part | web source |
//! |---|---|
//! | the table itself, header, rows, states | `core/frontend/src/ui/data-table/DataTable.tsx` |
//! | toolbar (idle face) and bulk-action bar | `…/data-table/Toolbar.tsx` |
//! | pagination footer | `…/data-table/Pagination.tsx` |
//! | loading skeleton | `…/data-table/Skeleton.tsx` |
//! | narrow layout (cards) | `…/data-table/Cards.tsx` |
//! | sort cycle, blanks, column labels | `…/data-table/helpers.ts` |
//! | the empty states it renders | `core/frontend/src/ui/EmptyState.tsx` |
//! | wording | `core/frontend/src/core/i18n/locales/fr/core.json` (`ui.dt_*`) |
//!
//! # It is a [`ListView`], plus chrome
//!
//! The row/column/sort/selection model is **not restated**: a [`DataTable`]
//! owns a [`crate::views::ListView`] — which itself owns
//! [`kubuno_desktop_controls::views::ListView`] — and reaches all of it through
//! [`Deref`]. Columns and their widths, items and their sub-items, the
//! per-item `selected` flag, [`kc::ListView::set_selected`] (which is what
//! enforces `MultiSelect`), [`kc::ListView::selected_indices`], the sort
//! direction, the click/ctrl/shift machine, the row band arithmetic
//! ([`kc::DetailsGeometry`]) and above all the **virtualisation**
//! ([`crate::views::ListView::visible_range`]) all come from there. What this
//! layer adds is exactly what the web component adds over a bare table: a
//! toolbar, a bulk-action bar, a pagination footer, three empty states, a
//! loading skeleton, a card layout for a narrow container — and the DataTable's
//! own ink.
//!
//! ## Why the rows are painted here and not by [`crate::views::ListView`]
//!
//! Geometry and state are reused; the **paint** is not, because the two
//! disagree on purpose and the source says so in as many words:
//!
//! * a selected file row wears « a tinted band plus the 3 DIP accent edge
//!   (`border-l-[3px] border-primary`) » (`views.rs`), while `DataTable.tsx`
//!   writes « Selection outranks both — it is a state, not a reading aid.
//!   **Tint only, never a left accent bar.** »;
//! * a data table is **zebra-striped** (`i % 2 === 1 && 'bg-surface-1'`) and
//!   hovers one step further on `surface-2`; a file row has no zebra and hovers
//!   on `row_hover`;
//! * every cell of a data table is `text-text-primary` at
//!   `--kb-text-body`, aligned per column; a file row paints column 0 at 14 and
//!   every metadata column at 12 / `text_tertiary`;
//! * a data table's first column is aligned like any other, where a file list
//!   forces column 0 to `Left` (the Win32 header cannot align its first column);
//! * a file row starts with an icon gutter and a `name_left()` inset; a data
//!   table has a `w-10` selection column, `px-4` cells and a `w-12` actions
//!   column instead;
//! * the header band is 40 DIP here (`py-2.5` over a body line) against 28 in
//!   the Drive file area.
//!
//! None of those six is a setting on [`crate::views::ListView`], so the choice
//! was between adding six flags to a family this one does not own and owning
//! ~60 lines of ink. The ink is here; every rectangle it paints into still
//! comes from the ListView.
//!
//! ## Virtualisation
//!
//! [`DataTable::visible_rows`] delegates to
//! [`crate::views::ListView::visible_range`] and then clamps to the current
//! page, so a table of 100 000 rows with pagination off paints a screenful.
//! The web has no virtualisation at all — it mounts `pageRows` into the DOM and
//! relies on the page size to keep that small.
//!
//! # What the web does and this port does NOT
//!
//! Said out loud rather than left to be discovered:
//!
//! * **the touch branch of the popups**: the column chooser, the row overflow
//!   menu, the bulk overflow and the right-click copy menu (cell / row /
//!   column) ARE ported — [`DataTable::menu`] builds each as a
//!   [`crate::lists::Menu`], [`DataTable::menu_rect`] places it against the
//!   monitor (the caller paints it in a `host::popup`, so it can leave the
//!   window), [`DataTable::menu_command`] says what an entry does. Their
//!   `MobileSheet` counterparts are not, nor « Copier le texte sélectionné »:
//!   a canvas has no text selection to copy.
//! * **`table-layout: auto`**: the web lets the browser distribute the width
//!   between columns from their content. Column widths here are the ones
//!   declared on the [`ColumnHeader`] — the web's state AFTER a first resize,
//!   when `useColumnResize` pins `table-layout: fixed`. Resizing itself (the
//!   8 DIP grip, the 56 DIP floor, the ±8 / ±24 keyboard nudge, the
//!   double-click reset) is ported, and so is the horizontal scroller: content
//!   wider than the box ([`DataTable::min_table_width`] included) scrolls
//!   inside it ([`DataTable::scroll_x`]).
//! * **the page-size chooser stays a radio group**, not a popup:
//!   `Pagination.tsx` says « three or four values do not justify a popup ».
//!   When the footer is too narrow for it, the web wraps the bar
//!   (`flex-wrap`); the one-line band here drops the group instead, as its own
//!   `compact` mode does.
//! * **`sortRows`' comparator**: the web sinks blank cells whatever the
//!   direction and compares with `localeCompare(…, {numeric, sensitivity})`, so
//!   « Poste 2 » precedes « Poste 10 ». Sorting here is
//!   [`crate::views::ListView::sort`], i.e. the replica's ordering by `cell()`.
//! * **restoring the incoming order** when the sort cycles back to « none ».
//!   The web sorts a *copy* and can hand back `rows` untouched; the replica
//!   sorts the item list in place, and keeping a shadow copy of it here would
//!   be a second source of truth for the rows — which rule 1 of the brief
//!   exists to prevent. [`DataTable::toggle_sort`] therefore reaches
//!   [`SortOrder::None`] (and stops drawing an indicator) without unsorting.
//! * **the coarse-pointer branch** (`isCoarsePointer`), which only chooses
//!   between a sheet and a menu — both of which are out of scope above.
//! * ARIA (`aria-sort`, `role="radiogroup"`, the `sr-only` status line).
//!
//! # What this port adds over the web
//!
//! * **row focus**: the web table has no keyboard row model at all. Rows are
//!   one tab stop here ([`FocusPart::Rows`]) walked with the arrows (the ARIA
//!   grid pattern), Space / Shift / Ctrl+A for selection, Enter for
//!   `onRowClick`, the context-menu key for the menus — [`DataTable::on_key`];
//! * **a vertical scroll** under a sticky header: the web table grows with its
//!   page and lets the page scroll. A box shorter than its page scrolls here
//!   ([`DataTable::scroll_by`]) with WinUI's overlay bar;
//! * Shift+click range selection on the row boxes;
//! * a per-cell painter ([`DataTable::cell_painter`]) standing in for the web
//!   column's `cell` renderer, which a plain string sub-item cannot express.

use std::ops::{Deref, DerefMut, Range};

use kubuno_drive_desktop_app_controls::{Canvas, Rect};
use kubuno_desktop_controls::enums::{CheckState, HorizontalAlignment, Size};
use kubuno_desktop_controls::views as kc;
use kubuno_desktop_controls::Control;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use kubuno_drive_desktop_app_controls::scrollbar::{self as sb, Scrollbar};
use kubuno_desktop_controls::host::{vk, Cursor, Modifiers};

use crate::buttons::{Button, Size as ButtonSize, Variant};
use crate::lists::{section, Menu, MenuEntry};
use crate::metrics::{control, height, pill, radius};
use crate::views::ListView;
use crate::widget::{Widget, WidgetState};

pub use crate::views::{ColumnHeader, ListViewItem, SortOrder};

mod edit;
pub use edit::{CellAction, CellEditor, CellError, CellMove, ERROR_GLYPH};

// ─────────────────────────────────────────────────────────────────────────────
//  Family metrics
//
//  `crate::metrics` is the crate's ONE table and everything it already answers
//  is taken from it (the spacing scale, the radii, `height::BUTTON_SM`, the
//  check-box geometry). What is left below is the `DataTable` grid itself: the
//  lengths those five files state literally and no token names. Each one quotes
//  the declaration it is.
// ─────────────────────────────────────────────────────────────────────────────

/// The web grid, verbatim from `data-table/*.tsx`.
pub mod grid {
    use crate::metrics::{height, space};

    /// A `<th>` / `<td>`: `px-4 py-2.5`. The spacing scale has no 10, so the
    /// vertical inset is written as the half-step Tailwind's `2.5` is.
    pub const CELL_PAD_X: f32 = space::LG;
    pub const CELL_PAD_Y: f32 = 10.0;

    /// The one body line box of this design system — 14 over 20, the same line
    /// the replica layer measures a `Body` role with.
    pub const LINE: f32 = 20.0;

    /// A row, header included: `py-2.5` over a body line. It comes to 40, which
    /// is also [`height::FILE_ROW`] and therefore
    /// [`crate::views::Density::Normal`] — a coincidence a test pins, so the
    /// two never drift apart silently.
    pub const ROW: f32 = CELL_PAD_Y * 2.0 + LINE;

    /// The selection column: `w-10 px-3` on both the `<th>` and the `<td>`.
    pub const SELECT_COL: f32 = 40.0;
    pub const SELECT_PAD_X: f32 = space::MD;

    /// The row-actions column: `w-12 px-2`, holding an `h-7 w-7` button with a
    /// 15 DIP glyph.
    pub const ACTIONS_COL: f32 = 48.0;
    pub const ACTIONS_PAD_X: f32 = space::SM;
    pub const ICON_BUTTON: f32 = 28.0;
    pub const ICON_GLYPH: f32 = 15.0;

    /// The sort indicator: `<ArrowUp size={12} />` after a `gap-1`.
    pub const SORT_GLYPH: f32 = 12.0;
    pub const SORT_GAP: f32 = space::XS;
    /// `opacity-60` — what the idle double chevron fades to under the pointer.
    pub const SORT_HINT_ALPHA: f32 = 0.6;

    /// The bordered box the table lives in: `rounded-xl border bg-surface-0`,
    /// with `gap-2` between it and the toolbar above it.
    pub const TOOLBAR_GAP: f32 = space::SM;

    /// The idle toolbar: a `h-8` column-chooser button (`px-2.5 gap-1.5`, a 14
    /// DIP glyph) beside the title, so the band is one button tall.
    pub const TOOLBAR_ROW: f32 = height::BUTTON_SM;
    pub const CHOOSER_PAD_X: f32 = 10.0;
    pub const CHOOSER_GAP: f32 = 6.0;
    pub const CHOOSER_GLYPH: f32 = 14.0;

    /// The bulk bar: `rounded-lg bg-primary-light px-2.5 py-2 gap-2` around
    /// `size="sm"` buttons, a `p-1` close button over a 15 DIP glyph and a
    /// `p-1.5` overflow over a 16 DIP one.
    pub const BAR_PAD_X: f32 = 10.0;
    pub const BAR_PAD_Y: f32 = space::SM;
    pub const BAR_GAP: f32 = space::SM;
    pub const BAR_HEIGHT: f32 = BAR_PAD_Y * 2.0 + height::BUTTON_SM;
    pub const CLOSE_BUTTON: f32 = 23.0;
    pub const CLOSE_GLYPH: f32 = 15.0;
    pub const OVERFLOW_BUTTON: f32 = 28.0;
    pub const OVERFLOW_GLYPH: f32 = 16.0;

    /// The footer: `border-t bg-surface-1 px-3 py-2` around a 28 DIP control
    /// row, `gap-0.5` inside the nav cluster, `gap-3` between the size group
    /// and the count, `gap-1.5` inside the size group itself.
    pub const FOOTER_PAD_X: f32 = space::MD;
    pub const FOOTER_PAD_Y: f32 = space::SM;
    pub const FOOTER_ROW: f32 = 28.0;
    pub const FOOTER_HEIGHT: f32 = FOOTER_PAD_Y * 2.0 + FOOTER_ROW;
    pub const NAV_GAP: f32 = space::XXS;
    /// `px-2` around « 3 / 12 ».
    pub const PAGE_LABEL_PAD: f32 = space::SM;
    /// The page-size pill group: `rounded-md bg-surface-2 p-0.5` around
    /// `rounded-sm px-1.5 py-0.5` buttons.
    pub const SIZE_GROUP_PAD: f32 = 2.0;
    pub const SIZE_PILL_PAD_X: f32 = 6.0;
    pub const SIZE_PILL_H: f32 = LINE + 4.0;
    pub const SIZE_LABEL_GAP: f32 = 6.0;
    pub const FOOTER_CLUSTER_GAP: f32 = space::MD;

    /// The skeleton: rows of `px-4 py-3 gap-4` holding `h-3.5` bars and, when
    /// the table is selectable, an 18 DIP box.
    pub const SKELETON_PAD_X: f32 = space::LG;
    pub const SKELETON_PAD_Y: f32 = space::MD;
    pub const SKELETON_GAP: f32 = space::LG;
    pub const SKELETON_BAR: f32 = 14.0;

    /// The card list: `gap-2 p-2` around `rounded-lg border p-3` cards whose
    /// head row is `gap-2.5 items-start` and whose `<dl>` is
    /// `mt-2 gap-x-3 gap-y-1`. The overflow control is a 44 DIP touch target
    /// pulled 4 DIP up (`-mt-1`), which is what sets the head row's height.
    pub const CARD_LIST_PAD: f32 = space::SM;
    pub const CARD_GAP: f32 = space::SM;
    pub const CARD_PAD: f32 = space::MD;
    pub const CARD_HEAD_GAP: f32 = 10.0;
    pub const CARD_TOUCH: f32 = 44.0;
    pub const CARD_TOUCH_RISE: f32 = space::XS;
    pub const CARD_HEAD: f32 = CARD_TOUCH - CARD_TOUCH_RISE;
    pub const CARD_DL_TOP: f32 = space::SM;
    pub const CARD_DL_GAP_X: f32 = space::MD;
    pub const CARD_DL_GAP_Y: f32 = space::XS;
    /// The line box of a `--kb-text-meta` node. **No source**: the web never
    /// declares one — it sets `fontSize` inline and inherits the line height.
    /// 16 is this family's decision (the 12/16 pair Tailwind's own `text-xs`
    /// ships), written here rather than hidden as a literal in a paint body.
    pub const META_LINE: f32 = 16.0;
    pub const CARD_BORDER: f32 = 1.0;

    /// The empty state, from `EmptyState.tsx`: `gap-3 px-6 py-12` around a
    /// `h-14 w-14` medallion holding a 24 DIP glyph, then a heading title, a
    /// body description `mt-1` under it, and a `size="sm"` action `mt-1` under
    /// that.
    pub const EMPTY_GAP: f32 = space::MD;
    pub const EMPTY_PAD_X: f32 = space::XL;
    pub const EMPTY_PAD_Y: f32 = 48.0;
    pub const MEDALLION: f32 = 56.0;
    pub const MEDALLION_GLYPH: f32 = 24.0;
    pub const EMPTY_TEXT_GAP: f32 = space::XS;
    /// `max-w-sm` on the text block.
    pub const EMPTY_TEXT_MAX: f32 = 384.0;

    /// Column resizing, from `useColumnResize.ts` and the handle's classes in
    /// `DataTable.tsx`: `MIN_WIDTH = 56` (« below this a column is a sliver
    /// that can hold nothing »), a `w-2 translate-x-1/2` grab strip centred on
    /// the column's right edge, whose visible rule is `after:w-px` inset
    /// `after:inset-y-1`, and a keyboard nudge of 8 (24 with Shift).
    pub const RESIZE_MIN: f32 = 56.0;
    pub const RESIZE_GRIP: f32 = space::SM;
    pub const RESIZE_RULE: f32 = 1.0;
    pub const RESIZE_RULE_INSET: f32 = space::XS;
    pub const RESIZE_STEP: f32 = 8.0;
    pub const RESIZE_STEP_BIG: f32 = 24.0;

    /// The sort button around a sortable header's label:
    /// `rounded-sm px-1 py-0.5 -mx-1` — what its focus ring outlines.
    pub const SORT_BUTTON_PAD_X: f32 = space::XS;
    pub const SORT_BUTTON_PAD_Y: f32 = space::XXS;

    /// `focus-visible:ring-2 ring-primary` — every focusable part of the
    /// table (header buttons, chooser, pagination, bulk bar) wears it.
    pub const FOCUS_RING: f32 = 2.0;

    /// The popups: every menu opens `top: r.bottom + 4` under its trigger; the
    /// row menu is pulled left by the 200 DIP it expects to need
    /// (`left: Math.max(8, r.right - 200)`), the column chooser declares
    /// `minWidth={220}`, and `MenuDropdown` keeps 8 DIP off the viewport edge.
    pub const MENU_GAP: f32 = space::XS;
    pub const MENU_EDGE: f32 = space::SM;
    pub const ROW_MENU_REACH: f32 = 200.0;
    pub const COLUMNS_MENU_MIN: f32 = 220.0;
    /// The menus' `SHADOW_MENU` spills about 7 DIP around the panel; the popup
    /// window is grown by this much on every side so none of it is clipped.
    pub const MENU_SHADOW: f32 = 10.0;

    /// Added to a MEASURED label before it becomes the width of its own box.
    /// DirectWrite lays the run out again at draw time and a fraction of a
    /// DIP of difference is enough to ellipsise the last letter — the
    /// « Lignes par pa… » defect. Not a design length: a rounding guard.
    pub const TEXT_SLACK: f32 = 1.0;
}

/// How many bulk actions stay inline before the rest fold into the overflow —
/// `INLINE_BULK_DESKTOP` / `INLINE_BULK_COMPACT` in `Toolbar.tsx`.
pub const INLINE_BULK_DESKTOP: usize = 3;
pub const INLINE_BULK_COMPACT: usize = 1;

/// The same colour at a lower alpha — how this design system dims. Same rule as
/// `buttons::fade` and [`kubuno_drive_desktop_app_controls::switch::fade`]; it is duplicated
/// rather than imported because both of those are private to their family.
fn fade(c: D2D1_COLOR_F, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: c.a * alpha, ..c }
}

// =============================================================================
//  Layout mode — the cards / table decision
// =============================================================================

/// What a [`DataTable`] is currently laid out as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Header, rows, columns.
    Table,
    /// One card per row: the primary column titles it, the rest become
    /// label/value pairs. A *different hierarchy*, not a squeezed table.
    Cards,
}

/// What the caller asked for — `layout` in `DataTableProps`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    /// Follow the container width.
    #[default]
    Auto,
    /// Always a table, however narrow the box.
    Table,
    /// Always cards, however wide.
    Cards,
}

/// **The layout decision, as a pure function of the width.**
///
/// `DataTable.tsx`:
/// `layout === 'cards' || (layout === 'auto' && width !== null && width < cardsBelow)`.
/// Two things follow from that line and both matter:
///
/// * the threshold is **strict** — a container exactly `cards_below` wide is
///   still a table;
/// * a container that genuinely measures 0 (a collapsed panel, a clipped
///   accordion) is honoured, not treated as wide. There is no « unmeasured »
///   state on this side: a caller always has a rectangle.
///
/// It is a free function, and the [`Mode`] it returns is what every painter and
/// hit test reads, so the switch is never an `if` buried in a paint body.
pub fn layout_mode(layout: Layout, width: f32, cards_below: f32) -> Mode {
    match layout {
        Layout::Table => Mode::Table,
        Layout::Cards => Mode::Cards,
        Layout::Auto => {
            if width < cards_below {
                Mode::Cards
            } else {
                Mode::Table
            }
        }
    }
}

// =============================================================================
//  Columns — the web's per-column booleans, on the replica's own payload
// =============================================================================

/// The flags `DataTableColumn` carries and `ColumnHeader` has no field for.
///
/// They ride in [`ColumnHeader::tag`], the toolkit's own general-purpose
/// per-column payload, as a space-separated set — the same trick
/// [`crate::lists::DANGER_TAG`] uses for a menu row, and for the same reason:
/// a second, parallel list of column options here would be a second source of
/// truth for the columns.
pub mod flags {
    /// `sortValue` is set: the header is a sort button.
    pub const SORTABLE: &str = "sortable";
    /// `primary`: the column that titles the row in the card layout.
    pub const PRIMARY: &str = "primary";
    /// `hideOnCards`: skip this column in the card layout.
    pub const HIDE_ON_CARDS: &str = "hide-on-cards";
    /// `required`: the column that identifies the row — it cannot be hidden.
    pub const REQUIRED: &str = "required";
    /// `ReadOnly` (WinForms `DataGridViewColumn.ReadOnly`): the cells of this column are never
    /// edited in place, even in an [`super::DataTable::editable`] table.
    pub const READ_ONLY: &str = "read-only";
}

/// A column: an id (the replica's `Name`, which *is* .NET's identity key for a
/// `ColumnHeader`), a header label, and a width.
pub fn column(id: &str, header: &str, width: i32) -> ColumnHeader {
    let mut c = ColumnHeader::new(header, width);
    c.name = id.to_string();
    c
}

/// Adds one of [`flags`] to a column.
pub fn with_flag(mut col: ColumnHeader, flag: &str) -> ColumnHeader {
    if !has_flag(&col, flag) {
        let tag = col.tag.take().unwrap_or_default();
        col.tag =
            Some(if tag.is_empty() { flag.to_string() } else { format!("{tag} {flag}") });
    }
    col
}

/// Sets a column's alignment (`align` in `DataTableColumn`).
pub fn aligned(mut col: ColumnHeader, align: HorizontalAlignment) -> ColumnHeader {
    col.text_align = align;
    col
}

/// Whether a column carries one of [`flags`].
pub fn has_flag(col: &ColumnHeader, flag: &str) -> bool {
    col.tag.as_deref().is_some_and(|t| t.split(' ').any(|f| f == flag))
}

/// The plain-text name of a column — `columnLabel` in `helpers.ts`: the header
/// text, falling back to the id when there is none.
pub fn column_label(col: &ColumnHeader) -> &str {
    if col.text.is_empty() {
        &col.name
    } else {
        &col.text
    }
}

// =============================================================================
//  Selection
// =============================================================================

/// The three states of the « select all » box.
///
/// `DataTable.tsx` derives them over the **current page only**, and says why:
/// « silently selecting rows the user cannot see (and may not have loaded)
/// before a bulk delete is a trap ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectAll {
    /// No row of the page is selected — and an empty page is here too, because
    /// `allOnPage` requires `pageKeys.length > 0`.
    None,
    /// Some but not all: `someOnPage && !allOnPage`, the indeterminate box.
    Some,
    /// Every row of the page.
    All,
}

impl SelectAll {
    /// The replica's own tri-state, so the box is painted by
    /// [`crate::buttons::CheckBox`] rather than by a second painter.
    pub fn check_state(self) -> CheckState {
        match self {
            SelectAll::None => CheckState::Unchecked,
            SelectAll::Some => CheckState::Indeterminate,
            SelectAll::All => CheckState::Checked,
        }
    }
}

/// One entry of the bulk-action bar (`DataTableBulkAction`).
#[derive(Debug, Clone)]
pub struct BulkAction {
    pub id: String,
    pub label: String,
    pub icon: Option<&'static str>,
    pub danger: bool,
}

impl BulkAction {
    pub fn new(id: &str, label: &str) -> Self {
        Self { id: id.to_string(), label: label.to_string(), icon: None, danger: false }
    }

    pub fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }

    pub fn danger(mut self, on: bool) -> Self {
        self.danger = on;
        self
    }

    /// The button that paints it: `secondary`, or `danger` when it destroys.
    fn button(&self) -> Button {
        let mut b = Button::new(&self.label)
            .variant(if self.danger { Variant::Danger } else { Variant::Secondary })
            .size(ButtonSize::Sm);
        if let Some(icon) = self.icon {
            b = b.icon(icon);
        }
        b
    }
}

// =============================================================================
//  Chrome geometry — pure, so it can be tested and hit-tested from one place
// =============================================================================

/// A piece of the table's chrome the pointer can be over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chrome {
    /// The « × » that drops the selection.
    ClearSelection,
    /// An inline bulk action, by index into [`DataTable::bulk_actions`].
    BulkAction(usize),
    /// The « … » that holds the bulk actions that did not fit.
    BulkOverflow,
    /// The column chooser.
    Columns,
    /// A column header (a sort button when the column is sortable).
    Header(usize),
    /// The select-all box.
    SelectAll,
    /// A row's own check box, by item index.
    RowCheck(usize),
    /// A row's overflow button, by item index.
    RowMenu(usize),
    /// A page-size option, by value.
    PageSize(usize),
    FirstPage,
    PrevPage,
    NextPage,
    LastPage,
    /// The resize grip on the right edge of a visible column's header
    /// (`role="separator"` in `DataTable.tsx`). Outranks [`Chrome::Header`]:
    /// « a resize must not also sort the column ».
    ResizeHandle(usize),
    /// A scroll bar's rail — vertical, or horizontal when the table is wider
    /// than its box.
    ScrollBar { horizontal: bool },
}

/// What holds the keyboard focus inside a [`DataTable`].
///
/// The web table is a sequence of native tab stops (the chooser, the bulk
/// buttons, the select-all box, each sort button, each resize separator, the
/// pagination buttons) and has **no** row focus at all. Rows here are ONE
/// extra stop, [`FocusPart::Rows`], walked with the arrows — the ARIA grid
/// pattern — so a keyboard user can reach, select and act on a row without
/// a tab stop per check box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPart {
    /// A focusable piece of chrome (see [`DataTable::focus_stops`]).
    Chrome(Chrome),
    /// The body: the cursor row, moved with ↑ ↓ PgUp PgDn Home End.
    Rows,
}

/// One of the table's floating menus, each painted in a `host::popup`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableMenu {
    /// The row's overflow menu — [`DataTable::row_menu_actions`].
    Row(usize),
    /// The right-click copy menu: the cell under the pointer (a **visible**
    /// column index), its row, its column.
    Copy { row: usize, column: usize },
    /// The bulk actions that did not fit inline.
    BulkOverflow,
    /// The column chooser.
    Columns,
}

/// What choosing an entry of a [`TableMenu`] asks for — see
/// [`DataTable::menu_command`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableCommand {
    /// A row action, by its [`BulkAction::id`], on an item index.
    RowAction { row: usize, id: String },
    /// A bulk action, by id, on the current selection.
    BulkAction(String),
    /// Text to put on the clipboard (`copy.ts`).
    Copy(String),
    /// Shows or hides a column, by id. [`DataTable::apply_command`] applies it.
    ToggleColumn(String),
}

/// What a click or a key did that the table cannot finish on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableEvent {
    /// Open this menu (the caller owns the popup).
    OpenMenu(TableMenu),
    /// An inline bulk action, by id.
    BulkAction(String),
    /// Enter on the cursor row — the web's `onRowClick`.
    RowActivated(usize),
}

/// A column being resized by drag — `drag.current` in `useColumnResize.ts`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColumnDrag {
    /// The **visible** column.
    pub column: usize,
    pub start_x: f32,
    pub start_width: f32,
}

/// A scroll thumb being dragged: which bar, and where in the thumb it was
/// grabbed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollDrag {
    pub horizontal: bool,
    pub grab: f32,
}

/// One data cell, as handed to a [`CellPainter`].
#[derive(Clone, Copy)]
pub struct Cell<'a> {
    /// The cell's text — the sub-item, what the table would draw.
    pub text: &'a str,
    /// The item index.
    pub row: usize,
    /// The **model** column (an index into `columns`, what
    /// [`ListViewItem::cell`] reads) — stable when a column is hidden.
    pub column: usize,
    /// The content box: the cell minus its `px-4` padding, full row height.
    pub rect: Rect,
    pub selected: bool,
    pub hot: bool,
    /// The table is painted disabled.
    pub disabled: bool,
}

/// The web column's `cell: (row) => ReactNode` — a hook that paints one cell
/// itself (a badge, an avatar and a name…). Return `true` when it painted the
/// cell, `false` to let the table draw the text as usual. It paints inside
/// the table's clip, so it cannot spill out of the body.
pub type CellPainter = Box<dyn Fn(&dyn Canvas, &Cell<'_>) -> bool>;

/// Where a menu of `want` size opens against the monitor's work area `area`
/// — `MenuDropdown`'s rule: at `at`, pulled back inside the viewport by
/// [`grid::MENU_EDGE`], and flipped to END at `flip_bottom` (the trigger's
/// top, or the pointer) when it would run off the bottom. Pure.
pub fn place_menu(at: (f32, f32), flip_bottom: f32, want: Size, area: Rect) -> Rect {
    let edge = grid::MENU_EDGE;
    let (ax, ay) = at;
    let x = if ax + want.width > area.right - edge {
        (area.right - edge - want.width).max(area.left + edge)
    } else {
        ax.max(area.left + edge)
    };
    let y = if ay + want.height > area.bottom - edge {
        (flip_bottom - want.height).max(area.top + edge)
    } else {
        ay.max(area.top + edge)
    };
    Rect::new(x, y, x + want.width, y + want.height)
}

/// Every rectangle the table is made of, resolved once from the bounds it was
/// given — so the painters and the hit tests can never disagree about where
/// something is.
/// (`Rect` carries neither `Debug` nor `PartialEq`, so neither does this.)
#[derive(Clone, Copy)]
pub struct TableLayout {
    pub mode: Mode,
    /// The toolbar band (idle or selection face). Empty when there is nothing
    /// to put in it.
    pub toolbar: Rect,
    /// The bordered box: header + body + footer.
    pub frame: Rect,
    /// The column header band. Empty in cards mode, while loading, and on an
    /// empty body.
    pub header: Rect,
    /// Where rows, cards, the skeleton or the empty state are painted.
    pub body: Rect,
    /// The pagination band. Empty when the table is not paginated.
    pub footer: Rect,
    /// The band reserved under the rows for the horizontal scroll bar when the
    /// table is wider than its box (the web's `overflow-x-auto` scroller puts
    /// its bar below the last row, never over it). Zero height, at
    /// `body.bottom`, otherwise.
    pub hscroll: Rect,
}

/// The bulk bar's contents, laid out — see [`bulk_bar`].
#[derive(Clone)]
pub struct BulkBar {
    pub close: Rect,
    /// « N en sélection » — what is left between the close button and the
    /// actions, which is the web's `flex-1 truncate`.
    pub label: Rect,
    pub actions: Vec<Rect>,
    pub overflow: Option<Rect>,
}

/// Lays the bulk-action bar out inside `bar`.
///
/// Pure on purpose: `action_widths` are the measured button widths, so the
/// geometry can be asserted without a device. The bar is
/// `px-2.5 py-2 gap-2 items-center`, the close button hugs the left edge, the
/// actions and the overflow hug the right one, and the label takes what is
/// between them.
pub fn bulk_bar(bar: Rect, action_widths: &[f32], overflow: bool) -> BulkBar {
    let cy = (bar.top + bar.bottom) / 2.0;
    let half = |h: f32| (cy - h / 2.0, cy + h / 2.0);

    let (ct, cb) = half(grid::CLOSE_BUTTON);
    let close = Rect::new(
        bar.left + grid::BAR_PAD_X,
        ct,
        bar.left + grid::BAR_PAD_X + grid::CLOSE_BUTTON,
        cb,
    );

    let mut right = bar.right - grid::BAR_PAD_X;
    let overflow_rect = if overflow {
        let (t, b) = half(grid::OVERFLOW_BUTTON);
        let r = Rect::new(right - grid::OVERFLOW_BUTTON, t, right, b);
        right = r.left - grid::BAR_GAP;
        Some(r)
    } else {
        None
    };

    // Right to left, so the last action sits against the right edge; then
    // reversed, so the caller reads them in declaration order.
    let (t, b) = half(height::BUTTON_SM);
    let mut actions: Vec<Rect> = Vec::with_capacity(action_widths.len());
    for w in action_widths.iter().rev() {
        let r = Rect::new(right - w, t, right, b);
        right = r.left - grid::BAR_GAP;
        actions.push(r);
    }
    actions.reverse();

    // The label is the web's `flex-1 truncate`: it takes what is left, stopping
    // `gap-2` before whatever comes next — the first inline action, else the
    // overflow, else the bar's own padding.
    let label_right = match actions.first().map(|r| r.left).or(overflow_rect.map(|r| r.left)) {
        Some(x) => x - grid::BAR_GAP,
        None => bar.right - grid::BAR_PAD_X,
    };
    let label = Rect::new(
        close.right + grid::BAR_GAP,
        bar.top,
        label_right.max(close.right + grid::BAR_GAP),
        bar.bottom,
    );

    BulkBar { close, label, actions, overflow: overflow_rect }
}

/// How many of the first `action_widths` stay inline so that a label of
/// `label_width` keeps its whole width in the bar (the rest fold into the
/// overflow). `total` is the full action count, so the overflow button is
/// reserved as soon as one action is folded.
///
/// `Toolbar.tsx` fixes the count per mode (3 / 1) and lets the bar
/// `flex-wrap` when the container is too narrow; the one-line band here folds
/// instead, so « N en sélection » is never cut down to « 1… ».
pub fn fit_bulk(bar: Rect, action_widths: &[f32], total: usize, label_width: f32) -> usize {
    let most = action_widths.len().min(total);
    for n in (0..=most).rev() {
        let b = bulk_bar(bar, &action_widths[..n], total > n);
        if b.label.right - b.label.left >= label_width {
            return n;
        }
    }
    0
}

/// The pagination bar's navigation cluster: `[«] [‹] p / n [›] [»]`,
/// right-aligned in `footer`.
///
/// `label_width` is the measured width of « p / n ». `compact` drops the
/// first/last jumps, exactly as `Pagination.tsx` does in a narrow container.
/// Pure, so the arithmetic is testable without a device.
pub fn nav_cluster(footer: Rect, label_width: f32, compact: bool) -> Vec<(Chrome, Rect)> {
    let btn = grid::ICON_BUTTON;
    let gap = grid::NAV_GAP;
    let label = grid::PAGE_LABEL_PAD * 2.0 + label_width;
    let jumps = if compact { 0.0 } else { 2.0 };
    let buttons = 2.0 + jumps;
    // `buttons + 1` items, so `buttons` gaps between them.
    let total = buttons * btn + label + buttons * gap;

    let cy = (footer.top + footer.bottom) / 2.0;
    let (top, bottom) = (cy - btn / 2.0, cy + btn / 2.0);
    let mut x = footer.right - grid::FOOTER_PAD_X - total;
    let mut out = Vec::new();

    if !compact {
        out.push((Chrome::FirstPage, Rect::new(x, top, x + btn, bottom)));
        x += btn + gap;
    }
    out.push((Chrome::PrevPage, Rect::new(x, top, x + btn, bottom)));
    x += btn + gap + label + gap;
    out.push((Chrome::NextPage, Rect::new(x, top, x + btn, bottom)));
    x += btn + gap;
    if !compact {
        out.push((Chrome::LastPage, Rect::new(x, top, x + btn, bottom)));
    }
    out
}

/// Where the « p / n » label sits, between the two single chevrons.
pub fn nav_label_rect(footer: Rect, label_width: f32, compact: bool) -> Rect {
    let cluster = nav_cluster(footer, label_width, compact);
    let prev = cluster.iter().find(|(c, _)| *c == Chrome::PrevPage).map(|(_, r)| *r);
    let next = cluster.iter().find(|(c, _)| *c == Chrome::NextPage).map(|(_, r)| *r);
    match (prev, next) {
        (Some(p), Some(n)) => Rect::new(p.right, footer.top, n.left, footer.bottom),
        _ => Rect::new(footer.right, footer.top, footer.right, footer.bottom),
    }
}

/// The page-size radio group, left-aligned in `footer` after its label.
///
/// `pill_widths` are the measured widths of the option texts. Returns the group
/// box (the `bg-surface-2 p-0.5` pill) and one rectangle per option.
pub fn size_group(left: f32, cy: f32, pill_widths: &[f32]) -> (Rect, Vec<Rect>) {
    let pad = grid::SIZE_GROUP_PAD;
    let h = grid::SIZE_PILL_H;
    let mut pills = Vec::with_capacity(pill_widths.len());
    let mut x = left + pad;
    for (i, w) in pill_widths.iter().enumerate() {
        if i > 0 {
            x += pad;
        }
        let pw = w + grid::SIZE_PILL_PAD_X * 2.0;
        pills.push(Rect::new(x, cy - h / 2.0, x + pw, cy + h / 2.0));
        x += pw;
    }
    let right = x + pad;
    let gh = h + pad * 2.0;
    (Rect::new(left, cy - gh / 2.0, right, cy + gh / 2.0), pills)
}

// =============================================================================
//  Wording
// =============================================================================

/// The words one empty state uses. Defaults are the product's own French
/// strings (`ui.dt_*` in `locales/fr/core.json`) rather than invented ones.
#[derive(Debug, Clone)]
pub struct EmptyWording {
    pub title: String,
    pub description: String,
    /// The way out. `None` draws no button — which is what an unretryable
    /// error or an unfiltered empty collection gets.
    pub action: Option<String>,
}

impl EmptyWording {
    pub fn new(title: &str, description: &str, action: Option<&str>) -> Self {
        Self {
            title: title.to_string(),
            description: description.to_string(),
            action: action.map(str::to_string),
        }
    }
}

/// Every label the chrome paints, so nothing user-facing is a literal in a
/// paint body and a caller can translate the table.
#[derive(Debug, Clone)]
pub struct Wording {
    /// « en sélection », after the count.
    pub selected: String,
    /// « Colonnes » on the chooser.
    pub columns: String,
    /// « Lignes par page ».
    pub rows_per_page: String,
    pub empty: EmptyWording,
    pub no_results: EmptyWording,
    pub error: EmptyWording,
    /// `ui.dt_copy_cell` / `ui.dt_copy_row` — the copy menu.
    pub copy_cell: String,
    pub copy_row: String,
    /// `ui.dt_copy_column`; `{name}` is replaced by the column's label.
    pub copy_column: String,
}

impl Default for Wording {
    fn default() -> Self {
        Self {
            selected: "en sélection".into(),
            columns: "Colonnes".into(),
            rows_per_page: "Lignes par page".into(),
            copy_cell: "Copier cette cellule".into(),
            copy_row: "Copier cette ligne".into(),
            copy_column: "Copier la colonne « {name} »".into(),
            empty: EmptyWording::new(
                "Rien pour l’instant",
                "Les éléments que vous ajouterez apparaîtront dans ce tableau.",
                None,
            ),
            no_results: EmptyWording::new(
                "Aucun résultat",
                "Aucune ligne ne correspond aux filtres actifs.",
                Some("Effacer les filtres"),
            ),
            error: EmptyWording::new(
                "Chargement impossible",
                "La requête a échoué. Vérifiez la connexion, puis réessayez.",
                Some("Réessayer"),
            ),
        }
    }
}

/// Which of the three the body is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmptyKind {
    Error,
    NoResults,
    FirstUse,
}

// =============================================================================
//  DataTable
// =============================================================================

/// The design system's one table — a [`ListView`] with the DataTable's chrome
/// and the DataTable's ink.
///
/// ```text
///   DataTable
///     ├── list: kubuno_desktop_ui::views::ListView       ← columns, items, selection,
///     │     └── kubuno_desktop_controls::views::ListView   sort, row geometry,
///     │                                            visible_range
///     ├── title / loading / error / filtered     ← the body's four states
///     ├── page / page_size / total_rows          ← the pagination
///     ├── selectable / bulk_actions              ← the selection chrome
///     ├── layout / cards_below                   ← the narrow-container switch
///     └── paint(&dyn Canvas, …)                  ← DataTable pixels
/// ```
pub struct DataTable {
    list: ListView,

    /// The `<h3>` on the idle toolbar. Empty hides it.
    pub title: String,
    /// Shows the column chooser button.
    pub configurable_columns: bool,
    /// Column ids (a [`ColumnHeader::name`]) the chooser has turned off.
    pub hidden_columns: Vec<String>,

    /// The skeleton replaces the body.
    pub loading: bool,
    /// How many skeleton rows — `skeletonRows`, default 5.
    pub skeleton_rows: usize,
    /// Non-`None` switches the body to the error state; the string replaces the
    /// default description, as the web's `error: ReactNode` does.
    pub error: Option<String>,
    /// A search or filter is active — picks « no result » over « nothing yet ».
    pub filtered: bool,

    /// 0-based, like the whole web component. Read it through
    /// [`DataTable::page_index`], which applies the snap-back.
    pub page: usize,
    /// `0` disables pagination entirely.
    pub page_size: usize,
    pub page_size_options: Vec<usize>,
    /// The caller paginates server-side: the rows it hands over ARE the page.
    pub manual_pagination: bool,
    /// The total row count when [`DataTable::manual_pagination`] is on.
    pub total_rows: Option<usize>,
    /// The caller sorts: [`DataTable::toggle_sort`] only moves the indicator.
    pub manual_sort: bool,

    /// Draws the selection column.
    pub selectable: bool,
    pub bulk_actions: Vec<BulkAction>,
    /// Draws the per-row overflow column.
    pub row_actions: bool,

    pub layout: Layout,
    /// Container width under which `Auto` switches to cards — `cardsBelow`.
    pub cards_below: f32,
    /// The width the table wants before it starts overflowing its box.
    pub min_table_width: f32,

    /// The chrome element under the pointer, for the caller to feed back.
    pub hot_chrome: Option<Chrome>,

    pub copy: Wording,

    /// The entries of the row overflow menu (`rowActions`). Only read when
    /// [`DataTable::row_actions`] is on.
    pub row_menu_actions: Vec<BulkAction>,
    /// `resizableColumns` (default on): the header carries resize grips.
    pub resizable_columns: bool,
    /// The column being resized, while a drag is in progress.
    pub resize: Option<ColumnDrag>,
    /// The scroll thumb being dragged, if any.
    pub scroll_drag: Option<ScrollDrag>,
    /// The horizontal scroll of a table wider than its box — the web's
    /// `overflow-x-auto` scroller. Clamped wherever it is read.
    pub scroll_x: f32,
    /// The keyboard cursor row (an item index) — see [`FocusPart::Rows`].
    pub cursor: Option<usize>,
    /// What holds the focus inside the table, fed back by the caller from its
    /// focus manager, and whether the ring shows (`:focus-visible`).
    pub focus_part: Option<FocusPart>,
    pub focus_visible: bool,
    /// Per-cell painter — the web column's `cell` renderer.
    pub cell_painter: Option<CellPainter>,
    /// Owner-drawn cells (`DataGridView.CellPainting`): each cell is first offered to the
    /// owner-draw handler lent for the paint (`crate::graphics::owner_draw`), as a `DrawItem`
    /// with the row as `index` and the column as `sub_index`.
    pub owner_draw_cells: bool,
    /// Cells are edited in place (WinForms `DataGridView.ReadOnly = false`, see the `edit`
    /// module): F2, typing or a double-click begin an edit of the current cell. Off by default.
    pub editable: bool,
    /// The **visible** column of the current cell; its row is [`DataTable::cursor`].
    pub current_column: usize,
    /// The cells that show an error glyph (WinForms `DataGridViewCell.ErrorText`).
    pub cell_errors: Vec<CellError>,
    /// The cell being edited.
    editor: Option<CellEditor>,

    /// Shift-range anchor, and the selection as it was when the range began,
    /// so shrinking a range deselects what it had added and nothing else.
    anchor: Option<usize>,
    range_base: Option<Vec<usize>>,
    /// The widths the columns were DECLARED with, by id, recorded the first
    /// time one is resized — what a double-click on its grip goes back to.
    declared_widths: Vec<(String, i32)>,
    /// How many bulk actions the last laid-out bar kept inline ([`fit_bulk`]),
    /// so the overflow menu, the hit tests and the focus stops agree with it.
    bulk_fit: std::cell::Cell<Option<usize>>,
}

impl Default for DataTable {
    fn default() -> Self {
        let mut list = ListView::new();
        list.view = crate::views::View::Details;
        list.multi_select = true;
        list.full_row_select = true;
        // A data table's rows carry no icon and no `ImageIndex`; the file grid's
        // folder rule would tint half of them.
        list.container_icon = None;
        Self {
            list,
            title: String::new(),
            configurable_columns: false,
            hidden_columns: Vec::new(),
            loading: false,
            skeleton_rows: 5,
            error: None,
            filtered: false,
            page: 0,
            page_size: 25,
            page_size_options: vec![10, 25, 50, 100],
            manual_pagination: false,
            total_rows: None,
            manual_sort: false,
            selectable: false,
            bulk_actions: Vec::new(),
            row_actions: false,
            layout: Layout::default(),
            cards_below: 700.0,
            min_table_width: 640.0,
            hot_chrome: None,
            copy: Wording::default(),
            row_menu_actions: Vec::new(),
            resizable_columns: true,
            resize: None,
            scroll_drag: None,
            scroll_x: 0.0,
            cursor: None,
            focus_part: None,
            focus_visible: false,
            cell_painter: None,
            owner_draw_cells: false,
            editable: false,
            current_column: 0,
            cell_errors: Vec::new(),
            editor: None,
            anchor: None,
            range_base: None,
            declared_widths: Vec::new(),
            bulk_fit: std::cell::Cell::new(None),
        }
    }
}

impl DataTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// The table model underneath — columns, items, selection, sort.
    pub fn list(&self) -> &ListView {
        &self.list
    }

    pub fn list_mut(&mut self) -> &mut ListView {
        &mut self.list
    }

    // ── Columns ──────────────────────────────────────────────────────────

    /// The columns actually laid out — `visibleColumns` in the web: every
    /// column whose id is not in [`DataTable::hidden_columns`].
    pub fn visible_columns(&self) -> Vec<ColumnHeader> {
        self.list
            .columns
            .iter()
            .filter(|c| !self.hidden_columns.iter().any(|h| h == &c.name))
            .cloned()
            .collect()
    }

    /// Whether column `index` (into the **visible** columns) can be sorted.
    fn sortable(cols: &[ColumnHeader], index: usize) -> bool {
        cols.get(index).is_some_and(|c| has_flag(c, flags::SORTABLE))
    }

    /// The selection column's width — 0 when the table is not selectable.
    pub fn select_width(&self) -> f32 {
        if self.selectable {
            grid::SELECT_COL
        } else {
            0.0
        }
    }

    /// The actions column's width — 0 when there are no row actions.
    pub fn actions_width(&self) -> f32 {
        if self.row_actions {
            grid::ACTIONS_COL
        } else {
            0.0
        }
    }

    /// The width the table wants: the two fixed columns plus the declared
    /// widths of the visible ones.
    pub fn table_width(&self) -> f32 {
        let cols: f32 =
            self.visible_columns().iter().map(|c| c.width.max(0) as f32).sum();
        self.select_width() + cols + self.actions_width()
    }

    /// Whether the content is wider than the box it was given — what the web's
    /// `overflow-x-auto` scroller exists for, and what this port clips instead.
    pub fn overflows(&self, bounds: Rect) -> bool {
        self.table_width().max(self.min_table_width) > bounds.right - bounds.left
    }

    /// The left x of each visible column, plus one entry at the right edge of
    /// the last one — [`kc::DetailsGeometry::column_x_offsets`], the replica's
    /// own arithmetic, over the filtered column list and starting after the
    /// selection column.
    pub fn column_x_offsets(&self, bounds: Rect) -> Vec<f32> {
        self.offsets_of(bounds, &self.visible_columns())
    }

    /// The width the table's content spans inside a box `view` wide: the box
    /// itself (`w-full`), or more when the columns — or `minTableWidth` — ask
    /// for more, in which case it scrolls sideways inside the box.
    pub fn content_width(&self, view: f32) -> f32 {
        view.max(self.table_width().max(self.min_table_width))
    }

    /// How far the content can scroll sideways inside `frame`.
    pub fn max_scroll_x(&self, frame: Rect) -> f32 {
        let view = frame.right - frame.left;
        (self.content_width(view) - view).max(0.0)
    }

    /// [`DataTable::scroll_x`], clamped to what `frame` allows.
    fn sx(&self, frame: Rect) -> f32 {
        self.scroll_x.clamp(0.0, self.max_scroll_x(frame))
    }

    /// The content's horizontal span `(left, right)` inside `frame`, after the
    /// horizontal scroll: what the header, the rows and every column offset
    /// are laid out on, and what the body's clip cuts back to the box.
    pub fn content_span(&self, frame: Rect) -> (f32, f32) {
        let left = frame.left - self.sx(frame);
        (left, left + self.content_width(frame.right - frame.left))
    }

    fn offsets_of(&self, bounds: Rect, cols: &[ColumnHeader]) -> Vec<f32> {
        let (left, right) = self.content_span(bounds);
        let cells = Rect::new(
            left + self.select_width(),
            bounds.top,
            (right - self.actions_width()).max(left + self.select_width()),
            bounds.bottom,
        );
        kc::DetailsGeometry {
            content: cells,
            header_height: 0.0,
            row_height: self.list.row_height(),
        }
        .column_x_offsets(cols)
    }

    /// The visible column `x` falls in — header band and cells agree, because
    /// both read this.
    pub fn column_at(&self, bounds: Rect, x: f32) -> Option<usize> {
        let xs = self.column_x_offsets(bounds);
        xs.windows(2).position(|w| x >= w[0] && x < w[1])
    }

    // ── Pagination ───────────────────────────────────────────────────────

    /// Whether the footer exists at all — `paginated = pageSize > 0`.
    pub fn paginated(&self) -> bool {
        self.page_size > 0
    }

    /// The number of rows the pagination reasons about: the caller's
    /// `totalRows` when it paginates server-side, else the item count.
    pub fn total(&self) -> usize {
        if self.manual_pagination {
            self.total_rows.unwrap_or_else(|| self.list.item_count())
        } else {
            self.list.item_count()
        }
    }

    /// `Math.max(1, Math.ceil(total / pageSize))` — never zero, so a page
    /// counter always reads « 1 / 1 » rather than « 1 / 0 ».
    pub fn page_count(&self) -> usize {
        if !self.paginated() {
            return 1;
        }
        self.total().div_ceil(self.page_size).max(1)
    }

    /// The page actually shown.
    ///
    /// The web applies a snap-back in an effect — « a filter that shrinks the
    /// result set can strand the user on a page that no longer exists » — and
    /// it only fires past page 0. The same rule, as a projection of the raw
    /// [`DataTable::page`] rather than a second stored value.
    pub fn page_index(&self) -> usize {
        if self.paginated() && self.page > 0 && self.page >= self.page_count() {
            self.page_count() - 1
        } else {
            self.page
        }
    }

    /// The item indices the current page shows.
    ///
    /// With pagination off, or when the caller paginates server-side (its rows
    /// already *are* the page), that is every row.
    pub fn page_range(&self) -> Range<usize> {
        let count = self.list.item_count();
        if !self.paginated() || self.manual_pagination {
            return 0..count;
        }
        let start = (self.page_index() * self.page_size).min(count);
        let end = (start + self.page_size).min(count);
        start..end
    }

    /// What the footer's counter reads: `(first, last, total)`, with `first`
    /// **1-based** — « 0-based in the API, 1-based in what the user reads ».
    pub fn page_counter(&self) -> (usize, usize, usize) {
        let total = self.total();
        if total == 0 {
            return (0, 0, 0);
        }
        let size = self.page_size.max(1);
        let first = self.page_index() * size + 1;
        let last = ((self.page_index() + 1) * size).min(total);
        (first, last, total)
    }

    /// Goes to a page, clamped to the last one.
    pub fn set_page(&mut self, page: usize) {
        self.page = page.min(self.page_count().saturating_sub(1));
    }

    /// Changes the page size and returns to the first page — `setPageSize`
    /// calls `setPage(0)`, because the row under the cursor is not on page 3
    /// any more once the page holds four times as many.
    pub fn set_page_size(&mut self, size: usize) {
        self.page_size = size;
        self.page = 0;
    }

    // ── Selection ────────────────────────────────────────────────────────

    /// The selected items — [`kc::ListView::selected_indices`], the replica's
    /// own derivation from the per-item flag.
    pub fn selected(&self) -> Vec<usize> {
        self.list.selected_indices()
    }

    /// Whether the toolbar shows its selection face:
    /// `selectedRows.length > 0 && !!bulkActions?.length`.
    pub fn selection_mode(&self) -> bool {
        !self.selected().is_empty() && !self.bulk_actions.is_empty()
    }

    /// The state of the « select all » box, **over the current page only**.
    pub fn select_all_state(&self) -> SelectAll {
        let page = self.page_range();
        if page.is_empty() {
            // `allOnPage` requires a non-empty page, and `someOnPage` is false:
            // an empty page reads as unchecked, not as « all of nothing ».
            return SelectAll::None;
        }
        let mut all = true;
        let mut some = false;
        for i in page {
            match self.list.items.get(i).map(|it| it.selected) {
                Some(true) => some = true,
                _ => all = false,
            }
        }
        match (all, some) {
            (true, _) => SelectAll::All,
            (false, true) => SelectAll::Some,
            (false, false) => SelectAll::None,
        }
    }

    /// What a click on the « select all » box does.
    ///
    /// `toggleAll` in the web: when the whole page is selected it *removes the
    /// page's keys* and leaves the rest of the selection alone; otherwise it
    /// *adds* them. So the indeterminate state selects the page — it does not
    /// clear it — and a row selected on another page survives either way.
    pub fn toggle_all(&mut self) {
        let select = self.select_all_state() != SelectAll::All;
        for i in self.page_range() {
            self.list.set_selected(i, select);
        }
    }

    /// Toggles one row's own box (`toggleRow`).
    pub fn toggle_row(&mut self, index: usize) {
        let now = self.list.items.get(index).map(|it| it.selected).unwrap_or(false);
        self.list.set_selected(index, !now);
    }

    /// Drops the selection — the « × » on the bulk bar.
    pub fn clear_selection(&mut self) {
        self.list.clear_selection();
    }

    // ── Sorting ──────────────────────────────────────────────────────────

    /// The sort cycle, on the **visible** column `index`.
    ///
    /// `nextSort` in `helpers.ts`: a different column starts ascending, an
    /// ascending column goes descending, and a descending one goes back to no
    /// sort at all. Three states, so a table can always be put back to its
    /// declared order — see the module docs on what « back » does and does not
    /// mean here.
    pub fn toggle_sort(&mut self, index: usize) {
        let cols = self.visible_columns();
        if !Self::sortable(&cols, index) {
            return;
        }
        let Some(model) = self.model_column(&cols, index) else {
            return;
        };
        if self.list.sort_column != model || self.list.sorting == SortOrder::None {
            self.list.sort_column = model;
            self.list.sorting = SortOrder::Ascending;
        } else if self.list.sorting == SortOrder::Ascending {
            self.list.sorting = SortOrder::Descending;
        } else {
            self.list.sorting = SortOrder::None;
        }
        if !self.manual_sort && self.list.sorting != SortOrder::None {
            self.list.sort();
        }
    }

    /// The index into `list.columns` a **visible** column stands for. They
    /// differ as soon as a column is hidden, and the sort state is stored
    /// against the model's index so hiding a column does not move the sort.
    fn model_column(&self, cols: &[ColumnHeader], visible: usize) -> Option<usize> {
        let id = &cols.get(visible)?.name;
        self.list.columns.iter().position(|c| &c.name == id)
    }

    /// Which **visible** column carries the sort indicator, if any is on
    /// screen.
    pub fn sorted_column(&self) -> Option<usize> {
        if self.list.sorting == SortOrder::None {
            return None;
        }
        let model = self.list.columns.get(self.list.sort_column)?;
        self.visible_columns().iter().position(|c| c.name == model.name)
    }

    // ── Body geometry ────────────────────────────────────────────────────

    /// Which state the body is in, if it is not showing rows.
    fn empty_kind(&self) -> Option<EmptyKind> {
        if self.loading {
            return None;
        }
        if self.error.is_some() {
            return Some(EmptyKind::Error);
        }
        if self.page_range().is_empty() {
            return Some(if self.filtered { EmptyKind::NoResults } else { EmptyKind::FirstUse });
        }
        None
    }

    /// Whether the body is showing one of the three empty states — `isEmpty`.
    pub fn is_empty(&self) -> bool {
        self.empty_kind().is_some()
    }

    /// The height the toolbar band needs — 0 when there is nothing in it.
    pub fn toolbar_height(&self) -> f32 {
        if self.selection_mode() {
            grid::BAR_HEIGHT
        } else if self.title.is_empty() && !self.configurable_columns {
            // « if (!title && !toolbar && !configurableColumns) return null ».
            0.0
        } else {
            grid::TOOLBAR_ROW
        }
    }

    /// The height the footer needs. The web renders it only when
    /// `paginated && !loading && !isEmpty && total > 0`.
    pub fn footer_height(&self) -> f32 {
        if self.paginated() && !self.loading && !self.is_empty() && self.total() > 0 {
            grid::FOOTER_HEIGHT
        } else {
            0.0
        }
    }

    /// The height of one skeleton row — `px-4 py-3` around the taller of a text
    /// bar and the check box placeholder.
    fn skeleton_row(&self) -> f32 {
        let content =
            if self.selectable { grid::SKELETON_BAR.max(control::CHECK_BOX) } else { grid::SKELETON_BAR };
        grid::SKELETON_PAD_Y * 2.0 + content
    }

    /// The height one card needs, given how many label/value pairs it carries.
    fn card_height(&self, pairs: usize) -> f32 {
        let dl = if pairs == 0 {
            0.0
        } else {
            grid::CARD_DL_TOP
                + pairs as f32 * grid::META_LINE
                + (pairs - 1) as f32 * grid::CARD_DL_GAP_Y
        };
        grid::CARD_PAD * 2.0 + grid::CARD_HEAD + dl
    }

    /// **The layout**, resolved once from `bounds`.
    pub fn layout_of(&self, bounds: Rect) -> TableLayout {
        let mode = layout_mode(self.layout, bounds.right - bounds.left, self.cards_below);

        let th = self.toolbar_height();
        let toolbar = Rect::new(bounds.left, bounds.top, bounds.right, bounds.top + th);
        let frame_top = if th > 0.0 { toolbar.bottom + grid::TOOLBAR_GAP } else { bounds.top };
        let frame = Rect::new(bounds.left, frame_top, bounds.right, bounds.bottom.max(frame_top));

        let fh = self.footer_height();
        let footer = Rect::new(frame.left, (frame.bottom - fh).max(frame.top), frame.right, frame.bottom);

        // The column header exists only when the body is showing rows in table
        // mode: a skeleton, an empty state and a card list all replace it. Its
        // height is the ROW's, not a constant — a `<th>` and a `<td>` are the
        // same box on the web, so a denser table gets a denser header too.
        let hh = if mode == Mode::Table && !self.loading && !self.is_empty() {
            self.list.row_height()
        } else {
            0.0
        };
        let header = Rect::new(frame.left, frame.top, frame.right, frame.top + hh);
        let rows_bottom = footer.top.max(header.bottom);
        // A table wider than its box reserves a band for the horizontal bar
        // under the rows, as long as that still leaves one row visible.
        let band = if hh > 0.0
            && self.max_scroll_x(frame) > 0.5
            && rows_bottom - header.bottom >= hh + sb::SCROLLBAR_SIZE
        {
            sb::SCROLLBAR_SIZE
        } else {
            0.0
        };
        let body = Rect::new(frame.left, header.bottom, frame.right, rows_bottom - band);
        let hscroll = Rect::new(frame.left, body.bottom, frame.right, rows_bottom);

        TableLayout { mode, toolbar, frame, header, body, footer, hscroll }
    }

    /// The rectangle [`crate::views::ListView`] must be asked about so that
    /// **its** body coincides with the band this table paints rows into.
    ///
    /// The list's own header is the Drive file area's (28 DIP); a DataTable
    /// header is 40 (`py-2.5` over a body line), so the two bands do not
    /// coincide. Rather than re-derive the row arithmetic here, the rectangle
    /// is shifted by the difference — read from the list's own accessor, never
    /// from a literal.
    fn list_frame(&self, l: &TableLayout) -> Rect {
        Rect::new(
            l.body.left,
            l.body.top - self.list.header_height(),
            l.body.right,
            l.body.bottom,
        )
    }

    /// **The virtualisation entry point.** The item indices that intersect the
    /// body — the list's own [`crate::views::ListView::visible_range`],
    /// clamped to the current page and shifted onto its first index.
    ///
    /// The clamp is what makes it correct under pagination: the list's
    /// arithmetic stops at `item_count`, and a page is usually much shorter.
    pub fn visible_rows(&self, bounds: Rect) -> Range<usize> {
        let l = self.layout_of(bounds);
        self.visible_rows_in(&l)
    }

    fn visible_rows_in(&self, l: &TableLayout) -> Range<usize> {
        if l.mode == Mode::Cards {
            // Cards are not a fixed pitch — each one is as tall as its own
            // pairs — so the band is walked from the top instead.
            return self.page_range();
        }
        let page = self.page_range();
        let r = self.list.visible_range(self.list_frame(l), self.list.scroll);
        let len = page.len();
        let start = r.start.min(len);
        let end = r.end.min(len);
        (page.start + start)..(page.start + end.max(start))
    }

    /// The band item `index` occupies — the list's own `item_rect`, offset by
    /// the page's first index because a page always starts at the body's top.
    pub fn row_rect(&self, bounds: Rect, index: usize) -> Rect {
        let l = self.layout_of(bounds);
        self.row_rect_in(&l, index)
    }

    fn row_rect_in(&self, l: &TableLayout, index: usize) -> Rect {
        let page = self.page_range();
        let local = index.saturating_sub(page.start);
        let r = self.list.item_rect(self.list_frame(l), local, self.list.scroll);
        // The band runs along the content, which is wider than the box when
        // the table scrolls sideways; the body's clip cuts it back.
        let (left, right) = self.content_span(l.frame);
        Rect::new(left, r.top, right, r.bottom)
    }

    /// The item under `(x, y)`, or `None` for the header, the footer, a card
    /// gap or empty space past the last row of the page.
    pub fn row_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        let l = self.layout_of(bounds);
        let page = self.page_range();
        if l.mode == Mode::Cards {
            if !l.body.contains(x, y) {
                return None;
            }
            for (n, i) in page.clone().enumerate() {
                let card = self.card_rect(&l, n);
                if card.contains(x, y) {
                    return Some(i);
                }
            }
            return None;
        }
        let local = self.list.row_at(self.list_frame(&l), x, y)?;
        let index = page.start + local;
        (index < page.end).then_some(index)
    }

    /// Whether `(x, y)` lands in the header band, and on which visible column.
    pub fn header_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        let l = self.layout_of(bounds);
        if l.header.bottom <= l.header.top || !l.header.contains(x, y) {
            return None;
        }
        self.column_at(bounds, x)
    }

    /// The nth card's rectangle in the card list (`n` counts from the top of
    /// the page, not from the item index).
    fn card_rect(&self, l: &TableLayout, n: usize) -> Rect {
        let pairs = self.card_pairs().len();
        let h = self.card_height(pairs);
        let top =
            l.body.top + grid::CARD_LIST_PAD + n as f32 * (h + grid::CARD_GAP) - self.list.scroll;
        Rect::new(
            l.body.left + grid::CARD_LIST_PAD,
            top,
            l.body.right - grid::CARD_LIST_PAD,
            top + h,
        )
    }

    /// The card layout's columns: the primary one (its title) and the rest
    /// (its label/value pairs), with `hideOnCards` honoured.
    fn card_columns(&self) -> (Option<ColumnHeader>, Vec<ColumnHeader>) {
        let shown: Vec<ColumnHeader> = self
            .visible_columns()
            .into_iter()
            .filter(|c| !has_flag(c, flags::HIDE_ON_CARDS))
            .collect();
        // « Exactly one column should carry it; the first column is used when
        // none does. »
        let primary = shown
            .iter()
            .position(|c| has_flag(c, flags::PRIMARY))
            .or(if shown.is_empty() { None } else { Some(0) });
        match primary {
            None => (None, Vec::new()),
            Some(p) => {
                let title = shown[p].clone();
                let rest =
                    shown.iter().enumerate().filter(|(i, _)| *i != p).map(|(_, c)| c.clone()).collect();
                (Some(title), rest)
            }
        }
    }

    fn card_pairs(&self) -> Vec<ColumnHeader> {
        self.card_columns().1
    }

    // ── Chrome hit testing ───────────────────────────────────────────────

    /// The chrome element under `(x, y)`.
    ///
    /// Takes a [`Canvas`] because half of the chrome is sized by its text (a
    /// bulk action's button, the page-size pills, the page counter); the parts
    /// that are not — [`bulk_bar`], [`nav_cluster`], [`size_group`] — are pure
    /// functions this only feeds.
    pub fn chrome_at(&self, c: &dyn Canvas, bounds: Rect, x: f32, y: f32) -> Option<Chrome> {
        let l = self.layout_of(bounds);

        if l.toolbar.contains(x, y) {
            if self.selection_mode() {
                let bar = self.bulk_bar_of(c, &l);
                let inline = self.inline_bulk(l.mode);
                if bar.close.contains(x, y) {
                    return Some(Chrome::ClearSelection);
                }
                if let Some(o) = bar.overflow {
                    if o.contains(x, y) {
                        return Some(Chrome::BulkOverflow);
                    }
                }
                for (i, r) in bar.actions.iter().enumerate() {
                    if r.contains(x, y) && i < inline {
                        return Some(Chrome::BulkAction(i));
                    }
                }
                return None;
            }
            if self.configurable_columns && self.chooser_rect(c, &l).contains(x, y) {
                return Some(Chrome::Columns);
            }
            return None;
        }

        if l.footer.bottom > l.footer.top && l.footer.contains(x, y) {
            for (chrome, r) in self.nav_cluster_of(c, &l) {
                if r.contains(x, y) {
                    return Some(chrome);
                }
            }
            if let Some((_, pills)) = self.size_group_of(c, &l) {
                for (n, r) in self.page_size_options.iter().zip(pills) {
                    if r.contains(x, y) {
                        return Some(Chrome::PageSize(*n));
                    }
                }
            }
            return None;
        }

        if let Some(col) = self.resize_handle_at(bounds, x, y) {
            return Some(Chrome::ResizeHandle(col));
        }
        if self.selectable && l.header.bottom > l.header.top && l.header.contains(x, y) {
            let (left, _) = self.content_span(l.frame);
            let sel = Rect::new(left, l.header.top, left + grid::SELECT_COL, l.header.bottom);
            if sel.contains(x, y) {
                return Some(Chrome::SelectAll);
            }
        }
        if let Some(col) = self.header_at(bounds, x, y) {
            return Some(Chrome::Header(col));
        }

        for horizontal in [false, true] {
            if self.scrollbar_in(&l, horizontal, false).is_some_and(|b| b.rail.contains(x, y)) {
                return Some(Chrome::ScrollBar { horizontal });
            }
        }

        if let Some(row) = self.row_at(bounds, x, y) {
            if l.mode == Mode::Table {
                let rect = self.row_rect_in(&l, row);
                if self.selectable && x < rect.left + grid::SELECT_COL {
                    return Some(Chrome::RowCheck(row));
                }
                if self.row_actions && x >= rect.right - grid::ACTIONS_COL {
                    return Some(Chrome::RowMenu(row));
                }
            }
        }
        None
    }

    /// How many bulk actions stay inline in this mode: the web's per-mode
    /// count, lowered to what the last laid-out bar could fit (see
    /// [`fit_bulk`]), so the overflow menu always holds exactly the rest.
    fn inline_bulk(&self, mode: Mode) -> usize {
        let base = Self::inline_base(mode);
        self.bulk_fit.get().map_or(base, |fit| fit.min(base))
    }

    fn inline_base(mode: Mode) -> usize {
        if mode == Mode::Cards {
            INLINE_BULK_COMPACT
        } else {
            INLINE_BULK_DESKTOP
        }
    }

    fn bulk_bar_of(&self, c: &dyn Canvas, l: &TableLayout) -> BulkBar {
        let widths: Vec<f32> = self
            .bulk_actions
            .iter()
            .take(Self::inline_base(l.mode))
            .map(|a| {
                // « {compact ? null : a.label} »: a narrow bar shows the icon
                // only, so the button is measured on an empty label.
                if l.mode == Mode::Cards {
                    a.button().width_of(0.0)
                } else {
                    a.button().width(c)
                }
            })
            .collect();
        let label = c.measure(&self.bulk_label(), &c.formats().body) + grid::TEXT_SLACK;
        let inline = fit_bulk(l.toolbar, &widths, self.bulk_actions.len(), label);
        self.bulk_fit.set(Some(inline));
        bulk_bar(l.toolbar, &widths[..inline], self.bulk_actions.len() > inline)
    }

    /// « N en sélection ».
    fn bulk_label(&self) -> String {
        format!("{} {}", self.selected().len(), self.copy.selected)
    }

    fn chooser_rect(&self, c: &dyn Canvas, l: &TableLayout) -> Rect {
        let f = c.formats();
        let label = if l.mode == Mode::Cards {
            0.0
        } else {
            c.measure(&self.copy.columns, &f.body) + grid::TEXT_SLACK
        };
        let w = grid::CHOOSER_PAD_X * 2.0
            + grid::CHOOSER_GLYPH
            + if label > 0.0 { grid::CHOOSER_GAP + label } else { 0.0 };
        let cy = (l.toolbar.top + l.toolbar.bottom) / 2.0;
        Rect::new(
            l.toolbar.right - w,
            cy - grid::TOOLBAR_ROW / 2.0,
            l.toolbar.right,
            cy + grid::TOOLBAR_ROW / 2.0,
        )
    }

    fn page_label(&self) -> String {
        format!("{} / {}", self.page_index() + 1, self.page_count())
    }

    fn nav_cluster_of(&self, c: &dyn Canvas, l: &TableLayout) -> Vec<(Chrome, Rect)> {
        let w = c.measure(&self.page_label(), &c.formats().body) + grid::TEXT_SLACK;
        nav_cluster(l.footer, w, l.mode == Mode::Cards)
    }

    /// The page-size group, or `None` when the footer does not show one:
    /// « Hidden when the container is narrow (the count alone is enough) », and
    /// a single option is not a choice.
    fn size_group_of(&self, c: &dyn Canvas, l: &TableLayout) -> Option<(Rect, Vec<Rect>)> {
        if l.mode == Mode::Cards || self.page_size_options.len() < 2 {
            return None;
        }
        let f = c.formats();
        let cy = (l.footer.top + l.footer.bottom) / 2.0;
        // The label is MEASURED, plus the rounding guard, so its own box is
        // never a hair too narrow for it.
        let label = c.measure(&self.copy.rows_per_page, &f.body) + grid::TEXT_SLACK;
        let left = l.footer.left + grid::FOOTER_PAD_X + label + grid::SIZE_LABEL_GAP;
        let widths: Vec<f32> = self
            .page_size_options
            .iter()
            .map(|n| c.measure(&n.to_string(), &f.body) + grid::TEXT_SLACK)
            .collect();
        let (group, pills) = size_group(left, cy, &widths);
        // The web bar is `flex-wrap`: a footer too narrow for the size group,
        // the counter AND the navigation wraps onto a second line. The band
        // here has one line, so the group — the optional part, the one
        // `compact` drops too — goes instead of anything overlapping.
        let counter = c.measure(&self.counter_text(), &f.body) + grid::TEXT_SLACK;
        let nav_left = self
            .nav_cluster_of(c, l)
            .first()
            .map(|(_, r)| r.left)
            .unwrap_or(l.footer.right);
        let needed = group.right + grid::FOOTER_CLUSTER_GAP + counter + grid::FOOTER_CLUSTER_GAP;
        (needed <= nav_left).then_some((group, pills))
    }

    /// « first–last / total ».
    fn counter_text(&self) -> String {
        let (first, last, total) = self.page_counter();
        format!("{first}–{last} / {total}")
    }

    /// Where the rectangle of one piece of chrome is, for anchoring a popup
    /// on it or outlining it with a focus ring. `None` when it is not on
    /// screen.
    pub fn chrome_rect(&self, c: &dyn Canvas, bounds: Rect, chrome: Chrome) -> Option<Rect> {
        let l = self.layout_of(bounds);
        match chrome {
            Chrome::ClearSelection | Chrome::BulkAction(_) | Chrome::BulkOverflow => {
                if !self.selection_mode() {
                    return None;
                }
                let bar = self.bulk_bar_of(c, &l);
                match chrome {
                    Chrome::ClearSelection => Some(bar.close),
                    Chrome::BulkOverflow => bar.overflow,
                    Chrome::BulkAction(i) => bar.actions.get(i).copied(),
                    _ => None,
                }
            }
            Chrome::Columns => (self.configurable_columns
                && !self.selection_mode()
                && l.toolbar.bottom > l.toolbar.top)
                .then(|| self.chooser_rect(c, &l)),
            Chrome::SelectAll => (self.selectable && l.header.bottom > l.header.top).then(|| {
                let (left, _) = self.content_span(l.frame);
                Rect::new(left, l.header.top, left + grid::SELECT_COL, l.header.bottom)
            }),
            Chrome::Header(i) => {
                if l.header.bottom <= l.header.top {
                    return None;
                }
                let xs = self.column_x_offsets(l.frame);
                let (a, b) = (*xs.get(i)?, *xs.get(i + 1)?);
                Some(Rect::new(a, l.header.top, b, l.header.bottom))
            }
            Chrome::ResizeHandle(i) => self.resize_grip(&l, i),
            Chrome::RowCheck(r) | Chrome::RowMenu(r) => {
                if l.mode != Mode::Table || !self.page_range().contains(&r) {
                    return None;
                }
                let row = self.row_rect_in(&l, r);
                Some(match chrome {
                    Chrome::RowCheck(_) => Rect::new(row.left, row.top, row.left + grid::SELECT_COL, row.bottom),
                    _ => {
                        let cy = (row.top + row.bottom) / 2.0;
                        Rect::new(
                            row.right - grid::ACTIONS_PAD_X - grid::ICON_BUTTON,
                            cy - grid::ICON_BUTTON / 2.0,
                            row.right - grid::ACTIONS_PAD_X,
                            cy + grid::ICON_BUTTON / 2.0,
                        )
                    }
                })
            }
            Chrome::PageSize(n) => {
                let (_, pills) = self.size_group_of(c, &l)?;
                let i = self.page_size_options.iter().position(|o| *o == n)?;
                pills.get(i).copied()
            }
            Chrome::FirstPage | Chrome::PrevPage | Chrome::NextPage | Chrome::LastPage => {
                if l.footer.bottom <= l.footer.top {
                    return None;
                }
                self.nav_cluster_of(c, &l).into_iter().find(|(k, _)| *k == chrome).map(|(_, r)| r)
            }
            Chrome::ScrollBar { horizontal } => self.scrollbar_in(&l, horizontal, false).map(|b| b.rail),
        }
    }

    /// Whether a pagination button is disabled (`disabled:opacity-40`).
    fn nav_disabled(&self, chrome: Chrome) -> bool {
        match chrome {
            Chrome::FirstPage | Chrome::PrevPage => self.page_index() == 0,
            Chrome::NextPage | Chrome::LastPage => self.page_index() + 1 >= self.page_count(),
            _ => false,
        }
    }

    // ── Column resizing ──────────────────────────────────────────────────

    /// Whether the header carries resize grips right now: « table layout
    /// only: a card list has no columns ».
    fn can_resize(&self, l: &TableLayout) -> bool {
        self.resizable_columns && l.mode == Mode::Table && l.header.bottom > l.header.top
    }

    /// The grab strip of visible column `i`: `RESIZE_GRIP` wide, centred on
    /// its right edge, the header's full height.
    fn resize_grip(&self, l: &TableLayout, i: usize) -> Option<Rect> {
        if !self.can_resize(l) {
            return None;
        }
        let xs = self.column_x_offsets(l.frame);
        let x = *xs.get(i + 1)?;
        let half = grid::RESIZE_GRIP / 2.0;
        Some(Rect::new(x - half, l.header.top, x + half, l.header.bottom))
    }

    /// The column whose resize grip is under `(x, y)`. Where two strips
    /// could meet, the column to the LEFT of the edge wins — the grip belongs
    /// to the edge, and the edge to the column it ends.
    pub fn resize_handle_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        let l = self.layout_of(bounds);
        if !self.can_resize(&l) || !l.header.contains(x, y) {
            return None;
        }
        let n = self.visible_columns().len();
        (0..n).find(|&i| self.resize_grip(&l, i).is_some_and(|g| g.contains(x, y)))
    }

    /// A visible column's current width.
    pub fn column_width(&self, visible: usize) -> f32 {
        self.visible_columns().get(visible).map(|c| c.width.max(0) as f32).unwrap_or(0.0)
    }

    /// Sets a visible column's width, never below [`grid::RESIZE_MIN`]. The
    /// declared width is remembered the first time, for
    /// [`DataTable::reset_column_width`].
    pub fn set_column_width(&mut self, visible: usize, width: f32) {
        let cols = self.visible_columns();
        let Some(model) = self.model_column(&cols, visible) else {
            return;
        };
        let Some(col) = self.list.columns.get_mut(model) else {
            return;
        };
        if !self.declared_widths.iter().any(|(id, _)| id == &col.name) {
            self.declared_widths.push((col.name.clone(), col.width));
        }
        col.width = width.max(grid::RESIZE_MIN).round() as i32;
    }

    /// The keyboard equivalent — `nudge`: « a mouse-only affordance is not an
    /// affordance ».
    pub fn nudge_column(&mut self, visible: usize, delta: f32) {
        let w = self.column_width(visible);
        self.set_column_width(visible, w + delta);
    }

    /// A double-click on a grip: back to the declared width.
    pub fn reset_column_width(&mut self, visible: usize) {
        let cols = self.visible_columns();
        let Some(model) = self.model_column(&cols, visible) else {
            return;
        };
        let Some(col) = self.list.columns.get_mut(model) else {
            return;
        };
        if let Some(pos) = self.declared_widths.iter().position(|(id, _)| id == &col.name) {
            col.width = self.declared_widths.remove(pos).1;
        }
    }

    /// Starts a drag on column `visible`'s grip, the pointer at `x`.
    pub fn begin_resize(&mut self, visible: usize, x: f32) {
        self.resize =
            Some(ColumnDrag { column: visible, start_x: x, start_width: self.column_width(visible) });
    }

    /// Follows the pointer during a drag: `startW + (x - startX)`, floored.
    pub fn drag_resize(&mut self, x: f32) {
        if let Some(d) = self.resize {
            self.set_column_width(d.column, d.start_width + (x - d.start_x));
        }
    }

    pub fn end_resize(&mut self) {
        self.resize = None;
    }

    // ── Scrolling ────────────────────────────────────────────────────────

    /// The height the body's content needs: the page's rows, or its cards.
    pub fn content_height(&self, bounds: Rect) -> f32 {
        let l = self.layout_of(bounds);
        self.content_height_in(&l)
    }

    fn content_height_in(&self, l: &TableLayout) -> f32 {
        if self.loading || self.is_empty() {
            return 0.0;
        }
        let n = self.page_range().len();
        match l.mode {
            Mode::Table => n as f32 * self.list.row_height(),
            Mode::Cards => {
                if n == 0 {
                    return 0.0;
                }
                let h = self.card_height(self.card_pairs().len());
                grid::CARD_LIST_PAD * 2.0 + n as f32 * h + (n - 1) as f32 * grid::CARD_GAP
            }
        }
    }

    /// How far the body can scroll down: the header stays put (it is its own
    /// band), only the rows move under it.
    pub fn max_scroll_y(&self, bounds: Rect) -> f32 {
        let l = self.layout_of(bounds);
        (self.content_height_in(&l) - (l.body.bottom - l.body.top)).max(0.0)
    }

    /// Scrolls by `(dx, dy)` DIP (the web's sign: positive = right / down),
    /// clamped. Returns whether anything moved, so a caller can let the wheel
    /// through to its own container when the table is already at an end.
    pub fn scroll_by(&mut self, bounds: Rect, dx: f32, dy: f32) -> bool {
        let l = self.layout_of(bounds);
        let (old_x, old_y) = (self.sx(l.frame), self.list.scroll);
        self.scroll_x = (old_x + dx).clamp(0.0, self.max_scroll_x(l.frame));
        self.list.scroll = (old_y + dy).clamp(0.0, self.max_scroll_y(bounds));
        (self.scroll_x - old_x).abs() > f32::EPSILON || (self.list.scroll - old_y).abs() > f32::EPSILON
    }

    /// Brings the scroll back inside what `bounds` allows (after a page
    /// change, a resize, a filter).
    pub fn clamp_scroll(&mut self, bounds: Rect) {
        self.scroll_by(bounds, 0.0, 0.0);
    }

    /// A scroll bar over the body, when there is something to scroll.
    /// `expanded` is WinUI's unfolded gutter (hover or drag).
    fn scrollbar_in(&self, l: &TableLayout, horizontal: bool, expanded: bool) -> Option<Scrollbar> {
        if self.loading || self.is_empty() || l.body.bottom <= l.body.top {
            return None;
        }
        if horizontal {
            if l.mode != Mode::Table {
                return None;
            }
            let extent = self.content_width(l.frame.right - l.frame.left);
            Scrollbar::new(&Self::bar_area(l, true), extent, self.sx(l.frame), true, expanded)
        } else {
            Scrollbar::new(&Self::bar_area(l, false), self.content_height_in(l), self.list.scroll, false, expanded)
        }
    }

    /// The area the horizontal bar is laid against: the body plus the band
    /// reserved under it, so the bar's rail lands in that band.
    fn hbar_area(l: &TableLayout) -> Rect {
        Rect::new(l.body.left, l.body.top, l.body.right, l.hscroll.bottom.max(l.body.bottom))
    }

    /// The area a bar is laid against, shortened so its gutter stays inside the
    /// frame's `rounded-xl` border — clear of the rounded corners, which a bar
    /// running down to the frame's edge (no footer, no header) would cross.
    fn bar_area(l: &TableLayout, horizontal: bool) -> Rect {
        let (frame, r) = crate::range::inside_border(l.frame, radius::XL);
        if horizontal {
            let a = Self::hbar_area(l);
            let rail = Rect::new(a.left, a.bottom - sb::SCROLLBAR_SIZE, a.right, a.bottom);
            let fit = crate::range::fit_rail(rail, frame, r);
            Rect::new(fit.left, a.top, fit.right, a.bottom)
        } else {
            let b = l.body;
            let rail = Rect::new(b.right - sb::SCROLLBAR_SIZE, b.top, b.right, b.bottom);
            let fit = crate::range::fit_rail(rail, frame, r);
            Rect::new(b.left, fit.top, b.right, fit.bottom)
        }
    }

    /// Starts dragging a scroll bar at `(x, y)`. On the thumb, it grabs it
    /// where it was pressed; on the rail, the thumb first jumps under the
    /// pointer — the overlay bar's behaviour — then follows it.
    pub fn begin_scroll_drag(&mut self, bounds: Rect, horizontal: bool, x: f32, y: f32) {
        let l = self.layout_of(bounds);
        let Some(bar) = self.scrollbar_in(&l, horizontal, true) else {
            return;
        };
        let (pos, start, len) = if horizontal {
            (x, bar.thumb.left, bar.thumb.right - bar.thumb.left)
        } else {
            (y, bar.thumb.top, bar.thumb.bottom - bar.thumb.top)
        };
        let grab = if pos >= start && pos <= start + len { pos - start } else { len / 2.0 };
        self.scroll_drag = Some(ScrollDrag { horizontal, grab });
        self.drag_scroll(bounds, x, y);
    }

    /// Follows the pointer while a thumb is dragged.
    pub fn drag_scroll(&mut self, bounds: Rect, x: f32, y: f32) {
        let Some(d) = self.scroll_drag else {
            return;
        };
        let l = self.layout_of(bounds);
        let Some(bar) = self.scrollbar_in(&l, d.horizontal, true) else {
            return;
        };
        if d.horizontal {
            let extent = self.content_width(l.frame.right - l.frame.left);
            self.scroll_x = bar.scroll_at(&Self::hbar_area(&l), extent, x, d.grab);
        } else {
            let extent = self.content_height_in(&l);
            self.list.scroll = bar.scroll_at(&l.body, extent, y, d.grab);
        }
        self.clamp_scroll(bounds);
    }

    pub fn end_scroll_drag(&mut self) {
        self.scroll_drag = None;
    }

    /// Scrolls the least needed for item `index` to be fully in the body —
    /// what a keyboard move does after it moves the cursor.
    pub fn scroll_into_view(&mut self, bounds: Rect, index: usize) {
        let l = self.layout_of(bounds);
        let page = self.page_range();
        if !page.contains(&index) {
            return;
        }
        let n = index - page.start;
        let (top, bottom) = match l.mode {
            Mode::Table => {
                let rh = self.list.row_height();
                (n as f32 * rh, (n + 1) as f32 * rh)
            }
            Mode::Cards => {
                let h = self.card_height(self.card_pairs().len());
                let t = grid::CARD_LIST_PAD + n as f32 * (h + grid::CARD_GAP);
                (t - grid::CARD_LIST_PAD, t + h + grid::CARD_LIST_PAD)
            }
        };
        let view = l.body.bottom - l.body.top;
        if top < self.list.scroll {
            self.list.scroll = top;
        } else if bottom > self.list.scroll + view {
            self.list.scroll = bottom - view;
        }
        self.clamp_scroll(bounds);
    }

    // ── Cells ────────────────────────────────────────────────────────────

    /// The content box of one cell: item `row`, **visible** column `column`,
    /// minus the `px-4` padding. `None` when the row is not on the current
    /// page or the table is not laid out as a table. The rectangle can lie
    /// outside the body when the row is scrolled away; the table clips it.
    pub fn cell_rect(&self, bounds: Rect, row: usize, column: usize) -> Option<Rect> {
        let l = self.layout_of(bounds);
        if l.mode != Mode::Table || !self.page_range().contains(&row) {
            return None;
        }
        let xs = self.column_x_offsets(l.frame);
        let (a, b) = (*xs.get(column)?, *xs.get(column + 1)?);
        let r = self.row_rect_in(&l, row);
        Some(Rect::new(a + grid::CELL_PAD_X, r.top, (b - grid::CELL_PAD_X).max(a + grid::CELL_PAD_X), r.bottom))
    }

    /// The text of one cell — item `row`, **visible** column `column`.
    pub fn cell_text(&self, row: usize, column: usize) -> String {
        let cols = self.visible_columns();
        match (self.model_column(&cols, column), self.list.items.get(row)) {
            (Some(m), Some(item)) => item.cell(m).to_string(),
            _ => String::new(),
        }
    }

    /// One row, tab-separated — `rowText` in `copy.ts`: « pastes into a
    /// spreadsheet as separate columns ».
    pub fn row_text(&self, row: usize) -> String {
        (0..self.visible_columns().len()).map(|c| self.cell_text(row, c)).collect::<Vec<_>>().join("\t")
    }

    /// Every row of the CURRENT PAGE for one column, one value per line —
    /// `columnText`, which copies « the current page, because that is all the
    /// DOM holds ».
    pub fn column_text(&self, column: usize) -> String {
        self.page_range().map(|r| self.cell_text(r, column)).collect::<Vec<_>>().join("\n")
    }

    // ── Menus ────────────────────────────────────────────────────────────

    /// The bulk actions folded into the overflow in `mode`.
    fn overflow_actions(&self, mode: Mode) -> &[BulkAction] {
        let inline = self.inline_bulk(mode).min(self.bulk_actions.len());
        &self.bulk_actions[inline..]
    }

    /// Builds one of the table's menus, as `MenuDropdown` would list it.
    pub fn menu(&self, kind: TableMenu, mode: Mode) -> Menu {
        let action = |a: &BulkAction| {
            let mut e = MenuEntry::new(a.label.clone());
            if let Some(icon) = a.icon {
                e = e.icon(icon);
            }
            if a.danger {
                e = e.danger();
            }
            e.build()
        };
        match kind {
            TableMenu::Row(_) => Menu::with_items(self.row_menu_actions.iter().map(action).collect()),
            TableMenu::BulkOverflow => Menu::with_items(self.overflow_actions(mode).iter().map(action).collect()),
            TableMenu::Copy { column, .. } => {
                let name = self.visible_columns().get(column).map(|c| column_label(c).to_string()).unwrap_or_default();
                Menu::with_items(vec![
                    // lucide `Copy`, `Rows3`, `Columns3`; `Rows3` is not in the
                    // desktop's icon set, `AlignJustify` (three rules) stands in.
                    MenuEntry::new(self.copy.copy_cell.clone()).icon("Copy").build(),
                    MenuEntry::new(self.copy.copy_row.clone()).icon("AlignJustify").build(),
                    MenuEntry::new(self.copy.copy_column.replace("{name}", &name)).icon("Columns3").build(),
                ])
            }
            TableMenu::Columns => {
                // `{ type: 'label', text: tr('ui.dt_columns') }`, then one check
                // box per column, a `required` one disabled.
                let mut items = vec![section(self.copy.columns.clone())];
                for col in &self.list.columns {
                    let shown = !self.hidden_columns.iter().any(|h| h == &col.name);
                    items.push(
                        MenuEntry::new(column_label(col).to_string())
                            .checked(shown)
                            .enabled(!has_flag(col, flags::REQUIRED))
                            .build(),
                    );
                }
                Menu::with_items(items)
            }
        }
    }

    /// What choosing entry `index` of `kind` asks for, or `None` for a row
    /// that is not an action (the chooser's label, a disabled column).
    pub fn menu_command(&self, kind: TableMenu, mode: Mode, index: usize) -> Option<TableCommand> {
        match kind {
            TableMenu::Row(row) => self
                .row_menu_actions
                .get(index)
                .map(|a| TableCommand::RowAction { row, id: a.id.clone() }),
            TableMenu::BulkOverflow => {
                self.overflow_actions(mode).get(index).map(|a| TableCommand::BulkAction(a.id.clone()))
            }
            TableMenu::Copy { row, column } => match index {
                0 => Some(TableCommand::Copy(self.cell_text(row, column))),
                1 => Some(TableCommand::Copy(self.row_text(row))),
                2 => Some(TableCommand::Copy(self.column_text(column))),
                _ => None,
            },
            TableMenu::Columns => {
                // Entry 0 is the section label.
                let col = self.list.columns.get(index.checked_sub(1)?)?;
                (!has_flag(col, flags::REQUIRED)).then(|| TableCommand::ToggleColumn(col.name.clone()))
            }
        }
    }

    /// Applies what the table can apply on its own — showing or hiding a
    /// column. Returns whether it did; every other command is the caller's.
    pub fn apply_command(&mut self, cmd: &TableCommand) -> bool {
        match cmd {
            TableCommand::ToggleColumn(id) => {
                if let Some(pos) = self.hidden_columns.iter().position(|h| h == id) {
                    self.hidden_columns.remove(pos);
                } else if self.list.columns.iter().any(|c| &c.name == id) {
                    self.hidden_columns.push(id.clone());
                }
                true
            }
            _ => false,
        }
    }

    /// Where menu `kind` opens: `(point, flip_bottom)` for [`place_menu`].
    /// `pointer` is used by the copy menu, which opens at the right click
    /// (or, opened from the keyboard, under its cell).
    pub fn menu_origin(
        &self,
        c: &dyn Canvas,
        bounds: Rect,
        kind: TableMenu,
        pointer: Option<(f32, f32)>,
    ) -> ((f32, f32), f32) {
        let under = |r: Rect, left: f32| ((left, r.bottom + grid::MENU_GAP), r.top - grid::MENU_GAP);
        let fallback = |r: Rect| ((r.left, r.bottom), r.top);
        match kind {
            TableMenu::Row(row) => match self.chrome_rect(c, bounds, Chrome::RowMenu(row)) {
                Some(r) => under(r, r.right - grid::ROW_MENU_REACH),
                None => fallback(self.row_rect(bounds, row)),
            },
            TableMenu::Copy { row, column } => match pointer {
                Some((x, y)) => ((x, y), y),
                None => match self.cell_rect(bounds, row, column) {
                    Some(r) => under(r, r.left),
                    None => fallback(self.row_rect(bounds, row)),
                },
            },
            TableMenu::BulkOverflow | TableMenu::Columns => {
                let chrome = if kind == TableMenu::Columns { Chrome::Columns } else { Chrome::BulkOverflow };
                match self.chrome_rect(c, bounds, chrome) {
                    Some(r) => under(r, r.left),
                    None => fallback(bounds),
                }
            }
        }
    }

    /// The panel of menu `kind`, placed against the monitor's work area.
    pub fn menu_rect(
        &self,
        c: &dyn Canvas,
        bounds: Rect,
        kind: TableMenu,
        pointer: Option<(f32, f32)>,
        area: Rect,
    ) -> Rect {
        let mode = self.layout_of(bounds).mode;
        let mut want = self.menu(kind, mode).measure(c);
        if kind == TableMenu::Columns {
            want.width = want.width.max(grid::COLUMNS_MENU_MIN);
        }
        let (at, flip) = self.menu_origin(c, bounds, kind, pointer);
        place_menu(at, flip, want, area)
    }

    // ── Clicks and keys ──────────────────────────────────────────────────

    /// A click on a row's check box. With Shift, the rows between the last
    /// clicked one and this one all take the clicked row's NEW state — the
    /// range gesture of mail and file lists.
    pub fn click_row_check(&mut self, index: usize, shift: bool) {
        let on = !self.list.items.get(index).is_some_and(|it| it.selected);
        match (shift, self.anchor) {
            (true, Some(a)) => {
                let (lo, hi) = (a.min(index), a.max(index));
                for i in lo..=hi.min(self.list.item_count().saturating_sub(1)) {
                    self.list.set_selected(i, on);
                }
            }
            _ => {
                self.list.set_selected(index, on);
                self.anchor = Some(index);
            }
        }
        self.range_base = None;
        self.cursor = Some(index);
    }

    /// Applies a click on `chrome`. Everything the table owns is done here;
    /// what it does not (a menu to open, a bulk action to run) comes back as
    /// a [`TableEvent`]. `shift` is the Shift key, for range selection.
    pub fn activate(&mut self, chrome: Chrome, shift: bool) -> Option<TableEvent> {
        match chrome {
            Chrome::Header(col) => self.toggle_sort(col),
            Chrome::SelectAll => self.toggle_all(),
            Chrome::RowCheck(row) => self.click_row_check(row, shift),
            Chrome::ClearSelection => self.clear_selection(),
            Chrome::FirstPage => self.go_to_page(0),
            Chrome::PrevPage => self.go_to_page(self.page_index().saturating_sub(1)),
            Chrome::NextPage => self.go_to_page(self.page_index() + 1),
            Chrome::LastPage => self.go_to_page(self.page_count().saturating_sub(1)),
            Chrome::PageSize(n) => {
                self.set_page_size(n);
                self.list.scroll = 0.0;
            }
            Chrome::BulkAction(i) => {
                return self.bulk_actions.get(i).map(|a| TableEvent::BulkAction(a.id.clone()));
            }
            Chrome::BulkOverflow => return Some(TableEvent::OpenMenu(TableMenu::BulkOverflow)),
            Chrome::Columns => return Some(TableEvent::OpenMenu(TableMenu::Columns)),
            Chrome::RowMenu(row) => return Some(TableEvent::OpenMenu(TableMenu::Row(row))),
            Chrome::ResizeHandle(_) | Chrome::ScrollBar { .. } => {}
        }
        None
    }

    /// A page change resets the vertical scroll: the new page starts at its
    /// top, as a freshly mounted `<tbody>` does.
    fn go_to_page(&mut self, page: usize) {
        let before = self.page_index();
        self.set_page(page);
        if self.page_index() != before {
            self.list.scroll = 0.0;
        }
    }

    /// The focus stops, in Tab order — the web's DOM order: the toolbar
    /// (chooser, or the bulk bar's close, actions and overflow), the
    /// select-all box, each sort button then its resize separator, the rows,
    /// the page-size radios, the enabled pagination buttons. A caller
    /// registers each with its focus manager and feeds the focused one back
    /// through [`DataTable::focus_part`].
    pub fn focus_stops(&self, c: &dyn Canvas, bounds: Rect) -> Vec<(FocusPart, Rect)> {
        let l = self.layout_of(bounds);
        // The candidates in DOM order; `None` stands for the rows.
        let mut order: Vec<Option<Chrome>> = Vec::new();
        if self.selection_mode() {
            order.push(Some(Chrome::ClearSelection));
            // The bar as laid out now: a narrow one folds actions away.
            for i in 0..self.bulk_bar_of(c, &l).actions.len() {
                order.push(Some(Chrome::BulkAction(i)));
            }
            order.push(Some(Chrome::BulkOverflow));
        } else {
            order.push(Some(Chrome::Columns));
        }
        order.push(Some(Chrome::SelectAll));
        let cols = self.visible_columns();
        for i in 0..cols.len() {
            if Self::sortable(&cols, i) {
                order.push(Some(Chrome::Header(i)));
            }
            order.push(Some(Chrome::ResizeHandle(i)));
        }
        order.push(None);
        order.extend(self.page_size_options.iter().map(|n| Some(Chrome::PageSize(*n))));
        for chrome in [Chrome::FirstPage, Chrome::PrevPage, Chrome::NextPage, Chrome::LastPage] {
            if !self.nav_disabled(chrome) {
                order.push(Some(chrome));
            }
        }

        let rows_on = !self.loading && !self.is_empty() && l.body.bottom > l.body.top;
        order
            .into_iter()
            .filter_map(|slot| match slot {
                Some(chrome) => {
                    self.chrome_rect(c, bounds, chrome).map(|r| (FocusPart::Chrome(chrome), r))
                }
                None => rows_on.then_some((FocusPart::Rows, l.body)),
            })
            .collect()
    }

    /// The row the cursor stands on, defaulting to the first row of the page
    /// the first time the rows get the focus.
    fn cursor_or_first(&self) -> Option<usize> {
        let page = self.page_range();
        match self.cursor {
            Some(c) if c < self.list.item_count() => Some(c),
            _ => (!page.is_empty()).then_some(page.start),
        }
    }

    /// The rows the cursor may reach: every row when the table paginates
    /// itself (moving past the page's end turns the page), only the page when
    /// the caller paginates server-side.
    fn cursor_span(&self) -> Range<usize> {
        if self.manual_pagination || !self.paginated() {
            self.page_range()
        } else {
            0..self.list.item_count()
        }
    }

    /// Moves the cursor to `target`, turning the page if it lands on another
    /// one, extending the selection when `extend`.
    pub fn move_cursor(&mut self, bounds: Rect, target: usize, extend: bool) {
        let span = self.cursor_span();
        if span.is_empty() {
            return;
        }
        let target = target.clamp(span.start, span.end - 1);
        let from = self.cursor_or_first().unwrap_or(target);
        if extend {
            let anchor = *self.anchor.get_or_insert(from);
            let base = self.range_base.get_or_insert_with(|| self.list.selected_indices()).clone();
            let (lo, hi) = (anchor.min(target), anchor.max(target));
            for i in 0..self.list.item_count() {
                let on = (lo..=hi).contains(&i) || base.binary_search(&i).is_ok();
                if self.list.items.get(i).is_some_and(|it| it.selected != on) {
                    self.list.set_selected(i, on);
                }
            }
        } else {
            self.anchor = Some(target);
            self.range_base = None;
        }
        self.cursor = Some(target);
        if self.paginated() && !self.manual_pagination {
            self.go_to_page(target / self.page_size);
        }
        self.scroll_into_view(bounds, target);
    }

    /// How many rows a PgUp / PgDn moves: one bodyful, less one so the row at
    /// the edge stays in view as context.
    fn page_step(&self, bounds: Rect) -> usize {
        let l = self.layout_of(bounds);
        let rh = self.list.row_height().max(1.0);
        (((l.body.bottom - l.body.top) / rh).floor() as usize).saturating_sub(1).max(1)
    }

    /// A key-down while the table holds the focus. `true` when the table
    /// used it (the caller then consumes it); events that need the caller
    /// come back in the second slot.
    ///
    /// * on the rows: ↑ ↓ move the cursor, PgUp PgDn by a bodyful, Home End
    ///   to the page's ends (Ctrl for the whole table), Shift with any of
    ///   them extends the selection; Space toggles the cursor row (Shift+Space
    ///   selects anchor → cursor); Ctrl+A selects the page; Enter is
    ///   [`TableEvent::RowActivated`]; the context-menu key or Shift+F10 opens
    ///   the row menu (the copy menu when the row has no action);
    /// * on a resize separator: ← → nudge by 8 (24 with Shift), as the web;
    /// * on any other stop: Enter or Space activates it, like a `<button>`.
    pub fn on_key(&mut self, bounds: Rect, key: u16, mods: Modifiers) -> (bool, Option<TableEvent>) {
        let Some(part) = self.focus_part else {
            return (false, None);
        };
        let plain = mods.matches(Modifiers::NONE);
        let shift_only = mods.matches(Modifiers::SHIFT);
        match part {
            FocusPart::Chrome(Chrome::ResizeHandle(i)) => {
                if !(plain || shift_only) {
                    return (false, None);
                }
                let step = if mods.shift { grid::RESIZE_STEP_BIG } else { grid::RESIZE_STEP };
                match key {
                    vk::RIGHT => self.nudge_column(i, step),
                    vk::LEFT => self.nudge_column(i, -step),
                    _ => return (false, None),
                }
                (true, None)
            }
            FocusPart::Chrome(chrome) => {
                if plain && (key == vk::ENTER || key == vk::SPACE) {
                    if matches!(chrome, Chrome::FirstPage | Chrome::PrevPage | Chrome::NextPage | Chrome::LastPage)
                        && self.nav_disabled(chrome)
                    {
                        return (true, None);
                    }
                    let ev = self.activate(chrome, false);
                    return (true, ev);
                }
                (false, None)
            }
            FocusPart::Rows => self.on_row_key(bounds, key, mods),
        }
    }

    fn on_row_key(&mut self, bounds: Rect, key: u16, mods: Modifiers) -> (bool, Option<TableEvent>) {
        let Some(cur) = self.cursor_or_first() else {
            return (false, None);
        };
        let extend = mods.shift;
        let nav_mods = mods.matches(Modifiers::NONE) || mods.matches(Modifiers::SHIFT);
        let whole = mods.ctrl && !mods.alt;
        let page = self.page_range();
        let span = self.cursor_span();
        let target = match key {
            vk::DOWN if nav_mods => Some(cur + 1),
            vk::UP if nav_mods => Some(cur.saturating_sub(1)),
            vk::PAGE_DOWN if nav_mods => Some(cur + self.page_step(bounds)),
            vk::PAGE_UP if nav_mods => Some(cur.saturating_sub(self.page_step(bounds))),
            vk::HOME if whole => Some(span.start),
            vk::END if whole => Some(span.end.saturating_sub(1)),
            vk::HOME if nav_mods => Some(page.start),
            vk::END if nav_mods => Some(page.end.saturating_sub(1)),
            _ => None,
        };
        if let Some(t) = target {
            // Home/End with Ctrl+Shift also extend.
            self.move_cursor(bounds, t, extend);
            return (true, None);
        }
        match key {
            vk::SPACE if mods.matches(Modifiers::NONE) => {
                self.toggle_row(cur);
                self.anchor = Some(cur);
                self.range_base = None;
                self.cursor = Some(cur);
                (true, None)
            }
            vk::SPACE if mods.matches(Modifiers::SHIFT) => {
                let anchor = self.anchor.unwrap_or(cur);
                self.cursor = Some(anchor);
                self.move_cursor(bounds, cur, true);
                (true, None)
            }
            k if k == vk::letter('A') && mods.matches(Modifiers::CTRL) => {
                for i in page {
                    self.list.set_selected(i, true);
                }
                (true, None)
            }
            vk::ENTER if mods.matches(Modifiers::NONE) => {
                self.cursor = Some(cur);
                (true, Some(TableEvent::RowActivated(cur)))
            }
            k if (k == vk::APPS && mods.matches(Modifiers::NONE))
                || (k == vk::F10 && mods.matches(Modifiers::SHIFT)) =>
            {
                self.cursor = Some(cur);
                let menu = if self.row_actions && k == vk::APPS {
                    TableMenu::Row(cur)
                } else {
                    TableMenu::Copy { row: cur, column: 0 }
                };
                (true, Some(TableEvent::OpenMenu(menu)))
            }
            _ => (false, None),
        }
    }

    /// The pointer cursor the table wants at `(x, y)`: the column-resize
    /// arrows over a grip and throughout a resize drag (`cursor-col-resize`),
    /// else none of its own.
    pub fn cursor_at(&self, bounds: Rect, x: f32, y: f32) -> Option<Cursor> {
        (self.resize.is_some() || self.resize_handle_at(bounds, x, y).is_some()).then_some(Cursor::ResizeEW)
    }

    /// Whether the focus ring shows on `part`.
    fn ring_on(&self, part: FocusPart) -> bool {
        self.focus_visible && self.focus_part == Some(part)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Painting
// ─────────────────────────────────────────────────────────────────────────────

impl DataTable {
    /// The check box, painted by [`crate::buttons::CheckBox`] so a table's box
    /// and a form's box are the same 18 DIP control, tri-state included.
    fn paint_check(&self, c: &dyn Canvas, cell: Rect, state: CheckState, hot: bool, ring: bool) {
        let cy = (cell.top + cell.bottom) / 2.0;
        let box_rect = Rect::new(
            cell.left,
            cy - control::CHECK_BOX / 2.0,
            cell.left + control::CHECK_BOX,
            cy + control::CHECK_BOX / 2.0,
        );
        let mut cb = crate::buttons::CheckBox::new("");
        cb.check_state = state;
        cb.three_state = true;
        cb.paint(c, box_rect, WidgetState::REST.hot(hot).focused(ring).focus_visible(ring));
    }

    /// `focus-visible:ring-2 ring-primary`: a 2 DIP accent ring drawn just
    /// OUTSIDE `rect`, as a `box-shadow` ring is.
    fn paint_ring(c: &dyn Canvas, rect: Rect, corner: f32) {
        let t = c.theme();
        let half = grid::FOCUS_RING / 2.0;
        let r = Rect::new(rect.left - half, rect.top - half, rect.right + half, rect.bottom + half);
        c.stroke_rounded_w(&r, corner + half, &t.accent, grid::FOCUS_RING);
    }

    /// Text in a cell, aligned per column, and ELLIPSISED whatever the
    /// alignment: a value too long for its cell fills it from the left edge,
    /// where alignment no longer means anything, and never spills over its
    /// neighbour.
    fn cell_text_in(c: &dyn Canvas, text: &str, rect: &Rect, align: HorizontalAlignment, ink: &D2D1_COLOR_F) {
        let f = c.formats();
        if align == HorizontalAlignment::Left
            || c.measure(text, &f.body) + grid::TEXT_SLACK > rect.right - rect.left
        {
            c.text_ellipsis(text, rect, &f.body, ink);
        } else {
            c.text_aligned(text, rect, &f.body, ink, align.dwrite());
        }
    }

    /// The column header band: `bg-surface-1` with a `border-b`, one
    /// `font-medium text-text-secondary` label per column at body size, and the
    /// sort indicator.
    fn paint_header(&self, c: &dyn Canvas, l: &TableLayout, cols: &[ColumnHeader], xs: &[f32]) {
        if l.header.bottom <= l.header.top {
            return;
        }
        let t = c.theme();
        let f = c.formats();
        c.fill_rounded(&l.header, 0.0, &t.card_background);
        // The header scrolls sideways with the rows but never vertically — it
        // is its own band, so it stays put (sticky) over a scrolled body.
        c.push_clip(&l.header);
        let (content_left, _) = self.content_span(l.frame);

        if self.selectable {
            let cell = Rect::new(
                content_left + grid::SELECT_PAD_X,
                l.header.top,
                content_left + grid::SELECT_COL,
                l.header.bottom,
            );
            self.paint_check(
                c,
                cell,
                self.select_all_state().check_state(),
                self.hot_chrome == Some(Chrome::SelectAll),
                self.ring_on(FocusPart::Chrome(Chrome::SelectAll)),
            );
        }

        let sorted = self.sorted_column();
        for (i, col) in cols.iter().enumerate() {
            let cell = Rect::new(
                xs[i] + grid::CELL_PAD_X,
                l.header.top,
                (xs[i + 1] - grid::CELL_PAD_X).max(xs[i] + grid::CELL_PAD_X),
                l.header.bottom,
            );
            let hot = self.hot_chrome == Some(Chrome::Header(i));
            // « hover:text-text-primary » on the sort button; a plain header
            // stays `text-text-secondary`.
            let colour = if hot && Self::sortable(cols, i) { &t.text_primary } else { &t.text_secondary };
            let label_w = c.measure(&col.text, &f.body_strong);

            let glyph = if sorted == Some(i) {
                Some(if self.list.sorting == SortOrder::Ascending { "ArrowUp" } else { "ArrowDown" })
            } else if Self::sortable(cols, i) && hot {
                // « opacity-0 transition-opacity group-hover:opacity-60 »: the
                // hint exists only under the pointer.
                Some("ChevronsUpDown")
            } else {
                None
            };
            let group = label_w + glyph.map(|_| grid::SORT_GAP + grid::SORT_GLYPH).unwrap_or(0.0);

            // The `<th>`'s `text-align` cannot reach inside the sort button, so
            // the web restates it as an automatic margin; here the whole group
            // is simply placed, which is the same thing without the CSS.
            let left = match col.text_align {
                HorizontalAlignment::Left => cell.left,
                HorizontalAlignment::Right => (cell.right - group).max(cell.left),
                HorizontalAlignment::Center => {
                    (cell.left + (cell.right - cell.left - group) / 2.0).max(cell.left)
                }
            };
            // The label's box runs to the glyph, or to the cell's edge when
            // there is none — **not** to `left + label_w`. A rectangle exactly
            // as wide as the measured text ellipsises it: DirectWrite lays the
            // run out again at draw time and a fraction of a DIP of difference
            // is enough to trim the last letter.
            let gx = glyph
                .map(|_| (left + label_w + grid::SORT_GAP).min(cell.right - grid::SORT_GLYPH));
            let label_rect = Rect::new(
                left,
                cell.top,
                gx.unwrap_or(cell.right).max(left),
                cell.bottom,
            );
            c.text_ellipsis(&col.text, &label_rect, &f.body_strong, colour);

            if let (Some(name), Some(gx)) = (glyph, gx) {
                let gr = Rect::new(gx, cell.top, gx + grid::SORT_GLYPH, cell.bottom);
                let ink = if sorted == Some(i) {
                    t.accent
                } else {
                    fade(t.text_secondary, grid::SORT_HINT_ALPHA)
                };
                c.vector_icon(name, &gr, grid::SORT_GLYPH, &ink);
            }

            // The sort button's ring: `rounded-sm px-1 py-0.5` around the
            // label and its glyph, pulled flush by `-mx-1`.
            if Self::sortable(cols, i) && self.ring_on(FocusPart::Chrome(Chrome::Header(i))) {
                let right = gx.map(|g| g + grid::SORT_GLYPH).unwrap_or(left + label_w).min(cell.right);
                let cy = (cell.top + cell.bottom) / 2.0;
                let half = grid::LINE / 2.0 + grid::SORT_BUTTON_PAD_Y;
                let button = Rect::new(
                    left - grid::SORT_BUTTON_PAD_X,
                    cy - half,
                    right + grid::SORT_BUTTON_PAD_X,
                    cy + half,
                );
                Self::paint_ring(c, button, radius::SM);
            }
        }

        // The resize grips: an 8 DIP strip whose only visible part is a 1 DIP
        // rule — `hover:after:bg-border-strong`, `focus-visible:after:bg-primary`
        // — so the header is not a permanent grid of lines.
        if self.can_resize(l) {
            for i in 0..cols.len() {
                let Some(grip) = self.resize_grip(l, i) else { continue };
                let dragging = self.resize.is_some_and(|d| d.column == i);
                let ink = if self.ring_on(FocusPart::Chrome(Chrome::ResizeHandle(i))) || dragging {
                    t.accent
                } else if self.hot_chrome == Some(Chrome::ResizeHandle(i)) {
                    t.border_strong
                } else {
                    continue;
                };
                let cx = (grip.left + grip.right) / 2.0;
                let rule = Rect::new(
                    cx - grid::RESIZE_RULE / 2.0,
                    grip.top + grid::RESIZE_RULE_INSET,
                    cx + grid::RESIZE_RULE / 2.0,
                    grip.bottom - grid::RESIZE_RULE_INSET,
                );
                c.fill_rounded(&rule, 0.0, &ink);
            }
        }
        c.pop_clip();

        let rule = Rect::new(l.header.left, l.header.bottom - 1.0, l.header.right, l.header.bottom);
        c.fill_rounded(&rule, 0.0, &t.card_stroke);
    }

    /// The rows — **only the visible range**, clipped to the body.
    fn paint_rows(
        &self,
        c: &dyn Canvas,
        l: &TableLayout,
        cols: &[ColumnHeader],
        xs: &[f32],
        state: WidgetState,
    ) {
        let t = c.theme();
        let page = self.page_range();
        let ink = if state.disabled { &t.text_tertiary } else { &t.text_primary };
        // Each visible column's MODEL index — what `item.cell` reads. The two
        // differ as soon as a column is hidden.
        let models: Vec<usize> =
            (0..cols.len()).map(|i| self.model_column(cols, i).unwrap_or(i)).collect();
        let rows_ring = self.ring_on(FocusPart::Rows);

        c.push_clip(&l.body);
        let owner = (self.owner_draw_cells && crate::graphics::owner_draw::has_handler()).then(|| crate::graphics::Graphics::new(c));
        for i in self.visible_rows_in(l) {
            let Some(item) = self.list.items.get(i) else {
                continue;
            };
            let row = self.row_rect_in(l, i);

            // « Zebra on surface-1, hover one step further on surface-2. The two
            // must not share a tone. Selection outranks both. Tint only, never a
            // left accent bar. »
            if item.selected {
                c.fill_rounded(&row, 0.0, &t.accent_light);
            } else if self.list.hot_index == Some(i) {
                c.fill_rounded(&row, 0.0, &t.surface_2);
            } else if (i - page.start) % 2 == 1 {
                c.fill_rounded(&row, 0.0, &t.card_background);
            }
            // « border-b border-border … last:border-0 ».
            if i + 1 < page.end {
                let rule = Rect::new(row.left, row.bottom - 1.0, row.right, row.bottom);
                c.fill_rounded(&rule, 0.0, &t.card_stroke);
            }

            if self.selectable {
                let cell = Rect::new(
                    row.left + grid::SELECT_PAD_X,
                    row.top,
                    row.left + grid::SELECT_COL,
                    row.bottom,
                );
                let check =
                    if item.selected { CheckState::Checked } else { CheckState::Unchecked };
                self.paint_check(c, cell, check, self.hot_chrome == Some(Chrome::RowCheck(i)), false);
            }

            let hot = self.list.hot_index == Some(i);
            for (col, header) in cols.iter().enumerate() {
                let cell = Rect::new(
                    xs[col] + grid::CELL_PAD_X,
                    row.top,
                    (xs[col + 1] - grid::CELL_PAD_X).max(xs[col] + grid::CELL_PAD_X),
                    row.bottom,
                );
                let model = models[col];
                // The cell being edited shows its editor instead (painted after the rows).
                if self.editor.as_ref().is_some_and(|e| e.row == i && e.column == col) {
                    continue;
                }
                if let Some(g) = &owner {
                    use crate::graphics::owner_draw::{self, DrawItemEventArgs, DrawItemState};
                    let st = DrawItemState::NONE
                        .with(DrawItemState::SELECTED, item.selected)
                        .with(DrawItemState::HOT_LIGHT, hot)
                        .with(DrawItemState::FOCUS, self.cursor == Some(i) && rows_ring)
                        .with(DrawItemState::DISABLED, state.disabled);
                    let mut e = DrawItemEventArgs::new(g, "DataTable", Some(i), cell, st, item.cell(model));
                    e.sub_index = Some(model);
                    e.back_color = crate::graphics::Color::TRANSPARENT;
                    c.push_clip(&Rect::new(xs[col], row.top, xs[col + 1], row.bottom));
                    let done = owner_draw::draw_item(&mut e);
                    c.pop_clip();
                    if done {
                        continue;
                    }
                }
                // The column's own renderer first — the web's `col.cell(row)`.
                // It paints inside the cell's clip: a badge wider than its
                // column is cut at the column's edge, not over the next one.
                if let Some(painter) = &self.cell_painter {
                    let info = Cell {
                        text: item.cell(model),
                        row: i,
                        column: model,
                        rect: cell,
                        selected: item.selected,
                        hot,
                        disabled: state.disabled,
                    };
                    c.push_clip(&Rect::new(xs[col], row.top, xs[col + 1], row.bottom));
                    let done = painter(c, &info);
                    c.pop_clip();
                    if done {
                        continue;
                    }
                }
                // Every cell is body-sized and `text-text-primary`, aligned per
                // column — column 0 included, unlike a file list.
                Self::cell_text_in(c, item.cell(model), &cell, header.text_align, ink);
            }
            // The error glyphs (WinForms `ErrorText`) over the cells' text, not over an editor.
            if !self.cell_errors.is_empty() {
                for (col, &model) in models.iter().enumerate() {
                    let editing = self.editor.as_ref().is_some_and(|e| e.row == i && e.column == col);
                    if !editing && self.cell_errors.iter().any(|e| e.row == i && e.column == model) {
                        Self::paint_error_glyph(c, Rect::new(xs[col], row.top, xs[col + 1], row.bottom));
                    }
                }
            }

            if self.row_actions {
                // `h-7 w-7 rounded-md text-text-secondary hover:bg-surface-2
                // hover:text-text-primary`. Drawn whenever the column is on,
                // as it always was here, whether or not entries are listed in
                // `row_menu_actions` (a caller may own its menu).
                let cy = (row.top + row.bottom) / 2.0;
                let bx = Rect::new(
                    row.right - grid::ACTIONS_PAD_X - grid::ICON_BUTTON,
                    cy - grid::ICON_BUTTON / 2.0,
                    row.right - grid::ACTIONS_PAD_X,
                    cy + grid::ICON_BUTTON / 2.0,
                );
                let menu_hot = self.hot_chrome == Some(Chrome::RowMenu(i));
                if menu_hot {
                    c.fill_rounded(&bx, radius::SM, &t.surface_2);
                }
                let glyph = if menu_hot { &t.text_primary } else { &t.text_secondary };
                c.vector_icon("MoreVertical", &bx, grid::ICON_GLYPH, glyph);
            }

            // An editable table rings its current CELL instead of the row (the editor, when
            // there is one, shows the focus itself).
            if self.editable && rows_ring && self.cursor == Some(i) {
                if self.editor.is_none() {
                    let column = self.current_column.min(cols.len().saturating_sub(1));
                    if column + 1 < xs.len() {
                        self.paint_current_cell(c, l.body, Rect::new(xs[column], row.top, xs[column + 1], row.bottom));
                    }
                }
            } else if rows_ring && self.cursor == Some(i) {
                // The keyboard cursor: an inset accent ring on the row, only while
                // the rows hold a VISIBLE focus (a click does not draw it).
                // On the VISIBLE part of the band: a row scrolled sideways
                // must still show its whole outline.
                let inset = grid::FOCUS_RING / 2.0;
                let (left, right) = (row.left.max(l.body.left), row.right.min(l.body.right));
                let r = Rect::new(left + inset, row.top + inset, right - inset, row.bottom - inset);
                c.stroke_rounded_w(&r, radius::SM, &t.accent, grid::FOCUS_RING);
            }
        }
        // The cell editor over its cell, inside the body's clip.
        if let Some(e) = &self.editor {
            if self.visible_rows_in(l).contains(&e.row) && e.column + 1 < xs.len() {
                let row = self.row_rect_in(l, e.row);
                let rect = Rect::new(xs[e.column], row.top, xs[e.column + 1], row.bottom).inflate(-2.0, -2.0);
                self.paint_editor(c, rect);
            }
        }
        c.pop_clip();

        // The rows hold the focus but the cursor is on another page or none
        // is set yet: ring the body so the focus is never invisible.
        if rows_ring && !self.cursor.is_some_and(|cur| page.contains(&cur)) {
            let inset = grid::FOCUS_RING / 2.0;
            let r = Rect::new(l.body.left + inset, l.body.top + inset, l.body.right - inset, l.body.bottom - inset);
            c.stroke_rounded_w(&r, radius::SM, &t.accent, grid::FOCUS_RING);
        }
    }

    /// The scroll bars over the body — WinUI's thin indicator that unfolds
    /// under the pointer, skinned as the web's `::-webkit-scrollbar`.
    fn paint_scrollbars(&self, c: &dyn Canvas, l: &TableLayout) {
        for horizontal in [false, true] {
            let hot = self.hot_chrome == Some(Chrome::ScrollBar { horizontal })
                || self.scroll_drag.is_some_and(|d| d.horizontal == horizontal);
            if let Some(bar) = self.scrollbar_in(l, horizontal, hot) {
                sb::draw(c, &bar, 1.0, hot);
            }
        }
    }

    /// The narrow layout: one card per row.
    fn paint_cards(&self, c: &dyn Canvas, l: &TableLayout, state: WidgetState) {
        let t = c.theme();
        let f = c.formats();
        let (primary, pairs) = self.card_columns();
        let ink = if state.disabled { &t.text_tertiary } else { &t.text_primary };
        // The MODEL index of a column — what `item.cell` reads, stable when a
        // column before it is hidden.
        let index_of =
            |col: &ColumnHeader| self.list.columns.iter().position(|c| c.name == col.name).unwrap_or(0);

        c.push_clip(&l.body);
        for (n, i) in self.page_range().enumerate() {
            let Some(item) = self.list.items.get(i) else {
                continue;
            };
            let card = self.card_rect(l, n);
            if card.top > l.body.bottom {
                break;
            }
            // « border-primary bg-primary-light » when selected, else
            // « border-border bg-surface-0 ».
            let (fill, stroke) = if item.selected {
                (&t.accent_light, &t.accent)
            } else {
                (&t.layer_background, &t.card_stroke)
            };
            c.fill_rounded(&card, radius::LG, fill);
            c.stroke_rounded_w(&card, radius::LG, stroke, grid::CARD_BORDER);

            let mut x = card.left + grid::CARD_PAD;
            let head = Rect::new(x, card.top + grid::CARD_PAD, card.right - grid::CARD_PAD, card.top + grid::CARD_PAD + grid::LINE);
            if self.selectable {
                let check = if item.selected { CheckState::Checked } else { CheckState::Unchecked };
                self.paint_check(c, Rect::new(x, head.top, x + control::CHECK_BOX, head.bottom), check, false, false);
                x += control::CHECK_BOX + grid::CARD_HEAD_GAP;
            }
            // The overflow control is always drawn: on a card it carries the
            // copy entries even when the row has no action of its own.
            let menu = Rect::new(
                card.right - grid::CARD_PAD - grid::ICON_BUTTON,
                head.top,
                card.right - grid::CARD_PAD,
                head.bottom,
            );
            c.vector_icon("MoreVertical", &menu, grid::OVERFLOW_GLYPH, &t.text_secondary);

            if let Some(col) = &primary {
                let title = Rect::new(x, head.top, menu.left - grid::CARD_HEAD_GAP, head.bottom);
                c.text_ellipsis(item.cell(index_of(col)), &title, &f.body_strong, ink);
            }

            // The `<dl>`: a tertiary label column and a primary value column.
            let label_w = pairs
                .iter()
                .map(|col| c.measure(column_label(col), &f.caption))
                .fold(0.0f32, f32::max);
            let mut y = card.top + grid::CARD_PAD + grid::CARD_HEAD + grid::CARD_DL_TOP;
            for col in &pairs {
                let dt = Rect::new(card.left + grid::CARD_PAD, y, card.left + grid::CARD_PAD + label_w, y + grid::META_LINE);
                let dd = Rect::new(dt.right + grid::CARD_DL_GAP_X, y, card.right - grid::CARD_PAD, y + grid::META_LINE);
                c.text_ellipsis(column_label(col), &dt, &f.caption, &t.text_tertiary);
                let model = index_of(col);
                let info = Cell {
                    text: item.cell(model),
                    row: i,
                    column: model,
                    rect: dd,
                    selected: item.selected,
                    hot: self.list.hot_index == Some(i),
                    disabled: state.disabled,
                };
                let painted = self.cell_painter.as_ref().is_some_and(|p| {
                    c.push_clip(&dd);
                    let done = p(c, &info);
                    c.pop_clip();
                    done
                });
                if !painted {
                    c.text_ellipsis(item.cell(model), &dd, &f.caption, ink);
                }
                y += grid::META_LINE + grid::CARD_DL_GAP_Y;
            }

            if self.ring_on(FocusPart::Rows) && self.cursor == Some(i) {
                Self::paint_ring(c, card, radius::LG);
            }
        }
        c.pop_clip();
    }

    /// The loading state — a skeleton, not a spinner: « a table with these
    /// columns is arriving here », so the layout does not jump.
    fn paint_skeleton(&self, c: &dyn Canvas, l: &TableLayout, columns: usize) {
        let t = c.theme();
        let rh = self.skeleton_row();
        // « Deterministic pseudo-widths: a random() would reshuffle on every
        // re-render and make the placeholder shimmer sideways. »
        let width = |row: usize, col: usize| 45.0 + ((row * 7 + col * 23) % 5) as f32 * 11.0;

        c.push_clip(&l.body);
        for r in 0..self.skeleton_rows {
            let top = l.body.top + r as f32 * rh;
            if top >= l.body.bottom {
                break;
            }
            if r > 0 {
                let rule = Rect::new(l.body.left, top, l.body.right, top + 1.0);
                c.fill_rounded(&rule, 0.0, &t.card_stroke);
            }
            let cy = top + rh / 2.0;
            let mut x = l.body.left + grid::SKELETON_PAD_X;
            if self.selectable {
                let b = control::CHECK_BOX;
                c.fill_rounded(&Rect::new(x, cy - b / 2.0, x + b, cy + b / 2.0), radius::SM, &t.surface_2);
                x += b + grid::SKELETON_GAP;
            }
            let right = l.body.right - grid::SKELETON_PAD_X;
            let each =
                ((right - x) - grid::SKELETON_GAP * columns.saturating_sub(1) as f32) / columns as f32;
            for col in 0..columns {
                let cell_left = x + col as f32 * (each + grid::SKELETON_GAP);
                // « max-width: {width}% » on a `flex-1` bar.
                let w = each * width(r, col) / 100.0;
                let bar = Rect::new(
                    cell_left,
                    cy - grid::SKELETON_BAR / 2.0,
                    cell_left + w.max(0.0),
                    cy + grid::SKELETON_BAR / 2.0,
                );
                c.fill_rounded(&bar, radius::SM, &t.surface_2);
            }
        }
        c.pop_clip();
    }

    /// One of the three empty states, painted as `EmptyState.tsx` lays it out.
    fn paint_empty(&self, c: &dyn Canvas, l: &TableLayout, kind: EmptyKind) {
        let t = c.theme();
        let f = c.formats();
        let (copy, glyph, medallion, ink) = match kind {
            EmptyKind::Error => (&self.copy.error, "AlertCircle", t.danger_light, t.danger),
            EmptyKind::NoResults => {
                (&self.copy.no_results, "SearchX", t.surface_2, t.text_secondary)
            }
            EmptyKind::FirstUse => (&self.copy.empty, "Inbox", t.accent_light, t.accent),
        };
        let description = match (kind, &self.error) {
            // « typeof error === 'boolean' ? default : error » — a caller's
            // message replaces the generic one.
            (EmptyKind::Error, Some(msg)) if !msg.is_empty() => msg.as_str(),
            _ => copy.description.as_str(),
        };

        let action = copy.action.as_ref().map(|label| {
            Button::new(label).variant(Variant::Secondary).size(ButtonSize::Sm)
        });
        let action_h = action.as_ref().map(|_| grid::EMPTY_GAP + height::BUTTON_SM).unwrap_or(0.0);
        // The description wraps inside the `max-w-sm` block, as the web's
        // paragraph does: a narrow table must not clip it at both ends.
        let text_w = (l.body.right - l.body.left - grid::EMPTY_PAD_X * 2.0).min(grid::EMPTY_TEXT_MAX);
        let desc_lines = crate::feedback::wrap_lines(description, text_w.max(1.0), |s| c.measure(s, &f.body));
        let total = grid::MEDALLION
            + grid::EMPTY_GAP
            + grid::LINE
            + grid::EMPTY_TEXT_GAP
            + grid::LINE * desc_lines.len().max(1) as f32
            + action_h;

        let cx = (l.body.left + l.body.right) / 2.0;
        let mut y = (l.body.top + (l.body.bottom - l.body.top - total) / 2.0)
            .max(l.body.top + grid::EMPTY_PAD_Y.min((l.body.bottom - l.body.top) / 4.0));

        c.push_clip(&l.body);
        let med = Rect::new(
            cx - grid::MEDALLION / 2.0,
            y,
            cx + grid::MEDALLION / 2.0,
            y + grid::MEDALLION,
        );
        c.fill_rounded(&med, pill(grid::MEDALLION), &medallion);
        c.vector_icon(glyph, &med, grid::MEDALLION_GLYPH, &ink);
        y = med.bottom + grid::EMPTY_GAP;

        let block = Rect::new(cx - text_w / 2.0, y, cx + text_w / 2.0, y + grid::LINE);
        c.text(&copy.title, &block, &f.heading, &t.text_primary, true);
        y = block.bottom + grid::EMPTY_TEXT_GAP;
        for line in &desc_lines {
            let desc = Rect::new(block.left, y, block.right, y + grid::LINE);
            c.text(line, &desc, &f.body, &t.text_secondary, true);
            y = desc.bottom;
        }
        y += grid::EMPTY_GAP;

        if let Some(button) = action {
            let w = button.width(c);
            let rect = Rect::new(cx - w / 2.0, y, cx + w / 2.0, y + height::BUTTON_SM);
            button.paint(c, rect, WidgetState::REST);
        }
        c.pop_clip();
    }

    /// The toolbar's **selection face**: « N en sélection », the bulk actions,
    /// and a way out. It replaces the idle face rather than stacking under it.
    fn paint_bulk_bar(&self, c: &dyn Canvas, l: &TableLayout) {
        let t = c.theme();
        let f = c.formats();
        c.fill_rounded(&l.toolbar, radius::LG, &t.accent_light);

        let bar = self.bulk_bar_of(c, l);
        if self.hot_chrome == Some(Chrome::ClearSelection) {
            c.fill_rounded(&bar.close, radius::SM, &fade(t.text_primary, 0.08));
        }
        c.vector_icon("X", &bar.close, grid::CLOSE_GLYPH, &t.accent);

        c.text_ellipsis(&self.bulk_label(), &bar.label, &f.body, &t.accent);

        for (i, rect) in bar.actions.iter().enumerate() {
            let Some(action) = self.bulk_actions.get(i) else {
                continue;
            };
            let mut button = action.button();
            if l.mode == Mode::Cards {
                // Icon only in a narrow bar — the web passes `null` as the child.
                button = Button::new("")
                    .variant(if action.danger { Variant::Danger } else { Variant::Secondary })
                    .size(ButtonSize::Sm);
                if let Some(icon) = action.icon {
                    button = button.icon(icon);
                }
            }
            let hot = self.hot_chrome == Some(Chrome::BulkAction(i));
            let ring = self.ring_on(FocusPart::Chrome(Chrome::BulkAction(i)));
            button.paint(c, *rect, WidgetState::REST.hot(hot).focused(ring).focus_visible(ring));
        }

        if let Some(o) = bar.overflow {
            if self.hot_chrome == Some(Chrome::BulkOverflow) {
                c.fill_rounded(&o, radius::SM, &fade(t.text_primary, 0.08));
            }
            c.vector_icon("MoreHorizontal", &o, grid::OVERFLOW_GLYPH, &t.accent);
            if self.ring_on(FocusPart::Chrome(Chrome::BulkOverflow)) {
                Self::paint_ring(c, o, radius::SM);
            }
        }
        if self.ring_on(FocusPart::Chrome(Chrome::ClearSelection)) {
            Self::paint_ring(c, bar.close, radius::SM);
        }
    }

    /// The toolbar's **idle face**: the title and the column chooser.
    fn paint_toolbar(&self, c: &dyn Canvas, l: &TableLayout) {
        let t = c.theme();
        let f = c.formats();
        let chooser = self.configurable_columns.then(|| self.chooser_rect(c, l));

        if !self.title.is_empty() {
            let right = chooser.map(|r| r.left - grid::TOOLBAR_GAP).unwrap_or(l.toolbar.right);
            let rect = Rect::new(l.toolbar.left, l.toolbar.top, right.max(l.toolbar.left), l.toolbar.bottom);
            c.text_ellipsis(&self.title, &rect, &f.heading, &t.text_primary);
        }

        if let Some(rect) = chooser {
            let hot = self.hot_chrome == Some(Chrome::Columns);
            c.fill_rounded(&rect, radius::SM, if hot { &t.card_background } else { &t.layer_background });
            c.stroke_rounded(&rect, radius::SM, &t.card_stroke);
            let ink = if hot { &t.text_primary } else { &t.text_secondary };
            let gx = rect.left + grid::CHOOSER_PAD_X;
            c.vector_icon(
                "Settings2",
                &Rect::new(gx, rect.top, gx + grid::CHOOSER_GLYPH, rect.bottom),
                grid::CHOOSER_GLYPH,
                ink,
            );
            if l.mode != Mode::Cards {
                let label = Rect::new(
                    gx + grid::CHOOSER_GLYPH + grid::CHOOSER_GAP,
                    rect.top,
                    rect.right - grid::CHOOSER_PAD_X,
                    rect.bottom,
                );
                c.text_ellipsis(&self.copy.columns, &label, &f.body, ink);
            }
            if self.ring_on(FocusPart::Chrome(Chrome::Columns)) {
                Self::paint_ring(c, rect, radius::SM);
            }
        }
    }

    /// The pagination band — always inside the table's own footer.
    fn paint_footer(&self, c: &dyn Canvas, l: &TableLayout) {
        if l.footer.bottom <= l.footer.top {
            return;
        }
        let t = c.theme();
        let f = c.formats();
        c.fill_rounded(&l.footer, 0.0, &t.card_background);
        let rule = Rect::new(l.footer.left, l.footer.top, l.footer.right, l.footer.top + 1.0);
        c.fill_rounded(&rule, 0.0, &t.card_stroke);

        let mut left = l.footer.left + grid::FOOTER_PAD_X;

        if let Some((group, pills)) = self.size_group_of(c, l) {
            let label = Rect::new(left, l.footer.top, group.left - grid::SIZE_LABEL_GAP, l.footer.bottom);
            c.text_ellipsis(&self.copy.rows_per_page, &label, &f.body, &t.text_secondary);
            c.fill_rounded(&group, radius::SM, &t.surface_2);
            for (n, rect) in self.page_size_options.iter().zip(&pills) {
                let current = *n == self.page_size;
                if current {
                    c.fill_rounded(rect, radius::SM, &t.layer_background);
                }
                let ink = if current {
                    &t.accent
                } else if self.hot_chrome == Some(Chrome::PageSize(*n)) {
                    &t.text_primary
                } else {
                    &t.text_secondary
                };
                c.text(&n.to_string(), rect, &f.body, ink, true);
                if self.ring_on(FocusPart::Chrome(Chrome::PageSize(*n))) {
                    Self::paint_ring(c, *rect, radius::SM);
                }
            }
            left = group.right + grid::FOOTER_CLUSTER_GAP;
        }

        // The counter stops before the navigation cluster, so a narrow footer
        // ellipsises it rather than writing it under the chevrons.
        let nav = self.nav_cluster_of(c, l);
        let nav_left = nav.first().map(|(_, r)| r.left - grid::FOOTER_CLUSTER_GAP).unwrap_or(l.footer.right);
        let counter_rect = Rect::new(left, l.footer.top, nav_left.max(left), l.footer.bottom);
        c.text_ellipsis(&self.counter_text(), &counter_rect, &f.body, &t.text_secondary);

        for (chrome, rect) in nav {
            // « disabled:pointer-events-none disabled:opacity-40 ».
            let disabled = self.nav_disabled(chrome);
            if !disabled && self.ring_on(FocusPart::Chrome(chrome)) {
                Self::paint_ring(c, rect, radius::SM);
            }
            let glyph = match chrome {
                Chrome::FirstPage => "ChevronsLeft",
                Chrome::PrevPage => "ChevronLeft",
                Chrome::NextPage => "ChevronRight",
                _ => "ChevronsRight",
            };
            if !disabled && self.hot_chrome == Some(chrome) {
                c.fill_rounded(&rect, radius::SM, &t.surface_2);
            }
            // `hover:text-text-primary` over `hover:bg-surface-2`.
            let ink = if disabled {
                fade(t.text_secondary, 0.4)
            } else if self.hot_chrome == Some(chrome) {
                t.text_primary
            } else {
                t.text_secondary
            };
            c.vector_icon(glyph, &rect, grid::ICON_GLYPH, &ink);
        }
        let label = self.page_label();
        let w = c.measure(&label, &f.body) + grid::TEXT_SLACK;
        c.text(&label, &nav_label_rect(l.footer, w, l.mode == Mode::Cards), &f.body, &t.text_secondary, true);
    }
}

impl Deref for DataTable {
    type Target = ListView;
    fn deref(&self) -> &ListView {
        &self.list
    }
}
impl DerefMut for DataTable {
    fn deref_mut(&mut self) -> &mut ListView {
        &mut self.list
    }
}

impl Widget for DataTable {
    fn model(&self) -> &dyn Control {
        self.list.model()
    }

    /// The declared column widths (plus the two fixed columns) by the height
    /// every band adds up to — never less than [`DataTable::min_table_width`],
    /// which is what the web pins its table at before it starts scrolling.
    fn measure(&self, _canvas: &dyn Canvas) -> Size {
        let rh = self.list.row_height();
        let body = if self.loading {
            self.skeleton_rows as f32 * self.skeleton_row()
        } else if self.is_empty() {
            grid::EMPTY_PAD_Y * 2.0 + grid::MEDALLION + grid::EMPTY_GAP + grid::LINE * 2.0
        } else {
            // The header band plus the page's rows.
            rh + self.page_range().len() as f32 * rh
        };
        let toolbar = self.toolbar_height();
        Size::new(
            self.table_width().max(self.min_table_width),
            toolbar + if toolbar > 0.0 { grid::TOOLBAR_GAP } else { 0.0 } + body + self.footer_height(),
        )
    }

    /// Paints into the `bounds` **argument** — never the model's own rectangle.
    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        let l = self.layout_of(bounds);

        if l.toolbar.bottom > l.toolbar.top {
            if self.selection_mode() {
                self.paint_bulk_bar(canvas, &l);
            } else {
                self.paint_toolbar(canvas, &l);
            }
        }

        // « rounded-xl border border-border bg-surface-0 », and everything
        // inside it clipped to that shape.
        canvas.fill_rounded(&l.frame, radius::XL, &t.layer_background);
        canvas.push_clip_rounded(&l.frame, radius::XL);

        let cols = self.visible_columns();
        let xs = self.offsets_of(l.frame, &cols);

        if self.loading {
            self.paint_skeleton(canvas, &l, cols.len().max(1));
        } else if let Some(kind) = self.empty_kind() {
            self.paint_empty(canvas, &l, kind);
        } else if l.mode == Mode::Cards {
            self.paint_cards(canvas, &l, state);
            self.paint_scrollbars(canvas, &l);
        } else {
            self.paint_header(canvas, &l, &cols, &xs);
            self.paint_rows(canvas, &l, &cols, &xs, state);
            self.paint_scrollbars(canvas, &l);
        }
        self.paint_footer(canvas, &l);

        canvas.pop_clip_rounded();
        canvas.stroke_rounded(&l.frame, radius::XL, &t.card_stroke);
    }

    fn type_name(&self) -> &'static str {
        "DataTable"
    }
}

// =============================================================================
//  Tests — pure geometry and pure state, no device needed.
// =============================================================================

#[cfg(test)]
impl DataTable {
    /// [`DataTable::chrome_at`] for the parts that need no text measure.
    fn chrome_at_pure(&self, bounds: Rect, x: f32, y: f32) -> Option<Chrome> {
        self.resize_handle_at(bounds, x, y).map(Chrome::ResizeHandle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four columns, the first sortable and primary, the last right-aligned.
    fn table(rows: usize) -> DataTable {
        let mut t = DataTable::new();
        t.selectable = true;
        t.columns = vec![
            with_flag(with_flag(column("name", "Nom", 300), flags::SORTABLE), flags::PRIMARY),
            with_flag(column("role", "Rôle", 140), flags::SORTABLE),
            column("created", "Créé le", 150),
            aligned(column("size", "Taille", 100), HorizontalAlignment::Right),
        ];
        t.items = (0..rows)
            .map(|i| {
                ListViewItem::new(format!("ligne {i:05}"))
                    .with_sub("Membre")
                    .with_sub("hier")
                    .with_sub("48 Ko")
            })
            .collect();
        t
    }

    /// Wide enough to stay a table, tall enough for a screenful of rows.
    fn bounds() -> Rect {
        Rect::new(0.0, 0.0, 900.0, 500.0)
    }

    // ── The cards / table threshold ──────────────────────────────────────

    #[test]
    fn the_cards_threshold_is_strict_and_only_applies_to_auto() {
        // `width < cardsBelow` — a container exactly at the threshold is still
        // a table.
        assert_eq!(layout_mode(Layout::Auto, 700.0, 700.0), Mode::Table);
        assert_eq!(layout_mode(Layout::Auto, 699.99, 700.0), Mode::Cards);
        assert_eq!(layout_mode(Layout::Auto, 1200.0, 700.0), Mode::Table);
        // A container that genuinely measures 0 is honoured, not treated as
        // wide — that is the bug the web's `width !== null` guard exists for.
        assert_eq!(layout_mode(Layout::Auto, 0.0, 700.0), Mode::Cards);
        // A forced layout ignores the width in both directions.
        assert_eq!(layout_mode(Layout::Table, 10.0, 700.0), Mode::Table);
        assert_eq!(layout_mode(Layout::Cards, 4000.0, 700.0), Mode::Cards);
        // And the threshold is a value, not a constant.
        assert_eq!(layout_mode(Layout::Auto, 500.0, 400.0), Mode::Table);
    }

    #[test]
    fn the_table_follows_the_box_it_was_given_not_the_screen() {
        let t = table(10);
        assert_eq!(t.layout_of(Rect::new(0.0, 0.0, 900.0, 400.0)).mode, Mode::Table);
        assert_eq!(t.layout_of(Rect::new(0.0, 0.0, 380.0, 400.0)).mode, Mode::Cards);
    }

    // ── Row and column hit tests, at the edges ───────────────────────────

    #[test]
    fn a_row_starts_at_its_own_top_edge_and_ends_before_the_next() {
        let t = table(10);
        let b = bounds();
        let l = t.layout_of(b);
        let top = l.body.top;
        assert_eq!(l.header.bottom - l.header.top, grid::ROW);
        assert_eq!(t.row_at(b, 100.0, top), Some(0));
        assert_eq!(t.row_at(b, 100.0, top + grid::ROW - 0.001), Some(0));
        assert_eq!(t.row_at(b, 100.0, top + grid::ROW), Some(1));
        // The header band is not a row, and neither is the footer.
        assert_eq!(t.row_at(b, 100.0, l.header.top), None);
        assert_eq!(t.row_at(b, 100.0, top - 0.001), None);
        assert_eq!(t.row_at(b, 100.0, l.footer.top + 1.0), None);
    }

    #[test]
    fn nothing_is_hit_past_the_last_row_of_the_page() {
        let mut t = table(3);
        t.page_size = 2;
        let b = bounds();
        let top = t.layout_of(b).body.top;
        assert_eq!(t.row_at(b, 100.0, top + grid::ROW), Some(1));
        // Row 2 exists in the model but is on page 2 — the band under row 1 is
        // empty, not row 2.
        assert_eq!(t.row_at(b, 100.0, top + 2.0 * grid::ROW), None);
        t.page = 1;
        assert_eq!(t.row_at(b, 100.0, top), Some(2));
        assert_eq!(t.row_at(b, 100.0, top + grid::ROW), None);
    }

    #[test]
    fn a_column_owns_its_left_edge_and_not_the_next_ones() {
        let t = table(3);
        let b = bounds();
        let l = t.layout_of(b);
        let xs = t.column_x_offsets(l.frame);
        // The selection column is 40 wide and comes before column 0.
        assert_eq!(xs, vec![40.0, 340.0, 480.0, 630.0, 730.0]);
        assert_eq!(t.column_at(l.frame, 40.0), Some(0));
        assert_eq!(t.column_at(l.frame, 39.999), None, "the selection column is not a column");
        assert_eq!(t.column_at(l.frame, 339.999), Some(0));
        assert_eq!(t.column_at(l.frame, 340.0), Some(1));
        assert_eq!(t.column_at(l.frame, 729.999), Some(3));
        assert_eq!(t.column_at(l.frame, 730.0), None, "past the last column there is none");
    }

    #[test]
    fn hiding_a_column_takes_it_out_of_the_geometry_but_not_out_of_the_sort() {
        let mut t = table(3);
        t.list.sort_column = 3; // « Taille », the model's index
        t.list.sorting = SortOrder::Ascending;
        assert_eq!(t.sorted_column(), Some(3));
        t.hidden_columns = vec!["role".into()];
        let cols = t.visible_columns();
        assert_eq!(cols.len(), 3);
        assert_eq!(t.column_x_offsets(t.layout_of(bounds()).frame), vec![40.0, 340.0, 490.0, 590.0]);
        // The sort is stored against the MODEL's column, so hiding one before
        // it does not move the indicator onto a different column.
        assert_eq!(t.sorted_column(), Some(2));
    }

    #[test]
    fn the_header_band_answers_only_inside_itself() {
        let t = table(3);
        let b = bounds();
        let l = t.layout_of(b);
        assert_eq!(t.header_at(b, 400.0, l.header.top), Some(1));
        assert_eq!(t.header_at(b, 400.0, l.header.bottom - 0.001), Some(1));
        assert_eq!(t.header_at(b, 400.0, l.header.bottom), None);
    }

    // ── The « select all » box in its three states ───────────────────────

    #[test]
    fn the_select_all_box_reads_none_partial_and_all() {
        let mut t = table(6);
        t.page_size = 0; // one page of everything
        assert_eq!(t.select_all_state(), SelectAll::None);
        assert_eq!(t.select_all_state().check_state(), CheckState::Unchecked);

        t.toggle_row(2);
        assert_eq!(t.select_all_state(), SelectAll::Some);
        assert_eq!(t.select_all_state().check_state(), CheckState::Indeterminate);

        for i in 0..6 {
            t.list.set_selected(i, true);
        }
        assert_eq!(t.select_all_state(), SelectAll::All);
        assert_eq!(t.select_all_state().check_state(), CheckState::Checked);
    }

    #[test]
    fn an_empty_page_reads_unchecked_and_a_click_on_it_does_nothing() {
        let mut t = table(0);
        t.page_size = 0;
        assert_eq!(t.select_all_state(), SelectAll::None);
        t.toggle_all();
        assert!(t.selected().is_empty());
    }

    #[test]
    fn clicking_the_select_all_box_selects_from_none_and_from_partial() {
        let mut t = table(6);
        t.page_size = 0;
        t.toggle_all();
        assert_eq!(t.selected(), vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(t.select_all_state(), SelectAll::All);
        // From « all » it clears the page…
        t.toggle_all();
        assert!(t.selected().is_empty());
        // …and from « partial » it SELECTS the page, it does not clear it.
        t.toggle_row(1);
        assert_eq!(t.select_all_state(), SelectAll::Some);
        t.toggle_all();
        assert_eq!(t.selected(), vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn select_all_covers_the_current_page_only() {
        let mut t = table(10);
        t.page_size = 4;
        t.toggle_row(0); // page 0
        t.page = 1;
        t.toggle_all(); // selects 4..8
        assert_eq!(t.selected(), vec![0, 4, 5, 6, 7]);
        assert_eq!(t.select_all_state(), SelectAll::All);
        // Un-selecting the page leaves the row selected on page 0 alone: a bulk
        // delete must never reach rows the user cannot see.
        t.toggle_all();
        assert_eq!(t.selected(), vec![0]);
        assert_eq!(t.select_all_state(), SelectAll::None);
    }

    // ── Pagination arithmetic ────────────────────────────────────────────

    #[test]
    fn the_first_page_of_a_full_table() {
        let mut t = table(100);
        t.page_size = 25;
        assert_eq!(t.page_count(), 4);
        assert_eq!(t.page_range(), 0..25);
        assert_eq!(t.page_counter(), (1, 25, 100));
    }

    #[test]
    fn the_last_page_is_partial_and_the_counter_stops_on_the_total() {
        let mut t = table(101);
        t.page_size = 25;
        assert_eq!(t.page_count(), 5);
        t.page = 4;
        assert_eq!(t.page_range(), 100..101);
        assert_eq!(t.page_counter(), (101, 101, 101));
    }

    #[test]
    fn a_single_page_still_counts_as_one() {
        let mut t = table(10);
        t.page_size = 25;
        assert_eq!(t.page_count(), 1);
        assert_eq!(t.page_range(), 0..10);
        assert_eq!(t.page_counter(), (1, 10, 10));
    }

    #[test]
    fn zero_rows_never_produces_a_zeroth_page() {
        let mut t = table(0);
        t.page_size = 25;
        // « Math.max(1, …) »: the counter reads « 1 / 1 », never « 1 / 0 ».
        assert_eq!(t.page_count(), 1);
        assert_eq!(t.page_range(), 0..0);
        assert_eq!(t.page_counter(), (0, 0, 0));
        assert_eq!(t.footer_height(), 0.0, "no footer over an empty body");
    }

    #[test]
    fn a_stranded_page_snaps_back_to_the_last_one() {
        let mut t = table(100);
        t.page_size = 25;
        t.page = 3;
        assert_eq!(t.page_index(), 3);
        // The filter shrinks the set under the user: the page no longer exists.
        t.items.truncate(30);
        assert_eq!(t.page_count(), 2);
        assert_eq!(t.page_index(), 1, "snapped back to the last page");
        assert_eq!(t.page_range(), 25..30);
        // But page 0 is never snapped — that is what `page > 0` guards.
        t.items.clear();
        t.page = 0;
        assert_eq!(t.page_index(), 0);
    }

    #[test]
    fn page_size_zero_disables_pagination_entirely() {
        let mut t = table(100);
        t.page_size = 0;
        assert!(!t.paginated());
        assert_eq!(t.page_count(), 1);
        assert_eq!(t.page_range(), 0..100);
        assert_eq!(t.footer_height(), 0.0);
    }

    #[test]
    fn changing_the_page_size_goes_back_to_the_first_page() {
        let mut t = table(100);
        t.page_size = 25;
        t.page = 3;
        t.set_page_size(50);
        assert_eq!(t.page, 0);
        assert_eq!(t.page_range(), 0..50);
    }

    #[test]
    fn server_side_pagination_takes_the_total_from_the_caller() {
        let mut t = table(25);
        t.manual_pagination = true;
        t.total_rows = Some(400);
        t.page_size = 25;
        t.page = 3;
        assert_eq!(t.page_count(), 16);
        // The rows handed over ARE the page: they are not sliced again.
        assert_eq!(t.page_range(), 0..25);
        assert_eq!(t.page_counter(), (76, 100, 400));
    }

    // ── The sort cycle ───────────────────────────────────────────────────

    #[test]
    fn a_sort_toggles_ascending_descending_then_none() {
        let mut t = table(3);
        t.manual_sort = true; // assert the indicator, not the ordering
        t.toggle_sort(0);
        assert_eq!(t.list.sorting, SortOrder::Ascending);
        assert_eq!(t.sorted_column(), Some(0));
        t.toggle_sort(0);
        assert_eq!(t.list.sorting, SortOrder::Descending);
        t.toggle_sort(0);
        assert_eq!(t.list.sorting, SortOrder::None);
        assert_eq!(t.sorted_column(), None, "no indicator once the cycle is back to none");
        t.toggle_sort(0);
        assert_eq!(t.list.sorting, SortOrder::Ascending, "and round it goes");
    }

    #[test]
    fn sorting_another_column_starts_it_ascending_whatever_the_previous_one_was() {
        let mut t = table(3);
        t.manual_sort = true;
        t.toggle_sort(0);
        t.toggle_sort(0); // descending on column 0
        t.toggle_sort(1);
        assert_eq!(t.list.sort_column, 1);
        assert_eq!(t.list.sorting, SortOrder::Ascending);
    }

    #[test]
    fn a_column_without_the_sortable_flag_never_sorts() {
        let mut t = table(3);
        t.toggle_sort(2); // « Créé le » carries no flag
        assert_eq!(t.list.sorting, SortOrder::None);
    }

    #[test]
    fn sorting_reorders_the_rows_through_the_replicas_own_sort() {
        let mut t = DataTable::new();
        t.columns = vec![
            with_flag(column("name", "Nom", 200), flags::SORTABLE),
            with_flag(column("role", "Rôle", 100), flags::SORTABLE),
        ];
        t.items = vec![
            ListViewItem::new("charlie").with_sub("b"),
            ListViewItem::new("alpha").with_sub("c"),
            ListViewItem::new("bravo").with_sub("a"),
        ];
        t.toggle_sort(0);
        assert_eq!(t.items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), [
            "alpha", "bravo", "charlie"
        ]);
        t.toggle_sort(1);
        assert_eq!(t.items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), [
            "bravo", "charlie", "alpha"
        ]);
    }

    // ── Virtualisation ───────────────────────────────────────────────────

    #[test]
    fn the_visible_range_includes_a_partial_first_and_last_row() {
        let mut t = table(100);
        t.page_size = 0;
        // A body of exactly four rows: header 40 on top, no footer.
        let b = Rect::new(0.0, 0.0, 900.0, grid::ROW * 5.0);
        assert_eq!(t.visible_rows(b), 0..4);
        t.list.scroll = grid::ROW / 2.0;
        assert_eq!(t.visible_rows(b), 0..5, "a half-scrolled view touches five rows");
        t.list.scroll = grid::ROW;
        assert_eq!(t.visible_rows(b), 1..5);
    }

    #[test]
    fn a_hundred_thousand_rows_still_paint_a_screenful() {
        let mut t = table(0);
        t.page_size = 0;
        t.items = (0..100_000).map(|i| ListViewItem::new(format!("l{i}"))).collect();
        let b = Rect::new(0.0, 0.0, 900.0, grid::ROW * 5.0);
        // Deep in the middle of the list: four rows, not a hundred thousand.
        t.list.scroll = 1_000_000.0;
        let mid = t.visible_rows(b);
        assert!(mid.end - mid.start <= 5, "range {mid:?} is not a screenful");
        assert_eq!(mid, 25_000..25_004);
        // And it never runs past the end.
        t.list.scroll = 10_000_000.0;
        assert!(t.visible_rows(b).is_empty());
    }

    #[test]
    fn the_visible_range_never_leaves_the_current_page() {
        let mut t = table(100);
        t.page_size = 10;
        t.page = 3;
        // A body far taller than ten rows: the range still stops at the page.
        let b = Rect::new(0.0, 0.0, 900.0, 2000.0);
        assert_eq!(t.visible_rows(b), 30..40);
    }

    #[test]
    fn an_empty_table_has_an_empty_range() {
        let t = table(0);
        assert!(t.visible_rows(bounds()).is_empty());
    }

    #[test]
    fn the_row_box_is_the_web_cell_box_and_the_lists_own_row() {
        // `py-2.5` over a body line = 40, which is `height::FILE_ROW`, which is
        // what `views::Density::Normal` answers. Pinned so the two never drift.
        let t = table(1);
        assert_eq!(grid::ROW, 40.0);
        assert_eq!(t.list.row_height(), grid::ROW);
    }

    // ── The bulk bar's geometry ──────────────────────────────────────────

    #[test]
    fn the_bulk_bar_puts_the_close_left_the_actions_right_and_the_label_between() {
        let bar = Rect::new(0.0, 0.0, 600.0, grid::BAR_HEIGHT);
        let b = bulk_bar(bar, &[100.0, 80.0], false);
        // `px-2.5` then a 23 DIP `p-1` button, vertically centred in 48.
        assert_eq!((b.close.left, b.close.right), (10.0, 33.0));
        assert_eq!(b.close.top, (grid::BAR_HEIGHT - grid::CLOSE_BUTTON) / 2.0);
        // The actions hug the right edge, in declaration order, `gap-2` apart.
        assert_eq!(b.actions.len(), 2);
        assert_eq!((b.actions[1].left, b.actions[1].right), (510.0, 590.0));
        assert_eq!((b.actions[0].left, b.actions[0].right), (402.0, 502.0));
        assert_eq!(b.actions[0].bottom - b.actions[0].top, height::BUTTON_SM);
        // The label is what is left, `gap-2` off both neighbours.
        assert_eq!((b.label.left, b.label.right), (41.0, 394.0));
        assert!(b.overflow.is_none());
    }

    #[test]
    fn the_overflow_button_takes_the_right_edge_and_pushes_the_actions_in() {
        let bar = Rect::new(0.0, 0.0, 600.0, grid::BAR_HEIGHT);
        let b = bulk_bar(bar, &[100.0], true);
        let o = b.overflow.expect("an overflow was asked for");
        assert_eq!((o.left, o.right), (562.0, 590.0));
        assert_eq!((b.actions[0].left, b.actions[0].right), (454.0, 554.0));
        // And with no inline action left, the label still stops before it.
        let empty = bulk_bar(bar, &[], true);
        assert_eq!(empty.label.right, 554.0);
    }

    #[test]
    fn a_narrow_bar_keeps_one_action_inline_and_a_wide_one_keeps_three() {
        let mut t = table(4);
        t.bulk_actions = (0..5).map(|i| BulkAction::new(&format!("a{i}"), "Action")).collect();
        assert_eq!(t.inline_bulk(Mode::Table), INLINE_BULK_DESKTOP);
        assert_eq!(t.inline_bulk(Mode::Cards), INLINE_BULK_COMPACT);
    }

    #[test]
    fn a_bar_too_narrow_for_its_label_folds_actions_into_the_overflow() {
        // Bar 600: close ends at 33, label starts at 41; overflow at 562..590.
        let bar = Rect::new(0.0, 0.0, 600.0, grid::BAR_HEIGHT);
        let widths = [150.0, 150.0, 150.0];
        // A short label keeps all three (5 actions total → overflow reserved).
        assert_eq!(fit_bulk(bar, &widths, 5, 30.0), 3);
        // Three actions leave 41..80 = 39 for the label, two leave 41..238;
        // a 100 wide label forces one out.
        assert_eq!(fit_bulk(bar, &widths, 5, 100.0), 2);
        // Wider than even the empty bar: nothing inline, all in the overflow.
        assert_eq!(fit_bulk(bar, &widths, 5, 1000.0), 0);
        // Exactly three actions and no overflow: folding one adds the button.
        let three = fit_bulk(bar, &widths, 3, 100.0);
        assert_eq!(three, 2);
        assert!(bulk_bar(bar, &widths[..three], 3 > three).overflow.is_some());
    }

    #[test]
    fn a_folded_action_moves_to_the_overflow_menu() {
        let mut t = table(4);
        t.bulk_actions = (0..4).map(|i| BulkAction::new(&format!("a{i}"), "Action")).collect();
        t.bulk_fit.set(Some(1));
        assert_eq!(t.inline_bulk(Mode::Table), 1);
        assert_eq!(
            t.menu_command(TableMenu::BulkOverflow, Mode::Table, 0),
            Some(TableCommand::BulkAction("a1".into()))
        );
        // The fit never raises the per-mode count.
        t.bulk_fit.set(Some(9));
        assert_eq!(t.inline_bulk(Mode::Cards), INLINE_BULK_COMPACT);
    }

    #[test]
    fn the_bulk_bar_only_appears_when_there_is_a_selection_and_an_action() {
        let mut t = table(4);
        assert_eq!(t.toolbar_height(), 0.0, "no title, no chooser, no selection");
        t.toggle_row(1);
        assert_eq!(t.toolbar_height(), 0.0, "a selection with no bulk action changes nothing");
        t.bulk_actions = vec![BulkAction::new("del", "Supprimer").danger(true)];
        assert!(t.selection_mode());
        assert_eq!(t.toolbar_height(), grid::BAR_HEIGHT);
        t.clear_selection();
        assert!(!t.selection_mode());
        assert_eq!(t.toolbar_height(), 0.0);
        // A title alone still opens the idle face.
        t.title = "Utilisateurs".into();
        assert_eq!(t.toolbar_height(), grid::TOOLBAR_ROW);
    }

    // ── The pagination bar's geometry ────────────────────────────────────

    #[test]
    fn the_nav_cluster_is_right_aligned_and_drops_its_jumps_when_narrow() {
        let footer = Rect::new(0.0, 0.0, 600.0, grid::FOOTER_HEIGHT);
        let full = nav_cluster(footer, 40.0, false);
        assert_eq!(full.len(), 4);
        assert_eq!(full[0].0, Chrome::FirstPage);
        assert_eq!(full[3].0, Chrome::LastPage);
        // The last button ends `px-3` off the right edge.
        assert_eq!(full[3].1.right, 588.0);
        assert_eq!(full[3].1.bottom - full[3].1.top, grid::ICON_BUTTON);
        // 4 buttons + a 56-wide label + 4 gaps = 176.
        assert_eq!(full[0].1.left, 588.0 - 176.0);

        let compact = nav_cluster(footer, 40.0, true);
        assert_eq!(compact.len(), 2);
        assert_eq!(compact[0].0, Chrome::PrevPage);
        assert_eq!(compact[1].0, Chrome::NextPage);
        assert_eq!(compact[1].1.right, 588.0);
        // The label always sits between the two single chevrons.
        let label = nav_label_rect(footer, 40.0, true);
        assert_eq!(label.left, compact[0].1.right);
        assert_eq!(label.right, compact[1].1.left);
    }

    #[test]
    fn the_page_size_group_is_a_padded_pill_per_option() {
        let (group, pills) = size_group(100.0, 50.0, &[10.0, 14.0]);
        assert_eq!(pills.len(), 2);
        // `p-0.5` then `px-1.5` around each measured label, `gap-0.5` between.
        assert_eq!((pills[0].left, pills[0].right), (102.0, 124.0));
        assert_eq!((pills[1].left, pills[1].right), (126.0, 152.0));
        assert_eq!(pills[0].bottom - pills[0].top, grid::SIZE_PILL_H);
        assert_eq!((group.left, group.right), (100.0, 154.0));
        assert_eq!(group.bottom - group.top, grid::FOOTER_ROW);
    }

    // ── The body's four states ───────────────────────────────────────────

    #[test]
    fn the_body_picks_the_right_empty_state() {
        let mut t = table(0);
        assert_eq!(t.empty_kind(), Some(EmptyKind::FirstUse));
        t.filtered = true;
        assert_eq!(t.empty_kind(), Some(EmptyKind::NoResults), "filters change the wording");
        t.error = Some("502".into());
        assert_eq!(t.empty_kind(), Some(EmptyKind::Error), "an error outranks both");
        // Loading outranks everything: nothing is known about the rows yet.
        t.loading = true;
        assert_eq!(t.empty_kind(), None);
        assert!(!t.is_empty());
    }

    #[test]
    fn a_table_with_rows_is_not_empty_but_an_error_over_rows_still_is() {
        let mut t = table(5);
        assert!(!t.is_empty());
        t.error = Some(String::new());
        assert!(t.is_empty(), "an error means nothing is known about the rows");
    }

    #[test]
    fn the_header_and_the_footer_disappear_with_the_rows() {
        let mut t = table(0);
        let l = t.layout_of(bounds());
        assert_eq!(l.header.bottom, l.header.top, "no columns over an empty state");
        assert_eq!(l.footer.bottom, l.footer.top);
        t.items = (0..3).map(|i| ListViewItem::new(format!("l{i}"))).collect();
        let l = t.layout_of(bounds());
        assert_eq!(l.header.bottom - l.header.top, grid::ROW);
        assert_eq!(l.footer.bottom - l.footer.top, grid::FOOTER_HEIGHT);
        // …and while loading, the skeleton replaces the columns too.
        t.loading = true;
        let l = t.layout_of(bounds());
        assert_eq!(l.header.bottom, l.header.top);
        assert_eq!(l.footer.bottom, l.footer.top);
    }

    #[test]
    fn the_bands_stack_without_overlapping() {
        let mut t = table(20);
        t.title = "Utilisateurs".into();
        let l = t.layout_of(bounds());
        assert_eq!(l.toolbar.bottom - l.toolbar.top, grid::TOOLBAR_ROW);
        assert_eq!(l.frame.top, l.toolbar.bottom + grid::TOOLBAR_GAP);
        assert_eq!(l.header.top, l.frame.top);
        assert_eq!(l.body.top, l.header.bottom);
        assert_eq!(l.body.bottom, l.footer.top);
        assert_eq!(l.footer.bottom, l.frame.bottom);
    }

    // ── Columns, flags and overflow ──────────────────────────────────────

    #[test]
    fn column_flags_ride_on_the_replicas_own_tag() {
        let col = with_flag(with_flag(column("name", "Nom", 100), flags::SORTABLE), flags::PRIMARY);
        assert_eq!(col.name, "name");
        assert_eq!(col.tag.as_deref(), Some("sortable primary"));
        assert!(has_flag(&col, flags::SORTABLE));
        assert!(has_flag(&col, flags::PRIMARY));
        assert!(!has_flag(&col, flags::REQUIRED));
        // Setting the same flag twice does not duplicate it.
        let twice = with_flag(col, flags::SORTABLE);
        assert_eq!(twice.tag.as_deref(), Some("sortable primary"));
        // A label falls back to the id when the header is empty.
        assert_eq!(column_label(&twice), "Nom");
        assert_eq!(column_label(&column("size", "", 10)), "size");
    }

    #[test]
    fn the_card_layout_titles_itself_with_the_primary_column() {
        let mut t = table(3);
        let (primary, rest) = t.card_columns();
        assert_eq!(primary.map(|c| c.name), Some("name".into()));
        assert_eq!(rest.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), [
            "role", "created", "size"
        ]);
        // `hideOnCards` drops a column from the pairs…
        t.columns[2] = with_flag(t.columns[2].clone(), flags::HIDE_ON_CARDS);
        assert_eq!(t.card_pairs().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), [
            "role", "size"
        ]);
        // …and with no `primary` at all the first visible column titles it.
        let mut plain = DataTable::new();
        plain.columns = vec![column("a", "A", 10), column("b", "B", 10)];
        assert_eq!(plain.card_columns().0.map(|c| c.name), Some("a".into()));
    }

    #[test]
    fn the_table_knows_when_it_is_wider_than_its_box() {
        let t = table(3);
        // 40 (selection) + 300 + 140 + 150 + 100 = 730.
        assert_eq!(t.table_width(), 730.0);
        assert!(!t.overflows(Rect::new(0.0, 0.0, 900.0, 100.0)));
        assert!(t.overflows(Rect::new(0.0, 0.0, 600.0, 100.0)));
        // …and never narrower than `minTableWidth`.
        let mut narrow = DataTable::new();
        narrow.columns = vec![column("a", "A", 100)];
        assert!(narrow.overflows(Rect::new(0.0, 0.0, 300.0, 100.0)));
    }

    #[test]
    fn the_actions_column_takes_its_width_off_the_right_edge() {
        let mut t = table(3);
        t.row_actions = true;
        let l = t.layout_of(bounds());
        let xs = t.column_x_offsets(l.frame);
        // The columns still start after the selection column and are laid out
        // by their declared widths; the actions column is simply reserved.
        assert_eq!(xs[0], grid::SELECT_COL);
        assert_eq!(t.table_width(), 730.0 + grid::ACTIONS_COL);
    }

    // ── Popup placement ──────────────────────────────────────────────────

    #[test]
    fn a_menu_opens_at_its_point_and_is_pulled_back_inside_the_screen() {
        let area = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let want = Size::new(200.0, 150.0);
        let r = place_menu((100.0, 100.0), 90.0, want, area);
        assert_eq!((r.left, r.top, r.right, r.bottom), (100.0, 100.0, 300.0, 250.0));
        // Past the right edge: pulled back 8 DIP inside.
        let r = place_menu((950.0, 100.0), 90.0, want, area);
        assert_eq!(r.right, 992.0);
        // Past the bottom: flipped to END at the trigger's top.
        let r = place_menu((100.0, 700.0), 660.0, want, area);
        assert_eq!((r.top, r.bottom), (510.0, 660.0));
        // Never past the left or top edge either (the row menu's
        // `Math.max(8, r.right - 200)`).
        let r = place_menu((-50.0, -20.0), 0.0, want, area);
        assert_eq!((r.left, r.top), (8.0, 8.0));
    }

    // ── Column resizing ──────────────────────────────────────────────────

    #[test]
    fn a_column_resizes_by_drag_never_below_the_web_minimum_and_resets() {
        let mut t = table(3);
        t.begin_resize(1, 500.0);
        t.drag_resize(560.0);
        assert_eq!(t.column_width(1), 200.0, "140 + 60");
        t.drag_resize(300.0);
        assert_eq!(t.column_width(1), grid::RESIZE_MIN, "floored at MIN_WIDTH = 56");
        t.end_resize();
        assert!(t.resize.is_none());
        // The keyboard nudge, then the double-click reset to the DECLARED width.
        t.nudge_column(1, grid::RESIZE_STEP_BIG);
        assert_eq!(t.column_width(1), grid::RESIZE_MIN + 24.0);
        t.reset_column_width(1);
        assert_eq!(t.column_width(1), 140.0);
    }

    #[test]
    fn the_resize_grip_straddles_the_right_edge_and_outranks_the_sort_button() {
        let t = table(3);
        let b = bounds();
        let l = t.layout_of(b);
        let y = l.header.top + 5.0;
        // Column 0 ends at 340: its grip covers 336..344.
        assert_eq!(t.resize_handle_at(b, 336.0, y), Some(0));
        assert_eq!(t.resize_handle_at(b, 343.9, y), Some(0));
        assert_eq!(t.resize_handle_at(b, 335.9, y), None);
        assert_eq!(t.chrome_at_pure(b, 338.0, y), Some(Chrome::ResizeHandle(0)));
        // Not in the body, and not when resizing is off.
        assert_eq!(t.resize_handle_at(b, 338.0, l.body.top + 5.0), None);
        let mut off = table(3);
        off.resizable_columns = false;
        assert_eq!(off.resize_handle_at(b, 338.0, y), None);
    }

    // ── Scrolling ────────────────────────────────────────────────────────

    #[test]
    fn the_body_scrolls_under_a_header_that_stays_put() {
        let mut t = table(20);
        t.page_size = 0;
        // Header 40 + a body of five rows.
        let b = Rect::new(0.0, 0.0, 900.0, grid::ROW * 6.0);
        assert_eq!(t.max_scroll_y(b), 15.0 * grid::ROW);
        let header_before = t.layout_of(b).header;
        assert!(t.scroll_by(b, 0.0, 100.0));
        assert_eq!(t.list.scroll, 100.0);
        let header_after = t.layout_of(b).header;
        assert_eq!(header_before.top, header_after.top, "the header is sticky");
        // Clamped at both ends, and a move that goes nowhere says so.
        t.scroll_by(b, 0.0, 1e6);
        assert_eq!(t.list.scroll, 15.0 * grid::ROW);
        assert!(!t.scroll_by(b, 0.0, 10.0));
        t.scroll_by(b, 0.0, -1e6);
        assert_eq!(t.list.scroll, 0.0);
    }

    #[test]
    fn a_table_wider_than_its_box_scrolls_sideways_inside_it() {
        let mut t = table(3);
        let b = Rect::new(0.0, 0.0, 500.0, 400.0);
        // 730 of columns in a 500 box.
        assert_eq!(t.max_scroll_x(t.layout_of(b).frame), 230.0);
        t.scroll_by(b, 100.0, 0.0);
        let xs = t.column_x_offsets(t.layout_of(b).frame);
        assert_eq!(xs[0], grid::SELECT_COL - 100.0);
        // The row band follows the content, not the box.
        let r = t.row_rect(b, 0);
        assert_eq!((r.left, r.right), (-100.0, 630.0));
        t.scroll_by(b, 1e6, 0.0);
        assert_eq!(t.scroll_x, 230.0);
        // A table that fits never scrolls sideways.
        let mut fits = table(3);
        fits.scroll_by(bounds(), 50.0, 0.0);
        assert_eq!(fits.scroll_x, 0.0);
    }

    #[test]
    fn the_horizontal_bar_gets_its_own_band_under_the_rows() {
        let mut t = table(30);
        t.layout = Layout::Table;
        let b = Rect::new(0.0, 0.0, 500.0, 400.0);
        let l = t.layout_of(b);
        // The band sits between the rows and the footer, one gutter tall.
        assert_eq!(l.hscroll.top, l.body.bottom);
        assert_eq!(l.hscroll.bottom, l.footer.top);
        assert_eq!(l.hscroll.bottom - l.hscroll.top, sb::SCROLLBAR_SIZE);
        // The bar's rail lies inside that band, not over the last row.
        let bar = t.scrollbar_in(&l, true, false).expect("a horizontal bar");
        assert!(bar.rail.top >= l.body.bottom - 0.01);
        assert!(bar.rail.bottom <= l.hscroll.bottom + 0.01);
        // A table that fits keeps no band.
        let fits = table(30);
        let l = fits.layout_of(bounds());
        assert_eq!(l.hscroll.top, l.hscroll.bottom);
        assert_eq!(l.body.bottom, l.footer.top);
    }

    #[test]
    fn scroll_into_view_moves_the_least_needed() {
        let mut t = table(20);
        t.page_size = 0;
        let b = Rect::new(0.0, 0.0, 900.0, grid::ROW * 6.0);
        t.scroll_into_view(b, 7);
        assert_eq!(t.list.scroll, 3.0 * grid::ROW, "row 7 ends at the body's bottom");
        t.scroll_into_view(b, 5);
        assert_eq!(t.list.scroll, 3.0 * grid::ROW, "already visible: no move");
        t.scroll_into_view(b, 1);
        assert_eq!(t.list.scroll, grid::ROW);
    }

    // ── Keyboard ─────────────────────────────────────────────────────────

    fn focused_rows(rows: usize) -> DataTable {
        let mut t = table(rows);
        t.focus_part = Some(FocusPart::Rows);
        t
    }

    #[test]
    fn arrows_move_the_cursor_and_space_selects() {
        let mut t = focused_rows(10);
        t.page_size = 0;
        let b = bounds();
        assert_eq!(t.on_key(b, vk::DOWN, Modifiers::NONE), (true, None));
        assert_eq!(t.cursor, Some(1), "the first move starts from the first row");
        t.on_key(b, vk::DOWN, Modifiers::NONE);
        assert!(t.selected().is_empty(), "moving never selects by itself");
        t.on_key(b, vk::SPACE, Modifiers::NONE);
        assert_eq!(t.selected(), vec![2]);
        t.on_key(b, vk::SPACE, Modifiers::NONE);
        assert!(t.selected().is_empty(), "Space toggles");
        t.on_key(b, vk::END, Modifiers::NONE);
        assert_eq!(t.cursor, Some(9));
        t.on_key(b, vk::DOWN, Modifiers::NONE);
        assert_eq!(t.cursor, Some(9), "clamped at the last row");
        t.on_key(b, vk::HOME, Modifiers::NONE);
        assert_eq!(t.cursor, Some(0));
        // Not a key the rows use: left for the caller.
        assert_eq!(t.on_key(b, vk::letter('Q'), Modifiers::NONE), (false, None));
    }

    #[test]
    fn shift_extends_a_range_that_can_shrink_back() {
        let mut t = focused_rows(10);
        t.page_size = 0;
        let b = bounds();
        t.toggle_row(8); // selected before the range: must survive it
        t.cursor = Some(2);
        t.on_key(b, vk::DOWN, Modifiers::SHIFT);
        t.on_key(b, vk::DOWN, Modifiers::SHIFT);
        assert_eq!(t.selected(), vec![2, 3, 4, 8]);
        t.on_key(b, vk::UP, Modifiers::SHIFT);
        assert_eq!(t.selected(), vec![2, 3, 8], "shrinking deselects what the range added");
        // A plain move ends the range; Shift+Space then selects anchor → cursor.
        t.on_key(b, vk::DOWN, Modifiers::NONE);
        t.on_key(b, vk::DOWN, Modifiers::NONE);
        assert_eq!(t.cursor, Some(5));
        t.anchor = Some(5);
        t.cursor = Some(7);
        t.on_key(b, vk::SPACE, Modifiers::SHIFT);
        assert_eq!(t.selected(), vec![2, 3, 5, 6, 7, 8]);
    }

    #[test]
    fn the_cursor_turns_the_page_and_ctrl_a_selects_the_page() {
        let mut t = focused_rows(12);
        t.page_size = 5;
        let b = bounds();
        t.cursor = Some(4);
        t.on_key(b, vk::DOWN, Modifiers::NONE);
        assert_eq!((t.cursor, t.page_index()), (Some(5), 1), "past the page's end: next page");
        t.on_key(b, vk::letter('A'), Modifiers::CTRL);
        assert_eq!(t.selected(), vec![5, 6, 7, 8, 9]);
        t.on_key(b, vk::END, Modifiers::CTRL);
        assert_eq!((t.cursor, t.page_index()), (Some(11), 2), "Ctrl+End: the whole table");
    }

    #[test]
    fn enter_and_the_menu_key_come_back_as_events() {
        let mut t = focused_rows(3);
        t.row_actions = true;
        let b = bounds();
        t.cursor = Some(1);
        assert_eq!(t.on_key(b, vk::ENTER, Modifiers::NONE), (true, Some(TableEvent::RowActivated(1))));
        assert_eq!(
            t.on_key(b, vk::APPS, Modifiers::NONE),
            (true, Some(TableEvent::OpenMenu(TableMenu::Row(1))))
        );
        assert_eq!(
            t.on_key(b, vk::F10, Modifiers::SHIFT),
            (true, Some(TableEvent::OpenMenu(TableMenu::Copy { row: 1, column: 0 })))
        );
    }

    #[test]
    fn a_focused_button_activates_on_enter_and_a_grip_nudges_on_arrows() {
        let mut t = table(12);
        t.page_size = 5;
        let b = bounds();
        t.focus_part = Some(FocusPart::Chrome(Chrome::NextPage));
        t.on_key(b, vk::ENTER, Modifiers::NONE);
        assert_eq!(t.page_index(), 1);
        t.focus_part = Some(FocusPart::Chrome(Chrome::Header(0)));
        t.manual_sort = true;
        t.on_key(b, vk::SPACE, Modifiers::NONE);
        assert_eq!(t.list.sorting, SortOrder::Ascending);
        t.focus_part = Some(FocusPart::Chrome(Chrome::ResizeHandle(1)));
        t.on_key(b, vk::RIGHT, Modifiers::NONE);
        assert_eq!(t.column_width(1), 148.0);
        t.on_key(b, vk::LEFT, Modifiers::SHIFT);
        assert_eq!(t.column_width(1), 124.0);
        // Nothing focused: nothing used.
        t.focus_part = None;
        assert_eq!(t.on_key(b, vk::ENTER, Modifiers::NONE), (false, None));
    }

    #[test]
    fn shift_click_on_a_box_sets_the_whole_range_to_the_clicked_state() {
        let mut t = table(10);
        t.page_size = 0;
        t.click_row_check(2, false);
        t.click_row_check(6, true);
        assert_eq!(t.selected(), vec![2, 3, 4, 5, 6]);
        // From the same anchor, a shift click on a SELECTED row clears the range.
        t.click_row_check(4, true);
        assert_eq!(t.selected(), vec![5, 6]);
    }

    #[test]
    fn activate_does_what_the_table_owns_and_reports_the_rest() {
        let mut t = table(12);
        t.page_size = 5;
        t.bulk_actions = vec![BulkAction::new("del", "Supprimer")];
        assert_eq!(t.activate(Chrome::NextPage, false), None);
        assert_eq!(t.page_index(), 1);
        assert_eq!(t.activate(Chrome::BulkAction(0), false), Some(TableEvent::BulkAction("del".into())));
        assert_eq!(t.activate(Chrome::Columns, false), Some(TableEvent::OpenMenu(TableMenu::Columns)));
        assert_eq!(t.activate(Chrome::RowMenu(6), false), Some(TableEvent::OpenMenu(TableMenu::Row(6))));
        t.list.scroll = 80.0;
        assert_eq!(t.activate(Chrome::PageSize(10), false), None);
        assert_eq!((t.page_size, t.page, t.list.scroll), (10, 0, 0.0));
    }

    // ── Menus and copy ───────────────────────────────────────────────────

    #[test]
    fn the_copy_menu_copies_what_is_on_screen_by_visible_column() {
        let mut t = table(3);
        t.hidden_columns = vec!["role".into()];
        // Visible column 1 is « Créé le » now, not « Rôle ».
        assert_eq!(t.cell_text(0, 1), "hier");
        let copy = TableMenu::Copy { row: 1, column: 1 };
        assert_eq!(t.menu_command(copy, Mode::Table, 0), Some(TableCommand::Copy("hier".into())));
        assert_eq!(
            t.menu_command(copy, Mode::Table, 1),
            Some(TableCommand::Copy("ligne 00001\thier\t48 Ko".into()))
        );
        assert_eq!(t.menu_command(copy, Mode::Table, 2), Some(TableCommand::Copy("hier\nhier\nhier".into())));
        assert_eq!(t.menu_command(copy, Mode::Table, 3), None);
    }

    #[test]
    fn the_column_chooser_toggles_columns_but_never_a_required_one() {
        let mut t = table(3);
        t.columns[0] = with_flag(t.columns[0].clone(), flags::REQUIRED);
        // Entry 0 is the « Colonnes » label; entries follow the model order.
        assert_eq!(t.menu_command(TableMenu::Columns, Mode::Table, 0), None);
        assert_eq!(t.menu_command(TableMenu::Columns, Mode::Table, 1), None, "required");
        let cmd = t.menu_command(TableMenu::Columns, Mode::Table, 2);
        assert_eq!(cmd, Some(TableCommand::ToggleColumn("role".into())));
        let cmd = cmd.expect("the role column is optional");
        assert!(t.apply_command(&cmd));
        assert_eq!(t.hidden_columns, vec!["role".to_string()]);
        assert!(t.apply_command(&cmd));
        assert!(t.hidden_columns.is_empty(), "a second toggle shows it again");
        assert!(!t.apply_command(&TableCommand::Copy("x".into())), "the caller's job");
    }

    #[test]
    fn the_bulk_overflow_menu_holds_only_what_did_not_fit() {
        let mut t = table(3);
        t.bulk_actions = (0..5).map(|i| BulkAction::new(&format!("a{i}"), "Action")).collect();
        assert_eq!(t.menu(TableMenu::BulkOverflow, Mode::Table).items().len(), 2);
        assert_eq!(
            t.menu_command(TableMenu::BulkOverflow, Mode::Table, 0),
            Some(TableCommand::BulkAction("a3".into()))
        );
        assert_eq!(t.menu(TableMenu::BulkOverflow, Mode::Cards).items().len(), 4);
    }

    #[test]
    fn a_hidden_column_does_not_shift_the_cells_after_it() {
        let mut t = table(1);
        t.hidden_columns = vec!["name".into()];
        assert_eq!(t.row_text(0), "Membre\thier\t48 Ko");
    }
}
