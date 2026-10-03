//! Tables.
//!
//! # The contract, and the trap in it
//!
//! A table resolves to a grid of cells, each of which lays its own content out
//! in its own column width, and a row is as tall as its tallest cell — unless
//! the row's height mode is `exactly`, which fixes it.
//!
//! **`cellBorders` has three states, not two.** An object, an explicit `null`,
//! and the key being absent are three different documents: `null` is an
//! explicit "no border on this side", which is not the same as "nothing said".
//! Collapsing the two into `Option::None` corrupts every table that has a side
//! deliberately turned off. This module must therefore read borders in a way
//! that keeps presence distinct from null, exactly as the model does.
//!
//! [`BorderSide`] is that three-state value, and [`edges`] is what it is for:
//! the web resolves borders **per edge, not per cell** (`paintTableBorders`,
//! `canvas-engine.ts:2705-2785`), weighting absent as 0 (inherit the table
//! default), `null` as 1 (an explicit veto that evicts the neighbour's
//! inherited default) and an object as 2. Higher weight wins; on a tie the
//! thicker border wins. Resolving before stroking is also what stops every
//! interior edge being drawn twice, which the comment at `:2695-2697` records
//! as a visible regression: doubled dashes and washed-out translucent colour.
//!
//! # What this module does, and where it stops
//!
//! Ported from `layoutTable` (`canvas-engine.ts:1363-1592`). It resolves
//! column widths (both branches of the `autofit`/`fixed` split), places cells
//! through the occupancy map so `colspan`/`rowspan` land where the web puts
//! them, sizes rows, and returns one box per placed cell.
//!
//! **Cell content layout is the caller's.** The caller implements
//! [`CellContent`]: it lays each cell out with [`crate::doc::para::layout`] at
//! [`Cell::layout_width`] and hands back the content's extent, and the table
//! engine turns that into row heights. The reason is not tidiness — the cell
//! engine and the body engine must be the *same* one, and it lives above this
//! module.
//!
//! **Splitting a table across a page break is also the caller's**, and
//! [`cells_in_band`] is the seam: the web keeps one continuous table geometry
//! and re-emits the cell rectangles per page in page-local coordinates,
//! filtered to the page band (`canvas-engine.ts:3421-3425`). Pagination there
//! is line-based, not row-based, so a row is split mid-row; nothing in this
//! module decides that, and nothing in it needs to change when it lands.
//!
//! Four traps this module exists to keep out of the rest of the app:
//!
//! * **`colCount` counts merged cells.** `colsInRow += colspan` (`:790`) runs
//!   over every cell including the `merged` ones that placement then skips
//!   (`:1506`), so a table with absorbed cells really does get phantom trailing
//!   columns. That is parity, not a bug to fix here.
//! * **`exactly` does not grow.** A row whose mode is `exactly` and whose
//!   `rowHeights` entry is positive keeps that height whatever is inside it —
//!   and a rowspan deficit is not allowed to push it either (`:1529-1533`).
//! * **A vertical cell lays out at 100 000 px, not `f32::MAX`** (`:1513`).
//!   See [`VERTICAL_LAYOUT_WIDTH`].
//! * **A table narrower than its column is normal.** Hand-set `colWidths` that
//!   fit are respected as they are (`:1444`); only a table with no explicit
//!   widths is stretched to fill the text column.

#![allow(dead_code)]

use std::collections::HashMap;

use serde_json::Value;

use super::DocPx;
use crate::marks::TextMark;
use crate::measure::Measure;
use crate::model::Node;

// ── The web engine's constants (`canvas-engine.ts:1348-1352`) ─────────────────

/// Default horizontal cell padding (`CELL_PAD_X`, `:1348`).
const CELL_PAD_X: DocPx = 6.0;
/// Default vertical cell padding (`CELL_PAD_Y`, `:1349`). Deliberately small:
/// Word uses ~0, and a generous value visibly inflates every row.
const CELL_PAD_Y: DocPx = 2.0;
/// Minimum height of a row (`MIN_ROW_H`, `:1350`).
const MIN_ROW_H: DocPx = 22.0;
/// Minimum width of a column (`MIN_COL_W`, `:1352`).
const MIN_COL_W: DocPx = 24.0;
/// A column width at or below this is treated as "not set" — the test the web
/// uses both to decide whether `colWidths` is usable at all (`:1434`) and to
/// find the entries a ragged array is missing (`:1477-1479`).
const COL_WIDTH_EPSILON: DocPx = 4.0;
/// `table.accent` fallback (`:1367`).
const DEFAULT_ACCENT: &str = "#1a73e8";
/// `tableBorderColor` / `tableBorderWidth` fallbacks (`:2708-2710`).
const DEFAULT_BORDER_COLOR: &str = "#bdc1c6";
const DEFAULT_BORDER_WIDTH: DocPx = 1.0;
/// Alpha of the header-row tint and of the striped-row tint (`:1554-1555`).
pub const HEADER_TINT_ALPHA: f32 = 0.16;
pub const STRIPE_TINT_ALPHA: f32 = 0.06;

/// The width a vertical (`cellDir` 90/270) cell's content is laid out at
/// (`:1513`): wide enough that nothing wraps, small enough that the font engine
/// can still build a layout. **Not `f32::MAX`** — a text layout at that width
/// is the bug this constant exists to keep shut.
pub const VERTICAL_LAYOUT_WIDTH: DocPx = 100_000.0;

/// Default family and size for the intrinsic-width helper, from the web's
/// `DEFAULT_FAM` / `DEFAULT_SZ` (the same pair `doc::convert` uses).
/// `horizontalRule` is a text paragraph of 60 box-drawing characters at 8 pt
/// (`:997-1003`), not a drawing primitive — so it has an intrinsic width.
const RULE_CHARS: usize = 60;
const RULE_SIZE_PT: f32 = 8.0;

// ── Value types ──────────────────────────────────────────────────────────────

/// How a border is stroked. Anything the document does not recognise is solid,
/// which is what the web's cast falls back to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BorderStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

impl BorderStyle {
    fn parse(v: Option<&Value>) -> Self {
        match v.and_then(Value::as_str) {
            Some("dashed") => BorderStyle::Dashed,
            Some("dotted") => BorderStyle::Dotted,
            _ => BorderStyle::Solid,
        }
    }
}

/// One stroked border: `{ w, s, c }` in the document (`canvas-engine.ts:129`).
#[derive(Clone, Debug, PartialEq)]
pub struct BorderSpec {
    pub width: DocPx,
    pub style: BorderStyle,
    /// The colour exactly as the document wrote it — a CSS colour string. It is
    /// never parsed here: the painter owns colour, and a colour this module
    /// cannot parse must still reach it unchanged.
    pub color: String,
}

impl BorderSpec {
    /// The table-level default border (`:2708-2710`).
    fn table_default(width: Option<DocPx>, style: BorderStyle, color: Option<String>) -> Self {
        Self {
            width: width.filter(|w| *w != 0.0).unwrap_or(DEFAULT_BORDER_WIDTH),
            style,
            color: color.unwrap_or_else(|| DEFAULT_BORDER_COLOR.to_string()),
        }
    }

    /// A zero-width placeholder that carries the veto's weight but paints
    /// nothing (`:2728`).
    fn veto() -> Self {
        Self { width: 0.0, style: BorderStyle::Solid, color: "transparent".into() }
    }
}

/// One side of a cell's `cellBorders`, in all **three** of its states.
///
/// This is the type the module comment is about. `Inherit` and `None` are not
/// the same answer and must never be folded into one: `Inherit` lets the
/// table's default paint the edge, `None` forbids it — including when the
/// *neighbouring* cell would have inherited a default onto the shared edge.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum BorderSide {
    /// The key is absent: nothing was said, so the table default applies.
    #[default]
    Inherit,
    /// The key is present and `null`: this side deliberately has no border.
    None,
    /// The key is present and holds a border.
    Set(BorderSpec),
}

impl BorderSide {
    /// The web's edge weights (`:2716-2722`): absent 0, explicit `null` 1,
    /// explicit border 2. Higher wins.
    pub fn weight(&self) -> u8 {
        match self {
            BorderSide::Inherit => 0,
            BorderSide::None => 1,
            BorderSide::Set(_) => 2,
        }
    }

    pub fn spec(&self) -> Option<&BorderSpec> {
        match self {
            BorderSide::Set(s) => Some(s),
            _ => None,
        }
    }

    /// Reads one side out of a `cellBorders` object.
    ///
    /// A side that is present but is neither `null` nor an object (a number, a
    /// string) is read as `Inherit`: the web would build a border with an
    /// undefined width and stroke nothing visible, and inheriting is the
    /// nearest honest reading that cannot poison a coordinate with NaN.
    fn parse(borders: &serde_json::Map<String, Value>, key: &str) -> Self {
        match borders.get(key) {
            Option::None => BorderSide::Inherit,
            Some(Value::Null) => BorderSide::None,
            Some(Value::Object(side)) => BorderSide::Set(BorderSpec {
                // A side object with no `w` wins its edge and paints nothing,
                // which is what the web's `undefined` width does at paint time.
                width: side.get("w").and_then(number).unwrap_or(0.0),
                style: BorderStyle::parse(side.get("s")),
                color: side
                    .get("c")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_BORDER_COLOR)
                    .to_string(),
            }),
            Some(_) => BorderSide::Inherit,
        }
    }
}

/// A cell's four sides. All-`Inherit` is both the default and what an absent or
/// `null` `cellBorders` means (`:2738-2743` reads `bd ? bd.t : undefined`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellBorders {
    pub top:    BorderSide,
    pub bottom: BorderSide,
    pub left:   BorderSide,
    pub right:  BorderSide,
}

impl CellBorders {
    fn parse(attrs: Option<&Value>) -> Self {
        // Absent, null, or anything that is not an object: nothing was said
        // about any side. Note this is NOT the same as every side being `None`.
        let Some(Value::Object(map)) = attrs.and_then(|a| a.get("cellBorders")) else {
            return Self::default();
        };
        Self {
            top:    BorderSide::parse(map, "t"),
            bottom: BorderSide::parse(map, "b"),
            left:   BorderSide::parse(map, "l"),
            right:  BorderSide::parse(map, "r"),
        }
    }
}

/// `tableStyle` (`:1516`). An unrecognised value behaves as `Grid`, which is
/// exactly what the web's string comparisons do with it: it is not `plain`, so
/// the table default border applies, and it is neither `header` nor `striped`,
/// so no row is tinted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableStyle {
    Plain,
    #[default]
    Grid,
    Striped,
    Header,
}

impl TableStyle {
    fn parse(v: Option<&Value>) -> Self {
        match v.and_then(Value::as_str) {
            Some("plain") => TableStyle::Plain,
            Some("striped") => TableStyle::Striped,
            Some("header") => TableStyle::Header,
            _ => TableStyle::Grid,
        }
    }
}

/// `tableAlign` (`:1525`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// `tableWrap` (`:1550`). Whether the table actually floats is the caller's
/// decision — it also depends on the table being narrow enough
/// (`tw < contentW - MIN_SEG_W - 20`, `:1227`) — but the attribute is resolved
/// here so the caller does not have to re-read the node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableWrap {
    #[default]
    None,
    Around,
}

/// `cellVAlign` (`:1491`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VAlign {
    #[default]
    Top,
    Center,
    Bottom,
}

/// `cellDir` (`:1494`) — Word's "text direction".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CellDir {
    #[default]
    Horizontal,
    /// 90°, top to bottom.
    Down,
    /// 270°, bottom to top.
    Up,
}

impl CellDir {
    pub fn is_vertical(self) -> bool {
        !matches!(self, CellDir::Horizontal)
    }
}

/// `rowHeightModes` (`:1523`). The parser's fallback is `atleast`, and so is
/// anything unrecognised (`table.rowHeightModes?.[i] || 'atleast'`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RowHeightMode {
    #[default]
    AtLeast,
    /// A fixed row: it does not grow to fit its content, and its content is
    /// clipped to it at paint time (`:2560-2564`).
    Exactly,
}

/// What fills a cell behind its content (`:1552-1556`).
///
/// The tint is left as an alpha over the table's `accent` rather than being
/// resolved to a colour: this module never parses a colour string, so it cannot
/// degrade one.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum CellFill {
    #[default]
    None,
    /// `cellBg`, verbatim.
    Explicit(String),
    /// `accent` at this alpha.
    Tint(f32),
}

// ── The grid ─────────────────────────────────────────────────────────────────

/// The resolved geometry of one table.
///
/// Coordinates are document pixels with the origin at the **top-left of the
/// table**, x already carrying the table's own alignment/indent offset so that
/// a cell's `x` is relative to the content column exactly as the web's is.
#[derive(Clone, Debug)]
pub struct Grid {
    pub column_widths: Vec<DocPx>,
    pub row_heights:   Vec<DocPx>,
    /// One mode per row, defaulted and repaired to `row_heights.len()`.
    pub row_height_modes: Vec<RowHeightMode>,
    /// Cumulative column boundaries, `column_widths.len() + 1` entries, with
    /// `x_offset` applied. The web keeps these for the drag-resize handles
    /// (`:133-134`).
    pub col_x: Vec<DocPx>,
    /// Cumulative row boundaries, `row_heights.len() + 1` entries.
    pub row_y: Vec<DocPx>,
    /// The table's own width and height.
    pub width:  DocPx,
    pub height: DocPx,
    /// How far the table is pushed right by its alignment or indent.
    pub x_offset: DocPx,
    pub style:  TableStyle,
    pub accent: String,
    /// The table-level default border, or `None` for `tableStyle: 'plain'`,
    /// which has no default at all — only explicit sides paint (`:2707`).
    pub default_border: Option<BorderSpec>,
    pub pad_left:   DocPx,
    pub pad_right:  DocPx,
    pub pad_top:    DocPx,
    pub pad_bottom: DocPx,
    /// `cellSpacing`: every cell is shrunk by half of it on all four sides,
    /// which is what stops adjacent cells sharing a border (`:1558`).
    pub spacing: DocPx,
    pub align:   TableAlign,
    pub indent:  DocPx,
    pub wrap:    TableWrap,
    pub header_repeat: bool,
    pub header_rows:   usize,
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            column_widths: Vec::new(),
            row_heights: Vec::new(),
            row_height_modes: Vec::new(),
            col_x: vec![0.0],
            row_y: vec![0.0],
            width: 0.0,
            height: 0.0,
            x_offset: 0.0,
            style: TableStyle::Grid,
            accent: DEFAULT_ACCENT.to_string(),
            default_border: Some(BorderSpec::table_default(None, BorderStyle::Solid, None)),
            pad_left: CELL_PAD_X,
            pad_right: CELL_PAD_X,
            pad_top: CELL_PAD_Y,
            pad_bottom: CELL_PAD_Y,
            spacing: 0.0,
            align: TableAlign::Left,
            indent: 0.0,
            wrap: TableWrap::None,
            header_repeat: false,
            header_rows: 0,
        }
    }
}

impl Grid {
    pub fn column_count(&self) -> usize {
        self.column_widths.len()
    }

    pub fn row_count(&self) -> usize {
        self.row_heights.len()
    }

    /// The mode of row `i`, defaulting to `AtLeast` past the end.
    pub fn row_mode(&self, i: usize) -> RowHeightMode {
        self.row_height_modes.get(i).copied().unwrap_or_default()
    }
}

/// One cell's box within the table.
#[derive(Clone, Debug, Default)]
pub struct Cell {
    pub row:     usize,
    pub column:  usize,
    pub rowspan: usize,
    pub colspan: usize,
    pub x:       DocPx,
    pub y:       DocPx,
    pub width:   DocPx,
    pub height:  DocPx,
    /// Where this cell came from: the index of its row in the table's children,
    /// and of the cell in that row's children. The caller needs it to find the
    /// [`Node`] whose content it must lay out — a placed cell is not the n-th
    /// cell of the row, because `merged` cells are skipped.
    pub source_row:  usize,
    pub source_cell: usize,
    pub v_align: VAlign,
    pub dir:     CellDir,
    pub background: CellFill,
    pub borders: CellBorders,
}

impl Cell {
    /// The width available to the cell's content, padding removed.
    pub fn inner_width(&self, grid: &Grid) -> DocPx {
        (self.width - grid.pad_left - grid.pad_right).max(0.0)
    }

    /// The height available to the cell's content, padding removed.
    pub fn inner_height(&self, grid: &Grid) -> DocPx {
        (self.height - grid.pad_top - grid.pad_bottom).max(0.0)
    }

    /// The width the cell's content must be laid out at.
    ///
    /// For a vertical cell that is [`VERTICAL_LAYOUT_WIDTH`], not the cell's
    /// own width: the text does not wrap, it is rotated afterwards, and its
    /// longest line becomes the cell's vertical extent (`:1490-1494`, `:1513`).
    pub fn layout_width(&self, grid: &Grid) -> DocPx {
        if self.dir.is_vertical() {
            VERTICAL_LAYOUT_WIDTH
        } else {
            self.inner_width(grid)
        }
    }

    /// Where the cell's content starts, given the height it laid out to
    /// (`:1576-1587`). The vertical slack goes below the content for `Top`,
    /// above it for `Bottom`, and is split for `Center`.
    pub fn content_origin(&self, grid: &Grid, content_height: DocPx) -> (DocPx, DocPx) {
        let slack = (self.inner_height(grid) - content_height).max(0.0);
        let dy = match self.v_align {
            VAlign::Top => 0.0,
            VAlign::Center => slack / 2.0,
            VAlign::Bottom => slack,
        };
        (self.x + grid.pad_left, self.y + grid.pad_top + dy)
    }
}

// ── The caller's half ────────────────────────────────────────────────────────

/// What the table engine needs to know about a cell's content.
///
/// Content layout belongs to the caller, because the engine that lays a cell
/// out has to be the same one that lays the body out — see the module comment.
/// The engine asks two questions and nothing else.
pub trait CellContent {
    /// The content's intrinsic widths — `(min, max)` — **without** cell
    /// padding, which the table adds itself because only it knows the padding.
    ///
    /// `min` is the widest unbreakable unit (plus the paragraph's indents),
    /// `max` is the content with no wrapping at all. [`intrinsic_widths`]
    /// implements the web's rule and is a usable default.
    ///
    /// Only asked for unmerged, `colspan: 1` cells (`:1426`): a spanning cell
    /// tells you nothing about any single column's width.
    fn intrinsic_widths(&mut self, cell: &Node) -> (DocPx, DocPx);

    /// The extent of the content along the cell's block axis, laid out at
    /// `width`.
    ///
    /// For an ordinary cell that is the laid-out height. **For a vertical cell
    /// it is the longest line's width** (`maxLineW`, `:1490-1494`): the cell is
    /// laid out unwrapped at [`VERTICAL_LAYOUT_WIDTH`] and then rotated, so it
    /// is the text's width that becomes the row's height.
    fn extent(&mut self, cell: &Node, width: DocPx) -> DocPx;
}

/// A [`CellContent`] that knows nothing: every cell is empty.
///
/// Used by [`layout`], and by anything that wants the table's geometry without
/// paying for its content. With it, `autofit` degenerates to equal columns —
/// honestly, because with no measurement there is no content width to fit.
/// Explicit `colWidths`, `fixed` layout, spans and `rowHeights` all still work.
pub struct NoContent;

impl CellContent for NoContent {
    fn intrinsic_widths(&mut self, _cell: &Node) -> (DocPx, DocPx) {
        (0.0, 0.0)
    }

    fn extent(&mut self, _cell: &Node, _width: DocPx) -> DocPx {
        0.0
    }
}

// ── Attribute reading ────────────────────────────────────────────────────────

/// JavaScript's `Number(x)` for the shapes that occur here: a JSON number, or a
/// numeric string. Anything else has no numeric value.
fn number(v: &Value) -> Option<f32> {
    match v {
        Value::Number(n) => n.as_f64().map(|n| n as f32),
        Value::String(s) => s.trim().parse::<f32>().ok(),
        _ => Option::None,
    }
}

/// `attrs.key`, treating `null` as absent — the web's `!= null` test.
fn attr<'a>(attrs: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    attrs?.get(key).filter(|v| !v.is_null())
}

/// `attrs.key != null ? Number(attrs.key) : default`, with one deliberate
/// divergence: a present-but-unparseable value falls back to the default
/// instead of producing `NaN`. In JavaScript that NaN propagates into every
/// coordinate derived from it; here one bad attribute cannot erase a table.
fn attr_num(attrs: Option<&Value>, key: &str, default: DocPx) -> DocPx {
    attr(attrs, key).and_then(number).filter(|v| v.is_finite()).unwrap_or(default)
}

/// `Number(attrs.key) || default` — the web's truthiness fallback, where 0 and
/// NaN both fall back.
fn attr_num_truthy(attrs: Option<&Value>, key: &str, default: DocPx) -> DocPx {
    attr(attrs, key)
        .and_then(number)
        .filter(|v| v.is_finite() && *v != 0.0)
        .unwrap_or(default)
}

fn attr_str(attrs: Option<&Value>, key: &str) -> Option<String> {
    attr(attrs, key).and_then(Value::as_str).map(str::to_string)
}

fn attr_bool(attrs: Option<&Value>, key: &str) -> bool {
    // `!!attrs.key`: any truthy JSON value, not just `true`.
    match attr(attrs, key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|v| v != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
        _ => false,
    }
}

/// An array attribute of numbers. Entries that are not numbers become `NaN`,
/// which every downstream test (`> 4`) then rejects — the same outcome as the
/// web's `Number(undefined)`.
fn attr_num_array(attrs: Option<&Value>, key: &str) -> Option<Vec<DocPx>> {
    match attr(attrs, key) {
        Some(Value::Array(items)) => {
            Some(items.iter().map(|v| number(v).unwrap_or(f32::NAN)).collect())
        }
        _ => Option::None,
    }
}

/// The node's `attrs`, parsed. Kept as a `Value` so that `null` stays
/// distinguishable from an absent key, which is the whole point of this file.
fn attrs_of(node: &Node) -> Option<Value> {
    serde_json::from_str(node.raw("attrs")?.get()).ok()
}

/// A cell's own attributes (`DocumentEditorPage.tsx:1484-1500`).
#[derive(Clone, Debug, Default)]
struct CellAttrs {
    colspan: usize,
    rowspan: usize,
    merged:  bool,
    bg:      Option<String>,
    v_align: VAlign,
    dir:     CellDir,
    borders: CellBorders,
}

impl CellAttrs {
    fn read(cell: &Node) -> Self {
        let attrs = attrs_of(cell);
        let attrs = attrs.as_ref();
        // `Math.max(1, Number(x) || 1)` (`:788-789`): both spans floor at 1.
        let span = |key: &str| attr_num_truthy(attrs, key, 1.0).max(1.0) as usize;
        Self {
            colspan: span("colspan"),
            rowspan: span("rowspan"),
            merged:  attr_bool(attrs, "merged"),
            bg:      attr_str(attrs, "cellBg"),
            v_align: match attr(attrs, "cellVAlign").and_then(Value::as_str) {
                Some("center") => VAlign::Center,
                Some("bottom") => VAlign::Bottom,
                _ => VAlign::Top,
            },
            dir: match attr_num_truthy(attrs, "cellDir", 0.0) as i32 {
                90 => CellDir::Down,
                270 => CellDir::Up,
                _ => CellDir::Horizontal,
            },
            borders: CellBorders::parse(attrs),
        }
    }
}

/// The table's own attributes (`DocumentEditorPage.tsx:1512-1560`), resolved to
/// the values `layoutTable` actually uses.
struct TableAttrs {
    col_widths:  Option<Vec<DocPx>>,
    row_heights: Option<Vec<DocPx>>,
    row_modes:   Option<Vec<RowHeightMode>>,
    fixed:       bool,
    grid:        Grid,
}

impl TableAttrs {
    fn read(table: &Node) -> Self {
        let attrs = attrs_of(table);
        let attrs = attrs.as_ref();
        let style = TableStyle::parse(attr(attrs, "tableStyle"));
        let header_repeat = attr_bool(attrs, "headerRepeat");
        let grid = Grid {
            style,
            // `table.accent || '#1a73e8'`: an empty string falls back too.
            accent: attr_str(attrs, "accent")
                .filter(|a| !a.is_empty())
                .unwrap_or_else(|| DEFAULT_ACCENT.to_string()),
            // `tableStyle: 'plain'` means there is no table default at all.
            default_border: (style != TableStyle::Plain).then(|| {
                BorderSpec::table_default(
                    attr(attrs, "tableBorderWidth").and_then(number).filter(|w| w.is_finite()),
                    BorderStyle::parse(attr(attrs, "tableBorderStyle")),
                    attr_str(attrs, "tableBorderColor"),
                )
            }),
            pad_left:   attr_num(attrs, "cellMarginLeft", CELL_PAD_X),
            pad_right:  attr_num(attrs, "cellMarginRight", CELL_PAD_X),
            pad_top:    attr_num(attrs, "cellMarginTop", CELL_PAD_Y),
            pad_bottom: attr_num(attrs, "cellMarginBottom", CELL_PAD_Y),
            spacing:    attr_num_truthy(attrs, "cellSpacing", 0.0).max(0.0),
            align: match attr(attrs, "tableAlign").and_then(Value::as_str) {
                Some("center") => TableAlign::Center,
                Some("right") => TableAlign::Right,
                _ => TableAlign::Left,
            },
            indent: attr_num_truthy(attrs, "tableIndent", 0.0),
            wrap: match attr(attrs, "tableWrap").and_then(Value::as_str) {
                Some("around") => TableWrap::Around,
                _ => TableWrap::None,
            },
            header_repeat,
            // `Math.max(0, Number(headerRows) || (headerRepeat ? 1 : 0))`.
            header_rows: attr_num_truthy(attrs, "headerRows", f32::from(u8::from(header_repeat)))
                .max(0.0) as usize,
            ..Grid::default()
        };
        Self {
            col_widths:  attr_num_array(attrs, "colWidths"),
            row_heights: attr_num_array(attrs, "rowHeights"),
            row_modes:   attr(attrs, "rowHeightModes").and_then(|v| match v {
                Value::Array(items) => Some(
                    items
                        .iter()
                        .map(|m| match m.as_str() {
                            Some("exactly") => RowHeightMode::Exactly,
                            _ => RowHeightMode::AtLeast,
                        })
                        .collect(),
                ),
                _ => Option::None,
            }),
            fixed: attr(attrs, "tableLayout").and_then(Value::as_str) == Some("fixed"),
            grid,
        }
    }
}

// ── Column widths ────────────────────────────────────────────────────────────

/// Whether `colWidths` may be used as preferred widths (`:1434`): it must exist,
/// have exactly one entry per column, and every entry must be a real width.
fn widths_are_explicit(col_widths: Option<&Vec<DocPx>>, col_count: usize) -> bool {
    col_widths
        .is_some_and(|w| w.len() == col_count && w.iter().all(|v| *v > COL_WIDTH_EPSILON))
}

/// CSS auto-table-layout, the web's `autofitWidths` (`:1417-1463`) — and not
/// "equal columns": Word widens a column at its neighbours' expense before it
/// wraps anything, and only wraps once the neighbours are at their minimum.
fn autofit_widths(
    mins: &[DocPx],
    maxs: &[DocPx],
    col_widths: Option<&Vec<DocPx>>,
    content_w: DocPx,
) -> Vec<DocPx> {
    let col_count = mins.len();
    let explicit = widths_are_explicit(col_widths, col_count);
    let pref: Vec<DocPx> = if explicit {
        // `explicit` guarantees the length, so the index is in range; the
        // fallback keeps this total rather than relying on that.
        col_widths
            .map(|w| w.iter().zip(mins).map(|(w, m)| w.max(*m)).collect())
            .unwrap_or_else(|| mins.to_vec())
    } else {
        maxs.iter().zip(mins).map(|(w, m)| w.max(*m)).collect()
    };
    let sum: DocPx = pref.iter().sum();

    if sum <= content_w {
        // Hand-set widths are respected as they are: the table is allowed to be
        // narrower than the text column (`:1444`). Without explicit widths the
        // slack is spread equally so the table fills the column, like a table
        // inserted in Word.
        if explicit {
            return pref;
        }
        let extra = (content_w - sum) / col_count as f32;
        return pref.iter().map(|w| w + extra).collect();
    }

    // Too wide: trim in proportion to each column's slack (preferred minus
    // minimum), so a column already at its minimum does not move — that is what
    // makes text wrap instead of a column collapsing to nothing.
    let slack: Vec<DocPx> = pref.iter().zip(mins).map(|(w, m)| (w - m).max(0.0)).collect();
    let total_slack: DocPx = slack.iter().sum();
    if total_slack <= 0.0 {
        let k = content_w / sum;
        return pref.iter().map(|w| w * k).collect();
    }
    let cut = (sum - content_w).min(total_slack);
    let out: Vec<DocPx> =
        pref.iter().zip(&slack).map(|(w, s)| w - (s / total_slack) * cut).collect();
    let s2: DocPx = out.iter().sum();
    if s2 > content_w {
        // Still too wide — every column is at its minimum — so scale the whole
        // row down homothetically.
        out.iter().map(|w| w * (content_w / s2)).collect()
    } else {
        out
    }
}

/// `tableLayout: 'fixed'` (`:1476-1483`). A ragged `colWidths` is **repaired,
/// not ignored**: missing entries take the average of the known ones and
/// surplus entries are dropped. Ignoring the array wholesale used to flip the
/// whole table to uniform columns in one step.
fn fixed_widths(col_widths: Option<&Vec<DocPx>>, col_count: usize, content_w: DocPx) -> Vec<DocPx> {
    let usable = col_widths.filter(|w| !w.is_empty() && w.iter().any(|v| *v > COL_WIDTH_EPSILON));
    let Some(cw) = usable else {
        return vec![content_w / col_count as f32; col_count];
    };
    let known: Vec<DocPx> = cw.iter().copied().filter(|v| *v > COL_WIDTH_EPSILON).collect();
    let avg: DocPx = known.iter().sum::<DocPx>() / known.len() as f32;
    let mut widths: Vec<DocPx> = (0..col_count)
        .map(|i| match cw.get(i) {
            Some(w) if *w > COL_WIDTH_EPSILON => *w,
            _ => avg,
        })
        .collect();
    let sum: DocPx = widths.iter().sum();
    if sum > content_w {
        let k = content_w / sum;
        for w in &mut widths {
            *w *= k;
        }
    }
    widths
}

// ── Layout ───────────────────────────────────────────────────────────────────

/// A cell that has found its place in the grid.
struct Placed {
    row:     usize,
    column:  usize,
    colspan: usize,
    rowspan: usize,
    source_cell: usize,
    attrs:   CellAttrs,
    /// The content's extent along the block axis, padding excluded.
    extent:  DocPx,
}

/// The width of the `cspan` columns starting at `c0`, clamped to the grid —
/// the web's `cellW` (`:1486`), which reads the cumulative boundary array.
fn span_width(widths: &[DocPx], c0: usize, cspan: usize) -> DocPx {
    let end = (c0 + cspan).min(widths.len());
    widths[c0.min(end)..end].iter().sum()
}

/// Lays a `table` node out into a grid, given the column the table sits in.
///
/// Geometry only: with no [`CellContent`] the cells are treated as empty, so
/// `autofit` columns come out equal and rows come out at their declared or
/// minimum height. Use [`layout_with`] for a table with content in it.
pub fn layout(table: &Node, available: DocPx) -> (Grid, Vec<Cell>) {
    layout_with(table, available, &mut NoContent)
}

/// Lays a `table` node out, asking `content` about each cell.
///
/// Ported from `layoutTable` (`canvas-engine.ts:1363-1592`).
pub fn layout_with(
    table: &Node,
    available: DocPx,
    content: &mut dyn CellContent,
) -> (Grid, Vec<Cell>) {
    let content_w = available.max(1.0);
    let spec = TableAttrs::read(table);
    let mut grid = spec.grid;
    let rows = table.children();

    // ── Column count. `colsInRow += colspan` runs over EVERY cell including
    // the `merged` ones that placement then skips (`:790` vs `:1506`), so a
    // table with absorbed cells really does grow phantom trailing columns.
    // Parity first: this is reproduced, not corrected.
    let row_attrs: Vec<Vec<CellAttrs>> =
        rows.iter().map(|r| r.children().iter().map(CellAttrs::read).collect()).collect();
    let col_count = row_attrs
        .iter()
        .map(|cells| cells.iter().map(|c| c.colspan).sum::<usize>())
        .max()
        .unwrap_or(0)
        .max(1);

    // ── Intrinsic widths, for `autofit` only: one measurement pass over the
    // unmerged, single-column cells (`:1422-1433`). Note the column cursor here
    // walks EVERY cell including merged ones and does NOT skip columns held by
    // a rowspan — unlike placement below. That asymmetry is the web's.
    let mut widths = if spec.fixed {
        fixed_widths(spec.col_widths.as_ref(), col_count, content_w)
    } else {
        let mut mins = vec![MIN_COL_W; col_count];
        let mut maxs = vec![MIN_COL_W; col_count];
        let pad_x = grid.pad_left + grid.pad_right;
        for (row, attrs) in rows.iter().zip(&row_attrs) {
            let mut c = 0usize;
            for (cell, a) in row.children().iter().zip(attrs) {
                if !a.merged && a.colspan == 1 && c < col_count {
                    let (min, max) = content.intrinsic_widths(cell);
                    mins[c] = mins[c].max((min + pad_x).min(content_w));
                    maxs[c] = maxs[c].max((max + pad_x).min(content_w));
                }
                c += a.colspan;
            }
        }
        autofit_widths(&mins, &maxs, spec.col_widths.as_ref(), content_w)
    };
    for w in &mut widths {
        // A non-finite width would poison every coordinate downstream. The
        // arithmetic above cannot produce one from finite inputs, but the
        // inputs come from a document.
        if !w.is_finite() {
            *w = MIN_COL_W;
        }
    }

    let mut col_x = Vec::with_capacity(col_count + 1);
    let mut acc = 0.0;
    col_x.push(0.0);
    for w in &widths {
        acc += *w;
        col_x.push(acc);
    }
    let table_w = acc;

    // ── Placement. The occupancy map carries a rowspan down into later rows,
    // and `merged` cells are skipped entirely (`:1501-1519`).
    let mut placed: Vec<Placed> = Vec::new();
    let mut occupied = vec![0usize; col_count];
    for (r, (row, attrs)) in rows.iter().zip(&row_attrs).enumerate() {
        let mut c = 0usize;
        for (source_cell, (cell, a)) in row.children().iter().zip(attrs).enumerate() {
            if a.merged {
                continue;
            }
            while c < col_count && occupied[c] > 0 {
                c += 1;
            }
            if c >= col_count {
                break;
            }
            let colspan = a.colspan.min(col_count - c);
            let rowspan = a.rowspan.max(1);
            let vertical = a.dir.is_vertical();
            let width = if vertical {
                VERTICAL_LAYOUT_WIDTH
            } else {
                span_width(&widths, c, colspan) - grid.pad_left - grid.pad_right - grid.spacing
            };
            let extent = content.extent(cell, width);
            placed.push(Placed {
                row: r,
                column: c,
                colspan,
                rowspan,
                source_cell,
                attrs: a.clone(),
                extent: if extent.is_finite() { extent.max(0.0) } else { 0.0 },
            });
            occupied[c..c + colspan].fill(rowspan);
            c += colspan;
        }
        for o in &mut occupied {
            *o = o.saturating_sub(1);
        }
    }

    // ── Row heights (`:1521-1534`).
    let row_count = rows.len();
    let modes: Vec<RowHeightMode> = (0..row_count)
        .map(|i| spec.row_modes.as_ref().and_then(|m| m.get(i)).copied().unwrap_or_default())
        .collect();
    let pad_y = grid.pad_top + grid.pad_bottom + grid.spacing;
    let mut row_h: Vec<DocPx> = (0..row_count)
        .map(|i| {
            let declared = spec
                .row_heights
                .as_ref()
                .and_then(|h| h.get(i))
                .copied()
                .filter(|v| v.is_finite())
                .unwrap_or(0.0);
            // `exactly` with a positive declared height is frozen; everything
            // else is a minimum, floored by MIN_ROW_H.
            if modes[i] == RowHeightMode::Exactly && declared > 0.0 {
                declared
            } else {
                declared.max(MIN_ROW_H)
            }
        })
        .collect();
    for p in &placed {
        if p.rowspan == 1 && modes[p.row] != RowHeightMode::Exactly {
            row_h[p.row] = row_h[p.row].max(p.extent + pad_y);
        }
    }
    for p in &placed {
        if p.rowspan <= 1 {
            continue;
        }
        // A spanning cell that does not fit adds its deficit to the LAST row it
        // covers — unless that row is `exactly`, which nothing may stretch.
        let end = (p.row + p.rowspan).min(row_count);
        let have: DocPx = row_h[p.row..end].iter().sum();
        let need = p.extent + pad_y;
        let last = (p.row + p.rowspan - 1).min(row_count.saturating_sub(1));
        if need > have && modes.get(last).copied().unwrap_or_default() != RowHeightMode::Exactly {
            row_h[last] += need - have;
        }
    }

    let mut row_y = Vec::with_capacity(row_count + 1);
    let mut acc = 0.0;
    row_y.push(0.0);
    for h in &row_h {
        acc += *h;
        row_y.push(acc);
    }
    let table_h = acc;

    // ── Horizontal placement of the table itself (`:1538-1543`).
    let x_offset = match grid.align {
        TableAlign::Center => ((content_w - table_w) / 2.0).max(0.0),
        TableAlign::Right => (content_w - table_w).max(0.0),
        TableAlign::Left => grid.indent.max(0.0),
    };
    for x in &mut col_x {
        *x += x_offset;
    }

    // ── Boxes (`:1546-1558`). `cellSpacing` shrinks every cell by half of it on
    // all four sides, which is what un-shares the borders.
    let sp = grid.spacing;
    let cells: Vec<Cell> = placed
        .iter()
        .map(|p| {
            let end = (p.row + p.rowspan).min(row_count);
            let h: DocPx = row_h[p.row..end].iter().sum();
            let w = span_width(&widths, p.column, p.colspan);
            Cell {
                row: p.row,
                column: p.column,
                rowspan: p.rowspan,
                colspan: p.colspan,
                x: col_x[p.column] + sp / 2.0,
                y: row_y[p.row] + sp / 2.0,
                width: (w - sp).max(1.0),
                height: (h - sp).max(1.0),
                source_row: p.row,
                source_cell: p.source_cell,
                v_align: p.attrs.v_align,
                dir: p.attrs.dir,
                background: fill_of(&p.attrs, p.row, grid.style),
                borders: p.attrs.borders.clone(),
            }
        })
        .collect();

    grid.column_widths = widths;
    grid.row_heights = row_h;
    grid.row_height_modes = modes;
    grid.col_x = col_x;
    grid.row_y = row_y;
    grid.width = table_w;
    grid.height = table_h;
    grid.x_offset = x_offset;
    (grid, cells)
}

/// A cell's background (`:1552-1556`): an explicit `cellBg` beats everything,
/// then the style's header band and striped rows.
fn fill_of(attrs: &CellAttrs, row: usize, style: TableStyle) -> CellFill {
    if let Some(bg) = &attrs.bg {
        return CellFill::Explicit(bg.clone());
    }
    if row == 0 && matches!(style, TableStyle::Header | TableStyle::Striped) {
        return CellFill::Tint(HEADER_TINT_ALPHA);
    }
    if style == TableStyle::Striped && row % 2 == 1 {
        return CellFill::Tint(STRIPE_TINT_ALPHA);
    }
    CellFill::None
}

// ── Borders ──────────────────────────────────────────────────────────────────

/// One resolved edge of the grid, ready to stroke.
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub x0: DocPx,
    pub y0: DocPx,
    pub x1: DocPx,
    pub y1: DocPx,
    pub spec: BorderSpec,
}

struct EdgeWin {
    spec: BorderSpec,
    weight: u8,
}

/// `pickEdge` (`:2699-2704`): higher weight wins; on a tie the thicker wins,
/// and a tie between equals keeps the one already there.
fn better(prev: EdgeWin, cand: EdgeWin) -> EdgeWin {
    if prev.weight != cand.weight {
        if cand.weight > prev.weight {
            cand
        } else {
            prev
        }
    } else if cand.spec.width > prev.spec.width {
        cand
    } else {
        prev
    }
}

/// Resolves the table's borders **per edge** and returns what to stroke.
///
/// This is the three-state rule in action (`paintTableBorders`, `:2705-2785`).
/// Two adjacent cells contribute the same edge — same rounded geometry, same
/// key — so the edge is resolved once and stroked once. Resolving is what stops
/// a shared edge being drawn twice (doubled dashes, translucent colour) and
/// what lets a cell with `null` on one side stop its neighbour's *inherited*
/// default from redrawing it.
///
/// Device-pixel snapping and the minimum one-device-pixel stroke width
/// (`:2754-2773`) belong to the painter: they depend on the transform, which
/// this module never sees.
pub fn edges(grid: &Grid, cells: &[Cell]) -> Vec<Edge> {
    // Rounded to 1/100 px, as the web's key is (`:2714`).
    let key = |a: DocPx, b: DocPx, c: DocPx, d: DocPx| {
        let k = |v: DocPx| (v * 100.0).round() as i64;
        (k(a), k(b), k(c), k(d))
    };
    let mut index: HashMap<(i64, i64, i64, i64), usize> = HashMap::new();
    // A Vec beside the map, so the output order is the order the edges were
    // first seen rather than a hash order — a painter that batches by brush
    // must be able to produce the same picture twice.
    let mut found: Vec<(DocPx, DocPx, DocPx, DocPx, EdgeWin)> = Vec::new();

    for cell in cells {
        let (x0, y0) = (cell.x, cell.y);
        let (x1, y1) = (cell.x + cell.width, cell.y + cell.height);
        let sides = [
            (x0, y0, x1, y0, &cell.borders.top),
            (x0, y1, x1, y1, &cell.borders.bottom),
            (x0, y0, x0, y1, &cell.borders.left),
            (x1, y0, x1, y1, &cell.borders.right),
        ];
        for (ax, ay, bx, by, side) in sides {
            let cand = match side {
                // Absent: the table default, if the style has one at all.
                BorderSide::Inherit => match &grid.default_border {
                    Some(spec) => EdgeWin { spec: spec.clone(), weight: 0 },
                    Option::None => continue,
                },
                // Explicit "none": recorded even though it paints nothing, so
                // that it can evict the neighbour's inherited default.
                BorderSide::None => EdgeWin { spec: BorderSpec::veto(), weight: 1 },
                BorderSide::Set(spec) => EdgeWin { spec: spec.clone(), weight: 2 },
            };
            let k = key(ax, ay, bx, by);
            match index.get(&k) {
                Some(&i) => {
                    let prev = std::mem::replace(
                        &mut found[i].4,
                        EdgeWin { spec: BorderSpec::veto(), weight: 0 },
                    );
                    found[i].4 = better(prev, cand);
                }
                Option::None => {
                    index.insert(k, found.len());
                    found.push((ax, ay, bx, by, cand));
                }
            }
        }
    }

    found
        .into_iter()
        .filter(|(_, _, _, _, win)| win.weight != 1 && win.spec.width > 0.0)
        .map(|(x0, y0, x1, y1, win)| Edge { x0, y0, x1, y1, spec: win.spec })
        .collect()
}

// ── Page splitting: the seam ─────────────────────────────────────────────────

/// The cells of a table that fall in one page's band, in page-local
/// coordinates (`canvas-engine.ts:3421-3425`).
///
/// This is **all** this module does about page splitting. Deciding *where* a
/// table breaks is pagination's job and is line-based, not row-based, in the
/// web: the cell text lines are ordinary lines and the break lands wherever the
/// column runs out, so a row is split mid-row. When that lands, it calls this
/// with the band it kept; the geometry itself never changes.
///
/// Cells entirely outside the band are dropped rather than clipped: without
/// that filter, the rows belonging to other pages paint their borders and fills
/// into this page's margins. Clipping proper is the painter's, with the
/// `CELL_PAD_Y + 1` bleed that keeps a page-leading table's top border visible
/// (`:2529-2539`).
pub fn cells_in_band(cells: &[Cell], top: DocPx, height: DocPx) -> Vec<Cell> {
    cells
        .iter()
        .map(|c| Cell { y: c.y - top, ..c.clone() })
        .filter(|c| c.y + c.height > 0.5 && c.y < height - 0.5)
        .collect()
}

// ── Intrinsic widths ─────────────────────────────────────────────────────────

/// JavaScript's `\s`, the crate's one definition. The web splits on `/(\s+)/`
/// here too (`:1399`), and the set is not Rust's `char::is_whitespace`: U+00A0
/// breaks, U+200B does not.
use super::paragraph::is_js_space;

/// The intrinsic widths of a cell's content — the web's `contentWidths`
/// (`:1388-1412`), padding excluded.
///
/// `min` is the widest unbreakable unit plus the paragraph's indents; `max` is
/// the content laid end to end with no wrapping. Both are measured on spans
/// with no layout at all, which is what makes an `autofit` table affordable.
///
/// Three known gaps, all of them narrowing a column rather than inventing a
/// number, and all of them fixable by a caller that implements
/// [`CellContent`] over the real converter:
///
/// * A footnote or endnote mark contributes **nothing**. The web measures the
///   mark's actual text, which is a document-wide auto-number (`:874`, `:890`)
///   that does not exist at this level.
/// * A heading in a cell is measured at the body size unless its runs carry an
///   explicit `fontSize`: the heading size table lives in `doc::convert` and is
///   private to it.
/// * A `blockquote` or a `taskList` contributes nothing — the web's parser
///   emits no spans for them at all (`:1012-1014`), so neither does this.
///
/// A block `image` contributes nothing either, but that is parity rather than a
/// gap: the web's image paragraph carries no spans, so a full-width image in a
/// cell does not widen its column.
pub fn intrinsic_widths(cell: &Node, m: &dyn Measure) -> (DocPx, DocPx) {
    let mut min = 0.0f32;
    let mut max = 0.0f32;
    measure_blocks(cell, m, &mut min, &mut max);
    (min, max)
}

fn measure_blocks(parent: &Node, m: &dyn Measure, min: &mut f32, max: &mut f32) {
    for block in parent.children() {
        match block.node_type() {
            Some("paragraph") | Some("heading") | Some("codeBlock") => {
                measure_paragraph(block, m, min, max)
            }
            // A nested table produces a paragraph with no spans of its own, so
            // it contributes nothing to the outer column's width.
            Some("table") => {}
            Some("horizontalRule") => {
                let style = TextMark { font_size: Some(RULE_SIZE_PT), ..Default::default() };
                let rule: String = "─".repeat(RULE_CHARS);
                let w = m.width(&rule, &style);
                *min = min.max(w);
                *max = max.max(w);
            }
            // Lists are recursed into (`:986-995`), like the web's parser.
            Some("bulletList") | Some("orderedList") | Some("listItem") => {
                measure_blocks(block, m, min, max)
            }
            _ => {}
        }
    }
}

fn measure_paragraph(para: &Node, m: &dyn Measure, min: &mut f32, max: &mut f32) {
    let attrs = attrs_of(para);
    let attrs = attrs.as_ref();
    let pad = attr_num_truthy(attrs, "indent", 0.0) + attr_num_truthy(attrs, "indentRight", 0.0);
    let base = TextMark::default();

    let mut sum = 0.0f32;
    let mut word = 0.0f32;
    for inline in para.children() {
        match inline.node_type() {
            Some("text") => {
                let Some(text) = inline.text() else { continue };
                let style = style_of(inline, &base);
                for part in split_keeping_spaces(&text) {
                    let w = m.width(part, &style);
                    sum += w;
                    if part.starts_with(is_js_space) {
                        // A whitespace run closes the current word.
                        *min = min.max(word + pad);
                        word = 0.0;
                    } else {
                        word += w;
                    }
                }
            }
            // An inline image is an atom: it bounds the column's minimum the
            // way an unbreakable word does (`:1394`).
            Some("inlineImage") => {
                let a = attrs_of(inline);
                let w = attr_num_truthy(a.as_ref(), "width", 0.0).max(1.0);
                sum += w;
                *min = min.max(w + pad);
                word = 0.0;
            }
            // A field is an atom too, measured on its cached result (`:1397`).
            Some("field") => {
                let a = attrs_of(inline);
                let text = attr_str(a.as_ref(), "cached").unwrap_or_default();
                let w = m.width(&text, &style_of(inline, &base));
                sum += w;
                *min = min.max(w + pad);
                word = 0.0;
            }
            // `hardBreak` emits no span at all (`:848-849`), so it neither adds
            // width nor closes the current word.
            _ => {}
        }
    }
    *min = min.max(word + pad);
    *max = max.max(sum + pad);
}

/// Splits on maximal whitespace runs, keeping the separators — JavaScript's
/// `split(/(\s+)/)` minus its empty pieces.
fn split_keeping_spaces(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut current: Option<bool> = Option::None;
    for (i, c) in text.char_indices() {
        let space = is_js_space(c);
        match current {
            Some(prev) if prev == space => {}
            Some(_) => {
                out.push(&text[start..i]);
                start = i;
            }
            Option::None => start = i,
        }
        current = Some(space);
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// The run style of a text node, from its marks (the one mark reader, `crate::marks`).
fn style_of(text_node: &Node, _base: &TextMark) -> TextMark {
    crate::marks::text_mark_of(text_node)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Fixtures ─────────────────────────────────────────────────────────────

    fn node(json: &str) -> Node {
        Node::from_slice(json.as_bytes()).expect("fixture parses")
    }

    /// A table of `rows` rows, each holding the given cell JSON fragments.
    fn table(attrs: &str, rows: &[&[&str]]) -> Node {
        let rows: Vec<String> = rows
            .iter()
            .map(|cells| format!(r#"{{"content":[{}],"type":"tableRow"}}"#, cells.join(",")))
            .collect();
        node(&format!(
            r#"{{"attrs":{attrs},"content":[{}],"type":"table"}}"#,
            rows.join(",")
        ))
    }

    /// A cell with the given attrs and no content.
    fn cell(attrs: &str) -> String {
        format!(r#"{{"attrs":{attrs},"content":[],"type":"tableCell"}}"#)
    }

    /// Every character is half its point size wide; ascent/descent are fixed.
    /// Nothing here depends on a real font, which is the point.
    struct Ruler;

    impl Measure for Ruler {
        fn width(&self, text: &str, style: &TextMark) -> f32 {
            text.chars().count() as f32 * style.size_pt() * 0.5
        }

        fn ascent_descent(&self, style: &TextMark) -> (f32, f32) {
            (style.size_pt() * 0.8, style.size_pt() * 0.2)
        }
    }

    /// Content whose intrinsic widths and extent are read from test-only
    /// attributes, so a test can state the content a cell has without also
    /// having to own a paragraph engine.
    struct Stub;

    impl Stub {
        fn num(cell: &Node, key: &str) -> f32 {
            let attrs = attrs_of(cell);
            attr_num(attrs.as_ref(), key, 0.0)
        }
    }

    impl CellContent for Stub {
        fn intrinsic_widths(&mut self, cell: &Node) -> (DocPx, DocPx) {
            (Self::num(cell, "tMin"), Self::num(cell, "tMax"))
        }

        fn extent(&mut self, cell: &Node, _width: DocPx) -> DocPx {
            Self::num(cell, "tH")
        }
    }

    /// Records the width each cell was asked to lay out at.
    #[derive(Default)]
    struct Recorder {
        widths: Vec<DocPx>,
    }

    impl CellContent for Recorder {
        fn intrinsic_widths(&mut self, _cell: &Node) -> (DocPx, DocPx) {
            (0.0, 0.0)
        }

        fn extent(&mut self, _cell: &Node, width: DocPx) -> DocPx {
            self.widths.push(width);
            0.0
        }
    }

    fn near(a: DocPx, b: DocPx) -> bool {
        (a - b).abs() < 0.01
    }

    // ── The three states of `cellBorders` ────────────────────────────────────

    #[test]
    fn the_three_cell_border_states_stay_three() {
        // The single most likely silent corruption in the whole slice: absent,
        // explicit null, and an object are three different documents.
        let c = node(
            r##"{"attrs":{"cellBorders":{"b":null,"t":{"c":"#f00","s":"dashed","w":2}}},"type":"tableCell"}"##,
        );
        let b = CellBorders::parse(attrs_of(&c).as_ref());
        assert_eq!(b.top, BorderSide::Set(BorderSpec {
            width: 2.0,
            style: BorderStyle::Dashed,
            color: "#f00".into(),
        }));
        assert_eq!(b.bottom, BorderSide::None, "an explicit null is not an absent key");
        assert_eq!(b.left, BorderSide::Inherit, "an absent key is not an explicit null");
        assert_eq!(b.right, BorderSide::Inherit);
        assert_eq!((b.top.weight(), b.bottom.weight(), b.left.weight()), (2, 1, 0));
    }

    #[test]
    fn an_absent_cell_borders_object_is_not_a_row_of_explicit_nones() {
        for json in [
            r#"{"attrs":{},"type":"tableCell"}"#,
            r#"{"attrs":{"cellBorders":null},"type":"tableCell"}"#,
            r#"{"attrs":{"cellBorders":{}},"type":"tableCell"}"#,
            r#"{"type":"tableCell"}"#,
        ] {
            let b = CellBorders::parse(attrs_of(&node(json)).as_ref());
            assert_eq!(b, CellBorders::default(), "for {json}");
            assert_eq!(b.top.weight(), 0, "for {json}");
        }
    }

    #[test]
    fn an_explicit_none_side_vetoes_the_neighbours_inherited_default() {
        // Two cells side by side. The left one says "no border on my right".
        // The right one says nothing, so it would inherit the table default
        // onto the SAME edge. The veto must win, or turning a border off does
        // nothing visible.
        let t = table(
            r#"{"colWidths":[100,100],"tableLayout":"fixed"}"#,
            &[&[&cell(r#"{"cellBorders":{"r":null}}"#), &cell("{}")]],
        );
        let (grid, cells) = layout(&t, 400.0);
        let shared = grid.col_x[1];
        let painted = edges(&grid, &cells);
        assert!(
            !painted.iter().any(|e| e.x0 == e.x1 && near(e.x0, shared)),
            "the shared edge must not be painted: {painted:?}"
        );
        // The outer edges still are — the veto is one side, not the table.
        assert!(painted.iter().any(|e| e.x0 == e.x1 && near(e.x0, grid.col_x[0])));
    }

    #[test]
    fn an_explicit_border_beats_a_neighbours_veto() {
        let t = table(
            r#"{"colWidths":[100,100],"tableLayout":"fixed"}"#,
            &[&[
                &cell(r#"{"cellBorders":{"r":null}}"#),
                &cell(r##"{"cellBorders":{"l":{"c":"#0f0","s":"solid","w":3}}}"##),
            ]],
        );
        let (grid, cells) = layout(&t, 400.0);
        let shared = grid.col_x[1];
        let edge = edges(&grid, &cells)
            .into_iter()
            .find(|e| e.x0 == e.x1 && near(e.x0, shared))
            .expect("an explicit border outranks a veto");
        assert_eq!(edge.spec.color, "#0f0");
        assert_eq!(edge.spec.width, 3.0);
    }

    #[test]
    fn the_thicker_of_two_explicit_borders_wins_the_shared_edge() {
        let t = table(
            r#"{"colWidths":[100,100],"tableLayout":"fixed"}"#,
            &[&[
                &cell(r##"{"cellBorders":{"r":{"c":"#thin","s":"solid","w":1}}}"##),
                &cell(r##"{"cellBorders":{"l":{"c":"#thick","s":"solid","w":4}}}"##),
            ]],
        );
        let (grid, cells) = layout(&t, 400.0);
        let edge = edges(&grid, &cells)
            .into_iter()
            .find(|e| e.x0 == e.x1 && near(e.x0, grid.col_x[1]))
            .expect("the shared edge exists");
        assert_eq!(edge.spec.color, "#thick");
    }

    #[test]
    fn a_shared_edge_is_resolved_once_and_never_stroked_twice() {
        // Drawing each cell's whole box painted every interior edge twice,
        // which showed up as doubled dashes and translucent colour.
        let t = table(
            r#"{"colWidths":[60,60,60],"tableLayout":"fixed"}"#,
            &[&[&cell("{}"), &cell("{}"), &cell("{}")], &[&cell("{}"), &cell("{}"), &cell("{}")]],
        );
        let (grid, cells) = layout(&t, 400.0);
        let painted = edges(&grid, &cells);
        // 3 columns x 2 rows: 4 vertical lines x 2 rows + 3 horizontal x 3 rows
        // of boundaries = 8 + 9 segments, each exactly once.
        assert_eq!(painted.len(), 17);
        let mut seen: Vec<(i64, i64, i64, i64)> = painted
            .iter()
            .map(|e| {
                let k = |v: DocPx| (v * 100.0).round() as i64;
                (k(e.x0), k(e.y0), k(e.x1), k(e.y1))
            })
            .collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), before, "an edge was emitted twice");
    }

    #[test]
    fn a_plain_table_paints_only_the_borders_a_cell_asked_for() {
        let t = table(
            r#"{"colWidths":[100,100],"tableLayout":"fixed","tableStyle":"plain"}"#,
            &[&[&cell(r##"{"cellBorders":{"t":{"c":"#123","s":"solid","w":1}}}"##), &cell("{}")]],
        );
        let (grid, cells) = layout(&t, 400.0);
        assert!(grid.default_border.is_none(), "'plain' has no table default at all");
        let painted = edges(&grid, &cells);
        assert_eq!(painted.len(), 1);
        assert_eq!(painted[0].spec.color, "#123");
    }

    #[test]
    fn the_table_default_border_falls_back_to_the_webs_values() {
        let (grid, _) = layout(&table("{}", &[&[&cell("{}")]]), 400.0);
        let def = grid.default_border.expect("a grid table has a default border");
        assert_eq!(def.width, DEFAULT_BORDER_WIDTH);
        assert_eq!(def.color, DEFAULT_BORDER_COLOR);
        assert_eq!(def.style, BorderStyle::Solid);
    }

    // ── Row heights ──────────────────────────────────────────────────────────

    #[test]
    fn an_exactly_row_does_not_grow_to_fit_its_content() {
        // The whole point of `exactly`: a row dragged to a height keeps it, and
        // the content is clipped rather than the row stretched.
        let t = table(
            r#"{"rowHeightModes":["exactly"],"rowHeights":[40]}"#,
            &[&[&cell(r#"{"tH":300}"#)]],
        );
        let (grid, cells) = layout_with(&t, 400.0, &mut Stub);
        assert_eq!(grid.row_heights, vec![40.0]);
        assert_eq!(cells[0].height, 40.0);
    }

    #[test]
    fn an_atleast_row_grows_to_its_tallest_cell() {
        let t = table(
            r#"{"rowHeightModes":["atleast"],"rowHeights":[40]}"#,
            &[&[&cell(r#"{"tH":30}"#), &cell(r#"{"tH":100}"#)]],
        );
        let (grid, _) = layout_with(&t, 400.0, &mut Stub);
        // 100 of content + the default 2 + 2 padding.
        assert!(near(grid.row_heights[0], 104.0), "{:?}", grid.row_heights);
    }

    #[test]
    fn an_exactly_row_with_no_declared_height_is_still_a_minimum() {
        // `exactly` freezes a row only when a positive height goes with it;
        // without one there is nothing to freeze it at.
        let t = table(r#"{"rowHeightModes":["exactly"]}"#, &[&[&cell(r#"{"tH":90}"#)]]);
        let (grid, _) = layout_with(&t, 400.0, &mut Stub);
        assert_eq!(grid.row_heights, vec![MIN_ROW_H]);
    }

    #[test]
    fn a_row_shorter_than_the_minimum_is_raised_to_it() {
        let t = table(r#"{"rowHeights":[5]}"#, &[&[&cell("{}")]]);
        let (grid, _) = layout(&t, 400.0);
        assert_eq!(grid.row_heights, vec![MIN_ROW_H]);
    }

    #[test]
    fn a_rowspan_deficit_lands_on_the_last_row_it_covers() {
        // A tall spanning cell does not grow every row it crosses — only the
        // last one, by exactly what is missing.
        let t = table(
            "{}",
            &[
                &[&cell(r#"{"rowspan":2,"tH":100}"#), &cell(r#"{"tH":0}"#)],
                &[&cell(r#"{"tH":0}"#)],
            ],
        );
        let (grid, cells) = layout_with(&t, 400.0, &mut Stub);
        assert_eq!(grid.row_heights[0], MIN_ROW_H, "the first row is untouched");
        // need = 100 + 4 padding = 104; have = 22 + 22; deficit lands on row 1.
        assert!(near(grid.row_heights[1], 104.0 - MIN_ROW_H), "{:?}", grid.row_heights);
        let spanning = &cells[0];
        assert!(near(spanning.height, 104.0));
    }

    #[test]
    fn a_rowspan_deficit_never_stretches_an_exactly_row() {
        let t = table(
            r#"{"rowHeightModes":["atleast","exactly"],"rowHeights":[0,30]}"#,
            &[
                &[&cell(r#"{"rowspan":2,"tH":500}"#), &cell(r#"{"tH":0}"#)],
                &[&cell(r#"{"tH":0}"#)],
            ],
        );
        let (grid, _) = layout_with(&t, 400.0, &mut Stub);
        assert_eq!(grid.row_heights, vec![MIN_ROW_H, 30.0]);
    }

    #[test]
    fn a_spanning_cell_does_not_size_the_rows_it_crosses() {
        // Only rowspan-1 cells feed the per-row maximum (`:1529`); a spanning
        // cell is handled by the deficit rule instead.
        let t = table(
            "{}",
            &[&[&cell(r#"{"rowspan":2,"tH":200}"#), &cell(r#"{"tH":40}"#)], &[&cell(r#"{"tH":0}"#)]],
        );
        let (grid, _) = layout_with(&t, 400.0, &mut Stub);
        assert!(near(grid.row_heights[0], 44.0), "{:?}", grid.row_heights);
    }

    // ── Spans and placement ──────────────────────────────────────────────────

    #[test]
    fn a_colspan_cell_is_as_wide_as_the_columns_it_covers() {
        let t = table(
            r#"{"colWidths":[100,50,50],"tableLayout":"fixed"}"#,
            &[&[&cell(r#"{"colspan":2}"#), &cell("{}")], &[&cell("{}"), &cell("{}"), &cell("{}")]],
        );
        let (_, cells) = layout(&t, 400.0);
        assert_eq!(cells[0].colspan, 2);
        assert!(near(cells[0].width, 150.0));
        assert_eq!(cells[1].column, 2);
    }

    #[test]
    fn a_rowspan_pushes_the_next_rows_cells_past_the_columns_it_holds() {
        let t = table(
            r#"{"colWidths":[60,60,60],"tableLayout":"fixed"}"#,
            &[
                &[&cell(r#"{"rowspan":2}"#), &cell("{}"), &cell("{}")],
                &[&cell("{}"), &cell("{}")],
            ],
        );
        let (_, cells) = layout(&t, 400.0);
        let second_row: Vec<usize> =
            cells.iter().filter(|c| c.row == 1).map(|c| c.column).collect();
        assert_eq!(second_row, vec![1, 2], "column 0 is held by the rowspan");
    }

    #[test]
    fn a_merged_cell_is_not_placed_but_still_counts_toward_the_column_count() {
        // Parity trap: `colsInRow` counts merged cells while placement skips
        // them, so an absorbed cell really does add a phantom column. Recorded
        // here rather than rediscovered.
        let t = table(
            "{}",
            &[&[&cell(r#"{"colspan":2}"#), &cell(r#"{"colspan":1,"merged":true}"#)]],
        );
        let (grid, cells) = layout(&t, 300.0);
        assert_eq!(grid.column_count(), 3, "the merged cell's colspan still counts");
        assert_eq!(cells.len(), 1, "a merged cell is never placed");
        assert_eq!(cells[0].colspan, 2);
    }

    #[test]
    fn a_cell_knows_which_node_it_came_from() {
        // Placed cells are not the row's n-th child, because merged cells are
        // skipped — the caller needs the real index to find the content.
        let t = table(
            "{}",
            &[&[&cell(r#"{"merged":true}"#), &cell("{}")]],
        );
        let (_, cells) = layout(&t, 300.0);
        assert_eq!(cells.len(), 1);
        assert_eq!((cells[0].source_row, cells[0].source_cell), (0, 1));
    }

    #[test]
    fn a_colspan_is_clamped_to_the_columns_a_rowspan_has_left_free() {
        let t = table(
            "{}",
            &[
                &[&cell(r#"{"rowspan":2}"#), &cell("{}"), &cell("{}")],
                &[&cell(r#"{"colspan":3}"#)],
            ],
        );
        let (grid, cells) = layout(&t, 300.0);
        assert_eq!(grid.column_count(), 3);
        let last = cells.last().expect("placed");
        // Column 0 is held by the rowspan, so only two columns are left.
        assert_eq!((last.row, last.column, last.colspan), (1, 1, 2));
        assert_eq!(last.column + last.colspan, grid.column_count());
    }

    // ── Column widths ────────────────────────────────────────────────────────

    #[test]
    fn explicit_widths_that_fit_are_respected_and_the_table_stays_narrow() {
        // A table is allowed to be narrower than the text column: that is what
        // dragging its right edge in does.
        let t = table(r#"{"colWidths":[80,120]}"#, &[&[&cell("{}"), &cell("{}")]]);
        let (grid, _) = layout(&t, 600.0);
        assert_eq!(grid.column_widths, vec![80.0, 120.0]);
        assert!(near(grid.width, 200.0));
    }

    #[test]
    fn columns_with_no_explicit_widths_fill_the_text_column() {
        let t = table("{}", &[&[&cell("{}"), &cell("{}"), &cell("{}")]]);
        let (grid, _) = layout(&t, 600.0);
        assert!(near(grid.width, 600.0));
        for w in &grid.column_widths {
            assert!(near(*w, 200.0));
        }
    }

    #[test]
    fn a_ragged_explicit_width_array_is_not_treated_as_explicit() {
        // One entry per column, all real, or the array is preferred-width data
        // the autofit branch must not trust.
        let t = table(r#"{"colWidths":[80]}"#, &[&[&cell("{}"), &cell("{}")]]);
        let (grid, _) = layout(&t, 600.0);
        assert!(near(grid.width, 600.0), "it fell back to filling the column");
    }

    #[test]
    fn a_table_wider_than_its_column_takes_the_slack_from_the_widest_columns() {
        // Shrinking proportionally to slack is what keeps a column that is
        // already at its minimum from collapsing further.
        let t = table(
            "{}",
            &[&[&cell(r#"{"tMin":10,"tMax":500}"#), &cell(r#"{"tMin":180,"tMax":190}"#)]],
        );
        let (grid, _) = layout_with(&t, 300.0, &mut Stub);
        assert!(near(grid.width, 300.0));
        let [wide, narrow] = grid.column_widths[..] else { panic!("two columns") };
        // The elastic column gives up far more than the one near its minimum.
        assert!(wide < 200.0 && narrow > 150.0, "{:?}", grid.column_widths);
        assert!(narrow >= 180.0 + 12.0 - 30.0, "{:?}", grid.column_widths);
    }

    #[test]
    fn columns_that_are_all_at_their_minimum_are_scaled_down_together() {
        let t = table(
            "{}",
            &[&[&cell(r#"{"tMin":400,"tMax":400}"#), &cell(r#"{"tMin":400,"tMax":400}"#)]],
        );
        let (grid, _) = layout_with(&t, 200.0, &mut Stub);
        assert!(near(grid.width, 200.0), "{:?}", grid.column_widths);
        assert!(near(grid.column_widths[0], grid.column_widths[1]));
    }

    #[test]
    fn a_column_narrower_than_the_minimum_shrinks_rather_than_overflowing() {
        // `MIN_COL_W` is a floor on the PREFERRED width, not on the result: a
        // table in a column narrower than its minimum is scaled down to fit
        // rather than allowed to spill out of the text column.
        let t = table("{}", &[&[&cell("{}")]]);
        let (grid, _) = layout(&t, 10.0);
        assert!(near(grid.width, 10.0), "{:?}", grid.column_widths);
    }

    #[test]
    fn a_fixed_layout_repairs_a_ragged_width_array_instead_of_ignoring_it() {
        // Missing entries take the average of the known ones; surplus entries
        // are dropped. Ignoring the array flipped the whole table to uniform
        // columns in one step.
        let t = table(
            r#"{"colWidths":[100,0,50,999],"tableLayout":"fixed"}"#,
            &[&[&cell("{}"), &cell("{}"), &cell("{}")]],
        );
        let (grid, _) = layout(&t, 600.0);
        assert!(near(grid.column_widths[0], 100.0));
        assert!(near(grid.column_widths[1], (100.0 + 50.0 + 999.0) / 3.0));
        assert!(near(grid.column_widths[2], 50.0));
        assert_eq!(grid.column_widths.len(), 3, "the surplus entry is dropped");
    }

    #[test]
    fn a_fixed_table_wider_than_its_column_is_scaled_down() {
        let t = table(
            r#"{"colWidths":[400,400],"tableLayout":"fixed"}"#,
            &[&[&cell("{}"), &cell("{}")]],
        );
        let (grid, _) = layout(&t, 300.0);
        assert!(near(grid.width, 300.0));
        assert!(near(grid.column_widths[0], 150.0));
    }

    #[test]
    fn a_fixed_table_with_no_widths_gets_uniform_columns() {
        let t = table(r#"{"tableLayout":"fixed"}"#, &[&[&cell("{}"), &cell("{}")]]);
        let (grid, _) = layout(&t, 300.0);
        assert_eq!(grid.column_widths, vec![150.0, 150.0]);
    }

    // ── Boxes ────────────────────────────────────────────────────────────────

    #[test]
    fn cell_spacing_shrinks_every_cell_on_all_four_sides() {
        // That gap is what stops two cells sharing a border edge.
        let t = table(
            r#"{"cellSpacing":8,"colWidths":[100,100],"tableLayout":"fixed"}"#,
            &[&[&cell("{}"), &cell("{}")]],
        );
        let (grid, cells) = layout(&t, 400.0);
        assert!(near(cells[0].x, 4.0));
        assert!(near(cells[0].width, 92.0));
        assert!(near(cells[0].y, 4.0));
        assert!(near(cells[0].height, MIN_ROW_H - 8.0));
        // The two cells no longer touch, so no edge is shared.
        assert!(cells[0].x + cells[0].width < cells[1].x);
        assert_eq!(edges(&grid, &cells).len(), 8, "four edges each, none shared");
    }

    #[test]
    fn a_centred_table_is_offset_by_half_its_slack() {
        let t = table(
            r#"{"colWidths":[100,100],"tableAlign":"center","tableLayout":"fixed"}"#,
            &[&[&cell("{}"), &cell("{}")]],
        );
        let (grid, cells) = layout(&t, 600.0);
        assert!(near(grid.x_offset, 200.0));
        assert!(near(cells[0].x, 200.0));
        assert!(near(grid.col_x[0], 200.0));
    }

    #[test]
    fn a_right_aligned_table_ends_at_the_column_edge() {
        let t = table(
            r#"{"colWidths":[100,100],"tableAlign":"right","tableLayout":"fixed"}"#,
            &[&[&cell("{}"), &cell("{}")]],
        );
        let (grid, _) = layout(&t, 600.0);
        assert!(near(grid.x_offset + grid.width, 600.0));
    }

    #[test]
    fn a_table_indent_moves_a_left_aligned_table_only() {
        let t = table(
            r#"{"colWidths":[100],"tableIndent":36,"tableLayout":"fixed"}"#,
            &[&[&cell("{}")]],
        );
        let (grid, _) = layout(&t, 600.0);
        assert!(near(grid.x_offset, 36.0));
    }

    #[test]
    fn vertical_alignment_puts_the_slack_where_the_cell_asked() {
        let t = table(
            r#"{"rowHeights":[100]}"#,
            &[&[
                &cell(r#"{"cellVAlign":"top"}"#),
                &cell(r#"{"cellVAlign":"center"}"#),
                &cell(r#"{"cellVAlign":"bottom"}"#),
            ]],
        );
        let (grid, cells) = layout(&t, 600.0);
        let ys: Vec<DocPx> = cells.iter().map(|c| c.content_origin(&grid, 20.0).1).collect();
        // inner height 96, content 20 → slack 76.
        assert!(near(ys[0], CELL_PAD_Y));
        assert!(near(ys[1], CELL_PAD_Y + 38.0));
        assert!(near(ys[2], CELL_PAD_Y + 76.0));
    }

    #[test]
    fn a_cell_is_laid_out_at_its_width_minus_its_padding() {
        let t = table(
            r#"{"cellMarginLeft":20,"cellMarginRight":20,"colWidths":[200],"tableLayout":"fixed"}"#,
            &[&[&cell("{}")]],
        );
        let mut rec = Recorder::default();
        let (grid, cells) = layout_with(&t, 400.0, &mut rec);
        assert_eq!(rec.widths, vec![160.0]);
        assert!(near(cells[0].inner_width(&grid), 160.0));
        assert!(near(cells[0].content_origin(&grid, 0.0).0, 20.0));
    }

    #[test]
    fn a_vertical_cell_is_laid_out_unwrapped_at_the_capped_width() {
        // Not `f32::MAX`: a text layout at that width is the bug this keeps
        // shut. The extent it returns is the longest line's WIDTH, which
        // becomes the row's height once the cell is rotated.
        let t = table(
            r#"{"colWidths":[60],"tableLayout":"fixed"}"#,
            &[&[&cell(r#"{"cellDir":270,"tH":150}"#)]],
        );
        let mut rec = Recorder::default();
        let (grid, cells) = layout_with(&t, 400.0, &mut rec);
        assert_eq!(rec.widths, vec![VERTICAL_LAYOUT_WIDTH]);
        assert!(VERTICAL_LAYOUT_WIDTH.is_finite());
        assert_eq!(cells[0].dir, CellDir::Up);
        assert!(near(cells[0].layout_width(&grid), VERTICAL_LAYOUT_WIDTH));

        let (grid, _) = layout_with(&t, 400.0, &mut Stub);
        assert!(near(grid.row_heights[0], 154.0), "{:?}", grid.row_heights);
    }

    #[test]
    fn cell_backgrounds_follow_the_table_style_unless_the_cell_says_otherwise() {
        let rows: &[&[&str]] = &[
            &[&cell("{}")],
            &[&cell("{}")],
            &[&cell(r##"{"cellBg":"#abcdef"}"##)],
        ];
        let (_, cells) = layout(&table(r#"{"tableStyle":"striped"}"#, rows), 400.0);
        assert_eq!(cells[0].background, CellFill::Tint(HEADER_TINT_ALPHA));
        assert_eq!(cells[1].background, CellFill::Tint(STRIPE_TINT_ALPHA));
        assert_eq!(cells[2].background, CellFill::Explicit("#abcdef".into()));

        let (_, plain) = layout(&table(r#"{"tableStyle":"grid"}"#, rows), 400.0);
        assert_eq!(plain[0].background, CellFill::None);
    }

    // ── Degenerate documents ─────────────────────────────────────────────────

    #[test]
    fn a_table_with_no_rows_produces_no_cells_and_no_panic() {
        let (grid, cells) = layout(&node(r#"{"type":"table"}"#), 400.0);
        assert!(cells.is_empty());
        assert_eq!(grid.row_count(), 0);
        assert_eq!(grid.column_count(), 1, "there is always at least one column");
        assert_eq!(grid.height, 0.0);
    }

    #[test]
    fn a_table_with_no_attributes_uses_the_webs_defaults() {
        let (grid, _) = layout(&node(r#"{"content":[],"type":"table"}"#), 400.0);
        assert_eq!(grid.accent, DEFAULT_ACCENT);
        assert_eq!(grid.style, TableStyle::Grid);
        assert_eq!((grid.pad_left, grid.pad_top), (CELL_PAD_X, CELL_PAD_Y));
        assert_eq!(grid.spacing, 0.0);
        assert_eq!(grid.align, TableAlign::Left);
        assert!(!grid.header_repeat);
        assert_eq!(grid.header_rows, 0);
    }

    #[test]
    fn header_rows_defaults_to_one_when_the_header_repeats() {
        let (grid, _) = layout(&table(r#"{"headerRepeat":true}"#, &[&[&cell("{}")]]), 400.0);
        assert!(grid.header_repeat);
        assert_eq!(grid.header_rows, 1);
    }

    #[test]
    fn a_garbage_attribute_falls_back_instead_of_poisoning_the_geometry() {
        // A NaN here would travel into every coordinate of the page.
        let t = table(
            r#"{"cellMarginLeft":"wide","cellSpacing":"lots","colWidths":["x",null],"rowHeights":["tall"],"tableIndent":{}}"#,
            &[&[&cell("{}"), &cell("{}")]],
        );
        let (grid, cells) = layout(&t, 400.0);
        assert!(grid.width.is_finite() && grid.height.is_finite());
        assert!(grid.column_widths.iter().all(|w| w.is_finite() && *w > 0.0));
        assert_eq!(grid.pad_left, CELL_PAD_X);
        assert_eq!(grid.spacing, 0.0);
        assert!(cells.iter().all(|c| c.x.is_finite() && c.height.is_finite()));
    }

    #[test]
    fn a_zero_width_column_is_still_a_column() {
        let t = table(r#"{"colWidths":[0,0],"tableLayout":"fixed"}"#, &[&[&cell("{}"), &cell("{}")]]);
        let (grid, cells) = layout(&t, 400.0);
        assert_eq!(grid.column_count(), 2);
        assert!(cells.iter().all(|c| c.width >= 1.0), "a cell box never has zero width");
    }

    // ── The page-splitting seam ──────────────────────────────────────────────

    #[test]
    fn cells_outside_a_page_band_are_dropped_not_clipped() {
        // Without the filter, the rows of other pages paint their borders and
        // fills into this page's margins.
        let t = table(
            r#"{"rowHeights":[100,100,100]}"#,
            &[&[&cell("{}")], &[&cell("{}")], &[&cell("{}")]],
        );
        let (_, cells) = layout(&t, 400.0);
        let band = cells_in_band(&cells, 100.0, 100.0);
        assert_eq!(band.len(), 1);
        assert_eq!(band[0].row, 1);
        assert!(near(band[0].y, 0.0), "the band is in page-local coordinates");
        assert!(near(band[0].height, 100.0), "the box itself is not clipped");
    }

    #[test]
    fn a_row_straddling_the_band_edge_stays_in_both_bands() {
        let t = table(r#"{"rowHeights":[100,100]}"#, &[&[&cell("{}")], &[&cell("{}")]]);
        let (_, cells) = layout(&t, 400.0);
        assert_eq!(cells_in_band(&cells, 0.0, 150.0).len(), 2);
        assert_eq!(cells_in_band(&cells, 150.0, 150.0).len(), 1);
    }

    // ── Intrinsic widths ─────────────────────────────────────────────────────

    fn measured(cell_json: &str) -> (DocPx, DocPx) {
        intrinsic_widths(&node(cell_json), &Ruler)
    }

    #[test]
    fn intrinsic_widths_break_on_spaces_and_keep_the_longest_word() {
        // min = the longest word, max = everything laid end to end.
        let (min, max) = measured(
            r#"{"content":[{"content":[{"text":"ab cdefgh","type":"text"}],"type":"paragraph"}],"type":"tableCell"}"#,
        );
        // 11 pt at half a point per character: 5.5 px each.
        assert!(near(min, 6.0 * 5.5), "longest word 'cdefgh': {min}");
        assert!(near(max, 9.0 * 5.5), "the whole string: {max}");
    }

    #[test]
    fn an_inline_image_bounds_the_column_like_an_unbreakable_word() {
        let (min, max) = measured(
            r#"{"content":[{"content":[{"text":"a ","type":"text"},{"attrs":{"width":120},"type":"inlineImage"}],"type":"paragraph"}],"type":"tableCell"}"#,
        );
        assert!(near(min, 120.0), "the image cannot be broken: {min}");
        assert!(near(max, 120.0 + 2.0 * 5.5));
    }

    #[test]
    fn a_paragraph_indent_counts_toward_the_column_minimum() {
        let (min, _) = measured(
            r#"{"content":[{"attrs":{"indent":40,"indentRight":10},"content":[{"text":"ab","type":"text"}],"type":"paragraph"}],"type":"tableCell"}"#,
        );
        assert!(near(min, 40.0 + 10.0 + 2.0 * 5.5), "{min}");
    }

    #[test]
    fn the_widest_paragraph_of_a_cell_sets_its_width() {
        let (min, max) = measured(
            r#"{"content":[{"content":[{"text":"aa","type":"text"}],"type":"paragraph"},{"content":[{"text":"bbbb","type":"text"}],"type":"paragraph"}],"type":"tableCell"}"#,
        );
        assert!(near(min, 4.0 * 5.5));
        assert!(near(max, 4.0 * 5.5));
    }

    #[test]
    fn a_font_size_mark_changes_what_a_word_measures() {
        let (_, max) = measured(
            r#"{"content":[{"content":[{"marks":[{"attrs":{"fontSize":"22pt"},"type":"textStyle"}],"text":"ab","type":"text"}],"type":"paragraph"}],"type":"tableCell"}"#,
        );
        assert!(near(max, 2.0 * 11.0), "22 pt, half a point per character: {max}");
    }

    #[test]
    fn a_list_inside_a_cell_is_measured_through_its_items() {
        let (_, max) = measured(
            r#"{"content":[{"content":[{"content":[{"content":[{"text":"abcd","type":"text"}],"type":"paragraph"}],"type":"listItem"}],"type":"bulletList"}],"type":"tableCell"}"#,
        );
        assert!(near(max, 4.0 * 5.5), "{max}");
    }

    #[test]
    fn a_nested_table_does_not_widen_the_cell_that_holds_it() {
        // Parity: the web's nested-table paragraph carries no spans, so it
        // contributes nothing to the outer column's intrinsic width.
        let (min, max) = measured(
            r#"{"content":[{"content":[{"content":[{"content":[{"content":[{"text":"very long text indeed","type":"text"}],"type":"paragraph"}],"type":"tableCell"}],"type":"tableRow"}],"type":"table"}],"type":"tableCell"}"#,
        );
        assert_eq!((min, max), (0.0, 0.0));
    }

    #[test]
    fn a_hard_break_does_not_split_a_word_for_measurement() {
        // The web's parser emits no span for a hardBreak, so the words either
        // side of it accumulate into one — the measurement must not be kinder.
        let (min, _) = measured(
            r#"{"content":[{"content":[{"text":"ab","type":"text"},{"type":"hardBreak"},{"text":"cd","type":"text"}],"type":"paragraph"}],"type":"tableCell"}"#,
        );
        assert!(near(min, 4.0 * 5.5), "{min}");
    }

    #[test]
    fn an_empty_cell_measures_as_nothing() {
        assert_eq!(measured(r#"{"content":[],"type":"tableCell"}"#), (0.0, 0.0));
        assert_eq!(measured(r#"{"type":"tableCell"}"#), (0.0, 0.0));
    }

    #[test]
    fn autofit_gives_a_wordy_column_more_room_than_a_terse_one() {
        // End to end through the real measurer, which is the thing a user sees.
        let long = r#"{"content":[{"content":[{"text":"a considerably longer run of words","type":"text"}],"type":"paragraph"}],"type":"tableCell"}"#;
        let short = r#"{"content":[{"content":[{"text":"no","type":"text"}],"type":"paragraph"}],"type":"tableCell"}"#;
        struct Real;
        impl CellContent for Real {
            fn intrinsic_widths(&mut self, cell: &Node) -> (DocPx, DocPx) {
                intrinsic_widths(cell, &Ruler)
            }
            fn extent(&mut self, _cell: &Node, _width: DocPx) -> DocPx {
                0.0
            }
        }
        let t = table("{}", &[&[long, short]]);
        let (grid, _) = layout_with(&t, 300.0, &mut Real);
        assert!(
            grid.column_widths[0] > grid.column_widths[1],
            "autofit is not equal columns: {:?}",
            grid.column_widths
        );
        assert!(near(grid.width, 300.0));
    }

    #[test]
    fn splitting_keeps_every_character_of_a_string() {
        for s in ["a b", "  a  b  ", "", "a", "   ", "a\u{00a0}b"] {
            assert_eq!(split_keeping_spaces(s).concat(), s, "for {s:?}");
        }
        assert_eq!(split_keeping_spaces("a  b"), vec!["a", "  ", "b"]);
        // U+200B is not a space in this set, so it glues its word together.
        assert_eq!(split_keeping_spaces("a\u{200b}b"), vec!["a\u{200b}b"]);
    }
}
