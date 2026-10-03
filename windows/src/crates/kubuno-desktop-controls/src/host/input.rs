//! Keyboard, wheel, cursor, clipboard and timer services of the host.
//!
//! [`super::Frame`] must stay `Copy` (every page takes it by reference and
//! several store it), so the variable-length part of a frame's input — the
//! keys pressed and the text typed since the previous frame — does not travel
//! in it. It sits in a thread-local queue instead, filled by the window
//! procedure and handed over at the start of each frame:
//!
//! ```text
//!   WM_KEYDOWN / WM_CHAR …  ──►  PENDING  ──(frame starts)──►  FRAME  ──►  events()
//!                                                                 └──►  take_key() / take_text()
//! ```
//!
//! Everything here is meant to be called **from inside the paint closure**
//! (the host is single-threaded; the queue is the UI thread's). Outside a frame
//! the calls are harmless: [`events`] returns what the last frame saw, the
//! setters are applied after the next frame.
//!
//! ## Consuming
//!
//! A frame's events can be *consumed*: [`take_key`], [`take_text`] and
//! [`consume`] mark what they return as handled, and [`events`] only lists what
//! nobody consumed yet. This is how layers cooperate without knowing each
//! other: the gallery shell takes `Ctrl+Tab` before the page runs, a focus
//! manager takes `Tab`, an open menu takes the arrows and `Escape`, and a text
//! field takes whatever text is left — the web's `preventDefault` +
//! `stopPropagation`, in paint order.
//!
//! ## IME composition
//!
//! While an input method composes (Japanese, Chinese, Korean…), the host
//! reports the in-progress string as [`InputEvent::Composition`] — `text` and
//! the IME's `caret` in it — every time it changes, and an empty `text` when
//! the composition ends or is cancelled. [`composition`] peeks at the latest
//! state of the frame. The committed result still arrives as ordinary
//! [`InputEvent::Text`], and the IME's own composition window still shows, so
//! an editor that ignores composition keeps working; one that wants the web's
//! inline look draws `text` at its caret, underlined, until the next update.

use std::cell::{Cell, RefCell};
use std::sync::OnceLock;
use std::time::Instant;

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

/// Virtual-key codes a page matches [`InputEvent::Key::vk`] against.
///
/// The values are Win32's `VK_*`; they are restated here as plain `u16` so a
/// page never needs the `windows` crate to read a key. Letters and digits are
/// their ASCII upper-case code (`b'A' as u16`, `b'0' as u16`) — see
/// [`vk::letter`] and [`vk::digit`].
pub mod vk {
    pub const BACK: u16 = 0x08;
    pub const TAB: u16 = 0x09;
    pub const ENTER: u16 = 0x0D;
    pub const SHIFT: u16 = 0x10;
    pub const CONTROL: u16 = 0x11;
    /// Alt.
    pub const MENU: u16 = 0x12;
    pub const PAUSE: u16 = 0x13;
    pub const CAPITAL: u16 = 0x14;
    pub const ESCAPE: u16 = 0x1B;
    pub const SPACE: u16 = 0x20;
    pub const PAGE_UP: u16 = 0x21;
    pub const PAGE_DOWN: u16 = 0x22;
    pub const END: u16 = 0x23;
    pub const HOME: u16 = 0x24;
    pub const LEFT: u16 = 0x25;
    pub const UP: u16 = 0x26;
    pub const RIGHT: u16 = 0x27;
    pub const DOWN: u16 = 0x28;
    pub const INSERT: u16 = 0x2D;
    pub const DELETE: u16 = 0x2E;
    pub const LWIN: u16 = 0x5B;
    pub const RWIN: u16 = 0x5C;
    /// The context-menu key (`VK_APPS`) — opens a context menu like Shift+F10.
    pub const APPS: u16 = 0x5D;
    pub const F1: u16 = 0x70;
    pub const F2: u16 = 0x71;
    pub const F3: u16 = 0x72;
    pub const F4: u16 = 0x73;
    pub const F5: u16 = 0x74;
    pub const F6: u16 = 0x75;
    pub const F7: u16 = 0x76;
    pub const F8: u16 = 0x77;
    pub const F9: u16 = 0x78;
    pub const F10: u16 = 0x79;
    pub const F11: u16 = 0x7A;
    pub const F12: u16 = 0x7B;

    /// The key of an ASCII letter, either case: `letter('a') == letter('A') == 0x41`.
    /// Any other character maps to `0`, which no key carries.
    pub const fn letter(c: char) -> u16 {
        let c = c.to_ascii_uppercase();
        if c.is_ascii_uppercase() {
            c as u16
        } else {
            0
        }
    }

    /// The key of a top-row digit `0..=9` (not the numeric keypad).
    pub const fn digit(d: u8) -> u16 {
        if d <= 9 {
            b'0' as u16 + d as u16
        } else {
            0
        }
    }
}

/// The modifier keys held when an event happened (or, in
/// [`super::Frame::mods`], when the frame started).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// The Windows key. Reported, but ignored by [`Modifiers::matches`]: the
    /// shell reserves nearly every Win+key chord for itself.
    pub meta: bool,
}

impl Modifiers {
    pub const NONE: Self = Self { ctrl: false, shift: false, alt: false, meta: false };
    pub const CTRL: Self = Self { ctrl: true, ..Self::NONE };
    pub const SHIFT: Self = Self { shift: true, ..Self::NONE };
    pub const ALT: Self = Self { alt: true, ..Self::NONE };
    pub const CTRL_SHIFT: Self = Self { ctrl: true, shift: true, ..Self::NONE };

    /// No Ctrl, Shift or Alt (the Windows key is ignored).
    pub fn is_none(self) -> bool {
        !self.ctrl && !self.shift && !self.alt
    }

    /// The web's "command" modifier on this platform — Ctrl on Windows. What a
    /// Ctrl+C / Ctrl+Z shortcut tests, so the intent reads in the code.
    pub fn command(self) -> bool {
        self.ctrl
    }

    /// Same Ctrl, Shift and Alt as `other` (the Windows key is ignored). This
    /// is how [`take_key`] matches a chord: `Tab` is not `Shift+Tab`.
    pub fn matches(self, other: Modifiers) -> bool {
        self.ctrl == other.ctrl && self.shift == other.shift && self.alt == other.alt
    }

    /// Reads the modifiers from the keyboard state of the message being
    /// processed (`GetKeyState`, not the asynchronous state).
    pub(crate) fn current() -> Self {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
        };
        // The high bit of `GetKeyState` is "down".
        let down = |k: u16| unsafe { GetKeyState(k as i32) } < 0;
        Self {
            ctrl: down(VK_CONTROL.0),
            shift: down(VK_SHIFT.0),
            alt: down(VK_MENU.0),
            meta: down(VK_LWIN.0) || down(VK_RWIN.0),
        }
    }
}

/// One discrete input event of a frame. Pointer motion, buttons and wheel are
/// NOT here — they are state, carried by [`super::Frame`].
///
/// `#[non_exhaustive]`: match with a trailing `_ => {}` arm, new kinds of
/// events may be added (IME composition was).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InputEvent {
    /// A key went down (`down == true`, possibly auto-`repeat`ed while held) or
    /// up. `vk` is a [`vk`] code. Delivered for every key, the ones that also
    /// produce text included: `A` gives a `Key{vk: letter('A')}` then a
    /// `Text("a")`. System keys (Alt chords, F10) are reported too, and still
    /// reach Windows afterwards so Alt+F4 and Alt+Space keep working.
    Key { vk: u16, down: bool, repeat: bool, mods: Modifiers },
    /// Text typed — one or more characters, already combined (surrogate pairs,
    /// dead keys, AltGr, IME results). Control characters (Backspace, Tab,
    /// Enter, Escape, Ctrl+letter) are never text: read them as [`Self::Key`].
    Text(String),
    /// The window gained (`true`) or lost (`false`) the keyboard focus. A
    /// caret stops blinking and hides while the window is not focused.
    WindowFocus(bool),
    /// The in-progress IME composition changed (the web's
    /// `compositionupdate`): `text` is the whole composition string as it
    /// stands (`GCS_COMPSTR`), `caret` the IME's cursor in it, in `char`s. An
    /// EMPTY `text` means the composition ended or was cancelled
    /// (`compositionend`) — stop drawing it.
    ///
    /// Informational: the committed result still arrives as [`Self::Text`],
    /// and the IME's own composition window still shows. An editor may draw
    /// `text` inline at its caret (underlined, as browsers do) until the next
    /// `Composition` or `Text`; one that ignores it loses nothing.
    Composition { text: String, caret: usize },
    /// The window was asked to close (the caption's close button, Alt+F4,
    /// [`close_window`], [`request_close`], the end of the Windows session)
    /// while the page declared it handles closing itself ([`defer_close`]).
    /// The page raises its own `FormClosing`, then either [`consume`]s this
    /// event and calls `host::quit` (the close goes on) or consumes it and
    /// calls [`cancel_close`] (the window stays). An event nobody consumed by
    /// the end of its frame closes the window, so a page that stops handling
    /// it never leaves a window that cannot be closed.
    CloseRequested(CloseReason),
    /// Files were dropped on the window from the Explorer, at `(x, y)` (client DIP, rounded), while
    /// the page accepted them ([`super::accept_files`]).
    FilesDropped { x: i32, y: i32, files: Vec<String> },
}

/// Why the host window is closing — carried by [`InputEvent::CloseRequested`]
/// (WinForms `CloseReason`, the values a host can tell apart).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum CloseReason {
    /// The caption's close button, Alt+F4, the task bar, [`close_window`]
    /// (WinForms gives `Form.Close()` this reason too).
    #[default]
    UserClosing,
    /// The application asked for it with [`request_close`] (WinForms
    /// `Application.Exit`).
    ApplicationExitCall,
    /// The Windows session is ending (`WM_QUERYENDSESSION`).
    WindowsShutDown,
    /// Another process asked the window to close. Not told apart from
    /// [`CloseReason::UserClosing`] by the host today (WinForms cannot always
    /// either); listed for [`request_close`] callers that know better.
    TaskManagerClosing,
}

impl CloseReason {
    /// The low bits of a `WM_CLOSE` `wParam` posted by [`request_close`].
    const fn code(self) -> usize {
        match self {
            Self::UserClosing => 0,
            Self::ApplicationExitCall => 1,
            Self::WindowsShutDown => 2,
            Self::TaskManagerClosing => 3,
        }
    }

    /// Decodes the `wParam` of a `WM_CLOSE`: [`CloseReason::UserClosing`]
    /// unless [`request_close`] marked it.
    pub(crate) fn from_wparam(wparam: usize) -> Self {
        if wparam & !0xF != CLOSE_REASON_MARKER {
            return Self::UserClosing;
        }
        match wparam & 0xF {
            1 => Self::ApplicationExitCall,
            2 => Self::WindowsShutDown,
            3 => Self::TaskManagerClosing,
            _ => Self::UserClosing,
        }
    }
}

/// Marks a `WM_CLOSE` posted by [`request_close`] (the reason is in the low
/// four bits). A plain `WM_CLOSE` has `wParam == 0`.
const CLOSE_REASON_MARKER: usize = 0x4B55_0000;

impl InputEvent {
    /// Whether this is `vk` going down (first press or auto-repeat) with
    /// exactly `mods` (see [`Modifiers::matches`]).
    pub fn is_key_down(&self, vk: u16, mods: Modifiers) -> bool {
        matches!(self, InputEvent::Key { vk: v, down: true, mods: m, .. } if *v == vk && m.matches(mods))
    }

    /// Whether this is `vk` going down, whatever the modifiers.
    pub fn is_key_down_any(&self, vk: u16) -> bool {
        matches!(self, InputEvent::Key { vk: v, down: true, .. } if *v == vk)
    }
}

/// The pointer shapes a page may ask for with [`set_cursor`] — the CSS
/// `cursor` values the web design system uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Cursor {
    /// `default`.
    #[default]
    Arrow,
    /// `text` — over editable text.
    IBeam,
    /// `pointer` — over a link or a clickable card.
    Hand,
    /// `ew-resize` / `col-resize`.
    ResizeEW,
    /// `ns-resize` / `row-resize`.
    ResizeNS,
    /// `nwse-resize`.
    ResizeNWSE,
    /// `nesw-resize`.
    ResizeNESW,
    /// `move` / `grab`.
    Move,
    /// `not-allowed`.
    NotAllowed,
    /// `wait` / `progress`.
    Wait,
    /// `crosshair` — over a 2-D picking surface (a colour area).
    Crosshair,
}

/// Mouse-wheel travel of one notch, in DIP — what a page scrolls by per
/// notch unless it has a better idea (Chromium on Windows scrolls 100 px per
/// notch at the default three lines).
pub const WHEEL_NOTCH_DIP: f32 = 100.0;

thread_local! {
    /// Events received since the last frame started.
    static PENDING: RefCell<Vec<InputEvent>> = const { RefCell::new(Vec::new()) };
    /// The current frame's events, each with its "consumed" flag.
    static FRAME: RefCell<Vec<(InputEvent, bool)>> = const { RefCell::new(Vec::new()) };
    /// The cursor the current frame asked for; reset to `Arrow` per frame.
    static CURSOR: Cell<Cursor> = const { Cell::new(Cursor::Arrow) };
    /// The shortest repaint delay asked for during the current frame, in ms.
    static REPAINT_AFTER: Cell<Option<u32>> = const { Cell::new(None) };
    /// The main window, owner of the clipboard while the host runs.
    static MAIN_HWND: Cell<isize> = const { Cell::new(0) };
    /// Whether something already scrolled with this frame's wheel travel.
    static WHEEL_CLAIMED: Cell<bool> = const { Cell::new(false) };
    /// The caption colours (band, ink) this frame asked for, if any.
    static CAPTION_COLORS: Cell<Option<(D2D1_COLOR_F, D2D1_COLOR_F)>> = const { Cell::new(None) };
    /// The shortest wake-up delay asked for during the current frame, in ms
    /// (see [`request_wake_after`]).
    static WAKE_AFTER: Cell<Option<u32>> = const { Cell::new(None) };
    /// Whether the last frame declared it handles closing ([`defer_close`]).
    static CLOSE_DEFERRED: Cell<bool> = const { Cell::new(false) };
    /// Set by [`cancel_close`], read back by the host after a synchronous
    /// close request (`WM_QUERYENDSESSION`).
    static CLOSE_CANCELLED: Cell<bool> = const { Cell::new(false) };
}

/// The host windows alive in the process (a UI thread may own several: a main window, its
/// dialogs, other forms): what a [`UiWaker`] (which may live on any thread) posts its wake-up
/// to.
struct UiWindow {
    thread: std::thread::ThreadId,
    hwnd: isize,
    /// A wake-up is already in the window's queue: later ones are coalesced.
    posted: bool,
}

static UI_WINDOWS: std::sync::Mutex<Vec<UiWindow>> = std::sync::Mutex::new(Vec::new());

fn ui_windows() -> std::sync::MutexGuard<'static, Vec<UiWindow>> {
    // A panic while holding this lock cannot leave the list inconsistent
    // (every critical section is a single push/retain/field write).
    UI_WINDOWS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The window message a [`UiWaker`] posts to the host window of its thread.
pub const WM_KUBUNO_WAKE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x4B51;

/// A `Send + Sync` handle that wakes the host window of one UI thread from
/// any thread: the next frame runs soon — even while the window is minimised
/// or hidden, when no `WM_PAINT` would come (the host then renders directly).
/// What `kubuno_desktop_views`' `UiDispatcher` and async executor use after posting
/// work for the UI thread (WinForms' `Control.BeginInvoke` wake-up).
///
/// Create it on the UI thread with [`ui_waker`] — before or after the window
/// exists: it finds the thread's host window when it wakes. Waking before the
/// window exists, or after it is gone, does nothing. Several wake-ups before
/// the window processes the first one are coalesced into one message.
#[derive(Debug, Clone)]
pub struct UiWaker {
    thread: std::thread::ThreadId,
}

impl UiWaker {
    /// Asks the UI thread's host window for a frame.
    pub fn wake(&self) {
        // Every window of the thread gets a frame: the work posted (a dispatcher closure, a
        // timer, a task) belongs to one of them, and a frame of the others costs one repaint.
        let targets: Vec<isize> = {
            let mut windows = ui_windows();
            windows
                .iter_mut()
                .filter(|w| w.thread == self.thread && !w.posted)
                .map(|w| {
                    w.posted = true;
                    w.hwnd
                })
                .collect()
        };
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
        for hwnd in targets {
            // SAFETY: posting to a window handle is sound from any thread, even if
            // the window has since been destroyed (the call then simply fails).
            let posted = unsafe { PostMessageW(Some(HWND(hwnd as *mut _)), WM_KUBUNO_WAKE, WPARAM(0), LPARAM(0)) };
            if posted.is_err() {
                if let Some(w) = ui_windows().iter_mut().find(|w| w.hwnd == hwnd) {
                    w.posted = false;
                }
            }
        }
    }

    /// Whether the calling thread is the UI thread this waker wakes
    /// (WinForms' `!InvokeRequired`).
    pub fn is_ui_thread(&self) -> bool {
        std::thread::current().id() == self.thread
    }
}

/// A [`UiWaker`] for the calling thread's host window. Call it on the UI
/// thread (the one that calls `host::run_with_options`).
pub fn ui_waker() -> UiWaker {
    UiWaker { thread: std::thread::current().id() }
}

/// The host processed a [`WM_KUBUNO_WAKE`]: the next wake-up posts again.
pub(crate) fn wake_received(hwnd: HWND) {
    let hwnd = hwnd.0 as isize;
    if let Some(w) = ui_windows().iter_mut().find(|w| w.hwnd == hwnd) {
        w.posted = false;
    }
}

/// The host windows of the calling thread, oldest first (what a modal window disables, and what
/// the end of the main message loop closes).
pub(crate) fn thread_windows() -> Vec<HWND> {
    let me = std::thread::current().id();
    ui_windows().iter().filter(|w| w.thread == me).map(|w| HWND(w.hwnd as *mut _)).collect()
}

// ── Host side (crate-private) ─────────────────────────────────────────────

/// Queues an event for the next frame. Adjacent text is merged, so a burst of
/// typing is one `Text` event.
pub(crate) fn push(ev: InputEvent) {
    PENDING.with(|p| {
        let mut p = p.borrow_mut();
        if let (InputEvent::Text(add), Some(InputEvent::Text(last))) = (&ev, p.last_mut()) {
            last.push_str(add);
            return;
        }
        p.push(ev);
    });
}

/// Moves the pending events into the frame about to be painted and resets the
/// per-frame requests (cursor, repaint delay).
pub(crate) fn begin_frame() {
    let pending: Vec<InputEvent> = PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
    FRAME.with(|f| *f.borrow_mut() = pending.into_iter().map(|e| (e, false)).collect());
    CURSOR.with(|c| c.set(Cursor::Arrow));
    REPAINT_AFTER.with(|r| r.set(None));
    WHEEL_CLAIMED.with(|w| w.set(false));
    CAPTION_COLORS.with(|c| c.set(None));
    WAKE_AFTER.with(|w| w.set(None));
    CLOSE_DEFERRED.with(|c| c.set(false));
}

/// The wake-up delay the frame that just ended asked for, taken.
pub(crate) fn take_wake_after() -> Option<u32> {
    WAKE_AFTER.with(|w| w.take())
}

/// Whether the last frame declared it handles closing ([`defer_close`]).
pub(crate) fn close_deferred() -> bool {
    CLOSE_DEFERRED.with(|c| c.get())
}

/// Queues an [`InputEvent::CloseRequested`] for the next frame, unless one is
/// already waiting (a second click on the close button while the first request
/// is pending is the same request).
pub(crate) fn queue_close_request(reason: CloseReason) {
    let waiting = PENDING.with(|p| p.borrow().iter().any(|e| matches!(e, InputEvent::CloseRequested(_))));
    if !waiting {
        push(InputEvent::CloseRequested(reason));
    }
}

/// The close request of the frame that just ended that nobody consumed.
pub(crate) fn unconsumed_close_request() -> Option<CloseReason> {
    FRAME.with(|f| {
        f.borrow().iter().find_map(|(e, used)| match e {
            InputEvent::CloseRequested(reason) if !used => Some(*reason),
            _ => None,
        })
    })
}

/// Reads and clears the [`cancel_close`] flag.
pub(crate) fn take_close_cancelled() -> bool {
    CLOSE_CANCELLED.with(|c| c.replace(false))
}

/// The caption colours the frame that just ended asked for.
pub(crate) fn frame_caption_colors() -> Option<(D2D1_COLOR_F, D2D1_COLOR_F)> {
    CAPTION_COLORS.with(|c| c.get())
}

/// The cursor the frame that just ended asked for.
pub(crate) fn frame_cursor() -> Cursor {
    CURSOR.with(|c| c.get())
}

/// The repaint delay the frame that just ended asked for, taken.
pub(crate) fn take_repaint_after() -> Option<u32> {
    REPAINT_AFTER.with(|r| r.take())
}

/// Records `hwnd` as the window of the current per-window state (see `super::window_tls`) and
/// registers it with the thread's wake-up targets.
pub(crate) fn set_main_hwnd(hwnd: HWND) {
    MAIN_HWND.with(|h| h.set(hwnd.0 as isize));
    if hwnd.is_invalid() {
        return;
    }
    // A window a `UiWaker` of this thread posts to.
    let me = std::thread::current().id();
    let mut windows = ui_windows();
    if !windows.iter().any(|w| w.hwnd == hwnd.0 as isize) {
        windows.push(UiWindow { thread: me, hwnd: hwnd.0 as isize, posted: false });
    }
}

/// `hwnd` is being destroyed: it is no longer a wake-up target, nor the current state's window.
pub(crate) fn window_destroyed(hwnd: HWND) {
    let raw = hwnd.0 as isize;
    MAIN_HWND.with(|h| {
        if h.get() == raw {
            h.set(0);
        }
    });
    ui_windows().retain(|w| w.hwnd != raw);
}

/// The per-window part of this module's thread-local state (see `super::window_tls`): what one
/// host window's frames read and write, swapped out while another window of the thread runs.
pub(crate) struct WindowState {
    pending: Vec<InputEvent>,
    frame: Vec<(InputEvent, bool)>,
    cursor: Cursor,
    repaint_after: Option<u32>,
    main_hwnd: isize,
    wheel_claimed: bool,
    caption_colors: Option<(D2D1_COLOR_F, D2D1_COLOR_F)>,
    wake_after: Option<u32>,
    close_deferred: bool,
    close_cancelled: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            frame: Vec::new(),
            cursor: Cursor::Arrow,
            repaint_after: None,
            main_hwnd: 0,
            wheel_claimed: false,
            caption_colors: None,
            wake_after: None,
            close_deferred: false,
            close_cancelled: false,
        }
    }
}

/// Takes the current window's state out of the thread-locals (leaving the defaults).
pub(crate) fn take_window_state() -> WindowState {
    WindowState {
        pending: PENDING.with(|p| p.try_borrow_mut().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()),
        frame: FRAME.with(|f| f.try_borrow_mut().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()),
        cursor: CURSOR.with(|c| c.replace(Cursor::Arrow)),
        repaint_after: REPAINT_AFTER.with(|c| c.take()),
        main_hwnd: MAIN_HWND.with(|c| c.replace(0)),
        wheel_claimed: WHEEL_CLAIMED.with(|c| c.replace(false)),
        caption_colors: CAPTION_COLORS.with(|c| c.take()),
        wake_after: WAKE_AFTER.with(|c| c.take()),
        close_deferred: CLOSE_DEFERRED.with(|c| c.replace(false)),
        close_cancelled: CLOSE_CANCELLED.with(|c| c.replace(false)),
    }
}

/// Installs a window's state (taken with [`take_window_state`]) into the thread-locals.
pub(crate) fn put_window_state(state: WindowState) {
    PENDING.with(|p| {
        if let Ok(mut v) = p.try_borrow_mut() {
            *v = state.pending;
        }
    });
    FRAME.with(|f| {
        if let Ok(mut v) = f.try_borrow_mut() {
            *v = state.frame;
        }
    });
    CURSOR.with(|c| c.set(state.cursor));
    REPAINT_AFTER.with(|c| c.set(state.repaint_after));
    MAIN_HWND.with(|c| c.set(state.main_hwnd));
    WHEEL_CLAIMED.with(|c| c.set(state.wheel_claimed));
    CAPTION_COLORS.with(|c| c.set(state.caption_colors));
    WAKE_AFTER.with(|c| c.set(state.wake_after));
    CLOSE_DEFERRED.with(|c| c.set(state.close_deferred));
    CLOSE_CANCELLED.with(|c| c.set(state.close_cancelled));
}

/// The main window registered by the host, `None` before it exists and after
/// it is destroyed.
pub(crate) fn main_hwnd() -> Option<HWND> {
    let h = MAIN_HWND.with(|h| h.get());
    (h != 0).then_some(HWND(h as *mut _))
}

/// Replaces this thread's frame input with `events` (none consumed), as the host does when a
/// frame starts — for the tests of code that reads keys and text from this module ([`consume`],
/// [`take_key`]…) without a window. Not for application code: a real window's frame input is
/// the host's.
#[doc(hidden)]
pub fn set_frame_events(events: Vec<InputEvent>) {
    FRAME.with(|f| *f.borrow_mut() = events.into_iter().map(|e| (e, false)).collect());
}

// ── Page side ─────────────────────────────────────────────────────────────

/// This frame's events that nobody consumed yet, in arrival order.
pub fn events() -> Vec<InputEvent> {
    events_unconsumed()
}

/// Marks this frame's wheel travel as used — the web's `preventDefault` on a
/// wheel event. A control that scrolls (a list, a slider stepping on the wheel)
/// claims it, so the scrolled area around it does not ALSO scroll.
/// Paints the Kubuno caption (`Chrome::Kubuno`) in `band` with `ink` for its
/// title and buttons — and the window border with it — instead of the theme's
/// accent. Asked for EVERY frame by whatever owns the top of the window (a
/// ribbon whose tab strip must continue into the caption); a frame that does
/// not ask gets the accent back.
pub fn set_caption_colors(band: D2D1_COLOR_F, ink: D2D1_COLOR_F) {
    CAPTION_COLORS.with(|c| c.set(Some((band, ink))));
}

pub fn claim_wheel() {
    WHEEL_CLAIMED.with(|w| w.set(true));
}

/// Whether something already claimed this frame's wheel travel (see
/// [`claim_wheel`]).
pub fn wheel_claimed() -> bool {
    WHEEL_CLAIMED.with(|w| w.get())
}

fn events_unconsumed() -> Vec<InputEvent> {
    FRAME.with(|f| f.borrow().iter().filter(|(_, used)| !used).map(|(e, _)| e.clone()).collect())
}

/// Whether this frame still has unconsumed events.
pub fn has_events() -> bool {
    FRAME.with(|f| f.borrow().iter().any(|(_, used)| !used))
}

/// Consumes every unconsumed event `pred` accepts and returns them, in order.
pub fn consume(mut pred: impl FnMut(&InputEvent) -> bool) -> Vec<InputEvent> {
    FRAME.with(|f| {
        let mut out = Vec::new();
        for (e, used) in f.borrow_mut().iter_mut() {
            if !*used && pred(e) {
                *used = true;
                out.push(e.clone());
            }
        }
        out
    })
}

/// Consumes EVERY key-down of `vk` with exactly `mods` this frame (repeats
/// included) and returns how many there were — `0` when the key was not
/// pressed. The matching key-ups are left alone.
pub fn take_key(vk: u16, mods: Modifiers) -> usize {
    consume(|e| e.is_key_down(vk, mods)).len()
}

/// Like [`take_key`], but whatever the modifiers; returns the modifiers of
/// each press consumed.
pub fn take_key_any(vk: u16) -> Vec<Modifiers> {
    consume(|e| e.is_key_down_any(vk))
        .into_iter()
        .filter_map(|e| match e {
            InputEvent::Key { mods, .. } => Some(mods),
            _ => None,
        })
        .collect()
}

/// Whether `vk` went down with exactly `mods` this frame, WITHOUT consuming it.
pub fn key_pressed(vk: u16, mods: Modifiers) -> bool {
    FRAME.with(|f| f.borrow().iter().any(|(e, used)| !used && e.is_key_down(vk, mods)))
}

/// Consumes all the text typed this frame, concatenated. Empty if none.
pub fn take_text() -> String {
    consume(|e| matches!(e, InputEvent::Text(_)))
        .into_iter()
        .filter_map(|e| match e {
            InputEvent::Text(s) => Some(s),
            _ => None,
        })
        .collect()
}

/// The latest IME composition state this frame reported, WITHOUT consuming
/// it: `Some((text, caret))` when an [`InputEvent::Composition`] arrived
/// (`text` empty = the composition ended), `None` when the composition did not
/// change this frame. Several updates in one frame collapse to the last.
pub fn composition() -> Option<(String, usize)> {
    FRAME.with(|f| {
        f.borrow().iter().rev().find_map(|(e, used)| match e {
            InputEvent::Composition { text, caret } if !used => Some((text.clone(), *caret)),
            _ => None,
        })
    })
}

/// Asks for the pointer shape while it is over the client area (or over one
/// of the page's popups). Call it every frame the shape applies: each frame
/// starts back at [`Cursor::Arrow`], and the last call of a frame wins.
pub fn set_cursor(cursor: Cursor) {
    CURSOR.with(|c| c.set(cursor));
}

/// Asks for one more frame in about `ms` milliseconds, even if no input
/// arrives — a caret blink, a spinner, a small animation. Several calls in one
/// frame keep the shortest delay; the request is re-armed by asking again on
/// the frame it produces (it is one-shot).
pub fn request_repaint_after(ms: u32) {
    REPAINT_AFTER.with(|r| {
        let v = match r.get() {
            Some(cur) => cur.min(ms),
            None => ms,
        };
        r.set(Some(v));
    });
}

thread_local! {
    static ZOOM: Cell<f32> = const { Cell::new(1.0) };
}

/// Zooms the page of this UI thread's window by `factor` (1.0: none), from the next frame: the
/// host renders at the window's DPI times `factor`, so a DIP covers `factor` times more pixels and
/// the frame's size in DIP shrinks accordingly. What a visual designer's zoom uses.
pub fn set_zoom(factor: f32) {
    let factor = if factor.is_finite() { factor.clamp(0.1, 8.0) } else { 1.0 };
    if (ZOOM.with(|z| z.get()) - factor).abs() > f32::EPSILON {
        ZOOM.with(|z| z.set(factor));
        request_repaint_after(0);
    }
}

/// The zoom set with [`set_zoom`].
pub fn zoom() -> f32 {
    ZOOM.with(|z| z.get())
}

/// The repaint the current frame asked for so far ([`request_repaint_after`]), without
/// taking it: for tests and diagnostics (an idle view must not ask for one).
pub fn repaint_requested() -> Option<u32> {
    REPAINT_AFTER.with(|r| r.get())
}

/// Asks for one more frame in about `ms` milliseconds, like
/// [`request_repaint_after`], but for WORK rather than looks: the frame also
/// runs while the window is minimised or hidden (the host renders directly
/// instead of waiting for a `WM_PAINT` that would not come). What a timer or
/// a pending async delay asks for (a WinForms `Timer` keeps ticking while its
/// form is minimised). Several calls in one frame keep the shortest delay;
/// one-shot, like [`request_repaint_after`].
pub fn request_wake_after(ms: u32) {
    WAKE_AFTER.with(|w| {
        let v = match w.get() {
            Some(cur) => cur.min(ms),
            None => ms,
        };
        w.set(Some(v));
    });
}

/// Declares, for the frame being painted, that the page handles closing
/// itself: a close request arriving before the next frame (the close button,
/// Alt+F4, [`close_window`], [`request_close`], the end of the session) no
/// longer destroys the window at once but reaches the next frame as
/// [`InputEvent::CloseRequested`] (see there). Call it every frame it applies
/// (each frame starts undeclared). The close handler
/// (`host::set_close_handler`) is still asked first; `host::quit` bypasses both.
pub fn defer_close() {
    CLOSE_DEFERRED.with(|c| c.set(true));
}

/// The page kept the window open in answer to an
/// [`InputEvent::CloseRequested`] (a `FormClosing` handler cancelled it): lets
/// the host refuse the end of the Windows session when that was the reason.
pub fn cancel_close() {
    CLOSE_CANCELLED.with(|c| c.set(true));
}

/// Asks the host window to close with `reason` — [`close_window`] with a
/// reason a deferring page ([`defer_close`]) receives in its
/// [`InputEvent::CloseRequested`] (WinForms `Application.Exit` is
/// [`CloseReason::ApplicationExitCall`]). Asynchronous (posted). A no-op
/// before the window exists.
pub fn request_close(reason: CloseReason) {
    post_close(CLOSE_REASON_MARKER | reason.code());
}

/// Milliseconds since the host started, monotonic (never goes back, unlike
/// the wall clock). What a blink phase or an animation is computed from.
pub fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

// ── Clipboard (CF_UNICODETEXT) ───────────────────────────────────────────
//
// Declared directly (raw-dylib, no import library needed) because the
// crate's `windows` feature set does not include `Win32_System_DataExchange`
// / `Win32_System_Memory`; the signatures are the documented Win32 ones.

#[link(name = "user32", kind = "raw-dylib")]
extern "system" {
    fn OpenClipboard(hwnd: isize) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn GetClipboardData(format: u32) -> isize;
    fn SetClipboardData(format: u32, mem: isize) -> isize;
}

#[link(name = "kernel32", kind = "raw-dylib")]
extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
    fn GlobalLock(mem: isize) -> *mut core::ffi::c_void;
    fn GlobalUnlock(mem: isize) -> i32;
    fn GlobalFree(mem: isize) -> isize;
    fn GlobalSize(mem: isize) -> usize;
}

/// Asks the host window to close, exactly as its caption's close button
/// does (`WM_CLOSE`): the message loop ends once the window is gone — unless
/// a close handler (`host::set_close_handler`) cancels it; `host::quit` does
/// not ask. A no-op before the window exists.
pub fn close_window() {
    post_close(0);
}

fn post_close(wparam: usize) {
    let owner = MAIN_HWND.with(|h| h.get());
    if owner == 0 {
        return;
    }
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    // SAFETY: posting to a window handle is sound even if the window has
    // since been destroyed (the call then simply fails).
    unsafe {
        let _ = PostMessageW(Some(HWND(owner as *mut _)), WM_CLOSE, WPARAM(wparam), LPARAM(0));
    }
}

const CF_UNICODETEXT: u32 = 13;
const GMEM_MOVEABLE: u32 = 0x0002;

/// Opens the clipboard for the host window, retrying briefly: another
/// process (a clipboard manager) may hold it for a few milliseconds.
fn open_clipboard() -> bool {
    let owner = MAIN_HWND.with(|h| h.get());
    for _ in 0..5 {
        if unsafe { OpenClipboard(owner) } != 0 {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

/// The clipboard's text, if it holds any (line breaks as the source wrote
/// them, usually `\r\n`). `None` when empty, not text, or unavailable.
pub fn clipboard_text() -> Option<String> {
    if !open_clipboard() {
        return None;
    }
    let text = unsafe {
        let mem = GetClipboardData(CF_UNICODETEXT);
        if mem == 0 {
            None
        } else {
            let ptr = GlobalLock(mem) as *const u16;
            if ptr.is_null() {
                None
            } else {
                // Bounded by the block's size: a missing terminator must not
                // read past it.
                let max = GlobalSize(mem) / 2;
                let mut len = 0;
                while len < max && *ptr.add(len) != 0 {
                    len += 1;
                }
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
                GlobalUnlock(mem);
                Some(s)
            }
        }
    };
    unsafe {
        CloseClipboard();
    }
    text
}

/// Replaces the clipboard's content with `text`. Returns whether it worked.
pub fn set_clipboard_text(text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    if !open_clipboard() {
        return false;
    }
    let ok = unsafe {
        EmptyClipboard();
        let mem = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2);
        if mem == 0 {
            false
        } else {
            let dst = GlobalLock(mem) as *mut u16;
            if dst.is_null() {
                GlobalFree(mem);
                false
            } else {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
                GlobalUnlock(mem);
                // On success the system owns the block; on failure it is ours.
                if SetClipboardData(CF_UNICODETEXT, mem) == 0 {
                    GlobalFree(mem);
                    false
                } else {
                    true
                }
            }
        }
    };
    unsafe {
        CloseClipboard();
    }
    ok
}

// ── IME composition (imm32) ──────────────────────────────────────────────
//
// Declared directly for the same reason as the clipboard: the crate's
// `windows` feature set does not include `Win32_UI_Input_Ime`.

#[link(name = "imm32", kind = "raw-dylib")]
extern "system" {
    fn ImmGetContext(hwnd: isize) -> isize;
    fn ImmReleaseContext(hwnd: isize, himc: isize) -> i32;
    fn ImmGetCompositionStringW(himc: isize, index: u32, buf: *mut core::ffi::c_void, len: u32) -> i32;
}

/// `WM_IME_ENDCOMPOSITION`.
pub(crate) const WM_IME_END_COMPOSITION: u32 = 0x010E;
/// `WM_IME_COMPOSITION`.
pub(crate) const WM_IME_COMPOSITION_MSG: u32 = 0x010F;
/// `GCS_COMPSTR`: the `lParam` flag (and string index) of the composition string.
pub(crate) const GCS_COMPSTR: u32 = 0x0008;
/// `GCS_CURSORPOS`: the index of the IME cursor in the composition string.
const GCS_CURSORPOS: u32 = 0x0080;

/// Reads the window's current IME composition string and cursor (the cursor
/// converted from UTF-16 units to `char`s). `None` if the input context
/// cannot be read.
pub(crate) fn read_composition(hwnd: HWND) -> Option<(String, usize)> {
    unsafe {
        let himc = ImmGetContext(hwnd.0 as isize);
        if himc == 0 {
            return None;
        }
        // The first call gives the size in BYTES, the second fills the buffer.
        let bytes = ImmGetCompositionStringW(himc, GCS_COMPSTR, std::ptr::null_mut(), 0);
        let text = if bytes > 0 {
            let mut buf = vec![0u16; bytes as usize / 2];
            let got = ImmGetCompositionStringW(
                himc,
                GCS_COMPSTR,
                buf.as_mut_ptr().cast(),
                (buf.len() * 2) as u32,
            );
            buf.truncate((got.max(0) as usize / 2).min(buf.len()));
            buf
        } else {
            Vec::new()
        };
        let cursor = ImmGetCompositionStringW(himc, GCS_CURSORPOS, std::ptr::null_mut(), 0);
        ImmReleaseContext(hwnd.0 as isize, himc);
        Some(composition_from_utf16(&text, cursor))
    }
}

/// A composition string in UTF-16 units with the IME cursor as a UTF-16
/// index → the string and the cursor as a `char` index (clamped to the text).
fn composition_from_utf16(units: &[u16], cursor: i32) -> (String, usize) {
    let text = String::from_utf16_lossy(units);
    let upto = (cursor.max(0) as usize).min(units.len());
    let caret = char::decode_utf16(units[..upto].iter().copied()).count();
    (text, caret)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(vk: u16, mods: Modifiers) -> InputEvent {
        InputEvent::Key { vk, down: true, repeat: false, mods }
    }

    #[test]
    fn text_merges_and_is_taken_once() {
        push(InputEvent::Text("a".into()));
        push(InputEvent::Text("b".into()));
        push(key(vk::TAB, Modifiers::NONE));
        begin_frame();
        assert_eq!(events().len(), 2);
        assert_eq!(take_text(), "ab");
        assert_eq!(take_text(), "");
        assert_eq!(events().len(), 1);
    }

    #[test]
    fn take_key_matches_exact_chord() {
        push(key(vk::TAB, Modifiers::SHIFT));
        push(key(vk::TAB, Modifiers::NONE));
        begin_frame();
        assert!(key_pressed(vk::TAB, Modifiers::NONE));
        assert_eq!(take_key(vk::TAB, Modifiers::CTRL), 0);
        assert_eq!(take_key(vk::TAB, Modifiers::NONE), 1);
        assert_eq!(take_key_any(vk::TAB), vec![Modifiers::SHIFT]);
        assert!(!has_events());
    }

    #[test]
    fn frame_requests_reset() {
        set_cursor(Cursor::IBeam);
        request_repaint_after(500);
        request_repaint_after(30);
        assert_eq!(frame_cursor(), Cursor::IBeam);
        assert_eq!(take_repaint_after(), Some(30));
        begin_frame();
        assert_eq!(frame_cursor(), Cursor::Arrow);
        assert_eq!(take_repaint_after(), None);
    }

    #[test]
    fn wake_requests_and_close_declarations_are_per_frame() {
        request_wake_after(1000);
        request_wake_after(40);
        defer_close();
        assert!(close_deferred());
        assert_eq!(take_wake_after(), Some(40));
        assert_eq!(take_wake_after(), None);
        request_wake_after(10);
        begin_frame();
        assert_eq!(take_wake_after(), None);
        assert!(!close_deferred(), "each frame starts undeclared");
        cancel_close();
        assert!(take_close_cancelled());
        assert!(!take_close_cancelled());
    }

    #[test]
    fn a_close_request_is_queued_once_and_reported_until_consumed() {
        begin_frame();
        queue_close_request(CloseReason::UserClosing);
        queue_close_request(CloseReason::ApplicationExitCall);
        begin_frame();
        assert_eq!(events(), vec![InputEvent::CloseRequested(CloseReason::UserClosing)]);
        assert_eq!(unconsumed_close_request(), Some(CloseReason::UserClosing));
        consume(|e| matches!(e, InputEvent::CloseRequested(_)));
        assert_eq!(unconsumed_close_request(), None);
        begin_frame();
        assert_eq!(unconsumed_close_request(), None);
    }

    #[test]
    fn close_reasons_round_trip_through_wparam() {
        for reason in [CloseReason::UserClosing, CloseReason::ApplicationExitCall, CloseReason::WindowsShutDown, CloseReason::TaskManagerClosing] {
            assert_eq!(CloseReason::from_wparam(CLOSE_REASON_MARKER | reason.code()), reason);
        }
        assert_eq!(CloseReason::from_wparam(0), CloseReason::UserClosing);
        assert_eq!(CloseReason::from_wparam(2), CloseReason::UserClosing, "an unmarked wParam is a user close");
    }

    #[test]
    fn a_waker_knows_its_ui_thread_and_is_harmless_without_a_window() {
        let waker = ui_waker();
        assert!(waker.is_ui_thread());
        waker.wake(); // No host window on this thread: nothing to post to.
        let other = std::thread::spawn(move || (waker.is_ui_thread(), waker.wake()));
        assert!(!other.join().map(|(ui, ())| ui).unwrap_or(true));
    }

    #[test]
    fn composition_is_reported_and_collapses() {
        push(InputEvent::Composition { text: "に".into(), caret: 1 });
        push(InputEvent::Composition { text: "にほ".into(), caret: 2 });
        push(InputEvent::Text("x".into()));
        begin_frame();
        assert_eq!(composition(), Some(("にほ".to_string(), 2)));
        // Text is still taken separately and is not merged into it.
        assert_eq!(take_text(), "x");
        begin_frame();
        assert_eq!(composition(), None);
    }

    #[test]
    fn composition_caret_counts_chars() {
        // "a😀b" is 4 UTF-16 units; a cursor after the emoji (unit 3) is char 2.
        let units: Vec<u16> = "a😀b".encode_utf16().collect();
        assert_eq!(composition_from_utf16(&units, 3), ("a😀b".to_string(), 2));
        assert_eq!(composition_from_utf16(&units, 99).1, 3);
        assert_eq!(composition_from_utf16(&units, -1).1, 0);
        assert_eq!(composition_from_utf16(&[], 0), (String::new(), 0));
    }

    #[test]
    fn vk_helpers() {
        assert_eq!(vk::letter('a'), 0x41);
        assert_eq!(vk::letter('Z'), 0x5A);
        assert_eq!(vk::letter('é'), 0);
        assert_eq!(vk::digit(7), 0x37);
    }
}
