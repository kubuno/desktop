//! The window as a WinForms `Form` (`vskubuno/docs/EVENTS.md` §16, a view's root properties):
//! title, icon, start position, border style, the caption buttons, task bar, top-most, opacity,
//! window state, minimum and maximum size.
//!
//! [`FormOptions`] is given once in [`super::HostOptions::form`] (what must be known before the
//! window shows: where it opens, in what state, whether it has a task bar button) and may be changed
//! any frame with [`set_form`] (a bound `Title`, `TopMost` or `Opacity`): the host compares it with
//! what it applied and changes only what differs, after the frame.

use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::window_chrome::ChromeStyle;

/// Where the window opens (`Form.StartPosition`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StartPosition {
    /// At [`FormOptions::location`].
    Manual,
    /// Centred on the work area of its monitor.
    CenterScreen,
    /// Where Windows puts a new window, at the designed size (WinForms' default).
    #[default]
    WindowsDefaultLocation,
    /// Where Windows puts a new window, at the size Windows chooses.
    WindowsDefaultBounds,
    /// Centred on its owner; a top-level window has none, so on its monitor.
    CenterParent,
}

/// The window's border and caption (`Form.FormBorderStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormBorderStyle {
    /// No border and no caption.
    None,
    FixedSingle,
    Fixed3D,
    FixedDialog,
    /// Resizable (WinForms' default).
    #[default]
    Sizable,
    /// A tool window: a close button only, not resizable.
    FixedToolWindow,
    /// A resizable tool window.
    SizableToolWindow,
}

impl FormBorderStyle {
    /// The user can resize the window by its borders.
    pub fn sizable(self) -> bool {
        matches!(self, Self::Sizable | Self::SizableToolWindow)
    }

    /// A tool window (close button only).
    pub fn tool_window(self) -> bool {
        matches!(self, Self::FixedToolWindow | Self::SizableToolWindow)
    }

    /// The window has a caption at all.
    pub fn has_caption(self) -> bool {
        self != Self::None
    }
}

/// Normal, minimized or maximized (`Form.WindowState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowState {
    #[default]
    Normal,
    Minimized,
    Maximized,
}

/// The `Form` properties of the window (see the module doc). The default is a normal WinForms form.
#[derive(Debug, Clone, PartialEq)]
pub struct FormOptions {
    /// Replaces [`super::HostOptions::title`] when set.
    pub title: Option<String>,
    /// An `.ico`, `.png` or `.bmp` file shown in the caption and the task bar.
    pub icon: Option<String>,
    pub start_position: StartPosition,
    /// The window's position on screen, in DIP, for [`StartPosition::Manual`].
    pub location: Option<(f32, f32)>,
    pub border_style: FormBorderStyle,
    /// The caption buttons and the window menu.
    pub control_box: bool,
    pub minimize_box: bool,
    pub maximize_box: bool,
    pub show_in_taskbar: bool,
    pub top_most: bool,
    /// 0 (transparent) to 1 (opaque).
    pub opacity: f32,
    pub window_state: WindowState,
    /// The smallest and largest page area, in DIP (0 on an axis = no limit).
    pub min_client_size: Option<(f32, f32)>,
    pub max_client_size: Option<(f32, f32)>,
    /// The Kubuno title band (height, colours, subtitle, alignment, caption button style, help
    /// button, the window's own caption buttons, content under the band…).
    pub chrome: ChromeStyle,
    /// The corners of the window, as a preset (`CornerPreference`): see [`FormOptions::corner_radius`].
    pub corner: CornerPreference,
    /// The radius of the window's corners, in DIP (`CornerRadius`); `None`: the preset
    /// [`FormOptions::corner`] gives it. `Some(0.0)` is square. Whatever is asked, a maximised,
    /// snapped or full-screen window is square (`super::frame`).
    pub corner_radius: Option<f32>,
    /// The colour DWM draws the window's 1 px border in; `None`: the band's colour (no seam).
    pub border_color: Option<D2D1_COLOR_F>,
    /// The system material behind the window; `None`: [`super::HostOptions::backdrop`].
    pub backdrop: Option<super::Backdrop>,
    /// Pixels of this colour are transparent and let clicks through (`Form.TransparencyKey`).
    pub transparency_key: Option<D2D1_COLOR_F>,
    /// The resize grip at the bottom-right corner (`Form.SizeGripStyle`).
    pub size_grip: SizeGripStyle,
    /// A borderless window (`FormBorderStyle::None`) that can still be resized by its edges.
    pub resize_border: bool,
    /// A floating panel (a flyout): what is behind the window, blurred, rounded at its own radius,
    /// under the page, with a drop shadow around it (see [`FloatingPanel`]). Fixed for the
    /// window's life (it decides how the window is created).
    pub panel: Option<FloatingPanel>,
}

/// A window shown as a floating panel ([`FormOptions::panel`], `host::backdrop`): the web's
/// frosted panels (the app launcher, the account panel), as a real window.
///
/// The window is the panel plus [`FloatingPanel::shadow_margin`] on every side, where the panel's
/// drop shadow falls; the page area (its size, its location, its mouse coordinates) is the panel
/// alone, and the page is clipped to the panel's rounded corners. The blur shows through wherever
/// the page leaves its ground translucent: a page's `BackColor` with an alpha is the panel's tint.
/// Where the system gives no backdrop, the panel's ground is the theme's window background.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatingPanel {
    /// The panel's corner radius, in DIP.
    pub radius: f32,
    /// The room around the panel for its shadow, in DIP.
    pub shadow_margin: f32,
}

impl FloatingPanel {
    /// The room the floating panels' shadow (`shape::SHADOW_WAFFLE`) needs: its widest layer
    /// reaches `spread + blur / 2` = 7 DIP, and its `dy` of 4 carries the bottom edge further.
    pub const SHADOW_MARGIN: f32 = 16.0;

    /// A panel rounded at `radius`, with the standard shadow margin.
    pub fn new(radius: f32) -> Self {
        Self { radius, shadow_margin: Self::SHADOW_MARGIN }
    }
}

/// The radius of a Kubuno desktop window's corners by default, in DIP: Windows 11's own (what DWM
/// draws for `DWMWCP_ROUND`), which is also the design system's largest radius (`--radius-xl:
/// 8px`, `theme.css`). The web's floating windows are square (`--kb-window-radius: 0px`, a GPU
/// workaround of 2026-08-30 that has no reason to exist on the desktop); desktop windows are
/// rounded by default since 2026-10-02 (product owner).
pub const DEFAULT_CORNER_RADIUS: f32 = 8.0;

/// Windows 11's small rounded corners (`DWMWCP_ROUNDSMALL`), in DIP — `--radius-sm`.
pub const SMALL_CORNER_RADIUS: f32 = 4.0;

/// The corners of a window, as a preset (`CornerPreference`); [`FormOptions::corner_radius`] sets
/// any other radius. A preset that matches one of Windows 11's is drawn by DWM there (its shadow,
/// its border); any other radius, and every radius on Windows 10, by the host (`super::frame`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CornerPreference {
    /// The window's own: rounded at [`DEFAULT_CORNER_RADIUS`] when it has a title bar (a main
    /// window, a dialog, a tool window), square without one (a borderless window). A view's
    /// `WindowKind` may say otherwise (a splash screen is rounded).
    #[default]
    Default,
    /// Windows 11's rounded corners (8 DIP).
    Round,
    /// Small rounded corners (4 DIP).
    RoundSmall,
    /// Square corners.
    DoNotRound,
}

impl CornerPreference {
    /// The `DWM_WINDOW_CORNER_PREFERENCE` value of the preset as such (`Default` = square: the
    /// host decides from the radius, see [`super::frame::plan`]).
    pub fn dwm_value(self) -> i32 {
        match self {
            Self::Default | Self::DoNotRound => 1,
            Self::Round => 2,
            Self::RoundSmall => 3,
        }
    }

    /// The radius of the preset, in DIP, for a window that has a title bar (`captioned`) or not.
    pub fn radius_for(self, captioned: bool) -> f32 {
        match self {
            Self::Default if captioned => DEFAULT_CORNER_RADIUS,
            Self::Default | Self::DoNotRound => 0.0,
            Self::Round => DEFAULT_CORNER_RADIUS,
            Self::RoundSmall => SMALL_CORNER_RADIUS,
        }
    }

    /// The radius of the preset for a window with a title bar (see [`CornerPreference::radius_for`]).
    pub fn radius(self) -> f32 {
        self.radius_for(true)
    }
}

/// A `CornerRadius` value as written (`"8"`, `" 12.5 "`): a finite, non-negative number of DIP.
pub fn parse_corner_radius(text: &str) -> Option<f32> {
    text.trim().parse::<f32>().ok().filter(|r| r.is_finite() && *r >= 0.0)
}

/// Whether the resize grip shows (`Form.SizeGripStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SizeGripStyle {
    /// On a resizable window — the web `FloatingWindow` shows its grip whenever it is resizable.
    #[default]
    Auto,
    Show,
    Hide,
}

impl Default for FormOptions {
    fn default() -> Self {
        Self {
            title: None,
            icon: None,
            start_position: StartPosition::default(),
            location: None,
            border_style: FormBorderStyle::default(),
            control_box: true,
            minimize_box: true,
            maximize_box: true,
            show_in_taskbar: true,
            top_most: false,
            opacity: 1.0,
            window_state: WindowState::default(),
            min_client_size: None,
            max_client_size: None,
            chrome: ChromeStyle::default(),
            corner: CornerPreference::Default,
            corner_radius: None,
            border_color: None,
            backdrop: None,
            transparency_key: None,
            size_grip: SizeGripStyle::Auto,
            resize_border: false,
            panel: None,
        }
    }
}

impl FormOptions {
    /// The band as drawn: a tool window's is the slim one.
    pub fn effective_chrome(&self) -> ChromeStyle {
        let mut c = self.chrome.clone();
        c.tool = c.tool || self.border_style.tool_window();
        c
    }

    /// The user can resize the window by its edges.
    pub fn resizable(&self) -> bool {
        self.border_style.sizable() || (self.border_style == FormBorderStyle::None && self.resize_border)
    }

    /// Whether the resize grip shows.
    pub fn shows_grip(&self) -> bool {
        match self.size_grip {
            SizeGripStyle::Hide => false,
            SizeGripStyle::Auto | SizeGripStyle::Show => self.resizable(),
        }
    }

    /// The radius of the window's corners as asked, in DIP: a floating panel's own, else
    /// `CornerRadius`, else the preset's ([`CornerPreference::radius_for`]). The window's state may
    /// still square them (maximised, snapped, full screen: `super::frame::effective_radius`).
    pub fn corner_radius(&self) -> f32 {
        if let Some(p) = self.panel {
            return p.radius.max(0.0);
        }
        match self.corner_radius {
            Some(r) if r.is_finite() => r.max(0.0),
            _ => self.corner.radius_for(self.border_style.has_caption()),
        }
    }

    /// The system buttons of the band.
    pub fn system_buttons(&self) -> crate::window_chrome::SystemButtons {
        let (minimize, maximize, close) = caption_buttons(self);
        crate::window_chrome::SystemButtons { minimize, maximize, close }
    }
}

/// Which caption button a Kubuno caption shows, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionButtonState {
    Shown,
    /// Shown greyed and inert (WinForms: `MinimizeBox = false` while the maximize button shows).
    Disabled,
    Hidden,
}

/// The caption buttons of a form: `(minimize, maximize, close)`.
pub fn caption_buttons(form: &FormOptions) -> (CaptionButtonState, CaptionButtonState, bool) {
    use CaptionButtonState::*;
    if !form.control_box || !form.border_style.has_caption() {
        return (Hidden, Hidden, false);
    }
    if form.border_style.tool_window() || (!form.minimize_box && !form.maximize_box) {
        return (Hidden, Hidden, true);
    }
    let state = |on: bool| if on { Shown } else { Disabled };
    (state(form.minimize_box), state(form.maximize_box), true)
}

/// The window styles of `form` over `base` (the host's own style for its chrome): the thick frame
/// only when sizable, the caption buttons and the window menu as asked.
pub fn window_style(form: &FormOptions, base: WINDOW_STYLE) -> WINDOW_STYLE {
    let mut s = base;
    if !form.border_style.has_caption() {
        s &= !(WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX);
        // A borderless window that can still be resized keeps the thick frame (its border is
        // stripped by `WM_NCCALCSIZE`, its edges answer `WM_NCHITTEST`).
        if form.resize_border {
            s |= WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU;
        }
        return s | WS_POPUP;
    }
    s &= !WS_POPUP;
    s |= WS_CAPTION;
    if form.border_style.sizable() {
        s |= WS_THICKFRAME;
    } else {
        s &= !WS_THICKFRAME;
    }
    let (min, max, _) = caption_buttons(form);
    set(&mut s, WS_SYSMENU, form.control_box);
    set(&mut s, WS_MINIMIZEBOX, min == CaptionButtonState::Shown);
    set(&mut s, WS_MAXIMIZEBOX, max == CaptionButtonState::Shown);
    s
}

/// The extended styles of `form` over `base`: no task bar button (a tool window), top-most,
/// layered when translucent.
pub fn window_ex_style(form: &FormOptions, base: WINDOW_EX_STYLE) -> WINDOW_EX_STYLE {
    let mut s = base;
    set_ex(&mut s, WS_EX_TOOLWINDOW, !form.show_in_taskbar || form.border_style.tool_window());
    set_ex(&mut s, WS_EX_LAYERED, form.opacity < 0.999 || form.transparency_key.is_some());
    // A floating panel's pixels come from its composition tree only (`host::backdrop`): without
    // this, Windows keeps a GDI redirection surface under it that shows wherever the swap chain is
    // transparent. Only effective at creation, where the host passes it.
    set_ex(&mut s, WS_EX_NOREDIRECTIONBITMAP, form.panel.is_some());
    s
}

/// A colour as GDI's `COLORREF` (`0x00BBGGRR`, sRGB bytes).
pub fn colorref(c: D2D1_COLOR_F) -> windows::Win32::Foundation::COLORREF {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    windows::Win32::Foundation::COLORREF((byte(c.b) << 16) | (byte(c.g) << 8) | byte(c.r))
}

fn set(s: &mut WINDOW_STYLE, flag: WINDOW_STYLE, on: bool) {
    if on {
        *s |= flag;
    } else {
        *s &= !flag;
    }
}

fn set_ex(s: &mut WINDOW_EX_STYLE, flag: WINDOW_EX_STYLE, on: bool) {
    if on {
        *s |= flag;
    } else {
        *s &= !flag;
    }
}

/// Where the top-left corner of an outer `size` (physical px) goes for `form` on a monitor whose work
/// area is `work` (physical px, `(left, top, right, bottom)`); `None` leaves it where Windows put it.
/// `scale` converts [`FormOptions::location`]'s DIP.
pub fn start_origin(form: &FormOptions, size: (i32, i32), work: (i32, i32, i32, i32), scale: f32) -> Option<(i32, i32)> {
    match form.start_position {
        // A floating panel's location is the panel's: its window starts one shadow margin before.
        StartPosition::Manual => {
            let m = form.panel.map_or(0.0, |p| p.shadow_margin);
            form.location.map(|(x, y)| (((x - m) * scale).round() as i32, ((y - m) * scale).round() as i32))
        }
        StartPosition::CenterScreen | StartPosition::CenterParent => {
            let (l, t, r, b) = work;
            Some((l + ((r - l) - size.0).max(0) / 2, t + ((b - t) - size.1).max(0) / 2))
        }
        StartPosition::WindowsDefaultLocation | StartPosition::WindowsDefaultBounds => None,
    }
}

/// Applies `form` to the window `hwnd`, changing only what differs from `old` (everything when
/// `old` is `None`, at creation). `base`/`base_ex` are the host's own styles for its chrome.
pub(crate) fn apply(hwnd: HWND, form: &FormOptions, old: Option<&FormOptions>, base: WINDOW_STYLE, base_ex: WINDOW_EX_STYLE) {
    let changed = |f: &dyn Fn(&FormOptions) -> bool| old.is_none_or(|o| f(o) != f(form));
    // SAFETY: plain window calls on the host's own live window, from its thread.
    unsafe {
        if old.is_none_or(|o| o.title != form.title) {
            if let Some(title) = &form.title {
                let _ = SetWindowTextW(hwnd, &HSTRING::from(title.as_str()));
            }
        }
        let styles = |f: &FormOptions| (window_style(f, base).0, window_ex_style(f, base_ex).0);
        // At creation, compared with what the window really has, so a default form changes nothing.
        let current = (GetWindowLongPtrW(hwnd, GWL_STYLE) as u32, GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32);
        let differs = match old {
            Some(o) => styles(o) != styles(form),
            None => styles(form) != current,
        };
        if differs {
            let taskbar_changed = old.is_some_and(|o| o.show_in_taskbar != form.show_in_taskbar);
            let visible = IsWindowVisible(hwnd).as_bool();
            // The task bar only notices a changed tool-window style when the window is shown again.
            if taskbar_changed && visible {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, window_style(form, base).0 as isize);
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, window_ex_style(form, base_ex).0 as isize);
            let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
            if taskbar_changed && visible {
                let _ = ShowWindow(hwnd, SW_SHOWNA);
            }
        }
        let layered = |f: &FormOptions| (f.opacity < 0.999, (f.opacity * 1000.0).round() as i32, f.transparency_key.map(|c| colorref(c).0));
        if old.is_none_or(|o| layered(o) != layered(form)) && (form.opacity < 0.999 || form.transparency_key.is_some()) {
            let alpha = (form.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
            let (key, flags) = match form.transparency_key {
                Some(c) => (colorref(c), LWA_ALPHA | LWA_COLORKEY),
                None => (windows::Win32::Foundation::COLORREF(0), LWA_ALPHA),
            };
            let _ = SetLayeredWindowAttributes(hwnd, key, alpha, flags);
        }
        if changed(&|f| f.top_most) {
            let after = if form.top_most { HWND_TOPMOST } else { HWND_NOTOPMOST };
            let _ = SetWindowPos(hwnd, Some(after), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        if old.is_some_and(|o| o.window_state != form.window_state) {
            let _ = ShowWindow(
                hwnd,
                match form.window_state {
                    WindowState::Normal => SW_RESTORE,
                    WindowState::Minimized => SW_MINIMIZE,
                    WindowState::Maximized => SW_MAXIMIZE,
                },
            );
        }
        if old.is_none_or(|o| o.icon != form.icon) {
            let (big, small) = match &form.icon {
                Some(path) => (load_icon(path, GetSystemMetrics(SM_CXICON)), load_icon(path, GetSystemMetrics(SM_CXSMICON))),
                None => (None, None),
            };
            if big.is_some() || old.is_some() {
                let as_param = |h: Option<HICON>| LPARAM(h.map(|h| h.0 as isize).unwrap_or(0));
                let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(as_param(big)));
                let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(as_param(small)));
            }
        }
    }
}

/// The `ShowWindow` command that shows a new window in `state`.
pub fn show_command(state: WindowState) -> SHOW_WINDOW_CMD {
    match state {
        WindowState::Normal => SW_SHOW,
        WindowState::Minimized => SW_SHOWMINIMIZED,
        WindowState::Maximized => SW_SHOWMAXIMIZED,
    }
}

/// An icon of `size` px for the window's `Icon` (the title bar, the task bar, Alt+Tab): an `.ico`
/// file gives its own image of that size; any other image file (SVG, PNG, JPEG, BMP, GIF, TIFF,
/// WebP) or a glyph name (`"FileText"`, drawn in the Kubuno accent colour) is rendered at that
/// size (`crate::icon_image::rasterize`).
fn load_icon(path: &str, size: i32) -> Option<HICON> {
    let lower = path.to_ascii_lowercase();
    // SAFETY: `LoadImageW` with LR_LOADFROMFILE reads the file; the handle is owned by the window
    // (kept for its life, like a WinForms form's icon).
    unsafe {
        if lower.ends_with(".ico") {
            if let Ok(h) = LoadImageW(None, &HSTRING::from(path), IMAGE_ICON, size, size, LR_LOADFROMFILE) {
                return Some(HICON(h.0));
            }
        }
    }
    let value = drive_app_controls::icon_name(path)?;
    let accent = drive_app_controls::Theme::light().accent;
    let raster = crate::icon_image::rasterize(value, size.max(1) as u32, size.max(1) as u32, accent, None)?;
    icon_from_raster(&raster)
}

/// A Windows icon from `raster` (a 32-bit colour bitmap with straight alpha and an empty mask).
fn icon_from_raster(raster: &crate::icon_image::Raster) -> Option<HICON> {
    use windows::Win32::Graphics::Gdi::{CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS};
    let (w, h) = (raster.width as i32, raster.height as i32);
    let pixels = raster.straight_alpha();
    // SAFETY: a top-down 32-bit DIB section of exactly `w * h * 4` bytes, filled from `pixels`;
    // `CreateIconIndirect` copies both bitmaps, which are deleted after.
    unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let color = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        if bits.is_null() {
            let _ = DeleteObject(color.into());
            return None;
        }
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u8>(), pixels.len());
        let mask = CreateBitmap(w, h, 1, 1, None);
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

/// `WM_GETMINMAXINFO`: the track sizes of `form`'s limits (page area in DIP) plus the non-client
/// frame `nc` (px), at `scale`.
pub(crate) fn min_max(info: &mut MINMAXINFO, form: &FormOptions, nc: (i32, i32), scale: f32) {
    let px = |v: f32| (v * scale).round() as i32;
    if let Some((w, h)) = form.min_client_size {
        if w > 0.0 {
            info.ptMinTrackSize.x = info.ptMinTrackSize.x.max(px(w) + nc.0);
        }
        if h > 0.0 {
            info.ptMinTrackSize.y = info.ptMinTrackSize.y.max(px(h) + nc.1);
        }
    }
    if let Some((w, h)) = form.max_client_size {
        if w > 0.0 {
            info.ptMaxTrackSize.x = px(w) + nc.0;
        }
        if h > 0.0 {
            info.ptMaxTrackSize.y = px(h) + nc.1;
        }
    }
}

/// The work area of the monitor `hwnd` is on (physical px).
pub(crate) fn work_area_px(hwnd: HWND) -> (i32, i32, i32, i32) {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    // SAFETY: plain monitor queries with a correctly sized MONITORINFO.
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let RECT { left, top, right, bottom } = info.rcWork;
            (left, top, right, bottom)
        } else {
            (0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
        }
    }
}

thread_local! {
    /// The form a page asked for this frame ([`set_form`]), applied after the frame.
    static PENDING: std::cell::RefCell<Option<FormOptions>> = const { std::cell::RefCell::new(None) };
}

/// Changes the window's `Form` properties (see the module doc): applied after the frame, only what
/// differs from what the window already has. Call it every frame or only on a change.
pub fn set_form(form: FormOptions) {
    PENDING.with(|p| *p.borrow_mut() = Some(form));
}

pub(crate) fn take_pending() -> Option<FormOptions> {
    PENDING.with(|p| p.try_borrow_mut().ok().and_then(|mut f| f.take()))
}

/// Puts back a pending form taken with [`take_pending`] (`super::window_tls`).
pub(crate) fn put_pending(form: Option<FormOptions>) {
    PENDING.with(|p| {
        if let Ok(mut slot) = p.try_borrow_mut() {
            *slot = form;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use CaptionButtonState::*;

    #[test]
    fn caption_buttons_follow_winforms() {
        let f = FormOptions::default();
        assert_eq!(caption_buttons(&f), (Shown, Shown, true));
        assert_eq!(caption_buttons(&FormOptions { minimize_box: false, ..f.clone() }), (Disabled, Shown, true));
        assert_eq!(caption_buttons(&FormOptions { minimize_box: false, maximize_box: false, ..f.clone() }), (Hidden, Hidden, true));
        assert_eq!(caption_buttons(&FormOptions { control_box: false, ..f.clone() }), (Hidden, Hidden, false));
        assert_eq!(caption_buttons(&FormOptions { border_style: FormBorderStyle::FixedToolWindow, ..f.clone() }), (Hidden, Hidden, true));
        assert_eq!(caption_buttons(&FormOptions { border_style: FormBorderStyle::None, ..f }), (Hidden, Hidden, false));
    }

    #[test]
    fn styles_follow_the_border_and_the_buttons() {
        let base = WS_OVERLAPPEDWINDOW;
        let f = FormOptions::default();
        assert_eq!(window_style(&f, base).0 & WS_THICKFRAME.0, WS_THICKFRAME.0);
        let fixed = FormOptions { border_style: FormBorderStyle::FixedDialog, maximize_box: false, ..f.clone() };
        let s = window_style(&fixed, base);
        assert_eq!(s.0 & WS_THICKFRAME.0, 0);
        assert_eq!(s.0 & WS_MAXIMIZEBOX.0, 0);
        assert_eq!(s.0 & WS_MINIMIZEBOX.0, WS_MINIMIZEBOX.0);
        let none = window_style(&FormOptions { border_style: FormBorderStyle::None, ..f.clone() }, base);
        assert_eq!(none.0 & WS_CAPTION.0, 0);
        assert_eq!(none.0 & WS_POPUP.0, WS_POPUP.0);
        let ex = window_ex_style(&FormOptions { show_in_taskbar: false, opacity: 0.5, ..f }, WINDOW_EX_STYLE(0));
        assert_eq!(ex.0 & WS_EX_TOOLWINDOW.0, WS_EX_TOOLWINDOW.0);
        assert_eq!(ex.0 & WS_EX_LAYERED.0, WS_EX_LAYERED.0);
    }

    #[test]
    fn a_window_opens_where_its_start_position_says() {
        let f = FormOptions { start_position: StartPosition::CenterScreen, ..FormOptions::default() };
        assert_eq!(start_origin(&f, (800, 600), (0, 0, 1920, 1040), 1.0), Some((560, 220)));
        let m = FormOptions { start_position: StartPosition::Manual, location: Some((100.0, 50.0)), ..FormOptions::default() };
        assert_eq!(start_origin(&m, (800, 600), (0, 0, 1920, 1040), 1.5), Some((150, 75)));
        assert_eq!(start_origin(&FormOptions::default(), (800, 600), (0, 0, 1920, 1040), 1.0), None);
    }

    #[test]
    fn windows_are_rounded_by_default_and_square_without_a_title_bar() {
        let f = FormOptions::default();
        assert_eq!(f.corner_radius(), DEFAULT_CORNER_RADIUS);
        assert_eq!(FormOptions { border_style: FormBorderStyle::FixedDialog, ..f.clone() }.corner_radius(), 8.0);
        assert_eq!(FormOptions { border_style: FormBorderStyle::SizableToolWindow, ..f.clone() }.corner_radius(), 8.0);
        assert_eq!(FormOptions { border_style: FormBorderStyle::None, ..f.clone() }.corner_radius(), 0.0);
        assert_eq!(FormOptions { corner: CornerPreference::RoundSmall, ..f.clone() }.corner_radius(), 4.0);
        assert_eq!(FormOptions { corner: CornerPreference::DoNotRound, ..f.clone() }.corner_radius(), 0.0);
        assert_eq!(FormOptions { border_style: FormBorderStyle::None, corner: CornerPreference::Round, ..f.clone() }.corner_radius(), 8.0);
        // `CornerRadius` wins over the preset; a floating panel's radius over both.
        assert_eq!(FormOptions { corner: CornerPreference::DoNotRound, corner_radius: Some(24.0), ..f.clone() }.corner_radius(), 24.0);
        assert_eq!(FormOptions { corner_radius: Some(0.0), ..f.clone() }.corner_radius(), 0.0);
        assert_eq!(FormOptions { corner_radius: Some(-5.0), ..f.clone() }.corner_radius(), 0.0);
        assert_eq!(FormOptions { corner_radius: Some(12.0), panel: Some(FloatingPanel::new(28.0)), ..f }.corner_radius(), 28.0);
    }

    #[test]
    fn a_corner_radius_is_a_non_negative_number() {
        assert_eq!(parse_corner_radius(" 12.5 "), Some(12.5));
        assert_eq!(parse_corner_radius("0"), Some(0.0));
        assert_eq!(parse_corner_radius("-1"), None);
        assert_eq!(parse_corner_radius("inf"), None);
        assert_eq!(parse_corner_radius("large"), None);
    }

    #[test]
    fn size_limits_include_the_frame() {
        let f = FormOptions { min_client_size: Some((300.0, 0.0)), max_client_size: Some((0.0, 500.0)), ..FormOptions::default() };
        let mut info = MINMAXINFO::default();
        info.ptMaxTrackSize.x = 5000;
        min_max(&mut info, &f, (16, 40), 1.25);
        assert_eq!(info.ptMinTrackSize.x, 375 + 16);
        assert_eq!(info.ptMaxTrackSize.y, 625 + 40);
        assert_eq!(info.ptMaxTrackSize.x, 5000, "no limit on that axis");
    }
}
