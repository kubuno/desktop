//! A minimal, reusable Win32 + Direct2D host for the control library.
//!
//! Every control in this crate paints through [`drive_app_controls::Canvas`],
//! and this module is the smallest thing that can put such a surface on screen:
//! a plain window, a Direct2D swap chain (built by
//! [`drive_app_controls::Renderer`]) and a message loop that, on every paint,
//! hands a `&dyn Canvas` to a caller-supplied closure. That is the whole point
//! — the demo binaries mount the control families over this host and compare
//! them side by side with the reference sheets.
//!
//! It is a HOST, not an application framework. It carries no pages, no
//! navigation: it opens a window, tracks the pointer so hover and pressed
//! states can be exercised, and repaints. Anything richer belongs in the
//! caller's closure, which owns the controls and their state.
//!
//! An application window (the desktop shell) configures it through
//! [`HostOptions`] / [`run_with_options`] — its own title bar
//! ([`Chrome::Custom`] + [`set_title_bar`]), a DWM backdrop, a font override —
//! and plugs into it with [`on_message`] (tray callbacks, worker-thread
//! messages), [`set_close_handler`] (hide to the tray), [`quit`],
//! [`set_theme`] and [`set_font_override`].
//!
//! The idioms follow the shell's own Win32 hosting (`shell/src/window.rs`):
//! the window's `self` pointer lives in `GWLP_USERDATA`, per-monitor-v2 DPI is
//! set for the process, and `WM_DPICHANGED` resizes both the window and the
//! swap chain so the controls always paint at the display's real scale.

/// Where a GUI application's logs, `println!`s and panics go without a console.
pub mod diagnostics;
/// The window's accessibility tree, exposed to UI Automation through AccessKit.
pub mod access;
/// The window as a WinForms `Form`: title, icon, start position, borders, caption buttons…
pub mod form;
pub use form::{set_form, CornerPreference, FloatingPanel, FormBorderStyle, FormOptions, SizeGripStyle, StartPosition, WindowState};
mod backdrop;
/// The corners of a top-level window: DWM's presets, or the frame the host draws itself.
pub mod frame;
/// The page's side of the Kubuno chrome: the band's regions, drag areas, window events.
pub mod chrome;
pub use chrome::{take_window_events, title_bar_height, title_bar_layout, WindowEvent};
mod crash;
/// Drag and drop over OLE: the window as a drop target, `do_drag_drop` as a source (EVT-8).
pub mod dnd;
pub mod input;
/// The paint debug overlay (invalidated regions, layout bounds, frame time) (EVT-8).
pub mod paint_debug;
pub mod painter;
/// Several host windows on one UI thread: each window's thread-local state, swapped in and out.
mod window_tls;

pub use input::{
    clipboard_text, composition, consume, events, has_events, key_pressed, now_ms, repaint_requested, request_repaint_after, set_zoom, zoom,
    claim_wheel, close_window, set_caption_colors, set_clipboard_text, set_cursor, take_key, take_key_any, take_text, vk, wheel_claimed,
    Cursor, InputEvent, Modifiers, WHEEL_NOTCH_DIP,
};
// Lifecycle and cross-thread services (`vskubuno/docs/EVENTS.md` EVT-6).
pub use input::{cancel_close, defer_close, request_close, request_wake_after, ui_waker, CloseReason, UiWaker, WM_KUBUNO_WAKE};
pub use painter::Painter;

use crate::control::ControlCanvas;
use crate::system::Visuals;
use crate::theme::ThemeRenderer;
use drive_app_controls::{Rect, Renderer, Theme};
use windows::core::{w, Result, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, InvalidateRect, MonitorFromWindow, ScreenToClient, ValidateRect,
    MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForWindow, GetSystemMetricsForDpi, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::Graphics::Dwm::{DwmFlush, DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetCapture, GetDoubleClickTime, GetFocus, GetKeyState, IsWindowEnabled, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE,
    TRACKMOUSEEVENT, VK_LBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::*;

pub mod caption;
use caption::Hot as CaptionHot;
pub use caption::{Hot as CaptionButton, TitleBar};

/// Which chrome the host paints around the client area.
///
/// * `Chrome::System` — Windows draws its own title bar and buttons. The
///   default; unchanged behaviour for every existing caller.
/// * `Chrome::Kubuno` — the host strips the system caption (WM_NCCALCSIZE),
///   paints the Kubuno title band itself (see [`caption`]), and turns clicks on
///   the min/max/close slots into the matching `SC_*` system command. On
///   Windows 10 the same window opens with the system chrome, because
///   `WM_NCCALCSIZE` still fires but the DWM tint injected by [`run`] is a
///   no-op — the fallback is honest, not silent.
/// * `Chrome::Custom` — the system caption is stripped exactly as for
///   `Kubuno`, but the host paints NOTHING and reserves nothing
///   (`Frame::chrome_top == 0`): the page draws its own title bar and declares
///   its geometry every frame with [`set_title_bar`], which drives
///   `WM_NCHITTEST` (resize borders, `HTMINBUTTON`/`HTMAXBUTTON`/`HTCLOSE`,
///   drag band). The hovered caption button is read back with
///   [`caption_hot`]. The DWM caption/border tint is not applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Chrome {
    #[default]
    System,
    Kubuno,
    Custom,
}

/// The system backdrop material DWM puts behind the window (Windows 11 22H2+,
/// a no-op elsewhere). The client area still paints opaquely: the material
/// shows in the frame, and wherever a page chooses to leave pixels
/// transparent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backdrop {
    /// Leave the attribute alone — the window's default look.
    #[default]
    None,
    /// `DWMSBT_MAINWINDOW`, the Mica material.
    Mica,
    /// `DWMSBT_TABBEDWINDOW`, "Mica Alt" — the one Windows 11 gives to shells
    /// with a content pane.
    MicaAlt,
    /// `DWMSBT_TRANSIENTWINDOW`, the Acrylic material (flyouts, popovers).
    Acrylic,
}

impl Backdrop {
    /// The `DWM_SYSTEMBACKDROP_TYPE` value, or `None` for "do not set".
    const fn dwm_value(self) -> Option<i32> {
        match self {
            Self::None => None,
            Self::Mica => Some(2),
            Self::Acrylic => Some(3),
            Self::MicaAlt => Some(4),
        }
    }
}

/// Everything [`run_with_options`] needs to open the host window. Build it
/// with [`HostOptions::new`] and adjust the public fields.
pub struct HostOptions {
    /// Window title (task bar, Alt+Tab, and the Kubuno caption's text).
    pub title:         String,
    /// Logical client-ish size in DIP, applied once the real DPI is known.
    pub width:         u32,
    pub height:        u32,
    pub theme:         Theme,
    pub chrome:        Chrome,
    /// A font family that replaces the design system's embedded face in the
    /// renderer's text formats (see `Renderer::new`). `None` = the default.
    pub font_override: Option<String>,
    pub backdrop:      Backdrop,
    /// Create the window without showing it (a tray application started with
    /// Windows). [`show_window`] / [`restore_and_focus`] bring it up later.
    pub start_hidden:  bool,
    /// Clamp the initial size to the work area of the monitor the window opens
    /// on, so a large logical size still fits a small or highly scaled screen.
    pub fit_work_area: bool,
    /// `width`×`height` is the PAGE area (the client area below the host's own
    /// caption, from `y = Frame::chrome_top`), not the outer window: the window
    /// is grown by its frame and caption so the page gets exactly that size - a
    /// WinForms `Form` opening at its designed `ClientSize`. `false` (the
    /// default) keeps the historical outer-window size.
    pub client_size:   bool,
    /// Embed the host as a CHILD of this window (a raw `HWND` value, possibly
    /// owned by another process) instead of opening a top-level window.
    ///
    /// The window is then created `WS_CHILD | WS_CLIPSIBLINGS |
    /// WS_CLIPCHILDREN | WS_VISIBLE` at the parent's client origin and size,
    /// with no caption, no DWM attributes and no size-to-DPI pass: the PARENT
    /// drives the geometry (it moves/resizes the child, which reacts to its
    /// own `WM_SIZE`). `chrome`, `backdrop`, `start_hidden`, `fit_work_area`
    /// and the logical size are ignored. A click gives the child the keyboard
    /// focus, losing it is the "blur" that dismisses open menus (a child
    /// never sees `WM_ACTIVATE`), and the DPI follows the parent through
    /// `WM_DPICHANGED_AFTERPARENT`. The message loop ends when the child is
    /// destroyed with its parent, or when the parent window disappears
    /// (polled: a parent process that crashes may send nothing).
    ///
    /// Used by the Visual Studio designer surface (`vskubuno`, DSG-7), whose
    /// WPF `HwndHost` hands its own child window over the command line.
    pub parent:        Option<isize>,
    /// Install the process's diagnostics sink ([`diagnostics::install`]) when the window opens:
    /// `tracing`/`log` output, `println!`s without a console and panics go to the debugger's
    /// Output window, or to `%LOCALAPPDATA%\Kubuno\logs\<exe>.log`; a panic shows an error dialog
    /// (not for an embedded [`HostOptions::parent`] window). `true` by default; `false` for an
    /// application that routes its output itself.
    pub diagnostics:   bool,
    /// The window's `Form` properties (title, icon, start position, borders, caption buttons, task
    /// bar, top-most, opacity, state, size limits); a normal window by default. Changed later with
    /// [`set_form`]. Ignored for an embedded [`HostOptions::parent`] window.
    pub form:          FormOptions,
    /// The window that OWNS this one (a raw `HWND` value): an owned top-level window stays above
    /// its owner, is minimised with it and has no task bar button of its own — a WinForms
    /// `Form.Owner`, what a dialog opened with `ShowDialog(owner)` gets. `StartPosition =
    /// CenterParent` centres the window on it. Ignored for an embedded [`HostOptions::parent`] window.
    pub owner:         Option<isize>,
    /// While this window is open, every other host window of the thread is disabled (a WinForms
    /// modal dialog, `ShowDialog`): they get no input, and are enabled again before this window
    /// goes away, so the activation returns to the owner.
    pub modal:         bool,
}

impl HostOptions {
    /// Options with the defaults the historical [`run`] used: system chrome,
    /// no font override, no backdrop, shown at once, size not clamped.
    pub fn new(title: &str, width: u32, height: u32, theme: Theme) -> Self {
        Self {
            title: title.to_string(),
            width,
            height,
            theme,
            chrome: Chrome::System,
            font_override: None,
            backdrop: Backdrop::None,
            start_hidden: false,
            fit_work_area: false,
            client_size: false,
            parent: None,
            diagnostics: true,
            form: FormOptions::default(),
            owner: None,
            modal: false,
        }
    }
}

/// How a host window's life relates to the message loop of its thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// Opened by [`run_with_options`] with no other window running: its loop is the thread's main
    /// loop, and its destruction ends it (and closes the thread's other windows).
    Main,
    /// Opened by [`run_with_options`] / [`run_scoped`] from inside another window (a dialog opened
    /// from a frame): a nested loop that ends when this window is destroyed.
    Nested,
    /// Opened by [`open_window`]: no loop of its own (the running one dispatches its messages); its
    /// `Host` is freed when the window is destroyed.
    Modeless,
}

/// A window message as handed to a handler registered with [`on_message`].
#[derive(Debug, Clone, Copy)]
pub struct MessageArgs {
    pub hwnd:   HWND,
    pub msg:    u32,
    pub wparam: WPARAM,
    pub lparam: LPARAM,
}

/// A per-message handler (see [`on_message`]).
type MsgHandler = Box<dyn FnMut(&MessageArgs) -> Option<LRESULT>>;
/// The close handler (see [`set_close_handler`]).
type CloseFn = Box<dyn FnMut() -> bool>;
/// The set-up hook (see [`on_ready`]).
type ReadyFn = Box<dyn FnOnce(HWND)>;

/// Where [`Frame::mouse`] is parked once the pointer has left the window: far
/// outside any plausible layout, but finite so arithmetic on it stays finite.
pub const POINTER_AWAY: f32 = -100_000.0;

/// `WM_MOUSELEAVE`, the answer to a `TME_LEAVE` request. Stated here because the
/// `windows` crate files it under `Win32_UI_Controls`, not next to
/// `TrackMouseEvent`, and a pattern on an unresolved name silently binds
/// everything.
const WM_MOUSELEAVE: u32 = 0x02A3;

/// The `SetTimer` id of [`request_repaint_after`]'s one-shot repaint.
const REPAINT_TIMER_ID: usize = 0x4B55;

/// The `SetTimer` id of [`request_wake_after`]'s one-shot wake-up.
const WAKE_TIMER_ID: usize = 0x4B56;

/// Embedded mode only: how often the thread timer checks that the parent
/// window still exists ([`parent_watch`]), in milliseconds.
const PARENT_WATCH_MS: u32 = 500;

/// Fully transparent — the ground of a floating surface's window.
const TRANSPARENT: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F =
    windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

/// What the paint closure is told about the frame it is drawing.
///
/// Everything is in DIP (device-independent pixels), so a caller can lay out
/// controls and decide hover/pressed states without ever touching Win32 or the
/// DPI scale itself. `scale` is offered too, for the rare caller that sizes
/// something before it has the `Canvas` in hand.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    /// The client area, in DIP.
    pub size: (f32, f32),
    /// The pointer, in DIP, relative to the client area. Stays at its last
    /// position between moves; a control reads it to light its hover state.
    /// When the pointer leaves the window (and every interactive popup) it
    /// is parked at `(POINTER_AWAY, POINTER_AWAY)` — far off, finite — so no
    /// hover lingers; during a left-button drag it keeps following the pointer
    /// outside the window (the window captures the mouse).
    pub mouse: (f32, f32),
    /// Whether the left mouse button is currently held. A page detects a
    /// click by comparing this across frames (`down && !prev_down`), so a
    /// press is GUARANTEED to read as down for at least one rendered frame
    /// even when the matching release already arrived before that frame
    /// painted — a fast tap (touchpad, synthetic click) can complete its
    /// whole press+release between two frames, since `WM_PAINT` is only
    /// synthesized once the message queue is otherwise empty. See
    /// [`consume_button_edge`] for how the host latches this.
    pub mouse_down: bool,
    /// Whether the RIGHT mouse button is currently held — its rising edge is
    /// what opens a context menu. Latched exactly like [`Frame::mouse_down`].
    pub right_down: bool,
    /// Whether the MIDDLE mouse button is currently held. Latched exactly
    /// like [`Frame::mouse_down`].
    pub middle_down: bool,
    /// `true` for the one frame after the window lost activation — a click on
    /// the desktop or another application, Alt-Tab. Menus, popovers and help
    /// bubbles close on it, the way the web closes them on blur: a floating
    /// surface overflowing the window can be clicked around, and that click
    /// never reaches this window.
    pub dismiss: bool,
    /// `dpi / 96`, clamped to > 0 — the same factor `Canvas::scale` returns.
    pub scale: f32,
    /// The client area's top-left in screen DIP.
    pub client_origin: (f32, f32),
    /// The work area of the monitor the window sits on, in screen DIP
    /// `(left, top, right, bottom)`. See [`Frame::screen_area`] for the same
    /// rectangle in client coordinates, which is what a page places against.
    pub work_area: (f32, f32, f32, f32),
    /// How much of the client's top edge the host reserved for its own chrome,
    /// in DIP. `0.0` under `Chrome::System` and `Chrome::Custom` (the page
    /// paints its own title bar); the band's height under `Chrome::Kubuno` (44 DIP by default, `0` when the page extends under it). A page's own top strip (its tab strip, its ribbon…)
    /// starts at `y = chrome_top`; content below stays where it was.
    pub chrome_top: f32,
    /// The modifier keys held as the frame starts (Ctrl+click, Shift+click,
    /// Shift+wheel). Key events carry their own snapshot in
    /// [`InputEvent::Key`].
    pub mods: Modifiers,
    /// Wheel travel since the previous frame, in NOTCHES, web sign convention
    /// (`WheelEvent.deltaX/deltaY`): `.1 > 0` scrolls DOWN (wheel rolled
    /// toward the user), `.0 > 0` scrolls RIGHT. Fractional on precision
    /// touchpads. `(0, 0)` on a frame without wheel input. See
    /// [`Frame::wheel_dip`] for a distance. Shift+wheel is NOT turned into
    /// horizontal scrolling here — a page that wants it reads `mods.shift`.
    pub wheel: (f32, f32),
    /// How many quick successive left presses the current (or last) press
    /// completes: `1` single, `2` double, `3` triple (it saturates at 3), using
    /// the system double-click time and distance. Read it on the frame the
    /// button goes down. `0` before the first press.
    pub click_count: u8,
    /// Whether the host window holds the keyboard focus (the web's
    /// `document.hasFocus()`): a caret hides and stops blinking when not.
    pub window_focused: bool,
}

impl Frame {
    /// [`Frame::wheel`] as a distance in DIP ([`WHEEL_NOTCH_DIP`] per notch),
    /// same signs.
    pub fn wheel_dip(&self) -> (f32, f32) {
        (self.wheel.0 * WHEEL_NOTCH_DIP, self.wheel.1 * WHEEL_NOTCH_DIP)
    }

    /// Whether the pointer is on the host's "nowhere" position — it left the
    /// window (and every interactive popup) — so no hover should show.
    pub fn pointer_outside(&self) -> bool {
        self.mouse.0 <= POINTER_AWAY || self.mouse.1 <= POINTER_AWAY
    }

    /// The monitor's work area in **client** DIP — the viewport a floating
    /// surface places itself in. It reaches past the window on every side the
    /// monitor does, which is what lets a tooltip, a menu or a help bubble
    /// overflow its owner instead of being squeezed inside it.
    pub fn screen_area(&self) -> Rect {
        let (ox, oy) = self.client_origin;
        let (l, t, r, b) = self.work_area;
        Rect::new(l - ox, t - oy, r - ox, b - oy)
    }
}

/// A floating surface a page asked to paint this frame, in its own top-level
/// window that may extend beyond the host window (see [`overlay`] and
/// [`popup`]).
struct OverlayReq {
    /// Where the surface goes, in CLIENT DIP — it may lie partly or wholly
    /// outside the client area.
    bounds:      Rect,
    /// Whether the surface takes the pointer ([`popup`]) or lets it through
    /// ([`overlay`]).
    interactive: bool,
    /// Paints the surface into a canvas whose origin is `bounds`' top-left.
    paint:       OverlayPaintFn,
}

thread_local! {
    /// Floating surfaces requested during the current frame, drained by the host
    /// once the main window has finished painting. Thread-local because the host
    /// is single-threaded and the page has no handle to it — it just calls
    /// [`overlay`] or [`popup`] from inside its paint.
    static OVERLAYS: std::cell::RefCell<Vec<OverlayReq>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// Where content coordinates sit in the client area: the sum of the
    /// scroll offsets a painter has in force (see [`content_offset`]).
    static CONTENT_OFFSET: std::cell::Cell<(f32, f32)> = const { std::cell::Cell::new((0.0, 0.0)) };
    /// The interactive surfaces of the last frame, in client DIP — what
    /// [`over_popup`] tests the pointer against.
    static LAST_POPUPS: std::cell::RefCell<Vec<Rect>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Requests a **pass-through** floating surface — a tooltip — painted after the
/// current frame in a top-level window of its own, so it sits above everything
/// and may extend beyond the host window's edges.
///
/// `bounds` is in client DIP (place it against [`Frame::screen_area`], the
/// monitor, not the window); the closure paints into a canvas whose origin is
/// `bounds`' top-left. The pointer goes through it to whatever is underneath.
/// Several surfaces may be requested in one frame; later ones stack above
/// earlier ones. A surface not requested again is hidden.
pub fn overlay<F>(bounds: Rect, paint: F)
where
    F: FnOnce(&dyn ControlCanvas) + 'static,
{
    push_surface(bounds, false, paint);
}

/// Requests an **interactive** floating surface — a menu, a help bubble, a
/// popover — exactly like [`overlay`], except that it takes the pointer: hover
/// and clicks over it are reported through [`Frame::mouse`] in the page's own
/// client coordinates (outside the client area when the surface overflows), so
/// the page hit-tests it with the same rectangles it placed it with. It never
/// takes the keyboard focus from the window that opened it.
pub fn popup<F>(bounds: Rect, paint: F)
where
    F: FnOnce(&dyn ControlCanvas) + 'static,
{
    push_surface(bounds, true, paint);
}

fn push_surface<F>(bounds: Rect, interactive: bool, paint: F)
where
    F: FnOnce(&dyn ControlCanvas) + 'static,
{
    // A page inside a scrolled area places its surfaces in its own content
    // coordinates; the host windows live in client coordinates.
    let (dx, dy) = content_offset();
    let bounds = Rect::new(bounds.left + dx, bounds.top + dy, bounds.right + dx, bounds.bottom + dy);
    OVERLAYS.with(|o| {
        o.borrow_mut().push(OverlayReq { bounds, interactive, paint: Box::new(paint) })
    });
}

/// Where the content being painted sits in the client area, in DIP: the sum of
/// the scroll offsets in force (`Canvas::push_offset`). A point in content
/// coordinates plus this offset is a point in client coordinates. `(0, 0)`
/// outside any scrolled area.
///
/// [`overlay`] and [`popup`] already apply it; a component that keeps
/// rectangles across frames for hit-testing against the RAW pointer (a focus
/// ring) converts with it.
pub fn content_offset() -> (f32, f32) {
    CONTENT_OFFSET.with(|c| c.get())
}

/// Moves the content origin — called by the painter as offsets are pushed
/// and popped, so it always matches the transform in force.
pub(crate) fn shift_content_offset(dx: f32, dy: f32) {
    CONTENT_OFFSET.with(|c| {
        let (x, y) = c.get();
        c.set((x + dx, y + dy));
    });
}

/// Whether the client point `(x, y)` lies on one of the interactive floating
/// surfaces ([`popup`]) shown by the previous frame. A scrolled area uses it
/// to keep delivering the pointer to its content while a menu the content
/// opened overflows the area.
pub fn over_popup(x: f32, y: f32) -> bool {
    LAST_POPUPS.with(|l| l.borrow().iter().any(|r| r.contains(x, y)))
}

thread_local! {
    /// The title bar the page declared this frame (`Chrome::Custom`).
    static TITLE_BAR: std::cell::RefCell<TitleBar> = std::cell::RefCell::new(TitleBar::default());
    /// Mirror of `Host::caption_hot`, readable from the page (see [`caption_hot`]).
    static CAPTION_HOT: std::cell::Cell<Option<CaptionHot>> = const { std::cell::Cell::new(None) };
    /// Per-message handlers registered with [`on_message`], in registration order.
    static HANDLERS: std::cell::RefCell<std::collections::HashMap<u32, Vec<MsgHandler>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    /// The handler consulted on `WM_CLOSE` (see [`set_close_handler`]).
    static CLOSE_HANDLER: std::cell::RefCell<Option<CloseFn>> =
        const { std::cell::RefCell::new(None) };
    /// Set by [`quit`]: the next `WM_CLOSE` bypasses the close handler.
    static QUIT_REQUESTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A theme asked for by [`set_theme`], applied before the next frame.
    static PENDING_THEME: std::cell::RefCell<Option<Theme>> = const { std::cell::RefCell::new(None) };
    /// A font override asked for by [`set_font_override`] (outer `Some` = a
    /// change is pending), applied before the next frame.
    static PENDING_FONT: std::cell::RefCell<Option<Option<String>>> = const { std::cell::RefCell::new(None) };
    /// Run once the window exists, right before the message loop (see [`on_ready`]).
    static ON_READY: std::cell::RefCell<Option<ReadyFn>> = const { std::cell::RefCell::new(None) };
    /// The font override in force (see [`current_font_override`]).
    static CURRENT_FONT: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The application's font override in force ([`HostOptions::font_override`], [`set_font_override`]):
/// what a part of the window restyled with its own font (`crate::styled`) starts from.
pub fn current_font_override() -> Option<String> {
    CURRENT_FONT.with(|f| f.borrow().clone())
}

/// Declares the page's own title bar for `Chrome::Custom`, in client DIP. Call
/// it EVERY frame from the paint closure: each frame starts with an empty
/// declaration (no drag band, no buttons), and the last call of a frame is what
/// `WM_NCHITTEST` uses until the next one. Ignored under the other chromes.
pub fn set_title_bar(bar: TitleBar) {
    TITLE_BAR.with(|t| *t.borrow_mut() = bar);
}

/// Declares an interactive control placed in the title band (a search field, tabs, an avatar,
/// see [`title_bar_layout`]), in client DIP: it takes the pointer instead of dragging the window.
/// Declared every frame, like [`set_title_bar`].
pub fn add_title_bar_hole(rect: Rect) {
    TITLE_BAR.with(|t| t.borrow_mut().no_drag.push(rect));
}

/// Declares an area that drags the window (`TitleBar.Drag`): anywhere under the Kubuno chrome, and
/// the only way to move a borderless window. Declared every frame; wins over the holes.
pub fn add_title_bar_drag(rect: Rect) {
    chrome::push_drag(rect);
}

/// A message filter ([`add_message_filter`]): `true` when it handled the message, which is then
/// not dispatched.
pub type MessageFilter = Box<dyn FnMut(&MessageArgs) -> bool>;

thread_local! {
    /// The application's message filters (not per window: every loop of the thread runs them).
    static FILTERS: std::cell::RefCell<Vec<MessageFilter>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The theme every window of the thread follows ([`set_theme_all`]), and its generation.
    static THREAD_THEME: std::cell::RefCell<(u64, Option<Theme>)> = const { std::cell::RefCell::new((0, None)) };
}

/// Adds a filter every message of the thread's loops goes through before it is dispatched
/// (Windows Forms' `Application.AddMessageFilter`): returning `true` eats the message.
pub fn add_message_filter(filter: impl FnMut(&MessageArgs) -> bool + 'static) {
    FILTERS.with(|f| f.borrow_mut().push(Box::new(filter)));
}

/// Removes every message filter.
pub fn clear_message_filters() {
    FILTERS.with(|f| f.borrow_mut().clear());
}

/// Runs the filters on `msg`; `true` when one ate it. The filters are moved out while they run.
fn run_filters(msg: &MSG) -> bool {
    let Some(mut taken) = FILTERS.with(|f| f.try_borrow_mut().ok().map(|mut v| std::mem::take(&mut *v))) else { return false };
    if taken.is_empty() {
        return false;
    }
    let args = MessageArgs { hwnd: msg.hwnd, msg: msg.message, wparam: msg.wParam, lparam: msg.lParam };
    let eaten = taken.iter_mut().any(|f| f(&args));
    FILTERS.with(|f| {
        if let Ok(mut v) = f.try_borrow_mut() {
            let added = std::mem::take(&mut *v);
            taken.extend(added);
            *v = taken;
        }
    });
    eaten
}

/// Switches EVERY host window of the thread to `theme` (live theme switching: the open forms, their
/// dialogs and tool windows repaint in it), and the windows opened afterwards that do not ask for
/// another one.
pub fn set_theme_all(theme: Theme) {
    THREAD_THEME.with(|t| {
        let mut t = t.borrow_mut();
        t.0 += 1;
        t.1 = Some(theme);
    });
    for hwnd in input::thread_windows() {
        // SAFETY: invalidating a window handle is sound even if it has since been destroyed.
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}

fn thread_theme() -> (u64, Option<Theme>) {
    THREAD_THEME.with(|t| t.borrow().clone())
}

/// The caption button the pointer is over, under `Chrome::Kubuno` or
/// `Chrome::Custom`. The buttons are non-client (so the snap-layouts flyout
/// works), which means their hover never reaches [`Frame::mouse`]; a page that
/// paints its own buttons lights them from this instead.
pub fn caption_hot() -> Option<CaptionButton> {
    CAPTION_HOT.with(|c| c.get())
}

/// The host's main window, once it exists and until it is destroyed.
pub fn main_window() -> Option<HWND> {
    input::main_hwnd()
}

/// Registers `handler` for window message `msg` on the host's main window
/// (thread-local: call it on the thread that calls [`run_with_options`],
/// before or during the run). The wndproc calls the handlers of a message, in
/// registration order, BEFORE its own handling; the first to return
/// `Some(result)` ends the message with that result, `None` lets the next
/// handler (then the host) process it. The window is invalidated after the
/// handlers ran, so state they changed shows on the next frame.
///
/// For private messages (`WM_APP + n` posted by a worker thread, a tray
/// icon's callback), `WM_SETTINGCHANGE`, or a registered message such as
/// `TaskbarCreated`. No borrow is held while a handler runs, so it may call
/// any host API (including [`on_message`]); a message sent re-entrantly to the
/// window from inside a handler of the SAME message skips the handlers that
/// are running (only ones registered meanwhile see it) and goes on to the
/// host. `WM_NCCREATE` is never dispatched.
pub fn on_message(msg: u32, handler: impl FnMut(&MessageArgs) -> Option<LRESULT> + 'static) {
    HANDLERS.with(|h| h.borrow_mut().entry(msg).or_default().push(Box::new(handler)));
}

/// [`on_message`] with the result as a plain integer (for code that does not use the `windows`
/// crate).
pub fn on_message_value(msg: u32, mut handler: impl FnMut(&MessageArgs) -> Option<isize> + 'static) {
    on_message(msg, move |args| handler(args).map(LRESULT));
}

/// Decides what `WM_CLOSE` does (the caption's close button, Alt+F4,
/// [`close_window`]): `true` proceeds — the window is destroyed and the
/// message loop ends — `false` cancels it (a tray application hides instead).
/// Replaces any previous handler. [`quit`] bypasses it.
pub fn set_close_handler(handler: impl FnMut() -> bool + 'static) {
    CLOSE_HANDLER.with(|c| *c.borrow_mut() = Some(Box::new(handler)));
}

/// Really closes the host window and ends [`run_with_options`], whatever the
/// close handler says. Asynchronous (posted): the loop ends once the message
/// is processed.
pub fn quit() {
    QUIT_REQUESTED.with(|q| q.set(true));
    close_window();
}

/// Shows (`true`) or hides (`false`) the host window, without activating it.
pub fn show_window(show: bool) {
    let Some(hwnd) = main_window() else { return };
    // SAFETY: plain window-state calls on the host's own window, from its thread.
    unsafe {
        let _ = ShowWindow(hwnd, if show { SW_SHOW } else { SW_HIDE });
    }
}

/// Posted by [`resize_page`]: `wParam`/`lParam` are the page's width and height in hundredths of DIP.
const WM_KUBUNO_RESIZE_PAGE: u32 = WM_APP + 0x4B58;

/// Resizes the open window `hwnd` (one of this thread's host windows) so its page area is
/// `width × height` DIP — WinForms' `ClientSize` set on a shown form. A floating panel's window
/// keeps its shadow margin around the page; the window keeps its top-left corner.
///
/// Posted, not sent: asked from an event handler, it runs inside the window's frame, while the
/// renderer still holds its buffers (they cannot be resized then); the window is resized once the
/// frame is done.
pub fn resize_page(hwnd: isize, width: f32, height: f32) {
    let hundredths = |v: f32| (v.max(1.0) * 100.0).round() as usize;
    // SAFETY: a message posted to a window handle; a stale one makes the call fail.
    unsafe {
        let _ = PostMessageW(
            Some(HWND(hwnd as *mut core::ffi::c_void)),
            WM_KUBUNO_RESIZE_PAGE,
            WPARAM(hundredths(width)),
            LPARAM(hundredths(height) as isize),
        );
    }
}

/// Shows (`true`, activating it) or hides (`false`) the host window `hwnd` (a raw `HWND` value) —
/// a form's `Visible`, for any window of the thread rather than the running one.
pub fn set_window_visible(hwnd: isize, visible: bool) {
    // SAFETY: `ShowWindow` accepts any handle value; a stale one makes the call fail.
    unsafe {
        let _ = ShowWindow(HWND(hwnd as *mut core::ffi::c_void), if visible { SW_SHOW } else { SW_HIDE });
    }
}

/// Whether the window whose frame is running (else the thread's main host window) is on screen:
/// `false` while it is hidden (`start_hidden`, `Hide()`), `true` when there is no window at all.
/// A minimised window counts as shown. What a view reads to raise `Shown` only once its window has
/// actually been shown (a form started hidden gets `Load` from a frame run off screen).
pub fn window_visible() -> bool {
    match main_window() {
        // SAFETY: a plain window-state query; a stale handle answers `false`.
        Some(hwnd) => unsafe { IsWindowVisible(hwnd).as_bool() },
        None => true,
    }
}

/// Shows the host window, restores it if minimised, and brings it to the
/// foreground — what a tray icon's "Show" does.
pub fn restore_and_focus() {
    let Some(hwnd) = main_window() else { return };
    // SAFETY: plain window-state calls on the host's own window, from its thread.
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Switches the host to `theme` before the next frame: the page ground, the
/// painter's palette, the immersive dark mode of the frame and (except under
/// `Chrome::Custom`) the DWM caption/border tint.
pub fn set_theme(theme: Theme) {
    PENDING_THEME.with(|t| *t.borrow_mut() = Some(theme));
    invalidate_main();
}

/// Replaces the renderer's font override before the next frame. The renderer,
/// the system visuals and the floating-surface windows are rebuilt, exactly as
/// on a DPI change.
pub fn set_font_override(font: Option<String>) {
    PENDING_FONT.with(|f| *f.borrow_mut() = Some(font));
    invalidate_main();
}

/// Runs `f` once with the main window, after it is created and set up (and
/// shown, unless `start_hidden`) and right before the message loop starts —
/// where a tray icon is added or a worker thread is handed the window to post
/// to. Register it before [`run_with_options`].
pub fn on_ready(f: impl FnOnce(HWND) + 'static) {
    ON_READY.with(|r| *r.borrow_mut() = Some(Box::new(f)));
}

fn invalidate_main() {
    if let Some(hwnd) = main_window() {
        // SAFETY: invalidating a window handle is sound even if it has since
        // been destroyed (the call then simply fails).
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}

fn has_pending_changes() -> bool {
    PENDING_THEME.with(|t| t.borrow().is_some()) || PENDING_FONT.with(|f| f.borrow().is_some())
}

/// Runs the [`on_message`] handlers of `args.msg`. `None` when none is
/// registered; otherwise `Some(result)` with the first handler's answer.
///
/// The handlers are moved OUT of the map while they run, so a handler may
/// register more (they are appended after it) or call anything else without
/// meeting a live `RefCell` borrow.
fn dispatch_handlers(args: &MessageArgs) -> Option<Option<LRESULT>> {
    let mut taken = HANDLERS.with(|h| h.borrow_mut().remove(&args.msg))?;
    let mut result = None;
    for handler in taken.iter_mut() {
        if let Some(r) = handler(args) {
            result = Some(r);
            break;
        }
    }
    HANDLERS.with(|h| {
        let mut map = h.borrow_mut();
        let added = map.remove(&args.msg).unwrap_or_default();
        taken.extend(added);
        map.insert(args.msg, taken);
    });
    Some(result)
}

/// Asks the close handler whether `WM_CLOSE` may proceed. `true` when there is
/// none, or after [`quit`].
fn close_allowed() -> bool {
    if QUIT_REQUESTED.with(|q| q.get()) {
        return true;
    }
    let Some(mut handler) = CLOSE_HANDLER.with(|c| c.borrow_mut().take()) else {
        return true;
    };
    let proceed = handler();
    // Put it back unless it installed a replacement while running.
    CLOSE_HANDLER.with(|c| {
        let mut slot = c.borrow_mut();
        if slot.is_none() {
            *slot = Some(handler);
        }
    });
    proceed
}

/// The caller's per-frame paint callback, boxed so the host is not generic
/// over it — the message loop and the `GWLP_USERDATA` pointer both want a
/// concrete `Host` type, free of a closure type parameter.
type PaintFn = Box<dyn FnMut(&dyn ControlCanvas, &Frame)>;

/// A paint callback that borrows from the caller of [`run_scoped`].
type ScopedPaintFn<'a> = Box<dyn FnMut(&dyn ControlCanvas, &Frame) + 'a>;

/// A floating surface's one-shot painter, boxed for the same reason (see
/// [`OverlayReq`]).
type OverlayPaintFn = Box<dyn FnOnce(&dyn ControlCanvas)>;

/// The window and its render state. Boxed and parked in `GWLP_USERDATA`, it is
/// the only long-lived thing the host owns.
struct Host {
    hwnd:       HWND,
    renderer:   Option<Renderer>,
    theme:      Theme,
    /// The system's colours, metrics and UI font, read at [`Host::dpi`] and
    /// rebuilt whenever that changes. Owned here rather than by the `Painter`
    /// because a `Painter` lives for one frame and these cost three DirectWrite
    /// text formats to build.
    visuals:    Option<Visuals>,
    /// The themed parts `uxtheme.dll` renders, cached as Direct2D bitmaps.
    ///
    /// Owned by the window, like [`Host::visuals`], and for a stronger version
    /// of the same reason: rebuilding it per frame would mean a GDI round trip
    /// per control per frame. It is created unconditionally — on a themed-off
    /// machine it simply answers "no" to everything and every control paints
    /// classic, which costs one boolean read.
    parts:      ThemeRenderer,
    dpi:        f32,
    /// The pointer in DIP, carried across paints so a repaint triggered by
    /// anything other than a move still knows where the cursor is.
    mouse:      (f32, f32),
    mouse_down: bool,
    right_down: bool,
    middle_down: bool,
    /// Set by a `WM_*BUTTONDOWN`, consumed (cleared) the next time a frame is
    /// actually rendered: guarantees the matching [`Frame`] field reports at
    /// least one frame of "down", even if the button's `WM_*BUTTONUP` already
    /// arrived before that frame painted. Never cleared by the `*BUTTONUP`
    /// itself — that message is exactly the one that can race ahead of the
    /// paint, so touching the latch there would defeat the point. See
    /// [`consume_button_edge`].
    left_press_latch:   bool,
    right_press_latch:  bool,
    middle_press_latch: bool,
    /// Set when the window loses activation, handed to the next frame as
    /// [`Frame::dismiss`] and cleared.
    dismiss_pending: bool,
    /// The logical window size the caller asked for, applied once the real DPI
    /// is known (see [`Host::size_to_dpi`]).
    want:       (u32, u32),
    /// The caller's per-frame painter (see [`PaintFn`]).
    on_paint:   PaintFn,
    /// The popup windows that host floating surfaces ([`overlay`], [`popup`]),
    /// one per surface of the busiest frame so far, created on demand and
    /// reused — the n-th surface of a frame always goes to the n-th window.
    popups:     Vec<OverlayWindow>,
    title:      String,
    chrome:     Chrome,
    /// Which caption button the pointer is over, when `chrome == Kubuno`.
    /// Redrawn on `WM_NCMOUSEMOVE` so the wash follows the pointer.
    caption_hot: Option<CaptionHot>,
    /// Wheel notches accumulated since the last frame (see [`Frame::wheel`]).
    wheel:      (f32, f32),
    /// The press count of the current/last left press (see
    /// [`Frame::click_count`]), and when/where (screen px) that press was.
    click_count: u8,
    last_press: (u32, i32, i32),
    /// A UTF-16 high surrogate from `WM_CHAR`, waiting for its low half.
    high_surrogate: Option<u16>,
    /// Whether a `TME_LEAVE` request is armed on the main window.
    tracking_leave: bool,
    /// Whether the main window holds the keyboard focus.
    window_focused: bool,
    /// The pointer shape the last frame asked for (see [`set_cursor`]).
    cursor:     Cursor,
    /// The caption band painted last frame (the accent, or what the page asked
    /// for), and the colour DWM draws the window border in — kept equal so
    /// the frame shows no seam.
    caption_band: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F,
    dwm_band:     u32,
    /// The renderer's font override (see [`HostOptions::font_override`]).
    font_override: Option<String>,
    /// The DWM backdrop material, re-applied with the theme.
    backdrop:     Backdrop,
    /// Whether the initial size is clamped to the monitor's work area.
    fit_work_area: bool,
    /// Whether `want` is the page area ([`HostOptions::client_size`]).
    client_size:  bool,
    /// The window it is embedded in ([`HostOptions::parent`]), or `None` for
    /// a top-level host.
    parent:       Option<HWND>,
    /// Embedded mode only: raw `WM_KEYDOWN`/`WM_SYSKEYDOWN` messages received
    /// this frame, candidates for [`Host::forward_unhandled_keys`] once it is
    /// known whether the page consumed the matching key. Always empty when
    /// [`Host::parent`] is `None`.
    pending_forward: Vec<(u32, WPARAM, LPARAM, u16, Modifiers)>,
    /// A frame is being rendered: a direct render asked for meanwhile (a
    /// wake-up handled inside a modal loop the page opened) is turned into an
    /// invalidation instead of re-entering the paint closure.
    rendering: bool,
    /// The `Form` properties applied to the window ([`HostOptions::form`], [`set_form`]).
    form: FormOptions,
    /// The window's UI Automation adapter (top-level windows only), fed by [`access::publish`].
    access: Option<access::Access>,
    /// Files may be dropped on the window ([`accept_files`]).
    accepting_files: bool,
    /// The window is registered with OLE as a drop target ([`dnd::accept_drops`]): `None` not yet
    /// tried, `Some(false)` refused (another drop target owns the window, OLE unavailable).
    drop_target: Option<bool>,
    /// How the window relates to the thread's message loop.
    role: Role,
    /// Modal ([`HostOptions::modal`]): the windows it disabled, enabled again before it goes away.
    disabled_others: Vec<HWND>,
    /// A paint asked for while a frame was rendering (a modal loop opened from that frame): the
    /// window is invalidated once the frame ends.
    deferred_paint: bool,
    /// The Kubuno band's part under the pointer, and the one held down (`Chrome::Kubuno`).
    chrome_hot: Option<crate::window_chrome::Part>,
    chrome_pressed: Option<crate::window_chrome::Part>,
    /// The ink of the band painted last frame (the page may recolour the band, see
    /// [`set_caption_colors`]; the ground is painted before the page, so with last frame's colours).
    caption_override: Option<(windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F, windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F)>,
    /// The generation of the thread theme ([`set_theme_all`]) this window follows.
    theme_gen: u64,
    /// A floating panel's blurred backdrop ([`FormOptions::panel`]), once built: `None` when the
    /// window is no panel, or when the system gives no backdrop (the panel is then painted opaque).
    panel_backdrop: Option<backdrop::PanelBackdrop>,
    /// A window whose rounded corners the host draws itself (`frame::CornerPath::Host`): fixed at
    /// creation (it decides how the window is created). `None`: DWM's corners.
    rounded: Option<frame::HostFrame>,
    /// The offset `paint_shape` pushed and took back out of the content offset this frame.
    shape_offset: std::cell::Cell<(f32, f32)>,
}

/// A point and a size in a window frame's coordinates, DIP (`Host::frame_point`).
type FramePoint = ((f32, f32), (f32, f32));

/// Posted to the window after a frame asked for a drag ([`dnd::do_drag_drop`]): OLE's drag loop
/// runs from the message loop, never inside a paint.
const WM_KUBUNO_DRAG: u32 = WM_APP + 0x4B57;

/// The drop target's side of the host: each OLE call is recorded in [`dnd`]'s tracker and a frame is
/// rendered at once, so the page answers before OLE is answered.
struct DropSink {
    hwnd: HWND,
}

impl DropSink {
    fn dip(&self, pt: (i32, i32)) -> (f32, f32) {
        let (x, y) = dnd::ole::to_client(self.hwnd, &windows::Win32::Foundation::POINTL { x: pt.0, y: pt.1 });
        // SAFETY: plain query on the host's window.
        let scale = (unsafe { GetDpiForWindow(self.hwnd) } as f32 / 96.0).max(0.01);
        // In the page's coordinates: one shadow margin in for a panel or a host-rounded window.
        let inset = page_inset(self.hwnd);
        ((x - inset) as f32 / scale, (y - inset) as f32 / scale)
    }

    /// Renders a frame now (not inside another paint) and returns the page's answer.
    fn render(&self) -> dnd::DragDropEffects {
        // SAFETY: the host lives in `GWLP_USERDATA` for the window's life; OLE calls the drop target
        // from the message loop (or from the drag loop `WM_KUBUNO_DRAG` starts, which holds no
        // reference to the host), so no other `&mut Host` is in use.
        unsafe {
            let ptr = GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *mut Host;
            if !ptr.is_null() {
                (*ptr).render();
            }
        }
        dnd::with_tracker(|t| t.answered())
    }
}

impl dnd::ole::Sink for DropSink {
    fn enter(&self, data: dnd::DataObject, allowed: dnd::DragDropEffects, pt: (i32, i32), keys: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS) -> dnd::DragDropEffects {
        let (x, y) = self.dip(pt);
        let (mods, buttons) = dnd::ole::modifiers(keys);
        let internal = dnd::is_dragging();
        dnd::with_tracker(|t| t.enter(std::rc::Rc::new(data), allowed, x, y, mods, buttons, internal));
        self.render()
    }

    fn over(&self, pt: (i32, i32), keys: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS) -> dnd::DragDropEffects {
        let (x, y) = self.dip(pt);
        let (mods, buttons) = dnd::ole::modifiers(keys);
        dnd::with_tracker(|t| t.over(x, y, mods, buttons));
        self.render()
    }

    fn leave(&self) {
        dnd::with_tracker(|t| t.leave());
        self.render();
    }

    fn dropped(&self, pt: (i32, i32), keys: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS) -> dnd::DragDropEffects {
        let (x, y) = self.dip(pt);
        let (mods, _) = dnd::ole::modifiers(keys);
        // A page still using `accept_files` gets the files as before.
        let files = dnd::current().map(|f| f.data.files.clone()).unwrap_or_default();
        dnd::with_tracker(|t| t.drop_at(x, y, mods));
        let mut answer = self.render();
        if !files.is_empty() && ACCEPT_FILES_LAST.with(|a| a.get()) && answer.is_none() {
            let files = files.iter().map(|f| f.to_string_lossy().into_owned()).collect();
            input::push(InputEvent::FilesDropped { x: x.round() as i32, y: y.round() as i32, files });
            answer = dnd::DragDropEffects::COPY;
            // SAFETY: as in `render`.
            unsafe {
                let ptr = GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *mut Host;
                if !ptr.is_null() {
                    (*ptr).request_frame();
                }
            }
        }
        answer
    }
}

thread_local! {
    /// Whether the page accepts dropped files (see [`accept_files`]).
    static ACCEPT_FILES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// What the last frame said about dropped files (read by the OLE drop target).
    static ACCEPT_FILES_LAST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether files dragged from the Explorer may be dropped on the window this frame; each drop then
/// arrives as [`InputEvent::FilesDropped`]. Call it every frame (a frame that does not refuses them).
/// A page that routes drags itself uses [`dnd`] instead.
pub fn accept_files(accept: bool) {
    ACCEPT_FILES.with(|a| a.set(accept));
    dnd::accept_drops(accept);
}

/// A top-level popup that paints one floating surface. Owned by [`Host`],
/// reused across frames and resized to each surface.
struct OverlayWindow {
    hwnd:     HWND,
    /// Whether it takes the pointer (`WS_EX_TRANSPARENT` cleared). Fixed at
    /// creation: pass-through and interactive surfaces live in separate pools
    /// (see [`Host::render_overlays`]), so a window never switches role.
    interactive: bool,
    /// Whether the window is currently shown.
    visible:  bool,
    /// Where it was last placed, in screen pixels `(x, y, w, h)`.
    placed:   (i32, i32, i32, i32),
    renderer: Renderer,
    /// Its own themed-part cache: the surface's `Painter` cannot borrow the
    /// host's, whose bitmaps belong to the main window's device.
    parts:    ThemeRenderer,
    /// The swap chain's current size in physical pixels, so it is only resized
    /// when the surface's does.
    size_px:  (u32, u32),
}

/// Opens a window titled `title` at a logical `width` × `height`, and paints it
/// by calling `on_paint` on every frame with a live [`Canvas`] and a [`Frame`].
///
/// Blocks until the window is closed. A demo binary is essentially:
///
/// ```ignore
/// kubuno_controls::host::run("Boutons", 900, 700, Theme::light(), |c, f| {
///     let hot = my_button.hit_test(f.mouse.0, f.mouse.1);
///     my_button.paint(c, bounds);
/// })?;
/// ```
pub fn run<F>(title: &str, width: u32, height: u32, theme: Theme, on_paint: F) -> Result<()>
where
    F: FnMut(&dyn ControlCanvas, &Frame) + 'static,
{
    run_with_chrome(title, width, height, theme, Chrome::System, on_paint)
}

/// Opens a window with a chosen `chrome`. On `Chrome::Kubuno` the host paints
/// its own title bar (see [`caption`]) instead of Windows'. Everything else is
/// identical to [`run`].
pub fn run_with_chrome<F>(
    title: &str,
    width: u32,
    height: u32,
    theme: Theme,
    chrome: Chrome,
    on_paint: F,
) -> Result<()>
where
    F: FnMut(&dyn ControlCanvas, &Frame) + 'static,
{
    let mut opts = HostOptions::new(title, width, height, theme);
    opts.chrome = chrome;
    run_with_options(opts, on_paint)
}

/// Opens the host window described by `opts` and paints it by calling
/// `on_paint` every frame, like [`run`]. Blocks until the window is destroyed
/// (its close is allowed by [`set_close_handler`], or [`quit`] is called).
///
/// Called while another host window of the thread runs (from one of its frames: an event
/// handler), it opens a second window and runs a **nested** message loop until that window is
/// destroyed — how a dialog is shown: [`HostOptions::modal`] disables the thread's other windows
/// meanwhile, [`HostOptions::owner`] owns it. Otherwise its loop is the thread's main loop: when
/// the window is destroyed the loop ends, and the thread's other host windows ([`open_window`])
/// are closed with it (closing a WinForms application's main form ends the application).
pub fn run_with_options<F>(opts: HostOptions, on_paint: F) -> Result<()>
where
    F: FnMut(&dyn ControlCanvas, &Frame) + 'static,
{
    run_boxed(opts, Box::new(on_paint))
}

/// [`run_with_options`] for a paint closure that borrows from the caller — a modal dialog painting
/// a view its caller still owns afterwards (`ShowDialog`). The window is destroyed, and the closure
/// dropped, before this returns.
pub fn run_scoped<'a, F>(opts: HostOptions, on_paint: F) -> Result<()>
where
    F: FnMut(&dyn ControlCanvas, &Frame) + 'a,
{
    let boxed: ScopedPaintFn<'a> = Box::new(on_paint);
    // SAFETY: only the window's procedure calls the closure, through the `Host` that `run_boxed`
    // keeps on its own stack; `run_boxed` destroys the window (after which no message reaches the
    // host) and drops the host before it returns, so the closure is never called — nor dropped —
    // after `'a`. The two box types differ only by that lifetime.
    let boxed: PaintFn = unsafe { std::mem::transmute::<ScopedPaintFn<'a>, PaintFn>(boxed) };
    run_boxed(opts, boxed)
}

/// Opens a host window WITHOUT a loop of its own — a WinForms `Form.Show()`: the thread's running
/// loop ([`run_with_options`], or the nested loop of a dialog) dispatches its messages, and the
/// window lives until it is closed, or until the main window's loop ends. Call it on the UI thread,
/// typically from a frame of another window. Returns the new window.
pub fn open_window<F>(opts: HostOptions, on_paint: F) -> Result<HWND>
where
    F: FnMut(&dyn ControlCanvas, &Frame) + 'static,
{
    let created = create(opts, Box::new(on_paint), Role::Modeless)?;
    let hwnd = created.hwnd;
    // Owned by the window from here on: freed at its `WM_NCDESTROY`.
    let _ = Box::into_raw(created.host);
    drop(created.guard);
    Ok(hwnd)
}

/// A window [`create`] opened: its host (whose address the window keeps), its handle, the guard
/// that keeps its per-window state installed, and the embedded mode's parent watch timer.
struct Created {
    host: Box<Host>,
    hwnd: HWND,
    guard: window_tls::Guard,
    watch: usize,
}

fn run_boxed(opts: HostOptions, on_paint: PaintFn) -> Result<()> {
    let role = if window_tls::in_window() { Role::Nested } else { Role::Main };
    let Created { mut host, hwnd, guard, watch } = create(opts, on_paint, role)?;
    // SAFETY: the standard message loop of this thread; the host outlives its window (destroyed
    // below before the host is dropped).
    unsafe {
        let mut msg = MSG::default();
        if role == Role::Main {
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if run_filters(&msg) {
                    continue;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            // The main window is gone: the thread's other windows (`open_window`) go with it.
            for other in input::thread_windows() {
                if other != hwnd {
                    let _ = DestroyWindow(other);
                }
            }
        } else {
            while IsWindow(Some(hwnd)).as_bool() {
                if GetMessageW(&mut msg, None, 0, 0).0 == 0 {
                    // `WM_QUIT` belongs to the loop underneath: leave it for that loop.
                    PostQuitMessage(msg.wParam.0 as i32);
                    break;
                }
                if run_filters(&msg) {
                    continue;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if watch != 0 {
            let _ = KillTimer(None, watch);
            EMBED_WATCH.with(|w| w.set(None));
        }
        host.enable_others();
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = DestroyWindow(hwnd);
        }
    }
    drop(guard);
    drop(host);
    if role == Role::Main {
        // The images decoded for the windows of this thread go with its last window (see
        // `styled::release_images`).
        crate::styled::release_images();
        crate::icon_image::release();
    }
    Ok(())
}

fn create(opts: HostOptions, on_paint: PaintFn, role: Role) -> Result<Created> {
    let HostOptions {
        title,
        width,
        height,
        theme,
        chrome,
        font_override,
        backdrop,
        start_hidden,
        fit_work_area,
        client_size,
        parent,
        diagnostics: with_diagnostics,
        form,
        owner,
        modal,
    } = opts;
    let title = form.title.clone().unwrap_or(title);
    let first_window = !window_tls::in_window() && role != Role::Modeless;
    if with_diagnostics {
        // No console in a Kubuno GUI app: its logs, println!s and panics go to the debugger or a
        // log file, and a panic shows a dialog (not over the embedding process's windows).
        diagnostics::install(&diagnostics::exe_name(), parent.is_none());
    }
    if parent.is_none() && first_window {
        // The crash window names the application by its (main) window title ("EvtApp").
        diagnostics::set_display_name(&title);
    }
    // The debugger's Threads window and Parallel Stacks show "Kubuno UI thread" rather than "Main Thread".
    diagnostics::name_ui_thread();
    let parent = parent.map(|p| HWND(p as *mut core::ffi::c_void));
    let owner = owner.filter(|_| parent.is_none()).map(|o| HWND(o as *mut core::ffi::c_void));
    // A child has no caption of its own: the non-client chrome paths must
    // stay out of its way.
    let chrome = if parent.is_some() { Chrome::System } else { chrome };
    let title = title.as_str();
    // The new window's own thread-local state: the detached one (what the application registered
    // before opening its first window) for the first window, a fresh one for the others. In
    // both, a previous run's `quit` must not make this window's first close skip its handler.
    let buttons = form::caption_buttons(&form);
    let font = font_override.clone();
    window_tls::prepare_next(first_window, move |mut state| {
        state.current_font = font;
        state.quit_requested = false;
        state.with_caption_buttons(buttons)
    });
    // SAFETY: window creation and set-up on this thread; the host box outlives the window (see the
    // callers), and its address is handed to the window procedure through `lpCreateParams`.
    unsafe {
        // Per-monitor v2, exactly as the shell's `main` sets it: the controls
        // paint at the real scale of whatever display they sit on. Harmless if
        // the host binary already set it — the second call just fails and is
        // ignored.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        // The renderer builds its bitmaps through WIC, which is COM: without an
        // apartment on THIS thread, `Renderer::new` fails and the window opens
        // permanently blank. The host owns this rather than every caller —
        // forgetting it is invisible until nothing paints. `RPC_E_CHANGED_MODE`
        // means the caller already initialised COM its own way, which is fine.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let registered = (|| -> Result<windows::core::PCWSTR> {
            let instance = GetModuleHandleW(None)?;
            let class = w!("KubunoControlsHost");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                // An app that embeds an `app_icon` resource (its build script does)
                // shows it in the task bar and Alt+Tab; others get the default.
                hIcon: LoadIconW(Some(instance.into()), w!("app_icon")).unwrap_or_default(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: class,
                ..Default::default()
            };
            // A class is registered once per process; a second host window reuses
            // it, so an "already registered" failure is not fatal.
            RegisterClassExW(&wc);
            Ok(class)
        })();
        let class = match registered {
            Ok(class) => class,
            Err(e) => {
                window_tls::discard_next();
                return Err(e);
            }
        };
        let instance = GetModuleHandleW(None).ok();

        let accent = theme.accent;
        let mut host = Box::new(Host {
            hwnd: HWND::default(),
            renderer: None,
            theme,
            visuals: None,
            parts: ThemeRenderer::new(),
            dpi: 96.0,
            mouse: (0.0, 0.0),
            mouse_down: false,
            right_down: false,
            middle_down: false,
            left_press_latch: false,
            right_press_latch: false,
            middle_press_latch: false,
            dismiss_pending: false,
            want: (width.max(1), height.max(1)),
            on_paint,
            popups: Vec::new(),
            title: title.to_string(),
            chrome: Chrome::System,
            caption_hot: None,
            wheel: (0.0, 0.0),
            click_count: 0,
            last_press: (0, 0, 0),
            high_surrogate: None,
            tracking_leave: false,
            window_focused: false,
            cursor: Cursor::Arrow,
            caption_band: accent,
            dwm_band: accent_bgr(accent),
            font_override,
            backdrop,
            fit_work_area,
            client_size,
            parent,
            pending_forward: Vec::new(),
            rendering: false,
            drop_target: None,
            form: form.clone(),
            access: None,
            accepting_files: false,
            role,
            disabled_others: Vec::new(),
            deferred_paint: false,
            chrome_hot: None,
            chrome_pressed: None,
            caption_override: None,
            theme_gen: thread_theme().0,
            panel_backdrop: None,
            rounded: None,
            shape_offset: std::cell::Cell::new((0.0, 0.0)),
        });
        host.chrome = chrome;
        // Who rounds the corners (`frame`): a window the host rounds itself is created without a
        // redirection surface, like a floating panel.
        if let frame::CornerPath::Host(radius) = corner_path(&form, chrome, backdrop, parent.is_some()) {
            host.rounded = Some(frame::HostFrame { radius, ..Default::default() });
        }

        let created = match parent {
            None => CreateWindowExW(
                // A floating panel's pixels come from its composition tree only (`backdrop`), a
                // host-rounded window's from its swap chain only (`frame`).
                if form.panel.is_some() || host.rounded.is_some() { WS_EX_NOREDIRECTIONBITMAP } else { WINDOW_EX_STYLE::default() },
                class,
                &HSTRING::from(title),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                width.max(1) as i32,
                height.max(1) as i32,
                // An owned top-level window (a dialog): above its owner, no task bar button.
                owner,
                None,
                instance.map(Into::into),
                Some(host.as_mut() as *mut Host as *const _),
            ),
            Some(parent) => {
                // Sized to the parent's client area from the start; the parent
                // resizes it from then on. Creating a child of another
                // process's window is legal and attaches the two threads'
                // input queues (what makes focus and capture work across the
                // boundary).
                let mut rc = RECT::default();
                let _ = GetClientRect(parent, &mut rc);
                CreateWindowExW(
                    Default::default(),
                    class,
                    &HSTRING::from(title),
                    WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN | WS_VISIBLE,
                    0,
                    0,
                    (rc.right - rc.left).max(1),
                    (rc.bottom - rc.top).max(1),
                    Some(parent),
                    None,
                    instance.map(Into::into),
                    Some(host.as_mut() as *mut Host as *const _),
                )
            }
        };
        let hwnd = match created {
            Ok(hwnd) => hwnd,
            Err(e) => {
                window_tls::discard_next();
                return Err(e);
            }
        };
        debug_assert!(hwnd == host.hwnd);
        // The rest of the set-up (and, for the callers that run one, the loop) sees this window's
        // own thread-local state.
        let guard = window_tls::enter(hwnd);

        let watch = if let Some(parent) = parent {
            // A parent PROCESS that dies takes this child window with it
            // without a single message reaching this thread (no WM_DESTROY,
            // so no WM_QUIT): the loop below would wait forever. A timer bound
            // to the window would die with it too, so this one is a THREAD
            // timer, whose callback ends the loop once either window is gone.
            EMBED_WATCH.with(|w| w.set(Some((parent, hwnd))));
            SetTimer(None, 0, PARENT_WATCH_MS, Some(parent_watch))
        } else {
            host.apply_dwm();
            // The `Form` properties, before the window shows: styles, icon, top-most, opacity,
            // then where it opens and in which state.
            form::apply(hwnd, &form, None, WS_OVERLAPPEDWINDOW, host.base_ex_style());
            if host.rounded.is_some() {
                // The host draws the whole frame: DWM's square shadow and border would show
                // behind the curve.
                frame::set_dwm_frame(hwnd, false);
                host.update_rounded();
            }
            let mut rc = RECT::default();
            if GetWindowRect(hwnd, &mut rc).is_ok() {
                let size = (rc.right - rc.left, rc.bottom - rc.top);
                let origin = match owner {
                    // `CenterParent` with an owner: centred on it (WinForms' dialogs).
                    Some(owner) if form.start_position == StartPosition::CenterParent => {
                        let mut o = RECT::default();
                        GetWindowRect(owner, &mut o)
                            .ok()
                            .map(|()| (o.left + ((o.right - o.left) - size.0) / 2, o.top + ((o.bottom - o.top) - size.1) / 2))
                    }
                    _ => form::start_origin(&form, size, form::work_area_px(hwnd), host.scale()),
                };
                if let Some((x, y)) = origin {
                    let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                }
            }
            // An owned window stays above its owner — which a non-topmost window cannot do over a
            // topmost one (`TopMost`): it takes its owner's topmost state.
            if let Some(owner) = owner {
                let topmost = (GetWindowLongPtrW(owner, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0) != 0;
                if topmost && !form.top_most {
                    let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                }
            }
            if modal {
                // Every other window of the thread stops taking input while this one is open.
                for other in input::thread_windows() {
                    if other != hwnd && IsWindowEnabled(other).as_bool() {
                        let _ = EnableWindow(other, false);
                        host.disabled_others.push(other);
                    }
                }
            }
            if !start_hidden {
                let _ = ShowWindow(hwnd, form::show_command(form.window_state));
            } else {
                // A window started hidden gets no `WM_PAINT`, yet its page must start like a
                // shown one (Windows Forms raises `Load` when the form is created, whatever its
                // visibility: a tray application adds its icon there). One frame is asked for as
                // soon as the loop runs; a hidden window renders it off screen
                // (`request_frame`), at the size it was created with.
                let _ = PostMessageW(Some(hwnd), WM_KUBUNO_WAKE, WPARAM(0), LPARAM(0));
            }
            0
        };

        // The caller's set-up hook, taken out of its cell before it runs so
        // it may call any host API.
        if let Some(ready) = ON_READY.with(|r| r.borrow_mut().take()) {
            ready(hwnd);
        }
        Ok(Created { host, hwnd, guard, watch })
    }
}

thread_local! {
    /// Embedded mode: `(parent, own window)` watched by [`parent_watch`].
    static EMBED_WATCH: std::cell::Cell<Option<(HWND, HWND)>> = const { std::cell::Cell::new(None) };
    /// Embedded mode: the parent HWND ([`HostOptions::parent`]), so
    /// [`notify_tab_out`] — called from the page, which has no `&Host` — can
    /// reach it. `0` for a top-level host.
    static PARENT_HWND: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

/// Windows message posted to the parent HWND ([`HostOptions::parent`]) when
/// this window's own [`kubuno_ui::focus::FocusRing`]-style Tab/Shift+Tab
/// handling ran past its last or first focusable control — the "tabOut" half
/// of the keyboard protocol (`vskubuno/docs/DESIGNER.md` §7). `wParam` is `1`
/// for a Shift+Tab that ran off the FIRST control (backward), `0` for a Tab
/// that ran off the LAST one (forward); `lParam` is unused. Only ever posted
/// by [`notify_tab_out`], which only ever runs in embedded mode.
///
/// `WM_APP`-based so it cannot collide with a standard message; the
/// embedding host (vskubuno's `RustDesignSurfaceHost`) handles it in the
/// container window's own `WndProc`, exactly where it already handles
/// `WM_SIZE`, and moves the WPF focus out (`MoveFocus`).
pub const WM_KUBUNO_TAB_OUT: u32 = WM_APP + 0x4B4F;

/// Reports that this window's own Tab-order handling ran past its last
/// (`backward == false`) or first (`backward == true`) focusable control, and
/// the embedding parent should take the keyboard focus back instead. Posts
/// [`WM_KUBUNO_TAB_OUT`] to [`HostOptions::parent`]; a no-op for a top-level
/// host (`parent` is `None`).
///
/// A page owns the wrap detection itself (this crate has no page-level focus
/// manager to hook into): typically, before letting its own focus ring
/// consume this frame's `Tab`/`Shift+Tab`, it checks whether the currently
/// focused control is its last/first tabbable one and, if so, consumes the
/// key itself, blurs, and calls this instead of stepping the ring further.
pub fn notify_tab_out(backward: bool) {
    let p = PARENT_HWND.with(|c| c.get());
    if p == 0 {
        return;
    }
    // SAFETY: posting to a window handle is sound even if the window has
    // since been destroyed (the call then simply fails).
    unsafe {
        let _ = PostMessageW(Some(HWND(p as *mut _)), WM_KUBUNO_TAB_OUT, WPARAM(backward as usize), LPARAM(0));
    }
}

/// Windows message [`Host::forward_unhandled_keys`] posts to the parent HWND
/// immediately BEFORE the raw `WM_KEYDOWN`/`WM_SYSKEYDOWN` it forwards,
/// carrying the modifier keys held at the ORIGINAL moment the key went down
/// (`wParam` bit 0 = Ctrl, bit 1 = Shift, bit 2 = Alt; `lParam` unused). A
/// plain Win32 keyboard message carries no modifier state at all — modifiers
/// are a separate, per-thread `GetKeyState` table — so without this, whoever
/// re-dispatches the forwarded `WM_KEYDOWN` (`vskubuno`'s
/// `RustDesignSurfaceHost`) can only read whatever ITS thread's key-state
/// table says *at the time it finally processes the posted message*, which —
/// after the round trip through this frame, `PostMessage`'s queuing, and the
/// parent's own message pump — may already be stale (a fast Ctrl+S can
/// release Ctrl before the parent ever gets to it). `PostMessage` to the same
/// destination window from the same source thread is FIFO, so the parent is
/// guaranteed to see this message immediately before the key message it
/// describes.
pub const WM_KUBUNO_KEY_MODS: u32 = WM_APP + 0x4B50;

/// The embedded host's thread-timer callback: once the parent is gone, the
/// child window is destroyed (normal path, `WM_DESTROY` posts the quit) or,
/// if the system already destroyed it along with a dead parent process, the
/// loop is ended directly.
unsafe extern "system" fn parent_watch(_: HWND, _: u32, _: usize, _: u32) {
    let Some((parent, own)) = EMBED_WATCH.with(|w| w.get()) else { return };
    // SAFETY: `IsWindow` accepts any handle value, stale or foreign.
    unsafe {
        if IsWindow(Some(parent)).as_bool() {
            return;
        }
        EMBED_WATCH.with(|w| w.set(None));
        if IsWindow(Some(own)).as_bool() {
            let _ = DestroyWindow(own);
        } else {
            PostQuitMessage(0);
        }
    }
}



/// DWM expects a caption colour packed as `0x00BBGGRR`. The theme carries linear
/// float components (0..=1) that Direct2D uses; DWM wants sRGB bytes in BGR
/// order, so translate on the way out.
fn accent_bgr(c: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F) -> u32 {
    let b = (c.b.clamp(0.0, 1.0) * 255.0).round() as u32;
    let g = (c.g.clamp(0.0, 1.0) * 255.0).round() as u32;
    let r = (c.r.clamp(0.0, 1.0) * 255.0).round() as u32;
    (b << 16) | (g << 8) | r
}

/// Who rounds the corners of a window with `form` under `chrome` (`frame::plan`): the radius it
/// asks for (`KUBUNO_CORNER_RADIUS` overriding it, for testing), and whether the host can draw its
/// frame — not under Windows' own title bar, nor over a system material (only DWM clips those to a
/// curve), nor for a translucent (layered) window, a floating panel (rounded by its own
/// composition clip) or an embedded one.
fn corner_path(form: &FormOptions, chrome: Chrome, backdrop: Backdrop, embedded: bool) -> frame::CornerPath {
    let radius = frame::radius_override().filter(|_| form.panel.is_none()).unwrap_or_else(|| form.corner_radius());
    // A material only where the system draws one (`DWMWA_SYSTEMBACKDROP_TYPE`, Windows 11 22H2):
    // elsewhere `Backdrop` is a no-op, and the host may round the window itself.
    let material = form.backdrop.unwrap_or(backdrop).dwm_value().is_some() && frame::system_backdrops();
    let host_can_draw = !embedded
        && form.panel.is_none()
        && chrome != Chrome::System
        && !material
        && form.opacity >= 0.999
        && form.transparency_key.is_none();
    frame::plan(radius, frame::CornerContext { dwm_rounds: frame::dwm_rounds(), host_can_draw, force_host: frame::force_host() })
}

/// The window property holding how far a window's page is inset from its client area, in
/// physical pixels (the shadow margin of a floating panel or a host-rounded window), for
/// [`screen_geometry`], which may be asked about any window.
const PAGE_INSET_PROP: windows::core::PCWSTR = w!("KubunoPageInset");

fn set_page_inset(hwnd: HWND, px: i32) {
    use windows::Win32::UI::WindowsAndMessaging::{RemovePropW, SetPropW};
    // SAFETY: a window property on the host's own window: an integer stored in the handle slot.
    unsafe {
        if px > 0 {
            let _ = SetPropW(hwnd, PAGE_INSET_PROP, Some(windows::Win32::Foundation::HANDLE(px as isize as *mut _)));
        } else {
            let _ = RemovePropW(hwnd, PAGE_INSET_PROP);
        }
    }
}

fn page_inset(hwnd: HWND) -> i32 {
    // SAFETY: reads an integer property of a live window (0 when absent).
    unsafe { GetPropW(hwnd, PAGE_INSET_PROP).0 as isize as i32 }
}

/// Applies the theme-dependent DWM attributes of the host window.
///
/// * System and Kubuno chrome: the caption, its text and the border are
///   tinted with the accent (minimal-integration Kubuno look: the window reads
///   as Kubuno at a glance even with the system caption). Custom chrome keeps
///   DWM's own border — the page owns the whole top of the window.
/// * Rounded corners, Windows 11's normal look, made explicit.
/// * Immersive dark mode from the theme's mode (the frame and the system
///   menus follow it).
/// * The backdrop material, when one is asked for.
///
/// All of these are Windows 11+ attributes; on Windows 10 each call is a
/// no-op and the window keeps the default chrome.
fn apply_dwm_theme(hwnd: HWND, theme: &Theme, chrome: Chrome, backdrop: Backdrop, form: &FormOptions, corner: i32) {
    use windows::Win32::Graphics::Dwm::{DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE};
    let set = |attr, value: &i32| {
        // SAFETY: `value` is a live 4-byte integer for the duration of the
        // call, and every attribute set here is documented as a DWORD/BOOL/
        // COLORREF; `hwnd` is the host's own window.
        unsafe {
            let _ = DwmSetWindowAttribute(hwnd, attr, value as *const i32 as *const _, 4);
        }
    };
    if chrome != Chrome::Custom {
        let style = form.effective_chrome();
        let cap = accent_bgr(style.band_color(theme)) as i32;
        let ink = accent_bgr(style.ink_color(theme)) as i32;
        set(DWMWA_CAPTION_COLOR, &cap);
        set(DWMWA_TEXT_COLOR, &ink);
        // The band's colour unless asked otherwise: no visible border seam.
        let border = form.border_color.map_or(cap, |c| accent_bgr(c) as i32);
        set(DWMWA_BORDER_COLOR, &border);
    }
    // The corners `frame::plan` gave the window (`Host::corner_dwm`).
    set(DWMWA_WINDOW_CORNER_PREFERENCE, &corner);
    let dark = i32::from(theme.mode == drive_app_controls::ThemeMode::Dark);
    set(DWMWA_USE_IMMERSIVE_DARK_MODE, &dark);
    // A floating panel's material is its own composition backdrop, not DWM's.
    if let Some(material) = form.backdrop.unwrap_or(backdrop).dwm_value().filter(|_| form.panel.is_none()) {
        set(DWMWA_SYSTEMBACKDROP_TYPE, &material);
    }
}

/// What a mouse button's raw level (`down`) plus its press latch (`*latch`)
/// report for the frame about to be painted, consuming the latch.
///
/// `WM_PAINT` is only synthesized once the message queue is otherwise empty,
/// so a fast press+release — a touchpad tap, a synthetic click — can post
/// BOTH its `WM_*BUTTONDOWN` and `WM_*BUTTONUP` before the paint that would
/// have shown the button down ever runs. Reading only the level at render
/// time then sees the button already up and the press is lost: no widget
/// ever observes a `down && !prev_down` edge. `*latch` is set by the DOWN
/// handler (never touched by the UP handler — that message is exactly the
/// one that can race ahead of the paint) and is consumed here, so the frame
/// immediately following a press always reports the button down at least
/// once, whether or not it has already been released again by then; the
/// frame after THAT one reports the button's real (by-then-released) level,
/// which is the release edge a widget's own `!down && prev_down` check wants.
///
/// Free of `Host`/Win32 so it is unit-testable on its own (see `tests`).
fn consume_button_edge(down: bool, latch: &mut bool) -> bool {
    let reported = down || *latch;
    *latch = false;
    reported
}

impl Host {
    /// Applies what [`set_theme`] / [`set_font_override`] asked for since the
    /// last frame. Called at the very start of a frame, before anything is
    /// borrowed for painting.
    fn apply_pending(&mut self) {
        let (generation, shared) = thread_theme();
        let shared = if generation != self.theme_gen {
            self.theme_gen = generation;
            shared
        } else {
            None
        };
        if let Some(theme) = PENDING_THEME.with(|t| t.borrow_mut().take()).or(shared) {
            self.theme = theme;
            self.apply_dwm();
            // What DWM now shows; the Kubuno caption re-tints it per frame.
            self.caption_band = self.theme.accent;
            self.dwm_band = accent_bgr(self.theme.accent);
        }
        if let Some(font) = PENDING_FONT.with(|f| f.borrow_mut().take()) {
            CURRENT_FONT.with(|f| *f.borrow_mut() = font.clone());
            self.font_override = font;
            // Same as a DPI change: the popups' renderers carry the old text
            // formats, so they are rebuilt on demand.
            self.drop_popups();
            self.rebuild_renderer();
        }
    }

    /// Records the hovered caption button, mirrored for [`caption_hot`].
    /// Returns whether it changed.
    fn set_caption_hot(&mut self, hot: Option<CaptionHot>) -> bool {
        CAPTION_HOT.with(|c| c.set(hot));
        if hot == self.caption_hot {
            return false;
        }
        self.caption_hot = hot;
        true
    }

    /// The window's DWM attributes (caption and border tint, corners, dark mode, backdrop).
    fn apply_dwm(&self) {
        if self.parent.is_none() {
            apply_dwm_theme(self.hwnd, &self.theme, self.chrome, self.backdrop, &self.form, self.corner_dwm());
        }
    }

    /// The `DWM_WINDOW_CORNER_PREFERENCE` of the window: square when the host draws its corners (a
    /// floating panel, a host-rounded window: DWM's curve would cut their shadow margin), else
    /// what `frame::plan` gives its radius now — the nearest preset for a radius only the host
    /// could draw, as a window created on DWM's path cannot change path.
    fn corner_dwm(&self) -> i32 {
        if self.form.panel.is_some() || self.rounded.is_some() {
            return frame::DWMWCP_DONOTROUND;
        }
        match corner_path(&self.form, self.chrome, self.backdrop, self.parent.is_some()) {
            frame::CornerPath::Dwm(value) => value,
            frame::CornerPath::Host(radius) => frame::nearest_preset(radius),
        }
    }

    /// The extended styles the host gives the window beyond its `Form`'s (`form::apply`).
    fn base_ex_style(&self) -> WINDOW_EX_STYLE {
        if self.rounded.is_some() { WS_EX_NOREDIRECTIONBITMAP } else { WINDOW_EX_STYLE::default() }
    }

    /// A host-rounded window (`frame`): follows Windows squaring it (maximised, snapped, full
    /// screen) and keeps its hit region and its page inset in step with its size. Called at
    /// creation, on every size change and before every frame (Windows may flag a window snapped
    /// after its last size change).
    fn update_rounded(&mut self) {
        let Some(mut f) = self.rounded else { return };
        let squared = frame::squared(self.hwnd);
        let changed = squared != f.squared;
        f.squared = squared;
        let scale = self.scale();
        let mut rc = RECT::default();
        // SAFETY: plain query on the host's own window.
        let _ = unsafe { GetWindowRect(self.hwnd, &mut rc) };
        let px = |v: f32| (v * scale).round() as i32;
        // The region covers the margin too: it cuts what the window draws, and the shadow is there.
        let region = (rc.right - rc.left, rc.bottom - rc.top, px(f.margin()), px(f.shown_radius()), px(f.margin()));
        if f.region != Some(region) {
            f.region = Some(region);
            frame::set_hit_region(self.hwnd, region.0, region.1, region.2, region.3, region.4);
        }
        self.rounded = Some(f);
        set_page_inset(self.hwnd, px(f.margin()));
        if changed {
            self.invalidate();
        }
    }

    /// The window's shape, when the host draws it (a floating panel, a host-rounded window): its
    /// shadow in the `margin` and its ground (`ground`), then the frame's coordinates (`size` DIP,
    /// one margin in) and its rounded clip for everything painted after, until
    /// [`Host::end_shape`]. False (nothing pushed) for any other window.
    fn paint_shape(&self, painter: &Painter, size: (f32, f32), margin: f32, ground: &windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F) -> bool {
        use drive_app_controls::themes::shape;
        use drive_app_controls::Canvas as _;
        if self.parent.is_some() {
            return false;
        }
        let rect = Rect::new(margin, margin, margin + size.0, margin + size.1);
        let radius = if let Some(p) = self.form.panel {
            if margin <= 0.0 {
                return false;
            }
            // A floating panel: its shadow, its ground (the blur, or the window colour without one).
            painter.draw_shadow(&rect, p.radius, &shape::SHADOW_WAFFLE, shape::SHADOW_BLACK);
            if self.panel_backdrop.is_some() {
                // The shadow is stacked filled rounded rects: it darkened the panel's own area too,
                // which nothing paints over where the page is translucent.
                painter.erase_rounded(&rect, p.radius);
            } else {
                painter.fill_rounded(&rect, p.radius, &self.theme.window_background);
            }
            p.radius
        } else if let Some(f) = self.rounded {
            // A host-rounded window (`frame`): its shadow, then its opaque ground.
            let radius = f.shown_radius();
            if margin > 0.0 {
                painter.draw_shadow(&rect, radius, &frame::FRAME_SHADOW, shape::SHADOW_BLACK);
            }
            painter.fill_rounded(&rect, radius, ground);
            radius
        } else {
            return false;
        };
        // The frame's coordinates are the page's: what the page calls client coordinates (its
        // pointer, `content_offset`, the rectangles it hit-tests and opens popups at) start at the
        // frame, so the offset drawn here is taken back out of the content offset (it is put back
        // in `end_shape`, before the painter pops it). The host adds the margin back where it
        // meets Windows: popups, drops, accessibility bounds, `screen_geometry`.
        let before = content_offset();
        painter.push_offset(margin, margin);
        let after = content_offset();
        shift_content_offset(before.0 - after.0, before.1 - after.1);
        painter.push_clip_rounded(&Rect::new(0.0, 0.0, size.0, size.1), radius);
        self.shape_offset.set((after.0 - before.0, after.1 - before.1));
        true
    }

    /// Ends what [`Host::paint_shape`] began, then draws a host-rounded window's 1 px border along
    /// its curve — DWM's border, in the same colour: the form's `BorderColor`, else the band's (no
    /// seam), else the theme's floating-surface border.
    fn end_shape(&self, painter: &Painter, size: (f32, f32)) {
        use drive_app_controls::Canvas as _;
        painter.pop_clip_rounded();
        let (dx, dy) = self.shape_offset.take();
        shift_content_offset(dx, dy);
        painter.pop_offset();
        let Some(f) = self.rounded.filter(|f| f.margin() > 0.0) else { return };
        let m = f.margin();
        let color = self.form.border_color.unwrap_or_else(|| if self.has_kubuno_band() { self.caption_band } else { self.theme.flyout_border });
        painter.stroke_rounded(&Rect::new(m, m, m + size.0, m + size.1), f.shown_radius(), &color);
    }

    /// Whether the Kubuno band shows (Kubuno chrome, a captioned border style).
    fn has_kubuno_band(&self) -> bool {
        self.chrome == Chrome::Kubuno && self.form.border_style.has_caption()
    }

    /// The page area the host reserves at the top: the band, unless the page extends under it.
    fn chrome_top(&self) -> f32 {
        if !self.has_kubuno_band() {
            return 0.0;
        }
        let style = self.form.effective_chrome();
        if style.extend_content { 0.0 } else { style.band_height() }
    }

    /// The window's icon as the painters draw it: a glyph name, or an image file (SVG, PNG, ICO…)
    /// that `crate::icon_image` renders at the size of the title bar's icon.
    fn icon_glyph(&self) -> Option<&'static str> {
        drive_app_controls::icon_name(self.form.icon.as_deref()?)
    }

    /// The band this frame for a client `size` (DIP), with the regions the page declared.
    fn kubuno_layout(&self, size: (f32, f32)) -> Option<crate::window_chrome::ChromeLayout> {
        if !self.has_kubuno_band() {
            return None;
        }
        let ctx = self.chrome_context(size);
        Some(crate::window_chrome::layout(&ctx.style, ctx.bounds, ctx.has_icon, ctx.buttons, chrome::declared_slots()))
    }

    fn chrome_context(&self, size: (f32, f32)) -> chrome::ChromeContext {
        let style = self.form.effective_chrome();
        chrome::ChromeContext {
            style,
            bounds: Rect::new(0.0, 0.0, size.0, size.1),
            has_icon: self.form.icon.is_some() && self.form.chrome.show_icon,
            buttons: self.form.system_buttons(),
        }
    }

    /// The grip's rectangle when it shows (a resizable window that is not maximised), inside the
    /// window's rounded corner.
    fn grip_rect(&self, size: (f32, f32)) -> Option<Rect> {
        // SAFETY: plain window-state query.
        let zoomed = unsafe { IsZoomed(self.hwnd).as_bool() };
        (self.chrome == Chrome::Kubuno && self.form.shows_grip() && !zoomed)
            .then(|| crate::window_chrome::grip_rect(self.grip_bounds(size)))
    }

    /// The bounds the grip is placed in for a frame of `size` DIP: in from its corner's curve.
    fn grip_bounds(&self, size: (f32, f32)) -> Rect {
        frame::grip_bounds(Rect::new(0.0, 0.0, size.0, size.1), self.shown_radius())
    }

    /// The radius the window's corners show now, in DIP: the host's own curve, a floating
    /// panel's, or DWM's preset (none before Windows 11).
    fn shown_radius(&self) -> f32 {
        if let Some(f) = self.rounded {
            return f.shown_radius();
        }
        if let Some(p) = self.form.panel {
            return p.radius;
        }
        if !frame::dwm_rounds() {
            return 0.0;
        }
        match self.corner_dwm() {
            frame::DWMWCP_ROUND => form::DEFAULT_CORNER_RADIUS,
            frame::DWMWCP_ROUNDSMALL => form::SMALL_CORNER_RADIUS,
            _ => 0.0,
        }
    }

    /// The band's part a non-client message's hit code and screen point (`lparam`) stand for.
    fn nc_part(&self, ht: u32, lparam: LPARAM) -> Option<crate::window_chrome::Part> {
        let mut pt = POINT { x: (lparam.0 & 0xFFFF) as i16 as i32, y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32 };
        // SAFETY: plain coordinate conversion on the host's own window.
        let _ = unsafe { ScreenToClient(self.hwnd, &mut pt) };
        let scale = self.scale();
        let (cw, ch) = client_px(self.hwnd);
        // A host-rounded window's band is one shadow margin in.
        let m = self.panel_margin();
        let size = (cw as f32 / scale - 2.0 * m, ch as f32 / scale - 2.0 * m);
        let layout = self.kubuno_layout(size)?;
        chrome::part_of_ht(ht, &layout, pt.x as f32 / scale - m, pt.y as f32 / scale - m)
    }

    /// Records the band's part under the pointer; whether it changed.
    fn set_chrome_hot(&mut self, hot: Option<crate::window_chrome::Part>) -> bool {
        if hot == self.chrome_hot {
            return false;
        }
        self.chrome_hot = hot;
        true
    }

    fn init(&mut self, hwnd: HWND) {
        self.hwnd = hwnd;
        input::set_main_hwnd(hwnd);
        self.dpi = unsafe { GetDpiForWindow(hwnd) } as f32;
        // Embedded, the parent owns the geometry: no logical-size pass.
        if let Some(parent) = self.parent {
            PARENT_HWND.with(|c| c.set(parent.0 as isize));
        } else {
            // `StartPosition = WindowsDefaultBounds`: Windows chose the size too.
            if self.form.start_position != StartPosition::WindowsDefaultBounds {
                self.size_to_dpi();
            }
        }
        self.rebuild_renderer();
    }

    /// The window is created in physical pixels before its DPI is known, so on
    /// a scaled display the caller's logical size would come out tiny. Resize
    /// to `want` × scale once the DPI is in hand — the same fix the shell
    /// applies in `size_to_dpi`.
    fn size_to_dpi(&self) {
        let scale = self.scale();
        // A floating panel's window holds the panel and its shadow margin on every side.
        let margin = self.panel_margin() * 2.0;
        let (mut w, mut h) = (((self.want.0 as f32 + margin) * scale) as i32, ((self.want.1 as f32 + margin) * scale) as i32);
        if self.client_size {
            let (dw, dh) = self.non_client_px();
            w += dw;
            h += dh;
        }
        if self.fit_work_area {
            // The work area in physical pixels (`work_area_dip` at scale 1).
            let (l, t, r, b) = work_area_dip(self.hwnd, 1.0, (w as f32, h as f32));
            w = w.min((r - l) as i32);
            h = h.min((b - t) as i32);
        }
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                w.max(1),
                h.max(1),
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    /// What the window adds around its page area at the current DPI, physical px: the system
    /// frame and caption under `Chrome::System`; under `Kubuno`/`Custom` the resize frame
    /// `WM_NCCALCSIZE` keeps (left, right, bottom) plus, for `Kubuno`, the host's caption band.
    fn non_client_px(&self) -> (i32, i32) {
        let dpi = self.dpi as u32;
        match self.chrome {
            Chrome::System => {
                let mut rc = RECT::default();
                // SAFETY: plain Win32 call on a stack RECT.
                let ok = unsafe { AdjustWindowRectExForDpi(&mut rc, WS_OVERLAPPEDWINDOW, false, WINDOW_EX_STYLE::default(), dpi) };
                if ok.is_ok() { (rc.right - rc.left, rc.bottom - rc.top) } else { (0, 0) }
            }
            // `FormBorderStyle = None`: no frame, no caption.
            Chrome::Kubuno if !self.form.border_style.has_caption() => (0, 0),
            // A host-rounded window keeps no frame of Windows' (its resize band is in its shadow
            // margin, counted by the caller): only the band.
            Chrome::Kubuno | Chrome::Custom if self.rounded.is_some() => (0, (self.chrome_top() * self.scale()).round() as i32),
            Chrome::Kubuno | Chrome::Custom => {
                // SAFETY: plain metric queries.
                let (frame_x, frame_y) = unsafe {
                    (
                        GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi),
                        GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi),
                    )
                };
                let caption = (self.chrome_top() * self.scale()).round() as i32;
                (2 * frame_x, frame_y + caption)
            }
        }
    }

    fn rebuild_renderer(&mut self) {
        let (w, h) = client_px(self.hwnd);
        // The font override is `None` unless the caller asked for one
        // (`HostOptions::font_override`, `set_font_override`): the controls
        // then read in the design system's own embedded face.
        //
        // A failure here is NOT swallowed. `Renderer::new` builds its bitmaps
        // through WIC (`CoCreateInstance`), which fails on a thread with no COM
        // apartment — and the symptom is a window that opens permanently blank
        // with nothing logged anywhere. Saying so costs one line and saves an
        // afternoon.
        // A floating panel renders into a swap chain its own composition tree shows over the
        // blurred backdrop (`backdrop`); without a backdrop it falls back to the window's own target.
        self.panel_backdrop = None;
        if let Some(panel) = self.form.panel.filter(|_| self.parent.is_none()) {
            match Renderer::new_detached(w, h, self.dpi, self.font_override.as_deref()) {
                Ok(r) => match backdrop::PanelBackdrop::new(self.hwnd, r.swapchain(), panel.radius, panel.shadow_margin, self.scale()) {
                    Ok(b) => {
                        b.resize(w, h);
                        self.panel_backdrop = Some(b);
                        self.renderer = Some(r);
                    }
                    Err(e) => eprintln!("[host] floating panel backdrop: {e} (painted opaque)"),
                },
                Err(e) => eprintln!("[host] floating panel renderer {w}x{h}: {e}"),
            }
        }
        if self.panel_backdrop.is_none() {
            match Renderer::new(self.hwnd, w, h, self.dpi, self.font_override.as_deref()) {
                Ok(r) => self.renderer = Some(r),
                Err(e) => {
                    self.renderer = None;
                    eprintln!("[host] renderer {w}x{h} @ {} dpi: {e}", self.dpi);
                }
            }
        }
        // Every cached themed part is a bitmap belonging to the device that was
        // just replaced. Drawing one from the new context is undefined and
        // surfaces as a failure inside `EndDraw` — far from here, and only on
        // the machines that actually lose a device.
        self.parts.on_device_lost();
        self.rebuild_visuals();
    }

    /// Re-reads the system's colours, metrics and UI font at the current DPI.
    ///
    /// Called when the renderer is built and whenever the DPI moves. The
    /// metrics and the font are BOTH DPI-dependent, so they are re-read
    /// together: a window dragged to a 175 % display with a 96 DPI font would
    /// measure every control against text a third too small, and the mistake
    /// looks like a layout bug rather than a stale read.
    fn rebuild_visuals(&mut self) {
        // The themed parts are read per DPI too — `OpenThemeDataForDpi` hands
        // back the theme's own artwork for that scale, so a stale handle is a
        // part drawn from the 96 DPI bitmaps and stretched. Kept next to the
        // metrics and the font because all three go stale at the same instant.
        self.parts.set_dpi(self.dpi);
        let Some(renderer) = self.renderer.as_ref() else {
            self.visuals = None;
            return;
        };
        match Visuals::read(&renderer.dwrite, self.dpi) {
            Ok(v) => self.visuals = Some(v),
            Err(e) => {
                // Without the system font there is nothing honest to paint a
                // control with, so this is reported rather than papered over
                // with the design system's own face.
                self.visuals = None;
                eprintln!("[host] system visuals @ {} dpi: {e}", self.dpi);
            }
        }
    }

    fn resize(&mut self) {
        let (w, h) = client_px(self.hwnd);
        let dpi = self.dpi;
        if let Some(r) = self.renderer.as_mut() {
            // A failed resize leaves the swap chain at its old size, showing as
            // an unpainted strip — worth a line rather than a silent `let _`.
            if let Err(e) = r.resize(w, h, dpi) {
                eprintln!("[host] resize {w}x{h}: {e}");
            }
        }
        let scale = self.scale();
        if let Some(b) = self.panel_backdrop.as_mut() {
            b.set_scale(scale, w, h);
        }
        if self.parent.is_none() {
            if self.rounded.is_some() {
                self.update_rounded();
            } else {
                set_page_inset(self.hwnd, (self.panel_margin() * scale).round() as i32);
            }
        }
        self.invalidate();
    }

    /// The room around the window's visible frame for its shadow, in DIP: a floating panel's
    /// ([`FormOptions::panel`]), a host-rounded window's while Windows does not square it
    /// (`frame`); 0 for a window whose frame DWM draws.
    fn panel_margin(&self) -> f32 {
        if self.parent.is_some() {
            return 0.0;
        }
        if let Some(p) = self.form.panel {
            return p.shadow_margin;
        }
        self.rounded.map_or(0.0, |f| f.margin())
    }

    /// A host-rounded window's frame against a client point `(x, y)` of a client area `size` (DIP):
    /// the point and the size in the frame's own coordinates (one shadow margin in), or the hit
    /// test's answer when the point is on the curve's resize band (`border`) or in the margin,
    /// where the clicks go through. Any other window: the point and size unchanged.
    fn frame_point(&self, x: f32, y: f32, size: (f32, f32), border: Option<f32>) -> std::result::Result<FramePoint, LRESULT> {
        let Some(f) = self.rounded.filter(|_| self.parent.is_none()) else { return Ok(((x, y), size)) };
        let m = f.margin();
        let (fx, fy, fs) = (x - m, y - m, ((size.0 - 2.0 * m).max(0.0), (size.1 - 2.0 * m).max(0.0)));
        match frame::shape_hit(fs, f.shown_radius(), fx, fy, border) {
            frame::ShapeHit::Inside => Ok(((fx, fy), fs)),
            frame::ShapeHit::Resize(ht) => Err(LRESULT(ht as isize)),
            frame::ShapeHit::Outside => Err(LRESULT(HTTRANSPARENT as isize)),
        }
    }

    /// Embedded mode: moves the keyboard focus into this child window if it
    /// is not already there. No-op for a top-level host (activation does it).
    fn take_focus_if_embedded(&self) {
        if self.parent.is_some() {
            // SAFETY: plain focus query/change on the host's own window.
            unsafe {
                if GetFocus() != self.hwnd {
                    let _ = SetFocus(Some(self.hwnd));
                }
            }
        }
    }

    fn scale(&self) -> f32 {
        (self.dpi / 96.0).max(0.01)
    }

    fn dip(&self, px: f32) -> f32 {
        px / self.scale()
    }

    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    /// A modal window ([`HostOptions::modal`]) going away: the windows it disabled take input
    /// again. Idempotent.
    fn enable_others(&mut self) {
        for other in self.disabled_others.drain(..) {
            // SAFETY: enabling a window handle is sound even if it has since been destroyed.
            unsafe {
                let _ = EnableWindow(other, true);
            }
        }
    }

    /// Asks for a frame because there is WORK to do (a [`UiWaker`] wake-up, a
    /// [`request_wake_after`] timer, a close request): an invalidation when the
    /// window can paint, a direct render when it is minimised or hidden and no
    /// `WM_PAINT` would come.
    fn request_frame(&mut self) {
        // SAFETY: plain window-state queries on the host's own window.
        let offscreen = unsafe { IsIconic(self.hwnd).as_bool() || !IsWindowVisible(self.hwnd).as_bool() };
        if offscreen {
            self.render();
        } else {
            self.invalidate();
        }
    }

    /// Registers the window as an OLE drop target ([`dnd`]); `false` when OLE refuses (the window
    /// already has a drop target — a design surface's own —, or OLE cannot start on this thread).
    fn register_drop_target(&self) -> bool {
        use windows::Win32::System::Ole::{IDropTarget, OleInitialize, RegisterDragDrop};
        // SAFETY: OLE initialisation of this (UI, STA) thread and registration of a live COM object
        // for our own window; OLE keeps its own reference.
        unsafe {
            if let Err(e) = OleInitialize(None) {
                tracing::warn!("drag and drop disabled: OleInitialize failed ({e})");
                return false;
            }
            let target: IDropTarget = dnd::ole::DropTarget { sink: Box::new(DropSink { hwnd: self.hwnd }) }.into();
            match RegisterDragDrop(self.hwnd, &target) {
                Ok(()) => true,
                Err(e) => {
                    tracing::info!("the window keeps its own drop target ({e}): Kubuno drag and drop is off for it");
                    false
                }
            }
        }
    }

    fn render(&mut self) {
        if self.rendering {
            // A modal loop opened from this frame (a dialog, a drag) delivered a paint or a
            // wake-up: the window keeps its last frame, and paints again once this one ends. An
            // invalidation here would come straight back as `WM_PAINT` for as long as the modal
            // loop runs.
            self.deferred_paint = true;
            return;
        }
        self.rendering = true;
        self.render_frame();
        self.rendering = false;
        if std::mem::take(&mut self.deferred_paint) {
            self.invalidate();
        }
        // A close request the frame did not handle (the page stopped deferring
        // closes, or never reached its runtime this frame) closes the window,
        // as the request would have without deferral.
        if input::unconsumed_close_request().is_some() {
            quit();
        }
    }

    fn render_frame(&mut self) {
        // A theme or font change asked for since the last frame lands first,
        // so this frame already paints with it.
        self.apply_pending();
        // A host-rounded window squared by Windows since its last size change (a snap is flagged
        // after it): its margin and curve go now.
        if self.rounded.is_some() && self.parent.is_none() {
            self.update_rounded();
        }
        let (w, h) = client_px(self.hwnd);
        // Read the DPI back from the window rather than trusting the cached
        // field. `self.dpi` starts at 96 and is only refreshed on `init` and
        // `WM_DPICHANGED`, so any paint that slips in before or between those
        // hands the caller a 1:1 line while the canvas draws at the real scale
        // — layout then measures against a window far wider than the one it is
        // drawn into, and the right-hand content is silently clipped away.
        // One call per frame removes the whole class of staleness.
        // A zoom asked for by the page (`input::set_zoom`, a designer's zoom) scales the DPI: every
        // DIP then covers more or fewer pixels, the whole page included (bitmaps, icons, text).
        let dpi = unsafe { GetDpiForWindow(self.hwnd) } as f32 * input::zoom();
        if dpi > 0.0 {
            let changed = (dpi - self.dpi).abs() > 0.01;
            self.dpi = dpi;
            if changed {
                if let Some(r) = self.renderer.as_mut() {
                    if let Err(e) = r.resize(w, h, dpi) {
                        eprintln!("[host] zoom to {dpi} dpi: {e}");
                    }
                }
            }
        }
        // The system visuals are read PER DPI, and the check lives here rather
        // than only in `WM_DPICHANGED` for the same reason the DPI itself is
        // re-read above: a paint can arrive before the message does, and a
        // stale metric or font size is invisible until something measures a
        // pixel wrong.
        if !self.visuals.as_ref().is_some_and(|v| v.matches_dpi(self.dpi)) {
            self.rebuild_visuals();
        }

        let scale = (self.dpi / 96.0).max(0.01);
        // A pending dismissal is delivered to exactly one frame.
        let dismiss = std::mem::take(&mut self.dismiss_pending);
        // The keys and text received since the last frame become this frame's
        // queue (see `input`); the wheel travel is handed over the same way.
        input::begin_frame();
        // A frame starts unscrolled; an unbalanced push from the last one must
        // not shift this one's popups.
        CONTENT_OFFSET.with(|c| c.set((0.0, 0.0)));
        let wheel = std::mem::take(&mut self.wheel);
        let mods = Modifiers::current();
        // The paint pass is scoped so the immutable borrows of
        // `renderer`/`theme`/`visuals`/`parts` all end before the overlay pass,
        // which needs `&mut self` to build or resize the popup window.
        {
            // Borrow the fields directly (not through `&self` helpers) so the
            // immutable borrows of `renderer`/`theme`/`visuals`/`parts` and the
            // mutable borrow of `on_paint` are disjoint and all allowed at once.
            let Some(renderer) = self.renderer.as_ref() else { return };
            let parts = &self.parts;
            // No visuals means the system font could not be read, which
            // `rebuild_visuals` has already reported. Painting anyway would paint
            // in the design system's colours — the exact thing this crate stopped
            // doing — so the frame is skipped instead.
            let Some(visuals) = self.visuals.as_ref() else { return };

            // Under Chrome::Kubuno the client area's top band (`chrome_top`)
            // DIP belong to the host: the client keeps painting into its full
            // window, but the page's top strip must land at y = chrome_top.
            // Mouse, size and origin STAY in the full client's coordinates —
            // the caption reads them there, and the client shifts by chrome_top
            // wherever its layout needs to.
            // Under Chrome::Custom nothing is reserved: the page paints its
            // own title bar from y = 0 and declares it (`set_title_bar`).
            let chrome_top = self.chrome_top();
            // Consumed here, exactly once per actually-painted frame (not on
            // an early return above): a button pressed since the last paint
            // reads as down for this frame even if it was already released
            // again, so a click that completed entirely between two frames
            // is never silently lost. See `consume_button_edge`.
            let mouse_down = consume_button_edge(self.mouse_down, &mut self.left_press_latch);
            let right_down = consume_button_edge(self.right_down, &mut self.right_press_latch);
            let middle_down = consume_button_edge(self.middle_down, &mut self.middle_press_latch);
            // A floating panel's page is the panel alone, one shadow margin in from the window's
            // edges: its size, its pointer and its origin on screen are the panel's.
            let margin = self.panel_margin();
            let frame = Frame {
                size: (w as f32 / scale - margin * 2.0, h as f32 / scale - margin * 2.0),
                mouse: (self.mouse.0 - margin, self.mouse.1 - margin),
                mouse_down,
                right_down,
                middle_down,
                dismiss,
                scale,
                client_origin: { let (x, y) = client_origin_dip(self.hwnd, scale); (x + margin, y + margin) },
                work_area: work_area_dip(self.hwnd, scale, (w as f32 / scale, h as f32 / scale)),
                chrome_top,
                mods,
                wheel,
                click_count: self.click_count,
                window_focused: self.window_focused,
            };
            let ctx = &renderer.d2d_context;
            unsafe {
                ctx.BeginDraw();
                // The page background is painted OPAQUELY: an unpainted pixel comes
                // out black after a resize, and a solid ground is also the fair
                // backdrop to compare a control against its reference sheet.
                // `COLOR_BTNFACE`, which is what `Form.DefaultBackColor` resolves to
                // and exactly the ground the reference sheets were captured on.
                // Clearing to the Kubuno window colour instead left every page a
                // shade too light, which showed up as a pale slab behind a
                // GroupBox's caption notch.
                //
                // That holds for the replica demos (`Chrome::System`). A window
                // wearing the Kubuno chrome is a Kubuno surface: its ground is
                // the theme's page colour, or every pixel a page leaves unpainted
                // (a transparent tab strip, a gap between panes) stays the light
                // system face in the dark theme.
                let ground = match self.chrome {
                    // A floating panel: transparent, the backdrop shows through what the page
                    // leaves translucent (and the shadow margin is only the shadow).
                    _ if self.form.panel.is_some() && self.parent.is_none() => TRANSPARENT,
                    Chrome::System => visuals.colors.control,
                    Chrome::Kubuno | Chrome::Custom => self.theme.window_background,
                };
                // A host-rounded window (`frame`): transparent around and outside the curve; its
                // frame is filled with the ground below.
                let rounded = self.rounded.filter(|_| self.parent.is_none());
                ctx.Clear(Some(if rounded.is_some() { &TRANSPARENT } else { &ground }));
                if let Ok(painter) = Painter::new(renderer, &self.theme, visuals, parts) {
                    painter.set_scale(scale);
                    // A widget's opaque-ground preamble reads `current_bg()`;
                    // with nothing pushed it must be exactly this clear
                    // colour, so the preamble repaints the same pixels and
                    // leaves no square where the web paints nothing.
                    painter.set_ground(ground);
                    // The client paints from its own y = 0. Under Chrome::Kubuno
                    // the top `caption::HEIGHT` DIP belong to the host, so the
                    // client is told about them via `Frame::chrome_top` and lays
                    // its own top strip out below that. Translating the D2D
                    // context here is not enough: `vector_icon` resets it to
                    // identity on every icon, so a call like `text` painted
                    // between two icons would fall out of the translated frame.
                    // Each frame declares its title bar afresh (Custom chrome):
                    // a frame that does not has no drag band.
                    TITLE_BAR.with(|t| *t.borrow_mut() = TitleBar::default());
                    // The Kubuno band (`crate::window_chrome`, the web `FloatingWindow`'s): its
                    // ground BEFORE the page, so the page may put its own controls on it
                    // (`title_bar_layout`), then the icon, title and caption buttons AFTER it, so a
                    // page extending under the band still gets them on top. The page may recolour
                    // the band (a ribbon continuing its tab strip into it): the ground uses the
                    // colours of the last frame, a change repaints once more.
                    // The window's frame: the client area, less the shadow margin of a floating
                    // panel or a host-rounded window — everything below is drawn in its
                    // coordinates.
                    let full_size = (w as f32 / scale - margin * 2.0, h as f32 / scale - margin * 2.0);
                    let shaped = self.paint_shape(&painter, full_size, margin, &ground);
                    let band_ctx = self.has_kubuno_band().then(|| {
                        let mut c = self.chrome_context(full_size);
                        if let Some((band, ink)) = self.caption_override {
                            c.style.background = Some(band);
                            c.style.foreground = Some(ink);
                        }
                        c
                    });
                    chrome::begin_frame(band_ctx.clone());
                    if let Some(c) = &band_ctx {
                        let l = crate::window_chrome::layout(&c.style, c.bounds, c.has_icon, c.buttons, Default::default());
                        crate::window_chrome::paint_band(&painter, &c.style, &l);
                    }
                    paint_debug::begin_frame();
                    let paint_start = std::time::Instant::now();
                    let start_ms = now_ms();
                    // The application's frame, where its event handlers run: under a debugger a
                    // panic in it unwinds to here after the debugger's break (see run_frame).
                    let on_paint = &mut self.on_paint;
                    diagnostics::run_frame(|| on_paint(&painter, &frame));
                    paint_debug::record_frame(start_ms, paint_start.elapsed().as_micros().min(u128::from(u64::MAX)) as u64);
                    if let Some(c) = &band_ctx {
                        let colors = input::frame_caption_colors();
                        let mut style = c.style.clone();
                        if let Some((band, ink)) = colors {
                            style.background = Some(band);
                            style.foreground = Some(ink);
                        } else if self.caption_override.is_some() {
                            style.background = self.form.chrome.background;
                            style.foreground = self.form.chrome.foreground;
                        }
                        if colors != self.caption_override {
                            self.caption_override = colors;
                            request_repaint_after(1);
                        }
                        let layout = crate::window_chrome::layout(&style, c.bounds, c.has_icon, c.buttons, chrome::declared_slots());
                        // The window's icon (`Form.Icon`): a Lucide glyph name, or an image file.
                        let bitmap = if self.icon_glyph().is_none() {
                            self.form.icon.as_deref().and_then(|path| crate::styled::load_image(&painter, path))
                        } else {
                            None
                        };
                        let icon = match (self.icon_glyph(), bitmap.as_ref()) {
                            (Some(glyph), _) => crate::window_chrome::ChromeIcon::Glyph(glyph),
                            (None, Some(b)) => crate::window_chrome::ChromeIcon::Bitmap(b),
                            (None, None) => crate::window_chrome::ChromeIcon::None,
                        };
                        let state = crate::window_chrome::ChromeState {
                            hot: self.chrome_hot,
                            pressed: self.chrome_pressed,
                            maximized: IsZoomed(self.hwnd).as_bool(),
                        };
                        crate::window_chrome::paint_caption(&painter, &style, &layout, &self.title, icon, state);
                        self.caption_band = style.band_color(&self.theme);
                    }
                    if let Some(grip) = self.grip_rect(full_size) {
                        let hot = grip.contains(self.mouse.0 - margin, self.mouse.1 - margin);
                        crate::window_chrome::paint_grip(&painter, self.grip_bounds(full_size), hot);
                    }
                    if shaped {
                        self.end_shape(&painter, full_size);
                    }
                    // The paint debug overlay, over everything the window painted.
                    if paint_debug::paint(&painter, (w as f32 / scale, h as f32 / scale), now_ms()) {
                        request_repaint_after(16);
                    }
                }
                let _ = ctx.EndDraw(None, None);
            }
            let _ = renderer.present();
        }

        // The floating surfaces the frame requested, painted last into their own
        // top-level popup — over everything, the nav strip included, and free to
        // spill beyond the window's edges.
        self.render_overlays();
        // After everything that could have consumed a key this frame (the
        // page's own paint AND any popup it opened) — see
        // `forward_unhandled_keys`'s own doc.
        if self.parent.is_some() {
            self.forward_unhandled_keys();
        }
        self.apply_frame_requests();
        // The drag this frame saw: an enter becomes a drag moving over, a leave or a drop ends.
        dnd::with_tracker(|t| t.end_frame());
    }

    /// Embedded mode only: records a `WM_KEYDOWN`/`WM_SYSKEYDOWN` as a
    /// candidate for [`Host::forward_unhandled_keys`], alongside the decoded
    /// virtual key and modifiers `queue_key` will also queue it under.
    fn record_forward_candidate(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) {
        let vk = (wparam.0 & 0xFFFF) as u16;
        self.pending_forward.push((msg, wparam, lparam, vk, Modifiers::current()));
    }

    /// Embedded mode only: re-posts to the parent every key-down this frame's
    /// page (and any popup it opened) did NOT consume from the input queue —
    /// `FocusRing`/a text field takes `Tab`, arrows, typed characters… so
    /// whatever is left is, by definition, not meant for this surface.
    ///
    /// This is the "cheap variant" of `vskubuno/docs/DESIGNER.md` §7's
    /// keyboard shim: the SAME message (`msg`/`wParam`/`lParam`) a real
    /// keystroke on a VS-owned window would have produced is posted to the
    /// parent HWND, so VS's own message-pump filters
    /// (`IVsFilterKeys2.TranslateAcceleratorEx` / `ComponentDispatcher`) can
    /// route it to Ctrl+S, F5, Ctrl+Z, Ctrl+Shift+B and the rest of VS's
    /// accelerator table without this crate knowing anything about VS
    /// commands. A repeated key held down that is unconsumed for part of its
    /// repeat and consumed for the rest (unusual) may be forwarded a few more
    /// or fewer times than it repeated — acceptable for a shim whose whole
    /// purpose is single-shot accelerator chords, not text input.
    fn forward_unhandled_keys(&mut self) {
        let Some(parent) = self.parent else { return };
        if self.pending_forward.is_empty() {
            return;
        }
        let remaining = input::events();
        for (msg, wparam, lparam, vk, mods) in self.pending_forward.drain(..) {
            if remaining.iter().any(|e| e.is_key_down(vk, mods)) {
                let mods_bits = (mods.ctrl as usize) | ((mods.shift as usize) << 1) | ((mods.alt as usize) << 2);
                // SAFETY: posting to a window handle is sound even if the
                // window has since been destroyed (the call then simply
                // fails). Order matters: WM_KUBUNO_KEY_MODS FIRST (its own doc
                // explains why), the key message second — both posted from
                // this same thread to the same destination, so PostMessage's
                // FIFO ordering guarantees the parent sees them in this order.
                unsafe {
                    let _ = PostMessageW(Some(parent), WM_KUBUNO_KEY_MODS, WPARAM(mods_bits), LPARAM(0));
                    let _ = PostMessageW(Some(parent), msg, wparam, lparam);
                }
            }
        }
    }

    /// Applies what the frame asked for through [`set_cursor`] and
    /// [`request_repaint_after`], [`set_form`], [`access::publish`] and [`accept_files`].
    fn apply_frame_requests(&mut self) {
        if let Some(form) = form::take_pending() {
            if self.parent.is_none() && form != self.form {
                form::apply(self.hwnd, &form, Some(&self.form), WS_OVERLAPPEDWINDOW, self.base_ex_style());
                if form.title != self.form.title {
                    if let Some(title) = &form.title {
                        self.title = title.clone();
                    }
                }
                caption::set_buttons(form::caption_buttons(&form));
                let dwm_changed = (&form.chrome, form.corner, form.corner_radius, form.border_color, form.backdrop)
                    != (&self.form.chrome, self.form.corner, self.form.corner_radius, self.form.border_color, self.form.backdrop);
                self.form = form;
                // A host-rounded window follows a new radius itself (`CornerRadius` changed).
                if self.rounded.is_some() {
                    let radius = frame::radius_override().unwrap_or_else(|| self.form.corner_radius());
                    if let Some(f) = self.rounded.as_mut() {
                        f.radius = radius;
                    }
                    self.update_rounded();
                }
                if dwm_changed {
                    self.apply_dwm();
                    self.dwm_band = u32::MAX;
                }
                self.invalidate();
            }
        }
        if let Some(mut tree) = access::take_published() {
            // A floating panel's page is inset by its shadow margin: its elements' bounds are in
            // the page's coordinates, the window's start one margin before.
            let margin = self.panel_margin();
            if margin > 0.0 {
                for n in &mut tree.nodes {
                    n.bounds = (n.bounds.0 + margin, n.bounds.1 + margin, n.bounds.2 + margin, n.bounds.3 + margin);
                }
            }
            // Screen readers: the adapter is created with the first tree a page publishes (never
            // while answering `WM_GETOBJECT`, as AccessKit requires); a window whose page publishes
            // none keeps Windows' own answer.
            if self.parent.is_none() {
                let focused = self.window_focused;
                let hwnd = self.hwnd;
                self.access.get_or_insert_with(|| access::Access::new(hwnd, focused)).update(&tree);
            }
        }
        let accept = ACCEPT_FILES.with(|a| a.replace(false));
        ACCEPT_FILES_LAST.with(|a| a.set(accept));
        if accept != self.accepting_files && self.parent.is_none() {
            self.accepting_files = accept;
            // SAFETY: plain shell call on the host's own window.
            unsafe { DragAcceptFiles(self.hwnd, i32::from(accept)) };
        }
        // Drag and drop: the OLE drop target is registered the first time the page has a drop
        // target (an embedded surface's parent may own drops: a refusal is remembered, not retried).
        if dnd::take_accept() && self.drop_target.is_none() {
            self.drop_target = Some(self.register_drop_target());
        }
        if dnd::has_pending_start() {
            // SAFETY: posting to our own window.
            unsafe {
                let _ = PostMessageW(Some(self.hwnd), WM_KUBUNO_DRAG, WPARAM(0), LPARAM(0));
            }
        }
        let band = accent_bgr(self.caption_band);
        // Custom chrome leaves DWM's caption/border colours alone.
        if band != self.dwm_band && self.chrome != Chrome::Custom {
            self.dwm_band = band;
            // SAFETY: plain DWM attribute calls on our own live window.
            unsafe {
                let border = self.form.border_color.map_or(band, accent_bgr);
                let _ = DwmSetWindowAttribute(self.hwnd, DWMWA_BORDER_COLOR, &border as *const u32 as *const _, 4);
                let _ = DwmSetWindowAttribute(self.hwnd, DWMWA_CAPTION_COLOR, &band as *const u32 as *const _, 4);
            }
        }
        let cursor = input::frame_cursor();
        if cursor != self.cursor {
            self.cursor = cursor;
            // `WM_SETCURSOR` only comes with the next pointer move; a shape
            // that changes under a still pointer (a drag starting, a field
            // appearing under it) is applied now, if the pointer is ours.
            if self.pointer_is_ours() {
                unsafe {
                    SetCursor(Some(load_cursor(cursor)));
                }
            }
        }
        if let Some(ms) = input::take_repaint_after() {
            unsafe {
                // Same id each time: a new request replaces the pending one.
                SetTimer(Some(self.hwnd), REPAINT_TIMER_ID, ms.max(1), None);
            }
        }
        if let Some(ms) = input::take_wake_after() {
            unsafe {
                SetTimer(Some(self.hwnd), WAKE_TIMER_ID, ms.max(1), None);
            }
        }
    }

    /// Whether the pointer is over the main window's client area or one of its
    /// interactive popups, or captured by one of them (a drag).
    fn pointer_is_ours(&self) -> bool {
        unsafe {
            let cap = GetCapture();
            if !cap.is_invalid() && (cap == self.hwnd || self.popups.iter().any(|o| o.hwnd == cap)) {
                return true;
            }
            let mut p = POINT::default();
            if GetCursorPos(&mut p).is_err() {
                return false;
            }
            let under = WindowFromPoint(p);
            if under == self.hwnd {
                let lp = LPARAM(((p.y as u16 as isize) << 16) | (p.x as u16 as isize));
                let ht = SendMessageW(self.hwnd, WM_NCHITTEST, None, Some(lp));
                return ht.0 as u32 == HTCLIENT;
            }
            self.popups.iter().any(|o| o.interactive && o.hwnd == under)
        }
    }

    /// The pointer left the main window or a popup. Unless it went into the
    /// other one, or a drag holds it, the page is told it is nowhere, so hover
    /// states clear.
    fn pointer_left(&mut self) {
        if self.mouse_down || self.pointer_is_ours() {
            return;
        }
        self.mouse = (POINTER_AWAY, POINTER_AWAY);
        self.invalidate();
    }

    /// Arms `TME_LEAVE` on the main window, once per entry.
    fn track_leave(&mut self) {
        if self.tracking_leave {
            return;
        }
        let mut tme = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.hwnd,
            dwHoverTime: 0,
        };
        unsafe {
            if TrackMouseEvent(&mut tme).is_ok() {
                self.tracking_leave = true;
            }
        }
    }

    /// A left press (single or `WM_LBUTTONDBLCLK`) at screen point `p`:
    /// counts it as the next of a double/triple click when it is close enough
    /// in time and space to the previous one.
    fn register_press(&mut self, p: POINT) {
        unsafe {
            let t = GetMessageTime() as u32;
            let dt = GetDoubleClickTime();
            let hx = GetSystemMetrics(SM_CXDOUBLECLK) / 2;
            let hy = GetSystemMetrics(SM_CYDOUBLECLK) / 2;
            let (lt, lx, ly) = self.last_press;
            let near = (p.x - lx).abs() <= hx && (p.y - ly).abs() <= hy;
            self.click_count = if self.click_count > 0 && t.wrapping_sub(lt) <= dt && near {
                (self.click_count + 1).min(3)
            } else {
                1
            };
            self.last_press = (t, p.x, p.y);
        }
    }

    /// `WM_MOUSEWHEEL` / `WM_MOUSEHWHEEL`, from the main window or a popup:
    /// accumulates the travel for the next frame (web sign convention, see
    /// [`Frame::wheel`]) and moves the pointer to where the wheel turned.
    fn on_wheel(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) {
        let notches = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16 as f32 / WHEEL_DELTA as f32;
        if msg == WM_MOUSEHWHEEL {
            self.wheel.0 += notches;
        } else {
            self.wheel.1 -= notches;
        }
        let mut p = POINT {
            x: (lparam.0 & 0xFFFF) as i16 as i32,
            y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
        };
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut p);
        }
        self.mouse = (self.dip(p.x as f32), self.dip(p.y as f32));
        self.invalidate();
    }

    /// Returns the index in [`Host::popups`] of the `k`-th window of the given
    /// kind (pass-through or interactive), creating it — sized `w × h`
    /// physical pixels — when the pool of that kind has fewer. `None` if it
    /// could not be built.
    ///
    /// Each is a top-level `WS_POPUP` OWNED by the host window — so it stays
    /// above it and is destroyed with it — transparent through
    /// DirectComposition (the surface clears to nothing but what it paints) and
    /// never activated (`WS_EX_NOACTIVATE`, plus `MA_NOACTIVATE` in its window
    /// procedure), so the window that opened it keeps the focus. Being top-level
    /// rather than a child, it can extend past the host window's edges, which is
    /// the whole point.
    ///
    /// `WS_EX_NOREDIRECTIONBITMAP`: the window's pixels come from its swap
    /// chain ONLY. Without it Windows also keeps a GDI redirection surface
    /// under the composition visual, which nothing ever paints: wherever the
    /// swap chain is transparent — or not presented yet after a resize — that
    /// surface shows through, as a black or stale rectangle.
    ///
    /// The kind is fixed for the window's life (`WS_EX_TRANSPARENT` for a
    /// pass-through one): a tooltip's window never becomes a menu's.
    unsafe fn acquire_popup(&mut self, interactive: bool, k: usize, w: u32, h: u32) -> Option<usize> {
        let mut seen = 0;
        for (i, ov) in self.popups.iter().enumerate() {
            if ov.interactive == interactive {
                if seen == k {
                    return Some(i);
                }
                seen += 1;
            }
        }
        unsafe {
            let Ok(instance) = GetModuleHandleW(None) else { return None };
            let class = w!("KubunoControlsOverlay");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                // Double-clicks on a menu or a calendar arrive as
                // `WM_LBUTTONDBLCLK`, mapped to a press in `overlay_wndproc`.
                style: CS_DBLCLKS,
                lpfnWndProc: Some(overlay_wndproc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: class,
                ..Default::default()
            };
            // Idempotent: a second popup (or host) reuses the class, so an
            // "already registered" failure is not fatal.
            RegisterClassExW(&wc);

            let mut ex = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP;
            if !interactive {
                // Pass-through: a tooltip must never eat the click meant for
                // what it describes; a menu must.
                ex |= WS_EX_TRANSPARENT;
            }
            let Ok(hwnd) = CreateWindowExW(
                ex,
                class,
                w!(""),
                WS_POPUP,
                0,
                0,
                w.max(1) as i32,
                h.max(1) as i32,
                Some(self.hwnd),
                None,
                Some(instance.into()),
                None,
            ) else {
                return None;
            };
            // The popup reports the pointer back to this host (see
            // `overlay_wndproc`). The host is boxed for the program's life, so
            // the pointer stays valid as long as the popup does.
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, self as *mut Host as isize);

            match Renderer::new(hwnd, w.max(1), h.max(1), self.dpi, self.font_override.as_deref()) {
                Ok(renderer) => {
                    self.popups.push(OverlayWindow {
                        hwnd,
                        interactive,
                        visible: false,
                        placed: (0, 0, 0, 0),
                        renderer,
                        parts: ThemeRenderer::new(),
                        size_px: (w.max(1), h.max(1)),
                    });
                    Some(self.popups.len() - 1)
                }
                Err(e) => {
                    eprintln!("[host] popup renderer {w}x{h}: {e}");
                    let _ = DestroyWindow(hwnd);
                    None
                }
            }
        }
    }

    /// Paints the frame's floating surfaces into their popup windows.
    ///
    /// Drained after the main paint (hence `&mut self` here, not during the
    /// borrow-locked paint pass). Pass-through surfaces ([`overlay`]) and
    /// interactive ones ([`popup`]) come from two separate pools, the n-th
    /// surface of a kind going to the n-th window of that kind — so a tooltip
    /// and the menu that replaces it never share a window. Windows not used
    /// this frame are hidden, so a surface a page stops requesting disappears.
    /// Each is positioned in screen pixels from its client-DIP bounds, painted
    /// onto a transparent surface, and shown topmost in request order (a later
    /// surface — a submenu — stacks over an earlier one) without stealing
    /// focus.
    ///
    /// No stale pixels: a window that appears (or jumps to another place at
    /// another size, which is the same thing to the eye) is hidden first,
    /// repainted, and shown only once the compositor has taken the new frame
    /// (`DwmFlush`) — so it never flashes the surface it showed last time, nor
    /// an unpresented buffer. A surface that stays put and keeps its size (a
    /// menu whose highlight moves) or only moves (a tooltip following the
    /// pointer) is updated in place without waiting.
    fn render_overlays(&mut self) {
        let reqs: Vec<OverlayReq> = OVERLAYS.with(|o| o.borrow_mut().drain(..).collect());
        LAST_POPUPS.with(|l| {
            *l.borrow_mut() = reqs.iter().filter(|r| r.interactive).map(|r| r.bounds).collect();
        });
        let scale = self.scale();
        // The page's origin: one shadow margin in for a floating panel or a host-rounded window.
        let inset = (self.panel_margin() * scale).round() as i32;
        let mut origin = POINT { x: inset, y: inset };
        unsafe {
            let _ = ClientToScreen(self.hwnd, &mut origin);
        }

        let mut used: Vec<usize> = Vec::with_capacity(reqs.len());
        // Without the system visuals nothing can be painted (already reported
        // by `rebuild_visuals`): every surface is hidden rather than left up.
        let reqs = if self.visuals.is_some() { reqs } else { Vec::new() };
        let mut counts = [0usize; 2];
        for req in reqs {
            let b = req.bounds;
            let w = (((b.right - b.left) * scale).round().max(1.0)) as u32;
            let h = (((b.bottom - b.top) * scale).round().max(1.0)) as u32;
            let x = origin.x + (b.left * scale).round() as i32;
            let y = origin.y + (b.top * scale).round() as i32;

            let kind = usize::from(req.interactive);
            let k = counts[kind];
            counts[kind] += 1;
            let Some(i) = (unsafe { self.acquire_popup(req.interactive, k, w, h) }) else {
                continue;
            };
            used.push(i);

            let dpi = self.dpi;
            let place = (x, y, w as i32, h as i32);
            let (resized, fresh) = {
                let ov = &mut self.popups[i];
                let resized = ov.size_px != (w, h);
                let moved = (ov.placed.0, ov.placed.1) != (x, y);
                // Appearing, or jumping elsewhere at another size: to the eye
                // a new surface, so it must not show its previous content.
                let fresh = !ov.visible || (resized && moved);
                if fresh && ov.visible {
                    unsafe {
                        let _ = ShowWindow(ov.hwnd, SW_HIDE);
                    }
                    ov.visible = false;
                }
                if resized {
                    if let Err(e) = ov.renderer.resize(w, h, dpi) {
                        eprintln!("[host] popup resize {w}x{h}: {e}");
                    }
                    ov.size_px = (w, h);
                }
                (resized, fresh)
            };

            let Some(visuals) = self.visuals.as_ref() else { break };
            let ov = &self.popups[i];
            unsafe {
                let ctx = &ov.renderer.d2d_context;
                ctx.BeginDraw();
                // Cleared to fully transparent: only what the surface paints
                // shows, so every other pixel lets whatever is behind it — the
                // host window, another app, the desktop — through.
                ctx.Clear(None);
                if let Ok(painter) = Painter::new(&ov.renderer, &self.theme, visuals, &ov.parts) {
                    painter.set_scale(scale);
                    // The window is cleared to transparent, so an unpushed
                    // `current_bg()` is transparent too: a widget's
                    // opaque-ground preamble must not paint a square block
                    // around a rounded panel and its shadow.
                    painter.set_ground(TRANSPARENT);
                    (req.paint)(&painter);
                }
                let _ = ctx.EndDraw(None, None);
                let _ = ov.renderer.present();
                // A window about to appear or to change size waits until the
                // compositor has latched the frame just presented; otherwise
                // it can be shown (or grown) one refresh early, with whatever
                // its swap chain held before.
                if fresh || resized {
                    let _ = DwmFlush();
                }
                // Position and show last, so the first frame a surface appears
                // on is already painted rather than flashing an empty window.
                // Each is raised to the top of the topmost band in request
                // order, so a later surface stacks over an earlier.
                let _ = SetWindowPos(
                    ov.hwnd,
                    Some(HWND_TOPMOST),
                    x,
                    y,
                    w as i32,
                    h as i32,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
            let ov = &mut self.popups[i];
            ov.visible = true;
            ov.placed = place;
        }

        for (i, ov) in self.popups.iter_mut().enumerate() {
            if ov.visible && !used.contains(&i) {
                unsafe {
                    let _ = ShowWindow(ov.hwnd, SW_HIDE);
                }
                ov.visible = false;
            }
        }
    }

    /// Destroys every popup — their renderers are bound to a DPI (and a device)
    /// that just changed. The next surface rebuilds what it needs.
    fn drop_popups(&mut self) {
        for ov in self.popups.drain(..) {
            let hwnd = ov.hwnd;
            // The renderer (and its DirectComposition target) is released before
            // the window it targets is destroyed.
            drop(ov);
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}

/// A popup's window procedure. A pass-through popup never sees the pointer; an
/// interactive one reports it to the host that owns it, mapped into the host's
/// CLIENT coordinates — outside `[0, size]` when the surface overflows the
/// window — so the page hit-tests the surface with the very rectangles it placed
/// it with. A press captures the pointer, so a drag that leaves the surface is
/// still reported to it until release.
unsafe extern "system" fn overlay_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        // A floating surface acts for the host window that owns it: that window's thread-local
        // state (`window_tls`) is the one its pointer input goes to.
        let owner = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Host;
        let _state = (!owner.is_null()).then(|| window_tls::enter((*owner).hwnd));
        match msg {
            // Clicking a menu must not take the focus from the window it serves.
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            // The page's cursor applies over an interactive surface too.
            WM_SETCURSOR => {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Host;
                if ptr.is_null() {
                    return DefWindowProcW(hwnd, msg, wparam, lparam);
                }
                SetCursor(Some(load_cursor((*ptr).cursor)));
                LRESULT(1)
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Host;
                if ptr.is_null() {
                    return DefWindowProcW(hwnd, msg, wparam, lparam);
                }
                (*ptr).on_wheel(msg, wparam, lparam);
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Host;
                if !ptr.is_null() {
                    (*ptr).pointer_left();
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_LBUTTONUP | WM_RBUTTONDOWN
            | WM_RBUTTONUP | WM_MBUTTONDOWN | WM_MBUTTONUP => {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Host;
                if ptr.is_null() {
                    return DefWindowProcW(hwnd, msg, wparam, lparam);
                }
                let host = &mut *ptr;
                let mut p = POINT {
                    x: (lparam.0 & 0xFFFF) as i16 as i32,
                    y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
                };
                let _ = ClientToScreen(hwnd, &mut p);
                let screen = p;
                let _ = ScreenToClient(host.hwnd, &mut p);
                host.mouse = (host.dip(p.x as f32), host.dip(p.y as f32));
                // Leaving the surface for nowhere clears the hover (see
                // `Host::pointer_left`). Re-armed on every move: cheap, and a
                // popup is reused for different surfaces.
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
                match msg {
                    WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                        host.register_press(screen);
                        host.mouse_down = true;
                        host.left_press_latch = true;
                        SetCapture(hwnd);
                    }
                    WM_LBUTTONUP => {
                        host.mouse_down = false;
                        let _ = ReleaseCapture();
                    }
                    WM_RBUTTONDOWN => {
                        host.right_down = true;
                        host.right_press_latch = true;
                    }
                    WM_RBUTTONUP => host.right_down = false,
                    WM_MBUTTONDOWN => {
                        host.middle_down = true;
                        host.middle_press_latch = true;
                    }
                    WM_MBUTTONUP => host.middle_down = false,
                    _ => {}
                }
                host.invalidate();
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// The system cursor for a [`Cursor`]. The shared system cursors need no
/// cleanup; a failed load falls back to the null cursor, which Windows treats
/// as "no change of shape" rather than a crash.
fn load_cursor(c: Cursor) -> HCURSOR {
    let id = match c {
        Cursor::Arrow => IDC_ARROW,
        Cursor::IBeam => IDC_IBEAM,
        Cursor::Hand => IDC_HAND,
        Cursor::ResizeEW => IDC_SIZEWE,
        Cursor::ResizeNS => IDC_SIZENS,
        Cursor::ResizeNWSE => IDC_SIZENWSE,
        Cursor::ResizeNESW => IDC_SIZENESW,
        Cursor::Move => IDC_SIZEALL,
        Cursor::NotAllowed => IDC_NO,
        Cursor::Wait => IDC_WAIT,
        Cursor::Crosshair => IDC_CROSS,
    };
    unsafe { LoadCursorW(None, id).unwrap_or_default() }
}

/// `WM_KEYDOWN`/`WM_KEYUP`/`WM_SYSKEYDOWN`/`WM_SYSKEYUP` → key events for the
/// next frame. The repeat count packed in `lparam` (several auto-repeats
/// coalesced by a slow message loop) becomes that many events.
fn queue_key(msg: u32, wparam: WPARAM, lparam: LPARAM) {
    let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let was_down = (lparam.0 >> 30) & 1 == 1;
    let count = if down { (lparam.0 & 0xFFFF).clamp(1, 32) } else { 1 };
    let mods = Modifiers::current();
    let vk = (wparam.0 & 0xFFFF) as u16;
    for i in 0..count {
        input::push(InputEvent::Key { vk, down, repeat: down && (was_down || i > 0), mods });
    }
}

/// The client area's top-left corner in screen DIP — `(0, 0)` client mapped to
/// the screen, divided back to DIP so a page can add its DIP layout to it.
fn client_origin_dip(hwnd: HWND, scale: f32) -> (f32, f32) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe {
        let _ = ClientToScreen(hwnd, &mut p);
    }
    (p.x as f32 / scale, p.y as f32 / scale)
}

/// Where a host window is on screen, in its own DIP (screen pixels divided by the window's DPI
/// scale): what a popup anchored to one of its controls places itself with (`kubuno::popup`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenGeometry {
    /// The window's DPI scale (`1.75` at 168 DPI).
    pub scale: f32,
    /// The client area's top-left corner, in screen DIP.
    pub client_origin: (f32, f32),
    /// The client area's size, in DIP.
    pub client_size: (f32, f32),
    /// The work area of the monitor the window is on (the screen less the taskbar), in screen
    /// DIP: `(left, top, right, bottom)`.
    pub work_area: (f32, f32, f32, f32),
}

/// The screen geometry of the host window `hwnd` (a raw `HWND` value), `None` for a handle that is
/// not a live window.
pub fn screen_geometry(hwnd: isize) -> Option<ScreenGeometry> {
    let hwnd = HWND(hwnd as *mut core::ffi::c_void);
    // SAFETY: plain window queries; a stale handle makes them fail (checked).
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return None;
        }
        let dpi = GetDpiForWindow(hwnd);
        let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        // The page of a floating panel or a host-rounded window starts one shadow margin in.
        let inset = page_inset(hwnd) as f32 / scale;
        let client = ((rc.right - rc.left) as f32 / scale - 2.0 * inset, (rc.bottom - rc.top) as f32 / scale - 2.0 * inset);
        let (ox, oy) = client_origin_dip(hwnd, scale);
        Some(ScreenGeometry { scale, client_origin: (ox + inset, oy + inset), client_size: client, work_area: work_area_dip(hwnd, scale, client) })
    }
}

/// The work area of the monitor the window is on, in screen DIP. Falls back to
/// a rectangle at the origin of the given client size if the monitor cannot be
/// read — a floating surface then simply clamps to the window instead.
fn work_area_dip(hwnd: HWND, scale: f32, client: (f32, f32)) -> (f32, f32, f32, f32) {
    unsafe {
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(mon, &mut mi).as_bool() {
            let r = mi.rcWork;
            return (
                r.left as f32 / scale,
                r.top as f32 / scale,
                r.right as f32 / scale,
                r.bottom as f32 / scale,
            );
        }
    }
    (0.0, 0.0, client.0, client.1)
}

/// The client area in physical pixels, floored to 1 so a minimised window
/// never asks the swap chain for a zero-sized buffer.
fn client_px(hwnd: HWND) -> (u32, u32) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut r);
    }
    (((r.right - r.left).max(1)) as u32, ((r.bottom - r.top).max(1)) as u32)
}

/// The resize band's thickness in DIP for a custom-framed window, or `None`
/// while it is maximised (no resize border then).
fn resize_border_dip(hwnd: HWND, scale: f32) -> Option<f32> {
    // SAFETY: read-only queries on the host's own live window.
    unsafe {
        if IsZoomed(hwnd).as_bool() {
            return None;
        }
        let dpi = GetDpiForWindow(hwnd);
        Some(
            (GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)) as f32
                / scale,
        )
    }
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            // The new window's own thread-local state (`window_tls`) from its first message.
            let _state = window_tls::enter(hwnd);
            let cs = lparam.0 as *const CREATESTRUCTW;
            let host = (*cs).lpCreateParams as *mut Host;
            (*host).hwnd = hwnd;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, host as isize);
            // Known from here on, so `main_window()` already answers in a
            // `WM_CREATE` handler.
            input::set_main_hwnd(hwnd);
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Host;
        if ptr.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        // Several host windows may share this thread (a dialog, another form): the thread-local
        // state this message reads and writes is this window's until the procedure returns.
        let _state = window_tls::enter(hwnd);

        // ── Caller hooks ────────────────────────────────────────────────────
        // Run BEFORE the `&mut Host` below exists: a handler may call back
        // into the host (show/hide the window, which re-enters this procedure;
        // `set_theme`; `quit`) and must not find the host borrowed.
        let args = MessageArgs { hwnd, msg, wparam, lparam };
        if let Some(result) = dispatch_handlers(&args) {
            let _ = InvalidateRect(Some(hwnd), None, false);
            if let Some(r) = result {
                return r;
            }
        }
        // A drag this window asked for (`dnd::do_drag_drop`): OLE's modal drag loop runs here, with no
        // reference to the host held, because the window's own drop target renders frames (through
        // the host) while it runs.
        if msg == WM_KUBUNO_DRAG {
            if let Some(req) = dnd::take_start() {
                let data = std::rc::Rc::new(req.data);
                dnd::set_internal(Some(data.clone()));
                let effect = dnd::ole::run_drag(&data, req.allowed);
                dnd::set_internal(None);
                // The button went up inside OLE's loop: this window never saw the release.
                (*ptr).mouse_down = GetKeyState(i32::from(VK_LBUTTON.0)) < 0;
                (*ptr).left_press_latch = false;
                let _ = ReleaseCapture();
                (req.done)(effect);
                (*ptr).request_frame();
            }
            return LRESULT(0);
        }
        // Visual Studio's *Debug › Kubuno › Paint debug* (or any tool): toggles the overlay.
        if msg == paint_debug::message() && msg != 0 {
            paint_debug::set_flags(paint_debug::PaintDebugFlags(wparam.0 as u32));
            let _ = InvalidateRect(Some(hwnd), None, false);
            return LRESULT(0);
        }
        if msg == WM_CLOSE {
            // The close handler decides; `false` cancels (a tray app hides
            // instead). Proceeding is DefWindowProc's `DestroyWindow`.
            if !close_allowed() {
                return LRESULT(0);
            }
            // A page that handles closing itself (`defer_close`, a view runtime
            // raising FormClosing) gets the request as an input event of the
            // next frame, and closes with `quit` - or not.
            if input::close_deferred() && !QUIT_REQUESTED.with(|q| q.get()) {
                input::queue_close_request(CloseReason::from_wparam(wparam.0));
                (*ptr).request_frame();
                return LRESULT(0);
            }
            // A modal window enables the windows it disabled BEFORE it goes, so the activation
            // returns to its owner rather than to another application.
            (*ptr).enable_others();
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }

        let host = &mut *ptr;
        // WM_MOUSEMOVE packs signed 16-bit client coordinates into `lparam`.
        let x = || (lparam.0 & 0xFFFF) as i16 as f32;
        let y = || ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;

        match msg {
            WM_CREATE => {
                host.init(hwnd);
                LRESULT(0)
            }
            WM_SIZE => {
                host.resize();
                LRESULT(0)
            }
            WM_KUBUNO_RESIZE_PAGE if host.parent.is_none() => {
                let scale = host.scale();
                let margin = host.panel_margin() * 2.0;
                let page = (wparam.0 as f32 / 100.0, lparam.0 as f32 / 100.0);
                let (mut w, mut h) = (((page.0 + margin) * scale).round() as i32, ((page.1 + margin) * scale).round() as i32);
                if host.client_size {
                    let (dw, dh) = host.non_client_px();
                    w += dw;
                    h += dh;
                }
                let _ = SetWindowPos(hwnd, None, 0, 0, w.max(1), h.max(1), SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let old = host.dpi as u32;
                host.dpi = (wparam.0 & 0xFFFF) as f32;
                if old != host.dpi as u32 {
                    chrome::push_event(chrome::WindowEvent::DpiChanged { old, new: host.dpi as u32 });
                }
                // The popups carry renderers bound to the old DPI; drop them so
                // the next floating surfaces rebuild at the new scale.
                host.drop_popups();
                // The metrics and the UI font are both read per DPI, so they
                // are re-read here — before the resize, so the very next paint
                // already measures against the new display.
                host.rebuild_visuals();
                // Windows suggests a new window rect (in `lparam`) that keeps the
                // window the same logical size across the DPI change; honour it,
                // then resize the swap chain to the new client size.
                let rc = &*(lparam.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    rc.left,
                    rc.top,
                    rc.right - rc.left,
                    rc.bottom - rc.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                host.resize();
                LRESULT(0)
            }
            // Embedded: a child gets no `WM_DPICHANGED`. The top-level parent
            // does, resizes it, and the child hears about it afterwards. Same
            // rebuild as above, minus the window rect (the parent sets it).
            WM_DPICHANGED_AFTERPARENT => {
                host.dpi = GetDpiForWindow(hwnd) as f32;
                host.drop_popups();
                host.rebuild_visuals();
                host.resize();
                LRESULT(0)
            }
            // The user switched visual style, toggled high contrast, or logged
            // a policy change in. Every theme handle now points at an msstyles
            // file that is gone, and every cached part was drawn from it — so
            // both are dropped here. The SYSTEM colours move at the same moment
            // (high contrast rewrites the whole `COLOR_*` table), so the
            // classic fallback is re-read too; without that, turning high
            // contrast on would repaint the themed halves and leave the classic
            // ones in the old palette.
            WM_THEMECHANGED | WM_SYSCOLORCHANGE => {
                host.parts.on_theme_changed();
                host.rebuild_visuals();
                host.invalidate();
                LRESULT(0)
            }
            WM_PAINT => {
                host.render();
                let _ = ValidateRect(Some(hwnd), None);
                // A theme or font asked for DURING this paint was invalidated
                // before the validation above cancelled it: ask again.
                if has_pending_changes() {
                    host.invalidate();
                }
                LRESULT(0)
            }
            // The client area is fully repainted every frame, so eating the
            // erase avoids a flash of the background brush before the paint.
            WM_ERASEBKGND => LRESULT(1),
            WM_MOUSEMOVE => {
                // Repaint on every move so hover states track the pointer — the
                // whole reason the host forwards the mouse at all.
                host.mouse = (host.dip(x()), host.dip(y()));
                host.track_leave();
                host.invalidate();
                LRESULT(0)
            }
            // A double-click's second press arrives as WM_LBUTTONDBLCLK (the
            // class has CS_DBLCLKS); to the page it is a press like any other,
            // told apart by `Frame::click_count`.
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                // A child window is not focused by a click on its own; the
                // keyboard must follow the pointer into the embedded surface.
                host.take_focus_if_embedded();
                host.mouse = (host.dip(x()), host.dip(y()));
                let mut p = POINT { x: x() as i32, y: y() as i32 };
                let _ = ClientToScreen(hwnd, &mut p);
                host.register_press(p);
                host.mouse_down = true;
                host.left_press_latch = true;
                // A drag that leaves the window keeps reporting to it (a
                // splitter, a slider, a text selection) until release.
                SetCapture(hwnd);
                host.invalidate();
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                host.mouse = (host.dip(x()), host.dip(y()));
                host.mouse_down = false;
                let _ = ReleaseCapture();
                // Released outside the window: re-arm the leave tracking,
                // which then fires at once and clears the hover.
                host.tracking_leave = false;
                host.track_leave();
                host.invalidate();
                LRESULT(0)
            }
            // Capture taken away (Alt+Tab mid-drag, a system dialog): the
            // release will never come, so the button is up.
            WM_CAPTURECHANGED => {
                if host.mouse_down && lparam.0 != hwnd.0 as isize {
                    host.mouse_down = false;
                    host.invalidate();
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                host.tracking_leave = false;
                host.pointer_left();
                LRESULT(0)
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                host.on_wheel(msg, wparam, lparam);
                LRESULT(0)
            }
            WM_SETCURSOR if (lparam.0 & 0xFFFF) as u32 == HTCLIENT => {
                SetCursor(Some(load_cursor(host.cursor)));
                LRESULT(1)
            }
            // ── Keyboard ────────────────────────────────────────────────────
            // Keys go to this window even while a popup is open (popups never
            // take the focus), so an open menu reads its arrows here too.
            WM_KEYDOWN | WM_KEYUP => {
                if host.parent.is_some() && msg == WM_KEYDOWN {
                    host.record_forward_candidate(msg, wparam, lparam);
                }
                queue_key(msg, wparam, lparam);
                host.invalidate();
                LRESULT(0)
            }
            // Reported, then left to Windows: Alt+F4, Alt+Space and friends
            // must keep working.
            WM_SYSKEYDOWN | WM_SYSKEYUP => {
                if host.parent.is_some() && msg == WM_SYSKEYDOWN {
                    host.record_forward_candidate(msg, wparam, lparam);
                }
                queue_key(msg, wparam, lparam);
                host.invalidate();
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_CHAR => {
                let unit = (wparam.0 & 0xFFFF) as u16;
                let ch = match unit {
                    0xD800..=0xDBFF => {
                        host.high_surrogate = Some(unit);
                        None
                    }
                    0xDC00..=0xDFFF => host.high_surrogate.take().and_then(|hi| {
                        char::decode_utf16([hi, unit]).next().and_then(|r| r.ok())
                    }),
                    _ => {
                        host.high_surrogate = None;
                        char::from_u32(unit as u32)
                    }
                };
                // Control characters (Backspace, Tab, Enter, Escape,
                // Ctrl+letter) are keys, not text.
                if let Some(c) = ch.filter(|c| !c.is_control()) {
                    input::push(InputEvent::Text(c.to_string()));
                    host.invalidate();
                }
                LRESULT(0)
            }
            // Alt+letter has no menu bar to open here; letting Windows have it
            // only produces a beep. Alt+Space still opens the system menu.
            WM_SYSCHAR if (wparam.0 & 0xFFFF) as u32 != u32::from(b' ') => LRESULT(0),
            // A lone Alt (or F10) would put the window into menu mode, which
            // then swallows the next keystroke. There is no menu bar.
            WM_SYSCOMMAND if (wparam.0 as u32 & 0xFFF0) == SC_KEYMENU && lparam.0 == 0 => LRESULT(0),
            // ── IME composition ─────────────────────────────────────────────
            // The in-progress string is reported (see `input`), then Windows
            // handles the message as usual: the IME window still shows and
            // the result still arrives through `WM_CHAR`.
            m if m == input::WM_IME_COMPOSITION_MSG => {
                if (lparam.0 as u32) & input::GCS_COMPSTR != 0 {
                    if let Some((text, caret)) = input::read_composition(hwnd) {
                        input::push(InputEvent::Composition { text, caret });
                        host.invalidate();
                    }
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            m if m == input::WM_IME_END_COMPOSITION => {
                input::push(InputEvent::Composition { text: String::new(), caret: 0 });
                host.invalidate();
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            // Screen readers ask for the window's accessibility tree (`access`).
            WM_GETOBJECT => {
                match host.access.as_mut().and_then(|a| a.get_object(wparam, lparam)) {
                    Some(answer) => answer(),
                    None => DefWindowProcW(hwnd, msg, wparam, lparam),
                }
            }
            // `Form.MinimumSize` / `MaximumSize` (`set_form`).
            WM_GETMINMAXINFO if host.parent.is_none() => {
                let info = &mut *(lparam.0 as *mut MINMAXINFO);
                // The shadow margin of a floating panel or a host-rounded window is not the page.
                let (nx, ny) = host.non_client_px();
                let m = (host.panel_margin() * 2.0 * host.scale()).round() as i32;
                form::min_max(info, &host.form, (nx + m, ny + m), host.scale());
                LRESULT(0)
            }
            // Files dropped from the Explorer (`accept_files`): one input event per drop.
            WM_DROPFILES => {
                let drop = wparam.0 as isize;
                let count = DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0);
                let mut files = Vec::new();
                for i in 0..count {
                    let len = DragQueryFileW(drop, i, std::ptr::null_mut(), 0);
                    let mut buf = vec![0u16; len as usize + 1];
                    let got = DragQueryFileW(drop, i, buf.as_mut_ptr(), buf.len() as u32);
                    files.push(String::from_utf16_lossy(&buf[..got as usize]));
                }
                let mut pt = POINT::default();
                DragQueryPoint(drop, &mut pt);
                DragFinish(drop);
                let m = host.panel_margin();
                let (x, y) = (host.dip(pt.x as f32) - m, host.dip(pt.y as f32) - m);
                input::push(InputEvent::FilesDropped { x: x.round() as i32, y: y.round() as i32, files });
                host.request_frame();
                LRESULT(0)
            }
            WM_SETFOCUS | WM_KILLFOCUS => {
                if let Some(access) = host.access.as_mut() {
                    access.focus_changed(msg == WM_SETFOCUS);
                }
                host.window_focused = msg == WM_SETFOCUS;
                // Embedded, losing the focus is the blur (`WM_ACTIVATE` goes
                // to the top-level parent only): open menus close.
                if msg == WM_KILLFOCUS && host.parent.is_some() {
                    host.dismiss_pending = true;
                }
                input::push(InputEvent::WindowFocus(host.window_focused));
                host.invalidate();
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == REPAINT_TIMER_ID => {
                let _ = KillTimer(Some(hwnd), REPAINT_TIMER_ID);
                host.invalidate();
                LRESULT(0)
            }
            // Work, not looks (`request_wake_after`): runs even minimised.
            WM_TIMER if wparam.0 == WAKE_TIMER_ID => {
                let _ = KillTimer(Some(hwnd), WAKE_TIMER_ID);
                host.request_frame();
                LRESULT(0)
            }
            // Another thread posted work for this one (`UiWaker`).
            WM_KUBUNO_WAKE => {
                input::wake_received(hwnd);
                host.request_frame();
                LRESULT(0)
            }
            // The session is ending: a page deferring closes (`defer_close`)
            // runs its FormClosing now, synchronously, and may refuse
            // (`cancel_close`), as a WinForms form can.
            WM_QUERYENDSESSION if input::close_deferred() && !QUIT_REQUESTED.with(|q| q.get()) => {
                input::take_close_cancelled();
                input::queue_close_request(CloseReason::WindowsShutDown);
                host.render();
                LRESULT(if input::take_close_cancelled() { 0 } else { 1 })
            }
            // A window drag or resize ended: one frame, so a page that follows
            // the window's position (a view's Move event) sees where it landed.
            WM_ENTERSIZEMOVE => {
                chrome::push_event(chrome::WindowEvent::ResizeBegin);
                host.invalidate();
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_EXITSIZEMOVE => {
                chrome::push_event(chrome::WindowEvent::ResizeEnd);
                host.invalidate();
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_RBUTTONDOWN | WM_RBUTTONUP => {
                if msg == WM_RBUTTONDOWN {
                    host.take_focus_if_embedded();
                    host.right_press_latch = true;
                }
                host.mouse = (host.dip(x()), host.dip(y()));
                host.right_down = msg == WM_RBUTTONDOWN;
                host.invalidate();
                LRESULT(0)
            }
            WM_MBUTTONDOWN | WM_MBUTTONUP => {
                if msg == WM_MBUTTONDOWN {
                    host.take_focus_if_embedded();
                    host.middle_press_latch = true;
                }
                host.mouse = (host.dip(x()), host.dip(y()));
                host.middle_down = msg == WM_MBUTTONDOWN;
                host.invalidate();
                LRESULT(0)
            }
            // Losing activation — a click on the desktop or another app, which
            // a surface overflowing the window lets the user make without ever
            // touching this window — is the web's blur: the next frame carries
            // `Frame::dismiss` so open menus and bubbles close.
            WM_ACTIVATE => {
                if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
                    host.dismiss_pending = true;
                    host.invalidate();
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_DESTROY => {
                if host.drop_target == Some(true) {
                    // OLE holds the drop target (and through it this window) until revoked.
                    let _ = windows::Win32::System::Ole::RevokeDragDrop(hwnd);
                    host.drop_target = None;
                }
                host.enable_others();
                // Only the main window ends the thread's loop; a dialog's nested loop ends when
                // its window is gone, and a modeless window has no loop of its own.
                if host.role == Role::Main {
                    PostQuitMessage(0);
                }
                LRESULT(0)
            }
            // The last message the window gets: from here `main_window()`
            // answers `None` and the per-window page state is dropped.
            WM_NCDESTROY => {
                input::window_destroyed(hwnd);
                PARENT_HWND.with(|c| c.set(0));
                CAPTION_HOT.with(|c| c.set(None));
                TITLE_BAR.with(|t| *t.borrow_mut() = TitleBar::default());
                window_tls::destroyed(hwnd);
                let result = DefWindowProcW(hwnd, msg, wparam, lparam);
                if host.role == Role::Modeless {
                    // `open_window` gave the host to the window: it goes with it.
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    drop(Box::from_raw(ptr));
                }
                result
            }
            // ── Kubuno / Custom chrome: strip the system caption, route the buttons ──
            // A host-rounded window (`frame`) is all client area: its resize band lies in its own
            // shadow margin. Maximised, Windows pushes the (invisible) frame of its style off-screen:
            // taken back in, so the page fills the work area exactly.
            WM_NCCALCSIZE if host.rounded.is_some() && host.parent.is_none() && wparam.0 != 0 => {
                if IsZoomed(hwnd).as_bool() {
                    let rc = &mut *(lparam.0 as *mut RECT);
                    let dpi = GetDpiForWindow(hwnd);
                    let fx = GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                    let fy = GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                    rc.left += fx;
                    rc.right -= fx;
                    rc.top += fy;
                    rc.bottom -= fy;
                }
                LRESULT(0)
            }
            // `FormBorderStyle = None` under the Kubuno chrome: the whole window is client area.
            WM_NCCALCSIZE if host.chrome == Chrome::Kubuno && !host.form.border_style.has_caption() && wparam.0 != 0 => {
                // A borderless window is all client area. Maximised, Windows still pushes a resizable
                // window's (invisible) frame off-screen: take it back in so nothing is clipped.
                if IsZoomed(hwnd).as_bool() && host.form.resize_border {
                    let rc = &mut *(lparam.0 as *mut RECT);
                    let dpi = GetDpiForWindow(hwnd);
                    let fx = GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                    let fy = GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                    rc.left += fx;
                    rc.right -= fx;
                    rc.top += fy;
                    rc.bottom -= fy;
                }
                LRESULT(0)
            }
            WM_NCCALCSIZE if host.chrome != Chrome::System && wparam.0 != 0 => {
                // Same technique the shell and documents use: keep the resize
                // borders but hand the caption strip back to the client area.
                let rc = &mut *(lparam.0 as *mut RECT);
                let dpi = GetDpiForWindow(hwnd);
                let frame_x = GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi)
                    + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                let frame_y = GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi)
                    + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                rc.left += frame_x;
                rc.right -= frame_x;
                rc.bottom -= frame_y;
                // Maximised, Windows pushes the frame off-screen; the top has
                // to come back in or the header would be clipped.
                if IsZoomed(hwnd).as_bool() {
                    rc.top += frame_y;
                }
                LRESULT(0)
            }
            // A floating panel: its shadow margin is not the panel — a press there goes to the
            // window under it (which, taking the activation, closes a flyout), as on the web.
            WM_NCHITTEST if host.form.panel.is_some() && host.panel_margin() > 0.0 => {
                let mut pt = POINT { x: (lparam.0 & 0xFFFF) as i16 as i32,
                                     y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32 };
                let _ = ScreenToClient(hwnd, &mut pt);
                let scale = host.scale();
                let (x, y) = (pt.x as f32 / scale, pt.y as f32 / scale);
                let (cw, ch) = client_px(hwnd);
                let m = host.panel_margin();
                let inside = x >= m && y >= m && x < cw as f32 / scale - m && y < ch as f32 / scale - m;
                LRESULT(if inside { HTCLIENT as isize } else { HTTRANSPARENT as isize })
            }
            WM_NCHITTEST if host.chrome == Chrome::Kubuno => {
                let mut pt = POINT { x: (lparam.0 & 0xFFFF) as i16 as i32,
                                     y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32 };
                let _ = ScreenToClient(hwnd, &mut pt);
                let scale = host.scale();
                let (x, y) = (pt.x as f32 / scale, pt.y as f32 / scale);
                let (cw, ch) = client_px(hwnd);
                let size = (cw as f32 / scale, ch as f32 / scale);
                // The band (`crate::window_chrome`), the resize borders (never while maximised, nor
                // for a fixed border), the grip, the page's drag areas and holes — see
                // `chrome::hit_test` for the order. A borderless window has no band: only its drag
                // areas (`TitleBar.Drag`) move it, and it resizes by its edges with `resize_border`.
                let border = if host.form.resizable() { resize_border_dip(hwnd, scale) } else { None };
                // A host-rounded window: its curve's resize band and its shadow margin first, the
                // rest in the frame's coordinates.
                let ((x, y), size) = match host.frame_point(x, y, size, border) {
                    Ok(point) => point,
                    Err(ht) => return ht,
                };
                let layout = host.kubuno_layout(size);
                let grip = host.grip_rect(size);
                let drag = chrome::drag_areas();
                let ht = TITLE_BAR.with(|t| chrome::hit_test(layout.as_ref(), &t.borrow(), &drag, size, x, y, border, grip));
                LRESULT(ht as isize)
            }
            // Custom chrome: the page's own title bar, as it declared it this
            // frame (see `caption::custom_hit_test` for the order).
            WM_NCHITTEST if host.chrome == Chrome::Custom => {
                let mut pt = POINT { x: (lparam.0 & 0xFFFF) as i16 as i32,
                                     y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32 };
                let _ = ScreenToClient(hwnd, &mut pt);
                let scale = host.scale();
                let (x, y) = (pt.x as f32 / scale, pt.y as f32 / scale);
                let (cw, ch) = client_px(hwnd);
                let size = (cw as f32 / scale, ch as f32 / scale);
                let border = resize_border_dip(hwnd, scale);
                let ((x, y), size) = match host.frame_point(x, y, size, border) {
                    Ok(point) => point,
                    Err(ht) => return ht,
                };
                let ht = TITLE_BAR.with(|t| caption::custom_hit_test(&t.borrow(), size, x, y, border));
                LRESULT(ht as isize)
            }
            // The caption buttons live in the non-client area (Windows 11 pops
            // its snap flyout on hover of maximise), so their hover arrives
            // here rather than as a WM_MOUSEMOVE.
            WM_NCMOUSEMOVE if host.chrome != Chrome::System => {
                let ht = wparam.0 as u32;
                let next = match ht {
                    HTMINBUTTON => Some(CaptionHot::Min),
                    HTMAXBUTTON => Some(CaptionHot::Max),
                    HTCLOSE => Some(CaptionHot::Close),
                    _ => None,
                };
                let mut changed = host.set_caption_hot(next);
                if host.chrome == Chrome::Kubuno {
                    let part = host.nc_part(ht, lparam);
                    changed |= host.set_chrome_hot(part);
                    if part.is_none() && host.chrome_pressed.take().is_some() {
                        changed = true;
                    }
                }
                if changed {
                    host.invalidate();
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            // Any leave from the non-client area drops the hover.
            WM_NCMOUSELEAVE if host.chrome != Chrome::System => {
                let mut changed = host.set_caption_hot(None);
                changed |= host.set_chrome_hot(None);
                changed |= host.chrome_pressed.take().is_some();
                if changed {
                    host.invalidate();
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            // The press on a caption button is eaten: left to DefWindowProc it
            // starts the classic caption-button tracking loop, which swallows
            // the release (so WM_NCLBUTTONUP below never comes) and draws the
            // old system buttons over ours. The release does the work; the
            // Kubuno band shows the button pressed meanwhile.
            WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK
                if host.chrome != Chrome::System
                    && matches!(wparam.0 as u32, HTMINBUTTON | HTMAXBUTTON | HTCLOSE | HTHELP | HTOBJECT) =>
            {
                if host.chrome == Chrome::Kubuno {
                    host.chrome_pressed = host.nc_part(wparam.0 as u32, lparam);
                    host.invalidate();
                }
                LRESULT(0)
            }
            // A double-click on the band: `TitleBarDoubleClick`, then Windows maximises or
            // restores the window as usual.
            WM_NCLBUTTONDBLCLK if host.chrome == Chrome::Kubuno && wparam.0 as u32 == HTCAPTION => {
                chrome::push_event(chrome::WindowEvent::TitleBarDoubleClick);
                host.request_frame();
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            // A click on a caption button arrives as WM_NCLBUTTONUP with the same HT
            // code we returned from WM_NCHITTEST: min / max / close become the
            // system command (max too, so the snap-layouts flyout keeps working on
            // hover), help and the window's own buttons become window events.
            WM_NCLBUTTONUP if host.chrome != Chrome::System => {
                let ht = wparam.0 as u32;
                if host.chrome == Chrome::Kubuno {
                    let part = host.nc_part(ht, lparam);
                    let pressed = host.chrome_pressed.take();
                    host.invalidate();
                    // Released on another button than the one pressed: nothing happens.
                    if part.is_some() && pressed != part {
                        return LRESULT(0);
                    }
                    match part {
                        Some(crate::window_chrome::Part::Help) => {
                            chrome::push_event(chrome::WindowEvent::HelpButtonClicked);
                            host.request_frame();
                            return LRESULT(0);
                        }
                        Some(crate::window_chrome::Part::Command(i)) => {
                            if let Some(c) = host.form.chrome.commands.get(i) {
                                chrome::push_event(chrome::WindowEvent::CaptionButtonClick(c.id.clone()));
                                host.request_frame();
                            }
                            return LRESULT(0);
                        }
                        _ => {}
                    }
                }
                let cmd = match ht {
                    HTMINBUTTON => Some(SC_MINIMIZE),
                    HTCLOSE => Some(SC_CLOSE),
                    HTMAXBUTTON => Some(if IsZoomed(hwnd).as_bool() { SC_RESTORE } else { SC_MAXIMIZE }),
                    _ => None,
                };
                if let Some(cmd) = cmd {
                    let _ = SendMessageW(hwnd, WM_SYSCOMMAND, Some(WPARAM(cmd as usize)), Some(LPARAM(0)));
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

// Declared directly (raw-dylib), like `diagnostics`' kernel32 calls, so the crate's `windows` feature
// set does not grow for four shell functions.
#[link(name = "shell32", kind = "raw-dylib")]
extern "system" {
    fn DragAcceptFiles(hwnd: HWND, accept: i32);
    fn DragQueryFileW(drop: isize, index: u32, file: *mut u16, len: u32) -> u32;
    fn DragQueryPoint(drop: isize, point: *mut POINT) -> i32;
    fn DragFinish(drop: isize);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(msg: u32) -> MessageArgs {
        MessageArgs { hwnd: HWND::default(), msg, wparam: WPARAM(0), lparam: LPARAM(0) }
    }

    // ── Button edge latching (`consume_button_edge`) ───────────────────────
    // See the function's own doc: a frame must never lose a press+release
    // that completed entirely between two paints.

    #[test]
    fn a_quick_click_still_reports_one_frame_of_down() {
        // WM_*BUTTONDOWN sets the latch; WM_*BUTTONUP (simulated here by
        // dropping the raw level to false) never touches it — exactly what
        // happens when both messages are processed before the next WM_PAINT.
        let down = true;
        let mut latch = true;
        // The button is already physically up again (`down = false` would be
        // the raw level after WM_*BUTTONUP), but the latch alone must still
        // report a press for this one frame.
        let raw_after_release = false;
        assert!(consume_button_edge(down, &mut latch));
        // A second frame with nothing new must not still report down: the
        // latch was consumed, and the raw level is (by then) false.
        assert!(!consume_button_edge(raw_after_release, &mut latch));
    }

    #[test]
    fn a_held_button_keeps_reporting_down_across_frames() {
        let mut latch = true; // set by the WM_*BUTTONDOWN that started the hold
        assert!(consume_button_edge(true, &mut latch));
        // Still held on the next several frames, latch already consumed:
        // must keep reading true from the raw level alone.
        assert!(consume_button_edge(true, &mut latch));
        assert!(consume_button_edge(true, &mut latch));
        // Released for real: the very next frame sees the release edge.
        assert!(!consume_button_edge(false, &mut latch));
    }

    #[test]
    fn no_press_never_latches_a_click_out_of_nothing() {
        let mut latch = false;
        assert!(!consume_button_edge(false, &mut latch));
        assert!(!latch);
    }

    #[test]
    fn the_latch_is_cleared_even_when_the_button_is_still_down() {
        // Consuming the latch must not leave it set for a THIRD frame just
        // because the button happens to still be held on the second one.
        let mut latch = true;
        let _ = consume_button_edge(true, &mut latch);
        assert!(!latch);
    }

    #[test]
    fn unregistered_message_reports_none() {
        assert!(dispatch_handlers(&args(WM_APP + 0x700)).is_none());
    }

    #[test]
    fn first_some_wins_and_order_is_kept() {
        let msg = WM_APP + 0x701;
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let (a, b, c) = (log.clone(), log.clone(), log.clone());
        on_message(msg, move |_| {
            a.borrow_mut().push(1);
            None
        });
        on_message(msg, move |_| {
            b.borrow_mut().push(2);
            Some(LRESULT(42))
        });
        on_message(msg, move |_| {
            c.borrow_mut().push(3);
            None
        });
        let r = dispatch_handlers(&args(msg));
        assert_eq!(r.flatten().map(|r| r.0), Some(42));
        assert_eq!(*log.borrow(), vec![1, 2]);
    }

    #[test]
    fn handler_may_reenter_host_apis() {
        let msg = WM_APP + 0x702;
        let hits = std::rc::Rc::new(std::cell::Cell::new(0));
        let h = hits.clone();
        on_message(msg, move |a| {
            // Registering, dispatching and queueing changes from inside a
            // handler must not meet a live borrow.
            // Re-entrant dispatch of the same message skips the running
            // handlers.
            if h.get() == 0 {
                assert!(dispatch_handlers(a).is_none());
                let h2 = h.clone();
                on_message(a.msg, move |_| {
                    h2.set(h2.get() + 10);
                    None
                });
            }
            set_font_override(None);
            h.set(h.get() + 1);
            None
        });
        assert_eq!(dispatch_handlers(&args(msg)), Some(None));
        assert_eq!(hits.get(), 1);
        // The handler added during the first dispatch runs on the second.
        dispatch_handlers(&args(msg));
        assert_eq!(hits.get(), 1 + 1 + 10);
        PENDING_FONT.with(|f| *f.borrow_mut() = None);
    }

    #[test]
    fn close_handler_decides_unless_quitting() {
        assert!(close_allowed(), "no handler: close proceeds");
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let c = calls.clone();
        set_close_handler(move || {
            c.set(c.get() + 1);
            false
        });
        assert!(!close_allowed());
        assert!(!close_allowed(), "the handler is kept across calls");
        assert_eq!(calls.get(), 2);
        QUIT_REQUESTED.with(|q| q.set(true));
        assert!(close_allowed(), "quit bypasses the handler");
        assert_eq!(calls.get(), 2);
        QUIT_REQUESTED.with(|q| q.set(false));
        // A handler replacing itself while running keeps the replacement.
        set_close_handler(|| {
            set_close_handler(|| true);
            false
        });
        assert!(!close_allowed());
        assert!(close_allowed());
        CLOSE_HANDLER.with(|c| *c.borrow_mut() = None);
    }

    #[test]
    fn backdrop_values() {
        assert_eq!(Backdrop::None.dwm_value(), None);
        assert_eq!(Backdrop::Mica.dwm_value(), Some(2));
        assert_eq!(Backdrop::MicaAlt.dwm_value(), Some(4));
    }
}
