//! `ButtonBase` → `Button`, `CheckBox`, `RadioButton`.
//!
//! ## Why the shape of this file
//!
//! In WinForms `ButtonBase` is the abstract parent of the three clickable
//! controls. The reflection catalogue says it **declares 18** designer-visible
//! properties; `Button` adds 2, `CheckBox` 7, `RadioButton` 6. The chain is
//! mirrored with composition + `Deref`, exactly as [`crate::control`] describes:
//! a leaf owns only what its .NET counterpart *declares* and reaches everything
//! above through `Deref`.
//!
//! ### The re-declaration trap
//!
//! Three of `ButtonBase`'s 18 — `Text`, `BackColor`, `AutoSize` — are **not new
//! storage**: `Control` already declares all three, and `ButtonBase` merely
//! re-declares them to change designer metadata (browsability, serialization).
//! The port keeps them where they belong, on [`ControlBase`], and a
//! `ButtonBase` reaches them through `Deref`. Duplicating them here would be the
//! very drift the composition design exists to prevent. The same is true of
//! `Button.AutoSizeMode` (already `ControlBase::auto_size_mode`) and of
//! `RadioButton.TabStop` (already `ControlBase::tab_stop`) — both are
//! re-declarations that only *change the default*, which the leaf's `Default`
//! impl does, without a second field.
//!
//! So this module adds **15** real fields to `ButtonBase`, **1** to `Button`
//! (`DialogResult`), **5** to `CheckBox` (`Checked` is a projection of
//! `CheckState`, not a field) and **4** to `RadioButton`.
//!
//! ### What it looks like
//!
//! The family paints **real WinForms**, not the Kubuno skin: square corners, the
//! `GetSysColor` palette, the `SPI_GETNONCLIENTMETRICS` UI font, and borders
//! drawn as `DrawEdge` bevels ([`ControlCanvas::draw_edge`]) rather than as
//! strokes. Nothing in this file names a colour, a family or a point size — they
//! all come from [`crate::system::Visuals`], so the replica follows whatever the
//! machine is configured with, which is the only way it can match a reference
//! sheet the local toolkit painted.
//!
//! ### Two renderings, not one with an approximation
//!
//! That `GetSysColor` paint is the whole rendering on a machine with visual
//! styles **off**, and the only one there. With them **on** — which is how the
//! reference sheets were made — the toolkit hands the face and the glyph to
//! `uxtheme.dll`, and the difference is not a shade: the sheet's buttons are
//! `#FDFDFD` inside `#D0D0D0`, its check boxes `#626262` around `#F3F3F3`, its
//! ticked ones a solid accent `#005FB8`, and **not one** of those is any
//! `COLOR_*` index on the same machine. So each of the three drawings below is
//! one paint with two branches:
//!
//! ```ignore
//! if !c.draw_theme_part(theme::class::BUTTON, part, state, rect, ground) {
//!     // the existing classic painting, unchanged
//! }
//! ```
//!
//! Three parts are adopted — `BP_PUSHBUTTON` for the face, `BP_CHECKBOX` and
//! `BP_RADIOBUTTON` for the two glyphs — and every `(part, state)` pair was
//! chosen by rendering it and sampling it against the sheet, never by reading its
//! name. The tests at the bottom of the file pin those pixels, so a Windows
//! update that moves them is a failing test rather than a slow drift.
//!
//! Which `FlatStyle`s take that branch is itself a measurement:
//! [`face_is_themed`] and [`glyph_is_themed`] carry the sheet evidence that
//! `Standard` is theme-drawn just as `System` is, while `Flat` and `Popup` — on
//! the same sheet, in the same form — are not.
//!
//! One measurement is knowingly left behind: `Control::preferred_size` takes a
//! bare `Canvas`, which cannot answer for the system font, so widths are still
//! measured against the shared formats while the caption is *drawn* in the
//! system face. Paint and measure will only agree once that signature widens.
//!
//! ### Defaults that change down the chain — order matters
//!
//! `TextAlign` is `MiddleCenter` on `ButtonBase` but `MiddleLeft` on both
//! `CheckBox` and `RadioButton`; `CheckAlign` is `MiddleLeft`; `RadioButton`
//! turns `TabStop` off. A `Default` impl that built the base and forgot to
//! stamp these overrides would silently inherit the button's centering. Each
//! leaf's `Default` therefore *builds the base, then overrides* — and a test
//! pins every one of these values against the catalogue.

use kubuno_drive_desktop_app_controls::TextFormats;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
};

use crate::control::{Control, ControlBase, ControlCanvas, ControlState, FontRole};
use crate::enums::*;
use crate::system::{Border3DSide, Border3DStyle, SystemColors, SystemFonts};
use crate::theme;
use crate::theme::part::BP_PUSHBUTTON;
use crate::theme::state::{PBS_DEFAULTED, PBS_DISABLED, PBS_HOT, PBS_NORMAL, PBS_PRESSED};
use crate::{Canvas, Rect};

/// The two `BUTTON` parts this family draws that [`crate::theme::part`] does not
/// publish — the check box and the radio glyphs — with the twenty state ids that
/// go with them.
///
/// They live **here** rather than in `theme.rs` because `theme.rs` publishes only
/// what has been adopted, and adopting these is this file's job; the family that
/// measures a part is the one that names it. As there, they are re-exported from
/// the Windows SDK's own constants rather than written as numbers: a part id is
/// exactly the kind of magic number that is copied wrong once and then merely
/// looks « a bit off » forever, and a wrong id does not fail — it draws something
/// plausible.
///
/// Every one of them is pinned by a pixel test at the bottom of this file, and
/// the three the reference sheet actually contains are pinned to the sheet's own
/// bytes.
mod glyph_theme {
    use windows::Win32::UI::Controls as sdk;

    /// `BUTTON` — the check box glyph, tick and all. The theme draws the box,
    /// its fill AND its mark, so the whole classic glyph is the fallback.
    pub const BP_CHECKBOX: i32 = sdk::BP_CHECKBOX.0;
    /// `BUTTON` — the radio glyph, dot and all.
    pub const BP_RADIOBUTTON: i32 = sdk::BP_RADIOBUTTON.0;

    /// `BP_CHECKBOX`, unticked: normal, hot, pressed, disabled (1–4).
    pub const CBS_UNCHECKEDNORMAL: i32 = sdk::CBS_UNCHECKEDNORMAL.0;
    pub const CBS_UNCHECKEDHOT: i32 = sdk::CBS_UNCHECKEDHOT.0;
    pub const CBS_UNCHECKEDPRESSED: i32 = sdk::CBS_UNCHECKEDPRESSED.0;
    pub const CBS_UNCHECKEDDISABLED: i32 = sdk::CBS_UNCHECKEDDISABLED.0;
    /// `BP_CHECKBOX`, ticked (5–8).
    pub const CBS_CHECKEDNORMAL: i32 = sdk::CBS_CHECKEDNORMAL.0;
    pub const CBS_CHECKEDHOT: i32 = sdk::CBS_CHECKEDHOT.0;
    pub const CBS_CHECKEDPRESSED: i32 = sdk::CBS_CHECKEDPRESSED.0;
    pub const CBS_CHECKEDDISABLED: i32 = sdk::CBS_CHECKEDDISABLED.0;
    /// `BP_CHECKBOX`, the third state (9–12). The theme calls it **MIXED**;
    /// WinForms calls it `Indeterminate`. Same thing, and the only pair of names
    /// in this file that do not match — which is precisely why it is written down.
    pub const CBS_MIXEDNORMAL: i32 = sdk::CBS_MIXEDNORMAL.0;
    pub const CBS_MIXEDHOT: i32 = sdk::CBS_MIXEDHOT.0;
    pub const CBS_MIXEDPRESSED: i32 = sdk::CBS_MIXEDPRESSED.0;
    pub const CBS_MIXEDDISABLED: i32 = sdk::CBS_MIXEDDISABLED.0;

    /// `BP_RADIOBUTTON`, unselected (1–4).
    pub const RBS_UNCHECKEDNORMAL: i32 = sdk::RBS_UNCHECKEDNORMAL.0;
    pub const RBS_UNCHECKEDHOT: i32 = sdk::RBS_UNCHECKEDHOT.0;
    pub const RBS_UNCHECKEDPRESSED: i32 = sdk::RBS_UNCHECKEDPRESSED.0;
    pub const RBS_UNCHECKEDDISABLED: i32 = sdk::RBS_UNCHECKEDDISABLED.0;
    /// `BP_RADIOBUTTON`, selected (5–8). There is **no** mixed group: a radio has
    /// two states, which is the structural difference between the two parts.
    pub const RBS_CHECKEDNORMAL: i32 = sdk::RBS_CHECKEDNORMAL.0;
    pub const RBS_CHECKEDHOT: i32 = sdk::RBS_CHECKEDHOT.0;
    pub const RBS_CHECKEDPRESSED: i32 = sdk::RBS_CHECKEDPRESSED.0;
    pub const RBS_CHECKEDDISABLED: i32 = sdk::RBS_CHECKEDDISABLED.0;
}

use glyph_theme::*;

// ─────────────────────────────────────────────────────────────────────────────
// Layout metrics (DIP). These are geometry, not typography: the canvas paints in
// DIP and pixel-aligns internally (see `Canvas`), so nothing here is multiplied
// by `scale()`. Point sizes and font families are never named here — they come
// from `Visuals::fonts`, which reads the real UI font from Windows — only box
// sizes are, the same way `ControlBase` hard-codes its `Margin` of 3 DIP.
//
// Every *colour* and every *border* below comes from `Visuals` too: this family
// paints the WinForms surface, so a bevel is `ControlCanvas::draw_edge` (Win32's
// `DrawEdge`) over a `SystemColors` face, never a rounded stroke over a Kubuno
// palette. There is no corner radius anywhere in this file on purpose — a
// WinForms button is square, and the one control that is genuinely round (the
// radio glyph) says so where it is drawn.
// ─────────────────────────────────────────────────────────────────────────────

/// The classic check/radio glyph box, 13×13 DIP at 96 DPI — the size the toolkit
/// has drawn a checkbox tick in since forever.
const GLYPH: f32 = 13.0;
/// The tick / indeterminate bar drawn inside a glyph's **well** — the interior
/// the 3-D edge leaves behind, not the [`GLYPH`] box itself. Two rings of bevel
/// eat 2 DIP off each side of the 13 DIP box, so a mark sized to the box would
/// climb over its own border.
const MARK: f32 = 9.0;
/// Gap between the glyph and its label (`CheckBox` lays the two out side by side).
const GLYPH_TEXT_GAP: f32 = 4.0;
/// Content inset inside a button face, horizontally and vertically.
const BTN_PAD_X: f32 = 8.0;
const BTN_PAD_Y: f32 = 3.0;
/// WinForms' default button height (75×23). Used as a floor so a short label
/// still yields a button-shaped box.
const MIN_BTN_HEIGHT: f32 = 23.0;

/// A role's line height in DIP — the vertical space one line of that format
/// occupies. Taken from the shared formats' documented sizes (body 14/20,
/// caption 12/16, heading 16/24, title 20/28), so the number tracks the format
/// rather than a hard-coded point size.
///
/// It is what `preferred_size` measures with, so it stays as the parity work
/// established it; the paint places its text BAND with the same number, so the
/// caption sits where the measurement said it would. `SystemFonts` publishes an
/// em size but no line height, which is the one thing that would let this follow
/// the real UI font instead.
const fn line_height(role: FontRole) -> f32 {
    match role {
        FontRole::Caption | FontRole::CaptionStrong => 16.0,
        FontRole::Body | FontRole::BodyStrong => 20.0,
        FontRole::Heading => 24.0,
        FontRole::Title => 28.0,
    }
}

/// Resolves a [`FontRole`] to one of the shared DirectWrite formats. A control
/// never holds a font object; it holds a role and looks it up at paint time.
///
/// **Measurement only.** `Control::preferred_size` receives a bare
/// [`Canvas`], which cannot answer for the system's fonts, so the arithmetic
/// still runs against the shared formats; the *painting* goes through
/// [`message_format`] and uses the real UI font. Widening `preferred_size` to a
/// `&dyn ControlCanvas` is what would let the two agree, and is deliberately not
/// done here — this pass repaints, it does not re-measure.
fn format_for(fmts: &TextFormats, role: FontRole) -> &IDWriteTextFormat {
    match role {
        FontRole::Caption => &fmts.caption,
        FontRole::CaptionStrong => &fmts.caption_strong,
        FontRole::Body => &fmts.body,
        FontRole::BodyStrong => &fmts.body_strong,
        FontRole::Heading => &fmts.heading,
        FontRole::Title => &fmts.title,
    }
}

/// The **system** UI font a control paints its `Text` with —
/// `NONCLIENTMETRICS.lfMessageFont`, which is what .NET Core resolves
/// `Control.DefaultFont` to (Segoe UI 9 pt on a default Windows 11).
///
/// A `ButtonBase` carries a [`FontRole`] rather than a `Font`, and the system
/// publishes exactly one UI face in three weights, so the role collapses to the
/// only distinction that survives: whether it is a **strong** role, which takes
/// the bold cut. Everything else — family, size, italic — is the machine's,
/// never this file's.
fn message_format(fonts: &SystemFonts, role: FontRole) -> &IDWriteTextFormat {
    match role {
        FontRole::CaptionStrong | FontRole::BodyStrong => &fonts.message_bold,
        _ => &fonts.message,
    }
}

/// Maps a horizontal alignment fraction (0.0/0.5/1.0 from
/// [`ContentAlignment::fractions`]) to the DirectWrite alignment the canvas
/// wants. `RightToLeft` mirroring is not applied here yet — documented on the
/// call site.
fn h_alignment(frac: f32) -> DWRITE_TEXT_ALIGNMENT {
    if frac <= 0.0 {
        DWRITE_TEXT_ALIGNMENT_LEADING
    } else if frac >= 1.0 {
        DWRITE_TEXT_ALIGNMENT_TRAILING
    } else {
        DWRITE_TEXT_ALIGNMENT_CENTER
    }
}

/// Rebuilds a [`ContentAlignment`] from a `(horizontal, vertical)` fraction
/// pair — the inverse of [`ContentAlignment::fractions`], needed because the
/// `System` coercions below mix one axis from one property with the other axis
/// from another.
fn align_from(h: f32, v: f32) -> ContentAlignment {
    match (v <= 0.0, v >= 1.0, h <= 0.0, h >= 1.0) {
        (true, _, true, _) => ContentAlignment::TopLeft,
        (true, _, _, true) => ContentAlignment::TopRight,
        (true, ..) => ContentAlignment::TopCenter,
        (_, true, true, _) => ContentAlignment::BottomLeft,
        (_, true, _, true) => ContentAlignment::BottomRight,
        (_, true, ..) => ContentAlignment::BottomCenter,
        (_, _, true, _) => ContentAlignment::MiddleLeft,
        (_, _, _, true) => ContentAlignment::MiddleRight,
        _ => ContentAlignment::MiddleCenter,
    }
}

/// One **device** pixel, expressed in the DIP the canvas draws in.
///
/// This is the one legitimate use of `Canvas::scale`, and it DIVIDES rather than
/// multiplies: every ring of a WinForms border is one physical pixel at every
/// scale — a 2 DIP border at 200 % would be four pixels of bevel, which is not a
/// shape the toolkit has.
fn device_pixel(c: &dyn Canvas) -> f32 {
    1.0 / c.scale().max(0.01)
}

/// The interaction state as it actually reaches the painting.
///
/// A disabled control never reacts to the pointer — the toolkit stops routing
/// mouse messages to it, so it can be neither hot nor pressed nor focused.
/// Neutralising the state here, once, is what keeps a disabled button from
/// lighting up under the cursor; testing `enabled` at each of the half-dozen
/// use sites is how that bug gets reintroduced.
fn effective_state(enabled: bool, state: ControlState) -> ControlState {
    if enabled {
        state
    } else {
        ControlState::default()
    }
}

/// Which `BP_PUSHBUTTON` state a button is in — the themed counterpart of
/// [`effective_state`] plus [`ButtonBase::paint_border`]'s pushed/resting choice.
///
/// The ORDER is the whole content of the function, and it is the theme's own,
/// not an arbitrary one:
///
/// * **disabled first**, because a disabled control is never hot or pressed and
///   a themed disabled face is a distinct rendering (`#F9F9F9` inside `#E9E9E9`
///   on a default Windows 11), not a greyed caption over the resting one;
/// * **pressed before hot**, because the pointer is necessarily over a control
///   it is pressing, so testing hot first would make a press unreachable;
/// * a **checked** toggle in `Appearance::Button` reads as pressed — that is the
///   same equivalence [`ButtonBase::paint_border`] makes for the classic bevel,
///   kept in one place so the two renderings cannot disagree about what
///   "pushed" means;
/// * **defaulted before normal**, so the form's `AcceptButton` gets the theme's
///   accented frame instead of the classic `WindowFrame` ring.
///
/// * **focused reads as defaulted**, which is a measurement rather than a guess:
///   on the reference sheet the `Standard` button — the first control on the
///   form, and the only one holding the focus — is framed in the accent
///   `#0078D4`, while the button *labelled* « Default (accept) » is framed in the
///   resting `#D0D0D0`. The sheet's own source never sets `AcceptButton`, so the
///   accent cannot be coming from « is default »: it is coming from the focus.
///   `PBS_DEFAULTED` is the theme's only accented push-button state, and .NET's
///   `ButtonStandardAdapter` picks it for `IsDefault || Focused` — which is
///   exactly what the sheet shows.
///
/// Pure, so the ordering is tested without a canvas or a theme.
fn push_button_state(enabled: bool, state: ControlState, checked: bool) -> i32 {
    if !enabled {
        PBS_DISABLED
    } else if state.pressed || checked {
        PBS_PRESSED
    } else if state.hot {
        PBS_HOT
    } else if state.default || state.focused {
        PBS_DEFAULTED
    } else {
        PBS_NORMAL
    }
}

/// Whether a face is the THEME's to draw, for this `FlatStyle` and this
/// [`FaceFill`] source.
///
/// Two independent conditions, and both are measured on the reference sheet:
///
/// * **`Standard` is theme-drawn, not just `System`.** Every button on the sheet
///   except the two that name another style is `FlatStyle::Standard` — the
///   toolkit's default — and every one of them is a themed part: « Default
///   (accept) » and « AutoSize » are `#D0D0D0`/`#FDFDFD`/`#BABABA`
///   (`PBS_NORMAL`), « Disabled » is `#E9E9E9`/`#F9F9F9` (`PBS_DISABLED`), and
///   the `Appearance=Button` check box is `#D0D0D0`/`#FDFDFD` too. A classic
///   `DrawEdge` bevel over `COLOR_BTNFACE` would be `#F0F0F0` inside
///   `#FFFFFF`/`#A0A0A0` — it is not what the sheet holds. `Flat` (`#000000`
///   ring over `#F0F0F0`) and `Popup` (`#808080` ring over `#F0F0F0`) are on the
///   same sheet and are **not** themed, which is what makes this a distinction
///   rather than a blanket.
/// * **only when the face is the theme's to give.** A `Standard` button with an
///   explicit `BackColor` paints that colour, so asking the theme first would
///   silently overpaint it; `FlatStyle::System` cannot reach that arm at all
///   (its Remarks say `BackColor` « will be ignored for button controls »), so
///   the one condition covers both styles without a special case.
///
/// Pure, so « which styles are themed » is a tested fact rather than a condition
/// buried in a paint method.
fn face_is_themed(flat_style: FlatStyle, fill: FaceFill) -> bool {
    matches!(flat_style, FlatStyle::Standard | FlatStyle::System) && fill == FaceFill::VisualStyle
}

/// Whether a check/radio GLYPH is the theme's to draw.
///
/// The same two styles as [`face_is_themed`], and for the same reason: the
/// sheet's check boxes and radio buttons are all `FlatStyle::Standard` and all
/// themed parts (`#626262` over `#F3F3F3` unticked, accent `#005FB8` ticked),
/// while `Flat` and `Popup` keep the flat/lite rendering [`glyph_edge`] draws —
/// which is the whole reason those two members exist.
///
/// Unlike a face, a glyph has **no** `BackColor` arm: `BackColor` is the
/// control's ground, and the glyph is composited onto it rather than filled with
/// it, so a caller who names a background still gets the toolkit's own box.
fn glyph_is_themed(flat_style: FlatStyle) -> bool {
    matches!(flat_style, FlatStyle::Standard | FlatStyle::System)
}

/// Where a glyph's state id sits **within** its check group: normal, hot,
/// pressed or disabled, as the `0..=3` offset every group of four repeats.
///
/// Both parts lay their states out the same way — `CBS_*` in three groups of
/// four, `RBS_*` in two — so the ordering rule is written once. It is the same
/// order [`push_button_state`] uses, and for the same reasons: **disabled first**
/// (a dead control is never hot, and its themed glyph is a distinct rendering,
/// not a greyed one), then **pressed before hot** (the pointer is necessarily
/// over a control it is pressing, so testing hot first would make a press
/// unreachable).
///
/// There is deliberately no `focused` arm: unlike `BP_PUSHBUTTON`, neither glyph
/// part has an accented state, so focus is shown by the focus rectangle alone —
/// exactly as the toolkit does it.
fn glyph_state_offset(enabled: bool, state: ControlState) -> i32 {
    if !enabled {
        3
    } else if state.pressed {
        2
    } else if state.hot {
        1
    } else {
        0
    }
}

/// Which `BP_CHECKBOX` state a check box is in.
///
/// The three groups are chosen by `CheckState` and the offset within a group by
/// [`glyph_state_offset`]. The arithmetic is spelled out as twelve named
/// constants rather than as `base + offset` on purpose: the ids only *happen* to
/// be contiguous, and a table that names each one is what makes a Windows update
/// that renumbers them a compile error instead of a glyph that quietly shows the
/// wrong mark.
///
/// `Indeterminate` maps to the theme's **MIXED** group — the one place where the
/// toolkit's name and the theme's differ.
fn check_box_state(enabled: bool, state: ControlState, check: CheckState) -> i32 {
    let group = match check {
        CheckState::Unchecked => [
            CBS_UNCHECKEDNORMAL,
            CBS_UNCHECKEDHOT,
            CBS_UNCHECKEDPRESSED,
            CBS_UNCHECKEDDISABLED,
        ],
        CheckState::Checked => {
            [CBS_CHECKEDNORMAL, CBS_CHECKEDHOT, CBS_CHECKEDPRESSED, CBS_CHECKEDDISABLED]
        }
        CheckState::Indeterminate => {
            [CBS_MIXEDNORMAL, CBS_MIXEDHOT, CBS_MIXEDPRESSED, CBS_MIXEDDISABLED]
        }
    };
    group[glyph_state_offset(enabled, state) as usize]
}

/// Which `BP_RADIOBUTTON` state a radio is in — the same shape as
/// [`check_box_state`] with **two** groups instead of three, because a radio has
/// no third state to be in.
fn radio_button_state(enabled: bool, state: ControlState, checked: bool) -> i32 {
    let group = if checked {
        [RBS_CHECKEDNORMAL, RBS_CHECKEDHOT, RBS_CHECKEDPRESSED, RBS_CHECKEDDISABLED]
    } else {
        [RBS_UNCHECKEDNORMAL, RBS_UNCHECKEDHOT, RBS_UNCHECKEDPRESSED, RBS_UNCHECKEDDISABLED]
    };
    group[glyph_state_offset(enabled, state) as usize]
}

/// `TextAlign` as the toolkit actually applies it. Under `FlatStyle::System`
/// the OS draws the button and the `FlatStyle` Remarks list `TextAlign` among
/// the values it ignores, so the label falls back to the centred default.
///
/// This is Button-only on purpose: the same Remarks carry a Note saying that for
/// `CheckBox` and `RadioButton` under `System` « the text alignment remains
/// unchanged » — only the CHECK alignment is coerced (see
/// [`system_check_align`]). Applying the button rule to all three would silently
/// discard a `TextAlign` the toolkit honours.
fn effective_text_align(flat_style: FlatStyle, declared: ContentAlignment) -> ContentAlignment {
    if flat_style == FlatStyle::System {
        ContentAlignment::MiddleCenter
    } else {
        declared
    }
}

/// `CheckAlign` as the toolkit actually applies it under `FlatStyle::System`,
/// from the `FlatStyle` Remarks' Note: « The check box is horizontally aligned
/// with either the left or right edge of the control (a left or center
/// alignment appears left aligned, right remains unchanged), and vertically
/// aligned the same as the descriptive text. »
///
/// So the horizontal axis collapses to left-or-right and the vertical axis is
/// taken from `TextAlign`, not from `CheckAlign`. The property value itself is
/// unchanged — only its appearance — which is why this is a paint-time function
/// and never writes back to the field.
fn system_check_align(
    flat_style: FlatStyle,
    check: ContentAlignment,
    text: ContentAlignment,
) -> ContentAlignment {
    if flat_style != FlatStyle::System {
        return check;
    }
    let (ch, _) = check.fractions();
    let (_, tv) = text.fractions();
    align_from(if ch >= 1.0 { 1.0 } else { 0.0 }, tv)
}

/// Positions a single line of height `lh` inside `content` at the vertical
/// fraction `v`, returning the band the canvas should draw the line into (the
/// canvas centres text vertically within whatever rect it is given, so a band of
/// exactly one line height reproduces top/middle/bottom placement).
fn text_band(content: Rect, lh: f32, v: f32) -> Rect {
    let slack = (content.bottom - content.top - lh).max(0.0);
    let top = content.top + slack * v;
    Rect::new(content.left, top, content.right, top + lh)
}

// ─────────────────────────────────────────────────────────────────────────────
// Family-local enums and value types. These are declared by ButtonBase/Button
// and by no other family, so they live here rather than in the shared `enums`
// module — the same rule the shared enums file states about not inventing.
// ─────────────────────────────────────────────────────────────────────────────

/// `TextImageRelation` — where the image sits relative to the text on a button.
/// The discriminants are the toolkit's own (a flag-like set), so a round-trip is
/// exact. Only `Overlay` affects painting today; the four positioned layouts are
/// carried but not yet honoured (the port has no image pipeline — see [`ImageSource`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextImageRelation {
    #[default]
    Overlay = 0,
    ImageAboveText = 1,
    TextAboveImage = 2,
    ImageBeforeText = 4,
    TextBeforeImage = 8,
}

/// `DialogResult` — what a `Button` reports to a modal form when clicked. The
/// discriminants match `System.Windows.Forms.DialogResult` exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DialogResult {
    #[default]
    None = 0,
    OK = 1,
    Cancel = 2,
    Abort = 3,
    Retry = 4,
    Ignore = 5,
    Yes = 6,
    No = 7,
    TryAgain = 10,
    Continue = 11,
}

/// An opaque handle to a bitmap the host would resolve. The port has no raster
/// pipeline yet, so `Image`/`ImageList` are stored but never painted; keeping a
/// distinct newtype (rather than a bare `String`) prevents an image key from
/// being mistaken for text. **Not yet honoured** in [`Control::paint`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSource(pub String);

/// Which source a button face takes its colour from. Split out so the whole
/// precedence is a pure, tested decision rather than a chain of `if`s buried in
/// a paint method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FaceFill {
    /// `FlatAppearance.MouseDownBackColor`.
    MouseDown,
    /// `FlatAppearance.MouseOverBackColor`.
    MouseOver,
    /// `FlatAppearance.CheckedBackColor`.
    FlatChecked,
    /// The caller's explicit `BackColor`.
    Explicit,
    /// The theme's visual-style face.
    VisualStyle,
}

/// Which of the three `FlatAppearance` fills the caller has actually set. Only
/// « is it set », never the colour — the decision is about precedence, and
/// keeping colours out is what lets it stay canvas-free.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct FlatFillsSet {
    over:    bool,
    down:    bool,
    checked: bool,
}

/// The face-colour precedence, exactly as the toolkit documents it.
///
/// The order is **not** what one would guess, and two rules come straight from
/// Microsoft's own wording rather than from intuition:
///
/// 1. **`FlatStyle::System` ignores `BackColor` entirely.** The `FlatStyle`
///    Remarks: « If the System style is used, the appearance of the control is
///    determined by the user's operating system and the following property
///    values will be ignored: `BackgroundImage`, `ImageAlign`, `Image`,
///    `ImageIndex`, `ImageList`, and `TextAlign`. In addition, the
///    `Control.BackColor` property will be ignored for button controls. » So
///    `System` short-circuits to the visual-style face before anything else.
/// 2. **The `FlatAppearance` fills apply only under `FlatStyle::Flat`** — the
///    class is documented as « properties that specify the appearance of Button
///    controls whose `FlatStyle` is `Flat` ».
/// 3. **The mouse states outrank an explicit `BackColor`**, which is the
///    opposite of what a « the caller named a colour, so it wins » reading
///    suggests. `CheckedBackColor` settles it in its own description: « the
///    color of the client area of the button when the button is checked **and
///    the mouse pointer is outside the bounds of the control** ». `BackColor`
///    is the RESTING face; `MouseOverBackColor`/`MouseDownBackColor` are what
///    replaces it while the pointer is there. Ranking `BackColor` above them
///    would mean that naming a face colour permanently disables hover feedback
///    — the exact opposite of what those two properties exist for.
/// 4. `checked` only reaches `CheckedBackColor` when NOT hot, per the same
///    sentence.
/// 5. Otherwise an explicit `BackColor`, else the visual-style face. An unset
///    `BackColor` is AMBIENT (« take the parent's »), and the ambient
///    resolution here *is* the theme face, so `UseVisualStyleBackColor == false`
///    with no colour needs no branch — both paths land on `VisualStyle` and
///    nothing is silently dropped.
fn face_fill_source(
    flat_style: FlatStyle,
    back_color_set: bool,
    set: FlatFillsSet,
    state: ControlState,
    checked: bool,
) -> FaceFill {
    // (1) The OS owns the face; BackColor is ignored for button controls.
    if flat_style == FlatStyle::System {
        return FaceFill::VisualStyle;
    }
    // (2)-(4) FlatAppearance, under FlatStyle::Flat only.
    if flat_style == FlatStyle::Flat {
        if state.pressed && set.down {
            return FaceFill::MouseDown;
        }
        if state.hot && set.over {
            return FaceFill::MouseOver;
        }
        if checked && !state.hot && set.checked {
            return FaceFill::FlatChecked;
        }
    }
    // (5)
    if back_color_set {
        return FaceFill::Explicit;
    }
    FaceFill::VisualStyle
}

/// A named `ImageList` the host owns. **Not yet honoured** (see [`ImageSource`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageListRef(pub String);

/// An `ICommand` bound to the control's click. The port has no command bus, so
/// this is an opaque identifier the host dispatches. **Not yet honoured** — the
/// control never invokes it, because a control owns no behaviour loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRef(pub String);

/// `FlatButtonAppearance` — the get-only sub-object exposing the border colour,
/// border thickness and mouse-state fills used when `FlatStyle == Flat`. In .NET
/// the *property* is read-only (you mutate the object, not the reference), which
/// is why `ButtonBase` owns one by value.
///
/// **All five are honoured** through `Control::paint_with_state`. Every one of
/// them applies under `FlatStyle::Flat` only — the class is documented as
/// « properties that specify the appearance of `Button` controls whose
/// `FlatStyle` is `Flat` » — so a caller who sets them on a `Standard` button
/// correctly sees nothing, exactly as in the toolkit.
#[derive(Clone, Default)]
pub struct FlatButtonAppearance {
    /// Border colour of a `Flat` button; `None` falls back to the theme.
    pub border_color: Option<D2D1_COLOR_F>,
    /// Border thickness in DIP. WinForms default is 1; **0 means no border**,
    /// which is why the paint tests the width before stroking.
    pub border_size: i32,
    /// The face of a CHECKED `Flat` toggle — and, per its own description, only
    /// « when the button is checked *and the mouse pointer is outside the bounds
    /// of the control* », which is what ranks it below the two mouse states.
    pub checked_back_color: Option<D2D1_COLOR_F>,
    /// The face while the pointer is DOWN on the control. Outranks
    /// `mouse_over_back_color`.
    pub mouse_down_back_color: Option<D2D1_COLOR_F>,
    /// The face while the pointer is OVER the control.
    pub mouse_over_back_color: Option<D2D1_COLOR_F>,
}

// ═════════════════════════════════════════════════════════════════════════════
// ButtonBase — the 18 declared, of which 15 are new storage.
// ═════════════════════════════════════════════════════════════════════════════

/// The abstract base of `Button`, `CheckBox` and `RadioButton`.
///
/// `Text`, `BackColor` and `AutoSize` are declared here in .NET but are pure
/// re-declarations of `Control` members — they live on [`ControlBase`] and are
/// reached through `Deref`, never duplicated (see the module docs).
// No `Debug`: it carries colours (`D2D1_COLOR_F`) and a `ControlBase`, neither of
// which is `Debug`, matching the house rule in `control.rs`.
#[derive(Clone)]
pub struct ButtonBase {
    control: ControlBase,

    /// `AutoEllipsis` — trim overflowing text with « … » instead of clipping.
    pub auto_ellipsis: bool,
    /// `FlatStyle` — how the face is drawn. The four values are all painted.
    pub flat_style: FlatStyle,
    /// `FlatAppearance` — read-only sub-object; see [`FlatButtonAppearance`].
    pub flat_appearance: FlatButtonAppearance,

    /// `TextAlign` — where the label sits in the box. Painted via
    /// [`ContentAlignment::fractions`], except under `FlatStyle::System`, whose
    /// Remarks list `TextAlign` among the values the OS ignores for buttons —
    /// see [`effective_text_align`].
    pub text_align: ContentAlignment,
    /// `ImageAlign` — where the image sits. Carried; **not yet honoured**.
    pub image_align: ContentAlignment,
    /// `TextImageRelation` — relative placement of text and image.
    pub text_image_relation: TextImageRelation,

    /// `Image` — the bitmap on the face. **Not yet honoured** (no raster pipeline).
    pub image: Option<ImageSource>,
    /// `ImageIndex` — index into `ImageList`; WinForms default is `-1` (« none »).
    pub image_index: i32,
    /// `ImageKey` — key into `ImageList`; empty means « none ».
    pub image_key: String,
    /// `ImageList` — the source list. **Not yet honoured.**
    pub image_list: Option<ImageListRef>,

    /// `UseMnemonic` — treat `&&x` as a mnemonic. Carried; the port does not draw
    /// the underline or wire the accelerator yet, but the flag is honoured to the
    /// extent that the raw `&&` is left in `Text` unmodified when `false`.
    pub use_mnemonic: bool,
    /// `UseCompatibleTextRendering` — GDI+ vs GDI text metrics. The port has one
    /// text stack (DirectWrite), so this cannot change rendering; **carried, not
    /// honoured**.
    pub use_compatible_text_rendering: bool,
    /// `UseVisualStyleBackColor` — draw the face with visual styles rather than
    /// with `BackColor`. The catalogue exposes no default attribute; Microsoft
    /// documents the default as `true`. Honoured through
    /// [`ButtonBase::face_fill_source`], where an explicit `BackColor` outranks
    /// it (assigning one is what clears this flag in the toolkit).
    pub use_visual_style_back_color: bool,

    /// `Command` — `ICommand` fired on click. **Not yet honoured.**
    pub command: Option<CommandRef>,
    /// `CommandParameter` — payload passed to `Command`. Opaque, like `Tag`.
    pub command_parameter: Option<String>,
}

impl Default for ButtonBase {
    /// The catalogue's declared defaults for the 15 new members, plus the three
    /// re-declared ones which keep `ControlBase`'s defaults.
    fn default() -> Self {
        Self {
            control: ControlBase::default(),
            auto_ellipsis: false,
            flat_style: FlatStyle::Standard,
            flat_appearance: FlatButtonAppearance {
                // WinForms' documented `FlatAppearance.BorderSize` default.
                border_size: 1,
                ..FlatButtonAppearance::default()
            },
            text_align: ContentAlignment::MiddleCenter,
            image_align: ContentAlignment::MiddleCenter,
            text_image_relation: TextImageRelation::Overlay,
            image: None,
            image_index: -1,
            image_key: String::new(),
            image_list: None,
            use_mnemonic: true,
            use_compatible_text_rendering: false,
            use_visual_style_back_color: true,
            command: None,
            command_parameter: None,
        }
    }
}

impl ButtonBase {
    pub fn new() -> Self {
        Self::default()
    }

    /// The font role the control paints with — its own, else `Body`, the way the
    /// toolkit resolves an unset `Font` to the ambient default.
    fn role(&self) -> FontRole {
        self.control.font.unwrap_or(FontRole::Body)
    }

    /// Where the button face's colour comes from, for this control in this
    /// state. Thin wrapper over the pure [`face_fill_source`], which carries the
    /// reasoning and the citations.
    fn face_fill_source(&self, state: ControlState, checked: bool) -> FaceFill {
        face_fill_source(
            self.flat_style,
            self.control.back_color.is_some(),
            FlatFillsSet {
                over:    self.flat_appearance.mouse_over_back_color.is_some(),
                down:    self.flat_appearance.mouse_down_back_color.is_some(),
                checked: self.flat_appearance.checked_back_color.is_some(),
            },
            state,
            checked,
        )
    }

    /// The colour a check/radio GLYPH is composited onto — the control's own
    /// `BackColor` when one is set, else the form face.
    ///
    /// A glyph is not a face: it is a small part dropped onto the control's
    /// ground, and the ground is what its antialiased corners (a radio's whole
    /// circumference) blend into. So an unset `BackColor` resolves the same
    /// ambient way `ControlBase::resolved_fore` does — to the system `Control` —
    /// and a caller who named one gets the part blended into *that*, which is the
    /// difference between a clean glyph and one with a pale halo.
    fn glyph_ground(&self, c: &dyn ControlCanvas) -> D2D1_COLOR_F {
        self.control.back_color.unwrap_or(c.visuals().colors.control)
    }

    /// Paints a button face — the shared drawing behind `Button` and behind a
    /// check/radio whose `Appearance` is `Button`.
    ///
    /// `checked_look` is `None` for a plain `Button`, `Some(true/false)` for a
    /// toggle in `Appearance::Button`: a checked toggle reads as pushed in
    /// (sunken edge over a lighter face), an unchecked one as a resting button.
    ///
    /// The CLASSIC face — what a machine with visual styles off shows, and what
    /// the three `FlatStyle`s other than `System` show everywhere — is three
    /// moves in the toolkit's own order: fill the box with a **system** colour,
    /// put a `DrawEdge` bevel (or a flat ring) round it, then lay the caption
    /// inside. Nothing is rounded and nothing is tinted; the face is
    /// `ControlFace` at rest, at hover and while disabled — only its BORDER and
    /// its caption colour move.
    ///
    /// `FlatStyle::System` with visual styles on takes the first two moves from
    /// the theme instead (see the comment at the top of the body). The caption
    /// and the focus rectangle are the library's either way, exactly as they are
    /// in the toolkit.
    fn paint_face(
        &self,
        c: &dyn ControlCanvas,
        bounds: Rect,
        checked_look: Option<bool>,
        state: ControlState,
    ) {
        let colors = c.visuals().colors;
        let enabled = self.control.enabled;
        let checked = checked_look == Some(true);
        let state = effective_state(enabled, state);
        let px = device_pixel(c);

        // With visual styles on, a `Standard` or `System` button is `uxtheme`'s
        // `BP_PUSHBUTTON` and not the `DrawEdge` bevel below — a themed Windows
        // 11 button is a `#FDFDFD` face inside a `#D0D0D0` frame with a darker
        // `#BABABA` bottom edge, and not one of those three is a `COLOR_*`. See
        // `face_is_themed` for why the two styles share this and why `Flat` and
        // `Popup` do not.
        //
        // Which SOURCE the face colour comes from is decided FIRST, by a pure
        // function, because it also decides whether the theme is asked at all: a
        // `Standard` button with an explicit `BackColor` paints that colour, and
        // a themed part drawn over it would hide it completely.
        //
        // The part is then asked for before the default-button ring and before
        // the fill, because when it answers it replaces both: it draws its own
        // frame, its own face and its own `PBS_DEFAULTED` marking. The ground it
        // is composited onto is `Control` — the form's face, which is what shows
        // through the corners the part rounds off.
        //
        // Everything after this point is shared with the classic path: the focus
        // rectangle and the caption are the library's, not the theme's, exactly
        // as they are in the toolkit.
        let source = self.face_fill_source(state, checked);
        let themed = face_is_themed(self.flat_style, source)
            && c.draw_theme_part(
                theme::class::BUTTON,
                BP_PUSHBUTTON,
                push_button_state(enabled, state, checked),
                bounds,
                colors.control,
            );

        // The form's `AcceptButton` — « Default (accept) » on the reference
        // sheet — wears an extra one-pixel `WindowFrame` rectangle and is drawn
        // one pixel INSIDE it, so its own bevel is complete rather than
        // overdrawn. That ring, not a thicker border, is how the toolkit marks
        // the control Enter activates — in the CLASSIC rendering. A themed
        // button says the same thing with `PBS_DEFAULTED`, so drawing the ring
        // as well would say it twice.
        let bounds = if state.default && !themed {
            c.stroke_rect(&bounds, &colors.window_frame);
            bounds.inflate(-px, -px)
        } else {
            bounds
        };

        // The source was decided above (it is what gates the themed branch); only
        // the mapping from source to colour needs the system palette. Each arm
        // falls back to a system colour when the property is unset, so an unset
        // colour is never painted as black.
        //
        // The two mouse fallbacks are the one place this file approximates: the
        // toolkit computes them as `ControlPaint.Light/Dark(BackColor)`, an HLS
        // rotation of the *face*, and `SystemColors` publishes no such
        // derivation — so the nearest published neighbours stand in.
        let fill = match source {
            FaceFill::MouseDown => {
                self.flat_appearance.mouse_down_back_color.unwrap_or(colors.control_dark)
            }
            FaceFill::MouseOver => {
                self.flat_appearance.mouse_over_back_color.unwrap_or(colors.control_light)
            }
            FaceFill::FlatChecked => {
                self.flat_appearance.checked_back_color.unwrap_or(colors.control_light)
            }
            FaceFill::Explicit => self.control.back_color.unwrap_or(colors.control),
            // The system face. A pushed-in toggle lightens to `ControlLight`
            // (the toolkit dithers `Control` with `ControlLightLight` there, a
            // hatch this canvas cannot express); everything else — resting,
            // hot, pressed, disabled — is plain `Control`, exactly as a classic
            // button is. The sunken bevel below is what shows a press.
            FaceFill::VisualStyle => {
                if checked {
                    colors.control_light
                } else {
                    colors.control
                }
            }
        };
        // The themed part has already put down both the face and the frame, so
        // the classic pair is skipped rather than overdrawn — painting `fill`
        // under it would be invisible, but painting `paint_border` over it would
        // put a `DrawEdge` bevel on top of the theme's own frame.
        //
        // Its interior is TWO device pixels in, not one: the part reserves a
        // one-pixel transparent margin around its border (measured — see the
        // test in `theme.rs`), which is why a themed button's frame sits inside
        // its bounds while a classic bevel sits on them.
        let interior = if themed {
            bounds.inflate(-2.0 * px, -2.0 * px)
        } else {
            c.fill_rect(&bounds, &fill);
            self.paint_border(c, bounds, &colors, state, checked, px)
        };

        // The focus rectangle sits one pixel inside the border. The toolkit
        // draws it as a dotted XOR pattern (`ControlPaint.DrawFocusRectangle`);
        // the canvas offers no dash pattern, so it is a solid `ControlText`
        // hairline — the same rectangle, drawn solid.
        if state.focused {
            c.stroke_rect(&interior.inflate(-px, -px), &colors.control_text);
        }

        // Label, inset by the face padding then by the control's own Padding.
        // A pressed face shifts its caption one pixel down and right, the way a
        // classic button's content sinks with its bevel.
        let mut content = self.inset(bounds, Rect::new(BTN_PAD_X, BTN_PAD_Y, BTN_PAD_X, BTN_PAD_Y));
        if state.pressed {
            content = Rect::new(
                content.left + px,
                content.top + px,
                content.right + px,
                content.bottom + px,
            );
        }
        self.paint_label(c, content, effective_text_align(self.flat_style, self.text_align));
    }

    /// The border, which is where the four `FlatStyle` values genuinely differ.
    /// Returns the **interior** the border leaves behind.
    ///
    /// * `Standard` and `System` are the classic bevel: `Border3DStyle::Raised`
    ///   at rest, `Sunken` while pressed (or while a toggle is checked).
    ///   `System` differs from `Standard` only in what it IGNORES — `BackColor`,
    ///   `Image`, `TextAlign` — which is settled before this point, so the two
    ///   share one arm rather than one being a copy of the other.
    /// * `Flat` carries its own `FlatAppearance` border: `BorderSize` rings of
    ///   `BorderColor`, and a `BorderSize` of **0** means no border at all.
    /// * `Popup` is the member that exists FOR this distinction — the
    ///   `FlatStyle` Remarks: « The Popup style control initially appears Flat
    ///   until the mouse pointer moves over it. When the mouse pointer moves
    ///   over the Popup control, it appears as a Standard style control until
    ///   the mouse pointer is moved off of it again. » So it is one flat
    ///   `ControlDark` ring at rest, and a single-ring « lite » bevel — raised
    ///   under the pointer, sunken under the press — once the mouse arrives.
    fn paint_border(
        &self,
        c: &dyn ControlCanvas,
        bounds: Rect,
        colors: &SystemColors,
        state: ControlState,
        checked: bool,
        px: f32,
    ) -> Rect {
        let pushed = state.pressed || checked;
        match self.flat_style {
            FlatStyle::Standard | FlatStyle::System => {
                let style =
                    if pushed { Border3DStyle::Sunken } else { Border3DStyle::Raised };
                c.draw_edge(&bounds, style, Border3DSide::ALL)
            }
            FlatStyle::Flat => {
                // `BorderColor` unset falls back to the control's FOREGROUND,
                // not to a shadow grey: the reference sheet's `Flat` button is
                // outlined in pure black, which is `ControlText` here —
                // `ControlDark` (#A0A0A0) and `WindowFrame` (#646464) are both
                // visibly lighter than what the toolkit actually drew.
                let color = self
                    .flat_appearance
                    .border_color
                    .or(self.control.fore_color)
                    .unwrap_or(colors.control_text);
                let mut r = bounds;
                // `BorderSize` counts PIXELS, so each ring is one device pixel
                // however the display is scaled.
                for _ in 0..self.flat_appearance.border_size.max(0) {
                    c.stroke_rect(&r, &color);
                    r = r.inflate(-px, -px);
                }
                r
            }
            FlatStyle::Popup => {
                if pushed {
                    c.draw_edge(&bounds, Border3DStyle::SunkenOuter, Border3DSide::ALL)
                } else if state.hot {
                    c.draw_edge(&bounds, Border3DStyle::RaisedInner, Border3DSide::ALL)
                } else {
                    c.stroke_rect(&bounds, &colors.control_dark);
                    bounds.inflate(-px, -px)
                }
            }
        }
    }

    /// Draws the control's `Text` inside `content`, honouring `align`
    /// (`ContentAlignment` → horizontal DirectWrite alignment + vertical band)
    /// and `AutoEllipsis`. The face is the system UI font; the ink is the
    /// control's `ForeColor`, else `ControlText`, and `GrayText` when disabled —
    /// the one colour the toolkit greys a dead caption with.
    fn paint_label(&self, c: &dyn ControlCanvas, content: Rect, align: ContentAlignment) {
        if self.control.text.is_empty() {
            return;
        }
        let role = self.role();
        let fmt = message_format(&c.visuals().fonts, role);
        let color = if self.control.enabled {
            self.control.fore_color.unwrap_or(c.visuals().colors.control_text)
        } else {
            c.visuals().colors.gray_text
        };
        let (fh, fv) = align.fractions();
        let band = text_band(content, line_height(role), fv);

        if self.auto_ellipsis {
            // The canvas only offers leading- or centre-ellipsis; a trailing
            // alignment therefore falls back to centre-ellipsis (documented
            // limitation of the shared surface).
            if fh >= 1.0 || (fh - 0.5).abs() < f32::EPSILON {
                c.text_ellipsis_center(&self.control.text, &band, fmt, &color);
            } else {
                c.text_ellipsis(&self.control.text, &band, fmt, &color);
            }
        } else {
            c.text_aligned(&self.control.text, &band, fmt, &color, h_alignment(fh));
        }
    }

    /// Deflates `r` by a four-sided inset, never inverting it.
    fn inset(&self, r: Rect, by: Rect) -> Rect {
        Rect::new(
            r.left + by.left,
            r.top + by.top,
            (r.right - by.right).max(r.left + by.left),
            (r.bottom - by.bottom).max(r.top + by.top),
        )
    }
}

impl std::ops::Deref for ButtonBase {
    type Target = ControlBase;
    fn deref(&self) -> &ControlBase {
        &self.control
    }
}
impl std::ops::DerefMut for ButtonBase {
    fn deref_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Button — declares AutoSizeMode (a re-declaration of ControlBase) + DialogResult.
// ═════════════════════════════════════════════════════════════════════════════

/// A plain command button.
///
/// `AutoSizeMode` is declared by `Button` in .NET but is the same property as
/// `ControlBase::auto_size_mode`; it is reached through `Deref`, not duplicated.
/// The one genuinely new member is `DialogResult`.
#[derive(Clone)]
pub struct Button {
    base: ButtonBase,
    /// `DialogResult` — what this button reports when it closes a modal form.
    pub dialog_result: DialogResult,
}

impl Default for Button {
    fn default() -> Self {
        Self { base: ButtonBase::default(), dialog_result: DialogResult::None }
    }
}

impl Button {
    pub fn new() -> Self {
        Self::default()
    }
}

impl std::ops::Deref for Button {
    type Target = ButtonBase;
    fn deref(&self) -> &ButtonBase {
        &self.base
    }
}
impl std::ops::DerefMut for Button {
    fn deref_mut(&mut self) -> &mut ButtonBase {
        &mut self.base
    }
}

impl Control for Button {
    fn control(&self) -> &ControlBase {
        &self.base.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base.control
    }

    /// `GetPreferredSize`: the label plus the face padding plus the control's
    /// own `Padding`, floored at the toolkit's default button height, then
    /// clamped by `MinimumSize`/`MaximumSize`. `AutoSize` is the caller's
    /// concern — this returns the size the button *wants*; the layout applies it
    /// only when `AutoSize` is on.
    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        let role = self.base.role();
        let fmt = format_for(c.formats(), role);
        let text_w = c.measure(&self.base.control.text, fmt);
        let size = button_preferred(text_w, self.base.control.padding, line_height(role));
        self.base.control.clamp(size)
    }

    /// Resting paint DELEGATES to the state-aware one, never the reverse: the
    /// trait's provided `paint_with_state` calls `paint`, so a family that
    /// overrode `paint_with_state` and then had it call `paint` would recurse
    /// forever. The real drawing lives in `paint_with_state`.
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        self.paint_with_state(c, bounds, ControlState::default());
    }

    fn paint_with_state(&self, c: &dyn ControlCanvas, bounds: Rect, state: ControlState) {
        self.base.paint_face(c, bounds, None, state);
    }

    fn type_name(&self) -> &'static str {
        "Button"
    }
}

/// The content arithmetic of a button's preferred size, factored out so it can be
/// tested without a canvas: `label + 2·face-pad + own padding`, height floored at
/// [`MIN_BTN_HEIGHT`].
fn button_preferred(text_w: f32, padding: Padding, lh: f32) -> Size {
    let w = text_w + 2.0 * BTN_PAD_X + padding.horizontal();
    let h = (lh + 2.0 * BTN_PAD_Y + padding.vertical()).max(MIN_BTN_HEIGHT);
    Size::new(w, h)
}

// ═════════════════════════════════════════════════════════════════════════════
// CheckBox — 7 declared: Appearance, AutoCheck, CheckAlign, CheckState, Checked,
// TextAlign (re-declared, default MiddleLeft), ThreeState.
// ═════════════════════════════════════════════════════════════════════════════

/// A two- or three-state check box.
///
/// `Checked` is **not** a field: in the toolkit it is a projection of
/// `CheckState` (`Checked == CheckState != Unchecked`), and setting it snaps the
/// state to `Checked`/`Unchecked`. Keeping one source of truth avoids the two
/// disagreeing.
#[derive(Clone)]
pub struct CheckBox {
    base: ButtonBase,
    /// `Appearance` — a glyph (`Normal`) or a toggle button (`Button`).
    pub appearance: Appearance,
    /// `AutoCheck` — advance `CheckState` automatically on click.
    pub auto_check: bool,
    /// `CheckAlign` — where the glyph sits in the box.
    pub check_align: ContentAlignment,
    /// `CheckState` — the source of truth for the check.
    pub check_state: CheckState,
    /// `ThreeState` — allow `Indeterminate` in the click cycle.
    pub three_state: bool,
}

impl Default for CheckBox {
    fn default() -> Self {
        // CheckBox re-declares TextAlign to flip its default to MiddleLeft — a
        // label reads left-to-right from the box, not centred over it.
        let base = ButtonBase { text_align: ContentAlignment::MiddleLeft, ..ButtonBase::default() };
        Self {
            base,
            appearance: Appearance::Normal,
            auto_check: true,
            check_align: ContentAlignment::MiddleLeft,
            check_state: CheckState::Unchecked,
            three_state: false,
        }
    }
}

impl CheckBox {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Checked` getter — true unless `Unchecked`, matching the toolkit
    /// (`Indeterminate` reports as checked).
    pub fn checked(&self) -> bool {
        self.check_state != CheckState::Unchecked
    }

    /// `Checked` setter — snaps `CheckState` to the two-state value.
    pub fn set_checked(&mut self, checked: bool) {
        self.check_state = if checked { CheckState::Checked } else { CheckState::Unchecked };
    }

    /// Simulates a click: when `AutoCheck` is on, advance `CheckState` by the
    /// same cycle the toolkit uses. Owns no event loop — the host calls this.
    pub fn perform_click(&mut self) {
        if self.auto_check {
            self.check_state = next_check_state(self.check_state, self.three_state);
        }
    }
}

/// The `CheckState` cycle a click walks. Two-state: `Unchecked → Checked →
/// Unchecked`. Three-state, the toolkit's order: `Unchecked → Checked →
/// Indeterminate → Unchecked`. Pure, so the state machine is tested directly.
fn next_check_state(state: CheckState, three_state: bool) -> CheckState {
    match state {
        CheckState::Unchecked => CheckState::Checked,
        CheckState::Checked => {
            if three_state {
                CheckState::Indeterminate
            } else {
                CheckState::Unchecked
            }
        }
        CheckState::Indeterminate => CheckState::Unchecked,
    }
}

impl std::ops::Deref for CheckBox {
    type Target = ButtonBase;
    fn deref(&self) -> &ButtonBase {
        &self.base
    }
}
impl std::ops::DerefMut for CheckBox {
    fn deref_mut(&mut self) -> &mut ButtonBase {
        &mut self.base
    }
}

impl Control for CheckBox {
    fn control(&self) -> &ControlBase {
        &self.base.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base.control
    }

    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        let role = self.base.role();
        let fmt = format_for(c.formats(), role);
        let text_w = c.measure(&self.base.control.text, fmt);
        let size = match self.appearance {
            // A toggle button measures like a plain button.
            Appearance::Button => button_preferred(text_w, self.base.control.padding, line_height(role)),
            // A glyph control measures glyph + gap + label.
            Appearance::Normal => glyph_preferred(text_w, self.base.control.padding, line_height(role)),
        };
        self.base.control.clamp(size)
    }

    /// See `Button::paint` — `paint` delegates here, never the reverse.
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        self.paint_with_state(c, bounds, ControlState::default());
    }

    fn paint_with_state(&self, c: &dyn ControlCanvas, bounds: Rect, state: ControlState) {
        match self.appearance {
            Appearance::Button => {
                // A toggle button whose checked look tracks `Checked`.
                self.base.paint_face(c, bounds, Some(self.checked()), state);
            }
            Appearance::Normal => {
                let content = self.base.inset(bounds, Rect::new(0.0, 0.0, 0.0, 0.0));
                // `System` re-aligns the glyph but leaves `TextAlign` alone —
                // see `system_check_align` for the toolkit's exact wording.
                let align =
                    system_check_align(self.base.flat_style, self.check_align, self.base.text_align);
                let (glyph, label) = split_glyph(content, align);
                let enabled = self.base.control.enabled;
                let state = effective_state(enabled, state);
                paint_check_glyph(
                    c,
                    glyph,
                    self.check_state,
                    self.base.flat_style,
                    enabled,
                    state,
                    self.base.glyph_ground(c),
                );
                self.base.paint_label(c, label, self.base.text_align);
            }
        }
    }

    fn type_name(&self) -> &'static str {
        "CheckBox"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// RadioButton — 6 declared: Appearance, AutoCheck, CheckAlign, Checked, TabStop
// (re-declared, default false), TextAlign (re-declared, default MiddleLeft).
// ═════════════════════════════════════════════════════════════════════════════

/// A mutually-exclusive option button.
///
/// Unlike `CheckBox`, `RadioButton` has no `CheckState`/`ThreeState`: it is a
/// plain two-state `Checked`. Selecting one clears its siblings — a container
/// rule the port models with [`select_radio`], since a control holds no parent
/// pointer.
#[derive(Clone)]
pub struct RadioButton {
    base: ButtonBase,
    /// `Appearance` — a glyph (`Normal`) or a toggle button (`Button`).
    pub appearance: Appearance,
    /// `AutoCheck` — check automatically on click (radios never self-uncheck).
    pub auto_check: bool,
    /// `CheckAlign` — where the dot sits in the box.
    pub check_align: ContentAlignment,
    /// `Checked` — whether this option is selected.
    pub checked: bool,
}

impl Default for RadioButton {
    fn default() -> Self {
        let mut base = ButtonBase { text_align: ContentAlignment::MiddleLeft, ..ButtonBase::default() };
        // RadioButton re-declares TabStop with a `false` default: only the
        // *checked* member of a group is a tab stop, so a fresh (unchecked)
        // radio starts off the tab order. Reached through Deref onto ControlBase.
        base.control.tab_stop = false;
        Self {
            base,
            appearance: Appearance::Normal,
            auto_check: true,
            check_align: ContentAlignment::MiddleLeft,
            checked: false,
        }
    }
}

impl RadioButton {
    pub fn new() -> Self {
        Self::default()
    }

    /// Simulates a click: when `AutoCheck` is on, select this button. A radio
    /// does not toggle off on a second click — clearing happens by another
    /// sibling being chosen (see [`select_radio`]).
    pub fn perform_click(&mut self) {
        if self.auto_check {
            self.checked = true;
        }
    }
}

/// Models the container rule: checking one radio clears every sibling. Selecting
/// out of range clears the whole group (what the toolkit does when nothing is
/// checked). The clear applies regardless of a sibling's `AutoCheck`, exactly as
/// WinForms' `RadioButton.Checked` setter does.
pub fn select_radio(group: &mut [RadioButton], selected: usize) {
    for (i, radio) in group.iter_mut().enumerate() {
        radio.checked = i == selected;
    }
}

impl std::ops::Deref for RadioButton {
    type Target = ButtonBase;
    fn deref(&self) -> &ButtonBase {
        &self.base
    }
}
impl std::ops::DerefMut for RadioButton {
    fn deref_mut(&mut self) -> &mut ButtonBase {
        &mut self.base
    }
}

impl Control for RadioButton {
    fn control(&self) -> &ControlBase {
        &self.base.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base.control
    }

    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        let role = self.base.role();
        let fmt = format_for(c.formats(), role);
        let text_w = c.measure(&self.base.control.text, fmt);
        let size = match self.appearance {
            Appearance::Button => button_preferred(text_w, self.base.control.padding, line_height(role)),
            Appearance::Normal => glyph_preferred(text_w, self.base.control.padding, line_height(role)),
        };
        self.base.control.clamp(size)
    }

    /// See `Button::paint` — `paint` delegates here, never the reverse.
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        self.paint_with_state(c, bounds, ControlState::default());
    }

    fn paint_with_state(&self, c: &dyn ControlCanvas, bounds: Rect, state: ControlState) {
        match self.appearance {
            Appearance::Button => {
                self.base.paint_face(c, bounds, Some(self.checked), state);
            }
            Appearance::Normal => {
                let content = self.base.inset(bounds, Rect::new(0.0, 0.0, 0.0, 0.0));
                let align =
                    system_check_align(self.base.flat_style, self.check_align, self.base.text_align);
                let (glyph, label) = split_glyph(content, align);
                let enabled = self.base.control.enabled;
                let state = effective_state(enabled, state);
                paint_radio_glyph(
                    c,
                    glyph,
                    self.checked,
                    self.base.flat_style,
                    enabled,
                    state,
                    self.base.glyph_ground(c),
                );
                self.base.paint_label(c, label, self.base.text_align);
            }
        }
    }

    fn type_name(&self) -> &'static str {
        "RadioButton"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Glyph geometry and painting, shared by CheckBox and RadioButton.
// ─────────────────────────────────────────────────────────────────────────────

/// The content arithmetic of a glyph control's preferred size: `glyph + gap +
/// label + own padding`, height at least a glyph tall. Pure, for testing.
fn glyph_preferred(text_w: f32, padding: Padding, lh: f32) -> Size {
    let w = GLYPH + GLYPH_TEXT_GAP + text_w + padding.horizontal();
    let h = GLYPH.max(lh) + padding.vertical();
    Size::new(w, h)
}

/// Splits `content` into the glyph box and the remaining label rect, placing the
/// glyph by `align`. `CheckAlign`'s horizontal fraction decides which side the
/// glyph hugs (left is the common case); the vertical fraction places it in the
/// band. A centred horizontal alignment is treated as left, since a glyph
/// floating in the middle of the label is not a layout the toolkit offers by
/// default.
fn split_glyph(content: Rect, align: ContentAlignment) -> (Rect, Rect) {
    let (fh, fv) = align.fractions();
    let slack_v = (content.bottom - content.top - GLYPH).max(0.0);
    let gy = content.top + slack_v * fv;

    if fh >= 1.0 {
        // Glyph on the right, label to its left.
        let gx = content.right - GLYPH;
        let glyph = Rect::new(gx, gy, gx + GLYPH, gy + GLYPH);
        let label = Rect::new(content.left, content.top, gx - GLYPH_TEXT_GAP, content.bottom);
        (glyph, label)
    } else {
        // Glyph on the left, label to its right (the default).
        let gx = content.left;
        let glyph = Rect::new(gx, gy, gx + GLYPH, gy + GLYPH);
        let label = Rect::new(gx + GLYPH + GLYPH_TEXT_GAP, content.top, content.right, content.bottom);
        (glyph, label)
    }
}

/// The 3-D edge a check box's WELL is drawn with, for this `FlatStyle` in this
/// state.
///
/// `Standard` and `System` are the classic sunken well — two rings, dark then
/// darker on the top and left, so the box reads as a hole punched in the face.
/// `Flat` has no bevel at all (one `ControlDark` ring), and `Popup` is flat
/// until the pointer arrives, when it lifts into the single-ring « lite » bevel
/// — the same rule the family's buttons follow, which is the whole point of the
/// two styles existing.
fn glyph_edge(flat_style: FlatStyle, state: ControlState) -> Border3DStyle {
    match flat_style {
        FlatStyle::Flat => Border3DStyle::Flat,
        FlatStyle::Popup if state.pressed => Border3DStyle::SunkenOuter,
        FlatStyle::Popup if state.hot => Border3DStyle::RaisedInner,
        FlatStyle::Popup => Border3DStyle::Flat,
        FlatStyle::Standard | FlatStyle::System => Border3DStyle::Sunken,
    }
}

/// Paints a check box glyph in each of its three states.
///
/// **Themed first.** With visual styles on, a `Standard` or `System` check box is
/// `uxtheme`'s `BP_CHECKBOX`, and the part draws the box, its fill AND its mark
/// in one call — so the whole classic body below is the fallback, not a
/// decoration over it. The reference sheet's unticked box is `#626262` around
/// `#F3F3F3` and its ticked one is a solid accent `#005FB8`; the classic well is
/// a `DrawEdge` bevel around `COLOR_WINDOW` white, and neither of those greys is
/// any `GetSysColor` index.
///
/// `ground` is the colour the glyph is composited onto — the control's own
/// background — because the part antialiases its corners against it. Passing the
/// form face where a caller has named a `BackColor` would show as a pale fringe
/// on the four corners, not as a missing glyph.
///
/// The CLASSIC box is a **3-D well**: `DrawEdge` around a `Window`-coloured
/// field, which is what makes a classic check box read as recessed rather than as
/// a stroked square. The field drops to `Control` when the control is disabled or
/// the pointer is holding it down — the toolkit's own two exceptions to « a check
/// box is a white hole ».
///
/// The classic tick and indeterminate bar stay **vector geometries**
/// (`Canvas::vector_icon`), not text: a « ✓ » drawn through a text format is at
/// the mercy of whichever family resolves it — its optical size and baseline
/// would differ from the toolkit's, and it would not scale cleanly with the DPI
/// factor. `vector_icon` scales the path from its own view box, so the mark is
/// identical at every scale.
fn paint_check_glyph(
    c: &dyn ControlCanvas,
    box_rect: Rect,
    check: CheckState,
    flat_style: FlatStyle,
    enabled: bool,
    state: ControlState,
    ground: D2D1_COLOR_F,
) {
    if glyph_is_themed(flat_style)
        && c.draw_theme_part(
            theme::class::BUTTON,
            BP_CHECKBOX,
            check_box_state(enabled, state, check),
            box_rect,
            ground,
        )
    {
        return;
    }

    let colors = c.visuals().colors;
    let interior = c.draw_edge(&box_rect, glyph_edge(flat_style, state), Border3DSide::ALL);
    // A live field is `Window`; a dead one, or one held down, is the face —
    // the toolkit's way of saying « this hole is not accepting a click ».
    let field = if enabled && !state.pressed { colors.window } else { colors.control };
    c.fill_rect(&interior, &field);

    if check == CheckState::Unchecked {
        return;
    }
    // The tick lives on a `Window`-coloured field, so it takes `WindowText`.
    // `Indeterminate` is the toolkit's greyed third state: the mark is drawn in
    // the shadow grey over the face colour rather than in ink over white.
    let (name, mark) = match check {
        CheckState::Indeterminate => ("Minus", colors.control_dark),
        _ => ("Check", if enabled { colors.window_text } else { colors.gray_text }),
    };
    c.vector_icon(name, &interior, MARK, &mark);
}

/// Paints a radio button glyph: a recessed ring with an ink dot when checked.
///
/// **Themed first**, on the same two `FlatStyle`s as the check box and for the
/// same measured reason — `BP_RADIOBUTTON` is `#626262` around `#F3F3F3`
/// unselected, a solid accent `#005FB8` ring around `#FFFFFF` selected, and
/// `#C3C3C3` around `#F9F9F9` disabled, all three of which the reference sheet
/// holds byte for byte. The part is a genuine circle with genuine antialiasing,
/// which is exactly what the classic path below cannot draw — so `ground` (the
/// control's own background) matters more here than anywhere else in the file:
/// it is what the four corners the circle does not cover are filled with.
///
/// The CLASSIC glyph is the one shape in the family that is **not** square — a
/// WinForms radio button is a circle, and squaring it to match the rest of the
/// repaint would be a different control. The canvas has no ellipse or arc
/// primitive, so the circle is a rounded rectangle whose radius is half its side,
/// and the two-tone bevel a real radio has (shadow on the upper-left arc,
/// highlight on the lower-right) collapses to one `ControlDark` ring: `draw_edge`
/// is square-only, and four quarter-arcs in two colours is not something this
/// surface can express.
fn paint_radio_glyph(
    c: &dyn ControlCanvas,
    box_rect: Rect,
    checked: bool,
    flat_style: FlatStyle,
    enabled: bool,
    state: ControlState,
    ground: D2D1_COLOR_F,
) {
    if glyph_is_themed(flat_style)
        && c.draw_theme_part(
            theme::class::BUTTON,
            BP_RADIOBUTTON,
            radio_button_state(enabled, state, checked),
            box_rect,
            ground,
        )
    {
        return;
    }

    let colors = c.visuals().colors;
    let r = GLYPH / 2.0;
    let field = if enabled && !state.pressed { colors.window } else { colors.control };
    c.fill_rounded(&box_rect, r, &field);
    c.stroke_rounded(&box_rect, r, &colors.control_dark);
    if checked {
        // Inner dot, inset by ~3.5 DIP all round — the classic 5×5 mark inside a
        // 13×13 glyph.
        let dot = Rect::new(
            box_rect.left + 3.5,
            box_rect.top + 3.5,
            box_rect.right - 3.5,
            box_rect.bottom - 3.5,
        );
        let ink = if enabled { colors.window_text } else { colors.gray_text };
        c.fill_rounded(&dot, (dot.right - dot.left) / 2.0, &ink);
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests — defaults against the catalogue, measurement arithmetic, the CheckState
// state machine, RadioButton exclusion, and the re-declaration ordering traps.
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    // Tests deliberately build a control then mutate one field to set up a
    // scenario; that is clearer here than a full struct literal per case.
    #![allow(clippy::field_reassign_with_default)]

    use super::*;

    // ── Declared defaults, straight from the reflection catalogue. ──────────

    #[test]
    fn buttonbase_defaults_match_the_catalogue() {
        let b = ButtonBase::default();
        assert!(!b.auto_ellipsis);
        assert_eq!(b.flat_style, FlatStyle::Standard);
        assert_eq!(b.flat_appearance.border_size, 1);
        assert_eq!(b.text_align, ContentAlignment::MiddleCenter);
        assert_eq!(b.image_align, ContentAlignment::MiddleCenter);
        assert_eq!(b.image_index, -1);
        assert!(b.image_key.is_empty());
        assert_eq!(b.text_image_relation, TextImageRelation::Overlay);
        assert!(b.use_mnemonic);
        assert!(!b.use_compatible_text_rendering);
        assert!(b.use_visual_style_back_color);
        assert!(b.image.is_none() && b.image_list.is_none() && b.command.is_none());
        // Re-declared members keep ControlBase's defaults, reached through Deref.
        assert!(!b.auto_size);
        assert!(b.text.is_empty());
    }

    #[test]
    fn button_defaults_match_the_catalogue() {
        let b = Button::default();
        assert_eq!(b.dialog_result, DialogResult::None);
        // AutoSizeMode is ControlBase's, reached through Deref.
        assert_eq!(b.auto_size_mode, AutoSizeMode::GrowOnly);
        // Inherited ButtonBase default is untouched.
        assert_eq!(b.text_align, ContentAlignment::MiddleCenter);
    }

    #[test]
    fn checkbox_defaults_match_the_catalogue() {
        let b = CheckBox::default();
        assert_eq!(b.appearance, Appearance::Normal);
        assert!(b.auto_check);
        assert_eq!(b.check_align, ContentAlignment::MiddleLeft);
        assert_eq!(b.check_state, CheckState::Unchecked);
        assert!(!b.checked());
        assert!(!b.three_state);
        // The re-declaration trap: TextAlign default flips to MiddleLeft.
        assert_eq!(b.text_align, ContentAlignment::MiddleLeft);
    }

    #[test]
    fn radiobutton_defaults_match_the_catalogue() {
        let b = RadioButton::default();
        assert_eq!(b.appearance, Appearance::Normal);
        assert!(b.auto_check);
        assert_eq!(b.check_align, ContentAlignment::MiddleLeft);
        assert!(!b.checked);
        assert_eq!(b.text_align, ContentAlignment::MiddleLeft);
        // The two re-declaration traps: TextAlign left, and TabStop off.
        assert!(!b.tab_stop, "a fresh radio must start off the tab order");
    }

    // ── Measurement arithmetic (canvas-free, via the pure helpers). ─────────

    #[test]
    fn a_button_measures_label_plus_padding_floored() {
        // Short label and a small line: the height floor wins (10 + 2·3 = 16 < 23).
        let s = button_preferred(20.0, Padding::ZERO, 10.0);
        assert_eq!(s.width, 20.0 + 2.0 * BTN_PAD_X);
        assert_eq!(s.height, MIN_BTN_HEIGHT);
        // Own Padding widens and heightens; a tall line clears the floor.
        let s = button_preferred(100.0, Padding::all(10.0), 40.0);
        assert_eq!(s.width, 100.0 + 2.0 * BTN_PAD_X + 20.0);
        assert_eq!(s.height, 40.0 + 2.0 * BTN_PAD_Y + 20.0);
    }

    #[test]
    fn a_glyph_control_measures_glyph_gap_label() {
        let s = glyph_preferred(50.0, Padding::ZERO, 20.0);
        assert_eq!(s.width, GLYPH + GLYPH_TEXT_GAP + 50.0);
        // Height is the taller of glyph and line, here the 20 DIP line.
        assert_eq!(s.height, 20.0);
        // A caption shorter than the glyph is floored at the glyph height.
        let s = glyph_preferred(50.0, Padding::ZERO, 10.0);
        assert_eq!(s.height, GLYPH);
    }

    #[test]
    fn preferred_size_is_clamped_by_minimum_size() {
        // The clamp path is ControlBase's; assert the arithmetic feeds into it
        // by pushing a minimum wider than the content.
        let mut b = Button::default();
        b.minimum_size = Size::new(500.0, 0.0);
        // No canvas here, so exercise the clamp directly on a known content size.
        let content = button_preferred(10.0, Padding::ZERO, 20.0);
        assert!(b.control().clamp(content).width >= 500.0);
    }

    // ── BackColor / UseVisualStyleBackColor precedence. ─────────────────────

    const RED: D2D1_COLOR_F = D2D1_COLOR_F { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };

    fn hot() -> ControlState {
        ControlState { hot: true, ..ControlState::default() }
    }
    fn down() -> ControlState {
        ControlState { hot: true, pressed: true, ..ControlState::default() }
    }

    /// The ordering trap: an explicit `BackColor` must outrank
    /// `UseVisualStyleBackColor`, because assigning one is what clears the other
    /// in the toolkit. Checking the flag first would make the colour invisible.
    #[test]
    fn an_explicit_back_color_outranks_use_visual_style_back_color() {
        let mut b = Button::default();
        assert!(b.use_visual_style_back_color, "the flag is on by default");
        let rest = ControlState::default();
        // Unset BackColor: the visual-style face wins.
        assert_eq!(b.base.face_fill_source(rest, false), FaceFill::VisualStyle);
        // Naming a colour wins, even with the flag still on.
        b.back_color = Some(RED);
        assert_eq!(b.base.face_fill_source(rest, false), FaceFill::Explicit);
    }

    /// An unset `BackColor` is AMBIENT, not a literal colour, so clearing the
    /// flag without naming a colour still lands on the theme face — the same
    /// rule `ControlBase::resolved_fore` uses.
    #[test]
    fn an_unset_back_color_stays_ambient_whatever_the_flag() {
        let mut b = Button::default();
        b.use_visual_style_back_color = false;
        assert!(b.back_color.is_none());
        assert_eq!(
            b.base.face_fill_source(ControlState::default(), false),
            FaceFill::VisualStyle
        );
    }

    /// The trap the docs settle: `CheckedBackColor` is defined as the face
    /// « when the button is checked AND the mouse pointer is outside the bounds
    /// of the control », so the two mouse states rank ABOVE it — and above an
    /// explicit `BackColor` too, since `BackColor` is only the RESTING face.
    /// Ranking `BackColor` first would mean naming a face colour permanently
    /// disables hover feedback.
    #[test]
    fn the_mouse_states_outrank_both_checked_and_an_explicit_back_color() {
        let mut b = Button::default();
        b.flat_style = FlatStyle::Flat;
        b.back_color = Some(RED);
        b.flat_appearance.mouse_over_back_color = Some(RED);
        b.flat_appearance.mouse_down_back_color = Some(RED);
        b.flat_appearance.checked_back_color = Some(RED);

        let rest = ControlState::default();
        assert_eq!(b.base.face_fill_source(rest, false), FaceFill::Explicit, "resting");
        assert_eq!(b.base.face_fill_source(rest, true), FaceFill::FlatChecked, "checked, not hot");
        assert_eq!(b.base.face_fill_source(hot(), true), FaceFill::MouseOver, "hot beats checked");
        assert_eq!(b.base.face_fill_source(hot(), false), FaceFill::MouseOver, "hot beats BackColor");
        assert_eq!(b.base.face_fill_source(down(), false), FaceFill::MouseDown, "pressed beats hot");
    }

    /// Every `FlatAppearance` fill is documented as applying to buttons whose
    /// `FlatStyle` is `Flat`. Under any other style they must be inert, not
    /// quietly substituted.
    #[test]
    fn flat_appearance_fills_apply_under_flat_style_flat_only() {
        let mut b = Button::default();
        b.flat_appearance.mouse_over_back_color = Some(RED);
        b.flat_appearance.checked_back_color = Some(RED);
        for style in [FlatStyle::Standard, FlatStyle::Popup] {
            b.flat_style = style;
            assert_eq!(
                b.base.face_fill_source(hot(), true),
                FaceFill::VisualStyle,
                "{style:?} must ignore FlatAppearance"
            );
        }
        b.flat_style = FlatStyle::Flat;
        assert_eq!(b.base.face_fill_source(hot(), true), FaceFill::MouseOver);
    }

    /// A fill that is not SET never wins its slot — otherwise an unset colour
    /// would be painted as a literal black.
    #[test]
    fn an_unset_flat_fill_falls_through_to_the_next_source() {
        let mut b = Button::default();
        b.flat_style = FlatStyle::Flat;
        b.back_color = Some(RED);
        // Hot, but MouseOverBackColor was never set: fall through to BackColor.
        assert_eq!(b.base.face_fill_source(hot(), false), FaceFill::Explicit);
        // Pressed, only MouseOver set: fall through to MouseOver, not MouseDown.
        b.flat_appearance.mouse_over_back_color = Some(RED);
        assert_eq!(b.base.face_fill_source(down(), false), FaceFill::MouseOver);
    }

    /// `FlatStyle::System` hands the face to the OS: the Remarks say
    /// `Control.BackColor` « will be ignored for button controls ». It must
    /// short-circuit before every other source.
    #[test]
    fn flat_style_system_ignores_back_color_entirely() {
        let mut b = Button::default();
        b.flat_style = FlatStyle::System;
        b.back_color = Some(RED);
        b.flat_appearance.mouse_over_back_color = Some(RED);
        assert_eq!(b.base.face_fill_source(ControlState::default(), false), FaceFill::VisualStyle);
        assert_eq!(b.base.face_fill_source(hot(), true), FaceFill::VisualStyle);
    }

    /// The themed counterpart of the face precedence: which `BP_PUSHBUTTON`
    /// state a button is in. Every ordering rule in [`push_button_state`] gets
    /// its own assertion, because each one is a case that would otherwise be
    /// unreachable rather than merely wrong.
    #[test]
    fn the_push_button_state_follows_the_themes_own_precedence() {
        let rest = ControlState::default();
        assert_eq!(push_button_state(true, rest, false), PBS_NORMAL);
        assert_eq!(push_button_state(true, hot(), false), PBS_HOT);
        // The pointer is necessarily OVER a control it is pressing, so a press
        // is unreachable if hot is tested first.
        assert_eq!(push_button_state(true, down(), false), PBS_PRESSED);
        // A checked toggle in `Appearance::Button` reads as pushed, the same
        // equivalence the classic bevel makes.
        assert_eq!(push_button_state(true, rest, true), PBS_PRESSED);
        // Disabled outranks everything: a dead control cannot be hot, and its
        // themed face is a distinct rendering rather than a greyed caption.
        assert_eq!(push_button_state(false, down(), true), PBS_DISABLED);
        // The form's `AcceptButton` at rest.
        let default_state = ControlState { default: true, ..ControlState::default() };
        assert_eq!(push_button_state(true, default_state, false), PBS_DEFAULTED);
    }

    // ── The FlatStyle::System alignment coercions. ──────────────────────────

    #[test]
    fn flat_style_system_ignores_text_align_on_a_button_only() {
        // Ignored under System...
        assert_eq!(
            effective_text_align(FlatStyle::System, ContentAlignment::BottomRight),
            ContentAlignment::MiddleCenter
        );
        // ...honoured under every other style.
        for style in [FlatStyle::Flat, FlatStyle::Popup, FlatStyle::Standard] {
            assert_eq!(
                effective_text_align(style, ContentAlignment::BottomRight),
                ContentAlignment::BottomRight,
                "{style:?}"
            );
        }
    }

    /// The Remarks' Note: under `System` the check box aligns to the left or
    /// right edge only (« a left or center alignment appears left aligned, right
    /// remains unchanged ») and vertically follows the TEXT, not `CheckAlign`.
    #[test]
    fn flat_style_system_coerces_check_align_but_takes_its_vertical_from_the_text() {
        use ContentAlignment as A;
        // Centre reads as left; the vertical comes from TextAlign (TopRight → top).
        assert_eq!(
            system_check_align(FlatStyle::System, A::MiddleCenter, A::TopRight),
            A::TopLeft
        );
        // Right stays right.
        assert_eq!(
            system_check_align(FlatStyle::System, A::MiddleRight, A::TopRight),
            A::TopRight
        );
        // Untouched under any other style — the property value never changes.
        assert_eq!(
            system_check_align(FlatStyle::Standard, A::MiddleCenter, A::TopRight),
            A::MiddleCenter
        );
    }

    /// A disabled control cannot be hot, pressed or focused — the toolkit does
    /// not route mouse messages to it. Without this, a disabled button would
    /// still light up under the cursor.
    #[test]
    fn a_disabled_control_ignores_every_pointer_state() {
        let busy = ControlState { hot: true, pressed: true, focused: true, default: true };
        assert_eq!(effective_state(true, busy), busy, "enabled keeps its state");
        assert_eq!(
            effective_state(false, busy),
            ControlState::default(),
            "disabled drops hot/pressed/focused"
        );
    }

    /// A disabled `Flat` button must not reach `MouseOverBackColor` either — the
    /// neutralised state has to be what feeds the precedence, not the raw one.
    #[test]
    fn a_disabled_flat_button_never_reaches_its_mouse_over_colour() {
        let mut b = Button::default();
        b.flat_style = FlatStyle::Flat;
        b.flat_appearance.mouse_over_back_color = Some(RED);
        b.enabled = false;
        let state = effective_state(b.enabled, hot());
        assert_eq!(b.base.face_fill_source(state, false), FaceFill::VisualStyle);
    }

    #[test]
    fn align_from_round_trips_every_one_of_the_nine_cells() {
        use ContentAlignment as A;
        for a in [
            A::TopLeft, A::TopCenter, A::TopRight,
            A::MiddleLeft, A::MiddleCenter, A::MiddleRight,
            A::BottomLeft, A::BottomCenter, A::BottomRight,
        ] {
            let (h, v) = a.fractions();
            assert_eq!(align_from(h, v), a, "{a:?} must survive a round-trip");
        }
    }

    /// `BackColor` is declared on `Control` and only RE-declared by `ButtonBase`,
    /// so it must be reachable through `Deref` rather than shadowed by a field.
    #[test]
    fn back_color_is_reached_through_deref_not_duplicated() {
        let mut b = Button::default();
        b.back_color = Some(D2D1_COLOR_F { r: 0.5, g: 0.5, b: 0.5, a: 1.0 });
        // One source of truth: the ControlBase the whole chain derefs to.
        assert!(b.control().back_color.is_some());
    }

    // ── CheckState cycling state machine. ───────────────────────────────────

    #[test]
    fn two_state_cycle_skips_indeterminate() {
        let mut b = CheckBox::default();
        assert_eq!(b.check_state, CheckState::Unchecked);
        b.perform_click();
        assert_eq!(b.check_state, CheckState::Checked);
        b.perform_click();
        assert_eq!(b.check_state, CheckState::Unchecked, "two-state never reaches Indeterminate");
    }

    #[test]
    fn three_state_cycle_visits_indeterminate_in_order() {
        let mut b = CheckBox::default();
        b.three_state = true;
        b.perform_click();
        assert_eq!(b.check_state, CheckState::Checked);
        b.perform_click();
        assert_eq!(b.check_state, CheckState::Indeterminate, "Checked → Indeterminate, not Unchecked");
        b.perform_click();
        assert_eq!(b.check_state, CheckState::Unchecked);
    }

    #[test]
    fn auto_check_off_freezes_the_state() {
        let mut b = CheckBox::default();
        b.auto_check = false;
        b.perform_click();
        assert_eq!(b.check_state, CheckState::Unchecked, "no AutoCheck means no automatic advance");
    }

    #[test]
    fn checked_is_a_projection_of_check_state() {
        let mut b = CheckBox::default();
        b.check_state = CheckState::Indeterminate;
        assert!(b.checked(), "Indeterminate reports as Checked, like the toolkit");
        b.set_checked(false);
        assert_eq!(b.check_state, CheckState::Unchecked);
        b.set_checked(true);
        assert_eq!(b.check_state, CheckState::Checked);
    }

    // ── RadioButton mutual exclusion. ───────────────────────────────────────

    #[test]
    fn selecting_one_radio_clears_its_siblings() {
        let mut group = [RadioButton::default(), RadioButton::default(), RadioButton::default()];
        select_radio(&mut group, 1);
        assert_eq!(
            (group[0].checked, group[1].checked, group[2].checked),
            (false, true, false)
        );
        select_radio(&mut group, 2);
        assert_eq!(
            (group[0].checked, group[1].checked, group[2].checked),
            (false, false, true),
            "the previous selection must clear"
        );
    }

    #[test]
    fn a_radio_click_selects_but_never_self_unchecks() {
        let mut b = RadioButton::default();
        b.perform_click();
        assert!(b.checked);
        b.perform_click();
        assert!(b.checked, "clicking a checked radio again keeps it checked");
    }

    #[test]
    fn selecting_out_of_range_clears_the_group() {
        let mut group = [RadioButton::default(), RadioButton::default()];
        select_radio(&mut group, 0);
        select_radio(&mut group, 99);
        assert!(!group[0].checked && !group[1].checked);
    }

    // ── Geometry that stays pure. ───────────────────────────────────────────

    #[test]
    fn split_glyph_puts_the_box_left_by_default() {
        let content = Rect::new(0.0, 0.0, 100.0, 20.0);
        let (glyph, label) = split_glyph(content, ContentAlignment::MiddleLeft);
        assert_eq!(glyph.left, 0.0);
        assert_eq!(glyph.right, GLYPH);
        assert_eq!(label.left, GLYPH + GLYPH_TEXT_GAP);
        assert_eq!(label.right, 100.0);
        // Vertically centred within the 20 DIP band.
        assert!((glyph.top - (20.0 - GLYPH) / 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn split_glyph_honours_a_right_check_align() {
        let content = Rect::new(0.0, 0.0, 100.0, 20.0);
        let (glyph, label) = split_glyph(content, ContentAlignment::MiddleRight);
        assert_eq!(glyph.right, 100.0);
        assert_eq!(glyph.left, 100.0 - GLYPH);
        assert_eq!(label.left, 0.0);
        assert_eq!(label.right, 100.0 - GLYPH - GLYPH_TEXT_GAP);
    }

    #[test]
    fn text_band_places_a_line_by_the_vertical_fraction() {
        let content = Rect::new(0.0, 0.0, 100.0, 40.0);
        assert_eq!(text_band(content, 20.0, 0.0).top, 0.0, "top");
        assert_eq!(text_band(content, 20.0, 0.5).top, 10.0, "middle");
        assert_eq!(text_band(content, 20.0, 1.0).top, 20.0, "bottom");
    }

    // ── Which (part, state) each glyph adopts. ──────────────────────────────

    fn pressed() -> ControlState {
        ControlState { hot: true, pressed: true, ..ControlState::default() }
    }

    /// `BP_CHECKBOX`'s twelve states, as a table: three check groups × four
    /// interaction states. A transposed group (checked drawn with the mixed ids,
    /// say) draws a plausible glyph rather than failing, which is exactly the
    /// kind of mistake a table pins and a formula does not.
    #[test]
    fn the_check_box_state_is_its_check_group_times_its_interaction() {
        use CheckState as S;
        let rest = ControlState::default();
        for (check, [normal, hot_id, pressed_id, disabled]) in [
            (
                S::Unchecked,
                [
                    CBS_UNCHECKEDNORMAL,
                    CBS_UNCHECKEDHOT,
                    CBS_UNCHECKEDPRESSED,
                    CBS_UNCHECKEDDISABLED,
                ],
            ),
            (
                S::Checked,
                [CBS_CHECKEDNORMAL, CBS_CHECKEDHOT, CBS_CHECKEDPRESSED, CBS_CHECKEDDISABLED],
            ),
            // The one name that differs from the toolkit's: `Indeterminate` is
            // the theme's MIXED.
            (
                S::Indeterminate,
                [CBS_MIXEDNORMAL, CBS_MIXEDHOT, CBS_MIXEDPRESSED, CBS_MIXEDDISABLED],
            ),
        ] {
            assert_eq!(check_box_state(true, rest, check), normal, "{check:?} at rest");
            assert_eq!(check_box_state(true, hot(), check), hot_id, "{check:?} hot");
            // The pointer is necessarily OVER a control it is pressing, so a
            // press is unreachable if hot is tested first.
            assert_eq!(check_box_state(true, pressed(), check), pressed_id, "{check:?} pressed");
            // Disabled outranks every pointer state.
            assert_eq!(
                check_box_state(false, pressed(), check),
                disabled,
                "{check:?} disabled"
            );
        }
        // Focus has no themed glyph — unlike a push button, `BP_CHECKBOX` has no
        // accented state, so a focused check box is still `NORMAL`.
        let focused = ControlState { focused: true, ..ControlState::default() };
        assert_eq!(check_box_state(true, focused, S::Unchecked), CBS_UNCHECKEDNORMAL);
    }

    /// `BP_RADIOBUTTON`'s eight — the same shape with two groups, because a radio
    /// has no third state.
    #[test]
    fn the_radio_button_state_is_its_check_group_times_its_interaction() {
        let rest = ControlState::default();
        for (checked, [normal, hot_id, pressed_id, disabled]) in [
            (
                false,
                [
                    RBS_UNCHECKEDNORMAL,
                    RBS_UNCHECKEDHOT,
                    RBS_UNCHECKEDPRESSED,
                    RBS_UNCHECKEDDISABLED,
                ],
            ),
            (
                true,
                [RBS_CHECKEDNORMAL, RBS_CHECKEDHOT, RBS_CHECKEDPRESSED, RBS_CHECKEDDISABLED],
            ),
        ] {
            assert_eq!(radio_button_state(true, rest, checked), normal, "checked={checked} rest");
            assert_eq!(radio_button_state(true, hot(), checked), hot_id, "checked={checked} hot");
            assert_eq!(
                radio_button_state(true, pressed(), checked),
                pressed_id,
                "checked={checked} pressed"
            );
            assert_eq!(
                radio_button_state(false, pressed(), checked),
                disabled,
                "checked={checked} disabled"
            );
        }
    }

    /// The twenty ids must be twenty DIFFERENT ids. Two colliding would make one
    /// state silently draw as another — the failure mode this whole family is
    /// migrating away from.
    #[test]
    fn the_twenty_glyph_state_ids_are_all_distinct() {
        let mut ids = vec![];
        for check in [CheckState::Unchecked, CheckState::Checked, CheckState::Indeterminate] {
            for (enabled, st) in
                [(true, ControlState::default()), (true, hot()), (true, pressed()), (false, hot())]
            {
                ids.push(check_box_state(enabled, st, check));
            }
        }
        assert_eq!(ids.len(), 12);
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 12, "BP_CHECKBOX ids collide: {ids:?}");

        let mut ids = vec![];
        for checked in [false, true] {
            for (enabled, st) in
                [(true, ControlState::default()), (true, hot()), (true, pressed()), (false, hot())]
            {
                ids.push(radio_button_state(enabled, st, checked));
            }
        }
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 8, "BP_RADIOBUTTON ids collide: {ids:?}");
    }

    /// A **focused** push button wears the theme's accented frame, which is what
    /// the reference sheet shows: its `Standard` button — the first control on
    /// the form, and the only one with the focus — is framed in `#0078D4`, while
    /// the one *labelled* « Default (accept) » is framed in the resting
    /// `#D0D0D0`. The sheet's source never assigns `AcceptButton`, so the accent
    /// is the focus, not the default.
    #[test]
    fn a_focused_push_button_is_defaulted_like_the_form_s_accept_button() {
        let focused = ControlState { focused: true, ..ControlState::default() };
        assert_eq!(push_button_state(true, focused, false), PBS_DEFAULTED);
        // …but only while it is otherwise at rest: a focused button under the
        // pointer is hot, and a focused DISABLED one cannot be focused at all.
        let hot_and_focused = ControlState { hot: true, focused: true, ..ControlState::default() };
        assert_eq!(push_button_state(true, hot_and_focused, false), PBS_HOT);
        assert_eq!(push_button_state(false, focused, false), PBS_DISABLED);
    }

    // ── Which FlatStyles hand their drawing to the theme. ───────────────────

    /// Measured on the reference sheet, not assumed: `Standard` **is**
    /// theme-drawn (`#D0D0D0`/`#FDFDFD`/`#BABABA`, and `#E9E9E9`/`#F9F9F9` when
    /// disabled — none of them a `COLOR_*`), exactly like `System`. `Flat`
    /// (`#000000` ring) and `Popup` (`#808080` ring) are on the same sheet and
    /// are not.
    #[test]
    fn standard_and_system_are_theme_drawn_flat_and_popup_are_not() {
        for style in [FlatStyle::Standard, FlatStyle::System] {
            assert!(face_is_themed(style, FaceFill::VisualStyle), "{style:?}");
            assert!(glyph_is_themed(style), "{style:?} glyph");
        }
        for style in [FlatStyle::Flat, FlatStyle::Popup] {
            assert!(!face_is_themed(style, FaceFill::VisualStyle), "{style:?}");
            assert!(!glyph_is_themed(style), "{style:?} glyph");
        }
    }

    /// The gate that keeps a named colour visible: a `Standard` button with an
    /// explicit `BackColor` paints that colour, so the themed part — which draws
    /// its own opaque face — must not be asked for at all. `System` cannot reach
    /// this arm (its Remarks say `BackColor` is ignored for button controls), so
    /// one condition covers both.
    #[test]
    fn a_named_back_color_takes_the_face_back_from_the_theme() {
        for fill in [FaceFill::Explicit, FaceFill::MouseOver, FaceFill::MouseDown, FaceFill::FlatChecked]
        {
            assert!(!face_is_themed(FlatStyle::Standard, fill), "{fill:?}");
        }
        let mut b = Button::default();
        b.flat_style = FlatStyle::Standard;
        b.back_color = Some(RED);
        assert!(!face_is_themed(b.flat_style, b.base.face_fill_source(ControlState::default(), false)));
        // …and a glyph is not a face: `BackColor` is its GROUND, so the check box
        // of a control with a named background is still the toolkit's own box.
        assert!(glyph_is_themed(FlatStyle::Standard));
    }

    // ── The pixels, pinned against the reference sheet. ─────────────────────

    /// Renders one `BUTTON` part into a DIB and returns its pixels — the same GDI
    /// path `theme::render_part` uses, stopping short of the Direct2D upload so
    /// the assertions need no device, no window and no swap chain.
    ///
    /// `None` when theming is unavailable, which is a **skip**, not a failure: on
    /// a themed-off machine (or under [`theme::CLASSIC_ENV`]) the classic path is
    /// the correct rendering and there is no themed pixel to assert.
    ///
    /// Rendered at **96 DPI** into a 13×13 box — the classic [`GLYPH`] size, and
    /// the one the theme draws its glyph at 1:1. The reference sheet was captured
    /// at 144 DPI, where the same part is 20×20; the flat regions asserted below
    /// are byte-identical at both, which is what makes a 96-DPI test comparable
    /// with a 144-DPI sheet at all.
    fn sample_glyph(part: i32, state: i32) -> Option<Vec<u32>> {
        sample_part(part, state, GLYPH as i32, GLYPH as i32)
    }

    /// `#RRGGBB` for a BGRA word, so a failure prints the colour a human can
    /// compare with the reference sheet rather than a decimal.
    fn hex(px: u32) -> String {
        format!("#{:02X}{:02X}{:02X}", (px >> 16) & 0xFF, (px >> 8) & 0xFF, px & 0xFF)
    }

    /// The middle row's left EDGE and its CENTRE, for a 13×13 glyph. The edge is
    /// sampled halfway down rather than at a corner, where a rounded or
    /// antialiased part blends into the ground.
    fn edge_and_centre(px: &[u32]) -> (String, String) {
        let n = GLYPH as usize;
        let m = n / 2;
        (hex(px[m * n]), hex(px[m * n + m]))
    }

    /// The three `BP_CHECKBOX` states the reference sheet actually contains, to
    /// the byte.
    ///
    /// The sheet's check boxes are `FlatStyle::Standard` at 144 DPI:
    ///
    /// * « Unchecked » — a `#626262` frame around a `#F3F3F3` field;
    /// * « Checked » — a solid accent `#005FB8` box;
    /// * « Indeterminate » — the same `#005FB8` box with a paler `#7FAFDB` bar.
    ///
    /// None of those three is a `GetSysColor` index, which is the whole reason
    /// this family had to stop drawing the glyph itself.
    #[test]
    fn the_check_box_glyph_is_the_reference_sheet() {
        let Some(unchecked) = sample_glyph(BP_CHECKBOX, CBS_UNCHECKEDNORMAL) else {
            eprintln!("[buttons] visual styles unavailable — themed glyph not asserted");
            return;
        };
        let (edge, centre) = edge_and_centre(&unchecked);
        assert_eq!(edge, "#626262", "CBS_UNCHECKEDNORMAL frame");
        assert_eq!(centre, "#F3F3F3", "CBS_UNCHECKEDNORMAL field");

        let checked = sample_glyph(BP_CHECKBOX, CBS_CHECKEDNORMAL).expect("checked");
        let (edge, centre) = edge_and_centre(&checked);
        assert_eq!(edge, "#005FB8", "CBS_CHECKEDNORMAL frame");
        assert_eq!(centre, "#005FB8", "CBS_CHECKEDNORMAL is a SOLID accent box");
        // The tick itself, one row below centre and two columns left of it —
        // white laid over the accent, so its blend is the proof the theme drew a
        // mark and not just a filled square.
        let n = GLYPH as usize;
        let m = n / 2;
        assert_eq!(hex(checked[(m + 1) * n + m - 2]), "#9FC3E4", "the tick");

        let mixed = sample_glyph(BP_CHECKBOX, CBS_MIXEDNORMAL).expect("mixed");
        let (edge, centre) = edge_and_centre(&mixed);
        assert_eq!(edge, "#005FB8", "CBS_MIXEDNORMAL frame");
        // The sheet's indeterminate bar is `#7FAFDB` at 144 DPI; at 96 the same
        // half-covered bar lands on `#BFD7ED`. What matters is that it is a BAR:
        // the centre of a mixed box is not the accent a checked one shows there.
        assert_eq!(centre, "#BFD7ED", "CBS_MIXEDNORMAL bar");
        assert_ne!(centre, "#005FB8", "a mixed box is not a checked one");
    }

    /// The three `BP_RADIOBUTTON` states the sheet contains — « Option A »
    /// selected, « Option B » clear, and the disabled one.
    ///
    /// The unselected radio's frame is the SAME `#626262` as the check box's,
    /// which is why the sheet cannot tell the two parts apart on colour alone:
    /// only the round outline does, and only a real part draws it.
    #[test]
    fn the_radio_glyph_is_the_reference_sheet() {
        let Some(clear) = sample_glyph(BP_RADIOBUTTON, RBS_UNCHECKEDNORMAL) else {
            eprintln!("[buttons] visual styles unavailable — themed glyph not asserted");
            return;
        };
        let (edge, centre) = edge_and_centre(&clear);
        assert_eq!(edge, "#626262", "RBS_UNCHECKEDNORMAL ring");
        assert_eq!(centre, "#F3F3F3", "RBS_UNCHECKEDNORMAL field");

        let selected = sample_glyph(BP_RADIOBUTTON, RBS_CHECKEDNORMAL).expect("checked");
        let (edge, centre) = edge_and_centre(&selected);
        assert_eq!(edge, "#005FB8", "RBS_CHECKEDNORMAL ring");
        assert_eq!(centre, "#FFFFFF", "the dot's own white core");

        // The sheet's disabled radio: `#C3C3C3` around `#F9F9F9`. It is a
        // distinct RENDERING, not a greyed copy — which is what makes passing
        // the control's real `enabled` through worth doing.
        let dead = sample_glyph(BP_RADIOBUTTON, RBS_UNCHECKEDDISABLED).expect("disabled");
        let (edge, centre) = edge_and_centre(&dead);
        assert_eq!(edge, "#C3C3C3", "RBS_UNCHECKEDDISABLED ring");
        assert_eq!(centre, "#F9F9F9", "RBS_UNCHECKEDDISABLED field");
    }

    /// Every state id must reach the theme and come back with its OWN pixels.
    ///
    /// The states the sheet cannot show — nothing on it is hot or pressed — are
    /// pinned here instead: hot, pressed and disabled each render differently
    /// from normal and from each other, so an id mapped to the wrong slot fails
    /// rather than drawing a plausible neighbour.
    #[test]
    fn each_glyph_state_id_renders_differently() {
        let states: [(&str, i32, i32); 8] = [
            ("check normal", BP_CHECKBOX, CBS_UNCHECKEDNORMAL),
            ("check hot", BP_CHECKBOX, CBS_UNCHECKEDHOT),
            ("check pressed", BP_CHECKBOX, CBS_UNCHECKEDPRESSED),
            ("check disabled", BP_CHECKBOX, CBS_UNCHECKEDDISABLED),
            ("radio normal", BP_RADIOBUTTON, RBS_UNCHECKEDNORMAL),
            ("radio hot", BP_RADIOBUTTON, RBS_UNCHECKEDHOT),
            ("radio pressed", BP_RADIOBUTTON, RBS_UNCHECKEDPRESSED),
            ("radio disabled", BP_RADIOBUTTON, RBS_UNCHECKEDDISABLED),
        ];
        let mut seen: Vec<(&str, String, String)> = vec![];
        for (name, part, state) in states {
            let Some(px) = sample_glyph(part, state) else {
                eprintln!("[buttons] visual styles unavailable — themed states not asserted");
                return;
            };
            let (edge, centre) = edge_and_centre(&px);
            seen.push((name, edge, centre));
        }
        // Within one part, the four interaction states are four renderings.
        for group in seen.chunks(4) {
            for (i, a) in group.iter().enumerate() {
                for b in &group[i + 1..] {
                    assert_ne!(
                        (&a.1, &a.2),
                        (&b.1, &b.2),
                        "{} and {} render identically — one of the two ids is wrong",
                        a.0,
                        b.0
                    );
                }
            }
        }
        // The measured values, so a Windows update that moves them is a failing
        // test rather than a slow drift.
        assert_eq!((seen[1].1.as_str(), seen[1].2.as_str()), ("#626262", "#EAEAEA"), "check hot");
        assert_eq!(
            (seen[2].1.as_str(), seen[2].2.as_str()),
            ("#C3C3C3", "#E2E2E2"),
            "check pressed"
        );
        assert_eq!(
            (seen[3].1.as_str(), seen[3].2.as_str()),
            ("#C3C3C3", "#F9F9F9"),
            "check disabled"
        );
        assert_eq!((seen[5].1.as_str(), seen[5].2.as_str()), ("#626262", "#EAEAEA"), "radio hot");
        assert_eq!(seen[6].1.as_str(), "#C3C3C3", "radio pressed ring");
    }

    /// `BP_PUSHBUTTON` reserves a **one-pixel transparent margin**: index 0 of the
    /// middle row is still the pre-fill, and the frame only starts at index 1.
    ///
    /// That is the measurement `paint_face` insets its themed interior by TWO
    /// device pixels for, and it is what the sheet shows too — the `System`
    /// button's `#D0D0D0` frame sits one pixel INSIDE its bounds, where a classic
    /// bevel would sit on them. Pinned here as well as in `theme.rs` because it
    /// is this file that depends on it.
    #[test]
    fn the_themed_push_button_keeps_its_one_pixel_margin() {
        let (w, h) = (60usize, 26usize);
        let Some(px) = sample_push_button(PBS_NORMAL, w as i32, h as i32) else {
            eprintln!("[buttons] visual styles unavailable — themed margin not asserted");
            return;
        };
        let mid = h / 2;
        assert_eq!(hex(px[mid * w]), "#F0F0F0", "index 0 is the part's own margin");
        assert_eq!(hex(px[mid * w + 1]), "#D0D0D0", "the frame starts one pixel in");
        assert_eq!(hex(px[mid * w + w / 2]), "#FDFDFD", "the face");

        // The two states the sheet distinguishes from `PBS_NORMAL`, both on
        // `FlatStyle::Standard` buttons: the focused one wears the accent frame,
        // the disabled one an entirely different face.
        let defaulted = sample_push_button(PBS_DEFAULTED, w as i32, h as i32).expect("defaulted");
        assert_eq!(hex(defaulted[mid * w + 1]), "#0078D4", "the sheet's focused Standard button");
        let disabled = sample_push_button(PBS_DISABLED, w as i32, h as i32).expect("disabled");
        assert_eq!(hex(disabled[mid * w + 1]), "#E9E9E9", "the sheet's disabled Standard button");
        assert_eq!(hex(disabled[mid * w + w / 2]), "#F9F9F9", "…and its own face");
    }

    /// [`sample_glyph`] for a rectangle that is not a 13×13 glyph.
    fn sample_push_button(state: i32, w: i32, h: i32) -> Option<Vec<u32>> {
        sample_part(BP_PUSHBUTTON, state, w, h)
    }

    /// Renders one `BUTTON` part into a DIB and hands back its pixels — see
    /// [`sample_glyph`] for what the answer means and when it is `None`.
    fn sample_part(part: i32, state: i32, w: i32, h: i32) -> Option<Vec<u32>> {
        use windows::core::w as wide;
        use windows::Win32::Foundation::RECT;
        use windows::Win32::Graphics::Gdi::{
            CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, SelectObject,
            BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        };
        use windows::Win32::UI::Controls::{
            CloseThemeData, DrawThemeBackground, IsAppThemed, IsThemeActive,
        };
        use windows::Win32::UI::HiDpi::OpenThemeDataForDpi;

        if std::env::var_os(theme::CLASSIC_ENV).is_some() {
            return None;
        }
        // SAFETY: as `sample_glyph` — every handle is released on every path.
        unsafe {
            if !IsThemeActive().as_bool() || !IsAppThemed().as_bool() {
                return None;
            }
            let theme = OpenThemeDataForDpi(None, wide!("BUTTON"), 96);
            if theme.is_invalid() {
                return None;
            }
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                let _ = CloseThemeData(theme);
                return None;
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    // Negative: top-down, so row 0 is the TOP row. A bottom-up
                    // DIB would mirror the part and quietly move its edges.
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            {
                Ok(b) if !b.is_invalid() && !bits.is_null() => b,
                _ => {
                    let _ = DeleteDC(dc);
                    let _ = CloseThemeData(theme);
                    return None;
                }
            };
            let previous = SelectObject(dc, bitmap.into());
            let px = std::slice::from_raw_parts_mut(bits.cast::<u32>(), (w * h) as usize);
            // The sheet was captured on `COLOR_BTNFACE` (#F0F0F0), which is the
            // ground a form puts behind all three of these parts — so a pixel
            // sampled here is directly comparable with one sampled from it.
            px.fill(0xFFF0_F0F0);
            let rect = RECT { left: 0, top: 0, right: w, bottom: h };
            let drawn = DrawThemeBackground(theme, dc, part, state, &rect, None).is_ok();
            let _ = GdiFlush();
            let out = drawn.then(|| px.to_vec());
            SelectObject(dc, previous);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(dc);
            let _ = CloseThemeData(theme);
            out
        }
    }
}
