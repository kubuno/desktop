//! The Kubuno window chrome: the title band every Kubuno window wears — a top-level `Form` (drawn
//! by the host under `Chrome::Kubuno`), a dialog, a tool window, the designer's picture of a form
//! (`kubuno-views`' design surface) and the in-window `FloatingWindow` of `kubuno-ui`.
//!
//! **One painter, one geometry.** All of them call [`layout`], [`paint_band`] and
//! [`paint_caption`], so a form at run time, the same form in the Visual Studio designer and a
//! floating window inside a page are the same pixels.
//!
//! The source of truth is the web component `core/frontend/src/ui/FloatingWindow.tsx` and its
//! rules in `core/frontend/src/index.css` / `theme.css`:
//!
//! | web | here |
//! |---|---|
//! | `.kb-window-titlebar` `min-h-11 py-2.5` around the 30 px button | [`TITLEBAR_HEIGHT`] = 50 |
//! | `px-4`, `gap-2.5` | [`PAD_X`] = 16, [`GAP`] = 10 |
//! | `background: var(--color-primary)`, `color: var(--kb-window-title-fg)` (white) | `theme.accent`, `theme.accent_foreground` |
//! | title `font-medium`, `--kb-text-heading` | `formats().heading` |
//! | close `w-[30px] h-[30px] rounded-[5px]`, `X size={15}` | [`BUTTON`] = 30, [`BUTTON_RADIUS`] = 5, [`BUTTON_GLYPH`] = 15 |
//! | `opacity-80 hover:opacity-100 hover:bg-white/20` | [`REST_ALPHA`], [`HOVER_WASH`] |
//! | `.kb-window-actions` `gap-1` (title actions) | [`BUTTON_GAP`] = 4 |
//! | `--kb-window-radius: 0px` (square corners, decision of 2026-08-30) | [`WINDOW_RADIUS`] |
//! | `--kb-shadow-window: 0 6px 18px rgb(0 0 0 / 24%)` | `shape::SHADOW_WINDOW` |
//! | `.kb-window-footer` `px-4 py-3`, `border-top` | [`FOOTER_PAD_X`], [`FOOTER_PAD_Y`] |
//!
//! What the web has no equivalent for is extrapolated from the same rules and said so where it is
//! defined: minimise / maximise / help buttons (the close button's exact geometry, as the web
//! gives every title action), the subtitle, the centred title, the slim tool-window band, the
//! Windows-style caption buttons (opt-in), and the pressed state.

use drive_app_controls::themes::shape::{height, space};
use drive_app_controls::{Canvas, Rect, Theme};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap1;

use crate::host::form::CaptionButtonState;

/// The band's height as the web renders it: `.kb-window-titlebar` is `min-h-11` (44) but also
/// `py-2.5` around the 30 px close button, so it lays out at 10 + 30 + 10 = **50** (measured on the
/// live web: `getBoundingClientRect().height == 50`).
pub const TITLEBAR_HEIGHT: f32 = 2.0 * 10.0 + BUTTON;
/// A tool window's slim band (`FixedToolWindow` / `SizableToolWindow`). The web has no tool
/// window; 32 is the design system's small control height (`h-8`), so the band still holds a
/// 24 DIP button with 4 DIP of air above and below.
pub const TOOL_TITLEBAR_HEIGHT: f32 = height::BUTTON_SM;
/// `px-4`: the band's side insets.
pub const PAD_X: f32 = space::LG;
/// `gap-2.5`: between the icon, the title, the title actions and the close button.
pub const GAP: f32 = 10.0;
/// The band's leading glyph (the web's callers pass `size={14..16}`; the desktop draws 16).
pub const ICON: f32 = 16.0;
/// The close button: `w-[30px] h-[30px] rounded-[5px]` around an `X size={15}`. Every caption
/// button takes that exact geometry (`.kb-window-titlebar .kb-window-actions button`).
pub const BUTTON: f32 = 30.0;
pub const BUTTON_RADIUS: f32 = 5.0;
pub const BUTTON_GLYPH: f32 = 15.0;
/// `gap-1` between two title actions.
pub const BUTTON_GAP: f32 = space::XS;
/// The slim band's buttons and insets (extrapolated: 24 = 32 − 2 × 4, radius 4 = `--radius-sm`).
pub const TOOL_BUTTON: f32 = 24.0;
pub const TOOL_BUTTON_RADIUS: f32 = 4.0;
pub const TOOL_BUTTON_GLYPH: f32 = 13.0;
pub const TOOL_PAD_X: f32 = space::SM;
/// Windows-style caption buttons (opt-in, [`ButtonStyle::Windows`]): 46 DIP wide, full height.
pub const WINDOWS_BUTTON_W: f32 = 46.0;
/// Their height: 32 DIP, at the top of a taller band (a band of 32 or less is filled).
pub const WINDOWS_BUTTON_H: f32 = 32.0;
/// `--kb-window-radius: 0px`.
pub const WINDOW_RADIUS: f32 = 0.0;
/// `opacity-80` on a caption button's glyph at rest, `hover:opacity-100`.
pub const REST_ALPHA: f32 = 0.8;
/// `hover:bg-white/20`: the band's own ink at a fifth of its alpha (reads on any accent).
pub const HOVER_WASH: f32 = 0.2;
/// Pressed (`:active`) — the web has none; one step darker than the hover.
pub const PRESSED_WASH: f32 = 0.3;
/// A disabled button's ink (WinForms greys `MinimizeBox = false` next to a live maximise button).
pub const DISABLED_ALPHA: f32 = 0.4;
/// `.kb-window-footer`: `px-4 py-3`, `gap-2`, around a `md` (36) button.
pub const FOOTER_PAD_X: f32 = space::LG;
pub const FOOTER_PAD_Y: f32 = space::MD;
pub const FOOTER_GAP: f32 = space::SM;
pub const FOOTER_HEIGHT: f32 = 2.0 * space::MD + height::BUTTON_MD;
/// The resize grip of a resizable window (`.kb-window-grip`: 18 × 18, two oblique strokes).
pub const GRIP: f32 = 18.0;

/// How the caption buttons look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonStyle {
    /// The web `FloatingWindow`'s: 30 × 30 rounded boxes, a white veil on hover (the default).
    #[default]
    Kubuno,
    /// Windows 11's: 46 DIP wide and full height, flush with the corner; close turns red on hover.
    Windows,
}

/// Where the title sits in the band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TitleAlignment {
    /// After the icon (the web's).
    #[default]
    Left,
    /// Centred on the band (macOS / GNOME-style; extrapolated).
    Center,
}

/// A part of the band a pointer can be on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    Minimize,
    Maximize,
    Close,
    /// WinForms' `HelpButton`.
    Help,
    /// One of the window's own caption buttons ([`ChromeStyle::commands`]), by index.
    Command(usize),
}

/// A caption button of the window's own, next to minimise / maximise / close.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CaptionCommand {
    /// What the `CaptionButtonClick` event reports.
    pub id: String,
    /// A Lucide glyph name (`Settings2`, `Bell`, `Pin`…).
    pub glyph: String,
    /// Its tooltip and accessible name.
    pub tooltip: String,
    pub enabled: bool,
    /// A toggle that is on: drawn with the hover veil at rest.
    pub checked: bool,
}

impl CaptionCommand {
    pub fn new(id: impl Into<String>, glyph: impl Into<String>) -> Self {
        Self { id: id.into(), glyph: glyph.into(), tooltip: String::new(), enabled: true, checked: false }
    }
}

/// Everything about the band a window may customise (`TitleBarHeight`, `TitleBarBackground`,
/// `Subtitle`, `TitleAlignment`, `CaptionButtonStyle`, `HelpButton`, `ExtendContentIntoTitleBar`…).
/// The default is the web `FloatingWindow`'s band.
#[derive(Debug, Clone, PartialEq)]
pub struct ChromeStyle {
    /// `None`: [`TITLEBAR_HEIGHT`], or [`TOOL_TITLEBAR_HEIGHT`] for a tool window.
    pub height: Option<f32>,
    /// The band's side insets (`TitleBarPadding`), between the window's edges and what the band holds at
    /// its ends (the icon or the left region, Kubuno-style caption buttons). `None`: [`PAD_X`], or
    /// [`TOOL_PAD_X`] for a tool window.
    pub padding: Option<f32>,
    /// `None`: the theme's accent (`--color-primary`), so the band follows the theme and the module.
    pub background: Option<D2D1_COLOR_F>,
    /// `None`: `accent_foreground` (white on the accent).
    pub foreground: Option<D2D1_COLOR_F>,
    /// Shown after the title, smaller and dimmer (extrapolated: `--kb-text-meta` at 80 %).
    pub subtitle: String,
    pub alignment: TitleAlignment,
    pub buttons: ButtonStyle,
    /// Whether the window's icon shows in the band (`ShowIcon`).
    pub show_icon: bool,
    /// Whether the title shows at all (a band with only tabs in it).
    pub show_title: bool,
    /// The help button (`HelpButton`).
    pub help_button: bool,
    /// The window's own caption buttons, drawn left of the help / minimise buttons.
    pub commands: Vec<CaptionCommand>,
    /// The page paints under the band (`ExtendContentIntoTitleBar`): the band is not filled unless a
    /// background is set explicitly; the title and the buttons are drawn over the page.
    pub extend_content: bool,
    /// Mirrored for a right-to-left window (`RightToLeftLayout`): buttons on the left.
    pub right_to_left: bool,
    /// The slim band of a tool window.
    pub tool: bool,
}

impl Default for ChromeStyle {
    fn default() -> Self {
        Self {
            height: None,
            padding: None,
            background: None,
            foreground: None,
            subtitle: String::new(),
            alignment: TitleAlignment::Left,
            buttons: ButtonStyle::Kubuno,
            show_icon: true,
            show_title: true,
            help_button: false,
            commands: Vec::new(),
            extend_content: false,
            right_to_left: false,
            tool: false,
        }
    }
}

impl ChromeStyle {
    /// The band's height.
    pub fn band_height(&self) -> f32 {
        self.height.filter(|h| h.is_finite() && *h > 0.0).unwrap_or(if self.tool { TOOL_TITLEBAR_HEIGHT } else { TITLEBAR_HEIGHT })
    }

    /// The band's ground in `theme`.
    pub fn band_color(&self, theme: &Theme) -> D2D1_COLOR_F {
        self.background.unwrap_or(theme.accent)
    }

    /// The band's ink in `theme`.
    pub fn ink_color(&self, theme: &Theme) -> D2D1_COLOR_F {
        self.foreground.unwrap_or(theme.accent_foreground)
    }

    fn pad_x(&self) -> f32 {
        self.padding.filter(|p| p.is_finite() && *p >= 0.0).unwrap_or(if self.tool { TOOL_PAD_X } else { PAD_X })
    }

    fn gap(&self) -> f32 {
        if self.tool { space::SM } else { GAP }
    }

    fn button_box(&self) -> f32 {
        if self.tool { TOOL_BUTTON } else { BUTTON }
    }

    fn button_radius(&self) -> f32 {
        if self.tool { TOOL_BUTTON_RADIUS } else { BUTTON_RADIUS }
    }

    fn button_glyph(&self) -> f32 {
        if self.tool { TOOL_BUTTON_GLYPH } else { BUTTON_GLYPH }
    }
}

/// Which system buttons a window shows ([`crate::host::form::caption_buttons`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemButtons {
    pub minimize: CaptionButtonState,
    pub maximize: CaptionButtonState,
    pub close: bool,
}

impl Default for SystemButtons {
    fn default() -> Self {
        Self { minimize: CaptionButtonState::Shown, maximize: CaptionButtonState::Shown, close: true }
    }
}

impl SystemButtons {
    /// Only the close button (a dialog, a tool window, the in-window `FloatingWindow`).
    pub const CLOSE_ONLY: Self = Self { minimize: CaptionButtonState::Hidden, maximize: CaptionButtonState::Hidden, close: true };
    /// No button at all.
    pub const NONE: Self = Self { minimize: CaptionButtonState::Hidden, maximize: CaptionButtonState::Hidden, close: false };
}

/// How wide the page's controls in each title-bar region are (`TitleBar.Region`), in DIP.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SlotWidths {
    pub left: f32,
    pub center: f32,
    pub right: f32,
}

/// The band's geometry, in the coordinates of the window's top-left corner (`band.left`/`top`).
#[derive(Debug, Clone, PartialEq)]
pub struct ChromeLayout {
    pub band: Rect,
    /// The leading glyph, when shown.
    pub icon: Option<Rect>,
    /// Where the title (and subtitle) are written.
    pub title: Rect,
    /// The three regions a page places its own controls in (`TitleBar.Region`). An empty region
    /// is a zero-width rectangle at its place.
    pub left: Rect,
    pub center: Rect,
    pub right: Rect,
    /// The caption buttons, in paint order, with whether each one answers.
    pub buttons: Vec<(Part, Rect, bool)>,
}

impl ChromeLayout {
    /// The button at `(x, y)` — only an enabled one answers.
    pub fn hit(&self, x: f32, y: f32) -> Option<Part> {
        self.buttons.iter().find(|(_, r, on)| *on && r.contains(x, y)).map(|(p, _, _)| *p)
    }

    /// Whether `(x, y)` is on the band.
    pub fn in_band(&self, x: f32, y: f32) -> bool {
        self.band.contains(x, y)
    }

    /// The rectangle of `part`, when the band has it.
    pub fn rect_of(&self, part: Part) -> Option<Rect> {
        self.buttons.iter().find(|(p, _, _)| *p == part).map(|(_, r, _)| *r)
    }
}

/// Lays the band out over `bounds` (the whole window: the band is its top `band_height()`).
///
/// Left to right (mirrored when [`ChromeStyle::right_to_left`]): `[pad][icon][gap][left region]
/// [gap][title …][gap][right region][gap][commands][help][min][max][close][pad]`, the centre region
/// centred on the band. Buttons are `BUTTON_GAP` apart (the web's `gap-1`).
pub fn layout(style: &ChromeStyle, bounds: Rect, has_icon: bool, buttons: SystemButtons, slots: SlotWidths) -> ChromeLayout {
    let h = style.band_height();
    let band = Rect::new(bounds.left, bounds.top, bounds.right, bounds.top + h);
    let width = band.right - band.left;
    let cy = band.top + h / 2.0;
    let pad = style.pad_x();
    let gap = style.gap();

    // The buttons, from the right edge inwards (in local x: 0 = band.left).
    let mut order: Vec<(Part, bool)> = Vec::new();
    if buttons.close {
        order.push((Part::Close, true));
    }
    if buttons.maximize != CaptionButtonState::Hidden {
        order.push((Part::Maximize, buttons.maximize == CaptionButtonState::Shown));
    }
    if buttons.minimize != CaptionButtonState::Hidden {
        order.push((Part::Minimize, buttons.minimize == CaptionButtonState::Shown));
    }
    if style.help_button {
        order.push((Part::Help, true));
    }
    for (i, c) in style.commands.iter().enumerate().rev() {
        order.push((Part::Command(i), c.enabled));
    }
    // (part, (left, top, right, bottom) in band-local x, enabled)
    type Slot = (Part, (f32, f32, f32, f32), bool);
    let mut rects: Vec<Slot> = Vec::new();
    let mut right = match style.buttons {
        ButtonStyle::Kubuno => width - pad,
        ButtonStyle::Windows => width,
    };
    for (i, (part, on)) in order.iter().enumerate() {
        let (w, top, bottom) = match style.buttons {
            ButtonStyle::Kubuno => {
                let b = style.button_box();
                (b, cy - b / 2.0, cy + b / 2.0)
            }
            // Windows' own caption buttons are 32 DIP tall at the top of a taller band (WinUI's default
            // title bar), and fill a band that is not taller than that.
            ButtonStyle::Windows => (if style.tool { 32.0 } else { WINDOWS_BUTTON_W }, band.top, band.top + (band.bottom - band.top).min(WINDOWS_BUTTON_H)),
        };
        if i > 0 && style.buttons == ButtonStyle::Kubuno {
            right -= BUTTON_GAP;
        }
        rects.push((*part, (right - w, top, right, bottom), *on));
        right -= w;
    }
    let buttons_left = if rects.is_empty() { width - pad } else { right - gap };

    // Leading glyph.
    let mut left = pad;
    let icon = has_icon && style.show_icon;
    let icon_rect = icon.then(|| {
        let r = (left, cy - ICON / 2.0, left + ICON, cy + ICON / 2.0);
        left += ICON + gap;
        r
    });
    // The page's regions.
    let slot_left = (left, left + slots.left.max(0.0));
    if slots.left > 0.0 {
        left = slot_left.1 + gap;
    }
    let slot_right_w = slots.right.max(0.0);
    let slot_right = (buttons_left - slot_right_w, buttons_left);
    let title_right = if slot_right_w > 0.0 { slot_right.0 - gap } else { buttons_left };
    let cw = slots.center.max(0.0);
    // Centred on the band, but never over the left region or the right one (a wide search field next to the header's
    // buttons): pushed aside, and narrowed to the room between them when that is all there is.
    let slot_center = {
        let (lo, hi) = (left, title_right.max(left));
        let cw = cw.min(hi - lo);
        let start = ((width - cw) / 2.0).clamp(lo, (hi - cw).max(lo));
        (start, start + cw)
    };
    let title = match style.alignment {
        TitleAlignment::Left if cw > 0.0 => (left, slot_center.0.max(left) - gap),
        TitleAlignment::Left => (left, title_right),
        // Centred: symmetric around the band's middle, as wide as both sides allow.
        TitleAlignment::Center => {
            let half = ((width / 2.0 - left).min(title_right - width / 2.0)).max(0.0);
            (width / 2.0 - half, width / 2.0 + half)
        }
    };

    let mirror = style.right_to_left;
    let x = |a: f32, b: f32| -> (f32, f32) {
        if mirror {
            (band.left + width - b, band.left + width - a)
        } else {
            (band.left + a, band.left + b)
        }
    };
    let rect = |a: f32, b: f32, top: f32, bottom: f32| {
        let (l, r) = x(a, b.max(a));
        Rect::new(l, top, r, bottom)
    };
    ChromeLayout {
        band,
        icon: icon_rect.map(|(a, t, b, bt)| rect(a, b, t, bt)),
        title: rect(title.0, title.1, band.top, band.bottom),
        left: rect(slot_left.0, slot_left.1, band.top, band.bottom),
        center: rect(slot_center.0, slot_center.1, band.top, band.bottom),
        right: rect(slot_right.0, slot_right.1, band.top, band.bottom),
        buttons: rects.into_iter().map(|(p, (a, t, b, bt), on)| (p, rect(a, b, t, bt), on)).collect(),
    }
}

/// The window's leading glyph: a Lucide name or an image (an `.ico`/`.png` the host loaded).
#[derive(Clone, Copy)]
pub enum ChromeIcon<'a> {
    None,
    Glyph(&'static str),
    Bitmap(&'a ID2D1Bitmap1),
}

impl ChromeIcon<'_> {
    pub fn is_some(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// What changes from one frame to the next: the pointer, the window's state.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ChromeState {
    pub hot: Option<Part>,
    pub pressed: Option<Part>,
    /// Maximised: the maximise button shows the restore glyph.
    pub maximized: bool,
}

fn fade(c: D2D1_COLOR_F, k: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: c.a * k, ..c }
}

/// Paints the band's ground (`.kb-window-titlebar`'s background). Nothing when the page extends
/// under the band and no colour was asked for.
pub fn paint_band(canvas: &dyn Canvas, style: &ChromeStyle, layout: &ChromeLayout) {
    paint_band_rounded(canvas, style, layout, 0.0);
}

/// [`paint_band`] for a window DRAWN with rounded corners (`radius`): the band rounds its top ones.
pub fn paint_band_rounded(canvas: &dyn Canvas, style: &ChromeStyle, layout: &ChromeLayout, radius: f32) {
    if style.extend_content && style.background.is_none() {
        return;
    }
    let color = style.band_color(canvas.theme());
    if radius > 0.0 {
        canvas.fill_top_rounded(&layout.band, radius, &color);
    } else {
        canvas.fill_rounded(&layout.band, 0.0, &color);
    }
}

/// Paints what sits ON the band: the glyph, the title and subtitle, the caption buttons. Drawn
/// after the page, so a page extending under the band still gets its title and buttons on top.
pub fn paint_caption(canvas: &dyn Canvas, style: &ChromeStyle, layout: &ChromeLayout, title: &str, icon: ChromeIcon<'_>, state: ChromeState) {
    let theme = canvas.theme();
    // On an extended band with no colour of its own the ink must read on the page instead.
    let ink = if style.extend_content && style.background.is_none() && style.foreground.is_none() {
        theme.text_primary
    } else {
        style.ink_color(theme)
    };
    let f = canvas.formats();
    if let Some(r) = layout.icon {
        match icon {
            ChromeIcon::Glyph(name) => canvas.vector_icon(name, &r, ICON, &ink),
            ChromeIcon::Bitmap(bitmap) => canvas.image(bitmap, &r, ICON),
            ChromeIcon::None => {}
        }
    }
    if style.show_title && layout.title.right - layout.title.left > 4.0 {
        let font = if style.tool { &f.body_strong } else { &f.heading };
        let sub_font = &f.caption;
        let sub = style.subtitle.trim();
        match style.alignment {
            TitleAlignment::Left => {
                if sub.is_empty() {
                    canvas.text_ellipsis(title, &layout.title, font, &ink);
                } else {
                    let tw = canvas.measure(title, font).min((layout.title.right - layout.title.left) * 0.7);
                    let t = Rect::new(layout.title.left, layout.title.top, layout.title.left + tw + 1.0, layout.title.bottom);
                    canvas.text_ellipsis(title, &t, font, &ink);
                    let s = Rect::new(t.right + space::SM, layout.title.top, layout.title.right, layout.title.bottom);
                    if s.right > s.left + 8.0 {
                        canvas.text_ellipsis(sub, &s, sub_font, &fade(ink, REST_ALPHA));
                    }
                }
            }
            TitleAlignment::Center => {
                let text = if sub.is_empty() { title.to_string() } else { format!("{title} — {sub}") };
                canvas.text_ellipsis_center(&text, &layout.title, font, &ink);
            }
        }
    }
    for (part, r, on) in &layout.buttons {
        paint_button(canvas, style, *part, *r, *on, state, ink);
    }
}

fn paint_button(canvas: &dyn Canvas, style: &ChromeStyle, part: Part, r: Rect, on: bool, state: ChromeState, ink: D2D1_COLOR_F) {
    let hot = on && state.hot == Some(part);
    let pressed = on && state.pressed == Some(part);
    let checked = matches!(part, Part::Command(i) if style.commands.get(i).is_some_and(|c| c.checked));
    let windows = style.buttons == ButtonStyle::Windows;
    let mut glyph_ink = if !on {
        fade(ink, DISABLED_ALPHA)
    } else if hot || pressed || windows {
        ink
    } else {
        fade(ink, REST_ALPHA)
    };
    if windows && part == Part::Close && (hot || pressed) {
        // Windows 11's close: #C42B1C, white glyph.
        let red = D2D1_COLOR_F { r: 0.769, g: 0.169, b: 0.110, a: if pressed { 0.9 } else { 1.0 } };
        canvas.fill_rounded(&r, 0.0, &red);
        glyph_ink = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    } else if hot || pressed || checked {
        let wash = if pressed { PRESSED_WASH } else if windows { HOVER_WASH * 0.5 } else { HOVER_WASH };
        let radius = if windows { 0.0 } else { style.button_radius() };
        canvas.fill_rounded(&r, radius, &fade(ink, wash));
    }
    let g = style.button_glyph();
    let (cx, cy) = ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
    match part {
        Part::Close => canvas.vector_icon("X", &r, g, &glyph_ink),
        Part::Minimize => canvas.vector_icon("Minus", &r, g, &glyph_ink),
        Part::Help => canvas.vector_icon("HelpCircle", &r, g, &glyph_ink),
        Part::Maximize => {
            // No Lucide square is embedded: drawn by hand at the X's optical size.
            let s = g * 0.62;
            if state.maximized {
                let o = s * 0.28;
                let back = Rect::new(cx - s / 2.0 + o, cy - s / 2.0 - o, cx + s / 2.0 + o, cy + s / 2.0 - o);
                let front = Rect::new(cx - s / 2.0 - o * 0.4, cy - s / 2.0 + o * 0.4, cx + s / 2.0 - o * 0.4, cy + s / 2.0 + o * 0.4);
                canvas.stroke_rounded_w(&back, 1.5, &glyph_ink, 1.4);
                canvas.fill_rounded(&front, 1.5, &canvas.current_bg());
                canvas.stroke_rounded_w(&front, 1.5, &glyph_ink, 1.4);
            } else {
                let sq = Rect::new(cx - s / 2.0, cy - s / 2.0, cx + s / 2.0, cy + s / 2.0);
                canvas.stroke_rounded_w(&sq, 1.5, &glyph_ink, 1.5);
            }
        }
        Part::Command(i) => {
            if let Some(name) = style.commands.get(i).and_then(|c| drive_app_controls::icon_name(&c.glyph)) {
                canvas.vector_icon(name, &r, g, &glyph_ink);
            }
        }
    }
}

/// The footer band of a window (`.kb-window-footer`): the content colour, a hairline on top.
pub fn paint_footer(canvas: &dyn Canvas, footer: Rect) {
    let t = canvas.theme();
    canvas.fill_rounded(&footer, 0.0, &t.layer_background);
    canvas.fill_rounded(&Rect::new(footer.left, footer.top, footer.right, footer.top + 1.0), 0.0, &t.card_stroke);
}

/// The resize grip at the bottom-right corner of `bounds` (`.kb-window-grip`): two rounded oblique
/// strokes at 35 % (90 % when hot).
pub fn paint_grip(canvas: &dyn Canvas, bounds: Rect, hot: bool) {
    let t = canvas.theme();
    let ink = fade(t.text_primary, if hot { 0.9 } else { 0.35 });
    // `M12 6 6 12 M12 10 10 12` on a 14 × 14 grid centred in the 18 × 18 box, 2 DIP off the corner.
    let ox = bounds.right - 2.0 - GRIP + 2.0;
    let oy = bounds.bottom - 2.0 - GRIP + 2.0;
    let dot = |x: f32, y: f32| {
        let r = Rect::new(ox + x - 0.9, oy + y - 0.9, ox + x + 0.9, oy + y + 0.9);
        canvas.fill_rounded(&r, 0.9, &ink);
    };
    // A stroke drawn as dots along its length (the canvas has no free line primitive).
    for i in 0..=12 {
        let k = i as f32 / 12.0;
        dot(12.0 - 6.0 * k, 6.0 + 6.0 * k);
    }
    for i in 0..=4 {
        let k = i as f32 / 4.0;
        dot(12.0 - 2.0 * k, 10.0 + 2.0 * k);
    }
}

/// The grip's rectangle in `bounds`.
pub fn grip_rect(bounds: Rect) -> Rect {
    Rect::new(bounds.right - 2.0 - GRIP, bounds.bottom - 2.0 - GRIP, bounds.right - 2.0, bounds.bottom - 2.0)
}

/// The window's own frame as a picture (what DWM draws around a top-level window: its shadow and
/// its border), for surfaces that DRAW a window rather than host one — the designer and the
/// in-window floating windows. `radius` follows the window's corner preference.
pub fn paint_frame(canvas: &dyn Canvas, outer: Rect, radius: f32, border: Option<D2D1_COLOR_F>) {
    use drive_app_controls::themes::shape::SHADOW_WINDOW;
    canvas.draw_shadow(&outer, radius, &SHADOW_WINDOW, (0.0, 0.0, 0.0));
    canvas.fill_rounded(&outer, radius, &canvas.theme().layer_background);
    if let Some(border) = border {
        canvas.stroke_rounded(&outer, radius, &border);
    }
}

/// What a window's `Icon` draws as: a Lucide glyph (`"FileText"`) or an image file (SVG, PNG, ICO…,
/// drawn by `crate::icon_image`); `None` for an empty value or an unknown name.
pub fn icon_glyph(icon: &str) -> Option<&'static str> {
    if icon.is_empty() {
        None
    } else {
        drive_app_controls::icon_name(icon)
    }
}

/// The accessible name of a caption button (UI Automation).
pub fn accessible_name(style: &ChromeStyle, part: Part, maximized: bool) -> String {
    match part {
        Part::Minimize => "Minimize".into(),
        Part::Maximize if maximized => "Restore".into(),
        Part::Maximize => "Maximize".into(),
        Part::Close => "Close".into(),
        Part::Help => "Help".into(),
        Part::Command(i) => style.commands.get(i).map(|c| if c.tooltip.is_empty() { c.id.clone() } else { c.tooltip.clone() }).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: Rect = Rect { left: 0.0, top: 0.0, right: 600.0, bottom: 400.0 };

    #[test]
    fn default_band_is_the_web_floating_window() {
        let s = ChromeStyle::default();
        let l = layout(&s, W, true, SystemButtons::CLOSE_ONLY, SlotWidths::default());
        assert_eq!(l.band.bottom - l.band.top, 50.0);
        let close = l.rect_of(Part::Close).expect("close");
        // `px-4`: 16 from the right edge, 30 × 30, vertically centred.
        assert_eq!((close.left, close.top, close.right, close.bottom), (554.0, 10.0, 584.0, 40.0));
        let icon = l.icon.expect("icon");
        assert_eq!((icon.left, icon.right), (16.0, 32.0));
        // `gap-2.5` after the icon, and before the close button.
        assert_eq!(l.title.left, 42.0);
        assert_eq!(l.title.right, 544.0);
    }

    #[test]
    fn system_buttons_sit_four_apart_right_to_left() {
        let s = ChromeStyle::default();
        let l = layout(&s, W, false, SystemButtons::default(), SlotWidths::default());
        let order: Vec<Part> = l.buttons.iter().map(|b| b.0).collect();
        assert_eq!(order, vec![Part::Close, Part::Maximize, Part::Minimize]);
        let max = l.rect_of(Part::Maximize).expect("max");
        assert_eq!(max.right, 554.0 - BUTTON_GAP);
        assert_eq!(l.hit(max.left + 1.0, 20.0), Some(Part::Maximize));
        assert_eq!(l.hit(300.0, 20.0), None);
        assert_eq!(l.title.left, PAD_X, "no icon: the title starts at the inset");
    }

    #[test]
    fn disabled_buttons_do_not_answer_and_hidden_take_no_room() {
        let s = ChromeStyle::default();
        let b = SystemButtons { minimize: CaptionButtonState::Disabled, maximize: CaptionButtonState::Shown, close: true };
        let l = layout(&s, W, false, b, SlotWidths::default());
        let min = l.rect_of(Part::Minimize).expect("min");
        assert_eq!(l.hit(min.left + 2.0, 20.0), None);
        let l = layout(&s, W, false, SystemButtons::NONE, SlotWidths::default());
        assert!(l.buttons.is_empty());
        assert_eq!(l.title.right, 600.0 - PAD_X);
    }

    #[test]
    fn regions_commands_help_and_mirroring() {
        let s = ChromeStyle { help_button: true, commands: vec![CaptionCommand::new("pin", "Pin")], ..ChromeStyle::default() };
        let l = layout(&s, W, true, SystemButtons::CLOSE_ONLY, SlotWidths { left: 60.0, center: 100.0, right: 80.0 });
        let order: Vec<Part> = l.buttons.iter().map(|b| b.0).collect();
        assert_eq!(order, vec![Part::Close, Part::Help, Part::Command(0)]);
        let cmd = l.rect_of(Part::Command(0)).expect("cmd");
        assert_eq!(l.right.right, cmd.left - GAP);
        assert_eq!(l.right.right - l.right.left, 80.0);
        assert_eq!((l.center.left, l.center.right), (250.0, 350.0));
        assert_eq!(l.left.left, 42.0);
        assert!(l.title.left >= l.left.right + GAP && l.title.right <= l.center.left);
        let rtl = layout(&ChromeStyle { right_to_left: true, ..ChromeStyle::default() }, W, false, SystemButtons::CLOSE_ONLY, SlotWidths::default());
        let close = rtl.rect_of(Part::Close).expect("close");
        assert_eq!((close.left, close.right), (16.0, 46.0));
    }

    #[test]
    fn tool_and_windows_styles() {
        let tool = ChromeStyle { tool: true, ..ChromeStyle::default() };
        let l = layout(&tool, W, false, SystemButtons::CLOSE_ONLY, SlotWidths::default());
        assert_eq!(l.band.bottom, TOOL_TITLEBAR_HEIGHT);
        let close = l.rect_of(Part::Close).expect("close");
        assert_eq!(close.right - close.left, TOOL_BUTTON);
        let win = ChromeStyle { buttons: ButtonStyle::Windows, height: Some(32.0), ..ChromeStyle::default() };
        let l = layout(&win, W, false, SystemButtons::default(), SlotWidths::default());
        let close = l.rect_of(Part::Close).expect("close");
        assert_eq!((close.left, close.top, close.right, close.bottom), (554.0, 0.0, 600.0, 32.0));
        assert_eq!(l.rect_of(Part::Maximize).map(|r| r.right), Some(554.0));
        // A taller band keeps Windows' 32-DIP caption buttons at its top.
        let tall = ChromeStyle { buttons: ButtonStyle::Windows, height: Some(64.0), ..ChromeStyle::default() };
        let close = layout(&tall, W, false, SystemButtons::default(), SlotWidths::default()).rect_of(Part::Close).expect("close");
        assert_eq!((close.top, close.bottom), (0.0, WINDOWS_BUTTON_H));
    }

    /// `TitleBarPadding`: the band's side insets move what the band holds at its ends.
    #[test]
    fn padding_sets_the_side_insets() {
        let s = ChromeStyle { padding: Some(8.0), show_icon: false, ..ChromeStyle::default() };
        let l = layout(&s, W, false, SystemButtons::CLOSE_ONLY, SlotWidths { left: 40.0, center: 0.0, right: 0.0 });
        assert_eq!(l.left.left, 8.0);
        assert_eq!(l.rect_of(Part::Close).map(|r| r.right), Some(592.0));
        let bad = ChromeStyle { padding: Some(f32::NAN), ..ChromeStyle::default() };
        assert_eq!(layout(&bad, W, false, SystemButtons::NONE, SlotWidths::default()).title.left, PAD_X, "an invalid inset keeps the default");
    }

    #[test]
    fn centred_title_is_symmetric() {
        let s = ChromeStyle { alignment: TitleAlignment::Center, ..ChromeStyle::default() };
        let l = layout(&s, W, false, SystemButtons::default(), SlotWidths::default());
        let mid = (l.title.left + l.title.right) / 2.0;
        assert!((mid - 300.0).abs() < 0.01);
        assert!(l.title.right <= l.rect_of(Part::Minimize).map(|r| r.left).unwrap_or(600.0));
    }
}
