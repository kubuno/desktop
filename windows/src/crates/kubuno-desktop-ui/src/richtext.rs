//! Rich text editor — the editing area `@ui/RichText` puts under its toolbar.
//!
//! Web counterpart: `core/frontend/src/ui/RichText.tsx` — a `contenteditable`
//! surface under a toolbar (bold, italic, underline, bulleted / numbered
//! lists, link, clear formatting), `minHeight` 96 px, a controlled HTML value,
//! the placeholder at `top-2 left-3`, and a link row (`kb-richtext-linkbar`)
//! that opens under the toolbar instead of a browser dialog.
//!
//! The file has three layers, and only the last one paints:
//!
//! | layer | what | why it is separate |
//! |---|---|---|
//! | [`Document`] | blocks (paragraph, two heading levels, bullet / numbered items) of [`Span`]s carrying [`Marks`]; the selection; every editing operation; undo / redo | pure — the whole editing behaviour is unit-tested without a window |
//! | `Layout` (private) | word-wrapped lines of measured fragments, hit-testing, caret geometry | needs [`Canvas::measure`], nothing else |
//! | [`RichTextBox`] | the Kubuno chrome, the input routing (pointer, keys, text, clipboard, wheel), the toolbar and the link row | the [`Widget`] |
//!
//! ## Where the web is followed, and where it cannot be
//!
//! * **Box**: `kb-field-focus rounded-md border border-border bg-white
//!   overflow-hidden` — 4 DIP radius, `card_stroke` border, `layer_background`
//!   ground, content clipped to the rounded box. Focus is `:has(:focus-visible)`
//!   → `outline: 3px solid var(--color-primary); outline-offset: -1px` from
//!   `index.css`; a contenteditable is `:focus-visible` on click too, which is
//!   why the host registers the box with [`crate::focus::FocusOpts::TEXT`].
//! * **Editable area**: `px-3 py-2 text-sm leading-relaxed`, `[&_a]:text-primary
//!   [&_a]:underline`, `[&_ul]:list-disc [&_ol]:list-decimal [&_ul]:ml-5
//!   [&_ol]:ml-5`. Body text stays at the desktop's 12 DIP body size (a user
//!   decision), with the web's `leading-relaxed` ratio applied to it.
//! * **Keyboard**: what a browser does in a contenteditable — arrows,
//!   Ctrl+arrows by word, Home / End on the visual line, Ctrl+Home / End,
//!   Page Up / Down, Shift to extend, Ctrl+A, Ctrl+B / I / U, Ctrl+Z / Y,
//!   Ctrl+C / X / V (plain text), Enter splits (an empty list item leaves the
//!   list), Shift+Enter a line break, Backspace at the start of a list item
//!   turns it back into a paragraph. Tab is left to the focus ring, as the web
//!   leaves it to the browser.
//! * **Beyond the web toolbar**: strike-through, inline code and the two
//!   heading levels are not on `RichText.tsx`'s bar; they are in the model
//!   because the task asks the editing area to carry them, and a host can drive
//!   them through [`RichTextBox::toggle_mark`] / [`RichTextBox::set_block`].
//! * **Italic**: the shared [`kubuno_drive_desktop_app_controls::TextFormats`] has no slanted
//!   face, so the few derived formats a run needs (italic, bold, monospace,
//!   heading sizes) are built here from the family of `formats().body` — the
//!   user's font override is therefore honoured — and cached per thread.
//! * **IME**: committed text arrives through the host's `Text` events and is
//!   inserted like typing. The composition string itself is not exposed by the
//!   host, so it is not drawn inline.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};

use kubuno_drive_desktop_app_controls::{Canvas, Rect};
use kubuno_desktop_controls::enums::Size;
use kubuno_desktop_controls::host::{self, vk, Cursor, InputEvent, Modifiers};
use kubuno_desktop_controls::text::RichTextBox as RichTextModel;
use kubuno_desktop_controls::Control;
use windows::core::HSTRING;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_WORD_WRAPPING_NO_WRAP,
};

use crate::editors::{rich_metrics, RichTextCommand, RichTextToolbar};
use crate::metrics::{height, radius, space, text};
use crate::range::{ScrollBar, ScrollPart};
use kubuno_desktop_controls::toolstrip::StripItem;
use crate::widget::{Widget, WidgetState};

// ═════════════════════════════════════════════════════════════════════════════
// Metrics
// ═════════════════════════════════════════════════════════════════════════════

/// Every measurement of the editing area, with its source.
pub mod richtext_metrics {
    use crate::metrics::{space, text};

    /// `px-3 py-2` on the contenteditable.
    pub const PAD_X: f32 = space::MD;
    pub const PAD_Y: f32 = space::SM;
    /// `minHeight = 96` — the prop's default, applied to the editable area.
    pub const MIN_HEIGHT: f32 = 96.0;
    /// `leading-relaxed` (1.625), applied to the 13.5 DIP body.
    pub const LEADING: f32 = 1.625;
    /// One body line box.
    pub const LINE_BODY: f32 = text::BODY * LEADING;
    /// **No web source**: `RichText.tsx` has no heading command. A heading is
    /// the design system's heading / title step at a tighter leading (1.375 /
    /// 1.25, Tailwind's `leading-snug` / `leading-tight`), so a heading line is
    /// not twice as airy as its size.
    pub const LINE_H2: f32 = text::HEADING * 1.375;
    pub const LINE_H1: f32 = text::TITLE * 1.25;
    /// `[&_ul]:ml-5 [&_ol]:ml-5`.
    pub const LIST_INDENT: f32 = 20.0;
    /// The marker (`list-style-position: outside`) ends this far before the
    /// item's text — the space a browser puts after « • » and « 1. ».
    pub const MARKER_GAP: f32 = space::XS;
    /// `outline: 3px` of `.kb-field-focus:has(:focus-visible)`, drawn inward so
    /// the ring stays inside the bounds the caller gave.
    pub const FOCUS_OUTLINE: f32 = 3.0;
    /// The caret: one DIP, `currentColor` (the web never sets `caret-color`).
    pub const CARET_W: f32 = 1.0;
    /// The browser's highlight: `--color-primary` at 35 % — the same recipe as
    /// `edit_box`, since the web declares no `::selection` rule.
    pub const SELECTION_ALPHA: f32 = 0.35;
    /// How wide the highlight of a selected line break is (a browser paints a
    /// sliver where the newline is).
    pub const NEWLINE_SELECTION: f32 = 4.0;
    /// Underline / strike-through thickness (`text-decoration-thickness: auto`
    /// resolves to one pixel at this size).
    pub const DECORATION: f32 = 1.0;
    /// Where the decorations sit, as a fraction of the font size below the line
    /// box centre. DirectWrite metrics are not exposed through `Canvas`, so
    /// these are Segoe UI's (ascent 1.08 em, descent 0.25 em): the baseline is
    /// ≈ 0.415 em under the centre, the underline one tenth of an em lower, the
    /// strike-through 0.3 em above the baseline.
    pub const UNDERLINE_EM: f32 = 0.52;
    pub const STRIKE_EM: f32 = 0.12;
    /// The inline-code chip's corner (`rounded`).
    pub const CODE_RADIUS: f32 = 3.0;
    /// The link row: `px-2 py-1.5 gap-1.5`, an `@ui/Input` (`h-9`, `px-3`) and
    /// an « OK » text button (`px-2`).
    pub const LINK_PAD_X: f32 = space::SM;
    pub const LINK_PAD_Y: f32 = 6.0;
    pub const LINK_GAP: f32 = 6.0;
    pub const LINK_OK_PAD_X: f32 = space::SM;
    /// `@ui/Input`'s `focus:ring-2`.
    pub const INPUT_RING: f32 = 2.0;
    /// Undo depth. A browser keeps its whole session; this caps memory.
    pub const UNDO_LIMIT: usize = 200;
    /// Dragging a selection past the viewport scrolls by this fraction of the
    /// overshoot per frame.
    pub const AUTOSCROLL_RATE: f32 = 0.35;
    /// The repaint period while that drag auto-scrolls (one 60 Hz frame).
    pub const AUTOSCROLL_MS: u32 = 16;
    /// `disabled` has no style on `RichText.tsx`; the field family's
    /// `disabled:opacity-60` is borrowed so every field greys the same way.
    pub const DISABLED_ALPHA: f32 = 0.6;
}
use richtext_metrics as m;

// ═════════════════════════════════════════════════════════════════════════════
// Document model
// ═════════════════════════════════════════════════════════════════════════════

/// The inline formatting of a run — what `execCommand('bold' | 'italic' |
/// 'underline' | 'strikeThrough' | 'createLink')` toggles on the web.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
    /// The `href` of an `<a>` around the run.
    pub link: Option<String>,
}

/// One on/off inline mark — everything in [`Marks`] but the link, which
/// carries a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Bold,
    Italic,
    Underline,
    Strike,
    Code,
}

impl Marks {
    pub const NONE: Marks =
        Marks { bold: false, italic: false, underline: false, strike: false, code: false, link: None };

    pub fn has(&self, mark: Mark) -> bool {
        match mark {
            Mark::Bold => self.bold,
            Mark::Italic => self.italic,
            Mark::Underline => self.underline,
            Mark::Strike => self.strike,
            Mark::Code => self.code,
        }
    }

    pub fn set(&mut self, mark: Mark, on: bool) {
        match mark {
            Mark::Bold => self.bold = on,
            Mark::Italic => self.italic = on,
            Mark::Underline => self.underline = on,
            Mark::Strike => self.strike = on,
            Mark::Code => self.code = on,
        }
    }

    /// Builder: `Marks::NONE.with(Mark::Bold)`.
    pub fn with(mut self, mark: Mark) -> Self {
        self.set(mark, true);
        self
    }

    /// Builder: a link to `href`.
    pub fn linked(mut self, href: impl Into<String>) -> Self {
        self.link = Some(href.into());
        self
    }

    /// What two runs have in common (the toolbar lights a mark only when the
    /// whole selection carries it).
    fn intersect(&self, other: &Marks) -> Marks {
        Marks {
            bold: self.bold && other.bold,
            italic: self.italic && other.italic,
            underline: self.underline && other.underline,
            strike: self.strike && other.strike,
            code: self.code && other.code,
            link: if self.link == other.link { self.link.clone() } else { None },
        }
    }
}

/// The block a paragraph is — `<div>`/`<p>`, `<h1>`, `<h2>`, `<ul><li>`,
/// `<ol><li>`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BlockKind {
    #[default]
    Paragraph,
    Heading1,
    Heading2,
    BulletItem,
    NumberedItem,
}

impl BlockKind {
    pub fn is_list(self) -> bool {
        matches!(self, BlockKind::BulletItem | BlockKind::NumberedItem)
    }

    pub fn is_heading(self) -> bool {
        matches!(self, BlockKind::Heading1 | BlockKind::Heading2)
    }
}

/// A run of text with one set of marks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub marks: Marks,
}

/// A paragraph-level block. `'\n'` inside a span is a hard line break
/// (`<br>`, Shift+Enter); blocks themselves are separated by Enter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub spans: Vec<Span>,
}

impl Block {
    pub fn new(kind: BlockKind) -> Self {
        Self { kind, spans: Vec::new() }
    }

    /// Builder: appends a run.
    pub fn with(mut self, text: impl Into<String>, marks: Marks) -> Self {
        self.spans.push(Span { text: text.into(), marks });
        self.normalize();
        self
    }

    /// Builder: appends an unformatted run.
    pub fn plain(self, text: impl Into<String>) -> Self {
        self.with(text, Marks::NONE)
    }

    /// The block's text, marks dropped.
    pub fn text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    /// Length in bytes (offsets into a block are byte offsets on char
    /// boundaries).
    pub fn len(&self) -> usize {
        self.spans.iter().map(|s| s.text.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Merges neighbours with equal marks and drops empty runs, so two
    /// documents with the same content compare equal.
    fn normalize(&mut self) {
        let mut out: Vec<Span> = Vec::with_capacity(self.spans.len());
        for s in self.spans.drain(..) {
            if s.text.is_empty() {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.marks == s.marks => last.text.push_str(&s.text),
                _ => out.push(s),
            }
        }
        self.spans = out;
    }

    /// Splits the run straddling `at` so a run boundary falls there, and
    /// returns the index of the first run that starts at or after `at`.
    fn split_at(&mut self, at: usize) -> usize {
        let mut start = 0;
        for i in 0..self.spans.len() {
            let len = self.spans[i].text.len();
            if at <= start {
                return i;
            }
            if at < start + len {
                let tail = self.spans[i].text.split_off(at - start);
                let marks = self.spans[i].marks.clone();
                self.spans.insert(i + 1, Span { text: tail, marks });
                return i + 1;
            }
            start += len;
        }
        self.spans.len()
    }

    fn insert(&mut self, at: usize, s: &str, marks: Marks) {
        let i = self.split_at(at);
        self.spans.insert(i, Span { text: s.to_string(), marks });
        self.normalize();
    }

    fn insert_spans(&mut self, at: usize, spans: Vec<Span>) {
        let i = self.split_at(at);
        for (k, s) in spans.into_iter().enumerate() {
            self.spans.insert(i + k, s);
        }
        self.normalize();
    }

    fn remove(&mut self, a: usize, b: usize) {
        if a >= b {
            return;
        }
        let i = self.split_at(a);
        let j = self.split_at(b);
        self.spans.drain(i..j);
        self.normalize();
    }

    /// Cuts the block at `at` and returns the runs after it.
    fn split_off(&mut self, at: usize) -> Vec<Span> {
        let i = self.split_at(at);
        let tail = self.spans.split_off(i);
        self.normalize();
        tail
    }

    fn apply(&mut self, a: usize, b: usize, f: &dyn Fn(&mut Marks)) {
        if a >= b {
            return;
        }
        let i = self.split_at(a);
        let j = self.split_at(b);
        for s in &mut self.spans[i..j] {
            f(&mut s.marks);
        }
        self.normalize();
    }

    /// The runs covering `[a, b)`, each paired with its start offset.
    fn runs_in(&self, a: usize, b: usize) -> Vec<(usize, &Span)> {
        let mut out = Vec::new();
        let mut start = 0;
        for s in &self.spans {
            let end = start + s.text.len();
            if end > a && start < b {
                out.push((start, s));
            }
            start = end;
        }
        out
    }

    /// The marks typing at `at` inherits: the character before it, or the one
    /// after it at the start of the block — what a browser does.
    pub fn marks_at(&self, at: usize) -> Marks {
        let mut start = 0;
        let mut after: Option<&Span> = None;
        for s in &self.spans {
            let end = start + s.text.len();
            if at > start && at <= end {
                return s.marks.clone();
            }
            if after.is_none() && start >= at {
                after = Some(s);
            }
            start = end;
        }
        after.map(|s| s.marks.clone()).unwrap_or_default()
    }

    /// The `(start, end)` of the link run around `at`, if any.
    fn link_extent(&self, at: usize) -> Option<(usize, usize)> {
        let href = self.marks_at(at).link?;
        // Walk the maximal stretch of runs carrying the same href.
        let mut starts = Vec::new();
        let mut start = 0;
        for s in &self.spans {
            starts.push((start, start + s.text.len(), s.marks.link.as_deref() == Some(href.as_str())));
            start += s.text.len();
        }
        let idx = starts.iter().position(|&(a, b, _)| at >= a && at <= b && b > a)?;
        let (mut lo, mut hi) = (idx, idx);
        while lo > 0 && starts[lo - 1].2 {
            lo -= 1;
        }
        while hi + 1 < starts.len() && starts[hi + 1].2 {
            hi += 1;
        }
        Some((starts[lo].0, starts[hi].1))
    }
}

/// A place in the document: a block index and a byte offset into that block's
/// text (always on a char boundary). Ordered in reading order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub block: usize,
    pub offset: usize,
}

impl Pos {
    pub const fn new(block: usize, offset: usize) -> Self {
        Self { block, offset }
    }
}

/// The selection: where it started (`anchor`) and where the caret is
/// (`head`). Collapsed when both are equal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Pos,
    pub head: Pos,
}

impl Selection {
    pub fn caret(p: Pos) -> Self {
        Self { anchor: p, head: p }
    }

    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.head
    }

    /// `(start, end)` in reading order.
    pub fn range(&self) -> (Pos, Pos) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

/// What an edit was, for undo coalescing: consecutive typing (or deleting)
/// at the caret collapses into one undo step, as in a browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Typing,
    Deleting,
    Other,
}

#[derive(Debug, Clone)]
struct Snapshot {
    blocks: Vec<Block>,
    sel: Selection,
}

/// The character class a word motion walks over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Space,
    Word,
    Punct,
}

fn class_of(c: char) -> CharClass {
    if c.is_whitespace() {
        CharClass::Space
    } else if c.is_alphanumeric() || c == '_' || c == '\'' || c == '’' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

fn prev_boundary(s: &str, at: usize) -> usize {
    s[..at].char_indices().next_back().map(|(i, _)| i).unwrap_or(0)
}

fn next_boundary(s: &str, at: usize) -> usize {
    s[at..].chars().next().map(|c| at + c.len_utf8()).unwrap_or(at)
}

/// Where Ctrl+Left lands from `at` in `s`: back over spaces, then over one
/// run of the same class.
fn word_left(s: &str, at: usize) -> usize {
    let mut i = at;
    while i > 0 {
        let p = prev_boundary(s, i);
        if class_of(s[p..].chars().next().unwrap_or(' ')) != CharClass::Space {
            break;
        }
        i = p;
    }
    if i == 0 {
        return 0;
    }
    let p = prev_boundary(s, i);
    let cls = class_of(s[p..].chars().next().unwrap_or(' '));
    while i > 0 {
        let p = prev_boundary(s, i);
        if class_of(s[p..].chars().next().unwrap_or(' ')) != cls {
            break;
        }
        i = p;
    }
    i
}

/// Where Ctrl+Right lands from `at` in `s`, the Windows way: over one run of
/// the same class, then over the spaces after it — the start of the next word.
fn word_right(s: &str, at: usize) -> usize {
    let mut i = at;
    if let Some(c) = s[i..].chars().next() {
        let cls = class_of(c);
        if cls != CharClass::Space {
            while let Some(c) = s[i..].chars().next() {
                if class_of(c) != cls {
                    break;
                }
                i += c.len_utf8();
            }
        }
    }
    while let Some(c) = s[i..].chars().next() {
        if class_of(c) != CharClass::Space || c == '\n' {
            break;
        }
        i += c.len_utf8();
    }
    i
}

/// The word under `at` (a double-click): the run of one class that contains
/// the character after `at` (or before it at the end of the text).
fn word_around(s: &str, at: usize) -> (usize, usize) {
    if s.is_empty() {
        return (0, 0);
    }
    let probe = if at >= s.len() { prev_boundary(s, s.len()) } else { at };
    let cls = class_of(s[probe..].chars().next().unwrap_or(' '));
    let mut a = probe;
    while a > 0 {
        let p = prev_boundary(s, a);
        if class_of(s[p..].chars().next().unwrap_or(' ')) != cls {
            break;
        }
        a = p;
    }
    let mut b = probe;
    while let Some(c) = s[b..].chars().next() {
        if class_of(c) != cls {
            break;
        }
        b += c.len_utf8();
    }
    (a, b)
}

/// The rich document and its selection — the whole editing behaviour, with no
/// window, canvas or font involved.
#[derive(Debug, Clone)]
pub struct Document {
    blocks: Vec<Block>,
    sel: Selection,
    /// Marks toggled with a collapsed selection: they apply to the next typed
    /// text (the browser's « typing style »), and drop when the caret moves.
    stored: Option<Marks>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_edit: Option<EditKind>,
    revision: u64,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// One empty paragraph — an empty editor.
    pub fn new() -> Self {
        Self::from_blocks(Vec::new())
    }

    /// A document from its blocks; an empty list becomes one empty paragraph.
    pub fn from_blocks(mut blocks: Vec<Block>) -> Self {
        if blocks.is_empty() {
            blocks.push(Block::new(BlockKind::Paragraph));
        }
        for b in &mut blocks {
            b.normalize();
        }
        Self {
            blocks,
            sel: Selection::default(),
            stored: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: None,
            revision: 0,
        }
    }

    /// Plain text, one paragraph per line.
    pub fn from_plain(text: &str) -> Self {
        let blocks = text
            .replace("\r\n", "\n")
            .split('\n')
            .map(|l| Block::new(BlockKind::Paragraph).plain(l))
            .collect();
        Self::from_blocks(blocks)
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    pub fn selection(&self) -> Selection {
        self.sel
    }

    /// Bumped on every content change — a layout cache key.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Nothing but one empty paragraph: what shows the placeholder.
    pub fn is_empty(&self) -> bool {
        self.blocks.len() == 1 && self.blocks[0].is_empty() && self.blocks[0].kind == BlockKind::Paragraph
    }

    fn clamp(&self, p: Pos) -> Pos {
        let block = p.block.min(self.blocks.len() - 1);
        let text = self.blocks[block].text();
        let mut offset = p.offset.min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        Pos { block, offset }
    }

    /// Replaces the selection (clamped into the document).
    pub fn set_selection(&mut self, anchor: Pos, head: Pos) {
        let sel = Selection { anchor: self.clamp(anchor), head: self.clamp(head) };
        if sel != self.sel {
            self.stored = None;
            self.last_edit = None;
        }
        self.sel = sel;
    }

    /// Moves the caret to `p`, extending the selection when `extend`.
    pub fn move_to(&mut self, p: Pos, extend: bool) {
        let anchor = if extend { self.sel.anchor } else { p };
        self.set_selection(anchor, p);
    }

    pub fn select_all(&mut self) {
        let last = self.blocks.len() - 1;
        self.set_selection(Pos::new(0, 0), Pos::new(last, self.blocks[last].len()));
    }

    pub fn start(&self) -> Pos {
        Pos::new(0, 0)
    }

    pub fn end(&self) -> Pos {
        let last = self.blocks.len() - 1;
        Pos::new(last, self.blocks[last].len())
    }

    /// One character left (or right) of `p`, crossing block boundaries.
    pub fn char_left(&self, p: Pos) -> Pos {
        if p.offset > 0 {
            Pos::new(p.block, prev_boundary(&self.blocks[p.block].text(), p.offset))
        } else if p.block > 0 {
            Pos::new(p.block - 1, self.blocks[p.block - 1].len())
        } else {
            p
        }
    }

    pub fn char_right(&self, p: Pos) -> Pos {
        let len = self.blocks[p.block].len();
        if p.offset < len {
            Pos::new(p.block, next_boundary(&self.blocks[p.block].text(), p.offset))
        } else if p.block + 1 < self.blocks.len() {
            Pos::new(p.block + 1, 0)
        } else {
            p
        }
    }

    /// Ctrl+Left / Ctrl+Right from `p`.
    pub fn word_left(&self, p: Pos) -> Pos {
        if p.offset == 0 {
            return self.char_left(p);
        }
        Pos::new(p.block, word_left(&self.blocks[p.block].text(), p.offset))
    }

    pub fn word_right(&self, p: Pos) -> Pos {
        let len = self.blocks[p.block].len();
        if p.offset >= len {
            return self.char_right(p);
        }
        Pos::new(p.block, word_right(&self.blocks[p.block].text(), p.offset))
    }

    /// The word under `p` — a double-click.
    pub fn word_range(&self, p: Pos) -> (Pos, Pos) {
        let (a, b) = word_around(&self.blocks[p.block].text(), p.offset);
        (Pos::new(p.block, a), Pos::new(p.block, b))
    }

    /// The whole block — a triple-click selects the paragraph.
    pub fn block_range(&self, block: usize) -> (Pos, Pos) {
        let block = block.min(self.blocks.len() - 1);
        (Pos::new(block, 0), Pos::new(block, self.blocks[block].len()))
    }

    /// Left / Right as a browser does it: a selection collapses to its near
    /// edge first; otherwise the caret steps by a character (or a word).
    pub fn move_horizontal(&mut self, forward: bool, word: bool, extend: bool) {
        let (a, b) = self.sel.range();
        if !extend && a != b {
            self.move_to(if forward { b } else { a }, false);
            return;
        }
        let h = self.sel.head;
        let p = match (forward, word) {
            (false, false) => self.char_left(h),
            (true, false) => self.char_right(h),
            (false, true) => self.word_left(h),
            (true, true) => self.word_right(h),
        };
        self.move_to(p, extend);
    }

    // ── Reading ───────────────────────────────────────────────────────────────

    /// The whole text, blocks separated by `\n`.
    pub fn plain_text(&self) -> String {
        self.blocks.iter().map(Block::text).collect::<Vec<_>>().join("\n")
    }

    /// The text of `[a, b)`.
    pub fn text_in(&self, a: Pos, b: Pos) -> String {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let mut out = String::new();
        for i in a.block..=b.block {
            let t = self.blocks[i].text();
            let lo = if i == a.block { a.offset } else { 0 };
            let hi = if i == b.block { b.offset } else { t.len() };
            out.push_str(&t[lo.min(hi)..hi]);
            if i != b.block {
                out.push('\n');
            }
        }
        out
    }

    /// The selected text, as plain text (what Ctrl+C puts on the clipboard).
    pub fn selected_text(&self) -> String {
        let (a, b) = self.sel.range();
        self.text_in(a, b)
    }

    /// The marks the toolbar shows: the typing style when one is pending, the
    /// marks under a collapsed caret, else what the whole selection shares.
    pub fn active_marks(&self) -> Marks {
        if let Some(s) = &self.stored {
            return s.clone();
        }
        let (a, b) = self.sel.range();
        if a == b {
            return self.blocks[a.block].marks_at(a.offset);
        }
        let mut acc: Option<Marks> = None;
        for i in a.block..=b.block {
            let blk = &self.blocks[i];
            let lo = if i == a.block { a.offset } else { 0 };
            let hi = if i == b.block { b.offset } else { blk.len() };
            for (_, s) in blk.runs_in(lo, hi) {
                acc = Some(match acc {
                    None => s.marks.clone(),
                    Some(m) => m.intersect(&s.marks),
                });
            }
        }
        acc.unwrap_or_else(|| self.blocks[a.block].marks_at(a.offset))
    }

    /// The kind of the block holding the caret.
    pub fn active_block(&self) -> BlockKind {
        self.blocks[self.sel.head.block].kind
    }

    /// The number a numbered item shows: its rank in the run of consecutive
    /// numbered items it belongs to (`<ol>` restarts after any other block).
    pub fn list_number(&self, block: usize) -> Option<usize> {
        if self.blocks.get(block)?.kind != BlockKind::NumberedItem {
            return None;
        }
        let mut n = 1;
        let mut i = block;
        while i > 0 && self.blocks[i - 1].kind == BlockKind::NumberedItem {
            n += 1;
            i -= 1;
        }
        Some(n)
    }

    /// The link under the caret, if any.
    pub fn link_at_caret(&self) -> Option<String> {
        self.active_marks().link
    }

    /// The document as the web's controlled `value`: HTML with `<p>`, `<h1>`,
    /// `<h2>`, `<ul>` / `<ol>` + `<li>`, `<b>`, `<i>`, `<u>`, `<s>`, `<code>`,
    /// `<a href>` and `<br>`. An empty document is `""`, like `RichText.tsx`'s
    /// `onChange(isEmpty ? '' : html)`.
    pub fn to_html(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        fn esc(s: &str) -> String {
            s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
        }
        let mut out = String::new();
        let mut open_list: Option<BlockKind> = None;
        for b in &self.blocks {
            let list = if b.kind.is_list() { Some(b.kind) } else { None };
            if open_list != list {
                match open_list {
                    Some(BlockKind::BulletItem) => out.push_str("</ul>"),
                    Some(_) => out.push_str("</ol>"),
                    None => {}
                }
                match list {
                    Some(BlockKind::BulletItem) => out.push_str("<ul>"),
                    Some(_) => out.push_str("<ol>"),
                    None => {}
                }
                open_list = list;
            }
            let tag = match b.kind {
                BlockKind::Paragraph => "p",
                BlockKind::Heading1 => "h1",
                BlockKind::Heading2 => "h2",
                BlockKind::BulletItem | BlockKind::NumberedItem => "li",
            };
            out.push_str(&format!("<{tag}>"));
            for s in &b.spans {
                let mut inner = esc(&s.text).replace('\n', "<br>");
                let wraps: [(bool, &str); 5] = [
                    (s.marks.code, "code"),
                    (s.marks.strike, "s"),
                    (s.marks.underline, "u"),
                    (s.marks.italic, "i"),
                    (s.marks.bold, "b"),
                ];
                for (on, t) in wraps {
                    if on {
                        inner = format!("<{t}>{inner}</{t}>");
                    }
                }
                if let Some(href) = &s.marks.link {
                    inner = format!("<a href=\"{}\">{inner}</a>", esc(href));
                }
                out.push_str(&inner);
            }
            out.push_str(&format!("</{tag}>"));
        }
        match open_list {
            Some(BlockKind::BulletItem) => out.push_str("</ul>"),
            Some(_) => out.push_str("</ol>"),
            None => {}
        }
        out
    }

    // ── Undo ──────────────────────────────────────────────────────────────────

    fn snapshot(&self) -> Snapshot {
        Snapshot { blocks: self.blocks.clone(), sel: self.sel }
    }

    /// Records the state before an edit, unless it continues the previous one
    /// (typing after typing at the caret, deleting after deleting).
    fn begin(&mut self, kind: EditKind) {
        let coalesce = kind != EditKind::Other && self.last_edit == Some(kind) && self.sel.is_collapsed();
        if !coalesce {
            self.undo.push(self.snapshot());
            if self.undo.len() > m::UNDO_LIMIT {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
    }

    fn finish(&mut self, kind: EditKind) {
        self.revision += 1;
        self.last_edit = Some(kind);
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(s) = self.undo.pop() else { return false };
        self.redo.push(self.snapshot());
        self.blocks = s.blocks;
        self.sel = s.sel;
        self.stored = None;
        self.last_edit = None;
        self.revision += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(s) = self.redo.pop() else { return false };
        self.undo.push(self.snapshot());
        self.blocks = s.blocks;
        self.sel = s.sel;
        self.stored = None;
        self.last_edit = None;
        self.revision += 1;
        true
    }

    // ── Editing ───────────────────────────────────────────────────────────────

    /// Removes `[a, b)` and returns where the caret lands. The merged block
    /// keeps the first block's kind.
    fn delete_range_raw(&mut self, a: Pos, b: Pos) -> Pos {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        if a == b {
            return a;
        }
        if a.block == b.block {
            self.blocks[a.block].remove(a.offset, b.offset);
        } else {
            let len_a = self.blocks[a.block].len();
            self.blocks[a.block].remove(a.offset, len_a);
            let tail = self.blocks[b.block].split_off(b.offset);
            self.blocks.drain(a.block + 1..=b.block);
            let at = self.blocks[a.block].len();
            self.blocks[a.block].insert_spans(at, tail);
        }
        a
    }

    /// Splits the block at `p` (Enter). Returns the caret after the split.
    fn split_raw(&mut self, p: Pos) -> Pos {
        let kind = self.blocks[p.block].kind;
        if kind.is_list() && self.blocks[p.block].is_empty() {
            // An empty list item leaves the list rather than adding another.
            self.blocks[p.block].kind = BlockKind::Paragraph;
            return p;
        }
        let tail = self.blocks[p.block].split_off(p.offset);
        let new_kind = if kind.is_heading() && tail.is_empty() { BlockKind::Paragraph } else { kind };
        let mut nb = Block { kind: new_kind, spans: tail };
        nb.normalize();
        self.blocks.insert(p.block + 1, nb);
        Pos::new(p.block + 1, 0)
    }

    /// Typed or pasted text replaces the selection. `\n` starts a new block
    /// (so pasted lines become paragraphs, or items inside a list).
    pub fn insert_text(&mut self, s: &str) {
        let s = s.replace("\r\n", "\n").replace('\r', "\n");
        if s.is_empty() {
            return;
        }
        let kind = if s.contains('\n') { EditKind::Other } else { EditKind::Typing };
        self.begin(kind);
        let marks = match self.stored.take() {
            Some(m) => m,
            None => {
                let (a, _) = self.sel.range();
                self.blocks[a.block].marks_at(a.offset)
            }
        };
        let (a, b) = self.sel.range();
        let mut p = self.delete_range_raw(a, b);
        for (i, line) in s.split('\n').enumerate() {
            if i > 0 {
                p = self.split_raw(p);
            }
            if !line.is_empty() {
                self.blocks[p.block].insert(p.offset, line, marks.clone());
                p.offset += line.len();
            }
        }
        self.sel = Selection::caret(p);
        self.finish(kind);
    }

    /// Shift+Enter: a line break inside the block (`<br>`).
    pub fn insert_line_break(&mut self) {
        self.begin(EditKind::Other);
        let (a, b) = self.sel.range();
        let p = self.delete_range_raw(a, b);
        let marks = self.blocks[p.block].marks_at(p.offset);
        self.blocks[p.block].insert(p.offset, "\n", marks);
        self.sel = Selection::caret(Pos::new(p.block, p.offset + 1));
        self.finish(EditKind::Other);
    }

    /// Enter.
    pub fn insert_paragraph(&mut self) {
        self.begin(EditKind::Other);
        let (a, b) = self.sel.range();
        let p = self.delete_range_raw(a, b);
        let p = self.split_raw(p);
        self.sel = Selection::caret(p);
        self.stored = None;
        self.finish(EditKind::Other);
    }

    /// Deletes the selection; returns false when it was collapsed.
    pub fn delete_selection(&mut self) -> bool {
        if self.sel.is_collapsed() {
            return false;
        }
        self.begin(EditKind::Other);
        let (a, b) = self.sel.range();
        let p = self.delete_range_raw(a, b);
        self.sel = Selection::caret(p);
        self.finish(EditKind::Other);
        true
    }

    /// Backspace (Ctrl+Backspace with `word`).
    pub fn delete_backward(&mut self, word: bool) {
        if self.delete_selection() {
            return;
        }
        let h = self.sel.head;
        if h.offset == 0 {
            let kind = self.blocks[h.block].kind;
            if kind.is_list() || (kind.is_heading() && h.block == 0) {
                // A list item (or a heading with nothing before it) turns back
                // into a paragraph first — the browser's outdent.
                self.begin(EditKind::Other);
                self.blocks[h.block].kind = BlockKind::Paragraph;
                self.finish(EditKind::Other);
                return;
            }
            if h.block == 0 {
                return;
            }
            self.begin(EditKind::Other);
            let prev = Pos::new(h.block - 1, self.blocks[h.block - 1].len());
            let p = self.delete_range_raw(prev, h);
            self.sel = Selection::caret(p);
            self.finish(EditKind::Other);
            return;
        }
        let from = if word { self.word_left(h) } else { self.char_left(h) };
        self.begin(EditKind::Deleting);
        let p = self.delete_range_raw(from, h);
        self.sel = Selection::caret(p);
        self.finish(EditKind::Deleting);
    }

    /// Delete (Ctrl+Delete with `word`).
    pub fn delete_forward(&mut self, word: bool) {
        if self.delete_selection() {
            return;
        }
        let h = self.sel.head;
        let to = if word { self.word_right(h) } else { self.char_right(h) };
        if to == h {
            return;
        }
        let kind = if to.block != h.block { EditKind::Other } else { EditKind::Deleting };
        self.begin(kind);
        let p = self.delete_range_raw(h, to);
        self.sel = Selection::caret(p);
        self.finish(kind);
    }

    /// Applies `f` to the marks of every run in the selection.
    fn apply_marks(&mut self, f: &dyn Fn(&mut Marks)) {
        let (a, b) = self.sel.range();
        for i in a.block..=b.block {
            let len = self.blocks[i].len();
            let lo = if i == a.block { a.offset } else { 0 };
            let hi = if i == b.block { b.offset } else { len };
            self.blocks[i].apply(lo, hi, f);
        }
    }

    /// Ctrl+B / I / U and the toolbar's mark buttons: on a selection, the mark
    /// goes on unless every selected run already has it; on a caret, it
    /// toggles the typing style.
    pub fn toggle_mark(&mut self, mark: Mark) {
        let on = !self.active_marks().has(mark);
        self.set_mark(mark, on);
    }

    pub fn set_mark(&mut self, mark: Mark, on: bool) {
        if self.sel.is_collapsed() {
            let mut s = self.active_marks();
            s.set(mark, on);
            self.stored = Some(s);
            return;
        }
        self.begin(EditKind::Other);
        self.apply_marks(&|mk: &mut Marks| mk.set(mark, on));
        self.finish(EditKind::Other);
    }

    /// `createLink` / `unlink`. With a selection, the runs get (or lose) the
    /// href. With a caret, `Some(url)` inserts the URL itself as a link — what
    /// `createLink` does on a collapsed range — and `None` unlinks the link
    /// the caret is in.
    pub fn set_link(&mut self, href: Option<&str>) {
        if self.sel.is_collapsed() {
            let p = self.sel.head;
            match href {
                Some(url) if !url.is_empty() => {
                    self.begin(EditKind::Other);
                    let marks = Marks { link: Some(url.to_string()), ..self.blocks[p.block].marks_at(p.offset) };
                    self.blocks[p.block].insert(p.offset, url, marks);
                    self.sel = Selection::caret(Pos::new(p.block, p.offset + url.len()));
                    self.finish(EditKind::Other);
                }
                Some(_) => {}
                None => {
                    if let Some((a, b)) = self.blocks[p.block].link_extent(p.offset) {
                        self.begin(EditKind::Other);
                        self.blocks[p.block].apply(a, b, &|mk: &mut Marks| mk.link = None);
                        self.finish(EditKind::Other);
                    }
                }
            }
            return;
        }
        self.begin(EditKind::Other);
        let link = href.filter(|u| !u.is_empty()).map(str::to_string);
        self.apply_marks(&move |mk: &mut Marks| mk.link = link.clone());
        self.finish(EditKind::Other);
    }

    /// `removeFormat`: every inline mark off the selection (links included,
    /// as the toolbar's eraser does), or the typing style reset on a caret.
    pub fn clear_formatting(&mut self) {
        if self.sel.is_collapsed() {
            self.stored = Some(Marks::NONE);
            return;
        }
        self.begin(EditKind::Other);
        self.apply_marks(&|mk: &mut Marks| *mk = Marks::NONE);
        self.finish(EditKind::Other);
    }

    /// Sets the kind of every block the selection touches.
    pub fn set_block(&mut self, kind: BlockKind) {
        let (a, b) = self.sel.range();
        if (a.block..=b.block).all(|i| self.blocks[i].kind == kind) {
            return;
        }
        self.begin(EditKind::Other);
        for i in a.block..=b.block {
            self.blocks[i].kind = kind;
        }
        self.finish(EditKind::Other);
    }

    /// `insertOrderedList` / `insertUnorderedList`: the blocks become items of
    /// `kind`, or back to paragraphs when they all already are.
    pub fn toggle_block(&mut self, kind: BlockKind) {
        let (a, b) = self.sel.range();
        let all = (a.block..=b.block).all(|i| self.blocks[i].kind == kind);
        self.set_block(if all { BlockKind::Paragraph } else { kind });
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Fonts
// ═════════════════════════════════════════════════════════════════════════════

/// What selects a run's text format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FontKey {
    size_centi: u32,
    weight: i32,
    italic: bool,
    mono: bool,
}

struct FormatCache {
    factory: Option<IDWriteFactory>,
    family: String,
    entries: Vec<(FontKey, IDWriteTextFormat)>,
}

thread_local! {
    static FORMATS: RefCell<FormatCache> =
        const { RefCell::new(FormatCache { factory: None, family: String::new(), entries: Vec::new() }) };
}

/// The web's `font-mono` stack resolves to Consolas on Windows.
const MONO_FAMILY: &str = "Consolas";

/// The family `formats().body` was built with (the user's font override when
/// there is one).
fn body_family(c: &dyn Canvas) -> String {
    let f = &c.formats().body;
    // SAFETY: plain COM getters on a live text format.
    unsafe {
        let n = f.GetFontFamilyNameLength() as usize;
        let mut buf = vec![0u16; n + 1];
        if f.GetFontFamilyName(&mut buf).is_err() {
            return String::from("Segoe UI");
        }
        String::from_utf16_lossy(&buf[..n])
    }
}

fn block_size(kind: BlockKind) -> f32 {
    match kind {
        BlockKind::Heading1 => text::TITLE,
        BlockKind::Heading2 => text::HEADING,
        _ => text::BODY,
    }
}

fn line_height(kind: BlockKind) -> f32 {
    match kind {
        BlockKind::Heading1 => m::LINE_H1,
        BlockKind::Heading2 => m::LINE_H2,
        _ => m::LINE_BODY,
    }
}

fn font_key(kind: BlockKind, marks: &Marks) -> FontKey {
    let weight: DWRITE_FONT_WEIGHT = if marks.bold {
        DWRITE_FONT_WEIGHT_BOLD
    } else if kind.is_heading() {
        DWRITE_FONT_WEIGHT_SEMI_BOLD
    } else {
        DWRITE_FONT_WEIGHT_NORMAL
    };
    FontKey {
        size_centi: (block_size(kind) * 100.0).round() as u32,
        weight: weight.0,
        italic: marks.italic,
        mono: marks.code,
    }
}

/// The shared format closest to `key` — used when DirectWrite cannot build a
/// derived one (it never fails on a real machine; this keeps text visible if
/// it does).
fn fallback_format(c: &dyn Canvas, key: FontKey) -> IDWriteTextFormat {
    let f = c.formats();
    let size = key.size_centi as f32 / 100.0;
    if size >= text::TITLE {
        f.title.clone()
    } else if size >= text::HEADING {
        f.heading_strong.clone()
    } else if key.weight > DWRITE_FONT_WEIGHT_NORMAL.0 {
        f.body_strong.clone()
    } else {
        f.body.clone()
    }
}

fn format_for(c: &dyn Canvas, key: FontKey) -> IDWriteTextFormat {
    FORMATS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let family = body_family(c);
        if cache.family != family {
            cache.family = family;
            cache.entries.clear();
        }
        if let Some((_, f)) = cache.entries.iter().find(|(k, _)| *k == key) {
            return f.clone();
        }
        if cache.factory.is_none() {
            // SAFETY: creating the shared DirectWrite factory has no
            // preconditions.
            cache.factory = unsafe { DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) }.ok();
        }
        let Some(factory) = cache.factory.clone() else {
            return fallback_format(c, key);
        };
        let family = if key.mono { MONO_FAMILY.to_string() } else { cache.family.clone() };
        let style = if key.italic { DWRITE_FONT_STYLE_ITALIC } else { DWRITE_FONT_STYLE_NORMAL };
        // SAFETY: plain COM calls on a live factory.
        let made = unsafe {
            factory
                .CreateTextFormat(
                    &HSTRING::from(family.as_str()),
                    None,
                    DWRITE_FONT_WEIGHT(key.weight),
                    style,
                    DWRITE_FONT_STRETCH_NORMAL,
                    key.size_centi as f32 / 100.0,
                    &HSTRING::from("fr-FR"),
                )
                .and_then(|f| f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP).map(|_| f))
        };
        match made {
            Ok(f) => {
                cache.entries.push((key, f.clone()));
                f
            }
            Err(_) => fallback_format(c, key),
        }
    })
}

// ═════════════════════════════════════════════════════════════════════════════
// Layout
// ═════════════════════════════════════════════════════════════════════════════

/// A measured piece of one run on one line.
#[derive(Clone)]
struct Frag {
    start: usize,
    end: usize,
    x: f32,
    w: f32,
    key: FontKey,
    marks: Marks,
}

#[derive(Clone)]
struct Line {
    block: usize,
    start: usize,
    end: usize,
    top: f32,
    h: f32,
    /// Where the text starts (after a list indent).
    x0: f32,
    frags: Vec<Frag>,
    /// The first line of its block (where a list marker goes).
    first: bool,
    /// The last line of its block.
    last: bool,
}

#[derive(Clone)]
struct Layout {
    revision: u64,
    width: f32,
    lines: Vec<Line>,
    height: f32,
    /// Each block's text, so painting and hit-testing never re-concatenate.
    texts: Vec<String>,
}

/// A breakable unit: a word and the spaces after it, cut at run boundaries.
struct Atom {
    start: usize,
    end: usize,
    span: usize,
    w: f32,
    /// Width without the trailing spaces (what must fit on the line).
    w_fit: f32,
    /// A break is allowed after this atom.
    soft: bool,
    /// A `\n`: the line ends after it.
    hard: bool,
}

fn atoms_of(c: &dyn Canvas, block: &Block) -> Vec<Atom> {
    let mut out = Vec::new();
    let mut base = 0;
    for (si, s) in block.spans.iter().enumerate() {
        let fmt = format_for(c, font_key(block.kind, &s.marks));
        let t = &s.text;
        let mut i = 0;
        while i < t.len() {
            let rest = &t[i..];
            if let Some(stripped) = rest.strip_prefix('\n') {
                let _ = stripped;
                out.push(Atom { start: base + i, end: base + i + 1, span: si, w: 0.0, w_fit: 0.0, soft: false, hard: true });
                i += 1;
                continue;
            }
            // The word part, then the spaces after it.
            let word_len = rest.find(|ch: char| ch.is_whitespace()).unwrap_or(rest.len());
            let after = &rest[word_len..];
            let ws_len = after.find(|ch: char| !ch.is_whitespace() || ch == '\n').unwrap_or(after.len());
            let len = word_len + ws_len;
            let piece = &rest[..len];
            let w = c.measure(piece, &fmt);
            let w_fit = if ws_len > 0 { c.measure(&rest[..word_len], &fmt) } else { w };
            out.push(Atom {
                start: base + i,
                end: base + i + len,
                span: si,
                w,
                w_fit,
                soft: ws_len > 0,
                hard: false,
            });
            i += len;
        }
        base += t.len();
    }
    out
}

/// Starts a new line of block `bi` at byte `start`, below `cur`.
fn break_line(lines: &mut Vec<Line>, cur: &mut Line, start: usize, x: &mut f32) {
    let next = Line {
        block: cur.block,
        start,
        end: start,
        top: cur.top + cur.h,
        h: cur.h,
        x0: cur.x0,
        frags: Vec::new(),
        first: false,
        last: false,
    };
    lines.push(std::mem::replace(cur, next));
    *x = 0.0;
}

/// Appends `[start, end)` of run `span` to the line, merging it into the
/// previous fragment when that one has the same marks and ends there.
fn push_frag(cur: &mut Line, x: &mut f32, block: &Block, start: usize, end: usize, w: f32, span: usize) {
    let marks = &block.spans[span].marks;
    if let Some(last) = cur.frags.last_mut() {
        if last.end == start && last.marks == *marks {
            last.end = end;
            last.w += w;
            cur.end = end;
            *x += w;
            return;
        }
    }
    cur.frags.push(Frag {
        start,
        end,
        x: cur.x0 + *x,
        w,
        key: font_key(block.kind, marks),
        marks: marks.clone(),
    });
    cur.end = end;
    *x += w;
}

impl Layout {
    fn build(c: &dyn Canvas, doc: &Document, width: f32) -> Layout {
        let mut lines = Vec::new();
        let mut texts = Vec::new();
        let mut y = 0.0;
        for (bi, block) in doc.blocks.iter().enumerate() {
            let text = block.text();
            let indent = if block.kind.is_list() { m::LIST_INDENT } else { 0.0 };
            let avail = (width - indent).max(1.0);
            let atoms = atoms_of(c, block);
            let mut cur = Line {
                block: bi,
                start: 0,
                end: 0,
                top: y,
                h: line_height(block.kind),
                x0: indent,
                frags: Vec::new(),
                first: true,
                last: false,
            };
            let mut x = 0.0;

            let mut i = 0;
            while i < atoms.len() {
                // One word: atoms up to (and including) a break opportunity.
                let mut j = i;
                while !atoms[j].soft && !atoms[j].hard && j + 1 < atoms.len() && !atoms[j + 1].hard {
                    j += 1;
                }
                let word = &atoms[i..=j];
                let fit: f32 = word[..word.len() - 1].iter().map(|a| a.w).sum::<f32>() + word[word.len() - 1].w_fit;
                if !cur.frags.is_empty() && x + fit > avail {
                    break_line(&mut lines, &mut cur, word[0].start, &mut x);
                }
                for a in word {
                    if a.hard {
                        cur.end = a.end;
                        break_line(&mut lines, &mut cur, a.end, &mut x);
                        continue;
                    }
                    if x + a.w_fit <= avail {
                        push_frag(&mut cur, &mut x, block, a.start, a.end, a.w, a.span);
                        continue;
                    }
                    if !cur.frags.is_empty() && a.w_fit <= avail {
                        break_line(&mut lines, &mut cur, a.start, &mut x);
                        push_frag(&mut cur, &mut x, block, a.start, a.end, a.w, a.span);
                        continue;
                    }
                    // `overflow-wrap: break-word` (a contenteditable's
                    // default): cut the atom by characters.
                    let fmt = format_for(c, font_key(block.kind, &block.spans[a.span].marks));
                    let mut s = a.start;
                    while s < a.end {
                        let room = avail - x;
                        let (mut e, mut w) = (s, 0.0);
                        let mut k = s;
                        while k < a.end {
                            let nk = next_boundary(&text, k);
                            let nw = c.measure(&text[s..nk], &fmt);
                            if nw > room {
                                break;
                            }
                            e = nk;
                            w = nw;
                            k = nk;
                        }
                        if e == s {
                            if cur.frags.is_empty() {
                                // Not even one character fits: take it anyway.
                                e = next_boundary(&text, s);
                                w = c.measure(&text[s..e], &fmt);
                            } else {
                                break_line(&mut lines, &mut cur, s, &mut x);
                                continue;
                            }
                        }
                        push_frag(&mut cur, &mut x, block, s, e, w, a.span);
                        s = e;
                        if s < a.end {
                            break_line(&mut lines, &mut cur, s, &mut x);
                        }
                    }
                }
                i = j + 1;
            }
            cur.last = true;
            y = cur.top + cur.h;
            lines.push(cur);
            texts.push(text);
        }
        Layout { revision: doc.revision, width, lines, height: y, texts }
    }

    /// The line holding `p`: the last line of its block starting at or before
    /// it (a position at a wrap point belongs to the next line, as in a
    /// browser).
    fn line_of(&self, p: Pos) -> usize {
        let mut found = 0;
        for (i, l) in self.lines.iter().enumerate() {
            if l.block == p.block && l.start <= p.offset {
                found = i;
            }
            if l.block > p.block {
                break;
            }
        }
        found
    }

    /// The x of the caret at `p` on line `li`.
    fn x_at(&self, c: &dyn Canvas, li: usize, offset: usize) -> f32 {
        let l = &self.lines[li];
        let text = &self.texts[l.block];
        for f in &l.frags {
            if offset >= f.start && offset <= f.end {
                let fmt = format_for(c, f.key);
                return f.x + c.measure(&text[f.start..offset], &fmt);
            }
        }
        match l.frags.last() {
            Some(f) if offset > f.end => f.x + f.w,
            _ => l.x0,
        }
    }

    /// The last caret position that still draws on line `li` (End).
    fn visual_end(&self, li: usize) -> usize {
        let l = &self.lines[li];
        if l.last {
            return l.end;
        }
        let text = &self.texts[l.block];
        let seg = &text[l.start..l.end];
        match seg.chars().next_back() {
            Some(ch) if ch.is_whitespace() => l.end - ch.len_utf8(),
            _ => l.end,
        }
    }

    /// The position nearest `x` on line `li`.
    fn offset_at_x(&self, c: &dyn Canvas, li: usize, x: f32) -> usize {
        let l = &self.lines[li];
        let text = &self.texts[l.block];
        let end = self.visual_end(li);
        let Some(first) = l.frags.first() else { return l.start };
        if x <= first.x {
            return l.start;
        }
        for f in &l.frags {
            if x <= f.x + f.w {
                let fmt = format_for(c, f.key);
                let mut prev_x = f.x;
                let mut k = f.start;
                while k < f.end {
                    let nk = next_boundary(text, k);
                    let nx = f.x + c.measure(&text[f.start..nk], &fmt);
                    if x < (prev_x + nx) / 2.0 {
                        return k.min(end);
                    }
                    prev_x = nx;
                    k = nk;
                }
                return f.end.min(end);
            }
        }
        end
    }

    /// The position nearest `(x, y)` in layout coordinates.
    fn hit(&self, c: &dyn Canvas, x: f32, y: f32) -> Pos {
        if self.lines.is_empty() {
            return Pos::default();
        }
        let li = if y < 0.0 {
            0
        } else {
            self.lines.iter().position(|l| y < l.top + l.h).unwrap_or(self.lines.len() - 1)
        };
        Pos::new(self.lines[li].block, self.offset_at_x(c, li, x))
    }

    /// The caret's line box, in layout coordinates.
    fn caret_box(&self, c: &dyn Canvas, p: Pos) -> Rect {
        if self.lines.is_empty() {
            return Rect::new(0.0, 0.0, m::CARET_W, m::LINE_BODY);
        }
        let li = self.line_of(p);
        let l = &self.lines[li];
        let x = self.x_at(c, li, p.offset);
        Rect::new(x, l.top, x + m::CARET_W, l.top + l.h)
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// RichTextBox
// ═════════════════════════════════════════════════════════════════════════════

/// One frame of pointer input, in the coordinates `bounds` is painted in.
#[derive(Debug, Clone, Copy, Default)]
pub struct PointerInput {
    pub x: f32,
    pub y: f32,
    /// The left button is held.
    pub down: bool,
    /// The left button went down this frame (the rising edge).
    pub pressed: bool,
    /// 1 / 2 / 3 for a single / double / triple click (read with `pressed`).
    pub click_count: u8,
    /// Shift is held (Shift+click extends the selection).
    pub shift: bool,
    /// Wheel travel in DIP, `> 0` scrolls down (the web's `deltaY` sign).
    pub wheel_dy: f32,
}

/// What a held left button is doing.
#[derive(Debug, Clone, Copy)]
enum Drag {
    /// Extending the selection by characters.
    Char,
    /// By words, from the double-clicked word `[a, b)`.
    Word(Pos, Pos),
    /// By blocks, from the triple-clicked block.
    Block(Pos, Pos),
    /// Dragging the scroll bar thumb, grabbed `grab` DIP below its top.
    Thumb(f32),
}

/// The rich text editor: `@ui/RichText`'s whole box — toolbar, optional link
/// row, and the editing area — or the editing area alone
/// ([`RichTextBox::bare`]) for a host that brings its own bar.
///
/// The replica is WinForms' `RichTextBox` (for `enabled`, `read_only` and the
/// rest of the control surface, reached through [`Deref`]). Its `runs` are a
/// flat styled string with no paragraphs, lists or links, so the content lives
/// in [`RichTextBox::doc`] — the one concept the replica cannot hold.
pub struct RichTextBox {
    inner: RichTextModel,
    /// The content and the selection.
    pub doc: Document,
    /// The formatting bar, or `None` for a bare editing area.
    pub toolbar: Option<RichTextToolbar>,
    /// `placeholder`, shown in `text_tertiary` while the document is empty.
    pub placeholder: String,
    /// `minHeight` of the editing area (the web's default is 96).
    pub min_height: f32,
    /// The field family's `error`: the border and the ring turn `danger`.
    pub invalid: bool,
    /// Vertical scroll of the editing area, in DIP.
    pub scroll_y: f32,
    /// The link row's draft URL; `Some` while the row is open.
    pub link_draft: Option<String>,
    /// The window holds the keyboard focus (the caret hides when it does not).
    pub window_active: bool,
    goal_x: Option<f32>,
    drag: Option<Drag>,
    last_input_ms: u64,
    bar_hot: bool,
    ok_hot: bool,
    layout: RefCell<Option<Layout>>,
}

impl Deref for RichTextBox {
    type Target = RichTextModel;
    fn deref(&self) -> &RichTextModel {
        &self.inner
    }
}
impl DerefMut for RichTextBox {
    fn deref_mut(&mut self) -> &mut RichTextModel {
        &mut self.inner
    }
}

impl Default for RichTextBox {
    fn default() -> Self {
        Self::new()
    }
}

/// `colour` at `DISABLED_ALPHA` of its alpha — `opacity-60`.
fn faded(colour: &D2D1_COLOR_F) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: colour.a * m::DISABLED_ALPHA, ..*colour }
}

impl RichTextBox {
    /// The web component: the standard toolbar over an empty editing area.
    pub fn new() -> Self {
        let mut bar = RichTextToolbar::standard();
        // The box draws the frame; the bar sits inside it.
        bar.framed = false;
        Self::with_toolbar(Some(bar))
    }

    /// The editing area alone.
    pub fn bare() -> Self {
        Self::with_toolbar(None)
    }

    fn with_toolbar(toolbar: Option<RichTextToolbar>) -> Self {
        Self {
            inner: RichTextModel::new(),
            doc: Document::new(),
            toolbar,
            placeholder: String::new(),
            min_height: m::MIN_HEIGHT,
            invalid: false,
            scroll_y: 0.0,
            link_draft: None,
            window_active: true,
            goal_x: None,
            drag: None,
            last_input_ms: 0,
            bar_hot: false,
            ok_hot: false,
            layout: RefCell::new(None),
        }
    }

    /// Builder: the content.
    pub fn with_document(mut self, doc: Document) -> Self {
        self.doc = doc;
        self
    }

    /// Builder: the placeholder.
    pub fn with_placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// `contenteditable` and not disabled.
    pub fn editable(&self) -> bool {
        self.inner.enabled && !self.inner.read_only
    }

    /// `host::now_ms()` at the last edit or caret move — the caret's blink
    /// phase restarts there.
    pub fn last_input_ms(&self) -> u64 {
        self.last_input_ms
    }

    /// Restarts the caret's blink phase (a host calls it when the box gains
    /// focus, so the caret shows at once).
    pub fn touch(&mut self) {
        self.last_input_ms = host::now_ms();
    }

    // ── The API a toolbar drives ─────────────────────────────────────────────

    /// Toggles an inline mark on the selection (or the typing style).
    pub fn toggle_mark(&mut self, mark: Mark) {
        if self.editable() {
            self.doc.toggle_mark(mark);
            self.touch();
        }
    }

    /// Sets the block kind of every block the selection touches.
    pub fn set_block(&mut self, kind: BlockKind) {
        if self.editable() {
            self.doc.set_block(kind);
            self.touch();
        }
    }

    /// The marks the toolbar should light.
    pub fn active_marks(&self) -> Marks {
        self.doc.active_marks()
    }

    /// The block kind under the caret.
    pub fn active_block(&self) -> BlockKind {
        self.doc.active_block()
    }

    /// Runs one toolbar command, as `RichText.tsx`'s buttons do through
    /// `execCommand`. Returns whether it did anything (the alignment commands
    /// have no model here and answer `false`).
    pub fn apply_command(&mut self, cmd: RichTextCommand) -> bool {
        if !self.editable() {
            return false;
        }
        match cmd {
            RichTextCommand::Bold => self.doc.toggle_mark(Mark::Bold),
            RichTextCommand::Italic => self.doc.toggle_mark(Mark::Italic),
            RichTextCommand::Underline => self.doc.toggle_mark(Mark::Underline),
            RichTextCommand::OrderedList => self.doc.toggle_block(BlockKind::NumberedItem),
            RichTextCommand::BulletList => self.doc.toggle_block(BlockKind::BulletItem),
            RichTextCommand::ClearFormat => self.doc.clear_formatting(),
            RichTextCommand::Link => {
                // `setLinkOpen(o => !o)`; the selection is kept by the model,
                // so the web's saveSel / restoreSel pair has nothing to do.
                self.link_draft = match self.link_draft {
                    Some(_) => None,
                    None => Some(self.doc.link_at_caret().unwrap_or_default()),
                };
            }
            _ => return false,
        }
        self.touch();
        true
    }

    /// `applyLink`: the draft becomes the selection's link — `https://` is
    /// prefixed when no scheme was typed — and the row closes. An empty draft
    /// removes the link.
    pub fn apply_link(&mut self) {
        let Some(draft) = self.link_draft.take() else { return };
        self.doc.set_link(normalize_url(&draft).as_deref());
        self.touch();
    }

    /// Lights the toolbar from the caret: each command's `checked` follows
    /// [`RichTextBox::active_marks`] / [`RichTextBox::active_block`].
    pub fn sync_toolbar(&mut self) {
        let marks = self.doc.active_marks();
        let block = self.doc.active_block();
        let link_open = self.link_draft.is_some();
        if let Some(bar) = &mut self.toolbar {
            bar.set_active(RichTextCommand::Bold, marks.bold);
            bar.set_active(RichTextCommand::Italic, marks.italic);
            bar.set_active(RichTextCommand::Underline, marks.underline);
            bar.set_active(RichTextCommand::OrderedList, block == BlockKind::NumberedItem);
            bar.set_active(RichTextCommand::BulletList, block == BlockKind::BulletItem);
            bar.set_active(RichTextCommand::Link, link_open || marks.link.is_some());
        }
    }

    // ── Geometry ─────────────────────────────────────────────────────────────

    /// Inside the 1 DIP border.
    fn inner_rect(bounds: Rect) -> Rect {
        let b = rich_metrics::RULE_UNDER;
        Rect::new(bounds.left + b, bounds.top + b, bounds.right - b, bounds.bottom - b)
    }

    /// The corner radius of [`Self::inner_rect`]: the border's, less its width.
    fn inner_radius() -> f32 {
        (radius::SM - rich_metrics::RULE_UNDER).max(0.0)
    }

    /// The toolbar row (its `border-b` included), if there is a toolbar.
    pub fn toolbar_rect(&self, bounds: Rect) -> Option<Rect> {
        self.toolbar.as_ref()?;
        let r = Self::inner_rect(bounds);
        Some(Rect::new(r.left, r.top, r.right, (r.top + rich_metrics::HEIGHT).min(r.bottom)))
    }

    fn link_bar_height(&self) -> f32 {
        if self.link_draft.is_some() {
            height::BUTTON_MD + 2.0 * m::LINK_PAD_Y + rich_metrics::RULE_UNDER
        } else {
            0.0
        }
    }

    /// The link row (`kb-richtext-linkbar`, its `border-b` included), while open.
    pub fn link_bar_rect(&self, bounds: Rect) -> Option<Rect> {
        self.link_draft.as_ref()?;
        let r = Self::inner_rect(bounds);
        let top = self.toolbar_rect(bounds).map_or(r.top, |t| t.bottom);
        Some(Rect::new(r.left, top, r.right, (top + self.link_bar_height()).min(r.bottom)))
    }

    fn ok_width(c: &dyn Canvas) -> f32 {
        c.measure("OK", &c.formats().body_strong) + 2.0 * m::LINK_OK_PAD_X
    }

    /// The link row's `@ui/Input` and its « OK » button.
    fn link_parts(&self, c: &dyn Canvas, bounds: Rect) -> Option<(Rect, Rect)> {
        let row = self.link_bar_rect(bounds)?;
        let top = row.top + m::LINK_PAD_Y;
        let bottom = top + height::BUTTON_MD;
        let ok_right = row.right - m::LINK_PAD_X;
        let ok = Rect::new(ok_right - Self::ok_width(c), top, ok_right, bottom);
        let input = Rect::new(row.left + m::LINK_PAD_X, top, (ok.left - m::LINK_GAP).max(row.left), bottom);
        Some((input, ok))
    }

    /// The scrolling editing area, under the bars.
    pub fn viewport(&self, bounds: Rect) -> Rect {
        let r = Self::inner_rect(bounds);
        let top = self
            .link_bar_rect(bounds)
            .or_else(|| self.toolbar_rect(bounds))
            .map_or(r.top, |b| b.bottom)
            .min(r.bottom);
        Rect::new(r.left, top, r.right, r.bottom)
    }

    fn wrap_width(vp: Rect) -> f32 {
        (vp.right - vp.left - 2.0 * m::PAD_X).max(1.0)
    }

    /// The layout origin (content top-left, after padding and scroll).
    fn origin(&self, vp: Rect) -> (f32, f32) {
        (vp.left + m::PAD_X, vp.top + m::PAD_Y - self.scroll_y)
    }

    fn with_layout<R>(&self, c: &dyn Canvas, vp: Rect, f: impl FnOnce(&Layout) -> R) -> R {
        let width = Self::wrap_width(vp);
        let mut cache = self.layout.borrow_mut();
        let fresh = cache
            .as_ref()
            .is_some_and(|l| l.revision == self.doc.revision && (l.width - width).abs() <= 0.01);
        if !fresh {
            *cache = Some(Layout::build(c, &self.doc, width));
        }
        match cache.as_ref() {
            Some(l) => f(l),
            None => f(&Layout::build(c, &self.doc, width)),
        }
    }

    /// The content's full height, padding included.
    fn content_height(&self, c: &dyn Canvas, vp: Rect) -> f32 {
        self.with_layout(c, vp, |l| l.height) + 2.0 * m::PAD_Y
    }

    fn max_scroll(&self, c: &dyn Canvas, vp: Rect) -> f32 {
        (self.content_height(c, vp) - (vp.bottom - vp.top)).max(0.0)
    }

    /// The height the whole box needs at `width` to show everything without
    /// scrolling — the web box grows with its content (`minHeight`, no max).
    pub fn height_for_width(&self, c: &dyn Canvas, width: f32) -> f32 {
        let bars = self.toolbar.as_ref().map_or(0.0, |_| rich_metrics::HEIGHT) + self.link_bar_height();
        let border = 2.0 * rich_metrics::RULE_UNDER;
        let vp = Rect::new(0.0, 0.0, (width - border).max(1.0), 1.0);
        let content = Layout::build(c, &self.doc, Self::wrap_width(vp)).height + 2.0 * m::PAD_Y;
        bars + border + content.max(self.min_height)
    }

    /// The caret's rectangle on screen (for a host that anchors a popup to it),
    /// `None` when it is scrolled out of the editing area.
    pub fn caret_rect(&self, c: &dyn Canvas, bounds: Rect) -> Option<Rect> {
        let vp = self.viewport(bounds);
        let (ox, oy) = self.origin(vp);
        let b = self.with_layout(c, vp, |l| l.caret_box(c, self.doc.sel.head));
        let r = Rect::new(ox + b.left, oy + b.top, ox + b.right, oy + b.bottom);
        (r.bottom > vp.top && r.top < vp.bottom).then_some(r)
    }

    /// The document position under `(x, y)` (clamped into the text).
    pub fn pos_at(&self, c: &dyn Canvas, bounds: Rect, x: f32, y: f32) -> Pos {
        let vp = self.viewport(bounds);
        let (ox, oy) = self.origin(vp);
        self.with_layout(c, vp, |l| l.hit(c, x - ox, y - oy))
    }

    /// The href of the link under `(x, y)`, if any (a host may open it on
    /// Ctrl+click).
    pub fn link_at(&self, c: &dyn Canvas, bounds: Rect, x: f32, y: f32) -> Option<String> {
        let vp = self.viewport(bounds);
        if !vp.contains(x, y) {
            return None;
        }
        let (ox, oy) = self.origin(vp);
        self.with_layout(c, vp, |l| {
            let line = l.lines.iter().find(|ln| y - oy >= ln.top && y - oy < ln.top + ln.h)?;
            let f = line.frags.iter().find(|f| x - ox >= f.x && x - ox < f.x + f.w)?;
            f.marks.link.clone()
        })
    }

    /// The toolbar command under `(x, y)`.
    pub fn command_at(&self, bounds: Rect, x: f32, y: f32) -> Option<RichTextCommand> {
        let bar = self.toolbar.as_ref()?;
        let r = self.toolbar_rect(bounds)?;
        let i = bar.item_at(r, x, y)?;
        command_of(bar, i)
    }

    /// The toolbar cell `cmd` is drawn in (to anchor its tooltip).
    pub fn command_rect(&self, bounds: Rect, cmd: RichTextCommand) -> Option<Rect> {
        let bar = self.toolbar.as_ref()?;
        let r = self.toolbar_rect(bounds)?;
        (0..bar.items.len()).find(|&i| command_of(bar, i) == Some(cmd)).and_then(|i| bar.item_rect(r, i))
    }

    /// The pointer shape over `(x, y)`: an I-beam over the text and the link
    /// input, a hand over a toolbar button or « OK ».
    pub fn cursor_at(&self, c: &dyn Canvas, bounds: Rect, x: f32, y: f32) -> Option<Cursor> {
        if !bounds.contains(x, y) {
            return None;
        }
        if let Some((input, ok)) = self.link_parts(c, bounds) {
            if ok.contains(x, y) {
                return Some(Cursor::Hand);
            }
            if input.contains(x, y) {
                return Some(Cursor::IBeam);
            }
        }
        if self.command_at(bounds, x, y).is_some() {
            return Some(if self.editable() { Cursor::Hand } else { Cursor::NotAllowed });
        }
        let vp = self.viewport(bounds);
        if vp.contains(x, y) && !self.bar_hot {
            return Some(if self.inner.enabled { Cursor::IBeam } else { Cursor::NotAllowed });
        }
        None
    }

    /// Scrolls so the caret's line is inside the editing area.
    pub fn scroll_to_caret(&mut self, c: &dyn Canvas, bounds: Rect) {
        let vp = self.viewport(bounds);
        let b = self.with_layout(c, vp, |l| l.caret_box(c, self.doc.sel.head));
        let vh = vp.bottom - vp.top;
        // Content coordinates include the top padding; keep the padding in
        // view at both ends, as a browser scrolls a padded box.
        let top = b.top;
        let bottom = b.bottom + 2.0 * m::PAD_Y;
        if top < self.scroll_y {
            self.scroll_y = top;
        } else if bottom > self.scroll_y + vh {
            self.scroll_y = bottom - vh;
        }
        self.clamp_scroll(c, bounds);
    }

    fn clamp_scroll(&mut self, c: &dyn Canvas, bounds: Rect) {
        let vp = self.viewport(bounds);
        let max = self.max_scroll(c, vp);
        self.scroll_y = self.scroll_y.clamp(0.0, max);
    }

    /// The vertical bar, when the content overflows, and its rail — kept
    /// inside the box's `rounded-md` border, clear of the rounded corners.
    fn scroll_bar(&self, c: &dyn Canvas, bounds: Rect) -> Option<(ScrollBar, Rect)> {
        let vp = self.viewport(bounds);
        let content = self.content_height(c, vp);
        let mut bar = ScrollBar::from_content(false, content, vp.bottom - vp.top, self.scroll_y)?;
        bar.expanded = self.bar_hot || matches!(self.drag, Some(Drag::Thumb(_)));
        let rail = crate::range::fit_rail(bar.rail(&vp), Self::inner_rect(bounds), Self::inner_radius());
        Some((bar, rail))
    }

    /// Whether the content overflows, so the wheel scrolls the box.
    pub fn can_scroll(&self, c: &dyn Canvas, bounds: Rect) -> bool {
        self.max_scroll(c, self.viewport(bounds)) > 0.0
    }

    // ── Pointer ──────────────────────────────────────────────────────────────

    /// Feeds one frame of pointer input. Returns whether anything changed
    /// (selection, content, scroll or hover).
    pub fn handle_pointer(&mut self, c: &dyn Canvas, bounds: Rect, p: PointerInput) -> bool {
        let vp = self.viewport(bounds);
        let mut changed = false;

        // Hover: the toolbar's hot cell, « OK », the scroll bar.
        let tb = self.toolbar_rect(bounds);
        let editable = self.editable();
        if let (Some(bar), Some(r)) = (self.toolbar.as_mut(), tb) {
            let hot = if editable { bar.item_at(r, p.x, p.y) } else { None };
            if bar.hot_index != hot {
                bar.hot_index = hot;
                changed = true;
            }
        }
        let parts = self.link_parts(c, bounds);
        self.ok_hot = parts.is_some_and(|(_, ok)| ok.contains(p.x, p.y));
        let dragging_thumb = matches!(self.drag, Some(Drag::Thumb(_)));
        let bar_hot = self.scroll_bar(c, bounds).is_some_and(|(_, rail)| rail.contains(p.x, p.y)) || dragging_thumb;
        if bar_hot != self.bar_hot {
            self.bar_hot = bar_hot;
            changed = true;
        }

        if p.pressed && bounds.contains(p.x, p.y) {
            if let Some(cmd) = self.command_at(bounds, p.x, p.y) {
                return self.apply_command(cmd) || changed;
            }
            if let Some((_, ok)) = parts {
                if ok.contains(p.x, p.y) {
                    self.apply_link();
                    return true;
                }
                if self.link_bar_rect(bounds).is_some_and(|r| r.contains(p.x, p.y)) {
                    return changed;
                }
            }
            if let Some((sb, rail)) = self.scroll_bar(c, bounds) {
                if rail.contains(p.x, p.y) {
                    let vh = vp.bottom - vp.top;
                    match sb.part_at(rail, p.x, p.y) {
                        Some(ScrollPart::Thumb) => {
                            let thumb = sb.thumb_rect(rail);
                            self.drag = Some(Drag::Thumb(p.y - thumb.top));
                        }
                        Some(ScrollPart::PageLow) => self.scroll_y -= vh,
                        Some(ScrollPart::PageHigh) => self.scroll_y += vh,
                        Some(ScrollPart::ArrowLow) => self.scroll_y -= m::LINE_BODY,
                        Some(ScrollPart::ArrowHigh) => self.scroll_y += m::LINE_BODY,
                        _ => {}
                    }
                    self.clamp_scroll(c, bounds);
                    return true;
                }
            }
            if vp.contains(p.x, p.y) {
                let pos = self.pos_at(c, bounds, p.x, p.y);
                match p.click_count {
                    0 | 1 => {
                        self.doc.move_to(pos, p.shift);
                        self.drag = Some(Drag::Char);
                    }
                    2 => {
                        let (a, b) = self.doc.word_range(pos);
                        self.doc.set_selection(a, b);
                        self.drag = Some(Drag::Word(a, b));
                    }
                    _ => {
                        let (a, b) = self.doc.block_range(pos.block);
                        self.doc.set_selection(a, b);
                        self.drag = Some(Drag::Block(a, b));
                    }
                }
                self.goal_x = None;
                self.touch();
                return true;
            }
        }

        if p.down {
            match self.drag {
                Some(Drag::Thumb(grab)) => {
                    if let Some((sb, rail)) = self.scroll_bar(c, bounds) {
                        let thumb = sb.thumb_rect(rail);
                        let len = thumb.bottom - thumb.top;
                        let inset = if sb.expanded { crate::metrics::control::SCROLLBAR_ARROW } else { 0.0 };
                        let travel = (rail.bottom - rail.top - 2.0 * inset - len).max(1.0);
                        let frac = ((p.y - grab - rail.top - inset) / travel).clamp(0.0, 1.0);
                        self.scroll_y = frac * self.max_scroll(c, vp);
                        changed = true;
                    }
                }
                Some(drag) => {
                    // Past the top / bottom edge the view follows the pointer.
                    let over = if p.y < vp.top {
                        p.y - vp.top
                    } else if p.y > vp.bottom {
                        p.y - vp.bottom
                    } else {
                        0.0
                    };
                    if over != 0.0 {
                        self.scroll_y += over * m::AUTOSCROLL_RATE;
                        self.clamp_scroll(c, bounds);
                        host::request_repaint_after(m::AUTOSCROLL_MS);
                    }
                    let y = p.y.clamp(vp.top, (vp.bottom - 1.0).max(vp.top));
                    let pos = self.pos_at(c, bounds, p.x, y);
                    let sel = self.doc.sel;
                    match drag {
                        Drag::Char => self.doc.set_selection(sel.anchor, pos),
                        Drag::Word(a, b) => {
                            let (wa, wb) = self.doc.word_range(pos);
                            if pos < a {
                                self.doc.set_selection(b, wa);
                            } else {
                                self.doc.set_selection(a, wb.max(b));
                            }
                        }
                        Drag::Block(a, b) => {
                            let (ba, bb) = self.doc.block_range(pos.block);
                            if pos < a {
                                self.doc.set_selection(b, ba);
                            } else {
                                self.doc.set_selection(a, bb.max(b));
                            }
                        }
                        Drag::Thumb(_) => {}
                    }
                    if self.doc.sel != sel {
                        self.touch();
                        changed = true;
                    }
                }
                None => {}
            }
        } else {
            self.drag = None;
        }

        if p.wheel_dy != 0.0 && bounds.contains(p.x, p.y) {
            let before = self.scroll_y;
            self.scroll_y += p.wheel_dy;
            self.clamp_scroll(c, bounds);
            changed |= self.scroll_y != before;
        }
        changed
    }

    // ── Keyboard ─────────────────────────────────────────────────────────────

    /// Whether this editor, focused, takes `e` — the keys a contenteditable
    /// handles. Tab is not among them (the focus ring owns it), nor Escape
    /// unless the link row is open.
    pub fn wants_event(&self, e: &InputEvent) -> bool {
        let editable = self.editable();
        match e {
            InputEvent::Text(_) => editable,
            InputEvent::Key { vk: key, down: true, mods, .. } => {
                let key = *key;
                if mods.alt {
                    return false;
                }
                if self.link_draft.is_some() {
                    if matches!(key, vk::BACK | vk::ENTER | vk::ESCAPE) {
                        return true;
                    }
                    if mods.ctrl && key == vk::letter('V') {
                        return true;
                    }
                }
                match key {
                    vk::LEFT | vk::RIGHT | vk::UP | vk::DOWN | vk::HOME | vk::END | vk::PAGE_UP | vk::PAGE_DOWN => true,
                    vk::BACK | vk::DELETE | vk::ENTER => editable,
                    _ if mods.ctrl => {
                        let letter = |ch: char| key == vk::letter(ch);
                        let edit_chord = ['X', 'V', 'Z', 'Y', 'B', 'I', 'U', 'K'].into_iter().any(letter)
                            || (mods.shift && (key == vk::digit(7) || key == vk::digit(8)));
                        letter('A') || letter('C') || (editable && edit_chord)
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// Consumes, from the host's queue, every event this focused editor takes,
    /// and applies them. Returns whether anything changed.
    pub fn handle_host_input(&mut self, c: &dyn Canvas, bounds: Rect) -> bool {
        let events = host::consume(|e| self.wants_event(e));
        let mut changed = false;
        for e in &events {
            changed |= self.handle_event(c, bounds, e);
        }
        changed
    }

    /// The link row's share of the keyboard while it is open (`autoFocus` on
    /// its input). Returns `None` when the event is not the row's.
    fn link_row_event(&mut self, e: &InputEvent) -> Option<bool> {
        let draft = self.link_draft.as_mut()?;
        match e {
            InputEvent::Text(s) => draft.push_str(s),
            InputEvent::Key { vk: vk::BACK, .. } => {
                draft.pop();
            }
            InputEvent::Key { vk: vk::ENTER, .. } => self.apply_link(),
            InputEvent::Key { vk: vk::ESCAPE, .. } => self.link_draft = None,
            InputEvent::Key { vk: key, mods, .. } if mods.ctrl && *key == vk::letter('V') => {
                if let Some(t) = host::clipboard_text() {
                    draft.push_str(t.lines().next().unwrap_or("").trim());
                }
            }
            _ => return None,
        }
        self.touch();
        Some(true)
    }

    /// Applies one event (see [`RichTextBox::wants_event`]). Returns whether
    /// anything changed.
    pub fn handle_event(&mut self, c: &dyn Canvas, bounds: Rect, e: &InputEvent) -> bool {
        if !self.wants_event(e) {
            return false;
        }
        if let Some(done) = self.link_row_event(e) {
            self.scroll_to_caret(c, bounds);
            return done;
        }
        let (key, mods) = match e {
            InputEvent::Text(s) => {
                self.doc.insert_text(s);
                self.goal_x = None;
                self.touch();
                self.scroll_to_caret(c, bounds);
                return true;
            }
            InputEvent::Key { vk: key, mods, .. } => (*key, *mods),
            _ => return false,
        };
        let extend = mods.shift;
        let mut keep_goal = false;
        let mut paged = false;
        match key {
            vk::LEFT => self.doc.move_horizontal(false, mods.ctrl, extend),
            vk::RIGHT => self.doc.move_horizontal(true, mods.ctrl, extend),
            vk::UP | vk::DOWN => {
                self.move_vertical(c, bounds, key == vk::DOWN, 1, extend);
                keep_goal = true;
            }
            vk::PAGE_UP | vk::PAGE_DOWN => {
                let vp = self.viewport(bounds);
                let vh = vp.bottom - vp.top;
                let lines = ((vh / m::LINE_BODY).floor() as usize).max(1);
                let down = key == vk::PAGE_DOWN;
                self.move_vertical(c, bounds, down, lines, extend);
                self.scroll_y += if down { vh } else { -vh };
                keep_goal = true;
                paged = true;
            }
            vk::HOME | vk::END => {
                let target = if mods.ctrl {
                    if key == vk::HOME {
                        self.doc.start()
                    } else {
                        self.doc.end()
                    }
                } else {
                    let vp = self.viewport(bounds);
                    let head = self.doc.sel.head;
                    self.with_layout(c, vp, |l| {
                        let li = l.line_of(head);
                        let off = if key == vk::HOME { l.lines[li].start } else { l.visual_end(li) };
                        Pos::new(head.block, off)
                    })
                };
                self.doc.move_to(target, extend);
            }
            vk::BACK => self.doc.delete_backward(mods.ctrl),
            vk::DELETE => self.doc.delete_forward(mods.ctrl),
            vk::ENTER if mods.shift => self.doc.insert_line_break(),
            vk::ENTER => self.doc.insert_paragraph(),
            _ if mods.ctrl => self.chord(key, mods),
            _ => return false,
        }
        if !keep_goal {
            self.goal_x = None;
        }
        self.touch();
        if paged {
            self.clamp_scroll(c, bounds);
        } else {
            self.scroll_to_caret(c, bounds);
        }
        true
    }

    /// The Ctrl chords of a contenteditable.
    fn chord(&mut self, key: u16, mods: Modifiers) {
        let is = |ch: char| key == vk::letter(ch);
        if is('A') {
            self.doc.select_all();
        } else if is('C') || is('X') {
            let text = self.doc.selected_text();
            if !text.is_empty() {
                // The clipboard speaks CRLF.
                host::set_clipboard_text(&text.replace('\n', "\r\n"));
                if is('X') {
                    self.doc.delete_selection();
                }
            }
        } else if is('V') {
            if let Some(t) = host::clipboard_text() {
                self.doc.insert_text(&t);
            }
        } else if is('Z') && !mods.shift {
            self.doc.undo();
        } else if is('Y') || is('Z') {
            self.doc.redo();
        } else if is('B') {
            self.doc.toggle_mark(Mark::Bold);
        } else if is('I') {
            self.doc.toggle_mark(Mark::Italic);
        } else if is('U') {
            self.doc.toggle_mark(Mark::Underline);
        } else if is('K') {
            self.apply_command(RichTextCommand::Link);
        } else if key == vk::digit(7) {
            self.doc.toggle_block(BlockKind::NumberedItem);
        } else if key == vk::digit(8) {
            self.doc.toggle_block(BlockKind::BulletItem);
        }
    }

    /// Up / Down by `n` visual lines, keeping the column the caret started in.
    fn move_vertical(&mut self, c: &dyn Canvas, bounds: Rect, down: bool, n: usize, extend: bool) {
        let vp = self.viewport(bounds);
        let head = self.doc.sel.head;
        let goal = self.goal_x;
        let (target, x) = self.with_layout(c, vp, |l| {
            let li = l.line_of(head);
            let x = goal.unwrap_or_else(|| l.x_at(c, li, head.offset));
            let target = if down {
                if li + n < l.lines.len() {
                    let t = li + n;
                    Pos::new(l.lines[t].block, l.offset_at_x(c, t, x))
                } else {
                    // Down on the last line goes to the end, as in a browser.
                    let last = l.lines.len() - 1;
                    Pos::new(l.lines[last].block, l.lines[last].end)
                }
            } else if li >= n {
                let t = li - n;
                Pos::new(l.lines[t].block, l.offset_at_x(c, t, x))
            } else {
                Pos::new(0, 0)
            };
            (target, x)
        });
        self.doc.move_to(target, extend);
        self.goal_x = Some(x);
    }

    // ── Painting ─────────────────────────────────────────────────────────────

    fn paint_content(&self, c: &dyn Canvas, vp: Rect, state: WidgetState, disabled: bool) {
        let t = c.theme();
        let fm = c.formats();
        let (ox, oy) = self.origin(vp);
        let sel = self.doc.sel;
        let (a, b) = sel.range();
        let ink = if disabled { faded(&t.text_primary) } else { t.text_primary };
        let link_ink = if disabled { faded(&t.accent) } else { t.accent };
        let mut band = t.accent;
        band.a = m::SELECTION_ALPHA;

        self.with_layout(c, vp, |l| {
            for (li, line) in l.lines.iter().enumerate() {
                let top = oy + line.top;
                let bottom = top + line.h;
                if bottom < vp.top || top > vp.bottom {
                    continue;
                }
                let text = &l.texts[line.block];

                // The selection band.
                if a != b && line.block >= a.block && line.block <= b.block {
                    let lo = if line.block == a.block { a.offset.max(line.start) } else { line.start };
                    let hi = if line.block == b.block { b.offset.min(line.end) } else { line.end };
                    let starts_here = line.block != a.block || a.offset <= line.end;
                    let ends_here = line.block != b.block || b.offset >= line.start;
                    if starts_here && ends_here && lo <= hi {
                        let x0 = l.x_at(c, li, lo);
                        let mut x1 = l.x_at(c, li, hi);
                        if line.last && line.block < b.block {
                            x1 += m::NEWLINE_SELECTION;
                        }
                        if x1 > x0 {
                            c.fill_rounded(&Rect::new(ox + x0, top, ox + x1, bottom), 0.0, &band);
                        }
                    }
                }

                // The list marker, outside the item's text.
                if line.first {
                    let marker = match self.doc.blocks[line.block].kind {
                        BlockKind::BulletItem => Some("•".to_string()),
                        BlockKind::NumberedItem => self.doc.list_number(line.block).map(|n| format!("{n}.")),
                        _ => None,
                    };
                    if let Some(mk) = marker {
                        let w = c.measure(&mk, &fm.body);
                        let right = ox + line.x0 - m::MARKER_GAP;
                        c.text(&mk, &Rect::new(right - w, top, right + 1.0, bottom), &fm.body, &ink, false);
                    }
                }

                // The runs.
                let nfrags = line.frags.len();
                for (fi, f) in line.frags.iter().enumerate() {
                    let fmt = format_for(c, f.key);
                    let size = f.key.size_centi as f32 / 100.0;
                    let piece = text[f.start..f.end].trim_end_matches('\n');
                    let x = ox + f.x;
                    // A wrapped line's trailing spaces hang: no decoration
                    // under them.
                    let deco_w = if fi + 1 == nfrags && !line.last && piece.ends_with(char::is_whitespace) {
                        c.measure(piece.trim_end(), &fmt)
                    } else {
                        f.w
                    };
                    let cy = (top + bottom) / 2.0;
                    if f.marks.code {
                        let half = size * 0.7;
                        c.fill_rounded(
                            &Rect::new(x - 1.0, cy - half, x + deco_w + 1.0, cy + half),
                            m::CODE_RADIUS,
                            &t.surface_2,
                        );
                    }
                    let colour = if f.marks.link.is_some() { link_ink } else { ink };
                    c.text(piece, &Rect::new(x, top, x + f.w + 1.0, bottom), &fmt, &colour, false);
                    if f.marks.underline || f.marks.link.is_some() {
                        let y = cy + size * m::UNDERLINE_EM;
                        c.fill_rounded(&Rect::new(x, y, x + deco_w, y + m::DECORATION), 0.0, &colour);
                    }
                    if f.marks.strike {
                        let y = cy + size * m::STRIKE_EM;
                        c.fill_rounded(&Rect::new(x, y, x + deco_w, y + m::DECORATION), 0.0, &colour);
                    }
                }
            }

            // `{empty && placeholder && <div className="absolute top-2 left-3
            // text-sm text-text-tertiary">}`.
            if self.doc.is_empty() && !self.placeholder.is_empty() {
                let r = Rect::new(ox, oy, vp.right - m::PAD_X, oy + m::LINE_BODY);
                c.text_ellipsis(&self.placeholder, &r, &fm.body, &t.text_tertiary);
            }

            // The caret.
            let show = state.focused
                && !disabled
                && self.editable()
                && self.window_active
                && self.link_draft.is_none()
                && crate::focus::caret_visible(self.last_input_ms);
            if show {
                let bx = l.caret_box(c, sel.head);
                let r = Rect::new(ox + bx.left, oy + bx.top, ox + bx.right, oy + bx.bottom);
                c.fill_rounded(&r, 0.0, &t.text_primary);
            }
        });
    }

    fn paint_link_bar(&self, c: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let (Some(row), Some((input, ok)), Some(draft)) =
            (self.link_bar_rect(bounds), self.link_parts(c, bounds), self.link_draft.as_ref())
        else {
            return;
        };
        let t = c.theme();
        let fm = c.formats();
        // `bg-surface-1 border-b border-border`.
        c.fill_rounded(&row, 0.0, &t.card_background);
        c.fill_rounded(
            &Rect::new(row.left, row.bottom - rich_metrics::RULE_UNDER, row.right, row.bottom),
            0.0,
            &t.divider,
        );
        // `@ui/Input autoFocus`: the field holds the focus while the row is
        // open, so it wears `focus:ring-2 focus:ring-primary`.
        c.fill_rounded(&input, radius::SM, &t.layer_background);
        c.stroke_rounded(&input, radius::SM, &t.card_stroke);
        if state.focused {
            c.stroke_rounded_w(&input, radius::SM, &t.accent, m::INPUT_RING);
        }
        let text_r = Rect::new(input.left + space::MD, input.top, input.right - space::MD, input.bottom);
        c.push_clip(&text_r);
        if draft.is_empty() {
            c.text("https://…", &text_r, &fm.body, &t.text_tertiary, false);
        }
        // The field scrolls to keep its caret visible.
        let w = c.measure(draft, &fm.body);
        let avail = text_r.right - text_r.left;
        let x = text_r.left - (w - avail + m::CARET_W).max(0.0);
        c.text(draft, &Rect::new(x, input.top, x + w + 1.0, input.bottom), &fm.body, &t.text_primary, false);
        if state.focused && self.window_active && crate::focus::caret_visible(self.last_input_ms) {
            let cx = x + w;
            let half = m::LINE_BODY / 2.0;
            let cy = (input.top + input.bottom) / 2.0;
            c.fill_rounded(&Rect::new(cx, cy - half, cx + m::CARET_W, cy + half), 0.0, &t.text_primary);
        }
        c.pop_clip();
        // `text-sm font-medium text-primary px-2`.
        let ok_ink = if self.ok_hot { t.accent_hover } else { t.accent };
        c.text("OK", &ok, &fm.body_strong, &ok_ink, true);
    }
}

/// `/^https?:\/\//i.test(url) ? url : `https://${url}``, after `trim()`;
/// `None` for an empty draft.
pub fn normalize_url(draft: &str) -> Option<String> {
    let url = draft.trim();
    if url.is_empty() {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    Some(if lower.starts_with("http://") || lower.starts_with("https://") {
        url.to_string()
    } else {
        format!("https://{url}")
    })
}

/// Maps toolbar cell `i` back to its command (the cell's image is the
/// command's icon name).
fn command_of(bar: &RichTextToolbar, i: usize) -> Option<RichTextCommand> {
    use RichTextCommand::*;
    let StripItem::Button(b) = bar.items.get(i)? else { return None };
    let icon = b.item.image.as_deref()?;
    [Bold, Italic, Underline, OrderedList, BulletList, Link, ClearFormat, AlignLeft, AlignCenter, AlignRight, AlignJustify]
        .into_iter()
        .find(|c| c.icon() == icon)
}

impl Widget for RichTextBox {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    /// The toolbar's width by the bars plus `minHeight`; use
    /// [`RichTextBox::height_for_width`] for the height the content needs at a
    /// given width.
    fn measure(&self, _canvas: &dyn Canvas) -> Size {
        let border = 2.0 * rich_metrics::RULE_UNDER;
        let w = self.toolbar.as_ref().map_or(0.0, |b| b.content_width()) + border;
        let bars = self.toolbar.as_ref().map_or(0.0, |_| rich_metrics::HEIGHT) + self.link_bar_height();
        Size::new(w, bars + self.min_height + border)
    }

    fn paint(&self, c: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = c.theme();
        let disabled = state.disabled || !self.inner.enabled;
        // Rule: every widget lands on an opaque background.
        c.fill_rounded(&bounds, 0.0, &c.current_bg());
        // `bg-white`, or the field family's `disabled:bg-surface-2`.
        let ground = if disabled { t.surface_2 } else { t.layer_background };
        c.fill_rounded(&bounds, radius::SM, &ground);

        // `overflow-hidden` on a `rounded-md` box.
        c.push_clip_rounded(&bounds, radius::SM);
        c.push_bg(ground);
        if let (Some(bar), Some(r)) = (self.toolbar.as_ref(), self.toolbar_rect(bounds)) {
            bar.paint(c, r, WidgetState::REST.disabled(disabled || self.inner.read_only));
            // `border-b border-border` under the row.
            c.fill_rounded(
                &Rect::new(r.left, r.bottom - rich_metrics::RULE_UNDER, r.right, r.bottom),
                0.0,
                &t.divider,
            );
        }
        self.paint_link_bar(c, bounds, state);

        let vp = self.viewport(bounds);
        c.push_clip(&vp);
        self.paint_content(c, vp, state, disabled);
        c.pop_clip();
        if let Some((bar, rail)) = self.scroll_bar(c, bounds) {
            crate::range::paint_bar_in(
                c,
                &bar,
                rail,
                WidgetState::REST.hot(self.bar_hot),
                Self::inner_rect(bounds),
                Self::inner_radius(),
            );
        }
        c.pop_bg();
        c.pop_clip_rounded();

        // `border border-border` (`border-danger` when invalid), then the
        // focus outline over it.
        let line = if self.invalid { t.danger } else { t.card_stroke };
        let line = if disabled { faded(&line) } else { line };
        c.stroke_rounded(&bounds, radius::SM, &line);
        if state.show_focus_ring() && !disabled {
            let ring = if self.invalid { &t.danger } else { &t.accent };
            c.stroke_rounded_w(&bounds, radius::SM, ring, m::FOCUS_OUTLINE);
        }
    }

    fn type_name(&self) -> &'static str {
        "RichTextBox"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(block: usize, offset: usize) -> Pos {
        Pos::new(block, offset)
    }

    fn doc(text: &str) -> Document {
        Document::from_plain(text)
    }

    fn type_str(d: &mut Document, s: &str) {
        for ch in s.chars() {
            d.insert_text(&ch.to_string());
        }
    }

    // ── Blocks and spans ─────────────────────────────────────────────────────

    #[test]
    fn empty_document_is_one_empty_paragraph() {
        let d = Document::new();
        assert!(d.is_empty());
        assert_eq!(d.blocks().len(), 1);
        assert_eq!(d.to_html(), "");
        assert_eq!(d.selection(), Selection::caret(p(0, 0)));
    }

    #[test]
    fn spans_normalize_merge_and_drop_empties() {
        let b = Block::new(BlockKind::Paragraph).plain("ab").plain("").plain("cd").with("e", Marks::NONE.with(Mark::Bold));
        assert_eq!(b.spans.len(), 2);
        assert_eq!(b.spans[0].text, "abcd");
        assert_eq!(b.text(), "abcde");
    }

    #[test]
    fn marks_at_inherits_from_the_char_before() {
        let bold = Marks::NONE.with(Mark::Bold);
        let b = Block::new(BlockKind::Paragraph).plain("ab").with("cd", bold.clone());
        assert_eq!(b.marks_at(0), Marks::NONE);
        assert_eq!(b.marks_at(2), Marks::NONE);
        assert_eq!(b.marks_at(3), bold);
        assert_eq!(b.marks_at(4), bold);
        // At the start of a block, the first character's marks.
        let b = Block::new(BlockKind::Paragraph).with("x", bold.clone());
        assert_eq!(b.marks_at(0), bold);
    }

    // ── Typing ────────────────────────────────────────────────────────────────

    #[test]
    fn typing_inserts_at_the_caret() {
        let mut d = Document::new();
        type_str(&mut d, "Bonjour");
        assert_eq!(d.plain_text(), "Bonjour");
        assert_eq!(d.selection().head, p(0, 7));
        d.move_to(p(0, 3), false);
        d.insert_text("!");
        assert_eq!(d.plain_text(), "Bon!jour");
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut d = doc("hello world");
        d.set_selection(p(0, 0), p(0, 5));
        d.insert_text("bye");
        assert_eq!(d.plain_text(), "bye world");
        assert_eq!(d.selection(), Selection::caret(p(0, 3)));
    }

    #[test]
    fn typing_handles_multibyte_text() {
        let mut d = Document::new();
        d.insert_text("é😀a");
        assert_eq!(d.selection().head, p(0, "é😀a".len()));
        d.move_horizontal(false, false, false);
        d.move_horizontal(false, false, false);
        assert_eq!(d.selection().head, p(0, "é".len()));
        d.delete_forward(false);
        assert_eq!(d.plain_text(), "éa");
    }

    #[test]
    fn pasted_lines_become_blocks() {
        let mut d = doc("ab");
        d.move_to(p(0, 1), false);
        d.insert_text("1\r\n2\r\n3");
        assert_eq!(d.plain_text(), "a1\n2\n3b");
        assert_eq!(d.blocks().len(), 3);
        assert_eq!(d.selection().head, p(2, 1));
    }

    #[test]
    fn pasted_lines_inside_a_list_stay_items() {
        let mut d = Document::from_blocks(vec![Block::new(BlockKind::BulletItem).plain("x")]);
        d.move_to(p(0, 1), false);
        d.insert_text("\ny\nz");
        assert!(d.blocks().iter().all(|b| b.kind == BlockKind::BulletItem));
        assert_eq!(d.blocks().len(), 3);
    }

    // ── Enter ─────────────────────────────────────────────────────────────────

    #[test]
    fn enter_splits_the_block_and_keeps_marks() {
        let bold = Marks::NONE.with(Mark::Bold);
        let mut d = Document::from_blocks(vec![Block::new(BlockKind::Paragraph).with("abcd", bold.clone())]);
        d.move_to(p(0, 2), false);
        d.insert_paragraph();
        assert_eq!(d.plain_text(), "ab\ncd");
        assert_eq!(d.selection().head, p(1, 0));
        assert_eq!(d.blocks()[1].spans[0].marks, bold);
    }

    #[test]
    fn enter_at_the_end_of_a_heading_opens_a_paragraph() {
        let mut d = Document::from_blocks(vec![Block::new(BlockKind::Heading1).plain("Titre")]);
        d.move_to(p(0, 5), false);
        d.insert_paragraph();
        assert_eq!(d.blocks()[0].kind, BlockKind::Heading1);
        assert_eq!(d.blocks()[1].kind, BlockKind::Paragraph);
        // In the middle, both halves stay headings.
        let mut d = Document::from_blocks(vec![Block::new(BlockKind::Heading2).plain("abcd")]);
        d.move_to(p(0, 2), false);
        d.insert_paragraph();
        assert_eq!(d.blocks()[1].kind, BlockKind::Heading2);
    }

    #[test]
    fn enter_in_a_list_adds_an_item_and_an_empty_item_leaves_the_list() {
        let mut d = Document::from_blocks(vec![Block::new(BlockKind::NumberedItem).plain("one")]);
        d.move_to(p(0, 3), false);
        d.insert_paragraph();
        assert_eq!(d.blocks()[1].kind, BlockKind::NumberedItem);
        assert_eq!(d.list_number(1), Some(2));
        d.insert_paragraph();
        assert_eq!(d.blocks().len(), 2);
        assert_eq!(d.blocks()[1].kind, BlockKind::Paragraph);
    }

    #[test]
    fn shift_enter_inserts_a_line_break_inside_the_block() {
        let mut d = doc("ab");
        d.move_to(p(0, 1), false);
        d.insert_line_break();
        assert_eq!(d.blocks().len(), 1);
        assert_eq!(d.blocks()[0].text(), "a\nb");
        assert_eq!(d.selection().head, p(0, 2));
        assert_eq!(d.to_html(), "<p>a<br>b</p>");
    }

    // ── Deleting ──────────────────────────────────────────────────────────────

    #[test]
    fn backspace_deletes_a_char_then_merges_blocks() {
        let mut d = doc("ab\ncd");
        d.move_to(p(1, 1), false);
        d.delete_backward(false);
        assert_eq!(d.plain_text(), "ab\nd");
        d.delete_backward(false);
        assert_eq!(d.plain_text(), "abd");
        assert_eq!(d.selection().head, p(0, 2));
        d.move_to(p(0, 0), false);
        d.delete_backward(false);
        assert_eq!(d.plain_text(), "abd");
    }

    #[test]
    fn backspace_at_the_start_of_a_list_item_outdents_first() {
        let mut d = Document::from_blocks(vec![
            Block::new(BlockKind::Paragraph).plain("p"),
            Block::new(BlockKind::BulletItem).plain("item"),
        ]);
        d.move_to(p(1, 0), false);
        d.delete_backward(false);
        assert_eq!(d.blocks()[1].kind, BlockKind::Paragraph);
        assert_eq!(d.blocks().len(), 2);
        d.delete_backward(false);
        assert_eq!(d.plain_text(), "pitem");
    }

    #[test]
    fn delete_forward_merges_the_next_block() {
        let mut d = doc("ab\ncd");
        d.move_to(p(0, 2), false);
        d.delete_forward(false);
        assert_eq!(d.plain_text(), "abcd");
        d.move_to(d.end(), false);
        d.delete_forward(false);
        assert_eq!(d.plain_text(), "abcd");
    }

    #[test]
    fn ctrl_backspace_and_ctrl_delete_remove_words() {
        let mut d = doc("un deux trois");
        d.move_to(d.end(), false);
        d.delete_backward(true);
        assert_eq!(d.plain_text(), "un deux ");
        d.move_to(p(0, 0), false);
        d.delete_forward(true);
        assert_eq!(d.plain_text(), "deux ");
    }

    #[test]
    fn deleting_a_range_across_blocks_keeps_the_first_kind() {
        let mut d = Document::from_blocks(vec![
            Block::new(BlockKind::Heading1).plain("Title"),
            Block::new(BlockKind::Paragraph).plain("middle"),
            Block::new(BlockKind::BulletItem).plain("tail"),
        ]);
        d.set_selection(p(0, 2), p(2, 2));
        d.delete_backward(false);
        assert_eq!(d.blocks().len(), 1);
        assert_eq!(d.plain_text(), "Tiil");
        assert_eq!(d.blocks()[0].kind, BlockKind::Heading1);
        assert_eq!(d.selection(), Selection::caret(p(0, 2)));
    }

    // ── Marks ─────────────────────────────────────────────────────────────────

    #[test]
    fn toggle_mark_on_a_selection() {
        let mut d = doc("hello world");
        d.set_selection(p(0, 0), p(0, 5));
        d.toggle_mark(Mark::Bold);
        assert!(d.active_marks().bold);
        assert_eq!(d.blocks()[0].spans.len(), 2);
        assert_eq!(d.to_html(), "<p><b>hello</b> world</p>");
        // A mixed selection turns the mark ON everywhere first.
        d.set_selection(p(0, 3), p(0, 8));
        assert!(!d.active_marks().bold);
        d.toggle_mark(Mark::Bold);
        assert_eq!(d.blocks()[0].spans[0].text, "hello wo");
        // Then OFF.
        d.set_selection(p(0, 0), p(0, 8));
        d.toggle_mark(Mark::Bold);
        assert_eq!(d.blocks()[0].spans.len(), 1);
        assert!(!d.blocks()[0].spans[0].marks.bold);
    }

    #[test]
    fn toggle_mark_on_a_caret_sets_the_typing_style() {
        let mut d = doc("ab");
        d.move_to(p(0, 2), false);
        d.toggle_mark(Mark::Italic);
        assert!(d.active_marks().italic);
        d.insert_text("c");
        assert_eq!(d.blocks()[0].spans.len(), 2);
        assert!(d.blocks()[0].spans[1].marks.italic);
        // Typing continues in italic (inherited from the char before).
        d.insert_text("d");
        assert_eq!(d.blocks()[0].spans[1].text, "cd");
        // Moving the caret drops a pending style.
        d.toggle_mark(Mark::Bold);
        d.move_to(p(0, 0), false);
        assert!(!d.active_marks().bold);
    }

    #[test]
    fn marks_across_blocks_and_clear_formatting() {
        let mut d = doc("ab\ncd");
        d.select_all();
        d.toggle_mark(Mark::Underline);
        assert!(d.blocks().iter().all(|b| b.spans[0].marks.underline));
        d.toggle_mark(Mark::Strike);
        d.clear_formatting();
        assert!(d.blocks().iter().all(|b| b.spans[0].marks == Marks::NONE));
    }

    #[test]
    fn links_apply_remove_and_insert() {
        let mut d = doc("voir le site");
        d.set_selection(p(0, 8), p(0, 12));
        d.set_link(Some("https://kubuno.com"));
        assert_eq!(d.to_html(), "<p>voir le <a href=\"https://kubuno.com\">site</a></p>");
        // Caret inside the link: unlink removes the whole link.
        d.move_to(p(0, 10), false);
        assert_eq!(d.link_at_caret().as_deref(), Some("https://kubuno.com"));
        d.set_link(None);
        assert_eq!(d.to_html(), "<p>voir le site</p>");
        // Caret, no selection: the URL is inserted as a link.
        d.move_to(p(0, 0), false);
        d.set_link(Some("https://a.b"));
        assert_eq!(d.blocks()[0].spans[0].marks.link.as_deref(), Some("https://a.b"));
        assert_eq!(d.selection().head, p(0, "https://a.b".len()));
    }

    #[test]
    fn normalize_url_prefixes_a_scheme() {
        assert_eq!(normalize_url("  kubuno.com ").as_deref(), Some("https://kubuno.com"));
        assert_eq!(normalize_url("HTTP://x.y").as_deref(), Some("HTTP://x.y"));
        assert_eq!(normalize_url("   "), None);
    }

    // ── Blocks ────────────────────────────────────────────────────────────────

    #[test]
    fn toggle_block_turns_lists_on_and_off() {
        let mut d = doc("a\nb\nc");
        d.set_selection(p(0, 0), p(1, 1));
        d.toggle_block(BlockKind::NumberedItem);
        assert_eq!(d.blocks()[0].kind, BlockKind::NumberedItem);
        assert_eq!(d.blocks()[1].kind, BlockKind::NumberedItem);
        assert_eq!(d.blocks()[2].kind, BlockKind::Paragraph);
        assert_eq!(d.list_number(0), Some(1));
        assert_eq!(d.list_number(1), Some(2));
        assert_eq!(d.list_number(2), None);
        assert_eq!(d.to_html(), "<ol><li>a</li><li>b</li></ol><p>c</p>");
        d.toggle_block(BlockKind::NumberedItem);
        assert!(d.blocks().iter().all(|b| b.kind == BlockKind::Paragraph));
    }

    #[test]
    fn numbering_restarts_after_another_block() {
        let d = Document::from_blocks(vec![
            Block::new(BlockKind::NumberedItem).plain("a"),
            Block::new(BlockKind::NumberedItem).plain("b"),
            Block::new(BlockKind::Paragraph).plain("x"),
            Block::new(BlockKind::NumberedItem).plain("c"),
        ]);
        assert_eq!(d.list_number(1), Some(2));
        assert_eq!(d.list_number(3), Some(1));
    }

    #[test]
    fn html_escapes_and_nests_marks() {
        let d = Document::from_blocks(vec![
            Block::new(BlockKind::Heading2).plain("a<b>"),
            Block::new(BlockKind::BulletItem).with("x", Marks::NONE.with(Mark::Bold).with(Mark::Italic)),
            Block::new(BlockKind::Paragraph).with("c", Marks::NONE.with(Mark::Code)),
        ]);
        assert_eq!(d.to_html(), "<h2>a&lt;b&gt;</h2><ul><li><b><i>x</i></b></li></ul><p><code>c</code></p>");
    }

    // ── Selection and motion ─────────────────────────────────────────────────

    #[test]
    fn arrows_collapse_a_selection_to_its_edge_first() {
        let mut d = doc("abcdef");
        d.set_selection(p(0, 4), p(0, 1));
        d.move_horizontal(false, false, false);
        assert_eq!(d.selection(), Selection::caret(p(0, 1)));
        d.set_selection(p(0, 1), p(0, 4));
        d.move_horizontal(true, false, false);
        assert_eq!(d.selection(), Selection::caret(p(0, 4)));
    }

    #[test]
    fn shift_arrows_extend_across_blocks() {
        let mut d = doc("ab\ncd");
        d.move_to(p(0, 1), false);
        d.move_horizontal(true, false, true);
        d.move_horizontal(true, false, true);
        assert_eq!(d.selection().anchor, p(0, 1));
        assert_eq!(d.selection().head, p(1, 0));
        assert_eq!(d.selected_text(), "b\n");
    }

    #[test]
    fn ctrl_arrows_move_by_words_the_windows_way() {
        let mut d = doc("Un chat, deux.");
        d.move_to(p(0, 0), false);
        d.move_horizontal(true, true, false);
        assert_eq!(d.selection().head, p(0, 3)); // start of « chat »
        d.move_horizontal(true, true, false);
        assert_eq!(d.selection().head, p(0, 7)); // the comma
        d.move_to(d.end(), false);
        d.move_horizontal(false, true, false);
        assert_eq!(d.selection().head, p(0, 13)); // the final period
        d.move_horizontal(false, true, false);
        assert_eq!(d.selection().head, p(0, 9)); // start of « deux »
    }

    #[test]
    fn word_and_block_ranges() {
        let d = doc("hello brave world\nnext");
        assert_eq!(d.word_range(p(0, 8)), (p(0, 6), p(0, 11)));
        assert_eq!(d.word_range(p(0, 17)), (p(0, 12), p(0, 17)));
        assert_eq!(d.block_range(1), (p(1, 0), p(1, 4)));
    }

    #[test]
    fn select_all_and_selected_text() {
        let mut d = doc("ab\ncd");
        d.select_all();
        assert_eq!(d.selected_text(), "ab\ncd");
        assert_eq!(d.text_in(p(1, 1), p(0, 1)), "b\nc");
    }

    #[test]
    fn selection_is_clamped_to_the_document() {
        let mut d = doc("é");
        d.set_selection(p(9, 9), p(0, 1));
        assert_eq!(d.selection().anchor, p(0, 2));
        assert_eq!(d.selection().head, p(0, 0)); // 1 is inside « é »
    }

    #[test]
    fn active_marks_intersect_over_a_mixed_selection() {
        let bold = Marks::NONE.with(Mark::Bold);
        let bi = bold.clone().with(Mark::Italic);
        let mut d = Document::from_blocks(vec![Block::new(BlockKind::Paragraph).with("ab", bi).with("cd", bold)]);
        d.set_selection(p(0, 0), p(0, 4));
        let m = d.active_marks();
        assert!(m.bold);
        assert!(!m.italic);
    }

    // ── Undo / redo ──────────────────────────────────────────────────────────

    #[test]
    fn consecutive_typing_is_one_undo_step() {
        let mut d = Document::new();
        type_str(&mut d, "abc");
        d.insert_paragraph();
        type_str(&mut d, "de");
        assert!(d.undo());
        assert_eq!(d.plain_text(), "abc\n");
        assert!(d.undo());
        assert_eq!(d.plain_text(), "abc");
        assert!(d.undo());
        assert_eq!(d.plain_text(), "");
        assert!(!d.undo());
        assert!(d.redo());
        assert_eq!(d.plain_text(), "abc");
        assert!(d.redo());
        assert!(d.redo());
        assert_eq!(d.plain_text(), "abc\nde");
        assert!(!d.redo());
    }

    #[test]
    fn moving_the_caret_breaks_the_typing_group() {
        let mut d = Document::new();
        type_str(&mut d, "ab");
        d.move_to(p(0, 0), false);
        type_str(&mut d, "x");
        d.undo();
        assert_eq!(d.plain_text(), "ab");
    }

    #[test]
    fn undo_restores_the_selection_and_a_new_edit_clears_redo() {
        let mut d = doc("hello");
        d.set_selection(p(0, 1), p(0, 4));
        d.toggle_mark(Mark::Bold);
        d.undo();
        assert_eq!(d.selection(), Selection { anchor: p(0, 1), head: p(0, 4) });
        assert!(!d.active_marks().bold);
        assert!(d.can_redo());
        d.insert_text("z");
        assert!(!d.can_redo());
    }

    #[test]
    fn consecutive_backspaces_are_one_undo_step() {
        let mut d = doc("abcd");
        d.move_to(d.end(), false);
        d.delete_backward(false);
        d.delete_backward(false);
        assert_eq!(d.plain_text(), "ab");
        d.undo();
        assert_eq!(d.plain_text(), "abcd");
    }

    #[test]
    fn undo_depth_is_capped() {
        let mut d = Document::new();
        for _ in 0..(m::UNDO_LIMIT + 20) {
            d.insert_paragraph();
        }
        let mut n = 0;
        while d.undo() {
            n += 1;
        }
        assert_eq!(n, m::UNDO_LIMIT);
    }

    #[test]
    fn revision_bumps_on_edits_only() {
        let mut d = doc("ab");
        let r = d.revision();
        d.move_to(p(0, 1), false);
        assert_eq!(d.revision(), r);
        d.insert_text("x");
        assert!(d.revision() > r);
    }

    // ── Widget-level logic that needs no canvas ─────────────────────────────

    #[test]
    fn widget_routes_keys_it_handles_and_leaves_tab() {
        let rt = RichTextBox::new();
        let key = |vk: u16, mods: Modifiers| InputEvent::Key { vk, down: true, repeat: false, mods };
        assert!(rt.wants_event(&key(vk::LEFT, Modifiers::SHIFT)));
        assert!(rt.wants_event(&key(vk::letter('B'), Modifiers::CTRL)));
        assert!(rt.wants_event(&InputEvent::Text("a".into())));
        assert!(!rt.wants_event(&key(vk::TAB, Modifiers::NONE)));
        assert!(!rt.wants_event(&key(vk::ESCAPE, Modifiers::NONE)));
        assert!(!rt.wants_event(&key(vk::letter('B'), Modifiers::ALT)));
        // Read-only: navigation and copy only.
        let mut ro = RichTextBox::new();
        ro.read_only = true;
        assert!(ro.wants_event(&key(vk::letter('C'), Modifiers::CTRL)));
        assert!(!ro.wants_event(&key(vk::letter('V'), Modifiers::CTRL)));
        assert!(!ro.wants_event(&InputEvent::Text("a".into())));
        assert!(!ro.wants_event(&key(vk::BACK, Modifiers::NONE)));
    }

    #[test]
    fn toolbar_commands_drive_the_document_and_light_the_bar() {
        let mut rt = RichTextBox::new().with_document(doc("hello"));
        rt.doc.select_all();
        assert!(rt.apply_command(RichTextCommand::Bold));
        assert!(rt.apply_command(RichTextCommand::BulletList));
        rt.sync_toolbar();
        let bar = rt.toolbar.as_ref().expect("standard toolbar");
        assert!(bar.is_active(RichTextCommand::Bold));
        assert!(bar.is_active(RichTextCommand::BulletList));
        assert!(!bar.is_active(RichTextCommand::Italic));
        assert!(!rt.apply_command(RichTextCommand::AlignCenter));
        // The link row: open, type, apply.
        assert!(rt.apply_command(RichTextCommand::Link));
        assert_eq!(rt.link_draft.as_deref(), Some(""));
        rt.link_draft = Some("kubuno.com".into());
        rt.apply_link();
        assert!(rt.link_draft.is_none());
        assert_eq!(rt.active_marks().link.as_deref(), Some("https://kubuno.com"));
    }

    #[test]
    fn command_of_maps_every_toolbar_cell_back() {
        let bar = RichTextToolbar::with_alignments();
        let cmds: Vec<_> = (0..bar.items.len()).filter_map(|i| command_of(&bar, i)).collect();
        assert_eq!(cmds.len(), 11);
        assert_eq!(cmds[0], RichTextCommand::Bold);
        assert_eq!(cmds[10], RichTextCommand::AlignJustify);
    }

    #[test]
    fn a_disabled_editor_ignores_commands() {
        let mut rt = RichTextBox::new().with_document(doc("x"));
        rt.enabled = false;
        rt.doc.select_all();
        assert!(!rt.apply_command(RichTextCommand::Bold));
        rt.toggle_mark(Mark::Bold);
        assert!(!rt.active_marks().bold);
    }

    #[test]
    fn word_motion_helpers() {
        assert_eq!(word_left("ab  cd", 6), 4);
        assert_eq!(word_left("ab  cd", 4), 0);
        assert_eq!(word_right("ab  cd", 0), 4);
        assert_eq!(word_right("ab\ncd", 2), 2);
        assert_eq!(word_around("", 0), (0, 0));
        assert_eq!(word_around("a,b", 1), (1, 2));
    }
}
