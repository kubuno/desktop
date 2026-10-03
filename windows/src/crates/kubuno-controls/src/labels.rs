//! `Label` → `LinkLabel`; `PictureBox`; `ProgressBar` — the display family.
//!
//! ## What ties these three together
//!
//! None of them is interactive in the way a button is: they *show* something —
//! text, a bitmap, a fraction — and the toolkit measures and paints them from a
//! handful of properties. They live in one module because the reference sheet
//! (`05-labels.png`) groups them, and because they share the same discipline:
//! all geometry is a pure function that a test can check without a window, and
//! every pixel is drawn through [`ControlCanvas`] in the **system's** own
//! colours and UI font — never the Kubuno palette or the embedded face, which
//! would answer a different question than the one the sheet asks.
//!
//! ## The chain, mirrored with composition
//!
//! `Label` declares its own block on top of `ControlBase`; `LinkLabel` *is* a
//! `Label` plus a link model, so it composes `Label` and derefs to it — it never
//! restates `AutoSize`, `TextAlign`, `Text`… which are already there.
//! `PictureBox` and `ProgressBar` each sit directly on `ControlBase`.
//!
//! ```ignore
//! Label       { control: ControlBase, /* +13 */ }   → derefs to ControlBase
//! LinkLabel   { label:   Label,       /* +7  */ }   → derefs to Label → ControlBase
//! PictureBox  { control: ControlBase, /* +7  */ }
//! ProgressBar { control: ControlBase, /* +7  */ }
//! ```
//!
//! ## Two things the port cannot do faithfully, and says so
//!
//! * **Text height.** [`Canvas`] measures text *width* only ([`Canvas::measure`]).
//!   WinForms' `GetPreferredSize` needs the line height too, so it is derived
//!   from the **real** UI font — [`SystemFonts::size_dip`], read from
//!   `lfMessageFont` — times the face's own design line spacing
//!   ([`UI_LINE_SPACING`]). That is no longer an invented number: at the
//!   reference sheet's scale it reproduces the toolkit's `Font.Height` exactly
//!   (see the constant). The *arithmetic that adds padding and the border* is a
//!   pure function and is the part the tests pin down.
//! * **Bitmaps.** `Image`, `BackgroundImage`, `ErrorImage`… are supplied by the
//!   host at paint time (the `shell_icon` contract in `canvas.rs`). A control
//!   only needs the image's *natural size* to lay it out, so image-typed
//!   properties are modelled as `Option<Size>`; the host blits the pixels into
//!   the rectangle the control computes.

use std::cell::RefCell;

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING,
};

use crate::control::{Control, ControlBase, ControlCanvas, ControlState, FontRole};
use crate::enums::{BorderStyle, ContentAlignment, FlatStyle, Padding, Size};
use crate::system::{Border3DSide, Border3DStyle, SystemColors, SystemFonts};
use crate::{Canvas, Rect};

// ─────────────────────────────────────────────────────────────────────────────
// Colours the system does not publish
// ─────────────────────────────────────────────────────────────────────────────

/// An sRGB byte triple as a Direct2D colour. Opaque, like every system colour.
///
/// Used ONLY for the handful of values below, each of which is a colour the
/// toolkit paints that `GetSysColor` cannot answer for — never as a shortcut
/// around [`SystemColors`].
const fn rgb(r: u8, g: u8, b: u8) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
}

/// `LinkLabel.LinkColor`'s shipped default — **pure blue**.
///
/// TRAP: this is *not* `SystemColors.HotTrack`. `LinkLabel` resolves its three
/// link colours through `LinkUtilities`, which reads Internet Explorer's
/// « Anchor Color » settings and falls back to IE's own shipped defaults — blue,
/// purple, red. `COLOR_HOTLIGHT` reads `#0066CC` on a default Windows 11, while
/// the reference sheet's links are `#0000FF` (sampled at 376,128 in
/// `05-labels.png`): the two are visibly different blues, and only this one
/// matches the toolkit.
///
/// The registry value itself is not read — a control does no I/O (see the crate
/// rules) — so the port resolves to IE's shipped default, exactly as
/// [`LinkBehavior::underlined`] resolves `SystemDefault` without reading
/// *Underline links*.
const IE_LINK_COLOR: D2D1_COLOR_F = rgb(0, 0, 255);

/// `VisitedLinkColor`'s shipped default — IE's purple, `#800080`. Sampled at
/// (382,198) in the reference sheet; no `COLOR_*` index carries it.
const IE_VISITED_LINK_COLOR: D2D1_COLOR_F = rgb(128, 0, 128);

/// `ActiveLinkColor`'s shipped default — IE's red, `#FF0000`. The sheet has
/// nothing mid-click to confirm it, so it comes from the toolkit itself:
/// `new LinkLabel().ActiveLinkColor` answers `#FF0000` on this machine.
const IE_ACTIVE_LINK_COLOR: D2D1_COLOR_F = rgb(255, 0, 0);

/// `DisabledLinkColor`'s shipped default — `#858585`, which is
/// `ControlPaint.Dark(SystemColors.Control)`, again read back from a real
/// `LinkLabel`.
///
/// TRAP: it is *not* `COLOR_GRAYTEXT` (`#6D6D6D` here). A disabled **link** and
/// disabled **label text** are two different greys in this toolkit, and neither
/// of them is the one Windows publishes for inert text; see [`text_color`] for
/// the other.
const DISABLED_LINK_COLOR: D2D1_COLOR_F = rgb(0x85, 0x85, 0x85);

/// The themed progress-bar frame, `#BCBCBC` — sampled at (1100,114) in
/// `05-labels.png`. No `COLOR_*` index carries it: it belongs to the visual
/// style's `PP_BAR` part, which Windows publishes through UxTheme rather than
/// through `GetSysColor`.
const BAR_FRAME: D2D1_COLOR_F = rgb(0xBC, 0xBC, 0xBC);

/// The progress-bar well, `#E6E6E6` — sampled at (1100,125). Close to
/// `COLOR_3DLIGHT` (`#E3E3E3` here) but not equal to it, which is exactly why
/// it is not taken from [`SystemColors`].
const BAR_TROUGH: D2D1_COLOR_F = rgb(0xE6, 0xE6, 0xE6);

/// The progress-bar fill, `#0F7B0F` — sampled at (1009,125), the green the
/// reference sheet's `PP_CHUNK` is painted in. It is a theme colour, not a
/// system colour and not an accent: no `SystemColors` entry is green.
const BAR_FILL: D2D1_COLOR_F = rgb(0x0F, 0x7B, 0x0F);

// ─────────────────────────────────────────────────────────────────────────────
// The system UI font
// ─────────────────────────────────────────────────────────────────────────────

/// Segoe UI's own design line spacing: `(hhea.ascender + |hhea.descender| +
/// lineGap) / unitsPerEm` = `(2210 + 514 + 0) / 2048`.
///
/// This is the ratio GDI+ turns into `Font.Height`, and the port ceils it the
/// same way. Cross-checked against the reference sheet, which was captured at
/// 175 % (a 21 px em): `ceil(21 × 1.33008) = 28`, and the `Fixed3D` label there
/// measures **32 px** tall — 28 for the line plus the 2 px the border reserves
/// on each side, which is exactly what [`label_preferred`] computes. At 96 DPI
/// the same arithmetic gives 16 DIP.
const UI_LINE_SPACING: f32 = 2724.0 / 2048.0;

/// The default `lfMessageFont` size in DIP — Segoe UI 9 pt. Used only when the
/// font cannot be read from Windows at all, so that a failed read degrades to
/// the documented default instead of a zero-height line.
const DEFAULT_UI_SIZE_DIP: f32 = 12.0;

/// A single line's height for a UI font of `size_dip`. Ceiled, as WinForms ceils
/// `Font.Height`.
fn line_height(size_dip: f32) -> f32 {
    (size_dip * UI_LINE_SPACING).ceil()
}

/// Resolves a [`FontRole`] against the **system** UI font.
///
/// Windows publishes one UI font (`lfMessageFont`) and the toolkit paints every
/// one of these controls with it, so the roles cannot change the *size* — only
/// the weight, which is a real distinction the system font set carries. A role
/// that asks for a bigger face therefore comes out at the message size in bold;
/// that is a deliberate collapse, not an oversight, and it keeps a control from
/// inventing a point size the system never published.
fn font_format(fonts: &SystemFonts, role: FontRole) -> &IDWriteTextFormat {
    match role {
        FontRole::Caption | FontRole::Body => &fonts.message,
        FontRole::CaptionStrong | FontRole::BodyStrong | FontRole::Heading | FontRole::Title => {
            &fonts.message_bold
        }
    }
}

thread_local! {
    /// The system UI font, cached per DPI for the *measuring* path.
    ///
    /// `preferred_size` and `link_at_point` receive a bare [`Canvas`], which —
    /// unlike the [`ControlCanvas`] `paint` receives — cannot answer for the
    /// system's visuals, and their signatures are fixed by the `Control` trait.
    /// They must still measure with the toolkit's font: measuring with the
    /// embedded face would size every label to a font it is not painted in.
    ///
    /// So the font is read from Windows here, through the same
    /// `SPI_GETNONCLIENTMETRICS` call the host makes, and kept per thread and
    /// per DPI — the read is cheap but not free, and a control may be measured
    /// once per frame. Thread-local rather than a global: DirectWrite objects
    /// are agile, but a per-thread cache needs no lock and cannot be a shared
    /// mutable surface between windows.
    static UI_FONT: RefCell<Option<(u32, SystemFonts)>> = const { RefCell::new(None) };
}

/// The system UI font at the canvas's DPI, or `None` if Windows cannot be asked.
///
/// `Canvas::scale` is `dpi / 96` (the host sets it from `GetDpiForWindow`), so
/// the DPI is recoverable from the surface — which matters, because the font
/// must be read at the DPI it will be measured at.
fn ui_fonts(c: &dyn Canvas) -> Option<SystemFonts> {
    let dpi = (c.scale().max(0.01) * 96.0).round();
    let key = dpi as u32;
    UI_FONT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some((cached, fonts)) = slot.as_ref() {
            if *cached == key {
                return Some(fonts.clone());
            }
        }
        let dwrite: IDWriteFactory =
            unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.ok()?;
        let fonts = SystemFonts::read(&dwrite, dpi).ok()?;
        *slot = Some((key, fonts.clone()));
        Some(fonts)
    })
}

/// The colour a label-ish control paints its text in.
///
/// TRAP: a disabled label is **not** `GrayText`. .NET draws disabled label text
/// through `ControlPaint.Dark(BackColor)`, which special-cases
/// `SystemColors.Control` to `SystemColors.ControlDark` — so the sheet's
/// « Disabled » caption is `#A0A0A0` (`COLOR_BTNSHADOW`, sampled at 68,240) and
/// not the `#6D6D6D` `COLOR_GRAYTEXT` reads on the same machine. Painting
/// `gray_text` here would be a visibly darker grey than the toolkit's.
fn text_color(
    colors: &SystemColors,
    fore: Option<D2D1_COLOR_F>,
    enabled: bool,
) -> D2D1_COLOR_F {
    if enabled {
        fore.unwrap_or(colors.control_text)
    } else {
        colors.control_dark
    }
}

/// The border a `Label` or a `PictureBox` draws for its `BorderStyle`.
///
/// Both are `WS_EX_STATICEDGE` controls: `FixedSingle` is `WS_BORDER`, a single
/// flat ring in `COLOR_WINDOWFRAME`, and `Fixed3D` is the static edge — **one**
/// sunken ring, `COLOR_BTNSHADOW` on top and left, `COLOR_BTNHIGHLIGHT` on
/// bottom and right, i.e. [`Border3DStyle::SunkenOuter`]. Both are read straight
/// off the reference sheet: `#646464` at (46,208) for the flat frame, and
/// `#A0A0A0` at (120,152) over `#FFFFFF` at (120,183) for the bevel — which is
/// precisely what `system.rs`' `sunken_outer` recipe produces, so the inferred
/// `DrawEdge` table is confirmed for this ring.
///
/// Note that the single painted ring and the **two** DIP [`border_inset`]
/// reserves are not in disagreement: the toolkit reserves two pixels for a
/// static edge and paints one, which is why the sheet's 28 px line lands in a
/// 32 px box.
fn paint_border(c: &dyn ControlCanvas, bounds: Rect, style: BorderStyle) {
    match style {
        BorderStyle::None => {}
        BorderStyle::FixedSingle => c.stroke_rect(&bounds, &c.visuals().colors.window_frame),
        BorderStyle::Fixed3D => {
            c.draw_edge(&bounds, Border3DStyle::SunkenOuter, Border3DSide::ALL);
        }
    }
}

/// The DirectWrite horizontal alignment for a content alignment's horizontal
/// third. Vertical placement is done by positioning the one-line band, so only
/// the horizontal axis reaches the text call.
fn h_alignment(a: ContentAlignment) -> DWRITE_TEXT_ALIGNMENT {
    match a.fractions().0 {
        x if x < 0.25 => DWRITE_TEXT_ALIGNMENT_LEADING,
        x if x > 0.75 => DWRITE_TEXT_ALIGNMENT_TRAILING,
        _ => DWRITE_TEXT_ALIGNMENT_CENTER,
    }
}

/// Resolves the text WinForms would actually *display* given `UseMnemonic`.
///
/// With mnemonics on, `&&` collapses to a literal `&` and a lone `&` marks the
/// following character as the access key — which is removed from the drawn
/// string (the port does not yet paint the mnemonic underline; there is no
/// underline primitive on [`Canvas`], so it is dropped from the glyph run, not
/// silently kept). With mnemonics off, the text is shown verbatim.
fn display_text(text: &str, use_mnemonic: bool) -> String {
    if !use_mnemonic || !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '&' {
            // `&&` collapses to a literal `&`; a lone `&` is the mnemonic marker
            // — consumed, with the next character kept as the (would-be
            // underlined) access key.
            if chars.peek() == Some(&'&') {
                out.push('&');
                chars.next();
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Places a one-line text band vertically inside `content` for a given content
/// alignment, so the canvas — which centres within whatever rectangle it is
/// given — lands the glyphs at the top/middle/bottom third. Kept separate from
/// paint because it is the whole of the vertical-alignment geometry.
fn text_band(content: Rect, alignment: ContentAlignment, line_h: f32) -> Rect {
    let avail = (content.bottom - content.top).max(line_h);
    let vfrac = alignment.fractions().1;
    let top = content.top + (avail - line_h) * vfrac;
    Rect::new(content.left, top, content.right, top + line_h)
}

// ─────────────────────────────────────────────────────────────────────────────
// Accessibility live-region setting (`AutomationLiveSetting`)
// ─────────────────────────────────────────────────────────────────────────────

/// How assistive tech is told about changes to a live region
/// (`Automation.AutomationLiveSetting`). Carried so the property is not dropped;
/// the port has no UI Automation peer yet, so it is stored, not acted upon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LiveSetting {
    #[default]
    Off,
    Polite,
    Assertive,
}

// ─────────────────────────────────────────────────────────────────────────────
// Label
// ─────────────────────────────────────────────────────────────────────────────

/// `System.Windows.Forms.Label` — the 13 properties it declares on top of
/// `Control`, plus its `AutoSize` measurement and border/alignment painting.
///
/// The properties `AutoSize`, `BackgroundImageLayout` and `Text` are inherited
/// from `ControlBase` (WinForms merely re-declares them on `Label` to change
/// their designer metadata), so they are *not* restated here — reach them
/// through the deref. `PreferredWidth`/`PreferredHeight` are read-only views and
/// are exposed as methods, not fields.
#[derive(Clone)]
pub struct Label {
    pub control: ControlBase,

    /// `AutoEllipsis` — trim overflowing text with « … » instead of clipping.
    /// Honoured in paint via [`Canvas::text_ellipsis`].
    pub auto_ellipsis: bool,
    /// `BackgroundImage` — the host supplies the pixels; the port keeps only the
    /// natural size it would tile/stretch (laid out per `BackgroundImageLayout`,
    /// which lives on `ControlBase`). Not yet blitted.
    pub background_image: Option<Size>,
    /// `BorderStyle` — `None` by default for a label (unlike the enum's own
    /// `Fixed3D` default, which is the *TextBox* default).
    pub border_style: BorderStyle,
    /// `FlatStyle` — only changes how the border is drawn; the text is the same.
    pub flat_style: FlatStyle,
    /// `Image` — natural size of the foreground image; pixels host-supplied.
    pub image: Option<Size>,
    pub image_align: ContentAlignment,
    /// `ImageIndex` into `ImageList`; `-1` means « none ».
    pub image_index: i32,
    /// `ImageKey` into `ImageList`; empty means « none ».
    pub image_key: String,
    /// `ImageList` — modelled by its uniform `ImageSize` (every image in a list
    /// shares one size); the bitmaps themselves are host-supplied. `None` = unset.
    pub image_list: Option<Size>,
    /// `LiveSetting` — stored, not yet surfaced to UI Automation.
    pub live_setting: LiveSetting,
    /// `TextAlign` — one of the nine cells. Default `TopLeft` (a label's default,
    /// which differs from most controls' `MiddleCenter`).
    pub text_align: ContentAlignment,
    /// `UseCompatibleTextRendering` — GDI+ vs GDI text. The port has one text
    /// path, so this only affects measurement compatibility, which it does not
    /// model; stored for fidelity.
    pub use_compatible_text_rendering: bool,
    /// `UseMnemonic` — treat `&` as an access-key marker (see [`display_text`]).
    pub use_mnemonic: bool,
}

impl Default for Label {
    /// The catalogue's declared defaults: `AutoSize=false` (inherited),
    /// `BorderStyle=None`, `FlatStyle=Standard`, `ImageIndex=-1`,
    /// `ImageAlign=MiddleCenter`, `TextAlign=TopLeft`, `UseMnemonic=true`.
    fn default() -> Self {
        Self {
            control: ControlBase::new(),
            auto_ellipsis: false,
            background_image: None,
            border_style: BorderStyle::None,
            flat_style: FlatStyle::Standard,
            image: None,
            image_align: ContentAlignment::MiddleCenter,
            image_index: -1,
            image_key: String::new(),
            image_list: None,
            live_setting: LiveSetting::Off,
            text_align: ContentAlignment::TopLeft,
            use_compatible_text_rendering: false,
            use_mnemonic: true,
        }
    }
}

impl std::ops::Deref for Label {
    type Target = ControlBase;
    fn deref(&self) -> &ControlBase {
        &self.control
    }
}
impl std::ops::DerefMut for Label {
    fn deref_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }
}

/// How much a border eats on each side, in DIP. `Fixed3D` is the static edge:
/// the toolkit reserves two pixels for it and paints a single sunken ring in the
/// outer one — see [`paint_border`], and the sheet's 32 px `Fixed3D` box around
/// a 28 px line.
fn border_inset(style: BorderStyle) -> f32 {
    match style {
        BorderStyle::None => 0.0,
        BorderStyle::FixedSingle => 1.0,
        BorderStyle::Fixed3D => 2.0,
    }
}

/// The intrinsic size of a label: the text extent, grown by padding and the
/// border on all four sides. Pure so the measurement is tested without a canvas;
/// `preferred_size` feeds it a canvas-measured text extent.
fn label_preferred(text_extent: Size, padding: Padding, border: BorderStyle) -> Size {
    let frame = border_inset(border) * 2.0;
    Size::new(
        text_extent.width + padding.horizontal() + frame,
        text_extent.height + padding.vertical() + frame,
    )
}

impl Label {
    pub fn new() -> Self {
        Self::default()
    }

    /// The font role the label paints with — its own, else the body default.
    fn role(&self) -> FontRole {
        self.control.font.unwrap_or_default()
    }

    /// The text as it will be *drawn*, after mnemonic processing.
    pub fn shown_text(&self) -> String {
        display_text(&self.control.text, self.use_mnemonic)
    }

    /// `PreferredWidth` — the read-only view WinForms exposes.
    pub fn preferred_width(&self, c: &dyn Canvas) -> f32 {
        self.preferred_size(c).width
    }

    /// `PreferredHeight` — the read-only view WinForms exposes.
    pub fn preferred_height(&self, c: &dyn Canvas) -> f32 {
        self.preferred_size(c).height
    }

    /// The client rectangle a label draws its content into: `bounds` deflated by
    /// the border and then by padding.
    ///
    /// TRAP: `bounds` is a **parameter**, never `self.control.bounds`. The two
    /// live in different coordinate spaces — the rectangle handed to `paint` is
    /// in *canvas* space, while `control.bounds` is *parent-relative* — and they
    /// coincide only for a top-level control. Reading the field here made a
    /// container child paint its border in the right place and its text near the
    /// window origin, where the parent's clip swallowed it.
    fn content_rect(&self, bounds: Rect) -> Rect {
        let inset = border_inset(self.border_style);
        let b = bounds;
        let p = self.control.padding;
        Rect::new(
            b.left + inset + p.left,
            b.top + inset + p.top,
            (b.right - inset - p.right).max(b.left + inset + p.left),
            (b.bottom - inset - p.bottom).max(b.top + inset + p.top),
        )
    }

    /// Paints the border, if any — the shared [`paint_border`], so a `Label` and
    /// a `PictureBox` cannot drift into two different frames.
    ///
    /// `FlatStyle` does not reach it: on a label the toolkit's flat styles
    /// differ only under the mouse, and a label is never hot.
    fn paint_border(&self, c: &dyn ControlCanvas, bounds: Rect) {
        paint_border(c, bounds, self.border_style);
    }
}

impl Control for Label {
    fn control(&self) -> &ControlBase {
        &self.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }

    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        let role = self.role();
        let shown = self.shown_text();
        // Measured in the SYSTEM's UI font — the one it is painted in. See
        // `UI_FONT`: a bare `Canvas` cannot answer for the system visuals, so
        // the font is read (and cached) here.
        let fonts = ui_fonts(c);
        let size_dip = fonts.as_ref().map_or(DEFAULT_UI_SIZE_DIP, |f| f.size_dip);
        let width = match fonts.as_ref() {
            Some(f) if !shown.is_empty() => c.measure(&shown, font_format(f, role)),
            _ => 0.0,
        };
        let extent = Size::new(width, line_height(size_dip));
        self.control
            .clamp(label_preferred(extent, self.control.padding, self.border_style))
    }

    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        let v = c.visuals();

        // Background: only when the label carries an explicit colour. An unset
        // (ambient) back colour means « the parent already painted here » — a
        // label is transparent in the toolkit, which is why the sheet's labels
        // sit on the form's own face rather than on a repainted one.
        if let Some(bg) = self.control.back_color {
            c.fill_rect(&bounds, &bg);
        }
        self.paint_border(c, bounds);

        let shown = self.shown_text();
        if shown.is_empty() {
            return;
        }
        let role = self.role();
        let content = self.content_rect(bounds);
        let band = text_band(content, self.text_align, line_height(v.fonts.size_dip));

        let colour = text_color(&v.colors, self.control.fore_color, self.control.enabled);

        let fmt = font_format(&v.fonts, role);
        if self.auto_ellipsis {
            // Ellipsis honours horizontal alignment via the two centre/left
            // variants the canvas exposes; other alignments fall back to left.
            match h_alignment(self.text_align) {
                DWRITE_TEXT_ALIGNMENT_CENTER => c.text_ellipsis_center(&shown, &band, fmt, &colour),
                _ => c.text_ellipsis(&shown, &band, fmt, &colour),
            }
        } else {
            c.text_aligned(&shown, &band, fmt, &colour, h_alignment(self.text_align));
        }
    }

    fn type_name(&self) -> &'static str {
        "Label"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LinkLabel
// ─────────────────────────────────────────────────────────────────────────────

/// How a link is underlined (`System.Windows.Forms.LinkBehavior`). Local to this
/// module because only `LinkLabel` uses it; it could move to `enums.rs` if a
/// second control ever needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkBehavior {
    /// The default. Per the docs, « the behavior of this setting depends on the
    /// options set using the Internet Options dialog box in Control Panel » —
    /// i.e. the *Underline links* option, whose shipped Windows value is
    /// **Always**. See [`LinkBehavior::underlined`] for how the port resolves it.
    #[default]
    SystemDefault,
    /// The link always displays with underlined text.
    AlwaysUnderline,
    /// Underlined only while the pointer is over the link text.
    HoverUnderline,
    /// Never underlined; only `LinkColor` distinguishes it from ordinary text.
    NeverUnderline,
}

impl LinkBehavior {
    /// Whether a link should be underlined given whether the pointer is over it.
    ///
    /// `SystemDefault` resolves to « always underline »: the setting it defers to
    /// is Internet Options' *Underline links*, which ships as **Always** on
    /// Windows. Reading the live registry value would be I/O, which a control is
    /// forbidden to do, so the port resolves to the shipped default rather than
    /// leaving the member unhandled.
    pub const fn underlined(self, hovered: bool) -> bool {
        match self {
            Self::SystemDefault | Self::AlwaysUnderline => true,
            Self::HoverUnderline => hovered,
            Self::NeverUnderline => false,
        }
    }
}

/// `LinkArea` — the character span, into `Text`, of the label's *first* link.
/// A length of zero means « unset »: the whole text becomes one link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LinkArea {
    pub start: i32,
    pub length: i32,
}

impl LinkArea {
    pub const fn new(start: i32, length: i32) -> Self {
        Self { start, length }
    }
    /// `LinkArea.IsEmpty` — start and length both zero.
    pub const fn is_empty(self) -> bool {
        self.start == 0 && self.length == 0
    }
}

/// One entry of `LinkLabel.Links` — a span of the text plus its per-link state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub start: i32,
    pub length: i32,
    /// `Link.Enabled` — a disabled link paints in `DisabledLinkColor` and does
    /// not hit-test.
    pub enabled: bool,
    /// `Link.Visited` — paints in `VisitedLinkColor` once followed.
    pub visited: bool,
    /// `Link.LinkData` — the arbitrary payload the host routes on click.
    pub link_data: Option<String>,
}

impl Link {
    pub fn new(start: i32, length: i32) -> Self {
        Self { start, length, enabled: true, visited: false, link_data: None }
    }

    /// Whether a character index falls inside this link's span.
    pub const fn contains_char(&self, index: i32) -> bool {
        index >= self.start && index < self.start + self.length
    }
}

/// `System.Windows.Forms.LinkLabel` — a `Label` plus a link model. It composes
/// `Label` and derefs to it, so `Text`, `TextAlign`, `AutoSize`, `Padding`,
/// `TabStop` and `UseCompatibleTextRendering` are reached through the label /
/// control beneath — WinForms only re-declares those to tweak their metadata,
/// so they are not restated here.
#[derive(Clone)]
pub struct LinkLabel {
    pub label: Label,

    /// `ActiveLinkColor` — the colour while a link is pressed. Honoured from
    /// [`ControlState::pressed`]. `None` = ambient.
    pub active_link_color: Option<D2D1_COLOR_F>,
    /// `DisabledLinkColor` — a disabled link's colour. `None` = ambient (grey).
    pub disabled_link_color: Option<D2D1_COLOR_F>,
    /// `LinkArea` — the first link's span; see [`LinkArea`].
    pub link_area: LinkArea,
    /// `LinkBehavior` — honoured in [`Control::paint_with_state`], including
    /// `HoverUnderline`.
    ///
    /// Hover and press are **whole-control**, not per-link: [`ControlState`]
    /// reports *that* the pointer is over the control, not *where*, so with two
    /// links in one label both underline together. Resolving it per link needs a
    /// pointer position; [`LinkLabel::link_at_point`] already does that half, so
    /// the day `ControlState` carries coordinates this becomes a one-line change.
    pub link_behavior: LinkBehavior,
    /// `LinkColor` — an unvisited link's colour. `None` = ambient, which
    /// resolves to [`IE_LINK_COLOR`] and *not* to `SystemColors.HotTrack`.
    pub link_color: Option<D2D1_COLOR_F>,
    /// `LinkVisited` — a convenience that marks the *first* link visited.
    pub link_visited: bool,
    /// `VisitedLinkColor` — a followed link's colour. `None` falls back to the
    /// toolkit's own purple; see [`Self::resolved_visited`].
    pub visited_link_color: Option<D2D1_COLOR_F>,

    /// `LinkLabel.Links` — explicit links. When empty, `LinkArea` (or, when that
    /// is empty too, the whole text) defines a single implicit link.
    pub links: Vec<Link>,
}

impl Default for LinkLabel {
    /// Catalogue defaults: `LinkBehavior=SystemDefault`, `LinkVisited=false`,
    /// all colours ambient (`None`). A fresh `LinkLabel` also has `TabStop=true`
    /// (from `ControlBase`), which matches the toolkit.
    fn default() -> Self {
        Self {
            label: Label::new(),
            active_link_color: None,
            disabled_link_color: None,
            link_area: LinkArea::default(),
            link_behavior: LinkBehavior::default(),
            link_color: None,
            link_visited: false,
            visited_link_color: None,
            links: Vec::new(),
        }
    }
}

impl std::ops::Deref for LinkLabel {
    type Target = Label;
    fn deref(&self) -> &Label {
        &self.label
    }
}
impl std::ops::DerefMut for LinkLabel {
    fn deref_mut(&mut self) -> &mut Label {
        &mut self.label
    }
}

impl LinkLabel {
    pub fn new() -> Self {
        Self::default()
    }

    /// The links as WinForms would resolve them for painting and hit-testing:
    /// the explicit `Links` if any, else the `LinkArea`, else — when neither is
    /// set — the entire text as one link. `LinkVisited` marks the first.
    pub fn resolved_links(&self) -> Vec<Link> {
        let mut links = if !self.links.is_empty() {
            self.links.clone()
        } else if !self.link_area.is_empty() {
            vec![Link::new(self.link_area.start, self.link_area.length)]
        } else {
            let len = self.label.control.text.chars().count() as i32;
            if len == 0 {
                Vec::new()
            } else {
                vec![Link::new(0, len)]
            }
        };
        if self.link_visited {
            if let Some(first) = links.first_mut() {
                first.visited = true;
            }
        }
        links
    }

    /// Hit-tests a *character index* to a link, returning its position in
    /// [`Self::resolved_links`]. Disabled links do not match, exactly as the
    /// toolkit refuses to raise `LinkClicked` for them. Pure and testable.
    pub fn link_index_at_char(&self, index: i32) -> Option<usize> {
        self.resolved_links()
            .iter()
            .position(|l| l.enabled && l.contains_char(index))
    }

    /// Maps an (x, y) point in the label to a character index, then to a link.
    /// The point→character step needs measured glyph widths, so it takes a
    /// canvas; the character→link step is the pure [`Self::link_index_at_char`].
    ///
    /// The point is **parent-relative**, the space [`Control::hit_test`] works
    /// in — so this one resolves its content rectangle against `control.bounds`
    /// on purpose, unlike the paint path, which must use the canvas-space
    /// rectangle it is handed. Hit-testing and painting genuinely differ here;
    /// the field read below is the correct one, not the bug's twin.
    pub fn link_at_point(&self, c: &dyn Canvas, x: f32, y: f32) -> Option<usize> {
        let content = self.label.content_rect(self.label.control.bounds);
        if y < content.top || y >= content.bottom || x < content.left {
            return None;
        }
        let role = self.label.role();
        let shown = self.label.shown_text();
        let fonts = ui_fonts(c)?;
        let fmt = font_format(&fonts, role);
        // Walk the prefix widths until the cursor passes the point's x; the
        // index of the first character whose right edge is past x is the hit.
        let rel = x - content.left;
        let count = shown.chars().count();
        for n in 0..count {
            let prefix: String = shown.chars().take(n + 1).collect();
            if c.measure(&prefix, fmt) > rel {
                return self.link_index_at_char(n as i32);
            }
        }
        // Past the last glyph: attribute it to the final character.
        self.link_index_at_char(count.saturating_sub(1) as i32)
    }

    /// The unvisited-link colour. Ambient falls back to [`IE_LINK_COLOR`], the
    /// toolkit's own default — *not* `SystemColors.HotTrack`; see the constant.
    pub fn resolved_link(&self, _c: &dyn ControlCanvas) -> D2D1_COLOR_F {
        self.link_color.unwrap_or(IE_LINK_COLOR)
    }

    /// The active (pressed) link colour; ambient is IE's red.
    pub fn resolved_active(&self, _c: &dyn ControlCanvas) -> D2D1_COLOR_F {
        self.active_link_color.unwrap_or(IE_ACTIVE_LINK_COLOR)
    }

    /// The visited-link colour; ambient is IE's purple, which is what the
    /// reference sheet's followed link is painted in. No `COLOR_*` index carries
    /// a purple, so this one cannot come from [`SystemColors`].
    pub fn resolved_visited(&self, _c: &dyn ControlCanvas) -> D2D1_COLOR_F {
        self.visited_link_color.unwrap_or(IE_VISITED_LINK_COLOR)
    }

    /// The disabled-link colour; ambient is [`DISABLED_LINK_COLOR`], the grey
    /// the toolkit itself answers with — not `COLOR_GRAYTEXT`.
    pub fn resolved_disabled(&self, _c: &dyn ControlCanvas) -> D2D1_COLOR_F {
        self.disabled_link_color.unwrap_or(DISABLED_LINK_COLOR)
    }

    /// Which of the four link colours applies to `link` under `state` — the
    /// *decision*, separated from the canvas that turns it into a colour so the
    /// precedence is unit-tested without a device.
    ///
    /// Order mirrors the toolkit: disabled wins over everything, then
    /// `ActiveLinkColor` while the link is pressed (that is the whole purpose of
    /// the property), then visited, then the ordinary link colour.
    pub fn link_paint(&self, link: &Link, state: ControlState) -> LinkPaint {
        if !self.label.control.enabled || !link.enabled {
            LinkPaint::Disabled
        } else if state.pressed {
            LinkPaint::Active
        } else if link.visited {
            LinkPaint::Visited
        } else {
            LinkPaint::Normal
        }
    }

    /// Whether `link` is underlined under `state`. Pure: it is the whole
    /// interaction of `LinkBehavior`, the pointer and the link's own `Enabled`.
    ///
    /// A disabled link is never underlined — the toolkit paints it as inert
    /// text, and a rule under it would advertise a click it refuses.
    pub fn underlines_link(&self, link: &Link, state: ControlState) -> bool {
        let enabled = self.label.control.enabled && link.enabled;
        enabled && self.link_behavior.underlined(state.hot)
    }

    /// Resolves a [`LinkPaint`] to an actual colour.
    fn paint_colour(&self, c: &dyn ControlCanvas, paint: LinkPaint) -> D2D1_COLOR_F {
        match paint {
            LinkPaint::Disabled => self.resolved_disabled(c),
            LinkPaint::Active => self.resolved_active(c),
            LinkPaint::Visited => self.resolved_visited(c),
            LinkPaint::Normal => self.resolved_link(c),
        }
    }
}

/// Which of `LinkLabel`'s four link colours a link paints in. Exists so the
/// precedence between them is a value a test can assert, rather than a branch
/// buried in a paint method that needs a Direct2D device to reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkPaint {
    /// `DisabledLinkColor`.
    Disabled,
    /// `ActiveLinkColor` — while pressed.
    Active,
    /// `VisitedLinkColor`.
    Visited,
    /// `LinkColor`.
    Normal,
}

impl Control for LinkLabel {
    fn control(&self) -> &ControlBase {
        &self.label.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.label.control
    }

    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        // A link label measures its text exactly like a label.
        self.label.preferred_size(c)
    }

    /// The resting paint — delegates to [`Control::paint_with_state`] with a
    /// default (cold, unpressed) state.
    ///
    /// The delegation runs in *this* direction on purpose: the trait's provided
    /// `paint_with_state` calls `paint`, so a `LinkLabel` that overrode
    /// `paint_with_state` and then let `paint` call it back would recurse.
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        self.paint_with_state(c, bounds, ControlState::default());
    }

    fn paint_with_state(&self, c: &dyn ControlCanvas, bounds: Rect, state: ControlState) {
        let v = c.visuals();
        if let Some(bg) = self.label.control.back_color {
            c.fill_rect(&bounds, &bg);
        }
        self.label.paint_border(c, bounds);

        let shown = self.label.shown_text();
        if shown.is_empty() {
            return;
        }
        let role = self.label.role();
        let fmt = font_format(&v.fonts, role);
        let content = self.label.content_rect(bounds);
        let line_h = line_height(v.fonts.size_dip);
        let band = text_band(content, self.label.text_align, line_h);

        // The non-link text first, in the ordinary foreground colour…
        let base = text_color(
            &v.colors,
            self.label.control.fore_color,
            self.label.control.enabled,
        );
        c.text_aligned(&shown, &band, fmt, &base, DWRITE_TEXT_ALIGNMENT_LEADING);

        // …then each link substring overpainted in its own colour, with an
        // underline drawn as a hairline fill (the canvas has no underline glyph
        // attribute). Hover and press come from the host's `ControlState`, so
        // `HoverUnderline` and `ActiveLinkColor` are both live here; both are
        // whole-control, as documented on `link_behavior`. The two decisions are
        // taken by the pure `link_paint` / `underlines_link`.
        let chars: Vec<char> = shown.chars().collect();
        for link in self.resolved_links() {
            let start = link.start.max(0) as usize;
            let end = ((link.start + link.length).max(0) as usize).min(chars.len());
            if start >= end {
                continue;
            }
            let prefix: String = chars[..start].iter().collect();
            let piece: String = chars[start..end].iter().collect();
            let x0 = content.left + c.measure(&prefix, fmt);
            let w = c.measure(&piece, fmt);
            let piece_rect = Rect::new(x0, band.top, x0 + w, band.bottom);
            let colour = self.paint_colour(c, self.link_paint(&link, state));
            c.text_aligned(&piece, &piece_rect, fmt, &colour, DWRITE_TEXT_ALIGNMENT_LEADING);
            if self.underlines_link(&link, state) {
                let uy = band.bottom - line_h * 0.12;
                // 1 DIP: the canvas is already in DIP (the renderer sets the
                // device DPI), so multiplying by the scale would thicken the
                // rule on a high-DPI display instead of keeping it constant.
                let rule = Rect::new(x0, uy, x0 + w, uy + 1.0);
                c.fill_rect(&rule, &colour);
            }
        }
    }

    fn type_name(&self) -> &'static str {
        "LinkLabel"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PictureBox
// ─────────────────────────────────────────────────────────────────────────────

/// How a `PictureBox` fits its image to its box (`PictureBoxSizeMode`). The
/// discriminants match the toolkit's own so a round-trip is exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PictureBoxSizeMode {
    /// Image at native size, top-left; overflow is clipped.
    #[default]
    Normal = 0,
    /// Image stretched to fill the box, aspect NOT preserved.
    StretchImage = 1,
    /// The box resizes to the image; the image is drawn at native size.
    AutoSize = 2,
    /// Image at native size, centred; overflow clipped.
    CenterImage = 3,
    /// Image scaled to fit, aspect preserved, centred (letter-boxed).
    Zoom = 4,
}

/// `System.Windows.Forms.PictureBox`. `Font`, `ForeColor`, `RightToLeft`,
/// `Text` and `AllowDrop` are inherited from `ControlBase` (WinForms re-declares
/// them only to hide/show them in the designer), so they are not restated.
#[derive(Clone)]
pub struct PictureBox {
    pub control: ControlBase,

    pub border_style: BorderStyle,
    /// `ErrorImage` natural size; pixels host-supplied. Shown when a load fails.
    pub error_image: Option<Size>,
    /// `Image` natural size; the pixels are blitted by the host into the
    /// rectangle [`Self::image_rect`] computes.
    pub image: Option<Size>,
    /// `ImageLocation` — a path/URL the host loads from. The control does no I/O.
    pub image_location: Option<String>,
    /// `InitialImage` natural size; shown while an async load runs.
    pub initial_image: Option<Size>,
    pub size_mode: PictureBoxSizeMode,
    /// `WaitOnLoad` — load synchronously before painting. A host-side flag here.
    pub wait_on_load: bool,
}

impl Default for PictureBox {
    /// Catalogue defaults: `SizeMode=Normal`, `BorderStyle=None`,
    /// `WaitOnLoad=false`, every image unset.
    fn default() -> Self {
        Self {
            control: ControlBase::new(),
            border_style: BorderStyle::None,
            error_image: None,
            image: None,
            image_location: None,
            initial_image: None,
            size_mode: PictureBoxSizeMode::default(),
            wait_on_load: false,
        }
    }
}

impl std::ops::Deref for PictureBox {
    type Target = ControlBase;
    fn deref(&self) -> &ControlBase {
        &self.control
    }
}
impl std::ops::DerefMut for PictureBox {
    fn deref_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }
}

/// Where the image lands inside `client` for a given size mode — the whole of
/// the `PictureBox` geometry, pure so every mode is unit-tested. `client` is the
/// content rectangle (bounds minus border); `image` is the picture's native size.
pub fn image_rect(mode: PictureBoxSizeMode, client: Rect, image: Size) -> Rect {
    let (cw, ch) = (client.right - client.left, client.bottom - client.top);
    let (iw, ih) = (image.width, image.height);
    match mode {
        PictureBoxSizeMode::StretchImage => client,
        // Normal and AutoSize both draw at native size, top-left. (AutoSize
        // additionally resizes the control — see `preferred_size`.)
        PictureBoxSizeMode::Normal | PictureBoxSizeMode::AutoSize => {
            Rect::new(client.left, client.top, client.left + iw, client.top + ih)
        }
        PictureBoxSizeMode::CenterImage => {
            let x = client.left + (cw - iw) / 2.0;
            let y = client.top + (ch - ih) / 2.0;
            Rect::new(x, y, x + iw, y + ih)
        }
        PictureBoxSizeMode::Zoom => {
            if iw <= 0.0 || ih <= 0.0 {
                return client;
            }
            let scale = (cw / iw).min(ch / ih);
            let (w, h) = (iw * scale, ih * scale);
            let x = client.left + (cw - w) / 2.0;
            let y = client.top + (ch - h) / 2.0;
            Rect::new(x, y, x + w, y + h)
        }
    }
}

impl PictureBox {
    pub fn new() -> Self {
        Self::default()
    }

    /// The content rectangle: `bounds` deflated by the border.
    ///
    /// Same trap as [`Label::content_rect`]: `bounds` is a parameter because the
    /// rectangle a control paints into is in canvas space while
    /// `self.control.bounds` is parent-relative.
    fn content_rect(&self, bounds: Rect) -> Rect {
        let inset = border_inset(self.border_style);
        bounds.inflate(-inset, -inset)
    }

    /// The rectangle the host should blit the current image into, given the size
    /// mode. `None` when there is no image.
    ///
    /// Takes the same `bounds` the control is painted into, and returns a
    /// rectangle in that space — a caller positioning a bitmap needs the two to
    /// agree, which they do not if this reads `control.bounds` instead.
    pub fn resolved_image_rect(&self, bounds: Rect) -> Option<Rect> {
        self.image
            .map(|img| image_rect(self.size_mode, self.content_rect(bounds), img))
    }
}

impl Control for PictureBox {
    fn control(&self) -> &ControlBase {
        &self.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }

    fn preferred_size(&self, _c: &dyn Canvas) -> Size {
        let frame = border_inset(self.border_style) * 2.0;
        match (self.size_mode, self.image) {
            // AutoSize snaps the box to the image plus its border.
            (PictureBoxSizeMode::AutoSize, Some(img)) => {
                Size::new(img.width + frame, img.height + frame)
            }
            // Otherwise the toolkit keeps the designer size (default 100×50).
            _ => {
                let s = self.control.size();
                if s.is_empty() {
                    Size::new(100.0, 50.0)
                } else {
                    s
                }
            }
        }
    }

    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        // A `PictureBox` is an OPAQUE control — unlike a label, it grounds its
        // box in its own back colour, `SystemColors.Control` when ambient, and
        // that ground is what shows wherever the image does not reach (the
        // letter-box bands of `Zoom`, the margins of `CenterImage`).
        //
        // The image itself is blitted by the host into `resolved_image_rect`
        // (the bitmap lives host side per the `shell_icon` contract), so paint
        // stops at the ground and the frame.
        let ground = self.control.back_color.unwrap_or(c.visuals().colors.control);
        c.fill_rect(&bounds, &ground);
        paint_border(c, bounds, self.border_style);
    }

    fn type_name(&self) -> &'static str {
        "PictureBox"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ProgressBar
// ─────────────────────────────────────────────────────────────────────────────

/// How a `ProgressBar` renders progress (`ProgressBarStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProgressBarStyle {
    /// Discrete segments filling left-to-right — the classic default.
    #[default]
    Blocks,
    /// A single solid fill.
    Continuous,
    /// A scrolling block with no fixed value (indeterminate).
    Marquee,
}

/// `System.Windows.Forms.ProgressBar`. `Font`, `Text`, `BackgroundImage`,
/// `BackgroundImageLayout` and `AllowDrop` are inherited from `ControlBase`
/// (re-declared by WinForms only for designer metadata) and are not restated.
#[derive(Clone)]
pub struct ProgressBar {
    pub control: ControlBase,

    /// `MarqueeAnimationSpeed` — ms between marquee repaints. The control owns no
    /// timer (see the crate rules); the host ticks it and calls paint.
    pub marquee_animation_speed: i32,
    pub maximum: i32,
    pub minimum: i32,
    /// `RightToLeftLayout` — mirror the fill origin to the right edge.
    pub right_to_left_layout: bool,
    pub step: i32,
    pub style: ProgressBarStyle,
    /// `Value` — kept within `[minimum, maximum]` by every mutator.
    value: i32,
}

impl Default for ProgressBar {
    /// The catalogue's surprising defaults: `Minimum=0`, `Maximum=100`,
    /// `Value=0`, `Step=10`, `MarqueeAnimationSpeed=100`, `Style=Blocks`.
    fn default() -> Self {
        Self {
            control: ControlBase::new(),
            marquee_animation_speed: 100,
            maximum: 100,
            minimum: 0,
            right_to_left_layout: false,
            step: 10,
            style: ProgressBarStyle::default(),
            value: 0,
        }
    }
}

impl std::ops::Deref for ProgressBar {
    type Target = ControlBase;
    fn deref(&self) -> &ControlBase {
        &self.control
    }
}
impl std::ops::DerefMut for ProgressBar {
    fn deref_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }
}

impl ProgressBar {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Value` — always inside the current range.
    pub fn value(&self) -> i32 {
        self.value
    }

    /// Sets `Value`, clamping to `[Minimum, Maximum]`. WinForms *throws* if the
    /// value is out of range; a control library that panics on a setter is worse
    /// than one that clamps, so the port clamps and documents the divergence.
    pub fn set_value(&mut self, v: i32) {
        self.value = v.clamp(self.minimum, self.maximum);
    }

    /// Sets `Minimum`, keeping `Maximum >= Minimum` and re-clamping `Value` — the
    /// same invariant the toolkit maintains when the range moves under the value.
    pub fn set_minimum(&mut self, min: i32) {
        self.minimum = min;
        if self.maximum < self.minimum {
            self.maximum = self.minimum;
        }
        self.value = self.value.clamp(self.minimum, self.maximum);
    }

    /// Sets `Maximum`, keeping `Minimum <= Maximum` and re-clamping `Value`.
    pub fn set_maximum(&mut self, max: i32) {
        self.maximum = max;
        if self.minimum > self.maximum {
            self.minimum = self.maximum;
        }
        self.value = self.value.clamp(self.minimum, self.maximum);
    }

    /// `Increment(delta)` — advances `Value` by `delta`, clamped to the range.
    pub fn increment(&mut self, delta: i32) {
        self.set_value(self.value.saturating_add(delta));
    }

    /// `PerformStep()` — advances `Value` by `Step`.
    ///
    /// TRAP: despite what one might expect of a "step", this does **not** wrap.
    /// Microsoft's docs are explicit — once `Value` would exceed `Maximum` it
    /// *stays at* `Maximum` (and at `Minimum` for a negative `Step`). The port
    /// mirrors that clamp, so a stepped bar fills up and stops, never restarts.
    pub fn perform_step(&mut self) {
        self.increment(self.step);
    }

    /// The filled fraction in `0.0..=1.0`. An empty range reads as full, which is
    /// what the toolkit shows when `Minimum == Maximum`.
    pub fn fraction(&self) -> f32 {
        let span = self.maximum - self.minimum;
        if span <= 0 {
            1.0
        } else {
            (self.value - self.minimum) as f32 / span as f32
        }
    }
}

impl Control for ProgressBar {
    fn control(&self) -> &ControlBase {
        &self.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }

    fn preferred_size(&self, _c: &dyn Canvas) -> Size {
        // No content to measure; keep the designer size, defaulting to the
        // toolkit's 100×23 when unset.
        let s = self.control.size();
        if s.is_empty() {
            Size::new(100.0, 23.0)
        } else {
            s
        }
    }

    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        // The trough: a flat one-pixel frame around a light well, then the fill.
        //
        // NOT a `DrawEdge` bevel. A `ProgressBar` is a comctl32 v6 control, so
        // on Windows 11 it is drawn by the *theme* (`PP_BAR` / `PP_CHUNK`) and
        // not by the classic 3-D recipe: the reference sheet shows one even
        // `#BCBCBC` ring on all four sides, over a `#E6E6E6` well — where a
        // sunken edge would put `COLOR_BTNHIGHLIGHT` (white) along the bottom
        // and right. Asking `draw_edge` for a sunken frame here would look
        // *more* like WinForms 1.0 and *less* like the toolkit on this machine.
        //
        // None of the three colours is a `SystemColors` entry (the nearest,
        // `COLOR_3DLIGHT`, reads `#E3E3E3` against the well's `#E6E6E6`), so
        // they are sampled from `05-labels.png` at the coordinates named on each
        // constant and will follow the theme only when the theme is read.
        c.fill_rect(&bounds, &BAR_TROUGH);
        c.stroke_rect(&bounds, &BAR_FRAME);

        let inner = bounds.inflate(-1.0, -1.0);
        let iw = (inner.right - inner.left).max(0.0);
        let fill = BAR_FILL;

        match self.style {
            ProgressBarStyle::Continuous => {
                let w = iw * self.fraction();
                let r = Rect::new(inner.left, inner.top, inner.left + w, inner.bottom);
                c.fill_rect(&r, &fill);
            }
            ProgressBarStyle::Blocks => {
                // Discrete chunks: each ~0.6× the bar height wide, with a small
                // gap, filling only up to the current fraction.
                let filled = iw * self.fraction();
                let h = inner.bottom - inner.top;
                let block = (h * 0.6).max(2.0);
                let gap = (block * 0.25).max(1.0);
                let mut x = inner.left;
                while x + block <= inner.left + filled + 0.01 {
                    let r = Rect::new(x, inner.top, x + block, inner.bottom);
                    c.fill_rect(&r, &fill);
                    x += block + gap;
                }
            }
            ProgressBarStyle::Marquee => {
                // Indeterminate: one scrolling block. With no timer the control
                // paints a single frame — a block of ~1/3 the width parked at the
                // left; the host advances it using `marquee_animation_speed`.
                let w = iw / 3.0;
                let r = Rect::new(inner.left, inner.top, inner.left + w, inner.bottom);
                c.fill_rect(&r, &fill);
            }
        }
    }

    fn type_name(&self) -> &'static str {
        "ProgressBar"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Declared defaults, asserted against the catalogue ────────────────

    #[test]
    fn label_defaults_match_the_catalogue() {
        let l = Label::new();
        assert!(!l.auto_ellipsis);
        assert_eq!(l.border_style, BorderStyle::None);
        assert_eq!(l.flat_style, FlatStyle::Standard);
        assert_eq!(l.image_index, -1);
        assert_eq!(l.image_key, "");
        assert_eq!(l.image_align, ContentAlignment::MiddleCenter);
        assert_eq!(l.text_align, ContentAlignment::TopLeft);
        assert!(l.use_mnemonic);
        assert!(!l.use_compatible_text_rendering);
        assert_eq!(l.live_setting, LiveSetting::Off);
        // AutoSize is inherited, and false by default — the label surprise.
        assert!(!l.control.auto_size);
    }

    #[test]
    fn linklabel_defaults_match_the_catalogue() {
        let ll = LinkLabel::new();
        assert_eq!(ll.link_behavior, LinkBehavior::SystemDefault);
        assert!(!ll.link_visited);
        assert!(ll.link_color.is_none());
        assert!(ll.active_link_color.is_none());
        assert!(ll.visited_link_color.is_none());
        assert!(ll.links.is_empty());
        assert!(ll.link_area.is_empty());
        // Inherited chain still holds: TabStop true.
        assert!(ll.label.control.tab_stop);
    }

    #[test]
    fn picturebox_defaults_match_the_catalogue() {
        let p = PictureBox::new();
        assert_eq!(p.size_mode, PictureBoxSizeMode::Normal);
        assert_eq!(p.border_style, BorderStyle::None);
        assert!(!p.wait_on_load);
        assert!(p.image.is_none());
    }

    #[test]
    fn progressbar_defaults_match_the_catalogue() {
        let pb = ProgressBar::new();
        assert_eq!(pb.minimum, 0);
        assert_eq!(pb.maximum, 100);
        assert_eq!(pb.value(), 0);
        assert_eq!(pb.step, 10);
        assert_eq!(pb.marquee_animation_speed, 100);
        assert_eq!(pb.style, ProgressBarStyle::Blocks);
        assert!(!pb.right_to_left_layout);
    }

    // ── Label AutoSize measurement arithmetic (pure part) ────────────────

    #[test]
    fn label_preferred_adds_padding_and_border() {
        let text = Size::new(40.0, 15.0);
        // No border, no padding: the extent passes straight through.
        assert_eq!(label_preferred(text, Padding::ZERO, BorderStyle::None), text);
        // Padding grows both axes by its sums.
        let p = Padding::new(5.0, 4.0, 3.0, 2.0);
        let with_pad = label_preferred(text, p, BorderStyle::None);
        assert_eq!(with_pad, Size::new(40.0 + 8.0, 15.0 + 6.0));
        // A Fixed3D border reserves two DIP on every side (four total per axis).
        let with_border = label_preferred(text, Padding::ZERO, BorderStyle::Fixed3D);
        assert_eq!(with_border, Size::new(40.0 + 4.0, 15.0 + 4.0));
    }

    #[test]
    fn mnemonic_stripping_matches_winforms() {
        // Lone `&` marks an access key and is removed from the drawn text.
        assert_eq!(display_text("&Save", true), "Save");
        // `&&` collapses to a single literal ampersand.
        assert_eq!(display_text("Fish && Chips", true), "Fish & Chips");
        // With UseMnemonic off, everything is verbatim.
        assert_eq!(display_text("&Save", false), "&Save");
        // Trailing lone `&` just disappears.
        assert_eq!(display_text("A&", true), "A");
    }

    #[test]
    fn text_band_positions_the_line_vertically() {
        let content = Rect::new(0.0, 0.0, 100.0, 30.0);
        let top = text_band(content, ContentAlignment::TopLeft, 10.0);
        assert_eq!(top.top, 0.0);
        let mid = text_band(content, ContentAlignment::MiddleCenter, 10.0);
        assert_eq!(mid.top, 10.0); // (30-10)*0.5
        let bot = text_band(content, ContentAlignment::BottomRight, 10.0);
        assert_eq!(bot.top, 20.0); // (30-10)*1.0
    }

    // ── ProgressBar value clamping and stepping ──────────────────────────

    #[test]
    fn progressbar_clamps_value_to_the_range() {
        let mut pb = ProgressBar::new();
        pb.set_value(50);
        assert_eq!(pb.value(), 50);
        pb.set_value(999);
        assert_eq!(pb.value(), 100, "clamped to Maximum");
        pb.set_value(-10);
        assert_eq!(pb.value(), 0, "clamped to Minimum");
    }

    #[test]
    fn progressbar_reclamps_value_when_the_range_moves() {
        let mut pb = ProgressBar::new();
        pb.set_value(80);
        pb.set_maximum(50);
        assert_eq!(pb.value(), 50, "value follows a shrinking Maximum down");
        pb.set_minimum(60);
        assert_eq!(pb.maximum, 60, "Maximum is pushed up to keep the range valid");
        assert_eq!(pb.value(), 60);
    }

    /// TRAP: PerformStep CLAMPS at Maximum — it does not wrap back to Minimum.
    #[test]
    fn perform_step_clamps_at_maximum_it_does_not_wrap() {
        let mut pb = ProgressBar::new();
        pb.maximum = 25;
        pb.step = 10;
        pb.perform_step(); // 10
        pb.perform_step(); // 20
        assert_eq!(pb.value(), 20);
        pb.perform_step(); // would be 30 → clamped to 25, NOT 5
        assert_eq!(pb.value(), 25);
        pb.perform_step(); // stays at 25
        assert_eq!(pb.value(), 25);
    }

    #[test]
    fn progressbar_fraction_is_relative_to_minimum() {
        let mut pb = ProgressBar::new();
        pb.set_minimum(100);
        pb.set_maximum(200);
        pb.set_value(150);
        assert!((pb.fraction() - 0.5).abs() < 1e-6);
        // An empty range reads as full, not a divide-by-zero.
        let mut deg = ProgressBar::new();
        deg.set_maximum(0);
        assert_eq!(deg.fraction(), 1.0);
    }

    // ── PictureBox geometry per SizeMode ─────────────────────────────────

    const CLIENT: Rect = Rect { left: 0.0, top: 0.0, right: 200.0, bottom: 100.0 };

    #[test]
    fn stretch_fills_the_whole_client() {
        let r = image_rect(PictureBoxSizeMode::StretchImage, CLIENT, Size::new(40.0, 40.0));
        assert_eq!((r.left, r.top, r.right, r.bottom), (0.0, 0.0, 200.0, 100.0));
    }

    #[test]
    fn normal_draws_native_size_at_top_left() {
        let r = image_rect(PictureBoxSizeMode::Normal, CLIENT, Size::new(40.0, 30.0));
        assert_eq!((r.left, r.top, r.right, r.bottom), (0.0, 0.0, 40.0, 30.0));
    }

    #[test]
    fn center_places_native_size_in_the_middle() {
        let r = image_rect(PictureBoxSizeMode::CenterImage, CLIENT, Size::new(40.0, 20.0));
        // (200-40)/2 = 80 ; (100-20)/2 = 40
        assert_eq!((r.left, r.top, r.right, r.bottom), (80.0, 40.0, 120.0, 60.0));
    }

    #[test]
    fn zoom_preserves_aspect_and_centres() {
        // A 100×100 image into a 200×100 box fits by height: scale 1.0, 100×100,
        // centred horizontally → left inset (200-100)/2 = 50.
        let r = image_rect(PictureBoxSizeMode::Zoom, CLIENT, Size::new(100.0, 100.0));
        assert_eq!((r.left, r.top, r.right, r.bottom), (50.0, 0.0, 150.0, 100.0));
        // A wide 200×50 image into the same box fits by width: scale 1.0,
        // 200×50, centred vertically → top inset (100-50)/2 = 25.
        let r2 = image_rect(PictureBoxSizeMode::Zoom, CLIENT, Size::new(200.0, 50.0));
        assert_eq!((r2.left, r2.top, r2.right, r2.bottom), (0.0, 25.0, 200.0, 75.0));
    }

    // ── The coordinate-space invariant ───────────────────────────────────
    //
    // A control is painted into the rectangle the layout hands it, in CANVAS
    // space; `control.bounds` is PARENT-RELATIVE. For a top-level control the
    // two coincide, which is why reading the field instead of the argument
    // passed every earlier test and still lost the text of every container
    // child. These tests keep the two spaces deliberately different.

    /// A label's content rectangle must follow the `bounds` it is painted into,
    /// not its own parent-relative `control.bounds`.
    #[test]
    fn label_content_rect_follows_the_argument_not_the_field() {
        let mut l = Label::new();
        // Parent-relative position: 10 DIP into a container…
        l.control.set_bounds(Rect::new(10.0, 10.0, 110.0, 40.0));
        l.padding = Padding::all(2.0);
        l.border_style = BorderStyle::FixedSingle; // 1 DIP

        // …but the container sits at (500, 300) on the canvas, so paint receives
        // a rectangle in a completely different place.
        let painted = Rect::new(500.0, 300.0, 600.0, 330.0);
        let content = l.content_rect(painted);

        // 1 (border) + 2 (padding) inset from the PAINTED rectangle.
        assert_eq!(
            (content.left, content.top, content.right, content.bottom),
            (503.0, 303.0, 597.0, 327.0),
            "content must be resolved against the painted bounds"
        );
        assert!(
            content.left > 100.0,
            "a content rect near the origin means control.bounds leaked in"
        );
    }

    /// The same invariant for `PictureBox`, where it also reaches the public
    /// `resolved_image_rect` — a caller blitting a bitmap gets this rectangle.
    #[test]
    fn picturebox_image_rect_follows_the_argument_not_the_field() {
        let mut p = PictureBox::new();
        p.control.set_bounds(Rect::new(5.0, 5.0, 205.0, 105.0)); // parent-relative
        p.size_mode = PictureBoxSizeMode::CenterImage;
        p.image = Some(Size::new(40.0, 20.0));

        let painted = Rect::new(400.0, 200.0, 600.0, 300.0); // canvas space, 200×100
        let r = p.resolved_image_rect(painted).expect("an image is set");

        // Centred in the PAINTED box: 400 + (200-40)/2 = 480 ; 200 + (100-20)/2 = 240.
        assert_eq!((r.left, r.top, r.right, r.bottom), (480.0, 240.0, 520.0, 260.0));
    }

    /// `ProgressBar` was never affected: it paints straight from `bounds` and
    /// reads no position off itself. This pins that down so it stays true.
    #[test]
    fn progressbar_geometry_is_purely_a_function_of_the_painted_bounds() {
        let mut pb = ProgressBar::new();
        pb.control.set_bounds(Rect::new(9.0, 9.0, 59.0, 29.0)); // deliberately unrelated
        pb.set_value(50);
        // The only geometry a ProgressBar derives is the filled fraction, which
        // is a pure function of the value and the range — no coordinates at all.
        assert!((pb.fraction() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn autosize_content_rect_insets_by_the_border() {
        let mut p = PictureBox::new();
        p.size_mode = PictureBoxSizeMode::AutoSize;
        p.image = Some(Size::new(64.0, 48.0));
        p.border_style = BorderStyle::FixedSingle; // 1 DIP each side
        // The content rect is deflated by the 1-DIP border on every side.
        let inner = p.content_rect(Rect::new(0.0, 0.0, 66.0, 50.0));
        assert_eq!((inner.left, inner.top, inner.right, inner.bottom), (1.0, 1.0, 65.0, 49.0));
        // AutoSize geometry draws the image at native size, top-left of content.
        let r = image_rect(PictureBoxSizeMode::AutoSize, inner, Size::new(64.0, 48.0));
        assert_eq!((r.right - r.left, r.bottom - r.top), (64.0, 48.0));
    }

    // ── LinkLabel link model and hit-testing ─────────────────────────────

    #[test]
    fn implicit_link_covers_the_whole_text_when_unset() {
        let mut ll = LinkLabel::new();
        ll.label.control.text = "documentation".to_string(); // 13 chars
        let links = ll.resolved_links();
        assert_eq!(links.len(), 1);
        assert_eq!((links[0].start, links[0].length), (0, 13));
    }

    #[test]
    fn link_area_defines_the_first_link() {
        let mut ll = LinkLabel::new();
        ll.label.control.text = "Visit the docs page".to_string();
        ll.link_area = LinkArea::new(10, 4); // "docs"
        let links = ll.resolved_links();
        assert_eq!(links.len(), 1);
        assert_eq!((links[0].start, links[0].length), (10, 4));
    }

    #[test]
    fn char_hit_testing_finds_the_right_link_and_skips_disabled() {
        let mut ll = LinkLabel::new();
        ll.label.control.text = "Register Online. Visit MSN.".to_string();
        let mut a = Link::new(0, 8); // "Register"
        a.link_data = Some("register".into());
        let mut b = Link::new(23, 3); // "MSN"
        b.enabled = false; // disabled → must never match
        ll.links = vec![a, b];

        assert_eq!(ll.link_index_at_char(3), Some(0), "inside the first link");
        assert_eq!(ll.link_index_at_char(9), None, "in the gap between links");
        assert_eq!(ll.link_index_at_char(24), None, "inside the DISABLED link");
    }

    #[test]
    fn link_visited_marks_the_first_link() {
        let mut ll = LinkLabel::new();
        ll.label.control.text = "hello".to_string();
        ll.link_visited = true;
        assert!(ll.resolved_links()[0].visited);
    }

    #[test]
    fn link_behavior_underline_rules() {
        // SystemDefault resolves to « always »: the Internet Options setting it
        // defers to ships as Always on Windows.
        assert!(LinkBehavior::SystemDefault.underlined(false));
        assert!(LinkBehavior::AlwaysUnderline.underlined(false));
        assert!(!LinkBehavior::HoverUnderline.underlined(false));
        assert!(LinkBehavior::HoverUnderline.underlined(true));
        assert!(!LinkBehavior::NeverUnderline.underlined(true));
    }

    // ── LinkLabel under a ControlState ───────────────────────────────────

    /// A `LinkLabel` with one enabled link over the whole text.
    fn one_link() -> LinkLabel {
        let mut ll = LinkLabel::new();
        ll.label.control.text = "docs".to_string();
        ll
    }

    const COLD: ControlState = ControlState { hot: false, pressed: false, focused: false, default: false };
    const HOT: ControlState = ControlState { hot: true, pressed: false, focused: false, default: false };
    const DOWN: ControlState = ControlState { hot: true, pressed: true, focused: false, default: false };

    #[test]
    fn hover_underline_needs_the_pointer_and_the_others_do_not_care() {
        let mut ll = one_link();
        let link = Link::new(0, 4);

        ll.link_behavior = LinkBehavior::HoverUnderline;
        assert!(!ll.underlines_link(&link, COLD), "cold: no rule");
        assert!(ll.underlines_link(&link, HOT), "hot: underlined");

        ll.link_behavior = LinkBehavior::AlwaysUnderline;
        assert!(ll.underlines_link(&link, COLD));
        assert!(ll.underlines_link(&link, HOT));

        ll.link_behavior = LinkBehavior::NeverUnderline;
        assert!(!ll.underlines_link(&link, COLD));
        assert!(!ll.underlines_link(&link, HOT));

        ll.link_behavior = LinkBehavior::SystemDefault;
        assert!(ll.underlines_link(&link, COLD), "SystemDefault ≡ AlwaysUnderline");
    }

    #[test]
    fn a_disabled_link_is_never_underlined() {
        let mut ll = one_link();
        ll.link_behavior = LinkBehavior::AlwaysUnderline;
        let mut link = Link::new(0, 4);
        link.enabled = false;
        assert!(!ll.underlines_link(&link, HOT));
        // A disabled *control* suppresses it too, even for an enabled link.
        ll.label.control.enabled = false;
        assert!(!ll.underlines_link(&Link::new(0, 4), HOT));
    }

    /// The precedence between the four link colours, which is easy to get wrong:
    /// disabled beats pressed, pressed beats visited, visited beats normal.
    #[test]
    fn link_colour_precedence_is_disabled_then_active_then_visited() {
        let ll = one_link();
        let plain = Link::new(0, 4);
        let mut visited = Link::new(0, 4);
        visited.visited = true;
        let mut off = Link::new(0, 4);
        off.enabled = false;

        assert_eq!(ll.link_paint(&plain, COLD), LinkPaint::Normal);
        assert_eq!(ll.link_paint(&visited, COLD), LinkPaint::Visited);
        // ActiveLinkColor is exactly what `pressed` is for, and it outranks
        // Visited — a followed link still flashes active while held down.
        assert_eq!(ll.link_paint(&plain, DOWN), LinkPaint::Active);
        assert_eq!(ll.link_paint(&visited, DOWN), LinkPaint::Active);
        // Disabled outranks everything, pressed included.
        assert_eq!(ll.link_paint(&off, DOWN), LinkPaint::Disabled);

        // A disabled control paints every link disabled.
        let mut dead = one_link();
        dead.label.control.enabled = false;
        assert_eq!(dead.link_paint(&plain, COLD), LinkPaint::Disabled);
    }

    /// Hovering is merely *state*: it must never mutate the control, and the
    /// resting `paint` must be the `hot: false` case of `paint_with_state`.
    #[test]
    fn resting_paint_is_the_default_state() {
        let ll = one_link();
        let link = Link::new(0, 4);
        assert_eq!(ControlState::default(), COLD);
        assert_eq!(ll.link_paint(&link, ControlState::default()), ll.link_paint(&link, COLD));
        assert_eq!(
            ll.underlines_link(&link, ControlState::default()),
            ll.underlines_link(&link, COLD)
        );
    }
}
