//! `TextBoxBase` — the edit-control base — and its three concrete descendants
//! `TextBox`, `MaskedTextBox` and `RichTextBox`.
//!
//! ## Why this shape
//!
//! In WinForms `TextBoxBase` is an abstract control that declares the whole
//! editing surface shared by every text box — the text and selection state,
//! `MaxLength`, `ReadOnly`, `Multiline`, `WordWrap`, `BorderStyle` — and the
//! three concrete controls add only what is specific to them. The port mirrors
//! that with composition + `Deref`, exactly as `ButtonBase`/`Button` do:
//!
//! ```ignore
//! TextBoxBase   { control: ControlBase, /* the editing surface */ }
//! TextBox       { base: TextBoxBase,    /* single-line specifics */ }
//! MaskedTextBox { base: TextBoxBase,    /* the mask engine       */ }
//! RichTextBox   { base: TextBoxBase,    /* styled runs           */ }
//! ```
//!
//! A property `TextBoxBase` declares lives here once; a subclass that *re*-declares
//! one of them in .NET (`new`/`override`, almost always to change a default —
//! `RichTextBox.Multiline` defaults to `true`, `RichTextBox.MaxLength` to
//! `int.MaxValue`, `TextBoxBase.AutoSize` to `true`) does not get a second copy of
//! the field: its `Default` impl sets the overriding value on the inherited field.
//! Those default overrides are genuine order-of-construction traps, so each is
//! called out in a comment and pinned by a test.
//!
//! ## What is modelled, and what is deferred
//!
//! * **Text + selection** are a real, pure state machine (`Text`,
//!   `SelectionStart`, `SelectionLength`, `MaxLength`, `ReadOnly`, `Modified`).
//!   Selection arithmetic clamps against the current text length and is tested.
//! * **The mask engine** (`MaskedTextBox`) is implemented honestly for a
//!   documented subset of the WinForms mask language — see [`MaskChar`]. Every
//!   mask character we accept, we honour; we never silently ignore one.
//! * **RTF is out of scope** for this wave. `RichTextBox` models the *styled-run*
//!   structure (bold / italic / underline / colour spans) so its painting and its
//!   text can be exercised, but there is no RTF parser and no `Rtf` property.
//!   Bold, **italic**, underline and colour are all painted, each from the
//!   system's own UI font — see [`StyledRun::italic`].
//!
//! ## What it paints with
//!
//! The **system**, and nothing else. Every colour comes from
//! [`crate::system::SystemColors`] (`Window` for an editable field's ground,
//! `Control` for a read-only or disabled one, `WindowText`/`GrayText` for the
//! ink, `Highlight`/`HighlightText` for the selection), every thickness from
//! [`crate::system::SystemMetrics`], and every glyph from the real UI font
//! (`lfMessageFont`, Segoe UI 9 pt on a default Windows 11) through
//! [`crate::system::SystemFonts`].
//!
//! There is no rounded corner, no design-system palette and no embedded face
//! anywhere in this file: a `TextBox` here is meant to be indistinguishable from
//! the one the reference sheets were painted by.
//!
//! `BorderStyle::Fixed3D` is the one place the system colours alone cannot get
//! there, and it is now drawn both ways. With visual styles ON — how
//! `02-textboxbase.png` was rendered — the toolkit frames the field with a FLAT
//! theme line (`#ABADB3` on a default Windows 11) over a one-pixel `Window` pad,
//! and that colour is no `COLOR_*` on the same machine. With visual styles OFF
//! the same control is the classic two-ring `DrawEdge` sunken well.
//! [`paint_fixed3d_border`] asks [`crate::theme`] for the real part and falls
//! back to the bevel, so the library matches whichever machine it runs on.
//! Everything else on that sheet — the `FixedSingle` `#646464` `WindowFrame`
//! line, the `#F0F0F0` ground of a read-only or disabled field, its `#6D6D6D`
//! `GrayText` ink, the `#0078D7` selection — was already exact from the system
//! colours alone.

use kubuno_drive_desktop_app_controls::{Canvas, Rect};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

use crate::control::{Control, ControlBase, ControlCanvas, FontRole};
use crate::enums::{BorderStyle, HorizontalAlignment, ScrollBars, Size};
use crate::system::{edge_interior, Border3DSide, Border3DStyle};
use crate::theme;
use crate::theme::part::EP_EDITTEXT;
use crate::theme::state::{ETS_DISABLED, ETS_NORMAL, ETS_READONLY};

// ── Metrics ──────────────────────────────────────────────────────────────────
// All in DIP: the `Canvas` primitives already fold in the DPI scale, so a control
// never multiplies by `c.scale()` itself. These are the field's own paddings and
// line box, taken from the reference sheet's single-line field (≈23 DIP tall at
// the default font); everything that the SYSTEM publishes — border thickness,
// scroll-bar width, caret width, the font — is read from `Visuals` instead and
// never written down here.

/// The text line box a single glyph row occupies, at the system UI font size.
const TEXT_LINE_H: f32 = 16.0;
/// Baseline-to-baseline advance between wrapped/multiline rows.
const LINE_ADVANCE: f32 = 18.0;
/// Horizontal inset of the text from the border.
const FIELD_PAD_X: f32 = 8.0;
/// Vertical inset of the text from the border.
const FIELD_PAD_Y: f32 = 3.0;
/// The glyph WinForms shows for `UseSystemPasswordChar` (`●`, U+25CF).
const PASSWORD_GLYPH: char = '\u{25CF}';

// ── Family-local enumerations ──────────────────────────────────────────────────
// These describe property types that ONLY this family declares, so they live
// here rather than in the shared `enums.rs`. The rule is the one `enums.rs`
// states: a type moves there as soon as a second family declares a property of
// it. `HorizontalAlignment` (the type of `TextBox.TextAlign`) did exactly that —
// four families had grown their own copy — so it is now imported from `enums`,
// along with its `dwrite()` mapping.

/// Whether typed characters are forced to a case (`CharacterCasing`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CharacterCasing {
    #[default]
    Normal,
    Upper,
    Lower,
}

impl CharacterCasing {
    /// Applies the casing to one character, the way the toolkit does as it is typed.
    fn apply(self, ch: char) -> char {
        match self {
            Self::Normal => ch,
            // `next()` is safe: ASCII/Latin case mappings are one-to-one here, and
            // a char with no mapping maps to itself.
            Self::Upper => ch.to_uppercase().next().unwrap_or(ch),
            Self::Lower => ch.to_lowercase().next().unwrap_or(ch),
        }
    }
}

/// Whether `MaskedTextBox.Text` (and cut/copy) includes literals and/or the
/// prompt (`System.ComponentModel.MaskFormat`). The discriminants are the
/// toolkit's own, so the flag arithmetic below matches it exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaskFormat {
    ExcludePromptAndLiterals = 0,
    IncludePrompt = 1,
    /// The WinForms default for both `TextMaskFormat` and `CutCopyMaskFormat`.
    #[default]
    IncludeLiterals = 2,
    IncludePromptAndLiterals = 3,
}

impl MaskFormat {
    const fn include_prompt(self) -> bool {
        matches!(self, Self::IncludePrompt | Self::IncludePromptAndLiterals)
    }
    const fn include_literals(self) -> bool {
        matches!(self, Self::IncludeLiterals | Self::IncludePromptAndLiterals)
    }
}

/// The keyboard insert mode a masked box exposes (`InsertKeyMode`). Modelled for
/// completeness; the port has no live keyboard, so it is state only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InsertKeyMode {
    #[default]
    Default,
    Insert,
    Overwrite,
}

/// Auto-completion behaviour of a `TextBox` (`AutoCompleteMode`). We store the
/// value but do not drive a completion popup, so it is not-yet-honoured — see the
/// field's doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AutoCompleteMode {
    #[default]
    None,
    Suggest,
    Append,
    SuggestAppend,
}

/// Where a `TextBox` draws completion candidates from (`AutoCompleteSource`).
/// Modelled with the toolkit's members; not honoured (no completion engine).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AutoCompleteSource {
    FileSystem,
    HistoryList,
    RecentlyUsedList,
    AllUrl,
    AllSystemSources,
    FileSystemDirectories,
    CustomSource,
    ListItems,
    #[default]
    None,
}

/// `RichTextBox.ScrollBars` (`RichTextBoxScrollBars`) — a richer set than the
/// plain `ScrollBars` a single-line/multiline `TextBox` uses, hence its own type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RichTextBoxScrollBars {
    None,
    Horizontal,
    Vertical,
    #[default]
    Both,
    ForcedHorizontal,
    ForcedVertical,
    ForcedBoth,
}

impl RichTextBoxScrollBars {
    /// Whether a vertical band should be painted (any vertical/both variant).
    fn has_vertical(self) -> bool {
        matches!(
            self,
            Self::Vertical | Self::Both | Self::ForcedVertical | Self::ForcedBoth
        )
    }
}

/// The kind of the current selection (`RichTextBoxSelectionTypes`). Read-only in
/// the toolkit; the port always reports `Empty`/`Text` since it has no embedded
/// OLE objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RichTextBoxSelectionTypes {
    #[default]
    Empty,
    Text,
    Object,
    MultiChar,
    MultiObject,
}

// ── Pure string helpers ────────────────────────────────────────────────────────
// Selection and mask arithmetic is defined on CHARACTER counts, matching the
// toolkit's UTF-16 indices, not on Rust's UTF-8 byte offsets — so a multi-byte
// character counts as one, as it does in the designer.

/// Number of characters in `s` (`Text.Length` semantics).
fn char_len(s: &str) -> i32 {
    s.chars().count() as i32
}

/// The first `n` characters of `s` as an owned string (clamped, never panics on a
/// byte boundary the way `&s[..n]` would).
fn take_chars(s: &str, n: i32) -> String {
    if n <= 0 {
        return String::new();
    }
    s.chars().take(n as usize).collect()
}

/// The `len` characters of `s` starting at character `start` (both clamped).
fn slice_chars(s: &str, start: i32, len: i32) -> String {
    let start = start.max(0) as usize;
    let len = len.max(0) as usize;
    s.chars().skip(start).take(len).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// TextBoxBase
// ═══════════════════════════════════════════════════════════════════════════════

/// The properties `System.Windows.Forms.TextBoxBase` declares — the editing
/// surface every text box inherits.
///
/// `Text` is `ControlBase::text` (re-declared by `TextBoxBase` in .NET, same
/// storage). `BackColor`, `ForeColor`, `BackgroundImageLayout`, `AutoSize` are
/// `ControlBase` fields likewise; only their *defaults* change here (`AutoSize`
/// becomes `true`). `CanUndo`, `Lines`, `PreferredHeight` and `SelectedText` are
/// derived, so they are methods, not fields.
#[derive(Clone)]
pub struct TextBoxBase {
    control: ControlBase,

    // ── Behaviour ────────────────────────────────────────────────────────
    /// `AcceptsTab` — whether Tab inserts a tab in a multiline box instead of
    /// moving focus. State only (the port has no focus traversal yet).
    pub accepts_tab: bool,
    /// `BorderStyle` — how the field's edge is painted; honoured in `paint`.
    pub border_style: BorderStyle,
    /// `HideSelection` — hide the selection when the control loses focus. Stored;
    /// the port has no focus model, so the selection is always shown (documented).
    pub hide_selection: bool,
    /// `MaxLength` — the maximum number of characters `Text` may hold. `0` means
    /// "unbounded" (WinForms' "limited only by memory"). Honoured by `set_text`.
    pub max_length: i32,
    /// `Modified` — set once the user edits the text; cleared when `Text` is set
    /// programmatically. The port has no keystrokes, so callers set it via the API.
    pub modified: bool,
    /// `Multiline` — whether the text may span several lines; honoured in `paint`
    /// and `preferred_size`.
    pub multiline: bool,
    /// `ReadOnly` — whether the text can be edited; honoured (no caret painted).
    pub read_only: bool,
    /// `ShortcutsEnabled` — whether Ctrl-C/V/X etc. are active. State only (no
    /// live keyboard).
    pub shortcuts_enabled: bool,
    /// `WordWrap` — whether long lines wrap in a multiline box. Stored; the port's
    /// painter wraps on explicit newlines only, so soft-wrap is not-yet-honoured.
    pub word_wrap: bool,

    // ── Selection state (character indices) ──────────────────────────────
    /// `SelectionStart` — the caret / selection anchor, in characters.
    selection_start: i32,
    /// `SelectionLength` — the selection length, in characters.
    selection_length: i32,

    // ── Appearance the toolkit re-surfaces on TextBoxBase ─────────────────
    /// `BackgroundImage` — WinForms re-declares (and hides) `Control.BackgroundImage`
    /// here. `ControlBase` does not model an image, so it is carried as an opaque
    /// resource key and is not painted (documented, and reported as a shared gap).
    pub background_image: Option<String>,
}

impl Default for TextBoxBase {
    fn default() -> Self {
        let mut control = ControlBase::new();
        // TRAP: `TextBoxBase` overrides `Control.AutoSize` (false) to `true` — a
        // single-line box sizes its height to the font. Descendants that want the
        // Control default back (RichTextBox) must set it to false explicitly.
        control.auto_size = true;
        Self {
            control,
            accepts_tab: false,
            border_style: BorderStyle::Fixed3D, // the documented TextBox default
            hide_selection: true,
            max_length: 32767,
            modified: false,
            multiline: false,
            read_only: false,
            shortcuts_enabled: true,
            word_wrap: true,
            selection_start: 0,
            selection_length: 0,
            background_image: None,
        }
    }
}

impl TextBoxBase {
    pub fn new() -> Self {
        Self::default()
    }

    // ── Text (`ControlBase::text`) with the toolkit's setter semantics ────

    /// `Text` getter — the raw stored text.
    pub fn text(&self) -> &str {
        &self.control.text
    }

    /// `Text` setter. Truncates to `MaxLength`, resets the caret to 0 and clears
    /// `Modified` — the behaviour of a programmatic assignment in WinForms (only a
    /// USER edit sets `Modified`).
    pub fn set_text(&mut self, value: &str) {
        self.control.text = self.truncate(value);
        self.selection_start = 0;
        self.selection_length = 0;
        self.modified = false;
    }

    /// Truncates `value` to `MaxLength` characters (`0` = unbounded).
    fn truncate(&self, value: &str) -> String {
        if self.max_length > 0 && char_len(value) > self.max_length {
            take_chars(value, self.max_length)
        } else {
            value.to_string()
        }
    }

    // ── Selection: pure, clamped arithmetic ──────────────────────────────

    pub fn selection_start(&self) -> i32 {
        self.selection_start
    }

    pub fn selection_length(&self) -> i32 {
        self.selection_length
    }

    /// `SelectionStart = value`, clamped into the text, keeping the selection
    /// valid.
    pub fn set_selection_start(&mut self, value: i32) {
        self.selection_start = value.clamp(0, char_len(&self.control.text));
        self.clamp_selection();
    }

    /// `SelectionLength = value`, clamped so it never runs past the end.
    pub fn set_selection_length(&mut self, value: i32) {
        self.selection_length = value.max(0);
        self.clamp_selection();
    }

    /// `Select(start, length)` — set both at once, then clamp.
    pub fn select(&mut self, start: i32, length: i32) {
        self.selection_start = start.max(0);
        self.selection_length = length.max(0);
        self.clamp_selection();
    }

    /// `SelectAll()`.
    pub fn select_all(&mut self) {
        self.selection_start = 0;
        self.selection_length = char_len(&self.control.text);
    }

    /// Re-clamps the selection against the current text length. Called whenever
    /// the text may have shrunk, so `SelectionStart`/`SelectionLength` can never
    /// point past the end — the correctness rule the toolkit guarantees.
    fn clamp_selection(&mut self) {
        let n = char_len(&self.control.text);
        self.selection_start = self.selection_start.clamp(0, n);
        let room = n - self.selection_start;
        self.selection_length = self.selection_length.clamp(0, room);
    }

    /// `SelectedText` getter — the characters currently selected.
    pub fn selected_text(&self) -> String {
        slice_chars(&self.control.text, self.selection_start, self.selection_length)
    }

    /// `SelectedText` setter — replace the selection with `value`, honouring
    /// `MaxLength`, then place the caret after the inserted text. This *is* a user
    /// edit, so `Modified` becomes true.
    pub fn set_selected_text(&mut self, value: &str) {
        let n = char_len(&self.control.text);
        let start = self.selection_start.clamp(0, n);
        let end = (start + self.selection_length).clamp(0, n);
        let before = take_chars(&self.control.text, start);
        let after: String = self.control.text.chars().skip(end as usize).collect();
        let combined = format!("{before}{value}{after}");
        self.control.text = self.truncate(&combined);
        // Caret after the insertion, clamped in case truncation cut it short.
        self.selection_start = (start + char_len(value)).min(char_len(&self.control.text));
        self.selection_length = 0;
        self.modified = true;
    }

    // ── Derived read-only members ────────────────────────────────────────

    /// `Lines` — the text split into rows. WinForms splits on newlines; we accept
    /// both `\r\n` and `\n`.
    pub fn lines(&self) -> Vec<String> {
        self.control
            .text
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
            .collect()
    }

    /// `Lines = value` — join with `\n`.
    pub fn set_lines(&mut self, lines: &[String]) {
        let joined = lines.join("\n");
        self.set_text(&joined);
    }

    /// `CanUndo` — the port keeps no undo buffer, so it is always false
    /// (documented; a real editor would track an edit stack).
    pub fn can_undo(&self) -> bool {
        false
    }

    /// `PreferredHeight` — the height a single-line field wants for the current
    /// border. Pure (no `Canvas`): the row height is a fixed DIP at the shared
    /// font, so it can be measured without a device.
    pub fn preferred_height(&self) -> f32 {
        TEXT_LINE_H + 2.0 * FIELD_PAD_Y + 2.0 * border_thickness(self.border_style)
    }
}

impl std::ops::Deref for TextBoxBase {
    type Target = ControlBase;
    fn deref(&self) -> &ControlBase {
        &self.control
    }
}

impl std::ops::DerefMut for TextBoxBase {
    fn deref_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }
}

/// Border inset per style, in DIP — the **measurement** side of the border.
///
/// `None` takes nothing; `FixedSingle` is one line; `Fixed3D` is the two-ring
/// `DrawEdge` bevel, which is two *device* pixels and therefore under 2 DIP on
/// any scaled display — a single DIP is what `PreferredHeight` has always
/// budgeted for it and what the reference sheet's 3-D field measures at.
///
/// The **painting** side does not use this number: it takes the interior
/// [`ControlCanvas::draw_edge`] reports, which is exact at the current scale.
fn border_thickness(style: BorderStyle) -> f32 {
    match style {
        BorderStyle::None => 0.0,
        BorderStyle::FixedSingle => 1.0,
        BorderStyle::Fixed3D => 1.0,
    }
}

// ── Shared painting ────────────────────────────────────────────────────────────

/// The one text format a bare [`Canvas`] can reach — **the measuring gap**.
///
/// Painting sees a [`ControlCanvas`] and therefore the system UI font, which is
/// what every glyph in this file is drawn with. `GetPreferredSize` does not:
/// [`Control::preferred_size`] takes a `&dyn Canvas`, whose only fonts are the
/// shared design-system formats. So a width computed from a measurement is taken
/// against the design-system body face while the text is *painted* in Segoe UI —
/// the two can disagree by a few DIP on a long string.
///
/// The alternatives were both worse: widening `preferred_size` to
/// `&dyn ControlCanvas` is a change to the foundation this repaint is not allowed
/// to make, and estimating a width without DirectWrite would change the control's
/// resolved size. The gap is therefore named, isolated in this one function, and
/// reported — the fix belongs in `control.rs`, not here.
fn measuring_format(c: &dyn Canvas) -> &IDWriteTextFormat {
    &c.formats().body
}

/// Whether a bold run at `role` renders heavier **in the shared design-system
/// formats**.
///
/// It no longer describes what this family paints: since the repaint, a styled
/// run takes `SystemFonts::message_bold`, which exists whatever
/// [`FontRole`] the control carries — so as far as `RichTextBox` is concerned the
/// answer is now always yes. The predicate is kept because it is public API and
/// because `ControlBase::font` still names a role; it answers about the format
/// set, not about this control.
pub const fn role_has_strong_variant(role: FontRole) -> bool {
    !matches!(role, FontRole::Title)
}

/// Everything the shared field painter needs from a concrete control, in
/// primitives: the already-resolved visible string (password glyphs and mask
/// prompt substituted by the caller), an optional placeholder, and the alignment.
struct FieldPaint<'a> {
    /// The characters to draw (never the secret text: a password box passes the
    /// glyph string).
    display: &'a str,
    /// Shown greyed when `display` is empty and the field is enabled.
    placeholder: Option<&'a str>,
    /// Suppress the selection highlight and caret metrics (password fields still
    /// paint dots, but selecting them makes no visual sense here).
    is_password: bool,
    align: HorizontalAlignment,
    /// Styled runs (RichTextBox). When set, they are drawn instead of `display`.
    runs: Option<&'a [StyledRun]>,
}

/// Paints a text field the way the toolkit does: the `BorderStyle`'s chrome, the
/// field's ground, then the content (plain, password, placeholder, multiline or
/// styled), and a multiline vertical scroll bar.
///
/// Every colour is a system colour and every thickness a system metric; the
/// border is drawn first because [`ControlCanvas::draw_edge`] reports the
/// **interior** it leaves behind, which is exactly where the ground and the text
/// go — the caller never re-derives the bevel's thickness.
fn paint_field(
    c: &dyn ControlCanvas,
    bounds: Rect,
    base: &TextBoxBase,
    fp: &FieldPaint,
    vscroll: bool,
) {
    let v = c.visuals();

    // The three `BorderStyle` values, each as the toolkit paints it:
    //   Fixed3D     — the themed `EDIT` frame, or the classic `DrawEdge`
    //                 (EDGE_SUNKEN) well when visual styles are off,
    //   FixedSingle — one flat `WindowFrame` line,
    //   None        — nothing at all.
    let interior = match base.border_style {
        BorderStyle::None => bounds,
        BorderStyle::FixedSingle => {
            c.stroke_rect(&bounds, &v.colors.window_frame);
            edge_interior(&bounds, 1, c.scale())
        }
        BorderStyle::Fixed3D => paint_fixed3d_border(c, bounds, base.enabled, base.read_only),
    };

    // An editable field's ground is `Window`; a read-only or disabled one is
    // `Control` — the toolkit swaps `BackColor` for `SystemColors.Control` when
    // `ReadOnly` is set, so the two cases share one branch.
    let ground = if base.enabled && !base.read_only { v.colors.window } else { v.colors.control };
    c.fill_rect(&interior, &ground);

    let text_col = if base.enabled { v.colors.window_text } else { v.colors.gray_text };
    // The scroll bar is the system's own width, never a design-system band.
    let bar_w = if vscroll { v.metrics.vertical_scroll_width } else { 0.0 };
    let inner = Rect::new(
        interior.left + FIELD_PAD_X,
        interior.top + FIELD_PAD_Y,
        (interior.right - bar_w - FIELD_PAD_X).max(interior.left),
        interior.bottom - FIELD_PAD_Y,
    );
    let fmt = &v.fonts.message;

    c.push_clip(&inner);
    if let Some(runs) = fp.runs {
        paint_runs(c, &inner, runs, &text_col);
    } else if base.multiline {
        paint_multiline(c, &inner, fp.display, fmt, &text_col);
    } else {
        paint_single_line(c, &inner, base, fp, fmt, &text_col);
    }
    c.pop_clip();

    if vscroll {
        paint_vscrollbar(c, &interior);
    }
}

/// `BorderStyle::Fixed3D` — the themed `EDIT` frame, falling back to the
/// classic `DrawEdge` well. Returns the **interior**, as `draw_edge` does.
///
/// ## Two renderings, both correct
///
/// With visual styles ON — how the reference sheets were captured — a `Fixed3D`
/// field is framed by a flat theme line, `#ABADB3` on a default Windows 11, with
/// the frame's inner pixel in `Window`; there is no bevel at all. With them OFF
/// it is the classic two-ring sunken well built from `COLOR_3DDKSHADOW` and
/// friends. Neither approximates the other, and no `GetSysColor` index can
/// produce the first — which is what [`crate::theme`] exists for.
///
/// ## Why the pad is repainted
///
/// The part is `EP_EDITTEXT`, not `EP_EDITBORDER_NOSCROLL`: measured against the
/// sheet, the second draws Windows 11's modern rounded frame and the first draws
/// the toolkit's — see the `theme` module docs. `EP_EDITTEXT` paints a frame AND
/// a fill, and its fill under `ETS_DISABLED` is the theme's own tint, whereas
/// the toolkit shows `Window` there because its client area covers everything
/// but the frame's two rings. So the inner ring is put back explicitly, and the
/// caller then fills the client area over the rest.
///
/// The state is still the control's real one. It changes no pixel of the frame
/// today (the theme draws the same `#ABADB3` line in all four states, which the
/// sheet confirms for editable, read-only and disabled fields), but passing a
/// lie because it happens not to show would be a fact waiting to be wrong.
fn paint_fixed3d_border(
    c: &dyn ControlCanvas,
    bounds: Rect,
    enabled: bool,
    read_only: bool,
) -> Rect {
    let v = c.visuals();
    let state = if !enabled {
        ETS_DISABLED
    } else if read_only {
        ETS_READONLY
    } else {
        ETS_NORMAL
    };
    if c.draw_theme_part(theme::class::EDIT, EP_EDITTEXT, state, bounds, v.colors.window) {
        let pad = edge_interior(&bounds, 1, c.scale());
        c.fill_rect(&pad, &v.colors.window);
        return edge_interior(&bounds, 2, c.scale());
    }
    c.draw_edge(&bounds, Border3DStyle::Sunken, Border3DSide::ALL)
}

/// A single-line field: the selection, then the text (or placeholder), then the
/// caret when it is an editable field.
fn paint_single_line(
    c: &dyn ControlCanvas,
    inner: &Rect,
    base: &TextBoxBase,
    fp: &FieldPaint,
    fmt: &IDWriteTextFormat,
    text_col: &D2D1_COLOR_F,
) {
    let v = c.visuals();
    let cy = (inner.top + inner.bottom) / 2.0;
    let line_top = cy - TEXT_LINE_H / 2.0;
    let line_bottom = cy + TEXT_LINE_H / 2.0;

    // Empty + enabled + placeholder set → the hint, in the one colour the toolkit
    // greys text with.
    if fp.display.is_empty() {
        if let Some(ph) = fp.placeholder {
            if base.enabled {
                c.text_aligned(ph, inner, fmt, &v.colors.gray_text, fp.align.dwrite());
            }
        }
        return;
    }

    // The selection is the system's, both halves of it: `Highlight` behind and
    // `HighlightText` on top. Painting the highlight and then drawing the WHOLE
    // string in the foreground colour would leave dark ink on a dark blue ground,
    // so the run is drawn in three pieces, each measured from the same format.
    //
    // Left-aligned fields only — measuring a centred/right run's offset needs the
    // field width the reference does not vary.
    let selected =
        !fp.is_password && base.selection_length > 0 && fp.align == HorizontalAlignment::Left;
    if selected {
        let head = take_chars(fp.display, base.selection_start);
        let through = take_chars(fp.display, base.selection_start + base.selection_length);
        let body = slice_chars(fp.display, base.selection_start, base.selection_length);
        let tail: String = fp
            .display
            .chars()
            .skip((base.selection_start + base.selection_length).max(0) as usize)
            .collect();
        let x0 = inner.left + c.measure(&head, fmt);
        let x1 = inner.left + c.measure(&through, fmt);
        c.fill_rect(&Rect::new(x0, line_top, x1, line_bottom), &v.colors.highlight);

        let piece = |x: f32, s: &str, col: &D2D1_COLOR_F| {
            if !s.is_empty() {
                let r = Rect::new(x, inner.top, inner.right.max(x), inner.bottom);
                c.text_aligned(s, &r, fmt, col, HorizontalAlignment::Left.dwrite());
            }
        };
        piece(inner.left, &head, text_col);
        piece(x0, &body, &v.colors.highlight_text);
        piece(x1, &tail, text_col);
    } else {
        c.text_aligned(fp.display, inner, fmt, text_col, fp.align.dwrite());
    }

    // Caret at the selection start, when the field could take input. One system
    // border wide — a caret is a hairline at every scale, never a 1 DIP slab.
    if !base.read_only && base.enabled && fp.align == HorizontalAlignment::Left {
        let caret_x = inner.left + c.measure(&take_chars(fp.display, base.selection_start), fmt);
        let w = v.metrics.border_width.max(1.0 / c.scale());
        c.fill_rect(&Rect::new(caret_x, line_top, caret_x + w, line_bottom), text_col);
    }
}

/// Multiline plain text: one row per `\n`, from the top down. Soft word wrap is
/// not modelled (see `word_wrap`), so only explicit newlines break a line.
fn paint_multiline(
    c: &dyn Canvas,
    inner: &Rect,
    text: &str,
    fmt: &IDWriteTextFormat,
    text_col: &D2D1_COLOR_F,
) {
    let mut y = inner.top;
    for line in text.split('\n') {
        if y >= inner.bottom {
            break;
        }
        let row = Rect::new(inner.left, y, inner.right, y + LINE_ADVANCE);
        c.text(line.strip_suffix('\r').unwrap_or(line), &row, fmt, text_col, false);
        y += LINE_ADVANCE;
    }
}

/// Styled runs (RichTextBox): each run is laid left to right on the current row,
/// wrapping on an embedded `\n`.
///
/// All four attributes are honoured: `bold` takes the system font's bold face,
/// `italic` its italic face, `underline` draws a rule under the run and `color`
/// overrides the ink. The system publishes **no bold-italic** face
/// (`SystemFonts` builds `message`, `message_bold` and `message_italic` and no
/// fourth), so a run that is both renders bold — the heavier signal — rather than
/// silently dropping one of the two.
fn paint_runs(c: &dyn ControlCanvas, inner: &Rect, runs: &[StyledRun], default_col: &D2D1_COLOR_F) {
    let v = c.visuals();
    let mut x = inner.left;
    let mut y = inner.top;
    for run in runs {
        // Measuring and drawing MUST use the same format, or a bold run's advance
        // widths would be taken from the plain one and the next run would overlap.
        let fmt = match (run.bold, run.italic) {
            (true, _) => &v.fonts.message_bold,
            (false, true) => &v.fonts.message_italic,
            (false, false) => &v.fonts.message,
        };
        let col = run.color.as_ref().unwrap_or(default_col);
        // A run may itself contain line breaks.
        let mut first = true;
        for piece in run.text.split('\n') {
            if !first {
                x = inner.left;
                y += LINE_ADVANCE;
            }
            first = false;
            if y >= inner.bottom || piece.is_empty() {
                continue;
            }
            let w = c.measure(piece, fmt);
            let rect = Rect::new(x, y, x + w + 2.0, y + LINE_ADVANCE);
            c.text(piece, &rect, fmt, col, false);
            if run.underline {
                let uy = y + TEXT_LINE_H;
                let t = v.metrics.border_height.max(1.0 / c.scale());
                c.fill_rect(&Rect::new(x, uy, x + w, uy + t), col);
            }
            x += w;
        }
    }
}

/// The vertical scroll bar of a multiline field: a `Control`-coloured track with
/// a raised thumb, at the system's own `SM_CXVSCROLL` width.
///
/// The thumb is the classic Win32 one — a `Control`-coloured face under a raised
/// `DrawEdge`, which is what gives it its relief. The port has no scroll offset
/// to size it from, so it shows the presence of the bar, not a live position, and
/// the arrow buttons are the scroll-bar family's business, not this one's.
fn paint_vscrollbar(c: &dyn ControlCanvas, interior: &Rect) {
    let v = c.visuals();
    let track = Rect::new(
        (interior.right - v.metrics.vertical_scroll_width).max(interior.left),
        interior.top,
        interior.right,
        interior.bottom,
    );
    c.fill_rect(&track, &v.colors.control);
    let thumb = Rect::new(
        track.left,
        track.top,
        track.right,
        track.top + (track.bottom - track.top) * 0.4,
    );
    c.fill_rect(&thumb, &v.colors.control);
    c.draw_edge(&thumb, Border3DStyle::Raised, Border3DSide::ALL);
}

// ═══════════════════════════════════════════════════════════════════════════════
// TextBox
// ═══════════════════════════════════════════════════════════════════════════════

/// `System.Windows.Forms.TextBox` — the single-line/multiline plain editor.
///
/// It adds password display, a placeholder, horizontal alignment, scrollbars,
/// character casing and the (state-only) auto-complete surface. `Multiline` and
/// `Text` are re-declarations of the base fields and stay there.
#[derive(Clone)]
pub struct TextBox {
    base: TextBoxBase,

    /// `AcceptsReturn` — whether Enter inserts a newline in a multiline box.
    /// State only (no live keyboard).
    pub accepts_return: bool,
    /// `AutoCompleteCustomSource` — the candidate strings for `CustomSource`.
    /// Stored; not honoured (no completion popup).
    pub auto_complete_custom_source: Vec<String>,
    /// `AutoCompleteMode` — stored; not honoured.
    pub auto_complete_mode: AutoCompleteMode,
    /// `AutoCompleteSource` — stored; not honoured.
    pub auto_complete_source: AutoCompleteSource,
    /// `CharacterCasing` — forces typed characters to a case. Applied by
    /// [`TextBox::set_text`].
    pub character_casing: CharacterCasing,
    /// `PasswordChar` — the glyph shown instead of each character (single-line
    /// only). `None` = show the real text. Honoured in `paint`.
    pub password_char: Option<char>,
    /// `PlaceholderText` — hint shown when `Text` is empty. Honoured in `paint`.
    pub placeholder_text: String,
    /// `ScrollBars` — which bars a multiline box shows. The vertical one is
    /// painted; horizontal is not-yet-honoured (no horizontal scroll offset).
    pub scroll_bars: ScrollBars,
    /// `TextAlign` — horizontal alignment of the text. Honoured in `paint`.
    pub text_align: HorizontalAlignment,
    /// `UseSystemPasswordChar` — force the system password glyph, overriding
    /// `PasswordChar`. Honoured in `paint`.
    pub use_system_password_char: bool,
}

impl Default for TextBox {
    fn default() -> Self {
        Self {
            base: TextBoxBase::new(),
            accepts_return: false,
            auto_complete_custom_source: Vec::new(),
            auto_complete_mode: AutoCompleteMode::None,
            auto_complete_source: AutoCompleteSource::None,
            character_casing: CharacterCasing::Normal,
            password_char: None,
            placeholder_text: String::new(),
            scroll_bars: ScrollBars::None,
            text_align: HorizontalAlignment::Left,
            use_system_password_char: false,
        }
    }
}

impl TextBox {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Text` setter with `CharacterCasing` applied — the single-line/casing
    /// specialisation of the base setter.
    pub fn set_text(&mut self, value: &str) {
        let cased: String = value.chars().map(|c| self.character_casing.apply(c)).collect();
        self.base.set_text(&cased);
    }

    /// The glyph a password field shows, if any: the system glyph wins over
    /// `PasswordChar`, exactly as the toolkit resolves it.
    fn effective_password_char(&self) -> Option<char> {
        if self.use_system_password_char {
            Some(PASSWORD_GLYPH)
        } else {
            self.password_char
        }
    }

    /// The visible string: the password glyph repeated, or the real text. Password
    /// masking applies to single-line fields only (WinForms ignores it in
    /// multiline).
    fn display(&self) -> String {
        match self.effective_password_char() {
            Some(g) if !self.base.multiline => g.to_string().repeat(self.base.text().chars().count()),
            _ => self.base.text().to_string(),
        }
    }
}

impl std::ops::Deref for TextBox {
    type Target = TextBoxBase;
    fn deref(&self) -> &TextBoxBase {
        &self.base
    }
}

impl std::ops::DerefMut for TextBox {
    fn deref_mut(&mut self) -> &mut TextBoxBase {
        &mut self.base
    }
}

impl Control for TextBox {
    fn control(&self) -> &ControlBase {
        &self.base.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base.control
    }

    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        preferred_text_size(&self.base, c, self.base.text())
    }

    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        let display = self.display();
        let is_pw = self.effective_password_char().is_some() && !self.base.multiline;
        let vscroll = self.base.multiline
            && matches!(self.scroll_bars, ScrollBars::Vertical | ScrollBars::Both);
        let fp = FieldPaint {
            display: &display,
            placeholder: (!self.placeholder_text.is_empty()).then_some(self.placeholder_text.as_str()),
            is_password: is_pw,
            align: self.text_align,
            runs: None,
        };
        paint_field(c, bounds, &self.base, &fp, vscroll);
    }

    fn type_name(&self) -> &'static str {
        "TextBox"
    }
}

/// The shared `GetPreferredSize`: a fixed row height for single-line, the row
/// height times the line count for multiline; width is the measured content plus
/// gutters when `AutoSize`, else the control's current width.
fn preferred_text_size(base: &TextBoxBase, c: &dyn Canvas, content: &str) -> Size {
    let fmt = measuring_format(c);
    let bt = 2.0 * border_thickness(base.border_style);
    let pad = base.control.padding;

    let height = if base.multiline {
        let rows = content.split('\n').count().max(1) as f32;
        rows * LINE_ADVANCE + 2.0 * FIELD_PAD_Y + bt + pad.vertical()
    } else {
        base.preferred_height() + pad.vertical()
    };

    let width = if base.auto_size && !base.multiline {
        let text_w = content.split('\n').map(|l| c.measure(l, fmt)).fold(0.0_f32, f32::max);
        text_w + 2.0 * FIELD_PAD_X + bt + pad.horizontal()
    } else {
        base.control.width()
    };

    base.control.clamp(Size::new(width, height))
}

// ═══════════════════════════════════════════════════════════════════════════════
// MaskedTextBox
// ═══════════════════════════════════════════════════════════════════════════════

/// One parsed position of a mask string.
///
/// ## Supported mask language (a documented subset of the WinForms mask)
///
/// | Char | Meaning | Required |
/// |------|---------|----------|
/// | `0`  | digit (0–9) | yes |
/// | `9`  | digit or space | no |
/// | `#`  | digit, space or sign (`+`/`-`) | no |
/// | `L`  | letter | yes |
/// | `?`  | letter | no |
/// | `&`  | any non-control character | yes |
/// | `C`  | any non-control character | no |
/// | `A`  | letter or digit | yes |
/// | `a`  | letter or digit | no |
/// | `<`  | force following input to lower case | — |
/// | `>`  | force following input to upper case | — |
/// | `\|` | stop a previous case shift | — |
/// | `\\` | escape: treat the next character as a literal | — |
///
/// The localisable separators (`. , : / $`) are treated as **literals of that
/// exact character** — the port does not apply a `Culture`, so they render as
/// written (documented on `MaskedTextBox::culture`). Every other character is a
/// literal. No mask character is accepted and then ignored.
#[derive(Clone, Copy)]
enum MaskChar {
    Literal(char),
    Edit { kind: EditKind, required: bool, case: CharacterCasing },
}

/// The character class an editable mask position accepts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Digit,
    DigitOrSpace,
    DigitSignSpace,
    Letter,
    Any,
    AlphaNumeric,
}

impl EditKind {
    /// Whether `ch` is valid input for this position.
    fn accepts(self, ch: char) -> bool {
        match self {
            Self::Digit | Self::DigitOrSpace => ch.is_ascii_digit(),
            Self::DigitSignSpace => ch.is_ascii_digit() || ch == '+' || ch == '-' || ch == ' ',
            Self::Letter => ch.is_alphabetic(),
            Self::Any => !ch.is_control(),
            Self::AlphaNumeric => ch.is_alphanumeric(),
        }
    }
}

/// Parses a mask string into positions. Pure and total: an unterminated `\` at the
/// end escapes nothing and is dropped, matching the toolkit's lenient parse.
fn parse_mask(mask: &str) -> Vec<MaskChar> {
    let mut out = Vec::new();
    let mut case = CharacterCasing::Normal;
    let mut chars = mask.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if let Some(lit) = chars.next() {
                    out.push(MaskChar::Literal(lit));
                }
            }
            '<' => case = CharacterCasing::Lower,
            '>' => case = CharacterCasing::Upper,
            '|' => case = CharacterCasing::Normal,
            '0' => out.push(MaskChar::Edit { kind: EditKind::Digit, required: true, case }),
            '9' => out.push(MaskChar::Edit { kind: EditKind::DigitOrSpace, required: false, case }),
            '#' => out.push(MaskChar::Edit { kind: EditKind::DigitSignSpace, required: false, case }),
            'L' => out.push(MaskChar::Edit { kind: EditKind::Letter, required: true, case }),
            '?' => out.push(MaskChar::Edit { kind: EditKind::Letter, required: false, case }),
            '&' => out.push(MaskChar::Edit { kind: EditKind::Any, required: true, case }),
            'C' => out.push(MaskChar::Edit { kind: EditKind::Any, required: false, case }),
            'A' => out.push(MaskChar::Edit { kind: EditKind::AlphaNumeric, required: true, case }),
            'a' => out.push(MaskChar::Edit { kind: EditKind::AlphaNumeric, required: false, case }),
            other => out.push(MaskChar::Literal(other)),
        }
    }
    out
}

/// `System.Windows.Forms.MaskedTextBox` — a text box governed by a `Mask`.
///
/// The mask engine keeps a buffer of one `Option<char>` per editable position;
/// literals are fixed. `Text` returns the buffer formatted per `TextMaskFormat`.
/// The visible string always shows literals and the prompt for empty positions.
#[derive(Clone)]
pub struct MaskedTextBox {
    base: TextBoxBase,

    /// Parsed `Mask`; kept beside the raw string so painting and formatting do not
    /// re-parse. Not public — `mask()` / `set_mask()` guard the invariant that the
    /// buffer length matches the edit-position count.
    parsed: Vec<MaskChar>,
    mask: String,
    /// One slot per editable position (`None` = still showing the prompt).
    buffer: Vec<Option<char>>,

    /// `AllowPromptAsInput` — whether the prompt char may itself be typed as input.
    pub allow_prompt_as_input: bool,
    /// `AsciiOnly` — restrict letters to A–Z/a–z. Honoured by [`Self::set_text`].
    pub ascii_only: bool,
    /// `BeepOnError` — beep on an invalid keystroke. State only (no audio here).
    pub beep_on_error: bool,
    /// `Culture` — the culture whose separators the mask uses. Stored as its name;
    /// the port renders separators literally, so it is not-yet-honoured.
    pub culture: String,
    /// `CutCopyMaskFormat` — what a clipboard copy includes. Stored; drives
    /// [`Self::clipboard_text`].
    pub cut_copy_mask_format: MaskFormat,
    /// `HidePromptOnLeave` — hide the prompt when unfocused. Stored; the port has
    /// no focus, so the prompt is always shown (documented).
    pub hide_prompt_on_leave: bool,
    /// `InsertKeyMode` — insert vs. overwrite. State only.
    pub insert_key_mode: InsertKeyMode,
    /// `PasswordChar` — glyph shown for entered characters. Honoured in `paint`.
    pub password_char: Option<char>,
    /// `PromptChar` — the placeholder glyph for empty positions. Honoured.
    pub prompt_char: char,
    /// `RejectInputOnFirstFailure` — stop feeding input at the first character that
    /// does not fit. Honoured by [`Self::set_text`].
    pub reject_input_on_first_failure: bool,
    /// `ResetOnPrompt` — treat a typed prompt char as "skip this position".
    /// Stored; affects live typing, which the port does not drive (documented).
    pub reset_on_prompt: bool,
    /// `ResetOnSpace` — treat a typed space as "skip". Same status as above.
    pub reset_on_space: bool,
    /// `SkipLiterals` — let a typed literal that matches the mask advance past it.
    /// Same status as above.
    pub skip_literals: bool,
    /// `TextAlign` — honoured in `paint`.
    pub text_align: HorizontalAlignment,
    /// `TextMaskFormat` — what the `Text` property returns. Honoured by
    /// [`Self::text`].
    pub text_mask_format: MaskFormat,
    /// `UseSystemPasswordChar` — force the system glyph. Honoured in `paint`.
    pub use_system_password_char: bool,
}

impl Default for MaskedTextBox {
    fn default() -> Self {
        Self {
            base: TextBoxBase::new(),
            parsed: Vec::new(),
            mask: String::new(),
            buffer: Vec::new(),
            allow_prompt_as_input: true,
            ascii_only: false,
            beep_on_error: false,
            culture: String::new(),
            cut_copy_mask_format: MaskFormat::IncludeLiterals,
            hide_prompt_on_leave: false,
            insert_key_mode: InsertKeyMode::Default,
            password_char: None,
            prompt_char: '_',
            reject_input_on_first_failure: false,
            reset_on_prompt: true,
            reset_on_space: true,
            skip_literals: true,
            text_align: HorizontalAlignment::Left,
            text_mask_format: MaskFormat::IncludeLiterals,
            use_system_password_char: false,
        }
    }
}

impl MaskedTextBox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mask(&self) -> &str {
        &self.mask
    }

    /// `Mask = value` — re-parse and clear the buffer to one empty slot per
    /// editable position, exactly as assigning a new mask resets the toolkit's
    /// provider.
    pub fn set_mask(&mut self, mask: &str) {
        self.parsed = parse_mask(mask);
        self.mask = mask.to_string();
        self.buffer = vec![None; self.edit_count()];
        self.sync_display();
    }

    /// Number of editable positions in the current mask.
    fn edit_count(&self) -> usize {
        self.parsed.iter().filter(|m| matches!(m, MaskChar::Edit { .. })).count()
    }

    /// The on-screen string: literals verbatim, filled positions as their char (or
    /// the password glyph), empty positions as the prompt.
    fn display(&self) -> String {
        let pw = if self.use_system_password_char {
            Some(PASSWORD_GLYPH)
        } else {
            self.password_char
        };
        let mut out = String::new();
        let mut e = 0;
        for m in &self.parsed {
            match m {
                MaskChar::Literal(ch) => out.push(*ch),
                MaskChar::Edit { .. } => {
                    match self.buffer.get(e).copied().flatten() {
                        Some(filled) => out.push(pw.unwrap_or(filled)),
                        None => out.push(self.prompt_char),
                    }
                    e += 1;
                }
            }
        }
        out
    }

    /// Keeps `ControlBase::text` equal to the visible string, so the shared field
    /// painter shows the mask without knowing about it.
    fn sync_display(&mut self) {
        let d = self.display();
        self.base.control.text = d;
    }

    /// Formats the buffer under a [`MaskFormat`] — the arithmetic behind both
    /// `Text` and the clipboard, matching `MaskedTextProvider.ToString`.
    fn formatted(&self, fmt: MaskFormat) -> String {
        let (ip, il) = (fmt.include_prompt(), fmt.include_literals());
        let mut out = String::new();
        let mut e = 0;
        for m in &self.parsed {
            match m {
                MaskChar::Literal(ch) => {
                    if il {
                        out.push(*ch);
                    }
                }
                MaskChar::Edit { .. } => {
                    match self.buffer.get(e).copied().flatten() {
                        Some(filled) => out.push(filled),
                        None if ip => out.push(self.prompt_char),
                        None if il => out.push(' '),
                        None => {}
                    }
                    e += 1;
                }
            }
        }
        out
    }

    /// `Text` getter — the buffer formatted per `TextMaskFormat`. With no mask set,
    /// it is just the raw text, like the toolkit.
    pub fn text(&self) -> String {
        if self.parsed.is_empty() {
            self.base.text().to_string()
        } else {
            self.formatted(self.text_mask_format)
        }
    }

    /// What a cut/copy would place on the clipboard (per `CutCopyMaskFormat`).
    pub fn clipboard_text(&self) -> String {
        self.formatted(self.cut_copy_mask_format)
    }

    /// `Text = value` — feed the characters through the mask, filling editable
    /// positions in order. A character that does not fit its position is skipped;
    /// with `RejectInputOnFirstFailure`, feeding stops at that character (the
    /// toolkit's documented behaviour). With no mask, falls back to the base setter.
    pub fn set_text(&mut self, value: &str) {
        if self.parsed.is_empty() {
            self.base.set_text(value);
            return;
        }
        self.buffer.iter_mut().for_each(|s| *s = None);
        let slots: Vec<(EditKind, CharacterCasing)> = self
            .parsed
            .iter()
            .filter_map(|m| match m {
                MaskChar::Edit { kind, case, .. } => Some((*kind, *case)),
                MaskChar::Literal(_) => None,
            })
            .collect();
        let mut e = 0;
        for ch in value.chars() {
            if e >= slots.len() {
                break;
            }
            let (kind, case) = slots[e];
            if self.ascii_only && ch.is_alphabetic() && !ch.is_ascii_alphabetic() {
                if self.reject_input_on_first_failure {
                    break;
                }
                continue;
            }
            if kind.accepts(ch) {
                self.buffer[e] = Some(case.apply(ch));
                e += 1;
            } else if self.reject_input_on_first_failure {
                break;
            }
            // otherwise skip this input char, keep the position empty
        }
        self.sync_display();
    }

    /// Whether every REQUIRED position has been filled (`MaskCompleted`).
    pub fn mask_completed(&self) -> bool {
        let mut e = 0;
        for m in &self.parsed {
            if let MaskChar::Edit { required, .. } = m {
                if *required && self.buffer.get(e).copied().flatten().is_none() {
                    return false;
                }
                e += 1;
            }
        }
        true
    }
}

impl std::ops::Deref for MaskedTextBox {
    type Target = TextBoxBase;
    fn deref(&self) -> &TextBoxBase {
        &self.base
    }
}

impl std::ops::DerefMut for MaskedTextBox {
    fn deref_mut(&mut self) -> &mut TextBoxBase {
        &mut self.base
    }
}

impl Control for MaskedTextBox {
    fn control(&self) -> &ControlBase {
        &self.base.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base.control
    }

    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        preferred_text_size(&self.base, c, &self.display())
    }

    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        // If a password glyph is active the buffer chars are already substituted
        // inside `display()`, so the painter must not treat it as a raw password.
        let display = self.display();
        let fp = FieldPaint {
            display: &display,
            placeholder: None,
            is_password: false,
            align: self.text_align,
            runs: None,
        };
        paint_field(c, bounds, &self.base, &fp, false);
    }

    fn type_name(&self) -> &'static str {
        "MaskedTextBox"
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RichTextBox
// ═══════════════════════════════════════════════════════════════════════════════

/// One styled span of a [`RichTextBox`]. The port models the *structure* RTF would
/// carry — weight, slant, underline, colour — but does not parse or emit RTF.
#[derive(Clone)]
pub struct StyledRun {
    pub text: String,
    /// **Honoured** — the run is drawn in the system UI font's bold face
    /// (`SystemFonts::message_bold`, i.e. `lfMessageFont` at
    /// `DWRITE_FONT_WEIGHT_BOLD`), which is the same face and size as the plain
    /// text, only heavier.
    pub bold: bool,
    /// **Honoured** — the run is drawn in the system UI font's italic face
    /// (`SystemFonts::message_italic`).
    ///
    /// It was not, for as long as this family painted with the shared
    /// design-system formats: every one of them is built
    /// `DWRITE_FONT_STYLE_NORMAL`, there was no slanted format to select, and a
    /// control may not build its own font. The repaint onto the system visuals
    /// supplied one, so the slant is now a real pixel difference rather than a
    /// value carried for round-tripping.
    ///
    /// A run that is **both** bold and italic renders **bold**: the system
    /// publishes no bold-italic face — `SystemFonts` builds three formats, not
    /// four — and choosing the heavier of the two is visible where dropping both
    /// would not be.
    pub italic: bool,
    /// **Honoured** — drawn as a one-border-thick rule under the run, in the
    /// run's colour.
    pub underline: bool,
    /// `None` = the control's foreground colour. **Honoured.**
    pub color: Option<D2D1_COLOR_F>,
}

impl StyledRun {
    /// A plain, unstyled run — the common case.
    pub fn plain(text: impl Into<String>) -> Self {
        Self { text: text.into(), bold: false, italic: false, underline: false, color: None }
    }
}

/// `System.Windows.Forms.RichTextBox` — a multiline editor over styled runs.
///
/// It overrides several base defaults (`Multiline = true`, `AutoSize = false`,
/// `MaxLength = int.MaxValue`) and adds its own scrollbar type, zoom, margins and
/// the run model. `Text` is the concatenation of the runs.
#[derive(Clone)]
pub struct RichTextBox {
    base: TextBoxBase,

    /// The styled content. `Text` is the concatenation of these runs' text.
    pub runs: Vec<StyledRun>,

    /// `AutoWordSelection` — auto-extend selection to whole words. State only.
    pub auto_word_selection: bool,
    /// `BulletIndent` — indent applied to bulleted paragraphs. Stored; paragraph
    /// bullets are not modelled, so not-yet-honoured.
    pub bullet_indent: i32,
    /// `DetectUrls` — auto-format URLs as links. Stored; not honoured (no link run).
    pub detect_urls: bool,
    /// `EnableAutoDragDrop` — drag/drop of rich content. State only.
    pub enable_auto_drag_drop: bool,
    /// `RightMargin` — the right text margin in DIP. Stored; not-yet-honoured.
    pub right_margin: i32,
    /// `ScrollBars` — the rich scrollbar set. Vertical is painted.
    pub scroll_bars: RichTextBoxScrollBars,
    /// `ShowSelectionMargin` — a clickable margin on the left. Stored; not painted.
    pub show_selection_margin: bool,
    /// `ZoomFactor` — display scale, `1.0` = normal. Stored; the port renders at
    /// the shared font size, so zoom is not-yet-honoured (documented).
    pub zoom_factor: f32,
}

impl Default for RichTextBox {
    fn default() -> Self {
        let mut base = TextBoxBase::new();
        // TRAP: RichTextBox overrides three inherited defaults. Getting the order
        // wrong (e.g. leaving AutoSize = true from TextBoxBase) mis-sizes it.
        base.multiline = true; // vs. TextBoxBase's false
        base.control.auto_size = false; // back to Control's default, not TextBoxBase's true
        base.max_length = i32::MAX; // vs. TextBoxBase's 32767
        Self {
            base,
            runs: Vec::new(),
            auto_word_selection: false,
            bullet_indent: 0,
            detect_urls: true,
            enable_auto_drag_drop: false,
            right_margin: 0,
            scroll_bars: RichTextBoxScrollBars::Both,
            show_selection_margin: false,
            zoom_factor: 1.0,
        }
    }
}

impl RichTextBox {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Text` getter — the runs concatenated. (Setting plain `Text` would replace
    /// the runs with one plain run; provided as `set_plain_text`.)
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// Replace the styled content with a single unstyled run.
    pub fn set_plain_text(&mut self, value: &str) {
        self.runs = vec![StyledRun::plain(value)];
        self.base.control.text = value.to_string();
    }

    /// Replace the styled content wholesale, keeping `ControlBase::text` (the
    /// concatenation) in sync for the fallback painter and for `preferred_size`.
    pub fn set_runs(&mut self, runs: Vec<StyledRun>) {
        self.runs = runs;
        self.base.control.text = self.text();
    }

    /// `RedoActionName` — no undo/redo stack is modelled, so this is empty.
    pub fn redo_action_name(&self) -> &'static str {
        ""
    }

    /// `UndoActionName` — see [`Self::redo_action_name`].
    pub fn undo_action_name(&self) -> &'static str {
        ""
    }

    /// `SelectionType` — the port carries no embedded objects, so a non-empty
    /// selection is always `Text`.
    pub fn selection_type(&self) -> RichTextBoxSelectionTypes {
        if self.base.selection_length() > 0 {
            RichTextBoxSelectionTypes::Text
        } else {
            RichTextBoxSelectionTypes::Empty
        }
    }
}

impl std::ops::Deref for RichTextBox {
    type Target = TextBoxBase;
    fn deref(&self) -> &TextBoxBase {
        &self.base
    }
}

impl std::ops::DerefMut for RichTextBox {
    fn deref_mut(&mut self) -> &mut TextBoxBase {
        &mut self.base
    }
}

impl Control for RichTextBox {
    fn control(&self) -> &ControlBase {
        &self.base.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base.control
    }

    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        preferred_text_size(&self.base, c, &self.text())
    }

    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        let text = self.text();
        let fp = FieldPaint {
            display: &text,
            placeholder: None,
            is_password: false,
            align: HorizontalAlignment::Left,
            runs: (!self.runs.is_empty()).then_some(self.runs.as_slice()),
        };
        paint_field(c, bounds, &self.base, &fp, self.scroll_bars.has_vertical());
    }

    fn type_name(&self) -> &'static str {
        "RichTextBox"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Defaults, asserted against the reflection catalogue ──────────────

    #[test]
    fn textboxbase_defaults_match_the_catalogue() {
        let b = TextBoxBase::new();
        assert!(!b.accepts_tab);
        assert_eq!(b.border_style, BorderStyle::Fixed3D);
        assert!(b.hide_selection);
        assert_eq!(b.max_length, 32767);
        assert!(!b.modified);
        assert!(!b.multiline);
        assert!(!b.read_only);
        assert!(b.shortcuts_enabled);
        assert!(b.word_wrap);
        assert_eq!((b.selection_start(), b.selection_length()), (0, 0));
        assert!(!b.can_undo());
    }

    /// TRAP: `TextBoxBase.AutoSize` overrides `Control.AutoSize` (false) to true;
    /// `RichTextBox` overrides it back to false and also flips `Multiline` and
    /// `MaxLength`. This pins the whole override chain.
    #[test]
    fn autosize_and_the_rich_overrides_are_correct() {
        assert!(TextBoxBase::new().auto_size, "TextBoxBase forces AutoSize on");
        assert!(TextBox::new().auto_size, "TextBox inherits it");
        let r = RichTextBox::new();
        assert!(!r.auto_size, "RichTextBox turns AutoSize back off");
        assert!(r.multiline, "RichTextBox defaults Multiline on");
        assert_eq!(r.max_length, i32::MAX, "RichTextBox widens MaxLength");
        assert_eq!(r.scroll_bars, RichTextBoxScrollBars::Both);
        assert_eq!(r.zoom_factor, 1.0);
        assert!(r.detect_urls);
    }

    #[test]
    fn textbox_defaults_match_the_catalogue() {
        let t = TextBox::new();
        assert!(!t.accepts_return);
        assert_eq!(t.auto_complete_mode, AutoCompleteMode::None);
        assert_eq!(t.auto_complete_source, AutoCompleteSource::None);
        assert_eq!(t.character_casing, CharacterCasing::Normal);
        assert!(t.password_char.is_none());
        assert!(t.placeholder_text.is_empty());
        assert_eq!(t.scroll_bars, ScrollBars::None);
        assert_eq!(t.text_align, HorizontalAlignment::Left);
        assert!(!t.use_system_password_char);
        assert!(!t.multiline);
    }

    #[test]
    fn maskedtextbox_defaults_match_the_catalogue() {
        let m = MaskedTextBox::new();
        assert!(m.allow_prompt_as_input);
        assert!(!m.ascii_only);
        assert!(!m.beep_on_error);
        assert_eq!(m.cut_copy_mask_format, MaskFormat::IncludeLiterals);
        assert_eq!(m.text_mask_format, MaskFormat::IncludeLiterals);
        assert_eq!(m.insert_key_mode, InsertKeyMode::Default);
        assert_eq!(m.prompt_char, '_');
        assert!(!m.reject_input_on_first_failure);
        assert!(m.reset_on_prompt);
        assert!(m.reset_on_space);
        assert!(m.skip_literals);
        assert_eq!(m.text_align, HorizontalAlignment::Left);
        assert!(m.mask().is_empty());
        assert_eq!(m.max_length, 32767, "MaskedTextBox keeps the base MaxLength");
    }

    // ── Selection arithmetic (pure) ───────────────────────────────────────

    #[test]
    fn selection_clamps_into_the_text() {
        let mut b = TextBoxBase::new();
        b.set_text("hello"); // 5 chars
        b.set_selection_start(3);
        b.set_selection_length(10);
        assert_eq!(b.selection_start(), 3);
        assert_eq!(b.selection_length(), 2, "cannot run past the end");
        assert_eq!(b.selected_text(), "lo");
    }

    /// The key correctness rule: when the text shrinks, an existing selection must
    /// be re-clamped, never left dangling past the new end.
    #[test]
    fn selection_reclamps_when_text_shrinks() {
        let mut b = TextBoxBase::new();
        b.set_text("abcdefgh");
        b.select(5, 3); // "fgh"
        assert_eq!(b.selected_text(), "fgh");
        b.set_text("ab"); // set_text resets the caret to 0
        assert_eq!((b.selection_start(), b.selection_length()), (0, 0));
        // And a direct out-of-range set is clamped too.
        b.select(50, 50);
        assert_eq!((b.selection_start(), b.selection_length()), (2, 0));
    }

    #[test]
    fn selected_text_replacement_respects_max_length() {
        let mut b = TextBoxBase::new();
        b.max_length = 6;
        b.set_text("abcdef");
        b.select(2, 2); // "cd"
        b.set_selected_text("XYZ"); // "ab" + "XYZ" + "ef" = "abXYZef" (7) → truncated to 6
        assert_eq!(b.text(), "abXYZe");
        assert!(b.modified, "editing the selection is a user edit");
    }

    #[test]
    fn max_length_truncates_on_set_text() {
        let mut b = TextBoxBase::new();
        b.max_length = 4;
        b.set_text("abcdefgh");
        assert_eq!(b.text(), "abcd");
        b.max_length = 0; // 0 = unbounded
        b.set_text("abcdefgh");
        assert_eq!(b.text(), "abcdefgh");
    }

    #[test]
    fn character_casing_applies_on_set() {
        let mut t = TextBox::new();
        t.character_casing = CharacterCasing::Upper;
        t.set_text("aBc");
        assert_eq!(t.text(), "ABC");
    }

    #[test]
    fn lines_round_trip() {
        let mut b = TextBoxBase::new();
        b.set_lines(&["one".into(), "two".into(), "three".into()]);
        assert_eq!(b.text(), "one\ntwo\nthree");
        assert_eq!(b.lines(), vec!["one", "two", "three"]);
    }

    // ── Mask engine ───────────────────────────────────────────────────────

    #[test]
    fn mask_parsing_separates_literals_from_edits() {
        // A phone mask: "(999) 000-0000".
        let parsed = parse_mask("(999) 000-0000");
        let edits = parsed.iter().filter(|m| matches!(m, MaskChar::Edit { .. })).count();
        let literals = parsed.iter().filter(|m| matches!(m, MaskChar::Literal(_))).count();
        assert_eq!(edits, 10, "ten digit positions");
        assert_eq!(literals, parsed.len() - edits);
        // The escape turns a mask char into a literal.
        let esc = parse_mask("\\0L");
        assert!(matches!(esc[0], MaskChar::Literal('0')));
        assert!(matches!(esc[1], MaskChar::Edit { .. }));
    }

    #[test]
    fn masked_display_shows_prompt_and_literals() {
        let mut m = MaskedTextBox::new();
        m.set_mask("00/00/0000");
        assert_eq!(m.display(), "__/__/____", "empty shows prompt + literals");
        m.set_text("31122026");
        assert_eq!(m.display(), "31/12/2026");
    }

    /// The four `MaskFormat` modes drive what `Text` returns; this is the exact
    /// arithmetic of `MaskedTextProvider.ToString`.
    #[test]
    fn text_mask_format_modes() {
        let mut m = MaskedTextBox::new();
        m.set_mask("00/00");
        m.set_text("12"); // fills the first two positions only

        m.text_mask_format = MaskFormat::IncludePromptAndLiterals;
        assert_eq!(m.text(), "12/__");
        m.text_mask_format = MaskFormat::IncludeLiterals; // default
        assert_eq!(m.text(), "12/  ", "prompt hidden as spaces, literals kept");
        m.text_mask_format = MaskFormat::IncludePrompt;
        assert_eq!(m.text(), "12__", "no literals, empty positions show prompt");
        m.text_mask_format = MaskFormat::ExcludePromptAndLiterals;
        assert_eq!(m.text(), "12", "only the entered characters");
    }

    #[test]
    fn masked_input_validates_and_casing_applies() {
        let mut m = MaskedTextBox::new();
        m.set_mask(">LLL-000"); // three upper letters, dash, three digits
        m.set_text("abc123");
        assert_eq!(m.display(), "ABC-123");
        assert!(m.mask_completed());

        // A letter fed where a digit is required is skipped, not accepted-and-ignored.
        let mut m2 = MaskedTextBox::new();
        m2.set_mask("000");
        m2.set_text("1x2");
        assert_eq!(m2.formatted(MaskFormat::ExcludePromptAndLiterals), "12");
    }

    #[test]
    fn reject_on_first_failure_stops_feeding() {
        let mut m = MaskedTextBox::new();
        m.set_mask("000");
        m.reject_input_on_first_failure = true;
        m.set_text("1x2"); // stops at 'x'
        assert_eq!(m.formatted(MaskFormat::ExcludePromptAndLiterals), "1");
        assert!(!m.mask_completed());
    }

    #[test]
    fn masked_completion_tracks_required_positions() {
        let mut m = MaskedTextBox::new();
        m.set_mask("00-9"); // two required digits, a literal, one optional digit
        assert!(!m.mask_completed());
        m.set_text("12");
        assert!(m.mask_completed(), "optional position need not be filled");
    }

    // ── RichTextBox run model ─────────────────────────────────────────────

    #[test]
    fn rich_text_is_the_concatenation_of_runs() {
        let mut r = RichTextBox::new();
        r.set_runs(vec![
            StyledRun { text: "Bold".into(), bold: true, italic: false, underline: false, color: None },
            StyledRun::plain(" then plain"),
        ]);
        assert_eq!(r.text(), "Bold then plain");
        assert_eq!(r.selection_type(), RichTextBoxSelectionTypes::Empty);
        r.select(0, 4);
        assert_eq!(r.selection_type(), RichTextBoxSelectionTypes::Text);
    }

    /// `TextAlign` defaults to `Left` on both boxes per the catalogue. The type is
    /// now SHARED, so this pins the shared `#[default]` from this family's side:
    /// four copies of it had existed, and a sibling pair of another alignment type
    /// had already drifted on exactly this field.
    #[test]
    fn text_align_defaults_to_left_including_the_shared_types_own_default() {
        assert_eq!(HorizontalAlignment::default(), HorizontalAlignment::Left);
        assert_eq!(TextBox::new().text_align, HorizontalAlignment::Left);
        assert_eq!(MaskedTextBox::new().text_align, HorizontalAlignment::Left);
    }

    /// [`role_has_strong_variant`] answers about the SHARED design-system
    /// formats, which this family no longer paints with (a bold run now takes
    /// `SystemFonts::message_bold`, which every role has). The predicate is public
    /// API, so its answers are still pinned here.
    #[test]
    fn bold_promotes_the_controls_own_role() {
        // Every role the shared formats can make heavier does so...
        for role in [
            FontRole::Caption,
            FontRole::CaptionStrong,
            FontRole::Body,
            FontRole::BodyStrong,
            FontRole::Heading,
        ] {
            assert!(role_has_strong_variant(role), "{role:?} must have a strong step");
        }
        // ...and the one that cannot is Title, whose hierarchy is carried by size.
        assert!(
            !role_has_strong_variant(FontRole::Title),
            "Title has no heavier format in the shared set"
        );
    }

    /// `italic` is now painted (`SystemFonts::message_italic`), and it must still
    /// survive a round-trip through the model — the pixels are checked against the
    /// reference sheet, the storage here.
    #[test]
    fn italic_is_preserved_in_the_model() {
        let mut r = RichTextBox::new();
        r.set_runs(vec![StyledRun {
            text: "penché".into(),
            bold: false,
            italic: true,
            underline: false,
            color: None,
        }]);
        assert!(r.runs[0].italic, "the slant is kept in the model");
        assert_eq!(r.text(), "penché");
    }

    // ── Deref chain wiring ────────────────────────────────────────────────

    #[test]
    fn deref_reaches_control_base_through_two_levels() {
        let mut t = TextBox::new();
        // TextBox → TextBoxBase (multiline) → ControlBase (name/enabled).
        t.multiline = true;
        t.name = "champ".into();
        assert!(t.base.multiline);
        assert_eq!(t.control().name, "champ");
        assert_eq!(t.type_name(), "TextBox");
    }

    #[test]
    fn preferred_height_grows_with_the_border() {
        let mut b = TextBoxBase::new();
        b.border_style = BorderStyle::None;
        let none = b.preferred_height();
        b.border_style = BorderStyle::Fixed3D;
        assert!(b.preferred_height() > none, "a border adds vertical inset");
    }
}
