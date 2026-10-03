//! Kubuno primitives — text fields.
//!
//! ## Who does what
//!
//! | layer | what it owns | reached how |
//! |---|---|---|
//! | [`kubuno_desktop_controls::text`] | the **model**: `text`, `read_only`, `max_length`, `password_char`, `multiline`, `word_wrap`, `text_align`, `placeholder_text`, `character_casing`, the clamped selection, the whole mask engine | owned by value, reached through [`Deref`] |
//! | this file | the **Kubuno chrome** (ground, border, focus outline, placeholder, icon columns, the invalid state) **and the editor**: caret, selection, typing, clipboard, undo, scrolling, soft wrap, the text context menu | [`Widget::paint`] + `update` |
//!
//! The editor used to be borrowed from `kubuno_drive_desktop_app_controls::edit_box`, but that
//! view is stateless: it keeps the caret visible by pinning it to the right edge
//! every frame, it has no notion of a scroll offset that survives a caret move,
//! no multi-row selection and no blink. A browser input has all of those, so the
//! editing arithmetic now lives here — as PURE functions (word boundaries, the
//! edit buffer, undo history, key mapping, soft wrap) that the tests pin, driven
//! by a small per-field [`EditState`] that persists between frames.
//!
//! ## Driving a field
//!
//! ```ignore
//! // once per frame, in paint order:
//! let fs = ring.register_with("name", r, FocusOpts::TEXT);   // kubuno_desktop_ui::focus
//! let out = field.update(c, r, &EditInput::new(f, fs));     // mouse, keys, clipboard, menu
//! field.paint(c, r, fs.apply(WidgetState::REST));           // chrome + text + caret
//! if let Some(m) = field.menu_bounds() { ring.keep_focus_in(m); } // its context menu
//! ```
//!
//! A caller that never calls `update` still gets the previous behaviour: the
//! model's selection is painted, and a focused field shows its (now blinking)
//! caret, scrolled into view.
//!
//! ## Where the numbers come from
//!
//! `core/frontend/src/ui/Input.tsx`:
//!
//! ```text
//! rounded-md border bg-white text-sm text-text-primary placeholder:text-text-tertiary
//! px-3 py-2 h-9 kb-field-focus
//! disabled:bg-surface-2 disabled:cursor-not-allowed disabled:opacity-60
//! error ? 'border-danger kb-field-focus-danger' : 'border-border'
//! leftIcon && 'pl-9', rightIcon && 'pr-9'
//! ```
//!
//! and `core/frontend/src/index.css`:
//!
//! ```text
//! .kb-field-focus:focus-visible { outline: 3px solid var(--color-primary);
//!                                 outline-offset: -1px; border-color: var(--color-primary) }
//! .kb-field-focus-danger:focus-visible { outline-color: var(--color-danger); border-color: … }
//! ```
//!
//! `h-9` = [`height::BUTTON_MD`], `rounded-md` = [`radius::SM`], `px-3` =
//! [`space::MD`], `py-2` = [`space::SM`], `text-sm` = the body format (13.5 px, as
//! on the web). A text input always
//! matches `:focus-visible` in a browser, so the outline shows on any focus.
//!
//! `@ui/Textarea` ships the *same* skin plus `h-36 min-h-16`, so [`TextArea`] is
//! the `multiline` case of the same field. The context menu is
//! `core/shell/TextFieldMenuHost.tsx` over `ui/textFieldMenu.ts`, rendered by
//! the product's one menu, [`crate::lists::Menu`] (`@ui/MenuDropdown`). The
//! search pill is `core/shell/SearchBar.tsx` in its `compact` form (`h-9`).
//!
//! **There is no hover state.** Neither `@ui/Input` nor `@ui/Textarea` declares
//! one: a hovered field only changes the pointer (an I-beam).

use std::cell::Cell;
use std::ops::{Deref, DerefMut};

use kubuno_drive_desktop_app_controls::{Canvas, Rect};
use kubuno_desktop_controls::enums::{HorizontalAlignment, Size};
use kubuno_desktop_controls::host::{self, vk, Cursor, Frame, InputEvent, Modifiers};
use kubuno_desktop_controls::text::{MaskFormat, MaskedTextBox, TextBox, TextBoxBase};
use kubuno_desktop_controls::Control;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::focus::{caret_visible, FocusState};
use crate::lists::{self, Menu, MenuEntry};
use crate::metrics::{control, height, pill, radius, space};
use crate::widget::{Widget, WidgetState};

// ── Metrics ──────────────────────────────────────────────────────────────────
//
// `crate::metrics` is the one metric table and it is not this family's file, so
// what a field needs and the web never named is declared here, once, with its
// source. Everything that IS in the shared table (the 36 height, the 4 radius,
// the 12 and 8 paddings) is taken from there and never re-typed.

/// `@ui/Input`: `px-3` — the text's horizontal inset.
const PAD_X: f32 = space::MD;
/// `@ui/Input`: `py-2` — the vertical inset of the text rows. With [`LINE`] it
/// reconstructs `h-9` exactly: 8 + 20 + 8 = 36.
const PAD_Y: f32 = space::SM;
/// The line box the selection and the caret cover: `leading-normal` text in a
/// 20 DIP row. It is also the row advance of a multiline field, so a one-row
/// `TextArea` is exactly a `TextField`.
const LINE: f32 = 20.0;
/// The focus ring the predecessor (`edit_box`) drew: `focus:ring-2`. Kept for
/// callers that lay out against it; the field itself now draws
/// [`FOCUS_OUTLINE`], which is what `.kb-field-focus` says today.
pub const FOCUS_RING: f32 = 2.0;
/// `.kb-field-focus:focus-visible { outline: 3px solid var(--color-primary);
/// outline-offset: -1px }` — drawn INWARD here, so the field never paints
/// outside the bounds it was given; it covers the 1 DIP border, which the web
/// recolours to the same primary.
pub const FOCUS_OUTLINE: f32 = 3.0;
/// A leading/trailing glyph. The web sizes these per call site (14, 15 or 16);
/// the desktop draws every inline icon at 16, as the omnibar does.
const ICON: f32 = 16.0;
/// `@ui/Input`: the icon span is `absolute left-3` / `right-3`.
const ICON_INSET: f32 = space::MD;
/// `@ui/Input`: `pl-9` / `pr-9` — [`ICON_INSET`] + [`ICON`] + `gap-2`.
const ICON_COLUMN: f32 = 36.0;
/// `disabled:opacity-60`. Applied to the ink and the border; the ground is a
/// token of its own (`disabled:bg-surface-2`), so it is not faded twice.
const DISABLED_ALPHA: f32 = 0.6;

/// `@ui/Textarea`: `h-36` — the height a multiline field asks for, and
/// `min-h-16`, the floor it stays usable at.
const TEXTAREA_HEIGHT: f32 = 144.0;
const TEXTAREA_MIN_HEIGHT: f32 = 64.0;

/// The glyph a password field shows. `kubuno_desktop_controls::text::PASSWORD_GLYPH`
/// (private there) — WinForms' `UseSystemPasswordChar` bullet, U+25CF.
const PASSWORD_GLYPH: char = '\u{25CF}';

/// The editor's own metrics. None of them is a design token: they are what a
/// browser's native input does, named so a paint body carries no literal.
mod edit {
    /// The caret is a one-DIP rule spanning the line box (Chromium draws a
    /// 1 CSS px caret in `currentColor`; `caret-color` is never set in the web).
    pub const CARET_W: f32 = 1.0;
    /// The web declares no `::selection` rule, so the field keeps the
    /// browser's highlight — the primary at 35 %, a plain rectangle (the
    /// predecessor's reading, `edit_box::draw_padded`).
    pub const SELECTION_ALPHA: f32 = 0.35;
    /// How far back the undo stack reaches. Browsers do not publish a depth;
    /// 100 steps outlasts any form field.
    pub const UNDO_DEPTH: usize = 100;
    /// Typing (or deleting) within this many ms of the last edit of the same
    /// kind merges into one undo step, the way a browser groups a burst of
    /// keystrokes into one `insertText` entry.
    pub const UNDO_MERGE_MS: u64 = 1000;
    /// While a drag selection is held outside the text, the field keeps
    /// scrolling: one repaint per display frame.
    pub const AUTOSCROLL_MS: u32 = 16;
}

/// The search pill: `core/shell/SearchBar.tsx`, `compact` (the `h-9` form a
/// 40 DIP top bar holds — the only size a form row can mix with fields and
/// `md` buttons without misaligning).
mod search {
    use crate::metrics::{height, space};
    /// `compact ? 'h-9'` — the field height, so a toolbar row mixing a search
    /// pill, a field and an `md` button lines up.
    pub const HEIGHT: f32 = height::BUTTON_MD;
    /// The omnibar's height (`omnibar::HEIGHT`), what this pill used to be.
    pub const OMNIBAR_HEIGHT: f32 = 38.0;
    /// `compact ? 'pl-3 pr-2'` around the `<Search size={16} />` glyph.
    pub const ICON_LEFT: f32 = space::MD;
    pub const ICON_GAP: f32 = space::SM;
    /// Where the input starts: `pl-3` + 16 + `pr-2` = 36.
    pub const TEXT_LEFT: f32 = ICON_LEFT + super::ICON + ICON_GAP;
    /// The ✕ button: `px-1` around an `<X size={16} />` — 24 wide.
    pub const CLEAR_W: f32 = space::XS + super::ICON + space::XS;
    /// The trailing `pr-2` the pill keeps after its last button.
    pub const END_PAD: f32 = space::SM;
}

/// The text context menu (`TextFieldMenuHost.tsx`).
mod text_menu {
    /// `<MenuDropdown … minWidth={220} />`.
    pub const MIN_WIDTH: f32 = 220.0;
    /// `MenuDropdown` keeps the panel 8 px inside the viewport, and flips
    /// above the point when it would run off the bottom.
    pub const EDGE: f32 = 8.0;
    /// Room left around the panel in its popup window for `SHADOW_MENU`
    /// (~7 DIP), as the lists gallery sizes it.
    pub const SHADOW: f32 = 10.0;
}

// ── Geometry (pure — this is what the tests pin) ──────────────────────────────

/// Which side an icon column sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Leading,
    Trailing,
}

/// The rectangle the text lives in, given which icon columns are taken.
///
/// With no icons the inset is [`PAD_X`] on both sides (`px-3`). An icon widens
/// its own side to [`ICON_COLUMN`] (`pl-9` / `pr-9`). Vertically it is the full
/// box: a single-line field centres its line, and the multiline case insets by
/// [`PAD_Y`] itself (see [`row_rect`]).
pub fn content_rect(bounds: Rect, leading: bool, trailing: bool) -> Rect {
    let left = bounds.left + if leading { ICON_COLUMN } else { PAD_X };
    let right = bounds.right - if trailing { ICON_COLUMN } else { PAD_X };
    Rect::new(left, bounds.top, right.max(left), bounds.bottom)
}

/// The rectangle an icon is centred in, so its outer edge lands at
/// [`ICON_INSET`] from the border (`absolute left-3` / `right-3`).
pub fn icon_rect(bounds: Rect, side: Side) -> Rect {
    match side {
        Side::Leading => {
            let left = bounds.left + ICON_INSET;
            Rect::new(left, bounds.top, left + ICON, bounds.bottom)
        }
        Side::Trailing => {
            let right = bounds.right - ICON_INSET;
            Rect::new(right - ICON, bounds.top, right, bounds.bottom)
        }
    }
}

/// The rectangle of row `i` of a multiline field (unscrolled).
pub fn row_rect(bounds: Rect, i: usize) -> Rect {
    let top = bounds.top + PAD_Y + i as f32 * LINE;
    Rect::new(bounds.left, top, bounds.right, top + LINE)
}

/// The height a field asks for: `h-9` for a single line, and one [`LINE`] per
/// row plus [`PAD_Y`] top and bottom when multiline — which returns exactly
/// `h-9` again at one row (8 + 20 + 8 = 36).
pub fn field_height(multiline: bool, rows: usize) -> f32 {
    if multiline {
        rows.max(1) as f32 * LINE + 2.0 * PAD_Y
    } else {
        height::BUTTON_MD
    }
}

/// The search glyph's cell: `pl-3`, 16 wide.
pub fn search_icon_rect(bounds: Rect) -> Rect {
    let left = bounds.left + search::ICON_LEFT;
    Rect::new(left, bounds.top, left + ICON, bounds.bottom)
}

/// The ✕ clear button of a search field: `px-1` around a 16 glyph, before the
/// pill's trailing `pr-2`.
pub fn search_clear_rect(bounds: Rect) -> Rect {
    let right = bounds.right - search::END_PAD;
    Rect::new((right - search::CLEAR_W).max(bounds.left), bounds.top, right, bounds.bottom)
}

/// The editable strip inside a search pill, with the ✕ column reserved — the
/// conservative answer (the text never runs under the ✕). See
/// [`search_text_rect_with`] for the exact strip given whether the ✕ shows.
pub fn search_text_rect(bounds: Rect) -> Rect {
    search_text_rect_with(bounds, true)
}

/// The editable strip: from `pl-3 + 16 + pr-2` to the ✕ when it shows
/// (`flex-1 min-w-0` before the button), else to the trailing `pr-2`.
pub fn search_text_rect_with(bounds: Rect, clear: bool) -> Rect {
    let left = bounds.left + search::TEXT_LEFT;
    let right = if clear { search_clear_rect(bounds).left } else { bounds.right - search::END_PAD };
    Rect::new(left, bounds.top, right.max(left), bounds.bottom)
}

// ── Clipboard ────────────────────────────────────────────────────────────────

#[cfg(test)]
thread_local! {
    /// The clipboard of this module's tests: they never touch the user's, and can see what an
    /// action copied (or that a password field copied nothing).
    static TEST_CLIPBOARD: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Puts `text` on the clipboard (the system's; a per-thread stand-in under test).
fn clipboard_set(text: &str) {
    #[cfg(test)]
    TEST_CLIPBOARD.with(|c| *c.borrow_mut() = Some(text.to_string()));
    #[cfg(not(test))]
    host::set_clipboard_text(text);
}

/// The clipboard's text (the system's; a per-thread stand-in under test).
fn clipboard_get() -> Option<String> {
    #[cfg(test)]
    return TEST_CLIPBOARD.with(|c| c.borrow().clone());
    #[cfg(not(test))]
    host::clipboard_text()
}

// ── Characters ───────────────────────────────────────────────────────────────

/// Byte offset of character `n` (negative → 0, past the end → the length).
/// The replica counts a selection in characters, as .NET does; slicing needs
/// bytes, and a caret landing mid-character would panic the next slice.
#[cfg(test)]
fn byte_at(s: &str, n: i32) -> usize {
    if n <= 0 {
        return 0;
    }
    byte_of(s, n as usize)
}

/// Byte offset of character `n`, clamped to the end.
fn byte_of(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map_or(s.len(), |(i, _)| i)
}

/// The characters `a..b` of `s` (clamped, `a > b` swapped).
fn char_slice(s: &str, a: usize, b: usize) -> &str {
    let (a, b) = (a.min(b), a.max(b));
    &s[byte_of(s, a)..byte_of(s, b)]
}

fn char_count(s: &str) -> usize {
    s.chars().count()
}

/// How a caret move or a double-click groups characters. Windows' word
/// breaking, as Chromium applies it on this platform: letters and digits (and
/// `_`) form words, punctuation runs form their own stops, spaces separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Space,
    Break,
    Word,
    Punct,
}

fn class(c: char) -> Class {
    if c == '\n' {
        Class::Break
    } else if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Punct
    }
}

/// Ctrl+Left: back over spaces, then to the start of the run before them.
pub fn word_left(text: &str, i: usize) -> usize {
    let ch: Vec<char> = text.chars().collect();
    let mut j = i.min(ch.len());
    while j > 0 && matches!(class(ch[j - 1]), Class::Space | Class::Break) {
        j -= 1;
    }
    if j > 0 {
        let k = class(ch[j - 1]);
        while j > 0 && class(ch[j - 1]) == k {
            j -= 1;
        }
    }
    j
}

/// Ctrl+Right, the Windows way: over the current run, then over the spaces
/// after it — landing on the START of the next word (macOS stops at the end).
/// A line break is a stop of its own.
pub fn word_right(text: &str, i: usize) -> usize {
    let ch: Vec<char> = text.chars().collect();
    let n = ch.len();
    let start = i.min(n);
    let mut j = start;
    if j < n && !matches!(class(ch[j]), Class::Space | Class::Break) {
        let k = class(ch[j]);
        while j < n && class(ch[j]) == k {
            j += 1;
        }
    }
    while j < n && class(ch[j]) == Class::Space {
        j += 1;
    }
    if j == start && j < n && class(ch[j]) == Class::Break {
        j += 1;
        while j < n && class(ch[j]) == Class::Space {
            j += 1;
        }
    }
    j
}

/// What a double-click on character `i` selects: its run (a word, a
/// punctuation run or a space run). A word takes its trailing spaces with it
/// — Chromium's `select_trailing_whitespace` on Windows, what Notepad and Word
/// do too. A line break selects nothing.
pub fn word_range_at(text: &str, i: usize) -> (usize, usize) {
    let ch: Vec<char> = text.chars().collect();
    let n = ch.len();
    if n == 0 {
        return (0, 0);
    }
    let p = i.min(n - 1);
    let k = class(ch[p]);
    if k == Class::Break {
        return (p, p);
    }
    let mut a = p;
    while a > 0 && class(ch[a - 1]) == k {
        a -= 1;
    }
    let mut b = p + 1;
    while b < n && class(ch[b]) == k {
        b += 1;
    }
    if k == Class::Word {
        while b < n && class(ch[b]) == Class::Space {
            b += 1;
        }
    }
    (a, b)
}

/// What a triple-click on character `i` selects: its paragraph, without the
/// line break (a single-line field is one paragraph, so it selects all).
pub fn paragraph_range_at(text: &str, i: usize) -> (usize, usize) {
    let ch: Vec<char> = text.chars().collect();
    let n = ch.len();
    let p = i.min(n);
    let mut a = p;
    while a > 0 && ch[a - 1] != '\n' {
        a -= 1;
    }
    let mut b = p;
    while b < n && ch[b] != '\n' {
        b += 1;
    }
    (a, b)
}

/// What the clipboard may put into a field: a single-line input strips line
/// breaks (the HTML value sanitisation of `<input type=text>`), a multiline one
/// normalises `\r\n` and lone `\r` to `\n` (a `<textarea>` value).
pub fn sanitize_paste(text: &str, multiline: bool) -> String {
    if multiline {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.chars().filter(|c| *c != '\r' && *c != '\n').collect()
    }
}

// ── The edit buffer (pure) ───────────────────────────────────────────────────

/// A text with a directional selection, in characters: the whole editing
/// arithmetic of a field, without pixels. `anchor == caret` is a plain caret.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EditBuffer {
    pub text: String,
    pub anchor: usize,
    pub caret: usize,
}

impl EditBuffer {
    pub fn new(text: impl Into<String>, anchor: usize, caret: usize) -> Self {
        let text = text.into();
        let n = char_count(&text);
        Self { text, anchor: anchor.min(n), caret: caret.min(n) }
    }

    pub fn len(&self) -> usize {
        char_count(&self.text)
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// `(start, end)`, ordered.
    pub fn selection(&self) -> (usize, usize) {
        (self.anchor.min(self.caret), self.anchor.max(self.caret))
    }

    pub fn has_selection(&self) -> bool {
        self.anchor != self.caret
    }

    pub fn selected_text(&self) -> &str {
        let (a, b) = self.selection();
        char_slice(&self.text, a, b)
    }

    /// Moves the caret; `extend` keeps the anchor (Shift).
    pub fn set_caret(&mut self, pos: usize, extend: bool) {
        self.caret = pos.min(self.len());
        if !extend {
            self.anchor = self.caret;
        }
    }

    /// Selects `a..b`, caret at `b`.
    pub fn select(&mut self, a: usize, b: usize) {
        let n = self.len();
        self.anchor = a.min(n);
        self.caret = b.min(n);
    }

    pub fn select_all(&mut self) {
        self.select(0, self.len());
    }

    /// Replaces the selection with `ins`, keeping the whole text within
    /// `max_len` characters (`maxlength`: the browser truncates what is
    /// inserted to the room left, and refuses a key once full). The caret
    /// lands after the insertion. Returns whether the text changed.
    pub fn insert(&mut self, ins: &str, max_len: Option<usize>) -> bool {
        let (a, b) = self.selection();
        let kept = self.len() - (b - a);
        let ins: String = match max_len {
            Some(m) => ins.chars().take(m.saturating_sub(kept)).collect(),
            None => ins.to_string(),
        };
        if a == b && ins.is_empty() {
            return false;
        }
        let (ba, bb) = (byte_of(&self.text, a), byte_of(&self.text, b));
        self.text.replace_range(ba..bb, &ins);
        self.caret = a + char_count(&ins);
        self.anchor = self.caret;
        true
    }

    /// Backspace (`word`: Ctrl+Backspace). A selection is deleted whole.
    pub fn delete_back(&mut self, word: bool) -> bool {
        if !self.has_selection() {
            if self.caret == 0 {
                return false;
            }
            self.anchor = if word { word_left(&self.text, self.caret) } else { self.caret - 1 };
        }
        self.insert("", None)
    }

    /// Delete (`word`: Ctrl+Delete). A selection is deleted whole.
    pub fn delete_forward(&mut self, word: bool) -> bool {
        if !self.has_selection() {
            if self.caret >= self.len() {
                return false;
            }
            self.anchor = if word { word_right(&self.text, self.caret) } else { self.caret + 1 };
        }
        self.insert("", None)
    }
}

// ── Undo history (pure) ──────────────────────────────────────────────────────

/// What kind of edit produced an undo step — typing and deleting bursts merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Typing,
    Deleting,
    Other,
}

/// A field's text and selection at one point in time.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    text: String,
    anchor: usize,
    caret: usize,
}

impl Snapshot {
    fn of(b: &EditBuffer) -> Self {
        Self { text: b.text.clone(), anchor: b.anchor, caret: b.caret }
    }
}

/// The undo / redo stacks of one field — the browser's per-input undo, which
/// `textFieldMenu.ts` reaches through `execCommand('undo')`.
#[derive(Debug, Clone, Default)]
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last: Option<(EditKind, u64)>,
}

impl History {
    /// Records the state BEFORE an edit of `kind` made at `now_ms`. A typing
    /// (or deleting) burst coalesces into one step.
    fn record(&mut self, before: Snapshot, kind: EditKind, now_ms: u64) {
        let merge = kind != EditKind::Other
            && !self.undo.is_empty()
            && self
                .last
                .is_some_and(|(k, t)| k == kind && now_ms.saturating_sub(t) < edit::UNDO_MERGE_MS);
        if !merge {
            self.undo.push(before);
            if self.undo.len() > edit::UNDO_DEPTH {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last = Some((kind, now_ms));
    }

    fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let s = self.undo.pop()?;
        self.redo.push(current);
        self.last = None;
        Some(s)
    }

    fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let s = self.redo.pop()?;
        self.undo.push(current);
        self.last = None;
        Some(s)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Forgets everything (a programmatic `set_text` starts a new document).
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

// ── Keys (pure) ──────────────────────────────────────────────────────────────

/// What a key does in a text field, as Chromium maps it on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditCommand {
    Left,
    Right,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
    Up,
    Down,
    PageUp,
    PageDown,
    SelectAll,
    Backspace,
    BackspaceWord,
    Delete,
    DeleteWord,
    Undo,
    Redo,
    Copy,
    Cut,
    Paste,
    Enter,
    ContextMenu,
}

/// The command for a key-down, and whether it EXTENDS the selection (Shift).
/// Alt chords are never an edit (they belong to the window's menu / system).
pub fn command_for(key: u16, mods: Modifiers) -> Option<(EditCommand, bool)> {
    use EditCommand::*;
    if mods.alt {
        return None;
    }
    let (ctrl, shift) = (mods.ctrl, mods.shift);
    let cmd = match key {
        vk::LEFT => (if ctrl { WordLeft } else { Left }, shift),
        vk::RIGHT => (if ctrl { WordRight } else { Right }, shift),
        vk::HOME => (if ctrl { DocStart } else { LineStart }, shift),
        vk::END => (if ctrl { DocEnd } else { LineEnd }, shift),
        vk::UP if !ctrl => (Up, shift),
        vk::DOWN if !ctrl => (Down, shift),
        vk::PAGE_UP if !ctrl => (PageUp, shift),
        vk::PAGE_DOWN if !ctrl => (PageDown, shift),
        vk::BACK if ctrl && !shift => (BackspaceWord, false),
        vk::BACK if !ctrl => (Backspace, false),
        vk::DELETE if shift && !ctrl => (Cut, false),
        vk::DELETE if ctrl && !shift => (DeleteWord, false),
        vk::DELETE if !ctrl => (Delete, false),
        vk::INSERT if ctrl && !shift => (Copy, false),
        vk::INSERT if shift && !ctrl => (Paste, false),
        // Shift+Enter "extends": a multi-line field that submits on Enter breaks the line instead.
        vk::ENTER if !ctrl => (Enter, shift),
        vk::APPS if !ctrl && !shift => (ContextMenu, false),
        vk::F10 if shift && !ctrl => (ContextMenu, false),
        k if ctrl && k == vk::letter('a') && !shift => (SelectAll, false),
        k if ctrl && k == vk::letter('c') && !shift => (Copy, false),
        k if ctrl && k == vk::letter('x') && !shift => (Cut, false),
        k if ctrl && k == vk::letter('v') && !shift => (Paste, false),
        k if ctrl && k == vk::letter('z') => (if shift { Redo } else { Undo }, false),
        k if ctrl && k == vk::letter('y') && !shift => (Redo, false),
        _ => return None,
    };
    Some(cmd)
}

// ── Soft wrap (pure over a measuring function) ───────────────────────────────

/// One row of laid-out text: characters `start..end` (the line break itself is
/// not part of any row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualLine {
    pub start: usize,
    pub end: usize,
}

/// Splits `text` into rows: one per paragraph, and — when `width` is given —
/// wrapped greedily at word boundaries so no row is wider than `width`
/// (`<textarea wrap="soft">`: trailing spaces hang, a word longer than the row
/// breaks between characters, as `overflow-wrap: break-word` does).
pub fn wrap_lines(text: &str, width: Option<f32>, measure: &dyn Fn(&str) -> f32) -> Vec<VisualLine> {
    let ch: Vec<char> = text.chars().collect();
    let n = ch.len();
    let mut out = Vec::new();
    let mut para = 0usize;
    loop {
        let mut para_end = para;
        while para_end < n && ch[para_end] != '\n' {
            para_end += 1;
        }
        match width {
            Some(w) if w > 0.0 => wrap_paragraph(&ch, para, para_end, w, measure, &mut out),
            _ => out.push(VisualLine { start: para, end: para_end }),
        }
        if para_end >= n {
            break;
        }
        para = para_end + 1;
    }
    out
}

fn wrap_paragraph(
    ch: &[char],
    start: usize,
    end: usize,
    width: f32,
    measure: &dyn Fn(&str) -> f32,
    out: &mut Vec<VisualLine>,
) {
    let span = |a: usize, b: usize| -> String { ch[a..b].iter().collect() };
    let mut line = start;
    if start == end {
        out.push(VisualLine { start, end });
        return;
    }
    while line < end {
        let mut last_fit: Option<usize> = None;
        let mut p = line;
        let mut broke = false;
        while p < end {
            // One token: a (possibly empty) word, then the spaces after it.
            let mut word_end = p;
            while word_end < end && !ch[word_end].is_whitespace() {
                word_end += 1;
            }
            let mut tok_end = word_end;
            while tok_end < end && ch[tok_end].is_whitespace() {
                tok_end += 1;
            }
            // Trailing spaces hang past the edge, so only the word counts.
            if measure(&span(line, word_end)) <= width {
                last_fit = Some(tok_end);
                p = tok_end;
                continue;
            }
            match last_fit {
                Some(fit) if fit > line => {
                    out.push(VisualLine { start: line, end: fit });
                    line = fit;
                }
                _ => {
                    // A single word wider than the row: break between
                    // characters, keeping at least one per row.
                    let mut m = line + 1;
                    while m < word_end && measure(&span(line, m + 1)) <= width {
                        m += 1;
                    }
                    out.push(VisualLine { start: line, end: m });
                    line = m;
                }
            }
            broke = true;
            break;
        }
        if !broke {
            out.push(VisualLine { start: line, end });
            return;
        }
    }
}

/// The row that holds character position `pos`: the last row starting at or
/// before it (so a caret on a soft break belongs to the row that follows).
pub fn line_of(lines: &[VisualLine], pos: usize) -> usize {
    lines.iter().rposition(|l| l.start <= pos).unwrap_or(0)
}

// ── The text context menu ────────────────────────────────────────────────────

/// An entry of the text context menu (`TextFieldMenuHost.tsx`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditAction {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Delete,
    SelectAll,
}

/// What the menu needs to know to enable its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MenuFlags {
    /// `isEditable`: not read-only, not disabled.
    pub editable: bool,
    /// `hasSelection`: Cut, Copy and Delete need one.
    pub has_selection: bool,
    /// A password field never gives its text away (the browser greys Cut and
    /// Copy on `type=password`).
    pub copy_allowed: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

/// The menu `TextFieldMenuHost` builds, row for row — Undo / Redo /
/// separator / Cut (editable only), Copy, Paste and Delete (editable only),
/// separator, Select all — with the action each row runs (`None` on
/// separators). Undo and Redo are greyed when their stack is empty, which the
/// browser's `execCommand` would silently no-op.
pub fn text_menu(flags: MenuFlags) -> (Menu, Vec<Option<EditAction>>) {
    let mut items = Vec::new();
    let mut actions = Vec::new();
    let mut push = |item, action| {
        items.push(item);
        actions.push(action);
    };
    let copyable = flags.has_selection && flags.copy_allowed;
    if flags.editable {
        push(
            MenuEntry::new("Annuler").icon("Undo2").shortcut_text("Ctrl+Z").enabled(flags.can_undo).build(),
            Some(EditAction::Undo),
        );
        push(
            MenuEntry::new("Rétablir")
                .icon("Redo2")
                .shortcut_text("Ctrl+Shift+Z")
                .enabled(flags.can_redo)
                .build(),
            Some(EditAction::Redo),
        );
        push(lists::separator(), None);
        push(
            MenuEntry::new("Couper").icon("Scissors").shortcut_text("Ctrl+X").enabled(copyable).build(),
            Some(EditAction::Cut),
        );
    }
    push(
        MenuEntry::new("Copier").icon("Copy").shortcut_text("Ctrl+C").enabled(copyable).build(),
        Some(EditAction::Copy),
    );
    if flags.editable {
        push(
            MenuEntry::new("Coller").icon("ClipboardPaste").shortcut_text("Ctrl+V").build(),
            Some(EditAction::Paste),
        );
        push(
            MenuEntry::new("Supprimer").icon("Trash2").enabled(flags.has_selection).build(),
            Some(EditAction::Delete),
        );
    }
    push(lists::separator(), None);
    push(
        MenuEntry::new("Tout sélectionner").icon("TextSelect").shortcut_text("Ctrl+A").build(),
        Some(EditAction::SelectAll),
    );
    (Menu::with_items(items), actions)
}

/// Whether row `i` of the menu built from `flags` can be chosen.
fn menu_row_enabled(menu: &Menu, actions: &[Option<EditAction>], i: usize) -> bool {
    let enabled = match menu.items().get(i) {
        Some(kubuno_desktop_controls::toolstrip::StripItem::MenuItem(m)) => m.base.item.enabled,
        _ => false,
    };
    enabled && actions.get(i).copied().flatten().is_some()
}

/// The next choosable row from `from` in direction `step` (wrapping), for the
/// arrow keys.
fn menu_step(menu: &Menu, actions: &[Option<EditAction>], from: Option<usize>, forward: bool) -> Option<usize> {
    let n = menu.items().len();
    if n == 0 {
        return None;
    }
    let mut i = match from {
        Some(i) => i,
        None if forward => n - 1,
        None => 0,
    };
    for _ in 0..n {
        i = if forward { (i + 1) % n } else { (i + n - 1) % n };
        if menu_row_enabled(menu, actions, i) {
            return Some(i);
        }
    }
    None
}

/// Where a menu of `size` opens for the point `at`: at the point, pulled back
/// inside `screen` by [`text_menu::EDGE`], and above the point when it would
/// run off the bottom (`MenuDropdown`'s viewport rule; the viewport is the
/// monitor, since the menu lives in a popup window).
pub fn menu_panel_at(at: (f32, f32), size: (f32, f32), screen: Rect) -> Rect {
    let (w, h) = size;
    let edge = text_menu::EDGE;
    let x = if at.0 + w > screen.right - edge { (screen.right - edge - w).max(screen.left + edge) } else { at.0 };
    let y = if at.1 + h > screen.bottom - edge { (at.1 - h).max(screen.top + edge) } else { at.1 };
    Rect::new(x, y, x + w, y + h)
}

// ── Frame input ──────────────────────────────────────────────────────────────

/// One frame of input, as a field needs it: the pointer, the buttons, the
/// wheel and the focus. Keys are read from the host's queue
/// (`kubuno_desktop_controls::host`) while the field is focused.
#[derive(Clone, Copy)]
pub struct EditInput {
    /// Pointer in the SAME space as the bounds the field is painted in.
    pub mouse: (f32, f32),
    pub mouse_down: bool,
    pub right_down: bool,
    /// 1 / 2 / 3 for a single / double / triple click (read on the press).
    pub click_count: u8,
    pub mods: Modifiers,
    /// Wheel notches, web sign (`.1 > 0` scrolls down).
    pub wheel: (f32, f32),
    pub window_focused: bool,
    /// The window lost activation this frame: close the context menu.
    pub dismiss: bool,
    /// The monitor's work area, where the context menu is placed.
    pub screen: Rect,
    /// The field holds the keyboard focus (from the page's `FocusRing`).
    pub focused: bool,
    /// It gained the focus this frame.
    pub gained: bool,
}

impl EditInput {
    /// From the host frame and the field's [`FocusState`]
    /// (`FocusRing::register_with(id, rect, FocusOpts::TEXT)`).
    pub fn new(f: &Frame, focus: FocusState) -> Self {
        Self {
            mouse: f.mouse,
            mouse_down: f.mouse_down,
            right_down: f.right_down,
            click_count: f.click_count,
            mods: f.mods,
            wheel: f.wheel,
            window_focused: f.window_focused,
            dismiss: f.dismiss,
            screen: f.screen_area(),
            focused: focus.focused,
            gained: focus.gained,
        }
    }

    /// For a caller without a focus manager: `focused` says whether the field
    /// reads the keys.
    pub fn with_focus(f: &Frame, focused: bool) -> Self {
        Self::new(f, FocusState { focused, visible: focused, gained: false })
    }
}

/// What happened to a field during one [`TextField::update`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditOutcome {
    /// The text changed (typing, paste, cut, undo, the ✕…) — the web's `input`.
    pub changed: bool,
    /// Enter in a single-line field (a form's implicit submission).
    pub submitted: bool,
    /// Escape was pressed while focused and the field did not use it (a
    /// dialog may cancel on it). It is NOT consumed.
    pub escaped: bool,
    /// The search ✕ (or Escape in a search field) emptied the field.
    pub cleared: bool,
    /// The text context menu is open after this frame: a page swallows the
    /// next click (the web's backdrop) and keeps the focus in
    /// [`TextField::menu_bounds`].
    pub menu_open: bool,
}

// ── Per-field state ──────────────────────────────────────────────────────────

/// How a held drag extends the selection: by character, by word (it started
/// with a double-click) or by paragraph (a triple-click).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    Char,
    Word(usize, usize),
    Line(usize, usize),
}

/// An open context menu: where it was asked for, its panel (client space) and
/// its lit row.
#[derive(Clone, Copy)]
struct MenuState {
    at: (f32, f32),
    panel: Option<Rect>,
    hot: Option<usize>,
    last_mouse: (f32, f32),
}

/// Everything a field remembers between frames that is not the model: the
/// directional selection, the scroll offsets, the drag, the undo history, the
/// blink clock and the context menu.
///
/// The caret and the scroll are `Cell`s because [`Widget::paint`] (which takes
/// `&self`) is where a caret is scrolled into view — the view is only known
/// there — and where a selection set on the model by a caller is adopted.
#[derive(Clone, Default)]
pub struct EditState {
    anchor: Cell<usize>,
    caret: Cell<usize>,
    /// The model selection `(start, length)` this state last agreed with; a
    /// different one on the model was set by the caller and is adopted.
    seen: Cell<(i32, i32)>,
    scroll_x: Cell<f32>,
    scroll_y: Cell<f32>,
    /// The caret moved: scroll it into view at the next paint.
    reveal: Cell<bool>,
    /// The column Up/Down aim for, kept across a run of vertical moves.
    preferred_x: Option<f32>,
    drag: Option<Drag>,
    prev_down: bool,
    prev_right: bool,
    history: History,
    last_input_ms: u64,
    window_blurred: bool,
    had_focus: bool,
    /// The pointer is over the search ✕ (it lights `hover:text-text-primary`).
    clear_hot: Cell<bool>,
    menu: Option<MenuState>,
}

impl EditState {
    /// The caret position, in characters.
    pub fn caret(&self) -> usize {
        self.caret.get()
    }

    /// The selection anchor, in characters (`== caret` when nothing is selected).
    pub fn anchor(&self) -> usize {
        self.anchor.get()
    }

    /// The horizontal / vertical scroll of the text, in DIP.
    pub fn scroll(&self) -> (f32, f32) {
        (self.scroll_x.get(), self.scroll_y.get())
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Whether the text context menu is open.
    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// The open context menu's panel, in the field's (client) space.
    pub fn menu_bounds(&self) -> Option<Rect> {
        self.menu.and_then(|m| m.panel)
    }

    /// Adopts the model's selection when the caller changed it, and clamps
    /// the caret to a text that may have shrunk.
    fn sync_from_model(&self, start: i32, len: i32, n: usize) {
        if self.seen.get() != (start, len) {
            self.anchor.set((start.max(0) as usize).min(n));
            self.caret.set(((start.max(0) + len.max(0)) as usize).min(n));
            self.seen.set((start, len));
            self.reveal.set(true);
        }
        self.anchor.set(self.anchor.get().min(n));
        self.caret.set(self.caret.get().min(n));
    }

    fn buffer(&self, text: &str) -> EditBuffer {
        EditBuffer::new(text, self.anchor.get(), self.caret.get())
    }

    fn set_selection(&self, anchor: usize, caret: usize) {
        if self.caret.get() != caret || self.anchor.get() != anchor {
            self.reveal.set(true);
        }
        self.anchor.set(anchor);
        self.caret.set(caret);
    }

    fn selection(&self) -> (usize, usize) {
        let (a, c) = (self.anchor.get(), self.caret.get());
        (a.min(c), a.max(c))
    }

    /// Restarts the blink (solid caret while the user acts).
    fn touch(&mut self) {
        self.last_input_ms = host::now_ms();
    }
}

// ── Layout of the text inside a field ────────────────────────────────────────

/// Where a field lays out its text.
#[derive(Clone, Copy)]
struct TextGeom {
    /// The content box the rows are laid out in (single-line: the line is
    /// centred in it vertically; multiline: rows start at its top).
    text: Rect,
    /// Where text drawing is clipped.
    clip: Rect,
    /// Where a press starts editing (the field minus its buttons).
    hit: Rect,
    multiline: bool,
    wrap: bool,
    align: HorizontalAlignment,
}

/// The rows of a field's text, with their widths, at its current size.
struct Laid {
    lines: Vec<VisualLine>,
    widths: Vec<f32>,
    /// The width rows are wrapped to (the text box minus a scroll bar when one
    /// is needed).
    view_w: f32,
    view_h: f32,
    content_h: f32,
    /// A multiline field whose rows overflow: it shows a scroll bar.
    overflow_y: bool,
}

fn measure_body(c: &dyn Canvas, s: &str) -> f32 {
    c.measure(s, &c.formats().body)
}

fn lay_out(c: &dyn Canvas, display: &str, g: &TextGeom) -> Laid {
    let m = |s: &str| measure_body(c, s);
    let full_w = (g.text.right - g.text.left).max(0.0);
    let view_h = (g.text.bottom - g.text.top).max(0.0);
    if !g.multiline {
        let n = char_count(display);
        return Laid {
            lines: vec![VisualLine { start: 0, end: n }],
            widths: vec![m(display)],
            view_w: full_w,
            view_h,
            content_h: LINE,
            overflow_y: false,
        };
    }
    let wrap_at = |w: f32| wrap_lines(display, g.wrap.then_some(w), &m);
    let mut view_w = full_w;
    let mut lines = wrap_at(view_w);
    let mut overflow_y = lines.len() as f32 * LINE > view_h + 0.5;
    if overflow_y {
        // The scroll bar takes its gutter out of the row width (a classic,
        // non-overlay bar, as Chromium draws on Windows).
        view_w = (full_w - control::SCROLLBAR).max(0.0);
        lines = wrap_at(view_w);
        overflow_y = lines.len() as f32 * LINE > view_h + 0.5;
    }
    let widths = lines.iter().map(|l| m(char_slice(display, l.start, l.end))).collect();
    let content_h = lines.len() as f32 * LINE;
    Laid { lines, widths, view_w, view_h, content_h, overflow_y }
}

impl Laid {
    /// The left edge of row `i`'s text, before scrolling: alignment inside the
    /// view (only when the row fits — an overflowing row starts at the left
    /// and scrolls, as an `<input>` does).
    fn align_offset(&self, i: usize, align: HorizontalAlignment) -> f32 {
        let w = self.widths.get(i).copied().unwrap_or(0.0);
        let room = self.view_w - w - edit::CARET_W;
        if room <= 0.0 {
            return 0.0;
        }
        match align {
            HorizontalAlignment::Left => 0.0,
            HorizontalAlignment::Center => room / 2.0,
            HorizontalAlignment::Right => room,
        }
    }

    fn max_scroll_x(&self) -> f32 {
        let w = self.widths.first().copied().unwrap_or(0.0);
        (w + edit::CARET_W - self.view_w).max(0.0)
    }

    fn max_scroll_y(&self) -> f32 {
        (self.content_h - self.view_h).max(0.0)
    }
}

/// The top of row `i` on screen.
fn row_top(g: &TextGeom, i: usize, scroll_y: f32) -> f32 {
    if g.multiline {
        g.text.top + i as f32 * LINE - scroll_y
    } else {
        (g.text.top + g.text.bottom) / 2.0 - LINE / 2.0
    }
}

/// The x (relative to the row's origin) of position `pos` in `line`.
fn x_in_line(c: &dyn Canvas, display: &str, line: VisualLine, pos: usize) -> f32 {
    let p = pos.clamp(line.start, line.end);
    measure_body(c, char_slice(display, line.start, p))
}

/// The position in `line` nearest to `x` (relative to the row's origin), or —
/// with `floor` — the character whose span contains `x` (what a double-click
/// picks a word by).
fn index_in_line(c: &dyn Canvas, display: &str, line: VisualLine, x: f32, floor: bool) -> usize {
    if x <= 0.0 {
        return line.start;
    }
    let mut prev = 0.0_f32;
    for k in line.start + 1..=line.end {
        let w = measure_body(c, char_slice(display, line.start, k));
        if w >= x {
            if floor {
                return k - 1;
            }
            return if w - x < x - prev { k } else { k - 1 };
        }
        prev = w;
    }
    line.end
}

/// The row under screen `y`.
fn row_at(g: &TextGeom, laid: &Laid, y: f32, scroll_y: f32) -> usize {
    if !g.multiline || laid.lines.len() <= 1 {
        return 0;
    }
    let r = ((y - g.text.top + scroll_y) / LINE).floor();
    (r.max(0.0) as usize).min(laid.lines.len() - 1)
}

/// The text position under the screen point `(x, y)`.
#[allow(clippy::too_many_arguments)]
fn pos_at(c: &dyn Canvas, display: &str, g: &TextGeom, laid: &Laid, st: &EditState, x: f32, y: f32, floor: bool) -> usize {
    let (sx, sy) = st.scroll();
    let i = row_at(g, laid, y, sy);
    let line = laid.lines[i];
    let origin = g.text.left + laid.align_offset(i, g.align) - if g.multiline { 0.0 } else { sx };
    index_in_line(c, display, line, x - origin, floor)
}

// ── Painting ─────────────────────────────────────────────────────────────────

/// What the edge of a field looks like, resolved from the state.
struct Edge {
    radius: f32,
    disabled: bool,
    focused: bool,
    invalid: bool,
}

/// `colour` faded to `DISABLED_ALPHA` — `disabled:opacity-60`.
fn faded(colour: &D2D1_COLOR_F) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: colour.a * DISABLED_ALPHA, ..*colour }
}

/// Ground, border and focus outline — the whole of a box field's chrome.
fn paint_edge(c: &dyn Canvas, bounds: Rect, edge: &Edge, ground: &D2D1_COLOR_F) {
    let t = c.theme();
    let ground = if edge.disabled { &t.surface_2 } else { ground };
    c.fill_rounded(&bounds, edge.radius, ground);

    // `error ? border-danger : border-border`; focused, the border turns
    // primary (danger) under the outline, which covers it entirely.
    let line = if edge.invalid { &t.danger } else { &t.card_stroke };
    if edge.disabled {
        c.stroke_rounded(&bounds, edge.radius, &faded(line));
    } else if edge.focused {
        let ring = if edge.invalid { &t.danger } else { &t.accent };
        c.stroke_rounded_w(&bounds, edge.radius, ring, FOCUS_OUTLINE);
    } else {
        c.stroke_rounded(&bounds, edge.radius, line);
    }
}

/// The icon columns. Both are `text-text-secondary` in the web.
fn paint_icons(c: &dyn Canvas, bounds: Rect, leading: Option<&'static str>, trailing: Option<&'static str>, disabled: bool) {
    let t = c.theme();
    let colour = if disabled { faded(&t.text_secondary) } else { t.text_secondary };
    if let Some(name) = leading {
        c.vector_icon(name, &icon_rect(bounds, Side::Leading), ICON, &colour);
    }
    if let Some(name) = trailing {
        c.vector_icon(name, &icon_rect(bounds, Side::Trailing), ICON, &colour);
    }
}

/// How a field's text is painted this frame.
#[derive(Clone, Copy)]
struct TextPaint<'a> {
    display: &'a str,
    placeholder: &'a str,
    focused: bool,
    disabled: bool,
    /// Focused, editable: a caret is drawn.
    caret: bool,
    /// `HideSelection = false`: the selection stays visible while the field does not have the focus.
    keep_selection: bool,
}

/// The text of a field: placeholder, or the rows with their selection and
/// caret, scrolled and clipped to the content box — and a scroll bar on an
/// overflowing multiline field.
fn paint_text(c: &dyn Canvas, st: &EditState, g: &TextGeom, p: TextPaint) {
    let t = c.theme();
    let f = c.formats();
    let n = char_count(p.display);

    if p.display.is_empty() {
        st.scroll_x.set(0.0);
        st.scroll_y.set(0.0);
        if !p.placeholder.is_empty() {
            // `placeholder:text-text-tertiary`, on the first row.
            let colour = if p.disabled { faded(&t.text_tertiary) } else { t.text_tertiary };
            let top = row_top(g, 0, 0.0);
            let row = Rect::new(g.text.left, top, g.text.right, top + LINE);
            match g.align {
                HorizontalAlignment::Left => c.text_ellipsis(p.placeholder, &row, &f.body, &colour),
                a => c.text_aligned(p.placeholder, &row, &f.body, &colour, a.dwrite()),
            }
        }
        if p.caret {
            paint_caret(c, st, g, 0, row_top(g, 0, 0.0), &t.text_primary);
        }
        return;
    }

    let ink = if p.disabled { faded(&t.text_primary) } else { t.text_primary };

    // An unfocused single-line field rests at its start and ellipsises what
    // does not fit (Chromium drops the scroll on blur) — unless it keeps its
    // selection visible (`HideSelection = false`) and has one.
    let (sel_a, sel_b) = st.selection();
    let kept = p.keep_selection && !p.disabled && sel_a != sel_b;
    if !g.multiline && !p.focused && !kept {
        st.scroll_x.set(0.0);
        let top = row_top(g, 0, 0.0);
        let row = Rect::new(g.text.left, top, g.text.right, top + LINE);
        match g.align {
            HorizontalAlignment::Left => c.text_ellipsis(p.display, &row, &f.body, &ink),
            a => c.text_aligned(p.display, &row, &f.body, &ink, a.dwrite()),
        }
        return;
    }

    let laid = lay_out(c, p.display, g);
    let caret = st.caret.get().min(n);
    let caret_row = line_of(&laid.lines, caret);

    // Scroll: keep what the user scrolled to, and move only as far as needed
    // to show a caret that moved (an `<input>`'s `scrollLeft`).
    let (mut sx, mut sy) = st.scroll();
    if g.multiline {
        if st.reveal.get() && p.focused {
            let cy = caret_row as f32 * LINE;
            if cy < sy {
                sy = cy;
            } else if cy + LINE > sy + laid.view_h {
                sy = cy + LINE - laid.view_h;
            }
        }
        sy = sy.clamp(0.0, laid.max_scroll_y());
        sx = 0.0;
    } else {
        if st.reveal.get() && p.focused {
            let cx = laid.align_offset(0, g.align) + x_in_line(c, p.display, laid.lines[0], caret);
            if cx < sx {
                sx = cx;
            } else if cx > sx + laid.view_w - edit::CARET_W {
                sx = cx - laid.view_w + edit::CARET_W;
            }
        }
        sx = sx.clamp(0.0, laid.max_scroll_x());
        sy = 0.0;
    }
    st.scroll_x.set(sx);
    st.scroll_y.set(sy);
    if p.focused {
        st.reveal.set(false);
    }

    let show_selection = (p.focused || kept) && sel_a != sel_b;
    let mut band = t.accent;
    band.a = edit::SELECTION_ALPHA;
    if st.window_blurred {
        // Chromium greys the highlight of an inactive window.
        band = t.text_tertiary;
        band.a = edit::SELECTION_ALPHA;
    }

    c.push_clip(&g.clip);
    for (i, line) in laid.lines.iter().enumerate() {
        let top = row_top(g, i, sy);
        if top + LINE < g.clip.top || top > g.clip.bottom {
            continue;
        }
        let origin = g.text.left + laid.align_offset(i, g.align) - sx;
        if show_selection && sel_a <= line.end && sel_b >= line.start {
            let a = sel_a.max(line.start);
            let b = sel_b.min(line.end);
            let x0 = origin + x_in_line(c, p.display, *line, a);
            let mut x1 = origin + x_in_line(c, p.display, *line, b);
            // The selection runs on past this row (over its line break or a
            // soft wrap): the browser paints a space's worth after the text.
            if sel_b > line.end {
                x1 += measure_body(c, " ");
            }
            if x1 > x0 {
                c.fill_rounded(&Rect::new(x0, top, x1, top + LINE), 0.0, &band);
            }
        }
        let s = char_slice(p.display, line.start, line.end);
        if !s.is_empty() {
            let w = laid.widths.get(i).copied().unwrap_or(0.0);
            // Wide enough that DirectWrite never wraps the row itself.
            let r = Rect::new(origin, top, origin + w + LINE, top + LINE);
            c.text(s, &r, &f.body, &ink, false);
        }
    }
    if p.caret && !show_selection {
        let line = laid.lines[caret_row];
        let x = g.text.left + laid.align_offset(caret_row, g.align) - sx + x_in_line(c, p.display, line, caret);
        paint_caret_at(c, st, x, row_top(g, caret_row, sy), &t.text_primary);
    }
    c.pop_clip();

    if g.multiline && laid.overflow_y {
        paint_scrollbar(c, g, &laid, sy);
    }
}

/// The caret at position `pos` of row `row` of an EMPTY field.
fn paint_caret(c: &dyn Canvas, st: &EditState, g: &TextGeom, _pos: usize, top: f32, colour: &D2D1_COLOR_F) {
    let x = match g.align {
        HorizontalAlignment::Left => g.text.left,
        HorizontalAlignment::Center => (g.text.left + g.text.right) / 2.0,
        HorizontalAlignment::Right => g.text.right - edit::CARET_W,
    };
    paint_caret_at(c, st, x, top, colour);
}

/// The blinking caret: only in an active window, in its visible half-period
/// (`caret_visible` schedules the repaint that toggles it).
fn paint_caret_at(c: &dyn Canvas, st: &EditState, x: f32, top: f32, colour: &D2D1_COLOR_F) {
    if st.window_blurred || !caret_visible(st.last_input_ms) {
        return;
    }
    c.fill_rounded(&Rect::new(x, top, x + edit::CARET_W, top + LINE), 0.0, colour);
}

/// A thin thumb in the scroll-bar gutter of an overflowing multiline field.
fn paint_scrollbar(c: &dyn Canvas, g: &TextGeom, laid: &Laid, sy: f32) {
    let t = c.theme();
    // `g.clip` is the inside of the field's `rounded-md` border: the gutter
    // keeps clear of its rounded corners.
    let corner = (radius::SM - 1.0).max(0.0);
    let gutter = crate::range::fit_rail(
        Rect::new(g.clip.right - control::SCROLLBAR, g.clip.top, g.clip.right, g.clip.bottom),
        g.clip,
        corner,
    );
    let track_h = gutter.bottom - gutter.top;
    if track_h <= 0.0 || laid.content_h <= 0.0 {
        return;
    }
    let thumb_h = (track_h * laid.view_h / laid.content_h).clamp(control::SCROLLBAR_THUMB_MIN.min(track_h), track_h);
    let max = laid.max_scroll_y();
    let frac = if max > 0.0 { sy / max } else { 0.0 };
    let top = gutter.top + (track_h - thumb_h) * frac;
    let cx = (gutter.left + gutter.right) / 2.0;
    let half = control::SCROLLBAR_THUMB / 2.0;
    let thumb = Rect::new(cx - half, top, cx + half, top + thumb_h);
    c.push_clip_rounded(&g.clip, corner);
    c.fill_rounded(&thumb, half, &t.scrollbar_thumb);
    c.pop_clip_rounded();
}

// ── The shared editor over a `TextBox` ───────────────────────────────────────

/// A field's editing rules, resolved from its model.
#[derive(Debug, Clone, Copy)]
struct Spec {
    read_only: bool,
    disabled: bool,
    max_len: Option<usize>,
    password: bool,
    search: bool,
    multiline: bool,
    /// `AcceptsTab` on a multiline field: Tab types a tab character (the focus ring then leaves Tab to
    /// the field — register it with `FocusOpts::wants_tab`).
    accepts_tab: bool,
    /// Enter in a multiline field is a submit, not a new line (WinForms `AcceptsReturn = false`:
    /// the form's default button gets it). See [`TextField::enter_submits`].
    enter_submits: bool,
}

impl Spec {
    fn of(tb: &TextBox, disabled: bool, search: bool) -> Self {
        let password = !tb.multiline && (tb.use_system_password_char || tb.password_char.is_some());
        Self {
            read_only: tb.read_only,
            disabled: disabled || !tb.enabled,
            max_len: (tb.max_length > 0).then_some(tb.max_length as usize),
            password,
            search,
            multiline: tb.multiline,
            accepts_tab: tb.multiline && tb.accepts_tab,
            enter_submits: false,
        }
    }

    fn editable(&self) -> bool {
        !self.read_only && !self.disabled
    }
}

/// The characters drawn for a plain text box: the system glyph wins over
/// `PasswordChar`, and masking is a single-line concern (WinForms ignores it
/// when `Multiline`).
fn display_of(t: &TextBox) -> String {
    let glyph = if t.use_system_password_char { Some(PASSWORD_GLYPH) } else { t.password_char };
    match glyph {
        Some(g) if !t.multiline => g.to_string().repeat(t.text().chars().count()),
        _ => t.text().to_string(),
    }
}

/// Writes an edited buffer back into the model: `TextBox::set_text` applies
/// `CharacterCasing` and `MaxLength`, and this is a USER edit, so `Modified`.
fn commit(tb: &mut TextBox, st: &EditState, buf: &mut EditBuffer) {
    tb.set_text(&buf.text);
    tb.modified = true;
    buf.text = tb.text().to_string();
    let n = buf.len();
    buf.anchor = buf.anchor.min(n);
    buf.caret = buf.caret.min(n);
    write_selection(tb, st, buf.anchor, buf.caret);
}

fn write_selection(tb: &mut TextBoxBase, st: &EditState, anchor: usize, caret: usize) {
    let (a, b) = (anchor.min(caret), anchor.max(caret));
    tb.select(a as i32, (b - a) as i32);
    st.seen.set((tb.selection_start(), tb.selection_length()));
    st.set_selection(anchor, caret);
}

/// The press / drag / keys / menu of a `TextBox`-backed field, for one frame.
fn update_box(
    tb: &mut TextBox,
    st: &mut EditState,
    c: &dyn Canvas,
    g: &TextGeom,
    spec: Spec,
    input: &EditInput,
    clear: Option<Rect>,
) -> EditOutcome {
    let mut out = EditOutcome::default();

    // A textarea value is `\n`-separated; adopt that once, so every row and
    // every caret position is a real character.
    if spec.multiline && tb.text().contains('\r') {
        let modified = tb.modified;
        let normalised = sanitize_paste(tb.text(), true);
        tb.set_text(&normalised);
        tb.modified = modified;
    }
    let n = char_count(tb.text());
    st.sync_from_model(tb.selection_start(), tb.selection_length(), n);

    let pressed = input.mouse_down && !st.prev_down;
    let right_pressed = input.right_down && !st.prev_right;
    st.prev_down = input.mouse_down;
    st.prev_right = input.right_down;
    st.window_blurred = !input.window_focused;
    let (mx, my) = input.mouse;

    // Focus transitions.
    if st.had_focus && !input.focused {
        st.drag = None;
        st.menu = None;
        st.preferred_x = None;
        if !spec.multiline {
            st.scroll_x.set(0.0);
        }
    }
    if input.focused && !st.had_focus {
        st.touch();
        st.reveal.set(true);
        // Tab into an `<input>` selects its whole value; a click places the
        // caret instead (handled below).
        if !pressed && !right_pressed && !spec.multiline {
            write_selection(tb, st, 0, n);
        }
    }
    st.had_focus = input.focused;

    if spec.disabled {
        st.menu = None;
        st.drag = None;
        if g.hit.contains(mx, my) {
            host::set_cursor(Cursor::NotAllowed); // `disabled:cursor-not-allowed`
        }
        return out;
    }

    let flags = MenuFlags {
        editable: spec.editable(),
        has_selection: st.anchor.get() != st.caret.get(),
        copy_allowed: !spec.password,
        can_undo: st.history.can_undo(),
        can_redo: st.history.can_redo(),
    };

    // The open menu takes the pointer and the keys first, like the web's
    // backdrop: a click anywhere closes it, on a row it also runs the row.
    let mut swallowed = false;
    if st.menu.is_some() {
        let (action, took) = drive_menu(st, c, input, flags, pressed, right_pressed);
        swallowed = took;
        if let Some(a) = action {
            if run_action(tb, st, spec, a) {
                out.changed = true;
            }
        }
    }

    let display = display_of(tb);
    let laid = lay_out(c, &display, g);

    if !swallowed {
        // The search ✕ — `onMouseDown={e => { e.preventDefault(); handleChange('') }}`.
        let over_clear = clear.is_some_and(|r| r.contains(mx, my));
        st.clear_hot.set(over_clear);
        if over_clear && pressed && !tb.text().is_empty() {
            let mut buf = st.buffer(tb.text());
            let before = Snapshot::of(&buf);
            buf.select_all();
            if buf.insert("", None) {
                st.history.record(before, EditKind::Other, host::now_ms());
                commit(tb, st, &mut buf);
                out.changed = true;
                out.cleared = true;
            }
            st.touch();
        } else if pressed && g.hit.contains(mx, my) {
            let text = tb.text().to_string();
            let pos = pos_at(c, &display, g, &laid, st, mx, my, false);
            st.preferred_x = None;
            match input.click_count {
                2 => {
                    let under = pos_at(c, &display, g, &laid, st, mx, my, true);
                    let (a, b) = word_range_at(&text, under);
                    write_selection(tb, st, a, b);
                    st.drag = Some(Drag::Word(a, b));
                }
                c3 if c3 >= 3 => {
                    let (a, b) = if spec.multiline { paragraph_range_at(&text, pos) } else { (0, n) };
                    write_selection(tb, st, a, b);
                    st.drag = Some(Drag::Line(a, b));
                }
                _ => {
                    let anchor = if input.mods.shift { st.anchor.get() } else { pos };
                    write_selection(tb, st, anchor, pos);
                    st.drag = Some(Drag::Char);
                }
            }
            st.touch();
        } else if right_pressed && g.hit.contains(mx, my) && !over_clear {
            // A right click outside the selection moves the caret there first,
            // so the menu acts on what was clicked.
            let pos = pos_at(c, &display, g, &laid, st, mx, my, false);
            let (a, b) = st.selection();
            if !(a < b && pos >= a && pos <= b) {
                write_selection(tb, st, pos, pos);
            }
            open_menu(st, input.mouse);
            st.touch();
        }

        // A held drag extends the selection, by the unit it started with, and
        // keeps scrolling while the pointer is past the text's edge.
        if input.mouse_down {
            if let Some(d) = st.drag {
                let text = tb.text().to_string();
                let pos = pos_at(c, &display, g, &laid, st, mx, my, false);
                let (anchor, caret) = match d {
                    Drag::Char => (st.anchor.get(), pos),
                    Drag::Word(a, b) => {
                        let (wa, wb) = word_range_at(&text, pos_at(c, &display, g, &laid, st, mx, my, true));
                        if wa < a { (b, wa) } else { (a, wb.max(b)) }
                    }
                    Drag::Line(a, b) => {
                        let (pa, pb) = if spec.multiline { paragraph_range_at(&text, pos) } else { (0, n) };
                        if pa < a { (b, pa) } else { (a, pb.max(b)) }
                    }
                };
                write_selection(tb, st, anchor, caret);
                let outside = mx < g.text.left || mx > g.text.right || my < g.text.top || my > g.text.bottom;
                if outside {
                    if g.multiline {
                        // Scroll the rows towards the pointer.
                        let (_, sy) = st.scroll();
                        let dy = if my < g.text.top { my - g.text.top } else if my > g.text.bottom { my - g.text.bottom } else { 0.0 };
                        st.scroll_y.set((sy + dy.clamp(-LINE, LINE)).clamp(0.0, laid.max_scroll_y()));
                    }
                    host::request_repaint_after(edit::AUTOSCROLL_MS);
                }
            }
        } else {
            st.drag = None;
        }

        // The wheel scrolls a multiline field; a single-line one scrolls
        // sideways with the horizontal wheel or Shift+wheel.
        if g.clip.contains(mx, my) && (input.wheel.0 != 0.0 || input.wheel.1 != 0.0) {
            let (sx, sy) = st.scroll();
            if g.multiline && !input.mods.shift {
                st.scroll_y.set((sy + input.wheel.1 * host::WHEEL_NOTCH_DIP).clamp(0.0, laid.max_scroll_y()));
            } else if !g.multiline {
                let d = if input.mods.shift { input.wheel.1 } else { input.wheel.0 };
                st.scroll_x.set((sx + d * host::WHEEL_NOTCH_DIP).clamp(0.0, laid.max_scroll_x()));
            }
        }

        if st.drag.is_some() || (g.hit.contains(mx, my) && !over_clear) {
            host::set_cursor(Cursor::IBeam);
        }
    }

    // Keys, in the order they were typed.
    if input.focused && st.menu.is_none() {
        let events = host::consume(|e| match e {
            InputEvent::Text(_) => true,
            InputEvent::Key { vk: k, down: true, mods, .. } if *k == vk::TAB => spec.accepts_tab && mods.is_none(),
            InputEvent::Key { vk: k, down: true, mods, .. } => match command_for(*k, *mods) {
                Some((EditCommand::PageUp | EditCommand::PageDown, _)) => spec.multiline,
                Some(_) => true,
                None => false,
            },
            _ => false,
        });
        for e in events {
            let step = match e {
                InputEvent::Text(s) => type_text(tb, st, spec, &s),
                InputEvent::Key { vk: k, .. } if k == vk::TAB => type_text(tb, st, spec, "\t"),
                InputEvent::Key { vk: k, mods, .. } => match command_for(k, mods) {
                    Some((cmd, extend)) => run_command(tb, st, c, g, spec, cmd, extend, input),
                    None => KeyStep::default(),
                },
                _ => KeyStep::default(),
            };
            out.changed |= step.changed;
            out.submitted |= step.submitted;
        }
        // Escape: `type=search` clears a non-empty field (and uses the key);
        // any other field leaves it to the page, and says so.
        if spec.search && !tb.text().is_empty() && spec.editable() && host::take_key(vk::ESCAPE, Modifiers::NONE) > 0 {
            let mut buf = st.buffer(tb.text());
            let before = Snapshot::of(&buf);
            buf.select_all();
            buf.insert("", None);
            st.history.record(before, EditKind::Other, host::now_ms());
            commit(tb, st, &mut buf);
            out.changed = true;
            out.cleared = true;
            st.touch();
        } else if host::key_pressed(vk::ESCAPE, Modifiers::NONE) {
            out.escaped = true;
        }
    }

    // The menu may have been opened by a key this frame.
    if st.menu.is_some() {
        let flags = MenuFlags {
            has_selection: st.anchor.get() != st.caret.get(),
            can_undo: st.history.can_undo(),
            can_redo: st.history.can_redo(),
            ..flags
        };
        show_menu(st, c, input, flags);
    }
    out.menu_open = st.menu.is_some();
    out
}

/// The result of one key.
#[derive(Default)]
struct KeyStep {
    changed: bool,
    submitted: bool,
}

/// Typed text replaces the selection (`maxlength` honoured).
fn type_text(tb: &mut TextBox, st: &mut EditState, spec: Spec, s: &str) -> KeyStep {
    if !spec.editable() {
        return KeyStep::default();
    }
    let s = sanitize_paste(s, spec.multiline);
    let mut buf = st.buffer(tb.text());
    let before = Snapshot::of(&buf);
    let replaced = buf.has_selection();
    let changed = buf.insert(&s, spec.max_len);
    if changed {
        let kind = if replaced { EditKind::Other } else { EditKind::Typing };
        st.history.record(before, kind, host::now_ms());
        commit(tb, st, &mut buf);
    }
    st.preferred_x = None;
    st.touch();
    KeyStep { changed, submitted: false }
}

/// Runs one key command.
#[allow(clippy::too_many_arguments)]
fn run_command(
    tb: &mut TextBox,
    st: &mut EditState,
    c: &dyn Canvas,
    g: &TextGeom,
    spec: Spec,
    cmd: EditCommand,
    extend: bool,
    input: &EditInput,
) -> KeyStep {
    use EditCommand::*;
    st.touch();
    let text = tb.text().to_string();
    let n = char_count(&text);
    let mut buf = st.buffer(&text);
    let (a, b) = buf.selection();
    let vertical = matches!(cmd, Up | Down | PageUp | PageDown);
    if !vertical {
        st.preferred_x = None;
    }
    let mut step = KeyStep::default();
    let move_to = |tb: &mut TextBox, st: &EditState, buf: &EditBuffer, pos: usize| {
        let anchor = if extend { buf.anchor } else { pos };
        write_selection(tb, st, anchor, pos.min(n));
    };
    match cmd {
        Left => {
            let p = if !extend && buf.has_selection() { a } else { buf.caret.saturating_sub(1) };
            move_to(tb, st, &buf, p);
        }
        Right => {
            let p = if !extend && buf.has_selection() { b } else { (buf.caret + 1).min(n) };
            move_to(tb, st, &buf, p);
        }
        WordLeft => move_to(tb, st, &buf, word_left(&text, buf.caret)),
        WordRight => move_to(tb, st, &buf, word_right(&text, buf.caret)),
        DocStart => move_to(tb, st, &buf, 0),
        DocEnd => move_to(tb, st, &buf, n),
        LineStart | LineEnd => {
            let display = display_of(tb);
            let laid = lay_out(c, &display, g);
            let i = line_of(&laid.lines, buf.caret);
            let line = laid.lines[i];
            let p = if cmd == LineStart {
                line.start
            } else {
                // On a soft-wrapped row, stop before the space the row broke
                // at, so the caret stays on this row.
                let soft = laid.lines.get(i + 1).is_some_and(|nx| nx.start == line.end);
                let last = text.chars().nth(line.end.saturating_sub(1));
                if soft && line.end > line.start && last.is_some_and(char::is_whitespace) {
                    line.end - 1
                } else {
                    line.end
                }
            };
            move_to(tb, st, &buf, p);
        }
        Up | Down | PageUp | PageDown => {
            if !spec.multiline {
                // Up / Down in an `<input>` go to its ends.
                move_to(tb, st, &buf, if cmd == Up { 0 } else { n });
            } else {
                let display = display_of(tb);
                let laid = lay_out(c, &display, g);
                let i = line_of(&laid.lines, buf.caret);
                let rows = ((laid.view_h / LINE).floor() as usize).max(1);
                let target: isize = match cmd {
                    Up => i as isize - 1,
                    Down => i as isize + 1,
                    PageUp => i as isize - rows as isize,
                    _ => i as isize + rows as isize,
                };
                let x = st.preferred_x.unwrap_or_else(|| {
                    laid.align_offset(i, g.align) + x_in_line(c, &display, laid.lines[i], buf.caret)
                });
                let p = if target < 0 {
                    0
                } else if target as usize >= laid.lines.len() {
                    n
                } else {
                    let t = target as usize;
                    index_in_line(c, &display, laid.lines[t], x - laid.align_offset(t, g.align), false)
                };
                move_to(tb, st, &buf, p);
                st.preferred_x = Some(x);
            }
        }
        SelectAll => write_selection(tb, st, 0, n),
        Backspace | BackspaceWord | Delete | DeleteWord => {
            if spec.editable() {
                let before = Snapshot::of(&buf);
                let changed = match cmd {
                    Backspace => buf.delete_back(false),
                    BackspaceWord => buf.delete_back(true),
                    Delete => buf.delete_forward(false),
                    _ => buf.delete_forward(true),
                };
                if changed {
                    let kind = if before.anchor != before.caret { EditKind::Other } else { EditKind::Deleting };
                    st.history.record(before, kind, host::now_ms());
                    commit(tb, st, &mut buf);
                    step.changed = true;
                }
            }
        }
        Undo => step.changed = run_action(tb, st, spec, EditAction::Undo),
        Redo => step.changed = run_action(tb, st, spec, EditAction::Redo),
        Copy => {
            run_action(tb, st, spec, EditAction::Copy);
        }
        Cut => step.changed = run_action(tb, st, spec, EditAction::Cut),
        Paste => step.changed = run_action(tb, st, spec, EditAction::Paste),
        Enter => {
            // Shift+Enter (`extend`) always breaks a multi-line field, even one whose Enter submits (a chat composer).
            if spec.multiline && (!spec.enter_submits || extend) {
                if spec.editable() {
                    return type_text(tb, st, spec, "\n");
                }
            } else {
                step.submitted = true;
            }
        }
        ContextMenu => {
            // At the caret, just under its line box.
            let display = display_of(tb);
            let laid = lay_out(c, &display, g);
            let i = line_of(&laid.lines, buf.caret);
            let (sx, sy) = st.scroll();
            let x = g.text.left + laid.align_offset(i, g.align) - if g.multiline { 0.0 } else { sx }
                + x_in_line(c, &display, laid.lines[i], buf.caret);
            let y = row_top(g, i, sy) + LINE;
            open_menu(st, (x, y));
            let _ = input;
        }
    }
    step
}

/// Runs a context-menu action (also what Ctrl+Z / X / C / V do). Returns
/// whether the text changed.
fn run_action(tb: &mut TextBox, st: &mut EditState, spec: Spec, action: EditAction) -> bool {
    st.touch();
    let mut buf = st.buffer(tb.text());
    let now = host::now_ms();
    match action {
        EditAction::Undo | EditAction::Redo => {
            if !spec.editable() {
                return false;
            }
            let current = Snapshot::of(&buf);
            let restored = if action == EditAction::Undo { st.history.undo(current) } else { st.history.redo(current) };
            match restored {
                Some(s) => {
                    let mut b = EditBuffer::new(s.text, s.anchor, s.caret);
                    commit(tb, st, &mut b);
                    true
                }
                None => false,
            }
        }
        EditAction::Copy => {
            if !spec.password && buf.has_selection() {
                clipboard_set(buf.selected_text());
            }
            false
        }
        EditAction::Cut => {
            if spec.password || !spec.editable() || !buf.has_selection() {
                return false;
            }
            clipboard_set(buf.selected_text());
            let before = Snapshot::of(&buf);
            buf.insert("", None);
            st.history.record(before, EditKind::Other, now);
            commit(tb, st, &mut buf);
            true
        }
        EditAction::Paste => {
            if !spec.editable() {
                return false;
            }
            let Some(clip) = clipboard_get() else { return false };
            let clip = sanitize_paste(&clip, spec.multiline);
            let before = Snapshot::of(&buf);
            if buf.insert(&clip, spec.max_len) {
                st.history.record(before, EditKind::Other, now);
                commit(tb, st, &mut buf);
                true
            } else {
                false
            }
        }
        EditAction::Delete => {
            if !spec.editable() || !buf.has_selection() {
                return false;
            }
            let before = Snapshot::of(&buf);
            buf.insert("", None);
            st.history.record(before, EditKind::Other, now);
            commit(tb, st, &mut buf);
            true
        }
        EditAction::SelectAll => {
            let n = buf.len();
            write_selection(tb, st, 0, n);
            false
        }
    }
}

/// Opens the context menu for the point `at` (client space).
fn open_menu(st: &mut EditState, at: (f32, f32)) {
    st.menu = Some(MenuState { at, panel: None, hot: None, last_mouse: (f32::NAN, f32::NAN) });
    st.drag = None;
}

/// One frame of an open menu: routes a press against LAST frame's panel (so
/// the open menu swallows it), and the arrows / Enter / Escape. Returns the
/// chosen action and whether the pointer press was swallowed.
fn drive_menu(
    st: &mut EditState,
    c: &dyn Canvas,
    input: &EditInput,
    flags: MenuFlags,
    pressed: bool,
    right_pressed: bool,
) -> (Option<EditAction>, bool) {
    let Some(mut m) = st.menu else { return (None, false) };
    let _ = c;
    if input.dismiss || !input.focused {
        st.menu = None;
        return (None, false);
    }
    let (menu, actions) = text_menu(flags);
    let (mx, my) = input.mouse;
    let mut action = None;
    let mut close = false;
    let mut swallowed = false;
    if let Some(panel) = m.panel {
        // Hover follows the pointer when it moves over the panel.
        if m.last_mouse != input.mouse {
            m.last_mouse = input.mouse;
            if panel.contains(mx, my) {
                m.hot = menu.item_at(panel, mx, my).filter(|&i| menu_row_enabled(&menu, &actions, i));
            }
        }
        if pressed || right_pressed {
            swallowed = true;
            close = true;
            if panel.contains(mx, my) {
                match menu.item_at(panel, mx, my) {
                    Some(i) if pressed && menu_row_enabled(&menu, &actions, i) => {
                        action = actions[i];
                    }
                    // A dead row, a separator or the padding: nothing, and
                    // the menu stays.
                    _ => close = false,
                }
            }
        }
    }
    // Keyboard: the menu reads the same queue as the field.
    for _ in 0..host::take_key(vk::DOWN, Modifiers::NONE) {
        m.hot = menu_step(&menu, &actions, m.hot, true);
    }
    for _ in 0..host::take_key(vk::UP, Modifiers::NONE) {
        m.hot = menu_step(&menu, &actions, m.hot, false);
    }
    if host::take_key(vk::HOME, Modifiers::NONE) > 0 {
        m.hot = menu_step(&menu, &actions, None, true);
    }
    if host::take_key(vk::END, Modifiers::NONE) > 0 {
        m.hot = menu_step(&menu, &actions, None, false);
    }
    let enter = host::take_key(vk::ENTER, Modifiers::NONE) + host::take_key(vk::SPACE, Modifiers::NONE);
    if enter > 0 {
        if let Some(i) = m.hot {
            if menu_row_enabled(&menu, &actions, i) {
                action = actions[i];
                close = true;
            }
        }
    }
    if !host::take_key_any(vk::ESCAPE).is_empty() {
        close = true;
    }
    st.menu = if close { None } else { Some(m) };
    (action, swallowed)
}

/// Places the open menu against the screen and asks the host for its popup.
fn show_menu(st: &mut EditState, c: &dyn Canvas, input: &EditInput, flags: MenuFlags) {
    let Some(mut m) = st.menu else { return };
    let (mut menu, _) = text_menu(flags);
    let want = menu.measure(c);
    let size = (want.width.max(text_menu::MIN_WIDTH), want.height);
    let panel = menu_panel_at(m.at, size, input.screen);
    m.panel = Some(panel);
    menu.hot_index = m.hot;
    st.menu = Some(m);
    let s = text_menu::SHADOW;
    let pb = panel.inflate(s, s);
    let local = Rect::new(s, s, s + (panel.right - panel.left), s + (panel.bottom - panel.top));
    host::popup(pb, move |canvas| menu.paint(canvas, local, WidgetState::REST));
}

// ═════════════════════════════════════════════════════════════════════════════
// TextField
// ═════════════════════════════════════════════════════════════════════════════

/// A single-line Kubuno field — `@ui/Input` over
/// [`kubuno_desktop_controls::text::TextBox`].
///
/// Everything a caller sets on the *model* is the replica's:
///
/// ```ignore
/// let mut f = TextField::new();
/// f.set_text("hello");                   // TextBox::set_text — honours CharacterCasing
/// f.placeholder_text = "Serveur".into(); // TextBox::placeholder_text
/// f.read_only = true;                    // TextBoxBase::read_only
/// f.max_length = 64;                     // TextBoxBase::max_length (maxlength)
/// f.password_char = Some('*');           // TextBox::password_char
/// f.select(0, 5);                        // TextBoxBase::select — clamped
/// f.leading_icon = Some("Search");       // what Kubuno adds
/// ```
///
/// and it is edited by calling [`TextField::update`] every frame before
/// painting it (see the module documentation).
#[derive(Clone, Default)]
pub struct TextField {
    inner: TextBox,
    /// A glyph in the left column (`@ui/Input`'s `leftIcon`), by name from
    /// `assets/lucide-icons.txt`.
    pub leading_icon: Option<&'static str>,
    /// A glyph in the right column (`rightIcon`).
    pub trailing_icon: Option<&'static str>,
    /// `@ui/Input`'s `error`: the border and the focus outline turn `danger`.
    pub invalid: bool,
    /// A multiline field treats Enter as a submit ([`EditOutcome::submitted`]) instead of typing a
    /// new line — WinForms' `AcceptsReturn = false`, where Enter clicks the form's default button.
    /// `false` (the default) keeps the text area behaviour.
    pub enter_submits: bool,
    edit: EditState,
}

impl TextField {
    pub fn new() -> Self {
        Self::default()
    }

    /// The editing rules of this field.
    fn spec(&self) -> Spec {
        Spec { enter_submits: self.enter_submits, ..Spec::of(&self.inner, false, false) }
    }

    /// The characters drawn — the password glyph repeated, or the text.
    pub fn display(&self) -> String {
        display_of(&self.inner)
    }

    /// The rectangle the text occupies inside `bounds`, icon columns removed.
    pub fn content(&self, bounds: Rect) -> Rect {
        content_rect(bounds, self.leading_icon.is_some(), self.trailing_icon.is_some())
    }

    /// The editor's state: caret, scroll, undo availability, menu.
    pub fn edit_state(&self) -> &EditState {
        &self.edit
    }

    /// The open context menu's panel (client space) — hand it to
    /// `FocusRing::keep_focus_in` so a click on it does not blur the field.
    pub fn menu_bounds(&self) -> Option<Rect> {
        self.edit.menu_bounds()
    }

    /// Whether the text context menu is open.
    pub fn is_menu_open(&self) -> bool {
        self.edit.menu_open()
    }

    /// Closes the context menu (the page's own Escape or backdrop handling).
    pub fn close_menu(&mut self) {
        self.edit.menu = None;
    }

    /// Runs an edit action programmatically, as its menu row would. Returns
    /// whether the text changed.
    pub fn apply(&mut self, action: EditAction) -> bool {
        let spec = self.spec();
        run_action(&mut self.inner, &mut self.edit, spec, action)
    }

    fn geom(&self, bounds: Rect) -> TextGeom {
        let content = self.content(bounds);
        if self.multiline {
            // The rows start below `py-2`; the padding scrolls with them, so
            // the clip is the box inside its border.
            TextGeom {
                text: Rect::new(content.left, bounds.top + PAD_Y, content.right, (bounds.bottom - PAD_Y).max(bounds.top + PAD_Y)),
                clip: bounds.inflate(-1.0, -1.0),
                hit: bounds,
                multiline: true,
                wrap: self.word_wrap,
                align: self.text_align,
            }
        } else {
            TextGeom {
                text: content,
                clip: Rect::new(content.left, bounds.top, content.right, bounds.bottom),
                hit: bounds,
                multiline: false,
                wrap: false,
                align: self.text_align,
            }
        }
    }

    /// One frame of editing: pointer (click, drag, double / triple click,
    /// right-click menu, wheel), keys (typing, navigation, selection,
    /// clipboard, undo) and the I-beam cursor. Call it before
    /// [`Widget::paint`], with the bounds it will be painted at.
    pub fn update(&mut self, c: &dyn Canvas, bounds: Rect, input: &EditInput) -> EditOutcome {
        let g = self.geom(bounds);
        let spec = self.spec();
        update_box(&mut self.inner, &mut self.edit, c, &g, spec, input, None)
    }

    /// Makes the field behave as if it already held the focus: the next focused
    /// [`TextField::update`] is not a focus gain, so it keeps the model's
    /// selection instead of selecting the whole text (an editor that appears
    /// already focused — a table's cell editor — with its caret where it was put).
    pub(crate) fn assume_focused(&mut self) {
        self.edit.had_focus = true;
        self.edit.reveal.set(true);
        self.edit.touch();
    }

    /// Sets the text programmatically (the model's `set_text`) and forgets the
    /// undo history, as assigning `value` does in a browser.
    pub fn reset_text(&mut self, text: &str) {
        self.inner.set_text(text);
        self.edit.history.clear();
    }

    fn paint_field(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let disabled = state.disabled || !self.enabled;
        let edge = Edge { radius: radius::SM, disabled, focused: state.focused, invalid: self.invalid };
        paint_edge(canvas, bounds, &edge, &canvas.theme().layer_background);
        let display = self.display();
        self.edit.sync_from_model(self.selection_start(), self.selection_length(), char_count(&display));
        let focused = state.focused && !disabled;
        let p = TextPaint {
            display: &display,
            placeholder: &self.placeholder_text,
            focused,
            disabled,
            caret: focused && !self.read_only,
            keep_selection: !self.hide_selection,
        };
        paint_text(canvas, &self.edit, &self.geom(bounds), p);
        paint_icons(canvas, bounds, self.leading_icon, self.trailing_icon, disabled);
    }
}

impl Deref for TextField {
    type Target = TextBox;
    fn deref(&self) -> &TextBox {
        &self.inner
    }
}

impl DerefMut for TextField {
    fn deref_mut(&mut self) -> &mut TextBox {
        &mut self.inner
    }
}

impl Widget for TextField {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let display = self.display();
        let shown = if display.is_empty() { self.placeholder_text.as_str() } else { display.as_str() };
        let text_w = shown
            .split('\n')
            .map(|l| canvas.measure(l, &canvas.formats().body))
            .fold(0.0_f32, f32::max);
        let gutters = (if self.leading_icon.is_some() { ICON_COLUMN } else { PAD_X })
            + (if self.trailing_icon.is_some() { ICON_COLUMN } else { PAD_X });
        let rows = display.split('\n').count();
        // + the caret, so a field measured to its text does not scroll by one.
        let size = Size::new(
            text_w + edit::CARET_W + gutters + self.padding.horizontal(),
            field_height(self.multiline, rows) + self.padding.vertical(),
        );
        self.inner.clamp(size)
    }

    /// The field in `bounds`: its rounded ground is opaque, so nothing outside
    /// the rounded corners is painted (the parent shows there, as on the web).
    /// `state.focused` draws the focus outline and, when editable, the caret.
    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.paint_field(canvas, bounds, state);
    }

    fn type_name(&self) -> &'static str {
        "TextField"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// TextArea
// ═════════════════════════════════════════════════════════════════════════════

/// The multiline case of the same field — `@ui/Textarea`, which ships the
/// *same* class string as `@ui/Input` plus `h-36 min-h-16`.
///
/// It is a [`TextField`] with `Multiline` set: the chrome, the tokens and the
/// editing (including [`TextField::update`], reached through [`Deref`]) come
/// from there. Rows soft-wrap at the field's width (`word_wrap`, on by
/// default as in both WinForms and `<textarea>`), Enter inserts a line break,
/// and an overflowing text scrolls vertically with a scroll bar.
#[derive(Clone)]
pub struct TextArea {
    field: TextField,
}

impl Default for TextArea {
    fn default() -> Self {
        let mut field = TextField::new();
        field.multiline = true;
        Self { field }
    }
}

impl TextArea {
    pub fn new() -> Self {
        Self::default()
    }

    /// `@ui/Textarea`: `h-36` and `min-h-16`.
    pub const HEIGHT: f32 = TEXTAREA_HEIGHT;
    pub const MIN_HEIGHT: f32 = TEXTAREA_MIN_HEIGHT;

    /// The paragraphs the replica splits the text into (`TextBoxBase::lines`).
    pub fn rows(&self) -> usize {
        self.field.lines().len()
    }
}

impl Deref for TextArea {
    type Target = TextField;
    fn deref(&self) -> &TextField {
        &self.field
    }
}

impl DerefMut for TextArea {
    fn deref_mut(&mut self) -> &mut TextField {
        &mut self.field
    }
}

impl Widget for TextArea {
    fn model(&self) -> &dyn Control {
        self.field.model()
    }

    /// `h-36` — the height `@ui/Textarea` asks for, whatever it holds.
    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let inner = self.field.measure(canvas);
        self.field.clamp(Size::new(inner.width, Self::HEIGHT + self.padding.vertical()))
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.field.paint(canvas, bounds, state);
    }

    fn type_name(&self) -> &'static str {
        "TextArea"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// SearchField
// ═════════════════════════════════════════════════════════════════════════════

/// The search pill — `core/shell/SearchBar.tsx`'s search row in its `compact`
/// (`h-9`) form, without the module-specific buttons (voice, image, filters)
/// that composite adds after it.
///
/// ```text
/// pill: borderRadius 9999, background isActive ? '#ffffff' : var(--color-search-bg),
///       border 1px isActive ? '#e0e0e0' : transparent
/// row:  h-9 · pl-3 pr-2 <Search 16 text-text-secondary> · <input flex-1 min-w-0
///       bg-transparent outline-none placeholder:text-text-tertiary>
///       · {query && <button px-1 text-text-tertiary hover:text-text-primary><X 16/>}
///       · pr-2
/// ```
///
/// `isActive` is `focused`: the pill turns white and gains its border; there
/// is NO focus ring (`outline-none`). `--color-search-bg` is the surface-1 grey
/// (`card_background`). The text strip always stops before the ✕ while the ✕
/// shows, so the two never overlap however narrow the pill. `type=search`
/// clears on Escape.
///
/// The web pill also lifts with a `box-shadow` when active; a shadow paints
/// OUTSIDE the element, and a primitive stays inside its bounds, so it is not
/// drawn — the white ground and the border carry the state.
#[derive(Clone)]
pub struct SearchField {
    inner: TextBox,
    /// The leading glyph (`<Search />`).
    pub leading_icon: Option<&'static str>,
    /// The ✕ that clears the field, shown only when there is text
    /// (`{query && …}`).
    pub clear_icon: Option<&'static str>,
    edit: EditState,
}

impl Default for SearchField {
    fn default() -> Self {
        Self { inner: TextBox::new(), leading_icon: Some("Search"), clear_icon: Some("X"), edit: EditState::default() }
    }
}

impl SearchField {
    pub fn new() -> Self {
        Self::default()
    }

    /// The pill's height: `h-9`, the height of every field and `md` button.
    pub const HEIGHT: f32 = search::HEIGHT;
    /// What the pill measured before (the omnibar's 38), for a caller that
    /// still lays out a toolbar strip around it.
    pub const OMNIBAR_HEIGHT: f32 = search::OMNIBAR_HEIGHT;

    /// Whether the ✕ is currently on screen.
    pub fn shows_clear(&self) -> bool {
        self.clear_icon.is_some() && !self.text().is_empty() && self.enabled
    }

    /// Whether `(x, y)` lands on the ✕ rather than on the field.
    pub fn clear_hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        self.shows_clear() && search_clear_rect(bounds).contains(x, y)
    }

    /// The strip the text is laid out in, given whether the ✕ shows now.
    pub fn text_rect(&self, bounds: Rect) -> Rect {
        let left = if self.leading_icon.is_some() {
            bounds.left + search::TEXT_LEFT
        } else {
            bounds.left + search::ICON_LEFT
        };
        let r = search_text_rect_with(bounds, self.shows_clear());
        Rect::new(left, r.top, r.right.max(left), r.bottom)
    }

    pub fn edit_state(&self) -> &EditState {
        &self.edit
    }

    pub fn menu_bounds(&self) -> Option<Rect> {
        self.edit.menu_bounds()
    }

    pub fn is_menu_open(&self) -> bool {
        self.edit.menu_open()
    }

    pub fn close_menu(&mut self) {
        self.edit.menu = None;
    }

    fn geom(&self, bounds: Rect) -> TextGeom {
        let text = self.text_rect(bounds);
        let hit = if self.shows_clear() {
            Rect::new(bounds.left, bounds.top, search_clear_rect(bounds).left, bounds.bottom)
        } else {
            bounds
        };
        TextGeom {
            text,
            clip: Rect::new(text.left, bounds.top, text.right, bounds.bottom),
            hit,
            multiline: false,
            wrap: false,
            align: self.text_align,
        }
    }

    /// One frame of editing, as [`TextField::update`], plus the ✕ (a press
    /// clears the field and keeps the focus) and Escape (clears it).
    pub fn update(&mut self, c: &dyn Canvas, bounds: Rect, input: &EditInput) -> EditOutcome {
        let g = self.geom(bounds);
        let spec = Spec::of(&self.inner, false, true);
        let clear = self.shows_clear().then(|| search_clear_rect(bounds));
        let out = update_box(&mut self.inner, &mut self.edit, c, &g, spec, input, clear);
        if !self.shows_clear() {
            self.edit.clear_hot.set(false);
        }
        out
    }
}

impl Deref for SearchField {
    type Target = TextBox;
    fn deref(&self) -> &TextBox {
        &self.inner
    }
}

impl DerefMut for SearchField {
    fn deref_mut(&mut self) -> &mut TextBox {
        &mut self.inner
    }
}

impl Widget for SearchField {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let text = self.text();
        let shown = if text.is_empty() { self.placeholder_text.as_str() } else { text };
        let text_w = canvas.measure(shown, &canvas.formats().body);
        // Glyph column, text, and the ✕ column (reserved, so the pill does not
        // grow the moment the user types).
        let size = Size::new(
            search::TEXT_LEFT + text_w + edit::CARET_W + search::CLEAR_W + search::END_PAD + self.padding.horizontal(),
            Self::HEIGHT + self.padding.vertical(),
        );
        self.inner.clamp(size)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        let disabled = state.disabled || !self.enabled;
        let active = state.focused && !disabled;
        let r = pill(bounds.bottom - bounds.top);
        // `isActive ? '#ffffff' : var(--color-search-bg)` — and nothing
        // outside the pill: its corners show the parent, as on the web.
        let ground = if disabled {
            t.surface_2
        } else if active {
            t.layer_background
        } else {
            t.card_background
        };
        canvas.fill_rounded(&bounds, r, &ground);
        if active {
            canvas.stroke_rounded(&bounds, r, &t.card_stroke);
        }

        let icon_ink = if disabled { faded(&t.text_secondary) } else { t.text_secondary };
        if let Some(name) = self.leading_icon {
            canvas.vector_icon(name, &search_icon_rect(bounds), ICON, &icon_ink);
        }
        if self.shows_clear() {
            if let Some(name) = self.clear_icon {
                // `text-text-tertiary hover:text-text-primary`.
                let ink = if self.edit.clear_hot.get() { t.text_primary } else { t.text_tertiary };
                canvas.vector_icon(name, &search_clear_rect(bounds), ICON, &ink);
            }
        }

        let display = self.text().to_string();
        self.edit.sync_from_model(self.selection_start(), self.selection_length(), char_count(&display));
        let p = TextPaint {
            display: &display,
            placeholder: &self.placeholder_text,
            focused: active,
            disabled,
            caret: active && !self.read_only,
            keep_selection: !self.hide_selection,
        };
        paint_text(canvas, &self.edit, &self.geom(bounds), p);
    }

    fn type_name(&self) -> &'static str {
        "SearchField"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// MaskedField
// ═════════════════════════════════════════════════════════════════════════════

/// A field governed by a mask — `@ui/Input`'s chrome over
/// [`kubuno_desktop_controls::text::MaskedTextBox`].
///
/// The mask engine is the replica's: `set_mask` parses the WinForms mask
/// language, `set_text` feeds characters through the positions (skipping what
/// a position refuses), `mask_completed()` answers. Editing here only decides
/// WHICH characters to feed: the filled characters are read back from the
/// engine (`MaskFormat::ExcludePromptAndLiterals`), a typed character is
/// inserted at the edit position under the caret, and the whole run is fed
/// again. The caret skips literals as WinForms' does.
///
/// The visible string (literals verbatim, filled positions, the prompt char for
/// empty ones) is what the replica keeps in `ControlBase::text`.
#[derive(Clone, Default)]
pub struct MaskedField {
    inner: MaskedTextBox,
    /// As [`TextField::leading_icon`].
    pub leading_icon: Option<&'static str>,
    /// As [`TextField::trailing_icon`].
    pub trailing_icon: Option<&'static str>,
    /// As [`TextField::invalid`]. A natural driver is
    /// `!field.mask_completed()`, but that is the caller's decision.
    pub invalid: bool,
    edit: EditState,
}

impl MaskedField {
    pub fn new() -> Self {
        Self::default()
    }

    /// The visible string — literals, entered characters, prompt glyphs.
    pub fn display(&self) -> &str {
        TextBoxBase::text(&self.inner)
    }

    /// The string [`Self::display`] shows, but with the characters actually typed where a
    /// password mask (`PasswordChar` / `UseSystemPasswordChar`) displays its glyphs — what a
    /// binding reads and writes. Equal to [`Self::display`] for a field that masks nothing.
    pub fn value(&self) -> String {
        let mut probe = self.inner.clone();
        probe.text_mask_format = MaskFormat::IncludePromptAndLiterals;
        probe.text()
    }

    pub fn content(&self, bounds: Rect) -> Rect {
        content_rect(bounds, self.leading_icon.is_some(), self.trailing_icon.is_some())
    }

    pub fn edit_state(&self) -> &EditState {
        &self.edit
    }

    pub fn menu_bounds(&self) -> Option<Rect> {
        self.edit.menu_bounds()
    }

    pub fn is_menu_open(&self) -> bool {
        self.edit.menu_open()
    }

    /// The display positions of the editable slots, in order: an empty
    /// engine shows the prompt char exactly there. (A literal equal to the
    /// prompt char would be taken for a slot — WinForms forbids that mask.)
    pub fn edit_positions(&self) -> Vec<usize> {
        let mut probe = self.inner.clone();
        probe.set_text("");
        let prompt = probe.prompt_char;
        TextBoxBase::text(&probe)
            .chars()
            .enumerate()
            .filter(|(_, ch)| *ch == prompt)
            .map(|(i, _)| i)
            .collect()
    }

    /// The characters entered so far, in slot order.
    fn raw(&self) -> Vec<char> {
        let mut probe = self.inner.clone();
        probe.text_mask_format = MaskFormat::ExcludePromptAndLiterals;
        probe.text().chars().collect()
    }

    fn set_raw(&mut self, raw: &[char]) {
        let s: String = raw.iter().collect();
        self.inner.set_text(&s);
        self.inner.modified = true;
    }

    fn geom(&self, bounds: Rect) -> TextGeom {
        let content = self.content(bounds);
        TextGeom {
            text: content,
            clip: Rect::new(content.left, bounds.top, content.right, bounds.bottom),
            hit: bounds,
            multiline: false,
            wrap: false,
            align: self.text_align,
        }
    }

    /// Feeds `chars` in at the caret (replacing the selection first); returns
    /// the caret after them. A character a slot refuses is dropped by the
    /// engine, as WinForms drops it.
    fn feed(&mut self, chars: &str, sel: (usize, usize)) -> (usize, bool) {
        let slots = self.edit_positions();
        let before = self.raw();
        let mut raw = before.clone();
        let (a, b) = sel;
        let k0 = slots.iter().filter(|&&p| p < a).count();
        let k1 = slots.iter().filter(|&&p| p < b).count();
        raw.drain(k0.min(raw.len())..k1.min(raw.len()));
        let mut k = k0.min(raw.len());
        for ch in chars.chars() {
            if k >= slots.len() {
                break;
            }
            if raw.len() >= slots.len() {
                // Full: overwrite the slot under the caret.
                if k < raw.len() {
                    raw[k] = ch;
                }
            } else {
                raw.insert(k, ch);
            }
            let count_before = raw.len();
            self.set_raw(&raw);
            let now = self.raw();
            // Refused by the slot: the engine skipped it.
            if now.len() < count_before && now.get(k) != Some(&ch) {
                raw = now;
                continue;
            }
            raw = now;
            k += 1;
        }
        self.set_raw(&raw);
        let n = char_count(self.display());
        let caret = slots.get(k).copied().unwrap_or(n);
        (caret, self.raw() != before)
    }

    /// One frame of editing: click / drag / double-click selection, typing
    /// through the mask, Backspace / Delete over slots, arrows (skipping
    /// literals), Ctrl+A / C / X / V / Z / Y and the context menu.
    pub fn update(&mut self, c: &dyn Canvas, bounds: Rect, input: &EditInput) -> EditOutcome {
        let mut out = EditOutcome::default();
        let g = self.geom(bounds);
        let disabled = !self.enabled;
        let editable = !disabled && !self.read_only;
        let display = self.display().to_string();
        let n = char_count(&display);
        let st = &mut self.edit;
        st.sync_from_model(self.inner.selection_start(), self.inner.selection_length(), n);
        let pressed = input.mouse_down && !st.prev_down;
        let right_pressed = input.right_down && !st.prev_right;
        st.prev_down = input.mouse_down;
        st.prev_right = input.right_down;
        st.window_blurred = !input.window_focused;
        if st.had_focus && !input.focused {
            st.drag = None;
            st.menu = None;
        }
        if input.focused && !st.had_focus {
            st.touch();
            st.reveal.set(true);
            if !pressed && !right_pressed {
                // Tab in: the caret goes to the first empty slot.
                let slots = self.edit_positions();
                let filled = self.raw().len();
                let p = slots.get(filled).copied().unwrap_or(n);
                self.edit.set_selection(p, p);
            }
        }
        self.edit.had_focus = input.focused;
        if disabled {
            self.edit.menu = None;
            if g.hit.contains(input.mouse.0, input.mouse.1) {
                host::set_cursor(Cursor::NotAllowed);
            }
            return out;
        }

        let flags = MenuFlags {
            editable,
            has_selection: self.edit.anchor.get() != self.edit.caret.get(),
            copy_allowed: self.inner.password_char.is_none() && !self.inner.use_system_password_char,
            can_undo: self.edit.history.can_undo(),
            can_redo: self.edit.history.can_redo(),
        };
        let mut swallowed = false;
        let mut chosen = None;
        if self.edit.menu.is_some() {
            let (a, took) = drive_menu(&mut self.edit, c, input, flags, pressed, right_pressed);
            swallowed = took;
            chosen = a;
        }

        let laid = lay_out(c, &display, &g);
        let (mx, my) = input.mouse;
        if !swallowed {
            if pressed && g.hit.contains(mx, my) {
                let pos = pos_at(c, &display, &g, &laid, &self.edit, mx, my, false);
                match input.click_count {
                    2 => {
                        let (a, b) = word_range_at(&display, pos_at(c, &display, &g, &laid, &self.edit, mx, my, true));
                        self.edit.set_selection(a, b);
                        self.edit.drag = Some(Drag::Word(a, b));
                    }
                    k if k >= 3 => {
                        self.edit.set_selection(0, n);
                        self.edit.drag = Some(Drag::Line(0, n));
                    }
                    _ => {
                        let anchor = if input.mods.shift { self.edit.anchor.get() } else { pos };
                        self.edit.set_selection(anchor, pos);
                        self.edit.drag = Some(Drag::Char);
                    }
                }
                self.edit.touch();
            } else if right_pressed && g.hit.contains(mx, my) {
                open_menu(&mut self.edit, input.mouse);
            }
            if input.mouse_down {
                if let Some(Drag::Char) = self.edit.drag {
                    let pos = pos_at(c, &display, &g, &laid, &self.edit, mx, my, false);
                    let anchor = self.edit.anchor.get();
                    self.edit.set_selection(anchor, pos);
                }
            } else {
                self.edit.drag = None;
            }
            if self.edit.drag.is_some() || g.hit.contains(mx, my) {
                host::set_cursor(Cursor::IBeam);
            }
        }

        let run = |this: &mut Self, action: EditAction, out: &mut EditOutcome| {
            this.edit.touch();
            let (a, b) = this.edit.selection();
            let current = Snapshot {
                text: this.raw().iter().collect(),
                anchor: this.edit.anchor.get(),
                caret: this.edit.caret.get(),
            };
            match action {
                EditAction::Copy | EditAction::Cut => {
                    if a < b && flags.copy_allowed {
                        clipboard_set(char_slice(this.display(), a, b));
                        if action == EditAction::Cut && editable {
                            let (p, changed) = this.feed("", (a, b));
                            if changed {
                                this.edit.history.record(current, EditKind::Other, host::now_ms());
                                out.changed = true;
                            }
                            let _ = p;
                            this.edit.set_selection(a, a);
                        }
                    }
                }
                EditAction::Paste | EditAction::Delete => {
                    if editable {
                        let ins = if action == EditAction::Paste {
                            clipboard_get().map(|s| sanitize_paste(&s, false)).unwrap_or_default()
                        } else {
                            String::new()
                        };
                        let (p, changed) = this.feed(&ins, (a, b));
                        if changed {
                            this.edit.history.record(current, EditKind::Other, host::now_ms());
                            out.changed = true;
                        }
                        this.edit.set_selection(p, p);
                    }
                }
                EditAction::Undo | EditAction::Redo => {
                    if editable {
                        let s = if action == EditAction::Undo {
                            this.edit.history.undo(current)
                        } else {
                            this.edit.history.redo(current)
                        };
                        if let Some(s) = s {
                            let raw: Vec<char> = s.text.chars().collect();
                            this.set_raw(&raw);
                            this.edit.set_selection(s.anchor, s.caret);
                            out.changed = true;
                        }
                    }
                }
                EditAction::SelectAll => this.edit.set_selection(0, char_count(this.display())),
            }
        };
        if let Some(a) = chosen {
            run(self, a, &mut out);
        }

        if input.focused && self.edit.menu.is_none() {
            let events = host::consume(|e| match e {
                InputEvent::Text(_) => true,
                InputEvent::Key { vk: k, down: true, mods, .. } => {
                    command_for(*k, *mods).is_some_and(|(c, _)| !matches!(c, EditCommand::PageUp | EditCommand::PageDown))
                }
                _ => false,
            });
            for e in events {
                let n = char_count(self.display());
                let (a, b) = self.edit.selection();
                let caret = self.edit.caret.get();
                let slots = self.edit_positions();
                match e {
                    InputEvent::Text(s) => {
                        if editable {
                            let current = Snapshot {
                                text: self.raw().iter().collect(),
                                anchor: self.edit.anchor.get(),
                                caret,
                            };
                            let (p, changed) = self.feed(&s, (a, b));
                            if changed {
                                self.edit.history.record(current, EditKind::Typing, host::now_ms());
                                out.changed = true;
                            }
                            self.edit.set_selection(p, p);
                            self.edit.touch();
                        }
                    }
                    InputEvent::Key { vk: k, mods, .. } => {
                        let Some((cmd, extend)) = command_for(k, mods) else { continue };
                        self.edit.touch();
                        let text = self.display().to_string();
                        let mv = |st: &EditState, p: usize| {
                            let anchor = if extend { st.anchor.get() } else { p };
                            st.set_selection(anchor, p.min(n));
                        };
                        match cmd {
                            EditCommand::Left => {
                                let p = if !extend && a < b { a } else { caret.saturating_sub(1) };
                                mv(&self.edit, p);
                            }
                            EditCommand::Right => {
                                let p = if !extend && a < b { b } else { (caret + 1).min(n) };
                                mv(&self.edit, p);
                            }
                            EditCommand::WordLeft => mv(&self.edit, word_left(&text, caret)),
                            EditCommand::WordRight => mv(&self.edit, word_right(&text, caret)),
                            EditCommand::LineStart | EditCommand::DocStart | EditCommand::Up => mv(&self.edit, 0),
                            EditCommand::LineEnd | EditCommand::DocEnd | EditCommand::Down => mv(&self.edit, n),
                            EditCommand::Backspace | EditCommand::BackspaceWord | EditCommand::Delete | EditCommand::DeleteWord => {
                                if editable {
                                    let current = Snapshot {
                                        text: self.raw().iter().collect(),
                                        anchor: self.edit.anchor.get(),
                                        caret,
                                    };
                                    let back = matches!(cmd, EditCommand::Backspace | EditCommand::BackspaceWord);
                                    let range = if a < b {
                                        (a, b)
                                    } else if back {
                                        // The slot before the caret.
                                        let s = slots.iter().rev().find(|&&p| p < caret).copied();
                                        s.map_or((caret, caret), |s| (s, s + 1))
                                    } else {
                                        let s = slots.iter().find(|&&p| p >= caret).copied();
                                        s.map_or((caret, caret), |s| (s, s + 1))
                                    };
                                    let (_, changed) = self.feed("", range);
                                    if changed {
                                        self.edit.history.record(current, EditKind::Deleting, host::now_ms());
                                        out.changed = true;
                                    }
                                    let p = if back || a < b { range.0 } else { caret };
                                    self.edit.set_selection(p, p);
                                }
                            }
                            EditCommand::SelectAll => run(self, EditAction::SelectAll, &mut out),
                            EditCommand::Undo => run(self, EditAction::Undo, &mut out),
                            EditCommand::Redo => run(self, EditAction::Redo, &mut out),
                            EditCommand::Copy => run(self, EditAction::Copy, &mut out),
                            EditCommand::Cut => run(self, EditAction::Cut, &mut out),
                            EditCommand::Paste => run(self, EditAction::Paste, &mut out),
                            EditCommand::Enter => out.submitted = true,
                            EditCommand::ContextMenu => {
                                let x = g.text.left - self.edit.scroll_x.get()
                                    + x_in_line(c, &text, VisualLine { start: 0, end: n }, caret);
                                open_menu(&mut self.edit, (x, row_top(&g, 0, 0.0) + LINE));
                            }
                            EditCommand::PageUp | EditCommand::PageDown => {}
                        }
                    }
                    _ => {}
                }
            }
            if host::key_pressed(vk::ESCAPE, Modifiers::NONE) {
                out.escaped = true;
            }
        }

        // Keep the model's selection in step with the editor's.
        let (a, b) = self.edit.selection();
        self.inner.select(a as i32, (b - a) as i32);
        self.edit.seen.set((self.inner.selection_start(), self.inner.selection_length()));

        if self.edit.menu.is_some() {
            let flags = MenuFlags {
                has_selection: a != b,
                can_undo: self.edit.history.can_undo(),
                can_redo: self.edit.history.can_redo(),
                ..flags
            };
            show_menu(&mut self.edit, c, input, flags);
        }
        out.menu_open = self.edit.menu.is_some();
        out
    }
}

impl Deref for MaskedField {
    type Target = MaskedTextBox;
    fn deref(&self) -> &MaskedTextBox {
        &self.inner
    }
}

impl DerefMut for MaskedField {
    fn deref_mut(&mut self) -> &mut MaskedTextBox {
        &mut self.inner
    }
}

impl Widget for MaskedField {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let text_w = canvas.measure(self.display(), &canvas.formats().body);
        let gutters = (if self.leading_icon.is_some() { ICON_COLUMN } else { PAD_X })
            + (if self.trailing_icon.is_some() { ICON_COLUMN } else { PAD_X });
        let size = Size::new(
            text_w + edit::CARET_W + gutters + self.padding.horizontal(),
            field_height(false, 1) + self.padding.vertical(),
        );
        self.inner.clamp(size)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let disabled = state.disabled || !self.enabled;
        let edge = Edge { radius: radius::SM, disabled, focused: state.focused, invalid: self.invalid };
        paint_edge(canvas, bounds, &edge, &canvas.theme().layer_background);
        let display = self.display();
        self.edit.sync_from_model(self.selection_start(), self.selection_length(), char_count(display));
        let focused = state.focused && !disabled;
        // A mask already shows its prompts, so it never has a placeholder.
        let p = TextPaint { display, placeholder: "", focused, disabled, caret: focused && !self.read_only, keep_selection: !self.hide_selection };
        paint_text(canvas, &self.edit, &self.geom(bounds), p);
        paint_icons(canvas, bounds, self.leading_icon, self.trailing_icon, disabled);
    }

    fn type_name(&self) -> &'static str {
        "MaskedField"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 200 × 36 field at the origin — `@ui/Input`'s own height.
    fn bounds() -> Rect {
        Rect::new(0.0, 0.0, 200.0, height::BUTTON_MD)
    }

    /// A monospace stand-in for DirectWrite: 10 DIP per character.
    fn mono(s: &str) -> f32 {
        s.chars().count() as f32 * 10.0
    }

    // ── Geometry ─────────────────────────────────────────────────────────

    #[test]
    fn the_content_rect_is_px_3_on_both_sides() {
        let b = bounds();
        let c = content_rect(b, false, false);
        assert_eq!(c.left - b.left, 12.0);
        assert_eq!(b.right - c.right, 12.0);
        assert_eq!((c.top, c.bottom), (b.top, b.bottom));
    }

    /// `.kb-field-focus`: a 3 px outline (the predecessor's 2 px ring is kept
    /// as a constant for callers).
    #[test]
    fn the_focus_outline_is_three_dip() {
        assert_eq!(FOCUS_OUTLINE, 3.0);
        assert_eq!(FOCUS_RING, 2.0);
    }

    #[test]
    fn the_line_box_reconstructs_the_field_height() {
        assert_eq!(2.0 * PAD_Y + LINE, height::BUTTON_MD);
        assert_eq!(field_height(false, 1), height::BUTTON_MD);
        assert_eq!(field_height(true, 1), height::BUTTON_MD);
    }

    /// `SearchBar compact`: `h-9`, `pl-3` + 16 + `pr-2` before the text, a
    /// 24 wide ✕ before the trailing `pr-2` — and the same height as every
    /// other field and `md` button (the composition audit's misalignment).
    #[test]
    fn the_search_pill_is_the_compact_search_bar() {
        assert_eq!(SearchField::HEIGHT, height::BUTTON_MD);
        let b = Rect::new(100.0, 10.0, 500.0, 10.0 + SearchField::HEIGHT);
        let icon = search_icon_rect(b);
        assert_eq!((icon.left, icon.right), (112.0, 128.0));
        let clear = search_clear_rect(b);
        assert_eq!((clear.left, clear.right), (b.right - 32.0, b.right - 8.0));
        let with = search_text_rect_with(b, true);
        assert_eq!(with.left, b.left + 36.0);
        assert_eq!(with.right, clear.left, "the strip stops at the ✕");
        let without = search_text_rect_with(b, false);
        assert_eq!(without.right, b.right - 8.0);
        assert_eq!(pill(SearchField::HEIGHT), 18.0);
    }

    /// The ✕ column is reserved exactly while the ✕ shows, however narrow the
    /// pill: the text never runs under it.
    #[test]
    fn a_narrow_search_pill_keeps_the_text_off_the_clear_button() {
        let mut s = SearchField::new();
        let b = Rect::new(0.0, 0.0, 90.0, SearchField::HEIGHT);
        assert_eq!(s.text_rect(b).right, b.right - 8.0, "no ✕, no reservation");
        s.set_text("rapport trimestriel");
        let strip = s.text_rect(b);
        assert!(strip.right <= search_clear_rect(b).left);
        assert!(strip.right >= strip.left, "never inverted");
        let tiny = Rect::new(0.0, 0.0, 40.0, SearchField::HEIGHT);
        let r = s.text_rect(tiny);
        assert!(r.right >= r.left);
    }

    #[test]
    fn an_icon_takes_its_column() {
        let b = bounds();
        assert_eq!(content_rect(b, true, false).left - b.left, 36.0);
        assert_eq!(b.right - content_rect(b, false, true).right, 36.0);
        let lead = icon_rect(b, Side::Leading);
        assert_eq!(lead.left, b.left + 12.0);
        assert_eq!(lead.right - lead.left, ICON);
        let trail = icon_rect(b, Side::Trailing);
        assert_eq!(trail.right, b.right - 12.0);
        assert_eq!(lead.right + 8.0, b.left + ICON_COLUMN);
    }

    #[test]
    fn a_crushed_field_keeps_a_valid_content_rect() {
        let tiny = Rect::new(0.0, 0.0, 8.0, height::BUTTON_MD);
        let c = content_rect(tiny, true, true);
        assert!(c.right >= c.left);
    }

    // ── Characters and words ─────────────────────────────────────────────

    #[test]
    fn byte_offsets_clamp_at_both_ends() {
        assert_eq!(byte_at("héllo", -3), 0);
        assert_eq!(byte_at("héllo", 99), "héllo".len());
        assert_eq!(byte_at("", 0), 0);
        assert_eq!(char_slice("héllo", 1, 3), "él");
        assert_eq!(char_slice("héllo", 3, 1), "él", "reversed bounds");
    }

    /// Ctrl+Left / Ctrl+Right, the Windows way: Right lands on the START of
    /// the next word, punctuation is a stop of its own.
    #[test]
    fn word_moves_follow_windows() {
        let t = "Bonjour le monde, ça va";
        assert_eq!(word_right(t, 0), 8, "over « Bonjour » and its space");
        assert_eq!(word_right(t, 8), 11);
        assert_eq!(word_right(t, 11), 16, "stops before the comma");
        assert_eq!(word_right(t, 16), 18, "the comma run, then its space");
        assert_eq!(word_left(t, 18), 16);
        assert_eq!(word_left(t, 16), 11);
        assert_eq!(word_left(t, 11), 8);
        assert_eq!(word_left(t, 0), 0);
        assert_eq!(word_right(t, 23), 23);
        // A line break is a stop.
        let m = "un\ndeux";
        assert_eq!(word_right(m, 0), 2);
        assert_eq!(word_right(m, 2), 3);
        assert_eq!(word_left(m, 3), 0);
    }

    /// Double-click: the word and its trailing spaces (Windows); a space run
    /// or a punctuation run on its own.
    #[test]
    fn double_click_selects_the_word_and_its_trailing_space() {
        let t = "Bonjour le monde, ça va";
        assert_eq!(word_range_at(t, 2), (0, 8));
        assert_eq!(word_range_at(t, 12), (11, 16), "no space after « monde »: the comma stops it");
        assert_eq!(word_range_at(t, 16), (16, 17));
        assert_eq!(word_range_at(t, 99), (21, 23), "past the end: the last word");
        assert_eq!(word_range_at("", 0), (0, 0));
        assert_eq!(paragraph_range_at("un\ndeux\ntrois", 5), (3, 7));
        assert_eq!(paragraph_range_at("abc", 1), (0, 3));
    }

    #[test]
    fn paste_is_sanitised_like_an_input_and_a_textarea() {
        assert_eq!(sanitize_paste("a\r\nb\nc", false), "abc");
        assert_eq!(sanitize_paste("a\r\nb\rc", true), "a\nb\nc");
    }

    // ── The buffer ───────────────────────────────────────────────────────

    #[test]
    fn typing_replaces_the_selection_and_honours_maxlength() {
        let mut b = EditBuffer::new("hello", 1, 4);
        assert_eq!(b.selected_text(), "ell");
        assert!(b.insert("EY", None));
        assert_eq!((b.text.as_str(), b.caret, b.anchor), ("hEYo", 3, 3));
        // `maxlength=6`: two characters of room, the paste is cut to fit.
        assert!(b.insert("123456", Some(6)));
        assert_eq!(b.text, "hEY12o");
        // Full: a key is refused.
        assert!(!b.insert("x", Some(6)));
        assert_eq!(b.text, "hEY12o");
        // Replacing a selection frees its room first.
        b.select(0, 6);
        assert!(b.insert("z", Some(6)));
        assert_eq!(b.text, "z");
    }

    #[test]
    fn backspace_and_delete_by_char_word_and_selection() {
        let mut b = EditBuffer::new("un deux trois", 13, 13);
        assert!(b.delete_back(true));
        assert_eq!(b.text, "un deux ");
        assert!(b.delete_back(false));
        assert_eq!(b.text, "un deux");
        b.set_caret(0, false);
        assert!(!b.delete_back(false), "nothing before the caret");
        assert!(b.delete_forward(true));
        assert_eq!(b.text, "deux");
        b.select(1, 3);
        assert!(b.delete_forward(false), "a selection goes whole");
        assert_eq!(b.text, "dx");
        b.set_caret(2, false);
        assert!(!b.delete_forward(false));
    }

    #[test]
    fn multibyte_text_is_edited_by_characters() {
        let mut b = EditBuffer::new("héllo 😀", 7, 7);
        assert!(b.delete_back(false));
        assert_eq!(b.text, "héllo ");
        b.select(1, 2);
        b.insert("e", None);
        assert_eq!(b.text, "hello ");
    }

    #[test]
    fn shift_extends_and_a_plain_move_collapses() {
        let mut b = EditBuffer::new("abcdef", 2, 2);
        b.set_caret(5, true);
        assert_eq!(b.selection(), (2, 5));
        b.set_caret(1, true);
        assert_eq!(b.selection(), (1, 2), "the anchor stays put");
        b.set_caret(4, false);
        assert!(!b.has_selection());
    }

    // ── Undo ─────────────────────────────────────────────────────────────

    #[test]
    fn a_typing_burst_is_one_undo_step() {
        let mut h = History::default();
        let s = |t: &str| Snapshot { text: t.into(), anchor: t.len(), caret: t.len() };
        h.record(s(""), EditKind::Typing, 0);
        h.record(s("a"), EditKind::Typing, 100);
        h.record(s("ab"), EditKind::Typing, 200);
        assert_eq!(h.undo.len(), 1, "one burst");
        h.record(s("abc"), EditKind::Typing, 5000);
        assert_eq!(h.undo.len(), 2, "a pause starts a new step");
        h.record(s("abcd"), EditKind::Deleting, 5100);
        assert_eq!(h.undo.len(), 3, "deleting is a step of its own");
        let back = h.undo(s("abc")).map(|x| x.text);
        assert_eq!(back.as_deref(), Some("abcd"));
        assert!(h.can_redo());
        let fwd = h.redo(s("abcd")).map(|x| x.text);
        assert_eq!(fwd.as_deref(), Some("abc"));
        h.record(s("x"), EditKind::Other, 6000);
        assert!(!h.can_redo(), "a new edit drops the redo branch");
    }

    #[test]
    fn the_undo_stack_is_bounded() {
        let mut h = History::default();
        for i in 0..(edit::UNDO_DEPTH + 20) {
            h.record(Snapshot { text: i.to_string(), anchor: 0, caret: 0 }, EditKind::Other, i as u64);
        }
        assert_eq!(h.undo.len(), edit::UNDO_DEPTH);
    }

    // ── Keys ─────────────────────────────────────────────────────────────

    #[test]
    fn keys_map_like_a_windows_browser() {
        use EditCommand::*;
        let n = Modifiers::NONE;
        assert_eq!(command_for(vk::LEFT, n), Some((Left, false)));
        assert_eq!(command_for(vk::LEFT, Modifiers::CTRL_SHIFT), Some((WordLeft, true)));
        assert_eq!(command_for(vk::HOME, Modifiers::CTRL), Some((DocStart, false)));
        assert_eq!(command_for(vk::END, Modifiers::SHIFT), Some((LineEnd, true)));
        assert_eq!(command_for(vk::BACK, Modifiers::CTRL), Some((BackspaceWord, false)));
        assert_eq!(command_for(vk::ENTER, n), Some((Enter, false)));
        assert_eq!(command_for(vk::ENTER, Modifiers::SHIFT), Some((Enter, true)));
        assert_eq!(command_for(vk::DELETE, Modifiers::SHIFT), Some((Cut, false)));
        assert_eq!(command_for(vk::INSERT, Modifiers::CTRL), Some((Copy, false)));
        assert_eq!(command_for(vk::INSERT, Modifiers::SHIFT), Some((Paste, false)));
        assert_eq!(command_for(vk::letter('a'), Modifiers::CTRL), Some((SelectAll, false)));
        assert_eq!(command_for(vk::letter('z'), Modifiers::CTRL), Some((Undo, false)));
        assert_eq!(command_for(vk::letter('z'), Modifiers::CTRL_SHIFT), Some((Redo, false)));
        assert_eq!(command_for(vk::letter('y'), Modifiers::CTRL), Some((Redo, false)));
        assert_eq!(command_for(vk::F10, Modifiers::SHIFT), Some((ContextMenu, false)));
        assert_eq!(command_for(vk::APPS, n), Some((ContextMenu, false)));
        assert_eq!(command_for(vk::letter('a'), n), None, "a plain letter is TEXT, not a key");
        assert_eq!(command_for(vk::LEFT, Modifiers::ALT), None, "Alt chords belong to the window");
        assert_eq!(command_for(vk::TAB, n), None, "Tab is the focus ring's");
        assert_eq!(command_for(vk::ESCAPE, n), None);
    }

    // ── Soft wrap ────────────────────────────────────────────────────────

    #[test]
    fn rows_wrap_at_word_boundaries_and_break_long_words() {
        // 10 DIP per char, 60 DIP rows: six characters.
        let lines = wrap_lines("un deux trois", Some(60.0), &mono);
        let rows: Vec<(usize, usize)> = lines.iter().map(|l| (l.start, l.end)).collect();
        assert_eq!(rows, vec![(0, 3), (3, 8), (8, 13)], "trailing spaces hang");
        let long = wrap_lines("abcdefghij", Some(40.0), &mono);
        let rows: Vec<(usize, usize)> = long.iter().map(|l| (l.start, l.end)).collect();
        assert_eq!(rows, vec![(0, 4), (4, 8), (8, 10)]);
        // Paragraphs, including empty ones, are rows of their own.
        let paras = wrap_lines("a\n\nb", Some(60.0), &mono);
        let rows: Vec<(usize, usize)> = paras.iter().map(|l| (l.start, l.end)).collect();
        assert_eq!(rows, vec![(0, 1), (2, 2), (3, 4)]);
        // No width: no wrapping.
        assert_eq!(wrap_lines("un deux trois", None, &mono).len(), 1);
        assert_eq!(wrap_lines("", Some(60.0), &mono), vec![VisualLine { start: 0, end: 0 }]);
    }

    #[test]
    fn a_caret_on_a_soft_break_belongs_to_the_next_row() {
        let lines = wrap_lines("un deux trois", Some(60.0), &mono);
        assert_eq!(line_of(&lines, 2), 0);
        assert_eq!(line_of(&lines, 3), 1, "the soft break");
        assert_eq!(line_of(&lines, 13), 2);
        let paras = wrap_lines("ab\ncd", None, &mono);
        assert_eq!(line_of(&paras, 2), 0, "before the line break");
        assert_eq!(line_of(&paras, 3), 1);
    }

    // ── The context menu ─────────────────────────────────────────────────

    /// `TextFieldMenuHost`: the editable menu, and the read-only one that
    /// keeps only Copy and Select all.
    #[test]
    fn the_text_menu_is_the_web_menu() {
        let flags = MenuFlags { editable: true, has_selection: true, copy_allowed: true, can_undo: true, can_redo: false };
        let (menu, actions) = text_menu(flags);
        assert_eq!(menu.items().len(), 9);
        assert_eq!(
            actions,
            vec![
                Some(EditAction::Undo),
                Some(EditAction::Redo),
                None,
                Some(EditAction::Cut),
                Some(EditAction::Copy),
                Some(EditAction::Paste),
                Some(EditAction::Delete),
                None,
                Some(EditAction::SelectAll),
            ]
        );
        assert!(menu_row_enabled(&menu, &actions, 0));
        assert!(!menu_row_enabled(&menu, &actions, 1), "nothing to redo");
        assert!(!menu_row_enabled(&menu, &actions, 2), "a separator is never a target");
        assert_eq!(menu_step(&menu, &actions, Some(0), true), Some(3), "Down skips Redo and the rule");
        assert_eq!(menu_step(&menu, &actions, Some(0), false), Some(8), "Up wraps");

        let ro = MenuFlags { editable: false, has_selection: false, copy_allowed: true, ..Default::default() };
        let (menu, actions) = text_menu(ro);
        assert_eq!(actions, vec![Some(EditAction::Copy), None, Some(EditAction::SelectAll)]);
        assert!(!menu_row_enabled(&menu, &actions, 0), "no selection: Copy greyed");

        let pw = MenuFlags { editable: true, has_selection: true, copy_allowed: false, ..Default::default() };
        let (menu, actions) = text_menu(pw);
        assert!(!menu_row_enabled(&menu, &actions, 3), "a password is never cut");
        assert!(!menu_row_enabled(&menu, &actions, 4), "nor copied");
    }

    /// The menu opens at the point, is pulled back inside the monitor and
    /// flips above the point near the bottom.
    #[test]
    fn the_menu_stays_on_the_screen() {
        let screen = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let r = menu_panel_at((100.0, 100.0), (220.0, 300.0), screen);
        assert_eq!((r.left, r.top), (100.0, 100.0));
        let r = menu_panel_at((950.0, 100.0), (220.0, 300.0), screen);
        assert_eq!(r.right, 992.0, "8 DIP from the right edge");
        let r = menu_panel_at((100.0, 700.0), (220.0, 300.0), screen);
        assert_eq!(r.bottom, 700.0, "above the point");
        // A screen reaching past the window (negative client coordinates).
        let wide = Rect::new(-500.0, -200.0, 1500.0, 900.0);
        let r = menu_panel_at((-400.0, -150.0), (220.0, 300.0), wide);
        assert_eq!((r.left, r.top), (-400.0, -150.0));
    }

    // ── The model is the replica's ───────────────────────────────────────

    #[test]
    fn the_model_is_the_replicas() {
        let mut f = TextField::new();
        assert_eq!(f.max_length, 32767);
        f.max_length = 4;
        f.set_text("abcdefgh");
        assert_eq!(f.text(), "abcd");
        f.select(2, 99);
        assert_eq!(f.selection_length(), 2);
        assert_eq!(f.model().type_name(), "TextBox");
    }

    #[test]
    fn the_placeholder_comes_from_the_replica() {
        let mut f = TextField::new();
        f.placeholder_text = "Serveur".into();
        assert_eq!(f.inner.placeholder_text, "Serveur");
    }

    #[test]
    fn a_password_field_only_ever_displays_glyphs() {
        let mut f = TextField::new();
        f.set_text("s3cret");
        f.password_char = Some('*');
        assert_eq!(f.display(), "******");
        f.use_system_password_char = true;
        assert_eq!(f.display(), "●●●●●●");
        assert!(Spec::of(&f.inner, false, false).password);
        f.multiline = true;
        assert_eq!(f.display(), "s3cret");
    }

    /// The model of a password field holds the typed text (only the drawing is masked), and its
    /// text never reaches the clipboard: Copy and Cut do nothing, as in Windows Forms. Paste still
    /// types into it.
    #[test]
    fn a_password_field_keeps_its_text_and_never_gives_it_to_the_clipboard() {
        TEST_CLIPBOARD.with(|c| *c.borrow_mut() = None);
        let mut f = TextField::new();
        f.password_char = Some('\u{2022}');
        f.reset_text("s3cret");
        assert_eq!(f.text(), "s3cret", "the model holds the real text");
        assert_eq!(f.display(), "\u{2022}".repeat(6), "only the drawing is masked");
        f.apply(EditAction::SelectAll);
        assert!(!f.apply(EditAction::Copy));
        assert_eq!(TEST_CLIPBOARD.with(|c| c.borrow().clone()), None, "nothing copied");
        assert!(!f.apply(EditAction::Cut), "Cut is refused");
        assert_eq!(TEST_CLIPBOARD.with(|c| c.borrow().clone()), None, "nothing cut");
        assert_eq!(f.text(), "s3cret", "and the text stays");
        // Paste replaces the selection with the clipboard's text.
        TEST_CLIPBOARD.with(|c| *c.borrow_mut() = Some("hunter2".into()));
        assert!(f.apply(EditAction::Paste));
        assert_eq!(f.text(), "hunter2");
        assert_eq!(f.display(), "\u{2022}".repeat(7));
        // The same field unmasked copies normally.
        f.password_char = None;
        TEST_CLIPBOARD.with(|c| *c.borrow_mut() = None);
        f.apply(EditAction::SelectAll);
        f.apply(EditAction::Copy);
        assert_eq!(TEST_CLIPBOARD.with(|c| c.borrow().clone()).as_deref(), Some("hunter2"));
    }

    /// A masked field's `value()` is its display string with the typed characters where a
    /// password mask shows glyphs — what a binding gets.
    #[test]
    fn a_password_masked_field_binds_the_typed_characters() {
        let mut m = MaskedField::new();
        m.set_mask("0000");
        m.set_text("1234");
        assert_eq!(m.value(), m.display(), "nothing masked: the display string");
        m.password_char = Some('*');
        m.set_text("1234");
        assert_eq!(m.display(), "****");
        assert_eq!(m.value(), "1234");
    }

    /// A selection a caller sets on the model is adopted by the editor (and
    /// scrolled into view); the caret is its far end.
    #[test]
    fn the_editor_adopts_a_selection_set_on_the_model() {
        let mut f = TextField::new();
        f.set_text("abcdef");
        f.select(1, 3);
        let n = char_count(f.text());
        f.edit.sync_from_model(f.selection_start(), f.selection_length(), n);
        assert_eq!((f.edit.anchor(), f.edit.caret()), (1, 4));
        assert!(f.edit.reveal.get());
        // The text shrinks under it: clamped.
        f.set_text("ab");
        f.edit.sync_from_model(f.selection_start(), f.selection_length(), 2);
        assert!(f.edit.caret() <= 2);
    }

    /// Editing writes back through the replica: casing applies, `Modified` is
    /// set (a user edit), and the model's selection follows the caret.
    #[test]
    fn a_commit_goes_through_the_replica() {
        let mut f = TextField::new();
        f.character_casing = kubuno_desktop_controls::text::CharacterCasing::Upper;
        let mut buf = EditBuffer::new("abc", 3, 3);
        commit(&mut f.inner, &f.edit, &mut buf);
        assert_eq!(f.text(), "ABC");
        assert!(f.modified);
        assert_eq!((f.selection_start(), f.selection_length()), (3, 0));
    }

    #[test]
    fn a_textarea_is_the_multiline_case_of_the_same_replica() {
        let mut a = TextArea::new();
        assert!(a.multiline);
        assert!(a.word_wrap, "soft wrap by default, as <textarea>");
        assert_eq!(a.model().type_name(), "TextBox");
        assert_eq!(a.type_name(), "TextArea");
        a.set_text("un\ndeux\ntrois");
        assert_eq!(a.rows(), 3);
        let b = Rect::new(0.0, 0.0, 200.0, TEXTAREA_HEIGHT);
        assert_eq!(row_rect(b, 0).top, b.top + PAD_Y);
        assert_eq!(row_rect(b, 1).top - row_rect(b, 0).top, LINE);
        assert_eq!(TextArea::HEIGHT, 144.0);
        assert_eq!(TextArea::MIN_HEIGHT, 64.0);
        let g = a.geom(b);
        assert!(g.multiline && g.wrap);
        assert_eq!(g.text.top, PAD_Y);
    }

    #[test]
    fn a_search_field_clears_only_when_it_has_text() {
        let mut s = SearchField::new();
        let b = Rect::new(0.0, 0.0, 400.0, SearchField::HEIGHT);
        assert!(!s.shows_clear());
        let clear = search_clear_rect(b);
        let (cx, cy) = ((clear.left + clear.right) / 2.0, (clear.top + clear.bottom) / 2.0);
        assert!(!s.clear_hit_test(b, cx, cy));
        s.set_text("rapport");
        assert!(s.shows_clear());
        assert!(s.clear_hit_test(b, cx, cy));
        assert!(!s.clear_hit_test(b, b.left + 40.0, cy));
        // The editing target stops at the ✕.
        assert!(s.geom(b).hit.right <= clear.left);
        s.enabled = false;
        assert!(!s.shows_clear(), "a disabled pill offers no ✕");
        assert_eq!(s.type_name(), "SearchField");
    }

    #[test]
    fn hit_testing_is_the_box_half_open() {
        let f = TextField::new();
        let b = bounds();
        assert!(f.hit_test(b, 0.0, 0.0));
        assert!(f.hit_test(b, 199.0, 35.0));
        assert!(!f.hit_test(b, 200.0, 18.0));
        assert!(!f.hit_test(b, 100.0, 36.0));
        assert!(!f.hit_test(b, -1.0, 18.0));
    }

    // ── MaskedField: the replica's engine, driven ────────────────────────

    #[test]
    fn a_masked_field_shows_the_replicas_display_string() {
        let mut m = MaskedField::new();
        m.set_mask("00/00");
        assert_eq!(m.display(), "__/__");
        m.set_text("3112");
        assert_eq!(m.display(), "31/12");
        assert_eq!(m.text(), "31/12");
        assert!(m.mask_completed());
        let mut m2 = MaskedField::new();
        m2.set_mask("0000");
        m2.set_text("1a23");
        assert_eq!(m2.display(), "123_");
        assert!(!m2.mask_completed());
        assert_eq!(m2.model().type_name(), "MaskedTextBox");
        assert_eq!(m2.type_name(), "MaskedField");
    }

    /// Typing goes through the engine: slots are found from the empty mask,
    /// a digit lands in the slot under the caret, the caret skips the literal,
    /// a letter is refused by a digit slot, Backspace empties the slot before.
    #[test]
    fn typing_into_a_mask_skips_literals_and_refused_characters() {
        let mut m = MaskedField::new();
        m.set_mask("00/00");
        assert_eq!(m.edit_positions(), vec![0, 1, 3, 4]);
        let (p, changed) = m.feed("3", (0, 0));
        assert!(changed);
        assert_eq!((m.display(), p), ("3_/__", 1));
        let (p, _) = m.feed("1", (p, p));
        assert_eq!((m.display(), p), ("31/__", 3), "the caret jumps the « / »");
        let (p2, changed) = m.feed("x", (p, p));
        assert!(!changed, "a digit slot refuses a letter");
        assert_eq!(m.display(), "31/__");
        let (p, _) = m.feed("12", (p2, p2));
        assert_eq!((m.display(), p), ("31/12", 5));
        // Deleting the slot at 3 (the « 1 » of the month) pulls the rest left.
        let (_, changed) = m.feed("", (3, 4));
        assert!(changed);
        assert_eq!(m.display(), "31/2_");
        // Full: typing overwrites the slot under the caret.
        m.set_text("3112");
        let (_, _) = m.feed("0", (0, 0));
        assert_eq!(m.display(), "01/12");
    }

    #[test]
    fn a_masked_field_has_a_plain_fields_geometry() {
        let mut m = MaskedField::new();
        m.set_mask("(000) 000-0000");
        let b = bounds();
        assert_eq!(m.content(b).left - b.left, 12.0);
        m.leading_icon = Some("Lock");
        assert_eq!(m.content(b).left - b.left, 36.0);
    }
}
