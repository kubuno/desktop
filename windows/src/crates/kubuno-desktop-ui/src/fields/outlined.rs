//! The Material outlined text field — the base every form field is built on.
//!
//! Ported from the web `@ui/OutlinedField` (`core/frontend/src/ui/OutlinedField.tsx`).
//! The label starts inside the box, in the placeholder's place, and floats up
//! onto the top border once the field is focused or holds a value — the border
//! opening a notch around it (the real fieldset/legend technique, so it reads on
//! any background rather than a chip masking the line). Focus paints the border
//! and the label in the form's primary colour. An optional leading icon sits
//! outside the box on its left, like the web's e-mail field; an optional
//! trailing glyph sits inside on the right.
//!
//! Desktop is immediate-mode, so the resting↔floated move is a state, not a
//! tween: the label is drawn where it belongs for the current state. Everything
//! else — the metrics, the notch gap, the per-state colours — matches the web.
//!
//! ## Headroom (the floated label stays inside `bounds`)
//!
//! The web lets the floated label bleed 6–8 px above the element
//! (`fieldset { inset: -6px 0 0 0 }`), which is harmless in a DOM whose parents
//! never clip. On the desktop a container clip cut it in half and a label row
//! placed right above touched it. So the field reserves [`HEADROOM`] above its
//! box, INSIDE its bounds: [`OutlinedField::measure`] returns
//! `HEADROOM + box height`, and every pixel the field paints lies within the
//! rectangle it is given. A caller that still hands it only the bare box height
//! (the old contract) gets a box shortened by the headroom rather than ink
//! outside its bounds.
//!
//! ## Editing
//!
//! The text model is [`kubuno_desktop_controls::text::TextBox`], reached through
//! [`Deref`] as [`crate::text::TextField`] reaches it, so `text`,
//! `placeholder_text`, `read_only`, `multiline`, `max_length`,
//! `character_casing` and the password properties are set the same way.
//!
//! On top of it this file keeps the editing state the replica does not model —
//! a caret and an anchor (a directional selection, which Shift+arrows need),
//! an undo stack, the pointer drag — and the layout of the last paint (row
//! wrapping, glyph boundaries, scroll), which is what maps a click to a
//! character. The behaviour is the browser's `<input>` / `<textarea>`:
//!
//! * typing replaces the selection; `MaxLength` refuses the excess instead of
//!   cutting the tail; a single-line field strips pasted line breaks (Chromium);
//! * Left/Right (Ctrl: by word), Home/End (Ctrl: whole text), Up/Down between
//!   visual rows in a multiline field, Shift extends; Backspace/Delete (Ctrl: by
//!   word); Ctrl+A/C/X/V, Ctrl+Insert, Shift+Insert, Ctrl+Z, Ctrl+Y /
//!   Ctrl+Shift+Z; Enter inserts a line break in a multiline field and is
//!   reported as a submit on a single-line one;
//! * a click places the caret, Shift+click extends, a drag selects, a double
//!   click selects the word, a triple click everything; Tab-ing into the field
//!   selects its whole text, as the browser does;
//! * the caret blinks ([`crate::focus::caret_visible`]) and stays solid while
//!   the user types; it hides when the window loses activation;
//! * a single-line field scrolls horizontally to keep the caret visible and
//!   goes back to its start on blur (Chromium); a multiline field wraps at
//!   word boundaries and scrolls vertically.
//!
//! The pure parts (wrapping, word boundaries, key → edit) are functions and
//! methods with unit tests; [`OutlinedField::handle_frame`] is the thin host
//! binding that feeds them the frame's pointer and key queue.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};

use kubuno_drive_desktop_app_controls::geometry::Rect;
use kubuno_drive_desktop_app_controls::Canvas;
use kubuno_desktop_controls::enums::Size;
use kubuno_desktop_controls::host::{self, vk, Cursor, Frame, InputEvent, Modifiers};
use kubuno_desktop_controls::text::TextBox;
use kubuno_desktop_controls::Control;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

use crate::focus::{caret_visible, FocusState};
use crate::widget::{Widget, WidgetState};

/// The box height at rest — the web's `FIELD_H` (`OutlinedField.tsx:105`).
pub const HEIGHT: f32 = 48.0;
/// The `large` variant, for one-question-per-screen layouts.
pub const HEIGHT_LARGE: f32 = 56.0;
/// Multiline minimum height (`:214`, the non-large `minHeight`).
pub const MULTILINE_MIN_HEIGHT: f32 = 76.0;
pub const MULTILINE_MIN_HEIGHT_LARGE: f32 = 96.0;
/// The room the floated label takes ABOVE the box, reserved inside the field's
/// bounds. The web label is 12 px on a `12 × 1.35 / 14 × 14`-ish line box
/// centred on the top border (`translateY(-6px - HALF_LEAD * FLOAT_SCALE)`),
/// so its line box reaches 8 px above the border.
pub const HEADROOM: f32 = 8.0;

/// The family-local metric table — every number with its web source.
mod m {
    /// `padX = 12` (`:100`): the text's inline inset.
    pub const PAD_X: f32 = 12.0;
    /// `paddingInlineEnd: trailing ? 34 : padX` (`:207`).
    pub const PAD_END_TRAILING: f32 = 34.0;
    /// `borderRadius: 6` (`:146`).
    pub const RADIUS: f32 = 6.0;
    /// The resting border (`1px`, `:140`).
    pub const BORDER: f32 = 1.0;
    /// The focused border. The web goes 1 → 3 px; 2 DIP reads the same weight
    /// at desktop DPI (every desktop focus ring is 2).
    pub const BORDER_FOCUS: f32 = 2.0;
    /// `padding: '0 5px'` on the legend's span (`:259`): the clear gap the notch
    /// opens on each side of the floated label.
    pub const NOTCH_GAP: f32 = 5.0;
    /// The leading icon column and its gap to the box (`gap: 12`, `:250`; the
    /// Contacts icons are drawn at 20).
    pub const ICON_COL: f32 = 20.0;
    pub const ICON_GAP: f32 = 12.0;
    /// The trailing glyph: `insetInlineEnd: 10` (`:265`), drawn at 16.
    pub const TRAILING_INSET: f32 = 10.0;
    pub const TRAILING_ICON: f32 = 16.0;
    /// The line box the caret and the selection band cover: the 13.5 px body
    /// face's own line (`ceil(13.5 × 1.33008)` = 18; the web's 14 px face takes
    /// `round(14 × 1.35)` = 19); the large variant's 21.5 px face gets 28.
    pub const LINE: f32 = 18.0;
    pub const LINE_LARGE: f32 = 28.0;
    /// A multiline field's vertical padding (`padY`, `:101`). With [`LINE`] the
    /// 76 DIP minimum holds exactly three rows, the textarea's `rows={3}`.
    pub const MULTI_PAD_Y: f32 = 11.0;
    pub const MULTI_PAD_Y_LARGE: f32 = 14.0;
    /// `disabled:opacity-60`, as `@ui/Input` fades a disabled field.
    pub const DISABLED_ALPHA: f32 = 0.6;
    /// The browser's selection highlight (no `::selection` rule on the web):
    /// the primary colour at 35 %, as `edit_box` paints it.
    pub const SELECTION_ALPHA: f32 = 0.35;
    /// The caret's width.
    pub const CARET_W: f32 = 1.0;
    /// Added to a measured label before it is handed to the ellipsis layout:
    /// DirectWrite trims a string laid out in exactly its own width.
    pub const TEXT_SLACK: f32 = 1.0;
    /// The intrinsic width: a field stretches to its container (`flex: 1`), so
    /// this is only a usable minimum.
    pub const MIN_WIDTH: f32 = 240.0;
    /// Rows a wheel notch scrolls a multiline field (`SPI_GETWHEELSCROLLLINES`'
    /// default).
    pub const WHEEL_ROWS: f32 = 3.0;
    /// The undo stack's depth.
    pub const UNDO_DEPTH: usize = 100;
    /// The system password glyph (U+25CF), WinForms' `UseSystemPasswordChar`.
    pub const PASSWORD_GLYPH: char = '\u{25CF}';
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure text helpers — character indices throughout (Unicode scalars)
// ─────────────────────────────────────────────────────────────────────────────

fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// Byte offset of character `i` (clamped to the end).
fn byte_at(s: &str, i: usize) -> usize {
    s.char_indices().nth(i).map_or(s.len(), |(b, _)| b)
}

/// The characters `a..b` of `s`.
fn slice(s: &str, a: usize, b: usize) -> &str {
    let (a, b) = (a.min(b), a.max(b));
    &s[byte_at(s, a)..byte_at(s, b)]
}

/// The class a character belongs to for word motion: blanks, word characters,
/// and punctuation (a run of `.,;` is its own "word", as in Windows edits).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Punct,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Punct
    }
}

/// Where Ctrl+Left lands from `i`: back over blanks, then to the start of the
/// run before them.
pub fn prev_word(text: &str, i: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut k = i.min(chars.len());
    while k > 0 && class(chars[k - 1]) == Class::Space {
        k -= 1;
    }
    if k == 0 {
        return 0;
    }
    let run = class(chars[k - 1]);
    while k > 0 && class(chars[k - 1]) == run {
        k -= 1;
    }
    k
}

/// Where Ctrl+Right lands from `i`: past the current run, then past the blanks
/// after it — the start of the next word, as Windows edit controls do.
pub fn next_word(text: &str, i: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut k = i.min(n);
    if k < n && class(chars[k]) != Class::Space {
        let run = class(chars[k]);
        while k < n && class(chars[k]) == run {
            k += 1;
        }
    }
    while k < n && class(chars[k]) == Class::Space && chars[k] != '\n' {
        k += 1;
    }
    k
}

/// The run of same-class characters around `i` — what a double-click selects.
/// At the end of the text it is the run before the caret.
pub fn word_range(text: &str, i: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if n == 0 {
        return (0, 0);
    }
    let at = if i >= n { n - 1 } else { i };
    if chars[at] == '\n' {
        return (at, at);
    }
    let run = class(chars[at]);
    let (mut a, mut b) = (at, at + 1);
    while a > 0 && class(chars[a - 1]) == run && chars[a - 1] != '\n' {
        a -= 1;
    }
    while b < n && class(chars[b]) == run && chars[b] != '\n' {
        b += 1;
    }
    (a, b)
}

/// Splits `text` into visual rows no wider than `width`, as character ranges
/// `(start, end)` (the `\n` itself belongs to no row).
///
/// Hard breaks always split; a row too long breaks after its last blank, or —
/// a single word wider than the row — between characters (`overflow-wrap:
/// break-word`, what a textarea does). Blanks never force a break: they hang
/// past the edge, as in the browser. An empty text, and an empty line, are one
/// empty row.
pub fn wrap_rows(text: &str, width: f32, measure: impl Fn(&str) -> f32) -> Vec<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut line_start = 0;
    loop {
        let line_end = (line_start..n).find(|&k| chars[k] == '\n').unwrap_or(n);
        if line_start == line_end {
            out.push((line_start, line_end));
        }
        let mut s = line_start;
        while s < line_end {
            let mut end = line_end;
            let mut last_break: Option<usize> = None;
            let mut k = s;
            while k < line_end {
                if chars[k] != ' ' && k > s {
                    let w = measure(&chars[s..=k].iter().collect::<String>());
                    if w > width {
                        end = match last_break {
                            Some(b) if b > s => b,
                            _ => k,
                        };
                        break;
                    }
                }
                if chars[k] == ' ' {
                    last_break = Some(k + 1);
                }
                k += 1;
            }
            out.push((s, end));
            s = end;
        }
        if line_end >= n {
            break;
        }
        line_start = line_end + 1;
        if line_start == n {
            // A trailing line break opens one last, empty row.
            out.push((n, n));
            break;
        }
    }
    out
}

/// The boundary of `xs` (monotonic x of each caret stop) nearest to `x`.
pub fn nearest_boundary(xs: &[f32], x: f32) -> usize {
    for k in 0..xs.len().saturating_sub(1) {
        if x < (xs[k] + xs[k + 1]) / 2.0 {
            return k;
        }
    }
    xs.len().saturating_sub(1)
}

/// The visual row holding caret stop `i`: the last row starting at or before
/// it, so a caret at a soft-wrap boundary belongs to the row it starts.
fn row_of(rows: &[(usize, usize)], i: usize) -> usize {
    rows.iter().rposition(|&(s, _)| s <= i).unwrap_or(0)
}

// ─────────────────────────────────────────────────────────────────────────────
// Clipboard seam — the host's in the app, a fake in the tests
// ─────────────────────────────────────────────────────────────────────────────

/// Where Ctrl+C/X/V go. [`OutlinedField::handle_frame`] uses the host's
/// clipboard; tests pass their own.
pub trait Clipboard {
    fn get(&mut self) -> Option<String>;
    fn set(&mut self, text: &str);
}

/// The system clipboard (`kubuno_desktop_controls::host`).
pub struct HostClipboard;

impl Clipboard for HostClipboard {
    fn get(&mut self) -> Option<String> {
        host::clipboard_text()
    }
    fn set(&mut self, text: &str) {
        host::set_clipboard_text(text);
    }
}

/// What a key did to the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutcome {
    /// Not a key the field acts on — it stays for the next consumer.
    Ignored,
    /// The caret or the selection moved (or a copy happened).
    Moved,
    /// The text changed.
    Edited,
    /// Enter on a single-line field — the form's submit.
    Submit,
}

/// What [`OutlinedField::handle_frame`] saw this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldResponse {
    /// The text changed.
    pub changed: bool,
    /// Enter was pressed in a single-line field.
    pub submitted: bool,
    /// The left button went down inside the box this frame.
    pub pressed: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Editing state and the last paint's layout
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct Snap {
    text: String,
    caret: usize,
    anchor: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum EditKind {
    #[default]
    None,
    Typing,
    Other,
}

#[derive(Default)]
struct Edit {
    caret: usize,
    anchor: usize,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    last: EditKind,
    /// A left-button drag that started in the box is selecting.
    drag: bool,
    /// The x a run of Up/Down keeps aiming at (the browser's sticky column).
    goal_x: Option<f32>,
    /// `host::now_ms` at the last edit or caret move: the blink restarts there.
    last_input_ms: u64,
    /// `None` for a field nobody drives (a static exposition: solid caret),
    /// `Some(window_focused)` once [`OutlinedField::handle_frame`] ran.
    live: Option<bool>,
    /// The wheel scrolled a multiline field: its rows stay where the wheel
    /// put them until the caret or the text moves again (a textarea's scroll
    /// is independent of its caret).
    free_scroll: bool,
}

/// One visual row of the last layout: its character range and the x of every
/// caret stop in it, from the row's start.
#[derive(Clone)]
struct Row {
    start: usize,
    end: usize,
    xs: Vec<f32>,
}

#[derive(Default)]
struct View {
    /// What the rows were computed for: the display string, the wrap width's
    /// bits, `large`, `multiline`.
    key: Option<(String, u32, bool, bool)>,
    rows: Vec<Row>,
    /// The text area of the last paint, and its line box.
    area: Option<Rect>,
    line: f32,
    scroll_x: f32,
    scroll_y: f32,
}

impl View {
    fn ranges(&self) -> Vec<(usize, usize)> {
        self.rows.iter().map(|r| (r.start, r.end)).collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// OutlinedField
// ─────────────────────────────────────────────────────────────────────────────

/// A Material outlined field.
#[derive(Default)]
pub struct OutlinedField {
    inner: TextBox,
    /// The floating label — the field's resting hint and its floated title.
    pub label: String,
    /// A glyph outside the box on its left, by name from the icon set.
    pub leading_icon: Option<&'static str>,
    /// A glyph inside the box on its right (a chevron for a select trigger…).
    pub trailing_icon: Option<&'static str>,
    /// Adds a red asterisk to the label (`required`, `:262`).
    pub required: bool,
    /// The `large` variant (`:98`).
    pub large: bool,
    /// The border and label turn `danger` (the web `error` prop's counterpart,
    /// as on [`crate::text::TextField`]).
    pub invalid: bool,
    edit: Edit,
    view: RefCell<View>,
}

impl Deref for OutlinedField {
    type Target = TextBox;
    fn deref(&self) -> &TextBox {
        &self.inner
    }
}

impl DerefMut for OutlinedField {
    fn deref_mut(&mut self) -> &mut TextBox {
        &mut self.inner
    }
}

impl OutlinedField {
    pub fn new(label: &str) -> Self {
        Self { label: label.into(), ..Self::default() }
    }

    /// Builder: the leading icon.
    pub fn with_leading(mut self, icon: &'static str) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    /// Builder: the trailing glyph.
    pub fn with_trailing(mut self, icon: &'static str) -> Self {
        self.trailing_icon = Some(icon);
        self
    }

    /// Builder: the `large` variant.
    pub fn large(mut self, v: bool) -> Self {
        self.large = v;
        self
    }

    /// Builder: the required asterisk.
    pub fn required(mut self, v: bool) -> Self {
        self.required = v;
        self
    }

    /// Builder: the initial value, caret at its end.
    pub fn with_value(mut self, value: &str) -> Self {
        self.set_value(value);
        self
    }

    /// The room reserved above the box for the floated label — [`HEADROOM`].
    /// A layout that stacks fields adds it to the box height; [`Widget::measure`]
    /// already does.
    pub fn headroom() -> f32 {
        HEADROOM
    }

    fn box_height(&self) -> f32 {
        if self.multiline {
            if self.large { MULTILINE_MIN_HEIGHT_LARGE } else { MULTILINE_MIN_HEIGHT }
        } else if self.large {
            HEIGHT_LARGE
        } else {
            HEIGHT
        }
    }

    /// The whole height the field asks for: headroom plus box.
    pub fn outer_height(&self) -> f32 {
        HEADROOM + self.box_height()
    }

    /// The label floats up when the field is focused or holds a value — a filled
    /// field keeps its label up even unfocused (`floated`, `:88`).
    pub fn is_floated(&self, state: WidgetState) -> bool {
        state.focused || !self.text.is_empty()
    }

    /// The outlined box: [`HEADROOM`] below `bounds.top`, the leading-icon
    /// column removed from the left. A single-line box keeps its height; a
    /// multiline one grows to fill taller bounds (a textarea stretched by its
    /// layout). It never reaches past `bounds.bottom`.
    pub fn box_rect(&self, bounds: Rect) -> Rect {
        let left = if self.leading_icon.is_some() { bounds.left + m::ICON_COL + m::ICON_GAP } else { bounds.left };
        let top = (bounds.top + HEADROOM).min(bounds.bottom);
        let natural = top + self.box_height();
        let bottom = if self.multiline { natural.max(bounds.bottom) } else { natural };
        Rect::new(left.min(bounds.right), top, bounds.right, bottom.min(bounds.bottom).max(top))
    }

    /// The rectangle the value is laid out in: the box minus its paddings.
    /// Single-line: the whole box height (the line is centred in it);
    /// multiline: inset by the vertical padding.
    pub fn text_rect(&self, bounds: Rect) -> Rect {
        let bx = self.box_rect(bounds);
        let left = bx.left + m::PAD_X;
        let right = bx.right - if self.trailing_icon.is_some() { m::PAD_END_TRAILING } else { m::PAD_X };
        let (top, bottom) = if self.multiline {
            let pad = if self.large { m::MULTI_PAD_Y_LARGE } else { m::MULTI_PAD_Y };
            (bx.top + pad, (bx.bottom - pad).max(bx.top + pad))
        } else {
            (bx.top, bx.bottom)
        };
        Rect::new(left, top, right.max(left), bottom)
    }

    fn line_height(&self) -> f32 {
        if self.large { m::LINE_LARGE } else { m::LINE }
    }

    /// The face the value and the resting label use: the body role (13.5 px;
    /// the web field sets 14), or the title role (21.5 px) for `large` (the
    /// web's 20). Roles, not ad hoc sizes: the design system has six steps.
    fn font<'a>(&self, c: &'a dyn Canvas) -> &'a IDWriteTextFormat {
        if self.large { &c.formats().title } else { &c.formats().body }
    }

    /// The border colour for a state (`:125-127`).
    fn border_colour(&self, t: &crate::Theme, state: WidgetState, floated: bool) -> D2D1_COLOR_F {
        if self.invalid {
            t.danger
        } else if state.focused {
            t.accent
        } else if floated {
            t.border_strong
        } else {
            t.text_tertiary
        }
    }

    // ── The value and the selection (character indices) ─────────────────────

    /// The characters drawn: the password glyph repeated, or the text. As in
    /// WinForms, masking is a single-line concern.
    pub fn display(&self) -> String {
        let glyph = if self.use_system_password_char { Some(m::PASSWORD_GLYPH) } else { self.password_char };
        match glyph {
            Some(g) if !self.multiline => g.to_string().repeat(char_len(&self.text)),
            _ => self.text.clone(),
        }
    }

    fn is_password(&self) -> bool {
        !self.multiline && (self.use_system_password_char || self.password_char.is_some())
    }

    /// Replaces the value programmatically (no undo entry, `Modified` cleared),
    /// caret at the end — what assigning `value` does on the web.
    pub fn set_value(&mut self, value: &str) {
        self.inner.set_text(value);
        let n = char_len(&self.text);
        self.edit.caret = n;
        self.edit.anchor = n;
        self.edit.undo.clear();
        self.edit.redo.clear();
        self.edit.last = EditKind::None;
        self.sync_selection();
    }

    /// The caret, in characters.
    pub fn caret(&self) -> usize {
        self.edit.caret.min(char_len(&self.text))
    }

    /// The selection as `(start, end)`, ordered, in characters.
    pub fn selection(&self) -> (usize, usize) {
        let n = char_len(&self.text);
        let (a, c) = (self.edit.anchor.min(n), self.edit.caret.min(n));
        (a.min(c), a.max(c))
    }

    /// Selects `anchor..caret` (either direction), clamped.
    pub fn select_range(&mut self, anchor: usize, caret: usize) {
        let n = char_len(&self.text);
        self.edit.anchor = anchor.min(n);
        self.edit.caret = caret.min(n);
        self.edit.goal_x = None;
        self.edit.last = EditKind::None;
        self.sync_selection();
    }

    /// Selects the whole value (Ctrl+A, and Tab-ing in).
    pub fn select_all_text(&mut self) {
        let n = char_len(&self.text);
        self.select_range(0, n);
    }

    /// The selected characters.
    pub fn selected(&self) -> String {
        let (a, b) = self.selection();
        slice(&self.text, a, b).to_string()
    }

    /// Mirrors the selection into the replica (`SelectionStart/Length`), so a
    /// caller reading the model sees what the user selected.
    fn sync_selection(&mut self) {
        let (a, b) = self.selection();
        self.inner.select(a as i32, (b - a) as i32);
    }

    fn clamp(&mut self) {
        let n = char_len(&self.text);
        self.edit.caret = self.edit.caret.min(n);
        self.edit.anchor = self.edit.anchor.min(n);
    }

    fn move_to(&mut self, i: usize, extend: bool) {
        self.edit.caret = i;
        if !extend {
            self.edit.anchor = i;
        }
        self.edit.last = EditKind::None;
        self.sync_selection();
    }

    fn snapshot(&self) -> Snap {
        Snap { text: self.text.clone(), caret: self.edit.caret, anchor: self.edit.anchor }
    }

    fn restore(&mut self, s: Snap) {
        self.inner.set_text(&s.text);
        self.inner.modified = true;
        self.edit.caret = s.caret;
        self.edit.anchor = s.anchor;
        self.clamp();
        self.sync_selection();
    }

    /// Replaces the selection with `s` — the one edit primitive. Typing runs
    /// coalesce into one undo step. Returns whether the text changed.
    fn replace_selection(&mut self, s: &str, kind: EditKind) -> bool {
        if self.read_only {
            return false;
        }
        self.clamp();
        // Line breaks: a single-line field drops them (Chromium strips them
        // from a paste); a multiline one normalises CRLF to LF.
        let cleaned: String = if self.multiline {
            s.replace("\r\n", "\n").replace('\r', "\n")
        } else {
            s.chars().filter(|&c| c != '\r' && c != '\n').collect()
        };
        let (a, b) = self.selection();
        let n = char_len(&self.text);
        // MaxLength refuses the excess (the browser's `maxlength`), rather than
        // cutting the tail as the replica's setter would.
        let insert: String = if self.max_length > 0 {
            let room = (self.max_length as usize).saturating_sub(n - (b - a));
            cleaned.chars().take(room).collect()
        } else {
            cleaned
        };
        if insert.is_empty() && a == b {
            return false;
        }
        let coalesce = kind == EditKind::Typing && self.edit.last == EditKind::Typing && a == b;
        if !coalesce {
            let snap = self.snapshot();
            self.edit.undo.push(snap);
            if self.edit.undo.len() > m::UNDO_DEPTH {
                self.edit.undo.remove(0);
            }
        }
        self.edit.redo.clear();
        let new = format!("{}{}{}", slice(&self.text, 0, a), insert, slice(&self.text, b, n));
        // The replica's setter applies CharacterCasing (one char for one char,
        // so the indices hold).
        self.inner.set_text(&new);
        self.inner.modified = true;
        let caret = (a + char_len(&insert)).min(char_len(&self.text));
        self.edit.caret = caret;
        self.edit.anchor = caret;
        self.edit.goal_x = None;
        self.edit.last = kind;
        self.sync_selection();
        true
    }

    /// Types `s` at the caret (replacing the selection). Returns whether the
    /// text changed.
    pub fn type_text(&mut self, s: &str) -> bool {
        self.replace_selection(s, EditKind::Typing)
    }

    /// Ctrl+Z.
    pub fn undo(&mut self) -> bool {
        match self.edit.undo.pop() {
            Some(s) => {
                let now = self.snapshot();
                self.edit.redo.push(now);
                self.restore(s);
                self.edit.last = EditKind::None;
                true
            }
            None => false,
        }
    }

    /// Ctrl+Y / Ctrl+Shift+Z.
    pub fn redo(&mut self) -> bool {
        match self.edit.redo.pop() {
            Some(s) => {
                let now = self.snapshot();
                self.edit.undo.push(now);
                self.restore(s);
                self.edit.last = EditKind::None;
                true
            }
            None => false,
        }
    }

    /// The visual rows and the x of each caret stop from the last paint, or —
    /// before any paint, or when the text changed since — the hard lines with
    /// one unit per character (enough for the key logic to stay correct).
    fn rows_for_keys(&self) -> Vec<Row> {
        let display = self.display();
        let view = self.view.borrow();
        if view.key.as_ref().is_some_and(|k| k.0 == display) && !view.rows.is_empty() {
            return view.rows.clone();
        }
        let ranges = if self.multiline { wrap_rows(&display, f32::INFINITY, |_| 0.0) } else { vec![(0, char_len(&display))] };
        ranges
            .into_iter()
            .map(|(s, e)| Row { start: s, end: e, xs: (0..=(e - s)).map(|k| k as f32).collect() })
            .collect()
    }

    /// Whether the field acts on `vk` with `mods` — decided before the key is
    /// taken from the host queue, so an ignored key stays for the page.
    pub fn handles_key(&self, key: u16, mods: Modifiers) -> bool {
        let plain_or_shift = mods.matches(Modifiers::NONE) || mods.matches(Modifiers::SHIFT);
        let ctrl_any = mods.matches(Modifiers::CTRL) || mods.matches(Modifiers::CTRL_SHIFT);
        match key {
            vk::LEFT | vk::RIGHT | vk::HOME | vk::END => plain_or_shift || ctrl_any,
            vk::UP | vk::DOWN => self.multiline && plain_or_shift,
            vk::BACK | vk::DELETE => !self.read_only && (mods.matches(Modifiers::NONE) || mods.matches(Modifiers::CTRL) || mods.matches(Modifiers::SHIFT)),
            vk::ENTER => plain_or_shift,
            vk::INSERT => mods.matches(Modifiers::CTRL) || (!self.read_only && mods.matches(Modifiers::SHIFT)),
            k if k == vk::letter('a') || k == vk::letter('c') => mods.matches(Modifiers::CTRL),
            k if k == vk::letter('x') || k == vk::letter('v') || k == vk::letter('y') => {
                !self.read_only && mods.matches(Modifiers::CTRL)
            }
            k if k == vk::letter('z') => !self.read_only && ctrl_any,
            _ => false,
        }
    }

    /// Acts on one key-down. Pure but for `clip`: the whole keyboard
    /// behaviour of the field, testable without a window.
    pub fn key(&mut self, key: u16, mods: Modifiers, clip: &mut dyn Clipboard) -> KeyOutcome {
        if !self.handles_key(key, mods) {
            return KeyOutcome::Ignored;
        }
        self.clamp();
        let n = char_len(&self.text);
        let shift = mods.shift;
        let word = mods.ctrl;
        let (a, b) = self.selection();
        let caret = self.edit.caret;
        let text = self.text.clone();
        let moved = KeyOutcome::Moved;
        let edited = |changed: bool| if changed { KeyOutcome::Edited } else { KeyOutcome::Moved };
        match key {
            vk::LEFT => {
                self.edit.goal_x = None;
                let to = if a != b && !shift {
                    a
                } else if word {
                    prev_word(&text, caret)
                } else {
                    caret.saturating_sub(1)
                };
                self.move_to(to, shift);
                moved
            }
            vk::RIGHT => {
                self.edit.goal_x = None;
                let to = if a != b && !shift {
                    b
                } else if word {
                    next_word(&text, caret)
                } else {
                    (caret + 1).min(n)
                };
                self.move_to(to, shift);
                moved
            }
            vk::HOME | vk::END => {
                self.edit.goal_x = None;
                let home = key == vk::HOME;
                let to = if word || !self.multiline {
                    if home { 0 } else { n }
                } else {
                    let rows = self.rows_for_keys();
                    let ranges: Vec<(usize, usize)> = rows.iter().map(|r| (r.start, r.end)).collect();
                    let r = row_of(&ranges, caret);
                    let (s, e) = ranges[r];
                    if home {
                        s
                    } else if r + 1 < ranges.len() && ranges[r + 1].0 == e && e > s {
                        // A soft-wrapped row: its end IS the next row's start,
                        // so stop before the hanging blank to stay on this row.
                        e - 1
                    } else {
                        e
                    }
                };
                self.move_to(to, shift);
                moved
            }
            vk::UP | vk::DOWN => {
                let rows = self.rows_for_keys();
                let ranges: Vec<(usize, usize)> = rows.iter().map(|r| (r.start, r.end)).collect();
                let r = row_of(&ranges, caret);
                let row = &rows[r];
                let x = self.edit.goal_x.unwrap_or_else(|| row.xs.get(caret - row.start).copied().unwrap_or(0.0));
                let to = if key == vk::UP {
                    if r == 0 {
                        0
                    } else {
                        let t = &rows[r - 1];
                        t.start + nearest_boundary(&t.xs, x)
                    }
                } else if r + 1 >= rows.len() {
                    n
                } else {
                    let t = &rows[r + 1];
                    t.start + nearest_boundary(&t.xs, x)
                };
                self.move_to(to, shift);
                self.edit.goal_x = Some(x);
                moved
            }
            vk::BACK => {
                if a == b && a > 0 {
                    let from = if word { prev_word(&text, a) } else { a - 1 };
                    self.edit.anchor = from;
                    self.edit.caret = a;
                }
                edited(self.replace_selection("", EditKind::Other))
            }
            vk::DELETE => {
                if a == b && a < n {
                    let to = if word { next_word(&text, a) } else { a + 1 };
                    self.edit.anchor = a;
                    self.edit.caret = to;
                }
                edited(self.replace_selection("", EditKind::Other))
            }
            vk::ENTER => {
                if self.multiline {
                    edited(self.replace_selection("\n", EditKind::Other))
                } else {
                    KeyOutcome::Submit
                }
            }
            vk::INSERT => {
                if mods.ctrl {
                    self.copy(clip);
                    moved
                } else {
                    self.paste(clip)
                }
            }
            k if k == vk::letter('a') => {
                self.select_all_text();
                moved
            }
            k if k == vk::letter('c') => {
                self.copy(clip);
                moved
            }
            k if k == vk::letter('x') => {
                if self.is_password() || a == b {
                    return moved;
                }
                self.copy(clip);
                edited(self.replace_selection("", EditKind::Other))
            }
            k if k == vk::letter('v') => self.paste(clip),
            k if k == vk::letter('y') => edited(self.redo()),
            k if k == vk::letter('z') => edited(if shift { self.redo() } else { self.undo() }),
            _ => KeyOutcome::Ignored,
        }
    }

    /// Copies the selection. A password field never copies (the browser
    /// refuses to put a masked value on the clipboard).
    fn copy(&self, clip: &mut dyn Clipboard) {
        let (a, b) = self.selection();
        if a != b && !self.is_password() {
            clip.set(slice(&self.text, a, b));
        }
    }

    fn paste(&mut self, clip: &mut dyn Clipboard) -> KeyOutcome {
        match clip.get() {
            Some(s) if !s.is_empty() => {
                if self.replace_selection(&s, EditKind::Other) {
                    KeyOutcome::Edited
                } else {
                    KeyOutcome::Moved
                }
            }
            _ => KeyOutcome::Moved,
        }
    }

    /// The caret stop under a point, from the last paint's layout.
    pub fn index_at(&self, x: f32, y: f32) -> usize {
        let view = self.view.borrow();
        let Some(area) = view.area else {
            return self.caret();
        };
        if view.rows.is_empty() {
            return 0;
        }
        let r = if self.multiline {
            let row = ((y - area.top + view.scroll_y) / view.line.max(1.0)).floor();
            (row.max(0.0) as usize).min(view.rows.len() - 1)
        } else {
            0
        };
        let row = &view.rows[r];
        let local = x - area.left + view.scroll_x;
        row.start + nearest_boundary(&row.xs, local)
    }

    /// Pointer press on the box: place the caret (Shift extends), or select the
    /// word (double click) or everything (triple click).
    pub fn press_at(&mut self, x: f32, y: f32, click_count: u8, shift: bool) {
        let i = self.index_at(x, y);
        self.edit.goal_x = None;
        self.edit.last = EditKind::None;
        match click_count {
            0 | 1 => {
                self.move_to(i, shift);
                self.edit.drag = true;
            }
            2 => {
                let (a, b) = word_range(&self.text, i);
                self.select_range(a, b);
                self.edit.drag = false;
            }
            _ => {
                self.select_all_text();
                self.edit.drag = false;
            }
        }
    }

    /// One frame of host input for a live field: pointer (caret, drag,
    /// multi-click, cursor shape) always, keys and typed text while `focus`
    /// says it holds the focus. `pressed` is the left button's rising edge
    /// this frame (the caller resolves it once, e.g. the gallery's
    /// `Live::clicked`).
    pub fn handle_frame(&mut self, bounds: Rect, focus: FocusState, f: &Frame, pressed: bool) -> FieldResponse {
        let mut out = FieldResponse::default();
        let enabled = self.enabled;
        self.edit.live = Some(f.window_focused);
        let bx = self.box_rect(bounds);
        let (mx, my) = f.mouse;
        let over = bx.contains(mx, my);
        let before = (self.text.clone(), self.selection());

        if enabled && (over || self.edit.drag) {
            host::set_cursor(if self.read_only && !self.edit.drag { Cursor::Hand } else { Cursor::IBeam });
        }
        if enabled && pressed && over {
            out.pressed = true;
            self.press_at(mx, my, f.click_count, f.mods.shift);
        } else if self.edit.drag {
            if f.mouse_down && focus.focused {
                let i = self.index_at(mx, my);
                self.move_to(i, true);
            } else {
                self.edit.drag = false;
            }
        }
        // Tab-ing into a field selects its value, as the browser does; a click
        // placed the caret itself above.
        if focus.gained && !out.pressed {
            self.select_all_text();
        }

        if focus.focused && enabled {
            let read_only = self.read_only;
            // Decide per event with the same rule `key` applies, so what is
            // taken is exactly what is handled, and the rest stays queued.
            let taken = {
                let this = &*self;
                host::consume(|e| match e {
                    InputEvent::Text(_) => !read_only,
                    InputEvent::Key { vk: k, down: true, mods, .. } => this.handles_key(*k, *mods),
                    _ => false,
                })
            };
            let mut clip = HostClipboard;
            for e in taken {
                match e {
                    InputEvent::Text(s) => {
                        out.changed |= self.type_text(&s);
                    }
                    InputEvent::Key { vk: k, mods, .. } => match self.key(k, mods, &mut clip) {
                        KeyOutcome::Edited => out.changed = true,
                        KeyOutcome::Submit => out.submitted = true,
                        _ => {}
                    },
                    _ => {}
                }
            }
        } else {
            self.edit.drag = false;
        }

        if focus.gained || (self.text.as_str(), self.selection()) != (before.0.as_str(), before.1) {
            self.edit.last_input_ms = host::now_ms();
            self.edit.free_scroll = false;
        }

        // The wheel scrolls a multiline field under the pointer by three rows
        // a notch (the Windows default), whether or not it has the focus.
        if self.multiline && over && f.wheel.1 != 0.0 {
            let mut view = self.view.borrow_mut();
            let line = view.line.max(1.0);
            view.scroll_y += f.wheel.1 * m::WHEEL_ROWS * line;
            self.edit.free_scroll = true;
        }
        out
    }

    // ── Painting ────────────────────────────────────────────────────────────

    /// Recomputes the cached rows when the value, the width or the variant
    /// changed, then records this paint's area.
    fn layout(&self, c: &dyn Canvas, area: Rect, display: &str) {
        let font = self.font(c);
        let width = (area.right - area.left).max(1.0);
        let key = (display.to_string(), width.to_bits(), self.large, self.multiline);
        let mut view = self.view.borrow_mut();
        if view.key.as_ref() != Some(&key) {
            let ranges = if self.multiline {
                wrap_rows(display, width, |s| c.measure(s, font))
            } else {
                vec![(0, char_len(display))]
            };
            view.rows = ranges
                .into_iter()
                .map(|(s, e)| {
                    let row = slice(display, s, e);
                    let mut xs = Vec::with_capacity(e - s + 1);
                    xs.push(0.0);
                    for (b, ch) in row.char_indices() {
                        xs.push(c.measure(&row[..b + ch.len_utf8()], font));
                    }
                    Row { start: s, end: e, xs }
                })
                .collect();
            view.key = Some(key);
        }
        view.area = Some(area);
        view.line = self.line_height();
    }

    /// Keeps the caret inside the text area: horizontal scroll for a single
    /// line (back to the start on blur, as Chromium does), vertical for a
    /// multiline field (left where the wheel put it until the caret moves).
    fn scroll_to_caret(&self, editing: bool) {
        let caret = self.caret();
        let multiline = self.multiline;
        let free = self.edit.free_scroll;
        let mut view = self.view.borrow_mut();
        let Some(area) = view.area else { return };
        let (w, h) = (area.right - area.left, area.bottom - area.top);
        let ranges = view.ranges();
        let r = row_of(&ranges, caret);
        let Some(row) = view.rows.get(r) else { return };
        let x = row.xs.get(caret - row.start).copied().unwrap_or(0.0);
        let total = row.xs.last().copied().unwrap_or(0.0);
        if multiline {
            let line = view.line;
            let top = r as f32 * line;
            let content = view.rows.len() as f32 * line;
            let mut sy = view.scroll_y;
            if editing && !free {
                if top < sy {
                    sy = top;
                }
                if top + line > sy + h {
                    sy = top + line - h;
                }
            }
            view.scroll_y = sy.clamp(0.0, (content - h).max(0.0));
            view.scroll_x = 0.0;
        } else if !editing {
            view.scroll_x = 0.0;
            view.scroll_y = 0.0;
        } else {
            let inner = (w - m::CARET_W).max(1.0);
            let mut sx = view.scroll_x;
            if x - sx > inner {
                sx = x - inner;
            }
            if x < sx {
                sx = x;
            }
            view.scroll_x = sx.clamp(0.0, (total - inner).max(0.0));
            view.scroll_y = 0.0;
        }
    }

    /// Paints the value, the selection band and the caret in the text area.
    fn paint_value(&self, c: &dyn Canvas, bx: Rect, display: &str, editing: bool, ink: &D2D1_COLOR_F) {
        let t = c.theme();
        let font = self.font(c);
        let view = self.view.borrow();
        let Some(area) = view.area else { return };
        let line = view.line;
        let (sel_a, sel_b) = if editing { self.selection() } else { (0, 0) };
        let caret = self.caret();
        let show_caret = editing
            && match self.edit.live {
                None => true,
                Some(false) => false,
                Some(true) => caret_visible(self.edit.last_input_ms),
            };
        let mut band = t.accent;
        band.a *= m::SELECTION_ALPHA;

        // Clip to the padding box: the value scrolls under the paddings, never
        // over the border.
        let clip = Rect::new(area.left - m::CARET_W, bx.top + m::BORDER, area.right + m::CARET_W, bx.bottom - m::BORDER);
        c.push_clip(&clip);
        let ranges = view.ranges();
        let caret_row = row_of(&ranges, caret);
        for (i, row) in view.rows.iter().enumerate() {
            let (top, bottom) = if self.multiline {
                let top = area.top + i as f32 * line - view.scroll_y;
                (top, top + line)
            } else {
                let cy = (area.top + area.bottom) / 2.0;
                (cy - line / 2.0, cy + line / 2.0)
            };
            if bottom < clip.top || top > clip.bottom {
                continue;
            }
            let origin = area.left - view.scroll_x;
            let x_of = |k: usize| origin + row.xs.get(k.saturating_sub(row.start).min(row.xs.len() - 1)).copied().unwrap_or(0.0);
            // The selection band over this row's share of the selection.
            let (a, b) = (sel_a.max(row.start), sel_b.min(row.end));
            if sel_a != sel_b && a < b {
                c.fill_rounded(&Rect::new(x_of(a), top, x_of(b), bottom), 0.0, &band);
            } else if sel_a != sel_b && sel_a <= row.end && sel_b > row.end && self.multiline {
                // A selected line break shows as a sliver, as in a textarea.
                let x = x_of(row.end);
                c.fill_rounded(&Rect::new(x, top, x + 4.0, bottom), 0.0, &band);
            }
            let text = slice(display, row.start, row.end);
            if !text.is_empty() {
                let w = row.xs.last().copied().unwrap_or(0.0);
                let r = if self.multiline {
                    Rect::new(origin, top, origin + w + m::PAD_X, bottom)
                } else {
                    Rect::new(origin, area.top, origin + w + m::PAD_X, area.bottom)
                };
                c.text(text, &r, font, ink, false);
            }
            if show_caret && i == caret_row {
                let x = x_of(caret);
                c.fill_rounded(&Rect::new(x, top, x + m::CARET_W, bottom), 0.0, &t.text_primary);
            }
        }
        c.pop_clip();
    }
}

fn faded(c: D2D1_COLOR_F, disabled: bool) -> D2D1_COLOR_F {
    if disabled {
        D2D1_COLOR_F { a: c.a * m::DISABLED_ALPHA, ..c }
    } else {
        c
    }
}

impl Widget for OutlinedField {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn type_name(&self) -> &'static str {
        "OutlinedField"
    }

    /// The width is only a usable minimum (a field stretches to its container,
    /// `flex: 1`); the height is the headroom plus the box — the TRUE height of
    /// everything the field paints.
    fn measure(&self, _canvas: &dyn Canvas) -> Size {
        let icon = if self.leading_icon.is_some() { m::ICON_COL + m::ICON_GAP } else { 0.0 };
        Size::new(m::MIN_WIDTH + icon, self.outer_height())
    }

    fn paint(&self, c: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = c.theme();
        let f = c.formats();
        let floated = self.is_floated(state);
        let disabled = state.disabled || !self.enabled;
        let editing = state.focused && !disabled;
        let bx = self.box_rect(bounds);
        if bx.bottom - bx.top < 1.0 || bx.right - bx.left < 1.0 {
            return;
        }
        let area = self.text_rect(bounds);
        let display = self.display();

        // Leading icon, outside the box on its left: centred on a single line,
        // on the first row of a multiline field (`marginTop: padY + 4`).
        if let Some(icon) = self.leading_icon {
            let cy = if self.multiline { area.top + self.line_height() / 2.0 } else { (bx.top + bx.bottom) / 2.0 };
            let half = m::ICON_COL / 2.0;
            let r = Rect::new(bounds.left, cy - half, bounds.left + m::ICON_COL, cy + half);
            c.vector_icon(icon, &r, m::ICON_COL, &faded(t.text_secondary, disabled));
        }

        // The outline.
        let border = faded(self.border_colour(t, state, floated), disabled);
        let width = if editing { m::BORDER_FOCUS } else { m::BORDER };
        c.stroke_rounded_w(&bx, m::RADIUS, &border, width);

        // The value, laid out and scrolled to the caret.
        self.layout(c, area, &display);
        self.scroll_to_caret(editing);
        let ink = faded(t.text_primary, disabled);
        let caret_owner = editing && !self.read_only;
        if !display.is_empty() {
            self.paint_value(c, bx, &display, caret_owner, &ink);
        } else if caret_owner {
            // The empty field still shows its caret (and, focused, the web's
            // `placeholder` — shown only while floated, `:227`).
            if !self.placeholder_text.is_empty() {
                let row = if self.multiline {
                    Rect::new(area.left, area.top, area.right, area.top + self.line_height())
                } else {
                    area
                };
                c.text_ellipsis(&self.placeholder_text, &row, self.font(c), &t.text_tertiary);
            }
            self.paint_value(c, bx, &display, true, &ink);
        }

        // The label.
        let label_colour = faded(
            if self.invalid {
                t.danger
            } else if state.focused {
                t.accent
            } else {
                t.text_secondary
            },
            disabled,
        );
        let star = " *";
        let danger = faded(t.danger, disabled);
        if floated {
            // 12 px on the top border, its notch cleared; ellipsised to the box
            // (`maxWidth: calc(100% - 24px)`).
            let max = (bx.right - bx.left - 2.0 * m::PAD_X).max(0.0);
            let star_w = if self.required { c.measure(star, &f.caption) } else { 0.0 };
            // The label's layout gets all the room there is, so DirectWrite
            // trims only a label that really overflows; its MEASURED width
            // (clamped to that room) places the star and sizes the notch.
            let room = (max - star_w).max(0.0);
            let label_w = (c.measure(&self.label, &f.caption).ceil() + m::TEXT_SLACK).min(room);
            let text_w = label_w + star_w;
            let nx = bx.left + m::PAD_X - m::NOTCH_GAP;
            let notch = Rect::new(nx, bx.top - m::BORDER, nx + text_w + 2.0 * m::NOTCH_GAP, bx.top + m::BORDER_FOCUS + 0.5);
            // The notch: the ground the field sits on, so the border opens
            // around the label on any surface.
            c.fill_rounded(&notch, 0.0, &c.current_bg());
            let lx = bx.left + m::PAD_X;
            let lr = Rect::new(lx, bx.top - HEADROOM, lx + room, bx.top + HEADROOM);
            c.text_ellipsis(&self.label, &lr, &f.caption, &label_colour);
            if self.required {
                let sr = Rect::new(lx + label_w, lr.top, lx + label_w + star_w + 1.0, lr.bottom);
                c.text(star, &sr, &f.caption, &danger, false);
            }
        } else {
            // Resting: the label is the hint, on the very text it stands in
            // for, in `--color-text-secondary` (`:128`).
            let font = self.font(c);
            let row = if self.multiline {
                Rect::new(area.left, area.top, area.right, area.top + self.line_height())
            } else {
                area
            };
            let star_w = if self.required { c.measure(star, font) } else { 0.0 };
            let room = (row.right - row.left - star_w).max(0.0);
            let label_w = (c.measure(&self.label, font).ceil() + m::TEXT_SLACK).min(room);
            c.text_ellipsis(&self.label, &Rect::new(row.left, row.top, row.left + room, row.bottom), font, &label_colour);
            if self.required {
                let sr = Rect::new(row.left + label_w, row.top, row.left + label_w + star_w + 1.0, row.bottom);
                c.text(star, &sr, font, &danger, false);
            }
        }

        // Trailing glyph inside the box on the right.
        if let Some(icon) = self.trailing_icon {
            let cy = (bx.top + bx.bottom) / 2.0;
            let half = m::TRAILING_ICON / 2.0;
            let r = Rect::new(bx.right - m::TRAILING_INSET - m::TRAILING_ICON, cy - half, bx.right - m::TRAILING_INSET, cy + half);
            c.vector_icon(icon, &r, m::TRAILING_ICON, &faded(t.text_secondary, disabled));
        }
    }

    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        self.box_rect(bounds).contains(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clipboard that lives in the test.
    #[derive(Default)]
    struct Fake(Option<String>);
    impl Clipboard for Fake {
        fn get(&mut self) -> Option<String> {
            self.0.clone()
        }
        fn set(&mut self, text: &str) {
            self.0 = Some(text.to_string());
        }
    }

    fn field(text: &str) -> OutlinedField {
        OutlinedField::new("x").with_value(text)
    }

    fn press(f: &mut OutlinedField, key: u16, mods: Modifiers) -> KeyOutcome {
        f.key(key, mods, &mut Fake::default())
    }

    #[test]
    fn a_filled_field_keeps_its_label_floated_even_unfocused() {
        let mut f = OutlinedField::new("Nom");
        assert!(!f.is_floated(WidgetState::REST), "empty and unfocused rests");
        assert!(f.is_floated(WidgetState::REST.focused(true)), "focus floats it");
        f.text = "Ada".into();
        assert!(f.is_floated(WidgetState::REST), "a value keeps it floated when unfocused");
    }

    #[test]
    fn the_large_variant_is_taller() {
        assert!(OutlinedField::new("x").large(true).box_height() > OutlinedField::new("x").box_height());
    }

    #[test]
    fn a_leading_icon_pushes_the_box_right_by_its_column() {
        let bounds = Rect::new(0.0, 0.0, 300.0, HEADROOM + HEIGHT);
        let plain = OutlinedField::new("x");
        let iconed = OutlinedField::new("x").with_leading("Mail");
        assert_eq!(plain.box_rect(bounds).left, 0.0);
        assert!(iconed.box_rect(bounds).left > 0.0, "the box starts after the icon column");
    }

    #[test]
    fn multiline_is_taller_than_a_single_line() {
        let mut f = OutlinedField::new("x");
        f.multiline = true;
        assert!(f.box_height() >= MULTILINE_MIN_HEIGHT);
    }

    #[test]
    fn the_floated_label_room_is_inside_the_bounds() {
        let f = OutlinedField::new("x");
        assert_eq!(f.outer_height(), HEADROOM + HEIGHT);
        let bounds = Rect::new(0.0, 100.0, 300.0, 100.0 + f.outer_height());
        let bx = f.box_rect(bounds);
        // The label is centred on the top border, HEADROOM either side: its top
        // is exactly the bounds' top, and the box ends on the bounds' bottom.
        assert_eq!(bx.top - HEADROOM, bounds.top);
        assert_eq!(bx.bottom, bounds.bottom);
        assert_eq!(bx.bottom - bx.top, HEIGHT);
    }

    #[test]
    fn bounds_of_the_old_bare_box_height_shrink_the_box_instead_of_bleeding() {
        let f = OutlinedField::new("x");
        let bounds = Rect::new(0.0, 0.0, 300.0, HEIGHT);
        let bx = f.box_rect(bounds);
        assert!(bx.top - HEADROOM >= bounds.top);
        assert!(bx.bottom <= bounds.bottom);
    }

    #[test]
    fn a_multiline_box_fills_taller_bounds() {
        let mut f = OutlinedField::new("x");
        f.multiline = true;
        let bounds = Rect::new(0.0, 0.0, 300.0, 200.0);
        assert_eq!(f.box_rect(bounds).bottom, 200.0);
    }

    // ── Pure helpers ────────────────────────────────────────────────────────

    #[test]
    fn word_motion_follows_windows_edits() {
        let s = "Ada Lovelace, 1815";
        assert_eq!(next_word(s, 0), 4, "past the word and the blank");
        assert_eq!(next_word(s, 4), 12, "stops at the comma run");
        assert_eq!(prev_word(s, 12), 4);
        assert_eq!(prev_word(s, 4), 0);
        assert_eq!(prev_word(s, 0), 0);
        assert_eq!(next_word(s, 18), 18);
    }

    #[test]
    fn double_click_selects_the_run_under_the_pointer() {
        let s = "ada@kubuno.com";
        assert_eq!(word_range(s, 1), (0, 3));
        assert_eq!(word_range(s, 3), (3, 4), "the @ is its own run");
        assert_eq!(word_range(s, 14), (11, 14), "the end takes the run before it");
        assert_eq!(word_range("", 0), (0, 0));
    }

    fn ten(s: &str) -> f32 {
        s.chars().count() as f32 * 10.0
    }

    #[test]
    fn wrapping_breaks_after_the_last_blank_that_fits() {
        // 10 per char, 60 wide: "abc de" fits (60), "abc def" does not.
        assert_eq!(wrap_rows("abc def ghi", 60.0, ten), vec![(0, 4), (4, 8), (8, 11)]);
    }

    #[test]
    fn wrapping_keeps_hard_breaks_and_empty_lines() {
        assert_eq!(wrap_rows("ab\n\ncd", 100.0, ten), vec![(0, 2), (3, 3), (4, 6)]);
        assert_eq!(wrap_rows("ab\n", 100.0, ten), vec![(0, 2), (3, 3)]);
        assert_eq!(wrap_rows("", 100.0, ten), vec![(0, 0)]);
    }

    #[test]
    fn a_word_wider_than_the_row_breaks_between_characters() {
        assert_eq!(wrap_rows("abcdefgh", 30.0, ten), vec![(0, 3), (3, 6), (6, 8)]);
    }

    #[test]
    fn blanks_hang_rather_than_open_an_empty_row() {
        assert_eq!(wrap_rows("abc      def", 40.0, ten), vec![(0, 9), (9, 12)]);
    }

    #[test]
    fn the_nearest_caret_stop_splits_each_glyph_in_half() {
        let xs = [0.0, 10.0, 20.0, 30.0];
        assert_eq!(nearest_boundary(&xs, -5.0), 0);
        assert_eq!(nearest_boundary(&xs, 4.0), 0);
        assert_eq!(nearest_boundary(&xs, 6.0), 1);
        assert_eq!(nearest_boundary(&xs, 99.0), 3);
    }

    // ── Keyboard ─────────────────────────────────────────────────────────────

    #[test]
    fn typing_replaces_the_selection_and_moves_the_caret() {
        let mut f = field("Ada");
        assert_eq!(f.caret(), 3, "set_value puts the caret at the end");
        assert!(f.type_text(" L"));
        assert_eq!(f.text, "Ada L");
        f.select_range(0, 3);
        f.type_text("Bob");
        assert_eq!(f.text, "Bob L");
        assert_eq!(f.selection(), (3, 3));
        assert!(f.modified, "a user edit sets Modified");
    }

    #[test]
    fn arrows_collapse_a_selection_then_move() {
        let mut f = field("abcdef");
        f.select_range(1, 4);
        press(&mut f, vk::LEFT, Modifiers::NONE);
        assert_eq!(f.selection(), (1, 1), "Left collapses to the start");
        press(&mut f, vk::RIGHT, Modifiers::SHIFT);
        press(&mut f, vk::RIGHT, Modifiers::SHIFT);
        assert_eq!(f.selection(), (1, 3));
        press(&mut f, vk::RIGHT, Modifiers::NONE);
        assert_eq!(f.selection(), (3, 3), "Right collapses to the end");
        press(&mut f, vk::HOME, Modifiers::SHIFT);
        assert_eq!(f.selection(), (0, 3), "Shift+Home selects back to the start");
        press(&mut f, vk::END, Modifiers::NONE);
        assert_eq!(f.caret(), 6);
    }

    #[test]
    fn backspace_and_delete_by_char_and_by_word() {
        let mut f = field("Ada Lovelace");
        assert_eq!(press(&mut f, vk::BACK, Modifiers::NONE), KeyOutcome::Edited);
        assert_eq!(f.text, "Ada Lovelac");
        press(&mut f, vk::BACK, Modifiers::CTRL);
        assert_eq!(f.text, "Ada ");
        press(&mut f, vk::HOME, Modifiers::NONE);
        press(&mut f, vk::DELETE, Modifiers::CTRL);
        assert_eq!(f.text, "");
        assert_eq!(press(&mut f, vk::BACK, Modifiers::NONE), KeyOutcome::Moved, "nothing left to erase");
    }

    #[test]
    fn clipboard_shortcuts() {
        let mut f = field("Ada Lovelace");
        let mut clip = Fake::default();
        f.key(vk::letter('a'), Modifiers::CTRL, &mut clip);
        assert_eq!(f.selection(), (0, 12));
        f.key(vk::letter('c'), Modifiers::CTRL, &mut clip);
        assert_eq!(clip.0.as_deref(), Some("Ada Lovelace"));
        f.select_range(0, 4);
        assert_eq!(f.key(vk::letter('x'), Modifiers::CTRL, &mut clip), KeyOutcome::Edited);
        assert_eq!(f.text, "Lovelace");
        assert_eq!(clip.0.as_deref(), Some("Ada "));
        f.key(vk::END, Modifiers::NONE, &mut clip);
        f.key(vk::letter('v'), Modifiers::CTRL, &mut clip);
        assert_eq!(f.text, "LovelaceAda ");
    }

    #[test]
    fn a_single_line_field_strips_pasted_line_breaks() {
        let mut f = field("");
        let mut clip = Fake(Some("a\r\nb\nc".into()));
        f.key(vk::letter('v'), Modifiers::CTRL, &mut clip);
        assert_eq!(f.text, "abc");
        let mut m = field("");
        m.multiline = true;
        m.key(vk::letter('v'), Modifiers::CTRL, &mut clip);
        assert_eq!(m.text, "a\nb\nc");
    }

    #[test]
    fn max_length_refuses_the_excess_instead_of_cutting_the_tail() {
        let mut f = field("abcd");
        f.max_length = 5;
        f.select_range(2, 2);
        f.type_text("XYZ");
        assert_eq!(f.text, "abXcd");
        assert_eq!(f.caret(), 3);
        assert!(!f.type_text("Q"), "full");
    }

    #[test]
    fn a_password_field_never_copies() {
        let mut f = field("secret");
        f.use_system_password_char = true;
        assert_eq!(f.display(), "\u{25CF}".repeat(6));
        let mut clip = Fake::default();
        f.key(vk::letter('a'), Modifiers::CTRL, &mut clip);
        f.key(vk::letter('c'), Modifiers::CTRL, &mut clip);
        assert_eq!(clip.0, None);
    }

    #[test]
    fn undo_restores_whole_typing_runs_and_redo_replays_them() {
        let mut f = field("");
        f.type_text("a");
        f.type_text("b");
        f.type_text("c");
        press(&mut f, vk::BACK, Modifiers::NONE);
        assert_eq!(f.text, "ab");
        press(&mut f, vk::letter('z'), Modifiers::CTRL);
        assert_eq!(f.text, "abc", "the backspace is one step");
        press(&mut f, vk::letter('z'), Modifiers::CTRL);
        assert_eq!(f.text, "", "the typing run is one step");
        press(&mut f, vk::letter('y'), Modifiers::CTRL);
        assert_eq!(f.text, "abc");
        press(&mut f, vk::letter('z'), Modifiers::CTRL_SHIFT);
        assert_eq!(f.text, "ab");
    }

    #[test]
    fn enter_submits_a_single_line_and_breaks_a_multiline() {
        let mut f = field("a");
        assert_eq!(press(&mut f, vk::ENTER, Modifiers::NONE), KeyOutcome::Submit);
        let mut m = field("a");
        m.multiline = true;
        assert_eq!(press(&mut m, vk::ENTER, Modifiers::NONE), KeyOutcome::Edited);
        assert_eq!(m.text, "a\n");
    }

    #[test]
    fn up_and_down_move_between_rows_keeping_the_column() {
        let mut f = field("abcdef\nab\nabcdef");
        f.multiline = true;
        f.select_range(5, 5);
        press(&mut f, vk::DOWN, Modifiers::NONE);
        assert_eq!(f.caret(), 9, "clamped to the short row's end");
        press(&mut f, vk::DOWN, Modifiers::NONE);
        assert_eq!(f.caret(), 15, "the sticky column comes back");
        press(&mut f, vk::DOWN, Modifiers::NONE);
        assert_eq!(f.caret(), 16, "past the last row: the end");
        press(&mut f, vk::UP, Modifiers::SHIFT);
        assert_eq!(f.selection().1, 16);
        assert!(f.selection().0 < 16);
        assert!(!field("a").handles_key(vk::UP, Modifiers::NONE), "a single line leaves Up to the page");
    }

    #[test]
    fn read_only_navigates_and_copies_but_never_edits() {
        let mut f = field("abc");
        f.read_only = true;
        assert!(!f.type_text("x"));
        assert!(!f.handles_key(vk::BACK, Modifiers::NONE));
        assert!(f.handles_key(vk::LEFT, Modifiers::NONE));
        assert!(f.handles_key(vk::letter('c'), Modifiers::CTRL));
        assert!(!f.handles_key(vk::letter('v'), Modifiers::CTRL));
    }

    #[test]
    fn tab_and_escape_are_left_to_the_page() {
        let f = field("abc");
        assert!(!f.handles_key(vk::TAB, Modifiers::NONE));
        assert!(!f.handles_key(vk::ESCAPE, Modifiers::NONE));
        assert!(!f.handles_key(vk::letter('s'), Modifiers::CTRL));
    }

    #[test]
    fn without_a_paint_a_click_keeps_the_caret() {
        let mut f = field("abc");
        f.select_range(1, 1);
        assert_eq!(f.index_at(0.0, 0.0), 1);
        f.press_at(0.0, 0.0, 3, false);
        assert_eq!(f.selection(), (0, 3), "a triple click selects everything");
    }
}
