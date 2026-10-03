//! The **system** visuals — the colours, metrics and UI font Windows itself
//! publishes, and the 3-D edge recipe that turns them into a WinForms border.
//!
//! ## Why this file exists
//!
//! The controls in this crate reproduce the WinForms surface, and the oracle
//! they are compared against is the **local toolkit**: the reference sheets in
//! `tools/winforms-ref/shots/` are painted by the real `System.Windows.Forms`
//! on *this* machine. A replica that paints with the Kubuno palette and the
//! embedded Outfit face can never match those sheets, however good it
//! looks — it is answering a different question.
//!
//! So every visual fact a control needs is **read from the system**, never
//! written down here:
//!
//! * [`SystemColors`] ← `GetSysColor`, the same `COLOR_*` indices .NET's
//!   `System.Drawing.SystemColors` reads.
//! * [`SystemMetrics`] ← `GetSystemMetricsForDpi`, the same `SM_*` indices
//!   `SystemInformation` reads.
//! * [`SystemFonts`] ← `SystemParametersInfoForDpi(SPI_GETNONCLIENTMETRICS)`,
//!   whose `lfMessageFont` is what .NET Core's `Control.DefaultFont`
//!   (`SystemFonts.MessageBoxFont`) resolves to — Segoe UI 9 pt on a default
//!   Windows 11, and whatever the user configured on a machine that is not.
//!
//! Hard-coding any of them would make the port right on one machine and wrong
//! on every other, and — worse — would make it *silently* wrong, since the
//! comparison would still be run against the local toolkit.
//!
//! ## Two unit systems meet here
//!
//! Win32 answers in **physical pixels**; the canvas draws in **DIP** (the
//! `Renderer` calls `SetDpi`, so a rectangle at `y = 10.0` lands at 10 DIP).
//! Every metric is therefore converted on the way in ([`metric_to_dip`]) and
//! every field of [`SystemMetrics`] is already in DIP — a family never scales
//! anything, exactly as `docs/AGENT_BRIEF.md` requires.

use kubuno_drive_desktop_app_controls::geometry::Rect;
use windows::core::{Result, HSTRING};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::{
    IDWriteFactory, IDWriteTextFormat, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE,
    DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Gdi::{
    GetSysColor, LOGFONTW, COLOR_3DDKSHADOW, COLOR_3DLIGHT, COLOR_APPWORKSPACE, COLOR_BTNFACE,
    COLOR_BTNHIGHLIGHT, COLOR_BTNSHADOW, COLOR_BTNTEXT, COLOR_GRAYTEXT, COLOR_HIGHLIGHT,
    COLOR_HIGHLIGHTTEXT, COLOR_HOTLIGHT, COLOR_INACTIVEBORDER, COLOR_INFOBK, COLOR_INFOTEXT,
    COLOR_MENUBAR, COLOR_MENUTEXT, COLOR_WINDOW, COLOR_WINDOWFRAME, COLOR_WINDOWTEXT,
    SYS_COLOR_INDEX,
};
use windows::Win32::UI::HiDpi::{GetSystemMetricsForDpi, SystemParametersInfoForDpi};
use windows::Win32::UI::WindowsAndMessaging::{
    NONCLIENTMETRICSW, SM_CXBORDER, SM_CXEDGE, SM_CXFOCUSBORDER, SM_CXHSCROLL, SM_CXMENUCHECK,
    SM_CXVSCROLL, SM_CYBORDER, SM_CYEDGE, SM_CYFOCUSBORDER, SM_CYHSCROLL, SM_CYMENU,
    SM_CYMENUCHECK, SM_CYVSCROLL, SPI_GETNONCLIENTMETRICS, SYSTEM_METRICS_INDEX,
};

// ─────────────────────────────────────────────────────────────────────────────
// Colours
// ─────────────────────────────────────────────────────────────────────────────

/// A `COLORREF` — what `GetSysColor` returns — as a Direct2D colour.
///
/// **The byte order is the classic bug.** A `COLORREF` is `0x00BBGGRR`: the
/// LOW byte is RED, not blue, which is the reverse of the `0x00RRGGBB` literal
/// everyone writes by hand. Reading it as `0x00RRGGBB` compiles, runs, and
/// silently swaps every colour's red and blue channels — a `ControlDark` grey
/// still looks grey (its channels are equal), so the mistake hides until the
/// first coloured system colour (`Highlight`, `HotTrack`) comes out orange
/// instead of blue.
///
/// The alpha byte of a `COLORREF` is *not* an alpha channel (it carries flags
/// for `PALETTEINDEX`/`PALETTERGB`), so it is discarded and the result is fully
/// opaque — which is what every system colour is.
pub fn colorref_to_d2d(colorref: u32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: (colorref & 0xFF) as f32 / 255.0,
        g: ((colorref >> 8) & 0xFF) as f32 / 255.0,
        b: ((colorref >> 16) & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

/// The `COLOR_*` system colours a WinForms control paints with.
///
/// The names are .NET's (`System.Drawing.SystemColors`), not Win32's, because
/// that is the vocabulary the catalogue and the toolkit's own painting code
/// use — `ControlDark` rather than `COLOR_BTNSHADOW`. The Win32 index each one
/// reads is named in its doc comment so the mapping stays checkable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SystemColors {
    /// `COLOR_BTNFACE` — the face of every button, panel and dialog.
    pub control:             D2D1_COLOR_F,
    /// `COLOR_BTNTEXT` — text on a `Control`-coloured surface.
    pub control_text:        D2D1_COLOR_F,
    /// `COLOR_BTNSHADOW` — the inner half of a sunken/raised bevel.
    pub control_dark:        D2D1_COLOR_F,
    /// `COLOR_3DDKSHADOW` — the outer, darkest ring of a bevel.
    pub control_dark_dark:   D2D1_COLOR_F,
    /// `COLOR_3DLIGHT` — the lighter of the two highlights.
    pub control_light:       D2D1_COLOR_F,
    /// `COLOR_BTNHIGHLIGHT` — the brightest highlight (white by default).
    pub control_light_light: D2D1_COLOR_F,
    /// `COLOR_WINDOW` — the field background of a text box, list or tree.
    pub window:              D2D1_COLOR_F,
    /// `COLOR_WINDOWTEXT` — text on a `Window`-coloured surface.
    pub window_text:         D2D1_COLOR_F,
    /// `COLOR_WINDOWFRAME` — the flat 1 px frame of `BorderStyle::FixedSingle`.
    pub window_frame:        D2D1_COLOR_F,
    /// `COLOR_HIGHLIGHT` — selection background.
    pub highlight:           D2D1_COLOR_F,
    /// `COLOR_HIGHLIGHTTEXT` — text over a selection.
    pub highlight_text:      D2D1_COLOR_F,
    /// `COLOR_GRAYTEXT` — the one colour a disabled control paints its text in.
    pub gray_text:           D2D1_COLOR_F,
    /// `COLOR_INACTIVEBORDER`.
    pub inactive_border:     D2D1_COLOR_F,
    /// `COLOR_APPWORKSPACE` — the MDI client area behind child forms.
    pub app_workspace:       D2D1_COLOR_F,
    /// `COLOR_INFOBK` — tooltip background.
    pub info_background:     D2D1_COLOR_F,
    /// `COLOR_INFOTEXT` — tooltip text.
    pub info_text:           D2D1_COLOR_F,
    /// `COLOR_MENUBAR` — the `MenuStrip` band (distinct from `COLOR_MENU`,
    /// which is the drop-down's background).
    pub menu_bar:            D2D1_COLOR_F,
    /// `COLOR_MENUTEXT`.
    pub menu_text:           D2D1_COLOR_F,
    /// `COLOR_BTNSHADOW` again, under .NET's other name for it. Kept as its own
    /// field because the toolkit exposes both and a family should be free to
    /// name the one it means.
    pub button_shadow:       D2D1_COLOR_F,
    /// `COLOR_BTNHIGHLIGHT` again — see [`SystemColors::button_shadow`].
    pub button_highlight:    D2D1_COLOR_F,
    /// `COLOR_HOTLIGHT` — the hyperlink / hot-tracked colour.
    pub hot_track:           D2D1_COLOR_F,
}

impl SystemColors {
    /// Reads the whole set from Windows.
    ///
    /// Not DPI-dependent — colours are the same at every scale — so this is
    /// read once per [`Visuals`] and never per frame.
    pub fn read() -> Self {
        let c = |index: SYS_COLOR_INDEX| colorref_to_d2d(unsafe { GetSysColor(index) });
        Self {
            control:             c(COLOR_BTNFACE),
            control_text:        c(COLOR_BTNTEXT),
            control_dark:        c(COLOR_BTNSHADOW),
            control_dark_dark:   c(COLOR_3DDKSHADOW),
            control_light:       c(COLOR_3DLIGHT),
            control_light_light: c(COLOR_BTNHIGHLIGHT),
            window:              c(COLOR_WINDOW),
            window_text:         c(COLOR_WINDOWTEXT),
            window_frame:        c(COLOR_WINDOWFRAME),
            highlight:           c(COLOR_HIGHLIGHT),
            highlight_text:      c(COLOR_HIGHLIGHTTEXT),
            gray_text:           c(COLOR_GRAYTEXT),
            inactive_border:     c(COLOR_INACTIVEBORDER),
            app_workspace:       c(COLOR_APPWORKSPACE),
            info_background:     c(COLOR_INFOBK),
            info_text:           c(COLOR_INFOTEXT),
            menu_bar:            c(COLOR_MENUBAR),
            menu_text:           c(COLOR_MENUTEXT),
            button_shadow:       c(COLOR_BTNSHADOW),
            button_highlight:    c(COLOR_BTNHIGHLIGHT),
            hot_track:           c(COLOR_HOTLIGHT),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Metrics
// ─────────────────────────────────────────────────────────────────────────────

/// A `GetSystemMetricsForDpi` answer, in **physical pixels**, converted to the
/// **DIP** the canvas draws in.
///
/// `GetSystemMetricsForDpi(SM_CXVSCROLL, 120)` returns 21 — twenty-one *device*
/// pixels. The Direct2D space is already scaled, so drawing `21.0` there would
/// produce 26 device pixels at 125 %. The honest value is `21 * 96 / 120 =
/// 16.8` DIP, which lands back on 21 device pixels.
///
/// Note that the DIP value therefore **varies slightly with the DPI** (16.8 at
/// 120, 17.0 at 96): the system rounds to whole pixels at each scale, and
/// reproducing that rounding is the whole point of asking per-DPI rather than
/// scaling a 96-DPI answer ourselves.
pub fn metric_to_dip(px: i32, dpi: f32) -> f32 {
    px as f32 * 96.0 / dpi.max(1.0)
}

/// The `SM_*` metrics a WinForms control needs, **already in DIP**.
///
/// Read through `GetSystemMetricsForDpi` rather than `GetSystemMetrics`: the
/// latter answers for the process's *primary* DPI awareness context, so on a
/// per-monitor-v2 process (which the host is) it silently returns the 96 DPI
/// values whatever monitor the window is on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SystemMetrics {
    /// The DPI these metrics were read at — [`Visuals`] compares it to decide
    /// whether it must be rebuilt.
    pub dpi: f32,

    /// `SM_CXVSCROLL` — the width of a vertical scroll bar (17 DIP at 96).
    pub vertical_scroll_width:    f32,
    /// `SM_CYHSCROLL` — the height of a horizontal scroll bar.
    pub horizontal_scroll_height: f32,
    /// `SM_CXHSCROLL` — the width of a horizontal scroll bar's **arrow
    /// button**. Deliberately not the bar's own width: the X/Y pairing is
    /// inverted for the arrow metrics, which is why both are carried.
    pub horizontal_arrow_width:   f32,
    /// `SM_CYVSCROLL` — the height of a vertical scroll bar's arrow button.
    pub vertical_arrow_height:    f32,

    /// `SM_CXBORDER` — one window border, the thinnest line the system draws.
    pub border_width:       f32,
    /// `SM_CYBORDER`.
    pub border_height:      f32,
    /// `SM_CXEDGE` — a 3-D edge, i.e. two `SM_CXBORDER` rings.
    pub edge_width:         f32,
    /// `SM_CYEDGE`.
    pub edge_height:        f32,
    /// `SM_CXFOCUSBORDER` — the focus-rectangle line width.
    pub focus_border_width: f32,
    /// `SM_CYFOCUSBORDER`.
    pub focus_border_height: f32,

    /// `SM_CYMENU` — the height of a single-line menu bar.
    pub menu_height:      f32,
    /// `SM_CXMENUCHECK` — the check-mark cell in a menu item.
    pub menu_check_width: f32,
    /// `SM_CYMENUCHECK`.
    pub menu_check_height: f32,
}

impl SystemMetrics {
    /// Reads every metric at `dpi`.
    pub fn read(dpi: f32) -> Self {
        let d = dpi.max(1.0);
        let m = |index: SYSTEM_METRICS_INDEX| {
            metric_to_dip(unsafe { GetSystemMetricsForDpi(index, d as u32) }, d)
        };
        Self {
            dpi: d,
            vertical_scroll_width:    m(SM_CXVSCROLL),
            horizontal_scroll_height: m(SM_CYHSCROLL),
            horizontal_arrow_width:   m(SM_CXHSCROLL),
            vertical_arrow_height:    m(SM_CYVSCROLL),
            border_width:             m(SM_CXBORDER),
            border_height:            m(SM_CYBORDER),
            edge_width:               m(SM_CXEDGE),
            edge_height:              m(SM_CYEDGE),
            focus_border_width:       m(SM_CXFOCUSBORDER),
            focus_border_height:      m(SM_CYFOCUSBORDER),
            menu_height:              m(SM_CYMENU),
            menu_check_width:         m(SM_CXMENUCHECK),
            menu_check_height:        m(SM_CYMENUCHECK),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fonts
// ─────────────────────────────────────────────────────────────────────────────

/// A `LOGFONT` height, as `SPI_GETNONCLIENTMETRICS` reports it, in **points**.
///
/// `lfHeight` is **negative** by convention: a negative value is the *character*
/// height (the em size), which is what both `CreateFont` and DirectWrite mean by
/// « font size ». A positive value would be the *cell* height, em size plus
/// internal leading, which cannot be converted back without the face's own
/// metrics — see [`logfont_height_to_dip`] for what is done in that case.
///
/// The magnitude is in logical units at the DPI of the query, so
/// `points = -lfHeight * 72 / dpi`. The default Windows 11 message font is
/// `lfHeight = -12` at 96 DPI, which is `12 * 72 / 96 = 9` pt.
pub fn logfont_height_to_points(lf_height: i32, dpi: f32) -> f32 {
    lf_height.abs() as f32 * 72.0 / dpi.max(1.0)
}

/// The same height in **DIP**, which is what `CreateTextFormat` takes.
///
/// `dip = -lfHeight * 96 / dpi`, i.e. the point size times 96/72. The default
/// Windows 11 message font (`lfHeight = -12` at 96 DPI, Segoe UI 9 pt) gives
/// `12 * 96 / 96 = 12.0` DIP — the number WinForms measures every control
/// against, and the one the unit test below pins.
///
/// A **positive** `lfHeight` is treated as if it were negative. It is a cell
/// height, so this over-states the em size by the face's internal leading; the
/// alternative is refusing to paint. `SPI_GETNONCLIENTMETRICS` has never been
/// observed to report one, so the approximation is documented rather than
/// engineered around.
pub fn logfont_height_to_dip(lf_height: i32, dpi: f32) -> f32 {
    lf_height.abs() as f32 * 96.0 / dpi.max(1.0)
}

/// The real UI font, resolved from the system and turned into DirectWrite text
/// formats.
///
/// On .NET Core / .NET 5+, `Control.DefaultFont` is `SystemFonts.MessageBoxFont`
/// — `NONCLIENTMETRICS.lfMessageFont`. That is what every reference sheet was
/// rendered with, so it is what the port measures and paints with.
#[derive(Clone, Debug)]
pub struct SystemFonts {
    /// The resolved face name (`Segoe UI` on a default Windows 11). Exposed so
    /// the parity harness can assert that the port and the toolkit are
    /// measuring the *same* font, not merely landing on similar pixels.
    pub family:     String,
    /// The resolved size in **points** (`9.0` on a default Windows 11), for the
    /// same reason.
    pub point_size: f32,
    /// The same size in DIP — `point_size * 96 / 72`, `12.0` by default.
    pub size_dip:   f32,
    /// `lfWeight` as read (400 = normal).
    pub weight:     i32,

    /// The message font: what a control paints its `Text` with.
    pub message:        IDWriteTextFormat,
    /// The message font at bold weight — `MonthCalendar`'s bolded days, a
    /// `ToolStripStatusLabel` marked so, a `TabPage` header.
    pub message_bold:   IDWriteTextFormat,
    /// The message font in italic, for the few controls that offer it.
    pub message_italic: IDWriteTextFormat,
}

impl SystemFonts {
    /// Reads `lfMessageFont` **at `dpi`** and builds the formats from it.
    ///
    /// `SystemParametersInfoForDpi` rather than `SystemParametersInfoW`: the
    /// non-DPI form answers for the system DPI, so on a 168 DPI monitor the font
    /// would come back sized for 96 while [`SystemMetrics`] came back sized for
    /// 168 — text and metrics disagreeing is exactly the failure this crate
    /// exists to avoid.
    pub fn read(dwrite: &IDWriteFactory, dpi: f32) -> Result<Self> {
        let dpi = dpi.max(1.0);
        let lf = message_logfont(dpi);
        let family = face_name(&lf);
        // A blank face name means the query failed or the machine is configured
        // in a way we cannot read. Falling back to the system's own default UI
        // family keeps the window painting; the family field then says so.
        let family = if family.is_empty() { "Segoe UI".to_string() } else { family };
        let point_size = logfont_height_to_points(lf.lfHeight, dpi);
        let size_dip = logfont_height_to_dip(lf.lfHeight, dpi);
        // A zero height would produce a zero-sized format, which DirectWrite
        // rejects; 12 DIP is the documented default (9 pt at 96 DPI).
        let size_dip = if size_dip > 0.0 { size_dip } else { 12.0 };
        let point_size = if point_size > 0.0 { point_size } else { 9.0 };

        let locale = user_locale();
        let style =
            if lf.lfItalic != 0 { DWRITE_FONT_STYLE_ITALIC } else { DWRITE_FONT_STYLE_NORMAL };
        let weight = DWRITE_FONT_WEIGHT(lf.lfWeight);

        let make = |weight: DWRITE_FONT_WEIGHT, style: DWRITE_FONT_STYLE| -> Result<_> {
            create_format(dwrite, &family, weight, style, size_dip, &locale)
        };

        Ok(Self {
            message: make(weight, style)?,
            message_bold: make(DWRITE_FONT_WEIGHT_BOLD, style)?,
            message_italic: make(weight, DWRITE_FONT_STYLE_ITALIC)?,
            family,
            point_size,
            size_dip,
            weight: lf.lfWeight,
        })
    }
}

/// `NONCLIENTMETRICS.lfMessageFont` at `dpi`, or a zeroed `LOGFONTW` if the
/// query fails (which [`SystemFonts::read`] turns into the documented default).
fn message_logfont(dpi: f32) -> LOGFONTW {
    let mut ncm = NONCLIENTMETRICSW {
        // Windows validates `cbSize` and refuses the call without it — the
        // struct grew a field (`iPaddedBorderWidth`) in Vista and the size is
        // how the kernel knows which shape it is filling.
        cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    let ok = unsafe {
        SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS.0,
            std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
            Some(&mut ncm as *mut _ as *mut core::ffi::c_void),
            0,
            dpi.max(1.0) as u32,
        )
    };
    if ok.is_err() {
        return LOGFONTW::default();
    }
    ncm.lfMessageFont
}

/// The NUL-terminated `lfFaceName` as a `String`.
fn face_name(lf: &LOGFONTW) -> String {
    let end = lf.lfFaceName.iter().position(|&c| c == 0).unwrap_or(lf.lfFaceName.len());
    String::from_utf16_lossy(&lf.lfFaceName[..end])
}

/// The user's locale, which DirectWrite uses for font fallback and shaping.
/// Falls back to `en-us` — the value every DirectWrite sample passes — rather
/// than an empty string, which the API rejects.
fn user_locale() -> String {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buf = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
    let len = unsafe { GetUserDefaultLocaleName(&mut buf) };
    if len <= 1 {
        return "en-us".to_string();
    }
    // The count includes the terminating NUL.
    String::from_utf16_lossy(&buf[..(len as usize - 1)])
}

fn create_format(
    dwrite: &IDWriteFactory,
    family: &str,
    weight: DWRITE_FONT_WEIGHT,
    style: DWRITE_FONT_STYLE,
    size_dip: f32,
    locale: &str,
) -> Result<IDWriteTextFormat> {
    unsafe {
        let format = dwrite.CreateTextFormat(
            &HSTRING::from(family),
            None, // the SYSTEM collection: the toolkit's font, not an embedded one
            weight,
            style,
            DWRITE_FONT_STRETCH_NORMAL,
            size_dip,
            &HSTRING::from(locale),
        )?;
        // WinForms lays a control's `Text` out on one line unless the control
        // itself wraps, so the format must not wrap behind a family's back.
        format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        Ok(format)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The bundle
// ─────────────────────────────────────────────────────────────────────────────

/// Everything a control needs from the system to paint like the toolkit.
///
/// Built **once per DPI** and rebuilt when the DPI changes: the colours never
/// change, but both the metrics and the font are DPI-dependent, and rebuilding
/// the three together is what keeps them consistent with one another.
#[derive(Clone, Debug)]
pub struct Visuals {
    pub colors:  SystemColors,
    pub metrics: SystemMetrics,
    pub fonts:   SystemFonts,
}

impl Visuals {
    /// Reads the whole set at `dpi`.
    pub fn read(dwrite: &IDWriteFactory, dpi: f32) -> Result<Self> {
        Ok(Self {
            colors:  SystemColors::read(),
            metrics: SystemMetrics::read(dpi),
            fonts:   SystemFonts::read(dwrite, dpi)?,
        })
    }

    /// The DPI these visuals were read at.
    pub fn dpi(&self) -> f32 {
        self.metrics.dpi
    }

    /// Whether they are still valid for `dpi`. The host asks this every frame
    /// rather than trusting `WM_DPICHANGED` alone — a paint can arrive before
    /// the message does, and stale metrics are invisible until something is
    /// measured a pixel wrong.
    pub fn matches_dpi(&self, dpi: f32) -> bool {
        (self.metrics.dpi - dpi.max(1.0)).abs() < 0.5
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3-D edges
// ─────────────────────────────────────────────────────────────────────────────

/// `System.Windows.Forms.Border3DStyle` — the shapes `ControlPaint.DrawBorder3D`
/// (and, under it, Win32's `DrawEdge`) can draw.
///
/// A WinForms border is not a stroke: it is **two concentric one-pixel rings**,
/// each with a light side (top + left) and a dark side (bottom + right). That
/// two-tone bevel is what makes a classic control read as raised or sunken, and
/// it is why the rounded strokes this library already had could not express it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Border3DStyle {
    /// `BF_ADJUST` — draws nothing. Kept because it is a real member of the
    /// enum: a caller passes it to ask for the geometry without the paint.
    Adjust,
    /// `EDGE_BUMP` — raised outer, sunken inner: a ridge.
    Bump,
    /// `EDGE_ETCHED` — sunken outer, raised inner: the engraved line a
    /// `GroupBox` frames itself with.
    Etched,
    /// `BF_FLAT` — a single flat ring, no bevel.
    Flat,
    /// `EDGE_RAISED` — raised outer *and* inner: a resting `Button`.
    #[default]
    Raised,
    /// `BDR_RAISEDINNER` alone.
    RaisedInner,
    /// `BDR_RAISEDOUTER` alone.
    RaisedOuter,
    /// `EDGE_SUNKEN` — sunken outer *and* inner: a `TextBox`'s `Fixed3D` well,
    /// and a pressed `Button`.
    Sunken,
    /// `BDR_SUNKENINNER` alone.
    SunkenInner,
    /// `BDR_SUNKENOUTER` alone — the single-ring inset of a `ComboBox` button.
    SunkenOuter,
}

/// `System.Windows.Forms.Border3DSide` — which sides of the box the edge is
/// drawn on. `ALL` is what a framed control wants; the single sides exist for
/// separators, which are one etched line rather than a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Border3DSide(pub u8);

impl Border3DSide {
    pub const LEFT:   Self = Self(1);
    pub const TOP:    Self = Self(2);
    pub const RIGHT:  Self = Self(4);
    pub const BOTTOM: Self = Self(8);
    pub const ALL:    Self = Self(15);

    pub fn has(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl Default for Border3DSide {
    fn default() -> Self {
        Self::ALL
    }
}

/// The four colours a 3-D edge is painted with — outer and inner ring, each
/// split into its light side (top + left) and its dark side (bottom + right).
///
/// `None` means « this ring is not drawn », which is how a single-ring style
/// (`RaisedOuter`, `SunkenInner`, …) is expressed without a second enum.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Edge3D {
    pub outer_light: Option<D2D1_COLOR_F>,
    pub outer_dark:  Option<D2D1_COLOR_F>,
    pub inner_light: Option<D2D1_COLOR_F>,
    pub inner_dark:  Option<D2D1_COLOR_F>,
}

impl Edge3D {
    /// How many one-pixel rings this edge actually paints — 0, 1 or 2. The
    /// caller deflates by this many pixels to find the interior.
    pub fn rings(&self) -> u32 {
        u32::from(self.outer_light.is_some() || self.outer_dark.is_some())
            + u32::from(self.inner_light.is_some() || self.inner_dark.is_some())
    }
}

/// The two-tone recipe for each style, from the system colours.
///
/// The mapping is the classic (non-themed) `DrawEdge` one, stated per `BDR_*`
/// flag rather than per composite style — which is how the four composite
/// `EDGE_*` styles stay consistent with the four single-ring ones:
///
/// | flag | top + left | bottom + right |
/// |---|---|---|
/// | `BDR_RAISEDOUTER` | `ControlLight` | `ControlDarkDark` |
/// | `BDR_RAISEDINNER` | `ControlLightLight` | `ControlDark` |
/// | `BDR_SUNKENOUTER` | `ControlDark` | `ControlLightLight` |
/// | `BDR_SUNKENINNER` | `ControlDarkDark` | `ControlLight` |
///
/// Cross-checked against the two edges whose appearance is unmistakable:
/// `Etched` (sunken outer + raised inner) comes out as a dark line with a white
/// line under it — the `GroupBox` groove — and `Sunken` comes out as the
/// dark-then-darker inset of a `Fixed3D` text box.
pub fn edge_colors(colors: &SystemColors, style: Border3DStyle) -> Edge3D {
    let raised_outer = (colors.control_light, colors.control_dark_dark);
    let raised_inner = (colors.control_light_light, colors.control_dark);
    let sunken_outer = (colors.control_dark, colors.control_light_light);
    let sunken_inner = (colors.control_dark_dark, colors.control_light);

    let (outer, inner) = match style {
        Border3DStyle::Adjust => (None, None),
        // A flat border is one ring in a single colour on all four sides; the
        // bevel is deliberately absent, which is the whole point of the style.
        Border3DStyle::Flat => {
            return Edge3D {
                outer_light: Some(colors.control_dark),
                outer_dark:  Some(colors.control_dark),
                inner_light: None,
                inner_dark:  None,
            }
        }
        Border3DStyle::Bump => (Some(raised_outer), Some(sunken_inner)),
        Border3DStyle::Etched => (Some(sunken_outer), Some(raised_inner)),
        Border3DStyle::Raised => (Some(raised_outer), Some(raised_inner)),
        Border3DStyle::RaisedOuter => (Some(raised_outer), None),
        Border3DStyle::RaisedInner => (None, Some(raised_inner)),
        Border3DStyle::Sunken => (Some(sunken_outer), Some(sunken_inner)),
        Border3DStyle::SunkenOuter => (Some(sunken_outer), None),
        Border3DStyle::SunkenInner => (None, Some(sunken_inner)),
    };
    Edge3D {
        outer_light: outer.map(|(l, _)| l),
        outer_dark:  outer.map(|(_, d)| d),
        inner_light: inner.map(|(l, _)| l),
        inner_dark:  inner.map(|(_, d)| d),
    }
}

/// `rect` shrunk by `rings` one-physical-pixel rings — the interior a 3-D edge
/// leaves behind, which is what `DrawEdge` reports under `BF_ADJUST`.
///
/// `scale` is the canvas's, so the inset is one *device* pixel per ring however
/// the display is scaled: a bevel is always hairline-thin, never 2 DIP wide at
/// 200 %.
pub fn edge_interior(rect: &Rect, rings: u32, scale: f32) -> Rect {
    let t = rings as f32 / scale.max(0.01);
    Rect::new(
        rect.left + t,
        rect.top + t,
        (rect.right - t).max(rect.left + t),
        (rect.bottom - t).max(rect.top + t),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The classic bug: a `COLORREF` is `0x00BBGGRR`, so the LOW byte is red.
    #[test]
    fn a_colorref_is_bgr_not_rgb() {
        // Pure red as Win32 writes it: RGB(255, 0, 0) == 0x000000FF.
        let red = colorref_to_d2d(0x0000_00FF);
        assert_eq!((red.r, red.g, red.b, red.a), (1.0, 0.0, 0.0, 1.0));
        // Pure blue: RGB(0, 0, 255) == 0x00FF0000.
        let blue = colorref_to_d2d(0x00FF_0000);
        assert_eq!((blue.r, blue.g, blue.b), (0.0, 0.0, 1.0));
        // A value whose channels are all different, so a swap cannot hide:
        // RGB(0x12, 0x34, 0x56) == 0x00563412.
        let mixed = colorref_to_d2d(0x0056_3412);
        assert_eq!(mixed.r, 0x12 as f32 / 255.0);
        assert_eq!(mixed.g, 0x34 as f32 / 255.0);
        assert_eq!(mixed.b, 0x56 as f32 / 255.0);
    }

    /// Reading it as `0x00RRGGBB` — the literal form everyone types — must give
    /// a DIFFERENT answer, or the test above proves nothing.
    #[test]
    fn reading_a_colorref_as_rgb_would_swap_red_and_blue() {
        let c = colorref_to_d2d(0x0000_00FF);
        assert_ne!(c.b, 1.0, "0x000000FF is RED in COLORREF, not blue");
    }

    /// The system answers in physical pixels; the canvas draws in DIP.
    #[test]
    fn a_metric_converts_from_physical_pixels_to_dip() {
        // 96 DPI: the two units coincide.
        assert_eq!(metric_to_dip(17, 96.0), 17.0);
        // 120 DPI (125 %): the system reports 21 px for SM_CXVSCROLL, which is
        // 16.8 DIP — drawing 21.0 there would give 26 device pixels.
        assert!((metric_to_dip(21, 120.0) - 16.8).abs() < 1e-4);
        // 168 DPI (175 %): 30 px is 17.142… DIP.
        assert!((metric_to_dip(30, 168.0) - 30.0 * 96.0 / 168.0).abs() < 1e-4);
        // Round-tripping back to device pixels must land on the original.
        for (px, dpi) in [(17, 96.0f32), (21, 120.0), (30, 168.0)] {
            let back = metric_to_dip(px, dpi) * dpi / 96.0;
            assert!((back - px as f32).abs() < 1e-3, "{px} px at {dpi} dpi");
        }
        // A nonsensical DPI must not divide by zero.
        assert!(metric_to_dip(17, 0.0).is_finite());
    }

    /// The default Windows 11 message font is Segoe UI 9 pt, reported as
    /// `lfHeight = -12` at 96 DPI. Both conversions are pinned on it.
    #[test]
    fn the_default_message_font_is_nine_points_and_twelve_dip() {
        assert_eq!(logfont_height_to_points(-12, 96.0), 9.0);
        assert_eq!(logfont_height_to_dip(-12, 96.0), 12.0);
    }

    /// The same 9 pt font read at a higher DPI comes back with a bigger
    /// `lfHeight`, and must convert back to the SAME point size and the same
    /// DIP size — that is what makes text and metrics agree at any scale.
    #[test]
    fn the_logfont_height_is_dpi_relative() {
        // 9 pt at 120 DPI: -MulDiv(9, 120, 72) = -15.
        assert_eq!(logfont_height_to_points(-15, 120.0), 9.0);
        assert_eq!(logfont_height_to_dip(-15, 120.0), 12.0);
        // 9 pt at 168 DPI: -MulDiv(9, 168, 72) = -21.
        assert_eq!(logfont_height_to_points(-21, 168.0), 9.0);
        assert_eq!(logfont_height_to_dip(-21, 168.0), 12.0);
    }

    /// A positive `lfHeight` is a cell height, not an em size. It is accepted
    /// (see the doc comment) rather than producing a zero-sized font.
    #[test]
    fn a_positive_logfont_height_is_not_treated_as_a_negative_size() {
        assert_eq!(logfont_height_to_dip(12, 96.0), 12.0);
        assert!(logfont_height_to_dip(0, 96.0) == 0.0, "zero is caught by the caller");
    }

    /// The recipe is stated per ring, so a composite style and the single-ring
    /// styles it is made of must agree.
    #[test]
    fn a_composite_edge_is_its_two_single_rings() {
        let c = fake_colors();
        let raised = edge_colors(&c, Border3DStyle::Raised);
        let outer = edge_colors(&c, Border3DStyle::RaisedOuter);
        let inner = edge_colors(&c, Border3DStyle::RaisedInner);
        assert_eq!(raised.outer_light, outer.outer_light);
        assert_eq!(raised.outer_dark, outer.outer_dark);
        assert_eq!(raised.inner_light, inner.inner_light);
        assert_eq!(raised.inner_dark, inner.inner_dark);
        assert_eq!(raised.rings(), 2);
        assert_eq!(outer.rings(), 1);
    }

    /// Etched is the `GroupBox` groove: dark on top-left, light under it. If
    /// this ever inverts, every group box reads as embossed instead of engraved.
    #[test]
    fn an_etched_edge_is_dark_then_light() {
        let c = fake_colors();
        let e = edge_colors(&c, Border3DStyle::Etched);
        assert_eq!(e.outer_light, Some(c.control_dark));
        assert_eq!(e.outer_dark, Some(c.control_light_light));
        assert_eq!(e.inner_light, Some(c.control_light_light));
        assert_eq!(e.inner_dark, Some(c.control_dark));
    }

    #[test]
    fn adjust_draws_nothing_and_flat_draws_one_ring() {
        let c = fake_colors();
        assert_eq!(edge_colors(&c, Border3DStyle::Adjust).rings(), 0);
        let flat = edge_colors(&c, Border3DStyle::Flat);
        assert_eq!(flat.rings(), 1);
        assert_eq!(flat.outer_light, flat.outer_dark, "a flat edge has no bevel");
    }

    /// The interior is inset one DEVICE pixel per ring, so a bevel stays
    /// hairline-thin at any scale.
    #[test]
    fn the_interior_is_inset_one_device_pixel_per_ring() {
        let r = Rect::new(0.0, 0.0, 100.0, 50.0);
        let at_100 = edge_interior(&r, 2, 1.0);
        assert_eq!((at_100.left, at_100.right), (2.0, 98.0));
        let at_200 = edge_interior(&r, 2, 2.0);
        assert_eq!((at_200.left, at_200.right), (1.0, 99.0));
        // A box smaller than its own edge must not invert.
        let tiny = edge_interior(&Rect::new(0.0, 0.0, 1.0, 1.0), 2, 1.0);
        assert!(tiny.right >= tiny.left && tiny.bottom >= tiny.top);
    }

    #[test]
    fn the_sides_are_a_bit_set() {
        assert!(Border3DSide::ALL.has(Border3DSide::TOP));
        assert!(!Border3DSide::LEFT.has(Border3DSide::RIGHT));
        assert_eq!(Border3DSide::default(), Border3DSide::ALL);
    }

    /// The one test that talks to Windows.
    ///
    /// It needs no device, no window and no COM apartment — a DirectWrite
    /// factory, `GetSysColor` and `SystemParametersInfoForDpi` are all
    /// available to a bare test binary — so it belongs here rather than in a
    /// harness. It asserts INVARIANTS rather than values: the point is that the
    /// read wired up correctly, not that this machine is configured like any
    /// other. `cargo test -- --nocapture` prints what it actually found.
    #[test]
    fn the_real_system_answers_coherently() {
        use windows::Win32::Graphics::DirectWrite::{
            DWriteCreateFactory, DWRITE_FACTORY_TYPE_SHARED,
        };
        let dwrite: IDWriteFactory =
            unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.expect("DirectWrite");
        let v = Visuals::read(&dwrite, 96.0).expect("system visuals at 96 dpi");

        eprintln!(
            "[system] font = {:?} {} pt ({} DIP, weight {})",
            v.fonts.family, v.fonts.point_size, v.fonts.size_dip, v.fonts.weight
        );
        eprintln!("[system] control = {:?}", v.colors.control);
        eprintln!("[system] SM_CXVSCROLL = {} DIP", v.metrics.vertical_scroll_width);

        assert!(!v.fonts.family.is_empty(), "the UI font must have a name");
        assert!(v.fonts.point_size > 0.0 && v.fonts.point_size < 72.0);
        // The two sizes are the same fact in two units, and 96/72 apart.
        assert!((v.fonts.size_dip - v.fonts.point_size * 96.0 / 72.0).abs() < 1e-3);
        // Every system colour is opaque: a `COLORREF`'s high byte is flags, not
        // alpha, and reading it as alpha would give a mostly-invisible palette.
        assert_eq!(v.colors.control.a, 1.0);
        assert_eq!(v.colors.highlight.a, 1.0);
        // A scroll bar has a width, a menu has a height. Zero here means the
        // metric was asked for with the wrong index.
        assert!(v.metrics.vertical_scroll_width > 0.0);
        assert!(v.metrics.menu_height > 0.0);
        assert!(v.metrics.border_width > 0.0);
        assert_eq!(v.dpi(), 96.0);
        assert!(v.matches_dpi(96.0) && !v.matches_dpi(120.0));

        // The same read at 168 DPI must give the SAME point size (the font is a
        // user preference, not a scale) and LARGER physical metrics — expressed
        // in DIP they stay within a pixel of the 96 DPI answer.
        let hi = Visuals::read(&dwrite, 168.0).expect("system visuals at 168 dpi");
        assert!((hi.fonts.point_size - v.fonts.point_size).abs() < 0.5);
        assert!((hi.fonts.size_dip - v.fonts.size_dip).abs() < 0.5);
        assert!(
            (hi.metrics.vertical_scroll_width - v.metrics.vertical_scroll_width).abs() < 1.5,
            "a scroll bar is the same logical width at every scale"
        );
    }

    /// Four distinguishable colours, so a mixed-up field shows as a failure
    /// rather than as two equal greys.
    fn fake_colors() -> SystemColors {
        let g = |v: f32| D2D1_COLOR_F { r: v, g: v, b: v, a: 1.0 };
        let mut c = SystemColors {
            control:             g(0.75),
            control_text:        g(0.0),
            control_dark:        g(0.5),
            control_dark_dark:   g(0.25),
            control_light:       g(0.85),
            control_light_light: g(1.0),
            window:              g(1.0),
            window_text:         g(0.0),
            window_frame:        g(0.1),
            highlight:           g(0.2),
            highlight_text:      g(1.0),
            gray_text:           g(0.4),
            inactive_border:     g(0.6),
            app_workspace:       g(0.35),
            info_background:     g(0.95),
            info_text:           g(0.05),
            menu_bar:            g(0.9),
            menu_text:           g(0.0),
            button_shadow:       g(0.5),
            button_highlight:    g(1.0),
            hot_track:           g(0.3),
        };
        // Make the four bevel colours pairwise distinct beyond doubt.
        c.control_dark_dark.b = 0.24;
        c.control_light.b = 0.86;
        c
    }
}
