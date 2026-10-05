//! Kubuno primitives — **display**: [`Label`], [`LinkLabel`], [`Badge`],
//! [`Icon`], [`Tooltip`], [`Separator`].
//!
//! None of these is interactive the way a button is: they *show* something.
//! What they share is that the toolkit already knows how to model and place
//! them — so this module owns almost no state. `Label`, `LinkLabel` and `Icon`
//! wrap the replicas in [`kubuno_desktop_controls::labels`] and reach `text`,
//! `enabled`, `padding`, `text_align`, `auto_size`, `use_mnemonic`,
//! `image_align`, `border_style`, `size_mode`… through [`Deref`]. `Badge`,
//! `Tooltip` and `Separator` have no counterpart in WinForms (a `ToolTip` is a
//! *component* there, not a control), so they compose the closest replica that
//! carries the properties a layout needs — an empty [`kubuno_desktop_controls::labels::Label`]
//! — rather than declaring a rival property block.
//!
//! ## Where the pixels come from
//!
//! There is no desktop predecessor for this family, so the reference is the
//! **web** design system, read (not measured — none of these surfaces was
//! reachable from this machine):
//!
//! | primitive | web source |
//! |---|---|
//! | `Badge` | `core/frontend/src/ui/Badge.tsx` |
//! | `Tooltip` | `core/frontend/src/ui/Tooltip.tsx` + `ui/tooltipPlacement.ts` |
//! | `Separator` | `core/frontend/src/ui/Separator.tsx` |
//! | `LinkLabel` | the doc link in `core/frontend/src/ui/EmptyState.tsx` (`text-primary hover:underline focus-visible:ring-2`) |
//! | type scale | `core/frontend/src/theme.css` (`--kb-text-*`) |
//!
//! ## Overflow
//!
//! Text never leaves the box it is painted into. A [`Label`] clips to its
//! bounds and, by default, ends an overflowing line with « … » (the web's
//! `truncate`); [`TextOverflow::Wrap`] wraps on words instead (the web's
//! default `white-space: normal`) and clamps to the lines the box can hold.
//! A [`Badge`] ellipsizes inside its pill ([`Badge::max_width`]), and a
//! [`Tooltip`] wraps at `maxWidth: 280` exactly as `TOOLTIP_STYLE` does
//! (`whiteSpace: 'pre-line'`: explicit line breaks are honoured too). The
//! wrapping itself is the pure [`wrap_lines`], because the shared DirectWrite
//! formats are all `NO_WRAP`.
//!
//! Every number below names its line. The two that have **no** source at all
//! are [`Tooltip::ARROW`] and [`Tooltip::ARROW_HALF`]: the web bubble has no
//! arrow to copy.
//!
//! ## The trap this module is tested against
//!
//! A primitive paints into the `bounds` **argument**, never into its model's
//! own rectangle. The two are different spaces — the argument is in canvas
//! coordinates, `ControlBase::bounds` is parent-relative — and reading the
//! field has been shipped, found and fixed three times already (`Label`,
//! `PictureBox`, `LinkLabel`). The test
//! `no_paint_path_reads_the_models_own_rectangle` greps this very file and
//! fails if the field comes back.

use std::ops::{Deref, DerefMut};

use kubuno_drive_desktop_app_controls::{Canvas, Rect, TextFormats, Theme};
use kubuno_desktop_controls::enums::{BorderStyle, ContentAlignment, Padding, Size};
use kubuno_desktop_controls::labels::{self as kc, LinkPaint};
use kubuno_desktop_controls::Control;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
};

use crate::metrics::{control, pill, radius, space, text as type_size, SHADOW_GREY, SHADOW_MENU};
use crate::{Widget, WidgetState};

// ─────────────────────────────────────────────────────────────────────────────
// The typographic role — what Kubuno adds on top of a replica label
// ─────────────────────────────────────────────────────────────────────────────

/// The web type scale, as a role a label can be asked for.
///
/// This is *not* [`kubuno_desktop_controls::control::FontRole`], and it deliberately
/// does not replace it: that one names the **system** UI font's weights
/// (`Caption`/`CaptionStrong`/`Body`/…), which is what a replica paints with.
/// This one names the five steps `theme.css` publishes (`--kb-text-micro` …
/// `--kb-text-title`), which is what a *Kubuno* surface paints with. Two
/// different questions, so two different enums; a primitive answers this one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Role {
    /// `--kb-text-micro` (10.5) — badges, counters.
    Micro,
    /// `--kb-text-meta` (11.5) — metadata, captions, section labels.
    Meta,
    /// `--kb-text-body` (13.5) — the default: labels, fields, menus, tabs.
    #[default]
    Body,
    /// `--kb-text-heading` (15.5) — section headers, card and window titles.
    Heading,
    /// `--kb-text-title` (21.5).
    Title,
    /// `--kb-text-page` (22.5) — the title heading a page (the web's `h1`).
    Page,
    /// `--kb-text-page` inside the administration console (`.kb-admin`, 27.5).
    PageAdmin,
    /// `--kb-text-badge` (10) — initials and counters inside small avatar and
    /// count pills (the web's `text-[10px]`).
    Badge,
    /// `--kb-text-caption` (11) — small pills and chips (the web's `text-[11px]`).
    Caption,
    /// `--kb-text-subtitle` (16) — a group title a step above `Heading` (the
    /// web's `text-base`).
    Subtitle,
    /// `--kb-text-display` (24) — a large greeting or display line (the web's
    /// `text-2xl`).
    Display,
}

/// Every role, in scale order — for the gallery and for the tests.
pub const ROLES: [Role; 11] = [
    Role::Badge,
    Role::Micro,
    Role::Caption,
    Role::Meta,
    Role::Body,
    Role::Heading,
    Role::Subtitle,
    Role::Title,
    Role::Page,
    Role::Display,
    Role::PageAdmin,
];

impl Role {
    /// The em size, straight from [`crate::metrics::text`].
    pub const fn size(self) -> f32 {
        match self {
            Self::Micro => type_size::MICRO,
            Self::Meta => type_size::META,
            Self::Body => type_size::BODY,
            Self::Heading => type_size::HEADING,
            Self::Title => type_size::TITLE,
            Self::Page => type_size::PAGE,
            Self::PageAdmin => type_size::PAGE_ADMIN,
            Self::Badge => type_size::BADGE,
            Self::Caption => type_size::CAPTION,
            Self::Subtitle => type_size::SUBTITLE,
            Self::Display => type_size::DISPLAY,
        }
    }

    /// The **line box** the role occupies — what an auto-sized label is tall.
    ///
    /// The web never publishes these as tokens; they are the line-heights its
    /// utilities emit, and two of them are confirmed against surfaces that
    /// state their own arithmetic:
    ///
    /// * `Meta` → **16**: `TOOLTIP_STYLE` in `@ui/Tooltip` sets
    ///   `fontSize: 12, lineHeight: '16px'` — which is also Tailwind's own
    ///   `text-xs` pair.
    /// * `Body` → **20**: `shape::height::MENU_ITEM` is documented as
    ///   « 5px + **20px line** + 5px » (`MenuDropdown` sets `lineHeight: 20px`
    ///   on its 13.5 px rows), and Tailwind's `text-sm` leading (1.25 / 0.875)
    ///   gives 19.3 at the host's 13.5 px.
    /// * `Heading` → **24**: Tailwind's `text-base` pair, `1rem / 1.5rem`.
    /// * `Micro` → **16**: the Meta line box, so a counter lines up with the
    ///   metadata it sits next to.
    /// * `Title` → **30**: no Tailwind pair covers 21.5 (the web writes it as
    ///   an arbitrary size, so the browser's `normal` leading applies). It
    ///   holds the face's own design line spacing — the 2724/2048 ≈ 1.33008
    ///   ratio `kubuno_desktop_controls::labels::UI_LINE_SPACING` documents, ceiled the
    ///   way `Font.Height` is: `ceil(21.5 × 1.33008) = 29` — with one DIP to
    ///   spare; kept at 30 so title rows did not move when the scale was
    ///   aligned on the web.
    /// * `Page` → **32** and `PageAdmin` → **38**: the same design line spacing
    ///   (`ceil(22.5 × 1.33008) = 30`, `ceil(27.5 × 1.33008) = 37`), with room to spare.
    ///
    /// * `Badge` → **16** and `Caption` → **16**: the Meta line box, so a pill
    ///   lines up with the metadata around it (`ceil(10 × 1.33008) = 14`,
    ///   `ceil(11 × 1.33008) = 15`).
    /// * `Subtitle` → **24**: Tailwind's `text-base` pair, `1rem / 1.5rem`
    ///   (`ceil(16 × 1.33008) = 22`).
    /// * `Display` → **32**: Tailwind's `text-2xl` pair, `1.5rem / 2rem`
    ///   (`ceil(24 × 1.33008) = 32`).
    ///
    /// Every box holds its face's own line (`ceil(size × 1.33008)`: 14, 14, 15,
    /// 16, 18, 21, 22, 29, 30, 32, 37), so no role clips its glyphs.
    pub const fn line_height(self) -> f32 {
        match self {
            Self::Badge | Self::Micro | Self::Caption | Self::Meta => 16.0,
            Self::Body => 20.0,
            Self::Heading | Self::Subtitle => 24.0,
            Self::Display => 32.0,
            Self::Title => 30.0,
            Self::Page => 32.0,
            Self::PageAdmin => 38.0,
        }
    }

    /// The shared DirectWrite format the role paints with.
    ///
    /// Each role has its own face in [`TextFormats`]: `Micro` paints with
    /// `micro` (10.5), `Meta` with `caption` (11.5).
    pub fn format(self, f: &TextFormats) -> &IDWriteTextFormat {
        match self {
            Self::Micro => &f.micro,
            Self::Meta => &f.caption,
            Self::Body => &f.body,
            Self::Heading => &f.heading,
            Self::Title => &f.title,
            Self::Page => &f.page,
            Self::PageAdmin => &f.page_admin,
            Self::Badge => &f.role_badge,
            Self::Caption => &f.role_caption,
            Self::Subtitle => &f.role_subtitle,
            Self::Display => &f.role_display,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared geometry — pure, so every cell of it is a test and not a screenshot
// ─────────────────────────────────────────────────────────────────────────────

/// What a border eats on each side, in DIP.
///
/// TRAP: this is **not** the replica's inset. WinForms reserves two pixels for
/// `Fixed3D` because it paints a two-tone *bevel*; the Kubuno design system has
/// no bevel at all — a bordered surface is one hairline in `card_stroke`. So
/// both border styles reserve the same single DIP here, and `Fixed3D` means
/// « bordered », not « sunken ».
pub const fn border_inset(style: BorderStyle) -> f32 {
    match style {
        BorderStyle::None => 0.0,
        BorderStyle::FixedSingle | BorderStyle::Fixed3D => 1.0,
    }
}

/// `bounds` deflated by the border and then by padding — where content goes.
///
/// Takes the rectangle as an argument for the reason the module doc gives: the
/// caller's rectangle and the model's own are different spaces.
pub fn content_rect(bounds: Rect, style: BorderStyle, padding: Padding) -> Rect {
    let i = border_inset(style);
    let left = bounds.left + i + padding.left;
    let top = bounds.top + i + padding.top;
    Rect::new(
        left,
        top,
        (bounds.right - i - padding.right).max(left),
        (bounds.bottom - i - padding.bottom).max(top),
    )
}

/// Places a `size` box inside `content` at one of the nine
/// [`ContentAlignment`] cells.
///
/// The fractions come from the replica ([`ContentAlignment::fractions`] returns
/// 0.0/0.5/1.0 per axis), so the nine cells cannot disagree with the toolkit's.
/// A box larger than the space it is placed in is pinned to the top-left rather
/// than pushed out — `max` on the available extent, exactly as the replica's
/// own vertical band does.
pub fn aligned_box(content: Rect, alignment: ContentAlignment, size: Size) -> Rect {
    let (hf, vf) = alignment.fractions();
    let avail_w = (content.right - content.left).max(size.width);
    let avail_h = (content.bottom - content.top).max(size.height);
    let left = content.left + (avail_w - size.width) * hf;
    let top = content.top + (avail_h - size.height) * vf;
    Rect::new(left, top, left + size.width, top + size.height)
}

/// The one-line band a label's text sits in: full content width, `line_h` tall,
/// placed at the alignment's vertical third.
///
/// Only the vertical axis is resolved here — the horizontal one is handed to
/// DirectWrite through [`h_alignment`], which is what makes trimmed and
/// right-aligned text land correctly instead of being laid out twice.
pub fn text_band(content: Rect, alignment: ContentAlignment, line_h: f32) -> Rect {
    let box_ = aligned_box(content, alignment, Size::new(content.right - content.left, line_h));
    Rect::new(content.left, box_.top, content.right, box_.bottom)
}

/// The DirectWrite alignment for a content alignment's horizontal third.
pub fn h_alignment(a: ContentAlignment) -> DWRITE_TEXT_ALIGNMENT {
    match a.fractions().0 {
        x if x < 0.25 => DWRITE_TEXT_ALIGNMENT_LEADING,
        x if x > 0.75 => DWRITE_TEXT_ALIGNMENT_TRAILING,
        _ => DWRITE_TEXT_ALIGNMENT_CENTER,
    }
}

/// A label's intrinsic size: the text extent grown by padding and border.
/// Pure, so `AutoSize` is checked without a device.
pub fn label_size(text_extent: Size, padding: Padding, style: BorderStyle) -> Size {
    let frame = border_inset(style) * 2.0;
    Size::new(
        text_extent.width + padding.horizontal() + frame,
        text_extent.height + padding.vertical() + frame,
    )
}

/// The width to reserve for `text`, in DIP.
///
/// [`Canvas::measure`] returns DirectWrite's exact advance width, which is
/// fractional. Reserving exactly that much then laying the run out in it makes
/// the trimming pass fire on the last glyph — a bubble reading « au-desso… »
/// for a label that fits. One DIP of ceiling costs nothing and removes the
/// class of bug entirely, so every measurement here goes through this.
fn text_width(canvas: &dyn Canvas, text: &str, format: &IDWriteTextFormat) -> f32 {
    if text.is_empty() {
        0.0
    } else {
        canvas.measure(text, format).ceil()
    }
}

/// Slack allowed when comparing a measured run against the width it must fit.
/// DirectWrite widths are fractional; without it a run that fits to the
/// hundredth of a DIP is reported as overflowing and gets a spurious « … ».
const FIT_EPSILON: f32 = 0.5;

/// Breaks `text` into the lines a box `max_width` wide shows — the pure core
/// of every wrapping surface here, because the shared DirectWrite formats are
/// all `NO_WRAP` and a layout that wraps behind the caller's back could not be
/// measured.
///
/// The rules are CSS `white-space: pre-line` (what `TOOLTIP_STYLE` sets, and
/// what a wrapping web label does with explicit breaks):
///
/// * an explicit `\n` (or `\r\n`) always starts a new line;
/// * runs of spaces collapse to one, and a line never starts or ends with one;
/// * lines break greedily between words;
/// * a single word wider than the box is broken between characters
///   (`overflow-wrap: anywhere`) — the one deliberate departure from the
///   browser default, which would let it spill out of the box.
///
/// `max_width <= 0` (or not finite) means « no width constraint »: only the
/// explicit breaks apply. `measure` returns a run's width in DIP.
pub fn wrap_lines(text: &str, max_width: f32, measure: &mut dyn FnMut(&str) -> f32) -> Vec<String> {
    let constrained = max_width.is_finite() && max_width > 0.0;
    let fits = |w: f32| w <= max_width + FIT_EPSILON;
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        let paragraph = paragraph.strip_suffix('\r').unwrap_or(paragraph);
        let words: Vec<&str> = paragraph.split_whitespace().collect();
        if !constrained {
            out.push(words.join(" "));
            continue;
        }
        let mut line = String::new();
        for word in words {
            let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            if fits(measure(&candidate)) {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            if fits(measure(word)) {
                line = word.to_owned();
                continue;
            }
            // A word longer than the whole box: break it between characters.
            for ch in word.chars() {
                let mut grown = line.clone();
                grown.push(ch);
                if line.is_empty() || fits(measure(&grown)) {
                    line = grown;
                } else {
                    out.push(std::mem::replace(&mut line, ch.to_string()));
                }
            }
        }
        out.push(line);
    }
    out
}

/// Keeps at most `max_lines` of `lines` — the web's `line-clamp`. When lines
/// are dropped, the last kept line carries the rest of the text joined on, so
/// the ellipsizing painter ends it with « … » exactly where the box runs out.
/// Returns whether anything was dropped. `max_lines` is at least one: a box
/// shorter than a line still shows the first one, clipped.
pub fn clamp_lines(mut lines: Vec<String>, max_lines: usize) -> (Vec<String>, bool) {
    let max_lines = max_lines.max(1);
    if lines.len() <= max_lines {
        return (lines, false);
    }
    let rest = lines.split_off(max_lines - 1);
    lines.push(rest.join(" "));
    (lines, true)
}

/// How many `line_h` lines fit in `height` (at least one). Half a DIP of
/// tolerance so a box measured for N lines is never judged to hold N − 1.
pub fn lines_that_fit(height: f32, line_h: f32) -> usize {
    if line_h <= 0.0 {
        return 1;
    }
    (((height + FIT_EPSILON) / line_h).floor() as usize).max(1)
}

/// The widest of `lines`, ceiled like [`text_width`].
fn widest(canvas: &dyn Canvas, lines: &[String], format: &IDWriteTextFormat) -> f32 {
    lines.iter().map(|l| text_width(canvas, l, format)).fold(0.0, f32::max)
}

/// The colour inert text is painted in — `--color-text-tertiary`, the palest
/// step of the ramp and the one the web greys disabled controls with.
fn inert(t: &Theme) -> D2D1_COLOR_F {
    t.text_tertiary
}

/// Whether a widget must paint as inert: either the Kubuno state says so, or
/// the replica's own `Control.Enabled` does. Both are honoured because a
/// container greys a subtree through the state without touching its children.
fn is_inert(model: &kubuno_desktop_controls::ControlBase, state: WidgetState) -> bool {
    state.disabled || !model.enabled
}

// ─────────────────────────────────────────────────────────────────────────────
// Label
// ─────────────────────────────────────────────────────────────────────────────

/// What a [`Label`] does with text wider than its box.
///
/// Whatever the mode, nothing is painted outside the `bounds` a label is
/// given: the box clips.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextOverflow {
    /// One line per explicit break, an overflowing line ends in « … » — the
    /// web's `truncate` (`overflow: hidden; text-overflow: ellipsis;
    /// white-space: nowrap`), which is what every single-line label in the
    /// web's rows, cards and headers uses.
    #[default]
    Ellipsis,
    /// One line per explicit break, cut at the box edge with no marker —
    /// WinForms' own `AutoEllipsis = false`. `auto_ellipsis` still forces the
    /// marker, as it does in the toolkit.
    Clip,
    /// Wrapped on words to the box width (`white-space: normal`), clamped to
    /// the lines the box can hold, the last kept line ending in « … »
    /// (`line-clamp`). An auto-sized label wraps at `MaximumSize.Width` when
    /// one is set — the WinForms rule for a growing label.
    Wrap,
}

/// A Kubuno label — [`kubuno_desktop_controls::labels::Label`] with Kubuno type.
///
/// Everything a caller sets is the replica's: `text`, `text_align` (the nine
/// cells), `auto_size`, `auto_ellipsis`, `use_mnemonic` (`&` handling),
/// `image` / `image_align`, `border_style`, `padding`, `enabled`, `fore_color`.
/// The only field added here is [`Role`], because .NET has no equivalent of the
/// web's type scale.
pub struct Label {
    inner: kc::Label,
    /// Which step of the web type scale the text is set in.
    pub role: Role,
    /// What happens to text wider than the box; see [`TextOverflow`].
    pub overflow: TextOverflow,
    /// The shown text's character to underline (a `&Name` mnemonic, while Alt is held).
    pub mnemonic: Option<usize>,
}

impl Deref for Label {
    type Target = kc::Label;
    fn deref(&self) -> &kc::Label {
        &self.inner
    }
}
impl DerefMut for Label {
    fn deref_mut(&mut self) -> &mut kc::Label {
        &mut self.inner
    }
}

impl Label {
    /// A body-sized label. `TextAlign` keeps the replica's default (`TopLeft`,
    /// which is a *label's* default and not the usual `MiddleCenter`).
    pub fn new(text: impl Into<String>) -> Self {
        let mut inner = kc::Label::new();
        inner.text = text.into();
        Self { inner, role: Role::default(), overflow: TextOverflow::default(), mnemonic: None }
    }

    /// Builder: the type step.
    pub fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }

    /// Builder: one of the nine cells.
    pub fn align(mut self, a: ContentAlignment) -> Self {
        self.inner.text_align = a;
        self
    }

    /// Builder: what happens to text wider than the box.
    pub fn overflow(mut self, overflow: TextOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// Builder: shorthand for [`TextOverflow::Wrap`] — a multi-line label.
    pub fn wrap(self) -> Self {
        self.overflow(TextOverflow::Wrap)
    }

    /// What the frame (border + padding) eats on each axis, in DIP.
    fn frame(&self) -> (f32, f32) {
        let b = border_inset(self.inner.border_style) * 2.0;
        (self.inner.padding.horizontal() + b, self.inner.padding.vertical() + b)
    }

    /// The lines the text is laid out in when the content box is `width` wide
    /// (`None` = unconstrained): wrapped under [`TextOverflow::Wrap`], else one
    /// per explicit break.
    pub fn lines(&self, canvas: &dyn Canvas, width: Option<f32>) -> Vec<String> {
        let shown = self.inner.shown_text();
        let fmt = self.role.format(canvas.formats());
        let limit = match (self.overflow, width) {
            (TextOverflow::Wrap, Some(w)) => w,
            _ => 0.0,
        };
        wrap_lines(&shown, limit, &mut |s| canvas.measure(s, fmt))
    }

    /// The size this label needs when it is given `width` DIP (frame
    /// included) — the height-for-width a layout asks a wrapping label. For a
    /// non-wrapping label the height is one line per explicit break and the
    /// width is capped at `width`.
    pub fn measure_for_width(&self, canvas: &dyn Canvas, width: f32) -> Size {
        let (fx, fy) = self.frame();
        let lines = self.lines(canvas, Some((width - fx).max(0.0)));
        let fmt = self.role.format(canvas.formats());
        let w = widest(canvas, &lines, fmt).min((width - fx).max(0.0));
        let h = lines.len().max(1) as f32 * self.role.line_height();
        self.inner.clamp(Size::new(w + fx, h + fy))
    }

    /// The content rectangle for a given caller rectangle.
    pub fn content(&self, rect: Rect) -> Rect {
        content_rect(rect, self.inner.border_style, self.inner.padding)
    }

    /// The one-line band the text lands in, for a given caller rectangle.
    pub fn band(&self, rect: Rect) -> Rect {
        text_band(self.content(rect), self.inner.text_align, self.role.line_height())
    }

    /// Where the host should blit this label's image, if it carries one.
    ///
    /// The replica models images as their natural [`Size`] only (the pixels are
    /// host-supplied), so the primitive's job is the *placement* — which is
    /// `image_align`, resolved through the same nine-cell [`aligned_box`] the
    /// text uses. Returns a rectangle in the caller's space, like everything
    /// else here.
    pub fn image_rect(&self, rect: Rect) -> Option<Rect> {
        self.inner
            .image
            .map(|img| aligned_box(self.content(rect), self.inner.image_align, img))
    }
}

impl Widget for Label {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        // A wrapping label with a `MaximumSize.Width` grows downwards at that
        // width, as an auto-sized WinForms label does; everything else is its
        // natural extent — one line per explicit break.
        let max_w = self.inner.maximum_size.width;
        if self.overflow == TextOverflow::Wrap && max_w > 0.0 {
            return self.measure_for_width(canvas, max_w);
        }
        let fmt = self.role.format(canvas.formats());
        let lines = self.lines(canvas, None);
        let extent = Size::new(
            widest(canvas, &lines, fmt),
            lines.len().max(1) as f32 * self.role.line_height(),
        );
        // `clamp` is the replica's — MinimumSize/MaximumSize behave here
        // exactly as they do in the toolkit, including « empty means unset ».
        self.inner.clamp(label_size(extent, self.inner.padding, self.inner.border_style))
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();

        // A label is transparent unless it was given a colour — the same rule
        // the replica follows, so it sits on whatever surface it is placed on.
        if let Some(bg) = self.inner.back_color {
            canvas.fill_rounded(&bounds, radius::SM, &bg);
        }
        if self.inner.border_style != BorderStyle::None {
            canvas.stroke_rounded(&bounds, radius::SM, &t.card_stroke);
        }

        if self.inner.shown_text().is_empty() {
            return;
        }
        let colour = if is_inert(&self.inner.control, state) {
            inert(t)
        } else {
            self.inner.fore_color.unwrap_or(t.text_primary)
        };
        let fmt = self.role.format(canvas.formats());
        let align = h_alignment(self.inner.text_align);
        let line_h = self.role.line_height();

        let content = self.content(bounds);
        let avail_w = content.right - content.left;
        let lines = self.lines(canvas, Some(avail_w));
        let (lines, _) = clamp_lines(lines, lines_that_fit(content.bottom - content.top, line_h));
        // The block of lines is placed as one box at the alignment's vertical
        // third; with a single line this is exactly [`Label::band`].
        let block = text_band(content, self.inner.text_align, lines.len() as f32 * line_h);

        // The box clips: a word that cannot be shortened (Clip mode, or a box
        // narrower than « … ») is cut at the edge instead of running through
        // the card border next to it. Vertically the clip also admits the line
        // box itself, so a caller who hands a rectangle shorter than one line
        // still sees whole glyphs, as before.
        let clip = Rect::new(
            bounds.left,
            bounds.top.min(block.top),
            bounds.right,
            bounds.bottom.max(block.bottom),
        );
        canvas.push_clip(&clip);
        for (i, line) in lines.iter().enumerate() {
            let top = block.top + i as f32 * line_h;
            let band = Rect::new(content.left, top, content.right, top + line_h);
            let overflows = canvas.measure(line, fmt) > avail_w + FIT_EPSILON;
            let ellipsize = self.inner.auto_ellipsis
                || (overflows && self.overflow != TextOverflow::Clip);
            if ellipsize {
                // The canvas exposes trimming for leading and centred text
                // only; trailing falls back to leading, as the replica's does
                // (an overflowing line fills the box anyway).
                if align == DWRITE_TEXT_ALIGNMENT_CENTER {
                    canvas.text_ellipsis_center(line, &band, fmt, &colour);
                } else {
                    canvas.text_ellipsis(line, &band, fmt, &colour);
                }
            } else if overflows {
                // Clip: the run starts at the leading edge and is cut.
                canvas.text_aligned(line, &band, fmt, &colour, DWRITE_TEXT_ALIGNMENT_LEADING);
            } else {
                canvas.text_aligned(line, &band, fmt, &colour, align);
                // The mnemonic's letter, on a one-line label drawn whole.
                if let (Some(index), 1) = (self.mnemonic, lines.len()) {
                    crate::mnemonic::underline(canvas, line, index, &band, fmt, &colour, align);
                }
            }
        }
        canvas.pop_clip();
    }

    fn type_name(&self) -> &'static str {
        "Label"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LinkLabel
// ─────────────────────────────────────────────────────────────────────────────

/// A Kubuno hyperlink — [`kubuno_desktop_controls::labels::LinkLabel`] in the Kubuno
/// palette.
///
/// The whole link *model* is the replica's: `links` / `link_area` and the
/// « whole text is one link when neither is set » resolution
/// ([`kubuno_desktop_controls::labels::LinkLabel::resolved_links`]), the four-way
/// colour precedence ([`kubuno_desktop_controls::labels::LinkLabel::link_paint`]) and
/// the underline decision ([`kubuno_desktop_controls::labels::LinkLabel::underlines_link`],
/// which folds in `LinkBehavior` and the link's own `Enabled`). This layer
/// changes exactly one thing: which colour each of the four cases resolves to.
pub struct LinkLabel {
    inner: kc::LinkLabel,
    pub role: Role,
    /// The link under the pointer, as an index into `resolved_links()` —
    /// what makes hover and press **per link** (the web's `:hover` is per
    /// `<a>`). `None` keeps the replica's whole-control reading, where every
    /// link reacts to `WidgetState::hot` together. Resolve it with
    /// [`LinkLabel::link_at`].
    pub hot_link: Option<usize>,
    /// The link holding the keyboard focus, as an index into
    /// `resolved_links()`: the focus ring is drawn around it when the state
    /// says `show_focus_ring()`. `None` rings the first link. Each link is its
    /// own tab stop on the web (one `<a>` each); register every
    /// [`LinkLabel::link_rects`] entry with the focus ring to get the same.
    pub focused_link: Option<usize>,
}

impl Deref for LinkLabel {
    type Target = kc::LinkLabel;
    fn deref(&self) -> &kc::LinkLabel {
        &self.inner
    }
}
impl DerefMut for LinkLabel {
    fn deref_mut(&mut self) -> &mut kc::LinkLabel {
        &mut self.inner
    }
}

/// The Kubuno colour for each of the replica's four link cases.
///
/// * `Normal` → `accent`. The palette's own note on `link_visited` says the
///   accent is what covers the unvisited state.
/// * `Active` → `accent_hover` (`--color-primary-hover`), the pressed step the
///   web already publishes; IE's red has no place in this design system.
/// * `Visited` → `link_visited`, the token added for exactly this.
/// * `Disabled` → `text_tertiary`, like every other inert text here.
///
/// No colour literal: a link that hard-coded blue is what the token was created
/// to prevent.
pub fn link_colour(paint: LinkPaint, t: &Theme) -> D2D1_COLOR_F {
    match paint {
        LinkPaint::Normal => t.accent,
        LinkPaint::Active => t.accent_hover,
        LinkPaint::Visited => t.link_visited,
        LinkPaint::Disabled => t.text_tertiary,
    }
}

/// Metrics of the link's focus ring — `focus-visible:ring-2
/// focus-visible:ring-primary rounded-sm` on the web's doc link
/// (`ui/EmptyState.tsx`).
mod link_metrics {
    /// `ring-2`: a 2 px box-shadow ring drawn OUTSIDE the element's box.
    pub const FOCUS_RING: f32 = 2.0;
    /// `rounded-sm` — the element's own corner; the ring's outer corner is
    /// this plus the ring width, as a spread box-shadow's is.
    pub const RADIUS: f32 = crate::metrics::radius::SM;
}

impl LinkLabel {
    /// A body-sized link. Underlined on **hover only** — the web's
    /// `hover:underline` — rather than WinForms' always-underlined
    /// `SystemDefault`; set `link_behavior` back to taste.
    pub fn new(text: impl Into<String>) -> Self {
        let mut inner = kc::LinkLabel::new();
        inner.text = text.into();
        inner.link_behavior = kc::LinkBehavior::HoverUnderline;
        Self { inner, role: Role::default(), hot_link: None, focused_link: None }
    }

    /// Builder: the link under the pointer (see [`LinkLabel::hot_link`]).
    pub fn hot_link(mut self, i: Option<usize>) -> Self {
        self.hot_link = i;
        self
    }

    /// Builder: the link holding the focus (see [`LinkLabel::focused_link`]).
    pub fn focused_link(mut self, i: Option<usize>) -> Self {
        self.focused_link = i;
        self
    }

    /// Each resolved link's text span, in the caller's space, for a label
    /// painted into `bounds` — what a page hit-tests, registers with the focus
    /// ring and sets the `Hand` cursor over. Clipped to `bounds`, like the
    /// paint.
    pub fn link_rects(&self, canvas: &dyn Canvas, bounds: Rect) -> Vec<Rect> {
        let shown = self.inner.shown_text();
        let chars: Vec<char> = shown.chars().collect();
        let content = self.content(bounds);
        let band = self.band(bounds);
        let fmt = self.role.format(canvas.formats());
        self.inner
            .resolved_links()
            .iter()
            .map(|link| {
                let start = (link.start.max(0) as usize).min(chars.len());
                let end = ((link.start + link.length).max(0) as usize).min(chars.len()).max(start);
                let prefix: String = chars[..start].iter().collect();
                let piece: String = chars[start..end].iter().collect();
                let x0 = (content.left + canvas.measure(&prefix, fmt)).min(bounds.right);
                let x1 = (x0 + canvas.measure(&piece, fmt)).min(bounds.right);
                Rect::new(x0, band.top, x1, band.bottom)
            })
            .collect()
    }

    /// The enabled link under `(x, y)` — in the same space as `bounds` — as an
    /// index into `resolved_links()`. Disabled links never match, exactly as
    /// the toolkit refuses to raise `LinkClicked` for them.
    pub fn link_at(&self, canvas: &dyn Canvas, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        if !self.inner.enabled {
            return None;
        }
        let links = self.inner.resolved_links();
        self.link_rects(canvas, bounds)
            .iter()
            .zip(links.iter())
            .position(|(r, l)| l.enabled && r.contains(x, y))
    }

    pub fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }

    /// Marks the (single, implicit) link visited — the replica's `LinkVisited`.
    pub fn visited(mut self, v: bool) -> Self {
        self.inner.link_visited = v;
        self
    }

    pub fn content(&self, rect: Rect) -> Rect {
        content_rect(rect, self.inner.border_style, self.inner.padding)
    }

    pub fn band(&self, rect: Rect) -> Rect {
        text_band(self.content(rect), self.inner.text_align, self.role.line_height())
    }
}

impl Widget for LinkLabel {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let shown = self.inner.shown_text();
        let width = text_width(canvas, &shown, self.role.format(canvas.formats()));
        let extent = Size::new(width, self.role.line_height());
        self.inner.clamp(label_size(extent, self.inner.padding, self.inner.border_style))
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        if let Some(bg) = self.inner.back_color {
            canvas.fill_rounded(&bounds, radius::SM, &bg);
        }
        if self.inner.border_style != BorderStyle::None {
            canvas.stroke_rounded(&bounds, radius::SM, &t.card_stroke);
        }

        let shown = self.inner.shown_text();
        if shown.is_empty() {
            return;
        }
        let content = self.content(bounds);
        let band = self.band(bounds);
        let line_h = self.role.line_height();
        let fmt = self.role.format(canvas.formats());

        // The focus ring first, so the text sits on top of it: `ring-2`
        // around the focused link's inline box. The box is the text span
        // widened by the ring on both sides and the full line box tall (a
        // 12 px run's content box plus the 2 px ring fills the 20 px line
        // exactly as the browser's does). Like a CSS ring it is decoration,
        // not layout: it may reach `FOCUS_RING` past a label measured without
        // padding.
        if state.show_focus_ring() && !is_inert(&self.inner.label.control, state) {
            let rects = self.link_rects(canvas, bounds);
            let i = self.focused_link.unwrap_or(0);
            if let Some(r) = rects.get(i) {
                let ring = Rect::new(
                    r.left - link_metrics::FOCUS_RING,
                    band.top,
                    r.right + link_metrics::FOCUS_RING,
                    band.bottom,
                );
                canvas.stroke_rounded_w(
                    &ring,
                    link_metrics::RADIUS + link_metrics::FOCUS_RING,
                    &t.accent,
                    link_metrics::FOCUS_RING,
                );
            }
        }

        // Text never leaves the box (the ring above is the one exception).
        canvas.push_clip(&Rect::new(
            bounds.left,
            bounds.top.min(band.top),
            bounds.right,
            bounds.bottom.max(band.bottom),
        ));

        // The non-link run first, in ordinary label colour. Leading, not
        // `text_align`'s horizontal third: the per-link offsets below are
        // measured prefix widths from `content.left`, so the two passes only
        // line up if the base run starts there too. That is the replica's own
        // compromise, kept rather than re-invented.
        let base = if is_inert(&self.inner.label.control, state) {
            inert(t)
        } else {
            self.inner.fore_color.unwrap_or(t.text_primary)
        };
        canvas.text_aligned(&shown, &band, fmt, &base, DWRITE_TEXT_ALIGNMENT_LEADING);

        // …then every link span overpainted in its own colour. Both decisions
        // — which colour, and whether to underline — are the replica's pure
        // functions, fed the Kubuno state converted to a `ControlState`.
        let whole: kubuno_desktop_controls::ControlState = state.into();
        let chars: Vec<char> = shown.chars().collect();
        for (i, link) in self.inner.resolved_links().into_iter().enumerate() {
            // Per-link hover/press when the caller resolved the hot link;
            // the replica's whole-control reading otherwise.
            let control_state = match self.hot_link {
                Some(hot) if hot != i => kubuno_desktop_controls::ControlState { hot: false, pressed: false, ..whole },
                _ => whole,
            };
            let start = link.start.max(0) as usize;
            let end = ((link.start + link.length).max(0) as usize).min(chars.len());
            if start >= end {
                continue;
            }
            let prefix: String = chars[..start].iter().collect();
            let piece: String = chars[start..end].iter().collect();
            // EXACT widths here, not the ceiled `text_width`: these are
            // offsets INSIDE a run that has already been laid out, and
            // rounding each prefix up would walk the overpaint a pixel further
            // right with every link.
            let x0 = content.left + canvas.measure(&prefix, fmt);
            let w = canvas.measure(&piece, fmt);
            let piece_rect = Rect::new(x0, band.top, x0 + w, band.bottom);

            let mut colour = link_colour(self.inner.link_paint(&link, control_state), t);
            if is_inert(&self.inner.label.control, state) {
                colour = inert(t);
            }
            canvas.text_aligned(&piece, &piece_rect, fmt, &colour, DWRITE_TEXT_ALIGNMENT_LEADING);

            if self.inner.underlines_link(&link, control_state) && !state.disabled {
                // Offset and thickness are the replica's: 12 % of the line box
                // above the baseline band's foot, one DIP thick. Copying them
                // keeps a Kubuno link and a system link sitting on the same
                // rule rather than two rules a pixel apart.
                let uy = band.bottom - line_h * 0.12;
                canvas.fill_rounded(&Rect::new(x0, uy, x0 + w, uy + 1.0), 0.0, &colour);
            }
        }
        canvas.pop_clip();
    }

    fn type_name(&self) -> &'static str {
        "LinkLabel"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Badge
// ─────────────────────────────────────────────────────────────────────────────

/// A badge's tone. One-to-one with `@ui/Badge`'s `BadgeVariant`, names
/// included, so the two cannot drift: `default` is the quiet neutral pill,
/// `neutral` the louder one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BadgeVariant {
    /// `bg-surface-2 text-text-secondary` — the quiet default.
    #[default]
    Default,
    /// `bg-primary-light text-primary`.
    Primary,
    /// `bg-success-light text-success`.
    Success,
    /// `bg-warning-light text-warning`.
    Warning,
    /// `bg-danger-light text-danger`.
    Danger,
    /// `bg-surface-3 text-text-primary` — the loudest neutral.
    Neutral,
}

/// Every variant, in the order `@ui/Badge` declares them.
pub const BADGE_VARIANTS: [BadgeVariant; 6] = [
    BadgeVariant::Default,
    BadgeVariant::Primary,
    BadgeVariant::Success,
    BadgeVariant::Warning,
    BadgeVariant::Danger,
    BadgeVariant::Neutral,
];

impl BadgeVariant {
    /// `(ground, text)` — the two classes the web pairs, as tokens.
    pub fn colours(self, t: &Theme) -> (D2D1_COLOR_F, D2D1_COLOR_F) {
        match self {
            Self::Default => (t.surface_2, t.text_secondary),
            Self::Primary => (t.accent_light, t.accent),
            Self::Success => (t.success_light, t.success),
            Self::Warning => (t.warning_light, t.warning),
            Self::Danger => (t.danger_light, t.danger),
            Self::Neutral => (t.surface_3, t.text_primary),
        }
    }

    /// The leading dot's colour — `dotVariants` in `@ui/Badge`, which is a
    /// *different* map from the text colour (the dot is the saturated hue even
    /// when the label is not).
    pub fn dot_colour(self, t: &Theme) -> D2D1_COLOR_F {
        match self {
            Self::Default => t.text_tertiary,
            Self::Primary => t.accent,
            Self::Success => t.success,
            Self::Warning => t.warning,
            Self::Danger => t.danger,
            Self::Neutral => t.text_secondary,
        }
    }
}

/// The two sizes `@ui/Badge` ships.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BadgeSize {
    /// `text-[10px] px-1.5 py-0.5`.
    Sm,
    /// `text-xs px-2 py-0.5` — the default.
    #[default]
    Md,
}

impl BadgeSize {
    /// Horizontal padding: `px-1.5` = 6, `px-2` = 8 (`space::SM`).
    pub const fn pad_x(self) -> f32 {
        match self {
            Self::Sm => 6.0,
            Self::Md => space::SM,
        }
    }
}

/// A counting / status pill — `@ui/Badge`.
///
/// Its model is a [`kubuno_desktop_controls::labels::Label`], so `text`, `padding`,
/// `enabled`, `bounds`, `dock` and `anchor` are the replica's and a badge drops
/// into the same layout pass as anything else.
pub struct Badge {
    inner: kc::Label,
    pub variant: BadgeVariant,
    pub size: BadgeSize,
    /// The leading dot (`dot` in the web component).
    pub dot: bool,
    /// The widest the pill may grow, in DIP — the `max-w-* truncate` a web
    /// caller adds through `className`. Past it (or whenever the caller's
    /// rectangle is narrower than the text) the label ends in « … » inside
    /// the pill instead of running past it. `None` = no cap.
    pub max_width: Option<f32>,
}

impl Deref for Badge {
    type Target = kc::Label;
    fn deref(&self) -> &kc::Label {
        &self.inner
    }
}
impl DerefMut for Badge {
    fn deref_mut(&mut self) -> &mut kc::Label {
        &mut self.inner
    }
}

impl Badge {
    /// Vertical padding — `py-0.5`, i.e. 2 (`space::XXS`).
    pub const PAD_Y: f32 = space::XXS;
    /// The dot's diameter — `h-1.5 w-1.5`.
    pub const DOT: f32 = 6.0;
    /// The gap between the dot and the label — `gap-1` (`space::XS`).
    pub const DOT_GAP: f32 = space::XS;
    /// The line box a badge's text occupies — `text-xs`, so
    /// [`Role::Meta::line_height`](Role::line_height).
    pub const LINE: f32 = Role::Meta.line_height();

    pub fn new(text: impl Into<String>) -> Self {
        let mut inner = kc::Label::new();
        inner.text = text.into();
        // A pill's label is centred in it, not top-left like a bare label.
        inner.text_align = ContentAlignment::MiddleCenter;
        Self {
            inner,
            variant: BadgeVariant::default(),
            size: BadgeSize::default(),
            dot: false,
            max_width: None,
        }
    }

    /// Builder: the widest the pill may grow (see [`Badge::max_width`]).
    pub fn max_width(mut self, w: f32) -> Self {
        self.max_width = Some(w);
        self
    }

    pub fn variant(mut self, v: BadgeVariant) -> Self {
        self.variant = v;
        self
    }

    pub fn size(mut self, s: BadgeSize) -> Self {
        self.size = s;
        self
    }

    pub fn dot(mut self, d: bool) -> Self {
        self.dot = d;
        self
    }
}

/// A badge's intrinsic size for a measured text width — `inline-flex` with
/// `gap-1`, so: padding, an optional dot and its gap, then the label.
///
/// The height is the same for both sizes on purpose: the web's `sm` is
/// `text-[10px]`, and the shared format table carries no face below 12, so the
/// two sizes differ by their horizontal padding only (6 vs 8) and share the
/// `text-xs` line box. That is a documented collapse, not a rounding.
pub fn badge_size(text_width: f32, size: BadgeSize, dot: bool) -> Size {
    let lead = if dot { Badge::DOT + Badge::DOT_GAP } else { 0.0 };
    Size::new(
        size.pad_x() * 2.0 + lead + text_width,
        Badge::LINE + Badge::PAD_Y * 2.0,
    )
}

/// The width a badge's label has inside a pill `pill_width` wide: the pill
/// minus both paddings and the dot group. Never negative.
pub fn badge_text_room(pill_width: f32, size: BadgeSize, dot: bool) -> f32 {
    let lead = if dot { Badge::DOT + Badge::DOT_GAP } else { 0.0 };
    (pill_width - size.pad_x() * 2.0 - lead).max(0.0)
}

impl Widget for Badge {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let shown = self.inner.shown_text();
        // `caption_strong` and not `Role::Meta.format`: the web pill is
        // `font-medium`, and the medium 12 is a separate format.
        let w = text_width(canvas, &shown, &canvas.formats().caption_strong);
        let mut s = badge_size(w, self.size, self.dot);
        if let Some(cap) = self.max_width {
            // Never below the empty pill: the paddings and the dot stay whole.
            s.width = s.width.min(cap.max(badge_size(0.0, self.size, self.dot).width));
        }
        self.inner.clamp(s)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        let (ground, ink) = self.variant.colours(t);
        let height = bounds.bottom - bounds.top;

        // `rounded-full`: the radius is half the height, whatever the caller
        // made it — a pill that kept a fixed radius stops being a pill the
        // moment the row grows.
        canvas.fill_rounded(&bounds, pill(height), &ground);

        let shown = self.inner.shown_text();
        let fmt = &canvas.formats().caption_strong;
        let lead = if self.dot { Self::DOT + Self::DOT_GAP } else { 0.0 };
        // The label never gets more than the pill leaves it: a badge capped by
        // `max_width`, or squeezed by its caller, truncates INSIDE the pill.
        let room = badge_text_room(bounds.right - bounds.left, self.size, self.dot);
        let natural = text_width(canvas, &shown, fmt);
        let truncated = natural > room + FIT_EPSILON;
        let text_w = natural.min(room);

        // The content group is centred in `bounds`. When the caller sized the
        // badge from `measure` this is identical to laying it out from the left
        // padding; when the caller made it wider (a fixed-width counter column)
        // it stays centred, which is what `inline-flex items-center` reads as.
        let group = lead + text_w;
        let mut x = bounds.left + ((bounds.right - bounds.left) - group) / 2.0;
        let mid = (bounds.top + bounds.bottom) / 2.0;

        if self.dot {
            let dot = Rect::new(x, mid - Self::DOT / 2.0, x + Self::DOT, mid + Self::DOT / 2.0);
            let colour = if is_inert(&self.inner.control, state) {
                inert(t)
            } else {
                self.variant.dot_colour(t)
            };
            canvas.fill_rounded(&dot, pill(Self::DOT), &colour);
            x += Self::DOT + Self::DOT_GAP;
        }

        if shown.is_empty() {
            return;
        }
        let ink = if is_inert(&self.inner.control, state) { inert(t) } else { ink };
        let band = Rect::new(x, mid - Self::LINE / 2.0, x + text_w, mid + Self::LINE / 2.0);
        // Clipped to the pill whatever happens, so nothing reaches the card
        // border beside it.
        canvas.push_clip(&bounds);
        if truncated {
            canvas.text_ellipsis(&shown, &band, fmt, &ink);
        } else {
            canvas.text_aligned(&shown, &band, fmt, &ink, DWRITE_TEXT_ALIGNMENT_LEADING);
        }
        canvas.pop_clip();
    }

    /// A pill is a rounded rectangle, so its ends are not clickable at the
    /// corners. The test is the capsule: inside the straight middle, or inside
    /// one of the two end discs.
    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        let r = (bounds.bottom - bounds.top) / 2.0;
        if !bounds.contains(x, y) {
            return false;
        }
        let cx = x.clamp(bounds.left + r, (bounds.right - r).max(bounds.left + r));
        let cy = (bounds.top + bounds.bottom) / 2.0;
        (x - cx).powi(2) + (y - cy).powi(2) <= r * r
    }

    fn type_name(&self) -> &'static str {
        "Badge"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Icon
// ─────────────────────────────────────────────────────────────────────────────

/// Names the web uses for a glyph that the embedded geometry files carry under
/// another name — consulted only when a name is NOT embedded itself. Nothing
/// here maps a missing icon to a merely similar one: an entry is a rename of the
/// very same glyph, and its target must be embedded (the tests check it).
///
/// Empty today: the embedded set carries both spellings of every lucide rename
/// the web uses (`lucide-all.txt` the new names, `lucide-icons.txt` the old ones;
/// see `RENAMED_PAIRS` in the tests). Add an entry only when a web name stops
/// resolving; a name in neither the files nor this table paints the
/// missing-glyph box.
pub const ICON_ALIASES: [(&str, &str); 0] = [];

/// The embedded geometry name that draws `name`: the name itself when a file
/// carries it, else its lucide alias ([`ICON_ALIASES`]), else `None`.
///
/// Cached per name — the lookup scans the embedded geometry files, which is
/// too slow to repeat for every icon of every frame.
pub fn resolve_icon(name: &'static str) -> Option<&'static str> {
    use std::collections::HashMap;
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<&'static str, Option<&'static str>>> =
            std::cell::RefCell::new(HashMap::new());
    }
    CACHE.with(|c| {
        *c.borrow_mut().entry(name).or_insert_with(|| {
            kubuno_drive_desktop_app_controls::icon_name(name).or_else(|| {
                ICON_ALIASES
                    .iter()
                    .find(|(alias, _)| *alias == name)
                    .and_then(|(_, target)| kubuno_drive_desktop_app_controls::icon_name(target))
            })
        })
    })
}

/// Says once per name, in debug builds, that an icon has no geometry — the
/// diagnostic a silently blank glyph never gave. Release builds stay quiet:
/// the visible placeholder box is the user-facing signal.
fn report_missing_icon(name: &'static str) {
    #[cfg(debug_assertions)]
    {
        use std::collections::HashSet;
        thread_local! {
            static SEEN: std::cell::RefCell<HashSet<&'static str>> =
                std::cell::RefCell::new(HashSet::new());
        }
        SEEN.with(|s| {
            if s.borrow_mut().insert(name) {
                eprintln!(
                    "kubuno-desktop-ui: icon « {name} » has no geometry in themed-icons.txt, \
                     lucide-icons.txt or module-logos.txt (drawn as a placeholder box)"
                );
            }
        });
    }
    #[cfg(not(debug_assertions))]
    let _ = name;
}

/// The missing-glyph placeholder's geometry, as fractions of the glyph box.
mod icon_metrics {
    /// The box is 70 % of the glyph square — the proportion of a font's
    /// `.notdef` rectangle to its em, so it reads as « a character goes here »
    /// rather than as a checkbox.
    pub const MISSING_BOX: f32 = 0.7;
    /// Lucide's own stroke weight at the default 20 DIP box (2 on a 24 grid
    /// is ~1.5 at 20), so the placeholder weighs what the real glyph would.
    pub const MISSING_STROKE: f32 = 1.5;
    /// A small rounding, as lucide's squares carry (`rx="2"` on a 24 grid).
    pub const MISSING_RADIUS: f32 = 2.0;
}

/// A vector glyph — a [`kubuno_desktop_controls::labels::PictureBox`] whose picture is
/// a named geometry instead of a bitmap.
///
/// RULE: a glyph is **geometry, not text**. Arrows, chevrons and ticks drawn as
/// characters come out as tofu — the embedded face does not carry them — so
/// this paints through [`Canvas::vector_icon`] with a name from
/// `kubuno-drive-desktop-app-controls/assets/*.txt` and never through a text call.
///
/// Everything else is the replica's, and that is the point: the *size* is
/// `PictureBox::image` (the picture's natural size — so there is no second
/// storage for it), and the *placement* is `PictureBox::size_mode` resolved by
/// the replica's own [`kubuno_desktop_controls::labels::image_rect`], which already
/// implements all five modes and their letter-boxing.
pub struct Icon {
    inner: kc::PictureBox,
    /// The geometry's name, as it appears in `themed-icons.txt`,
    /// `lucide-icons.txt` or `module-logos.txt`.
    pub name: &'static str,
}

impl Deref for Icon {
    type Target = kc::PictureBox;
    fn deref(&self) -> &kc::PictureBox {
        &self.inner
    }
}
impl DerefMut for Icon {
    fn deref_mut(&mut self) -> &mut kc::PictureBox {
        &mut self.inner
    }
}

impl Icon {
    /// The default glyph box, in DIP.
    ///
    /// 20 is what the web's own navigation draws at — `core/frontend/src/core/
    /// shell/Sidebar.tsx` renders every module glyph as `<Icon size={20} />`.
    /// The other two sizes that appear there (16 for the collapse chevron, 18
    /// for the « new » plus) are per-call, not a default.
    pub const DEFAULT: f32 = 20.0;

    /// An icon at [`Icon::DEFAULT`], sized to its glyph (`AutoSize`).
    pub fn new(name: &'static str) -> Self {
        Self::sized(name, Self::DEFAULT)
    }

    /// An icon at an explicit box, in DIP.
    pub fn sized(name: &'static str, px: f32) -> Self {
        let mut inner = kc::PictureBox::new();
        // The natural size lives on the replica — one storage location, and it
        // is what `image_rect` and `preferred_size` both read.
        inner.image = Some(Size::new(px, px));
        inner.size_mode = kc::PictureBoxSizeMode::AutoSize;
        Self { inner, name }
    }

    /// Builder: the glyph's colour. `None` (the default) means the ambient
    /// `text_secondary`, which is what an unaccented Kubuno glyph is.
    pub fn tint(mut self, colour: Option<D2D1_COLOR_F>) -> Self {
        self.inner.fore_color = colour;
        self
    }

    /// The glyph's natural box; [`Icon::DEFAULT`] square when unset.
    pub fn natural(&self) -> Size {
        self.inner.image.unwrap_or(Size::new(Self::DEFAULT, Self::DEFAULT))
    }

    /// Whether a geometry exists for this icon's name (directly or through a
    /// lucide alias). `false` means it paints the missing-glyph box.
    pub fn is_known(&self) -> bool {
        resolve_icon(self.name).is_some()
    }

    /// Where the glyph lands inside a caller rectangle — the replica's own
    /// size-mode geometry, applied to the content rectangle.
    pub fn glyph_rect(&self, rect: Rect) -> Rect {
        let client = content_rect(rect, self.inner.border_style, self.inner.padding);
        kc::image_rect(self.inner.size_mode, client, self.natural())
    }
}

impl Widget for Icon {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, _canvas: &dyn Canvas) -> Size {
        let frame = border_inset(self.inner.border_style) * 2.0;
        let n = self.natural();
        let p = self.inner.padding;
        self.inner.clamp(Size::new(
            n.width + p.horizontal() + frame,
            n.height + p.vertical() + frame,
        ))
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        if let Some(bg) = self.inner.back_color {
            canvas.fill_rounded(&bounds, radius::SM, &bg);
        }
        if self.inner.border_style != BorderStyle::None {
            canvas.stroke_rounded(&bounds, radius::SM, &t.card_stroke);
        }

        let colour = if is_inert(&self.inner.control, state) {
            inert(t)
        } else {
            self.inner.fore_color.unwrap_or(t.text_secondary)
        };
        let r = self.glyph_rect(bounds);
        // `vector_icon` draws ONE square of `size`, centred in the rectangle it
        // is given; a non-square placement (Stretch, Zoom on a flat box) is
        // therefore fitted to its shorter side rather than distorted, since the
        // canvas has no non-uniform glyph scale.
        let size = (r.right - r.left).min(r.bottom - r.top);
        if size <= 0.0 {
            return;
        }
        match resolve_icon(self.name) {
            Some(name) => canvas.vector_icon(name, &r, size, &colour),
            None => {
                // No geometry under that name: a missing glyph used to paint
                // NOTHING, which read as a layout bug (an empty gutter before
                // an accordion title, a button with no icon). Paint the
                // font world's `.notdef` box instead — visibly « a glyph
                // belongs here » — and say which name is missing.
                report_missing_icon(self.name);
                let cx = (r.left + r.right) / 2.0;
                let cy = (r.top + r.bottom) / 2.0;
                let half = size * icon_metrics::MISSING_BOX / 2.0;
                let tofu = Rect::new(cx - half, cy - half, cx + half, cy + half);
                canvas.stroke_rounded_w(&tofu, icon_metrics::MISSING_RADIUS, &colour, icon_metrics::MISSING_STROKE);
            }
        }
    }

    fn type_name(&self) -> &'static str {
        "Icon"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tooltip
// ─────────────────────────────────────────────────────────────────────────────

/// Which side of the anchor a tooltip sits on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Side {
    Top,
    #[default]
    Bottom,
    Left,
    Right,
}

/// Every side, for the gallery and the tests.
pub const SIDES: [Side; 4] = [Side::Top, Side::Bottom, Side::Left, Side::Right];

impl Side {
    /// The side a tooltip flips to when this one has no room.
    pub const fn opposite(self) -> Self {
        match self {
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Top,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    /// Whether the bubble is displaced along Y (`Top`/`Bottom`) rather than X.
    pub const fn is_vertical(self) -> bool {
        matches!(self, Self::Top | Self::Bottom)
    }
}

/// Where a tooltip ended up: its rectangle, the side it settled on, and the tip
/// of its arrow (a point ON the bubble's anchor-facing edge).
///
/// Not `Debug`/`PartialEq`: [`Rect`] is neither, and giving this one a
/// hand-written pair would be a second definition of what "the same rectangle"
/// means.
#[derive(Clone, Copy)]
pub struct Placement {
    pub rect: Rect,
    pub side: Side,
    /// Where the arrow meets the bubble: the centre of its **base**, on the
    /// edge facing the anchor. The apex is [`Tooltip::ARROW`] further along, in
    /// the anchor's direction — kept implicit so a caller cannot place the two
    /// inconsistently.
    pub tip: (f32, f32),
}

/// The bubble — `@ui/Tooltip`'s `TOOLTIP_STYLE`, with an arrow the web has not
/// got.
///
/// Its model is an empty-bordered [`kubuno_desktop_controls::labels::Label`], so `text`,
/// `padding` and `enabled` are the replica's. A `ToolTip` is a *component* in
/// WinForms, not a control, so there is no replica to port — only a property
/// surface to borrow.
///
/// TRAP: the token pair `tooltip_background` / `tooltip_foreground` is
/// **identical in both palettes**. That is not an oversight in the theme: the
/// web ships exactly one tooltip style and no dark variant, so the bubble reads
/// as an overlay rather than as a surface. Do not "fix" it by switching on
/// `Theme::mode`.
pub struct Tooltip {
    inner: kc::Label,
    /// The side the caller prefers. Honoured when it fits; see [`place`].
    pub side: Side,
}

impl Deref for Tooltip {
    type Target = kc::Label;
    fn deref(&self) -> &kc::Label {
        &self.inner
    }
}
impl DerefMut for Tooltip {
    fn deref_mut(&mut self) -> &mut kc::Label {
        &mut self.inner
    }
}

impl Tooltip {
    /// Distance from the anchor to the bubble — `TOOLTIP_GAP` in
    /// `ui/tooltipPlacement.ts`, where it is documented as « clears the mouse
    /// cursor itself ». The arrow lives inside this gap.
    pub const GAP: f32 = 14.0;
    /// Keep-off from the viewport edges — `MARGIN` in the same file.
    pub const MARGIN: f32 = space::SM;
    /// `maxWidth: 280` in `TOOLTIP_STYLE`.
    pub const MAX_WIDTH: f32 = 280.0;
    /// `padding: '6px 10px'` in `TOOLTIP_STYLE`.
    pub const PAD_X: f32 = 10.0;
    pub const PAD_Y: f32 = 6.0;
    /// `lineHeight: '16px'`, i.e. [`Role::Meta`]'s line box.
    pub const LINE: f32 = Role::Meta.line_height();
    /// The widest a line of text may be: `maxWidth: 280` minus both paddings,
    /// because the web's preflight makes every box `border-box` — the 280
    /// includes the padding.
    pub const TEXT_MAX: f32 = Self::MAX_WIDTH - 2.0 * Self::PAD_X;
    /// `delay = 400` — how long the pointer must rest on the trigger before
    /// the bubble appears (`Tooltip.tsx`). See [`TooltipTrigger`].
    pub const DELAY_MS: u64 = 400;

    /// The arrow's height, from the bubble edge to its tip.
    ///
    /// **NO WEB SOURCE.** The web tooltip is a plain rounded rectangle with no
    /// arrow at all, so there is nothing to copy: 6 is chosen to be smaller
    /// than [`Tooltip::GAP`] (so the tip never touches the anchor) and no
    /// larger than the dot the badge draws, which is the smallest shape this
    /// design system otherwise paints.
    pub const ARROW: f32 = 6.0;
    /// Half the arrow's base. Equal to [`Tooltip::ARROW`], i.e. a right-angled
    /// tip — also unsourced, for the same reason.
    pub const ARROW_HALF: f32 = 6.0;

    pub fn new(text: impl Into<String>) -> Self {
        let mut inner = kc::Label::new();
        inner.text = text.into();
        Self { inner, side: Side::default() }
    }

    pub fn side(mut self, side: Side) -> Self {
        self.side = side;
        self
    }

    /// The lines the bubble shows at its natural width — `pre-line` wrapping
    /// at [`Tooltip::TEXT_MAX`].
    pub fn lines(&self, canvas: &dyn Canvas) -> Vec<String> {
        let fmt = &canvas.formats().caption_strong;
        wrap_lines(&self.inner.shown_text(), Self::TEXT_MAX, &mut |s| canvas.measure(s, fmt))
    }

    /// Places this bubble the WEB way — below the pointer, left-aligned with
    /// it — inside `viewport` (any rectangle: pass `Frame::screen_area()` so
    /// the bubble may leave the window). Thin wrapper over [`place_at_pointer`].
    pub fn place_at_pointer(&self, canvas: &dyn Canvas, pointer: (f32, f32), viewport: Rect) -> PointerPlacement {
        place_at_pointer(pointer, self.measure(canvas), viewport)
    }

    /// Places this bubble against `anchor` in a `viewport`, at its measured
    /// size. Thin wrapper over the pure [`place`].
    pub fn place(&self, canvas: &dyn Canvas, anchor: Rect, viewport: Size) -> Placement {
        place(anchor, self.measure(canvas), self.side, viewport)
    }

    /// Paints the bubble **and its arrow** at a computed placement.
    ///
    /// [`Widget::paint`] draws only the bubble, because the `Widget` contract
    /// knows a rectangle and not which side of what it is on; a caller that has
    /// a [`Placement`] uses this one.
    pub fn paint_placed(&self, canvas: &dyn Canvas, p: &Placement, state: WidgetState) {
        self.paint(canvas, p.rect, state);
        self.paint_arrow(canvas, p);
    }

    /// The arrow: one anti-aliased triangle through [`Canvas::fill_triangle`].
    ///
    /// It used to be rasterised here as a stack of one-pixel bands — a hard
    /// staircase that read as jagged and, carrying no shadow of its own, looked
    /// bare and detached on the side the bubble's offset drop shadow leaves
    /// ungrounded (the top). A single filled path is smooth and identical on
    /// every side.
    fn paint_arrow(&self, canvas: &dyn Canvas, p: &Placement) {
        let colour = canvas.theme().tooltip_background;
        let (tx, ty) = p.tip;
        let a = Self::ARROW;
        let h = Self::ARROW_HALF;
        // The two base corners sit on the bubble's anchor-facing edge, the apex
        // `ARROW` beyond it towards the anchor. `LIP` pushes the base a hair
        // back INTO the bubble so the two same-colour fills overlap and cannot
        // leave a hairline seam where they meet.
        const LIP: f32 = 0.75;
        let (base_a, base_b, apex) = match p.side {
            // Bubble BELOW the anchor → the arrow rises off its top edge.
            Side::Bottom => ((tx - h, ty + LIP), (tx + h, ty + LIP), (tx, ty - a)),
            // Bubble ABOVE → the arrow hangs off its bottom edge.
            Side::Top => ((tx - h, ty - LIP), (tx + h, ty - LIP), (tx, ty + a)),
            // Bubble LEFT of the anchor → the arrow points right.
            Side::Left => ((tx - LIP, ty - h), (tx - LIP, ty + h), (tx + a, ty)),
            // Bubble RIGHT → the arrow points left.
            Side::Right => ((tx + LIP, ty - h), (tx + LIP, ty + h), (tx - a, ty)),
        };
        canvas.fill_triangle(base_a, base_b, apex, &colour);
    }
}

/// Where a bubble of `size` goes against `anchor` in `viewport`, preferring
/// `preferred`.
///
/// The rules, and where each comes from:
///
/// 1. **Preferred side if it fits**, else the opposite, else whichever has the
///    most room. `ui/tooltipPlacement.ts` does the same flip on one axis
///    (« above the pointer when the bottom edge is too close »); this
///    generalises it to four sides because a desktop tooltip is anchored to a
///    *control*, not to the pointer, and a toolbar at the window edge needs
///    left/right too.
/// 2. **Gap** = [`Tooltip::GAP`], **viewport keep-off** = [`Tooltip::MARGIN`],
///    both verbatim from that file.
/// 3. **Centred on the anchor**, then clamped into the viewport — the same
///    « pull back only if it would overflow » the web does on x.
/// 4. The **arrow's base** follows the anchor's centre but is kept on the
///    bubble's straight edge: no closer to a corner than the corner radius plus
///    half the base, so the triangle never grows out of a rounded corner.
///
/// Pure: no canvas, no window — which is what makes each of the four edges a
/// unit test instead of a screenshot.
pub fn place(anchor: Rect, size: Size, preferred: Side, viewport: Size) -> Placement {
    let room = |s: Side| match s {
        Side::Top => anchor.top - Tooltip::MARGIN,
        Side::Bottom => viewport.height - anchor.bottom - Tooltip::MARGIN,
        Side::Left => anchor.left - Tooltip::MARGIN,
        Side::Right => viewport.width - anchor.right - Tooltip::MARGIN,
    };
    let need = |s: Side| {
        Tooltip::GAP + if s.is_vertical() { size.height } else { size.width }
    };

    let side = if room(preferred) >= need(preferred) {
        preferred
    } else if room(preferred.opposite()) >= need(preferred.opposite()) {
        preferred.opposite()
    } else {
        // Neither fits: take the roomier of the two rather than a third side,
        // so the bubble stays on the axis the caller asked for.
        let (a, b) = (preferred, preferred.opposite());
        if room(a) >= room(b) {
            a
        } else {
            b
        }
    };

    // Cross-axis: centred on the anchor, then clamped inside the margins. The
    // `max` guards a viewport narrower than the bubble, where the two clamps
    // would otherwise cross.
    let clamp_cross = |centre: f32, extent: f32, limit: f32| {
        let lo = Tooltip::MARGIN;
        let hi = (limit - extent - Tooltip::MARGIN).max(lo);
        (centre - extent / 2.0).clamp(lo, hi)
    };

    let rect = match side {
        Side::Top => {
            let left =
                clamp_cross((anchor.left + anchor.right) / 2.0, size.width, viewport.width);
            let bottom = anchor.top - Tooltip::GAP;
            Rect::new(left, bottom - size.height, left + size.width, bottom)
        }
        Side::Bottom => {
            let left =
                clamp_cross((anchor.left + anchor.right) / 2.0, size.width, viewport.width);
            let top = anchor.bottom + Tooltip::GAP;
            Rect::new(left, top, left + size.width, top + size.height)
        }
        Side::Left => {
            let top =
                clamp_cross((anchor.top + anchor.bottom) / 2.0, size.height, viewport.height);
            let right = anchor.left - Tooltip::GAP;
            Rect::new(right - size.width, top, right, top + size.height)
        }
        Side::Right => {
            let top =
                clamp_cross((anchor.top + anchor.bottom) / 2.0, size.height, viewport.height);
            let left = anchor.right + Tooltip::GAP;
            Rect::new(left, top, left + size.width, top + size.height)
        }
    };

    // The tip sits on the bubble's anchor-facing edge, level with the anchor's
    // centre, but never inside a rounded corner.
    let inset = radius::SM + Tooltip::ARROW_HALF;
    let tip = match side {
        Side::Top => (
            clamp_inside((anchor.left + anchor.right) / 2.0, rect.left, rect.right, inset),
            rect.bottom,
        ),
        Side::Bottom => (
            clamp_inside((anchor.left + anchor.right) / 2.0, rect.left, rect.right, inset),
            rect.top,
        ),
        Side::Left => (
            rect.right,
            clamp_inside((anchor.top + anchor.bottom) / 2.0, rect.top, rect.bottom, inset),
        ),
        Side::Right => (
            rect.left,
            clamp_inside((anchor.top + anchor.bottom) / 2.0, rect.top, rect.bottom, inset),
        ),
    };

    Placement { rect, side, tip }
}

/// Keeps `v` at least `inset` away from both ends of `[lo, hi]`, falling back
/// to the midpoint when the span is too short to hold the inset on both sides.
fn clamp_inside(v: f32, lo: f32, hi: f32, inset: f32) -> f32 {
    if hi - lo <= inset * 2.0 {
        return (lo + hi) / 2.0;
    }
    v.clamp(lo + inset, hi - inset)
}

/// Where a pointer-anchored bubble landed ([`place_at_pointer`]).
#[derive(Clone, Copy)]
pub struct PointerPlacement {
    /// The bubble's rectangle, in the viewport's space.
    pub rect: Rect,
    /// Whether it sits below the pointer (the house rule) or had to flip
    /// above it.
    pub below: bool,
}

/// `placeTooltip` from `ui/tooltipPlacement.ts`, verbatim: the bubble goes
/// [`Tooltip::GAP`] **below the pointer and left-aligned with it**, flips
/// above when the bottom edge is too close, and is pulled back left only when
/// it would run past the right edge — all with [`Tooltip::MARGIN`] off the
/// viewport's edges.
///
/// Unlike [`place`], the viewport is a rectangle rather than a size rooted at
/// the origin, so the monitor's work area (`Frame::screen_area()`, which
/// starts left of and above the client origin) is passed as is.
pub fn place_at_pointer(pointer: (f32, f32), size: Size, viewport: Rect) -> PointerPlacement {
    let (px, py) = pointer;
    let m = Tooltip::MARGIN;
    let below = py + Tooltip::GAP + size.height + m <= viewport.bottom;
    let top = if below { py + Tooltip::GAP } else { py - Tooltip::GAP - size.height };
    let mut left = px;
    if left + size.width + m > viewport.right {
        left = viewport.right - size.width - m;
    }
    if left < viewport.left + m {
        left = viewport.left + m;
    }
    let top = top.max(viewport.top + m);
    PointerPlacement { rect: Rect::new(left, top, left + size.width, top + size.height), below }
}

/// The web tooltip's show/hide timing (`Tooltip.tsx`), as a per-frame state
/// machine a page drives with the pointer:
///
/// * the bubble appears once the pointer has rested on the trigger for the
///   delay ([`Tooltip::DELAY_MS`] by default) — `setTimeout(…, delay)` on
///   `mouseenter`;
/// * it is anchored where the pointer was **when it appeared** and does not
///   chase the pointer afterwards (the web records the point on move, but
///   places the bubble only once);
/// * leaving the trigger hides it and resets the timer (`mouseleave`);
/// * pressing a button on the trigger hides it and keeps it hidden until the
///   pointer leaves (`mousedown` → `hide`, and nothing re-arms it before the
///   next `mouseenter`);
/// * [`TooltipTrigger::dismiss`] does the same for Escape or a lost window
///   activation — the WAI-ARIA tooltip pattern's « Escape dismisses ».
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TooltipTrigger {
    entered_at: Option<u64>,
    shown_at: Option<(f32, f32)>,
    suppressed: bool,
}

/// What a [`TooltipTrigger`] decided for this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TooltipTick {
    /// The pointer position to anchor the bubble at, when it is visible.
    pub show_at: Option<(f32, f32)>,
    /// When still waiting out the delay: how many ms until it elapses — ask
    /// the host for a repaint then (`host::request_repaint_after`), or the
    /// bubble only appears on the next mouse move.
    pub repaint_in_ms: Option<u32>,
}

impl TooltipTrigger {
    pub const fn new() -> Self {
        Self { entered_at: None, shown_at: None, suppressed: false }
    }

    /// Advances one frame. `hovering`: the pointer is over the trigger;
    /// `button_down`: a mouse button is held; `now_ms`: a monotonic clock.
    pub fn update(
        &mut self,
        hovering: bool,
        pointer: (f32, f32),
        button_down: bool,
        now_ms: u64,
        delay_ms: u64,
    ) -> TooltipTick {
        if !hovering {
            *self = Self::new();
            return TooltipTick::default();
        }
        if button_down {
            self.dismiss();
        }
        if self.suppressed {
            return TooltipTick::default();
        }
        let since = *self.entered_at.get_or_insert(now_ms);
        if self.shown_at.is_none() {
            let elapsed = now_ms.saturating_sub(since);
            if elapsed < delay_ms {
                let wait = (delay_ms - elapsed).min(u64::from(u32::MAX)) as u32;
                return TooltipTick { show_at: None, repaint_in_ms: Some(wait) };
            }
            self.shown_at = Some(pointer);
        }
        TooltipTick { show_at: self.shown_at, repaint_in_ms: None }
    }

    /// Hides the bubble until the pointer leaves the trigger and comes back.
    pub fn dismiss(&mut self) {
        self.shown_at = None;
        self.suppressed = true;
    }

    /// Whether the bubble is showing.
    pub fn is_visible(&self) -> bool {
        self.shown_at.is_some()
    }
}

impl Widget for Tooltip {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        // `caption_strong`: the web bubble is 12 px at `fontWeight: 500`.
        // Wrapped at `TEXT_MAX` on words, explicit breaks honoured
        // (`whiteSpace: 'pre-line'`), so a long label grows DOWN instead of
        // being trimmed — the bubble is exactly as wide as its widest line.
        let fmt = &canvas.formats().caption_strong;
        let lines = self.lines(canvas);
        let w = widest(canvas, &lines, fmt).min(Self::TEXT_MAX);
        Size::new(
            w + Self::PAD_X * 2.0,
            lines.len().max(1) as f32 * Self::LINE + Self::PAD_Y * 2.0,
        )
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, _state: WidgetState) {
        let t = canvas.theme();
        // `SHADOW_MENU`, as the family brief asks. The web bubble's own recipe
        // is `0 1px 3px rgba(0,0,0,.3), 0 4px 8px rgba(0,0,0,.15)` — the same
        // two-layer shape in pure black; the desktop uses the shared menu ramp
        // (and its grey) so every floating surface in the app casts one shadow.
        canvas.draw_shadow(&bounds, radius::SM, &SHADOW_MENU, SHADOW_GREY);
        canvas.fill_rounded(&bounds, radius::SM, &t.tooltip_background);

        if self.inner.shown_text().is_empty() {
            return;
        }
        let inner = Rect::new(
            bounds.left + Self::PAD_X,
            bounds.top + Self::PAD_Y,
            bounds.right - Self::PAD_X,
            bounds.bottom - Self::PAD_Y,
        );
        let fmt = &canvas.formats().caption_strong;
        // Re-wrapped to the width actually given, then clamped to the lines
        // the bubble holds: a caller that squeezes the bubble gets « … », never
        // text past its rounded edge.
        let shown = self.inner.shown_text();
        let lines = wrap_lines(&shown, inner.right - inner.left, &mut |s| canvas.measure(s, fmt));
        let (lines, _) = clamp_lines(lines, lines_that_fit(inner.bottom - inner.top, Self::LINE));
        canvas.push_clip(&bounds);
        for (i, line) in lines.iter().enumerate() {
            let top = inner.top + i as f32 * Self::LINE;
            // A line that fits within the wrap tolerance is drawn whole in a
            // band widened by that tolerance — trimming it would turn a
            // sub-DIP rounding difference into a spurious « … ». Only a real
            // overflow (a clamped tail) is ellipsized.
            let band = Rect::new(inner.left, top, inner.right + FIT_EPSILON, top + Self::LINE);
            if canvas.measure(line, fmt) <= inner.right - inner.left + FIT_EPSILON {
                canvas.text_aligned(line, &band, fmt, &t.tooltip_foreground, DWRITE_TEXT_ALIGNMENT_LEADING);
            } else {
                canvas.text_ellipsis(line, &band, fmt, &t.tooltip_foreground);
            }
        }
        canvas.pop_clip();
    }

    /// A tooltip is `pointerEvents: 'none'` on the web, and inert here too:
    /// it must never eat the click meant for what it describes.
    fn hit_test(&self, _bounds: Rect, _x: f32, _y: f32) -> bool {
        false
    }

    fn type_name(&self) -> &'static str {
        "Tooltip"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Separator
// ─────────────────────────────────────────────────────────────────────────────

/// Which way a rule runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Orientation {
    #[default]
    Horizontal,
    Vertical,
}

/// A rule — `@ui/Separator`: `bg-border`, `h-px w-full` or `w-px self-stretch`.
///
/// Its model is an empty [`kubuno_desktop_controls::labels::Label`], which is the
/// toolkit's own idiom for a divider and gives the rule `dock`, `anchor`,
/// `margin` and `visible` without a new property block.
pub struct Separator {
    inner: kc::Label,
    pub orientation: Orientation,
}

impl Deref for Separator {
    type Target = kc::Label;
    fn deref(&self) -> &kc::Label {
        &self.inner
    }
}
impl DerefMut for Separator {
    fn deref_mut(&mut self) -> &mut kc::Label {
        &mut self.inner
    }
}

impl Separator {
    /// The row a layout reserves for a rule, in DIP.
    ///
    /// Not a number of its own: it **is** [`crate::metrics::control::SEPARATOR`]
    /// (`h-px` / `w-px` on `@ui/Separator`), named here only so a caller does
    /// not have to reach into the metric table to size a rule. The *painted*
    /// line is one PHYSICAL pixel (see [`line_rect`]), so on a 150 % display
    /// the hairline is thinner than the DIP it is centred in; reserving less
    /// would let two stacked rules touch.
    pub const THICKNESS: f32 = control::SEPARATOR;

    pub fn horizontal() -> Self {
        Self { inner: kc::Label::new(), orientation: Orientation::Horizontal }
    }

    pub fn vertical() -> Self {
        Self { inner: kc::Label::new(), orientation: Orientation::Vertical }
    }
}

/// The hairline rectangle a separator strokes, centred in `bounds`.
///
/// Thickness is `1 / scale`, i.e. exactly ONE physical pixel at any DPI, and
/// the rectangle is handed to [`Canvas::stroke_rounded`] — which already snaps
/// to pixel centres and strokes at `1 / scale`. Deriving the alignment here
/// instead would be a second, disagreeing implementation of it.
pub fn line_rect(bounds: Rect, orientation: Orientation, scale: f32) -> Rect {
    let t = 1.0 / scale.max(0.01);
    match orientation {
        Orientation::Horizontal => {
            let y = bounds.top + ((bounds.bottom - bounds.top) - t) / 2.0;
            Rect::new(bounds.left, y, bounds.right, y + t)
        }
        Orientation::Vertical => {
            let x = bounds.left + ((bounds.right - bounds.left) - t) / 2.0;
            Rect::new(x, bounds.top, x + t, bounds.bottom)
        }
    }
}

impl Widget for Separator {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, _canvas: &dyn Canvas) -> Size {
        // The rule has no intrinsic length: it is `w-full` / `self-stretch`, so
        // the caller's layout gives it one. Only the thickness is its own.
        match self.orientation {
            Orientation::Horizontal => Size::new(0.0, Separator::THICKNESS),
            Orientation::Vertical => Size::new(Separator::THICKNESS, 0.0),
        }
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, _state: WidgetState) {
        let line = line_rect(bounds, self.orientation, canvas.scale());
        // `--color-border`, which is what `bg-border` resolves to.
        canvas.stroke_rounded(&line, 0.0, &canvas.theme().card_stroke);
    }

    fn type_name(&self) -> &'static str {
        "Separator"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// The nine cells, in the order the toolkit numbers them.
    const NINE: [ContentAlignment; 9] = [
        ContentAlignment::TopLeft,
        ContentAlignment::TopCenter,
        ContentAlignment::TopRight,
        ContentAlignment::MiddleLeft,
        ContentAlignment::MiddleCenter,
        ContentAlignment::MiddleRight,
        ContentAlignment::BottomLeft,
        ContentAlignment::BottomCenter,
        ContentAlignment::BottomRight,
    ];

    // ── Role / measurement ────────────────────────────────────────────────

    #[test]
    fn every_role_has_a_line_box_at_least_as_tall_as_its_em() {
        for role in ROLES {
            assert!(
                role.line_height() >= role.size(),
                "{role:?}: a {} px face cannot live in a {} px line",
                role.size(),
                role.line_height()
            );
        }
    }

    #[test]
    fn the_four_added_roles_have_the_specified_size_and_line_box() {
        for (role, size, line) in [
            (Role::Badge, 10.0, 16.0),
            (Role::Caption, 11.0, 16.0),
            (Role::Subtitle, 16.0, 24.0),
            (Role::Display, 24.0, 32.0),
        ] {
            assert_eq!(role.size(), size, "{role:?}");
            assert_eq!(role.line_height(), line, "{role:?}");
        }
    }

    #[test]
    fn roles_are_in_scale_order_and_every_line_box_holds_its_face() {
        for w in ROLES.windows(2) {
            assert!(w[0].size() < w[1].size(), "{:?} then {:?}", w[0], w[1]);
        }
        for role in ROLES {
            let natural = (role.size() * 1.33008).ceil();
            assert!(role.line_height() >= natural, "{role:?}: {} < {natural}", role.line_height());
        }
    }

    #[test]
    fn micro_reserves_the_line_box_of_the_face_it_is_painted_in() {
        // `Role::format` collapses Micro onto the meta face (no 11 px format
        // exists); reserving MICRO's own leading would clip it.
        assert_eq!(Role::Micro.line_height(), Role::Meta.line_height());
    }

    #[test]
    fn an_auto_sized_label_is_its_line_box_plus_padding_and_border() {
        // The width the canvas would measure, injected — so the arithmetic is
        // checked without a device.
        let text = 80.0;
        for role in ROLES {
            let bare = label_size(
                Size::new(text, role.line_height()),
                Padding::ZERO,
                BorderStyle::None,
            );
            assert_eq!(bare, Size::new(text, role.line_height()), "{role:?}");

            let padded = label_size(
                Size::new(text, role.line_height()),
                Padding::new(4.0, 3.0, 6.0, 5.0),
                BorderStyle::FixedSingle,
            );
            assert_eq!(
                padded,
                Size::new(text + 10.0 + 2.0, role.line_height() + 8.0 + 2.0),
                "{role:?}"
            );
        }
    }

    #[test]
    fn a_bordered_label_reserves_one_dip_for_either_border_style() {
        // Kubuno has no bevel: `Fixed3D` is not the two-pixel static edge the
        // replica reserves, it is the same single hairline as `FixedSingle`.
        assert_eq!(border_inset(BorderStyle::None), 0.0);
        assert_eq!(border_inset(BorderStyle::FixedSingle), 1.0);
        assert_eq!(border_inset(BorderStyle::Fixed3D), 1.0);
    }

    #[test]
    fn a_label_honours_the_replicas_minimum_and_maximum() {
        let mut l = Label::new("x");
        l.minimum_size = Size::new(200.0, 0.0);
        // `clamp` is the replica's; this only proves `measure` routes through it.
        assert_eq!(l.clamp(Size::new(10.0, 20.0)).width, 200.0);
    }

    #[test]
    fn mnemonics_come_from_the_replica() {
        let mut l = Label::new("&Fichier && dossier");
        assert_eq!(l.shown_text(), "Fichier & dossier");
        l.use_mnemonic = false;
        assert_eq!(l.shown_text(), "&Fichier && dossier");
    }

    // ── The nine alignments ───────────────────────────────────────────────

    #[test]
    fn the_nine_alignments_place_a_box_at_the_nine_cells() {
        let content = Rect::new(0.0, 0.0, 100.0, 60.0);
        let size = Size::new(20.0, 10.0);
        let expected = [
            (0.0, 0.0),
            (40.0, 0.0),
            (80.0, 0.0),
            (0.0, 25.0),
            (40.0, 25.0),
            (80.0, 25.0),
            (0.0, 50.0),
            (40.0, 50.0),
            (80.0, 50.0),
        ];
        for (a, (x, y)) in NINE.into_iter().zip(expected) {
            let r = aligned_box(content, a, size);
            assert_eq!((r.left, r.top), (x, y), "{a:?}");
            assert_eq!((r.right - r.left, r.bottom - r.top), (20.0, 10.0), "{a:?}");
        }
    }

    #[test]
    fn a_text_band_takes_the_full_width_and_only_the_vertical_third() {
        let content = Rect::new(10.0, 0.0, 110.0, 60.0);
        let line = Role::Body.line_height(); // 20
        for a in NINE {
            let b = text_band(content, a, line);
            assert_eq!((b.left, b.right), (10.0, 110.0), "{a:?}");
            assert_eq!(b.bottom - b.top, line, "{a:?}");
            // 60 tall, a 20 line: the three thirds land at 0, 20 and 40.
            let want = (60.0 - line) * a.fractions().1;
            assert_eq!(b.top, want, "{a:?}");
        }
    }

    #[test]
    fn the_horizontal_third_becomes_the_directwrite_alignment() {
        for a in NINE {
            let h = a.fractions().0;
            let want = if h < 0.25 {
                DWRITE_TEXT_ALIGNMENT_LEADING
            } else if h > 0.75 {
                DWRITE_TEXT_ALIGNMENT_TRAILING
            } else {
                DWRITE_TEXT_ALIGNMENT_CENTER
            };
            assert_eq!(h_alignment(a).0, want.0, "{a:?}");
        }
    }

    #[test]
    fn a_box_bigger_than_its_content_rect_is_pinned_not_pushed_out() {
        let content = Rect::new(0.0, 0.0, 10.0, 10.0);
        let r = aligned_box(content, ContentAlignment::BottomRight, Size::new(40.0, 40.0));
        assert_eq!((r.left, r.top), (0.0, 0.0));
    }

    // ── The bug this module is tested against ─────────────────────────────

    #[test]
    fn geometry_follows_the_argument_and_not_the_models_own_rectangle() {
        // A control whose model thinks it lives at the window origin, painted
        // somewhere else entirely — the exact shape of the shipped bug.
        let mut l = Label::new("x");
        l.set_bounds(Rect::new(0.0, 0.0, 10.0, 10.0));
        l.padding = Padding::all(4.0);
        l.border_style = BorderStyle::FixedSingle;

        let painted = Rect::new(500.0, 300.0, 700.0, 340.0);
        let c = l.content(painted);
        assert_eq!((c.left, c.top), (505.0, 305.0));
        assert_eq!((c.right, c.bottom), (695.0, 335.0));
        assert!(l.band(painted).top >= painted.top, "the band left the rectangle");

        let mut icon = Icon::new("Check");
        icon.set_bounds(Rect::new(0.0, 0.0, 10.0, 10.0));
        let g = icon.glyph_rect(painted);
        assert!(g.left >= painted.left && g.top >= painted.top, "glyph painted at the origin");
    }

    /// The non-regression gate itself: **no paint path in this file may read
    /// the model's own rectangle**. Reading `self.bounds` (through the deref to
    /// `ControlBase`) instead of the `bounds` argument has been shipped three
    /// times — `Label`, `PictureBox`, `LinkLabel` — and each time it painted a
    /// container's child near the window origin, where the parent's clip ate
    /// it. A geometry test cannot catch a re-introduction inside a `paint` body
    /// (no headless canvas exists to record calls), so this reads the source.
    ///
    /// If it fires: the fix is to take the rectangle from the argument. If a
    /// genuine need for the model's rectangle ever appears (hit-testing in
    /// parent space, as `LinkLabel::link_at_point` does one layer down), it
    /// belongs in a clearly named method — and this test's allow-list, not in a
    /// paint body.
    #[test]
    fn no_paint_path_reads_the_models_own_rectangle() {
        const SOURCE: &str = include_str!("display.rs");
        for (n, line) in SOURCE.lines().enumerate() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                break; // The tests below may say the words.
            }
            // Strip comments, so a doc comment naming the trap is not the trap.
            let code = line.split("//").next().unwrap_or("");
            for needle in [".bounds", "control().bounds"] {
                assert!(
                    !code.contains(needle),
                    "line {}: `{}` reads the model's own rectangle — paint into the \
                     `bounds` ARGUMENT (see this test's doc comment)",
                    n + 1,
                    code.trim()
                );
            }
        }
    }

    // ── Badge ─────────────────────────────────────────────────────────────

    #[test]
    fn a_badge_is_its_text_plus_its_padding() {
        // `px-2 py-0.5` on a `text-xs` line: 8 + w + 8 wide, 2 + 16 + 2 tall.
        assert_eq!(badge_size(30.0, BadgeSize::Md, false), Size::new(46.0, 20.0));
        // `px-1.5`: two DIP narrower on each side.
        assert_eq!(badge_size(30.0, BadgeSize::Sm, false), Size::new(42.0, 20.0));
        // The dot adds its diameter and `gap-1`.
        assert_eq!(
            badge_size(30.0, BadgeSize::Md, true),
            Size::new(46.0 + 6.0 + 4.0, 20.0)
        );
        // An empty badge is still a pill, not a zero-width sliver.
        assert_eq!(badge_size(0.0, BadgeSize::Md, false), Size::new(16.0, 20.0));
    }

    #[test]
    fn a_badge_grows_only_with_its_content() {
        let mut previous = 0.0;
        for w in [0.0, 7.0, 14.0, 60.0] {
            let s = badge_size(w, BadgeSize::Md, false);
            assert!(s.width > previous, "width must be monotonic in the text width");
            assert_eq!(s.height, 20.0, "height must NOT depend on the content");
            previous = s.width;
        }
    }

    #[test]
    fn every_badge_variant_pairs_a_ground_with_an_ink() {
        let t = Theme::light();
        for v in BADGE_VARIANTS {
            let (ground, ink) = v.colours(&t);
            assert!(ground.a > 0.0 && ink.a > 0.0, "{v:?}");
            assert!(
                (ground.r, ground.g, ground.b) != (ink.r, ink.g, ink.b),
                "{v:?}: invisible text"
            );
        }
    }

    #[test]
    fn a_pills_corners_are_not_clickable() {
        let b = Badge::new("9");
        let r = Rect::new(0.0, 0.0, 40.0, 20.0);
        assert!(b.hit_test(r, 20.0, 10.0), "the middle is the badge");
        assert!(b.hit_test(r, 1.0, 10.0), "so is the left cap");
        assert!(!b.hit_test(r, 0.5, 0.5), "but not the corner outside the cap");
    }

    // ── Icon ──────────────────────────────────────────────────────────────

    #[test]
    fn an_icons_size_lives_on_the_replica_and_drives_its_placement() {
        let icon = Icon::sized("ChevronDown", 16.0);
        assert_eq!(icon.image, Some(Size::new(16.0, 16.0)), "one storage location");
        assert_eq!(icon.natural(), Size::new(16.0, 16.0));

        // `AutoSize` draws at native size, top-left of the content rect — the
        // replica's own rule, not a re-derivation.
        let r = icon.glyph_rect(Rect::new(100.0, 100.0, 140.0, 140.0));
        assert_eq!((r.left, r.top, r.right, r.bottom), (100.0, 100.0, 116.0, 116.0));
    }

    #[test]
    fn a_centred_icon_uses_the_replicas_centring() {
        let mut icon = Icon::sized("Check", 16.0);
        icon.size_mode = kc::PictureBoxSizeMode::CenterImage;
        let r = icon.glyph_rect(Rect::new(0.0, 0.0, 40.0, 40.0));
        assert_eq!((r.left, r.top), (12.0, 12.0));
    }

    #[test]
    fn an_icons_frame_eats_into_the_box_it_is_given() {
        let mut icon = Icon::new("Star");
        assert_eq!(icon.natural(), Size::new(Icon::DEFAULT, Icon::DEFAULT));
        icon.border_style = BorderStyle::FixedSingle;
        icon.padding = Padding::all(2.0);
        // A 40×40 box, minus the 1 DIP border and the 2 DIP padding, leaves a
        // 34×34 client — and `AutoSize` puts the 20 px glyph at its top-left.
        let r = icon.glyph_rect(Rect::new(0.0, 0.0, 40.0, 40.0));
        assert_eq!((r.left, r.top), (3.0, 3.0));
        assert_eq!((r.right - r.left, r.bottom - r.top), (20.0, 20.0));
    }

    // ── Tooltip placement ─────────────────────────────────────────────────

    /// A 1000×800 window, and a bubble the size `measure` would give a short
    /// label: 100×28 (`16` line + 2×6 padding).
    const VIEW: Size = Size { width: 1000.0, height: 800.0 };
    const BUBBLE: Size = Size { width: 100.0, height: 28.0 };

    #[test]
    fn a_tooltip_takes_the_side_it_was_asked_for_when_there_is_room() {
        let anchor = Rect::new(480.0, 380.0, 520.0, 420.0);
        for side in SIDES {
            let p = place(anchor, BUBBLE, side, VIEW);
            assert_eq!(p.side, side, "{side:?} had room and was refused");
        }
        // …and it sits exactly GAP away from the anchor's edge.
        assert_eq!(place(anchor, BUBBLE, Side::Bottom, VIEW).rect.top, 420.0 + Tooltip::GAP);
        assert_eq!(place(anchor, BUBBLE, Side::Top, VIEW).rect.bottom, 380.0 - Tooltip::GAP);
        assert_eq!(place(anchor, BUBBLE, Side::Right, VIEW).rect.left, 520.0 + Tooltip::GAP);
        assert_eq!(place(anchor, BUBBLE, Side::Left, VIEW).rect.right, 480.0 - Tooltip::GAP);
    }

    #[test]
    fn a_tooltip_flips_at_each_of_the_four_edges() {
        // Against the TOP edge: `Top` cannot fit, so it flips below.
        let top = Rect::new(480.0, 0.0, 520.0, 20.0);
        assert_eq!(place(top, BUBBLE, Side::Top, VIEW).side, Side::Bottom);

        // Against the BOTTOM edge: flips above.
        let bottom = Rect::new(480.0, 780.0, 520.0, 800.0);
        assert_eq!(place(bottom, BUBBLE, Side::Bottom, VIEW).side, Side::Top);

        // Against the LEFT edge: flips right.
        let left = Rect::new(0.0, 380.0, 20.0, 420.0);
        assert_eq!(place(left, BUBBLE, Side::Left, VIEW).side, Side::Right);

        // Against the RIGHT edge: flips left.
        let right = Rect::new(980.0, 380.0, 1000.0, 420.0);
        assert_eq!(place(right, BUBBLE, Side::Right, VIEW).side, Side::Left);
    }

    #[test]
    fn a_flipped_tooltip_stays_inside_the_viewport() {
        for (anchor, side) in [
            (Rect::new(480.0, 0.0, 520.0, 20.0), Side::Top),
            (Rect::new(480.0, 780.0, 520.0, 800.0), Side::Bottom),
            (Rect::new(0.0, 380.0, 20.0, 420.0), Side::Left),
            (Rect::new(980.0, 380.0, 1000.0, 420.0), Side::Right),
            // Corners: both axes are against an edge at once.
            (Rect::new(0.0, 0.0, 20.0, 20.0), Side::Top),
            (Rect::new(980.0, 780.0, 1000.0, 800.0), Side::Bottom),
        ] {
            let r = place(anchor, BUBBLE, side, VIEW).rect;
            assert!(r.left >= Tooltip::MARGIN, "{side:?}: past the left edge");
            assert!(r.top >= Tooltip::MARGIN, "{side:?}: past the top edge");
            assert!(r.right <= VIEW.width - Tooltip::MARGIN, "{side:?}: past the right edge");
            assert!(r.bottom <= VIEW.height - Tooltip::MARGIN, "{side:?}: past the bottom edge");
        }
    }

    #[test]
    fn a_tooltip_is_centred_on_its_anchor_when_nothing_pushes_it() {
        let anchor = Rect::new(480.0, 380.0, 520.0, 420.0);
        let r = place(anchor, BUBBLE, Side::Bottom, VIEW).rect;
        assert_eq!((r.left + r.right) / 2.0, 500.0);
        let r = place(anchor, BUBBLE, Side::Right, VIEW).rect;
        assert_eq!((r.top + r.bottom) / 2.0, 400.0);
    }

    #[test]
    fn the_arrow_base_sits_on_the_anchor_facing_edge_of_each_side() {
        let anchor = Rect::new(480.0, 380.0, 520.0, 420.0);
        for side in SIDES {
            let p = place(anchor, BUBBLE, side, VIEW);
            match side {
                // The bubble is ABOVE, so the arrow leaves by its bottom edge…
                Side::Top => {
                    assert_eq!(p.tip.1, p.rect.bottom, "{side:?}");
                    assert_eq!(p.tip.0, 500.0, "{side:?}: follows the anchor centre");
                    // …and its apex is between the bubble and the anchor.
                    let apex = p.tip.1 + Tooltip::ARROW;
                    assert!(apex > p.rect.bottom && apex < anchor.top, "{side:?}: wrong way");
                }
                Side::Bottom => {
                    assert_eq!(p.tip.1, p.rect.top, "{side:?}");
                    let apex = p.tip.1 - Tooltip::ARROW;
                    assert!(apex < p.rect.top && apex > anchor.bottom, "{side:?}: wrong way");
                }
                Side::Left => {
                    assert_eq!(p.tip.0, p.rect.right, "{side:?}");
                    let apex = p.tip.0 + Tooltip::ARROW;
                    assert!(apex > p.rect.right && apex < anchor.left, "{side:?}: wrong way");
                }
                Side::Right => {
                    assert_eq!(p.tip.0, p.rect.left, "{side:?}");
                    let apex = p.tip.0 - Tooltip::ARROW;
                    assert!(apex < p.rect.left && apex > anchor.right, "{side:?}: wrong way");
                }
            }
        }
    }

    #[test]
    fn the_arrow_base_never_lands_in_a_rounded_corner() {
        let anchor = Rect::new(480.0, 380.0, 520.0, 420.0);
        let p = place(anchor, BUBBLE, Side::Bottom, VIEW);
        assert_eq!(p.tip.0, 500.0, "an unpushed bubble keeps the anchor's centre");

        // An anchor hard against the left edge drags the tip towards the
        // bubble's own left corner; it must stop short of it.
        let corner = Rect::new(0.0, 380.0, 8.0, 420.0);
        let p = place(corner, BUBBLE, Side::Bottom, VIEW);
        let inset = radius::SM + Tooltip::ARROW_HALF;
        assert!(p.tip.0 >= p.rect.left + inset, "the arrow grew out of a rounded corner");
        assert!(p.tip.0 <= p.rect.right - inset);
    }

    #[test]
    fn a_viewport_too_small_for_the_bubble_degrades_instead_of_crossing_its_clamps() {
        // 60×60 cannot hold a 100×28 bubble on any side. The CROSS axis is
        // still clamped into the margins; the main axis overflows, which is the
        // honest answer when the window is smaller than the content.
        let tiny = Size::new(60.0, 60.0);
        let anchor = Rect::new(20.0, 20.0, 40.0, 40.0);
        for side in SIDES {
            let p = place(anchor, BUBBLE, side, tiny);
            assert!(p.rect.left.is_finite() && p.rect.top.is_finite(), "{side:?}");
            if side.is_vertical() {
                assert_eq!(p.rect.left, Tooltip::MARGIN, "{side:?}: x clamp crossed");
            } else {
                assert!(p.rect.top >= Tooltip::MARGIN, "{side:?}: y clamp crossed");
            }
        }
    }

    #[test]
    fn the_tooltip_style_is_the_same_in_both_palettes() {
        // The web ships ONE tooltip style; a dark variant here would be a
        // divergence, not an improvement.
        let (l, d) = (Theme::light(), Theme::dark());
        assert_eq!(l.tooltip_background.r, d.tooltip_background.r);
        assert_eq!(l.tooltip_background.a, d.tooltip_background.a);
        assert_eq!(l.tooltip_foreground.r, d.tooltip_foreground.r);
    }

    #[test]
    fn a_tooltip_never_takes_a_click() {
        let t = Tooltip::new("Renommer");
        assert!(!t.hit_test(Rect::new(0.0, 0.0, 100.0, 28.0), 50.0, 14.0));
    }

    // ── Separator ─────────────────────────────────────────────────────────

    #[test]
    fn a_rule_is_exactly_one_physical_pixel_at_any_dpi() {
        let bounds = Rect::new(0.0, 0.0, 200.0, 1.0);
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
            let r = line_rect(bounds, Orientation::Horizontal, scale);
            assert!(
                ((r.bottom - r.top) - 1.0 / scale).abs() < 1e-6,
                "scale {scale}: {} DIP is not one device pixel",
                r.bottom - r.top
            );
            assert_eq!((r.left, r.right), (0.0, 200.0), "a rule spans its box");
        }
    }

    #[test]
    fn a_vertical_rule_is_the_same_line_turned_ninety_degrees() {
        let bounds = Rect::new(0.0, 0.0, 1.0, 200.0);
        let r = line_rect(bounds, Orientation::Vertical, 2.0);
        assert!(((r.right - r.left) - 0.5).abs() < 1e-6);
        assert_eq!((r.top, r.bottom), (0.0, 200.0));
    }

    #[test]
    fn a_rule_is_centred_in_whatever_row_it_is_given() {
        let r = line_rect(Rect::new(0.0, 10.0, 100.0, 20.0), Orientation::Horizontal, 1.0);
        assert_eq!(r.top, 14.5, "a 1 DIP line in a 10 DIP row sits in the middle");
    }

    #[test]
    fn a_separator_reserves_one_dip_on_its_thin_axis_only() {
        assert_eq!(Separator::THICKNESS, 1.0);
        assert_eq!(Separator::horizontal().orientation, Orientation::Horizontal);
        assert_eq!(Separator::vertical().orientation, Orientation::Vertical);
    }

    // ── Links ─────────────────────────────────────────────────────────────

    #[test]
    fn a_visited_link_uses_the_token_added_for_it() {
        let t = Theme::light();
        let visited = link_colour(LinkPaint::Visited, &t);
        assert_eq!((visited.r, visited.g, visited.b), (t.link_visited.r, t.link_visited.g, t.link_visited.b));
        // …and it is NOT the accent, which is what an unvisited one gets.
        assert_ne!(visited.r, t.accent.r);
    }

    #[test]
    fn the_four_link_states_are_four_distinct_colours() {
        for t in [Theme::light(), Theme::dark()] {
            let all = [LinkPaint::Normal, LinkPaint::Active, LinkPaint::Visited, LinkPaint::Disabled]
                .map(|p| link_colour(p, &t));
            for (i, a) in all.iter().enumerate() {
                for b in all.iter().skip(i + 1) {
                    assert!(
                        (a.r, a.g, a.b) != (b.r, b.g, b.b),
                        "two link states share a colour"
                    );
                }
            }
        }
    }

    #[test]
    fn the_link_precedence_is_the_replicas() {
        // Only that this layer routes through it: disabled beats visited.
        let mut l = LinkLabel::new("kubuno.com").visited(true);
        l.enabled = false;
        let link = l.resolved_links().remove(0);
        assert_eq!(
            l.link_paint(&link, Default::default()),
            LinkPaint::Disabled
        );
    }

    #[test]
    fn a_link_label_measures_like_a_label() {
        // Both routes must build the same extent from the same inputs.
        let extent = Size::new(64.0, Role::Body.line_height());
        assert_eq!(
            label_size(extent, Padding::all(2.0), BorderStyle::None),
            Size::new(68.0, Role::Body.line_height() + 4.0)
        );
    }

    // ── Wrapping / clamping ───────────────────────────────────────────────

    /// A monospace stand-in for DirectWrite: 7 DIP per character.
    fn mono(s: &str) -> f32 {
        s.chars().count() as f32 * 7.0
    }

    fn wrap(text: &str, w: f32) -> Vec<String> {
        wrap_lines(text, w, &mut mono)
    }

    #[test]
    fn wrapping_breaks_greedily_between_words() {
        // 70 DIP = 10 characters a line.
        assert_eq!(wrap("un deux trois quatre", 70.0), ["un deux", "trois", "quatre"]);
        assert_eq!(wrap("court", 70.0), ["court"]);
    }

    #[test]
    fn wrapping_honours_explicit_breaks_and_collapses_spaces_like_pre_line() {
        assert_eq!(wrap("a   b\r\nc", 700.0), ["a b", "c"]);
        // An empty paragraph is an empty line, as `pre-line` renders it.
        assert_eq!(wrap("a\n\nb", 700.0), ["a", "", "b"]);
        // Unconstrained: only the explicit breaks apply.
        assert_eq!(wrap("un deux trois\nquatre", 0.0), ["un deux trois", "quatre"]);
    }

    #[test]
    fn a_word_wider_than_the_box_is_broken_between_characters() {
        let lines = wrap("abcdefghijklmnopqrstuvwxyz", 70.0);
        assert_eq!(lines, ["abcdefghij", "klmnopqrst", "uvwxyz"]);
        assert!(lines.iter().all(|l| mono(l) <= 70.0), "a line left the box");
    }

    #[test]
    fn no_wrapped_line_is_wider_than_the_box() {
        let text = "Partage de liens publics — les destinataires voient le dossier sans compte";
        for w in [50.0, 91.0, 140.0, 260.0] {
            for l in wrap(text, w) {
                assert!(mono(&l) <= w + FIT_EPSILON, "{l:?} at {w}");
            }
        }
    }

    #[test]
    fn clamping_folds_the_rest_into_the_last_kept_line() {
        let lines = vec!["a".to_owned(), "b".to_owned(), "c".to_owned(), "d".to_owned()];
        let (kept, cut) = clamp_lines(lines.clone(), 2);
        assert!(cut);
        assert_eq!(kept, ["a", "b c d"], "the painter ellipsizes the joined tail");
        assert_eq!(clamp_lines(lines.clone(), 9), (lines.clone(), false));
        // Never zero lines: a box shorter than a line still shows the first.
        assert_eq!(clamp_lines(lines, 0).0.len(), 1);
    }

    #[test]
    fn lines_that_fit_tolerates_half_a_dip_and_never_says_zero() {
        assert_eq!(lines_that_fit(40.0, 20.0), 2);
        assert_eq!(lines_that_fit(39.6, 20.0), 2);
        assert_eq!(lines_that_fit(39.0, 20.0), 1);
        assert_eq!(lines_that_fit(4.0, 20.0), 1);
        assert_eq!(lines_that_fit(40.0, 0.0), 1);
    }

    #[test]
    fn a_label_ellipsizes_by_default_and_can_opt_into_wrapping() {
        assert_eq!(Label::new("x").overflow, TextOverflow::Ellipsis);
        assert_eq!(Label::new("x").wrap().overflow, TextOverflow::Wrap);
        assert_eq!(Label::new("x").overflow(TextOverflow::Clip).overflow, TextOverflow::Clip);
    }

    // ── Badge overflow ────────────────────────────────────────────────────

    #[test]
    fn a_badges_text_room_is_the_pill_minus_its_paddings_and_dot() {
        assert_eq!(badge_text_room(100.0, BadgeSize::Md, false), 84.0);
        assert_eq!(badge_text_room(100.0, BadgeSize::Sm, false), 88.0);
        assert_eq!(badge_text_room(100.0, BadgeSize::Md, true), 74.0);
        assert_eq!(badge_text_room(10.0, BadgeSize::Md, true), 0.0, "never negative");
    }

    #[test]
    fn max_width_is_a_builder_and_defaults_to_none() {
        assert_eq!(Badge::new("x").max_width, None);
        assert_eq!(Badge::new("x").max_width(80.0).max_width, Some(80.0));
    }

    // ── Icon resolution ───────────────────────────────────────────────────

    #[test]
    fn known_icons_resolve_to_themselves() {
        for name in ["Check", "ChevronDown", "Star", "Trash2", "Link2"] {
            assert_eq!(resolve_icon(name), Some(name), "{name}");
            assert!(Icon::new(name).is_known());
        }
    }

    /// Every lucide rename the web uses, as (new name, old name).
    const RENAMED_PAIRS: [(&str, &str); 9] = [
        ("CircleAlert", "AlertCircle"),
        ("CircleCheck", "CheckCircle2"),
        ("CircleHelp", "HelpCircle"),
        ("TriangleAlert", "AlertTriangle"),
        ("Ellipsis", "MoreHorizontal"),
        ("EllipsisVertical", "MoreVertical"),
        ("CodeXml", "Code2"),
        ("FilePen", "FileEdit"),
        ("SquareCheckBig", "CheckSquare"),
    ];

    #[test]
    fn both_spellings_of_every_lucide_rename_resolve() {
        for (new, old) in RENAMED_PAIRS {
            for name in [new, old] {
                let resolved = resolve_icon(name);
                assert!(resolved.is_some(), "{name} paints the missing-glyph box");
                assert!(Icon::new(name).is_known(), "{name}");
            }
        }
    }

    #[test]
    fn aliases_only_cover_missing_names_and_point_to_embedded_ones() {
        for (alias, target) in ICON_ALIASES {
            assert!(
                kubuno_drive_desktop_app_controls::icon_name(alias).is_none(),
                "{alias} is embedded itself: its alias is dead weight"
            );
            assert!(
                kubuno_drive_desktop_app_controls::icon_name(target).is_some(),
                "alias target {target} is not embedded"
            );
            assert_eq!(resolve_icon(alias), Some(target), "{alias}");
        }
    }

    #[test]
    fn an_unknown_icon_is_reported_as_unknown() {
        assert_eq!(resolve_icon("NoSuchGlyph"), None);
        assert!(!Icon::new("NoSuchGlyph").is_known());
    }

    // ── Link label ────────────────────────────────────────────────────────

    #[test]
    fn a_kubuno_link_underlines_on_hover_only_like_the_web() {
        let l = LinkLabel::new("kubuno.com");
        let link = l.resolved_links().remove(0);
        let rest = kubuno_desktop_controls::ControlState::default();
        let hot = kubuno_desktop_controls::ControlState { hot: true, ..rest };
        assert!(!l.underlines_link(&link, rest), "`hover:underline`: no rule at rest");
        assert!(l.underlines_link(&link, hot));
        assert_eq!((l.hot_link, l.focused_link), (None, None));
    }

    // ── Pointer-anchored tooltip ──────────────────────────────────────────

    #[test]
    fn a_pointer_tooltip_sits_below_and_left_aligned_like_place_tooltip() {
        let view = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let p = place_at_pointer((300.0, 200.0), BUBBLE, view);
        assert!(p.below);
        assert_eq!((p.rect.left, p.rect.top), (300.0, 200.0 + Tooltip::GAP));
    }

    #[test]
    fn a_pointer_tooltip_flips_above_and_pulls_back_at_the_edges() {
        let view = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let p = place_at_pointer((990.0, 790.0), BUBBLE, view);
        assert!(!p.below);
        assert_eq!(p.rect.bottom, 790.0 - Tooltip::GAP);
        assert_eq!(p.rect.right, 1000.0 - Tooltip::MARGIN);
    }

    #[test]
    fn a_pointer_tooltip_accepts_a_viewport_that_does_not_start_at_zero() {
        // The monitor work area in CLIENT coordinates starts left of/above the
        // window: the clamps must use its edges, not zero.
        let view = Rect::new(-200.0, -100.0, 800.0, 700.0);
        let p = place_at_pointer((-195.0, -90.0), BUBBLE, view);
        assert_eq!(p.rect.left, -200.0 + Tooltip::MARGIN);
        assert!(p.below);
    }

    #[test]
    fn the_tooltip_text_limit_is_280_minus_the_padding() {
        assert_eq!(Tooltip::TEXT_MAX, 260.0);
        assert_eq!(Tooltip::DELAY_MS, 400);
    }

    // ── Tooltip timing ────────────────────────────────────────────────────

    #[test]
    fn a_tooltip_waits_for_the_delay_then_anchors_where_it_appeared() {
        let mut t = TooltipTrigger::new();
        let d = Tooltip::DELAY_MS;
        let tick = t.update(true, (10.0, 10.0), false, 1000, d);
        assert_eq!(tick.show_at, None);
        assert_eq!(tick.repaint_in_ms, Some(400), "asks to be woken when due");
        assert_eq!(t.update(true, (12.0, 10.0), false, 1399, d).repaint_in_ms, Some(1));
        let shown = t.update(true, (15.0, 11.0), false, 1400, d);
        assert_eq!(shown.show_at, Some((15.0, 11.0)));
        // It does not chase the pointer afterwards.
        assert_eq!(t.update(true, (40.0, 30.0), false, 1500, d).show_at, Some((15.0, 11.0)));
        assert!(t.is_visible());
    }

    #[test]
    fn leaving_the_trigger_hides_and_resets_the_timer() {
        let mut t = TooltipTrigger::new();
        t.update(true, (0.0, 0.0), false, 0, 400);
        t.update(true, (0.0, 0.0), false, 400, 400);
        assert!(t.is_visible());
        assert_eq!(t.update(false, (0.0, 0.0), false, 450, 400), TooltipTick::default());
        // Back in: the full delay again.
        assert_eq!(t.update(true, (0.0, 0.0), false, 500, 400).repaint_in_ms, Some(400));
    }

    #[test]
    fn a_press_or_escape_keeps_it_hidden_until_the_pointer_leaves() {
        let mut t = TooltipTrigger::new();
        t.update(true, (0.0, 0.0), false, 0, 400);
        t.update(true, (0.0, 0.0), false, 400, 400);
        assert_eq!(t.update(true, (0.0, 0.0), true, 450, 400).show_at, None, "mousedown hides");
        assert_eq!(t.update(true, (0.0, 0.0), false, 5000, 400).show_at, None, "and it stays hidden");
        t.update(false, (0.0, 0.0), false, 5001, 400);
        t.update(true, (0.0, 0.0), false, 5002, 400);
        assert!(t.update(true, (0.0, 0.0), false, 5402, 400).show_at.is_some(), "re-armed by leaving");

        t.dismiss();
        assert!(!t.is_visible(), "Escape dismisses");
        assert_eq!(t.update(true, (0.0, 0.0), false, 9000, 400).show_at, None);
    }

    // ── The Widget contract ───────────────────────────────────────────────

    #[test]
    fn every_primitive_names_itself() {
        let names: Vec<&str> = vec![
            Label::new("a").type_name(),
            LinkLabel::new("a").type_name(),
            Badge::new("1").type_name(),
            Icon::new("Check").type_name(),
            Tooltip::new("a").type_name(),
            Separator::horizontal().type_name(),
        ];
        assert_eq!(
            names,
            ["Label", "LinkLabel", "Badge", "Icon", "Tooltip", "Separator"]
        );
    }

    #[test]
    fn the_default_hit_test_is_the_rectangle() {
        let l = Label::new("a");
        let r = Rect::new(10.0, 10.0, 50.0, 30.0);
        assert!(l.hit_test(r, 10.0, 10.0));
        assert!(!l.hit_test(r, 50.0, 20.0), "the right edge is exclusive");
        assert!(!l.hit_test(r, 9.9, 20.0));
    }
}
