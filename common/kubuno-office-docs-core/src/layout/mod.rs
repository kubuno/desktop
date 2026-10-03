//! The layout engine: a port of the web editor's `office/frontend/src/canvas-engine.ts`.
//!
//! The pipeline is the web's, stage for stage:
//!
//! 1. [`parse`] — `parseDoc`: the ProseMirror body → [`RenderParagraph`]s (one per textblock, list
//!    item paragraph, table, block image or rule), each span carrying its ProseMirror position.
//! 2. [`paragraph`] — `layoutParagraph`: one render paragraph → [`LayoutLine`]s (the line breaker,
//!    tabs, alignment, justification, list markers), and [`table`] — `layoutTable`.
//! 3. [`flow`] — `layoutParagraphs`: the paragraphs stacked into one **continuous** column
//!    ([`DocumentLayout`]), contextual spacing applied; cached per top-level block so a keystroke
//!    re-lays out one block.
//! 4. [`paginate`] — `paginateMulti`: the continuous lines sliced into pages and columns
//!    ([`PageLayout`]): page breaks, keep-with-next, keep-lines, repeated table headers.
//! 5. [`caret`] — `posToCoords`, `coordsToPos`, `selectionRects`, word/line/paragraph boundaries —
//!    all on the continuous layout, like the web (its caret code reads `contLayoutRef`).
//!
//! Units: everything is in **CSS px at 96 dpi** ("document pixels"); character sizes are in points
//! in the marks and converted on the way in. Positions are ProseMirror positions ([`crate::pm`]).
//!
//! Not ported yet (each listed in `vskubuno/docs/DOCUMENTS-EDITING.md` §3): floating objects and
//! text wrap around them (a floating image takes no room, as on the web, but text does not flow
//! around it), drop caps, vertical cell text (laid out horizontally), table-of-contents leaders,
//! footnote and endnote blocks (the reference numbers are laid out), heading numbering.

pub mod caret;
pub mod flow;
pub mod images;
pub mod paginate;
pub mod paragraph;
pub mod parity;
pub mod parse;
pub mod table;
pub mod tables;

use crate::marks::TextMark;

/// Document pixels: CSS px at 96 dpi.
pub type DocPx = f32;

/// `LH_RATIO` (`canvas-engine.ts:240`): the default line spacing.
pub const LH_RATIO: f32 = 1.15;
/// `LIST_INDENT` (`:241`): px per list nesting level.
pub const LIST_INDENT: DocPx = 32.0;
/// `CELL_PAD_X` / `CELL_PAD_Y` (`:1348-1349`).
pub const CELL_PAD_X: DocPx = 6.0;
pub const CELL_PAD_Y: DocPx = 2.0;

/// Points to document pixels.
pub fn pt_to_px(pt: f32) -> DocPx {
    pt * 96.0 / 72.0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

impl Align {
    pub fn parse(s: Option<&str>) -> Self {
        match s {
            Some("center") => Align::Center,
            Some("right") => Align::Right,
            Some("justify") => Align::Justify,
            _ => Align::Left,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "justify",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineSpacingMode {
    #[default]
    Multiple,
    AtLeast,
    Exactly,
}

/// An inline image (`inlineImage`): one atom laid out as a character of its size.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineImage {
    pub src: String,
    pub w: DocPx,
    pub h: DocPx,
    pub alt: Option<String>,
    pub rot: f32,
}

/// What a span is, beyond text.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum SpanKind {
    #[default]
    Text,
    /// An inline image (atom, 1 position).
    Image(InlineImage),
    /// A footnote reference: its number and the note's text.
    Footnote { n: usize, text: String },
    /// An endnote reference (lowercase roman number).
    Endnote { n: usize, text: String },
    /// A Word field: the painted text is its cached result.
    Field { kind: String, instr: String },
    /// A list or heading marker painted in the margin (no position of its own).
    Marker,
}

/// `RenderSpan`: a run of text (or an atom) with its marks and ProseMirror position.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderSpan {
    pub text: String,
    pub marks: TextMark,
    pub pm_pos: usize,
    pub kind: SpanKind,
    /// Positions covered when it is not the text's UTF-16 length (an atom: 1).
    pub pm_len: Option<usize>,
}

/// A block image (`image` node).
#[derive(Clone, Debug, PartialEq)]
pub struct BlockImage {
    pub src: String,
    pub width: DocPx,
    pub height: DocPx,
    pub align: Align,
    pub rotation: f32,
    pub wrap: String,
    pub wrap_x: DocPx,
    pub wrap_y: DocPx,
    pub alt: Option<String>,
}

impl BlockImage {
    pub fn floating(&self) -> bool {
        images::is_floating_wrap(&self.wrap)
    }
}

/// `RenderTableCell`.
#[derive(Clone, Debug)]
pub struct RenderTableCell {
    pub paras: Vec<RenderParagraph>,
}

/// `RenderTable` — the cells' paragraphs, by `[row][cell]` in the node's own order (merged cells
/// included, so the indices are those of the node's children).
#[derive(Clone, Debug)]
pub struct RenderTable {
    pub rows: Vec<Vec<RenderTableCell>>,
    /// The table node itself: its attributes drive the geometry (`tables::layout_with`).
    pub node: crate::model::Node,
}

/// `RenderParagraph`: a block ready to be laid out.
#[derive(Clone, Debug)]
pub struct RenderParagraph {
    pub spans: Vec<RenderSpan>,
    pub align: Align,
    /// Left indent (lists + paragraph indent), px.
    pub indent: DocPx,
    /// First line offset relative to `indent` (negative = hanging).
    pub first_line_indent: DocPx,
    pub indent_right: DocPx,
    /// Custom tab stops (px from the content's left edge), sorted.
    pub tab_stops: Option<Vec<DocPx>>,
    /// List or heading marker (`•`, `1.`).
    pub marker: Option<String>,
    pub marker_marks: Option<TextMark>,
    pub space_before: DocPx,
    pub space_after: DocPx,
    /// Position before the block's opening token, and after its closing one… as the web counts
    /// them: `pm_start` is the position of the block's opening token, `pm_end` the position of its
    /// closing token (the end of its content).
    pub pm_start: usize,
    pub pm_end: usize,
    /// Index of the top-level block (or of the block in its cell).
    pub doc_idx: usize,
    pub sec_idx: usize,
    pub break_before: bool,
    pub line_spacing: f32,
    pub line_spacing_mode: LineSpacingMode,
    pub line_spacing_pt: Option<DocPx>,
    pub contextual_spacing: bool,
    pub style_key: String,
    pub keep_lines: bool,
    pub keep_next: bool,
    /// The size (pt) an EMPTY paragraph carries (`attrs.fontMarks.fs`).
    pub empty_pt: Option<f32>,
    pub image: Option<BlockImage>,
    pub table: Option<RenderTable>,
    /// A horizontal rule: laid out as text but not a caret target.
    pub rule: bool,
}

impl Default for RenderParagraph {
    fn default() -> Self {
        Self {
            spans: Vec::new(),
            align: Align::Left,
            indent: 0.0,
            first_line_indent: 0.0,
            indent_right: 0.0,
            tab_stops: None,
            marker: None,
            marker_marks: None,
            space_before: 0.0,
            space_after: 0.0,
            pm_start: 0,
            pm_end: 0,
            doc_idx: 0,
            sec_idx: 0,
            break_before: false,
            line_spacing: LH_RATIO,
            line_spacing_mode: LineSpacingMode::Multiple,
            line_spacing_pt: None,
            contextual_spacing: false,
            style_key: "p".into(),
            keep_lines: false,
            keep_next: false,
            empty_pt: None,
            image: None,
            table: None,
            rule: false,
        }
    }
}

/// `LayoutSpan`: a token placed on a line.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutSpan {
    pub text: String,
    pub marks: TextMark,
    /// From the content area's left edge.
    pub x: DocPx,
    pub width: DocPx,
    pub pm_pos: usize,
    pub kind: SpanKind,
    pub pm_len: Option<usize>,
}

impl LayoutSpan {
    /// `isAtomSpan`: a tab, an inline image, a note reference or a field — the caret can only sit
    /// at its two edges, and its width is not derived from its text.
    pub fn is_atom(&self) -> bool {
        self.text == "\t" || !matches!(self.kind, SpanKind::Text | SpanKind::Marker)
    }

    /// `spanPmLen`: positions covered.
    pub fn pm_len(&self) -> usize {
        self.pm_len.unwrap_or_else(|| crate::pm::utf16_len(&self.text))
    }

    pub fn is_marker(&self) -> bool {
        matches!(self.kind, SpanKind::Marker)
    }
}

/// The block image a line carries.
#[derive(Clone, Debug, PartialEq)]
pub struct LineImage {
    pub src: String,
    pub w: DocPx,
    pub h: DocPx,
    pub x: DocPx,
    pub rotation: f32,
    pub wrap: String,
    pub wrap_y: DocPx,
    pub alt: Option<String>,
}

/// `LayoutLine`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutLine {
    pub spans: Vec<LayoutSpan>,
    /// Top of the line box (continuous layout: from the top of the document's content).
    pub y: DocPx,
    /// Line box height, line spacing included.
    pub height: DocPx,
    /// Natural text height (without the spacing).
    pub natural_h: DocPx,
    pub ascent: DocPx,
    /// From the same origin as `y`.
    pub baseline: DocPx,
    pub pm_start: usize,
    pub pm_end: usize,
    pub image: Option<LineImage>,
    /// Horizontal bounds of the table cell the line is in (for `coordsToPos`).
    pub cell_x: Option<DocPx>,
    pub cell_w: Option<DocPx>,
    /// The caret's x on an EMPTY line (alignment and indent).
    pub caret_x: Option<DocPx>,
    /// A repeated table header row: painted, never hit or selected.
    pub phantom: bool,
    /// Not a caret target (a horizontal rule, a block image).
    pub no_caret: bool,
}

/// The cell rectangles and borders of a table, for painting.
#[derive(Clone, Debug, Default)]
pub struct LayoutTable {
    pub grid: tables::Grid,
    pub cells: Vec<tables::Cell>,
    /// Offset applied to the geometry's coordinates (x: table alignment; y: the table's top in the
    /// continuous layout or on the page).
    pub dx: DocPx,
    pub dy: DocPx,
    pub style: String,
    pub accent: Option<String>,
}

/// `LayoutParagraph`.
#[derive(Clone, Debug, Default)]
pub struct LayoutParagraph {
    pub lines: Vec<LayoutLine>,
    /// Top, space before included.
    pub y: DocPx,
    /// Total height, spaces included.
    pub height: DocPx,
    pub pm_start: usize,
    pub pm_end: usize,
    pub doc_idx: usize,
    pub sec_idx: usize,
    pub break_before: bool,
    pub keep_lines: bool,
    pub keep_next: bool,
    pub table: Option<LayoutTable>,
}

/// `DocumentLayout`: the continuous layout.
#[derive(Clone, Debug, Default)]
pub struct DocumentLayout {
    pub paragraphs: Vec<LayoutParagraph>,
    pub total_height: DocPx,
    pub content_w: DocPx,
}

impl DocumentLayout {
    /// Every line in document order, with the index of its paragraph.
    pub fn lines(&self) -> impl Iterator<Item = (usize, &LayoutLine)> {
        self.paragraphs.iter().enumerate().flat_map(|(i, p)| p.lines.iter().map(move |l| (i, l)))
    }
}

/// `CursorMetrics`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CursorMetrics {
    pub x: DocPx,
    /// Top of the caret box.
    pub y: DocPx,
    pub height: DocPx,
    /// Lean of an italic caret (tan), 0 upright.
    pub italic_angle: f32,
    pub baseline: Option<DocPx>,
    /// The full line box (navigation).
    pub line_top: Option<DocPx>,
    pub line_h: Option<DocPx>,
}

/// `SelectionRect`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SelectionRect {
    pub x: DocPx,
    pub y: DocPx,
    pub w: DocPx,
    pub h: DocPx,
}

/// One text column of a page, seen from the continuous layout (`PageColumnBand`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ColumnBand {
    pub start_y: DocPx,
    pub x_shift: DocPx,
}

/// `PageLayout`: one page, its lines in page-local coordinates (y = 0 at the top of the content
/// box) but global ProseMirror positions.
#[derive(Clone, Debug, Default)]
pub struct PageLayout {
    pub paragraphs: Vec<LayoutParagraph>,
    /// The continuous ordinate painted at the top of the page's content box.
    pub start_y: DocPx,
    pub height: DocPx,
    pub sec_idx: usize,
    pub columns: Vec<ColumnBand>,
}

impl PageLayout {
    /// The first and last positions on the page (`None` for a page without lines).
    pub fn pm_range(&self) -> Option<(usize, usize)> {
        let mut lines = self.paragraphs.iter().flat_map(|p| p.lines.iter()).filter(|l| !l.phantom);
        let first = lines.next()?;
        let last = lines.next_back().unwrap_or(first);
        Some((first.pm_start, last.pm_end))
    }
}
